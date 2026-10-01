# Deskmate 0.7.1 - fixes from your feedback, several Windows users, update check

Thanks to everyone who opened issues after 0.7.0. This release is mostly their
list.

## Update (2 minutes)

1. **Home Assistant:** update the integration in HACS to 0.7.1 and restart Home
   Assistant. It is now listed simply as **Deskmate**; nothing needs
   reconfiguring.
2. **PC:** install `Deskmate_0.7.1_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.

The order does not matter; the connection protocol is unchanged.

New to Deskmate? The [guide](../GUIDE.md) covers everything from installing to
automations.

## What is new

**Antivirus no longer flags Deskmate for notifications** (#2). Up to 0.7.0 the
app ran PowerShell that compiled C# on the fly to create its notification
shortcut, and notifications with buttons went through PowerShell too.
Bitdefender quarantined `deskmate.exe` for it. All of that now happens inside
the app; PowerShell only runs for custom commands you create yourself.
Notifications are labelled "Deskmate" (no more "HomeOS"), and the old
`HomeOS` Start Menu entry is removed.

**Several Windows users on one PC** (#3). Install Deskmate on the other account,
then in Home Assistant open the Deskmate entry → *Reconfigure* → *Pair another
Windows user* and paste that code there. Both accounts report as the same
device. Only the account someone is using holds the connection; switch users
and it moves over within seconds.

**Update check.** Once a day Deskmate asks GitHub whether a newer release
exists and shows it in the tray menu, on the Status page and once as a
notification. Nothing is downloaded or installed. Settings → Updates turns it
off or checks right away.

**The Deskmate page in Home Assistant.** The sidebar entry is now called
*Deskmate*, with a **Computers** tab next to Files: which PCs are paired,
whether they are online, app version, encryption, and links to their device
pages. The integration also shows its icon (Home Assistant 2026.3 or newer).

**One guide.** [docs/GUIDE.md](../GUIDE.md) replaces the scattered setup notes:
install, pairing, notifications, controls, files, automation examples, several
users, troubleshooting.

## Fixed

- The widget panel could not be moved or closed. Drag it by its top bar.
- Switches that were on looked like a solid black pill in some WebView2
  versions.
- The window opened behind other windows after installing, and starting
  Deskmate again from Start while it ran in the tray did nothing.
- Text typed into the PC by `type_text` is limited to printable characters, so
  it can no longer press Enter by itself (#1, thanks @anupamme).

Full list: [CHANGELOG.md](../../CHANGELOG.md).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.7.1_x64-setup.exe` | `AB096FEC881AC475EDE66A956CAB2AF5FF4606355F63E0CDD471AC906D1FFCA9` |
| `Deskmate_0.7.1_arm64-setup.exe` | `FCDCDCBD78C064F70453701F99E7D17AD6BEFB6B99E872FA5677FDADAA50A3B1` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
