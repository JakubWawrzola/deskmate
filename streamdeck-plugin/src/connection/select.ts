import type { ChannelStatus } from "../channel/client.ts";
import type { ConnectionMode } from "../settings.ts";

export type TransportKind = "deskmate" | "direct";

/** The transport in use, or `none` when neither can be used. */
export type Selection = TransportKind | "none";

/**
 * Picks the transport for Home Assistant.
 *
 * - `deskmate`: always the app, connected or not; nothing falls back.
 * - `direct`: the Direct connection when an address and token are set.
 * - `auto`: the app when its channel is up and it has Home Assistant set up;
 *   otherwise Direct when an address and token are set; otherwise the app
 *   when its channel is up at all, so that its answer can say what is
 *   missing; otherwise nothing.
 */
export function selectTransport(
  mode: ConnectionMode,
  channel: ChannelStatus,
  directConfigured: boolean,
): Selection {
  if (mode === "deskmate") {
    return "deskmate";
  }
  if (mode === "direct") {
    return directConfigured ? "direct" : "none";
  }
  if (channel.state === "ready" && channel.ha) {
    return "deskmate";
  }
  if (directConfigured) {
    return "direct";
  }
  return channel.state === "ready" ? "deskmate" : "none";
}

/** Whether Home Assistant keys need the app's channel in this mode. */
export function homeAssistantKeysNeedChannel(mode: ConnectionMode): boolean {
  return mode !== "direct";
}
