//! Graphics card readings for AMD and Intel, from the DRM devices in sysfs.
//! Each driver exposes its own subset of the files and only what is there is
//! reported. NVIDIA is not covered: its driver has none of them.

use super::{entries, file_name, hottest, hwmon_temperatures, read_number, read_text};
use std::path::{Path, PathBuf};

/// PCI vendor ids as sysfs prints them.
const VENDOR_AMD: &str = "0x1002";
const VENDOR_INTEL: &str = "0x8086";
const BYTES_PER_MIB: f64 = 1_048_576.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuReading {
    /// The busiest card, 0 to 100.
    pub usage_percent: Option<f64>,
    /// Video memory in use, all cards together.
    pub memory_used_mib: Option<f64>,
    /// Video memory of the card that has the most.
    pub memory_total_mib: Option<f64>,
    /// The hottest sensor of any card.
    pub temperature_celsius: Option<f64>,
}

/// With several cards the values combine the way the Windows collector
/// combines them.
pub fn read(sys: &Path) -> GpuReading {
    let cards = cards(sys);
    let attribute = |name: &str| -> Vec<f64> {
        cards
            .iter()
            .filter_map(|device| read_number::<f64>(&device.join(name)))
            .filter(|value| value.is_finite() && *value >= 0.0)
            .collect()
    };
    let used = attribute("mem_info_vram_used");
    GpuReading {
        usage_percent: hottest(attribute("gpu_busy_percent")).map(|busy| busy.min(100.0)),
        memory_used_mib: (!used.is_empty()).then(|| used.iter().sum::<f64>() / BYTES_PER_MIB),
        memory_total_mib: hottest(attribute("mem_info_vram_total"))
            .filter(|bytes| *bytes > 0.0)
            .map(|bytes| bytes / BYTES_PER_MIB),
        temperature_celsius: hottest(
            cards
                .iter()
                .flat_map(|device| entries(&device.join("hwmon")))
                .flat_map(|hwmon| hwmon_temperatures(&hwmon))
                .map(|(_, celsius)| celsius),
        ),
    }
}

