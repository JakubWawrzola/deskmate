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
- [11. Screen time and family rules](#11-screen-time-and-family-rules)
- [12. Privacy switches](#12-privacy-switches)
- [13. Updates](#13-updates)
- [14. Troubleshooting](#14-troubleshooting)
- [15. Using MQTT instead](#15-using-mqtt-instead)
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

On Linux, a daemon brings the sensors and power buttons: see [LINUX.md](LINUX.md).

## 2. Install the Home Assistant integration

With HACS (Deskmate is in the default catalog, no custom repository needed):

1. Open HACS and search for **Deskmate**.
2. Download it and restart Home Assistant.

If it does not show up in the search yet, HACS has not refreshed its catalog:
wait a few hours, or add `https://github.com/JakubWawrzola/deskmate` once under
the three-dot menu → **Custom repositories** (type **Integration**). The entry
is replaced by the catalog one on its own.

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
  The choice survives updates. **Start minimized to tray** right below it
  keeps the window closed when Deskmate starts. Both apply as soon as you
  switch them, without *Save & reconnect*.

Settings → *Appearance* switches between light, dark and following Windows.

Settings are stored in `%APPDATA%\Deskmate\config.json`. Keys and tokens go to
Windows Credential Manager, never into that file.

## 4. What shows up in Home Assistant

One device per PC, with:

- **Sensors**: CPU, memory, disk, network up and down, battery, uptime, idle
  time, session locked, current user, "presenting or full screen", *Screen
  on*, and (off by default) *Last active*, the time of the last keyboard or
  mouse input, handy for presence. With supported hardware
  also GPU load and temperature, VRAM, per-drive usage and speed. Sensors a PC
  cannot read are simply not created.
- **Idle time** counts game controllers as well as the keyboard and mouse:
  someone playing with an Xbox-style controller is not idle. Sticks that
  drift a little at rest do not count.
- **User state** (off by default, a privacy-sensitive sensor) says what
  Windows thinks you are doing: `normal`, `fullscreen` (a full-screen app),
  `game` (a full-screen game), `presentation` (Windows presentation settings
  are on) or `busy` (the screen saver is on, the PC is locked, or another
  user's session is active).
- **Screen on** follows the PC's display: off when Windows turned the screen
  off, on when it is on or dimmed.
- **CPU temperature**: Windows only lets programs with administrator rights
  read it. Settings → *CPU temperature* sets up a small helper for that, see
  [below](#cpu-temperature). Running LibreHardwareMonitor works too.
- **Buttons**: Lock, Sleep, Hibernate, Shutdown, Restart, Sign out, Monitors
  off, media keys, empty recycle bin. Shutdown and Restart wait 5 seconds.
- **Controls**: volume slider, mute, Keep awake (stops sleep and screen-off
  while it is on), and *Screen brightness* from 0 to 100 %. Brightness works
  on a laptop's own screen and on monitors that support DDC/CI (many have it
  as an option in their own menu). It sets every such screen at once and
  appears once a screen has answered since Deskmate started. A screen that
  stops answering for a while, in standby or behind a closed lid, leaves the
  control in place; until a screen answers again it shows no value and
  refuses new ones. The Sensors page can switch it off.
- **Sound**: *Mute microphone*, a *Microphone volume* slider, and *Sound
  output* / *Sound input* lists that switch the Windows default device (the
  speakers, a headset, a USB microphone). Plugging a device in or out updates
  the lists. See [sound devices](#sound-devices-and-the-microphone).
- **Privacy-sensitive sensors**, off until you enable them on the Sensors page:
  active window title, Wi-Fi name, what is playing, camera in use, microphone
  in use, user state.
- **Media player**: with a Deskmate that supports it, a *Media* entity shows
  what the PC is playing and has play/pause, next, previous, volume and mute
  controls that work in any media card. It appears once you turn on *Media
  status* in Deskmate, because it reveals what you play. It shows the title,
  artist and app only while their own sensors are on, and the volume while
  the *Volume* sensor is on; the separate media sensors and buttons stay as
  they are.
- **Connection**: a diagnostic sensor that Home Assistant adds itself. It says
  what is connected for this PC (*App*, *Service*, *Daemon*, or *Connected*
  for a Deskmate that does not say what it is) and, when nothing is, whether
  the PC went to sleep (*Sleeping*), shut down (*Off*) or just dropped off
  (*Disconnected*). It stays available while the PC is offline, so
  automations can react to it.

Turning a sensor off in Deskmate removes its entity from Home Assistant.
Drives, the GPU and temperatures are found when Deskmate starts and have their
own panel on the Sensors page, **Hardware**, with *All on* and *All off*. Every
drive is its own set of entities; *Disk usage* in the system sensors is the
system drive only.

### Your own sensors

Sensors page → *Your sensors* adds sensors of your own:

| Type | Shows | Example |
|---|---|---|
| Program running | on/off | `steam.exe` |
| Window open | on/off: a visible window whose title contains the text | `Microsoft Teams` |
| Windows service state | `running`, `stopped`, `paused`, ... | `Spooler` |
| PowerShell output | the first line a command prints (optional unit) | `(Get-Process chrome -ErrorAction SilentlyContinue).Count` |

PowerShell sensors run in the background on their own interval (30 seconds or
more), keep their last value if a run fails, and start disabled: enable one
after checking the command.

### CPU temperature

1. Install [PawnIO](https://pawnio.eu), a signed open-source driver for reading
   hardware sensors (also `winget install namazso.PawnIO`).
2. Deskmate → Settings → *CPU temperature* → *Install helper*. Windows asks for
   administrator approval once.

The helper is a copy of Deskmate in Program Files that Windows starts with
administrator rights at sign-in. It reads the processor's temperature through
PawnIO and hands it to Deskmate, which keeps running without those rights. The
*CPU temperature* sensor then appears in Home Assistant. Intel and AMD Ryzen
(and newer) on x64; ARM processors have no such sensor. *Remove* in the same
panel takes the helper away again; uninstalling Deskmate leaves it idle.

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
(shown in Deskmate under Settings). A PC that is off, or that nobody is
signed in to, does not stop the others: every PC is tried, and the action
then fails with an error that names each PC that did not get the
notification and why. A clicked button fires the event
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

Two optional fields need a Deskmate that supports them. `tag` names a
notification (letters, digits, `_` and `-`, up to 64): a new notification with
the same tag replaces the old one, so a doorbell that rings twice shows one
notification. `priority` is `normal` (the default), `high` or `urgent`;
`urgent` asks Windows to show the notification even while Do not disturb is
on, as far as Windows allows it. An older Deskmate shows the notification
without them.

`deskmate_link.clear_notification` removes a tagged notification from the
screen and from the notification centre, for example once the door is open:

```yaml
action: deskmate_link.clear_notification
data:
  tag: doorbell
```

Like `notify`, it goes to every paired PC unless you set `node_id`. A PC
that is off, or whose Deskmate is too old to clear notifications, does not
stop the others: every PC is tried, and the action then fails with an error
that names each PC it could not clear and why.

Windows asks you once whether to allow important notifications from
Deskmate, and the choice stays yours under *Settings → System →
Notifications*.

## 6. Controlling the PC

Besides the built-in buttons:

- **Your own controls** (Commands page), each off until you enable it and
  able to ask for confirmation on the PC first:
  - *Start a program*: a button that opens a program, file, shortcut or URL
    (`steam://...` works), with optional arguments;
  - *Stop a program*: a button that closes every running copy of a program by
    name, e.g. `steam.exe`;
  - *Press keys*: a button that presses a key combination in the window in
    front, e.g. `Ctrl+F5` to reload the page in a browser, `Win+D` to show the
    desktop, or several in a row (`Ctrl+L, Ctrl+C`). You set the keys on the
    PC; Home Assistant can only press the button;
  - *PowerShell script*: any command as a button, switch or slider. A slider's
    value reaches the script as `$env:DESKMATE_VALUE`, never inside the
    command text.

  Start, stop and keys run without PowerShell. Together with a *Program
  running* sensor they make a program controllable from Home Assistant.
  Keys cannot reach a window running as administrator, nor anything while
  Windows is locked. With *Confirm* on, the key press waits for your answer.
- **Remote input** (Settings, off by default): Home Assistant can type text
  into the focused window, drive a presentation, open allowed web pages and
  press *Wake screen*. Typed text is printable characters only, so it cannot
  press Enter on its own. *Wake screen* turns the screens back on as a tiny
  mouse nudge would; the pointer does not move, and while the PC is locked
  Windows can refuse it. Windows counts all of this as your input: it resets
  the idle time, so it also postpones the screen saver, the lock after
  inactivity and an automatic update install.
- **Text to speech** (Settings, off by default): the PC reads out text sent from
  Home Assistant.
- **Clipboard** (Clipboard page): reading and writing are separate, each *Off*,
  *Ask* or *Automatically*. Both pause while Windows is locked.

### Sound devices and the microphone

*Mute microphone* mutes the default recording device, and the default device
for calls when that is a different one (a headset next to a webcam
microphone). *Sound output* and *Sound input* select a device by its Windows
name and make it the default for apps, media and calls at once, like *Set as
default device* in the Windows sound settings. The Sensors page → *Sound*
switches the volume sliders and the device lists off if you do not want them.

Mute the microphone while the radio plays:

```yaml
triggers:
  - trigger: state
    entity_id: media_player.kitchen_radio
    to: playing
actions:
  - action: switch.turn_on
    target:
      entity_id: switch.my_pc_mute_microphone
```

Switch to the headset when it is turned on (any sensor that tells you so):

```yaml
triggers:
  - trigger: state
    entity_id: binary_sensor.headset_power
    to: "on"
actions:
  - action: select.select_option
    target:
      entity_id: select.my_pc_sound_output
    data:
      option: Headphones (WH-1000XM4)
```

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
- **Dashboards**: Settings → *Home Assistant dashboard* opens a Home Assistant
  dashboard in a Deskmate window, from the tray (*Open Home Assistant*), a
  hotkey or a quick action (*Open HA dashboard*, optionally with its own path
  such as `lovelace/office`). Log in once; the session is kept. This one needs
  no token.
- **Stream Deck**: keys for entities, services, scenes and your Deskmate
  hotkeys and tray quick actions. Settings → *Stream Deck* installs the plugin
  into Stream Deck (*Install plugin*, then restart Stream Deck) and has the
  switch *Let Stream Deck use Deskmate*, which lets the plugin work through
  Deskmate without a token of its own. The panel also shows whether the plugin
  is connected, and offers *Update plugin* and *Remove plugin*. See
  [STREAMDECK.md](STREAMDECK.md).

## 8. Files between your phone and the PC

Open **Deskmate** in the Home Assistant sidebar (also in the mobile app). It has
two tabs: **Files** and **Computers**.

Sending to the PC needs **Receive files** on the PC (Deskmate → **Files**):

- *Ask on this computer*: a dialog for every file. Refused while Windows is
  locked.
- *Accept automatically*: no questions.

Files land in `Downloads\Deskmate` unless you pick another folder, are never
overwritten (`photo (1).jpg`), and Windows treats them as downloaded from the
internet. Size limit 256 MB per file by default.

Downloading from the PC needs **Shared folders** on the same page: add
specific folders there. Home Assistant can read only inside them, never write.
*Recent transfers* on that page lists what was received or read, from the local
security log.

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
   computer's entry → **Reconfigure → Show the pairing code**. It shows the
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

### When nobody is signed in: the Deskmate service

Without a signed-in user the Deskmate app does not run, and the computer is
unavailable in Home Assistant. The Deskmate service fills that gap: it keeps
the computer's sensors and its power buttons in Home Assistant at the sign-in
screen, after a restart and while everyone is signed out. It never shows
notifications, receives files or sees anything from a user's session.

It needs the Deskmate integration (not MQTT), a paired computer and an
integration recent enough for the service; Deskmate checks that while it is
connected. To install it:

1. Open **Settings → Deskmate service** in Deskmate and press **Install**.
2. Approve the Windows prompt for `deskmate-service.exe`. An administrator
   can approve it with their own account.
3. The panel shows *Running: waiting while the Deskmate app holds the
   connection*. That is right: while you are signed in, the app connects.

In Home Assistant the computer's **Connection** sensor says who is connected:
`app` while you are signed in, `service` while nobody is, `sleeping` while the
computer sleeps, `off` after a shutdown, `disconnected` otherwise. While the
service is connected, the sensors that need a user (active window, media,
volume and so on) and the controls that act in a session are unavailable; the
processor, memory, system drive, network, uptime and battery sensors and the
Sleep, Hibernate, Shut down and Restart buttons keep working. When you sign in
or out, the connection moves between the app and the service within a few
seconds, and Home Assistant keeps the computer available meanwhile. For five
minutes after the computer starts or wakes up, the service refuses the power
buttons, so a misfiring automation cannot keep it off.

The panel tells you when something needs doing:

- **Needs update**: a Deskmate update brought a newer service. Press
  **Update**. Nothing is updated without you.
- **Settings differ**: you changed the address, the pairing, the device name
  or the sensors in the app. Press **Apply** to give the service the current
  ones.
- **Integration too old**: update the Deskmate integration in HACS.

If you press **Disconnect** on the Status page, the service does not connect
either until you press **Connect**.

To remove the service, press **Remove** in the same panel and approve the
prompt. Uninstalling Deskmate removes it as well; updating Deskmate does not,
also when you run the new installer by hand.

## 11. Screen time and family rules

Microsoft Family Safety needs a Microsoft account and its cloud, and the Home
Assistant integrations for it run on an unofficial API: people report sign-ins
that expire after a day, five-minute delays and limits that never reach the PC.
Deskmate counts screen time on the PC itself and enforces the rules there, also
while Home Assistant or the internet is down. Home Assistant only changes the
rules.

**Turn it on** in the child's Windows account: Deskmate → **Family** → *Count
screen time on this account*. Time counts while the account is unlocked and in
use (keyboard or mouse in the last 5 minutes, or something playing full
screen). It starts from zero at midnight.

The computer's device in Home Assistant then gets:

| Entity | What it does |
|---|---|
| Screen time today | Minutes in use today |
| Screen time left | Minutes left of the daily limit; unknown without a limit |
| Screen time status | `off`, `ok`, `time_up` or `outside_hours` |
| Screen time rules | Switch: enforce the limit and the allowed hours |
| Daily screen time limit | Minutes per day, 0 for no limit |
| Extra time today | Minutes on top of the limit, back to 0 at midnight |
| Allowed from, Allowed until | `HH:MM`, empty for no schedule; `22:00` to `02:00` works across midnight |
| When time is up | *Lock* or *Sign out* |
| Block apps, Blocked apps | Switch and program names separated by commas (`steam, robloxplayerbeta`); they close as soon as they start |
| Lock with message | Text shown on the PC, which locks 10 seconds later |

A *Sign out* button is there on every PC, family rules or not.

What the child sees: a notification 10, 5 and 1 minute before the limit or the
end of the allowed hours. When the time is up, a notification with *Ask for
more time* and one minute to save, then the PC locks (or signs out). Unlocking
again locks after 20 seconds, until there is time again. Open programs keep
running behind the lock screen.

**Ask for more time** fires `deskmate_link_notify_action` with `action:
screen_time_more` (over MQTT: the `notify/action` topic). This automation asks
you on your phone and adds 15 minutes:

```yaml
triggers:
  - trigger: event
    event_type: deskmate_link_notify_action
    event_data:
      action: screen_time_more
actions:
  - action: notify.mobile_app_my_phone
    data:
      message: The kids' PC asks for more screen time
      data:
        actions:
          - action: GRANT_15
            title: +15 minutes
  - wait_for_trigger:
      - trigger: event
        event_type: mobile_app_notification_action
        event_data:
          action: GRANT_15
    timeout: "00:10:00"
    continue_on_timeout: false
  - action: number.set_value
    target:
      entity_id: number.kids_pc_extra_time_today
    data:
      value: "{{ states('number.kids_pc_extra_time_today') | int(0) + 15 }}"
```

**Protect the account.** A child can quit Deskmate or edit its settings file.
Give the child a standard (not administrator) Windows account, then in that
account: Family → *Protect this account*, and type an administrator's password
once. From then on:

- Windows starts Deskmate again within a minute when it is not running (the
  scheduled task *Deskmate watchdog*);
- the rules can only be changed in Home Assistant; the Family page shows them
  read-only;
- Home Assistant remembers the rules and puts them back when the PC reports
  something else (Deskmate integration; over MQTT the PC keeps what it has);
- *Remove protection* needs the administrator password again.

Protection does not stop an administrator account, another Windows account
without Deskmate, or starting the PC from another system. The *Connection*
entity in Home Assistant shows when the PC goes offline, and a standard
account cannot change the clock.

Several children on one PC: each Windows account counts its own time. The rules
set in Home Assistant apply to the account that is connected, because the
device in Home Assistant is the computer.

## 12. Privacy switches

Everything that can reveal what you do, or lets Home Assistant act on the PC
beyond the built-in buttons, is off until you turn it on:

| Feature | Where | Default |
|---|---|---|
| Window title, Wi-Fi name, media, camera, microphone sensors | Sensors | Off |
| Custom commands and program controls | Commands | Off, ask before running |
| PowerShell sensors | Sensors → Your sensors | Off |
| CPU temperature helper (administrator, at sign-in) | Settings → CPU temperature | Not installed |
| Remote input (typing, presentation, opening pages) | Settings | Off |
| Text to speech | Settings | Off |
| Clipboard read / write | Clipboard | Off |
| Receive files | Files | Off |
| Shared folders | Files | No folders |
| Screen time and family rules | Family | Off |
| Stream Deck channel (programs running as you reach Home Assistant through Deskmate) | Settings → Stream Deck | Off |
| Daily update check | Settings → Updates | On, asks before it installs |
| Installing updates without asking | Settings → Updates | Off |

Every security-relevant action is logged locally in
`%APPDATA%\Deskmate\security.log`, without contents. The full threat model is in
[SECURITY.md](SECURITY.md).

## 13. Updates

Deskmate asks GitHub once a day whether a newer release exists. A new version
shows up in the tray menu, on the Status page and once as a notification.
Settings → **Updates** decides what happens next:

- **Ask first** (the default). The notification and the Status page offer
  *Install now* and *Later*. *Install now* downloads the installer, checks its
  signature and installs it; Deskmate restarts by itself. *Later* asks again
  the next day, and the tray entry brings the choice back sooner.
- **Install automatically.** Deskmate does the same without asking, once the
  computer has not been used for five minutes and nothing runs full screen. A
  notification tells you afterwards.
- **Only tell me.** Nothing is downloaded. The tray entry and the Status page
  lead to the release page, and you run the installer yourself.

The same panel turns the daily check off or checks right away. With the check
off, Deskmate never looks for, downloads or installs an update by itself.

An update keeps your settings, the pairing, *Start with Windows*, the CPU
temperature helper and the family protection. Deskmate runs only an installer
signed with its release key, and Home Assistant cannot start an update; see
[SECURITY.md](SECURITY.md#updates).

You can always update by hand: run the new installer over the old version.
Keep *Uninstall before installing* and leave *Delete the application data*
unticked; the Deskmate service then stays installed.
Update the Home Assistant integration through HACS; the release notes say when
the order matters.

## 14. Troubleshooting

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

**An update does not install.** The Status page says why the last attempt
stopped, and *Release page* opens the download: running that installer over
the old version always works. *Install automatically* does not try a failed
version again by itself while Deskmate keeps running, daily checks included;
press *Try again* when the cause is fixed. If Deskmate does not come back after an update,
start it again; the Status page then offers the update once more.

**Entities are unavailable after switching Windows users.** The other account
needs Deskmate as well, paired with *Show the pairing code* (section 10).

**Sound output does not change the device.** The list holds the devices
Windows has enabled. A device that is disabled in the Windows sound settings
does not show up. A program that picked a device by itself (some games and
call apps) keeps using it; change it in that program.

**The family rules lock the PC but the child keeps playing.** Check that the
child's account is a standard account and protected (Family page). Without
protection, closing Deskmate stops the rules until it starts again.

**"Connection error ... (os error 10013)".** Windows refused the connection
before it left the PC. That is almost always a firewall or antivirus blocking
`deskmate.exe`, often after an update or reinstall because the program file
changed. Allow Deskmate in it (in Bitdefender: the firewall's application
rules), then press *Reconnect* on the Status page. Deskmate 0.7.3 and
newer say this in the status text.

**A sensor I turned off is still in Home Assistant.** Check the *Hardware*
panel on the Sensors page: each drive, the GPU and the temperatures have their
own switch there. Versions before 0.8.3 did not list them, so they could not be
turned off.

**I disabled Deskmate in Home Assistant and the PC keeps trying to connect.**
Press *Disconnect* on the Status page, or *Disconnect from Home Assistant* in
the tray menu. Deskmate stays disconnected, also after a restart, until you
press *Connect*.

**I reinstalled Deskmate and it does not connect.** You do not need a new
entry in Home Assistant. Open the computer's entry → *Reconfigure* → *Show the
pairing code* and paste that code into Deskmate. Do not choose *Generate a new
pairing key* for this: it works too, but every other copy of the old key
(another Windows account, a second install) stops connecting.

**Starting completely over.**

1. Windows: *Settings → Apps → Deskmate → Uninstall* and tick *Delete the
   application data*. Since 0.7.3 that also removes the settings
   (`%APPDATA%\Deskmate`) and the saved keys. Files received from Home
   Assistant stay where they are.
2. Older versions leave the keys in *Control Panel → Credential Manager →
   Windows Credentials → Generic Credentials*. Remove the entries ending in
   `.Deskmate Link`, `.Deskmate Link Cascade`, `.Deskmate HA Token` and
   `.Deskmate MQTT` (the part before the dot is the computer's node id or
   your MQTT user), and the folder `%APPDATA%\Deskmate`.
3. Home Assistant: delete the computer's entry under *Settings → Devices &
   services → Deskmate*, then *Add entry* for a new pairing code.
4. Install Deskmate again and paste the new code.

**The widget panel will not move.** Drag it by the bar at the top. Versions up
to 0.7.0 lacked the window permission for that.

**The integration has no icon.** Local brand icons need Home Assistant 2026.3
or newer.

Still stuck: [open an issue](https://github.com/JakubWawrzola/deskmate/issues)
with the Status page text and, from Home Assistant, the integration's
diagnostics download (keys are removed from it).

## 15. Using MQTT instead

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
