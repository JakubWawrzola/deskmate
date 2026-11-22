//! The health Windows reports for each physical drive: the verdict of its
//! storage stack, built on the drive's own failure prediction (SMART). Any
//! account may read it. Temperature and wear are in another class that only
//! an administrator may read, so they are not offered here: a sensor that
//! exists only while the service is connected would come and go in Home
//! Assistant.

use std::time::{Duration, Instant};

/// A drive does not fail and recover within minutes, and the query is not
/// free.
const READ_EVERY: Duration = Duration::from_secs(600);
/// More drives than this are a storage array, not a desktop.
const MAX_DRIVES: usize = 16;

/// One drive: its number in Windows (Disk 0, Disk 1) and its health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriveHealth {
    pub number: u32,
    pub health: &'static str,
}

/// The health of each drive, read again every `READ_EVERY`.
#[derive(Default)]
pub struct DiskHealth {
    last_read: Option<Instant>,
    drives: Vec<DriveHealth>,
}

impl DiskHealth {
    pub fn read(&mut self) -> &[DriveHealth] {
        if self.last_read.is_none_or(|at| at.elapsed() >= READ_EVERY) {
            self.drives = read_now();
            self.last_read = Some(Instant::now());
        }
        &self.drives
    }
}

#[cfg(windows)]
fn read_now() -> Vec<DriveHealth> {
    let rows = crate::wmi::query(
        "ROOT\\Microsoft\\Windows\\Storage",
        "SELECT DeviceId, HealthStatus FROM MSFT_PhysicalDisk",
        &["DeviceId", "HealthStatus"],
    );
    let mut drives: Vec<DriveHealth> = rows
        .iter()
        .filter_map(|row| drive(row[0].number(), row[1].number()))
        .collect();
    drives.sort_by_key(|drive| drive.number);
    drives.dedup_by_key(|drive| drive.number);
    drives.truncate(MAX_DRIVES);
    drives
}

#[cfg(not(windows))]
fn read_now() -> Vec<DriveHealth> {
    Vec::new()
}

/// A row of MSFT_PhysicalDisk. A drive without a number or with a health
/// Windows does not know is left out rather than shown as "unknown".
#[cfg_attr(not(windows), allow(dead_code, reason = "only Windows has the class"))]
fn drive(device_id: Option<f64>, health_status: Option<f64>) -> Option<DriveHealth> {
    let number = device_id.filter(|id| id.fract() == 0.0 && (0.0..1000.0).contains(id))? as u32;
    let health = match health_status? as i64 {
        0 => "Healthy",
        1 => "Warning",
        2 => "Unhealthy",
        _ => return None,
    };
    Some(DriveHealth { number, health })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_three_verdicts() {
        let health = |status: f64| drive(Some(1.0), Some(status)).map(|drive| drive.health);
        assert_eq!(health(0.0), Some("Healthy"));
        assert_eq!(health(1.0), Some("Warning"));
        assert_eq!(health(2.0), Some("Unhealthy"));
        // 5 is "Unknown" in Windows.
        assert_eq!(health(5.0), None);
        assert_eq!(health(99.0), None);
        assert_eq!(drive(Some(1.0), None), None);
    }

    #[test]
    fn a_drive_needs_a_number() {
        assert_eq!(
            drive(Some(0.0), Some(0.0)),
            Some(DriveHealth {
                number: 0,
                health: "Healthy"
            })
        );
        assert_eq!(drive(None, Some(0.0)), None);
        assert_eq!(drive(Some(-1.0), Some(0.0)), None);
        assert_eq!(drive(Some(1.5), Some(0.0)), None);
        assert_eq!(drive(Some(5000.0), Some(0.0)), None);
    }

    #[test]
    fn the_drives_are_read_once_and_kept() {
        let mut health = DiskHealth::default();
        let first = health.read().to_vec();
        assert!(health.last_read.is_some());
        assert_eq!(health.read(), first.as_slice());
    }

    /// Asks the computer the test runs on. A build agent may run on storage
    /// that reports nothing, so only the shape is checked.
    #[cfg(windows)]
    #[test]
    fn the_real_drives_have_distinct_numbers_and_a_verdict() {
        let drives = read_now();
        for pair in drives.windows(2) {
            assert!(pair[0].number < pair[1].number);
        }
        for drive in drives {
            assert!(["Healthy", "Warning", "Unhealthy"].contains(&drive.health));
        }
    }
}
