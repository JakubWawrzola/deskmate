//! Deskmate Link v2: authenticated WebSocket handshake and encrypted JSON frames.
//!
//! The pairing key (PSK) authenticates both ends; session keys come from an
//! ephemeral X25519 exchange mixed with the PSK, so a key leaked later does not
//! decrypt recorded sessions. Every handshake field is covered by the MAC
//! through a length-prefixed encoding. Protocol description: docs/LINK.md.
//!
//! This module is the protocol only: URL and key checks, the handshake, the
//! frame codec and what a failure means. Connecting, retrying and the content
//! of the frames belong to the program that uses it.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chacha20poly1305::ChaCha20Poly1305;
use futures_util::{SinkExt, StreamExt};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

mod frames;

pub use frames::{info_frame, parse_info, state_frame, ClientPlatform, ClientRole, HubInfo};

type HmacSha256 = Hmac<Sha256>;
/// A 32-byte secret that is wiped from memory when dropped.
pub type SecretKey = Zeroizing<[u8; 32]>;

/// Path of the Link WebSocket on Home Assistant.
pub const WS_PATH: &str = "/api/deskmate_link/ws";
/// Largest accepted difference between the two clocks, in seconds.
pub const MAX_SKEW_SECS: i64 = 90;
pub const PROTOCOL_VERSION: u64 = 2;
/// How long a client waits for `welcome` after sending `hello`.
pub const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a client sends `ping` in an established session.
pub const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Why a Link session ended. Drives both the status text and the retry delay:
/// a rejected pairing is not worth retrying every two seconds, and repeating it
/// only pushes the client into the server-side lockout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFailure {
    /// The pairing key, the node, the cascade setting or the protocol version
    /// does not match. Retrying will not help until someone changes settings.
    Auth,
    /// Home Assistant temporarily refused this node after repeated failures.
    Locked,
    /// This computer's clock and Home Assistant's differ by more than 90 s.
    Clock,
    /// Network, URL or a dropped connection.
    Transport,
}

/// A failed connection attempt or session: the kind of failure and a text
/// for the user.
#[derive(Debug)]
pub struct LinkError {
    pub kind: LinkFailure,
    pub message: String,
}

impl LinkError {
    pub fn new(kind: LinkFailure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<String> for LinkError {
    fn from(message: String) -> Self {
        Self::new(LinkFailure::Transport, message)
    }
}

impl From<&str> for LinkError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

const LOCKED_MESSAGE: &str =
    "Home Assistant is temporarily refusing this node after repeated failed handshakes";
const CLOCK_MESSAGE: &str =
    "this computer's clock differs from Home Assistant's by more than 90 seconds - sync the Windows time (Settings > Time & language > Sync now)";

/// A 429 during the WebSocket upgrade is the server-side handshake lockout,
/// not an ordinary network problem.
pub fn classify_connect_error(error: tokio_tungstenite::tungstenite::Error) -> LinkError {
    if let tokio_tungstenite::tungstenite::Error::Http(response) = &error {
        if response.status().as_u16() == 429 {
            return LinkError::new(LinkFailure::Locked, LOCKED_MESSAGE);
        }
    }
    LinkError::from(error.to_string())
}

/// Maps the server's `reject` reason to a failure the user can act on.
pub fn classify_reject(reason: &str) -> LinkError {
    match reason {
        "locked" => LinkError::new(LinkFailure::Locked, LOCKED_MESSAGE),
        "clock" => LinkError::new(LinkFailure::Clock, CLOCK_MESSAGE),
        "cascade" => LinkError::new(
            LinkFailure::Auth,
            "cascade encryption is on at one end only - enable or disable it on both (Geeky stuff here, Reconfigure in Home Assistant)",
        ),
        "version" => LinkError::new(
            LinkFailure::Auth,
            "Home Assistant does not accept this protocol version - update the Deskmate Link integration to 0.6.0 or newer",
        ),
        _ => LinkError::new(
            LinkFailure::Auth,
            "Home Assistant rejected the pairing - Deskmate is disabled or deleted there, the pairing key does not match, or the integration is older than 0.6.0. Disconnect on the Status page stops the attempts",
        ),
    }
}

/// Retry delay after a non-network failure, in seconds. `attempts` counts the
/// non-network failures in a row before this one.
pub fn auth_retry_delay(kind: LinkFailure, attempts: u32) -> u64 {
    match kind {
        LinkFailure::Locked => 60,
        LinkFailure::Clock => 30,
        _ => match attempts {
            0 | 1 => 5,
            2 => 15,
            3 => 30,
            _ => 60,
        },
    }
}

/// Checks a Link URL from the settings and completes its path. An empty
/// input stays empty.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let mut url = url::Url::parse(raw).map_err(|e| format!("invalid Link URL: {e}"))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err("Link URL must use ws:// or wss://".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Link URL cannot contain credentials, query or fragment".into());
    }
    match url.path() {
        "" | "/" => url.set_path(WS_PATH),
        WS_PATH => {}
        _ => return Err(format!("Link URL path must be {WS_PATH}")),
    }
    if url.scheme() == "ws" && !is_private_host(&url) {
        return Err(
            "plain ws:// is only allowed for LAN, .local and Tailscale addresses - use wss:// for anything reachable from the internet"
                .into(),
        );
    }
    Ok(url.to_string())
}

