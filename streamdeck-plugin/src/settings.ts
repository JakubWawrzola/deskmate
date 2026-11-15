/** How the plugin reaches Home Assistant. */
export type ConnectionMode = "auto" | "deskmate" | "direct";

export const DEFAULT_CONNECTION_MODE: ConnectionMode = "auto";

/**
 * The plugin's global settings, as the property inspectors store them. Every
 * field can be missing or hold anything: the values are checked when read.
 */
export type GlobalSettings = {
  connection?: string;
  haUrl?: string;
  haToken?: string;
};

/** Where and how the Direct connection logs in. */
export type DirectConfig = {
  /** Scheme, host, port and an optional path prefix, without a trailing slash. */
  baseUrl: string;
  token: string;
};

export function parseConnectionMode(value: unknown): ConnectionMode {
  return value === "deskmate" || value === "direct" || value === "auto"
    ? value
    : DEFAULT_CONNECTION_MODE;
}

/**
 * The Direct connection's address and token, or `undefined` when either is
 * missing or the address is not an http or https URL.
 */
export function parseDirectConfig(settings: GlobalSettings): DirectConfig | undefined {
  const rawUrl = typeof settings.haUrl === "string" ? settings.haUrl.trim() : "";
  const token = typeof settings.haToken === "string" ? settings.haToken.trim() : "";
  if (rawUrl === "" || token === "") {
    return undefined;
  }
  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return undefined;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return undefined;
  }
  const path = url.pathname.replace(/\/+$/, "");
  return { baseUrl: `${url.origin}${path}`, token };
}
