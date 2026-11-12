# Deskmate 0.8.5 - "Start minimized to tray" is saved

A fix from your report. Thanks @itzhouze.

**Start minimized to tray is saved** (#11). The switch in Settings only took
effect after *Save & reconnect* at the bottom of the page, although *Start
with Windows* right above it applies at once. Switched on and left alone, it
was off again the next time you opened Settings, and Deskmate kept starting
with its window open. It is now stored the moment you switch it, like its
neighbour.

The other switches that apply at once (remote input, text-to-speech, branded
toasts, the update check) had a related fault: after leaving Settings and
coming back they could show their previous state, and a later *Save* on any
page stored that state again. The window now reads the stored settings back
after every change.

Not tested end to end against a live Home Assistant: this release was checked
with the automated tests, and the change is limited to how the Settings page
stores its switches.

## Update

1. **PC:** install `Deskmate_0.8.5_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept. Then open Settings and switch
   *Start minimized to tray* on once more.
2. **Home Assistant:** update Deskmate in HACS to 0.8.5 when it shows up. The
   integration has no changes since 0.8.4; this only keeps the versions in
   step.

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.5_x64-setup.exe` | `33b10322523d9c2ef9d5141ca0d4a82a0b7c42fbfbda0389f25712383bd895af` |
| `Deskmate_0.8.5_arm64-setup.exe` | `d34d77ff5a07bebad1b3afd4554a3bf639b3788de046f9611f98a07837efc98b` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
