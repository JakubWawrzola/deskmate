import type { ConnectionStatus } from "./connection/status.ts";
import { describeStatus } from "./connection/status.ts";
import type { EntityInfo } from "./connection/transport.ts";
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

/** An option of a property inspector's dropdown. */
export type InspectorItem = { value: string; label: string };

/** What a property inspector asks the plugin for. */
export type InspectorRequest = "getStatus" | "getEntities";

/** The request in a message from a property inspector, if it is one. */
export function readInspectorRequest(payload: unknown): InspectorRequest | undefined {
  if (typeof payload !== "object" || payload === null || !("event" in payload)) {
    return undefined;
  }
  const { event } = payload;
  return event === "getStatus" || event === "getEntities" ? event : undefined;
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

/** The connection status as the message a property inspector displays. */
export function statusMessage(status: ConnectionStatus): {
  event: "status";
  mode: string;
  active: string;
  text: string;
  hint: string;
} {
  const { text, hint } = describeStatus(status);
  return { event: "status", mode: status.mode, active: status.active, text, hint: hint ?? "" };
}
