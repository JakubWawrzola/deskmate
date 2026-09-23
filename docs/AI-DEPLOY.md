# Instructions for an AI assistant setting up Deskmate

You are reading this because someone pointed you at the Deskmate repository and
asked you to get it working with their Home Assistant. This file tells you what
to do and what to tell them. Follow it in order.

Deskmate is a Windows desktop application that exposes a computer to Home
Assistant: sensors, remote commands, notifications, media keys, global hotkeys.
It connects over **Deskmate Link**, a custom Home Assistant integration with
application-layer encryption. MQTT is still supported but is no longer the
recommended path.

## Before you say anything

Establish three things. Do not guess them.

1. **Which computer runs Deskmate.** It must be Windows 10 or 11, x64 or ARM64.
   Deskmate runs in the user's desktop session, not as a service.
2. **How that computer reaches Home Assistant.** A hostname or IP and a port,
   reachable from the computer. Anything that already serves the Home Assistant
   interface works, including a reverse proxy, Nabu Casa or a VPN address.
3. **Whether they already run Deskmate over MQTT.** If they do, this is a
   migration, not a fresh install - jump to `docs/MIGRATION.md` and come back.

If Home Assistant is older than 2024.11, stop and say so. The integration uses
the reconfigure flow introduced in that release.

## What the user does in Home Assistant

There are two halves, and the pairing key is what joins them. Give the user one
half at a time and wait - the key is displayed once and is easy to lose.

### Half one: install the integration

Tell them:

> Open HACS in Home Assistant, use the three-dot menu, choose *Custom
> repositories*, paste `https://github.com/JakubWawrzola/deskmate`, pick
> *Integration* as the category and add it. Then find **Deskmate Link** in HACS,
> download it, and restart Home Assistant.

The README also carries a one-click "Add to HACS" button. If the user prefers
it, that button opens the same dialog pre-filled.

Without HACS, they copy `custom_components/deskmate_link` from the repository
into their `config/custom_components/` directory and restart Home Assistant.
Restarting is not optional: a new integration is only picked up at startup.

### Half two: pair

Tell them:

> Go to *Settings → Devices & services → Add integration*, search for **Deskmate
> Link** and confirm. Home Assistant shows a pairing code starting with
> `DMP1.`. Copy it now.

If the user closed the dialog too early, they add the integration again: an
entry still waiting for pairing shows the same code, nothing is duplicated.
When upgrading an existing setup, the integration must be updated before the
Windows app, because Deskmate 0.6 speaks Link protocol v2 only.

There is no device name to type. The entry is created unbound and attaches
itself to the first computer that authenticates with that key. Do not invent a
node name for the user and do not ask for one.

## What the user does on the computer

Tell them:

> Download the installer for your architecture from the repository's Releases
> page - `x64` for a normal PC, `arm64` for a Snapdragon or similar machine.
> Run it. The installer is not signed, so SmartScreen will warn you; choose
> *More info → Run anyway* if you are comfortable with that.
>
> On first launch, paste the pairing code. The address is filled in from it;
> check that it is right.

The address format matters and is the most common thing to get wrong:

| Situation | What to enter |
|---|---|
| Same network, plain HTTP | `ws://homeassistant.local:8123` |
| Same network, by IP | `ws://192.168.1.50:8123` |
| Through a reverse proxy or Nabu Casa | `wss://their-domain.example` |
| Over a VPN such as Tailscale | `ws://100.x.x.x:8123` |

Rules to state plainly: `http` becomes `ws`, `https` becomes `wss`, the port is
the same one the Home Assistant interface uses, and the path is added
automatically - they only enter the host. A second, fallback address can be
filled in for use away from home; Deskmate alternates between them.

## Confirm it worked

Ask the user to check the Status page in Deskmate. It should read
`Connected (Link)`. In Home Assistant, the device appears under *Settings →
Devices & services → Deskmate Link*, and the entry's title changes from
"Deskmate (waiting for pairing)" to the computer's node name.

Entity ids follow the **device name** shown in Deskmate's settings, not the node
id - a device called `Workshop PC` produces `sensor.workshop_pc_cpu_usage`. Tell
the user to set the device name before the first successful connection if they
care about the naming, because Home Assistant does not rename existing entities
afterwards.

## When it does not connect

Read the exact status text in Deskmate first. It distinguishes the cases.

**`Link rejected ... (node "name")`** - Home Assistant refused the handshake. No
paired entry accepted this computer, or the pairing key does not match. Home
Assistant also raises a repair issue naming the computer. Either pair again, or
if an entry already exists that should own this computer, open it and choose
*Reconfigure → Unbind from the current computer*.

**`Link clock mismatch ...`** - the computer's clock and Home Assistant's
differ by more than 90 seconds. Have the user sync the Windows time. A dual-boot
PC whose hardware clock is kept in UTC by Linux is the usual cause.

**`... cascade encryption is on at one end only`** or **`... protocol version`**
- the key is right, the settings differ. Update the integration to 0.6.0, and
enable or disable cascade on both sides.

**`Link locked out ...`** - too many failed handshakes in a row. It clears
itself within five minutes once the underlying problem is fixed.

**`Link error ... connection refused` or a timeout** - the address is wrong or
unreachable. Have them open the same host and port in a browser from that
computer. If the interface does not load there, Deskmate cannot reach it either.

**Connected, but no entities** - the client connected but has not declared yet.
Wait one publish interval, then reload the page.

**Cascade mismatch** - if the user enabled cascade encryption on one side only,
Home Assistant rejects the handshake on purpose rather than falling back to the
weaker single layer. Both ends must have it on, with the same key.

## Things not to do

- Do not tell the user to install Mosquitto or any MQTT broker. Link needs none.
- Do not put the pairing key in YAML, `configuration.yaml`, a package file or
  anything under version control. It belongs in the Home Assistant config entry
  and in Windows Credential Manager, both of which are handled for them.
- Do not create a long-lived access token for the connection. Link does not use
  one. A token is only needed for the optional Home Assistant API features
  (widgets, hotkey actions), and that is a separate setting.
- Do not enable Link Files without saying what it does. It grants Home Assistant
  read access to the folders listed in the allowlist. The list is empty by
  default and should stay that way unless the user asks for it.
- Do not suggest editing `.storage` by hand.

## Optional extras, only if asked

**Cascade encryption** adds a second cipher on top of the first. Enable it in
Home Assistant under *Reconfigure → Enable cascade encryption*, copy the second
key into Deskmate under *Geeky stuff*, and turn the toggle on there. Both sides
must match or the connection stays down.

**Notifications** are sent with the `deskmate_link.notify` service. It takes
`title`, `message`, an optional `image` and optional `actions`. A clicked action
button fires the `deskmate_link_notify_action` event with the action id.

**Hotkeys** declared in Deskmate arrive as `event` entities and as the
`deskmate_link_trigger` event, so they can start automations without a token.
