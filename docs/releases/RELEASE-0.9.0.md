# Deskmate 0.9.0 - your PC stays in Home Assistant before you sign in

With the new optional Deskmate service, your PC stays in Home Assistant while
nobody is signed in. Deskmate also runs on Linux now, as a preview. And from
this version on, the Windows app can install its updates for you after you
say yes. Many of the smaller additions come from your requests in issues and
on the forums. Thank you.

## Update

1. In HACS, update Deskmate to 0.9.0 and restart Home Assistant. Do this
   first. The Deskmate service, the Linux daemon, the media player,
   notification tags and the *Connection* sensor need the new integration. A
   PC that still runs Deskmate 0.8.5 keeps working with it.
2. On the PC, install `Deskmate_0.9.0_x64-setup.exe` (or
   `Deskmate_0.9.0_arm64-setup.exe`) over the old version. Settings and
   pairing are kept. Install this version by hand. Later versions can install
   themselves. Settings → *Updates* starts at *Ask first*: when a new version
   is out, Deskmate shows a notification and installs it only when you choose
   *Install now*. With the daily check switched off, Deskmate does not look
   for updates.
3. If you want them, install the service from Settings → *Deskmate service*
   and the Stream Deck plugin from Settings → *Stream Deck*. Neither is
   installed until you do. After installing the plugin, restart Stream Deck,
   and turn on *Let Stream Deck use Deskmate* if its keys should go through
   Deskmate.
4. On Linux, install the daemon if the computer should be in Home Assistant
   from the moment it boots, with nobody signed in. Install the desktop app
   for the Deskmate window with notifications, media and files. You can
   install both. [LINUX.md](../LINUX.md) has the steps.

## What is new

### The Deskmate service (#8)

Without a signed-in user the app does not run, so the PC used to drop out of
Home Assistant at the sign-in screen and after every restart. The optional
service keeps it there. The processor, memory, system drive, network, uptime
and battery sensors stay, and so do the Sleep, Hibernate, Shutdown and
Restart buttons. Install it from Settings → *Deskmate service* with one
administrator approval.

When you sign in, the app takes over the connection. When you sign out, the
service takes it back. The PC stays available in Home Assistant during the
change. For five minutes after the PC starts or wakes up, the service refuses
the power buttons, so a misfiring automation cannot keep it off. The service
never shows notifications or receives files. It works only with the Deskmate
integration, not with MQTT.

### Deskmate for Linux

The daemon runs under systemd and puts a Linux computer into Home Assistant
from the moment it boots, with its sensors and power buttons. It comes as a
`.deb` or a tarball, for x86_64 and ARM64.

The desktop app brings the Deskmate window to Linux as a `.deb` or an
AppImage, with pairing, notifications, media controls, volume, Link Files,
widgets and dashboards. Both pair with the same code as the Windows app.
[LINUX.md](../LINUX.md) has the steps and a table of what works on Linux, on
X11 and on Wayland.

### Automatic updates on Windows

Settings → *Updates* has three modes. *Ask first*, the default, shows
*Install now* and *Later* in a notification and on the Status page. *Install
automatically* waits until the PC has not been used for five minutes and
nothing runs full screen. *Only tell me* works as before. Deskmate installs
only a newer version from this project's GitHub releases with a valid
signature from the release key. Your settings, the pairing, *Start with
Windows*, the CPU temperature helper and the family protection stay. Home
Assistant cannot start an update.

### Stream Deck through Deskmate

The plugin ships with the app, and Settings → *Stream Deck* installs it into
Stream Deck. Turn on *Let Stream Deck use Deskmate*, which is off by default,
and its keys reach Home Assistant through Deskmate. They use the Home
Assistant address and access token set in Deskmate under Settings → *Home
Assistant API*, so Stream Deck needs no token of its own. A new key,
*Deskmate Action*, runs one of your Deskmate hotkeys or tray quick actions. A
direct connection with a token still works. The plugin needs Stream Deck 7.1
or newer. After you install it, quit Stream Deck and start it again to load
it. See [STREAMDECK.md](../STREAMDECK.md).

### Media player

Turn on *Media status* on the Sensors page and the PC gets a *Media* entity
with play/pause, next, previous, volume and mute. It works with any media
card.

### Notification tags and priority

