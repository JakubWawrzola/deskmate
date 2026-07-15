# Roadmap

Ordered by user value. Done items stay for context.

## Done

- **0.1** — MQTT discovery core: sensors, commands, custom PowerShell buttons,
  toast notifications with image, media keys, tray app, 5-field wizard.
- **0.2** — actionable notifications (toast buttons → MQTT), clipboard bridge
  PC↔HA, remote text input + presentation control, TTS, custom switch/number
  controls, broker failover (local + fallback address), "HomeOS" toast
  branding (Start Menu shortcut with AUMID; PowerShell-AUMID fallback that
  always renders), toast buttons via `deskmate:` protocol activation.
- **0.3** — Home Assistant API channel (REST, token in Credential Manager,
  URL failover), **global hotkeys** (toggle entity / call service / local
  command / widget panel / MQTT device trigger), **always-on-top widget
  panel** with live entity tiles, **tray quick actions**, keep-awake switch,
  camera/microphone-in-use sensors (opt-in), empty recycle bin, open URL
  (http/https only), Stream Deck plugin (standalone, SDK v2).

## Next

- WebSocket to HA for widget tiles (replace 3 s polling; live updates + lower
  overhead once tile counts grow).
- Custom user sensors (PowerShell script → JSON → entities) — the v0.4 opener.
- Media artwork as an HA `image` entity (SMTC thumbnail over MQTT).
- Full `media_player` entity (needs a small custom integration on the HA side
  — the MQTT discovery schema has no media_player).
- File transfer PC↔HA (upload to `www/`, token-scoped, path-traversal safe).
- Screenshot on demand (privacy-heavy; needs a deliberate opt-in design).
- Optional application-layer signed command envelopes (HMAC + timestamp/nonce)
  if a future HA-side Deskmate integration can generate and verify them. MQTT
  TLS and per-node broker ACLs remain the primary transport controls.
- Broker autodetection (mDNS `_mqtt._tcp`), UI i18n (EN/PL), tray icon
  reflecting connection state, light theme for the widget panel.

## Infrastructure

- GitHub Actions: build x64 + ARM64, attach release artifacts.
- Code signing (cost decision pending).
- Tauri updater (auto-updates).
