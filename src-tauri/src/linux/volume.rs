//! Volume and mute of the default playback device, through `wpctl`
//! (PipeWire) or else `pactl` (PulseAudio, and PipeWire's PulseAudio
//! server). Both are run with fixed arguments; the only value from Home
//! Assistant is the volume, parsed and clamped to 0-100 before it is written
//! into an argument as a number.

use super::{find_program, run_fixed};

/// The tool this computer has, looked up on every call: a sound server can
/// be installed while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Wpctl,
    Pactl,
}

const WPCTL_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const PACTL_SINK: &str = "@DEFAULT_SINK@";

fn tool() -> Option<(Tool, std::path::PathBuf)> {
    if let Some(path) = find_program("wpctl") {
        return Some((Tool::Wpctl, path));
    }
    find_program("pactl").map(|path| (Tool::Pactl, path))
}

/// Whether volume and mute can be read and set here.
pub fn available() -> bool {
    tool().is_some()
}

/// The volume of the default playback device in percent.
pub fn get() -> Option<u8> {
    let (tool, path) = tool()?;
    match tool {
        Tool::Wpctl => parse_wpctl(&run_fixed(&path, &["get-volume", WPCTL_SINK]).ok()?)
            .map(|(level, _)| level),
        Tool::Pactl => {
            parse_pactl_volume(&run_fixed(&path, &["get-sink-volume", PACTL_SINK]).ok()?)
        }
    }
}

/// Whether the default playback device is muted.
pub fn muted() -> Option<bool> {
    let (tool, path) = tool()?;
    match tool {
        Tool::Wpctl => parse_wpctl(&run_fixed(&path, &["get-volume", WPCTL_SINK]).ok()?)
            .map(|(_, muted)| muted),
        Tool::Pactl => parse_pactl_mute(&run_fixed(&path, &["get-sink-mute", PACTL_SINK]).ok()?),
    }
}

/// Sets the volume from Home Assistant's payload, a number from 0 to 100.
pub fn set(payload: &str) -> Result<(), String> {
    let percent = parse_percent(payload)?;
    let (tool, path) = tool().ok_or(NO_TOOL)?;
    match tool {
        Tool::Wpctl => {
            let level = format!("{:.2}", f32::from(percent) / 100.0);
            run_fixed(&path, &["set-volume", WPCTL_SINK, &level])
        }
        Tool::Pactl => {
            let level = format!("{percent}%");
            run_fixed(&path, &["set-sink-volume", PACTL_SINK, &level])
        }
    }
    .map(|_| ())
}

/// Mutes or unmutes the default playback device.
pub fn set_muted(muted: bool) -> Result<(), String> {
    let (tool, path) = tool().ok_or(NO_TOOL)?;
    let flag = if muted { "1" } else { "0" };
    match tool {
        Tool::Wpctl => run_fixed(&path, &["set-mute", WPCTL_SINK, flag]),
        Tool::Pactl => run_fixed(&path, &["set-sink-mute", PACTL_SINK, flag]),
    }
    .map(|_| ())
}

const NO_TOOL: &str = "neither wpctl nor pactl is installed";

/// "40", " 40.6 " -> 40, 41; anything else is refused.
fn parse_percent(payload: &str) -> Result<u8, String> {
    let value: f32 = payload
        .trim()
        .parse()
        .map_err(|_| "volume: not a number".to_string())?;
    if !value.is_finite() {
        return Err("volume: not a number".into());
    }
    // In range after the clamp, so the cast cannot truncate.
    Ok(value.round().clamp(0.0, 100.0) as u8)
}

/// "Volume: 0.40" or "Volume: 0.40 [MUTED]" -> (40, muted).
fn parse_wpctl(output: &str) -> Option<(u8, bool)> {
    let rest = output.trim().strip_prefix("Volume:")?.trim();
    let level: f32 = rest.split_whitespace().next()?.parse().ok()?;
    if !level.is_finite() {
        return None;
    }
    let percent = (level * 100.0).round().clamp(0.0, 100.0) as u8;
    Some((percent, rest.contains("[MUTED]")))
}

/// The first "NN%" of `pactl get-sink-volume`.
fn parse_pactl_volume(output: &str) -> Option<u8> {
    let percent: u32 = output
        .split_whitespace()
        .find_map(|word| word.strip_suffix('%'))?
        .parse()
        .ok()?;
    u8::try_from(percent.min(100)).ok()
}

/// "Mute: yes" / "Mute: no".
fn parse_pactl_mute(output: &str) -> Option<bool> {
    match output.trim().strip_prefix("Mute:")?.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_volume_and_mute_from_wpctl() {
        assert_eq!(parse_wpctl("Volume: 0.40\n"), Some((40, false)));
        assert_eq!(parse_wpctl("Volume: 0.07 [MUTED]\n"), Some((7, true)));
        // PipeWire allows more than 100 %; Home Assistant's slider ends there.
        assert_eq!(parse_wpctl("Volume: 1.50"), Some((100, false)));
        assert_eq!(parse_wpctl("Volume: nan"), None);
        assert_eq!(parse_wpctl("Error"), None);
    }

    #[test]
    fn reads_the_volume_and_mute_from_pactl() {
        let output = "Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB\n        balance 0.00\n";
        assert_eq!(parse_pactl_volume(output), Some(40));
        assert_eq!(parse_pactl_volume("Volume: mono: 98304 / 150%"), Some(100));
        assert_eq!(parse_pactl_volume("no sink"), None);
        assert_eq!(parse_pactl_mute("Mute: yes\n"), Some(true));
        assert_eq!(parse_pactl_mute("Mute: no"), Some(false));
        assert_eq!(parse_pactl_mute("Stumm: nein"), None);
    }

    #[test]
    fn the_volume_from_home_assistant_is_a_clamped_number() {
        assert_eq!(parse_percent("40"), Ok(40));
        assert_eq!(parse_percent(" 40.6 "), Ok(41));
        assert_eq!(parse_percent("250"), Ok(100));
        assert_eq!(parse_percent("-3"), Ok(0));
        assert!(parse_percent("40; rm -rf /").is_err());
        assert!(parse_percent("NaN").is_err());
        assert!(parse_percent("inf").is_err());
        assert!(parse_percent("").is_err());
    }
}
