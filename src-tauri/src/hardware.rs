//! Optional Windows hardware telemetry.
//! Missing PDH/DXGI/WMI data is represented by omitting the sensor definition.

use crate::sensors::OwnedSensorDef;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sysinfo::Disks;

pub struct HardwareCollector {
    disk_primed: bool,
    gpu: Option<pdh::GpuCounters>,
    gpu_total_mib: Option<f64>,
    temperatures: TemperatureCache,
}

impl HardwareCollector {
    pub fn new() -> Self {
        Self {
            disk_primed: false,
            gpu: pdh::GpuCounters::new(),
            gpu_total_mib: dxgi_total_vram_mib(),
            temperatures: TemperatureCache::default(),
        }
    }

    pub fn collect(
        &mut self,
        disks: &Disks,
        elapsed: f64,
    ) -> (HashMap<String, String>, Vec<OwnedSensorDef>) {
        let mut values = HashMap::new();
        let mut defs = Vec::new();

        let mut read_bytes = 0u64;
        let mut written_bytes = 0u64;
        for disk in disks.list() {
            let Some(letter) = drive_letter(disk.mount_point()) else {
                continue;
            };
            let prefix = format!("disk_{}", letter.to_ascii_lowercase());
            let total = disk.total_space();
            if total > 0 {
                let free_id = format!("{prefix}_free");
                defs.push(sensor(
                    &free_id,
                    &format!("Disk {letter} free"),
                    Some("GiB"),
                    "mdi:harddisk",
                ));
                values.insert(
                    free_id,
                    format!("{:.2}", disk.available_space() as f64 / 1_073_741_824.0),
                );

                let usage_id = format!("{prefix}_usage");
                defs.push(sensor(
                    &usage_id,
                    &format!("Disk {letter} usage"),
                    Some("%"),
                    "mdi:harddisk",
                ));
                let used = total.saturating_sub(disk.available_space());
                values.insert(
                    usage_id,
                    format!("{:.1}", used as f64 / total as f64 * 100.0),
                );
            }
            let usage = disk.usage();
            read_bytes = read_bytes.saturating_add(usage.read_bytes);
            written_bytes = written_bytes.saturating_add(usage.written_bytes);
        }
        if !disks.list().is_empty() {
            defs.push(sensor(
                "disk_read",
                "Disk read",
                Some("MB/s"),
                "mdi:harddisk-plus",
            ));
            defs.push(sensor(
                "disk_write",
                "Disk write",
                Some("MB/s"),
                "mdi:harddisk-plus",
            ));
            if self.disk_primed {
                values.insert(
                    "disk_read".into(),
                    format!("{:.2}", read_bytes as f64 / elapsed / 1_048_576.0),
                );
                values.insert(
                    "disk_write".into(),
                    format!("{:.2}", written_bytes as f64 / elapsed / 1_048_576.0),
                );
            } else {
                values.insert("disk_read".into(), "0.00".into());
                values.insert("disk_write".into(), "0.00".into());
                self.disk_primed = true;
            }
        }

        if let Some(gpu) = &mut self.gpu {
            if let Some(sample) = gpu.sample() {
                if let Some(usage) = sample.usage_percent {
                    defs.push(sensor(
                        "gpu_usage",
                        "GPU usage",
                        Some("%"),
                        "mdi:expansion-card",
                    ));
                    values.insert("gpu_usage".into(), format!("{usage:.1}"));
                }
                if let Some(used_mib) = sample.dedicated_mib {
                    defs.push(sensor(
                        "gpu_memory",
                        "GPU memory",
                        Some("MiB"),
                        "mdi:memory",
                    ));
                    values.insert("gpu_memory".into(), format!("{used_mib:.0}"));
                }
            }
        }
        if let Some(total_mib) = self.gpu_total_mib {
            defs.push(sensor(
                "gpu_memory_total",
                "GPU memory total",
                Some("MiB"),
                "mdi:memory",
            ));
            values.insert("gpu_memory_total".into(), format!("{total_mib:.0}"));
        }

        let temps = self.temperatures.read();
        if let Some(value) = temps.cpu {
            defs.push(sensor(
                "cpu_temperature",
                "CPU temperature",
                Some("°C"),
                "mdi:thermometer",
            ));
            values.insert("cpu_temperature".into(), format!("{value:.1}"));
        }
        if let Some(value) = temps.gpu {
            defs.push(sensor(
                "gpu_temperature",
                "GPU temperature",
                Some("°C"),
                "mdi:thermometer",
            ));
            values.insert("gpu_temperature".into(), format!("{value:.1}"));
        }

        defs.sort_by(|a, b| a.id.cmp(&b.id));
        (values, defs)
    }
}

