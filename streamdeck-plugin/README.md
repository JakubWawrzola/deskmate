# HomeOS for Home Assistant — Elgato Stream Deck plugin

A standalone Stream Deck plugin (SDK v2, Node 20, TypeScript) that controls
Home Assistant directly through its API — it works independently of the
Deskmate desktop app. Actions go over REST (`POST /api/services/...`), live
key state comes over WebSocket (`/api/websocket`).

## Actions

| Action | What it does | Key settings |
|---|---|---|
| Toggle Entity | `homeassistant.toggle` on an entity; the key shows live on/off state (ON = bright, OFF = dimmed) | `Entity ID`, e.g. `light.living_room` |
| Call Service | any HA service with an optional entity and JSON data | `Service` (`light.turn_on`), `Entity ID` (optional), `Data (JSON)` (optional object) |
| Activate Scene | `scene.turn_on` | `Scene`, e.g. `scene.movie_night` (`movie_night` works too) |

Global configuration (entered once, shared by all keys, visible in every
Property Inspector):
- **HA URL** — e.g. `http://192.168.1.10:8123` (LAN) or a Tailscale/domain address
- **Access token** — a long-lived access token from HA

The token lives in the Stream Deck plugin's global settings (locally on this
computer), never in the repo or the code.

## Building

Requirements: Node 20+, npm.

```powershell
cd streamdeck-plugin
npm install
npm run build
```

Output: `com.homeos.homeassistant.sdPlugin\bin\plugin.js` (rollup bundle).
`npm run icons` regenerates the placeholder PNG icons (pure Node, no deps).

## Installing into Stream Deck

Option A — copy the folder:
1. Quit the Stream Deck app (tray → Quit Stream Deck).
2. Copy the WHOLE `com.homeos.homeassistant.sdPlugin` folder to
   `%AppData%\Elgato\StreamDeck\Plugins\`.
3. Start Stream Deck again.

Option B — developer link (nicer while working on the code):
```powershell
npm install -g @elgato/cli
streamdeck link <path>\streamdeck-plugin\com.homeos.homeassistant.sdPlugin
streamdeck restart com.homeos.homeassistant
```

## Configuration

1. In HA: your profile → Security → Long-lived access tokens → Create token.
   Copy it (it is shown only once).
2. In Stream Deck: drag any action from the "Home Assistant" category onto a
   key and fill in **HA URL** and **Access token** in the Property Inspector
   (global fields — once is enough).
3. Fill in the action fields (entity / service / scene).

Note: the Property Inspector loads the `sdpi-components` library from Elgato's
official CDN (`sdpi-components.dev`) — same as the official plugin template.
The first time you open the inspector the computer needs internet access; the
plugin itself (keys, REST/WS to HA) runs fine on LAN only.

Missing URL/token or a network error = a yellow alert triangle on key press;
details in `com.homeos.homeassistant.sdPlugin\logs\`.

## Layout

```
streamdeck-plugin/
├── src/
│   ├── plugin.ts              # action registration + connect + global settings
│   ├── ha-client.ts           # singleton: REST (fetch) + WebSocket (ws), reconnect, state cache
│   └── actions/
│       ├── toggle-entity.ts   # homeassistant.toggle + live state (setState 0/1)
│       ├── call-service.ts    # any domain.service + entity + JSON data
│       └── activate-scene.ts  # scene.turn_on
├── scripts/generate-icons.mjs # placeholder PNG generator (pure Node)
├── com.homeos.homeassistant.sdPlugin/
│   ├── manifest.json          # SDK v2, Node 20 runtime, 3 actions
│   ├── bin/plugin.js          # npm run build output
│   ├── ui/*.html              # Property Inspectors (sdpi-components)
│   └── imgs/                  # icons (placeholders, replace at will)
├── package.json / tsconfig.json / rollup.config.mjs
```

## Manual test checklist

Needs: a physical Stream Deck (or the Stream Deck app's key preview) and an
HA instance reachable from this computer.

1. Build (`npm install`, `npm run build`), install the plugin (option A or B),
   restart the Stream Deck app. Expected: a "Home Assistant" category with 3
   actions and icons appears in the action list.
2. Drag "Toggle Entity" onto a key. Expected: the Property Inspector shows
   Entity ID / HA URL / Access token fields (no fields = no internet for the
   sdpi-components CDN, see Configuration).
3. Press the key WITHOUT URL/token set. Expected: yellow alert triangle, the
   plugin stays alive.
4. Enter the HA URL and token, set Entity ID to a real toggleable entity.
   Expected: within a few seconds the key icon reflects the current state
   (bright when ON, dimmed when OFF).
5. Press the key. Expected: the entity toggles, the key icon updates in ~1 s
   (no alert triangle).
6. Change that entity's state from elsewhere (HA app / dashboard). Expected:
   the key updates by itself (WebSocket subscription).
7. Add "Call Service": Service `light.turn_on`, an Entity ID, Data
   `{"brightness_pct": 20}`. Press. Expected: the light turns on dimmed, an OK
   checkmark on the key.
8. Break the Data JSON on purpose, e.g. `{broken`. Press. Expected: alert
   triangle, nothing was sent to HA.
9. Add "Activate Scene" with an existing scene. Press. Expected: scene runs,
   OK checkmark.
10. Resilience: disconnect the network (or stop HA), press Toggle. Expected:
    alert triangle. After the network returns, the key recovers by itself in
    <60 s (WS reconnect backoff).

## Known limitations (MVP)

- Icons are programmatic placeholders — replace with final artwork.
- No entity dropdown (you type entity_id by hand) — planned; needs an HA
  request from the Property Inspector.
- Keypad only (no dial/encoder actions yet).
- Invalid token: the plugin deliberately stops retrying the WS connection
  until settings change (avoids spamming HA with failed logins).
