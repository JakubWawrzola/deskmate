//! What the service was set up with: the settings file in the data folder
//! and the keys in the vault, read together and written together.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::admin::{Provision, Request};
use crate::audit::AuditSink;
use crate::config::ServiceConfig;
use crate::runner::{Event, EventSender};
use crate::secrets::{KeyStore, Keys};

/// Settings and keys that passed every check.
pub struct Setup {
    pub config: ServiceConfig,
    pub keys: Keys,
    /// `ServiceConfig::fingerprint` of the two.
    pub fingerprint: String,
}

/// What the service finds when it looks for its settings.
pub enum Provisioning {
    /// No settings file: the service was never set up, or was cleared.
    NotProvisioned,
    /// Settings or keys exist but cannot be used. The text is for the status
    /// line and names no value.
    Invalid(String),
    Ready(Box<Setup>),
}

/// Where the runner gets its settings from.
pub trait SetupSource: Send + Sync {
    fn load(&self) -> Provisioning;
}

/// The data folder and the key store of this service.
pub struct Provisioner {
    dir: PathBuf,
    keys: Arc<dyn KeyStore>,
}

impl Provisioner {
    pub fn new(dir: &Path, keys: Arc<dyn KeyStore>) -> Self {
        Self {
            dir: dir.to_path_buf(),
            keys,
        }
    }

    /// Stores what a `provision` frame brought. The keys go first: settings
    /// without a key would only produce an error state.
    pub fn provision(&self, request: &Provision) -> Result<(), String> {
        self.keys
            .store(
                request.key.as_str(),
                request.cascade_key.as_ref().map(|key| key.as_str()),
            )
            .map_err(|e| format!("cannot store the keys: {e}"))?;
        request.config.save(&self.dir)
    }

    /// Forgets settings and keys. Both are attempted even when one fails.
    pub fn deprovision(&self) -> Result<(), String> {
        let settings = ServiceConfig::remove(&self.dir);
        let keys = self
            .keys
            .clear()
            .map_err(|e| format!("cannot remove the keys: {e}"));
        settings.and(keys)
    }
}

/// Carries out what an admin frame asks for, and tells the runner to read
/// its setup again.
pub struct AdminActions {
    provisioner: Arc<Provisioner>,
    events: EventSender,
    audit: Arc<dyn AuditSink>,
}

impl AdminActions {
    pub fn new(
        provisioner: Arc<Provisioner>,
        events: EventSender,
        audit: Arc<dyn AuditSink>,
    ) -> Self {
        Self {
            provisioner,
            events,
            audit,
        }
    }

    /// Runs one request. The error text names the step that failed and no
    /// value from the request.
    pub fn carry_out(&self, request: &Request) -> Result<(), String> {
        let name = request.name();
        let result = match request {
            Request::Provision(provision) => self.provisioner.provision(provision),
            Request::Deprovision => self.provisioner.deprovision(),
            Request::Reload => Ok(()),
        };
        match &result {
            Ok(()) => log::info!("admin: {name}"),
            Err(error) => log::error!("admin: {name} failed: {error}"),
        }
        let outcome = if result.is_ok() { "ok" } else { "failed" };
        self.audit
            .record("admin", &format!("op={name} result={outcome}"));
        // Whatever was stored, also half of it, is what the runner must see.
        let _ = self.events.send(Event::Reload);
        result
    }
}

