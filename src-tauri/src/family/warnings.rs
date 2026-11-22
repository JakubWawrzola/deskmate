//! When the "time is almost up" notifications appear: how many minutes
//! before the limit or the end of the allowed hours. The list comes from Home
//! Assistant or the Family page; an empty one means the built-in times.

/// The times used when none are set.
pub const DEFAULT_WARN_AT_MIN: [u32; 3] = [10, 5, 1];
/// Most warnings one list may hold.
pub const MAX_WARNINGS: usize = 5;
/// The earliest a warning may come, in minutes before the stop.
pub const MAX_WARN_AT_MIN: u32 = 120;
/// Longest text Home Assistant may send for the list.
pub const MAX_TEXT_LEN: usize = 40;
/// What Home Assistant accepts in the text entity: empty, or up to five
/// numbers of at most three digits, separated by commas.
pub const TEXT_PATTERN: &str = r"^$|^ *[0-9]{1,3}( *, *[0-9]{1,3}){0,4} *$";

/// "15, 5,2" -> [15, 5, 2]. Empty text gives an empty list, which stands for
/// the built-in times. The result is sorted from the earliest warning to the
/// last, without repeats.
pub fn parse(text: &str) -> Result<Vec<u32>, String> {
    let mut minutes = Vec::new();
    for part in text.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let value: u32 = part
            .parse()
            .map_err(|_| format!("not a number of minutes: {part}"))?;
        minutes.push(value);
    }
    validate(minutes)
}

/// Checks a list and puts it in order.
pub fn validate(mut minutes: Vec<u32>) -> Result<Vec<u32>, String> {
    if let Some(bad) = minutes
        .iter()
        .find(|&&value| value == 0 || value > MAX_WARN_AT_MIN)
    {
        return Err(format!(
            "a warning comes 1 to {MAX_WARN_AT_MIN} minutes before, not {bad}"
        ));
    }
    minutes.sort_unstable_by(|a, b| b.cmp(a));
    minutes.dedup();
    if minutes.len() > MAX_WARNINGS {
        return Err(format!("at most {MAX_WARNINGS} warnings"));
    }
    Ok(minutes)
}

/// The times in force: the stored ones, or the built-in ones when none are
/// stored.
pub fn in_force(stored: &[u32]) -> &[u32] {
    if stored.is_empty() {
        &DEFAULT_WARN_AT_MIN
    } else {
        stored
    }
}

/// "15, 5, 2", the way the text entity and the Family page show the list.
pub fn as_text(stored: &[u32]) -> String {
    in_force(stored)
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a warning is due with `minutes_left` to the stop. Every time that
/// has already passed is marked as shown in `warned`, so a limit that was
/// lowered gives one notification, not one per time skipped.
pub fn due(warn_at: &[u32], minutes_left: u32, warned: &mut Vec<u32>) -> bool {
    let passed: Vec<u32> = warn_at
        .iter()
        .copied()
        .filter(|&at| minutes_left <= at && !warned.contains(&at))
        .collect();
    warned.extend(&passed);
    !passed.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_list_and_puts_it_in_order() {
        assert_eq!(parse("15,5,2").unwrap(), [15, 5, 2]);
        assert_eq!(parse(" 2 ; 15, 5 ").unwrap(), [15, 5, 2]);
        assert_eq!(parse("5, 5, 5").unwrap(), [5]);
        assert_eq!(parse("120").unwrap(), [120]);
    }

    #[test]
    fn empty_text_stands_for_the_built_in_times() {
        assert_eq!(parse("").unwrap(), Vec::<u32>::new());
        assert_eq!(parse(" , ").unwrap(), Vec::<u32>::new());
        assert_eq!(in_force(&[]), DEFAULT_WARN_AT_MIN);
        assert_eq!(as_text(&[]), "10, 5, 1");
        assert_eq!(as_text(&[15, 5, 2]), "15, 5, 2");
    }

    #[test]
    fn refuses_what_is_not_a_list_of_minutes() {
        for bad in ["soon", "5 min", "0", "121", "-5", "1.5", "1,2,3,4,5,6"] {
            assert!(parse(bad).is_err(), "{bad} was accepted");
        }
        assert!(validate(vec![10, 0]).is_err());
        assert!(validate(vec![1, 2, 3, 4, 5]).is_ok());
    }

    #[test]
    fn the_pattern_for_home_assistant_matches_what_parse_accepts() {
        // The pattern is checked by Home Assistant, not here; this keeps its
        // shape and the limits in step with `parse`.
        assert!(TEXT_PATTERN.contains("{1,3}"));
        assert!(TEXT_PATTERN.contains(&format!("{{0,{}}}", MAX_WARNINGS - 1)));
        assert!("120, 120, 120, 120, 120".len() <= MAX_TEXT_LEN);
    }

    #[test]
    fn each_time_warns_once() {
        let warn_at = [15, 5, 2];
        let mut warned = Vec::new();
        assert!(!due(&warn_at, 16, &mut warned));
        assert!(due(&warn_at, 15, &mut warned));
        assert!(!due(&warn_at, 14, &mut warned));
        assert!(!due(&warn_at, 6, &mut warned));
        assert!(due(&warn_at, 5, &mut warned));
        assert!(due(&warn_at, 2, &mut warned));
        assert!(!due(&warn_at, 1, &mut warned));
        assert_eq!(warned, [15, 5, 2]);
    }

    #[test]
    fn a_lowered_limit_warns_once_for_every_time_skipped() {
        let mut warned = Vec::new();
        assert!(due(&DEFAULT_WARN_AT_MIN, 3, &mut warned));
        assert_eq!(warned, [10, 5]);
        assert!(!due(&DEFAULT_WARN_AT_MIN, 2, &mut warned));
        assert!(due(&DEFAULT_WARN_AT_MIN, 1, &mut warned));
    }
}
