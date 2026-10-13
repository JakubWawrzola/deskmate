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
- **0.8.0** — community requests (#5, #7, Reddit, Facebook): Deskmate Link
  as the default for new installs, own sensors (program running, window open,
  service state, PowerShell output), program controls without PowerShell,
  Last active sensor, dark theme, Home Assistant dashboards in a window, CPU
  temperature through PawnIO with an optional elevated helper, "Start with
  Windows" kept across updates, DeskmateTV groundwork (TV app not released).

## In progress

- Stream Deck: Deskmate Copy (see Next).

## Next

- **Auto-updater** (in-app updates instead of downloading the installer):
  - Tauri updater plugin: Deskmate downloads the new installer in the
    background and installs it in update mode, then restarts itself;
  - an update manifest (`latest.json`) attached to every GitHub release, signed
    with an Ed25519 key (decide where the private key lives: local only or CI
    secret; never in the repo);
  - Settings → Updates: install automatically, ask first (default), or only
    tell me (today's behaviour);
  - the installer's update mode also keeps "Start with Windows", the root
    cause of #7;
  - keep the CPU temperature helper and the pairing untouched across updates;
    the Home Assistant integration keeps updating through HACS.
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
