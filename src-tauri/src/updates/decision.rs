//! When to act on a release that was found: the three modes, "Later" and the
//! idle rule of the `auto` mode. No I/O, so every case has a test.

use super::policy::same_version;

/// "Later" postpones the question by a day.
pub const LATER_SECS: u64 = 24 * 60 * 60;
/// The `auto` mode installs only after this long without keyboard or mouse
/// input.
pub const AUTO_IDLE_SECS: u64 = 5 * 60;
/// After an update that began with the window hidden, the restarted app stays
/// hidden only when it starts this soon after the installer ran.
const HIDDEN_RESTART_SECS: u64 = 10 * 60;

/// The time until which "Later", pressed at `now`, holds. Unix seconds.
pub fn later_deadline(now: u64) -> u64 {
    now.saturating_add(LATER_SECS)
}

/// Whether "Later" still holds at `now`. A deadline more than a day ahead can
/// only come from a clock that was set back or from an edited settings file,
/// and does not hold.
pub fn is_postponed(later_until: u64, now: u64) -> bool {
    later_until > now && later_until - now <= LATER_SECS
}

/// Whether the app, restarted by the installer, keeps its window hidden: the
/// update began with the window hidden, it was applied, and this is the start
/// right after it. A start by hand later on shows the window as usual.
pub fn stays_hidden_after_update(
    hidden: bool,
    updated_to: &str,
    running: &str,
    started_at: u64,
    now: u64,
) -> bool {
    hidden
        && same_version(updated_to, running)
        && now >= started_at
        && now - started_at <= HIDDEN_RESTART_SECS
}

/// What happens when a newer release is found (Settings > Updates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// Install without asking, while the computer is idle.
    Auto,
    /// Offer "Install now" and "Later".
    Ask,
    /// Tell and link to the release page. Nothing is downloaded.
    Notify,
}

impl UpdateMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "ask" => Some(Self::Ask),
            "notify" => Some(Self::Notify),
            _ => None,
        }
    }

    /// The mode stored in the settings. A value Deskmate does not know, from
    /// an edited file, asks first: it never widens what happens by itself.
    pub fn from_config(value: &str) -> Self {
        Self::parse(value).unwrap_or(Self::Ask)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
            Self::Notify => "notify",
        }
    }
}

/// What the app knows when it decides what to do about a release it found.
#[derive(Debug, Clone, Copy)]
pub struct Situation {
    pub mode: UpdateMode,
    /// A signed installer for this computer is on offer. False: only the
    /// version number is known.
    pub installable: bool,
    /// This version was already announced with a notification.
    pub announced: bool,
    /// "Later" still holds.
    pub postponed: bool,
    /// A download or an install is running.
    pub in_progress: bool,
    /// An install of this version already failed, in this run or before the
    /// last restart.
    pub failed: bool,
    /// Seconds without keyboard or mouse input.
    pub idle_secs: u64,
    /// Something runs full screen: a film, a game, a presentation.
    pub presenting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// One notification that links to the release page.
    Tell,
    /// One notification with "Install now" and "Later".
    Ask,
    /// Download, verify and install now.
    Install,
}

/// Whether the `auto` mode will install this release by itself as soon as the
/// computer is idle. Not after "Later", and not a version that already
/// failed: that one waits for a click.
pub fn installs_when_idle(situation: &Situation) -> bool {
    situation.installable
        && situation.mode == UpdateMode::Auto
        && !situation.postponed
        && !situation.failed
}

