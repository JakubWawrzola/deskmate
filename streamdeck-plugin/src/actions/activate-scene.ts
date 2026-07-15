import streamDeck, { action, SingletonAction, type KeyDownEvent } from "@elgato/streamdeck";
import { haClient } from "../ha-client";

/** Action settings (per key). */
type SceneSettings = {
	/** Scene entity, e.g. "scene.film". A bare suffix ("film") also works. */
	sceneId?: string;
};

const logger = streamDeck.logger.createScope("activate-scene");

/**
 * Activate Scene — key press calls scene.turn_on on the configured scene.
 */
@action({ UUID: "com.homeos.homeassistant.scene" })
export class ActivateScene extends SingletonAction<SceneSettings> {
	override async onKeyDown(ev: KeyDownEvent<SceneSettings>): Promise<void> {
		let sceneId = ev.payload.settings.sceneId?.trim();
		if (!sceneId || !haClient.isConfigured) {
			await ev.action.showAlert();
			return;
		}
		if (!sceneId.includes(".")) {
			sceneId = `scene.${sceneId}`;
		}
		try {
			await haClient.callService("scene", "turn_on", { entity_id: sceneId });
			await ev.action.showOk();
		} catch (err) {
			logger.error(`Scene ${sceneId} failed: ${err instanceof Error ? err.message : String(err)}`);
			await ev.action.showAlert();
		}
	}
}
