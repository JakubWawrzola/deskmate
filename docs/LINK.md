# Deskmate Link

Deskmate Link is the connection between the Windows app and the Home
Assistant integration (named **Deskmate** in Home Assistant and HACS). For
everyday setup and use, read [GUIDE.md](GUIDE.md); this document covers the
connection itself.

Deskmate Link is the recommended transport between the Windows app and the
`deskmate_link` Home Assistant integration: one encrypted WebSocket, no broker.
MQTT stays available and can be selected again without losing its settings.

## Set up Home Assistant

Update the integration before Deskmate. Deskmate 0.6 speaks protocol v2 only,
and an integration older than 0.6.0 refuses it as a wrong key.

1. Install the `deskmate_link` integration. With HACS: search for **Deskmate**
   (it is in the default catalog) and download it. Without HACS: copy
   `custom_components/deskmate_link` from this repository into
   `config/custom_components/`. Restart Home Assistant once either way.
2. **Settings → Devices & services → Add integration**, search for
   **Deskmate**, select it and confirm. There is nothing to type.
3. The next screen shows a **pairing code** starting with `DMP1.`. It carries
   the pairing key and this Home Assistant's local and remote addresses, as far
   as Home Assistant knows them (*Settings → System → Network*). The bare key is
   shown below it for older Deskmate versions.
4. If the dialog was closed before the code was copied, just add the integration
   again: an entry that is still waiting for pairing shows its code again
   instead of creating a second entry.
5. Do not put the pairing code or key in YAML, `config.json` or source control.

To rotate a key or move an entry to a different computer, open the entry and
choose **Reconfigure**: *Generate a new pairing key* invalidates the old key,
*Unbind from the current computer* releases the entry so the next computer
using that key takes it over. Entities and their history survive both.

*Show the pairing code* shows the code of a paired computer again: for a
reinstalled Deskmate, or for a second Windows account on the same PC. It carries the same key plus the computer's node id
(and the cascade key when cascade is on), so that account reports as the same
device. The accounts coordinate through a named object in the Windows Global
namespace: only one holds the connection, and an account whose session is not
the one in use hands it over to an account that is. Nothing changes in the
entry itself, and a new pairing key cuts off all accounts at once.

If a computer ends up paired to a second entry (for example after pairing again
because the first attempt failed), the entry it actually connects with takes
over the entities of the old one, keeps their entity IDs and history, and the
old entry is removed. Entities no longer get duplicated with a `_2` suffix.

## Set up Deskmate

1. On first run the wizard starts on the Deskmate integration. Paste the
   pairing code: the key, the local address and the remote address are filled
   in from it. Later changes are under **Settings → Deskmate integration**,
   where the key field accepts a pairing code as well.
2. Check the addresses. Deskmate appends `/api/deskmate_link/ws` itself. Plain
   `ws://` is accepted only for LAN addresses, `.local`, `.lan`, single-label
   hostnames and Tailscale (`100.64.0.0/10`, `*.ts.net`). Anything reachable
   from the internet needs `wss://`.
3. Choose **Save & connect**. The key is stored in Windows Credential Manager,
   never in `config.json`.
4. Check Status for `Connected to Home Assistant`, then find the device under
   Settings → Devices & services → Deskmate.

Local and fallback connections perform a fresh authenticated handshake and
derive fresh session keys on every reconnect.

## When the connection is refused

Home Assistant answers a failed handshake with a reason, and Deskmate shows it:

- **`Link rejected ... (node "<name>")`**: no paired entry accepted this
  computer. The entry is disabled or deleted in Home Assistant, the pairing
  key does not match, the entry is bound to a different computer, or the
  integration is older than 0.6.0. Update the integration, pair again, or open
  the entry and choose Reconfigure → Unbind. To stop the attempts, press
  *Disconnect* on the Status page.
- **`Link clock mismatch ...`**: this computer's clock and Home Assistant's
  differ by more than 90 seconds. Sync the Windows time (*Settings → Time &
  language → Sync now*) or fix the clock on the Home Assistant host. Dual-boot
  PCs where Linux keeps the hardware clock in UTC are the usual cause. Before
  0.6.0 this looked exactly like a wrong key.
- **`... cascade encryption is on at one end only`**: the key is right, but only
  one side has cascade enabled.
- **`... does not accept this protocol version`**: the integration is too old,
  or this entry already moved to v2 and something tried to connect with v1.