pub fn next_step(situation: &Situation) -> Step {
    if situation.in_progress {
        return Step::Nothing;
    }
    if !situation.installable || situation.mode == UpdateMode::Notify {
        // "Later" can come from a notification shown while the release was
        // still installable, and it holds here too.
        return if situation.announced || situation.postponed {
            Step::Nothing
        } else {
            Step::Tell
        };
    }
    if situation.postponed {
        return Step::Nothing;
    }
    if installs_when_idle(situation) {
        return if situation.idle_secs >= AUTO_IDLE_SECS && !situation.presenting {
            Step::Install
        } else {
            Step::Nothing
        };
    }
    if situation.announced {
        Step::Nothing
    } else {
        Step::Ask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_holds_until_the_next_day() {
        let now = 1_800_000_000;
        let until = later_deadline(now);
        assert_eq!(until, now + 24 * 60 * 60);
        assert!(is_postponed(until, now));
        assert!(is_postponed(until, until - 1));
        assert!(!is_postponed(until, until));
        assert!(!is_postponed(until, until + 1));
    }

    #[test]
    fn later_does_not_hold_without_a_click_or_with_a_wrong_clock() {
        let now = 1_800_000_000;
        assert!(!is_postponed(0, now));
        // The clock was set back, or the file was edited: more than a day ahead.
        assert!(!is_postponed(now + LATER_SECS + 1, now));
        assert!(!is_postponed(u64::MAX, now));
        assert_eq!(later_deadline(u64::MAX), u64::MAX);
    }

    #[test]
    fn the_restart_stays_hidden_only_right_after_a_hidden_update() {
        let started = 1_800_000_000;
        assert!(stays_hidden_after_update(
            true,
            "0.9.0",
            "0.9.0",
            started,
            started + 30
        ));
        // The window was open when the update began.
        assert!(!stays_hidden_after_update(
            false,
            "0.9.0",
            "0.9.0",
            started,
            started + 30
        ));
        // The installer did not go through: the old version is still running.
        assert!(!stays_hidden_after_update(
            true,
            "0.9.0",
            "0.8.5",
            started,
            started + 30
        ));
        // Started by hand much later, or with a clock that went backwards.
        assert!(!stays_hidden_after_update(
            true,
            "0.9.0",
            "0.9.0",
            started,
            started + HIDDEN_RESTART_SECS + 1
        ));
        assert!(!stays_hidden_after_update(
            true,
            "0.9.0",
            "0.9.0",
            started,
            started - 1
        ));
    }

    #[test]
    fn the_stored_mode_is_read_strictly() {
        assert_eq!(UpdateMode::parse("auto"), Some(UpdateMode::Auto));
        assert_eq!(UpdateMode::parse("ask"), Some(UpdateMode::Ask));
        assert_eq!(UpdateMode::parse("notify"), Some(UpdateMode::Notify));
        for unknown in ["", "Auto", "automatic", "auto ", "install"] {
            assert_eq!(UpdateMode::parse(unknown), None, "{unknown:?}");
            assert_eq!(UpdateMode::from_config(unknown), UpdateMode::Ask);
        }
        for mode in [UpdateMode::Auto, UpdateMode::Ask, UpdateMode::Notify] {
            assert_eq!(UpdateMode::parse(mode.as_str()), Some(mode));
        }
    }

    fn situation(mode: UpdateMode) -> Situation {
        Situation {
            mode,
            installable: true,
            announced: false,
            postponed: false,
            in_progress: false,
            failed: false,
            idle_secs: 0,
            presenting: false,
        }
    }

    #[test]
    fn notify_tells_once_and_installs_nothing() {
        let mut found = situation(UpdateMode::Notify);
        found.idle_secs = AUTO_IDLE_SECS * 10;
        assert_eq!(next_step(&found), Step::Tell);
        found.announced = true;
        assert_eq!(next_step(&found), Step::Nothing);
        assert!(!installs_when_idle(&found));
    }

    #[test]
    fn ask_asks_once_and_again_after_later() {
        let mut found = situation(UpdateMode::Ask);
        found.idle_secs = AUTO_IDLE_SECS * 10;
        assert_eq!(next_step(&found), Step::Ask);
        found.announced = true;
        assert_eq!(next_step(&found), Step::Nothing);

        // "Later": nothing for a day, then the question comes back.
        found.announced = false;
        found.postponed = true;
        assert_eq!(next_step(&found), Step::Nothing);
        found.postponed = false;
        assert_eq!(next_step(&found), Step::Ask);
    }

    #[test]
    fn auto_waits_until_the_computer_is_idle() {
        let mut found = situation(UpdateMode::Auto);
        assert!(installs_when_idle(&found));
        for busy in [0, 1, 60, AUTO_IDLE_SECS - 1] {
            found.idle_secs = busy;
            assert_eq!(next_step(&found), Step::Nothing, "{busy}");
        }
        for idle in [AUTO_IDLE_SECS, AUTO_IDLE_SECS + 1, 86_400] {
            found.idle_secs = idle;
            assert_eq!(next_step(&found), Step::Install, "{idle}");
        }
    }

    #[test]
    fn auto_does_not_interrupt_a_film_or_a_presentation() {
        let mut found = situation(UpdateMode::Auto);
        found.idle_secs = AUTO_IDLE_SECS * 2;
        found.presenting = true;
        assert_eq!(next_step(&found), Step::Nothing);
        found.presenting = false;
        assert_eq!(next_step(&found), Step::Install);
    }

    #[test]
    fn auto_respects_later() {
        let mut found = situation(UpdateMode::Auto);
        found.idle_secs = AUTO_IDLE_SECS * 2;
        found.postponed = true;
        assert_eq!(next_step(&found), Step::Nothing);
        assert!(!installs_when_idle(&found));
    }

    #[test]
    fn auto_does_not_retry_a_failed_version_by_itself() {
        let mut found = situation(UpdateMode::Auto);
        found.idle_secs = AUTO_IDLE_SECS * 2;
        found.failed = true;
        assert!(!installs_when_idle(&found));
        assert_eq!(next_step(&found), Step::Ask);
        found.announced = true;
        assert_eq!(next_step(&found), Step::Nothing);
    }

    #[test]
    fn nothing_starts_while_an_install_is_running() {
        for mode in [UpdateMode::Auto, UpdateMode::Ask, UpdateMode::Notify] {
            let mut found = situation(mode);
            found.idle_secs = AUTO_IDLE_SECS * 2;
            found.in_progress = true;
            assert_eq!(next_step(&found), Step::Nothing);
        }
    }

    #[test]
    fn a_release_without_an_installer_is_only_announced() {
        for mode in [UpdateMode::Auto, UpdateMode::Ask, UpdateMode::Notify] {
            let mut found = situation(mode);
            found.installable = false;
            found.idle_secs = AUTO_IDLE_SECS * 2;
            assert_eq!(next_step(&found), Step::Tell);
            assert!(!installs_when_idle(&found));
            found.announced = true;
            assert_eq!(next_step(&found), Step::Nothing);
        }
    }

    #[test]
    fn later_holds_for_a_release_that_is_only_announced() {
        // "Later" on an old notification forgets the announcement, after the
        // installer was dropped or the mode became `notify`.
        let link_only = [UpdateMode::Auto, UpdateMode::Ask, UpdateMode::Notify]
            .into_iter()
            .map(|mode| Situation {
                installable: false,
                ..situation(mode)
            });
        for mut found in link_only.chain([situation(UpdateMode::Notify)]) {
            found.postponed = true;
            assert_eq!(next_step(&found), Step::Nothing);
            found.postponed = false;
            assert_eq!(next_step(&found), Step::Tell);
        }
    }
}
