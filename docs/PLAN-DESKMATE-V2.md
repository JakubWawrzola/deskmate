# DEPLOYMENT PLAN — Deskmate v0.2

Source: research workflow 2026-07-10 (4 agents: HASS.Agent inventory, other
HAOS companions, Deskmate audit, Rust/Tauri ARM64 feasibility + synthesis).
Baseline: v0.1.2, 18 sensors + 9 commands + custom PS, toast with image, SMTC
media, clean MQTT discovery, rumqttc without TLS, windows crate 0.61, ARM64
without clang.
Overriding rule: zero ring/openssl-sys/C build.

## STATUS (2026-07-10): SHIPPED
Steps 2-6 + hardening implemented (cargo check + tsc green, x64+ARM64
installers in dist-installers/). Step 1 (branded toast) DEFERRED to backlog -
COM AUMID failed to compile on windows 0.61; toast works via PowerShell
AUMID. Details: HANDOFF.md section 0.2.0, tests: HomeAssistant/docs/
DO-PRZETESTOWANIA.md section E.

## v0.2 scope (6 features + hardening) — implementation for this iteration

Criterion: green on ARM64, fits clean discovery (or closes a bug),
best value/effort ratio. Order = ship order.

### Step 1 — Branded toast (AUMID in Start Menu shortcut) — closes bug 0.1.2
Shortcut `%AppData%\Microsoft\Windows\Start Menu\Programs\Deskmate.lnk` with
`System.AppUserModel.ID` (IShellLinkW + IPropertyStore, PKEY
{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3},5, IPersistFile::Save). Then switch
back to our own TOAST_AUMID instead of POWERSHELL_APP_ID. Shortcut created
once.
Files: notify.rs (ensure_start_menu_shortcut + AUMID switch), consts.rs.
Risk: PROPVARIANT VT_LPWSTR (init/free), relogin needed for AUMID to take
effect.

### Step 2 — Actionable notifications (toast buttons -> MQTT) — value 5
notify payload with `actions:[{title,action}]`. tauri-winrt-notification
0.7->0.8: add_button + on_activated -> publish to
deskmate/<node>/notify/action. Callback = Fn holding a tokio Handle/mpsc
(not an async closure).
Files: Cargo.toml (0.8), notify.rs, mqtt.rs, consts.rs, src/pages/Notifications.
Risk: on_activated only works in-process while the app is alive (persistent
tray = OK).

### Step 3 — PC <-> HA clipboard (S) — value 4, cheapest
arboard 3 (clipboard-win + windows-sys, zero C). Sensor
deskmate/<node>/clipboard (privacy=true, default OFF) + text entity
clipboard/set -> arboard set_text. Files: new clipboard.rs, sensors.rs,
mqtt.rs, discovery.rs (text component), Cargo.toml.
Risk: most sensitive sensor (passwords/2FA) - hard opt-in + warning in UI.

### Step 4 — Remote text entry + presentation control (S) — value 4
SendInput (Win32_UI_Input_KeyboardAndMouse - already present). Text via
KEYEVENTF_UNICODE. Presentation: VK_RIGHT/LEFT/F5/ESCAPE/B. Entities: text
type/set + present_* buttons. Opt-in. Files: sys_commands.rs, mqtt.rs,
discovery.rs, config.rs.
Risk: UIPI (won't reach admin windows when the app isn't running as admin),
whitelist instead of raw passthrough.

### Step 5 — TTS (PC speaks text from HA) (S/M) — value 4
SAPI ISpVoice::Speak (Win32_Media_Speech), CLSID SpVoice, SPF_ASYNC.
Channel: text entity tts/set (opt-in, tts_enabled flag). Files: new tts.rs,
mqtt.rs, discovery.rs, config.rs, Cargo.toml, SettingsPage.
Risk: SAPI = STA COM - dedicated thread with
CoInitializeEx(APARTMENTTHREADED), not from the tokio worker pool; text
queueing.

### Step 6 — switch / number in custom controls (S)
Custom controls from config: kind=button|switch|number, matching discovery
component, run_custom with a typed/validated value (like volume, anti-RCE).
Files: config.rs (CustomControl.kind), sys_commands.rs, discovery.rs,
Commands.tsx, types.ts. Risk: retained switch state.

### Hardening (cheap fixes from the audit, roll into v0.2)
- skip the first net_down/net_up tick (startup spike), sensors.rs
- expire_after also for binary_sensor (session_locked/ac_power), discovery.rs
- clean up the old Credential Manager entry when host/user changes, config.rs
- notification history timestamps local instead of UTC, mqtt.rs

## Backlog (not now — with rationale)
- User custom sensors (script->JSON), value 5 = v0.3 opener (a platform
  shift, not a point feature; builds on switch/number + custom PS).
- Full media_player entity + cover art = v0.4 (HA has no MQTT-discovery
  media_player; requires HACS - breaks "zero HACS"). MVP stays on SMTC.
- On-demand screenshot = v0.4 (xcap pulls in a 2nd windows crate version;
  GDI = a lot of code; privacy-invasive).
- File transfer = v0.3 (HTTP not discovery; token in keyring, path
  traversal).
- Global hotkeys / quick-actions overlay = v0.3-0.5 (transparent window is
  finicky, reverse direction PC->HA).
- TLS broker (mqtts) = on hold (rustls/ring needs clang on ARM64).
- Satellite/service without login = deliberate omission (conflicts with the
  Tauri model).
- Camera/mic in use, Windows Updates, Process/Service State, per-core CPU,
  per-adapter network, external IP, WebView, Wake-on-LAN = cheap, add after
  v0.2.

## DO-PRZETESTOWANIA checklists (Jakub) — added to repo HomeAssistant/docs/DO-PRZETESTOWANIA.md section E
See per-step above; manual tests, nothing run automatically.
