//! The daemon's keys: one file each in the state folder, through the key
//! files of `deskmate_core::secrets`. A file is mode 0600 and belongs to the
//! daemon's user; one that the group or others can read is refused.
//!
//! The names are the ones the Windows service uses in its credential vault,
//! so `/var/lib/deskmate` holds `machine.deskmate-service-link.key` and, with
//! cascade encryption, `machine.deskmate-service-link-cascade.key`.

use deskmate_core::secrets::SecretStore;
use std::path::{Path, PathBuf};

use super::files::{self, Account};
use crate::secrets::{KeyStore, StoredKeys, CASCADE_KEY_SERVICE, KEY_ACCOUNT, PAIRING_KEY_SERVICE};

/// The file of the pairing key, as `deskmate_core::secrets` names it.
pub const PAIRING_KEY_FILE: &str = "machine.deskmate-service-link.key";
/// The file of the cascade key.
pub const CASCADE_KEY_FILE: &str = "machine.deskmate-service-link-cascade.key";
/// Read and write for the owner only, as `deskmate_core::secrets` writes them.
const KEY_FILE_MODE: u32 = 0o600;

/// The key files in one folder.
pub struct KeyFiles {
    store: SecretStore,
    dir: PathBuf,
    /// Who gets the files that are written. None: whoever writes them.
    owner: Option<Account>,
}

impl KeyFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self::owned_by(dir, None)
    }

    /// Key files that root writes for `owner`, the daemon's user.
    pub fn owned_by(dir: PathBuf, owner: Option<Account>) -> Self {
        Self {
            store: SecretStore::Files(dir.clone()),
            dir,
            owner,
        }
    }

    fn write(&self, path: &Path, service: &str, key: &str) -> Result<(), String> {
        match self.owner {
            // The daemon's user owns this folder and can swap a file for a
            // link at any moment, so the file gets its owner while root
            // still holds it open, never by its name.
            Some(owner) => files::write_whole(path, key.as_bytes(), KEY_FILE_MODE, Some(owner)),
            None => self.store.set(service, KEY_ACCOUNT, key),
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
        let [pairing_file, cascade_file] = self.paths();
        self.write(&pairing_file, PAIRING_KEY_SERVICE, pairing)?;
        match cascade {
            Some(key) => self.write(&cascade_file, CASCADE_KEY_SERVICE, key),
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

    #[test]
    fn keys_written_for_the_daemon_never_hand_over_a_linked_file() {
        use std::os::unix::fs::{symlink, MetadataExt};

        let scratch = Scratch::new("owned");
        let state = scratch.0.join("state");
        fs::create_dir(&state).unwrap();
        // Root gives the files away; anyone else can only give them to itself.
        let owner = if files::effective_uid().unwrap() == 0 {
            Account {
                uid: 65534,
                gid: 65534,
            }
        } else {
            let own = fs::metadata(&state).unwrap();
            Account {
                uid: own.uid(),
                gid: own.gid(),
            }
        };
        let keys = KeyFiles::owned_by(state.clone(), Some(owner));
        // What the daemon's user could have put at the names meanwhile.
        let victim = scratch.0.join("shadow");
        fs::write(&victim, b"untouched").unwrap();
        let before = fs::metadata(&victim).unwrap();
        for path in keys.paths() {
            symlink(&victim, &path).unwrap();
        }

        keys.store(KEY_A, Some(KEY_B)).unwrap();
        for path in keys.paths() {
            let file = fs::symlink_metadata(&path).unwrap();
            assert!(file.is_file(), "{} is a file, not a link", path.display());
            assert_eq!((file.uid(), file.gid()), (owner.uid, owner.gid));
            assert_eq!(file.permissions().mode() & 0o777, 0o600);
        }
        let after = fs::metadata(&victim).unwrap();
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        let stored = keys.load().unwrap().unwrap();
        assert_eq!(stored.pairing.as_str(), KEY_A);
    }
}
