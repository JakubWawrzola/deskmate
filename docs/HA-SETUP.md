# Home Assistant setup

## 1. Broker: TLS and a dedicated identity

Home Assistant OS: install the official **Mosquitto broker** app, then configure
a certificate and key located in `/ssl`:

```yaml
certfile: fullchain.pem
keyfile: privkey.pem
require_certificate: false
```

This enables MQTT TLS on port 8883. In Deskmate select **TLS**, use the broker
hostname present in the certificate, and leave the custom CA path empty for a
publicly trusted certificate. For a private CA, copy its PEM certificate to the
PC and select that path. Certificate verification cannot be disabled.

Create a dedicated MQTT user for every Deskmate PC. The official app accepts HA
users or users from its `logins` configuration and does not support anonymous
access. After all clients use TLS, blank/remove ports 1883 and 1884 in the
Mosquitto app Network card.

### Per-node ACL example

Enable custom Mosquitto configuration:

```yaml
customize:
  active: true
  folder: mosquitto
```

Create `/share/mosquitto/acl.conf`:

```text
acl_file /share/mosquitto/accesscontrollist
```

For node `YOUR_NODE_ID`, create `/share/mosquitto/accesscontrollist`:

```text
user addons
topic readwrite #

user homeassistant
topic readwrite #

user YOUR_DESKMATE_MQTT_USER
topic read deskmate/YOUR_NODE_ID/cmd/+
topic read deskmate/YOUR_NODE_ID/notify
topic write deskmate/YOUR_NODE_ID/state/#
topic write deskmate/YOUR_NODE_ID/availability
topic write deskmate/YOUR_NODE_ID/notify/action
topic write deskmate/YOUR_NODE_ID/hotkey/#
topic write homeassistant/+/YOUR_NODE_ID/#
```

Restart Mosquitto after changing the files. Use one block and one account per
PC; do not replace it with `topic readwrite #` for Deskmate.

Official option reference: [Home Assistant Mosquitto app](https://github.com/home-assistant/addons/blob/master/mosquitto/DOCS.md).

## 2. Verify the device

After "Save & connect" in Deskmate: Settings > Devices & services > MQTT >
devices — you should see a device named after your computer with sensor,
button and number entities. Entity ids look like `sensor.<node_id>_cpu`,
`button.<node_id>_lock`, `number.<node_id>_volume`.

## 3. Notifications as a service

Add this script (Settings > Automations & scenes > Scripts > new, YAML mode),
replacing `NODE_ID` with the node id from Deskmate's Status page:

```yaml
alias: Notify PC
description: Windows toast on the PC via Deskmate
fields:
  title:
    description: Toast title
    example: Dishwasher
  message:
    description: Toast body
    example: Ready to unload
  image:
    description: Optional image URL (use https or LAN /local/ URL)
    example: "http://homeassistant.local:8123/local/img.png"
sequence:
  - action: mqtt.publish
    data:
      topic: deskmate/NODE_ID/notify
      payload: >-
        {"title": {{ title | tojson }},
         "message": {{ message | tojson }},
         "image": {{ (image | default('')) | tojson }}}
mode: queued
```

Use it from automations:

```yaml
action: script.notify_pc
data:
  title: "Zmywarka"
  message: "Jest do rozpakowania"
  image: "http://homeassistant.local:8123/local/icons/dishwasher.png"
```

Tip: image URLs must be reachable **from the PC** (LAN IP or public URL, not
`/local/...` relative paths).

## 4. Example automations

Pause media and lock the PC when you leave home:

```yaml
triggers:
  - trigger: state
    entity_id: person.you
    from: home
actions:
  - action: button.press
    target:
      entity_id: button.NODE_ID_media_play_pause
  - action: button.press
    target:
      entity_id: button.NODE_ID_lock
```

Dim house lights when the PC session locks (movie over):

```yaml
triggers:
  - trigger: state
    entity_id: binary_sensor.NODE_ID_session_locked
    to: "on"
```

## 5. Actionable notifications (buttons)

Send a toast with buttons, then react to the click. Publish to
`deskmate/NODE_ID/notify`:

```yaml
action: mqtt.publish
data:
  topic: deskmate/NODE_ID/notify
  payload: >-
    {"title": "Backup", "message": "Run nightly backup now?",
     "actions": [{"title": "Run", "action": "run"},
                 {"title": "Skip", "action": "skip"}]}
```

Clicking a button publishes `{"action": "run"}` to
`deskmate/NODE_ID/notify/action`. Catch it:

```yaml
triggers:
  - trigger: mqtt
    topic: deskmate/NODE_ID/notify/action
conditions:
  - condition: template
    value_template: "{{ trigger.payload_json.action == 'run' }}"
actions:
  - action: button.press
    target:
      entity_id: button.NODE_ID_custom_backup
```

Note: toast buttons only work while Deskmate is running (it lives in the tray).

## 6. Text-to-speech, clipboard, remote input

Enable these opt-in features in Deskmate → Settings, then:

```yaml
# Make the PC speak
action: text.set_value
target: { entity_id: text.NODE_ID_tts_say }
data: { value: "Coffee is ready" }

# Put text on the PC clipboard (Clipboard write mode: Confirm or Automatic)
action: text.set_value
target: { entity_id: text.NODE_ID_clipboard_set }
data: { value: "https://example.com" }

# Type into the focused window (enable "Remote input" first)
action: text.set_value
target: { entity_id: text.NODE_ID_type_text }
data: { value: "typed from Home Assistant" }

# Open a page in the default browser (Remote input + allowlisted origin)
action: text.set_value
target: { entity_id: text.NODE_ID_open_url }
data: { value: "https://home-assistant.io" }
```

Clipboard publication and writes are separate settings:

- **Off** removes the corresponding HA entity and rejects the operation.
- **Confirm** asks locally for every changed clipboard value being published,
  or for every write received from HA.
- **Automatic** performs the operation without prompts.

Both directions stop while Windows is locked. An approved clipboard-read value
is republished until the clipboard changes so the HA entity does not expire.
Remember that published content may remain in HA Recorder/history.

Add `https://home-assistant.io` under **Allowed URL origins** before the
`open_url` example. Notification image origins use the same allowlist;
configured HA API origins are allowed automatically.

## 7. Keep awake, hotkey triggers, camera/mic

```yaml
# Block sleep while a backup runs
action: switch.turn_on
target: { entity_id: switch.NODE_ID_keep_awake }
```

Hotkeys of kind **MQTT trigger** appear as device triggers: automation editor →
Trigger → Device → pick your computer → "hotkey: <name>". No token needed.

The opt-in **Camera in use** / **Microphone in use** binary sensors make
"recording" automations trivial:

```yaml
triggers:
  - trigger: state
    entity_id: binary_sensor.NODE_ID_mic_in_use
    to: "on"
actions:
  - action: light.turn_on
    target: { entity_id: light.desk }
    data: { color_name: red }
```

## 8. Hotkeys / widgets / tray acting on HA

Those features call HA's REST API directly. In Deskmate → Settings → "Home
Assistant API" enter your HA URL plus a long-lived access token (HA profile →
Security → Long-lived access tokens). A local URL may use HTTP only on a trusted
LAN. An optional fallback URL must use HTTPS. The token is stored in Windows
Credential Manager; use a dedicated non-admin HA user where possible.
