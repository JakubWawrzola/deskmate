//! The Linux path rules of Link Files (link_files.rs). The Windows rules
//! refuse UNC and device paths, alternate data streams and reparse points;
//! here the same intent is:
//!
//! - an absolute path, without `.` and `..`, control characters or NUL;
//! - no symbolic link anywhere in it (link_files.rs checks every component,
//!   as it does for junctions on Windows), so nothing leads out of a shared
//!   folder;
//! - nothing inside the kernel's virtual file systems (`/proc`, `/sys`,
//!   `/dev`), where reading a "file" reveals processes, devices or keys;
//! - no device file, pipe or socket, wherever it is.
//!
//! Paths compare case-sensitively: `/home/me/Shared` and `/home/me/shared`
//! are different folders on Linux.

use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Component, Path, PathBuf};

/// The longest path Linux accepts.
const MAX_PATH_BYTES: usize = 4096;

/// Folders whose contents are not files of the user.
const VIRTUAL_ROOTS: [&str; 3] = ["/proc", "/sys", "/dev"];

/// The default inbox inside the user's download folder.
const INBOX_FOLDER: &str = "Deskmate";

/// Checks the form of a path from the configuration or from Home Assistant.
pub fn validate_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty()
        || raw.len() > MAX_PATH_BYTES
        || raw.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err("path is empty or malformed".into());
    }
    if !raw.starts_with('/') {
        return Err("path must be an absolute path".into());
    }
    if raw
        .split('/')
        .any(|component| matches!(component, "." | ".."))
    {
        return Err("relative path components are not allowed".into());
    }
    Ok(PathBuf::from(raw))
}

/// Refuses what a resolved path must not be: inside a virtual file system,
/// or a device file, pipe or socket.
pub fn reject_special(canonical: &Path) -> Result<(), String> {
    if VIRTUAL_ROOTS
        .iter()
        .any(|root| canonical.starts_with(Path::new(root)))
    {
        return Err("system folders such as /proc, /sys and /dev are not allowed".into());
    }
    let metadata = fs::symlink_metadata(canonical).map_err(|_| "cannot validate path metadata")?;
    if is_special(&metadata.file_type()) {
        return Err("devices, pipes and sockets are not allowed".into());
    }
    Ok(())
}

/// A device file, pipe or socket.
pub fn is_special(file_type: &fs::FileType) -> bool {
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
}

/// Whether two path components are the same name: byte for byte.
pub fn component_eq(left: Component<'_>, right: Component<'_>) -> bool {
    left.as_os_str() == right.as_os_str()
}

/// The default inbox: `Deskmate` in the user's download folder, as
/// `xdg-user-dirs` names it (`~/Pobrane` on a Polish desktop), or else
/// `~/Downloads`.
pub fn default_inbox() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "cannot resolve the home folder")?;
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{home}/.config"));
    let configured = fs::read_to_string(Path::new(&config_home).join("user-dirs.dirs"))
        .ok()
        .and_then(|text| download_dir(&text, &home));
    let downloads = configured.unwrap_or_else(|| Path::new(&home).join("Downloads"));
    Ok(downloads.join(INBOX_FOLDER))
}

/// `XDG_DOWNLOAD_DIR="$HOME/Pobrane"` from `user-dirs.dirs`. Only `$HOME/...`
/// and absolute paths count, as the format allows.
fn download_dir(text: &str, home: &str) -> Option<PathBuf> {
    let line = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("XDG_DOWNLOAD_DIR="))?;
    let value = line.strip_prefix('"')?.strip_suffix('"')?;
    let path = match value.strip_prefix("$HOME") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        Some(_) => return None,
        None => value.to_string(),
    };
    // The home folder itself is no download folder.
    if path.trim_end_matches('/') == home.trim_end_matches('/') {
        return None;
    }
    validate_path(&path).ok()
}

