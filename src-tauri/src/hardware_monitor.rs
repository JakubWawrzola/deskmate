//! The sensors of LibreHardwareMonitor (or OpenHardwareMonitor) as sensors
//! of the user's own: power, clock speeds, temperatures, fans, of whatever
//! that program can read on this computer. Deskmate reads the list the
//! program publishes through WMI while it runs. Nothing is sent until the
//! user picks a sensor on the Sensors page.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "the monitoring program and its WMI classes exist on Windows only"
    )
)]

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest identifier accepted, such as "/amdcpu/0/temperature/2".
const MAX_IDENTIFIER_CHARS: usize = 120;
/// Longest name offered for a sensor.
const MAX_NAME_CHARS: usize = 60;
/// Most sensors listed. A workstation with every option has a few hundred.
const MAX_SENSORS: usize = 500;
/// The values are read once for all sensors of a publish interval, and not
/// again for this long.
const VALUES_MAX_AGE: Duration = Duration::from_secs(5);

/// One sensor the monitoring program offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MonitorSensor {
    /// The program's own path of the sensor; what a custom sensor stores.
    pub identifier: String,
    /// The hardware's name and the sensor's, for the list in the window.
    pub name: String,
    pub unit: String,
    pub value: f64,
}

/// A row of the program's `Sensor` class.
#[derive(Debug, Clone, Default)]
pub struct SensorRow {
    pub identifier: String,
    pub name: String,
    pub sensor_type: String,
    pub parent: String,
    pub value: Option<f64>,
}

/// "/amdcpu/0/power/0": a path of lowercase letters, digits, `-` and `_`.
/// The text comes from another program, and it ends up in the configuration
/// and in the window.
pub fn is_identifier(text: &str) -> bool {
    text.len() <= MAX_IDENTIFIER_CHARS
        && text.starts_with('/')
        && !text.ends_with('/')
        && !text.contains("//")
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'/' | b'-' | b'_')
        })
}

/// The unit of a sensor type of the program; empty for one without a unit.
pub fn unit_of(sensor_type: &str) -> &'static str {
    match sensor_type {
        "Temperature" => "°C",
        "Power" => "W",
        "Clock" => "MHz",
        "Fan" => "RPM",
        "Load" | "Control" | "Level" => "%",
        "Voltage" => "V",
        "Current" => "A",
        "Frequency" => "Hz",
        "Data" => "GB",
        "SmallData" => "MB",
        "Throughput" => "B/s",
        "Flow" => "L/h",
        "Energy" => "mWh",
        "Noise" => "dBA",
        _ => "",
    }
}

/// The sensors worth offering, out of the program's rows. `hardware` maps
/// the identifier of a piece of hardware to its name.
pub fn sensors_from(
    rows: Vec<SensorRow>,
    hardware: &HashMap<String, String>,
) -> Vec<MonitorSensor> {
    let mut sensors: Vec<MonitorSensor> = rows
        .into_iter()
        .filter(|row| is_identifier(&row.identifier))
        .filter_map(|row| {
            let value = row.value.filter(|value| value.is_finite())?;
            let part = hardware.get(&row.parent).map(String::as_str).unwrap_or("");
            let kind = row.sensor_type.to_lowercase();
            Some(MonitorSensor {
                name: clean_name(&format!("{part} {} {kind}", row.name)),
                unit: unit_of(&row.sensor_type).to_string(),
                identifier: row.identifier,
                value,
            })
        })
        .collect();
    sensors.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    sensors.dedup_by(|a, b| a.identifier == b.identifier);
    sensors.truncate(MAX_SENSORS);
    sensors
}

/// A name without control characters and repeated spaces, cut to length.
fn clean_name(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

/// Every sensor the monitoring program publishes right now. Empty when it
/// does not run. Blocking: a WMI query.
#[cfg(windows)]
pub fn list() -> Vec<MonitorSensor> {
    use deskmate_core::wmi::query;

    for namespace in ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"] {
        let rows: Vec<SensorRow> = query(
            namespace,
            "SELECT Identifier, Name, SensorType, Parent, Value FROM Sensor",
            &["Identifier", "Name", "SensorType", "Parent", "Value"],
        )
        .into_iter()
        .map(|row| {
            let text = |index: usize| row[index].text().unwrap_or_default().to_string();
            SensorRow {
                identifier: text(0),
                name: text(1),
                sensor_type: text(2),
                parent: text(3),
                value: row[4].number(),
            }
        })
        .collect();
        if rows.is_empty() {
            continue;
        }
        let hardware = query(
            namespace,
            "SELECT Identifier, Name FROM Hardware",
            &["Identifier", "Name"],
        )
        .into_iter()
        .filter_map(|row| Some((row[0].text()?.to_string(), row[1].text()?.to_string())))
        .collect();
        return sensors_from(rows, &hardware);
    }
    Vec::new()
}

