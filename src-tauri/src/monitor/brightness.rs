//! The "Screen brightness" number, 0 to 100 percent.
//!
//! A laptop's built-in panel is set through WMI (WmiMonitorBrightness to read,
//! WmiMonitorBrightnessMethods.WmiSetBrightness to set). External monitors are
//! set over DDC/CI (GetMonitorBrightness and SetMonitorBrightness on the
//! physical monitors of each display). The number shows the first display
//! that answers, the built-in panel before external monitors, and a new value
//! goes to every display that takes it.
//!
//! Many monitors do not speak DDC/CI, or have it switched off in their own
//! menu. The number is declared only while at least one display answers, so
//! a desktop PC without such a monitor gets no entity that does nothing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub use deskmate_core::sensors::BRIGHTNESS_KEY as KEY;

/// Sensor ticks in a row without any display answering before the number is
/// withdrawn. A DDC/CI monitor misses a reading now and then, and the entity
/// should not come and go with every miss.
const MISSES_BEFORE_WITHDRAWN: u8 = 3;

/// The highest brightness, in percent.
const MAX_PERCENT: u8 = 100;

/// At least one display answered recently: the number is declared.
static SUPPORTED: AtomicBool = AtomicBool::new(false);
/// Set when SUPPORTED changed; the entities are declared again.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Whether a display can be read and set right now.
pub fn is_supported() -> bool {
    SUPPORTED.load(Ordering::Relaxed)
}

/// True once after the support changed since the last call.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

fn remember_supported(supported: bool) {
    if SUPPORTED.swap(supported, Ordering::Relaxed) != supported {
        CHANGED.store(true, Ordering::Relaxed);
    }
}

/// One display whose brightness a program may be able to read and set.
pub(super) trait Display {
    /// The brightness in percent, None when the display does not answer.
    fn read(&mut self) -> Option<u8>;
    /// Sets the brightness in percent (0 to 100).
    fn write(&mut self, percent: u8) -> Result<(), String>;
}

/// The brightness of the first display that answers.
pub(super) fn read_first(displays: &mut [Box<dyn Display>]) -> Option<u8> {
    displays.iter_mut().find_map(|display| display.read())
}

/// Sets every display. Succeeds when at least one display took the value.
pub(super) fn write_all(displays: &mut [Box<dyn Display>], percent: u8) -> Result<(), String> {
    let mut written = 0usize;
    let mut errors = Vec::new();
    for display in displays.iter_mut() {
        match display.write(percent) {
            Ok(()) => written += 1,
            Err(error) => errors.push(error),
        }
    }
    if written > 0 {
        return Ok(());
    }
    if errors.is_empty() {
        return Err("no display whose brightness can be set".into());
    }
    Err(format!(
        "no display took the brightness: {}",
        errors.join("; ")
    ))
}

/// A brightness value from Home Assistant: a number from 0 to 100, rounded
/// to a whole percent. Anything else is refused.
pub fn parse_percent(payload: &str) -> Result<u8, String> {
    let value: f64 = payload
        .trim()
        .parse()
        .map_err(|_| "brightness: not a number".to_string())?;
    if !value.is_finite() || !(0.0..=f64::from(MAX_PERCENT)).contains(&value) {
        return Err(format!("brightness: must be between 0 and {MAX_PERCENT}"));
    }
    // In range 0..=100 after the check above, so the cast cannot truncate.
    Ok(value.round() as u8)
}

/// A DDC/CI reading as a percentage of the monitor's own range. None for a
/// monitor that did not answer or reported a range that makes no sense.
pub(super) fn percent_from_ddc(answered: bool, min: u32, current: u32, max: u32) -> Option<u8> {
    if !answered || max <= min || current < min || current > max {
        return None;
    }
    let span = u64::from(max - min);
    let percent = (u64::from(current - min) * u64::from(MAX_PERCENT) + span / 2) / span;
    u8::try_from(percent).ok()
}

/// The DDC/CI value for a percentage of the monitor's range.
pub(super) fn ddc_from_percent(min: u32, max: u32, percent: u8) -> u32 {
    let span = u64::from(max.saturating_sub(min));
    let offset = (span * u64::from(percent.min(MAX_PERCENT)) + u64::from(MAX_PERCENT) / 2)
        / u64::from(MAX_PERCENT);
    // offset is at most span, which fits in u32.
    min.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))
}

