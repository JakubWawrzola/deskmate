import {
  action,
  type KeyDownEvent,
  SingletonAction,
  type WillAppearEvent,
  type WillDisappearEvent,
} from "@elgato/streamdeck";
import type { ConnectionHub } from "../connection/hub.ts";
import { errorCode } from "../errors.ts";
import { parseSceneId } from "../ha-ids.ts";
import { ACTION_UUID } from "../plugin-info.ts";
import { type ActionLog, VisibleKeys } from "./shared.ts";

/** Per-key settings. */
type SceneSettings = {
  /** The scene entity, `scene.movie`; the part after the dot works too. */
  sceneId?: string;
};

const SCENE_SERVICE = "scene.turn_on";

/** Activate Scene: a press turns the scene on. */
@action({ UUID: ACTION_UUID.activateScene })
export class ActivateScene extends SingletonAction<SceneSettings> {
  private readonly hub: ConnectionHub;
  private readonly log: ActionLog;
  private readonly keys: VisibleKeys;

  constructor(hub: ConnectionHub, log: ActionLog) {
    super();
    this.hub = hub;
    this.log = log;
    this.keys = new VisibleKeys(() => hub.retain("homeAssistant"));
  }

  override onWillAppear(ev: WillAppearEvent<SceneSettings>): void {
    this.keys.appeared(ev.action.id);
  }

  override onWillDisappear(ev: WillDisappearEvent<SceneSettings>): void {
    this.keys.disappeared(ev.action.id);
  }

  override async onKeyDown(ev: KeyDownEvent<SceneSettings>): Promise<void> {
    const sceneId = parseSceneId(ev.payload.settings.sceneId);
    if (!sceneId) {
      await ev.action.showAlert();
      return;
    }
    try {
      await this.hub.callService({ service: SCENE_SERVICE, entityId: sceneId, data: undefined });
      await ev.action.showOk();
    } catch (error) {
      this.log.error(`Activating a scene failed: ${errorCode(error)}`);
      await ev.action.showAlert();
    }
  }
}
