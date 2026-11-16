//! Which mounted file systems get their own sensors, and under which names.
//! Windows names a drive by its letter; here it is the mount point.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The mount point of the system drive.
pub const ROOT_MOUNT: &str = "/";
/// This process's view of the mount table, one line per mount, with the file
/// system's own options next to those of the mount.
pub const MOUNTINFO: &str = "/proc/self/mountinfo";
/// Ends the optional fields of a mountinfo line.
const MOUNTINFO_SEPARATOR: &str = "-";
/// What the system drive is called in a sensor key.
const ROOT_KEY: &str = "root";
/// Boot partitions hold nothing a user fills up. Media that a desktop mounts
/// for the signed-in user carry that user's name and the volume's label in
/// their path, which the machine has no business reporting.
const SKIPPED_ROOTS: [&str; 4] = ["/boot", "/efi", "/media", "/run/media"];
/// Keeps `disk_<key>_usage` well inside what Home Assistant accepts.
const MAX_KEY_CHARS: usize = 40;

/// One entry of the mount table.
pub struct Mount {
    /// What is mounted: `/dev/sda2`, or a name such as `overlay`.
    pub device: String,
    /// Where it is mounted.
    pub point: PathBuf,
    /// The file system itself is read-only, not only this view of it.
    pub read_only: bool,
    /// Another file system is mounted on top, so the numbers read at this
    /// mount point are that one's.
    pub hidden: bool,
}

/// The mount on top at one mount point, as `/proc/self/mountinfo` has it.
#[derive(Debug, PartialEq, Eq)]
pub struct TopMount {
    /// The source, as the mount table spells it.
    pub device: String,
    /// From the file system's own options, not the mount's.
    pub read_only: bool,
}

impl Mount {
    /// The mount at `point` from `device`, judged by `top`, the mount that
    /// `/proc/self/mountinfo` has on top there. The daemon's sandbox
    /// (`ProtectSystem=strict`) makes every mount read-only for it, and
    /// statvfs reports that, so whether the drive can fill up is read from
    /// the file system's options instead. `ProtectHome=yes` mounts an empty
    /// folder over `/home`, which hides the drive there. Without `top`,
    /// `statvfs_read_only` is all there is.
    pub fn new(
        device: String,
        point: PathBuf,
        statvfs_read_only: bool,
        top: Option<&TopMount>,
    ) -> Self {
        let (read_only, hidden) = match top {
            Some(top) => (top.read_only, top.device != device),
            None => (statvfs_read_only, false),
        };
        Self {
            device,
            point,
            read_only,
            hidden,
        }
    }
}

/// The mount on top at each mount point in `mountinfo`. A line lists the id,
/// the parent's id, the device number, the root, the mount point and its
/// options, optional fields up to a lone `-`, then the type, the source and
/// the file system's options. A later mount at the same point lies on top
/// of an earlier one.
pub fn top_mounts(mountinfo: &str) -> HashMap<PathBuf, TopMount> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let point = fields.get(4)?;
            let separator = fields
                .iter()
                .skip(6)
                .position(|field| *field == MOUNTINFO_SEPARATOR)?
                + 6;
            let device = fields.get(separator + 2)?;
            let options = fields.get(separator + 3)?;
            Some((
                PathBuf::from(unescape(point)),
                TopMount {
                    device: (*device).to_string(),
                    read_only: options.split(',').any(|option| option == "ro"),
                },
            ))
        })
        .collect()
}

