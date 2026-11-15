import { connect } from "node:net";
import { userInfo } from "node:os";
import streamDeck from "@elgato/streamdeck";
import { ActivateScene } from "./actions/activate-scene.ts";
import { CallService } from "./actions/call-service.ts";
import { DeskmateAction } from "./actions/deskmate-action.ts";
import { ToggleEntity } from "./actions/toggle-entity.ts";
import { ChannelClient } from "./channel/client.ts";
import { resolvePipePath } from "./channel/pipe-name.ts";
import { DeskmateTransport } from "./connection/deskmate.ts";
import { DirectTransport } from "./connection/direct.ts";
import { ConnectionHub } from "./connection/hub.ts";
import { connectInspector } from "./inspector.ts";
import { PLUGIN_VERSION } from "./plugin-info.ts";
import type { GlobalSettings } from "./settings.ts";

// "info" is enough for normal operation; "trace" would also log payloads, and
// the global settings carry the access token.
streamDeck.logger.setLevel("info");

const channel = new ChannelClient({
  path: resolvePipePath(process.env, () => userInfo().username),
  version: PLUGIN_VERSION,
  connect: (path) => connect(path),
  log: streamDeck.logger.createScope("channel"),
});
const hub = new ConnectionHub(
  new DeskmateTransport(channel),
  new DirectTransport(streamDeck.logger.createScope("direct")),
);

streamDeck.actions.registerAction(
  new ToggleEntity(hub, streamDeck.logger.createScope("toggle-entity")),
);
streamDeck.actions.registerAction(
  new CallService(hub, streamDeck.logger.createScope("call-service")),
);
streamDeck.actions.registerAction(
  new ActivateScene(hub, streamDeck.logger.createScope("activate-scene")),
);
streamDeck.actions.registerAction(
  new DeskmateAction(hub, streamDeck.logger.createScope("deskmate-action")),
);
connectInspector(hub, streamDeck.logger.createScope("inspector"));

// The property inspectors write the connection settings to the global
// settings. Since SDK 3 this fires only for changes made there.
streamDeck.settings.onDidReceiveGlobalSettings<GlobalSettings>((ev) => {
  hub.configure(ev.settings);
});

await streamDeck.connect();

// The stored settings can only be read once the connection is up.
hub.configure(await streamDeck.settings.getGlobalSettings<GlobalSettings>());
