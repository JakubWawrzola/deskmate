import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ChannelClient,
  ChannelError,
  type ChannelStatus,
  REQUEST_TIMEOUT_MS,
} from "../src/channel/client.ts";
import { MAX_LINE_BYTES } from "../src/channel/framing.ts";
import type { EntityState } from "../src/channel/protocol.ts";
import { FakeClock, FakeSocket, settle } from "./helpers.ts";

const PIPE_PATH = "test-pipe";

type Harness = {
  client: ChannelClient;
  clock: FakeClock;
  sockets: FakeSocket[];
  statuses: ChannelStatus[];
  states: EntityState[];
  /** The socket of the latest connection attempt. */
  socket: () => FakeSocket;
};

function createHarness(): Harness {
  const clock = new FakeClock();
  const sockets: FakeSocket[] = [];
  const statuses: ChannelStatus[] = [];
  const states: EntityState[] = [];
  const client = new ChannelClient({
    path: PIPE_PATH,
    version: "0.2.0",
    schedule: clock.schedule,
    connect: (path) => {
      assert.equal(path, PIPE_PATH);
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
  });
  client.onStatus((status) => statuses.push(status));
  client.onState((state) => states.push(state));
  const socket = () => {
    const latest = sockets.at(-1);
    assert.ok(latest, "no connection was attempted");
    return latest;
  };
  return { client, clock, sockets, statuses, states, socket };
}

/** Opens the latest socket and answers its `hello`. */
async function acceptHello(harness: Harness, ha = true): Promise<void> {
  const socket = harness.socket();
  socket.open();
  socket.reply({ id: socket.lastSent.id, ok: true, v: 1, app: "0.9.0", ha });
  await settle();
}

async function connect(harness: Harness): Promise<void> {
  harness.client.setWanted(true);
  await acceptHello(harness);
}

function rejectionCode(promise: Promise<unknown>): Promise<string> {
  return promise.then(
    () => "resolved",
    (error: unknown) =>
      error instanceof ChannelError ? error.code : `not a ChannelError: ${error}`,
  );
}

test("nothing connects until the channel is wanted", () => {
  const harness = createHarness();
  assert.equal(harness.sockets.length, 0);
  assert.deepEqual(harness.client.status, { state: "idle" });
});

test("hello is the first request and names the client, its version and the protocol", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  assert.deepEqual(harness.client.status, { state: "connecting" });
  harness.socket().open();
  assert.deepEqual(harness.socket().sent, [
    { id: 1, op: "hello", v: 1, client: "streamdeck", version: "0.2.0" },
  ]);

  harness.socket().reply({ id: 1, ok: true, v: 1, app: "0.9.0", ha: true });
  await settle();
  assert.deepEqual(harness.client.status, { state: "ready", app: "0.9.0", ha: true });
  assert.deepEqual(harness.statuses, [
    { state: "connecting" },
    { state: "ready", app: "0.9.0", ha: true },
  ]);
});

test("hello reports whether the app has Home Assistant configured", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  await acceptHello(harness, false);
  assert.deepEqual(harness.client.status, { state: "ready", app: "0.9.0", ha: false });
});

test("a request waits for hello before it is sent", async () => {
  const harness = createHarness();
  const actions = harness.client.request("actions");
  const socket = harness.socket();
  socket.open();
  assert.deepEqual(
    socket.sent.map((message) => message.op),
    ["hello"],
  );

  socket.reply({ id: 1, ok: true, v: 1, app: "0.9.0", ha: true });
  await settle();
  assert.deepEqual(socket.lastSent, { id: 2, op: "actions" });
  socket.reply({ id: 2, ok: true, actions: [] });
  assert.deepEqual(await actions, { id: 2, ok: true, actions: [] });
});

