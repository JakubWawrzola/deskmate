# Deskmate 0.8.2 - press keys from Home Assistant

A small update with one new control.

**Press keys** (Commands page → Custom controls → *Press keys*): a button in
Home Assistant that presses a key combination on the PC, in the window in
front. `Ctrl+F5` reloads the page in a browser, `Win+D` shows the desktop,
`Ctrl+Shift+M` mutes you in Teams, and `Ctrl+L, Ctrl+C` presses two
combinations in a row. You choose the keys on the PC; Home Assistant can only
press the button. Like every custom control it starts disabled and can ask on
the PC first.

## Update

1. **PC:** install `Deskmate_0.8.2_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.
2. **Home Assistant:** update Deskmate in HACS to 0.8.2 when it shows up. The
   integration has no functional changes; 0.8.1 keeps working.

Running Deskmate while nobody is signed in to Windows (#8) is planned:
[roadmap](https://github.com/JakubWawrzola/deskmate/blob/main/docs/dev/ROADMAP.md).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.2_x64-setup.exe` | `fae099a310ef1cf54da8cde9e3c5ccb61c20a784666f97fb2079b96b0fbbbf58` |
| `Deskmate_0.8.2_arm64-setup.exe` | `126d7e858893ff87f7d5739e44429c29108047603faf2f4d84e19d2a29d95ad2` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
