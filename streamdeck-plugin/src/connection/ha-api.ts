import type { EntityInfo } from "./transport.ts";

/** State reported for an entity Home Assistant does not have. */
export const UNAVAILABLE_STATE = "unavailable";

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Address of the WebSocket API for an http or https base URL. */
export function webSocketUrl(baseUrl: string): string {
  return `${baseUrl.replace(/^http/i, "ws")}/api/websocket`;
}

/** One state object of the REST or WebSocket API, or `undefined` if it is not one. */
export function readEntity(value: unknown): EntityInfo | undefined {
  if (!isRecord(value) || typeof value.entity_id !== "string" || typeof value.state !== "string") {
    return undefined;
  }
  const attributes = isRecord(value.attributes) ? value.attributes : {};
  const friendlyName = attributes.friendly_name;
  return {
    entityId: value.entity_id,
    name: typeof friendlyName === "string" && friendlyName !== "" ? friendlyName : value.entity_id,
    state: value.state,
  };
}

/**
 * The answer of `GET /api/states` as a list for a property inspector: only
 * the wanted domains, sorted by name. Entries that are not state objects are
 * left out.
 */
export function entitiesFromStates(
  value: unknown,
  domains: readonly string[] | undefined,
): EntityInfo[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const wanted = domains ? new Set(domains) : undefined;
  const entities: EntityInfo[] = [];
  for (const item of value) {
    const entity = readEntity(item);
    if (entity && (!wanted || wanted.has(entity.entityId.split(".", 1)[0] ?? ""))) {
      entities.push(entity);
    }
  }
  return entities.sort((a, b) => a.name.localeCompare(b.name, "en"));
}

export type SocketMessage =
  | { type: "auth_required" | "auth_ok" | "auth_invalid" | "pong" | "other" }
  | { type: "result"; id: number; states: EntityInfo[] }
  | { type: "state_changed"; entityId: string; state: string };

/** One message of Home Assistant's WebSocket API, reduced to what the plugin uses. */
export function readSocketMessage(raw: string): SocketMessage {
  let message: unknown;
  try {
    message = JSON.parse(raw);
  } catch {
    return { type: "other" };
  }
  if (!isRecord(message)) {
    return { type: "other" };
  }
  switch (message.type) {
    case "auth_required":
    case "auth_ok":
    case "auth_invalid":
    case "pong":
      return { type: message.type };
    case "result": {
      if (typeof message.id !== "number" || message.success !== true) {
        return { type: "other" };
      }
      return {
        type: "result",
        id: message.id,
        states: entitiesFromStates(message.result, undefined),
      };
    }
    case "event": {
      const event = isRecord(message.event) ? message.event : {};
      const data = isRecord(event.data) ? event.data : {};
      if (event.event_type !== "state_changed" || typeof data.entity_id !== "string") {
        return { type: "other" };
      }
      // A removed entity has no new state.
      const state = readEntity(data.new_state)?.state ?? UNAVAILABLE_STATE;
      return { type: "state_changed", entityId: data.entity_id, state };
    }
    default:
      return { type: "other" };
  }
}
