# PLAN — Deskmate v0.3 "Control Anywhere"

Date: 2026-07-15. Source of ideas: v0.2 backlog (PLAN-DESKMATE-V2 + ROADMAP),
inventory of HASS.Agent/IoTLink/go-hass-agent from the 2026-07-10 research, Jakub's
ideas (hotkeys without a Stream Deck, widgets) plus original ones. Overriding rule,
unchanged: zero ring/openssl/clang (ARM64 must build cleanly), zero hardcoded values.

Statuses: [W] = shipped in this iteration, [P] = partially prepared, [B] = backlog.

## Foundation

### F1. Home Assistant API channel (REST) [W]
Until now Deskmate was "mute" toward HA - it only received commands over MQTT.
Controlling ANY HA entity from the computer (hotkeys, widgets, tray) requires a
return channel. New module `ha_api.rs`:
- Configuration in Settings ("Home Assistant API"): local URL + fallback URL
  (failover just like the MQTT broker) + long-lived access token. The token is
  kept EXCLUSIVELY in Windows Credential Manager (keyring, same as the MQTT
  password) - never in config.json.
- REST: `POST /api/services/{domain}/{service}` (actions), `GET /api/states/{id}`
  and `GET /api/states` (state for widgets). ureq + native-tls (schannel) - also
  works over https, zero new C dependencies.
- Failover: try the local URL first, on connection error fall back to the
  secondary URL (Tailscale).
- Optional: without a configured API, everything keeps working except the
  features that explicitly require it (widgets, HA actions in hotkeys); the UI
  communicates this.

## Control without a Stream Deck

### F2. Global keyboard shortcuts (hotkeys) [W]
Shortcuts working system-wide (via tauri-plugin-global-shortcut), even when
Deskmate is minimized to the tray. Each hotkey = an accelerator (e.g.
`Ctrl+Alt+L`) + an action. Action types:
- `toggle` - toggles an HA entity (homeassistant.toggle on entity_id) via F1.
- `service` - any HA service (`domain.service` + optional entity_id +
  JSON data), e.g. `scene.turn_on` on scene.dobranoc.
- `command` - a local Deskmate command (builtin lock/media etc., or a custom PS
  script).
- `widget` - show/hide the widget panel (F3).
- `mqtt` - publishes an event to `deskmate/<node>/hotkey/<id>`; in HA it appears
  as a device trigger (F2a) - automations without an API token.
Configuration: a new "Hotkeys" page in the UI (list, add, remove, accelerator
validation, conflict warnings). Registered on startup and on every change.
A registration error (shortcut already taken by another app) produces a
readable message, and the remaining hotkeys keep working.

### F2a. Hotkeys as HA device triggers (MQTT discovery) [W]
Every hotkey of type `mqtt` publishes a `device_automation` discovery (trigger).
In HA, the automation editor suggests "LaptopWawrzola: hotkey <name>" as a
trigger - the user just clicks, no YAML needed. Zero token, pure MQTT.

### F3. Widget panel (always-on-top) [W]
A second, small Tauri window (frameless, always on top, draggable, remembers
its position) with tiles for configured HA entities:
- Each tile: friendly name + live state (REST polling every 3 s, cheap)
  + click = `homeassistant.toggle` (lights/switches/media).
- Entities that aren't toggleable (sensor, temperature) = read-only tile.
- Configuration: a "Widgets" page in the UI (list of entity_id + labels).
- Show/hide: hotkey (`widget` action), tray menu, button in the UI.
- Monochrome style, matching the rest of the app (and the HAOS dashboards).
Requires F1. Without the API, the panel shows setup instructions.

### F4. Quick actions in the tray menu [W]
Configurable tray menu items (above Open/Quit): a name + an action of the same
type as a hotkey (toggle/service/command). Right-click on the tray icon ->
"Living room light", "Movie scene", "Lock PC". Refreshed after every
configuration change.

## New sensors / entities

### F5. "Keep awake" switch [W]
A switch (in HA and locally) that prevents the computer from sleeping and the
screen from dimming (`SetThreadExecutionState ES_SYSTEM_REQUIRED|ES_DISPLAY_REQUIRED`).
Use case: "don't let the laptop sleep, a backup/download is running". State
retained in HA.

### F6. "Camera in use" / "Microphone in use" sensors [W]
Binary sensors (privacy, OFF by default, opt-in like the clipboard) reading the
registry `HKCU\...\CapabilityAccessManager\ConsentStore\{webcam,microphone}` -
`LastUsedTimeStop == 0` for any app means the device is active.
Use case: "I'm recording" automation -> door light turns red.

### F7. "Empty recycle bin" command [W]
Builtin button (SHEmptyRecycleBinW, no confirmation dialogs or sound).

### F8. "Open URL" command [W]
Text entity `open_url`: HA sends a URL, the computer opens it in the default
browser. Validation: http/https ONLY (no file:, cmd, etc.).
Gate: allow_input (the same trust group as text input).

## GitHub / presentation

### F9. Documentation in English [W]
README.md rewritten in EN and expanded (feature list, quick start, security
model, architecture overview, roadmap link, badge-ready). ARCHITECTURE.md,
HA-SETUP.md, ROADMAP.md, streamdeck-plugin/README.md -> EN. Working files
(HANDOFF.md, PLAN-*.md, STATUS.md) stay in Polish - those are workshop notes,
not a showcase.

### F10. "Computers" dashboard in HAOS [W]
A sample dashboard for screenshots: monochrome style matching "Dom"/"Kontrola",
split into **Laptop** (LaptopWawrzola - real entities from MQTT discovery: CPU,
RAM, disk, network, battery, media, lock/sleep/media buttons, volume,
clipboard) and **PC** (Ryzen - not yet added: a placeholder card "Install
Deskmate on this computer", showing what onboarding a second device looks like).
File: HomeAssistant/dashboards/komputery.yaml + entry in configuration.yaml.

## Backlog (not now - with a reason)
- Full media_player in HA (requires HACS/a custom integration - breaks the
  zero-HACS rule).
- Track cover art as an image entity (base64 over MQTT; to reconsider after the
  media features).
- WebSocket for widget state instead of polling (tungstenite; 3 s polling is
  enough for 5-10 tiles, WS would add reconnect complexity).
- File transfer PC<->HA (upload/token/path traversal - a separate iteration).
- On-demand screenshot (privacy-invasive; needs a well-thought-out opt-in).
- TLS to the MQTT broker (rustls/ring still requires clang on ARM64).
- mDNS broker autodiscovery, PL/EN i18n, tray icon reflecting connection state,
  Tauri updater, code signing, GitHub Actions release pipeline.
- Light/dark widget theme following the system (currently: dark only).

## Ship order (this iteration)
1. F1 ha_api.rs + Settings + keyring
2. F5/F6/F7/F8 (cheap, independent)
3. F2 + F2a hotkeys (config, registration, actions, discovery, UI page)
4. F3 widgets (window, config page, polling)
5. F4 tray quick actions
6. F9 docs EN
7. F10 HAOS dashboard
8. Build 0.3.0 x64+ARM64, commit, STATUS, TO TEST
