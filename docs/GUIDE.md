# Deskmate guide

Everything you need to use Deskmate, in the order you will need it. Start at the
top; stop when you have what you wanted. Protocol details, the threat model and
release history live in separate documents linked at the end.

- [1. What you are installing](#1-what-you-are-installing)
- [2. Install the Home Assistant integration](#2-install-the-home-assistant-integration)
- [3. Install the Windows app and pair it](#3-install-the-windows-app-and-pair-it)
- [4. What shows up in Home Assistant](#4-what-shows-up-in-home-assistant)
- [5. Notifications on the PC](#5-notifications-on-the-pc)
- [6. Controlling the PC](#6-controlling-the-pc)
- [7. Controlling your home from the PC](#7-controlling-your-home-from-the-pc)
- [8. Files between your phone and the PC](#8-files-between-your-phone-and-the-pc)
- [9. Automation ideas](#9-automation-ideas)
- [10. Several computers, several Windows users](#10-several-computers-several-windows-users)
- [11. Privacy switches](#11-privacy-switches)
- [12. Updates](#12-updates)
- [13. Troubleshooting](#13-troubleshooting)
- [14. Using MQTT instead](#14-using-mqtt-instead)
- [Further reading](#further-reading)

## 1. What you are installing

Two parts that talk to each other:

- **The Deskmate integration** in Home Assistant. It receives the PC's sensors,
  sends it commands, and adds a **Deskmate** page to the sidebar.
- **The Deskmate app** on Windows 10 or 11 (x64 or ARM64). A small tray app: no
  window needs to stay open and no background service is installed. It runs
  while a Windows user is signed in.

They connect over one encrypted connection that the app opens to Home
Assistant, so nothing on the PC is exposed to the network and no MQTT broker is
needed. In the code and in [LINK.md](LINK.md) this connection is called
*Deskmate Link*.

## 2. Install the Home Assistant integration

With HACS:

1. HACS → three-dot menu (top right) → **Custom repositories**.
2. Repository `https://github.com/JakubWawrzola/deskmate`, type **Integration**,
   **Add**. HACS does not add an unknown repository from a link, so this step
   comes first.
3. Search HACS for **Deskmate**, download it, restart Home Assistant.

Without HACS: copy `custom_components/deskmate_link` from this repository into
`config/custom_components/` on your Home Assistant and restart.

Then **Settings → Devices & services → Add integration → Deskmate**. Home
Assistant shows a **pairing code** that starts with `DMP1.`. Keep that dialog
open, or copy the code somewhere: it holds the key and your Home Assistant's
address. Closed it too early? Add the integration again and the same code comes
back; no second entry is created.

Home Assistant 2024.11 or newer. The integration icon appears from Home
Assistant 2026.3 on.

## 3. Install the Windows app and pair it

1. Download `Deskmate_<version>_x64-setup.exe` (most PCs) or `_arm64-setup.exe`
   (Snapdragon laptops) from
   [Releases](https://github.com/JakubWawrzola/deskmate/releases) and run it.
   The installer is not code-signed, so SmartScreen warns about an unknown
   publisher: *More info → Run anyway*.
2. It installs for your Windows account only and needs no administrator
   rights. Another account on the same PC installs it again for itself (see
   [section 10](#10-several-computers-several-windows-users)).
3. On first start paste the pairing code and choose **Connect**. The address is
   filled in from the code.

The Status page should say **Connected to Home Assistant** within a few
seconds, and the PC appears as a device under *Settings → Devices & services →
Deskmate*.

If Home Assistant does not know its own address (*Settings → System →
Network*), the code has none and you type it into Deskmate yourself:

| Where Home Assistant is | Address |
|---|---|
| Same network | `ws://homeassistant.local:8123` or `ws://192.168.1.50:8123` |
| Behind a reverse proxy, Cloudflare Tunnel or Nabu Casa | `wss://your-domain.example` |
| Over Tailscale or another VPN | `ws://100.x.x.x:8123` |

A second, fallback address can be added for when you are away from home.
Plain `ws://` is accepted only for local and VPN addresses.

Two things worth doing right away:

- **Device name** (Settings): entity ids are built from it, so
  `Workshop PC` gives `sensor.workshop_pc_cpu_usage`. Set it before the first
  connection if the naming matters; Home Assistant does not rename entities
  later.
- **Start with Windows** (Settings), so the PC reports after every reboot.

Settings are stored in `%APPDATA%\Deskmate\config.json`. Keys and tokens go to
Windows Credential Manager, never into that file.

## 4. What shows up in Home Assistant

One device per PC, with:

- **Sensors**: CPU, memory, disk, network up and down, battery, uptime, idle
  time, session locked, current user, "presenting or full screen". With
  supported hardware also GPU load and temperature, VRAM, per-drive usage and
  speed, CPU temperature. Sensors a PC cannot read are simply not created.
- **Buttons**: Lock, Sleep, Hibernate, Shutdown, Restart, Monitors off, media
  keys, empty recycle bin. Shutdown and Restart wait 5 seconds.
- **Controls**: volume slider, mute, Keep awake (stops sleep and screen-off
  while it is on).
- **Privacy-sensitive sensors**, off until you enable them on the Sensors page:
  active window title, Wi-Fi name, what is playing, camera in use, microphone
  in use.

Turning a sensor off in Deskmate removes its entity from Home Assistant.

## 5. Notifications on the PC

Native Windows notifications, with an optional image and buttons:

```yaml
action: deskmate_link.notify
data:
  title: Doorbell
  message: Someone is at the front door
  image: https://your-ha.example/local/doorbell.jpg
  actions:
    - id: open_gate
      title: Open gate
    - id: ignore
      title: Ignore
```

Leave out `node_id` to notify every paired PC, or set it to one PC's node id
(shown in Deskmate under Settings). A clicked button fires the event
`deskmate_link_notify_action` with `node_id` and `action`:

```yaml
triggers:
  - trigger: event
    event_type: deskmate_link_notify_action
    event_data:
      action: open_gate
actions:
  - action: switch.turn_on
    target:
      entity_id: switch.gate
```

Images are downloaded by the PC, so their address must be allowed under
Settings → **Allowed URL origins**. Addresses entered under Settings → Home
Assistant API are allowed automatically.

## 6. Controlling the PC

Besides the built-in buttons:

- **Your own commands** (Commands page): any PowerShell command as a Home
  Assistant button, switch or slider. Each one is off until you enable it and
  can ask for confirmation on the PC first. A slider's value reaches the script
  as `$env:DESKMATE_VALUE`, never inside the command text.
- **Remote input** (Settings, off by default): Home Assistant can type text
  into the focused window, drive a presentation and open allowed web pages.
  Typed text is printable characters only, so it cannot press Enter on its own.
- **Text to speech** (Settings, off by default): the PC reads out text sent from
  Home Assistant.
- **Clipboard** (Settings): reading and writing are separate, each *Off*,
  *Confirm* or *Automatic*. Both pause while Windows is locked.

## 7. Controlling your home from the PC

These need a Home Assistant **long-lived access token** (your Home Assistant
profile → Security → Create token) entered under Settings → Home Assistant API.

- **Hotkeys**: system-wide shortcuts, for example `Ctrl+Alt+L` toggles the desk
  lamp from any app. A hotkey can toggle an entity, call any action with data,
  run a local command, or just send an event: each hotkey also appears in Home
  Assistant as an event entity `hotkey: <name>` and fires
  `deskmate_link_trigger` with `key: hotkey_<id>`.
- **Widgets**: a small always-on-top panel with tiles for the entities you pick.
  Open it from the tray (*Show/hide widgets*) or a hotkey, move it by its top
  bar, close it with `x`.
- **Tray quick actions**: your own entries in the tray menu.
- **Stream Deck**: a separate [plugin](../streamdeck-plugin/README.md).

## 8. Files between your phone and the PC

Open **Deskmate** in the Home Assistant sidebar (also in the mobile app). It has
two tabs: **Files** and **Computers**.

Sending to the PC needs **Receive files** on the PC (Deskmate → Settings):

- *Ask on this computer*: a dialog for every file. Refused while Windows is
  locked.
- *Accept automatically*: no questions.

Files land in `Downloads\Deskmate` unless you pick another folder, are never
overwritten (`photo (1).jpg`), and Windows treats them as downloaded from the
internet. Size limit 256 MB per file by default.

Downloading from the PC needs **File access**: add specific folders there. Home
Assistant can read only inside them, never write.

For automations there are two actions:

```yaml
action: deskmate_link.send_file
data:
  path: /media/snapshots/doorbell.jpg
```

`deskmate_link.fetch_file` copies a file from a shared folder into Home
Assistant. Both only touch Home Assistant paths listed in
`allowlist_external_dirs` (`/media` is by default).

## 9. Automation ideas

Lock the PC when you walk away (use your own presence sensor):

```yaml
triggers:
  - trigger: state
    entity_id: binary_sensor.office_presence
    to: "off"
    for: "00:02:00"
conditions:
  - condition: state
    entity_id: binary_sensor.my_pc_session_locked
    state: "off"
actions:
  - action: button.press
    target:
      entity_id: button.my_pc_lock
```

Turn on an "on air" light while the camera is in use (enable the Camera in use
sensor first):

```yaml
triggers:
  - trigger: state
    entity_id: binary_sensor.my_pc_camera_in_use
actions:
  - action: "light.turn_{{ 'on' if trigger.to_state.state == 'on' else 'off' }}"
    target:
      entity_id: light.on_air
```

Keep the PC awake while a backup runs, then let it sleep:

```yaml
actions:
  - action: switch.turn_on
    target:
      entity_id: switch.my_pc_keep_awake
  - wait_for_trigger:
      - trigger: state
        entity_id: sensor.backup_state
        to: idle
  - action: switch.turn_off
    target:
      entity_id: switch.my_pc_keep_awake
```

A weekly restart at 4 am when nobody has touched the PC for an hour:

```yaml
triggers:
  - trigger: time
    at: "04:00:00"
conditions:
  - condition: time
    weekday: sun
  - condition: numeric_state
    entity_id: sensor.my_pc_idle_time
    above: 3600
actions:
  - action: button.press
    target:
      entity_id: button.my_pc_restart
```

Replace `my_pc` with your device's name as it appears in its entity ids.
Turning a PC back **on** after Shutdown cannot come from Deskmate, which is not
running then; Home Assistant's Wake on LAN integration does that for PCs on
Ethernet.

## 10. Several computers, several Windows users

**Another computer**: add the integration again in Home Assistant, paste the new
code into Deskmate on that computer. Each computer is its own device.

**Another Windows account on the same computer** (a family PC, say). Each
account runs its own Deskmate, and all of them should report as the same
device:

1. Sign in to the other account and install Deskmate there as well.
2. In Home Assistant: **Settings → Devices & services → Deskmate**, the
   computer's entry → **Reconfigure → Pair another Windows user**. It shows a
   code for that computer.
3. Paste that code into Deskmate on the other account.

Only one account is connected at a time: the one someone is using. Switch users
and within a few seconds the connection moves to the new account; the other one
shows *Another Windows user on this computer holds the connection*. When
nobody else is using the PC, an account that stays signed in in the background
keeps reporting.

The code contains this computer's pairing key, so give it only to accounts on
that computer. *Reconfigure → Generate a new pairing key* cuts off every account
at once; paste the new code on each one you want to keep.

## 11. Privacy switches

Everything that can reveal what you do, or lets Home Assistant act on the PC
beyond the built-in buttons, is off until you turn it on:

| Feature | Where | Default |
|---|---|---|
| Window title, Wi-Fi name, media, camera, microphone sensors | Sensors | Off |
| Custom commands | Commands | Off, ask before running |
| Remote input (typing, presentation, opening pages) | Settings | Off |
| Text to speech | Settings | Off |
| Clipboard read / write | Settings | Off |
| Receive files | Settings | Off |
| File access (shared folders) | Settings | No folders |
| Daily update check (asks GitHub only) | Settings → Updates | On |

Every security-relevant action is logged locally in
`%APPDATA%\Deskmate\security.log`, without contents. The full threat model is in
[SECURITY.md](SECURITY.md).

## 12. Updates

Deskmate asks GitHub once a day whether a newer release exists. It does not
download or install anything: a new version shows up in the tray menu, on the
Status page and once as a notification. Settings → **Updates** turns it off or
checks right away.

To update, run the new installer over the old version. Settings and pairing are
kept. Update the Home Assistant integration through HACS; the release notes say
when the order matters.

## 13. Troubleshooting

**The Status page shows a rejection or a clock mismatch.** The message says
which; [LINK.md](LINK.md#when-the-connection-is-refused) lists every reason and
the fix. The usual ones: the code was pasted on the wrong computer, the entry
was unbound, or the PC's clock is more than 90 seconds off.

**Nothing happens when I start Deskmate.** It is already running in the tray
(near the clock, possibly behind the `^` arrow). Starting it again from Start
now brings the window to the front.

**Notifications do not appear.** Check Windows *Do not disturb* and
*Settings → System → Notifications → Deskmate*. If they still do not show,
turn off *Branded toasts* in Deskmate Settings: notifications then carry the
"Windows PowerShell" label, which always renders.

**My antivirus quarantined deskmate.exe.** Deskmate 0.7.0 and older created
their notification shortcut by running PowerShell with compiled C#, which looks
like malware to some products (Bitdefender, for one). Newer versions do this
inside the app and start no PowerShell for notifications. The installers are
also not code-signed, which makes heuristics more suspicious. Restore the file,
update, and if it happens again report it as a false positive to your antivirus
vendor; custom commands you define yourself do run PowerShell by design.

**Entities are unavailable after switching Windows users.** The other account
needs Deskmate as well, paired with *Pair another Windows user* (section 10).

**The widget panel will not move.** Drag it by the bar at the top. Versions up
to 0.7.0 lacked the window permission for that.

**The integration has no icon.** Local brand icons need Home Assistant 2026.3
or newer.

Still stuck: [open an issue](https://github.com/JakubWawrzola/deskmate/issues)
with the Status page text and, from Home Assistant, the integration's
diagnostics download (keys are removed from it).

## 14. Using MQTT instead

MQTT remains supported for setups that already run a broker: Settings →
Transport → MQTT broker. Broker, TLS and ACL setup, and MQTT-specific examples
are in [HA-SETUP.md](HA-SETUP.md). Moving an existing MQTT setup to the
integration without changing entity ids: [MIGRATION.md](MIGRATION.md).

## Further reading

- [LINK.md](LINK.md): the connection, pairing, remote access, protocol v2,
  cascade encryption
- [SECURITY.md](SECURITY.md): threat model, defaults, residual risks
- [AI-DEPLOY.md](AI-DEPLOY.md): the setup procedure written for an AI assistant
- [CHANGELOG.md](../CHANGELOG.md) and [releases/](releases/): what changed when
- [dev/](dev/): architecture, plans and notes for contributors
