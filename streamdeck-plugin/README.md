# Deskmate for Home Assistant: Stream Deck plugin

The Stream Deck plugin of [Deskmate](../README.md). It puts Home Assistant
entities, services and scenes, and the actions defined in the Deskmate app, on
Stream Deck keys.

What it does, how to install it and how to set up a key is in
[docs/STREAMDECK.md](../docs/STREAMDECK.md). This file is for working on the
code.

The plugin is versioned on its own: `0.2.0` in `package.json`,
`manifest.template.json` and `src/plugin-info.ts`, and a test keeps the three
equal.

## Requirements

- Node.js 24, the version Stream Deck runs the plugin on.
- Stream Deck 7.1 or newer to load it (`@elgato/streamdeck` 3, manifest
  `SDKVersion` 3).

## Commands

Run them in `streamdeck-plugin/`.

| Command | What it does |
|---|---|
| `npm ci` | Installs the dependencies from the lockfile. |
| `npx tsc --noEmit` | Type check, `src/` and `test/`. |
| `npm run lint` | Biome: lint and formatting check. |
| `npm run format` | Biome: rewrites the files in the repository's style. |
| `npm test` | Unit tests with `node --test`. |
| `npm run build` | Bundles the plugin, then checks the result (below). |
| `npm run watch` | Rebuilds on every change. |
| `npm run icons` | Redraws the PNG icons (`scripts/generate-icons.mjs`). |

CI runs `npm ci`, the type check, the lint, the tests and the build, in that
order.

## What the build writes

`npm run build` fills in `com.deskmate.streamdeck.sdPlugin/`, which is the
folder Stream Deck loads:

```
com.deskmate.streamdeck.sdPlugin/
├── manifest.json      written by the build, a copy of manifest.template.json
├── bin/
│   ├── plugin.js      written by the build: the bundle, dependencies included
│   └── package.json   written by the build: marks the bundle as an ES module
├── imgs/              icons, in the repository
└── ui/                property inspectors and their script and stylesheet, in the repository
```

The three written files are ignored by git. `logs/` appears next to them once
Stream Deck has run the plugin; it is ignored too and is not part of the
plugin.

The manifest lives in the repository as `manifest.template.json` because the
repository may contain only one file named `manifest.json`, the Home Assistant
integration's: the HACS checks require that. Edit the template, never the
copy.

## The check after the build

`scripts/check-bundle.mjs` runs at the end of every build:

1. Every file the manifest and the property inspectors point at is in the
   folder.
2. The bundle is started the way Stream Deck starts it, against stand-ins that
   run on this computer: a WebSocket server in place of the Stream Deck app, a
   pipe in place of the Deskmate app's local channel, and an HTTP and
   WebSocket server in place of Home Assistant. Keys of every action are shown
   and pressed, first through the stand-in app, then directly against the
   stand-in Home Assistant, then with neither.

The check needs no Stream Deck, no Deskmate and no Home Assistant, and it
never opens the real app's pipe: the plugin is pointed at the stand-in with
the `DESKMATE_STREAMDECK_PIPE` environment variable.

## Layout

```
streamdeck-plugin/
├── src/
│   ├── plugin.ts            wiring: channel, transports, hub, actions, inspector
│   ├── plugin-info.ts       version and action identifiers
│   ├── settings.ts          the global settings, checked when read
│   ├── ha-ids.ts            entity ids, services and service data as typed by the user
│   ├── actions/             one file per action
│   ├── channel/             client of the Deskmate app's local channel
│   │   ├── pipe-name.ts     pipe path from the Windows user name
│   │   ├── framing.ts       one JSON object per line, 65536 bytes at most
│   │   ├── protocol.ts      messages and their validation
│   │   ├── backoff.ts       reconnect delays
│   │   └── client.ts        connect, hello, requests, watch list, reconnect
│   ├── connection/
│   │   ├── transport.ts     the interface both transports implement
│   │   ├── deskmate.ts      Home Assistant through the Deskmate app
│   │   ├── direct.ts        Home Assistant over its REST and WebSocket API
│   │   ├── ha-api.ts        reading Home Assistant's answers
│   │   ├── select.ts        which transport the connection setting selects
│   │   ├── status.ts        the status line of the property inspectors
│   │   └── hub.ts           what the actions talk to
│   ├── inspector.ts         answers the property inspectors
│   └── inspector-items.ts   the lists and messages they are given
├── test/                    unit tests, one file per module
├── scripts/                 the icon generator and the check after the build
├── manifest.template.json   the plugin manifest
└── com.deskmate.streamdeck.sdPlugin/   icons and property inspectors
```

The actions never choose a connection themselves. They ask the hub, and the
hub sends the operation over the transport that the `connection` setting
selects. The protocol of the app's local channel is specified in
[docs/STREAMDECK.md](../docs/STREAMDECK.md#local-channel); a change to it
changes that section and the tests in the same commit.

## Conventions

- Relative imports name the `.ts` file, and the code uses no enums and no
  constructor parameter properties. Node runs the tests by stripping the
  types, which only works for syntax it can strip; `tsconfig.json` enforces
  both. The build rewrites the extensions.
- A module that the tests import must not import `@elgato/streamdeck`. The SDK
  creates its log folder in the working directory as soon as it is loaded, and
  the action classes use its decorators, which Node does not run without a
  build. Logic goes into modules without the SDK; `plugin.ts`, `inspector.ts`
  and the actions do the wiring, and the check after the build covers them.
- Nothing typed by the user and nothing received from the app or from Home
  Assistant goes into the log: operations and error codes only.
- TypeScript stays on 5.x. With 7 the build fails inside
  `@rollup/plugin-typescript` 12, and Dependabot is told to skip that major.
- `zod` is overridden to 4.6.6 or newer in `package.json`. The SDK brings it
  in as `^3.25.24`, and every 3.x release is covered by a high-severity
  advisory. Remove the override once the SDK depends on a fixed release.

## Loading a development build into Stream Deck

Copy the built folder as described under
[Install, by hand](../docs/STREAMDECK.md#by-hand), or link it with Elgato's
command-line tool so that Stream Deck loads it from the repository:

```powershell
streamdeck link com.deskmate.streamdeck.sdPlugin
streamdeck restart com.deskmate.streamdeck
```

The property inspectors load Elgato's `sdpi-components` from
`sdpi-components.dev`, like Elgato's plugin template does.
