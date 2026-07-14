# Deskmate

A modern Windows companion for Home Assistant. Successor in spirit to
HASS.Agent: your PC shows up in Home Assistant as a device with sensors,
buttons and notifications — with a five-field setup instead of a config maze.

Works natively on Windows 11, both x64 and ARM64 (Snapdragon).

## What it does

- **System sensors** — CPU, memory, disk, network throughput, battery, uptime,
  idle time, session locked, current user. Updated over MQTT on your interval.
- **Privacy-aware sensors** — active window title, WiFi SSID, currently playing
  media (title/artist/app/state via Windows media session). These are **off by
  default** and enabled per-sensor, consciously, in the app.
- **Remote commands** — lock, sleep, hibernate, shutdown, restart, monitors
  off, media play/pause/next/previous, master volume slider. Plus your own
  PowerShell commands, each exposed as a button entity in HA.
- **Notifications** — publish JSON to an MQTT topic and get a native Windows
  toast with title, message and an image ("Dishwasher is ready to unload").
- **Zero-YAML integration** — uses MQTT discovery; entities appear in HA
  automatically, grouped under one device per computer.

## Requirements

- Windows 11 (x64 or ARM64)
- An MQTT broker reachable from the PC. On Home Assistant OS install the
  **Mosquitto broker** add-on and create a HA user for it. The **MQTT
  integration** must be set up in HA (it usually is, automatically).

## Install

Grab the installer from Releases (`Deskmate_x64-setup.exe` or
`Deskmate_arm64-setup.exe`), run it, then:

1. Start Deskmate.
2. Enter broker address (your HA IP), port (1883), MQTT username and password.
3. Save & connect. Done — check Settings > Devices & services > MQTT in HA.

Configuration lives in `%APPDATA%\Deskmate\config.json`. The MQTT password is
stored in Windows Credential Manager, never on disk in plain text.

## Building from source

```
npm install
npm run tauri dev            # development
npm run tauri build          # release for the current architecture
npm run tauri build -- --target x86_64-pc-windows-msvc   # cross to x64
```

Toolchain: Node 20+, Rust stable (`aarch64-pc-windows-msvc` and/or
`x86_64-pc-windows-msvc`). No C compiler needed.

## Notifications from Home Assistant

See [docs/HA-SETUP.md](docs/HA-SETUP.md) for a copy-paste `script` that gives
you a `notify`-style service. Quick test from Developer Tools > Actions:

```yaml
action: mqtt.publish
data:
  topic: deskmate/YOUR_NODE_ID/notify
  payload: '{"title": "Hello", "message": "From Home Assistant", "image": "https://.../img.png"}'
```

The node id is shown on the Status page.

## Security model

- Deskmate never executes MQTT payload content. Built-in commands are a fixed
  allowlist; custom commands run exactly the PowerShell you typed into the app
  yourself, triggered by ID.
- Anyone who can publish to your broker can press those buttons. Protect the
  broker with credentials (and network segmentation if you are serious).
- Privacy sensors are opt-in per sensor and can be turned off at any time;
  disabling one removes the entity from HA.

## Project docs

- [docs/PLAN.md](docs/PLAN.md) — task breakdown and progress
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how it is put together
- [docs/HA-SETUP.md](docs/HA-SETUP.md) — Home Assistant side setup
- [docs/STREAMDECK-PLAN.md](docs/STREAMDECK-PLAN.md) — planned Elgato Stream Deck integration
- [docs/ROADMAP.md](docs/ROADMAP.md) — where this is going
- [HANDOFF.md](HANDOFF.md) — state of work, for contributors and AI agents

## License

MIT
