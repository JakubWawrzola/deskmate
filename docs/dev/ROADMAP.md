# Roadmap

Ordered by user value. Done items stay for context; release details are in
`CHANGELOG.md`.

## Done

- **0.1-0.3** — MQTT discovery, sensors, commands, custom PowerShell controls,
  toasts with images and buttons, clipboard bridge, remote input, TTS, hotkeys,
  widget panel, tray actions, Home Assistant API channel, Stream Deck plugin.
- **0.4-0.6** — Deskmate Link: own Home Assistant integration over one
  encrypted WebSocket, pairing code, protocol v2 with forward secrecy, cascade
  encryption, hardware sensors (GPU, VRAM, disks), read-only file access.
- **0.7.x** — files from phone or laptop to the PC through the Deskmate
  sidebar page, several Windows users on one PC, daily update check, no
  PowerShell for notifications, camera/microphone sensors for current Windows
  11, full uninstall, readable connection errors.

## In progress (0.8.0)

- Deskmate Link as the default transport for new installs; MQTT stays.
- Community requests (#5, #7, Reddit/Facebook): dark theme, a `last_active`
  timestamp sensor for presence, own sensors (process running, service state,
  window title, PowerShell output), process control (start and stop programs
  without PowerShell), "Start with Windows" kept across updates.
- Home Assistant dashboards (Lovelace) in a Deskmate window, from the tray or
  a hotkey.
- CPU temperature without third-party tools: an optional helper that reads
  Intel/AMD sensors through the signed PawnIO driver, approved once with UAC.
- DeskmateTV groundwork: the integration understands TV nodes; the TV app is
  not released until it is stable (see below).

## Next

- **DeskmateTV release** (Android TV / Google TV, `tv/`, `docs/TV.md`):
  - stability on more TVs before the first public build;
  - notifications with a live camera preview from a Home Assistant camera
    entity (doorbell on the TV), not just a still frame — asked for in the
    Reddit thread;
  - a picture-in-picture style overlay that does not take the remote's focus.
- **Deskmate for Linux** (asked for by the community): the Rust core and the
  Link protocol are portable; sensors, notifications (libnotify / D-Bus),
  tray and hotkeys need Linux backends. Packaging: AppImage first, then .deb.
- **Stream Deck: Deskmate Copy** (branch `codex/streamdeck-neo`): files and
  clipboard text between PCs from Stream Deck keys; awaiting build and tests.
- Full `media_player` entity for the PC (now play/pause/next buttons and
  sensors).
- Media artwork as an `image` entity.
- WebSocket to Home Assistant for widget tiles instead of polling.

## Infrastructure

- GitHub Actions: build x64 + ARM64, attach release artifacts (local uplink is
  slow, about 6 KB/s).
- Code signing (cost decision pending); it would also calm antivirus
  heuristics.
- Tauri updater (in-app updates instead of downloading the installer).
