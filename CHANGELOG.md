# Changelog

All notable changes to Deskmate are documented here. Release-specific upgrade
notes and asset names are available in `docs/RELEASE-*.md`.

## Unreleased

### Security

- Text typed by `type_text` is filtered to printable characters. Enter, Tab,
  Esc and other control characters were delivered as real key presses, so typed
  text could submit itself, for example as a command in a focused terminal.
  Thanks @anupamme (#1).

## 0.7.0 - 2026-09-24

Files between your phone or laptop and the PC, through Home Assistant. How to turn
it on: `docs/RELEASE-0.7.0.md`.

### Added

- **Deskmate Files** page in the Home Assistant sidebar (administrators only):
  send files from a phone or laptop to a paired computer with progress per
  file, browse the folders the computer shares and download from them. Works in
  the Home Assistant mobile app.
- Receive files on the computer: Settings → Receive files, off by default, with
  *Ask on this computer* or *Accept automatically*, a target folder
  (`Downloads\Deskmate` by default) and a size limit (256 MB by default, shared
  with reads). A toast confirms each received file.
- `deskmate_link.send_file` and `deskmate_link.fetch_file` services for
  automations, limited to `allowlist_external_dirs`.

### Fixed

- A confirmation dialog on the computer (custom command, clipboard write)
  stalled the whole Link session until someone answered, and Home Assistant
  dropped the connection after missed pings. Commands and file requests now run
  alongside the session.
- Unit tests no longer write into the real security log.

### Security

- Received files: plain file names only, no overwrite, `.part` then rename
  after a SHA-256 check, Mark-of-the-Web applied, stalled uploads removed after
  two minutes and when the session ends, at most four at a time.

## 0.6.0 - 2026-09-23

Protocol v2 for Deskmate Link, a fix for duplicated entities after re-pairing,
and one pairing code instead of a key plus an address.

Update the Home Assistant integration first. Deskmate 0.6 speaks only protocol
v2, which integrations older than 0.6.0 reject. Setup and upgrade steps:
`docs/RELEASE-0.6.0.md`.

### Added

- Pairing code (`DMP1.`) in the integration's pairing dialog. It carries the key
  and Home Assistant's local and remote addresses; pasting it in the Deskmate
  wizard or Settings fills in every Link field.
- Home Assistant tells Deskmate why a handshake failed: wrong key, clock more
  than 90 s off, cascade on one side only, or an unsupported protocol version.
  A wrong clock used to look exactly like a wrong key.

### Fixed

- Pairing again after a failed attempt no longer duplicates the computer. The
  entry the computer connects with takes over the old entry's entities, keeps
  their entity IDs and history, and removes the old entry. Adding the
  integration while an entry still waits for pairing shows that entry's code
  again instead of creating another one.
- Hardware sensors (GPU usage, GPU memory, temperatures) no longer disappear and
  reappear in Home Assistant. A failed PDH or WMI read used to withdraw the
  entity on the spot; PDH reads now retry when the GPU engine list grows
  mid-read, and a sensor is withdrawn only after about 2.5 minutes without data.
  This hit desktops with a discrete GPU and LibreHardwareMonitor the most.
- Saving settings validates every key before touching Credential Manager. A
  failed cascade check could previously delete the stored pairing key and leave
  the computer unpaired.
- Renaming the node keeps the pairing key, the cascade key and the Home
  Assistant token. The pairing key used to be deleted and the other two were
  orphaned.
- The English translation of the integration was missing the cascade menu.
- Downloaded toast images are deleted ten minutes after display and at startup.
  Camera snapshots used to accumulate in `%TEMP%`.
- The unit test for the Link frame codec did not compile.

### Security

- Link protocol v2: ephemeral X25519 per connection mixed with the pairing key
  (forward secrecy), a MAC over the whole handshake transcript including the
  cascade flag and both public keys, and length-prefixed encoding for all MAC
  and KDF input. Existing entries accept v1 until the first v2 connection and
  refuse it afterwards.
- Toast buttons carry a single-use random token. Before, any web page or program
  could launch `deskmate:action?name=...` and fire Home Assistant automations.
- Content Security Policy for the app's web view (was disabled).
- Pairing keys and session keys are wiped from memory after use.
- `ws://` Link addresses are accepted only for LAN, `.local`, `.lan`,
  single-label names and Tailscale; internet-facing addresses need `wss://`.
- The cascade key must differ from the pairing key.
- The toast branding script runs through `-EncodedCommand` instead of a
  fixed-name script file in `%TEMP%`.

## 0.5.0 - 2026-07-31

Deskmate Link becomes the recommended way to connect. The integration is
installable through HACS, pairing no longer asks you to retype a device name,
and a failed connection finally says why.

### Added

- The Home Assistant integration ships in this repository under
  `custom_components/deskmate_link` and installs through HACS.
- Cascade encryption: an opt-in second layer of ChaCha20-Poly1305 wrapped around
  the existing AES-256-GCM, keyed separately and derived with its own HKDF
  labels. Configured under the new **Geeky stuff** tab and enabled in Home
  Assistant through *Reconfigure → Enable cascade encryption*.
- `Presenting or full screen` binary sensor, driven by the same shell signal
  Windows uses to suppress its own notifications — a full-screen app, a game,
  a presentation or Do Not Disturb.
- `Mute audio` switch for the default playback device, which also follows mute
  changes made on the computer itself.
- Home Assistant keeps the last entity declaration, so entities exist
  immediately after a restart instead of appearing only once the computer
  reconnects. This is the equivalent of MQTT's retained discovery and was the
  last thing Link was missing before it could replace MQTT.
- Reconfigure flow for a paired entry: rotate the pairing key, or unbind it from
  a computer so another one can take it over, without deleting entities.
- `docs/AI-DEPLOY.md`, a deployment procedure written for an AI assistant, and
  `docs/MIGRATION.md` for moving an existing MQTT setup across.

### Fixed

- **Toast action buttons now render.** Two independent faults produced the same
  symptom, which is why this took so long to pin down. Windows silently drops
  `<actions>` unless the sending application registers a toast activator CLSID,
  which an unpackaged app has to do for itself — Deskmate now registers one on
  startup and writes it into its Start Menu shortcut. Separately,
  `tauri-winrt-notification` 0.8 creates each `<action>` element, sets its
  attributes and never appends it to `<actions>`, so any toast sent through the
  crate arrived with an empty action list. Notifications carrying buttons now
  use Deskmate's own notification XML, which is correct. Clicks travel over the
  `deskmate:` protocol, so no COM server has to run.
- The Start Menu shortcut is rewritten whenever the identifiers it carries
  change. It used to be written once and never corrected, so a shortcut created
  by an older build kept pointing at an identifier that no longer existed.
- A rejected handshake is no longer a silently closed socket. Home Assistant
  answers with a reason, Deskmate distinguishes a refused pairing from a lockout
  and from a network failure, and Home Assistant raises a repair issue naming
  the computer that keeps failing.
- Failed authentication backs off (5, 15, 30, 60 seconds) instead of retrying
  every two seconds, which used to flood the Home Assistant log and keep the
  computer permanently locked out.
- Pairing no longer asks for a node id. An entry binds itself to the first
  computer that authenticates with its key, removing a whole class of silent
  failure where the two sides disagreed about a name.

### Changed

- New installations default to Deskmate Link; existing configurations keep the
  transport they already use.
- Handshake lockout is counted per computer and address rather than per address
  alone, so one misconfigured machine cannot lock out a working one behind the
  same public address.

### Security

- Handshake nonces are single-use. A captured `hello` replayed inside the clock
  tolerance window could previously terminate the live session; verifying a
  handshake no longer touches session state at all.
- Cascade keys are stored in their own Windows Credential Manager entry and are
  redacted from Home Assistant diagnostics.
- A mismatched cascade setting is rejected rather than negotiated down to the
  weaker single layer.

## 0.4.0 - 2026-07-20

### Added

- Optional encrypted Deskmate Link transport running in parallel with the
  existing MQTT transport; MQTT remains the default.
- Link parity for text entities and hotkey events, including full re-declare
  after configuration changes so Home Assistant can prune removed entities.
- Dynamic hardware sensors for GPU usage and memory, disk capacity and I/O,
  plus CPU/GPU temperatures when Windows exposes a reliable provider.
- Link Files v1 with read-only list/stat/read operations, an empty default
  root allowlist, path containment checks and a local security audit log.

### Security

- Link session traffic uses RustCrypto AES-256-GCM, HKDF, HMAC and SHA-256
  with anti-replay counters. Pairing keys remain in Windows Credential Manager.
- Files rejects parent traversal, UNC/device paths, alternate data streams and
  reparse points. Reads are limited to 256 KiB per chunk and 16 MiB per file.

### Compatibility

- Windows 11 x64 and ARM64 installers are built separately.
- Existing MQTT configurations continue to work without enabling Link or file
  access. File access stays disabled until the user adds an allowed root.

## 0.3.1 - 2026-07-15

- Security hardening release: MQTT TLS defaults, split clipboard policies,
  URL allowlists, command confirmation and stricter rate/size limits.
- Full notes: `docs/RELEASE-0.3.1.md`.
