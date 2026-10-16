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

## In progress

- Stream Deck: Deskmate Copy (branch `codex/streamdeck-neo`), part of 0.9.0.

## Next update (0.9.0): files and auto-update

- **Auto-update** (Jakub, 14.10: moved from 0.8.1 to 0.9.0): the daily check
  finds a new release and shows a notification with *Install now* and
  *Later*; the same choice sits in the tray menu and on the Status page.
  - *Install now*: Deskmate downloads the installer for its architecture from
    the GitHub release, checks it, runs it in update mode and starts again
    with the same settings and pairing;
  - the check: Tauri updater plugin with an update manifest (`latest.json`)
    signed with an Ed25519 key on every release, plus the SHA-256 from the
    release notes; nothing unsigned runs (decide where the private key lives:
    local only or CI secret, never in the repo);
  - Settings → Updates: install automatically, ask first (default), or only
    tell me (today's behaviour); *Later* asks again the next day;
  - the installer's update mode keeps "Start with Windows" (the root cause of
    #7), the CPU temperature helper and the family protection; the watchdog
    copy in Program Files is refreshed with the next protection step;
  - the Home Assistant integration keeps updating through HACS; the
    notification says when it needs updating too.

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

## Next

- **Deskmate as a Windows service** (#8): Home Assistant keeps the PC's
  machine sensors and power buttons while nobody is signed in.
  - an optional service, installed with one administrator approval, that
    reports CPU, memory, disks, network, temperatures and uptime, and accepts
    only shutdown, restart, sleep and hibernate;
  - at sign-in the user's Deskmate takes the connection over, the same way it
    already moves between Windows users (seat.rs); at sign-out the service
    takes it back;
  - a headless Link client without the app window (Windows runs services in a
    session without a desktop, so windows, media, clipboard, input and
    notifications stay with the signed-in app);
  - its own copy of the pairing key, readable only by SYSTEM and
    administrators, handed over during the elevated install;
  - entities that need a signed-in user show as unavailable instead of being
    removed, so names, areas and dashboards survive;
  - the installer's update mode refreshes the service; uninstall removes it.

- **Family rules, second step**: per-child rules on a shared PC (keyed by the
  Windows account), a weekday/weekend schedule, opt-in time per app, and an
  MQTT equivalent of the values Home Assistant keeps.

- **DeskmateTV release** (Android TV / Google TV, `tv/`, `docs/TV.md`):
  - stability on more TVs before the first public build;
  - notifications with a live camera preview from a Home Assistant camera
    entity (doorbell on the TV), not just a still frame — asked for in the
    Reddit thread;
  - a picture-in-picture style overlay that does not take the remote's focus.
- **Deskmate for Linux** (asked for by the community): the Rust core and the
  Link protocol are portable; sensors, notifications (libnotify / D-Bus),
  tray and hotkeys need Linux backends. Packaging: AppImage first, then .deb.
- Full `media_player` entity for the PC (now play/pause/next buttons and
  sensors).
- Media artwork as an `image` entity.
- WebSocket to Home Assistant for widget tiles instead of polling.

## Infrastructure

- GitHub Actions: build x64 + ARM64, attach release artifacts (local uplink is
  slow, about 6 KB/s).
- Code signing (cost decision pending); it would also calm antivirus
  heuristics.
