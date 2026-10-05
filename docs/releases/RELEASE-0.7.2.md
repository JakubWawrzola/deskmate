# Deskmate 0.7.2 - camera and microphone sensors fixed

**Camera in use** and **Microphone in use** stayed off on current Windows 11
builds, even during a video call (#4, thanks @eXtreme). Deskmate read the
camera and microphone usage that Windows records in the registry, and recent
Windows updates stopped marking an app as "still using" the device there.
HASS.Agent ran into the same change.

Deskmate now asks Windows directly:

- **Microphone:** is any app recording from an active microphone right now
  (Core Audio sessions).
- **Camera:** is any app streaming from a camera right now (the Media
  Foundation sensor activity monitor, which only watches and never opens the
  camera).

The old registry check stays as a fallback for systems where those are not
available. Both sensors are still off until you enable them on the Sensors
page.

## Update

Install `Deskmate_0.7.2_x64-setup.exe` (or `_arm64`) over the old version.
Settings and pairing are kept. The Home Assistant integration has no changes in
this release; updating it in HACS is optional.

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.7.2_x64-setup.exe` | `7150BD0CA5D56B186589CA802BC8B1F89AD03B80A75B4DA3142DE84BEF6B8965` |
| `Deskmate_0.7.2_arm64-setup.exe` | `13D011DC6820B47077D461CD7FC66A81858E147D531AA35282C394B8F1212A8D` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64.
