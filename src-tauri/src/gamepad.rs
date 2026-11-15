//! Game controller input for the idle time sensor. Windows counts only the
//! keyboard and the mouse in GetLastInputInfo, so someone playing with a
//! controller would look idle. While the idle sensors are on, a thread asks
//! XInput for the state of the four controller slots and remembers when a
//! controller was last used.
//!
//! XInput numbers every change of a controller's state (`dwPacketNumber`). A
//! new number counts as input when a button changed, or when a stick or a
//! trigger is pushed past the dead zone Microsoft recommends, before or after
//! the change. Sticks that drift a little at rest change the number too; the
//! dead zone keeps them from counting as someone playing.
//!
//! XInputGetState reports only the state at the moment of the call, so a
//! button pressed and released between two calls leaves no trace. A
//! connected controller is therefore asked more often than a quick tap
//! lasts. Empty slots are asked once a second only: Microsoft advises
//! against asking them often, because each such call looks for a new device.

use std::time::{Duration, Instant};

/// How often a connected controller is asked. A quick tap of a button lasts
/// about 60 to 100 ms; asking every 50 ms sees each one at least once.
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How often every slot is asked, to find a controller that was switched on
/// or plugged in.
const SCAN_INTERVAL: Duration = Duration::from_secs(1);

/// The number of controller slots XInput has (XUSER_MAX_COUNT).
const SLOT_COUNT: usize = 4;

/// Dead zones from the XInput documentation (XINPUT_GAMEPAD_*_DEADZONE,
/// XINPUT_GAMEPAD_TRIGGER_THRESHOLD).
const LEFT_THUMB_DEAD_ZONE: i64 = 7849;
const RIGHT_THUMB_DEAD_ZONE: i64 = 8689;
const TRIGGER_THRESHOLD: u8 = 30;

/// The state of one connected controller, as XInput reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reading {
    pub packet: u32,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_thumb: (i16, i16),
    pub right_thumb: (i16, i16),
}

impl Reading {
    /// A stick or a trigger is held past its dead zone.
    fn is_deflected(&self) -> bool {
        past(self.left_thumb, LEFT_THUMB_DEAD_ZONE)
            || past(self.right_thumb, RIGHT_THUMB_DEAD_ZONE)
            || self.left_trigger > TRIGGER_THRESHOLD
            || self.right_trigger > TRIGGER_THRESHOLD
    }
}

/// The stick is further from its centre than the dead zone (a circle).
fn past((x, y): (i16, i16), dead_zone: i64) -> bool {
    let (x, y) = (i64::from(x), i64::from(y));
    x * x + y * y > dead_zone * dead_zone
}

/// What one reading of a slot says about its previous one: true when someone
/// used the controller in between.
fn is_input(previous: Option<&Reading>, current: Option<&Reading>) -> bool {
    match (previous, current) {
        // A controller was switched on or plugged in.
        (None, Some(_)) => true,
        (Some(previous), Some(current)) => {
            current.packet != previous.packet
                && (current.buttons != previous.buttons
                    || current.is_deflected()
                    || previous.is_deflected())
        }
        // Switched off, or nothing there.
        (_, None) => false,
    }
}

/// Remembers the last reading of every slot between polls.
#[derive(Debug, Default)]
struct Tracker {
    slots: [Option<Reading>; SLOT_COUNT],
    primed: bool,
    last_scan: Option<Instant>,
}

impl Tracker {
    /// The slots to ask in the poll taken at `now`: the connected ones, and
    /// every slot when a scan is due.
    fn slots_to_read(&mut self, now: Instant) -> [bool; SLOT_COUNT] {
        let scan = self
            .last_scan
            .is_none_or(|at| now.saturating_duration_since(at) >= SCAN_INTERVAL);
        if scan {
            self.last_scan = Some(now);
        }
        std::array::from_fn(|index| scan || self.slots[index].is_some())
    }

    /// The wait before the next poll: short while a controller is connected.
    fn wait(&self) -> Duration {
        if self.slots.iter().any(Option::is_some) {
            ACTIVE_POLL_INTERVAL
        } else {
            SCAN_INTERVAL
        }
    }

    /// Takes one poll of all slots. True when any controller was used since
    /// the previous poll. The first poll only records what is connected: a
    /// controller lying on the desk when the sensor starts is not input.
    fn update(&mut self, current: [Option<Reading>; SLOT_COUNT]) -> bool {
        let used = self.primed
            && self
                .slots
                .iter()
                .zip(current.iter())
                .any(|(previous, now)| is_input(previous.as_ref(), now.as_ref()));
        self.slots = current;
        self.primed = true;
        used
    }
}

/// The idle time to report: the shorter of keyboard and mouse idle time and
/// controller idle time. Without controller input so far, the keyboard and
/// mouse alone.
pub fn combined_idle_seconds(keyboard_and_mouse: u64, controller: Option<u64>) -> u64 {
    controller.map_or(keyboard_and_mouse, |controller| {
        keyboard_and_mouse.min(controller)
    })
}

