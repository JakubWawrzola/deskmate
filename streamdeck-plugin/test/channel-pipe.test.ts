import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, createServer, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { ChannelClient, ChannelError } from "../src/channel/client.ts";
import { encodeLine, LineDecoder } from "../src/channel/framing.ts";
import { PIPE_NAME_PREFIX } from "../src/channel/pipe-name.ts";
import {
  type EntityState,
  type JsonRecord,
  readActions,
  readEntities,
} from "../src/channel/protocol.ts";

const isWindows = process.platform === "win32";

/**
 * A stand-in for the Deskmate app: answers the requests of the protocol from
 * fixed data and records what it was asked.
 */
class FakeApp {
  readonly requests: JsonRecord[] = [];
  private readonly server: Server;
  private readonly sockets = new Set<Socket>();

  constructor() {
    this.server = createServer((socket) => this.serve(socket));
  }

  listen(path: string): Promise<void> {
    return new Promise((resolve, reject) => {
      this.server.once("error", reject);
      this.server.listen(path, resolve);
    });
  }

  /** Sends an event to every client. */
  push(event: JsonRecord): void {
    for (const socket of this.sockets) {
      socket.write(encodeLine(event));
    }
  }

  /** Ends every connection but keeps listening, like an app that restarts a channel. */
  dropClients(): void {
    for (const socket of this.sockets) {
      socket.destroy();
    }
  }

  close(): Promise<void> {
    this.dropClients();
    return new Promise((resolve) => this.server.close(() => resolve()));
  }

  private serve(socket: Socket): void {
    const decoder = new LineDecoder();
    this.sockets.add(socket);
    socket.on("close", () => this.sockets.delete(socket));
    socket.on("error", () => {});
    socket.on("data", (chunk) => {
      for (const line of decoder.push(chunk)) {
        // The test client only sends objects.
        const request = JSON.parse(line) as JsonRecord;
        this.requests.push(request);
        socket.write(encodeLine({ id: request.id, ...this.answer(request) }));
      }
    });
  }

  private answer(request: JsonRecord): JsonRecord {
    switch (request.op) {
      case "hello":
        return { ok: true, v: 1, app: "0.9.0", ha: true };
      case "actions":
        return {
          ok: true,
          actions: [
            { id: "hotkey:lights", name: "Lights" },
            { id: "tray:pc_sleep", name: "Sleep" },
          ],
        };
      case "run":
        return request.action === "hotkey:lights"
          ? { ok: true }
          : { ok: false, error: "unknown_action" };
      case "entities":
        return { ok: true, entities: [{ entity_id: "light.desk", name: "Desk", state: "on" }] };
      case "watch":
        return { ok: true, states: { "light.desk": "on" } };
      case "toggle":
      case "call":
        return { ok: true };
      default:
        return { ok: false, error: "bad_request" };
    }
  }
}

function waitFor(condition: () => boolean, what: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + 5000;
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

/** The whole conversation, over whatever `net` listens on at `path`. */
async function talkToFakeApp(path: string): Promise<void> {
  const app = new FakeApp();
  await app.listen(path);
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

    assert.deepEqual(readActions(await client.request("actions")), [
      { id: "hotkey:lights", name: "Lights" },
      { id: "tray:pc_sleep", name: "Sleep" },
    ]);
    await client.request("run", { action: "hotkey:lights" });
    await assert.rejects(
      client.request("run", { action: "hotkey:missing" }),
      (error) => error instanceof ChannelError && error.code === "unknown_action",
    );
    assert.deepEqual(readEntities(await client.request("entities", { domains: ["light"] })), [
      { entityId: "light.desk", name: "Desk", state: "on" },
    ]);
    await client.request("toggle", { entity_id: "light.desk" });
    await client.request("call", { service: "scene.turn_on", entity_id: "scene.movie", data: {} });
    await assert.rejects(
      client.request("no_such_op"),
      (error) => error instanceof ChannelError && error.code === "bad_request",
    );

    await waitFor(() => states.length === 1, "the watch states");
    app.push({ ev: "state", entity_id: "light.desk", state: "off" });
    await waitFor(() => states.length === 2, "the state event");
    assert.deepEqual(states, [
      { entityId: "light.desk", state: "on" },
      { entityId: "light.desk", state: "off" },
    ]);

    // The app drops the connection: the client comes back by itself after
    // the first, one-second wait and watches the same entities again.
    const watchesBefore = app.requests.filter((request) => request.op === "watch").length;
    app.dropClients();
    await waitFor(() => client.status.state !== "ready", "the client to notice the drop");
    await waitFor(() => client.status.state === "ready", "the reconnect");
    await waitFor(
      () => app.requests.filter((request) => request.op === "watch").length === watchesBefore + 1,
      "the watch list to be sent again",
    );

    assert.deepEqual(
      app.requests.slice(0, 2).map((request) => request.op),
      ["hello", "watch"],
    );
    assert.deepEqual(app.requests[0], {
      id: 1,
      op: "hello",
      v: 1,
      client: "streamdeck",
      version: "0.2.0",
    });
  } finally {
    client.setWanted(false);
    await app.close();
  }
}

test("the client talks to a server on a real Windows named pipe", {
  skip: isWindows ? false : "named pipes exist only on Windows",
}, async () => {
  // Not the app's own pipe name: a real Deskmate may be running here.
  const name = `${PIPE_NAME_PREFIX}test-${process.pid}-${randomBytes(4).toString("hex")}`;
  await talkToFakeApp(`\\\\.\\pipe\\${name}`);
});

test("the client talks to a server on a Unix socket", {
  skip: isWindows ? "covered by the named pipe test on Windows" : false,
}, async () => {
  const folder = mkdtempSync(join(tmpdir(), "deskmate-channel-"));
  try {
    await talkToFakeApp(join(folder, "channel.sock"));
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test("connecting to a pipe nobody listens on fails cleanly", async () => {
  const path = isWindows
    ? `\\\\.\\pipe\\${PIPE_NAME_PREFIX}test-absent-${randomBytes(4).toString("hex")}`
    : join(tmpdir(), `deskmate-channel-absent-${randomBytes(4).toString("hex")}.sock`);
  const client = new ChannelClient({
    path,
    version: "0.2.0",
    connect: (target) => connect(target),
  });
  await assert.rejects(
    client.request("actions"),
    (error) => error instanceof ChannelError && error.code === "not_connected",
  );
  assert.deepEqual(client.status, { state: "idle" });
});
