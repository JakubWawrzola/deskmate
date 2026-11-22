//! Three readings of how busy the computer is that need no driver and no
//! administrator: the memory in use, the speed the processor runs at, and how
//! much of the time the drives were busy. Windows reads performance counters,
//! Linux reads /proc. A reading the computer does not give is left out.

use sysinfo::System;

const BYTES_PER_MIB: f64 = 1_048_576.0;

/// One of the readings, ready to become a hardware sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub id: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub icon: &'static str,
    pub value: String,
}

pub struct LoadCounters {
    #[cfg(windows)]
    counters: Option<win::Counters>,
    #[cfg(target_os = "linux")]
    disks: linux::BusyTime,
}

impl LoadCounters {
    #[allow(
        clippy::new_without_default,
        reason = "new() opens performance counters; Default would hide that work"
    )]
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            counters: win::Counters::open(),
            #[cfg(target_os = "linux")]
            disks: linux::BusyTime::primed(),
        }
    }

    /// The readings for the `elapsed_secs` since the last call. `sys` is the
    /// collector's sysinfo handle; without it only the counters are read.
    pub fn collect(&mut self, sys: Option<&mut System>, elapsed_secs: f64) -> Vec<Reading> {
        let mut readings = Vec::new();
        let mut nominal_mhz = None;
        if let Some(sys) = sys {
            sys.refresh_memory();
            readings.push(Reading {
                id: "memory_used",
                name: "Memory used",
                unit: "MiB",
                icon: "mdi:memory",
                value: format!("{:.0}", sys.used_memory() as f64 / BYTES_PER_MIB),
            });
            sys.refresh_cpu_frequency();
            nominal_mhz = mean_mhz(sys.cpus().iter().map(sysinfo::Cpu::frequency));
        }
        let (speed_mhz, busy_percent) = self.sample(nominal_mhz, elapsed_secs);
        if let Some(mhz) = speed_mhz {
            readings.push(Reading {
                id: "cpu_frequency",
                name: "CPU speed",
                unit: "MHz",
                icon: "mdi:speedometer",
                value: format!("{mhz:.0}"),
            });
        }
        if let Some(percent) = busy_percent {
            readings.push(Reading {
                id: "disk_activity",
                name: "Disk activity",
                unit: "%",
                icon: "mdi:harddisk",
                value: format!("{percent:.1}"),
            });
        }
        readings
    }

    /// The processor's speed in MHz and the drives' busy time in percent.
    #[cfg(windows)]
    fn sample(
        &mut self,
        nominal_mhz: Option<f64>,
        _elapsed_secs: f64,
    ) -> (Option<f64>, Option<f64>) {
        let Some(sample) = self.counters.as_mut().and_then(win::Counters::sample) else {
            return (None, None);
        };
        (
            effective_mhz(nominal_mhz, sample.performance_percent),
            sample.idle_percent.and_then(busy_from_idle),
        )
    }

    /// On Linux sysinfo already reports the speed each processor runs at.
    #[cfg(target_os = "linux")]
    fn sample(
        &mut self,
        nominal_mhz: Option<f64>,
        elapsed_secs: f64,
    ) -> (Option<f64>, Option<f64>) {
        let stats = std::fs::read_to_string(linux::DISKSTATS).unwrap_or_default();
        (nominal_mhz, self.disks.busy_percent(&stats, elapsed_secs))
    }
}

/// The mean of the speeds sysinfo reports, in MHz; none when it reports
/// nothing.
fn mean_mhz(speeds: impl Iterator<Item = u64>) -> Option<f64> {
    let (sum, count) = speeds
        .filter(|mhz| *mhz > 0)
        .fold((0u64, 0u32), |(sum, count), mhz| (sum + mhz, count + 1));
    (count > 0).then(|| sum as f64 / f64::from(count))
}

/// The speed the processor runs at: its nominal speed times the share of it
/// that Windows says is in use, the way Task Manager works out "Speed". The
/// share goes above 100 while the processor runs faster than its nominal
/// speed.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "only Windows reads the counter")
)]
fn effective_mhz(nominal_mhz: Option<f64>, performance_percent: Option<f64>) -> Option<f64> {
    let (nominal, percent) = (nominal_mhz?, performance_percent?);
    (percent.is_finite() && percent > 0.0 && percent <= 1000.0).then(|| nominal * percent / 100.0)
}

/// Busy time from the share of time the drives were idle.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "only Windows reads the counter")
)]
fn busy_from_idle(idle_percent: f64) -> Option<f64> {
    idle_percent
        .is_finite()
        .then(|| (100.0 - idle_percent).clamp(0.0, 100.0))
}

