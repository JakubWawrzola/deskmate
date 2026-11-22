//! Reads and sets the volume of single programs through the sound sessions
//! of the default playback device: what the Windows volume mixer shows. The
//! entities are named and described in `deskmate_core::app_volume`.
//!
//! Everything here blocks (COM calls), so callers run it off the async
//! runtime.

#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "the sound sessions of single programs are a Windows mixer feature"
    )
)]

use deskmate_core::app_volume::{mute_key, volume_key, NO_SESSION};
use std::collections::HashMap;

/// The level of one program: several sound sessions of it count as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub percent: u8,
    pub muted: bool,
}

/// The values of the two entities of every listed program. A program that
/// has no sound session right now gets `NO_SESSION` for both.
pub fn collect(programs: &[String], out: &mut HashMap<String, String>) {
    if programs.is_empty() {
        return;
    }
    let levels = levels();
    for program in programs {
        let (volume, muted) = match levels.get(program.as_str()) {
            Some(level) => (
                level.percent.to_string(),
                if level.muted { "ON" } else { "OFF" }.to_string(),
            ),
            None => (NO_SESSION.to_string(), NO_SESSION.to_string()),
        };
        out.insert(volume_key(program), volume);
        out.insert(mute_key(program), muted);
    }
}

/// Several sessions of one program as one level: the loudest volume, and
/// muted only when every session is.
fn combined(sessions: impl IntoIterator<Item = Level>) -> Option<Level> {
    sessions.into_iter().reduce(|total, session| Level {
        percent: total.percent.max(session.percent),
        muted: total.muted && session.muted,
    })
}

