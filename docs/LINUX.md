# Deskmate on Linux (preview)

The Deskmate daemon puts a Linux computer into Home Assistant as a device with
its sensors and power buttons. It runs in the background as a systemd service,
from boot on, whether or not anybody is signed in. It talks to the same
Deskmate integration as the Windows app, over the same encrypted connection,
and pairs with the same pairing code.

A desktop app for Linux is in preview as well: the Deskmate window with
the session's sensors and controls, for the user who is signed in. It is
described in [Desktop app (preview)](#desktop-app-preview); the rest of
this page is about the daemon.

- [What it does](#what-it-does)
- [What you need](#what-you-need)
- [Install](#install)
- [Pair it with Home Assistant](#pair-it-with-home-assistant)
- [Status and logs](#status-and-logs)
- [Change the settings](#change-the-settings)
- [Update](#update)
- [Remove](#remove)
- [Linux and Windows compared](#linux-and-windows-compared)
- [Files](#files)
- [Desktop app (preview)](#desktop-app-preview)

## What it does

**Sensors**: CPU, memory, the root file system, network down and up, uptime,
and where the computer has them: battery and AC power, drives by their mount
point (*Disk / free*), disk read and write, CPU
temperature, and for AMD and Intel graphics the GPU load, memory and
temperature.

**Buttons**: Sleep, Hibernate, Shutdown and Restart. Hibernate appears only
when the computer can hibernate (the kernel offers it and logind agrees, which
needs swap that is large enough). The buttons do nothing for the first five
minutes after the computer starts or wakes up: an automation that turns the
computer off as soon as it connects would otherwise do so on every start.

**Connection**: the diagnostic sensor that Home Assistant adds shows *Daemon*
while the daemon is connected, and *Sleeping* or *Off* when the computer went
to sleep or shut down.

Notifications and Link Files need the desktop app. Home Assistant gets an
answer at once that the daemon does not support them, so an automation that
tries fails straight away instead of waiting.

## What you need

- A 64-bit computer, x86 (`x86_64`, `amd64`) or ARM (`aarch64`, `arm64`).
- systemd 245 or newer, glibc 2.35 or newer and OpenSSL 3.
- polkit with JavaScript rules (polkit 0.106 or newer) for the power buttons.
  Debian 12, Ubuntu 23.04 and newer, and Fedora have it. Ubuntu 22.04 ships an
  older polkit: there the sensors work and the power buttons are refused.
- The Deskmate integration 0.9.0 or newer in Home Assistant. With an older
  one the daemon connects, sends nothing, shows `integration_too_old` and
  looks again every hour.

## Install

Download the package for your processor from
[Releases](https://github.com/JakubWawrzola/deskmate/releases), together with
`SHA256SUMS.txt`, and check it:

```sh
sha256sum --check --ignore-missing SHA256SUMS.txt
```

**Debian, Ubuntu and their relatives**: install the `.deb`.

```sh
sudo apt install ./deskmate-daemon_<version>_amd64.deb   # or _arm64.deb
```

**Other distributions**: unpack the tarball and run its install script. It
copies the files to the places the `.deb` uses and runs systemd's own tools;
it downloads nothing.

```sh
tar -xzf deskmate-daemon-<version>-x86_64-linux.tar.gz   # or -aarch64-linux
cd deskmate-daemon-<version>-x86_64-linux
sudo ./install.sh
```

Either way you get the system user `deskmate`, the service `deskmate.service`,
the helper `deskmate-sleep.service` and the polkit rule, and the daemon starts.
It waits for a pairing.

## Pair it with Home Assistant

1. In Home Assistant: **Settings → Devices & services → Add integration →
   Deskmate**. Copy the pairing code (`DMP1.…`). For a computer that is
   already paired, open its entry and use *Show the pairing code*.
2. On the computer:

   ```sh
   sudo deskmate-service pair
   ```

   Paste the code and press Enter. The code is read from the terminal and
   never from the command line, so it does not end up in your shell history
   or in the list of running programs. `sudo deskmate-service pair < code.txt`
   works too; delete the file afterwards.
3. The daemon connects within seconds. The computer appears as a device named
   after its host name, under *Settings → Devices & services → Deskmate*.

A code from *Show the pairing code* carries the computer's node id, so a
reinstalled daemon comes back as the same device. Otherwise the node id is
made from the host name.

`pair` fails without changing anything when the code is not a Deskmate
pairing code, when its key cannot be used, or when it carries no address of
Home Assistant. Set the address under *Settings → System → Network* in Home
Assistant and show the code again.

## Status and logs

```sh
deskmate-service status
```

prints one line of JSON, which any user may read:

```json
{"v":1,"version":"0.9.0","state":"connected","node_id":"my_pc","detail":"Connected to Home Assistant.","config":"<64 hex digits>"}
```

| `state` | Meaning |
|---|---|
| `not_provisioned` | Not paired yet: `sudo deskmate-service pair` |
| `connecting` | Connecting, or trying again after a network failure |
| `connected` | Connected to Home Assistant |
| `waiting` | The computer is going to sleep, or a Deskmate desktop app holds the connection |
| `integration_too_old` | The integration in Home Assistant is older than 0.9.0 |
| `error` | Something to fix; `detail` says what, for example a pairing that Home Assistant rejected |

`config` is a fingerprint of the settings and keys in use. It shows nothing of
either.

The log is in the journal:

```sh
journalctl -u deskmate          # add -f to follow it
systemctl status deskmate
```

Every power command, carried out or refused, is also recorded in
`/var/lib/deskmate/security.log`, which only root and the daemon can read.

## Change the settings

`/etc/deskmate/config.json` has the same fields as the settings of the Windows
service:

```json
{
  "node_id": "my_pc",
  "link_url": "ws://homeassistant.local:8123/api/deskmate_link/ws",
  "link_url_remote": "",
  "link_cascade": false,
  "publish_interval_secs": 15,
  "device_name": "My PC",
  "sensors_enabled": {"battery": false}
}
```

`device_name` is the name in Home Assistant (empty: the host name),
`publish_interval_secs` the seconds between two updates (2 to 3600), and
`sensors_enabled` turns sensors off by their key. Edit the file as root, then:

```sh
sudo systemctl reload deskmate
```

A file with an unknown field or a value out of range is refused as a whole;
`deskmate-service status` then shows `error`. Pairing again keeps
`device_name`, `publish_interval_secs` and `sensors_enabled`.

## Update

Install the new `.deb` the same way, or run `install.sh` from the new tarball.
The daemon restarts; its settings and keys stay.

## Remove

```sh
sudo deskmate-service unpair      # forgets the settings and the keys
sudo apt remove deskmate-daemon   # or, for the tarball: sudo ./install.sh --remove
```

`sudo apt purge deskmate-daemon` also deletes `/etc/deskmate` and
`/var/lib/deskmate`. The system user `deskmate` stays, as Debian does with
system users; `sudo userdel deskmate` removes it. The device stays in Home
Assistant until you delete it there.

## Linux and Windows compared

| | Windows app | Linux daemon | Linux desktop app (preview) |
|---|---|---|---|
| CPU, memory, disks, network, uptime, battery | Yes | Yes | Yes |
| GPU sensors | Yes | AMD and Intel | AMD and Intel |
| CPU temperature | Yes, best with the helper | Yes, from the kernel's sensors | Yes, from the kernel's sensors |
| Idle time, locked, who is signed in | Yes | No | Yes, through logind |
| Media title, artist, player, status | Yes, when switched on | No | Yes, through MPRIS, when switched on |
| Window, Wi-Fi, camera, microphone | Yes, when switched on | No | No |
| Sleep, Hibernate, Shutdown, Restart | Yes | Yes | Yes |
| Lock, volume, mute, media keys | Yes | No | Yes |
| Sign out, screens off, microphone, sound devices | Yes | No | No |
| Notifications | Yes | No | Yes |
| Link Files | Yes | No | Yes, without asking on the computer |
| Open URL | Yes | No | Yes |
| Clipboard, typing text, speech | Yes | No | No |
| Your own sensors and commands | Yes | No | No |
| Hotkeys | Yes | No | X11 only |
| Widgets, dashboard window, tray quick actions | Yes | No | Yes (quick actions need a tray) |
| Stream Deck, screen time and family rules | Yes | No | No |
| Connected while nobody is signed in | With the Deskmate service | Yes | With the daemon |
| Where the keys are | Windows Credential Manager | A file only the daemon can read | The user's keyring (Secret Service) |
| Updates | Inside the app | A new package | A new package; the app says when one is out |

## Files

| Path | What |
|---|---|
| `/usr/bin/deskmate-service` | The program |
| `/usr/lib/systemd/system/deskmate.service` | The service |
| `/usr/lib/systemd/system/deskmate-sleep.service` | Tells the daemon before the computer sleeps and after it wakes up |
| `/usr/share/polkit-1/rules.d/50-deskmate.rules` | Lets the `deskmate` user suspend, hibernate, power off and restart, nothing else |
| `/usr/lib/sysusers.d/deskmate.conf`, `/usr/lib/tmpfiles.d/deskmate.conf` | The user and the folders |
| `/etc/deskmate/config.json` | Settings, never a key. Mode 0600, owned by `deskmate` |
| `/var/lib/deskmate/machine.deskmate-service-link.key` | The pairing key. Mode 0600, owned by `deskmate` |
| `/var/lib/deskmate/machine.deskmate-service-link-cascade.key` | The cascade key, with cascade encryption on |
| `/var/lib/deskmate/security.log` | The power commands and their results |
| `/run/deskmate/status.json` | The status, while the daemon runs |

How the daemon is locked down is in [SECURITY.md](SECURITY.md#the-linux-daemon).

## Desktop app (preview)

The desktop app is the Deskmate window for Linux: the setup wizard and
pairing, Deskmate Link or MQTT, and the sensors and controls of the session
you are signed in to. It talks to the same integration as the Windows app and
pairs with the same code.

### Install the desktop app

Download the package for your processor from
[Releases](https://github.com/JakubWawrzola/deskmate/releases) and check it
against `SHA256SUMS.txt` as above.

```sh
sudo apt install ./deskmate_<version>_amd64.deb   # or _arm64.deb
```

On other distributions use the AppImage: make it executable and start it.

```sh
chmod +x Deskmate_<version>_amd64.AppImage
./Deskmate_<version>_amd64.AppImage
```

It needs WebKitGTK 4.1, a D-Bus session and, for the pairing key, a keyring
that offers the Secret Service: GNOME Keyring, KWallet or KeePassXC. Without
one, saving the pairing fails and the window says that no keyring service is
running. *Start when I sign in* in Settings writes
`~/.config/autostart/Deskmate.desktop`.

### What works on X11 and on Wayland

| | X11 | Wayland |
|---|---|---|
| Window, setup wizard, pairing, Link and MQTT | Yes | Yes |
| Machine sensors, Sleep, Hibernate, Shutdown, Restart | Yes | Yes |
| Idle time, locked, Lock (logind) | Yes | Yes |
| Media sensors and keys (MPRIS) | Yes | Yes |
| Volume and mute (`wpctl` or `pactl`) | Yes | Yes |
| Notifications, with buttons where the notification service has them | Yes | Yes |
| Open URL (`xdg-open`), Link Files | Yes | Yes |
| Tray icon and quick actions | Where an AppIndicator host runs | Where an AppIndicator host runs |
| Global hotkeys | Yes | No |
| Clipboard, active window, typing text | No | No |

Home Assistant gets only the entities that work on the computer: no Hibernate
button where the computer cannot hibernate, no volume without `wpctl` or
`pactl`, no Lock without a logind session. Where a feature is missing, the
window says so in one line in its place.

Idle time comes from logind's idle hint, which the desktop sets after its own
idle delay; until then the computer counts as in use. The power buttons go
through `systemctl`, which polkit allows for the user of the active local
session.

Receiving files works with *Off* and *Accept automatically*. The app cannot
ask on the computer for each file yet, so *Ask on this computer* is not
offered. Files land in `Deskmate` inside your download folder (`xdg-user-dirs`
decides its name) unless you set another folder.

### The tray on GNOME

GNOME shows no tray icons by itself. With the extension *AppIndicator and
KStatusNotifierItem Support* (package `gnome-shell-extension-appindicator` on
Debian; Ubuntu ships it switched on) Deskmate gets its icon. Without a tray
the window does not hide: closing it quits Deskmate, and *Start minimized to
tray* is off. KDE Plasma, Xfce, Cinnamon and MATE have a tray.

### With the daemon on the same computer

The app and the daemon pair as the same computer: paste the code from *Show
the pairing code* into both. Only one of them holds the connection. While you
are signed in and the app runs, the daemon hands the connection over to it;
when you quit the app or sign out, the daemon takes it back, and Home
Assistant keeps the device. With the daemon running, *Disconnect* in the app
keeps the connection closed instead of handing it to the daemon.
