# Stream Deck plugin

**Deskmate for Home Assistant** puts your home, and the actions you defined in
Deskmate, on Elgato Stream Deck keys. It needs Windows 10 or 11 and Stream
Deck 7.1 or newer.

- [What the keys do](#what-the-keys-do)
- [Two ways to connect](#two-ways-to-connect)
- [Install](#install)
- [Set up a key](#set-up-a-key)
- [Troubleshooting](#troubleshooting)
- [Local channel](#local-channel): the protocol between the plugin and the
  Deskmate app, for developers

## What the keys do

The plugin adds a **Deskmate** category to Stream Deck with four actions.

| Action | A press | The key shows |
|---|---|---|
| **Toggle Entity** | toggles a light, a switch or another on/off entity | whether it is on or off, live |
| **Call Service** | calls any Home Assistant service, with an optional entity and data | a check mark when the call went through |
| **Activate Scene** | turns a scene on | a check mark when the call went through |
| **Deskmate Action** | runs a hotkey or a tray quick action you defined in Deskmate | a check mark when it ran |

A key shows Stream Deck's yellow warning triangle when a press could not be
carried out.

## Two ways to connect

The plugin can reach Home Assistant in two ways. You can set up one of them or
both.

**Through the Deskmate app.** The plugin talks to Deskmate on the same
computer, and Deskmate talks to Home Assistant with the address and token it
already has. There is nothing to type into the plugin. Deskmate has to be
running, with **Stream Deck** turned on in its Settings. This is the only way
to use the **Deskmate Action** key.

**Direct.** The plugin talks to Home Assistant itself. You give it the address
of your Home Assistant and a long-lived access token. It works whether
Deskmate is running or not.

Every Home Assistant key works the same over both. Which one is used is the
**Connection** setting, at the bottom of each key's settings:

| Connection | What the plugin does | Choose it when |
|---|---|---|
| **Automatic** (default) | Uses the Deskmate app whenever it is reachable. Otherwise uses the direct connection, if an address and token are filled in. | You use Deskmate and want the keys to keep working when it is closed, or you do not want to think about it. |
| **Deskmate app** | Uses only the app. Keys show the warning triangle while it is not reachable. | You do not want a Home Assistant token stored in Stream Deck. |
| **Direct** | Uses only the address and token. | The computer has a Stream Deck but no Deskmate, or you want the keys independent of the app. |

In Automatic, a Deskmate app that has no Home Assistant address and token of
its own is passed over when a direct connection is filled in.

The Connection setting, the address and the token belong to the plugin, not to
one key: set them on any key and every key uses them. Stream Deck stores them
on this computer. The line under the setting says which connection is in use
right now, and what to do when neither is available.

## Install

### From Deskmate

The plugin comes with the Deskmate installer. Stream Deck has to be installed
and to have been started once, so that its folder exists.

1. Open Deskmate and go to **Settings**, **Stream Deck**.
2. Click **Install plugin**. Deskmate copies the plugin to
   `%AppData%\Elgato\StreamDeck\Plugins\com.deskmate.streamdeck.sdPlugin`.
3. Restart Stream Deck: right-click its icon next to the clock, **Quit Stream
   Deck**, then start it again. It loads plugins only when it starts. The
   action list now has a **Deskmate** category.
4. Turn on **Let Stream Deck use Deskmate** in the same panel, unless you want
   to use only the direct connection. The line below the switch says whether
   the plugin is connected.

The panel shows the version of the plugin that Deskmate carries and the one
that is installed.

**Update plugin** appears after a Deskmate update that brought a newer plugin.
Quit Stream Deck first, click it, and start Stream Deck again: while Stream
Deck runs, it keeps the plugin's files in use, and Deskmate tells you so
instead of replacing them. Your keys and their settings are kept, and so is
the plugin's log.

**Remove plugin** deletes the plugin's folder, also with Stream Deck closed.
The keys stay in your Stream Deck profiles and work again once the plugin is
installed. Uninstalling Deskmate does not remove the plugin.

### By hand

You need [Node.js](https://nodejs.org) 24 and the repository.

```powershell
cd streamdeck-plugin
npm ci
npm run build
```

1. Quit Stream Deck (right-click its tray icon, **Quit Stream Deck**).
2. Copy the whole folder `streamdeck-plugin\com.deskmate.streamdeck.sdPlugin`
   to `%AppData%\Elgato\StreamDeck\Plugins\`.
3. Start Stream Deck again.

## Set up a key

Drag an action from the **Deskmate** category onto a key and select the key.
Its settings appear below the key layout.

### Toggle Entity

- **Entity ID**: the entity to toggle, for example `light.desk`.
- **Pick**: a list of your on/off entities, read from Home Assistant through
  the connection in use. Picking one fills in the Entity ID. The list is there
  when a connection is; the field can always be typed into by hand, and any
  entity that `homeassistant.toggle` accepts works.

The key is bright while the entity is `on` and dimmed otherwise. It follows
changes made anywhere else, such as a wall switch or the Home Assistant app.

### Call Service

- **Service**: `domain.service`, for example `light.turn_on` or
  `script.turn_on`.
- **Entity ID**: optional, the entity the service acts on.
- **Pick**: the entities of the service's own domain. For a service of the
  `homeassistant` domain, or before a service is typed in, every entity is
  listed.
- **Data (JSON)**: optional, a JSON object with the service's parameters, for
  example `{"brightness_pct": 40}`.

A press with a service that is not written as `domain.service`, an Entity ID
that is not an entity id, or data that is not a JSON object shows the warning
triangle, and nothing is sent.

### Activate Scene

- **Scene**: the scene, for example `scene.movie`. The part after the dot,
  `movie`, is enough.
- **Pick**: your scenes.

### Deskmate Action

- **Action**: one of the hotkeys and tray quick actions you defined in
  Deskmate. The list comes from the app, so Deskmate has to be running with
  Stream Deck turned on while you choose. The round arrow reads the list
  again after you add an action in Deskmate.

The key keeps its action when Deskmate is closed; the settings then show the
action's name greyed out. If you delete the action in Deskmate, a press shows
the warning triangle.

### The direct connection

Fill these in on any key, under **Connection**:

- **HA URL**: the address of your Home Assistant as you open it in a browser,
  for example `http://homeassistant.local:8123`. It has to start with
  `http://` or `https://`.
- **Access token**: in Home Assistant, open your profile (bottom left), the
  **Security** tab, **Long-lived access tokens**, **Create token**. Home
  Assistant shows the token once: copy it straight into the field.

The two fields are hidden while Connection is set to **Deskmate app**.

## Troubleshooting

**A key shows the yellow warning triangle.** Select the key and read the
status line in its settings.

| The status says | What to do |
|---|---|
| Not connected. | Open Deskmate, go to Settings and turn on Stream Deck, or fill in the HA URL and the access token. |
| Not connected to the Deskmate app. | Open Deskmate and turn on Stream Deck in its Settings. With Connection on Automatic you can fill in the HA URL and token instead. |
| Deskmate has no Home Assistant address and token yet. | Add them in Deskmate, or fill them in here and set Connection to Automatic or Direct. |
| Home Assistant refused the access token. | Create a new long-lived token in Home Assistant and paste it in. Until then the keys do not follow state changes. |
| This plugin and the Deskmate app use different versions of their connection. | Update Deskmate and the plugin. |
| Connected through the Deskmate app. / Using a direct connection to Home Assistant. | The connection is fine, so look at the key: the entity, service or scene may be misspelled or gone from Home Assistant, the data may not be a JSON object, or the Deskmate action may have been deleted. |

**The settings of a key are empty.** The plugin's folder is incomplete.
Install the plugin again from Deskmate (Settings, **Stream Deck**) and restart
Stream Deck. Keys that are already set up keep working. The settings panel
needs no internet access: its form components ship with the plugin.

**The Pick list does not appear.** It is shown only when the connection in use
returned at least one matching entity. Check the status line, then click the
round arrow next to the list once the connection is there.

**A Toggle Entity key stays dimmed.** The key is dimmed until the plugin has
learned the entity's state, and for every state other than `on`. An entity
that Home Assistant does not have is reported as `unavailable`. Through the
Deskmate app, at most 64 different entities are followed at a time; keys past
that still toggle but do not show their state.

**After starting Deskmate the keys take a moment.** While the app is not
reachable, the plugin looks for it again after 1 second, then 2, 4, 8 and 16,
and from then on every 30 seconds. A press looks for it at once.

**Deskmate says that another program has taken the channel's name.** The
plugin and Deskmate meet under a name that belongs to your Windows account.
Another program is using that name, so Deskmate does not open the channel.
Close programs you do not know, or sign out of Windows and in again; Deskmate
tries again every 5 seconds. Until then, keys set to **Direct** keep working.
See [SECURITY.md](SECURITY.md#a-program-that-takes-the-pipes-name).

**Where the log is.** The plugin writes
`%AppData%\Elgato\StreamDeck\Plugins\com.deskmate.streamdeck.sdPlugin\logs\`.
The log names operations and error codes. It does not contain the token, the
address, entity names or message contents.

## Local channel

This section is the reference for the connection between the plugin and the
Deskmate app. Users do not need it.

The app is the server and the plugin is the client. The plugin sends requests;
the app answers each one and may also send events on its own.

### The pipe

The channel is a Windows named pipe:

```
\\.\pipe\deskmate-streamdeck-<user>
```

`<user>` is the Windows user name, lowercased, with every character outside
`a` to `z` and `0` to `9` replaced by `_`. A character is a Unicode code point.
Lowercasing comes first and is the full Unicode one, so a letter whose
lowercase form is two code points gives two characters.

| User name | `<user>` |
|---|---|
| `Admin` | `admin` |
| `Jan Kowalski` | `jan_kowalski` |
| `anna.nowak-2` | `anna_nowak_2` |
| `Zoë` | `zo_` |
| `İ` (U+0130) | `i_` |

The plugin takes the name from `os.userInfo().username`, which on Windows is
what `GetUserNameW` returns.

The app takes the name from `GetUserNameW` itself. Both sides are tested
against one table of names, `streamdeck-plugin/test/pipe-names.json`.

The pipe exists only while the user has **Stream Deck** turned on in
Deskmate's Settings. The app creates it for the current user alone (and the
system account) and for local clients only, and keeps a free instance
listening so that a second client does not have to wait. Each connection is
independent: it has its own `hello` and its own watch list. The app serves 16
connections at a time and closes one more at once.

The app only ever creates the pipe; it never joins one that exists. When
another program holds the name, there is no channel: Deskmate says so in
Settings and tries again every 5 seconds.

No secret travels over the channel. The Home Assistant token stays in the app.

### Framing

- UTF-8 text, one JSON object per line. A line ends with `\n`.
- A line is at most 65536 bytes, not counting the `\n`.
- A longer line, a line that is not valid UTF-8, a line that is not JSON and a
  line whose JSON is not an object (an empty line included) all close the
  connection. The side that receives such a line closes; it sends no answer.
- Both sides ignore fields they do not know.

### Requests, responses, events

A request carries a numeric `id` and an `op`. The `id` is an integer that
JavaScript represents exactly (up to 2^53 - 1 either side of zero). The plugin
numbers its requests from 1 on every connection and never reuses a number on
that connection.

Every request gets exactly one response, with the same `id` and `ok` set to
`true` or `false`. This includes refusals: `version` and `bad_request` carry
the `id` too. A failure has `error`, a short code, and may have `message`,
English text meant for the user. A code is a lowercase letter followed by at
most 31 lowercase letters, digits and `_`; the plugin reads any other value
as `error`, since it writes codes to its log. The one response without an
`id` is the `bad_request` for a request that has no usable `id` itself.

```
{"id":3,"ok":false,"error":"unknown_action","message":"That action no longer exists."}
```

An event is a line the app sends on its own. It has no `id` and carries `ev`.
The plugin ignores events it does not know.

Responses may arrive in any order; the plugin matches them by `id`. A response
whose `id` it is not waiting for is ignored. The app works on up to 16
requests of a connection at a time, so that a slow answer from Home Assistant
does not hold up a key press; it reads further requests as those are answered.

### hello

The first request on a connection.

```
> {"id":1,"op":"hello","v":1,"client":"streamdeck","version":"0.2.0"}
< {"id":1,"ok":true,"v":1,"app":"0.9.0","ha":true}
```

- `v` is the protocol version, currently `1`. `client` names the client and
  `version` is its own version. `client` is 1 to 32 characters from `a-z`,
  `0-9`, `_` and `-`; `version` is 1 to 32 letters, digits, `.`, `-` and `+`.
- `app` is the Deskmate version. `ha` says whether the app has a Home
  Assistant address and token configured.
- A `v` the app does not support is refused:
  `{"id":1,"ok":false,"error":"version"}`. Nothing falls back to another
  version: the plugin closes the connection, and also closes it when the
  answer carries a `v` other than the one it asked for.
- `ha` is read once per connection. When the app's Home Assistant settings
  change, the app closes its connections; the plugin reconnects and reads the
  new value.
- Any other request before a successful `hello` is answered with
  `bad_request`.

### actions

The actions the user defined in Deskmate: hotkeys and tray quick actions.

```
> {"id":2,"op":"actions"}
< {"id":2,"ok":true,"actions":[{"id":"hotkey:lights","name":"Lights"},{"id":"tray:pc_sleep","name":"Sleep"}]}
```

The prefix of an `id` says what the action is: `hotkey:` or `tray:`. The
plugin groups the list by it and stores the `id` on the key. Hotkeys come
first, each group in the order of the app's lists. An action without a name
has its own id as the name. Like `entities`, the list is cut to what fits in
one line.

### run

```
> {"id":3,"op":"run","action":"hotkey:lights"}
< {"id":3,"ok":true}
```

Running a hotkey does what pressing it does, the event it sends to Home
Assistant included. The answer comes once the action has run. An action that asks for a
confirmation on the PC answers after the confirmation. `ok` says that the
action exists and was carried out, not how it went: an action that calls Home
Assistant answers `ok` even when Home Assistant could not be reached.

`action` is made of `a-z`, `0-9`, `_` and `:`, 128 characters at most;
anything else is a `bad_request`. Errors: `unknown_action`.

### entities

```
> {"id":4,"op":"entities","domains":["light","switch"]}
< {"id":4,"ok":true,"entities":[{"entity_id":"light.desk","name":"Desk","state":"on"}]}
```

`domains` is optional, with 64 entries at most; without it every entity is
listed. `name` is the entity's friendly name, or its id when it has none. The
list is ordered by `entity_id`.

The answer has to fit in one line like any other. If the list would not, the
app leaves out the entities that do not fit: the last ones in that order.
Asking for fewer domains gets a list that fits.

Errors: `ha_not_configured`, `ha_error`.

### watch

```
> {"id":5,"op":"watch","entities":["light.desk"]}
< {"id":5,"ok":true,"states":{"light.desk":"on"}}
```

Replaces the watch list of this connection; an empty list clears it. The list
holds at most 64 entities, and a longer one is refused with `bad_request`.
`states` has one entry for every entity in the list. An entity Home Assistant
does not have has the state `unavailable`.

From then on the app sends an event whenever a watched entity changes:

```
< {"ev":"state","entity_id":"light.desk","state":"off"}
```

The app learns of a change by reading the watched entities from Home
Assistant every 2 seconds, and once more shortly after a `toggle` or a `call`
on the same connection went through. It reads only while a connection watches
something. While Home Assistant cannot be reached it sends no events; the
changes follow once it answers again.

Errors: `ha_not_configured`, `ha_error`. After `ha_error` the new list is in
place all the same: the app keeps asking Home Assistant and sends a `state`
event for every entity of the list as soon as it has an answer. A `watch`
refused with `bad_request` leaves the previous list as it was.

### toggle

```
> {"id":6,"op":"toggle","entity_id":"light.desk"}
< {"id":6,"ok":true}
```

Calls `homeassistant.toggle` on the entity. Errors: `ha_not_configured`,
`ha_error`.

### call

```
> {"id":7,"op":"call","service":"scene.turn_on","entity_id":"scene.movie","data":{}}
< {"id":7,"ok":true}
```

`service` is `domain.service`. `entity_id` and `data` are optional; `data` is
a JSON object with the service's parameters. Errors: `ha_not_configured`,
`ha_error`.

### Error codes

| Code | Meaning |
|---|---|
| `version` | The app does not support the protocol version in `hello`. |
| `bad_request` | An unknown `op`, a missing or malformed field, a request before `hello`, or a watch list that is too long. |
| `unknown_action` | `run` named an action that does not exist. |
| `ha_not_configured` | The app has no Home Assistant address and token. |
| `ha_error` | Home Assistant could not be reached or refused the request. |

The plugin treats a code it does not know as a failure of that request.

### What the plugin does

- **Connects on demand.** It opens the pipe when a key that needs the app is
  on screen, and when such a key is pressed. Home Assistant keys need the app
  unless Connection is set to Direct; a Deskmate Action key always does. When
  no visible key needs the app, the plugin closes the connection.
- **Says `hello` first**, and sends nothing else until it is answered.
- **Reconnects with a growing delay** while a key needs the app: 1 s, then 2,
  4, 8 and 16 s, then every 30 s. The delay starts over after a `hello` that
  succeeded. A key press tries at once, and so does a settings panel when it
  reads a list.
- **Sends `watch` again after every reconnect**, with the current list. On a
  new connection it sends no `watch` while the list is empty.
- **Gives a request 10 seconds.** After that the request fails on the plugin's
  side and a late answer is ignored. The connection stays open, except for
  `hello`: a `hello` that is not answered in time ends the attempt.
- **Never sends more than 64 entities** in `watch`; it leaves out the rest.
- **Never logs messages.** The log has operations and error codes only.

For development and for the build check, the environment variable
`DESKMATE_STREAMDECK_PIPE` replaces the pipe path. `npm run build` uses it to
run the built plugin against a stand-in for the app.
