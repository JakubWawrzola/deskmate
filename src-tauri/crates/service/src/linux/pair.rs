//! `pair` and `unpair`: the daemon's settings and keys, written by root for
//! the daemon's user.
//!
//! `pair` reads a pairing code, checks it the way the app does, and writes
//! `config.json` into the config folder and the key files into the state
//! folder, each mode 0600 and owned by the daemon's user. The keys go first:
//! settings without a key would only produce an error state. `unpair`
//! removes all three.

use deskmate_core::config::AppConfig;
use zeroize::Zeroizing;

use super::files::{self, Account};
use super::keys::KeyFiles;
use super::Paths;
use crate::config::{ServiceConfig, FILE_NAME};
use crate::pairing;
use crate::secrets::{KeyStore, Keys};

/// The settings and the keys are for the daemon's eyes only.
const PRIVATE_FILE_MODE: u32 = 0o600;
/// The config folder: everyone may look, only root may change it.
const CONFIG_DIR_MODE: u32 = 0o755;
/// The state folder: the daemon's own.
const STATE_DIR_MODE: u32 = 0o700;

/// Where pairing writes, and for whom.
pub struct Target {
    pub paths: Paths,
    /// The daemon's account, which gets the files. None for a test run in
    /// folders of its own, where the files stay with whoever runs it.
    pub owner: Option<Account>,
}

impl Target {
    fn keys(&self) -> KeyFiles {
        KeyFiles::owned_by(self.paths.state_dir.clone(), self.owner)
    }
}

/// Pairs the daemon with the Home Assistant that made `code`. Returns the
/// settings that were written. No error names the key.
pub fn pair(code: &str, target: &Target) -> Result<ServiceConfig, String> {
    let Some(code) = pairing::parse(code) else {
        return Err(format!(
            "this is not a pairing code. Copy the code that starts with {} from the Deskmate integration in Home Assistant",
            pairing::PREFIX
        ));
    };
    Keys::decode(&code.key, code.cascade_key.as_deref().map(String::as_str))
        .map_err(|e| format!("the pairing code holds a key that cannot be used: {e}"))?;
    // Settings that cannot be read are replaced, not kept.
    let previous = ServiceConfig::load(&target.paths.config_dir).ok().flatten();
    let config = code.settings(previous.as_ref(), &AppConfig::default().node_id)?;

    files::ensure_dir(&target.paths.config_dir, CONFIG_DIR_MODE, None)?;
    files::ensure_dir(&target.paths.state_dir, STATE_DIR_MODE, target.owner)?;
    let keys = target.keys();
    keys.store(&code.key, code.cascade_key.as_deref().map(String::as_str))
        .map_err(|e| format!("cannot store the keys: {e}"))?;
    let raw = Zeroizing::new(serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?);
    files::write_whole(
        &target.paths.config_dir.join(FILE_NAME),
        &raw,
        PRIVATE_FILE_MODE,
        target.owner,
    )?;
    Ok(config)
}

