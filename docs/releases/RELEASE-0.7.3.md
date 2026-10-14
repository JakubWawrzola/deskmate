# Deskmate 0.7.3 - reinstalling and starting over, fixed

Fixes from the second round of feedback in #2 (thanks @CBY0).

- **Reconfigure in Home Assistant had no labels**, just four arrows. The
  second one generated a new pairing key, so looking for the code cut off the
  existing install. Labels are back, and **Show the pairing code** gives the
  code of a paired computer again, for a reinstalled Deskmate or another
  Windows user, without changing the key.
- **A full uninstall now removes everything.** With *Delete the application
  data* ticked, the uninstaller also removes the settings, the saved keys in
  Credential Manager and Deskmate's registry entries. Files received from Home
  Assistant stay.
- **"os error 10013"** now reads as what it is: Windows blocked the
  connection, usually a firewall or antivirus such as Bitdefender. Allow
  Deskmate there and press *Reconnect*.
- **No more Polish in an English Home Assistant**: the pending entry's title,
  error messages and action descriptions.

## Update

1. **Home Assistant:** update Deskmate in HACS to 0.7.3 and restart Home
   Assistant.
2. **PC:** install `Deskmate_0.7.3_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.

Starting completely over is described in the
[guide](https://github.com/JakubWawrzola/deskmate/blob/main/docs/GUIDE.md#14-troubleshooting).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.7.3_x64-setup.exe` | `753DDBD83B05070392A53187709A43E8B68CCA2DB26724582AC0FF03D737D98F` |
| `Deskmate_0.7.3_arm64-setup.exe` | `1EFFDEE6CB081B8A8E8C84E82D65679D6A60D2ABDFA7F4AF91ACF44B07B53711` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
