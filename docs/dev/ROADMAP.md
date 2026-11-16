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
- **0.8.1** — microphone mute and volume, switching the default sound devices,
  screen time and family rules counted and enforced on the PC with an
  administrator's protection (watchdog task, rules owned by Home Assistant),
  Files and Clipboard pages, Sign out button, ready for the default HACS list.
- **0.8.2** — *Press keys* controls (a key combination such as Ctrl+F5 as a
  Home Assistant button).
- **0.8.3** — drive, GPU and temperature sensors can be switched off (#9),
  Disconnect / Connect on the Status page and in the tray (#10).
- **0.8.4** — security fix from the HACS review: computer names from
  unauthenticated connections are validated for every protocol version.
- **0.8.5** — *Start minimized to tray* is stored as soon as it is switched
  (#11).
- **0.9.0** — the Deskmate service (#8), Deskmate for Linux (preview), signed
  automatic updates, the Stream Deck plugin through the app, a media player
  entity, notification tags, screen and user state sensors, and the
  *Connection* sensor. Details in
  [RELEASE-0.9.0](../releases/RELEASE-0.9.0.md).

## Open after 0.9.0

- **Updates**: the update notification says when the Home Assistant
  integration needs updating too (today the Deskmate service panel says so
  for the service); the copies of Deskmate used by the CPU temperature
  helper and the family watchdog follow the app's version (today they stay
  at the version they were installed with).
- **Linux desktop app**: notification tags, the urgent priority and
  clearing; asking on the computer before a received file is saved;
  clipboard, typing text and the active window sensor.

## Next

Sending files to the PC and reading its shared folders already work (0.6 and
0.7). The two items below are what is still open.

- **File transfer**: the on-demand transfer and the Stream Deck *Deskmate Copy*
  flow (branch `codex/streamdeck-neo`) folded into the Files page; files from
  the PC to the phone, not only the other way.
- **Deskmate file server: shared folders across the home, encrypted end to
  end** (asked for by the community; encryption is the priority):
  - any computer with Deskmate can share folders read-only or read-write with
    the other Deskmate computers, or a folder on the Home Assistant host (the
    Raspberry Pi, e.g. `/share/deskmate`) acts as the central server;
  - a file browser on the Files page: browse, copy, move,
    drag and drop between computers; later a drive letter through a local
    WebDAV endpoint on 127.0.0.1 or the Windows Cloud Files API;
  - every transfer inside the existing encrypted Link sessions (X25519 per
    session, AES-256-GCM, optional ChaCha20-Poly1305 cascade); Home Assistant
    relays, nothing opens SMB/NFS or a port; for speed, two computers may
    agree on a direct LAN channel through Home Assistant, still encrypted,
    with the relay as fallback;
  - at rest on the Pi: zero-knowledge storage, chunked and encrypted with a
    per-share key that only the Deskmate computers hold (distributed like the
    pairing code), so a copy of the Pi's disk or of Home Assistant reveals
    nothing; names encrypted too;
  - per-share and per-computer permissions, SHA-256 per chunk, resumable
    transfers, no silent overwrite (versions or a trash), quotas, the local
    audit log, everything opt-in;
  - open questions: key rotation and recovery when a computer is lost,
    conflict handling for edits on two computers, Pi storage limits.

- **Family rules, second step**: per-child rules on a shared PC (keyed by the
  Windows account), a weekday/weekend schedule, opt-in time per app, and an
  MQTT equivalent of the values Home Assistant keeps.

- **DeskmateTV release** (Android TV / Google TV, `tv/`, `docs/TV.md`):
  - stability on more TVs before the first public build;
  - notifications with a live camera preview from a Home Assistant camera
    entity (doorbell on the TV), not just a still frame — asked for in the
    Reddit thread;
  - a picture-in-picture style overlay that does not take the remote's focus.
- Media artwork as an `image` entity.
- WebSocket to Home Assistant for widget tiles instead of polling.

## Infrastructure

- GitHub Actions: build the Windows installers for x64 and ARM64 and attach
  the release artifacts (the Linux packages are already built as workflow
  artifacts; the local uplink is slow, about 6 KB/s).
- Code signing (cost decision pending); it would also calm antivirus
  heuristics.
