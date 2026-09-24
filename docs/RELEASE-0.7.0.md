# Deskmate 0.7.0 - files from your phone to your PC through Home Assistant

Home Assistant gets a **Deskmate Files** page. Open it on your phone, pick a
photo or a document, and it lands in a folder on your PC a few seconds later.
The same page lets you browse folders the PC shares and download from them.
Everything travels over the encrypted Deskmate Link connection the app already
uses, so there is nothing new to expose to the network.

## Turn it on (2 minutes, on top of a working 0.6 setup)

1. **Home Assistant:** update **Deskmate Link** in HACS to 0.7.0 and restart
   Home Assistant.
2. **PC:** install `Deskmate_0.7.0_x64-setup.exe` (or `_arm64`) over the old
   version. Settings and the pairing are kept.
3. **PC:** Deskmate → Settings → **Receive files** → *Ask on this computer for
   every file* (or *Accept automatically*). Files go to `Downloads\Deskmate`
   unless you pick another folder.
4. **Phone or laptop:** Home Assistant sidebar → **Deskmate Files** → *Choose
   files*.

To download from the PC as well, add a folder under Settings → **File access**.
Share a specific folder, not your whole drive: every Home Assistant
administrator can read what you share.

New to Deskmate? Follow the setup in
[RELEASE-0.6.0.md](RELEASE-0.6.0.md#fastest-setup-new-install-about-5-minutes)
first. The order of updates does not matter for this release. A 0.6 app with
the 0.7 integration keeps all its entities; only the Files page shows an error
for that computer until it is updated. A 0.7 app with the 0.6 integration works
as before, without the Files page.

## What is new

**Deskmate Files page** in the Home Assistant sidebar, for administrators only.
It works in the Home Assistant mobile app. Choose several files at once or drop
them onto the page on a laptop; each shows progress and the name it was saved
under. Uploads travel to Home Assistant in 8 MB pieces, so large files also get
through a Cloudflare tunnel, which rejects single requests over 100 MB.

**Receiving on the PC**, off by default. *Ask on this computer* shows a dialog
for every file and refuses while Windows is locked; *Accept automatically* does
not ask. A toast tells you what arrived. Size limit 256 MB by default, adjustable.

**Two services for automations.** `deskmate_link.send_file` puts a file from
Home Assistant on the PC, for example a doorbell snapshot from `/media`.
`deskmate_link.fetch_file` copies a file from a shared PC folder into Home
Assistant. Both only touch paths in `allowlist_external_dirs`.

**Fixed: confirmation dialogs no longer drop the connection.** A custom command
that asks for confirmation (the default) or a clipboard write in *Confirm* mode
used to freeze the Link session until someone clicked. Home Assistant then saw
no replies to its pings and disconnected. Both now run alongside the session.

## How received files are handled

- Home Assistant can only add new files to the one inbox folder. It cannot
  overwrite, rename, delete or read anything through it.
- File names are reduced to a plain name: no folders, no `..`, no reserved
  Windows names. A name that already exists gets a number, `photo (1).jpg`.
- Data goes to a hidden `.part` file first and becomes visible only after its
  SHA-256 matches what Home Assistant sent.
- Each file gets the "downloaded from another computer" mark browsers add, so
  Windows warns before running an `.exe` that came from your phone.
- Stalled uploads are deleted after two minutes and when the connection drops.
- Every transfer is logged locally (name and result, never content).

Details: [docs/LINK.md](LINK.md#link-files-sending-and-fetching-files) and
[docs/SECURITY.md](SECURITY.md#link-files).

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.7.0_x64-setup.exe` | `FBC7991ECA388CB43A75C3229ABC1B23B52D49CF505AB0B4C85A2065FC298838` |
| `Deskmate_0.7.0_arm64-setup.exe` | `92EB2AEB81999B4B8E74E13439A4F2592A262B69EE45AD2CFD904354DC5757BA` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
