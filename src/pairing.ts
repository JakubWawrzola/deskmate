/** Pairing code from the Home Assistant integration: "DMP1." + base64url JSON
 * with the key ("k") and the local ("u") and remote ("r") WebSocket addresses.
 * One paste fills every connection field, so a mistyped address can no longer
 * look exactly like a wrong key.
 *
 * A code made for another Windows account on an already paired computer also
 * carries that computer's node id ("n") and, when enabled, the cascade key
 * ("c"), so the second account joins the same Home Assistant device. */
export interface PairingCode {
  key: string;
  url?: string;
  urlRemote?: string;
  node?: string;
  cascadeKey?: string;
}

const PREFIX = "DMP1.";
const NODE = /^[a-z0-9_]{1,64}$/;

export function parsePairingCode(input: string): PairingCode | null {
  const text = input.trim();
  if (!text.startsWith(PREFIX)) return null;
  try {
    let body = text.slice(PREFIX.length).replace(/-/g, "+").replace(/_/g, "/");
    while (body.length % 4) body += "=";
    const data: unknown = JSON.parse(atob(body));
    if (typeof data !== "object" || data === null) return null;
    const record = data as Record<string, unknown>;
    if (typeof record.k !== "string") return null;
    const url = typeof record.u === "string" ? record.u : undefined;
    const urlRemote = typeof record.r === "string" ? record.r : undefined;
    const node = typeof record.n === "string" && NODE.test(record.n) ? record.n : undefined;
    const cascadeKey = typeof record.c === "string" && record.c ? record.c : undefined;
    // Only a remote address known: use it as the primary one.
    return url
      ? { key: record.k, url, urlRemote, node, cascadeKey }
      : { key: record.k, url: urlRemote, node, cascadeKey };
  } catch {
    return null;
  }
}
