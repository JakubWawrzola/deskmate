//! What tells the daemon's runner that something happened: signals, and a
//! look at the clocks that notices when the computer slept.
//!
//! | Signal | Sent by | Event |
//! |---|---|---|
//! | SIGTERM | `systemctl stop`, and systemd while the system shuts down | `Shutdown` while the system stops, otherwise `Stop` |
//! | SIGINT | Ctrl+C in a terminal | `Stop` |
//! | SIGHUP | `systemctl reload deskmate` | `Reload` |
//! | SIGUSR1 | `deskmate-sleep.service`, before the system sleeps | `Suspend` |
//! | SIGUSR2 | `deskmate-sleep.service`, after it woke up | `Resume` |
//!
//! Without the sleep unit the daemon still notices a wake-up within a few
//! seconds: `CLOCK_BOOTTIME` (`/proc/uptime`) counts the time asleep and
//! `CLOCK_MONOTONIC` (`Instant`) does not, so the gap between them grows by
//! the length of every sleep.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;

use crate::runner::{Ack, Event, EventSender};

/// How often the clocks are compared.
const WAKE_CHECK: Duration = Duration::from_secs(5);
/// A gap that grows by more than this was a sleep, not jitter.
const SLEPT_AT_LEAST: Duration = Duration::from_secs(3);
/// Counts the time since boot, sleep included.
const UPTIME: &str = "/proc/uptime";
/// Asks systemd for the state of the system. Fixed: nothing received is in it.
const SYSTEMCTL: &str = "systemctl";
const IS_SYSTEM_RUNNING: [&str; 1] = ["is-system-running"];
/// Longest wait for that answer before a SIGTERM counts as a plain stop.
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(20);

/// Listens for the signals in the table above until aborted. With
/// `ask_systemd`, a SIGTERM while the system stops is a shutdown.
pub fn listen(events: EventSender, ask_systemd: bool) -> std::io::Result<JoinHandle<()>> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut sleeping = signal(SignalKind::user_defined1())?;
    let mut woke = signal(SignalKind::user_defined2())?;
    Ok(tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                Some(()) = terminate.recv() => {
                    if ask_systemd && system_stopping().await {
                        log::info!("the system is shutting down");
                        Event::Shutdown
                    } else {
                        Event::Stop
                    }
                }
                Some(()) = interrupt.recv() => Event::Stop,
                Some(()) = hangup.recv() => Event::Reload,
                // The sleep unit waits on its own before it lets the system
                // sleep, so nobody waits on the Ack.
                Some(()) = sleeping.recv() => Event::Suspend(Ack::none()),
                Some(()) = woke.recv() => Event::Resume,
                else => return,
            };
            if events.send(event).is_err() {
                return;
            }
        }
    }))
}

/// Whether systemd is taking the system down, asked without blocking the
/// runtime. Any doubt means no.
async fn system_stopping() -> bool {
    tokio::task::spawn_blocking(|| {
        run_fixed(SYSTEMCTL, &IS_SYSTEM_RUNNING, SYSTEMCTL_TIMEOUT)
            .is_some_and(|answer| answer.trim() == "stopping")
    })
    .await
    .unwrap_or(false)
}

/// Runs a program with fixed arguments and returns what it printed, or None
/// when it could not start or took longer than `limit`. Its exit code does
/// not matter: `is-system-running` exits non-zero in every state but
/// `running`.
fn run_fixed(program: &str, arguments: &[&str], limit: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(4096)
        .read_to_string(&mut output)
        .ok()?;
    Some(output)
}

/// The time since boot from `/proc/uptime`: seconds with a fraction, then
/// the idle time.
pub fn parse_uptime(text: &str) -> Option<Duration> {
    let seconds: f64 = text.split_whitespace().next()?.parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// Notices a sleep from two readings: the time since boot with sleep and
/// without it.
pub struct SleepDetector {
    /// Time asleep since boot as of the last reading.
    asleep: Option<Duration>,
}

impl SleepDetector {
    pub fn new() -> Self {
        Self { asleep: None }
    }

    /// Takes a reading; true when the computer slept since the last one.
    pub fn observe(&mut self, with_sleep: Duration, without_sleep: Duration) -> bool {
        let asleep = with_sleep.saturating_sub(without_sleep);
        let slept = self
            .asleep
            .is_some_and(|before| asleep.saturating_sub(before) > SLEPT_AT_LEAST);
        self.asleep = Some(asleep);
        slept
    }
}

impl Default for SleepDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Compares the clocks every few seconds and sends `Resume` after a sleep.
pub async fn watch_for_wake(events: EventSender) {
    let started = Instant::now();
    let mut detector = SleepDetector::new();
    let mut tick = tokio::time::interval(WAKE_CHECK);
    loop {
        tick.tick().await;
        let Some(with_sleep) = std::fs::read_to_string(UPTIME)
            .ok()
            .and_then(|text| parse_uptime(&text))
        else {
            log::warn!("cannot read {UPTIME}; wake-ups are noticed by the sleep unit only");
            return;
        };
        // Both counted from the same moment: the start of this watch.
        let since_start = started.elapsed();
        if detector.observe(with_sleep, since_start) {
            log::info!("the computer woke up");
            if events.send(Event::Resume).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn reads_the_time_since_boot() {
        assert_eq!(
            parse_uptime("12345.67 98765.43\n"),
            Some(Duration::from_secs_f64(12345.67))
        );
        assert_eq!(parse_uptime("0.00 0.00"), Some(Duration::ZERO));
        for broken in ["", "\n", "abc 1", "-5.0 1", "inf 1", "NaN 1"] {
            assert_eq!(parse_uptime(broken), None, "{broken}");
        }
    }

    #[test]
    fn a_growing_gap_between_the_clocks_is_a_sleep() {
        let mut detector = SleepDetector::new();
        // Booted 1000 s ago, of which 200 s asleep before the daemon started.
        assert!(!detector.observe(secs(1000), secs(800)));
        assert!(
            !detector.observe(secs(1005), secs(805)),
            "five seconds awake"
        );
        assert!(
            !detector.observe(secs(1012), secs(810)),
            "two seconds of jitter"
        );
        assert!(
            detector.observe(secs(1615), secs(815)),
            "ten minutes asleep"
        );
        assert!(!detector.observe(secs(1620), secs(820)), "awake again");
    }

    #[test]
    fn a_clock_that_goes_back_is_not_a_sleep() {
        let mut detector = SleepDetector::new();
        assert!(!detector.observe(secs(1000), secs(10)));
        assert!(!detector.observe(secs(900), secs(15)));
        assert!(!detector.observe(secs(905), secs(20)));
    }

    #[test]
    fn a_fixed_command_that_does_not_exist_or_takes_too_long_gives_nothing() {
        assert_eq!(
            run_fixed("deskmate-no-such-program", &[], Duration::from_secs(1)),
            None
        );
        assert_eq!(run_fixed("sleep", &["5"], Duration::from_millis(100)), None);
        assert_eq!(
            run_fixed("echo", &["stopping"], Duration::from_secs(5)).as_deref(),
            Some("stopping\n")
        );
    }
}