/// The polling thread. It stops when the watcher is dropped.
pub struct Watcher {
    /// Dropping the sender wakes the thread and ends it.
    _stop: std::sync::mpsc::Sender<()>,
    last_input: std::sync::Arc<std::sync::Mutex<Option<Instant>>>,
}

impl Watcher {
    /// Starts polling. None when the thread cannot be started; the idle
    /// sensor then reports keyboard and mouse input only.
    pub fn start() -> Option<Self> {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let last_input = std::sync::Arc::new(std::sync::Mutex::new(None));
        let shared = std::sync::Arc::clone(&last_input);
        let started = std::thread::Builder::new()
            .name("gamepad-idle".into())
            .spawn(move || {
                let mut tracker = Tracker::default();
                loop {
                    let wanted = tracker.slots_to_read(Instant::now());
                    if tracker.update(read_slots(wanted)) {
                        if let Ok(mut last) = shared.lock() {
                            *last = Some(Instant::now());
                        }
                    }
                    match stopped.recv_timeout(tracker.wait()) {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
            });
        match started {
            Ok(_) => Some(Self {
                _stop: stop,
                last_input,
            }),
            Err(error) => {
                log::warn!("cannot watch game controllers: {error}");
                None
            }
        }
    }

    /// Seconds since a controller was last used, None before the first use.
    pub fn idle_seconds(&self) -> Option<u64> {
        let last = *self.last_input.lock().ok()?;
        last.map(|at| at.elapsed().as_secs())
    }
}

/// One reading of the wanted slots. A slot without a controller, or one that
/// was not asked, is None.
#[cfg(windows)]
fn read_slots(wanted: [bool; SLOT_COUNT]) -> [Option<Reading>; SLOT_COUNT] {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
    let mut slots = [None; SLOT_COUNT];
    for ((index, slot), wanted) in (0u32..).zip(slots.iter_mut()).zip(wanted) {
        if !wanted {
            continue;
        }
        let mut state = XINPUT_STATE::default();
        // SAFETY: `state` is a valid, writable XINPUT_STATE for the duration
        // of the call, which only fills it in; `index` is below
        // XUSER_MAX_COUNT.
        let result = unsafe { XInputGetState(index, &mut state) };
        if result != ERROR_SUCCESS.0 {
            continue;
        }
        let pad = state.Gamepad;
        *slot = Some(Reading {
            packet: state.dwPacketNumber,
            buttons: pad.wButtons.0,
            left_trigger: pad.bLeftTrigger,
            right_trigger: pad.bRightTrigger,
            left_thumb: (pad.sThumbLX, pad.sThumbLY),
            right_thumb: (pad.sThumbRX, pad.sThumbRY),
        });
    }
    slots
}

#[cfg(not(windows))]
fn read_slots(_wanted: [bool; SLOT_COUNT]) -> [Option<Reading>; SLOT_COUNT] {
    [None; SLOT_COUNT]
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_BUTTON: u16 = 0x1000;

    fn at_rest(packet: u32) -> Reading {
        Reading {
            packet,
            ..Reading::default()
        }
    }

    fn only(slot: usize, reading: Reading) -> [Option<Reading>; SLOT_COUNT] {
        let mut slots = [None; SLOT_COUNT];
        slots[slot] = Some(reading);
        slots
    }

    #[test]
    fn the_shorter_idle_time_wins() {
        assert_eq!(combined_idle_seconds(600, Some(5)), 5);
        assert_eq!(combined_idle_seconds(3, Some(900)), 3);
        assert_eq!(combined_idle_seconds(42, Some(42)), 42);
        assert_eq!(combined_idle_seconds(600, None), 600);
        assert_eq!(combined_idle_seconds(0, None), 0);
    }

    #[test]
    fn a_pressed_button_is_input() {
        let pressed = Reading {
            packet: 2,
            buttons: A_BUTTON,
            ..Reading::default()
        };
        assert!(is_input(Some(&at_rest(1)), Some(&pressed)));
        // and releasing it again
        assert!(is_input(Some(&pressed), Some(&at_rest(3))));
    }

    #[test]
    fn the_same_packet_is_no_input() {
        let held = Reading {
            packet: 7,
            buttons: A_BUTTON,
            left_thumb: (32_767, 0),
            ..Reading::default()
        };
        assert!(!is_input(Some(&held), Some(&held)));
    }

    #[test]
    fn stick_drift_inside_the_dead_zone_is_no_input() {
        let drift = Reading {
            packet: 2,
            left_thumb: (3_000, -2_500),
            right_thumb: (-4_000, 4_000),
            left_trigger: TRIGGER_THRESHOLD,
            ..Reading::default()
        };
        assert!(!is_input(Some(&at_rest(1)), Some(&drift)));
    }

    #[test]
    fn a_stick_or_trigger_past_the_dead_zone_is_input() {
        let readings = [
            Reading {
                left_thumb: (7_000, 7_000),
                ..at_rest(2)
            },
            Reading {
                right_thumb: (0, -8_690),
                ..at_rest(2)
            },
            Reading {
                left_trigger: TRIGGER_THRESHOLD + 1,
                ..at_rest(2)
            },
            Reading {
                right_trigger: u8::MAX,
                ..at_rest(2)
            },
            Reading {
                left_thumb: (i16::MIN, i16::MIN),
                ..at_rest(2)
            },
        ];
        for reading in readings {
            assert!(is_input(Some(&at_rest(1)), Some(&reading)), "{reading:?}");
            // letting go counts as well
            assert!(is_input(Some(&reading), Some(&at_rest(3))), "{reading:?}");
        }
    }

    #[test]
    fn connecting_is_input_and_disconnecting_is_not() {
        assert!(is_input(None, Some(&at_rest(1))));
        assert!(!is_input(Some(&at_rest(1)), None));
        assert!(!is_input(None, None));
    }

    #[test]
    fn the_first_poll_is_only_a_baseline() {
        let mut tracker = Tracker::default();
        let pressed = Reading {
            buttons: A_BUTTON,
            ..at_rest(9)
        };
        assert!(!tracker.update(only(0, pressed)));
        assert!(!tracker.update(only(0, pressed)));
        assert!(tracker.update(only(0, at_rest(10))));
        assert!(!tracker.update(only(0, at_rest(10))));
    }

    #[test]
    fn any_slot_counts() {
        let mut tracker = Tracker::default();
        assert!(!tracker.update([None; SLOT_COUNT]));
        assert!(tracker.update(only(3, at_rest(1))), "plugged into slot 4");
        let mut both = only(3, at_rest(1));
        both[1] = Some(at_rest(5));
        assert!(tracker.update(both), "a second controller switched on");
        let mut moved = both;
        moved[1] = Some(Reading {
            buttons: A_BUTTON,
            ..at_rest(6)
        });
        assert!(tracker.update(moved));
        assert!(!tracker.update(moved));
    }

    /// Runs the polling schedule against a controller in slot 0 whose A
    /// button is held from `start` for `length`, and says whether a poll saw
    /// it. The thread's waits are replaced by a clock that jumps ahead.
    fn tap_is_seen(start: Duration, length: Duration) -> bool {
        let origin = Instant::now();
        let controller = |at: Duration| {
            if at < start {
                at_rest(1)
            } else if at < start + length {
                Reading {
                    buttons: A_BUTTON,
                    ..at_rest(2)
                }
            } else {
                at_rest(3)
            }
        };
        let mut tracker = Tracker::default();
        let mut now = Duration::ZERO;
        let mut seen = false;
        while now < start + length + SCAN_INTERVAL {
            let wanted = tracker.slots_to_read(origin + now);
            let slots =
                std::array::from_fn(|index| (index == 0 && wanted[0]).then(|| controller(now)));
            seen |= tracker.update(slots);
            now += tracker.wait();
        }
        seen
    }

    #[test]
    fn a_quick_tap_between_two_seconds_is_seen() {
        const QUICK_TAP: Duration = Duration::from_millis(60);
        for offset_ms in (0..2_000).step_by(7) {
            let start = Duration::from_millis(1_000 + offset_ms);
            assert!(tap_is_seen(start, QUICK_TAP), "tap at {start:?}");
        }
    }

    #[test]
    fn empty_slots_are_asked_only_once_a_second() {
        let origin = Instant::now();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.slots_to_read(origin), [true; SLOT_COUNT]);
        tracker.update(only(2, at_rest(1)));
        assert_eq!(tracker.wait(), ACTIVE_POLL_INTERVAL);
        let soon = origin + ACTIVE_POLL_INTERVAL;
        assert_eq!(tracker.slots_to_read(soon), [false, false, true, false]);
        assert_eq!(
            tracker.slots_to_read(origin + SCAN_INTERVAL),
            [true; SLOT_COUNT]
        );
        tracker.update([None; SLOT_COUNT]);
        assert_eq!(tracker.wait(), SCAN_INTERVAL, "nothing connected");
    }

    #[test]
    fn no_controller_input_reports_no_controller_idle_time() {
        let watcher = Watcher {
            _stop: std::sync::mpsc::channel().0,
            last_input: std::sync::Arc::new(std::sync::Mutex::new(None)),
        };
        assert_eq!(watcher.idle_seconds(), None);
        if let Ok(mut last) = watcher.last_input.lock() {
            *last = Some(Instant::now());
        }
        assert_eq!(watcher.idle_seconds(), Some(0));
    }
}