/// Hosts where an unencrypted WebSocket stays inside a trusted or already
/// encrypted network: RFC 1918, loopback, link-local, Tailscale's CGNAT range
/// and IPv6 ULA, plus mDNS and MagicDNS names. Frames are end-to-end encrypted
/// either way; this keeps the node name and traffic pattern off the internet.
pub fn is_private_host(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || (a == 100 && (64..=127).contains(&b))
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost"
                || name.ends_with(".local")
                || name.ends_with(".ts.net")
                || name.ends_with(".lan")
                || name.ends_with(".home.arpa")
                || !name.contains('.')
        }
        None => false,
    }
}

/// Decodes a pairing key or a cascade key: base64 of exactly 32 bytes.
pub fn validate_pairing_key(raw: &str) -> Result<SecretKey, String> {
    let decoded = Zeroizing::new(
        B64.decode(raw.trim())
            .map_err(|_| "Link pairing key must be base64".to_string())?,
    );
    let key: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| "Link pairing key must decode to exactly 32 bytes".to_string())?;
    Ok(Zeroizing::new(key))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Canonical encoding for MAC and KDF input: every field is prefixed with its
/// 4-byte big-endian length, so no two different field lists share bytes.
pub fn enc(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len() + 4).sum());
    for part in parts {
        out.extend_from_slice(&(part.len() as u32).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// The bytes of a `hello` that its MAC covers.
pub fn hello_bytes(node: &str, cn: &[u8; 16], ts: i64, epk: &[u8; 32], cascade: bool) -> Vec<u8> {
    enc(&[
        b"dml2 hello",
        b"2",
        node.as_bytes(),
        cn,
        ts.to_string().as_bytes(),
        epk,
        if cascade { b"1" } else { b"0" },
    ])
}

/// The bytes of a `welcome` that its MAC covers. The welcome is bound to a
/// hash of the whole hello, so tampering with any hello field (the cascade
/// flag included) breaks the server's MAC.
pub fn welcome_bytes(hello: &[u8], sn: &[u8; 16], ts: i64, epk: &[u8; 32]) -> Vec<u8> {
    let hello_hash = Sha256::digest(hello);
    enc(&[
        b"dml2 welcome",
        hello_hash.as_slice(),
        sn,
        ts.to_string().as_bytes(),
        epk,
    ])
}

fn keyed_hmac(psk: &[u8; 32]) -> Result<HmacSha256, String> {
    // HMAC takes a key of any length, so this does not fail for 32 bytes.
    <HmacSha256 as Mac>::new_from_slice(psk).map_err(|_| "HMAC refused the key".to_string())
}

/// HMAC-SHA256 of `data` under a pairing key.
pub fn mac(psk: &[u8; 32], data: &[u8]) -> Result<[u8; 32], String> {
    let mut hmac = keyed_hmac(psk)?;
    hmac.update(data);
    Ok(hmac.finalize().into_bytes().into())
}

/// Checks a base64 MAC from the peer in constant time.
pub fn verify_mac(psk: &[u8; 32], data: &[u8], encoded: &str) -> Result<(), String> {
    let supplied = B64
        .decode(encoded)
        .map_err(|_| "invalid handshake MAC".to_string())?;
    let mut hmac = keyed_hmac(psk)?;
    hmac.update(data);
    hmac.verify_slice(&supplied)
        .map_err(|_| "handshake authentication failed".into())
}

/// Session keys: HKDF-SHA256 with the DH result and a pairing key as input
/// keying material and the transcript hash as salt. `labels` separates the
/// primary layer from the cascade layer. Returns (client to server, server to
/// client).
pub fn derive_keys(
    psk: &[u8; 32],
    shared: &[u8; 32],
    hello: &[u8],
    welcome: &[u8],
    labels: (&[u8], &[u8]),
) -> Result<(SecretKey, SecretKey), String> {
    let salt = Sha256::digest(enc(&[hello, welcome]));
    let mut ikm = Zeroizing::new([0u8; 64]);
    ikm[..32].copy_from_slice(shared);
    ikm[32..].copy_from_slice(psk);
    let hkdf = Hkdf::<Sha256>::new(Some(salt.as_slice()), ikm.as_slice());
    let mut c2s = Zeroizing::new([0u8; 32]);
    let mut s2c = Zeroizing::new([0u8; 32]);
    hkdf.expand(labels.0, c2s.as_mut_slice())
        .map_err(|_| "HKDF c2s failed".to_string())?;
    hkdf.expand(labels.1, s2c.as_mut_slice())
        .map_err(|_| "HKDF s2c failed".to_string())?;
    Ok((c2s, s2c))
}

/// HKDF labels of the primary layer, for `derive_keys`.
pub const PRIMARY_LABELS: (&[u8], &[u8]) = (b"dml2 c2s", b"dml2 s2c");
/// HKDF labels of the cascade layer, for `derive_keys`.
pub const CASCADE_LABELS: (&[u8], &[u8]) = (b"dml2 cascade c2s", b"dml2 cascade s2c");

fn decode_exact<const N: usize>(value: Option<&Value>, what: &str) -> Result<[u8; N], String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("welcome missing {what}"))?;
    B64.decode(text)
        .map_err(|_| format!("invalid {what}"))?
        .try_into()
        .map_err(|_| format!("{what} must be {N} bytes"))
}

