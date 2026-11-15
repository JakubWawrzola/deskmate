//! Which mounted file systems get their own sensors, and under which names.
//! Windows names a drive by its letter; here it is the mount point.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The mount point of the system drive.
pub const ROOT_MOUNT: &str = "/";
/// What the system drive is called in a sensor key.
const ROOT_KEY: &str = "root";
/// Boot partitions hold nothing a user fills up. Media that a desktop mounts
/// for the signed-in user carry that user's name and the volume's label in
/// their path, which the machine has no business reporting.
const SKIPPED_ROOTS: [&str; 4] = ["/boot", "/efi", "/media", "/run/media"];
/// Keeps `disk_<key>_usage` well inside what Home Assistant accepts.
const MAX_KEY_CHARS: usize = 40;

/// A mounted file system with sensors of its own.
#[derive(Debug, PartialEq, Eq)]
pub struct Volume {
    /// Position in the list given to `volumes`.
    pub index: usize,
    /// The part of the sensor key that names the volume: `disk_<key>_free`.
    pub key: String,
    /// The part of the sensor name: `Disk <label> free`.
    pub label: String,
}

/// Picks the volumes out of `mounts`, the device and mount point of every
/// listed file system. The root always counts. Of the others, those on a
/// block device do, once per device: a device mounted in several places
/// (bind mounts, btrfs subvolumes) is one drive, named by its shortest mount
/// point.
pub fn volumes(mounts: &[(String, PathBuf)]) -> Vec<Volume> {
    let mut candidates: Vec<(usize, &str, &Path)> = mounts
        .iter()
        .enumerate()
        .map(|(index, (device, mount))| (index, device.as_str(), mount.as_path()))
        .filter(|(_, device, mount)| {
            *mount == Path::new(ROOT_MOUNT)
                || (device.starts_with("/dev/")
                    && !SKIPPED_ROOTS.iter().any(|root| mount.starts_with(root)))
        })
        .collect();
    candidates.sort_by_key(|(_, _, mount)| (mount.as_os_str().len(), mount.to_path_buf()));

    let mut devices = HashSet::new();
    let mut keys = HashSet::new();
    let mut picked: Vec<Volume> = candidates
        .into_iter()
        .filter(|(_, device, _)| devices.insert(*device))
        .filter_map(|(index, _, mount)| {
            let key = key(mount)?;
            keys.insert(key.clone()).then(|| Volume {
                index,
                key,
                label: mount.to_string_lossy().into_owned(),
            })
        })
        .collect();
    picked.sort_by(|a, b| a.label.cmp(&b.label));
    picked
}

/// "/" is "root", "/mnt/My Data" is "mnt_my_data".
fn key(mount: &Path) -> Option<String> {
    if mount == Path::new(ROOT_MOUNT) {
        return Some(ROOT_KEY.into());
    }
    let key: String = mount
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(MAX_KEY_CHARS)
        .collect();
    let key = key.trim_matches('_');
    (!key.is_empty()).then(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::{volumes, Volume};
    use std::path::PathBuf;

    fn mounts(list: &[(&str, &str)]) -> Vec<(String, PathBuf)> {
        list.iter()
            .map(|(device, mount)| (device.to_string(), PathBuf::from(mount)))
            .collect()
    }

    fn named(list: &[(&str, &str)]) -> Vec<(usize, String, String)> {
        volumes(&mounts(list))
            .into_iter()
            .map(|Volume { index, key, label }| (index, key, label))
            .collect()
    }

    fn volume(index: usize, key: &str, label: &str) -> (usize, String, String) {
        (index, key.into(), label.into())
    }

    #[test]
    fn drives_are_named_by_their_mount_point() {
        assert_eq!(
            named(&[
                ("/dev/sdb1", "/mnt/My Data"),
                ("/dev/nvme0n1p2", "/"),
                ("/dev/nvme0n1p3", "/home"),
            ]),
            [
                volume(1, "root", "/"),
                volume(2, "home", "/home"),
                volume(0, "mnt_my_data", "/mnt/My Data"),
            ]
        );
    }

    #[test]
    fn a_device_mounted_twice_is_one_drive() {
        assert_eq!(
            named(&[
                ("/dev/sda2", "/var/lib/docker/btrfs"),
                ("/dev/sda2", "/home"),
                ("/dev/sda2", "/"),
                ("/dev/sdc1", "/srv/backup/snapshots"),
                ("/dev/sdc1", "/srv/backup"),
            ]),
            [
                volume(2, "root", "/"),
                volume(4, "srv_backup", "/srv/backup")
            ]
        );
    }

    #[test]
    fn the_root_counts_whatever_it_is_mounted_from() {
        assert_eq!(
            named(&[
                ("overlay", "/"),
                ("rpool/data", "/data"),
                ("C:\\", "/mnt/c"),
                ("overlay", "/var/lib/docker/overlay2/abc/merged"),
            ]),
            [volume(0, "root", "/")]
        );
    }

    #[test]
    fn boot_partitions_and_user_media_are_left_out() {
        assert_eq!(
            named(&[
                ("/dev/sda1", "/boot/efi"),
                ("/dev/sda2", "/boot"),
                ("/dev/sda3", "/efi"),
                ("/dev/sdd1", "/media/someone/Holiday photos"),
                ("/dev/sde1", "/run/media/someone/stick"),
                ("/dev/sda4", "/bootstrap"),
            ]),
            [volume(5, "bootstrap", "/bootstrap")]
        );
        assert!(named(&[]).is_empty());
    }

    #[test]
    fn keys_stay_distinct_and_short() {
        let long = format!("/srv/{}", "x".repeat(80));
        let listed = named(&[
            ("/dev/sda1", "/mnt/a-b"),
            ("/dev/sdb1", "/mnt/a_b"),
            ("/dev/sdc1", long.as_str()),
            ("/dev/sdd1", "/---"),
        ]);
        let keys: Vec<&str> = listed.iter().map(|(_, key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            ["mnt_a_b", format!("srv_{}", "x".repeat(35)).as_str()]
        );
        assert_eq!(listed[0].2, "/mnt/a-b", "the first of two equal keys wins");
    }
}
