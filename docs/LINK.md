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

## Entity and notification coverage

Link declares the existing sensor, binary sensor, number, button, switch and
enabled custom-control definitions with the same keys and names used by MQTT.
Sensor updates remain partial and use the configured publish interval. Commands
return an acknowledgement to Home Assistant. Notifications support title,
message, optional image and action buttons; a click produces the
`deskmate_link_notify_action` event in Home Assistant.

The current Home Assistant Link schema does not expose MQTT-only text entities
(`clipboard_set`, `type_text`, `tts_say`, `open_url`) or MQTT device-automation
hotkey triggers. Clipboard reading, presentation buttons, standard commands,
custom controls, sensors and toast actions are supported where their entity
kind exists in the integration. Use MQTT when those MQTT-only entities are
required.

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
- If entities do not match the current settings after toggling sensors, reload
  the Deskmate Link integration or reconnect the client. Dynamic entity removal
  is limited by the current Home Assistant integration.
