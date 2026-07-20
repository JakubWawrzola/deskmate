# Deskmate Link

Deskmate Link is an optional direct transport between the Windows app and the
`deskmate_link` Home Assistant integration. MQTT remains the default and can be
selected again without losing its saved settings.

## Set up Home Assistant

1. Install the `deskmate_link` custom integration and restart Home Assistant.
2. Open Settings → Devices & services → Add integration → Deskmate Link.
3. Enter the same stable node ID shown by Deskmate. Complete the flow and copy
   the generated base64 pairing key while it is displayed.
4. Do not put the pairing key in YAML or source control.

## Set up Deskmate

1. Open Settings → Home Assistant transport and choose **Deskmate Link**.
2. Enter a local WebSocket URL such as `ws://homeassistant.local:8123`. Deskmate
   appends `/api/deskmate_link/ws` automatically.
3. Optionally enter a fallback `wss://` URL for use outside the local network.
4. Paste the pairing key and choose **Save & reconnect**. The key is stored in
   Windows Credential Manager; it is not written to `config.json`.
5. Check Status for `Connected (Link)`, then find the device under Settings →
   Devices & services → Deskmate Link.

Changing the node ID requires pairing that node again. Local and fallback
connections perform a fresh authenticated handshake and derive fresh session
keys on every reconnect.

## Text controls, presentation and hotkeys

Link declares the existing sensor, binary sensor, number, button, switch and
enabled custom-control definitions with the same keys and names used by MQTT.
Sensor updates remain partial and use the configured publish interval. Commands
return an acknowledgement to Home Assistant. Notifications support title,
message, optional image and action buttons; a click produces the
`deskmate_link_notify_action` event in Home Assistant.

Link v0.2 also declares the same conditional text controls as MQTT:
`type_text`, `open_url`, `tts_say` and `clipboard_set`. Their names are exactly
`Type text`, `Open URL`, `Say (TTS)` and `Set clipboard`; the existing opt-in,
allowlist, confirmation, lock-screen and size checks still apply. Presentation
controls remain buttons and use the same `Presentation ...` names as MQTT.

Every configured global hotkey is declared as a Link event entity named
`hotkey: <name>`, with event type `press`. Pressing it sends a `trigger` frame;
Home Assistant updates the event entity and fires `deskmate_link_trigger` with
`node_id`, `key` and `event`. The key is `hotkey_<hotkey id>`. Hotkeys using a
local/API action still perform that action; the dedicated HA event action only
publishes the event.

After every entity or hotkey configuration change, Deskmate sends a complete
new declaration. Link v0.2 removes entities omitted from that declaration, so
turning an option off or deleting a hotkey/custom control needs no integration
reload.

## Hardware sensors

On Windows systems that expose the corresponding counters, Deskmate declares
GPU usage, GPU memory used/total, free/used space per local volume, aggregate
disk read/write speed and CPU/GPU temperatures. It uses native PDH, DXGI and
WMI plus the existing lightweight disk collector. Unsupported readings are not
declared and never receive synthetic values. The same detected set is used for
MQTT discovery and Link `declare`.

## Link Files v1 (read-only)

Settings → File access (Link) contains the directory allowlist. It is empty by
default, which disables every `fs` request. Each root must be an existing
absolute local-drive directory. UNC/device paths, `.`/`..`, alternate data
streams, symlinks and reparse points are rejected before access.

The encrypted Link session accepts `list`, `stat` and chunked `read` only.
Reads are limited to 256 KiB per chunk and 16 MiB per file, with a global 4
MiB/s rate gate. Every allowed or rejected operation writes its operation,
path and result to `%APPDATA%\Deskmate\security.log`; file contents are never
logged. Link Files v1 has no write, rename or delete operation.

## MQTT and Link parity

| Capability | MQTT | Deskmate Link |
|---|---|---|
| Sensors, binary sensors and volume | MQTT discovery/state topics | Same names and partial states |
| Built-in and presentation buttons | Command topics | `cmd` with encrypted `ack` |
| Keep-awake and custom controls | Switch/number/button discovery | Same kinds, keys and names |
| Text input, URL, TTS, clipboard write | MQTT text entities | Link text entities with the same names |
| Clipboard read | MQTT sensor | Link sensor |
| Hotkeys | MQTT device trigger for the event action | Event entity plus `deskmate_link_trigger`; every configured hotkey emits `press` |
| Toasts and action buttons | Notify/action topics | Encrypted notify/ack and `deskmate_link_notify_action` |
| Hardware sensors | Dynamic MQTT discovery | Same detected set in `declare` |
| Read-only files | Not available | `fs` / `fs_res`, empty allowlist by default |
| Native MQTT device-trigger representation and raw topics | Supported | MQTT-only; Link uses its event entity and event-bus equivalent |
| Hotkeys/widgets/tray using the direct HA API | Independent of transport | Independent of transport |

## Security notes

- The handshake uses HMAC-SHA256 and checks timestamp skew.
- Session keys are derived with HKDF-SHA256. Client-to-server and
  server-to-client keys are separate.
- Frames use AES-256-GCM with authenticated direction/node metadata and a
  strictly increasing counter. A replayed or invalid frame closes the session.
- WebSocket TLS uses Windows Schannel for `wss://`; Link cryptography uses only
  RustCrypto crates.
- Treat the pairing key like a password. Remove the integration entry and pair
  again if it may have been exposed.

## Troubleshooting

- `Link pairing key missing`: paste the key in Settings and save again.
- `welcome timestamp outside allowed skew`: correct the Windows and Home
  Assistant clocks.
- Repeated local connection failures rotate to the configured fallback URL.
- If entities do not match the current settings, save the relevant setting or
  reconnect once. Both operations send a fresh full declaration; Link v0.2
  prunes omitted entities automatically.
