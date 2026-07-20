# Changelog

All notable changes to Deskmate are documented here. Release-specific upgrade
notes and asset names are available in `docs/RELEASE-*.md`.

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