/// Which way a frame travels. Each direction has its own key, nonce prefix and
/// associated data, so a frame cannot be replayed in the other direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Self::ClientToServer => "c2s",
            Self::ServerToClient => "s2c",
        }
    }

    fn nonce_prefix(self) -> [u8; 4] {
        match self {
            Self::ClientToServer => [1, 0, 0, 0],
            Self::ServerToClient => [2, 0, 0, 0],
        }
    }
}

/// Encrypts or decrypts the frames of one direction of a session and keeps
/// that direction's frame counter.
pub struct FrameCodec {
    cipher: Aes256Gcm,
    /// Optional second layer. When present every frame is encrypted twice, with
    /// two different ciphers under two different keys.
    cascade: Option<ChaCha20Poly1305>,
    nonce_prefix: [u8; 4],
    aad: Vec<u8>,
    cascade_aad: Vec<u8>,
    counter: u64,
}

impl FrameCodec {
    pub fn new(
        key: &[u8; 32],
        node: &str,
        direction: Direction,
        cascade_key: Option<&[u8; 32]>,
    ) -> Self {
        let label = direction.label().as_bytes();
        Self {
            cipher: Aes256Gcm::new(key.into()),
            cascade: cascade_key.map(|k| ChaCha20Poly1305::new(k.into())),
            nonce_prefix: direction.nonce_prefix(),
            aad: enc(&[b"dml2", node.as_bytes(), label]),
            cascade_aad: enc(&[b"dml2 cascade", node.as_bytes(), label]),
            counter: 0,
        }
    }

