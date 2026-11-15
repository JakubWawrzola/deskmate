import {
  action,
  type KeyDownEvent,
  SingletonAction,
  type WillAppearEvent,
  type WillDisappearEvent,
} from "@elgato/streamdeck";
import type { ConnectionHub } from "../connection/hub.ts";
import { errorCode } from "../errors.ts";
import { parseEntityId, parseService, parseServiceData } from "../ha-ids.ts";
import { ACTION_UUID } from "../plugin-info.ts";
import { type ActionLog, VisibleKeys } from "./shared.ts";

/** Per-key settings. */
type CallServiceSettings = {
  /** `domain.service`, for example `light.turn_on`. */
  service?: string;
  /** Optional target entity. */
  entityId?: string;
  /** Optional service data: a JSON object, as text. */
  data?: string;
};

/**
 * Call Service: a press calls any Home Assistant service, with an optional
 * entity and optional JSON data.
 */
@action({ UUID: ACTION_UUID.callService })
export class CallService extends SingletonAction<CallServiceSettings> {
  private readonly hub: ConnectionHub;
  private readonly log: ActionLog;
  private readonly keys: VisibleKeys;

  constructor(hub: ConnectionHub, log: ActionLog) {
    super();
    this.hub = hub;
    this.log = log;
    this.keys = new VisibleKeys(() => hub.retain("homeAssistant"));
  }

  override onWillAppear(ev: WillAppearEvent<CallServiceSettings>): void {
    this.keys.appeared(ev.action.id);
  }

  override onWillDisappear(ev: WillDisappearEvent<CallServiceSettings>): void {
    this.keys.disappeared(ev.action.id);
  }

  override async onKeyDown(ev: KeyDownEvent<CallServiceSettings>): Promise<void> {
    const settings = ev.payload.settings;
    const service = parseService(settings.service);
    const data = parseServiceData(settings.data);
    const typedEntity = settings.entityId?.trim() ?? "";
    const entityId = parseEntityId(typedEntity);
    // An entity that was typed in but is not an entity id is a mistake to
    // show, not a reason to call the service without a target.
    if (!service || !data.ok || (typedEntity !== "" && !entityId)) {
      await ev.action.showAlert();
      return;
    }

    try {
      await this.hub.callService({
        service: `${service.domain}.${service.service}`,
        entityId,
        data: data.data,
      });
      await ev.action.showOk();
    } catch (error) {
      this.log.error(`Calling a service failed: ${errorCode(error)}`);
      await ev.action.showAlert();
    }
  }
}
