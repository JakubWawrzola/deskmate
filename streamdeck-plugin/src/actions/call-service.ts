import streamDeck, { action, SingletonAction, type KeyDownEvent } from "@elgato/streamdeck";
import { haClient } from "../ha-client";

/** Per-key action settings. */
type CallServiceSettings = {
	/** Service in "domain.service" format, e.g. "light.turn_on". */
	service?: string;
	/** Optional target entity. */
	entityId?: string;
	/** Optional service data as JSON (object). */
	data?: string;
};

const logger = streamDeck.logger.createScope("call-service");

const SERVICE_PATTERN = /^([a-z0-9_]+)\.([a-z0-9_]+)$/i;

/**
 * Call Service — pressing the key calls an arbitrary HA service
 * (POST /api/services/{domain}/{service}) with an optional entity_id and JSON data.
 */
@action({ UUID: "com.homeos.homeassistant.call-service" })
export class CallService extends SingletonAction<CallServiceSettings> {
	override async onKeyDown(ev: KeyDownEvent<CallServiceSettings>): Promise<void> {
		const { service, entityId, data } = ev.payload.settings;
		const match = service?.trim().match(SERVICE_PATTERN);
		if (!match || !haClient.isConfigured) {
			await ev.action.showAlert();
			return;
		}

		let payload: Record<string, unknown> = {};
		const rawData = data?.trim();
		if (rawData) {
			try {
				const parsed: unknown = JSON.parse(rawData);
				if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
					throw new Error("data must be a JSON object");
				}
				payload = parsed as Record<string, unknown>;
			} catch (err) {
				logger.error(`Invalid JSON in data field: ${err instanceof Error ? err.message : String(err)}`);
				await ev.action.showAlert();
				return;
			}
		}

		const entity = entityId?.trim();
		if (entity) {
			payload["entity_id"] = entity;
		}

		try {
			await haClient.callService(match[1], match[2], payload);
			await ev.action.showOk();
		} catch (err) {
			logger.error(`Service ${match[0]} failed: ${err instanceof Error ? err.message : String(err)}`);
			await ev.action.showAlert();
		}
	}
}
