# Deskmate 0.3.1 — security hardening

This release addresses the security concerns raised after the 0.3.0 public
preview. It changes security defaults and adds local approval controls without
removing the existing MQTT features.

Rust and TypeScript checks and both Windows release builds passed. End-to-end
MQTT/REST validation against a live Home Assistant instance was not rerun before
publication because that instance was unavailable; the manual verification
items remain documented in `STATUS.md`.

## Highlights

- MQTT now defaults to certificate-verified TLS on port 8883 using the Windows
  trust store, with optional support for a private PEM CA.
- Clipboard read and write are separate capabilities. Each supports Off,
  Confirm and Automatic modes and defaults to Off.
- URL opening and notification images use strict HTTP(S) parsing and an exact
  origin allowlist. Image redirects are rejected.
- Custom PowerShell commands are disabled by default and can require local
  confirmation before every run. MQTT values are passed only through a
  sanitized environment variable.
- Retained command and notification messages are ignored. Sensitive inputs are
  size/rate limited, and security audit logs contain metadata only.
- Optional Home Assistant REST fallback requires HTTPS. MQTT passwords and HA
  tokens remain in Windows Credential Manager.

## Upgrade notes

An existing configuration without the new policy fields migrates to TLS/8883,
both clipboard directions Off, and custom commands disabled with confirmation
required. Configure MQTT TLS first, then review Settings and explicitly enable
only the capabilities you need.

For the threat model, exact Mosquitto TLS/ACL setup, residual risks and the
reason an application-level shared-token scheme is not a substitute for TLS,
see [SECURITY.md](SECURITY.md).

## Assets

- `Deskmate_0.3.1_x64-setup.exe` — Windows 11 x64
- `Deskmate_0.3.1_arm64-setup.exe` — Windows 11 ARM64
- `Deskmate_0.3.1_installers.zip` — both installers and SHA-256 checksums
- `SHA256SUMS.txt` — SHA-256 hashes for both standalone installers

Verify downloaded files against `SHA256SUMS.txt` from the release assets.
The installers are not Authenticode-signed yet, so Windows SmartScreen may show
an unknown-publisher warning on first run.

## Known issue

Toast action buttons still do not render on every Windows installation. Toast
title, message, image and branding continue to work through the existing
PowerShell fallback.