- **`Link locked out ...`**: Home Assistant is temporarily refusing this
  computer after repeated failed handshakes. It clears itself within five
  minutes once the cause is fixed.

Rejected handshakes retry on a slower schedule (5, 15, 30, then 60 seconds; 30
seconds for a clock problem) because retrying cannot succeed and only keeps the
node locked out. Ordinary network failures keep the fast retry. Home Assistant
also raises a repair issue naming a computer that keeps failing.

## Remote access (Cloudflare Tunnel / Nabu Casa)

Link works transparently through a reverse proxy that forwards WebSocket
traffic, since it is just one more path under the Home Assistant API. Verified
working setups:

- **Cloudflare Tunnel**: point the Deskmate WebSocket URL at your tunneled
  hostname, e.g. `wss://your-domain.example`. No extra Cloudflare
  configuration is needed beyond the existing tunnel route to Home Assistant's
  `8123` — the same route that serves the frontend also serves
  `/api/deskmate_link/ws`.
- **Nabu Casa remote UI**: use the `https://xxxx.ui.nabu.casa` hostname as
  `wss://xxxx.ui.nabu.casa`.
- **Tailscale**: use the machine's `100.x` tailnet address as
  `ws://100.x.x.x:8123` (still unencrypted transport-wise if not using
  `wss://`, but the tailnet link itself is already end-to-end encrypted).

Pick whichever is already reachable from where Deskmate is running; switching
between them later is just editing the URL field and reconnecting.

## Protocol v2

Deskmate 0.6 and the 0.6.0 integration use protocol v2:

1. Deskmate creates a fresh X25519 key pair for the connection and sends
   `hello` with the node name, a random 16-byte nonce, a timestamp, its public
   key and the cascade flag, authenticated with HMAC-SHA256 under the pairing
   key.
2. Home Assistant checks the timestamp (90 s), the MAC and that the nonce has
   not been seen before, creates its own X25519 key pair and answers `welcome`
   with its nonce, timestamp and public key. That MAC covers a SHA-256 hash of
   the whole `hello`, so changing any `hello` field breaks it.
3. Both sides compute the X25519 secret and derive direction-specific session
   keys with HKDF-SHA256: input keying material = X25519 secret || pairing key,
   salt = hash of both handshake messages.

Every MAC and KDF input uses a length-prefixed encoding, so no two different
field lists produce the same bytes. The ephemeral keys and the derived keys are
wiped from memory after use on the Deskmate side.

What this changes in practice: before v2 the session keys came from the pairing
key and two public nonces. Anyone who recorded the traffic and later got the
pairing key (for example from a Home Assistant backup, which contains
`.storage/core.config_entries`) could decrypt every recorded session. With v2 a
leaked key lets an attacker impersonate one side from then on, which rotating
the key stops, but it no longer opens old recordings.

Existing entries accept v1 until the first successful v2 connection. After that
the entry records `min_version: 2` and refuses v1 from that computer, so a
downgrade is not possible. New entries accept only v2.

## Cascade encryption

Every Link frame is encrypted with AES-256-GCM under a session key. Cascade adds
a second layer around it: ChaCha20-Poly1305, keyed from a separate cascade key,
derived with its own HKDF labels and authenticated over its own associated
data.

Be clear about what it buys. It helps only if one of the two ciphers is ever
broken. Both keys are stored in the same places (the integration entry in Home
Assistant, Credential Manager in Deskmate), so anyone who steals one steals
both. Protection against a stolen key comes from the X25519 exchange in
protocol v2, which is always on.

Enable it in Home Assistant on the paired entry: **Reconfigure → Enable cascade
encryption**. Copy the second key it shows into Deskmate under **Geeky stuff**
and turn the toggle on there. Deskmate refuses a cascade key identical to the
pairing key.

Both ends must agree. A handshake where one side asks for cascade and the other
does not is rejected rather than negotiated down. The flag is covered by the
handshake MAC, so it cannot be flipped on the way.

## Session frames

After `welcome`, both sides send JSON objects called frames. Each frame travels
encrypted, as one WebSocket text message:

```json
{"t": "e", "n": 1, "p": "<base64 ciphertext>"}
```

`n` is the sender's frame counter. It starts at 1 and each direction counts on
its own. A frame whose counter is not higher than the last one accepted ends
the session. `p` decrypts to the frame, and its `t` field names the type:

