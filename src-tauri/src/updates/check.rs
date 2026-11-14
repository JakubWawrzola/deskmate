//! The two places Deskmate asks about a newer release: GitHub's release API,
//! for the version number only, and the release manifest, through the updater
//! plugin. Nothing is downloaded here.

use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

use super::policy::{self, ReleaseVersion};
use super::RUNNING_VERSION;

const LATEST_API: &str = "https://api.github.com/repos/JakubWawrzola/deskmate/releases/latest";
const USER_AGENT: &str = concat!("Deskmate/", env!("CARGO_PKG_VERSION"));
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// Debug builds only: another address for the release manifest, to try the
/// flow against a local file server. Release builds never read it.
#[cfg(debug_assertions)]
const ENDPOINT_VAR: &str = "DESKMATE_UPDATE_ENDPOINT";

fn fetch_latest_tag() -> Result<String, String> {
    let connector = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(connector))
        .timeout(CHECK_TIMEOUT)
        .build();
    let body = agent
        .get(LATEST_API)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the release has no tag".to_string())
}

/// Asks GitHub's release API whether a newer release exists. Only the version
/// number is taken from the answer. Blocking; call it off the async runtime.
pub(super) fn newer_release() -> Result<Option<ReleaseVersion>, String> {
    let tag = fetch_latest_tag()?;
    let latest = ReleaseVersion::parse(&tag).ok_or_else(|| "unexpected release tag".to_string())?;
    Ok(policy::is_newer(RUNNING_VERSION, &tag).then_some(latest))
}

#[cfg(debug_assertions)]
fn endpoint_override() -> Option<url::Url> {
    let address = std::env::var(ENDPOINT_VAR).ok()?;
    url::Url::parse(address.trim()).ok()
}

#[cfg(not(debug_assertions))]
fn endpoint_override() -> Option<url::Url> {
    None
}

/// Reads the release manifest through the updater plugin. Ok(None): it offers
/// nothing newer than the running version. The installer it names is not
/// fetched here, only its address is checked.
pub(super) async fn installable_release(app: &AppHandle) -> Result<Option<Update>, String> {
    let mut builder = app
        .updater_builder()
        .version_comparator(|running, release| {
            policy::is_newer(&running.to_string(), &release.version.to_string())
        })
        .timeout(CHECK_TIMEOUT)
        .header("User-Agent", USER_AGENT)
        .map_err(|e| e.to_string())?;
    builder = match endpoint_override() {
        Some(endpoint) => builder
            .endpoints(vec![endpoint])
            .map_err(|e| e.to_string())?,
        // GitHub redirects to its file storage. Redirects are followed, but
        // never off HTTPS.
        None => builder.configure_client(|client| client.https_only(true)),
    };
    let updater = builder.build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if !policy::download_address_allowed(update.download_url.as_str()) {
        crate::security::audit("update_manifest", "refused_address");
        return Err("the release manifest points outside Deskmate's GitHub releases".into());
    }
    Ok(Some(update))
}

#[cfg(test)]
mod tests {
    use super::policy;

    /// The shape scripts/make_release_manifest.py writes: the plugin has to
    /// read it, and its addresses have to pass the address rule.
    #[test]
    fn a_manifest_from_the_release_script_is_understood() {
        let manifest = r#"{
          "version": "0.9.0",
          "notes": "Signed automatic updates.",
          "pub_date": "2026-11-14T12:30:00Z",
          "platforms": {
            "windows-x86_64": {
              "signature": "c2FtcGxl",
              "url": "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/Deskmate_0.9.0_x64-setup.exe"
            },
            "windows-aarch64": {
              "signature": "c2FtcGxl",
              "url": "https://github.com/JakubWawrzola/deskmate/releases/download/v0.9.0/Deskmate_0.9.0_arm64-setup.exe"
            }
          }
        }"#;
        let release: tauri_plugin_updater::RemoteRelease = serde_json::from_str(manifest).unwrap();

        assert!(policy::is_newer("0.8.5", &release.version.to_string()));
        for target in ["windows-x86_64", "windows-aarch64"] {
            let address = release.download_url(target).unwrap();
            assert!(policy::download_address_allowed(address.as_str()));
            assert_eq!(release.signature(target).unwrap(), "c2FtcGxl");
        }
        assert!(release.download_url("linux-x86_64").is_err());
    }

    /// tauri.conf.json is read by the updater plugin when the app starts. A
    /// typo there would stop the app, and a relaxed setting would widen what
    /// it installs.
    #[test]
    fn the_shipped_updater_settings_are_the_strict_ones() {
        use base64::Engine;

        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
        let settings: tauri_plugin_updater::Config =
            serde_json::from_value(conf["plugins"]["updater"].clone()).unwrap();

        let endpoints: Vec<&str> = settings.endpoints.iter().map(url::Url::as_str).collect();
        assert_eq!(
            endpoints,
            ["https://github.com/JakubWawrzola/deskmate/releases/latest/download/latest.json"]
        );
        assert!(settings.require_signed_version);
        assert!(!settings.allow_downgrades);
        assert!(!settings.dangerous_insecure_transport_protocol);
        assert!(!settings.dangerous_accept_invalid_certs);
        assert!(!settings.dangerous_accept_invalid_hostnames);

        // A minisign public key: a comment line, then algorithm, key id and key.
        let standard = base64::engine::general_purpose::STANDARD;
        let text = String::from_utf8(standard.decode(&settings.pubkey).unwrap()).unwrap();
        let mut lines = text.lines();
        assert!(lines
            .next()
            .unwrap()
            .starts_with("untrusted comment: minisign public key"));
        let key = standard.decode(lines.next().unwrap()).unwrap();
        assert_eq!(key.len(), 2 + 8 + 32);
        assert_eq!(&key[..2], b"Ed");
    }
}