/// Forgets the settings and both keys. Everything is attempted even when one
/// part fails; files that are already gone are not an error.
pub fn unpair(target: &Target) -> Result<(), String> {
    let settings = ServiceConfig::remove(&target.paths.config_dir);
    let keys = target
        .keys()
        .clear()
        .map_err(|e| format!("cannot remove the keys: {e}"));
    settings.and(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::files::tests::Scratch;
    use crate::provisioning::{Provisioner, Provisioning, SetupSource};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde_json::{json, Value};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    fn code(data: &Value) -> String {
        format!("DMP1.{}", URL_SAFE_NO_PAD.encode(data.to_string()))
    }

    fn target(scratch: &Scratch) -> Target {
        Target {
            paths: Paths {
                config_dir: scratch.0.join("etc"),
                state_dir: scratch.0.join("state"),
                status_file: scratch.0.join("status.json"),
            },
            owner: None,
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn load(target: &Target) -> Provisioning {
        Provisioner::new(
            &target.paths.config_dir,
            Arc::new(KeyFiles::new(target.paths.state_dir.clone())),
        )
        .load()
    }

    #[test]
    fn pairing_writes_private_files_that_the_daemon_reads() {
        let scratch = Scratch::new("pair");
        let target = target(&scratch);
        let pasted = code(&json!({
            "k": KEY_A,
            "u": "ws://homeassistant.local:8123",
            "n": "office_pc",
            "c": KEY_B,
        }));
        let config = pair(&format!("{pasted}\n"), &target).unwrap();
        assert_eq!(config.node_id, "office_pc");
        assert!(config.link_cascade);

        let settings = target.paths.config_dir.join(FILE_NAME);
        assert_eq!(mode(&settings), 0o600);
        assert_eq!(mode(&target.paths.state_dir), 0o700);
        for path in target.keys().paths() {
            assert_eq!(mode(&path), 0o600, "{}", path.display());
        }
        let text = fs::read_to_string(&settings).unwrap();
        assert!(!text.contains(KEY_A) && !text.contains(KEY_B) && !text.contains("key"));

        let Provisioning::Ready(setup) = load(&target) else {
            panic!("the daemon must find a complete setup");
        };
        assert_eq!(setup.config, config);
        assert_eq!(*setup.keys.pairing, [1u8; 32]);
        assert_eq!(setup.keys.cascade.as_deref(), Some(&[2u8; 32]));

        unpair(&target).unwrap();
        unpair(&target).unwrap();
        assert!(matches!(load(&target), Provisioning::NotProvisioned));
        for path in target.keys().paths() {
            assert!(!path.exists());
        }
    }

    #[test]
    fn pairing_again_keeps_the_choices_the_code_does_not_carry() {
        let scratch = Scratch::new("repair");
        let target = target(&scratch);
        let first = code(&json!({"k": KEY_A, "u": "ws://192.168.1.10:8123", "c": KEY_B}));
        let mut config = pair(&first, &target).unwrap();
        config.device_name = "Office".into();
        config.publish_interval_secs = 60;
        fs::write(
            target.paths.config_dir.join(FILE_NAME),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();

        let second = code(&json!({"k": KEY_B, "u": "ws://192.168.1.11:8123"}));
        let again = pair(&second, &target).unwrap();
        assert_eq!(again.device_name, "Office");
        assert_eq!(again.publish_interval_secs, 60);
        assert!(!again.link_cascade);
        let [_, cascade] = target.keys().paths();
        assert!(!cascade.exists(), "the old cascade key is gone");
    }

    #[test]
    fn a_code_that_cannot_be_used_changes_nothing() {
        let scratch = Scratch::new("refuse");
        let target = target(&scratch);
        let refused = [
            KEY_A.to_string(),
            "DMP1.!!!!".to_string(),
            code(&json!({"k": "not a key", "u": "ws://192.168.1.10:8123"})),
            code(&json!({"k": KEY_A, "u": "ws://192.168.1.10:8123", "c": KEY_A})),
            code(&json!({"k": KEY_A})),
            code(&json!({"k": KEY_A, "u": "ws://example.com"})),
        ];
        for input in refused {
            let error = pair(&input, &target).unwrap_err();
            assert!(!error.contains(KEY_A), "{error}");
            assert!(!target.paths.config_dir.exists(), "{input}");
            assert!(!target.paths.state_dir.exists(), "{input}");
        }
    }

    #[test]
    fn pairing_needs_a_folder_it_can_write_to() {
        let scratch = Scratch::new("readonly");
        let target = Target {
            paths: Paths {
                config_dir: scratch.0.join("file/etc"),
                ..target(&scratch).paths
            },
            owner: None,
        };
        fs::write(scratch.0.join("file"), b"not a folder").unwrap();
        let pasted = code(&json!({"k": KEY_A, "u": "ws://192.168.1.10:8123"}));
        assert!(pair(&pasted, &target).is_err());
    }
}
