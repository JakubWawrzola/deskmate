# DeskmateTV

DeskmateTV puts a Deskmate on your Android TV or Google TV. It lets a Home
Assistant voice assistant see the TV screen as plain text and act on it: open
apps, pick items, type into search fields, read what is playing, change the
volume or the HDMI input, and show messages on screen. The app is silent: no
speech, no sounds, nothing like TalkBack reading the screen out loud.

It uses the same encrypted [Deskmate Link](LINK.md) connection as Deskmate on
Windows. The TV connects out to Home Assistant, so nothing listens on the TV.

Source: [`tv/`](../tv/README.md). Home Assistant side:
[`custom_components/deskmate_link`](../custom_components/deskmate_link).

## Why text and not screenshots

An assistant that looks at screenshots spends a lot of tokens and still has to
guess where to click. The accessibility tree already knows every button, card
and text field, so DeskmateTV sends that instead, compressed to a few hundred
tokens:

```
app=Netflix (com.netflix.ninja) n=12 focus=#7
#1 tab "Home" [sel]
#2 tab "Search"
#7 item "Sex Education" [focus]
#9..#44 keys "abcdefghijklmnopqrstuvwxyz1234567890"
```

Clickable containers merge their children into one line, the way a screen
reader announces a card. On-screen keyboards collapse into a single `keys`
line. A screenshot is still available for the rare app that draws its own UI,
but only when the caller asks for one.

## Requirements

- Android TV or Google TV with Android 9 or newer. Android 13 or newer adds
  D-pad keys through the accessibility service; on older systems use the
  Android TV Remote integration in Home Assistant for arrow keys.
- Home Assistant with the Deskmate Link integration from this repository.
- A computer with ADB, once, for the installation. Debugging can be switched
  off afterwards; updates come through Home Assistant.

## What Home Assistant gets

| Entity | Meaning |
|---|---|
| `binary_sensor.<tv>_connection` | Is the TV connected. Always available, so automations can react to `off`. |
| `sensor.<tv>_app` | App in the foreground |
| `sensor.<tv>_title` | Title of what is playing (any app with a media session) |
| `sensor.<tv>_playback` | `playing`, `paused`, `stopped`, `buffering` |
| `number.<tv>_volume` | Volume in percent (also a Bluetooth speaker connected to the TV) |
| `switch.<tv>_mute` | Mute |

Entity names follow the TV language (English or Polish). The ids come from the
device name, `DeskmateTV` by default.

Services:

- `deskmate_link.notify` with the TV's `node_id` shows a bubble in the corner of
  the screen. `image` can be a camera entity (`camera.front_door`): Home
  Assistant grabs a frame and sends it with the message. Without `node_id` the
  service still goes to computers only, never to the TV.
- `deskmate_link.tv_update` sends a new build to the TV (see Updates).

For assistants and other integrations the hub exposes
`hub.async_tv_request(op, args)`. Operations:

| op | What it does |
|---|---|
| `info` | version, Android level, capabilities, screen on, foreground app, volume |
| `screen` | the screen as text (`max` lines, `geo` adds positions, `diff` returns only lines that changed since the last read) |
| `find` | items matching a text, without the full dump |
| `click` | by number from the last read or by text; falls back to focus + D-pad centre for apps that ignore accessibility clicks |
| `key` | up, down, left, right, center, back, home |
| `text` | types into the focused field, or presses the letters of an in-app keyboard |
| `scroll`, `wait` | scroll a list; wait for a screen change, an app or a text |
| `apps`, `launch`, `intent` | installed apps, start one, any deep link or intent |
| `inputs`, `input` | HDMI and other inputs (custom names included), switch to one |
| `media`, `volume` | state and transport controls of the active media session, volume in percent |
| `shot` | a JPEG screenshot (DRM video comes out black) |
| `overlay` | a bubble on screen |

`click`, `key` (centre), `launch` and `intent` accept a confirmation policy
from the caller: regular expressions for labels that need an explicit
`confirm: true` (buy, rent, uninstall, factory reset, account removal). The TV
refuses such a click until the caller confirms, so an assistant has to ask the
user first. The policy lives in Home Assistant and can change without an app
update.

## Installation

1. On the TV: Settings > System > About, press "Android TV OS build" seven
   times. Then Developer options: turn on **USB debugging** (and network ADB if
   the TV has a separate switch for it). On Ethernet the "Wireless debugging"
   switch turns itself off; plain network ADB on port 5555 is enough.
2. In Home Assistant the Deskmate Link integration must be the version from
   this repository.
3. On the computer, from `tv/`:

   ```powershell
   .\scripts\install.ps1 -TvIp 192.168.1.40 -HaUrl http://192.168.1.10:8123 -TokenFile .\ha-token.txt
   ```

   With `-TokenFile` (an admin long-lived token) the script creates the
   Deskmate Link entry itself. Without it, add the integration in Home
   Assistant, copy the `DMP1....` pairing code and pass `-PairingCode`.
4. When the TV asks "Allow USB debugging?", tick "Always allow" and confirm.
5. The script ends with a list of checks. Switch USB debugging off.

The script installs the app into internal storage (not onto an adopted USB
drive), enables the accessibility service and notification access, grants
`WRITE_SECURE_SETTINGS` to the app's watchdog, allows self-updates and pairs
the TV. The app's own screen (DeskmateTV in the launcher) shows the same
status.

## Updates

1. Raise `versionCode` and `versionName` in `tv/app/build.gradle.kts` and run
   `tv\scripts\build.ps1`. It signs with the same key.
2. Copy `dist\DeskmateTV-<version>.apk` to `/config/deskmate_tv/` in Home
   Assistant.
3. The next time the TV connects, Home Assistant sends the newer file on its
   own. You can also call `deskmate_link.tv_update` right away.

The TV checks the SHA-256 of the whole file, that it is the same package with a
higher version and that it is signed with the same certificate, then installs
it without a prompt. Keep a backup of the signing key: a build signed with any
other key will not install as an update.

## Security and privacy

- Pairing key plus an ephemeral X25519 exchange per connection, AES-256-GCM
  frames with strictly increasing counters, the same code as Deskmate on
  Windows. Unit tests check the Kotlin implementation against vectors made
  with the integration's own `crypto.py`.
- The pairing key is stored encrypted with a key from Android Keystore.
- Provisioning over ADB goes through a broadcast receiver protected by
  `WRITE_SECURE_SETTINGS`. Regular apps cannot send it.
- Home Assistant receives the foreground app and what is playing as states.
  Screen content only travels on request and never becomes a state, so it does
  not land in the recorder.

## Limitations

- Apps that draw their own interface may expose little or nothing to
  accessibility. Then a screenshot and the arrow keys are what is left.
- Google TV freezes apps in deep standby. Wake the TV with the Android TV Remote
  integration; DeskmateTV reconnects within seconds after the screen comes on.
- Some apps react to accessibility clicks only after focus moves to the item.
  `click` handles that, but a new app layout can still need a different path.