#[cfg(windows)]
mod win {
    use windows::core::PCWSTR;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
        PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE,
        PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    };

    /// English names work on every display language of Windows.
    const PERFORMANCE: &str = r"\Processor Information(_Total)\% Processor Performance";
    const IDLE_TIME: &str = r"\PhysicalDisk(_Total)\% Idle Time";

    pub struct Sample {
        pub performance_percent: Option<f64>,
        pub idle_percent: Option<f64>,
    }

    pub struct Counters {
        query: PDH_HQUERY,
        performance: Option<PDH_HCOUNTER>,
        idle: Option<PDH_HCOUNTER>,
    }

    // SAFETY: the query and its counters are plain handles of PDH, which may
    // be used from any thread as long as no two threads use them at once.
    // The sensor collector owns this value and calls it from one thread at a
    // time.
    unsafe impl Send for Counters {}

    impl Counters {
        pub fn open() -> Option<Self> {
            // SAFETY: `query` is a valid out-pointer, and it is used only
            // after PdhOpenQueryW reported success. A query that ends up
            // without a counter is closed before it is forgotten.
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                let performance = add(query, PERFORMANCE);
                let idle = add(query, IDLE_TIME);
                if performance.is_none() && idle.is_none() {
                    let _ = PdhCloseQuery(query);
                    return None;
                }
                // Both are rates: the first collection only sets the start.
                let _ = PdhCollectQueryData(query);
                Some(Self {
                    query,
                    performance,
                    idle,
                })
            }
        }

        pub fn sample(&mut self) -> Option<Sample> {
            // SAFETY: the query and the counters were opened by `open` and
            // are closed only by Drop.
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                Some(Sample {
                    performance_percent: self.performance.and_then(|counter| value(counter)),
                    idle_percent: self.idle.and_then(|counter| value(counter)),
                })
            }
        }
    }

    impl Drop for Counters {
        fn drop(&mut self) {
            // SAFETY: the query was opened by `open` and is closed once.
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }

    /// Adds a counter to an open query. Unsafe: `query` must be open.
    unsafe fn add(query: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut counter = PDH_HCOUNTER::default();
        (PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) == 0)
            .then_some(counter)
    }

    /// The counter's value after a collection. Unsafe: `counter` must belong
    /// to an open query.
    unsafe fn value(counter: PDH_HCOUNTER) -> Option<f64> {
        let mut formatted = PDH_FMT_COUNTERVALUE::default();
        if PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut formatted) != 0 {
            return None;
        }
        matches!(
            formatted.CStatus,
            PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
        )
        // The format asked for is a double, so that is the member PDH filled.
        .then(|| formatted.Anonymous.doubleValue)
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use std::collections::HashMap;

    /// One line per block device, with its counters since the system started.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(dead_code, reason = "read on Linux only")
    )]
    pub const DISKSTATS: &str = "/proc/diskstats";
    /// Position of the device name and of the milliseconds spent doing I/O in
    /// a line of /proc/diskstats.
    const NAME_FIELD: usize = 2;
    const IO_MILLIS_FIELD: usize = 12;

    /// The I/O time of every drive at the last reading.
    #[derive(Debug, Default)]
    pub struct BusyTime {
        io_millis: HashMap<String, u64>,
    }

    impl BusyTime {
        /// Starts from the drives' I/O time as it is now, so the first
        /// reading already has a value. Without it the sensor would appear
        /// one reading after the others, and the computer would declare
        /// its entities twice at every start.
        pub fn primed() -> Self {
            let stats = std::fs::read_to_string(DISKSTATS).unwrap_or_default();
            Self {
                io_millis: whole_disks(&stats),
            }
        }

        /// The share of `elapsed_secs` that the busiest drive spent doing
        /// I/O. None at the first reading and when no drive is listed.
        pub fn busy_percent(&mut self, diskstats: &str, elapsed_secs: f64) -> Option<f64> {
            let now = whole_disks(diskstats);
            let busiest = now
                .iter()
                .filter_map(|(name, millis)| {
                    let before = self.io_millis.get(name)?;
                    Some(millis.saturating_sub(*before))
                })
                .max();
            self.io_millis = now;
            let millis = busiest? as f64;
            (elapsed_secs > 0.0)
                .then(|| (millis / (elapsed_secs * 1000.0) * 100.0).clamp(0.0, 100.0))
        }
    }

    /// The I/O time of each whole drive, leaving out partitions, loop devices
    /// and the like.
    fn whole_disks(diskstats: &str) -> HashMap<String, u64> {
        diskstats
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let name = *fields.get(NAME_FIELD)?;
                let millis = fields.get(IO_MILLIS_FIELD)?.parse().ok()?;
                is_whole_disk(name).then(|| (name.to_string(), millis))
            })
            .collect()
    }

    /// `sda`, `vdb`, `nvme0n1` and `mmcblk0` are drives; `sda1`, `nvme0n1p2`,
    /// `mmcblk0p1`, `loop0`, `dm-0` and `zram0` are not.
    pub fn is_whole_disk(name: &str) -> bool {
        let letters_only =
            |rest: &str| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase());
        let digits_only = |rest: &str| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
        if let Some(rest) = name.strip_prefix("sd").or_else(|| name.strip_prefix("vd")) {
            return letters_only(rest);
        }
        if let Some(rest) = name.strip_prefix("nvme") {
            return rest.split_once('n').is_some_and(|(controller, namespace)| {
                digits_only(controller) && digits_only(namespace)
            });
        }
        name.strip_prefix("mmcblk").is_some_and(digits_only)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speed_is_the_nominal_one_times_the_share_in_use() {
        assert_eq!(effective_mhz(Some(3000.0), Some(50.0)), Some(1500.0));
        // Faster than nominal.
        assert_eq!(effective_mhz(Some(3000.0), Some(130.0)), Some(3900.0));
        assert_eq!(effective_mhz(None, Some(50.0)), None);
        assert_eq!(effective_mhz(Some(3000.0), None), None);
        for nonsense in [0.0, -5.0, f64::NAN, f64::INFINITY, 5000.0] {
            assert_eq!(
                effective_mhz(Some(3000.0), Some(nonsense)),
                None,
                "{nonsense}"
            );
        }
    }

    #[test]
    fn the_mean_speed_leaves_out_processors_that_report_none() {
        assert_eq!(mean_mhz([2000, 4000].into_iter()), Some(3000.0));
        assert_eq!(mean_mhz([0, 3000, 0].into_iter()), Some(3000.0));
        assert_eq!(mean_mhz([0, 0].into_iter()), None);
        assert_eq!(mean_mhz(std::iter::empty()), None);
    }

    #[test]
    fn busy_time_is_what_is_left_of_idle_time() {
        assert_eq!(busy_from_idle(100.0), Some(0.0));
        assert_eq!(busy_from_idle(25.0), Some(75.0));
        // The counter runs a little over and under.
        assert_eq!(busy_from_idle(101.3), Some(0.0));
        assert_eq!(busy_from_idle(-2.0), Some(100.0));
        assert_eq!(busy_from_idle(f64::NAN), None);
    }

    #[test]
    fn only_whole_drives_count() {
        for drive in ["sda", "sdab", "vdb", "nvme0n1", "nvme10n2", "mmcblk0"] {
            assert!(linux::is_whole_disk(drive), "{drive}");
        }
        for other in [
            "sda1",
            "vdb2",
            "nvme0n1p2",
            "nvme0",
            "mmcblk0p1",
            "mmcblk",
            "loop0",
            "dm-0",
            "zram0",
            "sr0",
            "sd",
            "",
        ] {
            assert!(!linux::is_whole_disk(other), "{other}");
        }
    }

    const BEFORE: &str = "\
   8       0 sda 100 0 800 50 200 0 1600 70 0 1000 120 0 0 0 0
   8       1 sda1 90 0 700 40 190 0 1500 60 0 900 100 0 0 0 0
 259       0 nvme0n1 500 0 4000 90 600 0 4800 80 0 5000 170 0 0 0 0
   7       0 loop0 10 0 80 1 0 0 0 0 0 4 1 0 0 0 0