/// A WMI CurrentBrightness value: already a percentage. None when missing or
/// out of range.
pub(super) fn percent_from_wmi(current: Option<u32>) -> Option<u8> {
    current
        .filter(|value| *value <= u32::from(MAX_PERCENT))
        .and_then(|value| u8::try_from(value).ok())
}

/// The WMI readings by InstanceName, for pairing them with the method
/// objects of the same panels. Rows with an unusable value are left out.
pub(super) fn readings_by_instance(rows: Vec<(String, Option<u32>)>) -> HashMap<String, u8> {
    rows.into_iter()
        .filter_map(|(instance, current)| Some((instance, percent_from_wmi(current)?)))
        .collect()
}

/// Whether the number is declared, given the readings so far.
#[derive(Debug, Default)]
struct Availability {
    supported: bool,
    misses: u8,
}

impl Availability {
    /// Takes the outcome of one reading and says whether the number should
    /// be declared now.
    fn update(&mut self, answered: bool) -> bool {
        if answered {
            self.supported = true;
            self.misses = 0;
        } else if self.supported {
            self.misses = self.misses.saturating_add(1);
            if self.misses >= MISSES_BEFORE_WITHDRAWN {
                self.supported = false;
                self.misses = 0;
            }
        }
        self.supported
    }
}

/// Reads the brightness on every sensor tick while the number is on.
#[derive(Debug, Default)]
pub struct Collector {
    availability: Availability,
}

impl Collector {
    pub fn collect(
        &mut self,
        cfg: &crate::config::AppConfig,
        out: &mut std::collections::HashMap<String, String>,
    ) {
        if !crate::sensors::is_enabled(cfg, KEY) {
            return;
        }
        let reading = super::brightness_win::with_displays(read_first);
        remember_supported(self.availability.update(reading.is_some()));
        if let Some(percent) = reading {
            out.insert(KEY.into(), percent.to_string());
        }
    }
}

