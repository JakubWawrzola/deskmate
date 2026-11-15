# Security

Last reviewed: 2026-11-15. Scope: Deskmate 0.8.5: the Windows app, the Home
Assistant integration (`deskmate_link`), the Deskmate Link connection between
them, the optional MQTT and REST channels, what Home Assistant can do on
the PC through them, and the local channel for the Stream Deck plugin, which
comes with the release after 0.8.5. To report a problem, see the
[security policy](../.github/SECURITY.md).

Deskmate lets Home Assistant read from a signed-in Windows session and act in
it. Whoever can speak as Home Assistant on the Deskmate connection can use
every capability that is switched on in the app. That connection is the
security boundary, and most of this document is about who can reach it and
what is on by default.

## Threat model

| Actor | What they can reach | What stands in the way |
|---|---|---|
| A device on the local network, a guest device or a hostile router | The traffic between the PC and Home Assistant | Link frames are encrypted and authenticated end to end: traffic cannot be read, replayed or injected, only blocked. MQTT uses TLS by default. |
| Anyone who can reach Home Assistant's web port | The Link endpoint, which answers before login because the handshake is the authentication | Without the pairing key the handshake fails, repeated failures lock the sender out, and nothing from a failed attempt is logged or shown unvalidated. |
| A copy of Home Assistant's storage or of a backup | The pairing key | Its holder can act as Home Assistant from then on, until the key is replaced. Recorded sessions stay closed. |
| A Home Assistant administrator, add-on or automation | Everything Home Assistant can do | Trusted by design: they can use every capability that is switched on. The risky ones are off by default and several ask on the PC first. |
| An MQTT client with broad topic access (MQTT transport only) | Other computers' command and state topics | One broker user and one ACL block per computer, described below. |
| Another Windows account on the same PC | The named objects that decide which account connects, and the name of the Stream Deck channel's pipe | It can keep Deskmate from connecting, and keep the Stream Deck channel from opening. It cannot connect to the channel. See [The Stream Deck channel](#the-stream-deck-channel). |
| Someone who can change what GitHub serves for Deskmate's releases, or who sits between the PC and GitHub | The release manifest and the installer that the updater downloads | An installer runs only with a valid signature from the release key, for a version higher than the running one. The key is not on GitHub. See [Updates](#updates). |
| A program running as the Windows user or as administrator | The user's session, `config.json`, the `deskmate:` link handler, and the Stream Deck channel while it is switched on | Outside the boundary: such a program can already read the clipboard and run programs. |

## What is on and what is off

Off until you turn it on in the app:

- sensors that show what you are doing: active window, media title, artist,
  app and status, Wi-Fi name, camera in use, microphone in use;
- reading the clipboard and writing to it, two separate settings;
- typing text, presentation keys and opening URLs (*Allow input*), and speech;
- receiving files and sharing folders, two separate settings;
- your own PowerShell commands and sensors: created disabled, with
  confirmation on;
- the CPU temperature helper, the Deskmate service and the family rules;
- the local channel for the Stream Deck plugin;
- cascade encryption.

On once the computer is paired:

- the other sensors, such as processor, memory, disks, network, battery,
  uptime, idle time, lock state, user name and volume. Each can be switched
  off on the Sensors page;
- the built-in controls: lock, sleep, hibernate, shut down, restart, sign out,
  monitor off, media keys, empty the recycle bin, keep awake, volume and mute.
  Whoever controls Home Assistant can press them;