/// Moves the received file into place without ever replacing a file: a hard
/// link fails when the name exists, where a rename would overwrite it. A
/// file system without hard links (FAT, some network shares) falls back to a
/// rename after a last check.
pub fn place_new_file(part: &Path, target: &Path) -> std::io::Result<()> {
    match fs::hard_link(part, target) {
        Ok(()) => fs::remove_file(part),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error),
        Err(_) if target.symlink_metadata().is_ok() => {
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        }
        Err(_) => fs::rename(part, target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("deskmate-linux-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn accepts_absolute_paths_only() {
        assert!(validate_path("/home/me/Shared/report.pdf").is_ok());
        assert!(validate_path("/").is_ok());
        assert!(validate_path("home/me/file").is_err());
        assert!(validate_path("~/file").is_err());
        assert!(validate_path(r"C:\Users\me").is_err());
        assert!(validate_path("").is_err());
    }

    #[test]
    fn refuses_parent_and_current_components_and_control_characters() {
        assert!(validate_path("/home/me/Shared/../.ssh/id_ed25519").is_err());
        assert!(validate_path("/home/me/./Shared").is_err());
        assert!(validate_path("/home/me/..").is_err());
        assert!(validate_path("/home/me/a\0b").is_err());
        assert!(validate_path("/home/me/a\nb").is_err());
        assert!(validate_path(&format!("/{}", "a".repeat(MAX_PATH_BYTES))).is_err());
        // A name that only starts with dots is an ordinary name.
        assert!(validate_path("/home/me/..hidden").is_ok());
    }

    #[test]
    fn refuses_the_virtual_file_systems() {
        assert!(reject_special(Path::new("/proc/self/environ")).is_err());
        assert!(reject_special(Path::new("/proc")).is_err());
        assert!(reject_special(Path::new("/sys/class")).is_err());
        assert!(reject_special(Path::new("/dev/sda")).is_err());
        // Only whole components count.
        assert!(!Path::new("/devices").starts_with("/dev"));
    }

    #[test]
    fn refuses_devices_pipes_and_sockets_but_not_files_and_folders() {
        let dir = scratch("special");
        let file = dir.join("file.txt");
        fs::write(&file, b"text").unwrap();
        let socket = dir.join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(reject_special(&file).is_ok());
        assert!(reject_special(&dir).is_ok());
        assert!(reject_special(&socket).is_err());
        let null = fs::metadata("/dev/null").unwrap();
        assert!(is_special(&null.file_type()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_compare_case_sensitively() {
        let upper = Path::new("/home/me/Shared");
        let lower = Path::new("/home/me/shared");
        let same = upper
            .components()
            .zip(lower.components())
            .all(|(a, b)| component_eq(a, b));
        assert!(!same);
        assert!(component_eq(
            Path::new("/x").components().next().unwrap(),
            Path::new("/y").components().next().unwrap()
        ));
    }

    #[test]
    fn finds_the_download_folder_of_the_desktop() {
        let home = "/home/me";
        let text = "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Pulpit\"\nXDG_DOWNLOAD_DIR=\"$HOME/Pobrane\"\n";
        assert_eq!(
            download_dir(text, home),
            Some(PathBuf::from("/home/me/Pobrane"))
        );
        assert_eq!(
            download_dir("XDG_DOWNLOAD_DIR=\"/data/downloads\"", home),
            Some(PathBuf::from("/data/downloads"))
        );
        assert_eq!(download_dir("XDG_DOWNLOAD_DIR=\"$HOME/\"", home), None);
        assert_eq!(download_dir("XDG_DOWNLOAD_DIR=\"$HOMEX/a\"", home), None);
        assert_eq!(
            download_dir("XDG_DOWNLOAD_DIR=\"$HOME/../etc\"", home),
            None
        );
        assert_eq!(download_dir("XDG_DOWNLOAD_DIR=relative", home), None);
        assert_eq!(download_dir("", home), None);
    }

    #[test]
    fn a_received_file_never_replaces_one_that_exists() {
        let dir = scratch("place");
        let part = dir.join(".deskmate-1.part");
        let target = dir.join("photo.jpg");
        fs::write(&target, b"old").unwrap();
        fs::write(&part, b"new").unwrap();
        assert!(place_new_file(&part, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");

        let free = dir.join("photo (1).jpg");
        place_new_file(&part, &free).unwrap();
        assert_eq!(fs::read(&free).unwrap(), b"new");
        assert!(!part.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