test("each response goes to the request with the same id, whatever the order", async () => {
  const harness = createHarness();
  await connect(harness);
  const first = harness.client.request("run", { action: "hotkey:lights" });
  const second = harness.client.request("toggle", { entity_id: "light.desk" });
  await settle();
  const [, runRequest, toggleRequest] = harness.socket().sent;
  assert.deepEqual(runRequest, { id: 2, op: "run", action: "hotkey:lights" });
  assert.deepEqual(toggleRequest, { id: 3, op: "toggle", entity_id: "light.desk" });

  harness.socket().reply({ id: 3, ok: true, which: "toggle" });
  harness.socket().reply({ id: 2, ok: true, which: "run" });
  assert.equal((await first).which, "run");
  assert.equal((await second).which, "toggle");
});

test("two responses in one chunk and one response in two chunks both arrive", async () => {
  const harness = createHarness();
  await connect(harness);
  const first = harness.client.request("actions");
  const second = harness.client.request("entities");
  const third = harness.client.request("actions");
  await settle();

  harness.socket().receive('{"id":2,"ok":true,"n":1}\n{"id":3,"ok":true,"n":2}\n{"id":4,"ok"');
  harness.socket().receive(':true,"n":3}\n');
  assert.equal((await first).n, 1);
  assert.equal((await second).n, 2);
  assert.equal((await third).n, 3);
});

test("a failure rejects with the error code and the message for the user", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:gone" });
  await settle();
  harness.socket().reply({
    id: 2,
    ok: false,
    error: "unknown_action",
    message: "That action no longer exists",
  });
  await assert.rejects(run, (error) => {
    assert.ok(error instanceof ChannelError);
    assert.equal(error.code, "unknown_action");
    assert.equal(error.message, "That action no longer exists");
    return true;
  });
});

test("a response with an id nobody waits for is ignored", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  harness.socket().reply({ id: 99, ok: true });
  harness.socket().reply({ ok: false, error: "bad_request" });
  harness.socket().reply({ note: "neither a response nor an event" });
  harness.socket().reply({ id: 2, ok: true });
  assert.deepEqual(await run, { id: 2, ok: true });
  assert.equal(harness.socket().destroyed, false);
});

test("a request without a response times out after ten seconds", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();

  harness.clock.advance(REQUEST_TIMEOUT_MS - 1);
  const early = await Promise.race([rejectionCode(run), settle().then(() => "pending")]);
  assert.equal(early, "pending");

  harness.clock.advance(1);
  assert.equal(await rejectionCode(run), "timeout");
  // The connection stays: one slow answer is no reason to drop the others.
  assert.equal(harness.socket().destroyed, false);
  assert.equal(harness.client.status.state, "ready");
});

test("a response that arrives after the timeout is ignored", async () => {
  const harness = createHarness();
  await connect(harness);
  const slow = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  harness.clock.advance(REQUEST_TIMEOUT_MS);
  assert.equal(await rejectionCode(slow), "timeout");

  harness.socket().reply({ id: 2, ok: true });
  const next = harness.client.request("actions");
  await settle();
  harness.socket().reply({ id: 3, ok: true, actions: [] });
  assert.deepEqual(await next, { id: 3, ok: true, actions: [] });
});

test("an answered request leaves no timer behind", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  harness.socket().reply({ id: 2, ok: true });
  await run;
  assert.deepEqual(harness.clock.waiting, []);
});

test("a request over the line limit is refused without being sent", async () => {
  const harness = createHarness();
  await connect(harness);
  const call = harness.client.request("call", {
    service: "notify.send",
    data: { message: "x".repeat(MAX_LINE_BYTES) },
  });
  assert.equal(await rejectionCode(call), "too_large");
  assert.equal(harness.socket().sent.length, 1);
  assert.equal(harness.socket().destroyed, false);
});

test("a request fails at once when the pipe is not there", async () => {
  const harness = createHarness();
  const run = harness.client.request("run", { action: "hotkey:lights" });
  harness.socket().fail();
  assert.equal(await rejectionCode(run), "not_connected");
  // Nothing asked for the channel to be kept, so there is no retry.
  assert.deepEqual(harness.clock.waiting, []);
  assert.deepEqual(harness.client.status, { state: "idle" });
});

