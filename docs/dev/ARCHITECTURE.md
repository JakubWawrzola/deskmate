# Architecture

## Overview

```
┌────────────────────────────── Windows PC ──────────────────────────────┐
│  Deskmate (Tauri 2)                                                    │
│  ┌───────────────┐   tauri commands/events   ┌───────────────────────┐ │
│  │ React UI      │ <───────────────────────> │ Rust backend          │ │
│  │ (5 pages +    │                           │  config / state       │ │
│  │  wizard)      │                           │  sensors (sysinfo +   │ │
│  └───────────────┘                           │   WinAPI + SMTC)      │ │
│                                              │  sys_commands         │ │
│                                              │  notify (WinRT toast) │ │
│                                              │  mqtt (rumqttc)       │ │
│                                              └──────────┬────────────┘ │
└─────────────────────────────────────────────────────────┼──────────────┘
                                                          │ MQTT 3.1.1 + TLS
                                              ┌───────────▼────────────┐
                                              │ Broker (np. Mosquitto) │
                                              └───────────┬────────────┘
                                                          │ MQTT discovery
                                              ┌───────────▼────────────┐
                                              │ Home Assistant         │
                                              └────────────────────────┘
```

## Data flow

- **Outbound**: sensor loop (interval from config, default 15 s) collects values
  (blocking WinAPI calls run on `spawn_blocking`), publishes to
  `deskmate/<node>/state/<key>`, caches values and emits a `deskmate://sensors`
  event to the UI.
- **Discovery**: on every ConnAck the full retained discovery set is published
  (`homeassistant/<component>/<node>/<object>/config`). Disabling a sensor
  publishes an empty retained payload = HA removes the entity.
- **Inbound commands**: subscription `deskmate/<node>/cmd/+`. Retained events
  are rejected. The key is matched
  against a fixed allowlist (`sys_commands::run_builtin`) or, with the
  `custom_` prefix, against commands stored in config. **Payload content is
  never executed** — the only parsed payload is the numeric volume.
- **Notifications**: subscription `deskmate/<node>/notify`, JSON payload,
  optional allowlisted image downloaded (5 MB cap) to temp and attached to a
  WinRT toast. Rendering is limited to 10 notifications/minute.
- **Availability**: LWT retained `offline` on `deskmate/<node>/availability`,
  explicit `online` after connect. Sensors also carry `expire_after` = 4x
  publish interval, so values go `unavailable` if the app dies silently.

## Key decisions

| Decision | Why |
|---|---|
| MQTT + discovery instead of a custom HA integration | works with vanilla HA, zero server-side install; same approach made HASS.Agent popular |
| rumqttc + ureq with native-tls (Schannel) | verified MQTT/REST TLS on x64 and ARM64 without OpenSSL, ring or Clang |
| Password in Windows Credential Manager (keyring) | config.json can be safely backed up / shared |
| SMTC for media | one API covers Spotify, browsers, UWP apps; also gives us control (play/pause/next/prev) without simulating key presses |
| Session lock detection via `OpenInputDesktop` | no message pump needed; polling fits the sensor loop |
| Toast via `tauri-winrt-notification` | dev builds use the PowerShell AUMID (works unregistered), release uses the NSIS-registered app id |
| One retained discovery snapshot per connect | idempotent, survives HA restarts, no per-entity bookkeeping |
| Clipboard read/write as separate Off/Confirm/Automatic capabilities | avoids silently turning a privacy sensor into a remote write permission |
| Exact URL-origin allowlist | preserves explicit local HA/CDN use while blocking arbitrary SSRF targets |
| Metadata-only rotating security log | audit decisions without persisting clipboard, URL or command contents |

## Control channel PC → HA (v0.3)

Everything above is HA → PC over MQTT. v0.3 adds the opposite direction:

- `ha_api.rs` — minimal REST client (`POST /api/services/...`,
  `GET /api/states/...`) with a local + fallback URL (failover on transport
  errors only) and the long-lived token in Credential Manager.
- `actions.rs` — one executor for an `ActionSpec` (`toggle` / `service` /
  `command` / `widget` / `mqtt`), shared by hotkeys and tray quick actions.
