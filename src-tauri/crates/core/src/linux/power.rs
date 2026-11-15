//! Whether this computer can hibernate. The Hibernate button is declared only
//! when it can.

use super::read_text;
use std::path::Path;
use std::process::{Command, Stdio};

/// Part of systemd, like the `systemctl` that runs the power commands.
const BUSCTL: &str = "busctl";
/// Asks logind. Fixed: nothing from the network or the settings is in it.
const CAN_HIBERNATE_CALL: [&str; 7] = [
    "--system",
    "--timeout=5",
    "call",
    "org.freedesktop.login1",
    "/org/freedesktop/login1",
    "org.freedesktop.login1.Manager",
    "CanHibernate",
];

/// The kernel has to be able to, and logind must agree. logind also knows
/// whether there is swap to hibernate to and whether this user may ask for
/// it. Where logind cannot be asked, the kernel's answer stands.
pub fn can_hibernate(sys: &Path) -> bool {
    kernel_can_hibernate(sys) && logind_can_hibernate().unwrap_or(true)
}

/// The kernel lists "disk" in /sys/power/state when it can suspend to disk.
pub fn kernel_can_hibernate(sys: &Path) -> bool {
    read_text(&sys.join("power/state"))
        .is_some_and(|states| states.split_whitespace().any(|state| state == "disk"))
}

fn logind_can_hibernate() -> Option<bool> {
    let output = Command::new(BUSCTL)
        .args(CAN_HIBERNATE_CALL)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_can_hibernate(&String::from_utf8_lossy(&output.stdout))
}

/// busctl prints the answer as `s "yes"`. Only "yes" means the button will
/// work: "challenge" asks for a password, and nobody is there to type it when
/// Home Assistant presses the button.
pub fn parse_can_hibernate(output: &str) -> Option<bool> {
    let answer = output.trim().strip_prefix("s \"")?.strip_suffix('"')?;
    match answer {
        "yes" => Some(true),
        "no" | "na" | "challenge" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{kernel_can_hibernate, parse_can_hibernate};
    use crate::test_tree::Tree;

    #[test]
    fn the_kernel_lists_disk_when_it_can_hibernate() {
        let tree = Tree::new();
        assert!(!kernel_can_hibernate(tree.path()), "no file: no");
        tree.file("power/state", "freeze mem disk\n");
        assert!(kernel_can_hibernate(tree.path()));
        tree.file("power/state", "freeze mem\n");
        assert!(!kernel_can_hibernate(tree.path()));
        tree.file("power/state", "\n");
        assert!(!kernel_can_hibernate(tree.path()));
        tree.file("power/state", "diskless\n");
        assert!(!kernel_can_hibernate(tree.path()));
    }

    #[test]
    fn only_a_plain_yes_from_logind_counts() {
        assert_eq!(parse_can_hibernate("s \"yes\"\n"), Some(true));
        assert_eq!(parse_can_hibernate("s \"no\"\n"), Some(false));
        assert_eq!(parse_can_hibernate("s \"na\"\n"), Some(false));
        assert_eq!(parse_can_hibernate("s \"challenge\"\n"), Some(false));
        assert_eq!(parse_can_hibernate(""), None);
        assert_eq!(parse_can_hibernate("s \"maybe\""), None);
        assert_eq!(parse_can_hibernate("b true"), None);
    }
}
