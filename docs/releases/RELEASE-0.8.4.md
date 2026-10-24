# Deskmate 0.8.4 - security fix in the integration

Found in the review for the default HACS list, thanks to @frenck.

The Home Assistant integration's Link endpoint can be reached without a login,
because the handshake is what authenticates. A protocol v1 hello with a wrong
MAC and a freely chosen computer name was rejected, but after three attempts
from one address the integration created an *unpaired computer* issue in
Repairs with that name in its text. Repairs renders markdown, so a crafted name
could show a link of the sender's choosing to an administrator.

The computer name is now checked against the form Deskmate itself creates
(`a-z`, `0-9`, `_`, up to 64 characters) for every protocol version, before a
failure is counted, logged or turned into an issue. Anything else becomes `?`
and never reaches the log, the issue or its id. Paired computers are not
affected. There is no report of it being used.

## Update

1. **Home Assistant:** update Deskmate in HACS to 0.8.4 and restart Home
   Assistant. This is the part that matters.
2. **PC:** install `Deskmate_0.8.4_x64-setup.exe` (or `_arm64`) over the old
   version. The app has no functional changes; this only keeps the versions
   in step.

## Assets

| File | SHA-256 |
|---|---|
| `Deskmate_0.8.4_x64-setup.exe` | `9541ced6bbaccdbf37cd1e9741a8b4a59b3a81793d2594cbd82b5cc7e7d93282` |
| `Deskmate_0.8.4_arm64-setup.exe` | `c5c19bf0f00f019754ec93c0e32f1eb0eb0ecd36e38045bc722a6481b0625d7c` |

Both installers are unsigned, so SmartScreen reports an unknown publisher.
Windows 10 and 11, x64 and ARM64. Home Assistant 2024.11 or newer.
