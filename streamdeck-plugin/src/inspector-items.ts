import type { ConnectionStatus } from "./connection/status.ts";
import { describeAppStatus, describeStatus } from "./connection/status.ts";
import type { DeskmateAction, EntityInfo } from "./connection/transport.ts";
import { domainsForService } from "./ha-ids.ts";
import { ACTION_UUID } from "./plugin-info.ts";

/**
 * Domains whose entities are on or off, which is what the Toggle Entity key
 * can show. Any other entity can still be typed in by hand.
 */
export const TOGGLE_DOMAINS: readonly string[] = [
  "light",
  "switch",
  "input_boolean",
  "fan",
  "automation",
  "humidifier",
  "siren",
  "remote",
  "group",
];

const SCENE_DOMAINS: readonly string[] = ["scene"];

/** Longest action identifier the plugin sends to the app. */
const MAX_ACTION_ID_LENGTH = 200;

/** The kinds of action Deskmate has, by the prefix of their identifier. */
const ACTION_GROUPS: readonly { prefix: string; label: string }[] = [
  { prefix: "hotkey:", label: "Hotkeys" },
  { prefix: "tray:", label: "Tray actions" },
];

/** An option of a property inspector's dropdown. */
export type InspectorItem = { value: string; label: string };

/** Options under a common heading. */
export type InspectorGroup = { label: string; children: InspectorItem[] };

/** What a property inspector asks the plugin for. */
export type InspectorRequest = "getStatus" | "getEntities" | "getActions";

/** The request in a message from a property inspector, if it is one. */
export function readInspectorRequest(payload: unknown): InspectorRequest | undefined {
  if (typeof payload !== "object" || payload === null || !("event" in payload)) {
    return undefined;
  }
  const { event } = payload;
  return event === "getStatus" || event === "getEntities" || event === "getActions"
    ? event
    : undefined;
}

/**
 * The entity domains to offer in an action's property inspector, or
 * `undefined` for every domain.
 */
export function entityDomainsFor(
  actionUuid: string,
  settings: { service?: unknown },
): readonly string[] | undefined {
  if (actionUuid === ACTION_UUID.toggleEntity) {
    return TOGGLE_DOMAINS;
  }
  if (actionUuid === ACTION_UUID.activateScene) {
    return SCENE_DOMAINS;
  }
  return domainsForService(typeof settings.service === "string" ? settings.service : undefined);
}

/** Entities as dropdown options: the name first, the id to tell twins apart. */
export function entityItems(entities: readonly EntityInfo[]): InspectorItem[] {
  return entities.map((entity) => ({
    value: entity.entityId,
    label:
      entity.name === entity.entityId ? entity.entityId : `${entity.name} (${entity.entityId})`,
  }));
}

/**
 * Deskmate's actions as dropdown options, hotkeys and tray actions under
 * their own headings. A kind this plugin does not know is listed after them
 * without a heading. Kinds with no action leave no empty heading.
 */
export function actionItems(
  actions: readonly DeskmateAction[],
): (InspectorItem | InspectorGroup)[] {
  const groups: InspectorGroup[] = ACTION_GROUPS.map(({ label }) => ({ label, children: [] }));
  const others: InspectorItem[] = [];
  for (const { id, name } of actions) {
    const item = { value: id, label: name === "" ? id : name };
    const index = ACTION_GROUPS.findIndex(({ prefix }) => id.startsWith(prefix));
    (groups[index]?.children ?? others).push(item);
  }
  return [...groups.filter((group) => group.children.length > 0), ...others];
}

/** The identifier of a Deskmate action as stored on a key, or `undefined` when none is set. */
export function parseActionId(value: string | undefined): string | undefined {
  const actionId = value?.trim() ?? "";
  return actionId !== "" && actionId.length <= MAX_ACTION_ID_LENGTH ? actionId : undefined;
}

/**
 * The connection status as the message a property inspector displays. A
 * Deskmate Action key is told about the app alone.
 */
export function statusMessage(
  status: ConnectionStatus,
  actionUuid: string | undefined,
): { event: "status"; mode: string; active: string; text: string; hint: string } {
  if (actionUuid === ACTION_UUID.deskmateAction) {
    const { text, hint } = describeAppStatus(status.channel);
    const active = status.channel.state === "ready" ? "deskmate" : "none";
    return { event: "status", mode: "deskmate", active, text, hint: hint ?? "" };
  }
  const { text, hint } = describeStatus(status);
  return { event: "status", mode: status.mode, active: status.active, text, hint: hint ?? "" };
}