    fn nonce(&self, counter: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.nonce_prefix);
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        nonce
    }

    /// Encrypts a frame into the text of one WebSocket message.
    pub fn encrypt(&mut self, value: &Value) -> Result<String, String> {
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| "frame counter exhausted".to_string())?;
        let plaintext = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        let nonce = self.nonce(self.counter);
        let mut ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &self.aad,
                },
            )
            .map_err(|_| "frame encryption failed".to_string())?;
        if let Some(cascade) = &self.cascade {
            ciphertext = cascade
                .encrypt(
                    chacha20poly1305::Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &self.cascade_aad,
                    },
                )
                .map_err(|_| "cascade encryption failed".to_string())?;
        }
        Ok(json!({"t": "e", "n": self.counter, "p": B64.encode(ciphertext)}).to_string())
    }

    /// Decrypts the text of one WebSocket message. A counter that is not
    /// higher than the last accepted one is refused.
    pub fn decrypt(&mut self, frame: &str) -> Result<Value, String> {
        let frame: Value =
            serde_json::from_str(frame).map_err(|_| "invalid encrypted frame JSON".to_string())?;
        if frame.get("t").and_then(Value::as_str) != Some("e") {
            return Err("unencrypted post-handshake frame".into());
        }
        let counter = frame
            .get("n")
            .and_then(Value::as_u64)
            .ok_or_else(|| "missing frame counter".to_string())?;
        if counter <= self.counter {
            return Err("replayed or out-of-order frame".into());
        }
        let mut ciphertext = B64
            .decode(
                frame
                    .get("p")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "missing frame payload".to_string())?,
            )
            .map_err(|_| "invalid frame payload".to_string())?;
        let nonce = self.nonce(counter);
        if let Some(cascade) = &self.cascade {
            ciphertext = cascade
                .decrypt(
                    chacha20poly1305::Nonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &self.cascade_aad,
                    },
                )
                .map_err(|_| "cascade authentication failed".to_string())?;
        }
        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &self.aad,
                },
            )
            .map_err(|_| "frame authentication failed".to_string())?;
        let value = serde_json::from_slice(&plaintext)
            .map_err(|_| "invalid encrypted payload JSON".to_string())?;
        self.counter = counter;
        Ok(value)
    }
}

/// The two frame codecs of an established session.
pub struct SessionCodecs {
    /// Encrypts the frames this client sends.
    pub encoder: FrameCodec,
    /// Decrypts the frames Home Assistant sends.
    pub decoder: FrameCodec,
}