`deskmate_link.notify` takes a `tag`, so a doorbell that rings twice shows
one notification. It also takes a `priority`. On Windows build 22546 and
newer, an `urgent` notification can show even while Do not disturb is on,
once you allow important notifications from Deskmate when Windows asks.
`deskmate_link.clear_notification` removes a tagged notification once the
door is open. Tags, priority and clearing need Deskmate 0.9.0 for Windows on
that PC.

### Screen and user state

*Screen on*, on by default, says whether the display is on. *Screen
brightness*, also on by default, sets a laptop's own screen and DDC/CI
monitors from 0 to 100 %. *Wake screen* turns the screens back on. It works
once you turn on *Remote input* in Settings, which is off by default. *User
state*, off by default, reports `normal`, `fullscreen`, `game`,
`presentation` or `busy`. Turn it on in the Sensors page. Idle time now
counts game controllers, so someone playing with a gamepad is not idle.

### Connection sensor

The integration adds a *Connection* sensor to every computer connected over
Link. It shows `app`, `service` or `daemon` while one of them is connected,
and `connected` for a client that does not say what it is, such as Deskmate
0.8.5 or DeskmateTV. When the computer is not connected it shows `sleeping`,
`off` or `disconnected`. The sensor stays available, so an automation can
tell a PC that went to sleep from one that dropped off.

Full list: [CHANGELOG.md](../../CHANGELOG.md). How each feature works:
[GUIDE.md](../GUIDE.md).

## Known limits

- While only the Deskmate service is connected, the sensors and controls that
  need a signed-in user are unavailable, and notifications and files are
  refused.
- On Ubuntu 22.04 the daemon's power buttons are refused, because its polkit
  is too old. The sensors work.
- The Linux desktop app has no clipboard, typing or active window sensor yet.
  Global hotkeys need X11. Notification tags, the urgent priority and
  clearing are not offered yet. Receiving files is *Off* or *Accept
  automatically*, because asking on the computer for each file is not
  offered yet. On GNOME the tray icon needs the AppIndicator extension.
- Linux packages do not update themselves. To update, install the new
  package.
- The CPU temperature helper and the family watchdog are copies of Deskmate
  made when you set them up, and an update does not replace them. To bring
  the helper up to date, press *Remove* and then *Install helper* in
  Settings → *CPU temperature*.
- While *Let Stream Deck use Deskmate* is on, any program running as your
  Windows user can act in Home Assistant through it. Leave it off if you do
  not use the plugin through Deskmate.
- The installers are not Authenticode-signed, so SmartScreen warns about an
  unknown publisher. Choose *More info*, then *Run anyway*.

## Requirements

- Windows 10 or 11, x64 or ARM64.
- Home Assistant 2024.11 or newer.
- On Linux, a 64-bit computer, x86_64 or ARM64. The daemon needs systemd 245,
  glibc 2.35 and OpenSSL 3 or newer, polkit 0.106 or newer for the power
  buttons, and the Deskmate integration 0.9.0. The desktop app needs
  WebKitGTK 4.1, a D-Bus session and a keyring that offers the Secret
  Service. [LINUX.md](../LINUX.md#what-you-need) has the details.

## Assets

| File | For |
|---|---|
| `Deskmate_0.9.0_x64-setup.exe` | Windows 10 and 11, x64 |
| `Deskmate_0.9.0_arm64-setup.exe` | Windows 10 and 11, ARM64 |
| `deskmate-daemon_0.9.0_amd64.deb`, `deskmate-daemon_0.9.0_arm64.deb` | The Linux daemon on Debian, Ubuntu and their relatives |
| `deskmate-daemon-0.9.0-x86_64-linux.tar.gz`, `deskmate-daemon-0.9.0-aarch64-linux.tar.gz` | The Linux daemon on other distributions |
| `Deskmate_0.9.0_amd64.deb`, `Deskmate_0.9.0_arm64.deb` | The Linux desktop app on Debian, Ubuntu and their relatives |
| `Deskmate_0.9.0_amd64.AppImage`, `Deskmate_0.9.0_aarch64.AppImage` | The Linux desktop app on other distributions |
| `SHA256SUMS.txt` | The SHA-256 checksums of the files above |

`latest.json`, `Deskmate_0.9.0_x64-setup.exe.sig` and
`Deskmate_0.9.0_arm64-setup.exe.sig` are for the app's updater: Deskmate reads
them to find the update and check its signature. You do not need to download
them.
