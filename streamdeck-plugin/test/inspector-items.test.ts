import assert from "node:assert/strict";
import { test } from "node:test";
import type { ChannelStatus } from "../src/channel/client.ts";
import { type ConnectionStatus, describeAppStatus } from "../src/connection/status.ts";
import {
  actionItems,
  entityDomainsFor,
  entityItems,
  parseActionId,
  readInspectorRequest,
  statusMessage,
  TOGGLE_DOMAINS,
} from "../src/inspector-items.ts";
import { ACTION_UUID } from "../src/plugin-info.ts";

const READY: ChannelStatus = { state: "ready", app: "0.9.0", ha: true };
const DOWN: ChannelStatus = { state: "down", reason: "unavailable" };

function status(channel: ChannelStatus): ConnectionStatus {
  return {
    mode: "direct",
    active: "direct",
    channel,
    directConfigured: true,
    directProblem: undefined,
  };
}

test("a property inspector request is read from its event", () => {
  assert.equal(readInspectorRequest({ event: "getStatus" }), "getStatus");
  assert.equal(readInspectorRequest({ event: "getEntities", isRefresh: true }), "getEntities");
  assert.equal(readInspectorRequest({ event: "getActions" }), "getActions");
});

test("anything else from a property inspector is not a request", () => {
  for (const payload of [null, undefined, "getStatus", 7, [], {}, { event: "deleteEverything" }]) {
    assert.equal(readInspectorRequest(payload), undefined);
  }
  assert.equal(readInspectorRequest({ event: 7 }), undefined);
});

test("each action offers the entities that fit it", () => {
  assert.equal(entityDomainsFor(ACTION_UUID.toggleEntity, {}), TOGGLE_DOMAINS);
  assert.deepEqual(entityDomainsFor(ACTION_UUID.activateScene, {}), ["scene"]);
  assert.deepEqual(entityDomainsFor(ACTION_UUID.callService, { service: "light.turn_on" }), [
    "light",
  ]);
  assert.equal(entityDomainsFor(ACTION_UUID.callService, {}), undefined);
  assert.equal(entityDomainsFor(ACTION_UUID.callService, { service: 7 }), undefined);
});

test("entities become options that show the name and the id", () => {
  assert.deepEqual(
    entityItems([
      { entityId: "light.desk", name: "Desk", state: "on" },
      { entityId: "light.unnamed", name: "light.unnamed", state: "off" },
    ]),
    [
      { value: "light.desk", label: "Desk (light.desk)" },
      { value: "light.unnamed", label: "light.unnamed" },
    ],
  );
});

test("Deskmate actions are grouped into hotkeys and tray actions", () => {
  assert.deepEqual(
    actionItems([
      { id: "tray:pc_sleep", name: "Sleep" },
      { id: "hotkey:lights", name: "Lights" },
      { id: "hotkey:movie", name: "Movie" },
    ]),
    [
      {
        label: "Hotkeys",
        children: [
          { value: "hotkey:lights", label: "Lights" },
          { value: "hotkey:movie", label: "Movie" },
        ],
      },
      { label: "Tray actions", children: [{ value: "tray:pc_sleep", label: "Sleep" }] },
    ],
  );
});

test("a kind with no action gets no heading, an unknown kind is listed plainly", () => {
  assert.deepEqual(actionItems([]), []);
  assert.deepEqual(
    actionItems([
      { id: "macro:build", name: "Build" },
      { id: "tray:lock", name: "" },
    ]),
    [
      { label: "Tray actions", children: [{ value: "tray:lock", label: "tray:lock" }] },
      { value: "macro:build", label: "Build" },
    ],
  );
});

test("an action id is whatever was picked, unless it is empty or absurdly long", () => {
  assert.equal(parseActionId("hotkey:lights"), "hotkey:lights");
  assert.equal(parseActionId("  tray:pc_sleep "), "tray:pc_sleep");
  assert.equal(parseActionId(undefined), undefined);
  assert.equal(parseActionId("   "), undefined);
  assert.equal(parseActionId("x".repeat(200)), "x".repeat(200));
  assert.equal(parseActionId("x".repeat(201)), undefined);
});

test("the status message carries the text and an empty hint when there is none", () => {
  const message = statusMessage(
    {
      mode: "auto",
      active: "deskmate",
      channel: READY,
      directConfigured: false,
      directProblem: undefined,
    },
    ACTION_UUID.toggleEntity,
  );
  assert.deepEqual(message, {
    event: "status",
    mode: "auto",
    active: "deskmate",
    text: "Connected through the Deskmate app.",
    hint: "",
  });
});

test("a Deskmate Action key is told about the app, whatever the connection setting", () => {
  assert.deepEqual(statusMessage(status(READY), ACTION_UUID.deskmateAction), {
    event: "status",
    mode: "deskmate",
    active: "deskmate",
    text: "Connected to the Deskmate app.",
    hint: "",
  });
  assert.deepEqual(statusMessage(status(DOWN), ACTION_UUID.deskmateAction), {
    event: "status",
    mode: "deskmate",
    active: "none",
    text: "Not connected to the Deskmate app.",
    hint: "Open Deskmate, go to Settings and turn on Stream Deck.",
  });
  // The same status for a Home Assistant key describes the Direct connection.
  assert.equal(
    statusMessage(status(DOWN), ACTION_UUID.toggleEntity).text,
    "Using a direct connection to Home Assistant.",
  );
});

test("the app status asks for an update when the versions differ", () => {
  assert.match(
    describeAppStatus({ state: "down", reason: "version" }).hint ?? "",
    /Update both\.$/,
  );
  assert.equal(describeAppStatus({ state: "idle" }).text, "Not connected to the Deskmate app.");
});
