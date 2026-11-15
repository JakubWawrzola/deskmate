import {
  action,
  type KeyDownEvent,
  SingletonAction,
  type WillAppearEvent,
  type WillDisappearEvent,
} from "@elgato/streamdeck";
import type { ConnectionHub } from "../connection/hub.ts";
import { errorCode } from "../errors.ts";
import { parseActionId } from "../inspector-items.ts";
import { ACTION_UUID } from "../plugin-info.ts";
import { type ActionLog, VisibleKeys } from "./shared.ts";

/** Per-key settings. */
type DeskmateActionSettings = {
  /** Identifier of the action in Deskmate, for example `hotkey:lights`. */
  actionId?: string;
  /** Its name when it was picked; the property inspector shows it while the list is not available. */
  actionName?: string;
};

/**
 * Deskmate Action: a press runs an action the user defined in the Deskmate
 * app, a hotkey or a tray quick action. Only the app can run these, so the
 * key needs the app's channel whatever the connection setting says.
 */
@action({ UUID: ACTION_UUID.deskmateAction })
export class DeskmateAction extends SingletonAction<DeskmateActionSettings> {
  private readonly hub: ConnectionHub;
  private readonly log: ActionLog;
  private readonly keys: VisibleKeys;

  constructor(hub: ConnectionHub, log: ActionLog) {
    super();
    this.hub = hub;
    this.log = log;
    this.keys = new VisibleKeys(() => hub.retain("deskmate"));
  }

  override onWillAppear(ev: WillAppearEvent<DeskmateActionSettings>): void {
    this.keys.appeared(ev.action.id);
  }

  override onWillDisappear(ev: WillDisappearEvent<DeskmateActionSettings>): void {
    this.keys.disappeared(ev.action.id);
  }

  override async onKeyDown(ev: KeyDownEvent<DeskmateActionSettings>): Promise<void> {
    const actionId = parseActionId(ev.payload.settings.actionId);
    if (!actionId) {
      await ev.action.showAlert();
      return;
    }
    try {
      await this.hub.runAction(actionId);
      await ev.action.showOk();
    } catch (error) {
      // Not connected, an action that no longer exists, or a failure in the app.
      this.log.error(`Running a Deskmate action failed: ${errorCode(error)}`);
      await ev.action.showAlert();
    }
  }
}
