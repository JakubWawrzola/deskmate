import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ProtocolError,
  parseServerLine,
  readActions,
  readEntities,
  readHello,
  readStateEvent,
  readStates,
} from "../src/channel/protocol.ts";

test("a successful response keeps its id and its body", () => {
  const message = parseServerLine('{"id":2,"ok":true,"actions":[]}');
  assert.deepEqual(message, {
    kind: "response",
    id: 2,
    ok: true,
    body: { id: 2, ok: true, actions: [] },
  });
});

test("a failure carries the error code and the optional message", () => {
  assert.deepEqual(parseServerLine('{"id":3,"ok":false,"error":"unknown_action"}'), {
    kind: "response",
    id: 3,
    ok: false,
    error: "unknown_action",
    message: undefined,
  });
  assert.deepEqual(
    parseServerLine('{"id":4,"ok":false,"error":"ha_error","message":"No answer"}'),
    { kind: "response", id: 4, ok: false, error: "ha_error", message: "No answer" },
  );
});

test("a failure without an id is still read as a response", () => {
  assert.deepEqual(parseServerLine('{"ok":false,"error":"version"}'), {
    kind: "response",
    id: undefined,
    ok: false,
    error: "version",
    message: undefined,
  });
});

test("a line with ev and no id is an event", () => {
  assert.deepEqual(parseServerLine('{"ev":"state","entity_id":"light.desk","state":"off"}'), {
    kind: "event",
    ev: "state",
    body: { ev: "state", entity_id: "light.desk", state: "off" },
  });
});

test("an object that is neither a response nor an event is marked unknown", () => {
  assert.deepEqual(parseServerLine('{"hello":"world"}'), { kind: "unknown" });
  assert.deepEqual(parseServerLine('{"id":"1","ok":true}'), { kind: "unknown" });
  assert.deepEqual(parseServerLine('{"id":1.5,"ok":true}'), { kind: "unknown" });
  assert.deepEqual(parseServerLine('{"id":1,"ev":"state"}'), { kind: "unknown" });
});

test("a line that is not a JSON object is refused", () => {
  for (const line of ["", "not json", '{"id":1', "[1,2]", '"text"', "42", "null", "true"]) {
    assert.throws(() => parseServerLine(line), ProtocolError, `line: ${line}`);
  }
});

test("hello is read when the version matches", () => {
  assert.deepEqual(readHello({ id: 1, ok: true, v: 1, app: "0.9.0", ha: true }), {
    app: "0.9.0",
    ha: true,
  });
});

test("hello with another version or a missing field is refused", () => {
  assert.throws(() => readHello({ v: 2, app: "0.9.0", ha: true }), ProtocolError);
  assert.throws(() => readHello({ app: "0.9.0", ha: true }), ProtocolError);
  assert.throws(() => readHello({ v: 1, ha: true }), ProtocolError);
  assert.throws(() => readHello({ v: 1, app: "0.9.0", ha: "yes" }), ProtocolError);
});

test("actions are read as id and name", () => {
  const body = {
    actions: [
      { id: "hotkey:lights", name: "Lights" },
      { id: "tray:pc_sleep", name: "Sleep" },
    ],
  };
  assert.deepEqual(readActions(body), [
    { id: "hotkey:lights", name: "Lights" },
    { id: "tray:pc_sleep", name: "Sleep" },
  ]);
});

test("a malformed action list is refused", () => {
  assert.throws(() => readActions({}), ProtocolError);
  assert.throws(() => readActions({ actions: {} }), ProtocolError);
  assert.throws(() => readActions({ actions: ["hotkey:lights"] }), ProtocolError);
  assert.throws(() => readActions({ actions: [{ id: "hotkey:lights" }] }), ProtocolError);
  assert.throws(() => readActions({ actions: [{ id: 7, name: "Lights" }] }), ProtocolError);
});

test("entities are read as id, name and state", () => {
  const body = { entities: [{ entity_id: "light.desk", name: "Desk", state: "on" }] };
  assert.deepEqual(readEntities(body), [{ entityId: "light.desk", name: "Desk", state: "on" }]);
});

test("a malformed entity list is refused", () => {
  assert.throws(() => readEntities({ entities: null }), ProtocolError);
  assert.throws(() => readEntities({ entities: [null] }), ProtocolError);
  assert.throws(
    () => readEntities({ entities: [{ entity_id: "light.desk", name: "Desk" }] }),
    ProtocolError,
  );
});

test("the states of a watch response are read per entity", () => {
  assert.deepEqual(readStates({ states: { "light.desk": "on", "switch.fan": "unavailable" } }), [
    { entityId: "light.desk", state: "on" },
    { entityId: "switch.fan", state: "unavailable" },
  ]);
  assert.deepEqual(readStates({ states: {} }), []);
});

test("malformed states are refused", () => {
  assert.throws(() => readStates({}), ProtocolError);
  assert.throws(() => readStates({ states: [] }), ProtocolError);
  assert.throws(() => readStates({ states: { "light.desk": true } }), ProtocolError);
});

test("a state event is read, a malformed one is refused", () => {
  assert.deepEqual(readStateEvent({ ev: "state", entity_id: "light.desk", state: "off" }), {
    entityId: "light.desk",
    state: "off",
  });
  assert.throws(() => readStateEvent({ ev: "state", entity_id: "light.desk" }), ProtocolError);
  assert.throws(() => readStateEvent({ ev: "state", state: "off" }), ProtocolError);
});
