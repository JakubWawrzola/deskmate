# Home Assistant setup

## 1. Broker

Home Assistant OS: install the **Mosquitto broker** add-on (Settings >
Add-ons), start it, and make sure the **MQTT integration** is configured
(Settings > Devices & services — it appears automatically). Create a
dedicated HA user (e.g. `deskmate`) or use add-on-level credentials; those go
into the Deskmate wizard.

Other setups: any MQTT 3.1.1 broker works. Point HA's MQTT integration and
Deskmate at the same broker.

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

# Put text on the PC clipboard (enable the Clipboard sensor first)
action: text.set_value
target: { entity_id: text.NODE_ID_clipboard_set }
data: { value: "https://example.com" }

# Type into the focused window (enable "Remote input" first)
action: text.set_value
target: { entity_id: text.NODE_ID_type_text }
data: { value: "typed from Home Assistant" }
```
