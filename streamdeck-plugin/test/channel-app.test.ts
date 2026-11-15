import assert from "node:assert/strict";
import { connect } from "node:net";
import { test } from "node:test";
import { ChannelClient, ChannelError } from "../src/channel/client.ts";
import { PIPE_PATH_VARIABLE } from "../src/channel/pipe-name.ts";
import { type EntityState, readActions, readEntities } from "../src/channel/protocol.ts";

// The pipe of the Deskmate app's own channel server, started with stand-ins
// for Home Assistant and for the actions by the app's test
// `the_plugin_client_talks_to_the_channel`:
//
//   cd src-tauri
//   cargo test -p deskmate plugin_client -- --ignored
//
// That test runs this file with the variable set. Without it there is no
// server to talk to and the test is skipped. It is never the pipe of a real
// Deskmate: the stand-ins are what the assertions below expect.
const path = process.env[PIPE_PATH_VARIABLE]?.trim();

function waitFor(condition: () => boolean, what: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + 10_000;
    const check = () => {
      if (condition()) {
        resolve();
      } else if (Date.now() > deadline) {
        reject(new Error(`Timed out waiting for ${what}`));
      } else {
        setTimeout(check, 10);
      }
    };
    check();
  });
}

test("the client talks to the Deskmate app's channel server", {
  skip: path ? false : `${PIPE_PATH_VARIABLE} does not name a channel server with stand-ins`,
}, async () => {
  assert.ok(path);
  const states: EntityState[] = [];
  const client = new ChannelClient({
    path,
    version: "0.2.0",
    connect: (target) => connect(target),
  });
  client.onState((state) => states.push(state));
  try {
    client.setWatch(["light.desk"]);
    client.setWanted(true);
    await client.ensureConnected();
    assert.deepEqual(client.status, { state: "ready", app: "0.9.0", ha: true });

    // The answer to `watch`, which the client sent by itself after `hello`.
    await waitFor(() => states.length === 1, "the watch states");
    assert.deepEqual(states[0], { entityId: "light.desk", state: "on" });

    assert.deepEqual(readActions(await client.request("actions")), [
      { id: "hotkey:lights", name: "Lights" },
      { id: "tray:pc_sleep", name: "Sleep" },
      { id: "tray:drop_connections", name: "Drop connections" },
    ]);
    await client.request("run", { action: "hotkey:lights" });
    await assert.rejects(
      client.request("run", { action: "hotkey:missing" }),
      (error) => error instanceof ChannelError && error.code === "unknown_action",
    );
    assert.deepEqual(readEntities(await client.request("entities", { domains: ["light"] })), [
      { entityId: "light.desk", name: "Desk", state: "on" },
    ]);
    assert.equal(readEntities(await client.request("entities")).length, 3);
    await client.request("call", { service: "scene.turn_on", entity_id: "scene.movie", data: {} });
    await assert.rejects(
      client.request("no_such_op"),
      (error) => error instanceof ChannelError && error.code === "bad_request",
    );
    await assert.rejects(
      client.request("toggle", { entity_id: "not an entity" }),
      (error) => error instanceof ChannelError && error.code === "bad_request",
    );

    // The stand-in for Home Assistant flips the entity; the app reads it
    // again and sends the event.
    await client.request("toggle", { entity_id: "light.desk" });
    await waitFor(() => states.length === 2, "the state event");
    assert.deepEqual(states[1], { entityId: "light.desk", state: "off" });

    // The app closes its connections, as it does when its Home Assistant
    // settings change. The answer to this request may be lost with the
    // connection. The client comes back by itself after the first,
    // one-second wait and is told the states of its watch list again.
    await client.request("run", { action: "tray:drop_connections" }).catch(() => {});
    await waitFor(() => client.status.state !== "ready", "the client to notice the drop");
    await waitFor(() => client.status.state === "ready", "the reconnect");
    await waitFor(() => states.length === 3, "the watch states after the reconnect");
    assert.deepEqual(states[2], { entityId: "light.desk", state: "off" });
  } finally {
    client.setWanted(false);
  }
});
