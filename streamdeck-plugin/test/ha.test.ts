import assert from "node:assert/strict";
import { test } from "node:test";
import {
  entitiesFromStates,
  readEntity,
  readSocketMessage,
  webSocketUrl,
} from "../src/connection/ha-api.ts";
import {
  domainsForService,
  parseEntityId,
  parseSceneId,
  parseService,
  parseServiceData,
} from "../src/ha-ids.ts";

test("an entity id is trimmed and lowercased", () => {
  assert.equal(parseEntityId("light.desk"), "light.desk");
  assert.equal(parseEntityId("  Light.Desk_2 "), "light.desk_2");
});

test("text that is not an entity id is refused", () => {
  for (const text of [
    undefined,
    "",
    "  ",
    "desk",
    "light.",
    ".desk",
    "light.desk.lamp",
    "light desk",
  ]) {
    assert.equal(parseEntityId(text), undefined, `text: ${text}`);
  }
  assert.equal(parseEntityId("light.desk; drop"), undefined);
  assert.equal(parseEntityId("light/../desk"), undefined);
});

test("a scene can be given with or without its domain", () => {
  assert.equal(parseSceneId("scene.movie"), "scene.movie");
  assert.equal(parseSceneId("movie"), "scene.movie");
  assert.equal(parseSceneId(" Movie_Night "), "scene.movie_night");
});

test("something that is not a scene is refused", () => {
  assert.equal(parseSceneId(undefined), undefined);
  assert.equal(parseSceneId(""), undefined);
  assert.equal(parseSceneId("light.desk"), undefined);
  assert.equal(parseSceneId("scene."), undefined);
  assert.equal(parseSceneId("scene.movie.night"), undefined);
});

test("a service is read as domain and name", () => {
  assert.deepEqual(parseService("light.turn_on"), { domain: "light", service: "turn_on" });
  assert.deepEqual(parseService(" Scene.Turn_On "), { domain: "scene", service: "turn_on" });
});

test("text that is not a service is refused", () => {
  for (const text of [undefined, "", "turn_on", "light.", "light.turn on", "light.turn_on.now"]) {
    assert.equal(parseService(text), undefined, `text: ${text}`);
  }
  assert.equal(parseService("light/turn_on"), undefined);
});

test("service data is an optional JSON object", () => {
  assert.deepEqual(parseServiceData(undefined), { ok: true, data: undefined });
  assert.deepEqual(parseServiceData("   "), { ok: true, data: undefined });
  assert.deepEqual(parseServiceData('{"brightness_pct": 40}'), {
    ok: true,
    data: { brightness_pct: 40 },
  });
});

test("service data that is not a JSON object is refused", () => {
  assert.deepEqual(parseServiceData("{broken"), { ok: false, reason: "not_json" });
  assert.deepEqual(parseServiceData("[1, 2]"), { ok: false, reason: "not_an_object" });
  assert.deepEqual(parseServiceData('"text"'), { ok: false, reason: "not_an_object" });
  assert.deepEqual(parseServiceData("null"), { ok: false, reason: "not_an_object" });
});

test("a service suggests the entities of its own domain", () => {
  assert.deepEqual(domainsForService("light.turn_on"), ["light"]);
  assert.equal(domainsForService("homeassistant.toggle"), undefined);
  assert.equal(domainsForService(""), undefined);
  assert.equal(domainsForService("not a service"), undefined);
});

test("the WebSocket address follows the scheme of the base address", () => {
  assert.equal(
    webSocketUrl("http://homeassistant.local:8123"),
    "ws://homeassistant.local:8123/api/websocket",
  );
  assert.equal(webSocketUrl("https://example.com/ha"), "wss://example.com/ha/api/websocket");
});

test("a state object gives the id, the friendly name and the state", () => {
  assert.deepEqual(
    readEntity({ entity_id: "light.desk", state: "on", attributes: { friendly_name: "Desk" } }),
    { entityId: "light.desk", name: "Desk", state: "on" },
  );
  assert.deepEqual(readEntity({ entity_id: "light.desk", state: "off" }), {
    entityId: "light.desk",
    name: "light.desk",
    state: "off",
  });
});

test("something that is not a state object is refused", () => {
  for (const value of [null, undefined, "light.desk", [], {}, { entity_id: "light.desk" }]) {
    assert.equal(readEntity(value), undefined);
  }
  assert.equal(readEntity({ entity_id: 5, state: "on" }), undefined);
});

test("the state list is filtered by domain and sorted by name", () => {
  const states = [
    { entity_id: "switch.fan", state: "off", attributes: { friendly_name: "Fan" } },
    { entity_id: "light.desk", state: "on", attributes: { friendly_name: "Desk" } },
    { entity_id: "light.ceiling", state: "off", attributes: { friendly_name: "Ceiling" } },
    { entity_id: "sensor.temperature", state: "21.5" },
    "garbage",
  ];
  assert.deepEqual(
    entitiesFromStates(states, ["light"]).map((entity) => entity.entityId),
    ["light.ceiling", "light.desk"],
  );
  assert.deepEqual(
    entitiesFromStates(states, undefined).map((entity) => entity.entityId),
    ["light.ceiling", "light.desk", "switch.fan", "sensor.temperature"],
  );
  assert.deepEqual(entitiesFromStates({ not: "an array" }, undefined), []);
});

test("WebSocket messages are reduced to what the plugin uses", () => {
  assert.deepEqual(readSocketMessage('{"type":"auth_required"}'), { type: "auth_required" });
  assert.deepEqual(readSocketMessage('{"type":"auth_ok"}'), { type: "auth_ok" });
  assert.deepEqual(readSocketMessage('{"type":"auth_invalid","message":"no"}'), {
    type: "auth_invalid",
  });
  assert.deepEqual(readSocketMessage('{"type":"pong","id":4}'), { type: "pong" });
  assert.deepEqual(
    readSocketMessage(
      '{"type":"result","id":2,"success":true,"result":[{"entity_id":"light.desk","state":"on"}]}',
    ),
    {
      type: "result",
      id: 2,
      states: [{ entityId: "light.desk", name: "light.desk", state: "on" }],
    },
  );
  assert.deepEqual(readSocketMessage('{"type":"result","id":1,"success":true,"result":null}'), {
    type: "result",
    id: 1,
    states: [],
  });
});

test("a state change carries the new state, or unavailable for a removed entity", () => {
  const changed = {
    type: "event",
    event: {
      event_type: "state_changed",
      data: { entity_id: "light.desk", new_state: { entity_id: "light.desk", state: "off" } },
    },
  };
  assert.deepEqual(readSocketMessage(JSON.stringify(changed)), {
    type: "state_changed",
    entityId: "light.desk",
    state: "off",
  });
  const removed = {
    type: "event",
    event: { event_type: "state_changed", data: { entity_id: "light.desk", new_state: null } },
  };
  assert.deepEqual(readSocketMessage(JSON.stringify(removed)), {
    type: "state_changed",
    entityId: "light.desk",
    state: "unavailable",
  });
});

test("anything else from the WebSocket is ignored", () => {
  for (const raw of [
    "not json",
    "[]",
    '{"type":"result","id":2,"success":false}',
    '{"type":"event","event":{"event_type":"call_service"}}',
    '{"type":"event"}',
    '{"type":"surprise"}',
  ]) {
    assert.deepEqual(readSocketMessage(raw), { type: "other" }, `raw: ${raw}`);
  }
});
