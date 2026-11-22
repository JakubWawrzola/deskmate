//! When to try again and at which address, for the Link loop of the app and
//! of the service. An address that cannot be reached gives way to the other
//! one at once: a computer away from home should not keep knocking on an
//! address that only exists on the home network. A session that broke is
//! retried at the same address first. A refused pairing, a lockout or a clock
//! problem is retried slowly, because repeating it cannot succeed and only
//! keeps the computer locked out.

use super::{auth_retry_delay, LinkFailure};
use std::time::Duration;

/// Failures in a row at one address before the other one is tried, for a
/// failure that says nothing about the address itself.
const FAILURES_BEFORE_ROTATING: u32 = 2;
/// Shortest wait before the first attempt at the other address.
const ROTATION_MIN_SECS: u64 = 3;
/// The wait before the remote address is tried after the local one could not
/// be reached. The way back to the local address waits `ROTATION_MIN_SECS`,
/// so two dead addresses are not tried in a tight loop.
const UNREACHABLE_ROTATION_SECS: u64 = 1;

/// Remembers the failures so far and picks the address and the delay.
#[derive(Debug)]
pub struct RetrySchedule {
    endpoints: Vec<String>,
    index: usize,
    failures: u32,
    auth_failures: u32,
}

impl RetrySchedule {
    /// `endpoints` holds one or two addresses, the local one first.
    pub fn new(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            index: 0,
            failures: 0,
            auth_failures: 0,
        }
    }

    /// The address to try now.
    pub fn endpoint(&self) -> &str {
        self.endpoints
            .get(self.index)
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// "" with one address, otherwise which of the two is tried now.
    pub fn label(&self) -> &'static str {
        match (self.endpoints.len(), self.index) {
            (0 | 1, _) => "",
            (_, 0) => " (local address)",
            _ => " (remote address)",
        }
    }

    /// The wait after a session that ended without a failure.
    pub fn base_delay(&self) -> Duration {
        Duration::from_secs(self.base_secs())
    }

    fn base_secs(&self) -> u64 {
        if self.endpoints.len() > 1 {
            2
        } else {
            5
        }
    }

    /// A session ended without a failure, or the computer woke up: start
    /// over with the fast schedule at the current address.
    pub fn reset(&mut self) {
        self.failures = 0;
        self.auth_failures = 0;
    }

    /// A connection attempt or a session failed. Returns how long to wait
    /// before the next attempt.
    pub fn on_failure(&mut self, kind: LinkFailure) -> Duration {
        self.failures += 1;
        let mut delay = self.base_secs();
        if kind.is_network() {
            self.auth_failures = 0;
        } else {
            delay = auth_retry_delay(kind, self.auth_failures);
            self.auth_failures += 1;
        }
        let unreachable = kind == LinkFailure::Unreachable;
        let rotates = unreachable || self.failures >= FAILURES_BEFORE_ROTATING;
        if self.endpoints.len() > 1 && rotates {
            self.index = (self.index + 1) % self.endpoints.len();
            self.failures = 0;
            delay = if unreachable && self.index != 0 {
                UNREACHABLE_ROTATION_SECS
            } else {
                delay.max(ROTATION_MIN_SECS)
            };
        }
        Duration::from_secs(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> RetrySchedule {
        RetrySchedule::new(vec!["ws://local".into()])
    }

    fn two() -> RetrySchedule {
        RetrySchedule::new(vec!["ws://local".into(), "wss://remote".into()])
    }

    fn secs(schedule: &mut RetrySchedule, kind: LinkFailure) -> u64 {
        schedule.on_failure(kind).as_secs()
    }

    #[test]
    fn one_address_retries_a_network_failure_every_five_seconds() {
        let mut schedule = one();
        for _ in 0..5 {
            assert_eq!(secs(&mut schedule, LinkFailure::Transport), 5);
            assert_eq!(schedule.endpoint(), "ws://local");
            assert_eq!(schedule.label(), "");
        }
    }

    #[test]
    fn an_address_that_cannot_be_reached_gives_way_at_once() {
        let mut schedule = two();
        assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 1);
        assert_eq!(schedule.endpoint(), "wss://remote");
        // Both are down: the way back to the local address is not rushed.
        assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 3);
        assert_eq!(schedule.endpoint(), "ws://local");
        assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 1);
        assert_eq!(schedule.endpoint(), "wss://remote");
    }

    #[test]
    fn one_address_that_cannot_be_reached_is_retried_every_five_seconds() {
        let mut schedule = one();
        for _ in 0..3 {
            assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 5);
            assert_eq!(schedule.endpoint(), "ws://local");
        }
    }

    #[test]
    fn a_session_that_broke_is_retried_at_the_same_address_first() {
        let mut schedule = two();
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 2);
        assert_eq!(schedule.endpoint(), "ws://local");
        // The retry cannot reach the address: now the other one is tried.
        assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 1);
        assert_eq!(schedule.endpoint(), "wss://remote");
    }

    #[test]
    fn two_addresses_rotate_after_two_failures_in_a_row() {
        let mut schedule = two();
        assert_eq!(schedule.label(), " (local address)");
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 2);
        assert_eq!(schedule.endpoint(), "ws://local");
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 3);
        assert_eq!(schedule.endpoint(), "wss://remote");
        assert_eq!(schedule.label(), " (remote address)");
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 2);
        assert_eq!(schedule.endpoint(), "wss://remote");
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 3);
        assert_eq!(schedule.endpoint(), "ws://local");
    }

    #[test]
    fn a_refused_pairing_backs_off_to_a_minute() {
        let mut schedule = one();
        let delays: Vec<u64> = (0..6)
            .map(|_| secs(&mut schedule, LinkFailure::Auth))
            .collect();
        assert_eq!(delays, [5, 5, 15, 30, 60, 60]);
    }

    #[test]
    fn a_lockout_waits_a_minute_and_a_clock_problem_half_a_minute() {
        let mut schedule = one();
        assert_eq!(secs(&mut schedule, LinkFailure::Locked), 60);
        assert_eq!(secs(&mut schedule, LinkFailure::Clock), 30);
    }

    #[test]
    fn a_network_failure_ends_the_slow_schedule() {
        let mut schedule = one();
        for _ in 0..4 {
            secs(&mut schedule, LinkFailure::Auth);
        }
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 60);
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 5);
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 5);

        let mut schedule = one();
        for _ in 0..4 {
            secs(&mut schedule, LinkFailure::Auth);
        }
        assert_eq!(secs(&mut schedule, LinkFailure::Unreachable), 5);
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 5);
    }

    #[test]
    fn a_good_session_starts_the_schedule_over() {
        let mut schedule = two();
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 2);
        schedule.reset();
        // One failure before the reset does not count towards the rotation.
        assert_eq!(secs(&mut schedule, LinkFailure::Transport), 2);
        assert_eq!(schedule.endpoint(), "ws://local");

        let mut schedule = one();
        for _ in 0..4 {
            secs(&mut schedule, LinkFailure::Auth);
        }
        schedule.reset();
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 5);
    }

    #[test]
    fn a_slow_retry_still_rotates_to_the_other_address() {
        let mut schedule = two();
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 5);
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 5);
        assert_eq!(schedule.endpoint(), "wss://remote");
        assert_eq!(secs(&mut schedule, LinkFailure::Auth), 15);
        assert_eq!(schedule.endpoint(), "wss://remote");
    }
}
