//! Linux readings: what the kernel exposes in sysfs, and which mounted file
//! systems count as drives. Every reader takes the folder it reads as an
//! argument, so the tests run it on a sample tree; the callers pass `SYS`.
//! A reading the computer does not offer comes back as `None`.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub mod disks;
pub mod gpu;
pub mod power;
pub mod power_supply;
pub mod thermal;

/// Where the kernel exposes its devices.
pub const SYS: &str = "/sys";

/// The text of a sysfs attribute without its line break.
fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
}

fn read_number<T: FromStr>(path: &Path) -> Option<T> {
    read_text(path)?.parse().ok()
}

/// The entries of a folder sorted by name, so that a reading does not depend
/// on the order the kernel lists them in.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map(|listing| listing.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// The same bounds the Windows collector accepts. A sensor that is not
/// connected reports 0 or a value far outside them.
fn plausible_celsius(celsius: f64) -> bool {
    celsius > 0.0 && celsius <= 150.0
}

/// The `tempN_input` attributes of a hardware monitor, in degrees Celsius,
/// each with the text of its `tempN_label` when there is one.
fn hwmon_temperatures(hwmon: &Path) -> Vec<(Option<String>, f64)> {
    entries(hwmon)
        .iter()
        .filter_map(|path| {
            let channel = file_name(path)
                .strip_prefix("temp")?
                .strip_suffix("_input")?;
            let celsius = read_number::<f64>(path)? / 1000.0;
            let label = read_text(&hwmon.join(format!("temp{channel}_label")));
            plausible_celsius(celsius).then_some((label, celsius))
        })
        .collect()
}

fn hottest(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    values.into_iter().reduce(f64::max)
}