| `t` | Sent by | Other fields | What it does |
|---|---|---|---|
| `info` | Client | `role`, `platform`, `app` | Says what the client is. Sent first, by clients that know the frame. |
| `info` | Home Assistant | `integration`, `caps` | Answers a valid `info` from the client. |
| `declare` | Client | `device`, `entities` | Replaces the device details and the complete list of entities. |
| `state` | Client | `s` | Updates entity states. `s` maps entity keys to values and may hold any subset of them. |
| `cmd` | Home Assistant | `id`, `key`, `action`, `value` | Acts on the entity `key`. `action` is `press` for a button and `set`, with `value`, for everything else. |
| `ack` | Client | `id`, `ok`, `error` | Result of the `cmd` or `notify` with that `id`. `error` is a text, present when `ok` is false. |
| `notify` | Home Assistant | `id`, `title`, `message`, `image`, `actions` | Shows a notification. `image` and `actions`, a list of `{"id", "title"}` buttons, are optional. |
| `notify_action` | Client | `action` | A notification button was clicked; `action` is its `id`. Fires `deskmate_link_notify_action`. |
| `trigger` | Client | `key`, `event` | An event entity fired, for example a hotkey. `event` is `press` when left out. Fires `deskmate_link_trigger`. |
| `fs` | Home Assistant | `id`, `op` and its arguments | A Link Files request. |
| `fs_res` | Client | `id`, `ok`, then `error` or the result | The answer to the `fs` with that `id`. |
| `tv` | Home Assistant | `id`, `op` and its arguments | A request to DeskmateTV, described in [TV.md](TV.md). Sent only to a device that declared itself a TV. |
| `tv_res` | Client | `id`, `ok`, then `error` or the result | The answer to the `tv` with that `id`. |
| `ping` | Either side | none | Asks for a `pong`. Deskmate sends one every 30 seconds. |
| `pong` | Either side | none | Answers a `ping`. |

Home Assistant numbers `cmd`, `notify`, `fs` and `tv` from one counter per
computer and matches each answer by its `id`. It waits 10 seconds for an `ack`
and 15 seconds for an `fs_res` (180 for `put_begin`, which may need a click on
the computer, and 60 for `put_end`).

The integration ignores a frame type it does not know. Deskmate does the same
and writes a warning to its log.

### `info`

```json
{"t": "info", "role": "app", "platform": "windows", "app": "0.9.0"}
```

- `role` is `app`, `service` or `daemon`.
- `platform` is `windows`, `linux` or `android_tv`.
- `app` is the client's version: three numbers separated by dots, optionally
  followed by `-` or `+` and up to 20 letters, digits, dots or hyphens, such as
  `0.9.0-beta.1`.

Home Assistant keeps the three values for the length of the session, shows
them in the entry's diagnostics, and answers:

```json
{"t": "info", "integration": "0.9.0", "caps": ["info"]}
```

`integration` is the version of the integration. `caps` lists the optional
frame types it understands, so a client can tell what an older integration
lacks.

An `info` with a missing or invalid value is ignored: it gets no answer and
changes nothing. Home Assistant logs a warning that names the field, never the
value it received. Fields other than these three are ignored. A client that
never sends `info`, such as Deskmate up to 0.8.5, gets no `info` from Home
Assistant, and nothing else changes for it.

### `declare`

`device` holds `name`, `model` and `sw_version`. Each item of `entities` has a
`key` and a `kind`, normally a `name` (the key stands in for a missing one),
and may add fields that its kind reads:

| `kind` | Optional fields |
|---|---|
| `sensor` | `icon`, `unit`, `device_class`, `state_class` |
| `binary_sensor` | `icon`, `device_class`, `link` (true marks the entity that shows whether the computer is connected) |
| `switch` | `icon`, `restore` |
| `number` | `icon`, `min`, `max`, `step`, `unit`, `restore` |
| `button` | `icon` |
| `text` | `icon`, `max`, `mode`, `pattern`, `restore` |
| `select` | `icon`, `options`, `restore` |
| `event` | `icon`, `event_types` |

`restore` marks a value that Home Assistant owns: it remembers the last value
set from its side and sends it again when the computer reports a different
one.

Every `declare` is complete. An entity whose key is missing from it is removed
from Home Assistant. The last `declare` is stored, so the entities exist after
a Home Assistant restart, before the computer reconnects.

