//! File system helpers of the daemon: who this process is, who the daemon's
//! account is, whether a folder can be trusted, and files written whole with
//! the right mode and owner.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// Write permission for the group and for everyone else.
const WRITABLE_BY_OTHERS: u32 = 0o022;
/// The account database. The daemon's account is a system user that
/// systemd-sysusers puts there.
const PASSWD: &str = "/etc/passwd";
/// What the kernel says about this process.
const PROC_STATUS: &str = "/proc/self/status";

/// A user id and its primary group id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Account {
    pub uid: u32,
    pub gid: u32,
}

/// The effective user id from the `Uid:` line of `/proc/self/status`: real,
/// effective, saved and file system id, separated by tabs.
pub fn parse_effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    let ids: Vec<&str> = line.split_whitespace().collect();
    if ids.len() != 4 {
        return None;
    }
    ids[1].parse().ok()
}

/// The effective user id of this process.
pub fn effective_uid() -> Result<u32, String> {
    let status = fs::read_to_string(PROC_STATUS)
        .map_err(|e| format!("cannot tell which user this is: {e}"))?;
    parse_effective_uid(&status).ok_or_else(|| "cannot tell which user this is".to_string())
}

/// The account named `name` in a passwd file: `name:x:uid:gid:gecos:home:shell`.
pub fn parse_passwd(passwd: &str, name: &str) -> Option<Account> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() != 7 || fields[0] != name {
            return None;
        }
        Some(Account {
            uid: fields[2].parse().ok()?,
            gid: fields[3].parse().ok()?,
        })
    })
}

/// The account the daemon runs as.
pub fn daemon_account(name: &str) -> Result<Account, String> {
    let passwd = fs::read_to_string(PASSWD).map_err(|e| format!("cannot read {PASSWD}: {e}"))?;
    parse_passwd(&passwd, name).ok_or_else(|| {
        format!("the user {name} does not exist. Install the Deskmate package first; it creates the user")
    })
}

/// Refuses a folder that is not a real folder, belongs to somebody other
/// than root or this process's user, or that the group or others can write
/// to. Whoever could change these folders could point the daemon at another
/// Home Assistant or replace its keys.
pub fn check_private_dir(dir: &Path) -> Result<(), String> {
    let uid = effective_uid()?;
    let metadata =
        fs::symlink_metadata(dir).map_err(|e| format!("cannot check {}: {e}", dir.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    if metadata.uid() != 0 && metadata.uid() != uid {
        return Err(format!(
            "{} belongs to another user; it must belong to root or to the daemon's user",
            dir.display()
        ));
    }
    if metadata.permissions().mode() & WRITABLE_BY_OTHERS != 0 {
        return Err(format!(
            "{} can be changed by other users (mode {:o}); run chmod go-w on it",
            dir.display(),
            metadata.permissions().mode() & 0o7777
        ));
    }
    Ok(())
}

/// Creates `dir` with `mode` when it is missing, and gives it to `owner`.
pub fn ensure_dir(dir: &Path, mode: u32, owner: Option<Account>) -> Result<(), String> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        fs::set_permissions(dir, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("cannot set the mode of {}: {e}", dir.display()))?;
    }
    if let Some(owner) = owner {
        chown(dir, owner)?;
    }
    Ok(())
}

/// Gives `path` to `owner`.
pub fn chown(path: &Path, owner: Account) -> Result<(), String> {
    std::os::unix::fs::chown(path, Some(owner.uid), Some(owner.gid))
        .map_err(|e| format!("cannot give {} to the daemon's user: {e}", path.display()))
}