";
    const AFTER: &str = "\
   8       0 sda 110 0 880 55 220 0 1760 77 0 1500 132 0 0 0 0
   8       1 sda1 99 0 770 44 209 0 1650 66 0 1400 110 0 0 0 0
 259       0 nvme0n1 500 0 4000 90 600 0 4800 80 0 5200 170 0 0 0 0
   7       0 loop0 99 0 800 9 0 0 0 0 0 4000 9 0 0 0 0
";

    #[test]
    fn disk_activity_is_the_busiest_whole_drive() {
        let mut busy = linux::BusyTime::default();
        // Nothing to compare with yet.
        assert_eq!(busy.busy_percent(BEFORE, 2.0), None);
        // sda was busy for 500 ms of 2 s, nvme0n1 for 200 ms; the loop
        // device and the partition do not count.
        assert_eq!(busy.busy_percent(AFTER, 2.0), Some(25.0));
        // No more I/O.
        assert_eq!(busy.busy_percent(AFTER, 2.0), Some(0.0));
        // Never over 100, whatever the clock says.
        assert_eq!(busy.busy_percent(BEFORE, 2.0), Some(0.0));
        let mut busy = linux::BusyTime::default();
        busy.busy_percent(BEFORE, 2.0);
        assert_eq!(busy.busy_percent(AFTER, 0.1), Some(100.0));
    }

    #[test]
    fn a_system_without_drives_has_no_disk_activity() {
        let mut busy = linux::BusyTime::default();
        assert_eq!(busy.busy_percent("", 2.0), None);
        assert_eq!(busy.busy_percent("", 2.0), None);
    }

    /// Runs the real counters of the computer the test runs on.
    #[test]
    fn the_real_readings_have_values() {
        let mut counters = LoadCounters::new();
        let mut sys = System::new().ok();
        let readings = counters.collect(sys.as_mut(), 1.0);
        for reading in &readings {
            let value: f64 = reading.value.parse().unwrap();
            assert!(value.is_finite() && value >= 0.0, "{}", reading.id);
        }
        if sys.is_some() {
            assert!(readings.iter().any(|reading| reading.id == "memory_used"));
        }
    }
}
