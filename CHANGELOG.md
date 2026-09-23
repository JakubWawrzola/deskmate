# Changelog

All notable changes to Deskmate are documented here. Release-specific upgrade
notes and asset names are available in `docs/RELEASE-*.md`.

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
