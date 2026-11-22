//! Optional hardware telemetry: drives, disk activity, GPU, temperatures,
//! and the load readings of load.rs.
//! Windows reads PDH, DXGI and WMI, Linux reads sysfs (linux.rs). A reading
//! the computer does not offer is represented by omitting the sensor
//! definition.

#[cfg(target_os = "linux")]
use crate::linux::gpu::GpuCounters;
use crate::sensors::OwnedSensorDef;
#[cfg(windows)]
use pdh::GpuCounters;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sysinfo::{Disk, System};

/// How many collection ticks a detected sensor survives without a reading
/// before its definition is withdrawn (about 2.5 minutes at the default 15 s).
const MISSED_TICKS_BEFORE_DROP: u32 = 10;

pub struct HardwareCollector {
    disk_primed: bool,
    gpu: Option<GpuCounters>,
    gpu_total_mib: Option<f64>,
    temperatures: TemperatureCache,
    load: crate::load::LoadCounters,
    /// Every definition seen so far, with the number of ticks it has been
    /// missing. A single failed PDH or WMI read used to drop the definition,
    /// which deleted the entity in Home Assistant and recreated it on the next
    /// tick. On desktops with a discrete GPU and LibreHardwareMonitor that
    /// happened constantly.
    known: HashMap<String, (OwnedSensorDef, u32)>,
}