- `hotkeys.rs` — global shortcuts via `tauri-plugin-global-shortcut`; a single
  plugin handler resolves the pressed shortcut against the config and runs the
  action on the async runtime. `mqtt`-kind hotkeys publish to
  `deskmate/<node>/hotkey/<id>` and are announced as `device_automation`
  discovery, so they appear as device triggers in HA's automation editor.
- **Widget panel** — a second, hidden-by-default always-on-top Tauri window
  (`index.html#widget`) rendering entity tiles; it polls `widget_states`
  (REST, 3 s) and toggles entities on click. Windows lifecycle: closing any
  window hides it; the app lives in the tray.

## Stream Deck

`src-tauri/src/streamdeck/` is the app's side of the Stream Deck plugin
(`streamdeck-plugin/`): the local channel the plugin talks to, specified in
[STREAMDECK.md](../STREAMDECK.md#local-channel), and the installer of the
plugin.

- `server.rs` and `pipe_security.rs` — the named pipe of the current user:
  created with its own access rules, never joined, one free instance always
  listening. Tokio serves it; there is no thread per client.
- `session.rs` — one connection as one task: `framing.rs` cuts the bytes into
  lines, `protocol.rs` validates each request, and the work that reaches Home
  Assistant or runs an action goes on next to the reading, so a slow answer
  does not hold up a key press. Watched entities are read from Home Assistant
  on an interval, only while a connection watches any.
- `backend.rs` — the two traits a connection works through. `app.rs`
  implements them with `ha_api.rs` and `actions.rs`; the tests implement them
  in memory.
- `plugin_install.rs` — copies the plugin from the app's resources into
  Stream Deck's plugin folder. The resources are listed in `tauri.conf.json`,
  and `build.rs` makes sure an installer is not built without them.

## Crates

`src-tauri/` is a Cargo workspace with three crates.

| Crate | Path | What it holds |
|---|---|---|
| `deskmate` | `src-tauri/` | The app: window, tray, Tauri commands, the connection loops (`mqtt.rs`, `link.rs`, `transport.rs`) and everything that reads or acts on the signed-in user's session (`sensors.rs`, `sys_commands.rs`, `audio.rs`, `media.rs`, `clipboard.rs`, `keys.rs`, `family.rs`, `notify.rs`, `link_files.rs`). `platform.rs` says what the desktop offers; `linux/` holds the Linux desktop's side, described under [The Linux desktop app](#the-linux-desktop-app) |
| `deskmate-core` | `src-tauri/crates/core/` | Code that does not depend on Tauri and works without a signed-in user. A program without a window, such as a service or a daemon, is built on it. It builds for Windows and for Linux |
| `deskmate-service` | `src-tauri/crates/service/` | The Windows service and the Linux daemon: the computer's sensors and power buttons while nobody is signed in. Described under [The Windows service](#the-windows-service) and [The Linux daemon](#the-linux-daemon) |

Modules of `deskmate-core`:

| Module | What |
|---|---|
| `config`, `consts`, `security` | The settings file and its folders, the stored secrets by name, shared names, the confirmation dialog and the audit log |
| `secrets` | `SecretStore`: where secrets are kept, the user's keyring or key files |
| `sensors` | The sensor registry (`SENSOR_DEFS`) and `MachineCollector`: CPU, memory, system drive, network, uptime, battery |
| `hardware`, `cpu_temperature` | Drives, disk activity, GPU, temperatures. `cpu_temperature` is the PawnIO helper and exists on Windows only |
| `linux` | Linux only: the readers of sysfs (power supplies, thermal sensors, graphics cards), the choice of drives, and whether the computer can hibernate |
| `sys_commands` | The command descriptors (`COMMAND_DEFS`), the power commands and the URL checks |
| `custom_sensors` | Key, component and icon of the sensors a user adds |
| `discovery` | The MQTT discovery messages and the Link `declare` frame |
| `link` | The Link protocol: URL and key checks, the handshake, the frame codec, the `info` and `state` frames |
| `machine` | The machine scope: descriptors, collector, `declare` and `state` frames and the power commands for a client without a signed-in user |
| `seat` | One connection per computer when several programs on it are paired as the same node. `seat::linux` is the Linux side |

The app's `sensors`, `sys_commands`, `discovery` and `link` modules add the
session part and re-export what they use from the core, so the rest of the app
reaches both through `crate::sensors` and so on.

### What the core does per platform

The entities are the same on both platforms: same keys, names, units and
device classes. What differs is where a value comes from. Platform code sits
behind `cfg(windows)` and `cfg(target_os = "linux")`.

| | Windows | Linux |
|---|---|---|
| Folders (`config::dirs`) | `%APPDATA%\Deskmate` for the settings and the security log | The XDG base directories: `~/.config/deskmate` for the settings, `~/.local/share/deskmate` for the security log. A program without a user replaces both with `config::set_dirs`; the daemon uses `/etc/deskmate` and `/var/lib/deskmate` (`Dirs::system`) |
| Secrets (`secrets::SecretStore`) | `Keyring`: Credential Manager | `Keyring`: the Secret Service (GNOME Keyring, KWallet), built in with the `secret-service` feature. `Files`: one file per secret in a folder, mode 0600; a file that the group or others can read is refused. The program picks one with `secrets::select` |
| `cpu`, `memory`, `uptime`, `net_down`, `net_up` | sysinfo | sysinfo, without the loopback interface |
| `disk` | The system drive | The root file system |
| Per-disk sensors | By drive letter: `disk_c_free`, "Disk C free" | By mount point: `disk_root_free`, "Disk / free". A device mounted in several places is one drive; read-only images, boot partitions and media mounted for a signed-in user are left out |
| `battery`, `ac_power` | `GetSystemPowerStatus` | `/sys/class/power_supply`, without the batteries of peripherals |
| `cpu_temperature` | The PawnIO helper, then LibreHardwareMonitor or OpenHardwareMonitor over WMI, then the ACPI thermal zone | The hardware monitor of the processor (`coretemp`, `k10temp`, `zenpower`), then a thermal zone of the processor, then the ACPI thermal zone |
| `gpu_usage`, `gpu_memory`, `gpu_memory_total`, `gpu_temperature` | PDH counters, DXGI, WMI | The DRM devices in sysfs, for AMD and Intel, as far as the driver has the files. NVIDIA is not covered |
| Entities a computer may lack (`machine::Offered`) | All are declared | `battery`, `ac_power` and the Hibernate button are declared only when the computer has them. Hibernation needs `disk` in `/sys/power/state` and a "yes" from logind |
| Power commands | `rundll32` and `shutdown` | `systemctl suspend`, `hibernate`, `poweroff`, `reboot` |
| Seat | Named objects in the `Global\` namespace. A session is in use while Windows reports it active | Unix sockets in the abstract namespace. A desktop app is in use while the virtual terminal of its session is on the screen; the daemon declares itself with `seat::linux::set_role`, asks `machine_turn` like the Windows service and yields to any waiting app |
| Device model | "Deskmate for Windows" | "Deskmate for Linux" |
| Confirmation dialog (`security::confirm`) | A message box | None: the answer is no |

The Linux readers take the folder they read as an argument and are tested on
sample trees, so their tests run on any Linux machine. CI runs the core's
checks on Linux in its own job, with and without the `secret-service` feature.

## The Windows service

`deskmate-service` runs as LocalSystem and holds the Link connection while no
Deskmate app wants it. It declares only the machine scope
(`deskmate_core::machine`): the sensors that need no user and the four power
buttons. The crate is a library (`deskmate_service`) and a program
(`deskmate-service.exe`). Everything outside the `win` and `linux` modules
is platform-neutral; the same runner drives the
[Linux daemon](#the-linux-daemon).

| Module | What |
|---|---|
| `runner` | The whole behaviour, driven by events (`Event`: stop, shutdown, suspend, resume, session change, reload) and reporting a `Status`. Traits stand for what it needs from outside: `Connector` and `LinkSession`, `Seat`, `Telemetry`, `PowerExecutor`, `Clock`, `SetupSource`, `AuditSink` |
| `connector` | The real `Connector`: a WebSocket with the handshake and frame codec of `deskmate_core::link` |
| `power` | `PowerExecutor`, `SystemPower` (calls `machine::run_command`), `DryRunPower` and the `SafetyValve` |
| `config` | `ServiceConfig`: the strict settings file and the fingerprint |
| `secrets` | `KeyStore`, `CredentialVault` (keyring) and, with `test-hooks`, `KeyFile` |
| `provisioning` | `Provisioner` (settings and keys together), `AdminActions` (what an admin frame does) |
| `admin` | The frames of the admin pipe: `parse`, `Request`, `Refusal`, the answers, `provision_frame` for a client |
| `status`, `retry`, `audit`, `logging`, `cli` | The status line, the retry schedule, `security.log`, `service.log`, the Windows command line |
| `pairing` | The `DMP1.` pairing code, read as `src/pairing.ts` reads it, and the settings it leads to |
| `win` | `host` puts the parts together; `service` (service manager), `pipes`, `security` (descriptors, folder check, elevation), `install` |
| `linux` | `host` puts the parts together; `events` (signals, wake-ups), `status_file`, `keys` (key files), `pair`, `files`, `journal`, `cli` and `app` (the Linux command line) |

### What the runner does

1. Takes the seat when no app waits for it (`seat::machine_turn`).
2. Connects to `link_url`, then `link_url_remote`, with the delays the app
   uses (`retry::RetrySchedule`).
3. Sends `info` with `role: "service"` (`"daemon"` on Linux) and waits 10
   seconds for Home Assistant's `info`.
4. Without the `scoped_declare` capability, or without an answer, it sends
   no `declare`: an integration older than 0.9.0 would delete the user's
   other entities. It closes, frees the seat, shows `integration_too_old`
   and looks again after an hour.
5. Otherwise it sends the machine `declare` at once, from a reading taken
   before connecting, then `state` every `publish_interval_secs` with all
   values, and a new `declare` when the detected hardware changes.
6. It answers `ping`, sends one every 30 seconds, and acknowledges every
   `cmd`. Only `sleep`, `hibernate`, `shutdown` and `restart` with the action
   `press` run; everything else gets `ok: false`. `notify`, `fs` and `tv` are
   answered with a refusal.
7. When an app waits for the seat (checked every 5 seconds and on every
   session change) it sends `bye` with `handover`, closes, and only then
   frees the seat. It takes the seat back when it is free again.
8. Before a suspend it sends `bye` with `suspend` and reconnects at once on
   resume. On shutdown or restart it sends `bye` with `shutdown`. A plain stop
   sends no `bye`. `bye` goes only to an integration that announced it.

Power commands are refused for five minutes after boot and after every
wake-up. With Fast Startup the time since boot does not start over when the
user "shuts down", so the valve also counts from the last resume event.

### Files

| Path | What |
|---|---|
| `%ProgramW6432%\Deskmate service\deskmate-service.exe` | The installed program |
| `%ProgramData%\Deskmate\service\config.json` | Settings; never a key |
| `%ProgramData%\Deskmate\service\service.log` | Operations and results; moves to `service.log.1` at 1 MiB |
| `%ProgramData%\Deskmate\service\security.log` | `<unix time> <event> <result>`, as in the app; moves to `security.log.1` at 1 MiB |

`%ProgramData%\Deskmate` and the folder inside it are created with
`O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)`. At every start the service
checks both: a real folder, owned by SYSTEM or Administrators, where no other
account holds a right to write, delete or change access. Otherwise it stops
with exit code 1. The exit codes of a failed start: 1 the data folder cannot
be trusted, 2 the log cannot be opened, 3 a pipe cannot be created, 4 the
runtime could not start.

`config.json`, read strictly (an unknown field makes the file invalid):

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

`node_id` (`a-z`, `0-9`, `_`, 1 to 64 characters) and `link_url` are required.
The addresses follow `link::normalize_url`, the interval is 2 to 3600, an
empty `device_name` means the host name, `sensors_enabled` holds at most 256
keys in the form of a node id.

The pairing key and the cascade key are in the Credential Manager of the
account the service runs as, as `machine.Deskmate Service Link` and
`machine.Deskmate Service Link Cascade`.

### The pipes

Both refuse remote clients and are created as the first instance of their
name. A client should open them with an impersonation level of
identification or lower.

`\\.\pipe\deskmate-service-admin`, for SYSTEM and Administrators
(`D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)`). One JSON object per line, at
most 8192 bytes, each answered with one line:

```json
{"v":1,"op":"provision","node_id":"my_pc","link_url":"ws://homeassistant.local:8123","link_url_remote":"","link_cascade":false,"publish_interval_secs":15,"device_name":"My PC","sensors_enabled":{},"key":"<base64 of 32 bytes>"}
{"v":1,"op":"deprovision"}
{"v":1,"op":"reload"}
```

`provision` takes the fields of `config.json`, `key` and, exactly when
`link_cascade` is true, `cascade_key`. The answer is `{"v":1,"ok":true}` or
`{"v":1,"ok":false,"error":"<code>","detail":"<text>"}`. The codes
`too_long`, `not_json`, `version`, `unknown_operation` and `invalid` close
the connection; `failed` means the frame was accepted and could not be
carried out. Nothing received is logged. Before sending a key, check that the
pipe is served by the service's process (`win::pipes::AdminClient::connect`
does): a program that took the name first would otherwise receive it.

`\\.\pipe\deskmate-service-status`, readable by any local user
(`D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)`). The service writes
one line and closes; it never reads:

```json
{"v":1,"version":"0.9.0","state":"connected","node_id":"my_pc","detail":"Connected to Home Assistant.","config":"<64 hex digits>"}
```

`state` is `not_provisioned`, `waiting`, `connecting`, `connected`,
`integration_too_old` or `error`. `config` is empty without usable settings.
Otherwise it is HMAC-SHA256 under the pairing key, in lowercase hex, over
these parts, each preceded by its length as four big-endian bytes
(`link::enc`): `deskmate service config v1`, `node_id`, `link_url` and
`link_url_remote` in their complete form, `1` or `0` for `link_cascade`, the
interval in decimal, `device_name`, the number of `sensors_enabled` entries
in decimal, then for every entry sorted by key the key and `1` or `0`, and
last the 32 bytes of the cascade key or an empty part. The app computes the
same value from its own settings and keys to see whether the service has to
be set up again (`ServiceConfig::fingerprint`).

### The command line

| Command | What |
|---|---|
| `deskmate-service install [--provision-pipe <name>]` | Copies itself to Program Files, creates the data folder, registers `DeskmateService` (LocalSystem, delayed automatic start, restart after 5 s, 30 s and 5 min on failure, a token limited to `SeChangeNotifyPrivilege` and `SeShutdownPrivilege`) and starts it. Over an existing service it replaces the program and keeps settings and keys. With `--provision-pipe` it then opens `\\.\pipe\<name>`, checks that the pipe is served by a program named `deskmate.exe`, reads one `provision` line and passes it to the admin pipe |
| `deskmate-service uninstall` | Asks the service to forget settings and keys, stops and removes it, deletes both folders. Run from the installed copy, that copy is removed at the next restart |
| `deskmate-service status` | Prints the status line |
| `deskmate-service run` | What the service manager starts |

Exit codes: 0 done, 1 failed, 2 wrong usage, 5 `install` or `uninstall`
without an elevated terminal, 6 `install` installed the service but could not
check the program behind `--provision-pipe`, so it took no settings. A
process of another account is checked with `SeDebugPrivilege`, switched on
for that look only (`win::security::EnabledPrivilege`).

A build with the cargo feature `test-hooks` is for tests only. It has
`DryRunPower` instead of `SystemPower`, so it cannot turn a computer off, and
`run`, `console` and `install` insist on `--dry-run-power`. It adds
`console` (the service in the foreground; stdin takes `stop`, `shutdown`,
`suspend`, `resume`, `session` and `reload`), `--data-dir`, `--key-file` and
`--pipe-suffix` for `run` and `console`, `--pipe-suffix` for `status`, and
`--provision-program <file name>` for `install`.

### The seat

`deskmate_core::seat` decides which Deskmate process on a computer holds the
connection. An app calls `turn`; the service calls `machine_turn` and
`release`. The service never announces that it waits, so a disconnected
remote desktop session keeps its connection as before. When the service
creates `Global\Deskmate.Seat.<node>` it uses
`D:(A;;GA;;;SY)(A;;0x00100001;;;AU)`: SYSTEM has full access, and
Authenticated Users may open the object, wait on it and release it
(`SYNCHRONIZE | MUTEX_MODIFY_STATE`). An app up to 0.8.x asks for full
access, is refused, and treats that as it always did: another account holds
the seat, so it announces that it waits, and the service hands over.

### The app's side

The installer ships the service as a resource: `tauri build` runs
`scripts/build-service.mjs` first, which builds `deskmate-service` in release
mode for the target and copies it to `src-tauri/service-bin`, installed as
`service\deskmate-service.exe` next to `deskmate.exe`. Without a built
service the folder stays empty and the panel says so.

| Module | What |
|---|---|
| `service` | The commands `service_status`, `set_up_service` (install, update, apply) and `remove_service`, and what `--remove-service` (`uninstall.rs`, started by the uninstaller) does. `view` turns the status line into the panel's state, `provision` builds the settings and the `provision` line with the service crate's own code |
| `service::win` | The one-time provisioning pipe, the elevated launch, the service manager query and the status pipe read |
| `farewell` | Which `bye` to send when the app quits, restarts for an update, the user signs out, Windows sleeps or shuts down, and the reconnect after a wake-up |
| `power_events` | A hidden window that receives `WM_POWERBROADCAST` and `WM_ENDSESSION` |

Install and Apply run `install --provision-pipe <name>`, Update `install`,
Remove `uninstall`, each with one UAC prompt. While the user has pressed
Disconnect and the service is installed, the app keeps the seat without
connecting (`service::hold_seat_while_paused`).

### Tests

The unit tests cover the runner against a stand-in hub with paused time, and
the parsers. `tests/service_e2e.py` runs the program against
`scripts/fake_link_hub.py`: `--console` on a developer's machine, `--service`
in the `service` job of CI, which installs the service for real.

## The Linux daemon

The same crate builds `deskmate-service` for Linux, with the `linux` module
in place of `win`. systemd starts it as the system user `deskmate`
(`packaging/linux/deskmate.service`). It sends `info` with `role: "daemon"` and
`platform: "linux"` and declares the machine scope, with everything the
runner does on Windows (the `scoped_declare` gate, states, pings, the four
power commands behind the five-minute valve, `bye`, reconnecting). `notify`
and `fs` get `ok: false` with "not supported by the Deskmate daemon". The
[Linux desktop app](#the-linux-desktop-app) takes the connection over through
the seat (`seat::machine_turn`, `seat::release`), as the Windows app does with
the service.

| Path | What |
|---|---|
| `/etc/deskmate/config.json` | The settings, the schema and checks of the Windows `config.json`. Mode 0600, owned by `deskmate`, read-only under `ProtectSystem=strict` |
| `/var/lib/deskmate/machine.deskmate-service-link.key` | The pairing key, a key file of `deskmate_core::secrets` (mode 0600; looser modes are refused). `machine.deskmate-service-link-cascade.key` with cascade encryption |
| `/var/lib/deskmate/security.log` | `<unix time> <event> <result>`, as on Windows |
| `/run/deskmate/status.json` | The status line of the Windows status pipe, mode 0644, written through a temporary file and a rename on every change, removed at exit |

The log goes to stderr with a `<n>` syslog priority in front of each line,
which journald reads. At start the daemon refuses the config and state
folders when they belong to someone other than root or its own user, or when
the group or others can write to them.

| Event | Source |
|---|---|
| `Stop` | SIGINT, or SIGTERM while `systemctl is-system-running` does not answer `stopping` |
| `Shutdown` | SIGTERM while it does |
| `Reload` | SIGHUP (`systemctl reload deskmate`) |
| `Suspend` | SIGUSR1 from `deskmate-sleep.service` (`Before=sleep.target`) |
| `Resume` | SIGUSR2 from the same unit when `sleep.target` stops, and the clock check: every 5 s the gap between `/proc/uptime` (with sleep) and `Instant` (without) is compared, and a growth of more than 3 s is a wake-up |

logind's `PrepareForSleep` is not used: hearing it takes a D-Bus client, and
`busctl monitor` needs root on the system bus.

| Command | What |
|---|---|
| `deskmate-service run` | What systemd starts |
| `deskmate-service pair` | Reads a `DMP1.` code from stdin (never from an argument), checks the key, writes the key files and then `config.json`, gives them to `deskmate` and asks systemd to reload the daemon. Needs root |
| `deskmate-service unpair` | Removes `config.json` and both key files. Needs root |
| `deskmate-service status` | Prints the status file |

Exit codes: 0 done, 1 failed, 2 wrong usage, 5 `pair` or `unpair` without
root. A build with `test-hooks` adds `console` (the daemon in the
foreground; stdin takes `stop`, `shutdown`, `suspend`, `resume` and
`reload`, and SIGTERM is always a stop), `--config-dir`, `--data-dir`,
`--status-file` and `--dry-run-power`; with its own folders `pair` and
`unpair` need no root and change no owner.

`packaging/linux/` holds the units, the polkit rule (`50-deskmate.rules`),
the sysusers and tmpfiles snippets, `install.sh` for the tarball, the Debian
maintainer scripts and `build.sh`, which packs a release binary into
`deskmate-daemon_<version>_<arch>.deb` and
`deskmate-daemon-<version>-<arch>-linux.tar.gz`. The `daemon` job of CI runs
the crate's lints and tests on Linux with and without `test-hooks`,
`tests/daemon_e2e.py` against the fake hub, and `systemd-analyze verify` and
`security` on the units; `daemon-package` builds both packages on Ubuntu
22.04, x86_64 and aarch64, and keeps them as workflow artifacts.

## The Linux desktop app

The `deskmate` crate builds for Linux as a preview. The Windows-only modules
(`family`, `streamdeck`, `power_events`, `uninstall`, `media_activity`, and
the CPU temperature helper of the core) are left out with `cfg(windows)`;
modules that keep Windows stubs say so in an `allow(dead_code)` with a
reason. The stubs that work on Linux call the `linux` module:

| Module | What |
|---|---|
| `linux::bus` | D-Bus connections (session and system bus) with a call timeout, and the name checks |
| `linux::session` | logind's `auto` session: `LockedHint`, `IdleHint` and `IdleSinceHint`, `Lock` |
| `linux::notify` | `org.freedesktop.Notifications` on one thread that also hears `ActionInvoked`; buttons carry random single-use keys; images in `$XDG_RUNTIME_DIR/deskmate` |
| `linux::media` | MPRIS: the playing, else paused, else first player; metadata and play/pause/next/previous |
| `linux::volume` | `wpctl`, else `pactl`, with fixed arguments and the C locale |
| `linux::open` | `xdg-open` for a checked web address |
| `linux::files` | The Link Files path rules (see SECURITY.md), the default inbox from `xdg-user-dirs`, placing a received file without replacing one |
| `linux::sensors` | The session sensors added to the machine sensors |
| `linux::capabilities` | What the desktop offers, looked up at start: session type, tray host (`org.kde.StatusNotifierWatcher`), notification service and its buttons, logind, the tools; the Secret Service on every request |
| `linux::entities` | Which entities are declared and which commands are accepted; filters the Link `declare` and the MQTT discovery |
| `platform` | `platform_capabilities` for the window (`PlatformCapabilities` in `src/types.ts`) and the platform of the `info` frame |

The app's secrets go through `deskmate_core::secrets` with the core feature
`secret-service`. The window reads the capabilities once (`src/platform.tsx`)
and shows one line in place of a missing feature. Without a tray host the
main window does not hide. The seat role is `App` (the default);
`linux::seat_tests` starts the test binary a second time as the daemon and
checks the hand-over both ways.

`tauri.linux.conf.json` sets the Linux bundles (`.deb`, AppImage) and leaves
the Windows resources out; `build.rs` asks for the Stream Deck plugin only for
a Windows target. The `desktop-linux` job of CI runs clippy and the tests on
Ubuntu 22.04 for x86_64 and aarch64, builds both bundles and keeps them as
workflow artifacts.

## Dependencies

Rust: tauri 2, tauri-plugin-autostart, tauri-plugin-single-instance,
tauri-plugin-global-shortcut, rumqttc, sysinfo, keyring,
tauri-winrt-notification, ureq(+native-tls), windows (Media_Control, Win32_*),
tokio, serde; on Linux dbus (libdbus, already linked by Tauri's runtime).
Frontend: React 19, Tailwind 4, @tauri-apps/api.

## Adding a sensor (checklist)

1. Add a `SensorDef` to `SENSOR_DEFS` in
   `src-tauri/crates/core/src/sensors.rs` (set `privacy: true` +
   `default_enabled: false` if it exposes user activity).
2. Add the collection branch. A sensor that reads the user's session goes
   into `Collector::collect` in `src-tauri/src/sensors.rs`. One that works
   without a signed-in user goes into `MachineCollector` in the core, and its
   key into `machine::SENSOR_KEYS`.
3. Done — discovery, UI list, toggles and publishing pick it up automatically.

## Adding a built-in command

1. Add a `CommandDef` to `COMMAND_DEFS` in
   `src-tauri/crates/core/src/sys_commands.rs`.
2. Add the match arm in `run_builtin` in `src-tauri/src/sys_commands.rs`.
