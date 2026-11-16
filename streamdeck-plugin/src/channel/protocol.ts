/** Version of the local channel this client speaks. */
export const PROTOCOL_VERSION = 1;

/** How the plugin names itself in `hello`. */
export const CLIENT_NAME = "streamdeck";

/** The app keeps at most this many watched entities per connection. */
export const MAX_WATCHED_ENTITIES = 64;

export type JsonRecord = Record<string, unknown>;

/** A line or a response body that does not follow the protocol. */
export class ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

export type ServerResponse =
  | { kind: "response"; id: number | undefined; ok: true; body: JsonRecord }
  | {
      kind: "response";
      id: number | undefined;
      ok: false;
      error: string;
      message: string | undefined;
    };

export type ServerEvent = { kind: "event"; ev: string; body: JsonRecord };

/** A well-formed object that is neither a response nor an event. */
export type UnknownMessage = { kind: "unknown" };

export type ServerMessage = ServerResponse | ServerEvent | UnknownMessage;

export type HelloInfo = { app: string; ha: boolean };
export type DeskmateAction = { id: string; name: string };
export type ChannelEntity = { entityId: string; name: string; state: string };
export type EntityState = { entityId: string; state: string };

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requireString(body: JsonRecord, key: string): string {
  const value = body[key];
  if (typeof value !== "string") {
    throw new ProtocolError(`"${key}" is not a string`);
  }
  return value;
}

function requireArray(body: JsonRecord, key: string): unknown[] {
  const value = body[key];
  if (!Array.isArray(value)) {
    throw new ProtocolError(`"${key}" is not an array`);
  }
  return value;
}

function requireRecord(value: unknown, what: string): JsonRecord {
  if (!isRecord(value)) {
    throw new ProtocolError(`${what} is not an object`);
  }
  return value;
}

/** What an error code from the app looks like: `ha_error`, `unknown_action`. */
const ERROR_CODE_PATTERN = /^[a-z][a-z0-9_]{0,31}$/;
/** Stands in for an error code that does not look like one. */
const UNREADABLE_ERROR_CODE = "error";

/**
 * The plugin does not know who serves the pipe, and the code goes into its
 * log, so only a short code passes.
 */
function readErrorCode(error: unknown): string {
  return typeof error === "string" && ERROR_CODE_PATTERN.test(error)
    ? error
    : UNREADABLE_ERROR_CODE;
}

/**
 * Reads one line from the app. Throws when the line is not a JSON object:
 * the caller closes the connection over that.
 */
export function parseServerLine(line: string): ServerMessage {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    throw new ProtocolError("The line is not JSON");
  }
  if (!isRecord(parsed)) {
    throw new ProtocolError("The line is not a JSON object");
  }

  const { id, ok, ev } = parsed;
  if (typeof ok === "boolean" && (id === undefined || Number.isSafeInteger(id))) {
    // Number.isSafeInteger has narrowed nothing for the compiler.
    const responseId = typeof id === "number" ? id : undefined;
    if (ok) {
      return { kind: "response", id: responseId, ok: true, body: parsed };
    }
    return {
      kind: "response",
      id: responseId,
      ok: false,
      error: readErrorCode(parsed.error),
      message: typeof parsed.message === "string" ? parsed.message : undefined,
    };
  }
  if (id === undefined && typeof ev === "string") {
    return { kind: "event", ev, body: parsed };
  }
  return { kind: "unknown" };
}

/** The body of a successful `hello`. A different version is refused. */
export function readHello(body: JsonRecord): HelloInfo {
  if (body.v !== PROTOCOL_VERSION) {
    throw new ProtocolError("The app answered with another protocol version");
  }
  if (typeof body.ha !== "boolean") {
    throw new ProtocolError('"ha" is not a boolean');
  }
  return { app: requireString(body, "app"), ha: body.ha };
}

export function readActions(body: JsonRecord): DeskmateAction[] {
  return requireArray(body, "actions").map((item) => {
    const action = requireRecord(item, "An action");
    return { id: requireString(action, "id"), name: requireString(action, "name") };
  });
}

export function readEntities(body: JsonRecord): ChannelEntity[] {
  return requireArray(body, "entities").map((item) => {
    const entity = requireRecord(item, "An entity");
    return {
      entityId: requireString(entity, "entity_id"),
      name: requireString(entity, "name"),
      state: requireString(entity, "state"),
    };
  });
}

/** The `states` object of a `watch` response. */
export function readStates(body: JsonRecord): EntityState[] {
  const states = requireRecord(body.states, '"states"');
  return Object.keys(states).map((entityId) => ({
    entityId,
    state: requireString(states, entityId),
  }));
}

/** The body of a `state` event. */
export function readStateEvent(body: JsonRecord): EntityState {
  return { entityId: requireString(body, "entity_id"), state: requireString(body, "state") };
}