/// Writes `contents` to `path` with `mode`, owned by `owner` when given: a
/// temporary file next to it first, then a rename, so a reader sees the old
/// file or the new one and never half of one. `create_new` refuses a file or
/// a link that somebody put at the temporary name.
///
/// The owner is set on the open file, before it gets its name. A chown by
/// path, of the temporary file or after the rename, would follow a link that
/// whoever can write the folder put there in the meantime, and give root's
/// choice of file to the daemon's user.
pub fn write_whole(
    path: &Path,
    contents: &[u8],
    mode: u32,
    owner: Option<Account>,
) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = Path::new(&temporary);
    let _ = fs::remove_file(temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(temporary)
        .and_then(|mut file| {
            file.write_all(contents)?;
            // The umask may have taken bits away; this mode is the one meant.
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            if let Some(owner) = owner {
                std::os::unix::fs::fchown(&file, Some(owner.uid), Some(owner.gid))?;
            }
            file.sync_all()
        })
        .and_then(|()| fs::rename(temporary, path))
        .map_err(|e| e.to_string());
    written.map_err(|error| {
        let _ = fs::remove_file(temporary);
        format!("cannot write {}: {error}", path.display())
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A folder of its own for every test, removed when dropped.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(name: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "deskmate-daemon-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn reads_the_effective_uid_from_the_status_of_a_process() {
        let status = "Name:\tdeskmate\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_effective_uid(status), Some(0));
        assert_eq!(
            parse_effective_uid("Uid:\t1000\t1000\t1000\t1000\n"),
            Some(1000)
        );
        for broken in [
            "",
            "Gid:\t1\t1\t1\t1\n",
            "Uid:\t1000\n",
            "Uid:\ta\tb\tc\td\n",
        ] {
            assert_eq!(parse_effective_uid(broken), None, "{broken}");
        }
        assert!(effective_uid().is_ok());
    }

    #[test]
    fn finds_an_account_by_its_exact_name() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
deskmate-old:x:998:998::/:/usr/sbin/nologin
deskmate:x:997:996:Deskmate daemon:/:/usr/sbin/nologin
";
        assert_eq!(
            parse_passwd(passwd, "deskmate"),
            Some(Account { uid: 997, gid: 996 })
        );
        assert_eq!(
            parse_passwd(passwd, "root"),
            Some(Account { uid: 0, gid: 0 })
        );
        assert_eq!(parse_passwd(passwd, "desk"), None);
        assert_eq!(parse_passwd(passwd, ""), None);
        assert_eq!(
            parse_passwd("deskmate:x:a:b::/:/bin/false\n", "deskmate"),
            None
        );
        assert_eq!(parse_passwd("deskmate:x:1:1\n", "deskmate"), None);
    }

    #[test]
    fn a_folder_others_can_write_to_is_refused() {
        let scratch = Scratch::new("folder");
        assert!(check_private_dir(&scratch.0).is_ok());
        fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_private_dir(&scratch.0).is_ok());
        for loose in [0o775, 0o757, 0o777] {
            fs::set_permissions(&scratch.0, fs::Permissions::from_mode(loose)).unwrap();
            let error = check_private_dir(&scratch.0).unwrap_err();
            assert!(error.contains("other users"), "{error}");
        }
        fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o700)).unwrap();

        let file = scratch.0.join("file");
        fs::write(&file, b"x").unwrap();
        assert!(check_private_dir(&file).is_err());
        let link = scratch.0.join("link");
        std::os::unix::fs::symlink(&scratch.0, &link).unwrap();
        assert!(check_private_dir(&link).is_err(), "a link is not a folder");
        assert!(check_private_dir(&scratch.0.join("missing")).is_err());
    }

    #[test]
    fn a_file_is_written_whole_with_its_mode() {
        let scratch = Scratch::new("write");
        let path = scratch.0.join("config.json");
        write_whole(&path, b"first", 0o600, None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(mode(&path), 0o600);

        write_whole(&path, b"second", 0o644, None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(mode(&path), 0o644);
        assert!(!scratch.0.join("config.json.tmp").exists());

        // A leftover temporary file does not stop the next write.
        fs::write(scratch.0.join("config.json.tmp"), b"stale").unwrap();
        write_whole(&path, b"third", 0o600, None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"third");

        assert!(write_whole(&scratch.0.join("missing/file"), b"x", 0o600, None).is_err());
    }

    #[test]
    fn a_missing_folder_is_created_with_its_mode() {
        let scratch = Scratch::new("ensure");
        let dir = scratch.0.join("state");
        ensure_dir(&dir, 0o700, None).unwrap();
        assert_eq!(mode(&dir), 0o700);
        // An existing folder keeps its mode.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
        ensure_dir(&dir, 0o700, None).unwrap();
        assert_eq!(mode(&dir), 0o750);
    }
}
