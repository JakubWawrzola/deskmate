//! The daemon's keys: one file each in the state folder, through the key
//! files of `deskmate_core::secrets`. A file is mode 0600 and belongs to the
//! daemon's user; one that the group or others can read is refused.
//!
//! The names are the ones the Windows service uses in its credential vault,
//! so `/var/lib/deskmate` holds `machine.deskmate-service-link.key` and, with
//! cascade encryption, `machine.deskmate-service-link-cascade.key`.

use deskmate_core::secrets::SecretStore;
use std::path::{Path, PathBuf};

use crate::secrets::{KeyStore, StoredKeys, CASCADE_KEY_SERVICE, KEY_ACCOUNT, PAIRING_KEY_SERVICE};

/// The file of the pairing key, as `deskmate_core::secrets` names it.
pub const PAIRING_KEY_FILE: &str = "machine.deskmate-service-link.key";
/// The file of the cascade key.
pub const CASCADE_KEY_FILE: &str = "machine.deskmate-service-link-cascade.key";

/// The key files in one folder.
pub struct KeyFiles {
    store: SecretStore,
    dir: PathBuf,
}

impl KeyFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            store: SecretStore::Files(dir.clone()),
            dir,
        }
    }

    /// Both files, whether they exist or not.
    pub fn paths(&self) -> [PathBuf; 2] {
        key_paths(&self.dir)
    }
}

/// Both key files in `dir`.
pub fn key_paths(dir: &Path) -> [PathBuf; 2] {
    [dir.join(PAIRING_KEY_FILE), dir.join(CASCADE_KEY_FILE)]
}

impl KeyStore for KeyFiles {
    fn load(&self) -> Result<Option<StoredKeys>, String> {
        let Some(pairing) = self.store.get(PAIRING_KEY_SERVICE, KEY_ACCOUNT)? else {
            return Ok(None);
        };
        Ok(Some(StoredKeys {
            pairing,
            cascade: self.store.get(CASCADE_KEY_SERVICE, KEY_ACCOUNT)?,
        }))
    }

    fn store(&self, pairing: &str, cascade: Option<&str>) -> Result<(), String> {
        self.store.set(PAIRING_KEY_SERVICE, KEY_ACCOUNT, pairing)?;
        match cascade {
            Some(key) => self.store.set(CASCADE_KEY_SERVICE, KEY_ACCOUNT, key),
            None => self.store.delete(CASCADE_KEY_SERVICE, KEY_ACCOUNT),
        }
    }

    fn clear(&self) -> Result<(), String> {
        let pairing = self.store.delete(PAIRING_KEY_SERVICE, KEY_ACCOUNT);
        let cascade = self.store.delete(CASCADE_KEY_SERVICE, KEY_ACCOUNT);
        pairing.and(cascade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::files::tests::Scratch;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // base64 of 32 bytes of 0x01 and of 0x02
    const KEY_A: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_B: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    #[test]
    fn stores_reads_and_forgets_both_keys_in_the_documented_files() {
        let scratch = Scratch::new("keys");
        let keys = KeyFiles::new(scratch.0.clone());
        assert!(keys.load().unwrap().is_none());

        keys.store(KEY_A, Some(KEY_B)).unwrap();
        let [pairing, cascade] = keys.paths();
        for path in [&pairing, &cascade] {
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", path.display());
        }
        assert_eq!(fs::read_to_string(&pairing).unwrap(), KEY_A);
        let stored = keys.load().unwrap().unwrap();
        assert_eq!(stored.pairing.as_str(), KEY_A);
        assert_eq!(stored.cascade.as_deref().map(String::as_str), Some(KEY_B));

        keys.store(KEY_A, None).unwrap();
        assert!(
            !cascade.exists(),
            "no cascade key without cascade encryption"
        );

        keys.clear().unwrap();
        keys.clear().unwrap();
        assert!(!pairing.exists());
        assert!(keys.load().unwrap().is_none());
    }

    #[test]
    fn a_key_file_others_can_read_is_refused() {
        let scratch = Scratch::new("loose");
        let keys = KeyFiles::new(scratch.0.clone());
        keys.store(KEY_A, None).unwrap();
        let [pairing, _] = keys.paths();
        fs::set_permissions(&pairing, fs::Permissions::from_mode(0o644)).unwrap();
        let error = keys.load().err().unwrap();
        assert!(error.contains("other users"), "{error}");
        assert!(!error.contains(KEY_A));
    }
}
