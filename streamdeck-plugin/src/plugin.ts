import streamDeck, { LogLevel } from "@elgato/streamdeck";
import { haClient, type HaGlobalSettings } from "./ha-client";
import { ActivateScene } from "./actions/activate-scene";
import { CallService } from "./actions/call-service";
import { ToggleEntity } from "./actions/toggle-entity";

// INFO is enough for normal operation; TRACE would also log payloads (token in global settings!)
streamDeck.logger.setLevel(LogLevel.INFO);

streamDeck.actions.registerAction(new ToggleEntity());
streamDeck.actions.registerAction(new CallService());
streamDeck.actions.registerAction(new ActivateScene());

// Property Inspector writes URL/token to global settings — every change lands here
streamDeck.settings.onDidReceiveGlobalSettings<HaGlobalSettings>((ev) => {
	haClient.configure(ev.settings);
});

await streamDeck.connect();

// konfiguracja startowa (po connect, bo dopiero wtedy mozna czytac settings)
const settings = await streamDeck.settings.getGlobalSettings<HaGlobalSettings>();
haClient.configure(settings);
