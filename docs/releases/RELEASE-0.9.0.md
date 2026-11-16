# Deskmate 0.9.0 - the Windows service, Linux, automatic updates

Your PC now stays in Home Assistant when nobody is signed in, Deskmate comes
to Linux as a preview, and from this version on the app installs its own
updates. Many of the smaller additions come from your requests in issues and
on the forums. Thank you.

## Update

1. **Home Assistant:** update Deskmate in HACS to 0.9.0 and restart Home
   Assistant. Do this first. The Deskmate service, the Linux daemon, the media
   player, notification tags and the *Connection* sensor need the new
   integration. A PC that still runs Deskmate 0.8.5 keeps working with it.
2. **PC:** install `Deskmate_0.9.0_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept. You install this one by hand; from
   0.9.0 on, Deskmate can install new versions itself. Settings → *Updates*
   starts at *Ask first*, so nothing installs until you choose *Install now*,
   and with the daily check off Deskmate looks for nothing.
3. **Optional:** Settings → *Deskmate service* installs the service, and
   Settings → *Stream Deck* installs the plugin. Both stay off until you
   install them.
4. **Linux:** install the daemon or the desktop app as described in
   [LINUX.md](../LINUX.md).

## What is new

**The Deskmate service** (#8). Without a signed-in user the app does not run,
so the PC used to drop out of Home Assistant at the sign-in screen and after
every restart. The optional service fills that gap: the processor, memory,
system drive, network, uptime and battery sensors stay, and so do Sleep,
Hibernate, Shut down and Restart. Install it from Settings → *Deskmate
service* with one administrator approval. When you sign in, the app takes the
connection over, and when you sign out the service takes it back; Home
Assistant keeps the PC available in between. For five minutes after the PC
starts or wakes up the service refuses the power buttons, so a misfiring
automation cannot keep it off. The service never shows notifications or
receives files. It needs the Deskmate integration; MQTT setups keep working as
before, without the service.

**Deskmate for Linux.** A daemon under systemd puts a Linux computer
into Home Assistant with its sensors and power buttons, from boot on, as a
`.deb` or a tarball for x86_64 and ARM64. The desktop app brings the Deskmate
window to Linux as a `.deb` or an AppImage: pairing, notifications, media
controls, volume, Link Files, widgets and dashboards. Both pair with the same
code as the Windows app. [LINUX.md](../LINUX.md) has the steps and a table of
what works on Linux, on X11 and on Wayland.

**Automatic updates.** Settings → *Updates* has three modes: *Ask first*
(the default) shows *Install now* and *Later*; *Install automatically* waits
until the PC has not been used for five minutes and nothing runs full screen;
*Only tell me* works as before. Deskmate installs only a newer version from
this project's GitHub releases with a valid signature from the release key.
Your settings, the pairing, *Start with Windows*, the CPU temperature helper
and the family protection stay. Home Assistant cannot start an update.

**Stream Deck through Deskmate.** The plugin ships with the app. Settings →
*Stream Deck* installs it into Stream Deck, and *Let Stream Deck use Deskmate*
(off by default) lets its keys reach Home Assistant through Deskmate, so
Stream Deck needs no token of its own. A new key, *Deskmate Action*, runs one
of your Deskmate hotkeys or tray quick actions. A direct connection with a
token still works. Stream Deck 7.1 or newer. See [STREAMDECK.md](../STREAMDECK.md).

**Media player.** With *Media status* on, the PC gets a *Media* entity with
play/pause, next, previous, volume and mute, ready for any media card.

**Notification tags and priority.** `deskmate_link.notify` takes a `tag`, so a
doorbell that rings twice shows one notification, and a `priority`; `urgent`
can show while Do not disturb is on, as far as Windows allows.
`deskmate_link.clear_notification` removes a tagged notification once the
door is open.

**Screen and user state.** *Screen on* says whether the display is on.
*Screen brightness* sets a laptop's own screen and DDC/CI monitors from 0 to
100 %. *Wake screen* turns the screens back on and sits behind *Remote input*
in Settings. *User state* (off by default) tells `normal`, `fullscreen`,
`game`, `presentation` and `busy` apart. Idle time now counts game
controllers, so someone playing with a gamepad is not idle.

**Connection sensor.** Home Assistant adds a *Connection* sensor to every
computer: `app`, `service` or `daemon` while connected, and `sleeping`, `off`
or `disconnected` when not. It stays available, so an automation can tell a
PC that went to sleep from one that dropped off.

Full list: [CHANGELOG.md](../../CHANGELOG.md). How each feature works:
[GUIDE.md](../GUIDE.md).

## Known limits

- While only the Deskmate service is connected, the sensors and controls that
  need a signed-in user are unavailable, and notifications and files are
  refused.
- On Ubuntu 22.04 the daemon's power buttons are refused, because its polkit
  is too old; the sensors work.
- The Linux desktop app has no clipboard, typing or active window sensor,
  global hotkeys need X11, and notification tags, the urgent priority and
  clearing are not offered yet. On GNOME the tray icon needs the AppIndicator
  extension.
- Linux packages do not update themselves; install the new package.
- The copies of Deskmate used by the CPU temperature helper and the family
  watchdog stay at the version they were installed with.
- While the Stream Deck channel is on, any program running as your Windows
  user can act in Home Assistant through it.
- The installers are not Authenticode-signed, so SmartScreen reports an
  unknown publisher.

This release was not tested end to end against a live Home Assistant before
it was published.

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

Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