/// Sets the brightness Home Assistant asked for.
pub fn set(payload: &str) -> Result<(), String> {
    let percent = parse_percent(payload)?;
    super::brightness_win::with_displays(|displays| write_all(displays, percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A display that answers with a fixed value, or not at all.
    struct StandIn {
        reading: Option<u8>,
        accepts: bool,
        written: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
    }

    impl StandIn {
        fn boxed(reading: Option<u8>, accepts: bool) -> Box<dyn Display> {
            Box::new(Self {
                reading,
                accepts,
                written: Default::default(),
            })
        }
    }

    impl Display for StandIn {
        fn read(&mut self) -> Option<u8> {
            self.reading
        }
        fn write(&mut self, percent: u8) -> Result<(), String> {
            if !self.accepts {
                return Err("refused".into());
            }
            self.written.borrow_mut().push(percent);
            Ok(())
        }
    }

    #[test]
    fn accepts_whole_and_fractional_percentages() {
        assert_eq!(parse_percent("0"), Ok(0));
        assert_eq!(parse_percent("100"), Ok(100));
        assert_eq!(parse_percent(" 42 "), Ok(42));
        assert_eq!(parse_percent("42.0"), Ok(42));
        assert_eq!(parse_percent("41.5"), Ok(42));
        assert_eq!(parse_percent("99.6"), Ok(100));
    }

    #[test]
    fn refuses_anything_else() {
        for payload in [
            "", " ", "-1", "100.1", "101", "1e3", "-0.5", "NaN", "inf", "-inf", "50%", "fifty",
            "50;100", "PRESS", "ON",
        ] {
            assert!(parse_percent(payload).is_err(), "{payload:?}");
        }
    }

    #[test]
    fn ddc_readings_scale_to_the_monitor_range() {
        assert_eq!(percent_from_ddc(true, 0, 0, 100), Some(0));
        assert_eq!(percent_from_ddc(true, 0, 70, 100), Some(70));
        assert_eq!(percent_from_ddc(true, 0, 100, 100), Some(100));
        assert_eq!(percent_from_ddc(true, 10, 55, 100), Some(50));
        assert_eq!(percent_from_ddc(true, 0, 1, 3), Some(33));
        assert_eq!(percent_from_ddc(true, 0, 255, 255), Some(100));
    }

    #[test]
    fn failed_or_nonsensical_ddc_readings_are_ignored() {
        assert_eq!(percent_from_ddc(false, 0, 50, 100), None);
        assert_eq!(percent_from_ddc(true, 0, 0, 0), None);
        assert_eq!(percent_from_ddc(true, 100, 50, 0), None);
        assert_eq!(percent_from_ddc(true, 10, 5, 100), None);
        assert_eq!(percent_from_ddc(true, 0, 101, 100), None);
        assert_eq!(percent_from_ddc(true, 0, u32::MAX, u32::MAX), Some(100));
    }

    #[test]
    fn percentages_scale_back_to_the_monitor_range() {
        assert_eq!(ddc_from_percent(0, 100, 0), 0);
        assert_eq!(ddc_from_percent(0, 100, 37), 37);
        assert_eq!(ddc_from_percent(0, 100, 100), 100);
        assert_eq!(ddc_from_percent(10, 100, 50), 55);
        assert_eq!(ddc_from_percent(0, 255, 50), 128);
        assert_eq!(ddc_from_percent(0, u32::MAX, 100), u32::MAX);
        // a broken range never goes below its minimum
        assert_eq!(ddc_from_percent(50, 10, 100), 50);
        for percent in 0..=100u8 {
            let raw = ddc_from_percent(0, 255, percent);
            assert_eq!(percent_from_ddc(true, 0, raw, 255), Some(percent));
        }
    }

    #[test]
    fn wmi_readings_are_percentages() {
        assert_eq!(percent_from_wmi(Some(0)), Some(0));
        assert_eq!(percent_from_wmi(Some(65)), Some(65));
        assert_eq!(percent_from_wmi(Some(100)), Some(100));
        assert_eq!(percent_from_wmi(Some(101)), None);
        assert_eq!(percent_from_wmi(Some(u32::MAX)), None);
        assert_eq!(percent_from_wmi(None), None);
    }

    #[test]
    fn wmi_readings_pair_by_instance() {
        let readings = readings_by_instance(vec![
            ("DISPLAY\\PANEL\\1".into(), Some(80)),
            ("DISPLAY\\BROKEN\\1".into(), Some(400)),
            ("DISPLAY\\EMPTY\\1".into(), None),
        ]);
        assert_eq!(readings.get("DISPLAY\\PANEL\\1"), Some(&80));
        assert_eq!(readings.len(), 1);
    }

    #[test]
    fn the_first_display_that_answers_is_shown() {
        let mut displays = vec![
            StandIn::boxed(None, false),
            StandIn::boxed(Some(30), true),
            StandIn::boxed(Some(90), true),
        ];
        assert_eq!(read_first(&mut displays), Some(30));
        assert_eq!(read_first(&mut []), None);
        assert_eq!(
            read_first(&mut [StandIn::boxed(None, true), StandIn::boxed(None, true)]),
            None
        );
    }

    #[test]
    fn a_new_value_goes_to_every_display_that_takes_it() {
        let written: std::rc::Rc<std::cell::RefCell<Vec<u8>>> = Default::default();
        let display = |accepts| -> Box<dyn Display> {
            Box::new(StandIn {
                reading: None,
                accepts,
                written: written.clone(),
            })
        };
        let mut displays = vec![display(true), display(false), display(true)];
        assert_eq!(write_all(&mut displays, 25), Ok(()));
        assert_eq!(*written.borrow(), vec![25, 25]);
    }

    #[test]
    fn setting_fails_when_no_display_takes_it() {
        let mut refusing = vec![StandIn::boxed(Some(50), false), StandIn::boxed(None, false)];
        let error = write_all(&mut refusing, 10).unwrap_err();
        assert!(error.contains("refused"), "{error}");
        assert!(write_all(&mut [], 10).is_err());
    }

    #[test]
    fn the_number_appears_at_once_and_goes_after_repeated_misses() {
        let mut availability = Availability::default();
        assert!(!availability.update(false));
        assert!(availability.update(true));
        for _ in 1..MISSES_BEFORE_WITHDRAWN {
            assert!(availability.update(false));
        }
        assert!(!availability.update(false));
        assert!(!availability.update(false));
        assert!(availability.update(true));
    }

    #[test]
    fn one_answer_resets_the_misses() {
        let mut availability = Availability::default();
        availability.update(true);
        for _ in 0..10 {
            for _ in 1..MISSES_BEFORE_WITHDRAWN {
                assert!(availability.update(false));
            }
            assert!(availability.update(true));
        }
    }
}
