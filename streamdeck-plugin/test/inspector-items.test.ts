import assert from "node:assert/strict";
import { test } from "node:test";
import {
  entityDomainsFor,
  entityItems,
  readInspectorRequest,
  statusMessage,
  TOGGLE_DOMAINS,
} from "../src/inspector-items.ts";
import { ACTION_UUID } from "../src/plugin-info.ts";

test("a property inspector request is read from its event", () => {
  assert.equal(readInspectorRequest({ event: "getStatus" }), "getStatus");
  assert.equal(readInspectorRequest({ event: "getEntities", isRefresh: true }), "getEntities");
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

test("the status message carries the text and an empty hint when there is none", () => {
  assert.deepEqual(
    statusMessage({
      mode: "auto",
      active: "deskmate",
      channel: { state: "ready", app: "0.9.0", ha: true },
      directConfigured: false,
      directProblem: undefined,
    }),
    {
      event: "status",
      mode: "auto",
      active: "deskmate",
      text: "Connected through the Deskmate app.",
      hint: "",
    },
  );
});
