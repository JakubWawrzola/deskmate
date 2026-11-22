//! Extra screen time without Home Assistant: a parent at the computer types
//! an administrator password into the Windows UAC prompt, and the child gets
//! fifteen minutes more.
//!
//! Deskmate never sees the password. The button starts Deskmate again with
//! elevation; Windows asks for the password on its secure desktop, and only
//! the elevated copy can write the grant, under HKEY_LOCAL_MACHINE, where a
//! standard account can read but not write. The elevated copy takes nothing
//! from the caller but the account: the size of a grant and the most per day
//! are constants here, so there is no number for a child to change.

use std::sync::atomic::{AtomicBool, Ordering};

/// Starts the elevated step: `--family-extra-time <account SID>`.
pub const ARG: &str = "--family-extra-time";
/// The notification button for it. A notification from Home Assistant may
/// not carry this action (notify.rs).
pub const ACTION: &str = "screen_time_local";
/// Minutes one approval adds.
pub const GRANT_MIN: u32 = 15;
/// Most minutes that can be added this way in one day.
pub const DAILY_MAX_MIN: u32 = 60;
#[cfg(windows)]
const KEY: &str = r"SOFTWARE\Deskmate\Family\LocalTime";

/// True while the UAC prompt is open. The rules do not lock the computer
/// under a parent who is typing the password.
static PENDING: AtomicBool = AtomicBool::new(false);

pub fn is_pending() -> bool {
    PENDING.load(Ordering::Relaxed)
}

/// What is stored for one account: the day, and the minutes added on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Granted {
    /// Local date, YYYY-MM-DD.
    pub date: String,
    pub minutes: u32,
}

/// The minutes that count today. A grant from another day is worth nothing,
/// and nothing counts beyond the daily maximum, whatever is stored.
pub fn minutes_today(stored: &Granted, today: &str) -> u32 {
    if stored.date == today {
        stored.minutes.min(DAILY_MAX_MIN)
    } else {
        0
    }
}

/// What to store after one more approval, or why there is none to give.
pub fn after_grant(stored: &Granted, today: &str) -> Result<Granted, String> {
    if !is_date(today) {
        return Err("cannot read today's date".into());
    }
    let minutes = minutes_today(stored, today) + GRANT_MIN;
    if minutes > DAILY_MAX_MIN {
        return Err(format!(
            "the {DAILY_MAX_MIN} extra minutes for today are used up"
        ));
    }
    Ok(Granted {
        date: today.to_string(),
        minutes,
    })
}

/// "2026-11-22".
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// What is stored for the account `sid`. Any account may read it.
#[cfg(windows)]
pub fn read(sid: &str) -> Granted {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(format!(r"{KEY}\{sid}")) else {
        return Granted::default();
    };
    Granted {
        date: key.get_value("Date").unwrap_or_default(),
        minutes: key.get_value("Minutes").unwrap_or_default(),
    }
}

#[cfg(not(windows))]
pub fn read(_sid: &str) -> Granted {
    Granted::default()
}

/// The elevated step: adds one grant for the account `sid` on `today`. Only
/// a process with administrator rights can write the key.
#[cfg(windows)]
pub fn grant_elevated(sid: &str, today: &str) -> Result<(), String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    if !super::protection::valid_sid(sid) {
        return Err("not a Windows account".into());
    }
    let granted = after_grant(&read(sid), today)?;
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey(format!(r"{KEY}\{sid}"))
        .map_err(|e| format!("cannot write the extra time: {e}"))?;
    key.set_value("Date", &granted.date)
        .and_then(|()| key.set_value("Minutes", &granted.minutes))
        .map_err(|e| format!("cannot write the extra time: {e}"))
}

#[cfg(not(windows))]
pub fn grant_elevated(_sid: &str, _today: &str) -> Result<(), String> {
    Err("windows only".into())
}

/// Asks Windows for the administrator password and waits for the elevated
/// step. Blocking: the prompt stays open until someone answers it.
#[cfg(windows)]
pub fn request(sid: &str) -> Result<(), String> {
    // Resets the flag on every way out, a panic included.
    struct Pending;
    impl Drop for Pending {
        fn drop(&mut self) {
            PENDING.store(false, Ordering::Relaxed);
        }
    }
    if PENDING.swap(true, Ordering::Relaxed) {
        return Err("the administrator prompt is already open".into());
    }
    let _pending = Pending;
    crate::cpu_temperature::run_elevated(&format!("{ARG} {sid}"))
}

#[cfg(not(windows))]
pub fn request(_sid: &str) -> Result<(), String> {
    Err("windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-22";

    fn stored(date: &str, minutes: u32) -> Granted {
        Granted {
            date: date.into(),
            minutes,
        }
    }

    #[test]
    fn each_approval_adds_fifteen_minutes() {
        let first = after_grant(&Granted::default(), TODAY).unwrap();
        assert_eq!(first, stored(TODAY, 15));
        let second = after_grant(&first, TODAY).unwrap();
        assert_eq!(second, stored(TODAY, 30));
    }

    #[test]
    fn the_day_has_a_maximum() {
        assert_eq!(
            after_grant(&stored(TODAY, 45), TODAY).unwrap(),
            stored(TODAY, 60)
        );
        assert!(after_grant(&stored(TODAY, 60), TODAY).is_err());
        // More than the maximum in the store, however it got there.
        assert!(after_grant(&stored(TODAY, 5000), TODAY).is_err());
        assert_eq!(minutes_today(&stored(TODAY, 5000), TODAY), DAILY_MAX_MIN);
    }

    #[test]
    fn a_new_day_starts_from_nothing() {
        let yesterday = stored("2026-11-21", 60);
        assert_eq!(minutes_today(&yesterday, TODAY), 0);
        assert_eq!(after_grant(&yesterday, TODAY).unwrap(), stored(TODAY, 15));
        // A date in the future is another day too.
        assert_eq!(minutes_today(&stored("2026-11-23", 30), TODAY), 0);
    }

    #[test]
    fn nothing_is_granted_without_a_date() {
        for bad in [
            "",
            "today",
            "2026-11-2",
            "2026/11/22",
            "2026-11-22 ",
            "x026-11-22",
        ] {
            assert!(after_grant(&Granted::default(), bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_prompt_is_not_open_by_default() {
        assert!(!is_pending());
    }

    /// Reads the real registry of the computer the test runs on: an account
    /// that does not exist has no grant.
    #[test]
    fn an_account_without_a_grant_reads_as_nothing() {
        assert_eq!(read("S-1-5-21-0-0-0-999999"), Granted::default());
    }
}