### `fs` and `fs_res`

| `op` | Arguments | Result in `fs_res` |
|---|---|---|
| `roots` | none | `roots`, `inbox`, `max_bytes` |
| `list` | `path` | `entries` |
| `stat` | `path` | `stat` |
| `read` | `path`, `offset`, `len` (at most 262144) | `data` (base64), `eof` |
| `put_begin` | `name`, `size`, `by` | `upload`, `name` |
| `put_chunk` | `upload`, `offset`, `data` (base64) | `written` (bytes received so far) |
| `put_end` | `upload`, `sha256` | `name`, `dir`, `size`, `sha256` |
| `put_abort` | `upload` | none |

`stat` and each item of `entries` hold `name`, `dir` (true for a folder),
`size` and `mtime`. In the result of `put_end`, `dir` is the folder the file
was saved in. `by` names who is sending the file, for the confirmation dialog.
`upload` is the token that `put_begin` returns for the transfer. What the
computer checks before it answers is listed under
[Link Files](#link-files-sending-and-fetching-files).

## Text controls, presentation and hotkeys

Link declares the existing sensor, binary sensor, number, button, switch and
enabled custom-control definitions with the same keys and names used by MQTT.
Sensor updates remain partial and use the configured publish interval. Commands
return an acknowledgement to Home Assistant. Notifications support title,
message, optional image and action buttons; a click produces the
`deskmate_link_notify_action` event in Home Assistant.

Link v0.2 also declares the same conditional text controls as MQTT:
`type_text`, `open_url`, `tts_say` and `clipboard_set`. Their names are exactly
`Type text`, `Open URL`, `Say (TTS)` and `Set clipboard`; the existing opt-in,
allowlist, confirmation, lock-screen and size checks still apply. Presentation
controls remain buttons and use the same `Presentation ...` names as MQTT.

Every configured global hotkey is declared as a Link event entity named
`hotkey: <name>`, with event type `press`. Pressing it sends a `trigger` frame;
Home Assistant updates the event entity and fires `deskmate_link_trigger` with
`node_id`, `key` and `event`. The key is `hotkey_<hotkey id>`. Hotkeys using a
local/API action still perform that action; the dedicated HA event action only
publishes the event.

After every entity or hotkey configuration change, Deskmate sends a complete
new declaration. Link v0.2 removes entities omitted from that declaration, so
turning an option off or deleting a hotkey/custom control needs no integration
reload.

## Hardware sensors

On Windows systems that expose the corresponding counters, Deskmate declares
GPU usage, GPU memory used/total, free/used space per local volume, aggregate
disk read/write speed and CPU/GPU temperatures. It uses native PDH, DXGI and
WMI plus the existing lightweight disk collector. Unsupported readings are not
declared and never receive synthetic values. The same detected set is used for
MQTT discovery and Link `declare`.

## Link Files: sending and fetching files

Two independent capabilities, both off by default:

- **Receive files** (Files page → Receive files): Home Assistant can put new
  files into one folder on the computer, `Downloads\Deskmate` unless you pick
  another. Mode *Ask on this computer* shows a dialog for every file (refused
  while Windows is locked); *Accept automatically* does not ask.
- **Shared folders** (Files page → Shared folders): Home Assistant can list and read
  files in the folders you add. Nothing else on the disk is reachable.

### The Deskmate page

The integration adds **Deskmate** to the Home Assistant sidebar, visible to
administrators only, with a **Files** and a **Computers** tab. On the Files tab,
from a phone (the Home Assistant app works) or a laptop:

1. pick the computer at the top if you have more than one;
2. **Send**: choose files or drop them on the page. Each file shows progress and
   the name it was saved under;
3. **Browse**: open a shared folder and download a file to the device you are
   on.

Uploads go to Home Assistant in 8 MiB requests, so they also pass a Cloudflare
tunnel (which rejects request bodies over 100 MB), and from there to the
computer in 256 KiB encrypted Link frames. With *Ask on this computer*, the
first request waits for someone to click Accept; through a Cloudflare tunnel
that wait must stay under about 100 seconds.

### Services for automations

- `deskmate_link.send_file`: copy a file from Home Assistant to the computer's
  inbox, e.g. a camera snapshot from `/media`. The path must be listed in
  `allowlist_external_dirs` (`/media` is by default).
- `deskmate_link.fetch_file`: copy a file from a shared folder on the computer
  into Home Assistant, under `allowlist_external_dirs` as well.

Both return the resulting name and location, and refuse non-admin users when
called from the UI.

### What the computer enforces

- Received files: the name is reduced to a plain file name (no folders, no
  `..`, no reserved device names or characters, at most 150 characters). An
  existing file is never overwritten; the new one becomes `name (1).ext`. Data
  is written to a hidden `.part` file first, checked against the SHA-256 Home
  Assistant computed, and only then renamed. Every received file gets the same
  "downloaded from another computer" mark a browser adds, so SmartScreen and
  Office Protected View treat it like a download. Half-finished uploads are
  deleted when they stall for two minutes or the Link session ends. At most four
  transfers run at once.
- Shared folders: absolute local paths only; UNC and device paths, `.`/`..`,
  alternate data streams, symlinks and junctions are rejected. Reads go in
  256 KiB chunks through a 4 MiB/s rate gate.
- One size limit (Files page → Receive files → Max size, 256 MB by default) applies
  to both directions.
- Every transfer is recorded in `%APPDATA%\Deskmate\security.log` with its
  operation, name and result, never its content.

Link Files has no rename, delete or overwrite operation, and the inbox cannot be
read back through Home Assistant unless you also add it as a shared folder.

## MQTT and Link parity

| Capability | MQTT | Deskmate Link |
|---|---|---|
| Sensors, binary sensors and volume | MQTT discovery/state topics | Same names and partial states |
| Built-in and presentation buttons | Command topics | `cmd` with encrypted `ack` |
| Keep-awake and custom controls | Switch/number/button discovery | Same kinds, keys and names |
| Text input, URL, TTS, clipboard write | MQTT text entities | Link text entities with the same names |
| Clipboard read | MQTT sensor | Link sensor |
| Hotkeys | MQTT device trigger for the event action | Event entity plus `deskmate_link_trigger`; every configured hotkey emits `press` |
| Toasts and action buttons | Notify/action topics | Encrypted notify/ack and `deskmate_link_notify_action` |
| Hardware sensors | Dynamic MQTT discovery | Same detected set in `declare` |
| Your own sensors (program, window, service, PowerShell) | Discovery under `csb_<id>` / `cs_<id>` | Same keys; on/off sensors sent as booleans |
| Program controls (start, stop, press keys) | Button discovery | Buttons, same keys as custom controls |
| Microphone mute and volume | Switch and number discovery | Same keys (`mic_mute`, `mic_volume`); switches sent as booleans |
| Default sound devices | Select discovery, options re-published when a device comes or goes | `select` entities; a new `declare` carries the new options |
| Screen time and family rules | Same entities through discovery | Same keys; on a protected account the rule entities are marked `restore` and Home Assistant puts back its values |
| Files | Not available | `fs` / `fs_res`: read from shared folders, write to the inbox; both off by default |
| Native MQTT device-trigger representation and raw topics | Supported | MQTT-only; Link uses its event entity and event-bus equivalent |
| Hotkeys/widgets/tray using the direct HA API | Independent of transport | Independent of transport |

## Security notes

- Handshake: protocol v2 as described above. Timestamp skew is limited to 90 s
  and every client nonce is accepted once.
- Session keys: HKDF-SHA256 over an ephemeral X25519 secret and the pairing
  key. Client-to-server and server-to-client keys are separate.
- Frames use AES-256-GCM with authenticated node and direction metadata and a
  strictly increasing counter. A replayed or invalid frame closes the session.
- WebSocket TLS uses Windows Schannel for `wss://`; Link cryptography uses only
  RustCrypto crates and `x25519-dalek`, all pure Rust.
- Treat the pairing key like a password. If it may have been exposed, use
  Reconfigure → Generate a new pairing key.
- Toast buttons carry a single-use token. A `deskmate:action?...` link opened
  from a web page or another program is ignored, so it cannot fire Home
  Assistant automations.

## Troubleshooting

- `Link pairing key missing`: paste the key in Settings and save again.
- `Link clock mismatch`: correct the Windows and Home Assistant clocks.
- Repeated local connection failures rotate to the configured fallback URL.
- If entities do not match the current settings, save the relevant setting or
  reconnect once. Both operations send a fresh full declaration; Link v0.2
  prunes omitted entities automatically.
