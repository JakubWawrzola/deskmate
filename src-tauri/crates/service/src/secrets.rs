//! The pairing key and the optional cascade key of the service.
//!
//! They live in the credential vault of the account the service runs as
//! (Windows Credential Manager for LocalSystem), under names of their own, so
//! they never meet the keys an app keeps for its user. They are not written
//! to the settings file, a log, the status line, an argument or the
//! environment, and every copy this crate makes is wiped when dropped.

use deskmate_core::link::{validate_pairing_key, SecretKey};
use zeroize::Zeroizing;

/// Service name of the pairing key in the credential vault.
pub const PAIRING_KEY_SERVICE: &str = "Deskmate Service Link";
/// Service name of the cascade key in the credential vault.
pub const CASCADE_KEY_SERVICE: &str = "Deskmate Service Link Cascade";
/// Account name of both entries. It is fixed, so the entries can be removed
/// without knowing which computer name the service was provisioned with.
pub const KEY_ACCOUNT: &str = "machine";

/// The keys of a provisioned service, decoded and checked.
pub struct Keys {
    pub pairing: SecretKey,
    /// Present when cascade encryption is on.
    pub cascade: Option<SecretKey>,
}

impl Keys {
    /// Decodes the keys as they arrive in a `provision` frame or come out of
    /// the vault: base64 of 32 bytes each, and two different keys.
    pub fn decode(pairing: &str, cascade: Option<&str>) -> Result<Self, String> {
        let pairing = validate_pairing_key(pairing)?;
        let cascade = match cascade {
            Some(raw) => {
                let key = validate_pairing_key(raw).map_err(|e| format!("cascade key: {e}"))?;
                if *key == *pairing {
                    return Err(
                        "the cascade key must be the separate key Home Assistant shows, not the pairing key"
                            .into(),
                    );
                }
                Some(key)
            }
            None => None,
        };
        Ok(Self { pairing, cascade })
    }
}

/// Where the service keeps its keys.
pub trait KeyStore: Send + Sync {
    /// The stored keys, still in their text form. Ok(None) when there is no
    /// pairing key.
    fn load(&self) -> Result<Option<StoredKeys>, String>;
    /// Replaces both keys. Without a cascade key the stored one is removed.
    fn store(&self, pairing: &str, cascade: Option<&str>) -> Result<(), String>;
    /// Forgets both keys. Keys that are already gone are not an error.
    fn clear(&self) -> Result<(), String>;
}

/// Keys as text (base64), wiped when dropped.
pub struct StoredKeys {
    pub pairing: Zeroizing<String>,
    pub cascade: Option<Zeroizing<String>>,
}

/// The credential vault of the account this process runs as.
pub struct CredentialVault;

impl CredentialVault {
    fn entry(service: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(service, KEY_ACCOUNT).map_err(|e| e.to_string())
    }

    fn read(service: &str) -> Result<Option<Zeroizing<String>>, String> {
        match Self::entry(service)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn remove(service: &str) -> Result<(), String> {
        match Self::entry(service)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl KeyStore for CredentialVault {
    fn load(&self) -> Result<Option<StoredKeys>, String> {
        let Some(pairing) = Self::read(PAIRING_KEY_SERVICE)? else {
            return Ok(None);
        };
        Ok(Some(StoredKeys {
            pairing,
            cascade: Self::read(CASCADE_KEY_SERVICE)?,
        }))
    }

    fn store(&self, pairing: &str, cascade: Option<&str>) -> Result<(), String> {
        Self::entry(PAIRING_KEY_SERVICE)?
            .set_password(pairing)
            .map_err(|e| e.to_string())?;
        match cascade {
            Some(key) => Self::entry(CASCADE_KEY_SERVICE)?
                .set_password(key)
                .map_err(|e| e.to_string()),
            None => Self::remove(CASCADE_KEY_SERVICE),
        }
    }

    fn clear(&self) -> Result<(), String> {
        let pairing = Self::remove(PAIRING_KEY_SERVICE);
        let cascade = Self::remove(CASCADE_KEY_SERVICE);
        pairing.and(cascade)
    }
}

/// A pairing key in a file, for local test runs: the vault of the developer's
/// account stays untouched. The file is only read.
#[cfg(feature = "test-hooks")]
pub struct KeyFile {
    path: std::path::PathBuf,
}

#[cfg(feature = "test-hooks")]
impl KeyFile {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(feature = "test-hooks")]
impl KeyStore for KeyFile {
    fn load(&self) -> Result<Option<StoredKeys>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let text = Zeroizing::new(text);
                Ok(Some(StoredKeys {
                    pairing: Zeroizing::new(text.trim().to_string()),
                    cascade: None,
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("cannot read the key file: {error}")),
        }
    }

    fn store(&self, _pairing: &str, _cascade: Option<&str>) -> Result<(), String> {
        Err("a run with a key file cannot be provisioned".into())
    }

    fn clear(&self) -> Result<(), String> {
        Err("a run with a key file cannot be deprovisioned".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    #[test]
    fn decodes_a_pairing_key_with_and_without_a_cascade_key() {
        let keys = Keys::decode(KEY_A, None).unwrap();
        assert_eq!(*keys.pairing, [1u8; 32]);
        assert!(keys.cascade.is_none());

        let keys = Keys::decode(KEY_A, Some(KEY_B)).unwrap();
        assert_eq!(keys.cascade.map(|key| *key), Some([2u8; 32]));
    }

    #[test]
    fn refuses_keys_that_cannot_be_used() {
        assert!(Keys::decode("", None).is_err());
        assert!(Keys::decode("not base64", None).is_err());
        assert!(Keys::decode("AQEB", None).is_err());
        assert!(Keys::decode(KEY_A, Some("AQEB")).is_err());
        let same = Keys::decode(KEY_A, Some(KEY_A));
        assert!(same.is_err_and(|error| error.contains("separate key")));
    }

    #[test]
    fn the_vault_names_are_not_the_ones_the_app_uses() {
        use deskmate_core::consts;
        for name in [PAIRING_KEY_SERVICE, CASCADE_KEY_SERVICE] {
            for taken in [
                consts::KEYRING_SERVICE,
                consts::LINK_KEYRING_SERVICE,
                consts::CASCADE_KEYRING_SERVICE,
                consts::HA_TOKEN_KEYRING_SERVICE,
            ] {
                assert_ne!(name, taken);
            }
        }
    }
}
