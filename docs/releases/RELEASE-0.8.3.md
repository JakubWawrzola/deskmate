# Deskmate 0.8.3 - switch off drive sensors, Disconnect button

Two fixes from your reports. Thanks @CBY0.

**Drive, GPU and temperature sensors can be switched off** (#9). Every drive
has its own entities in Home Assistant (free space, usage), plus disk read and
write, the GPU and temperatures. The Sensors page did not list them, so they
could not be turned off. They now have their own *Hardware* panel, with *All
on* and *All off*. A sensor you switch off disappears from Home Assistant, so
the recorder stops storing it.

**Disconnect** (#10): the Status page has *Disconnect* next to *Reconnect*,
and the tray menu has *Disconnect from Home Assistant*. Deskmate then stays
disconnected, also after a restart, until you press *Connect*. Handy while the
integration is disabled in Home Assistant.

## Update

1. **PC:** install `Deskmate_0.8.3_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.
2. **Home Assistant:** update Deskmate in HACS to 0.8.3 when it shows up. The
   integration has no functional changes; 0.8.1 and 0.8.2 keep working.

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.3_x64-setup.exe` | `eb2f1fd6cc557ae50fc4882b0338f5bb0d34d70e74916b16c21cb101dc5fab87` |
| `Deskmate_0.8.3_arm64-setup.exe` | `6744edeb367838e37836129202e5ebf66a35f1ca45f3e18e9e0a0ff4b5e249d5` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
