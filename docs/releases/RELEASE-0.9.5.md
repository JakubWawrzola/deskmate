# Deskmate 0.9.5 - what you asked for after 0.9.0

This version is made of your requests from GitHub, Reddit and Facebook: a
fuller media player, more hardware sensors, the volume of single programs, a
big picture in a notification, more screen time without Home Assistant, and
Deskmate in seven languages. Thank you for every one of them.

## Update

1. In HACS, update Deskmate to 0.9.5 and restart Home Assistant. Do this
   first. The progress bar and cover of the media player, the big
   notification picture and the volume of single programs need the new
   integration. A PC that still runs Deskmate 0.9.0 keeps working with it.
2. On the PC, let Deskmate install the update (Settings → *Updates*), or
   install `Deskmate_0.9.5_x64-setup.exe` (or
   `Deskmate_0.9.5_arm64-setup.exe`) over the old version. Settings and
   pairing are kept.
3. If you use the CPU temperature helper or protected a child's account,
   open their panels once: they now say when their helper is still a copy of
   the older Deskmate and offer *Update helper*.

## What is new

### Media player with a progress bar and a cover (#41)

The *Media* entity shows the album, the length of the track and where it is,
and you can drag the bar to seek. On Windows the card also shows the cover of
the track, while the *Media title* sensor is on. It works with players that
tell Windows how long the track is, such as Spotify and the browsers; a
player that does not has no bar. On Linux the bar and seeking work with MPRIS
players that report a position; there is no cover yet.

### A big picture in a notification (#43)

`image_placement: hero` shows the picture across the top of the
notification, `inline` under the text. Without the field it stays small:

```yaml
action: deskmate_link.notify
data:
  title: Doorbell
  message: Someone is at the front door
  image: https://your-ha.example/local/doorbell.jpg
  image_placement: hero
```

### More hardware sensors (#42)

- *Memory used* in MiB, *CPU speed* in MHz and *Disk activity* in percent,
  on Windows and Linux, without a driver.
- *GPU temperature* without LibreHardwareMonitor: Deskmate reads what the
  graphics driver reports to Windows, as Task Manager does.
- *Disk 0 health* and so on, one per physical drive: `Healthy`, `Warning` or
  `Unhealthy`.
- Any reading of a running LibreHardwareMonitor as a sensor: power draw,
  clock speeds, fans, drive and power supply temperatures. Sensors → *Your
  sensors* → *Hardware monitor sensor* lists what that program reads.

The frame rate is not there: it needs a different kind of access and comes
later.

### Volume of single programs (#44)

List programs under Sensors → *Sound*, for example `discord, spotify`, and
each gets a volume slider and a mute switch in Home Assistant, like its row
in the Windows volume mixer. The two have a value while the program plays
sound. Nothing is listed until you add a program.

### Screen time

- **More time without Home Assistant.** When the time is up and Home
  Assistant cannot be reached, or you are standing next to the PC anyway,
  press *15 min with a password* on the notification or *+15 min* on the
  Family page. Windows asks for an administrator password, and the limit of
  that day grows by 15 minutes, up to 60 a day. Deskmate does not see the
  password. Home Assistant shows the minutes and can turn this off.
- **Warnings you choose.** *Warnings before the limit* takes the minutes at
  which the warnings come, for example `15, 5, 2`.

### Languages (#48)

The window, the tray menu, Deskmate's own notifications and the Home
Assistant integration are in English, German, Spanish, French, Polish,
Brazilian Portuguese and Simplified Chinese. Deskmate and its installer
follow the language of Windows; Settings → *Language* picks another one for
the app. Entity names in Home
Assistant stay in English. Most translations were not written by native
speakers: corrections are welcome, and
[CONTRIBUTING.md](../../CONTRIBUTING.md) says which file to change.

### Smaller things

- **Session locked** turns on as soon as Windows locks, also on the screen
  that only shows the clock (#45).
- **Current user** shows `none` at the sign-in screen when the Deskmate
  service is installed, instead of the last name (#8).
- **User state** shows `game` for a game in a borderless window, when it
  runs from the folder of a game store or is on your list.
- **Away from home**, Deskmate tries the fallback address after five
  seconds instead of about a minute.
- The notification about a new Deskmate says to update the integration
  first when yours is older.
- Section headings in the window are easier to see (#46).

## Known limits

- The installer is not code-signed yet (#40). Windows SmartScreen asks once,
  and Smart App Control, where it is on, blocks the installer.
- The cover of the track is sent on Windows only.
- The volume of single programs and the cover need the Deskmate
  integration; they are not there over MQTT.
- The Linux apps are a preview, as in 0.9.0.

## Requirements

- Windows 10 or 11, x64 or ARM64. Linux: see [LINUX.md](../LINUX.md).
- Home Assistant with the Deskmate integration 0.9.5 from HACS, or an MQTT
  broker.

## Assets

| File | For |
|---|---|
| `Deskmate_0.9.5_x64-setup.exe` | Windows 10 and 11, x64 |
| `Deskmate_0.9.5_arm64-setup.exe` | Windows 10 and 11, ARM64 |
| `deskmate-daemon_0.9.5_amd64.deb`, `deskmate-daemon_0.9.5_arm64.deb` | The Linux daemon on Debian, Ubuntu and their relatives |
| `deskmate-daemon-0.9.5-x86_64-linux.tar.gz`, `deskmate-daemon-0.9.5-aarch64-linux.tar.gz` | The Linux daemon on other distributions |
| `Deskmate_0.9.5_amd64.deb`, `Deskmate_0.9.5_arm64.deb` | The Linux desktop app on Debian, Ubuntu and their relatives |
| `Deskmate_0.9.5_amd64.AppImage`, `Deskmate_0.9.5_aarch64.AppImage` | The Linux desktop app on other distributions |
| `SHA256SUMS.txt` | The SHA-256 checksums of the files above |

`latest.json`, `Deskmate_0.9.5_x64-setup.exe.sig` and
`Deskmate_0.9.5_arm64-setup.exe.sig` are for the app's updater: Deskmate reads
them to find the update and check its signature. You do not need to download
them.
