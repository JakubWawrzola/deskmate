import streamDeck from "@elgato/streamdeck";
import { ActivateScene } from "./actions/activate-scene";
import { CallService } from "./actions/call-service";
import { ToggleEntity } from "./actions/toggle-entity";
import { type HaGlobalSettings, haClient } from "./ha-client";

// "info" is enough for normal operation; "trace" would also log payloads, and
// the global settings carry the access token.
streamDeck.logger.setLevel("info");

streamDeck.actions.registerAction(new ToggleEntity());
streamDeck.actions.registerAction(new CallService());
streamDeck.actions.registerAction(new ActivateScene());

// The property inspector writes the URL and token to the global settings.
// Since SDK 3 this fires only for changes made there.
streamDeck.settings.onDidReceiveGlobalSettings<HaGlobalSettings>((ev) => {
  haClient.configure(ev.settings);
});

await streamDeck.connect();

// The stored settings can only be read once the connection is up.
const settings = await streamDeck.settings.getGlobalSettings<HaGlobalSettings>();
haClient.configure(settings);
