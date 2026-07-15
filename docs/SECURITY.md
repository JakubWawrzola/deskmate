# Security

Last reviewed: 2026-07-15. Scope: Deskmate 0.3.1, the Home
Assistant ↔ PC MQTT channel, the optional PC → Home Assistant REST channel,
and the Windows capabilities reached through those channels.

Deskmate deliberately turns Home Assistant into a remote control and telemetry
receiver for a logged-in Windows session. MQTT access to a Deskmate command
topic is therefore a security boundary, not merely a connectivity detail.

## Threat model

| Actor | Expected access | Relevant risk |
|---|---|---|
| Compromised IoT device, guest device or hostile router on the LAN | Can observe or alter local traffic | Steal plaintext MQTT credentials/data, inject commands, replay events or cause denial of service. |
| Authenticated MQTT client with broad topic access | Can publish/subscribe through the broker | Control other Deskmate nodes or read their privacy-sensitive states. |
| Compromised HA administrator, add-on or automation | Already trusted by the homeowner | Exercise every Deskmate capability that is enabled locally. |
| Network attacker between Deskmate and HA REST | Can observe/alter HTTP | Steal the HA bearer token and modify service calls. |
| Local process running as the Windows user | Acts inside the same user session | Modify non-secret config, invoke the public `deskmate:` URI handler, or use that user's desktop capabilities. |

Malware running as the Windows user or administrator is outside Deskmate's
security boundary: it can already read the clipboard and execute programs.
Deskmate still protects against lower-cost network and broker attacks so those
actors do not gain equivalent access remotely.

## Security defaults

- MQTT transport defaults to TLS on port 8883. Server certificates are verified
  using the Windows trust store or a user-selected PEM CA certificate.
- Plain MQTT is an explicit `insecure` transport choice. It is allowed only
  without a fallback broker address and is labelled trusted-LAN-only.
- MQTT and HA REST secrets are stored in Windows Credential Manager, not
  `config.json` or application logs.
- Clipboard publication and clipboard writes are separate capabilities, both
  defaulting to `Off`.
- New and migrated custom PowerShell commands are disabled and require desktop
  confirmation by default.
- `open_url` and notification images are denied unless their exact origin is
  allowlisted or matches a configured HA API origin.
- Retained command/notification messages are ignored.
- A remote HA fallback URL must use HTTPS. A local HA URL may use HTTP only for
  compatibility with a trusted LAN.

### Upgrade behaviour

An older config without security-policy fields migrates to TLS/8883, both
clipboard modes `Off`, an empty additional URL allowlist, and disabled custom
commands with confirmation required. This intentionally stops legacy insecure
automation until the user reviews it. Configure broker TLS before upgrading or
explicitly select Plain MQTT for a trusted LAN. Existing MQTT/HA credentials
remain in Credential Manager.

## MQTT transport and authentication

### TLS mode

TLS mode uses Windows Schannel through `native-tls`, so it works on Windows x64
and ARM64 without OpenSSL, ring or Clang. Certificate verification cannot be
disabled in TLS mode.

Use the broker hostname present in the certificate's Subject Alternative Name.
Connecting to a raw IP address fails correctly unless that IP is included in
the certificate. For a private CA, enter the CA certificate's local PEM path in
Deskmate; do not select the broker leaf certificate unless it is intentionally
self-signed as its own trust anchor.

The official Home Assistant Mosquitto app enables port 8883 when `certfile` and
`keyfile` are configured from `/ssl`:

```yaml
certfile: fullchain.pem
keyfile: privkey.pem
require_certificate: false
```

`require_certificate: false` still requires the normal MQTT username/password;
it only means a client certificate is not mandatory. After TLS clients are
working, remove/blank ports 1883 and 1884 on the app's Network card to stop the
broker listening for insecure connections.

