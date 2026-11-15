//! Battery charge and external power, from the power supplies in sysfs.

use super::{entries, read_number, read_text};
use std::path::Path;

/// What the power supplies say about the computer itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerStatus {
    /// Charge of the computer's batteries together, 0 to 100. None without a
    /// battery.
    pub battery_percent: Option<u8>,
    /// Whether the computer runs on external power. None when neither a power
    /// adapter nor a battery is there to tell.
    pub plugged_in: Option<bool>,
}

struct Battery {
    percent: f64,
    /// Energy now and when full, in the same unit. Weighs batteries of
    /// different sizes against each other.
    energy: Option<(f64, f64)>,
    discharging: bool,
}

pub fn read(sys: &Path) -> PowerStatus {
    let mut batteries = Vec::new();
    let mut adapter_online: Option<bool> = None;
    for supply in entries(&sys.join("class/power_supply")) {
        // The battery of a mouse, a keyboard or a headset says nothing about
        // the computer.
        if read_text(&supply.join("scope")).as_deref() == Some("Device") {
            continue;
        }
        match read_text(&supply.join("type")).as_deref() {
            Some("Battery") => batteries.extend(battery(&supply)),
            Some("Mains" | "USB" | "Wireless") => {
                if let Some(online) = read_number::<u8>(&supply.join("online")) {
                    adapter_online = Some(adapter_online.unwrap_or(false) || online != 0);
                }
            }
            _ => {}
        }
    }
    PowerStatus {
        battery_percent: charge(&batteries),
        // A board without a power adapter device still knows whether its
        // battery is being drained.
        plugged_in: adapter_online.or_else(|| {
            (!batteries.is_empty()).then(|| !batteries.iter().any(|battery| battery.discharging))
        }),
    }
}

fn battery(supply: &Path) -> Option<Battery> {
    let number = |name: &str| read_number::<f64>(&supply.join(name)).filter(|value| *value >= 0.0);
    let energy = number("energy_now")
        .zip(number("energy_full"))
        .filter(|(_, full)| *full > 0.0);
    let percent = number("capacity").or(energy.map(|(now, full)| now / full * 100.0))?;
    Some(Battery {
        percent,
        energy,
        discharging: read_text(&supply.join("status")).as_deref() == Some("Discharging"),
    })
}

fn charge(batteries: &[Battery]) -> Option<u8> {
    let energies: Option<Vec<(f64, f64)>> =
        batteries.iter().map(|battery| battery.energy).collect();
    let percent = match (batteries, energies) {
        ([], _) => return None,
        ([only], _) => only.percent,
        (_, Some(energies)) => {
            let now: f64 = energies.iter().map(|(now, _)| now).sum();
            let full: f64 = energies.iter().map(|(_, full)| full).sum();
            now / full * 100.0
        }
        (several, None) => {
            several.iter().map(|battery| battery.percent).sum::<f64>() / several.len() as f64
        }
    };
    // Bounded first, so the cast cannot wrap.
    Some(percent.round().clamp(0.0, 100.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::{read, PowerStatus};
    use crate::test_tree::Tree;

    #[test]
    fn a_laptop_on_battery() {
        let tree = Tree::new();
        tree.file("class/power_supply/AC/type", "Mains\n")
            .file("class/power_supply/AC/online", "0\n")
            .file("class/power_supply/BAT0/type", "Battery\n")
            .file("class/power_supply/BAT0/capacity", "73\n")
            .file("class/power_supply/BAT0/status", "Discharging\n");
        assert_eq!(
            read(tree.path()),
            PowerStatus {
                battery_percent: Some(73),
                plugged_in: Some(false),
            }
        );
    }

    #[test]
    fn a_laptop_charging_over_usb_c() {
        let tree = Tree::new();
        tree.file("class/power_supply/ADP1/type", "Mains")
            .file("class/power_supply/ADP1/online", "0")
            .file("class/power_supply/ucsi-source-psy-USBC000:001/type", "USB")
            .file("class/power_supply/ucsi-source-psy-USBC000:001/online", "1")
            .file("class/power_supply/BAT1/type", "Battery")
            .file("class/power_supply/BAT1/capacity", "100")
            .file("class/power_supply/BAT1/status", "Full");
        assert_eq!(
            read(tree.path()),
            PowerStatus {
                battery_percent: Some(100),
                plugged_in: Some(true),
            }
        );
    }

    #[test]
    fn a_desktop_reports_neither() {
        let tree = Tree::new();
        assert_eq!(read(tree.path()), PowerStatus::default());

        // A wireless mouse has a battery, the computer still has none.
        tree.file("class/power_supply/hidpp_battery_0/type", "Battery")
            .file("class/power_supply/hidpp_battery_0/scope", "Device")
            .file("class/power_supply/hidpp_battery_0/capacity", "55")
            .file("class/power_supply/hidpp_battery_0/status", "Discharging");
        assert_eq!(read(tree.path()), PowerStatus::default());
    }

    #[test]
    fn a_power_adapter_without_a_battery_reports_only_the_plug() {
        let tree = Tree::new();
        tree.file("class/power_supply/ACAD/type", "Mains")
            .file("class/power_supply/ACAD/online", "1");
        assert_eq!(
            read(tree.path()),
            PowerStatus {
                battery_percent: None,
                plugged_in: Some(true),
            }
        );
    }

    #[test]
    fn two_batteries_count_by_their_size() {
        let tree = Tree::new();
        tree.file("class/power_supply/BAT0/type", "Battery")
            .file("class/power_supply/BAT0/scope", "System")
            .file("class/power_supply/BAT0/capacity", "100")
            .file("class/power_supply/BAT0/energy_now", "24000000")
            .file("class/power_supply/BAT0/energy_full", "24000000")
            .file("class/power_supply/BAT0/status", "Unknown")
            .file("class/power_supply/BAT1/type", "Battery")
            .file("class/power_supply/BAT1/capacity", "50")
            .file("class/power_supply/BAT1/energy_now", "36000000")
            .file("class/power_supply/BAT1/energy_full", "72000000")
            .file("class/power_supply/BAT1/status", "Discharging");
        // (24 + 36) of (24 + 72) Wh, not the plain mean of 100 and 50.
        assert_eq!(
            read(tree.path()),
            PowerStatus {
                battery_percent: Some(63),
                plugged_in: Some(false),
            }
        );
    }

    #[test]
    fn a_battery_alone_tells_whether_power_is_connected() {
        let tree = Tree::new();
        tree.file("class/power_supply/battery/type", "Battery")
            .file("class/power_supply/battery/energy_now", "9000000")
            .file("class/power_supply/battery/energy_full", "20000000")
            .file("class/power_supply/battery/status", "Charging");
        assert_eq!(
            read(tree.path()),
            PowerStatus {
                battery_percent: Some(45),
                plugged_in: Some(true),
            }
        );
    }

    #[test]
    fn a_battery_without_a_charge_value_is_not_reported() {
        let tree = Tree::new();
        tree.file("class/power_supply/BAT0/type", "Battery")
            .file("class/power_supply/BAT0/capacity", "unknown")
            .file("class/power_supply/BAT0/status", "Discharging")
            .file("class/power_supply/ups/type", "UPS")
            .file("class/power_supply/ups/online", "1");
        assert_eq!(read(tree.path()), PowerStatus::default());
    }
}
