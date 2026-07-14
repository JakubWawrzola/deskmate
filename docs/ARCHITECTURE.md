# Architecture

## Overview

```
┌────────────────────────────── Windows PC ──────────────────────────────┐
│  Deskmate (Tauri 2)                                                    │
│  ┌───────────────┐   tauri commands/events   ┌───────────────────────┐ │
│  │ React UI      │ <───────────────────────> │ Rust core             │ │
│  │ (5 pages +    │                           │  config / state       │ │
│  │  wizard)      │                           │  sensors (sysinfo +   │ │
│  └───────────────┘                           │   WinAPI + SMTC)      │ │
│                                              │  sys_commands         │ │
│                                              │  notify (WinRT toast) │ │
│                                              │  mqtt (rumqttc)       │ │
│                                              └──────────┬────────────┘ │
└─────────────────────────────────────────────────────────┼──────────────┘
                                                          │ MQTT 3.1.1
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
- **Inbound commands**: subscription `deskmate/<node>/cmd/+`. Key is matched
  against a fixed allowlist (`sys_commands::run_builtin`) or, with the
  `custom_` prefix, against commands stored in config. **Payload content is
  never executed** — the only parsed payload is the numeric volume.
- **Notifications**: subscription `deskmate/<node>/notify`, JSON payload,
  optional image downloaded (5 MB cap) to temp and attached to a WinRT toast.
- **Availability**: LWT retained `offline` on `deskmate/<node>/availability`,
  explicit `online` after connect. Sensors also carry `expire_after` = 4x
  publish interval, so values go `unavailable` if the app dies silently.

## Key decisions

| Decision | Why |
|---|---|
| MQTT + discovery instead of a custom HA integration | works with vanilla HA, zero server-side install; same approach made HASS.Agent popular |
| rumqttc without TLS feature, ureq with native-tls (schannel) | `ring` needs clang on Windows ARM64; schannel is pure-Rust bindings to the OS. Broker TLS is on the roadmap (rustls with aws-lc or ring once toolchain allows) |
| Password in Windows Credential Manager (keyring) | config.json can be safely backed up / shared |
| SMTC for media | one API covers Spotify, browsers, UWP apps; also gives us control (play/pause/next/prev) without simulating key presses |
| Session lock detection via `OpenInputDesktop` | no message pump needed; polling fits the sensor loop |
| Toast via `tauri-winrt-notification` | dev builds use the PowerShell AUMID (works unregistered), release uses the NSIS-registered app id |
| One retained discovery snapshot per connect | idempotent, survives HA restarts, no per-entity bookkeeping |

## Crates / deps

Rust: tauri 2, tauri-plugin-autostart, rumqttc, sysinfo, keyring,
tauri-winrt-notification, ureq(+native-tls), windows (Media_Control, Win32_*),
tokio, serde. Frontend: React 18, Tailwind 4, @tauri-apps/api.

## Adding a sensor (checklist)

1. Add a `SensorDef` to `SENSOR_DEFS` in `src-tauri/src/sensors.rs`
   (set `privacy: true` + `default_enabled: false` if it exposes user activity).
2. Add the collection branch in `Collector::collect`.
3. Done — discovery, UI list, toggles and publishing pick it up automatically.

## Adding a built-in command

1. Add a `CommandDef` to `COMMAND_DEFS` in `src-tauri/src/sys_commands.rs`.
2. Add the match arm in `run_builtin`.
