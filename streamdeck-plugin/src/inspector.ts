import streamDeck from "@elgato/streamdeck";
import type { ConnectionHub } from "./connection/hub.ts";
import { errorCode } from "./errors.ts";
import {
  actionItems,
  entityDomainsFor,
  entityItems,
  type InspectorGroup,
  type InspectorItem,
  readInspectorRequest,
  statusMessage,
} from "./inspector-items.ts";
import { ACTION_UUID } from "./plugin-info.ts";

type InspectorLog = { warn(message: string): void };

/** The part of a visible action the inspector code needs. */
type InspectedAction = {
  readonly manifestId: string;
  getSettings(): Promise<{ service?: unknown }>;
};

/**
 * Answers the property inspectors: which connection is in use, which
 * entities can be picked and which Deskmate actions exist. All of it is sent
 * again whenever the connection changes, so an open inspector follows along.
 */
export function connectInspector(hub: ConnectionHub, log: InspectorLog): void {
  const sendStatus = (action: InspectedAction) =>
    streamDeck.ui.sendToPropertyInspector(statusMessage(hub.status, action.manifestId));

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

  const sendActions = async () => {
    let items: (InspectorItem | InspectorGroup)[] = [];
    try {
      items = actionItems(await hub.listActions());
    } catch (error) {
      log.warn(`Listing Deskmate actions failed: ${errorCode(error)}`);
    }
    await streamDeck.ui.sendToPropertyInspector({ event: "getActions", items });
  };

  /** The list the inspector of this action shows. */
  const sendList = (action: InspectedAction) =>
    action.manifestId === ACTION_UUID.deskmateAction ? sendActions() : sendEntities(action);

  streamDeck.ui.onSendToPlugin((ev) => {
    const request = readInspectorRequest(ev.payload);
    if (request === "getStatus") {
      void sendStatus(ev.action);
    } else if (request === "getEntities") {
      void sendEntities(ev.action);
    } else if (request === "getActions") {
      void sendActions();
    }
  });

  hub.onStatus(() => {
    const inspected = streamDeck.ui.action;
    if (inspected) {
      void sendStatus(inspected);
      void sendList(inspected);
    }
  });
}
