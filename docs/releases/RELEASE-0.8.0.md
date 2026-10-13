# Deskmate 0.8.0 - your own sensors, program controls, dark theme, CPU temperature

Most of this release comes from your feedback in issues #5 and #7 and from
the Reddit and Facebook threads. Thank you.

## Update (2 minutes)

1. **Home Assistant:** update Deskmate in HACS to 0.8.0 and restart Home
   Assistant. Needed for the *Last active* sensor and the new on/off sensors.
2. **PC:** install `Deskmate_0.8.0_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.

If you use *Start with Windows*, switch it on once more after this update.
From now on it survives updates (#7).

## What is new

**Your own sensors** (Sensors page → Your sensors): whether a program runs or
a window is open, the state of a Windows service, or the first line a
PowerShell command prints, on its own interval. PowerShell sensors start
disabled.

**Program controls without PowerShell** (Commands page): a button that starts a
program, file or URL (`steam://...` works) and a button that stops a program by
name. Together with a *Program running* sensor, a program becomes something
Home Assistant can switch.

**Last active**: the time of the last keyboard or mouse input, for presence,
like HASS.Agent's LastActive. Off by default; enable it on the Sensors page.

**Dark theme**: follows Windows, or choose under Settings → Appearance.

**Home Assistant dashboards in a Deskmate window**: from the tray (*Open Home
Assistant*), a hotkey or a quick action. Log in once; the session is kept.

**CPU temperature through PawnIO**: Windows only lets programs with
administrator rights read it. Install [PawnIO](https://pawnio.eu), then
Settings → CPU temperature → *Install helper* (one administrator approval). A
small helper reads Intel and AMD Ryzen sensors and the *CPU temperature*
sensor shows up in Home Assistant; Deskmate itself keeps running without
administrator rights. x64 only.

**Deskmate Link by default** for new installs. Existing MQTT setups stay on
MQTT.

## Fixed

- "Start with Windows" switched itself off after every update (#7).

Full list: [CHANGELOG.md](../../CHANGELOG.md). How each feature works:
[GUIDE.md](../GUIDE.md).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.0_x64-setup.exe` | `E1DF07E8CA6C6441E4F0AE9E318349AC622A78B462684657624FD7CFD9EE77A2` |
| `Deskmate_0.8.0_arm64-setup.exe` | `77C091B61D27B539B48733BCDED2A00FEA8595621C20BEBBF1D621578F895577` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