fn sensor(id: &str, name: &str, unit: Option<&str>, icon: &str) -> OwnedSensorDef {
    OwnedSensorDef {
        id: id.into(),
        name: name.into(),
        component: "sensor".into(),
        unit: unit.map(str::to_string),
        device_class: None,
        icon: Some(icon.into()),
        privacy: false,
        default_enabled: true,
    }
}

fn drive_letter(path: &std::path::Path) -> Option<char> {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let letter = chars.next()?;
    (letter.is_ascii_alphabetic() && chars.next() == Some(':')).then(|| letter.to_ascii_uppercase())
}

#[derive(Clone, Copy, Default)]
struct Temperatures {
    cpu: Option<f64>,
    gpu: Option<f64>,
}

#[derive(Default)]
struct TemperatureCache {
    last_read: Option<Instant>,
    values: Temperatures,
}

impl TemperatureCache {
    fn read(&mut self) -> Temperatures {
        if self
            .last_read
            .map(|at| at.elapsed() >= Duration::from_secs(60))
            .unwrap_or(true)
        {
            self.values = wmi_temperatures();
            self.last_read = Some(Instant::now());
        }
        self.values
    }
}

#[cfg(windows)]
fn dxgi_total_vram_mib() -> Option<f64> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut index = 0;
        let mut largest = 0usize;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            if let Ok(desc) = adapter.GetDesc1() {
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                    largest = largest.max(desc.DedicatedVideoMemory);
                }
            }
            index += 1;
        }
        (largest > 0).then(|| largest as f64 / 1_048_576.0)
    }
}

#[cfg(not(windows))]
fn dxgi_total_vram_mib() -> Option<f64> {
    None
}

#[cfg(windows)]
fn wmi_temperatures() -> Temperatures {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::System::Wmi::{ISWbemLocator, SWbemLocator};

    unsafe fn query(namespace: &str, sql: &str, names: &[&str]) -> Vec<Vec<VARIANT>> {
        let locator: ISWbemLocator =
            match CoCreateInstance(&SWbemLocator, None, CLSCTX_INPROC_SERVER) {
                Ok(value) => value,
                Err(_) => return Vec::new(),
            };
        let empty = BSTR::new();
        let services = match locator.ConnectServer(
            &empty,
            &BSTR::from(namespace),
            &empty,
            &empty,
            &empty,
            &empty,
            0,
            None,
        ) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        let set = match services.ExecQuery(&BSTR::from(sql), &BSTR::from("WQL"), 0, None) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        let count = set.Count().unwrap_or(0).clamp(0, 256);
        let mut rows = Vec::new();
        for index in 0..count {
            let Ok(object) = set.ItemIndex(index) else {
                continue;
            };
            let Ok(properties) = object.Properties_() else {
                continue;
            };
            let row = names
                .iter()
                .map(|name| {
                    properties
                        .Item(&BSTR::from(*name), 0)
                        .and_then(|property| property.Value())
                        .unwrap_or_default()
                })
                .collect();
            rows.push(row);
        }
        rows
    }

    fn number(value: &VARIANT) -> Option<f64> {
        f64::try_from(value)
            .ok()
            .or_else(|| u64::try_from(value).ok().map(|v| v as f64))
            .or_else(|| i64::try_from(value).ok().map(|v| v as f64))
            .or_else(|| u32::try_from(value).ok().map(|v| v as f64))
            .or_else(|| i32::try_from(value).ok().map(|v| v as f64))
    }

    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let uninitialize = hr.is_ok();
    let mut result = Temperatures::default();
    for namespace in ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"] {
        let rows = unsafe {
            query(
                namespace,
                "SELECT Identifier, Name, Value FROM Sensor WHERE SensorType='Temperature'",
                &["Identifier", "Name", "Value"],
            )
        };
        for row in rows {
            let identifier = BSTR::try_from(&row[0])
                .map(|v| v.to_string())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let name = BSTR::try_from(&row[1])
                .map(|v| v.to_string())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let Some(value) = number(&row[2]).filter(|v| (0.0..=150.0).contains(v)) else {
                continue;
            };
            let label = format!("{identifier} {name}");
            if label.contains("/gpu") || label.contains("gpu") {
                result.gpu = Some(result.gpu.map(|old| old.max(value)).unwrap_or(value));
            } else if label.contains("/cpu") || label.contains("cpu") || label.contains("package") {
                result.cpu = Some(result.cpu.map(|old| old.max(value)).unwrap_or(value));
            }
        }
        if result.cpu.is_some() || result.gpu.is_some() {
            break;
        }
    }
    if result.cpu.is_none() {
        let rows = unsafe {
            query(
                "ROOT\\WMI",
                "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
                &["CurrentTemperature"],
            )
        };
        for row in rows {
            if let Some(celsius) = number(&row[0])
                .map(|v| v / 10.0 - 273.15)
                .filter(|v| (0.0..=150.0).contains(v))
            {
                result.cpu = Some(result.cpu.map(|old| old.max(celsius)).unwrap_or(celsius));
            }
        }
    }
    if uninitialize {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(windows))]