Source: [Home Assistant Mosquitto app documentation](https://github.com/home-assistant/addons/blob/master/mosquitto/DOCS.md).

### Plain MQTT compatibility mode

Plain mode exists for migration and isolated networks. It does not protect the
MQTT username, password, sensor values or commands in transit. A password on
port 1883 authenticates to the broker but does not encrypt the connection. Do
not expose it to the Internet, forward it through a router, or configure a
remote fallback.

### Per-node ACL

Use a distinct MQTT user for each Deskmate installation. For node
`laptop_jakub`, the least-privilege ACL for that Deskmate client is:

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

The HA Mosquitto app requires its internal users to retain broad access when a
custom ACL file is enabled:

```text
user addons
topic readwrite #

user homeassistant
topic readwrite #
```

Enable `customize.active`, point Mosquitto to an `acl_file`, and append one
least-privilege block per Deskmate user. ACLs are enforced by the broker; a
desktop client cannot enforce what other broker users may publish.

## Clipboard protection

Clipboard read and write no longer share one switch.

### Publish clipboard to HA

| Mode | Behaviour |
|---|---|
| `Off` | No clipboard read entity is advertised and no clipboard text is published. |
| `Confirm` | When the clipboard changes, Windows asks whether this value may be published. Approval remains valid only until the clipboard changes. The approved value is republished periodically so the HA entity does not expire. |
| `Automatic` | The current value is published on every sensor interval without prompts. |

### Allow clipboard writes from HA

| Mode | Behaviour |
|---|---|
| `Off` | The write entity is removed and incoming writes are ignored. |
| `Confirm` | Every accepted MQTT write displays a local preview and requires approval. |
| `Automatic` | HA can replace the clipboard without a prompt. |

Both directions are blocked while the Windows session is locked. Writes are
limited to 64 KiB and one write per two seconds. Clipboard contents are never
written to Deskmate's logs. Be aware that values published to HA may remain in
Home Assistant Recorder/history after Deskmate stops publishing them.

Confirmation mode for periodic reads prompts only when the value changes. A
denied value is not requested again until the clipboard changes, avoiding a
prompt every sensor interval.

## URLs and notification images

All network-controlled URLs must:

- be absolute `http://` or `https://` URLs with a host;
- contain no whitespace, control characters or embedded credentials;
- be at most 2,048 characters;
- match an exact allowed origin (`scheme + host + effective port`).

Configured local/fallback HA API origins are allowed automatically. Additional
origins are entered in Settings, for example:

```text
https://example.com
http://homeassistant.local:8123
```

Paths are allowed in actual commands, but allowlist entries themselves must be
origins without a path, query or fragment. An allowed origin does not allow a
different port or a different scheme. Blocked notification images are omitted;
the text toast can still display. Downloads also have a 10-second timeout and
5 MiB body limit. Redirects are not followed because a redirect target could
otherwise escape the approved origin.

This closes the unrestricted SSRF/probing path while preserving explicitly
approved local HA image URLs and external sites.

## Custom PowerShell commands

Custom commands remain intentionally powerful, but MQTT payloads are not
evaluated as PowerShell:

- MQTT selects a configured command only by sanitized ID;
- the control value is filtered, limited to 64 characters and passed only as
  `$env:DESKMATE_VALUE`;
- new and migrated commands are disabled until enabled in the Commands page;
- each command has an independent `Require confirmation` switch, on by default;
- a required confirmation cannot be bypassed while Windows is locked;
- disabled commands are removed from MQTT discovery and rejected at execution.

Do not embed passwords/tokens in custom command text because the script itself
is intentionally stored in `%APPDATA%\Deskmate\config.json`. Prefer a program
that reads its own secret from Credential Manager or another OS-protected store.

## Input limits, replay and audit log

- Retained MQTT messages on `cmd/+` and `notify` are rejected, preventing stale
  shutdown, clipboard or notification events after reconnect.
- TTS has a bounded 16-message queue and 1,000-character message cap.
- Toast title/body/actions are bounded; at most 10 notifications per minute are
  rendered.
- Clipboard writes have the size and cooldown limits described above.
- REST validates HA base URLs, `domain.service` and state entity IDs. Error
  bodies are not copied into UI/log messages.

Security decisions are recorded in
`%APPDATA%\Deskmate\security.log`. The log contains only Unix timestamp, event
type and result such as `approved`, `denied`, `blocked_locked` or `completed`.
It never contains clipboard data, URL values, MQTT payloads, command scripts or
credentials. It rotates at 1 MiB to `security.log.1`.

## REST channel

The optional HA token is stored under the separate `Deskmate HA Token`
Credential Manager entry. The local URL may be HTTP for an explicitly trusted
LAN. The fallback URL is HTTPS-only. Use a dedicated, non-administrator HA user
where its permissions are sufficient, and rotate the token after any suspected
network or Windows-account exposure.

Failover occurs only on transport failures. An HTTP 4xx/5xx response does not
send the same bearer token to a second host.

## Findings addressed from the public review

| Review concern | Mitigation |
|---|---|
| MQTT can be sniffed or commands injected from the LAN | TLS is the default; plaintext is explicit and cannot use fallback; dedicated credentials and exact per-node ACL are documented. |
| `open_url` can probe arbitrary local targets | Strict parser plus exact-origin allowlist shared with notification images. |
| HA can read passwords/tokens from clipboard or replace pasted text | Read/write split into `Off`, `Confirm`, `Automatic`; both default off, stop while locked, and writes have preview/cooldown/size limits. |
| HA can run arbitrary PowerShell configured by the user | Payload-to-code injection remains blocked; commands are independently disabled/enabled and can require local confirmation for every run. |

## Residual risks and non-goals

- A compromised HA instance remains trusted for every capability the user set
  to `Automatic` and for built-in command entities.
- TLS protects traffic, not a compromised broker account with an overly broad
  ACL. TLS and ACLs are both required.
- Plain MQTT remains unsafe by design even though the UI warns about it.
- An approved clipboard value exists in plaintext inside HA and may be retained
  by Recorder/backups. End-to-end application encryption would require an HA
  component that decrypts before HA can use the state.
- The public `deskmate:` URI scheme used by toast actions can be invoked by
  another local process. HA automations consuming `notify/action` must validate
  exact expected action values before consequential services.
- Release installers are currently not Authenticode-signed. Windows SmartScreen
  may warn on first run; verify the SHA-256 hashes published with each release.
  Code signing is planned but requires a trusted publisher certificate.
- Deskmate cannot protect against malware already running as the Windows user.

## Why there is no shared-token hash yet

A plain hash does not hide clipboard text. An HMAC with a shared secret can
authenticate commands, but it still needs timestamp/nonce replay protection and
HA-side code capable of producing the envelope. AES-GCM or ChaCha20-Poly1305
could additionally encrypt clipboard text, but HA could not display/use that
state without a Deskmate integration or AppDaemon helper.

This remains a possible hardened mode for a future HA-side component. It is not
presented as a substitute for TLS and broker ACLs, which protect all current
MQTT discovery clients without inventing a custom cryptographic protocol.

## Deployment checklist

1. Configure a valid broker certificate and use TLS/8883 in Deskmate.
2. Disable the broker's insecure 1883/1884 listeners after migration.
3. Use one MQTT identity and one exact ACL block per Deskmate node.
4. Keep both clipboard modes `Off` unless required; prefer `Confirm` over
   `Automatic`.
5. Keep custom commands disabled until reviewed and retain confirmation for
   destructive/high-impact scripts.
6. Allow only the URL origins actually needed by automations.
7. Use HTTPS for remote HA and rotate credentials after suspected exposure.
8. Review `security.log` metadata and HA automations consuming toast/hotkey
   action topics after unexpected activity.
