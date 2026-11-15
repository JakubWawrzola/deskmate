import assert from "node:assert/strict";
import { test } from "node:test";
import type { ChannelStatus } from "../src/channel/client.ts";
import { ConnectionHub } from "../src/connection/hub.ts";
import {
  ConnectionError,
  type DeskmateAction,
  type DeskmateLink,
  type DirectLink,
  type DirectProblem,
  type EntityInfo,
  type EntityState,
  type HaTransport,
  type ServiceCall,
} from "../src/connection/transport.ts";
import type { DirectConfig } from "../src/settings.ts";

const READY: ChannelStatus = { state: "ready", app: "0.9.0", ha: true };
const DOWN: ChannelStatus = { state: "down", reason: "unavailable" };
const DIRECT_SETTINGS = { haUrl: "http://homeassistant.local:8123", haToken: "token" };

/** Records what a transport was asked to do. */
class FakeTransport implements HaTransport {
  readonly calls: string[] = [];
  watched: string[] = [];
  entities: EntityInfo[] = [];
  private readonly stateListeners = new Set<(state: EntityState) => void>();

  async toggle(entityId: string): Promise<void> {
    this.calls.push(`toggle ${entityId}`);
  }

  async callService(call: ServiceCall): Promise<void> {
    this.calls.push(`call ${call.service} ${call.entityId ?? "-"} ${JSON.stringify(call.data)}`);
  }

  async listEntities(domains: readonly string[] | undefined): Promise<EntityInfo[]> {
    this.calls.push(`entities ${domains?.join(",") ?? "*"}`);
    return this.entities;
  }

  watch(entityIds: readonly string[]): void {
    this.watched = [...entityIds];
  }

  onState(listener: (state: EntityState) => void): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  report(entityId: string, state: string): void {
    for (const listener of this.stateListeners) {
      listener({ entityId, state });
    }
  }
}

class FakeDeskmate extends FakeTransport implements DeskmateLink {
  status: ChannelStatus = { state: "idle" };
  wanted = false;
  connectAttempts = 0;
  /** What the channel becomes when a connection is attempted. */
  onConnect: ChannelStatus = DOWN;
  private readonly statusListeners = new Set<(status: ChannelStatus) => void>();

  onStatus(listener: (status: ChannelStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  setWanted(wanted: boolean): void {
    this.wanted = wanted;
  }

  async ensureConnected(): Promise<void> {
    this.connectAttempts += 1;
    this.become(this.onConnect);
    if (this.status.state !== "ready") {
      throw new ConnectionError("not_connected");
    }
  }

  async listActions(): Promise<DeskmateAction[]> {
    this.calls.push("actions");
    return [{ id: "hotkey:lights", name: "Lights" }];
  }

  async runAction(actionId: string): Promise<void> {
    this.calls.push(`run ${actionId}`);
  }

  become(status: ChannelStatus): void {
    this.status = status;
    for (const listener of this.statusListeners) {
      listener(status);
    }
  }
}

class FakeDirect extends FakeTransport implements DirectLink {
  config: DirectConfig | undefined;
  problem: DirectProblem | undefined;
  private readonly problemListeners = new Set<() => void>();

  get configured(): boolean {
    return this.config !== undefined;
  }

  configure(config: DirectConfig | undefined): void {
    this.config = config;
  }

  onProblem(listener: () => void): () => void {
    this.problemListeners.add(listener);
    return () => this.problemListeners.delete(listener);
  }

  fail(problem: DirectProblem): void {
    this.problem = problem;
    for (const listener of this.problemListeners) {
      listener();
    }
  }
}

function createHub() {
  const deskmate = new FakeDeskmate();
  const direct = new FakeDirect();
  const hub = new ConnectionHub(deskmate, direct);
  return { hub, deskmate, direct };
}

test("auto sends operations through the app when its channel is up", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);
  deskmate.become(READY);

  await hub.toggle("light.desk");
  await hub.callService({ service: "scene.turn_on", entityId: "scene.movie", data: undefined });
  await hub.listEntities(["light"]);
  assert.deepEqual(deskmate.calls, [
    "toggle light.desk",
    "call scene.turn_on scene.movie undefined",
    "entities light",
  ]);
  assert.deepEqual(direct.calls, []);
  assert.equal(hub.status.active, "deskmate");
});

test("auto tries the app once more, then uses Direct", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);

  await hub.toggle("light.desk");
  assert.equal(deskmate.connectAttempts, 1);
  assert.deepEqual(direct.calls, ["toggle light.desk"]);
  assert.deepEqual(deskmate.calls, []);
  assert.equal(hub.status.active, "direct");
});

test("auto picks up an app that was started after the plugin", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);
  deskmate.onConnect = READY;

  await hub.toggle("light.desk");
  assert.deepEqual(deskmate.calls, ["toggle light.desk"]);
  assert.deepEqual(direct.calls, []);
});

test("auto with neither connection refuses the operation", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure({});
  await assert.rejects(
    hub.toggle("light.desk"),
    (error) => error instanceof ConnectionError && error.code === "no_connection",
  );
  assert.deepEqual(deskmate.calls, []);
  assert.deepEqual(direct.calls, []);
  assert.equal(hub.status.active, "none");
});

test("deskmate mode never uses Direct, even when the app is away", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure({ ...DIRECT_SETTINGS, connection: "deskmate" });
  await hub.toggle("light.desk");
  // The fake accepts the call; the real transport would report not_connected.
  assert.deepEqual(deskmate.calls, ["toggle light.desk"]);
  assert.deepEqual(direct.calls, []);
});

