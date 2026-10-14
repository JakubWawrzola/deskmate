# Deskmate 0.8.1 - microphone, sound devices, screen time and family rules

Three requests from Reddit and the issues: control the PC's microphone from
Home Assistant, switch sound devices, and a local alternative to Microsoft
Family Safety. Plus Files and Clipboard as their own pages in the app.

## Update (2 minutes)

1. **Home Assistant:** update Deskmate in HACS to 0.8.1 and restart Home
   Assistant. Needed for the new lists (sound devices) and the family rules.
2. **PC:** install `Deskmate_0.8.1_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and pairing are kept.

## What is new

**Microphone**: a *Mute microphone* switch and a *Microphone volume* slider.
An automation can mute the PC's microphone when the radio starts.

**Sound devices**: *Sound output* and *Sound input* lists switch the Windows
default device for apps, media and calls, like *Set as default device* in the
Windows sound settings. A headset you plug in shows up in the list by itself.

**Screen time and family rules** (Family page, off by default): Deskmate
counts how long a Windows account is in use today and Home Assistant sets the
rules: a daily limit, extra time for today, allowed hours, programs to block,
and what happens when the time is up (lock or sign out). Warnings come 10, 5
and 1 minute before. The child can press *Ask for more time*, which reaches
Home Assistant as an event, ready for an automation that asks you on your
phone. Everything is counted and enforced on the PC, so it also works while
Home Assistant or the internet is down. No Microsoft account involved.

**Protect this account**: with one administrator password, Windows starts
Deskmate again within a minute when a child closes it, the rules can only be
changed in Home Assistant, and Home Assistant puts back a rule changed on the
PC. Use a standard Windows account for the child.

**Files and Clipboard pages**: receiving files, shared folders and the recent
transfers on one page, both clipboard permissions on another. They were panels
in Settings.

**Sign out** button for every PC.

## Fixed

- *Mute audio* over the Deskmate integration always showed as on.

How it all works, with example automations: [GUIDE.md](../GUIDE.md). Full
list: [CHANGELOG.md](../../CHANGELOG.md).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.1_x64-setup.exe` | `8cec57c12e6b068c4382d329fc1f853e7d6d6e1f3a05395c7c0bd5e1ed44f69d` |
| `Deskmate_0.8.1_arm64-setup.exe` | `a23d3afc93a4bff6a48a12defb7d0e09f6a05351d20e6316e83ef6c27b9fabd9` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