fn wmi_temperatures() -> Temperatures {
    Temperatures::default()
}

#[cfg(windows)]
mod pdh {
    use windows::core::PCWSTR;
    use windows::Win32::System::Performance::*;

    pub struct GpuSample {
        pub usage_percent: Option<f64>,
        pub dedicated_mib: Option<f64>,
    }

    pub struct GpuCounters {
        query: PDH_HQUERY,
        usage: Option<PDH_HCOUNTER>,
        memory: Option<PDH_HCOUNTER>,
    }

    // PDH query handles are used sequentially by the single sensor collector.
    unsafe impl Send for GpuCounters {}

    impl GpuCounters {
        pub fn new() -> Option<Self> {
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                let usage = add(query, r"\GPU Engine(*)\Utilization Percentage");
                let memory = add(query, r"\GPU Adapter Memory(*)\Dedicated Usage");
                if usage.is_none() && memory.is_none() {
                    let _ = PdhCloseQuery(query);
                    return None;
                }
                let _ = PdhCollectQueryData(query);
                Some(Self {
                    query,
                    usage,
                    memory,
                })
            }
        }

        pub fn sample(&mut self) -> Option<GpuSample> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let usage_percent = self
                    .usage
                    .and_then(|counter| array_values(counter))
                    .into_iter()
                    .flatten()
                    .filter(|value| value.is_finite() && *value >= 0.0)
                    .fold(None, |best: Option<f64>, value| {
                        Some(best.map(|old| old.max(value)).unwrap_or(value))
                    })
                    .map(|value| value.min(100.0));
                let dedicated_mib = self
                    .memory
                    .and_then(|counter| array_values(counter))
                    .map(|values| {
                        values
                            .into_iter()
                            .filter(|value| value.is_finite() && *value >= 0.0)
                            .sum::<f64>()
                            / 1_048_576.0
                    })
                    .filter(|value| *value >= 0.0);
                (usage_percent.is_some() || dedicated_mib.is_some()).then_some(GpuSample {
                    usage_percent,
                    dedicated_mib,
                })
            }
        }
    }

    impl Drop for GpuCounters {
        fn drop(&mut self) {
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }

    unsafe fn add(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut counter = PDH_HCOUNTER::default();
        (PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) == 0)
            .then_some(counter)
    }

    unsafe fn array_values(counter: PDH_HCOUNTER) -> Option<Vec<f64>> {
        let mut bytes = 0u32;
        let mut count = 0u32;
        let status =
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, None);
        if status != PDH_MORE_DATA || bytes == 0 {
            return None;
        }
        let words =
            (bytes as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        let mut storage = vec![0usize; words];
        let ptr = storage.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(ptr))
            != 0
        {
            return None;
        }
        let items = std::slice::from_raw_parts(ptr, count as usize);
        Some(
            items
                .iter()
                .filter_map(|item| {
                    matches!(
                        item.FmtValue.CStatus,
                        PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
                    )
                    .then(|| item.FmtValue.Anonymous.doubleValue)
                })
                .collect(),
        )
    }
}

#[cfg(not(windows))]
mod pdh {
    pub struct GpuCounters;
    impl GpuCounters {
        pub fn new() -> Option<Self> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_drive_letter_mounts() {
        assert_eq!(drive_letter(std::path::Path::new(r"C:\")), Some('C'));
        assert_eq!(drive_letter(std::path::Path::new(r"d:\data")), Some('D'));
        assert_eq!(drive_letter(std::path::Path::new(r"\\server\share")), None);
    }
}
