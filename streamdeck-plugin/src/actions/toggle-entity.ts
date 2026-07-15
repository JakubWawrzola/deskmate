import streamDeck, {
	action,
	SingletonAction,
	type DidReceiveSettingsEvent,
	type KeyDownEvent,
	type WillAppearEvent,
	type WillDisappearEvent
} from "@elgato/streamdeck";
import { haClient, type HaEntityState } from "../ha-client";

/** Per-key action settings. */
type ToggleSettings = {
	entityId?: string;
};

type VisibleAction = WillAppearEvent<ToggleSettings>["action"];

const logger = streamDeck.logger.createScope("toggle-entity");

/**
 * Toggle Entity — pressing the key calls homeassistant.toggle on the target entity.
 * The key shows the live state (WebSocket subscription):
 * state 1 = ON (bright icon), state 0 = OFF (dimmed).
 */
@action({ UUID: "com.homeos.homeassistant.toggle" })
export class ToggleEntity extends SingletonAction<ToggleSettings> {
	/** action.id -> state unsubscribe function. */
	private readonly subscriptions = new Map<string, () => void>();

	override onWillAppear(ev: WillAppearEvent<ToggleSettings>): void {
		this.resubscribe(ev.action, ev.payload.settings);
	}

	override onDidReceiveSettings(ev: DidReceiveSettingsEvent<ToggleSettings>): void {
		this.resubscribe(ev.action, ev.payload.settings);
	}

	override onWillDisappear(ev: WillDisappearEvent<ToggleSettings>): void {
		this.subscriptions.get(ev.action.id)?.();
		this.subscriptions.delete(ev.action.id);
	}

	override async onKeyDown(ev: KeyDownEvent<ToggleSettings>): Promise<void> {
		const entityId = ev.payload.settings.entityId?.trim();
		if (!entityId || !haClient.isConfigured) {
			await ev.action.showAlert();
			return;
		}
		try {
			await haClient.callService("homeassistant", "toggle", { entity_id: entityId });
			// no showOk — the key's state change from the WS subscription is the confirmation
		} catch (err) {
			logger.error(`Toggle ${entityId} failed: ${err instanceof Error ? err.message : String(err)}`);
			await ev.action.showAlert();
		}
	}

	private resubscribe(visibleAction: VisibleAction, settings: ToggleSettings): void {
		this.subscriptions.get(visibleAction.id)?.();
		this.subscriptions.delete(visibleAction.id);

		const entityId = settings.entityId?.trim();
		if (!entityId) {
			return;
		}
		const unsubscribe = haClient.onEntityState(entityId, (state) => {
			void this.render(visibleAction, state);
		});
		this.subscriptions.set(visibleAction.id, unsubscribe);
	}

	private async render(visibleAction: VisibleAction, state: HaEntityState | undefined): Promise<void> {
		if (!visibleAction.isKey()) {
			return;
		}
		try {
			await visibleAction.setState(state?.state === "on" ? 1 : 0);
		} catch (err) {
			logger.error(`setState failed: ${err instanceof Error ? err.message : String(err)}`);
		}
	}
}
