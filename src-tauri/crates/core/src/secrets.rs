//! Where secrets are kept: pairing keys, the MQTT password, the Home
//! Assistant token. The program picks the store. A desktop app uses the
//! keyring of the signed-in user. A program without a user, such as the
//! daemon, has no keyring to ask and keeps key files in its data folder.
//!
//! A secret is named by a service (`consts::LINK_KEYRING_SERVICE` and its
//! siblings) and an account, the node id for everything but MQTT.

use std::sync::OnceLock;
use zeroize::Zeroizing;

/// A place for secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretStore {
    /// The keyring of the signed-in user: Windows Credential Manager, or on
    /// Linux the Secret Service (GNOME Keyring, KWallet), which needs the
    /// `secret-service` feature of this crate.
    Keyring,
    /// One file per secret in this folder, readable by its owner only.
    #[cfg(unix)]
    Files(std::path::PathBuf),
}

impl SecretStore {
    /// The stored secret, or `Ok(None)` when there is none. An error means
    /// the store could not be asked or refused: it never stands for "no
    /// secret".
    pub fn get(&self, service: &str, account: &str) -> Result<Option<Zeroizing<String>>, String> {
        match self {
            Self::Keyring => user_keyring::get(service, account),
            #[cfg(unix)]
            Self::Files(dir) => key_files::read(dir, service, account),
        }
    }

    /// Stores `secret`, replacing the one already there.
    pub fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        match self {
            Self::Keyring => user_keyring::set(service, account, secret),
            #[cfg(unix)]
            Self::Files(dir) => key_files::write(dir, service, account, secret),
        }
    }

    /// Removes the secret. Not an error when there was none.
    pub fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        match self {
            Self::Keyring => user_keyring::delete(service, account),
            #[cfg(unix)]
            Self::Files(dir) => key_files::remove(dir, service, account),
        }
    }
}

static SELECTED: OnceLock<SecretStore> = OnceLock::new();

/// Picks the store that `config` keeps its secrets in, for this process. Call
/// it once at startup, before anything reads a secret; without the call the
/// store is the user's keyring. Fails when a store is already in use, because
/// secrets read so far came from somewhere else.
pub fn select(store: SecretStore) -> Result<(), String> {
    SELECTED
        .set(store)
        .map_err(|_| "the secret store was already selected or used".to_string())
}

/// The store in use.
pub fn selected() -> &'static SecretStore {
    SELECTED.get_or_init(|| SecretStore::Keyring)
}

#[cfg(any(windows, all(target_os = "linux", feature = "secret-service")))]
mod user_keyring {
    use zeroize::Zeroizing;

    /// keyring's own text on Windows, where it has always been shown as is.
    #[cfg(windows)]
    fn describe(error: keyring::Error) -> String {
        error.to_string()
    }

    /// Names the missing service: "platform secure storage failure" does not
    /// tell a Linux user that no keyring program runs in the session.
    #[cfg(not(windows))]
    fn describe(error: keyring::Error) -> String {
        match error {
            keyring::Error::PlatformFailure(cause) | keyring::Error::NoStorageAccess(cause) => {
                format!(
                    "no keyring service is available (the Secret Service of GNOME Keyring or KWallet, over D-Bus): {cause}"
                )
            }
            other => other.to_string(),
        }
    }

    fn entry(service: &str, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(service, account).map_err(describe)
    }

    pub fn get(service: &str, account: &str) -> Result<Option<Zeroizing<String>>, String> {
        match entry(service, account)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(describe(error)),
        }
    }

    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), String> {
        entry(service, account)?
            .set_password(secret)
            .map_err(describe)
    }

    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        match entry(service, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(describe(error)),
        }
    }
}

/// A build without a keyring refuses, so that a key is never accepted and
/// then lost.
#[cfg(not(any(windows, all(target_os = "linux", feature = "secret-service"))))]
mod user_keyring {
    use zeroize::Zeroizing;

    const UNAVAILABLE: &str =
        "this build of Deskmate cannot use the user's keyring; it keeps secrets in key files";

    pub fn get(_service: &str, _account: &str) -> Result<Option<Zeroizing<String>>, String> {
        Err(UNAVAILABLE.into())
    }

    pub fn set(_service: &str, _account: &str, _secret: &str) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }

    pub fn delete(_service: &str, _account: &str) -> Result<(), String> {
        Err(UNAVAILABLE.into())
    }
}

#[cfg(unix)]
mod key_files {
    use std::fs::{self, File, OpenOptions};
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use zeroize::Zeroizing;

    /// Read and write for the owner, nothing for anyone else.
    const KEY_FILE_MODE: u32 = 0o600;
    /// The permission bits of the group and of everyone else.
    const OTHERS_MASK: u32 = 0o077;
    /// No secret Deskmate stores comes close to this.
    const MAX_KEY_FILE_BYTES: usize = 4096;
    const MAX_ACCOUNT_CHARS: usize = 128;

