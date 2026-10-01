# Deskmate 0.4.0 - Link, hardware sensors and read-only Files

Deskmate 0.4.0 adds an optional direct, encrypted Home Assistant transport
without removing or changing the default MQTT workflow. It also expands native
Windows hardware telemetry and introduces tightly scoped read-only file access.

The release has passed Rust and TypeScript checks plus x64 and ARM64 release
builds. End-to-end testing with the rebuilt Home Assistant host remains manual
and must be completed before merge or publication.

## Highlights

- **Deskmate Link** is an optional WebSocket transport used in parallel with
  MQTT. Pairing uses a pre-shared key stored in Windows Credential Manager;
  application frames use independent AES-256-GCM session keys, HKDF, HMAC,
  SHA-256 and anti-replay counters from RustCrypto crates.
- **Entity parity** covers the existing sensors and controls, text entities and
  hotkey event triggers. Configuration changes send a complete re-declare so
  Home Assistant can prune entities which no longer exist.
- **Hardware sensors** expose GPU usage, used/total GPU memory, per-volume free
  space and usage, aggregate disk read/write rates, and CPU/GPU temperatures
  when Windows provides a reliable reading. Missing telemetry is not declared.
- **Link Files v1** provides only `list`, `stat` and chunked `read`. Allowed
  roots are empty by default and must be added explicitly in Settings.

## Files security boundaries

- No write, rename or delete operation exists in the v1 protocol.
- Requests outside an allowed canonical root are rejected, including parent
  traversal, UNC/device paths, alternate data streams and reparse points.
- Reads are capped at 256 KiB per chunk and 16 MiB per file, with a 4 MiB/s
  rate gate. Operations are recorded in `security.log` without file contents.
- Home Assistant/Jarvis may apply a stricter limit and confirmation policy on
  top of the client boundary.

## Upgrade notes

- MQTT remains selected after upgrade unless the user explicitly switches the
  transport to Link and completes pairing in Home Assistant.
- Link file access remains disabled because the allowed-root list defaults to
  empty. Add only directories which should be readable from Home Assistant.
- Review the Sensors page after first start. Hardware entities appear only for
  metrics successfully detected on that machine.
- Existing 0.3.1 TLS, clipboard, URL and custom-command policies are preserved.

## Assets

- `Deskmate_0.4.0_x64-setup.exe` - Windows 11 x64
- `Deskmate_0.4.0_arm64-setup.exe` - Windows 11 ARM64
- `Deskmate_0.4.0_installers.zip` - both installers and `SHA256SUMS.txt`
- `SHA256SUMS.txt` - SHA-256 hashes for both standalone installers

Verify downloaded files against `SHA256SUMS.txt`. The installers are not
Authenticode-signed, so Windows SmartScreen may show an unknown-publisher
warning on first run.

## Manual validation required before release

1. Upgrade an existing 0.3.1 installation on Windows x64 and ARM64.
2. Verify MQTT remains the default and existing discovery still works.
3. Pair Link, then verify sensors, text entities, hotkey events and re-declare.
4. Confirm unavailable hardware metrics do not create entities.
5. Confirm Files denies an empty allowlist and paths using `..`, UNC, ADS or a
   reparse point; then test list/stat/read inside one explicit test root.