#[cfg(not(windows))]
pub fn list() -> Vec<MonitorSensor> {
    Vec::new()
}

/// The current value of every sensor, by identifier, read at most once per
/// `VALUES_MAX_AGE`. Blocking.
pub fn values() -> HashMap<String, f64> {
    static CACHE: Mutex<Option<(Instant, HashMap<String, f64>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((read_at, values)) = cache.as_ref() {
        if read_at.elapsed() < VALUES_MAX_AGE {
            return values.clone();
        }
    }
    let values: HashMap<String, f64> = list()
        .into_iter()
        .map(|sensor| (sensor.identifier, sensor.value))
        .collect();
    *cache = Some((Instant::now(), values.clone()));
    values
}

/// A value the way Home Assistant gets it: one decimal place is what the
/// monitoring program itself shows.
pub fn formatted(value: f64) -> String {
    format!("{value:.1}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(identifier: &str, name: &str, sensor_type: &str, parent: &str, value: f64) -> SensorRow {
        SensorRow {
            identifier: identifier.into(),
            name: name.into(),
            sensor_type: sensor_type.into(),
            parent: parent.into(),
            value: Some(value),
        }
    }

    #[test]
    fn accepts_the_paths_the_program_uses() {
        for identifier in [
            "/amdcpu/0/power/0",
            "/gpu-nvidia/0/temperature/0",
            "/lpc/nct6798d/fan/1",
            "/nvme/0/level/0",
            "/intelcpu/0/clock/12",
            "/battery/x_1/voltage/0",
        ] {
            assert!(is_identifier(identifier), "{identifier}");
        }
    }

    #[test]
    fn refuses_what_is_not_such_a_path() {
        let long = format!("/{}", "a".repeat(MAX_IDENTIFIER_CHARS));
        for identifier in [
            "",
            "amdcpu/0/power/0",
            "/amdcpu/0/power/0/",
            "/amdcpu//power/0",
            "/AmdCpu/0/power/0",
            "/amd cpu/0",
            "/amdcpu/0/power/0'; DROP",
            "/amdcpu/../0",
            "/amdcpu/0\n",
            long.as_str(),
        ] {
            assert!(!is_identifier(identifier), "{identifier:?}");
        }
    }

    #[test]
    fn names_a_sensor_after_its_hardware_and_gives_it_its_unit() {
        let hardware = HashMap::from([("/amdcpu/0".to_string(), "AMD Ryzen 7 5800X".to_string())]);
        let sensors = sensors_from(
            vec![
                row("/amdcpu/0/power/0", "Package", "Power", "/amdcpu/0", 41.26),
                row("/amdcpu/0/clock/1", "Core #1", "Clock", "/amdcpu/0", 4650.0),
                row("/lpc/x/fan/0", "Fan #1", "Fan", "/lpc/x", 912.0),
            ],
            &hardware,
        );
        assert_eq!(sensors.len(), 3);
        // Sorted by identifier.
        assert_eq!(sensors[0].identifier, "/amdcpu/0/clock/1");
        assert_eq!(sensors[0].name, "AMD Ryzen 7 5800X Core #1 clock");
        assert_eq!(sensors[0].unit, "MHz");
        assert_eq!(sensors[1].name, "AMD Ryzen 7 5800X Package power");
        assert_eq!(sensors[1].unit, "W");
        assert_eq!(sensors[1].value, 41.26);
        // Hardware the program did not name.
        assert_eq!(sensors[2].name, "Fan #1 fan");
        assert_eq!(sensors[2].unit, "RPM");
    }

    #[test]
    fn leaves_out_rows_it_cannot_use() {
        let mut no_value = row("/amdcpu/0/power/1", "Cores", "Power", "/amdcpu/0", 0.0);
        no_value.value = None;
        let sensors = sensors_from(
            vec![
                row("/Bad Path", "X", "Power", "", 1.0),
                no_value,
                row("/amdcpu/0/power/2", "Y", "Power", "", f64::NAN),
                row("/amdcpu/0/power/0", "Package", "Power", "", 5.0),
                // The same sensor twice.
                row("/amdcpu/0/power/0", "Package", "Power", "", 6.0),
            ],
            &HashMap::new(),
        );
        assert_eq!(sensors.len(), 1);
        assert_eq!(sensors[0].identifier, "/amdcpu/0/power/0");
    }

    #[test]
    fn a_name_is_cleaned_and_cut() {
        assert_eq!(
            clean_name("  CPU \u{7} Package\tpower \n"),
            "CPU Package power"
        );
        assert_eq!(clean_name(&"x".repeat(200)).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_type_without_a_known_unit_has_none() {
        assert_eq!(unit_of("Temperature"), "°C");
        assert_eq!(unit_of("Factor"), "");
        assert_eq!(unit_of("temperature"), "");
        assert_eq!(formatted(41.26), "41.3");
    }
}
