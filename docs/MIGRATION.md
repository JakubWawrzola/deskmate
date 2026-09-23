# Moving from MQTT to Deskmate Link

Link replaces the broker with one encrypted WebSocket to Home Assistant. Nothing
forces the move - MQTT keeps working, and its settings survive the switch - but
Link is the transport Deskmate is built around now.

## Why bother

| | MQTT | Deskmate Link |
|---|---|---|
| Setup | Install a broker, create a user, configure ACLs, point Deskmate at it | One pairing key |
| Encryption | TLS, if you configured it | AES-256-GCM at the application layer, always, on every frame |
| Away from home | Broker must be exposed or reachable over a VPN | Any path that already serves Home Assistant |
| Entities after a restart | Retained discovery | Stored declaration, same effect |
| Extra software | A broker | None |

Link encrypts inside the connection rather than relying on it, so an unencrypted
`ws://` hop on your own network is still confidential, and a reverse proxy in
front of Home Assistant carries it without any extra configuration.

## What changes and what does not

**Entity ids stay the same.** Link declares entities with the names MQTT used,
and both derive the id from the device name. Dashboards, automations and scripts
that reference `sensor.<device>_cpu_usage` keep working.

**History stays.** The entities are recreated by a different integration, so
they are new registry entries, but the ids are unchanged and the recorder keeps
following them.

**Two things need editing.** Anything calling `mqtt.publish` directly - most
often a notification script - and any automation using an MQTT device trigger
for a hotkey. Both have direct replacements:

```yaml
# before
service: mqtt.publish
data:
  topic: deskmate/workshop_pc/notify
  payload: '{"title": "Backup", "message": "Finished"}'

# after
service: deskmate_link.notify
data:
  node_id: workshop_pc
  title: Backup
  message: Finished
```

Hotkeys become `event` entities, and each press also fires
`deskmate_link_trigger` with `node_id`, `key` and `event`. An automation can
trigger on either.

## The move

1. Install the integration and pair, as described in the README. Do not change
   anything in Deskmate yet.
2. In Deskmate, open *Settings*, switch the transport to **Deskmate Link**,
   enter the WebSocket address and paste the pairing key, then save.
3. Check the Status page for `Connected (Link)`.
4. Replace the `mqtt.publish` calls and MQTT device triggers listed above.
5. Once you are satisfied, delete the old MQTT device in Home Assistant under
   *Settings → Devices & services → MQTT*. Deskmate removes its retained
   discovery topics when it stops using MQTT, so the device does not come back.

Going back is a matter of selecting MQTT again in settings. The broker address,
credentials and TLS configuration are still there.

## If you keep both

You cannot: one Deskmate instance uses one transport at a time. Different
computers can use different transports against the same Home Assistant.
