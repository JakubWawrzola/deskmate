# Security policy

## Supported versions

Fixes go into the latest release of the Windows app and of the Home Assistant
integration. Please update before reporting.

| Version | Supported |
|---|---|
| 0.8.x | Yes |
| Older | No |

## Reporting a vulnerability

Report it privately: the **Security** tab of this repository → *Report a
vulnerability*. Please do not open a public issue or discussion for it.

Helpful to include:

- what an attacker can do, and from where (the local network, Home Assistant,
  another program on the PC);
- the steps to reproduce, and the versions involved;
- whether it needs a setting that is off by default.

Never send real pairing codes, tokens or passwords; make a test pairing.

You get an answer within a week. Confirmed problems are fixed in a release,
and the reporter is credited in the release notes unless they prefer not to
be.

## How Deskmate is secured

The threat model, the defaults and the known limits are in
[docs/SECURITY.md](../docs/SECURITY.md).
