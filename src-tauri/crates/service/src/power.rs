//! The power commands and the valve in front of them.
//!
//! The service runs exactly four commands for Home Assistant: sleep,
//! hibernate, shut down and restart. They are refused for five minutes after
//! the computer starts or wakes: an automation that turns the computer off
//! as soon as it connects would otherwise do so on every start, and nobody
//! could get in to switch it off.

use std::sync::Arc;
use std::time::Duration;

/// How long after a start or a wake-up the power commands stay refused.
pub const QUIET_AFTER_START: Duration = Duration::from_secs(5 * 60);

/// Runs a power command. `key` is one of `deskmate_core::machine::COMMAND_KEYS`;
/// the caller has checked that.
pub trait PowerExecutor: Send + Sync {
    fn run(&self, key: &str) -> Result<(), String>;
    /// How this executor shows up in the audit log.
    fn name(&self) -> &'static str;
}

/// The real thing: asks Windows to sleep, hibernate, shut down or restart.
/// It does not exist in a build with `test-hooks`.
#[cfg(not(feature = "test-hooks"))]
pub struct SystemPower;

#[cfg(not(feature = "test-hooks"))]
impl PowerExecutor for SystemPower {
    fn run(&self, key: &str) -> Result<(), String> {
        deskmate_core::machine::run_command(key)
    }

    fn name(&self) -> &'static str {
        "system"
    }
}

/// Runs nothing. The caller records the command in the audit log, which is
/// all that happens. Only a build with `test-hooks` has it.
#[cfg(feature = "test-hooks")]
pub struct DryRunPower;

#[cfg(feature = "test-hooks")]
impl PowerExecutor for DryRunPower {
    fn run(&self, _key: &str) -> Result<(), String> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "dry_run"
    }
}

/// Time since the computer started, sleep included.
pub trait Clock: Send + Sync {
    fn uptime(&self) -> Duration;
}

/// The system's own count of time since boot.
pub struct SystemClock;

impl Clock for SystemClock {
    fn uptime(&self) -> Duration {
        Duration::from_secs(sysinfo::System::uptime())
    }
}

/// Decides whether a power command may run now.
///
/// Time since boot alone is not enough: with Fast Startup, Windows shuts
/// down by hibernating the kernel, and the count goes on across what the
/// user sees as a cold start. The service is told about every wake-up, so
/// the valve counts from the later of the two.
pub struct SafetyValve {
    clock: Arc<dyn Clock>,
    /// Uptime at the last wake-up the service was told about.
    woke_at: Option<Duration>,
}

impl SafetyValve {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            woke_at: None,
        }
    }

    /// The computer woke from sleep or hibernation just now.
    pub fn note_wake(&mut self) {
        self.woke_at = Some(self.clock.uptime());
    }

    /// Ok when power commands may run; otherwise how long they stay refused.
    pub fn check(&self) -> Result<(), Duration> {
        let uptime = self.clock.uptime();
        let since_start = match self.woke_at {
            Some(woke_at) => uptime.saturating_sub(woke_at),
            None => uptime,
        };
        if since_start < QUIET_AFTER_START {
            Err(QUIET_AFTER_START - since_start)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A clock the test sets by hand.
    pub(crate) struct FakeClock(Mutex<Duration>);

    impl FakeClock {
        pub(crate) fn at(seconds: u64) -> Arc<Self> {
            Arc::new(Self(Mutex::new(Duration::from_secs(seconds))))
        }

        pub(crate) fn set(&self, seconds: u64) {
            *self.0.lock().unwrap() = Duration::from_secs(seconds);
        }
    }

    impl Clock for FakeClock {
        fn uptime(&self) -> Duration {
            *self.0.lock().unwrap()
        }
    }

    #[test]
    fn refuses_for_five_minutes_after_boot() {
        let clock = FakeClock::at(0);
        let valve = SafetyValve::new(clock.clone());
        assert_eq!(valve.check(), Err(Duration::from_secs(300)));
        clock.set(120);
        assert_eq!(valve.check(), Err(Duration::from_secs(180)));
        clock.set(299);
        assert_eq!(valve.check(), Err(Duration::from_secs(1)));
        clock.set(300);
        assert_eq!(valve.check(), Ok(()));
        clock.set(86_400);
        assert_eq!(valve.check(), Ok(()));
    }

    #[test]
    fn refuses_for_five_minutes_after_a_wake_up() {
        // Two days up, as after a Fast Startup "shutdown".
        let clock = FakeClock::at(172_800);
        let mut valve = SafetyValve::new(clock.clone());
        assert_eq!(valve.check(), Ok(()));

        valve.note_wake();
        assert_eq!(valve.check(), Err(Duration::from_secs(300)));
        clock.set(172_800 + 299);
        assert!(valve.check().is_err());
        clock.set(172_800 + 300);
        assert_eq!(valve.check(), Ok(()));
    }

    #[test]
    fn a_wake_up_soon_after_boot_starts_the_wait_again() {
        let clock = FakeClock::at(280);
        let mut valve = SafetyValve::new(clock.clone());
        valve.note_wake();
        clock.set(400);
        assert_eq!(valve.check(), Err(Duration::from_secs(180)));
        clock.set(580);
        assert_eq!(valve.check(), Ok(()));
    }

    #[test]
    fn a_clock_that_went_back_does_not_open_the_valve() {
        let clock = FakeClock::at(1000);
        let mut valve = SafetyValve::new(clock.clone());
        valve.note_wake();
        clock.set(10);
        assert!(valve.check().is_err());
    }
}
