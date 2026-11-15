import type { ChannelStatus } from "../channel/client.ts";
import type { ConnectionMode } from "../settings.ts";
import type { Selection } from "./select.ts";
import type { DirectProblem } from "./transport.ts";

export type ConnectionStatus = {
  mode: ConnectionMode;
  active: Selection;
  channel: ChannelStatus;
  directConfigured: boolean;
  directProblem: DirectProblem | undefined;
};

/** What a property inspector shows about the connection. */
export type StatusText = {
  /** Which connection is in use. */
  text: string;
  /** What to do when something is missing. */
  hint: string | undefined;
};

const OPEN_DESKMATE = "Open Deskmate, go to Settings and turn on Stream Deck";
const ENTER_ADDRESS = "enter a Home Assistant address and token here";
const UPDATE_BOTH =
  "This plugin and the Deskmate app use different versions of their connection. Update both.";

function sentence(text: string): string {
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}.`;
}

function channelHint(channel: ChannelStatus): string {
  return channel.state === "down" && channel.reason === "version"
    ? UPDATE_BOTH
    : sentence(OPEN_DESKMATE);
}

/** Says, in plain English, which connection is in use and what is missing. */
export function describeStatus(status: ConnectionStatus): StatusText {
  const { mode, active, channel } = status;

  if (active === "deskmate") {
    if (channel.state !== "ready") {
      return { text: "Not connected to the Deskmate app.", hint: channelHint(channel) };
    }
    return {
      text: "Connected through the Deskmate app.",
      hint: channel.ha
        ? undefined
        : mode === "auto"
          ? `Deskmate has no Home Assistant address and token yet. Add them in Deskmate, or ${ENTER_ADDRESS}.`
          : "Deskmate has no Home Assistant address and token yet. Add them in Deskmate.",
    };
  }

  if (active === "direct") {
    return {
      text: "Using a direct connection to Home Assistant.",
      hint:
        status.directProblem === "auth"
          ? "Home Assistant refused the access token. Create a new one and paste it here."
          : undefined,
    };
  }

  if (mode === "direct") {
    return { text: "Not connected.", hint: sentence(ENTER_ADDRESS) };
  }
  if (channel.state === "down" && channel.reason === "version") {
    return { text: "Not connected.", hint: `${UPDATE_BOTH} Or ${ENTER_ADDRESS}.` };
  }
  return { text: "Not connected.", hint: `${OPEN_DESKMATE}, or ${ENTER_ADDRESS}.` };
}

/**
 * The same for a Deskmate Action key. Only the app can run its actions, so
 * the connection setting and the Direct connection do not come into it.
 */
export function describeAppStatus(channel: ChannelStatus): StatusText {
  return channel.state === "ready"
    ? { text: "Connected to the Deskmate app.", hint: undefined }
    : { text: "Not connected to the Deskmate app.", hint: channelHint(channel) };
}
