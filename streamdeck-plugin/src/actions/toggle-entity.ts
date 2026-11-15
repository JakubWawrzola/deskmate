import {
  type Action,
  action,
  type DidReceiveSettingsEvent,
  type KeyDownEvent,
  SingletonAction,
  type WillAppearEvent,
  type WillDisappearEvent,
} from "@elgato/streamdeck";
import type { ConnectionHub } from "../connection/hub.ts";
import { errorCode } from "../errors.ts";
import { parseEntityId } from "../ha-ids.ts";
import { ACTION_UUID } from "../plugin-info.ts";
import type { ActionLog } from "./shared.ts";

/** Per-key settings. */
type ToggleSettings = {
  entityId?: string;
};

type VisibleKey = {
  release: () => void;
  unwatch: (() => void) | undefined;
};

const STATE_OFF = 0;
const STATE_ON = 1;

/**
 * Toggle Entity: a press toggles the entity, and the key shows whether it is
 * on (bright icon) or off (dimmed), following every change.
 */
@action({ UUID: ACTION_UUID.toggleEntity })
export class ToggleEntity extends SingletonAction<ToggleSettings> {
  private readonly hub: ConnectionHub;
  private readonly log: ActionLog;
  private readonly visible = new Map<string, VisibleKey>();

  constructor(hub: ConnectionHub, log: ActionLog) {
    super();
    this.hub = hub;
    this.log = log;
  }

  override onWillAppear(ev: WillAppearEvent<ToggleSettings>): void {
    this.show(ev.action, ev.payload.settings);
  }

  override onDidReceiveSettings(ev: DidReceiveSettingsEvent<ToggleSettings>): void {
    this.show(ev.action, ev.payload.settings);
  }

  override onWillDisappear(ev: WillDisappearEvent<ToggleSettings>): void {
    const key = this.visible.get(ev.action.id);
    key?.unwatch?.();
    key?.release();
    this.visible.delete(ev.action.id);
  }

  override async onKeyDown(ev: KeyDownEvent<ToggleSettings>): Promise<void> {
    const entityId = parseEntityId(ev.payload.settings.entityId);
    if (!entityId) {
      await ev.action.showAlert();
      return;
    }
    try {
      // No "OK" mark: the key changing state is the confirmation.
      await this.hub.toggle(entityId);
    } catch (error) {
      this.log.error(`Toggling an entity failed: ${errorCode(error)}`);
      await ev.action.showAlert();
    }
  }

  /** Starts, or moves, the state subscription of a key. */
  private show(visibleAction: Action<ToggleSettings>, settings: ToggleSettings): void {
    const key = this.visible.get(visibleAction.id) ?? {
      release: this.hub.retain("homeAssistant"),
      unwatch: undefined,
    };
    this.visible.set(visibleAction.id, key);
    key.unwatch?.();

    const entityId = parseEntityId(settings.entityId);
    key.unwatch = entityId
      ? this.hub.watchEntity(entityId, (state) => void this.render(visibleAction, state))
      : undefined;
  }

  private async render(
    visibleAction: Action<ToggleSettings>,
    state: string | undefined,
  ): Promise<void> {
    if (!visibleAction.isKey()) {
      return;
    }
    try {
      await visibleAction.setState(state === "on" ? STATE_ON : STATE_OFF);
    } catch (error) {
      this.log.error(`Showing a key state failed: ${errorCode(error)}`);
    }
  }
}
