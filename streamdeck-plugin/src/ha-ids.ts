import type { JsonRecord } from "./channel/protocol.ts";

const ENTITY_ID_PATTERN = /^[a-z0-9_]+\.[a-z0-9_]+$/;
const SERVICE_PATTERN = /^([a-z0-9_]+)\.([a-z0-9_]+)$/;
const OBJECT_ID_PATTERN = /^[a-z0-9_]+$/;

/** Services of this domain act on entities of any domain. */
const GENERIC_SERVICE_DOMAIN = "homeassistant";

export type ServiceName = { domain: string; service: string };

/**
 * An entity id as typed by the user, lowercased, or `undefined` when it is
 * not one. Home Assistant ids are always lowercase.
 */
export function parseEntityId(text: string | undefined): string | undefined {
  const entityId = text?.trim().toLowerCase() ?? "";
  return ENTITY_ID_PATTERN.test(entityId) ? entityId : undefined;
}

/** A scene as typed by the user: `scene.movie`, or just `movie`. */
export function parseSceneId(text: string | undefined): string | undefined {
  const scene = text?.trim().toLowerCase() ?? "";
  if (OBJECT_ID_PATTERN.test(scene)) {
    return `scene.${scene}`;
  }
  return scene.startsWith("scene.") ? parseEntityId(scene) : undefined;
}

/** A service as typed by the user, `domain.service`. */
export function parseService(text: string | undefined): ServiceName | undefined {
  const match = SERVICE_PATTERN.exec(text?.trim().toLowerCase() ?? "");
  const [, domain, service] = match ?? [];
  return domain && service ? { domain, service } : undefined;
}

export type ServiceData =
  | { ok: true; data: JsonRecord | undefined }
  | { ok: false; reason: "not_json" | "not_an_object" };

/** The optional JSON object a service call carries. Empty text means none. */
export function parseServiceData(text: string | undefined): ServiceData {
  const raw = text?.trim() ?? "";
  if (raw === "") {
    return { ok: true, data: undefined };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ok: false, reason: "not_json" };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, reason: "not_an_object" };
  }
  // Checked above: a non-null, non-array object parsed from JSON.
  return { ok: true, data: parsed as JsonRecord };
}

/**
 * The entity domains worth offering for a service: its own domain, or every
 * domain (`undefined`) when the service is generic or not typed in yet.
 */
export function domainsForService(text: string | undefined): string[] | undefined {
  const service = parseService(text);
  if (!service || service.domain === GENERIC_SERVICE_DOMAIN) {
    return undefined;
  }
  return [service.domain];
}