test("a connect function that throws counts as a failed attempt", async () => {
  const clock = new FakeClock();
  const client = new ChannelClient({
    path: PIPE_PATH,
    version: "0.2.0",
    schedule: clock.schedule,
    connect: () => {
      throw new Error("no pipes on this system");
    },
  });
  client.setWanted(true);
  assert.deepEqual(client.status, { state: "down", reason: "unavailable" });
  assert.deepEqual(clock.waiting, [1000]);
  assert.equal(await rejectionCode(client.request("actions")), "not_connected");
});

test("invalid JSON from the app closes the connection", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  const socket = harness.socket();
  socket.receive("this is not json\n");
  assert.equal(socket.destroyed, true);
  assert.equal(await rejectionCode(run), "disconnected");
  assert.deepEqual(harness.client.status, { state: "down", reason: "protocol" });
});

test("a line that is JSON but not an object closes the connection", async () => {
  const harness = createHarness();
  await connect(harness);
  harness.socket().receive("[1,2,3]\n");
  assert.equal(harness.socket().destroyed, true);
});

test("an oversize line from the app closes the connection", async () => {
  const harness = createHarness();
  await connect(harness);
  const socket = harness.socket();
  socket.receive("x".repeat(MAX_LINE_BYTES + 1));
  assert.equal(socket.destroyed, true);
  assert.deepEqual(harness.client.status, { state: "down", reason: "protocol" });
});

test("lines after the one that closed the connection are not processed", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  harness.socket().receive('garbage\n{"id":2,"ok":true}\n');
  assert.equal(await rejectionCode(run), "disconnected");
});

test("the app refusing the protocol version is reported as such", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().open();
  harness.socket().reply({ id: 1, ok: false, error: "version" });
  await settle();
  assert.equal(harness.socket().destroyed, true);
  assert.deepEqual(harness.client.status, { state: "down", reason: "version" });
});

test("a version refusal without an id is matched to hello", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().open();
  harness.socket().reply({ ok: false, error: "version" });
  await settle();
  assert.deepEqual(harness.client.status, { state: "down", reason: "version" });
});

test("a hello answered with another version is refused, never downgraded", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().open();
  harness.socket().reply({ id: 1, ok: true, v: 2, app: "1.0.0", ha: true });
  await settle();
  assert.equal(harness.socket().destroyed, true);
  assert.deepEqual(harness.client.status, { state: "down", reason: "version" });
});

test("a malformed hello closes the connection", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().open();
  harness.socket().reply({ id: 1, ok: true, v: 1, app: "0.9.0" });
  await settle();
  assert.equal(harness.socket().destroyed, true);
  assert.deepEqual(harness.client.status, { state: "down", reason: "protocol" });
});

test("a hello that is never answered ends the attempt after the timeout", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  const waiting = rejectionCode(harness.client.ensureConnected());
  harness.socket().open();
  harness.clock.advance(REQUEST_TIMEOUT_MS);
  await settle();
  assert.equal(harness.socket().destroyed, true);
  assert.equal(await waiting, "not_connected");
  assert.deepEqual(harness.client.status, { state: "down", reason: "unavailable" });
  assert.deepEqual(harness.clock.waiting, [1000]);
});

test("reconnect waits one second, then doubles up to thirty", () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  const delays: number[] = [];
  for (let attempt = 0; attempt < 8; attempt += 1) {
    harness.socket().fail();
    const [delay] = harness.clock.waiting;
    assert.ok(delay !== undefined);
    delays.push(delay);
    harness.clock.advance(delay);
  }
  assert.deepEqual(delays, [1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
  // One attempt at the start plus one after every wait.
  assert.equal(harness.sockets.length, 9);
});

test("no attempt is made before the wait is over", () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().fail();
  harness.clock.advance(999);
  assert.equal(harness.sockets.length, 1);
  harness.clock.advance(1);
  assert.equal(harness.sockets.length, 2);
});

