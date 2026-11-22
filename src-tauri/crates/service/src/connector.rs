//! The connection to Home Assistant as the runner sees it: something that
//! opens a session, and a session that carries frames. The real one is a
//! WebSocket with the Link handshake and frame encryption from
//! `deskmate_core::link`; the tests of the runner put their own in its place.

use deskmate_core::link::{client_handshake, open_in_time, FrameCodec, LinkError, SessionCodecs};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::future::Future;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::secrets::Keys;

/// Opens sessions with Home Assistant.
pub trait Connector: Send + Sync + 'static {
    type Session: LinkSession;

    /// Connects to `url` and completes the handshake as `node`.
    fn connect(
        &self,
        url: &str,
        node: &str,
        keys: &Keys,
    ) -> impl Future<Output = Result<Self::Session, LinkError>> + Send;
}

/// An open session: decrypted frames in both directions.
pub trait LinkSession: Send {
    fn send(&mut self, frame: &Value) -> impl Future<Output = Result<(), LinkError>> + Send;

    /// The next frame from Home Assistant; Ok(None) when it closed the
    /// connection. Dropping the future before it completes loses nothing.
    fn receive(&mut self) -> impl Future<Output = Result<Option<Value>, LinkError>> + Send;

    /// Closes the connection. Frames sent before are delivered first.
    fn close(&mut self) -> impl Future<Output = ()> + Send;
}

/// Connects over WebSocket; `wss://` uses the system's TLS.
pub struct WebSocketConnector;

/// A Link session over a WebSocket.
pub struct WebSocketSession {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    encoder: FrameCodec,
    decoder: FrameCodec,
}

impl Connector for WebSocketConnector {
    type Session = WebSocketSession;

    async fn connect(
        &self,
        url: &str,
        node: &str,
        keys: &Keys,
    ) -> Result<Self::Session, LinkError> {
        let (mut socket, _) = open_in_time(tokio_tungstenite::connect_async(url)).await?;
        let SessionCodecs { encoder, decoder } =
            client_handshake(&mut socket, node, &keys.pairing, keys.cascade.as_deref()).await?;
        Ok(WebSocketSession {
            socket,
            encoder,
            decoder,
        })
    }
}

impl LinkSession for WebSocketSession {
    async fn send(&mut self, frame: &Value) -> Result<(), LinkError> {
        let text = self.encoder.encrypt(frame)?;
        self.socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| LinkError::from(e.to_string()))
    }

    async fn receive(&mut self) -> Result<Option<Value>, LinkError> {
        loop {
            // The only await: a frame is either still in the socket or fully
            // read and decrypted, so a dropped future loses nothing.
            match self.socket.next().await {
                None | Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Err(error)) => return Err(error.to_string().into()),
                Some(Ok(Message::Text(text))) => {
                    return Ok(Some(self.decoder.decrypt(text.as_str())?));
                }
                // The WebSocket library answers a ping by itself.
                Some(Ok(_)) => {}
            }
        }
    }

    async fn close(&mut self) {
        let _ = self.socket.close(None).await;
    }
}