/// 0.0 to 1.0 as whole percent.
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "the mixer is read on Windows only")
)]
fn percent_of(scalar: f32) -> u8 {
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// The level of every program that has a sound session, by its file name in
/// lower case.
#[cfg(windows)]
fn levels() -> HashMap<String, Level> {
    let mut by_program: HashMap<String, Vec<Level>> = HashMap::new();
    for (program, volume) in win::sessions().unwrap_or_default() {
        if let Some(level) = win::level(&volume) {
            by_program.entry(program).or_default().push(level);
        }
    }
    by_program
        .into_iter()
        .filter_map(|(program, sessions)| Some((program, combined(sessions)?)))
        .collect()
}

#[cfg(not(windows))]
fn levels() -> HashMap<String, Level> {
    HashMap::new()
}

/// Sets the volume of every sound session of `program`.
#[cfg(windows)]
pub fn set_volume(program: &str, percent: u8) -> Result<(), String> {
    win::for_each_session(program, |volume| win::set_volume(volume, percent))
}

/// Mutes or unmutes every sound session of `program`.
#[cfg(windows)]
pub fn set_muted(program: &str, muted: bool) -> Result<(), String> {
    win::for_each_session(program, |volume| win::set_muted(volume, muted))
}

#[cfg(not(windows))]
pub fn set_volume(_program: &str, _percent: u8) -> Result<(), String> {
    Err("the volume of single programs is not available on this system".into())
}

#[cfg(not(windows))]
pub fn set_muted(_program: &str, _muted: bool) -> Result<(), String> {
    Err("the volume of single programs is not available on this system".into())
}

#[cfg(windows)]
mod win {
    use super::{percent_of, Level};
    use std::collections::HashMap;
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// The sound sessions of the default playback device, each with the file
    /// name of its program in lower case. The session of the system sounds
    /// belongs to no program and is left out.
    pub fn sessions() -> windows::core::Result<Vec<(String, ISimpleAudioVolume)>> {
        let names: HashMap<u32, String> = crate::processes::snapshot().into_iter().collect();
        // SAFETY: COM is initialized on this thread before the first object
        // is made; as elsewhere in the app it stays initialized for the life
        // of the thread. Every call below is a method on a COM object that
        // windows-rs keeps alive and releases, and none takes a pointer from
        // here.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let list = manager.GetSessionEnumerator()?;
            let mut found = Vec::new();
            for index in 0..list.GetCount()? {
                let Ok(session) = list.GetSession(index) else {
                    continue;
                };
                let Ok(pid) = session
                    .cast::<IAudioSessionControl2>()
                    .and_then(|session| session.GetProcessId())
                else {
                    continue;
                };
                let (Some(name), Ok(volume)) =
                    (names.get(&pid), session.cast::<ISimpleAudioVolume>())
                else {
                    continue;
                };
                found.push((name.clone(), volume));
            }
            Ok(found)
        }
    }

    pub fn level(volume: &ISimpleAudioVolume) -> Option<Level> {
        // SAFETY: both calls only read from a live COM object.
        unsafe {
            Some(Level {
                percent: percent_of(volume.GetMasterVolume().ok()?),
                muted: volume.GetMute().ok()?.as_bool(),
            })
        }
    }

    pub fn set_volume(volume: &ISimpleAudioVolume, percent: u8) -> windows::core::Result<()> {
        // SAFETY: a null event context is allowed; it only means that the
        // change is not tagged for the session's listeners.
        unsafe { volume.SetMasterVolume(f32::from(percent.min(100)) / 100.0, std::ptr::null()) }
    }

    pub fn set_muted(volume: &ISimpleAudioVolume, muted: bool) -> windows::core::Result<()> {
        // SAFETY: as in `set_volume`.
        unsafe { volume.SetMute(muted, std::ptr::null()) }
    }

    /// Runs `change` on every session of `program`. A program without a
    /// session cannot be changed: Windows keeps no volume for it.
    pub fn for_each_session(
        program: &str,
        change: impl Fn(&ISimpleAudioVolume) -> windows::core::Result<()>,
    ) -> Result<(), String> {
        let sessions = sessions().map_err(|e| format!("cannot read the sound sessions: {e}"))?;
        let mut changed = false;
        for (_, volume) in sessions.iter().filter(|(name, _)| name == program) {
            change(volume).map_err(|e| format!("cannot change the volume: {e}"))?;
            changed = true;
        }
        if changed {
            Ok(())
        } else {
            Err("the program is not playing sound, so it has no volume to set".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(percent: u8, muted: bool) -> Level {
        Level { percent, muted }
    }

    #[test]
    fn several_sessions_of_a_program_count_as_one() {
        assert_eq!(combined([level(40, false)]), Some(level(40, false)));
        // A browser with a quiet tab and a loud one.
        assert_eq!(
            combined([level(20, false), level(80, false)]),
            Some(level(80, false))
        );
        // Muted only when nothing of it can be heard.
        assert_eq!(
            combined([level(50, true), level(50, false)]),
            Some(level(50, false))
        );
        assert_eq!(
            combined([level(50, true), level(30, true)]),
            Some(level(50, true))
        );
        assert_eq!(combined([]), None);
    }

    #[test]
    fn a_scalar_becomes_whole_percent() {
        assert_eq!(percent_of(0.0), 0);
        assert_eq!(percent_of(0.424), 42);
        assert_eq!(percent_of(1.0), 100);
        assert_eq!(percent_of(1.7), 100);
        assert_eq!(percent_of(-0.2), 0);
    }

    #[test]
    fn nothing_is_collected_for_an_empty_list() {
        let mut out = HashMap::new();
        collect(&[], &mut out);
        assert!(out.is_empty());
    }

    /// Reads the real mixer of the computer the test runs on. A program
    /// with this name does not exist, so it has no session.
    #[test]
    fn a_listed_program_without_sound_is_reported_as_having_no_session() {
        let mut out = HashMap::new();
        collect(&["deskmate-no-such-program.exe".to_string()], &mut out);
        assert_eq!(out["appvol_deskmate_no_such_program"], NO_SESSION);
        assert_eq!(out["appmute_deskmate_no_such_program"], NO_SESSION);
    }
}