test("the status says down once, not at every failed attempt", () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  for (let attempt = 0; attempt < 4; attempt += 1) {
    harness.socket().fail();
    harness.clock.advance(30000);
  }
  assert.deepEqual(harness.statuses, [
    { state: "connecting" },
    { state: "down", reason: "unavailable" },
  ]);
});

test("the delay starts over after a connection that worked", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().fail();
  harness.clock.advance(1000);
  harness.socket().fail();
  assert.deepEqual(harness.clock.waiting, [2000]);
  harness.clock.advance(2000);
  await acceptHello(harness);
  assert.equal(harness.client.status.state, "ready");

  harness.socket().close();
  assert.deepEqual(harness.client.status, { state: "down", reason: "unavailable" });
  assert.deepEqual(harness.clock.waiting, [1000]);
});

test("a dropped connection rejects what was waiting and reconnects", async () => {
  const harness = createHarness();
  await connect(harness);
  const run = harness.client.request("run", { action: "hotkey:lights" });
  await settle();
  harness.socket().close();
  assert.equal(await rejectionCode(run), "disconnected");
  assert.deepEqual(harness.clock.waiting, [1000]);

  harness.clock.advance(1000);
  assert.equal(harness.sockets.length, 2);
  await acceptHello(harness);
  assert.equal(harness.client.status.state, "ready");
});

test("a request during the wait connects at once instead of waiting it out", async () => {
  const harness = createHarness();
  harness.client.setWanted(true);
  harness.socket().fail();
  assert.deepEqual(harness.clock.waiting, [1000]);

  const actions = harness.client.request("actions");
  assert.equal(harness.sockets.length, 2);
  assert.deepEqual(harness.clock.waiting, []);
  await acceptHello(harness);
  harness.socket().reply({ id: 2, ok: true, actions: [] });
  assert.deepEqual(await actions, { id: 2, ok: true, actions: [] });
});

test("several callers waiting for the connection share one attempt", async () => {
  const harness = createHarness();
  const first = harness.client.ensureConnected();
  const second = harness.client.ensureConnected();
  assert.equal(harness.sockets.length, 1);
  await acceptHello(harness);
  await Promise.all([first, second]);
});

test("the channel no longer being wanted closes it and stops the retries", async () => {
  const harness = createHarness();
  await connect(harness);
  harness.client.setWanted(false);
  assert.equal(harness.socket().destroyed, true);
  assert.deepEqual(harness.client.status, { state: "idle" });
  assert.deepEqual(harness.clock.waiting, []);

  const retrying = createHarness();
  retrying.client.setWanted(true);
  retrying.socket().fail();
  assert.deepEqual(retrying.clock.waiting, [1000]);
  retrying.client.setWanted(false);
  assert.deepEqual(retrying.clock.waiting, []);
  assert.deepEqual(retrying.client.status, { state: "idle" });
  retrying.clock.advance(60000);
  assert.equal(retrying.sockets.length, 1);
});

test("events from a connection that was given up are ignored", async () => {
  const harness = createHarness();
  await connect(harness);
  const old = harness.socket();
  harness.client.setWanted(false);
  old.reply({ ev: "state", entity_id: "light.desk", state: "on" });
  old.close();
  assert.deepEqual(harness.states, []);
  assert.deepEqual(harness.client.status, { state: "idle" });
});

test("the watch list is sent once connected and its states are reported", async () => {
  const harness = createHarness();
  harness.client.setWatch(["light.desk", "switch.fan"]);
  await connect(harness);
  assert.deepEqual(harness.socket().lastSent, {
    id: 2,
    op: "watch",
    entities: ["light.desk", "switch.fan"],
  });

  harness.socket().reply({
    id: 2,
    ok: true,
    states: { "light.desk": "on", "switch.fan": "unavailable" },
  });
  await settle();
  assert.deepEqual(harness.states, [
    { entityId: "light.desk", state: "on" },
    { entityId: "switch.fan", state: "unavailable" },
  ]);
});

