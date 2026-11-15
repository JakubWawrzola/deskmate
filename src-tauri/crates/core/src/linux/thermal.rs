//! CPU temperature from the kernel's hardware monitors and thermal zones.

use super::{entries, file_name, hottest, hwmon_temperatures, plausible_celsius};
use super::{read_number, read_text};
use std::path::Path;

/// The hardware monitor drivers that read the processor's own sensor: Intel,
/// AMD, and the out-of-tree AMD driver.
const CPU_DRIVERS: [&str; 3] = ["coretemp", "k10temp", "zenpower"];
/// The thermal zone of the mainboard, the last resort, as on Windows.
const ACPI_ZONE: &str = "acpitz";

/// The CPU temperature in degrees Celsius: the processor's own sensor, else a
/// thermal zone of the processor, else the ACPI zone.
pub fn cpu_celsius(sys: &Path) -> Option<f64> {
    cpu_sensor(sys)
        .or_else(|| thermal_zone(sys, is_cpu_zone))
        .or_else(|| thermal_zone(sys, |kind| kind == ACPI_ZONE))
}

fn cpu_sensor(sys: &Path) -> Option<f64> {
    let readings: Vec<(Option<String>, f64)> = entries(&sys.join("class/hwmon"))
        .iter()
        .filter(|hwmon| {
            read_text(&hwmon.join("name")).is_some_and(|name| CPU_DRIVERS.contains(&name.as_str()))
        })
        .flat_map(|hwmon| hwmon_temperatures(hwmon))
        .collect();
    // The die and the package are the processor as a whole. AMD's Tctl can
    // carry an offset for the fan curve, so it only counts without a Tdie.
    let labelled = |wanted: fn(&str) -> bool| {
        hottest(
            readings
                .iter()
                .filter(|(label, _)| label.as_deref().is_some_and(wanted))
                .map(|(_, celsius)| *celsius),
        )
    };
    labelled(|label| label == "Tdie")
        .or_else(|| labelled(|label| label.starts_with("Package id")))
        .or_else(|| hottest(readings.iter().map(|(_, celsius)| *celsius)))
}

/// Boards without a processor driver name the zone after what it measures:
/// "cpu-thermal", "cpu0-thermal", "soc_thermal", Intel's "x86_pkg_temp".
fn is_cpu_zone(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind == "x86_pkg_temp" || kind.contains("cpu") || kind.contains("soc")
}

fn thermal_zone(sys: &Path, wanted: impl Fn(&str) -> bool) -> Option<f64> {
    hottest(
        entries(&sys.join("class/thermal"))
            .iter()
            .filter(|zone| file_name(zone).starts_with("thermal_zone"))
            .filter(|zone| read_text(&zone.join("type")).is_some_and(|kind| wanted(&kind)))
            .filter_map(|zone| read_number::<f64>(&zone.join("temp")))
            .map(|millidegrees| millidegrees / 1000.0)
            .filter(|celsius| plausible_celsius(*celsius)),
    )
}

#[cfg(test)]
mod tests {
    use super::cpu_celsius;
    use crate::test_tree::Tree;

    #[test]
    fn intel_reports_the_package() {
        let tree = Tree::new();
        tree.file("class/hwmon/hwmon0/name", "acpitz\n")
            .file("class/hwmon/hwmon0/temp1_input", "27800\n")
            .file("class/hwmon/hwmon3/name", "coretemp\n")
            .file("class/hwmon/hwmon3/temp1_label", "Package id 0\n")
            .file("class/hwmon/hwmon3/temp1_input", "52000\n")
            .file("class/hwmon/hwmon3/temp2_label", "Core 0\n")
            .file("class/hwmon/hwmon3/temp2_input", "61000\n")
            .file("class/hwmon/hwmon3/temp2_max", "100000\n");
        assert_eq!(cpu_celsius(tree.path()), Some(52.0));
    }

