import { ChannelError } from "./channel/client.ts";
import { ConnectionError } from "./connection/transport.ts";

/**
 * A short code for the log. Only the code is written, never the error's
 * text: that can come from the app or from Home Assistant and name the
 * user's entities.
 */
export function errorCode(error: unknown): string {
  if (error instanceof ChannelError || error instanceof ConnectionError) {
    return error.code;
  }
  return error instanceof Error ? error.name : "unknown";
}