test("direct mode never touches the app for Home Assistant", async () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure({ ...DIRECT_SETTINGS, connection: "direct" });
  deskmate.become(READY);
  await hub.toggle("light.desk");
  assert.deepEqual(direct.calls, ["toggle light.desk"]);
  assert.deepEqual(deskmate.calls, []);
  assert.equal(deskmate.connectAttempts, 0);
});

test("direct mode without an address refuses the operation", async () => {
  const { hub } = createHub();
  hub.configure({ connection: "direct" });
  await assert.rejects(hub.toggle("light.desk"), ConnectionError);
});

test("Deskmate actions always go to the app", async () => {
  const { hub, deskmate } = createHub();
  hub.configure({ ...DIRECT_SETTINGS, connection: "direct" });
  assert.deepEqual(await hub.listActions(), [{ id: "hotkey:lights", name: "Lights" }]);
  await hub.runAction("hotkey:lights");
  assert.deepEqual(deskmate.calls, ["actions", "run hotkey:lights"]);
});

test("the channel is wanted while a key needs it", () => {
  const { hub, deskmate } = createHub();
  hub.configure({});
  assert.equal(deskmate.wanted, false);

  const releaseFirst = hub.retain("homeAssistant");
  const releaseSecond = hub.retain("homeAssistant");
  assert.equal(deskmate.wanted, true);
  releaseFirst();
  assert.equal(deskmate.wanted, true);
  releaseSecond();
  assert.equal(deskmate.wanted, false);
  // Releasing twice must not count twice.
  releaseSecond();
  hub.retain("homeAssistant");
  assert.equal(deskmate.wanted, true);
});

test("in direct mode only Deskmate Action keys need the channel", () => {
  const { hub, deskmate } = createHub();
  hub.configure({ ...DIRECT_SETTINGS, connection: "direct" });
  hub.retain("homeAssistant");
  assert.equal(deskmate.wanted, false);
  const release = hub.retain("deskmate");
  assert.equal(deskmate.wanted, true);
  release();
  assert.equal(deskmate.wanted, false);
});

test("changing the mode changes whether the channel is wanted", () => {
  const { hub, deskmate } = createHub();
  hub.configure({ ...DIRECT_SETTINGS, connection: "direct" });
  hub.retain("homeAssistant");
  assert.equal(deskmate.wanted, false);
  hub.configure({ ...DIRECT_SETTINGS, connection: "auto" });
  assert.equal(deskmate.wanted, true);
});

test("watched entities are watched on the connection in use", () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);
  hub.watchEntity("light.desk", () => {});
  hub.watchEntity("switch.fan", () => {});
  assert.deepEqual(direct.watched, ["light.desk", "switch.fan"]);
  assert.deepEqual(deskmate.watched, []);

  deskmate.become(READY);
  assert.deepEqual(deskmate.watched, ["light.desk", "switch.fan"]);
  assert.deepEqual(direct.watched, []);

  deskmate.become(DOWN);
  assert.deepEqual(direct.watched, ["light.desk", "switch.fan"]);
  assert.deepEqual(deskmate.watched, []);
});

test("a listener gets the states of its entity from the connection in use", () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);
  deskmate.become(READY);
  const seen: (string | undefined)[] = [];
  hub.watchEntity("light.desk", (state) => seen.push(state));
  assert.deepEqual(seen, [undefined]);

  deskmate.report("light.desk", "on");
  deskmate.report("switch.fan", "on");
  direct.report("light.desk", "off");
  assert.deepEqual(seen, [undefined, "on"]);
});

test("a second listener starts with the state already known", () => {
  const { hub, deskmate } = createHub();
  hub.configure({});
  deskmate.become(READY);
  hub.watchEntity("light.desk", () => {});
  deskmate.report("light.desk", "on");
  const seen: (string | undefined)[] = [];
  hub.watchEntity("light.desk", (state) => seen.push(state));
  assert.deepEqual(seen, ["on"]);
});

test("when the connection in use changes, known states are forgotten", () => {
  const { hub, deskmate, direct } = createHub();
  hub.configure(DIRECT_SETTINGS);
  deskmate.become(READY);
  const seen: (string | undefined)[] = [];
  hub.watchEntity("light.desk", (state) => seen.push(state));
  deskmate.report("light.desk", "on");

  deskmate.become(DOWN);
  direct.report("light.desk", "off");
  assert.deepEqual(seen, [undefined, "on", undefined, "off"]);
});

test("an entity is watched until its last listener is gone", () => {
  const { hub, deskmate } = createHub();
  hub.configure({});
  deskmate.become(READY);
  const stopFirst = hub.watchEntity("light.desk", () => {});
  const stopSecond = hub.watchEntity("light.desk", () => {});
  stopFirst();
  assert.deepEqual(deskmate.watched, ["light.desk"]);
  stopSecond();
  assert.deepEqual(deskmate.watched, []);
  stopSecond();
  assert.deepEqual(deskmate.watched, []);
});

test("the status follows the settings, the channel and Direct's problems", () => {
  const { hub, deskmate, direct } = createHub();
  let changes = 0;
  hub.onStatus(() => {
    changes += 1;
  });

  hub.configure(DIRECT_SETTINGS);
  assert.equal(changes, 1);
  assert.deepEqual(hub.status, {
    mode: "auto",
    active: "direct",
    channel: { state: "idle" },
    directConfigured: true,
    directProblem: undefined,
  });

  deskmate.become(READY);
  assert.equal(changes, 2);
  assert.equal(hub.status.active, "deskmate");

  // The same status again is not announced again.
  deskmate.become(READY);
  hub.configure(DIRECT_SETTINGS);
  assert.equal(changes, 2);

  direct.fail("auth");
  assert.equal(changes, 3);
  assert.equal(hub.status.directProblem, "auth");
});