    #[test]
    fn two_sockets_report_the_hotter_package() {
        let tree = Tree::new();
        for (hwmon, package, millidegrees) in [("hwmon1", 0, "48000"), ("hwmon2", 1, "55500")] {
            tree.file(&format!("class/hwmon/{hwmon}/name"), "coretemp")
                .file(
                    &format!("class/hwmon/{hwmon}/temp1_label"),
                    &format!("Package id {package}"),
                )
                .file(&format!("class/hwmon/{hwmon}/temp1_input"), millidegrees);
        }
        assert_eq!(cpu_celsius(tree.path()), Some(55.5));
    }

    #[test]
    fn amd_prefers_the_die_over_the_control_value() {
        let tree = Tree::new();
        tree.file("class/hwmon/hwmon2/name", "k10temp\n")
            .file("class/hwmon/hwmon2/temp1_label", "Tctl\n")
            .file("class/hwmon/hwmon2/temp1_input", "68250\n")
            .file("class/hwmon/hwmon2/temp2_label", "Tdie\n")
            .file("class/hwmon/hwmon2/temp2_input", "41250\n");
        assert_eq!(cpu_celsius(tree.path()), Some(41.25));

        let control_only = Tree::new();
        control_only
            .file("class/hwmon/hwmon2/name", "k10temp\n")
            .file("class/hwmon/hwmon2/temp1_label", "Tctl\n")
            .file("class/hwmon/hwmon2/temp1_input", "45125\n");
        assert_eq!(cpu_celsius(control_only.path()), Some(45.125));
    }

    #[test]
    fn an_arm_board_reports_its_cpu_zone() {
        let tree = Tree::new();
        tree.file("class/thermal/thermal_zone0/type", "cpu-thermal\n")
            .file("class/thermal/thermal_zone0/temp", "47774\n")
            .file("class/thermal/thermal_zone1/type", "gpu-thermal\n")
            .file("class/thermal/thermal_zone1/temp", "60000\n")
            .file("class/thermal/cooling_device0/type", "cpu-thermal\n")
            .file("class/thermal/cooling_device0/temp", "99000\n");
        assert_eq!(cpu_celsius(tree.path()), Some(47.774));
    }

    #[test]
    fn the_acpi_zone_is_the_last_resort() {
        let tree = Tree::new();
        tree.file("class/thermal/thermal_zone0/type", "acpitz\n")
            .file("class/thermal/thermal_zone0/temp", "38000\n")
            .file("class/thermal/thermal_zone1/type", "iwlwifi_1\n")
            .file("class/thermal/thermal_zone1/temp", "71000\n");
        assert_eq!(cpu_celsius(tree.path()), Some(38.0));

        tree.file("class/thermal/thermal_zone2/type", "x86_pkg_temp\n")
            .file("class/thermal/thermal_zone2/temp", "44000\n");
        assert_eq!(cpu_celsius(tree.path()), Some(44.0));
    }

    #[test]
    fn a_computer_without_a_sensor_reports_nothing() {
        let tree = Tree::new();
        assert_eq!(cpu_celsius(tree.path()), None);

        // A sensor that is not connected: zero, far too hot, or not a number.
        tree.file("class/hwmon/hwmon0/name", "coretemp")
            .file("class/hwmon/hwmon0/temp1_input", "0")
            .file("class/hwmon/hwmon0/temp2_input", "255000")
            .file("class/hwmon/hwmon0/temp3_input", "n/a")
            .file("class/thermal/thermal_zone0/type", "acpitz")
            .file("class/thermal/thermal_zone0/temp", "-273200");
        assert_eq!(cpu_celsius(tree.path()), None);
    }

    #[test]
    fn other_hardware_monitors_are_not_the_cpu() {
        let tree = Tree::new();
        tree.file("class/hwmon/hwmon0/name", "nvme")
            .file("class/hwmon/hwmon0/temp1_input", "39850")
            .file("class/hwmon/hwmon1/name", "amdgpu")
            .file("class/hwmon/hwmon1/temp1_input", "58000");
        assert_eq!(cpu_celsius(tree.path()), None);
    }
}