- the daily update check. It asks before it installs anything: a new version
  is installed after you choose *Install now*, or by itself only if you
  switch to automatic installs in Settings (see [Updates](#updates)).

New installs use Deskmate Link. The key is stored in Windows Credential
Manager, and a plain `ws://` address is accepted only inside a private network.

### Upgrading an old configuration

A configuration from before the security settings existed migrates to MQTT
over TLS on port 8883, both clipboard modes `Off`, an empty URL allowlist and
disabled custom commands that ask for confirmation. This intentionally stops
old insecure automations until you review them. Credentials already in
Credential Manager stay there.

## Deskmate Link

Link is the connection between the app and the integration: one WebSocket,
encrypted by Deskmate itself, independent of the transport underneath. The
protocol is described in [LINK.md](LINK.md).

### Handshake and session keys

Since 0.6.0 Link uses protocol v2. Both sides authenticate with HMAC-SHA256
under the pairing key over the whole handshake transcript, including an
ephemeral X25519 public key from each side and the cascade flag. Session keys
come from HKDF-SHA256 over the X25519 secret mixed with the pairing key, per
connection and per direction. That gives forward secrecy: a pairing key leaked
later, for example from a Home Assistant backup that contains
`.storage/core.config_entries`, does not decrypt recorded sessions. Under v1 it
did.

Frames are encrypted with AES-256-GCM and carry a strictly increasing counter;
a repeated or out-of-order counter closes the session. The pairing key lives
in Windows Credential Manager and never reaches `config.json`; key material is
wiped from memory after use.

An entry accepts v1 only until its computer connects with v2 once. After that
v1 is refused, so the protocol cannot be downgraded. New entries accept only
v2.

Handshake nonces are single-use within the clock tolerance window. Before
0.5.0 a captured `hello` replayed inside that window produced a valid welcome
and tore down the live session, so an eavesdropper on a plain `ws://` hop could
disconnect a client at will without decrypting anything. Verifying a handshake
no longer touches session state; the session codecs are built only once the
connection is accepted.

### The endpoint before authentication

The Link endpoint in Home Assistant is reachable without a login, because the
handshake is what authenticates. Everything in the first frame can therefore
be chosen by an attacker.

- **Computer names.** Only a name made of `a-z`, `0-9` and `_`, up to 64
  characters, the form Deskmate itself creates, is accepted, for every protocol
  version, before anything is counted, logged or shown. Any other name is
  replaced by a fixed `?` and rejected: it never reaches the log, the failure
  counters or the *unpaired computer* issue in Repairs. The address shown in
  that issue is reduced to address characters too.
- **Lockout.** Ten failed handshakes for one computer from one address, or a
  hundred from one address, lock that sender out for five minutes. Failures
  are counted per computer and address, not per address alone: behind a
  reverse proxy every client can share one apparent address, and a
  per-address counter let one misconfigured machine lock out a working one.
- **Reasons.** A rejection carries a coarse reason so a client can tell a
  refused pairing from a network fault: `auth`, `locked`, `clock` (timestamp
  more than 90 seconds off, checked before the MAC and revealing nothing about
  pairing) and, only after a valid MAC, `cascade` or `version`. The endpoint
  never confirms whether a particular computer is paired.

### Cascade encryption

An opt-in second layer: ChaCha20-Poly1305 wrapped around the AES-256-GCM
frame, keyed from a separate key with its own HKDF labels and its own
associated data. It defends against a future weakness in a single cipher, not
against anything known to be broken today, and not against key theft: both
keys are stored side by side on each end. A mismatch between the two ends is
rejected, not negotiated down. The cascade key has its own Credential Manager
entry and is redacted from Home Assistant diagnostics.

### What Link does not do

- It does not authenticate Home Assistant beyond possession of the pairing
  key. Whoever obtains that key can impersonate either side, which is why it
  is shown once and stored in the credential vault on both ends. If it may
  have leaked, use *Reconfigure → Generate a new pairing key*.
- It does not hide that a connection exists. Deskmate accepts `ws://` only for
  private, `.local`, `.lan`, single-label and Tailscale addresses, so the
  computer name and the traffic pattern do not cross the internet in the
  clear. Anything else needs `wss://`.

## Link Files

Reading and receiving are separate settings and both start off. Home Assistant
can read only inside the folders listed under Shared folders, and can write
only new files into the inbox folder: no overwrite, rename or delete, file
names reduced to a plain name, data verified by SHA-256 before it becomes
visible, and the Mark-of-the-Web applied so Windows treats received files like
downloads. Shared folders must be absolute local paths; UNC and device paths,
`.` and `..`, alternate data streams, symlinks and junctions are refused. In
*Ask on this computer* mode a person accepts each file; the dialog is not shown
while Windows is locked.

The Home Assistant side (the Deskmate sidebar page, its HTTP API and the
`send_file` / `fetch_file` services) is limited to administrators. Services
called from automations have no user and are allowed; they can only touch Home
Assistant paths in `allowlist_external_dirs`. A compromised Home Assistant
administrator account can therefore place files in the inbox (in automatic
mode without anyone noticing) and read the shared folders. Keep the inbox in
*Ask* mode if other people administer your Home Assistant, and do not share
folders that contain secrets.

## Several Windows users on one computer

*Reconfigure → Show the pairing code* in Home Assistant shows a code with the
computer's existing pairing key, its node id and, when enabled, the cascade
key. Each Windows account keeps its own copy in its own Credential Manager.
Anyone who can paste that code into Deskmate can act as this computer towards
Home Assistant, exactly like with the original code, so it belongs only on
accounts of that computer. Revoking is all or nothing: a new pairing key cuts
off every account until the new code is pasted again.

The accounts agree on who connects through two named kernel objects in the
Windows `Global\` namespace (`Deskmate.Seat.<node>` and
`Deskmate.Seat.<node>.waiting`). They carry no data. Another local program
could create them to keep Deskmate from connecting, which is a local denial of
service only; such a program could just as well stop the app.

The [Deskmate service](#the-deskmate-service) uses the same seat. It takes it
only while no app wants it and gives it up as soon as one does. An app whose
user pressed *Disconnect* keeps the seat without connecting while the service
is installed, so the service does not connect against the user's wish.

## What Home Assistant can do on the PC

Nothing received from Home Assistant is executed. A command selects a fixed
action by its identifier; the only values taken from the network are the ones
described below, each checked and bounded.

### Clipboard

Reading and writing are separate settings.

| Publish clipboard to Home Assistant | Behaviour |
|---|---|
| `Off` | No clipboard entity exists and no clipboard text is sent. |
| `Confirm` | When the clipboard changes, Windows asks whether this value may be sent. Approval lasts until the clipboard changes again. A refused value is not asked about again. |
| `Automatic` | The current value is sent on every sensor interval without asking. |

| Allow clipboard writes from Home Assistant | Behaviour |
|---|---|
| `Off` | The write entity is removed and incoming writes are ignored. |
| `Confirm` | Every write shows a preview on the PC and needs approval. |
| `Automatic` | Home Assistant can replace the clipboard without a prompt. |

Both directions are blocked while Windows is locked. Writes are limited to
64 KiB and one write per two seconds. Clipboard contents are never written to
Deskmate's logs. Values sent to Home Assistant may stay in its recorder and
history after Deskmate stops sending them.

### URLs and notification images

Every URL that arrives from Home Assistant must:

- be an absolute `http://` or `https://` URL with a host;
- contain no whitespace, control characters or embedded credentials;
- be at most 2,048 characters;
- match an exact allowed origin (scheme, host and effective port).

The Home Assistant origins configured in the app are allowed automatically.
Other origins are entered in Settings, for example `https://example.com` or
`http://homeassistant.local:8123`. An allowlist entry is an origin without a
path, query or fragment, and it does not allow a different port or scheme. A
blocked notification image is left out; the text still shows. Downloads have a
10-second timeout and a 5 MiB limit, redirects are not followed because the
target could leave the approved origin, and downloaded images are deleted ten
minutes after display.

### Typing text

`type_text` (part of *Allow input*) types into whichever window has focus. It
sends characters through the SendInput Unicode path, which cannot press Ctrl,
Alt or Win, so it cannot trigger shortcuts. Control characters are removed
before typing: most applications treat a typed CR, LF, Tab or Esc as the real
key, and text followed by CR would otherwise run in a focused terminal.

### Your own PowerShell commands

Custom commands are powerful on purpose, but a value from Home Assistant is
never evaluated as PowerShell:

- Home Assistant selects a configured command only by its identifier;
- the control value is filtered, limited to 64 characters and passed only as
  `$env:DESKMATE_VALUE`;
- new and migrated commands are disabled until enabled on the Commands page;
- each command has its own *Require confirmation* switch, on by default;
- a required confirmation cannot be bypassed while Windows is locked;
- a disabled command is not announced to Home Assistant and is refused if
  called.

Do not put passwords or tokens in the command text: the script is stored in
`%APPDATA%\Deskmate\config.json`. Use a program that reads its own secret from
Credential Manager or another protected store.

### Your own sensors and program controls

*Start a program* opens the configured target with ShellExecute, *Stop a
program* ends processes with the configured executable name that the user's
account may end (never Deskmate), and *Press keys* sends the configured key
combination with SendInput to the window in front. None of them uses a shell,
and Home Assistant cannot change what they act on: only the press comes from
it. Key presses are refused while Windows is locked, and Ctrl+Alt+Delete cannot
be configured. PowerShell sensors run the configured command on an interval
and send the first line of its output; like PowerShell commands they start
disabled.

### Sound devices

The default device is changed through `IPolicyConfig`, the COM interface the
Windows sound settings use. It is not documented by Microsoft but has been
stable since Windows 7. Home Assistant sends a device name; Deskmate acts only
on a name that matches an active device it listed itself. Microphone mute and
volume use Core Audio like the speaker controls.

### Notifications

Titles, bodies and buttons are bounded, and at most ten notifications a minute
are shown. Speech has a 16-message queue and a 1,000-character limit.

Notification buttons work through the `deskmate:` link scheme, which any web
page or local program can also open. Every button therefore carries a
single-use random token that exists only in the running process; a click
without a valid token is dropped and logged. Buttons on notifications shown
before Deskmate restarted no longer work. Automations that react to a button
should still check for the exact action value they expect.

Deskmate shows notifications itself, through COM and WinRT. It starts
PowerShell only for the commands and sensors you created.

### Screen time and family rules

Off until turned on per Windows account. Counting happens on the PC, the usage
file is `%APPDATA%\Deskmate\screen-time.json`, and only minutes and the rule
states go to Home Assistant, never window titles or program lists (except the
block list the parent typed). The block list closes processes with the listed
names that the account may close; Windows processes and Deskmate itself are
refused.

Without protection the rules are a convenience: the child can quit Deskmate or
edit `config.json`. Protection is the administrator's part:

- *Protect this account* starts deskmate.exe through UAC with the account's
  SID as an argument (an administrator typing a password into UAC runs the
  step as the administrator, so the account cannot come from the elevated
  process itself). The SID is checked to be a user SID.
- The account is listed under `HKLM\SOFTWARE\Deskmate\Family\Protected`.
  Standard accounts can read but not change it.
- A copy of deskmate.exe goes to `%ProgramFiles%\Deskmate watchdog\`, where
  only administrators can write, and the scheduled task *Deskmate watchdog*
  runs it for members of Users at sign-in and every minute, without elevation.
  It starts the account's Deskmate when no Deskmate runs in that session, or
  itself in its place when the per-user install was deleted. For accounts that
  are not listed it exits at once.
- On a protected account the Family page cannot change the rules, the entities
  are declared as owned by Home Assistant, and the integration stores their
  values and sends them back when the PC reports a different one (at most
  every 30 seconds). Over MQTT that last part is missing.

What it does not cover: an administrator account, another Windows account,
another operating system, a PC kept offline after its settings file was edited
(the rules come back with the connection), or ending the task every minute by
hand, which standard accounts cannot do for a task an administrator created.

## The CPU temperature helper

Optional, installed from Settings with one UAC prompt. It is a copy of
deskmate.exe in `%ProgramFiles%\Deskmate CPU temperature\`, started by the
scheduled task *Deskmate CPU temperature* at the user's sign-in with the
highest privileges. The copy is placed where only administrators can write: a
task that ran the per-user install folder elevated would let any program the
user runs replace the file and gain administrator rights. The task is
registered through the Task Scheduler API from an in-memory definition, so
there is no temporary XML file to swap either.

The helper loads `PawnIOLib.dll` from the PawnIO installation (Program Files)
and one signed PawnIO module (IntelMSR or AMDFamily17, LGPL-2.1, embedded
unmodified). Those modules allow reading thermal registers; the helper uses
nothing else. It accepts no input: each connection to
`\\.\pipe\deskmate-cpu-temperature` (local clients only) gets one JSON line
with a temperature or an error code. It stops serving once Deskmate is
uninstalled for that user; *Remove* in Settings deletes the task and the copy.

## The Deskmate service

Optional, installed from *Settings → Deskmate service*. It keeps the
computer's sensors and power buttons in Home Assistant while nobody is
signed in. [ARCHITECTURE.md](dev/ARCHITECTURE.md#the-windows-service)
describes how it works.

### What runs, and with which rights

`deskmate-service.exe` in `%ProgramFiles%\Deskmate service\`, registered as
the Windows service `DeskmateService`. It runs as LocalSystem, and Windows
removes every privilege from its token except two: bypass traverse checking,
which every process needs to open files by path, and shut down the system.

It can:

- connect to Home Assistant as this computer, with the pairing key of the
  computer, and only while no Deskmate app on the computer wants the
  connection (the seat, see [Several Windows users](#several-windows-users-on-one-computer));
- report the sensors that need no signed-in user: processor, memory, the
  system drive, network, uptime, battery and AC power;
- press sleep, hibernate, shut down and restart. These four, with the action
  `press`, are the only commands it runs.

It cannot, by its code and by the integration's: show notifications, send or
receive files, read the clipboard, type, open URLs, run your own commands or
read anything from a user's session. Home Assistant sends a session with the
role `service` no notification and no file request, and the service refuses
them anyway. It reads neither the users' settings nor their Credential
Manager; the app hands it what it needs (below).

Power commands are refused for five minutes after Windows starts and after
every wake-up. With Fast Startup the time since boot does not start over when
someone shuts down, so the five minutes also count from the last wake-up. An
automation that presses Shut down in a loop cannot keep the computer from
starting.

Updating the service is a step of its own: an app update brings a newer copy
of the service but does not install it. Settings and the Status page offer
*Update* until someone presses it.

### Where its settings and keys live

- `%ProgramData%\Deskmate\service\config.json`: the node id, the Link
  addresses, cascade on or off, the interval, the device name and the sensor
  switches. Never a key.
- `service.log` and `security.log` next to it: operations and results, never
  the content of a frame.
- The pairing key and the cascade key: in the Credential Manager of the
  system account, as `machine.Deskmate Service Link` and
  `machine.Deskmate Service Link Cascade`, apart from the keys any user's app
  keeps.

`%ProgramData%\Deskmate` and the folder inside it allow access only to SYSTEM
and Administrators and inherit nothing from `%ProgramData%`. The service
checks both folders at every start and stops when anyone else could change
them or when one is a link. Standard users therefore cannot read the
settings, the logs or the keys. Administrators can, because an administrator
can always run a program as SYSTEM.

### The pipes

Both of the service's pipes refuse remote clients, and the service creates
each as the first instance of its name, so a program that took a name first
keeps the service from starting instead of receiving anything.

- `\\.\pipe\deskmate-service-admin`: SYSTEM and Administrators only. It
  takes the settings with the keys, makes the service forget them, or makes
  it read them again. A client checks that the service's process serves the
  pipe before it sends a key. Nothing that arrives is logged.
- `\\.\pipe\deskmate-service-status`: any signed-in local user may read one
  line: the state, the version, the node id, a short fixed text and a
  fingerprint of the settings. The fingerprint is HMAC-SHA256 under the
  pairing key, so it tells nothing to whoever does not hold that key. The app
  reads the line only when the service's own process serves the pipe, and
  checks every field before it shows any of it.

### The elevated step

*Install*, *Update*, *Apply* and *Remove* in Settings each start
`service\deskmate-service.exe` from Deskmate's install folder with one UAC
prompt: `install --provision-pipe <name>`, `install` or `uninstall`. The app
itself never runs elevated. The arguments are fixed in the app, and so is the
path, next to `deskmate.exe`; nothing received from Home Assistant chooses
either.

The settings and the keys go over a one-time pipe that the app creates before
the prompt. Its name holds 128 random bits. It is the first and only instance
of that name, refuses remote clients, and lets only the signed-in user and
Administrators read it, the second for an administrator who approves the
prompt with their own account; nobody may write to it or add an instance.
The app serves it to one reader, the process it has just started, and
disconnects any other client unread. It closes the pipe when that process has
ended, or after two minutes. The installer checks that the pipe is served by
a program named `deskmate.exe`, reads one line and passes it to the admin
pipe. The keys come from the user's Credential Manager; they never appear in
an argument, a file or the environment, and every copy the app makes is
wiped.

The residual risk is the same as for the CPU temperature helper's install
step: the program behind the prompt lives in the per-user install folder
`%LOCALAPPDATA%\Deskmate`, which any program running as the user can change.
Such a program could swap `deskmate-service.exe` and wait for the next
prompt, and approving it would run that program with administrator rights.
Approve the prompt only right after pressing a button in the panel, and check
that it names `deskmate-service.exe`. Once installed, the service runs from
Program Files, which a standard user cannot change, so the swap only works
during an approved step.

### Removing it

*Remove* in Settings, or uninstalling Deskmate, runs `uninstall` with one
administrator approval: the service forgets its settings and keys, is removed,
and both folders are deleted. An update of the app does not touch the
service. Without an installed service the uninstaller asks for nothing.

## Updates

Deskmate executes nothing that it receives over the network, with one
exception: a Deskmate installer that carries a valid signature from the
release key. This section describes that exception. Home Assistant has no part
in it.

### The three modes

Once a day, and when *Check now* is pressed, the app looks for a newer
release. Settings → Updates decides what follows:

| Mode | What is requested | What happens with a newer release |
|---|---|---|
| *Only tell me* (`notify`) | One HTTPS request to `api.github.com` for the latest release. Only the version number is taken from the answer. | A tray entry, a banner on the Status page and one notification, each leading to the release page. Nothing is downloaded. |
| *Ask first* (`ask`, the default) | One HTTPS request to `github.com` for the release manifest, `latest.json`, of the latest release. | The same, with *Install now* and *Later*. The installer is downloaded only after *Install now*. *Later* hides the question for a day. |
| *Install automatically* (`auto`) | The same manifest. | The installer is downloaded and run without a question, but only once the computer has had no keyboard or mouse input for five minutes and nothing runs full screen. |

Every request carries a `Deskmate/<version>` user agent and no other data.
When a release has no manifest, or its manifest is refused, the app falls
back to the request and the behaviour of *Only tell me*. Links to the release
page are built locally from the version number.

*Check for updates once a day* is the main switch. Off, Deskmate sends no
request, downloads nothing and installs nothing by itself, whatever the mode.

### What is checked before an installer runs

The manifest names a version and, for each processor architecture, a download
address and a signature. It is not trusted: it only says where to look. Before
anything is downloaded:

- The offered version must be strictly higher than the running one. An equal
  or lower version, a pre-release and a version that cannot be read are
  refused.
- The download address must be
  `https://github.com/JakubWawrzola/deskmate/releases/download/<tag>/<file>`.
  Another host, repository, scheme or port, a user name, a query or a deeper
  path is refused, and the refusal is logged.

The download stays on HTTPS, including the redirect to GitHub's file storage,
and stops after 64 MiB or ten minutes. Then:

- The file must carry a valid minisign (Ed25519) signature from Deskmate's
  release key. The public key is compiled into the app.
- The signature must name the version that the manifest offered. That version
  is part of what the key signed, so an older installer with a genuine
  signature cannot be passed off under a newer number.

Only a file that passed every check is written to the user's temporary folder
and started. It runs as the Windows user, in the installer's update mode:
it replaces the program files and starts Deskmate again. Settings, pairing,
*Start with Windows*, the CPU temperature helper and the family protection are
not touched.

The window cannot reach the updater: every step above runs in the app's
backend, and the window only sends *Install now*, *Later* and the chosen
mode.

### The release key

The private key signs the installers when a release is built. The maintainer
keeps it outside the repository; the build reads it from the environment. The
repository and the app hold only the public key.

Whoever can publish files under Deskmate's name on GitHub, or intercept the
connection, but does not have the key, cannot make Deskmate run anything: the
worst they can do is hide a new release. Whoever has the key and can also
change what the release address serves can run code on every computer that
installs updates. If the key is ever exposed, a new key ships with a new
release, and computers on older versions have to install that release by
hand.

### What Home Assistant cannot do

Link and MQTT have no update command. Home Assistant cannot start a check, an
install or a postponement, cannot change the mode, and cannot supply a
manifest, an address or a file. The buttons on the update notification use
their own `deskmate:update` link with single-use tokens; a notification sent
by Home Assistant cannot carry that link, and a click on it is not reported to
Home Assistant. The idle time that *Install automatically* waits for is
measured on the PC.

### Limits

- A valid signature says who built the installer, not that it is free of
  faults. *Only tell me* leaves every install to you.
- Updated installers are still not Authenticode-signed (see below).
- The copies of `deskmate.exe` used by the CPU temperature helper and the
  family watchdog stay at the version they were installed with.

## MQTT transport

MQTT is the older transport and stays available. It has no encryption of its
own above the connection: the broker, and every client allowed on the topics,
sees the states and can send commands. Use Link when that matters.

### TLS

MQTT defaults to TLS on port 8883. Server certificates are verified against
the Windows trust store or a PEM CA certificate you select, and verification
cannot be switched off in TLS mode. TLS uses Windows Schannel through
`native-tls`, so it works on x64 and ARM64 without OpenSSL, ring or Clang.

Use the broker hostname that is in the certificate's Subject Alternative Name;
a raw IP address fails unless the certificate lists it. For a private CA,
enter the CA certificate's local PEM path, not the broker's own certificate
(unless that certificate is its own trust anchor).

The official Home Assistant Mosquitto app enables port 8883 when `certfile` and
`keyfile` are configured from `/ssl`:

```yaml
certfile: fullchain.pem
keyfile: privkey.pem
require_certificate: false
```

`require_certificate: false` still requires the MQTT username and password; it
only means a client certificate is not mandatory. Once TLS clients work,
remove ports 1883 and 1884 on the app's Network card so the broker stops
listening without encryption. Source:
[Home Assistant Mosquitto app documentation](https://github.com/home-assistant/addons/blob/master/mosquitto/DOCS.md).

### Plain MQTT

Plain mode exists for migration and isolated networks, is an explicit choice
labelled trusted-LAN-only, and is allowed only without a fallback broker
address. It does not protect the MQTT username, password, sensor values or
commands in transit. Do not expose it to the internet or forward it through a
router.

### One user and one ACL per computer

Use a separate MQTT user for each Deskmate installation. For node
`laptop_jakub`, the least-privilege ACL is:

```text
user deskmate-laptop-jakub
topic read deskmate/laptop_jakub/cmd/+
topic read deskmate/laptop_jakub/notify
topic write deskmate/laptop_jakub/state/#
topic write deskmate/laptop_jakub/availability
topic write deskmate/laptop_jakub/notify/action
topic write deskmate/laptop_jakub/hotkey/#
topic write homeassistant/+/laptop_jakub/#
```

The Home Assistant Mosquitto app requires its internal users to keep broad
access when a custom ACL file is enabled:

```text
user addons
topic readwrite #

user homeassistant
topic readwrite #
```

Enable `customize.active`, point Mosquitto to an `acl_file`, and add one block
per Deskmate user. ACLs are enforced by the broker; a desktop client cannot
control what other broker users may publish.

Retained messages on the command and notification topics are ignored, so a
stale shutdown, clipboard or notification message is not replayed after a
reconnect.

## REST channel

Optional, used by hotkeys, widgets and tray actions to call Home Assistant.
The long-lived token is stored under its own `Deskmate HA Token` entry in
Credential Manager. The local URL may be HTTP on a trusted network; the
fallback URL must be HTTPS. Use a dedicated, non-administrator Home Assistant
user where its permissions are enough, and replace the token after any
suspected exposure.

Failover happens only on transport failures. An HTTP 4xx or 5xx answer does not
send the same token to a second host. Home Assistant URLs, `domain.service`
names and entity IDs are validated, and error bodies are not copied into the
window or the log.

## The Stream Deck channel

Off until you turn on *Let Stream Deck use Deskmate* in Settings. While it is
on, Deskmate serves a named pipe on this computer,
`\\.\pipe\deskmate-streamdeck-<user>`, for the
[Deskmate plugin for Stream Deck](STREAMDECK.md). Turning the switch off
closes the pipe and every connection at once. The protocol is described in
[STREAMDECK.md](STREAMDECK.md#local-channel).

### What it exposes

A program connected to the pipe can:

- list the entities of your Home Assistant with their names and states, and
  follow the state of up to 64 of them;
- toggle an entity and call any Home Assistant service, with the rights of
  the token of the [REST channel](#rest-channel);
- list your hotkeys and tray quick actions by name, and run one of them.

It learns whether Deskmate has a Home Assistant address and token, and the
Deskmate version. It cannot read the token, a pairing key or any other
setting, and it cannot change one. Nothing it sends is executed: `run` picks
one of the actions you defined, by its identifier, and that action runs as it
does from the tray. A custom command stays refused while it is disabled and
keeps its *Require confirmation* dialog, which cannot be bypassed while
Windows is locked.

Every field of a request is checked before it is used: entity ids, service
names and action ids against their patterns, lists against their limits. A
line is at most 64 KiB, a connection has at most 16 requests in progress, and
at most 16 programs are connected at a time.

### Who can connect

Windows decides, from the access rules Deskmate gives the pipe:

- the Windows user who runs Deskmate, and the system account, may connect;
- every other account on the computer is refused;
- a connection over the network is refused twice: the pipe is created for
  local clients only, and its access rules deny network logons.

There is no further check. Any program that runs as you can connect, not
only Stream Deck. Such a program is outside Deskmate's boundary: it can read
`config.json`, press keys, start programs, and read the Home Assistant token
from your Credential Manager, where a program running as you may read what
you stored. The channel gives it nothing it could not get, but it makes
acting in Home Assistant easier, and that is why the channel is off until you
use it.

### A program that takes the pipe's name

Pipe names are shared by the whole computer, and this one can be guessed. A
program, also one of another Windows account, could create a pipe of that
name before Deskmate does. Had Deskmate joined such a pipe, it would have
served it under that program's access rules, to whoever those let in.

Deskmate therefore only ever creates the pipe, with the option that makes
Windows refuse when the name exists, and never joins one. When the name is
taken, there is no channel: Settings shows *taken by another program*, the
audit log records `streamdeck_channel name_taken`, and Deskmate tries again
every 5 seconds. Once Deskmate holds the name, another account cannot add
instances to its pipe.

What remains is on the plugin's side. The plugin does not check who serves
the pipe it opens. A program that holds the name can refuse the plugin, or
answer it: it would then see which entities, services and actions your keys
ask for, and could show wrong states. It receives no token and no key: the
plugin sends none over the channel. Use the plugin's *Direct* connection on a
computer where you do not trust the other accounts.

### Installing the plugin

*Install plugin* copies the plugin that came with the Deskmate installer from
Deskmate's install folder into
`%AppData%\Elgato\StreamDeck\Plugins\com.deskmate.streamdeck.sdPlugin`. Both
folders are fixed in the app; the window supplies no path. Nothing is
downloaded, and nothing is copied when Stream Deck's folder does not exist.
The new copy is written completely before it replaces the old one. *Remove
plugin* deletes that one folder.

The plugin then runs inside Stream Deck, as you. Its settings panels load no
script from the network: the form library they use ships with the plugin. In
its *Direct* connection the plugin keeps a Home Assistant token in Stream
Deck's own settings, outside Credential Manager; with *Deskmate app* as the
connection, the plugin holds no token at all.

## The audit log

Security decisions are recorded in `%APPDATA%\Deskmate\security.log`: a Unix
timestamp, the event and a result such as `approved`, `denied`,
`blocked_locked` or `completed`, and for Link Files the operation and the
path. It never contains clipboard data, URLs, command scripts, file contents
or credentials. It rotates at 1 MiB to `security.log.1`.

An update install is recorded as `update_install from=<version> to=<version>
result=<result>`: `starting_installer` once the installer is verified and
about to run, and at the next start `ok` or `not_applied`. An attempt that
stopped carries the reason instead: `version`, `address`, `too_large`,
`network`, `signature`, `format` or `launch`. A refused manifest is recorded
as `update_manifest refused_address`, and a click on an update notification
without a valid token as `update_prompt blocked_unknown_token`.

The Stream Deck channel is recorded as `streamdeck_channel` with `on`, `off`,
`name_taken` or `failed`, and a program that connects or leaves as
`streamdeck_client connected` and `disconnected`. What a program does through
it is recorded with its result: `streamdeck_run action=<id> result=<result>`,
`streamdeck_toggle entity=<id> result=<result>` and
`streamdeck_call service=<domain.service> entity=<id> result=<result>`, where
the result is `ok`, `unknown_action`, `ha_error` or `ha_not_configured`. The
data of a service call is not recorded, and neither is anything from a
request that was refused as malformed. Installing and removing the plugin is
recorded as `streamdeck_plugin` with `installed`, `updated`, `removed`,
`install_failed` or `remove_failed`.

## The app itself

- The window runs under a Content Security Policy: scripts only from the
  bundled app and no remote connections, only the channel to the app's own
  backend.
- Secrets (the pairing key, the cascade key, the MQTT password, the REST
  token) are in Windows Credential Manager, never in `config.json`, logs,
  diagnostics or error messages.
- Release installers are not Authenticode-signed. Windows SmartScreen may warn
  on first run; verify the SHA-256 sums published with each release. An
  update that the app installs itself is verified against the release key
  instead (see [Updates](#updates)).

## Known limits

- Whoever controls Home Assistant controls every capability set to `Automatic`
  and the built-in controls.
- A pairing key copied from Home Assistant's storage works until it is
  replaced.
- A clipboard value you allowed is plain text inside Home Assistant and may be
  kept by its recorder and backups.
- With MQTT, TLS protects the traffic, not a broker account with too broad an
  ACL. Plain MQTT is unsafe by design even though the app warns about it.
- Deskmate cannot protect against malware already running as the Windows user.
- While the Stream Deck channel is on, every program running as the Windows
  user can act in Home Assistant through it, and the plugin does not verify
  that the pipe it opens is Deskmate's.
- Updates are as trustworthy as the release key and the person who holds it.

## Checklist

1. Keep the pairing code out of YAML, notes and source control; replace the
   key if it may have leaked.
2. Use `wss://` for any address that is reachable from the internet.
3. Keep both clipboard modes `Off` unless needed; prefer `Confirm` over
   `Automatic`.
4. Keep the file inbox in *Ask* mode if other people administer your Home
   Assistant, and do not share folders that hold secrets.
5. Keep custom commands disabled until reviewed, with confirmation on for
   anything destructive.
6. Allow only the URL origins your automations need.
7. With MQTT: TLS on 8883, no listeners on 1883 or 1884, one user and one ACL
   block per computer.
8. Leave the Stream Deck channel off unless you use the plugin.
9. Look at `security.log` after unexpected activity.

## Reported and fixed

| Report | Fix |
|---|---|
| MQTT can be sniffed or commands injected from the LAN (public review) | TLS is the default; plain MQTT is explicit and cannot use a fallback; per-computer credentials and ACLs are documented. Deskmate Link, added later, encrypts above the transport. |
| `open_url` can probe arbitrary local targets (public review) | Strict URL parser and exact-origin allowlist, shared with notification images. |
| Home Assistant can read passwords from the clipboard or replace pasted text (public review) | Reading and writing split into `Off`, `Confirm`, `Automatic`; both default off, stop while locked, and writes have a preview, a cooldown and a size limit. |
| Home Assistant can run arbitrary PowerShell configured by the user (public review) | Values are never evaluated as code; commands are enabled one by one and can require confirmation on the PC for every run. |
| Typed text followed by Enter could run in a focused terminal (#1, reported by @anupamme) | Control characters are removed before typing. |
| A replayed `hello` could disconnect a live session (before 0.5.0) | Verifying a handshake no longer touches session state. |
| Antivirus heuristics flagged the PowerShell helpers used for notifications (#2, up to 0.7.0) | Notifications are shown through COM and WinRT without helper processes. |
| A crafted computer name from an unauthenticated connection could reach the Repairs text (HACS review) | Fixed in 0.8.4: names are validated for every protocol version before they are counted, logged or shown. |