impl HardwareCollector {
    #[allow(
        clippy::new_without_default,
        reason = "new() opens the GPU counters and looks for the graphics cards; Default would hide that work"
    )]
    pub fn new() -> Self {
        Self {
            disk_primed: false,
            gpu: GpuCounters::new(),
            gpu_total_mib: total_vram_mib(),
            temperatures: TemperatureCache::default(),
            load: crate::load::LoadCounters::new(),
            known: HashMap::new(),
        }
    }

    /// `sys` is the collector's sysinfo handle, for the readings that come
    /// from it; none when it could not be opened.
    pub fn collect(
        &mut self,
        disks: &[Disk],
        sys: Option<&mut System>,
        elapsed: f64,
    ) -> (HashMap<String, String>, Vec<OwnedSensorDef>) {
        let mut values = HashMap::new();
        let mut defs = Vec::new();

        let mut read_bytes = 0u64;
        let mut written_bytes = 0u64;
        for Volume { disk, key, label } in volumes(disks) {
            let prefix = format!("disk_{key}");
            let total = disk.total_space();
            if total > 0 {
                let free_id = format!("{prefix}_free");
                defs.push(sensor(
                    &free_id,
                    &format!("Disk {label} free"),
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
                    &format!("Disk {label} usage"),
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
        if !disks.is_empty() {
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

        for reading in self.load.collect(sys, elapsed) {
            defs.push(sensor(
                reading.id,
                reading.name,
                Some(reading.unit),
                reading.icon,
            ));
            values.insert(reading.id.into(), reading.value);
        }

        let defs = self.stabilize(defs);
        (values, defs)
    }

    /// Keeps a definition alive through short gaps in its readings. The value
    /// is simply not published for those ticks.
    fn stabilize(&mut self, current: Vec<OwnedSensorDef>) -> Vec<OwnedSensorDef> {
        let present: std::collections::HashSet<String> =
            current.iter().map(|def| def.id.clone()).collect();
        for def in current {
            self.known.insert(def.id.clone(), (def, 0));
        }
        self.known.retain(|id, (_, missed)| {
            if present.contains(id) {
                return true;
            }
            *missed += 1;
            *missed < MISSED_TICKS_BEFORE_DROP
        });
        let mut defs: Vec<OwnedSensorDef> =
            self.known.values().map(|(def, _)| def.clone()).collect();
        defs.sort_by(|a, b| a.id.cmp(&b.id));
        defs
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
        hardware: true,
    }
}

/// A drive with sensors of its own: `disk_<key>_free`, "Disk <label> free".
struct Volume<'a> {
    disk: &'a Disk,
    key: String,
    label: String,
}

/// On Windows a drive is a volume with a letter: `disk_c_free`, "Disk C free".
#[cfg(windows)]
fn volumes(disks: &[Disk]) -> Vec<Volume<'_>> {
    disks
        .iter()
        .filter_map(|disk| {
            let letter = drive_letter(disk.mount_point())?;
            Some(Volume {
                disk,
                key: letter.to_ascii_lowercase().to_string(),
                label: letter.to_string(),
            })
        })
        .collect()
}

/// On Linux a drive is named by its mount point: `disk_root_free`,
/// "Disk / free" (linux/disks.rs).
#[cfg(target_os = "linux")]
fn volumes(list: &[Disk]) -> Vec<Volume<'_>> {
    use crate::linux::disks::{top_mounts, Mount, MOUNTINFO};

    let tops = match std::fs::read_to_string(MOUNTINFO) {
        Ok(text) => top_mounts(&text),
        Err(error) => {
            log::debug!("cannot read {MOUNTINFO}, going by statvfs: {error}");
            HashMap::new()
        }
    };
    let mounts: Vec<Mount> = list
        .iter()
        .map(|disk| {
            let point = disk.mount_point().to_path_buf();
            let top = tops.get(&point);
            Mount::new(
                disk.name().to_string_lossy().into_owned(),
                point,
                disk.is_read_only(),
                top,
            )
        })
        .collect();
    crate::linux::disks::volumes(&mounts)
        .into_iter()
        .map(|volume| Volume {
            disk: &list[volume.index],
            key: volume.key,
            label: volume.label,
        })
        .collect()
}

#[cfg(windows)]
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
            self.values = read_temperatures();
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

#[cfg(windows)]
fn total_vram_mib() -> Option<f64> {
    dxgi_total_vram_mib()
}

#[cfg(target_os = "linux")]
fn total_vram_mib() -> Option<f64> {
    crate::linux::gpu::read(std::path::Path::new(crate::linux::SYS)).memory_total_mib
}

#[cfg(windows)]
fn read_temperatures() -> Temperatures {
    wmi_temperatures()
}

#[cfg(target_os = "linux")]
fn read_temperatures() -> Temperatures {
    let sys = std::path::Path::new(crate::linux::SYS);
    Temperatures {
        cpu: crate::linux::thermal::cpu_celsius(sys),
        gpu: crate::linux::gpu::read(sys).temperature_celsius,
    }
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
    // The PawnIO helper, when installed, is the most direct CPU source.
    let mut result = Temperatures {
        cpu: crate::cpu_temperature::read_celsius(),
        ..Temperatures::default()
    };
    let cpu_from_helper = result.cpu.is_some();
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
            } else if (label.contains("/cpu") || label.contains("cpu") || label.contains("package"))
                && !cpu_from_helper
            {
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
        // GPU Engine instances come and go with every process that touches the
        // GPU, so the array can grow between the size query and the read. PDH
        // then reports PDH_MORE_DATA again with the new size; retry with it.
        let mut storage: Vec<usize> = Vec::new();
        let mut filled = false;
        for _ in 0..4 {
            // slack for instances that appear between the two calls
            let wanted = bytes as usize + 4096;
            let words = wanted.div_ceil(std::mem::size_of::<usize>());
            storage = vec![0usize; words];
            bytes = (words * std::mem::size_of::<usize>()) as u32;
            let ptr = storage.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            match PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                Some(ptr),
            ) {
                0 => {
                    filled = true;
                    break;
                }
                status if status == PDH_MORE_DATA => continue,
                _ => return None,
            }
        }
        if !filled {
            return None;
        }
        let ptr = storage.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let items = std::slice::from_raw_parts(ptr, count as usize);
        Some(
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item.FmtValue.CStatus,
                        PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
                    )
                })
                .map(|item| item.FmtValue.Anonymous.doubleValue)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn accepts_only_drive_letter_mounts() {
        assert_eq!(drive_letter(std::path::Path::new(r"C:\")), Some('C'));
        assert_eq!(drive_letter(std::path::Path::new(r"d:\data")), Some('D'));
        assert_eq!(drive_letter(std::path::Path::new(r"\\server\share")), None);
    }

    /// Runs the real probes of the computer the test runs on: whatever it
    /// reports, a value never goes out without its definition.
    #[cfg(target_os = "linux")]
    #[test]
    fn every_collected_value_has_a_definition() {
        let disks = sysinfo::Disks::new_with_refreshed_list().unwrap();
        let mut collector = HardwareCollector::new();
        for _ in 0..2 {
            let mut sys = System::new().ok();
            let (values, defs) = collector.collect(&disks, sys.as_mut(), 1.0);
            for key in values.keys() {
                assert!(defs.iter().any(|def| &def.id == key), "{key}");
            }
        }
    }
}