impl SetupSource for Provisioner {
    fn load(&self) -> Provisioning {
        let config = match ServiceConfig::load(&self.dir) {
            Ok(Some(config)) => config,
            Ok(None) => return Provisioning::NotProvisioned,
            Err(error) => {
                log::error!("settings: {error}");
                return Provisioning::Invalid("The settings file is not valid.".into());
            }
        };
        let stored = match self.keys.load() {
            Ok(Some(stored)) => stored,
            Ok(None) => return Provisioning::Invalid("The pairing key is missing.".into()),
            Err(error) => {
                log::error!("keys: cannot read the vault: {error}");
                return Provisioning::Invalid("The pairing key cannot be read.".into());
            }
        };
        // A cascade key that is stored but switched off is left alone.
        let cascade = if config.link_cascade {
            match stored.cascade.as_ref() {
                Some(key) => Some(key.as_str()),
                None => {
                    return Provisioning::Invalid(
                        "Cascade encryption is on but its key is missing.".into(),
                    )
                }
            }
        } else {
            None
        };
        let keys = match Keys::decode(&stored.pairing, cascade) {
            Ok(keys) => keys,
            Err(error) => {
                // The text comes from the key checks and names no value.
                log::error!("keys: {error}");
                return Provisioning::Invalid("A stored key is not valid.".into());
            }
        };
        match config.fingerprint(&keys.pairing, keys.cascade.as_deref()) {
            Ok(fingerprint) => Provisioning::Ready(Box::new(Setup {
                config,
                keys,
                fingerprint,
            })),
            Err(error) => {
                log::error!("settings: cannot compute the fingerprint: {error}");
                Provisioning::Invalid("The settings cannot be used.".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{self, Request};
    use crate::secrets::StoredKeys;
    use serde_json::json;
    use std::sync::Mutex;
    use zeroize::Zeroizing;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    /// A key store in memory.
    #[derive(Default)]
    struct MemoryKeys(Mutex<Option<(String, Option<String>)>>);

    impl KeyStore for MemoryKeys {
        fn load(&self) -> Result<Option<StoredKeys>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .as_ref()
                .map(|(pairing, cascade)| StoredKeys {
                    pairing: Zeroizing::new(pairing.clone()),
                    cascade: cascade.clone().map(Zeroizing::new),
                }))
        }

        fn store(&self, pairing: &str, cascade: Option<&str>) -> Result<(), String> {
            *self.0.lock().unwrap() = Some((pairing.into(), cascade.map(Into::into)));
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "deskmate-service-setup-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn provision_request(cascade: bool) -> Box<Provision> {
        let mut frame = json!({
            "v": 1,
            "op": "provision",
            "node_id": "office_pc",
            "link_url": "ws://homeassistant.local:8123",
            "key": KEY_A,
        });
        if cascade {
            frame["link_cascade"] = json!(true);
            frame["cascade_key"] = json!(KEY_B);
        }
        match admin::parse(frame.to_string().as_bytes()) {
            Ok(Request::Provision(request)) => request,
            _ => panic!("the frame must be accepted"),
        }
    }

    #[test]
    fn a_fresh_folder_is_not_provisioned() {
        let dir = folder("fresh");
        let provisioner = Provisioner::new(&dir, Arc::new(MemoryKeys::default()));
        assert!(matches!(provisioner.load(), Provisioning::NotProvisioned));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn provisioning_stores_settings_and_keys_and_deprovisioning_forgets_both() {
        let dir = folder("cycle");
        let keys = Arc::new(MemoryKeys::default());
        let provisioner = Provisioner::new(&dir, keys.clone());

        provisioner.provision(&provision_request(true)).unwrap();
        let Provisioning::Ready(setup) = provisioner.load() else {
            panic!("the service must be ready");
        };
        assert_eq!(setup.config.node_id, "office_pc");
        assert_eq!(*setup.keys.pairing, [1u8; 32]);
        assert_eq!(setup.keys.cascade.as_deref(), Some(&[2u8; 32]));
        assert_eq!(
            setup.fingerprint,
            setup
                .config
                .fingerprint(&[1u8; 32], Some(&[2u8; 32]))
                .unwrap()
        );
        // The settings file never holds a key.
        let file = std::fs::read_to_string(dir.join(crate::config::FILE_NAME)).unwrap();
        assert!(!file.contains(KEY_A) && !file.contains(KEY_B));
        assert!(!file.contains("key"));

        // Provisioning again without cascade drops the cascade key.
        provisioner.provision(&provision_request(false)).unwrap();
        assert_eq!(keys.0.lock().unwrap().as_ref().unwrap().1, None);

        provisioner.deprovision().unwrap();
        assert!(matches!(provisioner.load(), Provisioning::NotProvisioned));
        assert!(keys.0.lock().unwrap().is_none());
        provisioner.deprovision().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_without_usable_keys_are_an_error_that_names_no_value() {
        let dir = folder("keys");
        let keys = Arc::new(MemoryKeys::default());
        let provisioner = Provisioner::new(&dir, keys.clone());
        provisioner.provision(&provision_request(true)).unwrap();

        let detail = |provisioner: &Provisioner| match provisioner.load() {
            Provisioning::Invalid(detail) => detail,
            _ => panic!("the setup must be invalid"),
        };

        *keys.0.lock().unwrap() = None;
        assert_eq!(detail(&provisioner), "The pairing key is missing.");

        *keys.0.lock().unwrap() = Some((KEY_A.into(), None));
        assert_eq!(
            detail(&provisioner),
            "Cascade encryption is on but its key is missing."
        );

        *keys.0.lock().unwrap() = Some(("not-a-key".into(), Some(KEY_B.into())));
        assert_eq!(detail(&provisioner), "A stored key is not valid.");

        *keys.0.lock().unwrap() = Some((KEY_A.into(), Some(KEY_A.into())));
        assert_eq!(detail(&provisioner), "A stored key is not valid.");

        std::fs::write(dir.join(crate::config::FILE_NAME), b"{\"node_id\": 5}").unwrap();
        assert_eq!(detail(&provisioner), "The settings file is not valid.");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