/// The device folders of the AMD and Intel cards. `card0-HDMI-A-1` and its
/// kind are connectors, not cards.
fn cards(sys: &Path) -> Vec<PathBuf> {
    entries(&sys.join("class/drm"))
        .into_iter()
        .filter(|card| {
            file_name(card).strip_prefix("card").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map(|card| card.join("device"))
        .filter(|device| {
            read_text(&device.join("vendor"))
                .map(|vendor| vendor.to_ascii_lowercase())
                .is_some_and(|vendor| vendor == VENDOR_AMD || vendor == VENDOR_INTEL)
        })
        .collect()
}

/// One reading of the usage counters, shaped like the Windows one in
/// hardware.rs so that the collector treats both alike.
pub struct GpuSample {
    pub usage_percent: Option<f64>,
    pub dedicated_mib: Option<f64>,
}

/// The usage counters of this computer's cards.
pub struct GpuCounters {
    sys: PathBuf,
}

impl GpuCounters {
    /// None when no card reports usage or memory in use.
    pub fn new() -> Option<Self> {
        let counters = Self {
            sys: PathBuf::from(super::SYS),
        };
        counters.sample().map(|_| counters)
    }

    pub fn sample(&self) -> Option<GpuSample> {
        let reading = read(&self.sys);
        (reading.usage_percent.is_some() || reading.memory_used_mib.is_some()).then_some(
            GpuSample {
                usage_percent: reading.usage_percent,
                dedicated_mib: reading.memory_used_mib,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{read, GpuReading};
    use crate::test_tree::Tree;

    #[test]
    fn an_amd_card_reports_everything() {
        let tree = Tree::new();
        tree.file("class/drm/card1/device/vendor", "0x1002\n")
            .file("class/drm/card1/device/gpu_busy_percent", "37\n")
            .file("class/drm/card1/device/mem_info_vram_used", "1610612736\n")
            .file("class/drm/card1/device/mem_info_vram_total", "8589934592\n")
            .file("class/drm/card1/device/hwmon/hwmon4/name", "amdgpu\n")
            .file("class/drm/card1/device/hwmon/hwmon4/temp1_label", "edge\n")
            .file("class/drm/card1/device/hwmon/hwmon4/temp1_input", "54000\n")
            .file(
                "class/drm/card1/device/hwmon/hwmon4/temp2_label",
                "junction\n",
            )
            .file("class/drm/card1/device/hwmon/hwmon4/temp2_input", "61000\n")
            .file("class/drm/card1/device/hwmon/hwmon4/fan1_input", "1200\n")
            .file("class/drm/card1-DP-1/status", "connected\n");
        assert_eq!(
            read(tree.path()),
            GpuReading {
                usage_percent: Some(37.0),
                memory_used_mib: Some(1536.0),
                memory_total_mib: Some(8192.0),
                temperature_celsius: Some(61.0),
            }
        );
    }

    #[test]
    fn an_intel_card_reports_only_what_its_driver_offers() {
        let integrated = Tree::new();
        integrated
            .file("class/drm/card0/device/vendor", "0x8086\n")
            .file("class/drm/card0/device/class", "0x030000\n");
        assert_eq!(read(integrated.path()), GpuReading::default());

        let discrete = Tree::new();
        discrete
            .file("class/drm/card0/device/vendor", "0x8086\n")
            .file("class/drm/card0/device/hwmon/hwmon2/name", "xe\n")
            .file("class/drm/card0/device/hwmon/hwmon2/temp2_label", "pkg\n")
            .file("class/drm/card0/device/hwmon/hwmon2/temp2_input", "49000\n")
            .file(
                "class/drm/card0/device/hwmon/hwmon2/energy1_input",
                "123456\n",
            );
        assert_eq!(
            read(discrete.path()),
            GpuReading {
                temperature_celsius: Some(49.0),
                ..GpuReading::default()
            }
        );
    }

    #[test]
    fn two_cards_combine_like_on_windows() {
        let tree = Tree::new();
        tree.file("class/drm/card0/device/vendor", "0x8086")
            .file("class/drm/card0/device/hwmon/hwmon1/temp1_input", "40000")
            .file("class/drm/card1/device/vendor", "0x1002")
            .file("class/drm/card1/device/gpu_busy_percent", "12")
            .file("class/drm/card1/device/mem_info_vram_used", "1048576")
            .file("class/drm/card1/device/mem_info_vram_total", "4294967296")
            .file("class/drm/card1/device/hwmon/hwmon5/temp1_input", "66000")
            .file("class/drm/card2/device/vendor", "0x1002")
            .file("class/drm/card2/device/gpu_busy_percent", "80")
            .file("class/drm/card2/device/mem_info_vram_used", "3145728")
            .file("class/drm/card2/device/mem_info_vram_total", "2147483648");
        assert_eq!(
            read(tree.path()),
            GpuReading {
                usage_percent: Some(80.0),
                memory_used_mib: Some(4.0),
                memory_total_mib: Some(4096.0),
                temperature_celsius: Some(66.0),
            }
        );
    }

    #[test]
    fn other_vendors_and_broken_values_are_left_out() {
        let tree = Tree::new();
        // nouveau exposes a temperature for NVIDIA cards; they are out of scope.
        tree.file("class/drm/card0/device/vendor", "0x10de")
            .file("class/drm/card0/device/gpu_busy_percent", "55")
            .file("class/drm/card0/device/hwmon/hwmon1/temp1_input", "70000")
            .file("class/drm/card1/device/vendor", "0x1002")
            .file("class/drm/card1/device/gpu_busy_percent", "busy")
            .file("class/drm/card1/device/mem_info_vram_total", "0")
            .file("class/drm/card1/device/hwmon/hwmon2/temp1_input", "0")
            .file("class/drm/renderD128/device/vendor", "0x1002")
            .file("class/drm/renderD128/device/gpu_busy_percent", "99");
        assert_eq!(read(tree.path()), GpuReading::default());
        assert_eq!(read(&tree.path().join("missing")), GpuReading::default());
    }
}