/// The client side of the handshake, on a WebSocket that is already connected:
/// sends `hello`, checks `welcome` and derives the session keys. `cascade` is
/// the separate cascade key when that layer is on.
pub async fn client_handshake<S>(
    socket: &mut WebSocketStream<S>,
    node: &str,
    psk: &[u8; 32],
    cascade: Option<&[u8; 32]>,
) -> Result<SessionCodecs, LinkError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut cn = [0u8; 16];
    OsRng.fill_bytes(&mut cn);
    let cn_b64 = B64.encode(cn);
    let ts = now_unix();
    // Fresh X25519 key for this session only; dropped (and wiped) right after
    // the shared secret is computed.
    let ephemeral = EphemeralSecret::random_from_rng(OsRng);
    let client_epk = PublicKey::from(&ephemeral).to_bytes();
    let hello_data = hello_bytes(node, &cn, ts, &client_epk, cascade.is_some());
    let hello = json!({
        "t": "hello",
        "v": PROTOCOL_VERSION,
        "node": node,
        "cn": cn_b64,
        "ts": ts,
        "epk": B64.encode(client_epk),
        // Cascade must match on both ends; Home Assistant rejects a mismatch
        // rather than quietly falling back to the weaker single layer. The flag
        // is covered by the MAC, so nobody on the path can flip it.
        "casc": cascade.is_some(),
        "mac": B64.encode(mac(psk, &hello_data)?),
    });
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;
    let welcome_message = tokio::time::timeout(WELCOME_TIMEOUT, socket.next())
        .await
        .map_err(|_| "welcome timeout".to_string())?
        .ok_or_else(|| "connection closed before welcome".to_string())?
        .map_err(|e| e.to_string())?;
    let welcome_text = welcome_message
        .into_text()
        .map_err(|_| "welcome must be a text frame".to_string())?;
    let welcome: Value = serde_json::from_str(welcome_text.as_str())
        .map_err(|_| "invalid welcome JSON".to_string())?;
    match welcome.get("t").and_then(Value::as_str) {
        Some("welcome") => {}
        // The server says why it refused. Without this the only trace was a
        // dropped connection and guessing on the user's side.
        Some("reject") => {
            return Err(classify_reject(
                welcome
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("auth"),
            ));
        }
        _ => return Err("expected welcome".into()),
    }
    if welcome.get("v").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(classify_reject("version"));
    }
    let sn: [u8; 16] = decode_exact(welcome.get("sn"), "server nonce")?;
    let server_epk: [u8; 32] = decode_exact(welcome.get("epk"), "server key")?;
    let server_ts = welcome
        .get("ts")
        .and_then(Value::as_i64)
        .ok_or_else(|| "welcome missing ts".to_string())?;
    if (now_unix() - server_ts).abs() > MAX_SKEW_SECS {
        return Err(LinkError::new(LinkFailure::Clock, CLOCK_MESSAGE));
    }
    let welcome_data = welcome_bytes(&hello_data, &sn, server_ts, &server_epk);
    verify_mac(
        psk,
        &welcome_data,
        welcome.get("mac").and_then(Value::as_str).unwrap_or(""),
    )?;
    let shared = ephemeral.diffie_hellman(&PublicKey::from(server_epk));
    if !shared.was_contributory() {
        return Err("server sent a low-order X25519 key".into());
    }
    let (c2s, s2c) = derive_keys(
        psk,
        shared.as_bytes(),
        &hello_data,
        &welcome_data,
        PRIMARY_LABELS,
    )?;
    let cascade_keys = match cascade {
        Some(key) => Some(derive_keys(
            key,
            shared.as_bytes(),
            &hello_data,
            &welcome_data,
            CASCADE_LABELS,
        )?),
        None => None,
    };
    drop(shared);
    let encoder = FrameCodec::new(
        &c2s,
        node,
        Direction::ClientToServer,
        cascade_keys.as_ref().map(|(c, _)| &**c),
    );
    let decoder = FrameCodec::new(
        &s2c,
        node,
        Direction::ServerToClient,
        cascade_keys.as_ref().map(|(_, s)| &**s),
    );
    Ok(SessionCodecs { encoder, decoder })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;
    use tokio_tungstenite::tungstenite::protocol::Role;
    use x25519_dalek::StaticSecret;

    #[test]
    fn normalizes_link_url() {
        assert_eq!(
            normalize_url("ws://ha.local:8123").unwrap(),
            "ws://ha.local:8123/api/deskmate_link/ws"
        );
        assert!(normalize_url("https://ha.local").is_err());
        assert!(normalize_url("wss://user@ha.local").is_err());
    }

    #[test]
    fn plain_ws_only_on_private_hosts() {
        assert!(normalize_url("ws://192.168.1.10:8123").is_ok());
        assert!(normalize_url("ws://100.101.102.103:8123").is_ok());
        assert!(normalize_url("ws://homeassistant:8123").is_ok());
        assert!(normalize_url("ws://ha.tail1234.ts.net:8123").is_ok());
        assert!(normalize_url("ws://ha.example.com").is_err());
        assert!(normalize_url("ws://8.8.8.8:8123").is_err());
        assert!(normalize_url("wss://ha.example.com").is_ok());
    }

    #[test]
    fn pairing_key_must_be_base64_of_32_bytes() {
        assert!(validate_pairing_key(&B64.encode([7u8; 32])).is_ok());
        assert!(validate_pairing_key(&B64.encode([7u8; 31])).is_err());
        assert!(validate_pairing_key(&B64.encode([7u8; 33])).is_err());
        assert!(validate_pairing_key("not base64!").is_err());
        assert!(validate_pairing_key("").is_err());
    }

    #[test]
    fn encoding_is_unambiguous() {
        assert_ne!(enc(&[b"ab", b"c"]), enc(&[b"a", b"bc"]));
    }

    #[test]
    fn reject_reasons_map_to_failures_and_retry_delays() {
        assert_eq!(classify_reject("locked").kind, LinkFailure::Locked);
        assert_eq!(classify_reject("clock").kind, LinkFailure::Clock);
        assert_eq!(classify_reject("cascade").kind, LinkFailure::Auth);
        assert_eq!(classify_reject("version").kind, LinkFailure::Auth);
        assert_eq!(classify_reject("auth").kind, LinkFailure::Auth);
        assert_eq!(classify_reject("anything else").kind, LinkFailure::Auth);

        assert_eq!(auth_retry_delay(LinkFailure::Locked, 0), 60);
        assert_eq!(auth_retry_delay(LinkFailure::Clock, 7), 30);
        let auth: Vec<u64> = (0..6)
            .map(|attempts| auth_retry_delay(LinkFailure::Auth, attempts))
            .collect();
        assert_eq!(auth, [5, 5, 15, 30, 60, 60]);
    }

    fn b64<const N: usize>(vector: &Value, key: &str) -> [u8; N] {
        B64.decode(vector[key].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    #[test]
    fn matches_home_assistant_python_v2_vectors() {
        let vector: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/deskmate_link_v2.json"
        ))
        .unwrap();
        let psk = validate_pairing_key(vector["psk_b64"].as_str().unwrap()).unwrap();
        let cascade = validate_pairing_key(vector["cascade_b64"].as_str().unwrap()).unwrap();
        let node = vector["node"].as_str().unwrap();
        let cn: [u8; 16] = b64(&vector, "cn_b64");
        let sn: [u8; 16] = b64(&vector, "sn_b64");
        let client_esk = StaticSecret::from(b64::<32>(&vector, "client_esk_b64"));
        let client_epk = PublicKey::from(&client_esk).to_bytes();
        assert_eq!(client_epk, b64::<32>(&vector, "client_epk_b64"));
        let server_epk: [u8; 32] = b64(&vector, "server_epk_b64");

        let hello = hello_bytes(
            node,
            &cn,
            vector["hello_ts"].as_i64().unwrap(),
            &client_epk,
            true,
        );
        assert_eq!(
            B64.encode(mac(&psk, &hello).unwrap()),
            vector["hello_mac_b64"].as_str().unwrap()
        );
        let welcome = welcome_bytes(
            &hello,
            &sn,
            vector["welcome_ts"].as_i64().unwrap(),
            &server_epk,
        );
        verify_mac(&psk, &welcome, vector["welcome_mac_b64"].as_str().unwrap()).unwrap();

        // Flipping the cascade flag must invalidate the MAC chain.
        let tampered = hello_bytes(
            node,
            &cn,
            vector["hello_ts"].as_i64().unwrap(),
            &client_epk,
            false,
        );
        assert_ne!(mac(&psk, &tampered).unwrap(), mac(&psk, &hello).unwrap());

        let shared = client_esk.diffie_hellman(&PublicKey::from(server_epk));
        let (c2s, s2c) =
            derive_keys(&psk, shared.as_bytes(), &hello, &welcome, PRIMARY_LABELS).unwrap();
        let (cc2s, cs2c) = derive_keys(
            &cascade,
            shared.as_bytes(),
            &hello,
            &welcome,
            CASCADE_LABELS,
        )
        .unwrap();
        assert_eq!(B64.encode(*c2s), vector["c2s_key_b64"].as_str().unwrap());
        assert_eq!(B64.encode(*s2c), vector["s2c_key_b64"].as_str().unwrap());
        assert_eq!(
            B64.encode(*cc2s),
            vector["cascade_c2s_key_b64"].as_str().unwrap()
        );
        assert_eq!(
            B64.encode(*cs2c),
            vector["cascade_s2c_key_b64"].as_str().unwrap()
        );

        let mut decoder = FrameCodec::new(&c2s, node, Direction::ClientToServer, Some(&cc2s));
        let frame = vector["python_c2s_frame"].to_string();
        assert_eq!(decoder.decrypt(&frame).unwrap(), vector["payload"]);
        assert!(
            decoder.decrypt(&frame).is_err(),
            "the same counter must be rejected as replay"
        );
        let mut from_server = FrameCodec::new(&s2c, node, Direction::ServerToClient, Some(&cs2c));
        let ping = vector["python_s2c_ping_frame"].to_string();
        assert_eq!(from_server.decrypt(&ping).unwrap()["t"], "ping");

        // Without the cascade layer the same frame must not authenticate.
        let mut single = FrameCodec::new(&c2s, node, Direction::ClientToServer, None);
        assert!(single.decrypt(&frame).is_err());
    }

    #[test]
    fn rust_frames_round_trip() {
        let key = [7u8; 32];
        let mut encoder = FrameCodec::new(&key, "pc", Direction::ClientToServer, None);
        let mut decoder = FrameCodec::new(&key, "pc", Direction::ClientToServer, None);
        let frame = encoder.encrypt(&json!({"t": "ping"})).unwrap();
        assert_eq!(decoder.decrypt(&frame).unwrap()["t"], "ping");
        let mut other_node = FrameCodec::new(&key, "laptop", Direction::ClientToServer, None);
        assert!(other_node.decrypt(&frame).is_err(), "AAD binds the node");
        let mut other_direction = FrameCodec::new(&key, "pc", Direction::ServerToClient, None);
        assert!(
            other_direction.decrypt(&frame).is_err(),
            "AAD and nonce bind the direction"
        );
    }

    // ---- the handshake against a stand-in for Home Assistant ----

    const NODE: &str = "pc";
    const PSK: [u8; 32] = [3u8; 32];
    const CASCADE: [u8; 32] = [9u8; 32];

    /// How the stand-in hub departs from a correct `welcome`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Hub {
        Correct,
        Reject(Option<&'static str>),
        WrongKey,
        OldVersion,
        ClockOff,
        LowOrderKey,
    }

    async fn sockets() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
        let (client, server) = tokio::io::duplex(64 * 1024);
        (
            WebSocketStream::from_raw_socket(client, Role::Client, None).await,
            WebSocketStream::from_raw_socket(server, Role::Server, None).await,
        )
    }

    /// Reads `hello`, answers as `mode` says and returns the hub's codecs for
    /// (frames from the client, frames to the client).
    async fn hub_answers(
        socket: &mut WebSocketStream<DuplexStream>,
        cascade: Option<&[u8; 32]>,
        mode: Hub,
    ) -> (FrameCodec, FrameCodec) {
        let text = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let hello: Value = serde_json::from_str(text.as_str()).unwrap();
        assert_eq!(hello["t"], "hello");
        assert_eq!(hello["v"], PROTOCOL_VERSION);
        assert_eq!(hello["node"], NODE);
        assert_eq!(hello["casc"], cascade.is_some());
        let cn: [u8; 16] = b64(&hello, "cn");
        let client_epk: [u8; 32] = b64(&hello, "epk");
        let hello_data = hello_bytes(
            NODE,
            &cn,
            hello["ts"].as_i64().unwrap(),
            &client_epk,
            cascade.is_some(),
        );
        verify_mac(&PSK, &hello_data, hello["mac"].as_str().unwrap()).unwrap();

        if let Hub::Reject(reason) = mode {
            let mut reject = json!({"t": "reject"});
            if let Some(reason) = reason {
                reject["reason"] = json!(reason);
            }
            socket
                .send(Message::Text(reject.to_string().into()))
                .await
                .unwrap();
        }

        let secret = StaticSecret::from([5u8; 32]);
        let epk = if mode == Hub::LowOrderKey {
            [0u8; 32]
        } else {
            PublicKey::from(&secret).to_bytes()
        };
        let sn = [6u8; 16];
        let ts = if mode == Hub::ClockOff {
            now_unix() - MAX_SKEW_SECS - 30
        } else {
            now_unix()
        };
        let welcome_data = welcome_bytes(&hello_data, &sn, ts, &epk);
        let mac_key = if mode == Hub::WrongKey {
            [4u8; 32]
        } else {
            PSK
        };
        let welcome = json!({
            "t": "welcome",
            "v": if mode == Hub::OldVersion { 1 } else { PROTOCOL_VERSION },
            "sn": B64.encode(sn),
            "epk": B64.encode(epk),
            "ts": ts,
            "mac": B64.encode(mac(&mac_key, &welcome_data).unwrap()),
        });
        if !matches!(mode, Hub::Reject(_)) {
            socket
                .send(Message::Text(welcome.to_string().into()))
                .await
                .unwrap();
        }

        let shared = secret.diffie_hellman(&PublicKey::from(client_epk));
        let (c2s, s2c) = derive_keys(
            &PSK,
            shared.as_bytes(),
            &hello_data,
            &welcome_data,
            PRIMARY_LABELS,
        )
        .unwrap();
        let layer = cascade.map(|key| {
            derive_keys(
                key,
                shared.as_bytes(),
                &hello_data,
                &welcome_data,
                CASCADE_LABELS,
            )
            .unwrap()
        });
        (
            FrameCodec::new(
                &c2s,
                NODE,
                Direction::ClientToServer,
                layer.as_ref().map(|(c, _)| &**c),
            ),
            FrameCodec::new(
                &s2c,
                NODE,
                Direction::ServerToClient,
                layer.as_ref().map(|(_, s)| &**s),
            ),
        )
    }

    async fn handshake_with(
        cascade: Option<&[u8; 32]>,
        mode: Hub,
    ) -> (Result<SessionCodecs, LinkError>, (FrameCodec, FrameCodec)) {
        let (mut client, mut hub) = sockets().await;
        tokio::join!(
            client_handshake(&mut client, NODE, &PSK, cascade),
            hub_answers(&mut hub, cascade, mode),
        )
    }

    async fn failure_with(mode: Hub) -> LinkError {
        match handshake_with(None, mode).await.0 {
            Ok(_) => panic!("the handshake must fail"),
            Err(error) => error,
        }
    }

    #[tokio::test]
    async fn handshake_gives_both_ends_the_same_keys() {
        for cascade in [None, Some(&CASCADE)] {
            let (client, (mut from_client, mut to_client)) =
                handshake_with(cascade, Hub::Correct).await;
            let SessionCodecs {
                mut encoder,
                mut decoder,
            } = client.unwrap();

            let frame = encoder.encrypt(&json!({"t": "ping"})).unwrap();
            assert_eq!(from_client.decrypt(&frame).unwrap()["t"], "ping");
            let answer = to_client.encrypt(&json!({"t": "pong"})).unwrap();
            assert_eq!(decoder.decrypt(&answer).unwrap()["t"], "pong");
        }
    }

    #[tokio::test]
    async fn handshake_reports_why_home_assistant_refused() {
        assert_eq!(
            failure_with(Hub::Reject(Some("locked"))).await.kind,
            LinkFailure::Locked
        );
        assert_eq!(
            failure_with(Hub::Reject(Some("clock"))).await.kind,
            LinkFailure::Clock
        );
        assert_eq!(
            failure_with(Hub::Reject(Some("cascade"))).await.kind,
            LinkFailure::Auth
        );
        assert_eq!(
            failure_with(Hub::Reject(None)).await.kind,
            LinkFailure::Auth
        );
    }

    #[tokio::test]
    async fn handshake_refuses_a_welcome_it_cannot_trust() {
        let wrong_key = failure_with(Hub::WrongKey).await;
        assert_eq!(wrong_key.kind, LinkFailure::Transport);
        assert_eq!(wrong_key.message, "handshake authentication failed");

        assert_eq!(
            failure_with(Hub::OldVersion).await.kind,
            LinkFailure::Auth,
            "another protocol version is never accepted"
        );
        assert_eq!(failure_with(Hub::ClockOff).await.kind, LinkFailure::Clock);

        let low_order = failure_with(Hub::LowOrderKey).await;
        assert_eq!(low_order.kind, LinkFailure::Transport);
        assert_eq!(low_order.message, "server sent a low-order X25519 key");
    }

    #[tokio::test]
    async fn handshake_fails_when_the_connection_closes_first() {
        let (mut client, hub) = sockets().await;
        drop(hub);
        let error = match client_handshake(&mut client, NODE, &PSK, None).await {
            Ok(_) => panic!("the handshake must fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind, LinkFailure::Transport);
    }
}