    /// `<account>.<service>.key`. The service part holds no dot, so two
    /// different secrets never share a file name.
    pub fn path(dir: &Path, service: &str, account: &str) -> Result<PathBuf, String> {
        let service: String = service
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let usable = !account.is_empty()
            && account.len() <= MAX_ACCOUNT_CHARS
            && !account.starts_with('.')
            && account
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '@'));
        if service.is_empty() || !usable {
            return Err("this name cannot be used for a key file".into());
        }
        Ok(dir.join(format!("{account}.{service}.key")))
    }

    pub fn read(
        dir: &Path,
        service: &str,
        account: &str,
    ) -> Result<Option<Zeroizing<String>>, String> {
        let path = path(dir, service, account)?;
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot open {}: {error}", path.display())),
        };
        // Checked on the open file, so the file that passes the check is the
        // one that is read.
        let metadata = file
            .metadata()
            .map_err(|error| format!("cannot check {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        let mode = metadata.permissions().mode();
        if mode & OTHERS_MASK != 0 {
            return Err(format!(
                "{} can be read or changed by other users (mode {:o}); run chmod 600 on it and pair again if someone else may have seen the key",
                path.display(),
                mode & 0o777
            ));
        }
        // Room for the whole file up front: a buffer that grows would leave
        // copies of the key behind in freed memory.
        let mut secret = Zeroizing::new(String::with_capacity(MAX_KEY_FILE_BYTES + 1));
        file.take(MAX_KEY_FILE_BYTES as u64 + 1)
            .read_to_string(&mut secret)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if secret.len() > MAX_KEY_FILE_BYTES {
            return Err(format!("{} is too large to be a key", path.display()));
        }
        // A file written by hand usually ends with a line break.
        let kept = secret.trim_end_matches(['\r', '\n']).len();
        secret.truncate(kept);
        Ok((!secret.is_empty()).then_some(secret))
    }

    pub fn write(dir: &Path, service: &str, account: &str, secret: &str) -> Result<(), String> {
        let path = path(dir, service, account)?;
        fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        // Written next to the target and moved over it, so a reader sees the
        // old key or the new one, never half of a file. create_new refuses a
        // file or link someone put at the temporary name.
        let temporary = path.with_extension("key.tmp");
        let _ = fs::remove_file(&temporary);
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(KEY_FILE_MODE)
            .open(&temporary)
            .and_then(|mut file| {
                file.write_all(secret.as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&temporary, &path));
        written.map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("cannot write {}: {error}", path.display())
        })
    }

    pub fn remove(dir: &Path, service: &str, account: &str) -> Result<(), String> {
        let path = path(dir, service, account)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::SecretStore;
    use crate::consts::{CASCADE_KEYRING_SERVICE, LINK_KEYRING_SERVICE};
    use crate::test_tree::Tree;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // A sample value, not a key anyone uses.
    const SAMPLE: &str = "sample-pairing-value";

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_key_file_round_trips_and_only_its_owner_can_read_it() {
        let tree = Tree::new();
        let store = SecretStore::Files(tree.path().join("data"));
        assert_eq!(store.get(LINK_KEYRING_SERVICE, "office_pc").unwrap(), None);

        store
            .set(LINK_KEYRING_SERVICE, "office_pc", SAMPLE)
            .unwrap();
        let file = tree.path().join("data/office_pc.deskmate-link.key");
        assert_eq!(mode(&file), 0o600);
        assert_eq!(
            store
                .get(LINK_KEYRING_SERVICE, "office_pc")
                .unwrap()
                .unwrap()
                .as_str(),
            SAMPLE
        );
        assert!(!tree
            .path()
            .join("data/office_pc.deskmate-link.key.tmp")
            .exists());

        store.delete(LINK_KEYRING_SERVICE, "office_pc").unwrap();
        assert!(!file.exists());
        assert_eq!(store.get(LINK_KEYRING_SERVICE, "office_pc").unwrap(), None);
        store.delete(LINK_KEYRING_SERVICE, "office_pc").unwrap();
    }

    #[test]
    fn a_key_file_others_can_read_is_refused() {
        let tree = Tree::new();
        let store = SecretStore::Files(tree.path().to_path_buf());
        store
            .set(LINK_KEYRING_SERVICE, "office_pc", SAMPLE)
            .unwrap();
        let file = tree.path().join("office_pc.deskmate-link.key");
        for open_mode in [0o640, 0o604, 0o620, 0o644, 0o666] {
            fs::set_permissions(&file, fs::Permissions::from_mode(open_mode)).unwrap();
            let error = store.get(LINK_KEYRING_SERVICE, "office_pc").unwrap_err();
            assert!(error.contains("other users"), "{open_mode:o}: {error}");
            assert!(!error.contains(SAMPLE), "the key must not be in the error");
        }
        fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(store
            .get(LINK_KEYRING_SERVICE, "office_pc")
            .unwrap()
            .is_some());
    }

    #[test]
    fn writing_replaces_a_key_and_tightens_its_mode() {
        let tree = Tree::new();
        let store = SecretStore::Files(tree.path().to_path_buf());
        store
            .set(LINK_KEYRING_SERVICE, "office_pc", "first")
            .unwrap();
        let file = tree.path().join("office_pc.deskmate-link.key");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

        store
            .set(LINK_KEYRING_SERVICE, "office_pc", "second")
            .unwrap();
        assert_eq!(mode(&file), 0o600);
        let read = store
            .get(LINK_KEYRING_SERVICE, "office_pc")
            .unwrap()
            .unwrap();
        assert_eq!(read.as_str(), "second");
    }

    #[test]
    fn each_secret_has_its_own_file() {
        let tree = Tree::new();
        let store = SecretStore::Files(tree.path().to_path_buf());
        store.set(LINK_KEYRING_SERVICE, "pc", "link").unwrap();
        store.set(CASCADE_KEYRING_SERVICE, "pc", "cascade").unwrap();
        store
            .set(LINK_KEYRING_SERVICE, "pc.cascade", "other")
            .unwrap();
        assert!(tree.path().join("pc.deskmate-link.key").is_file());
        assert!(tree.path().join("pc.deskmate-link-cascade.key").is_file());
        let link = store.get(LINK_KEYRING_SERVICE, "pc").unwrap().unwrap();
        let cascade = store.get(CASCADE_KEYRING_SERVICE, "pc").unwrap().unwrap();
        assert_eq!((link.as_str(), cascade.as_str()), ("link", "cascade"));
    }

    #[test]
    fn a_name_that_could_leave_the_folder_is_refused() {
        let tree = Tree::new();
        let store = SecretStore::Files(tree.path().join("data"));
        for account in ["", "..", "../pc", "a/b", ".hidden", "a b", "a\\b", "pc\n"] {
            assert!(
                store.set(LINK_KEYRING_SERVICE, account, SAMPLE).is_err(),
                "{account:?}"
            );
            assert!(
                store.get(LINK_KEYRING_SERVICE, account).is_err(),
                "{account:?}"
            );
            assert!(
                store.delete(LINK_KEYRING_SERVICE, account).is_err(),
                "{account:?}"
            );
        }
        assert!(store
            .set(LINK_KEYRING_SERVICE, &"a".repeat(129), SAMPLE)
            .is_err());
        assert!(!tree.path().join("data").exists(), "nothing was written");
    }

    #[test]
    fn a_hand_written_key_file_is_read_without_its_line_break() {
        let tree = Tree::new();
        tree.file("pc.deskmate-link.key", "typed-by-hand\n");
        let file = tree.path().join("pc.deskmate-link.key");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let store = SecretStore::Files(tree.path().to_path_buf());
        let read = store.get(LINK_KEYRING_SERVICE, "pc").unwrap().unwrap();
        assert_eq!(read.as_str(), "typed-by-hand");

        fs::write(&file, "\n").unwrap();
        assert_eq!(store.get(LINK_KEYRING_SERVICE, "pc").unwrap(), None);
        fs::write(&file, "x".repeat(5000)).unwrap();
        assert!(store.get(LINK_KEYRING_SERVICE, "pc").is_err());
    }

    #[test]
    fn a_folder_in_place_of_a_key_file_is_refused() {
        let tree = Tree::new();
        fs::create_dir(tree.path().join("pc.deskmate-link.key")).unwrap();
        fs::set_permissions(
            tree.path().join("pc.deskmate-link.key"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let store = SecretStore::Files(tree.path().to_path_buf());
        assert!(store.get(LINK_KEYRING_SERVICE, "pc").is_err());
    }

    #[cfg(not(feature = "secret-service"))]
    #[test]
    fn a_build_without_a_keyring_refuses_instead_of_losing_the_key() {
        let error = SecretStore::Keyring
            .set(LINK_KEYRING_SERVICE, "pc", SAMPLE)
            .unwrap_err();
        assert!(error.contains("key files"), "{error}");
        assert!(SecretStore::Keyring
            .get(LINK_KEYRING_SERVICE, "pc")
            .is_err());
    }

    /// Runs in a child process whose session bus address leads nowhere, so
    /// the developer's real keyring is never touched.
    #[cfg(all(target_os = "linux", feature = "secret-service"))]
    #[test]
    fn without_a_keyring_service_storing_fails_with_a_clear_error() {
        const CHILD: &str = "DESKMATE_TEST_KEYRING_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let error = SecretStore::Keyring
                .set(LINK_KEYRING_SERVICE, "deskmate_test_node", SAMPLE)
                .unwrap_err();
            assert!(error.contains("no keyring service is available"), "{error}");
            assert!(SecretStore::Keyring
                .get(LINK_KEYRING_SERVICE, "deskmate_test_node")
                .is_err());
            return;
        }
        let tree = Tree::new();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "secrets::tests::without_a_keyring_service_storing_fails_with_a_clear_error",
            ])
            .env(CHILD, "1")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", tree.path().join("no-bus").display()),
            )
            .status()
            .unwrap();
        assert!(status.success());
    }
}
