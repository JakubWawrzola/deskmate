import streamDeck from "@elgato/streamdeck";
import type { ConnectionHub } from "./connection/hub.ts";
import { errorCode } from "./errors.ts";
import {
  entityDomainsFor,
  entityItems,
  type InspectorItem,
  readInspectorRequest,
  statusMessage,
} from "./inspector-items.ts";

type InspectorLog = { warn(message: string): void };

/** The part of a visible action the inspector code needs. */
type InspectedAction = {
  readonly manifestId: string;
  getSettings(): Promise<{ service?: unknown }>;
};

/**
 * Answers the property inspectors: which connection is in use, and which
 * entities can be picked. Both are sent again whenever the connection
 * changes, so an open inspector follows along.
 */
export function connectInspector(hub: ConnectionHub, log: InspectorLog): void {
  const sendStatus = () => streamDeck.ui.sendToPropertyInspector(statusMessage(hub.status));

  const sendEntities = async (action: InspectedAction) => {
    let items: InspectorItem[] = [];
    try {
      const domains = entityDomainsFor(action.manifestId, await action.getSettings());
      items = entityItems(await hub.listEntities(domains));
    } catch (error) {
      // The inspector still gets an answer: its dropdown waits for one.
      log.warn(`Listing entities failed: ${errorCode(error)}`);
    }
    await streamDeck.ui.sendToPropertyInspector({ event: "getEntities", items });
  };

  streamDeck.ui.onSendToPlugin((ev) => {
    const request = readInspectorRequest(ev.payload);
    if (request === "getStatus") {
      void sendStatus();
    } else if (request === "getEntities") {
      void sendEntities(ev.action);
    }
  });

  hub.onStatus(() => {
    void sendStatus();
    const inspected = streamDeck.ui.action;
    if (inspected) {
      void sendEntities(inspected);
    }
  });
}