/// The kernel writes a space, a tab, a line break and a backslash in a path
/// as three octal digits after a backslash: `\040`.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let octal = bytes.get(index + 1..index + 4).and_then(octal_byte);
        match (bytes[index], octal) {
            (b'\\', Some(byte)) => {
                out.push(byte);
                index += 4;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Three octal digits as the byte they stand for.
fn octal_byte(digits: &[u8]) -> Option<u8> {
    if !digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
        return None;
    }
    let value = digits
        .iter()
        .fold(0u32, |value, digit| value * 8 + u32::from(digit - b'0'));
    u8::try_from(value).ok()
}

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

/// Picks the volumes out of `mounts`, every listed file system. The root
/// always counts. Of the others, those on a block device that can be written
/// to do, once per device: a device mounted in several places (bind mounts,
/// btrfs subvolumes) is one drive, named by its shortest mount point. A
/// read-only file system is an image that never fills up, and a hidden one
/// cannot be measured.
pub fn volumes(mounts: &[Mount]) -> Vec<Volume> {
    let mut candidates: Vec<(usize, &str, &Path)> = mounts
        .iter()
        .enumerate()
        .filter(|(_, mount)| {
            mount.point == Path::new(ROOT_MOUNT)
                || (mount.device.starts_with("/dev/")
                    && !mount.read_only
                    && !mount.hidden
                    && !SKIPPED_ROOTS
                        .iter()
                        .any(|root| mount.point.starts_with(root)))
        })
        .map(|(index, mount)| (index, mount.device.as_str(), mount.point.as_path()))
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
    use super::{top_mounts, volumes, Mount, TopMount, Volume};
    use std::path::{Path, PathBuf};

    fn mount(device: &str, point: &str, read_only: bool) -> Mount {
        Mount {
            device: device.into(),
            point: PathBuf::from(point),
            read_only,
            hidden: false,
        }
    }

    /// What the daemon sees under `deskmate.service`: `ProtectSystem=strict`
    /// made every mount read-only but its own folders, `ProtectHome=yes` put
    /// an empty folder over `/home`, and `PrivateTmp=yes` bound `/var/tmp`.
    const SANDBOXED_MOUNTINFO: &str = "\
24 1 259:2 / / ro,relatime shared:1 - ext4 /dev/nvme0n1p2 rw,errors=remount-ro
25 24 259:3 / /home ro,nosuid,nodev,relatime shared:2 - ext4 /dev/nvme0n1p3 rw
26 24 8:17 / /mnt/data ro,relatime shared:3 - ext4 /dev/sdb1 rw
27 24 259:4 / /var ro,relatime shared:4 - ext4 /dev/nvme0n1p4 rw
28 24 8:33 / /mnt/My\\040Photos ro,relatime shared:6 - ext4 /dev/sdc1 rw
29 24 8:49 / /mnt/archive ro,relatime shared:7 - ext4 /dev/sdd1 ro
40 25 0:25 /systemd/inaccessible/dir /home ro,nosuid,nodev,noexec,relatime shared:20 - tmpfs tmpfs rw,mode=755
41 27 259:4 /tmp/systemd-private-1-deskmate.service-2/tmp /var/tmp rw,relatime shared:4 - ext4 /dev/nvme0n1p4 rw
42 27 259:4 /lib/deskmate /var/lib/deskmate rw,relatime shared:4 - ext4 /dev/nvme0n1p4 rw
";

    #[test]
    fn reads_the_mount_on_top_and_its_own_options() {
        let tops = top_mounts(SANDBOXED_MOUNTINFO);
        let top = |point: &str| tops.get(Path::new(point));
        assert_eq!(
            top("/"),
            Some(&TopMount {
                device: "/dev/nvme0n1p2".into(),
                read_only: false,
            })
        );
        assert_eq!(top("/home").map(|top| top.device.as_str()), Some("tmpfs"));
        assert_eq!(top("/mnt/archive").map(|top| top.read_only), Some(true));
        assert!(top("/mnt/My Photos").is_some(), "\\040 is a space");
        for broken in [
            "",
            "24 1 259:2 / /",
            "24 1 259:2 / / ro,relatime shared:1 ext4 /dev/sda1 rw",
            "24 1 259:2 / / ro - ext4",
        ] {
            assert!(top_mounts(broken).is_empty(), "{broken}");
        }
        let odd = top_mounts("1 0 8:1 / /a\\b\\0\\777 rw - ext4 /dev/sda1 rw\n");
        assert!(
            odd.contains_key(Path::new("/a\\b\\0\\777")),
            "{odd:?} keeps what is no escape"
        );
    }

    #[test]
    fn the_sandbox_neither_hides_drives_nor_adds_its_own_folders() {
        let tops = top_mounts(SANDBOXED_MOUNTINFO);
        // As sysinfo lists them: one entry per mount point, the first
        // device there, statvfs's flags, no tmpfs.
        let mounts: Vec<Mount> = [
            ("/dev/nvme0n1p2", "/", true),
            ("/dev/nvme0n1p3", "/home", true),
            ("/dev/sdb1", "/mnt/data", true),
            ("/dev/nvme0n1p4", "/var", true),
            ("/dev/sdc1", "/mnt/My Photos", true),
            ("/dev/sdd1", "/mnt/archive", true),
            ("/dev/nvme0n1p4", "/var/tmp", false),
            ("/dev/nvme0n1p4", "/var/lib/deskmate", false),
        ]
        .into_iter()
        .map(|(device, point, statvfs_read_only)| {
            let point = PathBuf::from(point);
            let top = tops.get(&point);
            Mount::new(device.into(), point, statvfs_read_only, top)
        })
        .collect();
        assert_eq!(
            listed(&mounts),
            [
                volume(0, "root", "/"),
                volume(4, "mnt_my_photos", "/mnt/My Photos"),
                volume(2, "mnt_data", "/mnt/data"),
                volume(3, "var", "/var"),
            ]
        );
    }

    #[test]
    fn without_the_mount_table_statvfs_decides() {
        let mount = Mount::new("/dev/sdb1".into(), PathBuf::from("/mnt/data"), true, None);
        assert!(mount.read_only && !mount.hidden);
    }

    fn listed(mounts: &[Mount]) -> Vec<(usize, String, String)> {
        volumes(mounts)
            .into_iter()
            .map(|Volume { index, key, label }| (index, key, label))
            .collect()
    }

    /// Writable mounts, as (device, mount point).
    fn named(list: &[(&str, &str)]) -> Vec<(usize, String, String)> {
        let mounts: Vec<Mount> = list
            .iter()
            .map(|(device, point)| mount(device, point, false))
            .collect();
        listed(&mounts)
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
    fn a_read_only_image_is_not_a_drive() {
        assert_eq!(
            listed(&[
                mount("/dev/sdb", "/usr/src/linux-headers", true),
                mount("/dev/sdc1", "/mnt/archive", true),
                mount("/dev/sdc1", "/mnt/archive-rw", false),
                mount("/dev/sda1", "/", true),
            ]),
            [
                volume(3, "root", "/"),
                volume(2, "mnt_archive_rw", "/mnt/archive-rw"),
            ]
        );
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