test("state events are reported as they arrive", async () => {
  const harness = createHarness();
  await connect(harness);
  harness.socket().reply({ ev: "state", entity_id: "light.desk", state: "off" });
  harness.socket().reply({ ev: "state", entity_id: "light.desk" });
  harness.socket().reply({ ev: "something_new", entity_id: "light.desk", state: "on" });
  assert.deepEqual(harness.states, [{ entityId: "light.desk", state: "off" }]);
  assert.equal(harness.socket().destroyed, false);
});

test("a new watch list replaces the old one", async () => {
  const harness = createHarness();
  await connect(harness);
  harness.client.setWatch(["light.desk"]);
  harness.socket().reply({ id: 2, ok: true, states: { "light.desk": "on" } });
  await settle();

  harness.client.setWatch(["switch.fan"]);
  assert.deepEqual(harness.socket().lastSent, { id: 3, op: "watch", entities: ["switch.fan"] });
  harness.client.setWatch([]);
  harness.socket().reply({ id: 3, ok: true, states: { "switch.fan": "off" } });
  await settle();
  assert.deepEqual(harness.socket().lastSent, { id: 4, op: "watch", entities: [] });
  // The answer for a list that was already replaced is not reported.
  assert.deepEqual(harness.states, [{ entityId: "light.desk", state: "on" }]);
});

test("an unchanged watch list is not sent again", async () => {
  const harness = createHarness();
  await connect(harness);
  harness.client.setWatch(["light.desk", "light.desk"]);
  harness.client.setWatch(["light.desk"]);
  assert.deepEqual(
    harness.socket().sent.map((message) => message.op),
    ["hello", "watch"],
  );
  assert.deepEqual(harness.socket().lastSent.entities, ["light.desk"]);
});

test("the watch list is sent again after a reconnect", async () => {
  const harness = createHarness();
  harness.client.setWatch(["light.desk"]);
  await connect(harness);
  harness.socket().reply({ id: 2, ok: true, states: { "light.desk": "on" } });
  await settle();

  harness.socket().close();
  harness.clock.advance(1000);
  await acceptHello(harness);
  assert.equal(harness.sockets.length, 2);
  assert.deepEqual(harness.socket().sent, [
    { id: 1, op: "hello", v: 1, client: "streamdeck", version: "0.2.0" },
    { id: 2, op: "watch", entities: ["light.desk"] },
  ]);
});

test("an empty watch list is not sent on a fresh connection", async () => {
  const harness = createHarness();
  await connect(harness);
  assert.deepEqual(
    harness.socket().sent.map((message) => message.op),
    ["hello"],
  );
});

test("at most 64 entities are watched", async () => {
  const harness = createHarness();
  await connect(harness);
  const many = Array.from({ length: 70 }, (_, index) => `light.lamp_${index}`);
  harness.client.setWatch(many);
  const entities = harness.socket().lastSent.entities;
  assert.ok(Array.isArray(entities));
  assert.equal(entities.length, 64);
  assert.deepEqual(entities, many.slice(0, 64));
});

test("malformed watch states close the connection", async () => {
  const harness = createHarness();
  harness.client.setWatch(["light.desk"]);
  await connect(harness);
  harness.socket().reply({ id: 2, ok: true, states: ["on"] });
  await settle();
  assert.equal(harness.socket().destroyed, true);
});

test("a refused watch leaves the connection up", async () => {
  const harness = createHarness();
  harness.client.setWatch(["light.desk"]);
  await connect(harness);
  harness.socket().reply({ id: 2, ok: false, error: "ha_not_configured" });
  await settle();
  assert.equal(harness.socket().destroyed, false);
  assert.equal(harness.client.status.state, "ready");
  assert.deepEqual(harness.states, []);
});
