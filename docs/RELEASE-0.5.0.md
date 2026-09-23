# Deskmate 0.5.0 — Link first, and toasts you can click

This release makes Deskmate Link the way to connect. The Home Assistant
integration now installs through HACS, pairing takes one key and nothing else,
and the two things that made Link feel unfinished — entities disappearing after
a Home Assistant restart, and a failed connection saying nothing useful — are
fixed. Toast action buttons work as well, after a root cause that turned out to
have nothing to do with the notification payload.

## Install

[![Open your Home Assistant instance and open a repository inside the Home Assistant Community Store.](https://my.home-assistant.io/badges/hacs_repository.svg)](https://my.home-assistant.io/redirect/hacs_repository/?owner=JakubWawrzola&repository=deskmate&category=integration)

1. Add the integration through HACS with the button above, or add
   `https://github.com/JakubWawrzola/deskmate` as a custom repository of
   category *Integration*. Restart Home Assistant.
2. *Settings → Devices & services → Add integration → Deskmate Link*. Copy the
   pairing key it shows once.
3. Install the app, pick **Deskmate Link**, enter the WebSocket address of your
   Home Assistant and paste the key.

Upgrading from MQTT: see [docs/MIGRATION.md](MIGRATION.md). Entity ids do not
change. Setting up with an AI assistant: point it at
[docs/AI-DEPLOY.md](AI-DEPLOY.md).

## Highlights

**Toast action buttons render.** Two unrelated faults produced the same symptom,
which is why this survived several attempts. Windows drops the `<actions>`
element unless the sending application has a toast activator CLSID registered,
and for an unpackaged app that registration is its own job — Deskmate now does
it at startup and writes the CLSID into its Start Menu shortcut. On top of that,
`tauri-winrt-notification` 0.8 creates each `<action>` element, sets its
attributes and never appends it to `<actions>`, so every toast sent through the
crate carried an empty action list. Notifications with buttons now use
Deskmate's own notification XML. Clicks travel over the `deskmate:` URL
protocol, so no COM server has to be running.

**Pairing cannot silently mismatch.** The integration no longer asks for a node
id. An entry is created unbound and attaches to the first computer that
authenticates with its key. Retyping a name on both sides was a reliable way to
produce a connection that failed with no explanation on either end.

**Failures explain themselves.** A rejected handshake gets an explicit reason
instead of a closed socket. Deskmate separates *rejected pairing*, *locked out*
and *network failure*, and Home Assistant raises a repair issue naming the
computer that keeps failing. Authentication failures back off to 5, 15, 30 and
60 seconds instead of retrying every two.

**Entities survive a restart.** Home Assistant stores the last entity
declaration, so entities exist immediately after a restart even with the
computer switched off. This was the last capability MQTT's retained discovery
had and Link did not.

**Cascade encryption**, under a new *Geeky stuff* tab, wraps a second
independently keyed ChaCha20-Poly1305 layer around the existing AES-256-GCM.
Enable it on the entry in Home Assistant, paste the second key into Deskmate.
A mismatch between the two ends is rejected rather than quietly downgraded.

**Two new entities**: a `Presenting or full screen` binary sensor driven by the
shell signal Windows uses to suppress its own notifications, and a `Mute audio`
switch that also follows mute changes made on the computer itself.

## Security

- Handshake nonces are single-use. A captured `hello` replayed inside the clock
  tolerance window could previously drop a live session without ever decrypting
  anything.
- Handshake lockout counts per computer and address rather than per address, so
  one misconfigured machine behind a shared public address cannot lock out a
  working one.
- Cascade keys live in their own Credential Manager entry and are redacted from
  Home Assistant diagnostics.

Full detail in [CHANGELOG.md](../CHANGELOG.md) and
[docs/SECURITY.md](SECURITY.md).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.5.0_x64-setup.exe` | `404BE9AAE9189231D1FE581DD04C52F5F1B61E72C139EAD53057772CEBDFA767` |
| `Deskmate_0.5.0_arm64-setup.exe` | `E4890C49D3A57153F19819DEE0DE493AE72D74F6EDD586FA0A32C255E4B0FB3B` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
