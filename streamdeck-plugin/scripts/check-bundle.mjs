// Checks the built plugin folder. Runs at the end of `npm run build`.
//
// 1. Every file the manifest and the property inspectors point at is there.
// 2. The bundle starts under this Node and behaves: it registers with a
//    stand-in for the Stream Deck app, and its keys work both through a
//    stand-in for the Deskmate app and against a stand-in for Home Assistant.
//
// Nothing here needs Stream Deck, Deskmate or Home Assistant. The stand-ins
// are local servers that speak only the messages the check uses: a WebSocket
// server for Stream Deck, a pipe for the app's local channel, and an HTTP and
// WebSocket server for Home Assistant, all on this machine.

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { createServer as createPipeServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { WebSocketServer } from "ws";

const PLUGIN_FOLDER = "com.deskmate.streamdeck.sdPlugin";
const PIPE_PATH_VARIABLE = "DESKMATE_STREAMDECK_PIPE";
const STEP_TIMEOUT_MS = 15_000;
const DEVICE_ID = "check-device";
const HA_TOKEN = "check-token";
const ENTITY_ID = "light.desk";
const ACTION_ID = "hotkey:lights";

const pluginDir = join(dirname(fileURLToPath(import.meta.url)), "..", PLUGIN_FOLDER);

function fail(message) {
  console.error(`check-bundle: ${message}`);
  process.exit(1);
}

// ---------- files ----------

function readManifest() {
  const path = join(pluginDir, "manifest.json");
  if (!existsSync(path)) {
    fail(`${PLUGIN_FOLDER}/manifest.json is missing; run the build first`);
  }
  return JSON.parse(readFileSync(path, "utf8"));
}

/** The manifest names images without an extension; Stream Deck loads the PNG pair. */
function imageFiles(image) {
  return [`${image}.png`, `${image}@2x.png`];
}

/**
 * Scripts and stylesheets a property inspector loads, relative to the plugin
 * folder. All of them ship with the plugin: a page that loads one from
 * another address fails the check, because a script from the network could
 * read the plugin's settings, the Home Assistant token among them.
 */
function inspectorAssets(page) {
  const pagePath = join(pluginDir, page);
  if (!existsSync(pagePath)) {
    return [];
  }
  const html = readFileSync(pagePath, "utf8");
  const assets = [];
  for (const match of html.matchAll(/(?:src|href)="([^"]+)"/g)) {
    if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(match[1])) {
      fail(`${page} loads ${match[1]}, which is not a file of the plugin`);
    }
    assets.push(join(dirname(page), match[1]));
  }
  return assets;
}

function referencedFiles(manifest) {
  const files = [
    manifest.CodePath,
    // The bundle is an ES module only because of the package.json beside it.
    join(dirname(manifest.CodePath), "package.json"),
    ...imageFiles(manifest.Icon),
    ...imageFiles(manifest.CategoryIcon),
  ];
  for (const action of manifest.Actions) {
    files.push(...imageFiles(action.Icon));
    for (const state of action.States) {
      files.push(...imageFiles(state.Image));
    }
    if (action.PropertyInspectorPath) {
      files.push(action.PropertyInspectorPath, ...inspectorAssets(action.PropertyInspectorPath));
    }
  }
  return [...new Set(files.map((file) => file.replaceAll("\\", "/")))];
}

function checkFiles(manifest) {
  const files = referencedFiles(manifest);
  const missing = files.filter((file) => !existsSync(join(pluginDir, file)));
  if (missing.length > 0) {
    fail(`files the plugin needs are missing from ${PLUGIN_FOLDER}:\n  ${missing.join("\n  ")}`);
  }
  return files;
}

// ---------- stand-ins ----------

/** Messages as they arrive, and a way to wait for a particular one. */
class Inbox {
  #items = [];
  #waiters = [];

  add(item) {
    const index = this.#waiters.findIndex((waiter) => waiter.matches(item));
    if (index === -1) {
      this.#items.push(item);
      return;
    }
    const [waiter] = this.#waiters.splice(index, 1);
    waiter.resolve(item);
  }

  /** Drops what already arrived, so that the next `take` only sees what comes after. */
  forget(matches) {
    this.#items = this.#items.filter((item) => !matches(item));
  }

  /** The first message that matches, whether it already arrived or still has to. */
  take(matches, what) {
    const index = this.#items.findIndex(matches);
    if (index !== -1) {
      return Promise.resolve(this.#items.splice(index, 1)[0]);
    }
    return new Promise((resolve, reject) => {
      const waiter = { matches, resolve: undefined };
      const timer = setTimeout(() => {
        this.#waiters.splice(this.#waiters.indexOf(waiter), 1);
        reject(new Error(`timed out waiting for ${what}`));
      }, STEP_TIMEOUT_MS);
      waiter.resolve = (item) => {
        clearTimeout(timer);
        resolve(item);
      };
      this.#waiters.push(waiter);
    });
  }
}

function listen(server, target) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(target, () => resolve());
  });
}

function closeServer(server) {
  return new Promise((resolve) => server.close(() => resolve()));
}

/** The Stream Deck app, as far as a plugin sees it. */
class StreamDeckStandIn {
  received = new Inbox();
  #server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  #socket;

  constructor() {
    this.#server.on("connection", (socket) => {
      this.#socket = socket;
      socket.on("message", (data) => this.received.add(JSON.parse(data.toString())));
    });
  }

  ready() {
    return new Promise((resolve, reject) => {
      this.#server.once("error", reject);
      this.#server.once("listening", () => resolve(this.#server.address().port));
    });
  }

  send(message) {
    this.#socket.send(JSON.stringify(message));
  }

  /**
   * Forgets the states a key was given so far. A key is set to "off" whenever
   * its state becomes unknown, and such an earlier "off" must not pass for
   * the one a later step waits for.
   */
  forgetStates(context) {
    this.received.forget((message) => message.event === "setState" && message.context === context);
  }

  /** A message of this kind from the plugin, about this key. */
  expect(event, context, matches = () => true) {
    return this.received.take(
      (message) => message.event === event && message.context === context && matches(message),
      `"${event}" for ${context}`,
    );
  }

  appear(action, context, settings) {
    const payload = {
      settings,
      coordinates: { column: 0, row: 0 },
      controller: "Keypad",
      isInMultiAction: false,
      state: 0,
    };
    this.send({ event: "willAppear", action, context, device: DEVICE_ID, payload });
  }

  press(action, context, settings) {
    const payload = {
      settings,
      coordinates: { column: 0, row: 0 },
      isInMultiAction: false,
      state: 0,
    };
    this.send({ event: "keyDown", action, context, device: DEVICE_ID, payload });
  }

  close() {
    this.#socket?.terminate();
    return closeServer(this.#server);
  }
}

/** The Deskmate app's local channel: one JSON object per line. */
class AppStandIn {
  requests = new Inbox();
  #server = createPipeServer((socket) => this.#serve(socket));
  #sockets = new Set();

  listen(path) {
    return listen(this.#server, path);
  }

  push(event) {
    for (const socket of this.#sockets) {
      socket.write(`${JSON.stringify(event)}\n`);
    }
  }

  expect(op, matches = () => true) {
    return this.requests.take(
      (request) => request.op === op && matches(request),
      `the "${op}" request on the app's channel`,
    );
  }

  close() {
    for (const socket of this.#sockets) {
      socket.destroy();
    }
    return closeServer(this.#server);
  }

  #serve(socket) {
    let buffered = "";
    this.#sockets.add(socket);
    socket.on("close", () => this.#sockets.delete(socket));
    socket.on("error", () => {});
    socket.setEncoding("utf8");
    socket.on("data", (text) => {
      buffered += text;
      const lines = buffered.split("\n");
      buffered = lines.pop();
      for (const line of lines) {
        const request = JSON.parse(line);
        socket.write(`${JSON.stringify({ id: request.id, ...this.#answer(request) })}\n`);
        this.requests.add(request);
      }
    });
  }

  #answer(request) {
    switch (request.op) {
      case "hello":
        return { ok: true, v: 1, app: "0.0.0", ha: true };
      case "watch":
        return {
          ok: true,
          states: Object.fromEntries(request.entities.map((entityId) => [entityId, "on"])),
        };
      case "entities":
        return { ok: true, entities: [{ entity_id: ENTITY_ID, name: "Desk", state: "on" }] };
      case "toggle":
      case "call":
        return { ok: true };
      case "actions":
        return { ok: true, actions: [{ id: ACTION_ID, name: "Lights" }] };
      case "run":
        return request.action === ACTION_ID ? { ok: true } : { ok: false, error: "unknown_action" };
      default:
        return { ok: false, error: "bad_request" };
    }
  }
}

/** Home Assistant's REST and WebSocket API, for one entity that is on. */
class HomeAssistantStandIn {
  calls = new Inbox();
  #http = createHttpServer((request, response) => this.#handle(request, response));
  #sockets = new WebSocketServer({ server: this.#http, path: "/api/websocket" });
  #clients = new Set();

  constructor() {
    this.#sockets.on("connection", (socket) => this.#serve(socket));
  }

  async listen() {
    await listen(this.#http, { host: "127.0.0.1", port: 0 });
    return `http://127.0.0.1:${this.#http.address().port}`;
  }

  /** Reports a state change to every subscribed client. */
  change(entityId, state) {
    const event = {
      event_type: "state_changed",
      data: { entity_id: entityId, new_state: { entity_id: entityId, state } },
    };
    for (const socket of this.#clients) {
      socket.send(JSON.stringify({ type: "event", event }));
    }
  }

  expectCall(path, matches = () => true) {
    return this.calls.take(
      (call) => call.path === path && matches(call.body),
      `the request ${path} at Home Assistant`,
    );
  }

  close() {
    for (const socket of this.#clients) {
      socket.terminate();
    }
    this.#http.closeAllConnections();
    return closeServer(this.#http);
  }

  #handle(request, response) {
    const reply = (status, body) => {
      response.writeHead(status, { "Content-Type": "application/json" });
      response.end(JSON.stringify(body));
    };
    if (request.headers.authorization !== `Bearer ${HA_TOKEN}`) {
      reply(401, { message: "Unauthorized" });
      return;
    }
    const state = { entity_id: ENTITY_ID, state: "on", attributes: { friendly_name: "Desk" } };
    if (request.method === "GET" && request.url === "/api/states") {
      reply(200, [state]);
    } else if (request.method === "GET" && request.url === `/api/states/${ENTITY_ID}`) {
      reply(200, state);
    } else if (request.method === "POST" && request.url.startsWith("/api/services/")) {
      let text = "";
      request.setEncoding("utf8");
      request.on("data", (chunk) => {
        text += chunk;
      });
      request.on("end", () => {
        reply(200, []);
        this.calls.add({ path: request.url, body: JSON.parse(text) });
      });
    } else {
      reply(404, { message: "Not found" });
    }
  }

  #serve(socket) {
    socket.send(JSON.stringify({ type: "auth_required" }));
    socket.on("close", () => this.#clients.delete(socket));
    socket.on("message", (data) => {
      const message = JSON.parse(data.toString());
      const send = (answer) => socket.send(JSON.stringify(answer));
      if (message.type === "auth") {
        send({ type: message.access_token === HA_TOKEN ? "auth_ok" : "auth_invalid" });
      } else if (message.type === "subscribe_events") {
        this.#clients.add(socket);
        send({ id: message.id, type: "result", success: true, result: null });
      } else if (message.type === "get_states") {
        const states = [{ entity_id: ENTITY_ID, state: "on" }];
        send({ id: message.id, type: "result", success: true, result: states });
      } else if (message.type === "ping") {
        send({ id: message.id, type: "pong" });
      }
    });
  }
}

// ---------- the check ----------

/**
 * Starts the bundle the way Stream Deck does and plays every other side of
 * the conversation. Returns one line per thing that was confirmed.
 */
async function checkBehaviour(manifest) {
  const confirmed = [];
  // The plugin reads manifest.json from its working directory and writes its
  // log there, so it runs in a copy that is thrown away afterwards.
  const scratch = mkdtempSync(join(tmpdir(), "deskmate-streamdeck-check-"));
  const cwd = join(scratch, PLUGIN_FOLDER);
  mkdirSync(cwd);
  cpSync(join(pluginDir, "manifest.json"), join(cwd, "manifest.json"));

  // Never the real pipe name: a real Deskmate may be running on this machine.
  const pipePath =
    process.platform === "win32"
      ? `\\\\.\\pipe\\deskmate-streamdeck-check-${process.pid}-${randomBytes(4).toString("hex")}`
      : join(scratch, "channel.sock");

  const streamDeck = new StreamDeckStandIn();
  const app = new AppStandIn();
  const homeAssistant = new HomeAssistantStandIn();
  let child;
  let output = "";

  const uuid = (suffix) => `${manifest.UUID}.${suffix}`;
  const toggleKey = "check-toggle";
  const serviceKey = "check-service";
  const sceneKey = "check-scene";
  const actionKey = "check-action";

  const inspect = (action, context) =>
    streamDeck.send({ event: "propertyInspectorDidAppear", action, context, device: DEVICE_ID });
  const stopInspecting = (action, context) =>
    streamDeck.send({ event: "propertyInspectorDidDisappear", action, context, device: DEVICE_ID });
  const ask = (action, context, event) =>
    streamDeck.send({ event: "sendToPlugin", action, context, payload: { event } });

  try {
    const port = await streamDeck.ready();
    await app.listen(pipePath);
    const homeAssistantUrl = await homeAssistant.listen();

    const info = {
      application: {
        font: "",
        language: "en",
        platform: "windows",
        platformVersion: "10",
        version: "7.1.0.0",
      },
      colors: {},
      devicePixelRatio: 1,
      devices: [{ id: DEVICE_ID, name: "Check", size: { columns: 5, rows: 3 }, type: 0 }],
      plugin: { uuid: manifest.UUID, version: manifest.Version },
    };
    const args = [
      join(pluginDir, manifest.CodePath),
      "-port",
      String(port),
      "-pluginUUID",
      manifest.UUID,
      "-registerEvent",
      "registerPlugin",
      "-info",
      JSON.stringify(info),
    ];
    child = spawn(process.execPath, args, {
      cwd,
      env: { ...process.env, [PIPE_PATH_VARIABLE]: pipePath },
      stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
    });

    // Start-up.
    const registration = await streamDeck.received.take(
      (message) => message.event === "registerPlugin",
      "the plugin to register",
    );
    if (registration.uuid !== manifest.UUID) {
      throw new Error(`the plugin registered as ${registration.uuid}, not ${manifest.UUID}`);
    }
    const settingsRequest = await streamDeck.received.take(
      (message) => message.event === "getGlobalSettings",
      "the plugin to ask for its settings",
    );
    streamDeck.send({
      event: "didReceiveGlobalSettings",
      id: settingsRequest.id,
      payload: { settings: {} },
    });
    confirmed.push("the bundle starts and registers");

    // Every action is wired up: a key with no settings answers with an alert.
    for (const [index, action] of manifest.Actions.entries()) {
      const context = `check-empty-${index}`;
      streamDeck.appear(action.UUID, context, {});
      streamDeck.press(action.UUID, context, {});
      await streamDeck.expect("showAlert", context);
    }
    confirmed.push(`${manifest.Actions.length} actions answer a key press`);

    // Through the Deskmate app.
    const hello = await app.expect("hello");
    const expectedVersion = manifest.Version.split(".").slice(0, 3).join(".");
    if (hello.v !== 1 || hello.client !== "streamdeck" || hello.version !== expectedVersion) {
      throw new Error("the plugin's hello does not match the protocol");
    }
    const toggleSettings = { entityId: ENTITY_ID };
    streamDeck.appear(uuid("toggle"), toggleKey, toggleSettings);
    await app.expect("watch", (request) => request.entities.includes(ENTITY_ID));
    await streamDeck.expect("setState", toggleKey, (message) => message.payload.state === 1);
    streamDeck.press(uuid("toggle"), toggleKey, toggleSettings);
    await app.expect("toggle", (request) => request.entity_id === ENTITY_ID);
    streamDeck.forgetStates(toggleKey);
    app.push({ ev: "state", entity_id: ENTITY_ID, state: "off" });
    await streamDeck.expect("setState", toggleKey, (message) => message.payload.state === 0);

    const serviceSettings = {
      service: "light.turn_on",
      entityId: ENTITY_ID,
      data: '{"brightness_pct": 40}',
    };
    streamDeck.appear(uuid("call-service"), serviceKey, serviceSettings);
    streamDeck.press(uuid("call-service"), serviceKey, serviceSettings);
    await app.expect(
      "call",
      (request) =>
        request.service === "light.turn_on" &&
        request.entity_id === ENTITY_ID &&
        request.data.brightness_pct === 40,
    );
    await streamDeck.expect("showOk", serviceKey);

    const sceneSettings = { sceneId: "movie" };
    streamDeck.appear(uuid("scene"), sceneKey, sceneSettings);
    streamDeck.press(uuid("scene"), sceneKey, sceneSettings);
    await app.expect(
      "call",
      (request) => request.service === "scene.turn_on" && request.entity_id === "scene.movie",
    );
    await streamDeck.expect("showOk", sceneKey);

    // A Deskmate action: one the app knows, then one it does not.
    const actionSettings = { actionId: ACTION_ID, actionName: "Lights" };
    streamDeck.appear(uuid("action"), actionKey, actionSettings);
    streamDeck.press(uuid("action"), actionKey, actionSettings);
    await app.expect("run", (request) => request.action === ACTION_ID);
    await streamDeck.expect("showOk", actionKey);
    streamDeck.press(uuid("action"), actionKey, { actionId: "hotkey:gone" });
    await app.expect("run", (request) => request.action === "hotkey:gone");
    await streamDeck.expect("showAlert", actionKey);

    // The property inspector of the Deskmate Action key, then of the toggle key.
    inspect(uuid("action"), actionKey);
    ask(uuid("action"), actionKey, "getActions");
    await streamDeck.expect(
      "sendToPropertyInspector",
      actionKey,
      (message) =>
        message.payload.event === "getActions" &&
        message.payload.items.some((group) =>
          group.children?.some((item) => item.value === ACTION_ID),
        ),
    );
    ask(uuid("action"), actionKey, "getStatus");
    await streamDeck.expect(
      "sendToPropertyInspector",
      actionKey,
      (message) => message.payload.event === "status" && message.payload.active === "deskmate",
    );
    stopInspecting(uuid("action"), actionKey);

    inspect(uuid("toggle"), toggleKey);
    ask(uuid("toggle"), toggleKey, "getStatus");
    await streamDeck.expect(
      "sendToPropertyInspector",
      toggleKey,
      (message) => message.payload.event === "status" && message.payload.active === "deskmate",
    );
    ask(uuid("toggle"), toggleKey, "getEntities");
    await streamDeck.expect(
      "sendToPropertyInspector",
      toggleKey,
      (message) =>
        message.payload.event === "getEntities" &&
        message.payload.items.some((item) => item.value === ENTITY_ID),
    );
    confirmed.push("keys and the property inspector work through a stand-in for the Deskmate app");

    // Directly against Home Assistant.
    streamDeck.send({
      event: "didReceiveGlobalSettings",
      payload: {
        settings: { connection: "direct", haUrl: homeAssistantUrl, haToken: HA_TOKEN },
      },
    });
    await streamDeck.expect(
      "sendToPropertyInspector",
      toggleKey,
      (message) => message.payload.event === "status" && message.payload.active === "direct",
    );
    await streamDeck.expect("setState", toggleKey, (message) => message.payload.state === 1);
    streamDeck.press(uuid("toggle"), toggleKey, toggleSettings);
    await homeAssistant.expectCall(
      "/api/services/homeassistant/toggle",
      (body) => body.entity_id === ENTITY_ID,
    );
    streamDeck.forgetStates(toggleKey);
    homeAssistant.change(ENTITY_ID, "off");
    await streamDeck.expect("setState", toggleKey, (message) => message.payload.state === 0);

    streamDeck.press(uuid("call-service"), serviceKey, serviceSettings);
    await homeAssistant.expectCall(
      "/api/services/light/turn_on",
      (body) => body.entity_id === ENTITY_ID && body.brightness_pct === 40,
    );
    await streamDeck.expect("showOk", serviceKey);
    streamDeck.press(uuid("scene"), sceneKey, sceneSettings);
    await homeAssistant.expectCall(
      "/api/services/scene/turn_on",
      (body) => body.entity_id === "scene.movie",
    );
    await streamDeck.expect("showOk", sceneKey);
    confirmed.push("keys work against a stand-in for Home Assistant");

    // With neither connection, a press is answered with an alert. The app
    // leaves first, so that the plugin cannot find it again.
    await app.close();
    streamDeck.send({ event: "didReceiveGlobalSettings", payload: { settings: {} } });
    await streamDeck.expect(
      "sendToPropertyInspector",
      toggleKey,
      (message) => message.payload.event === "status" && message.payload.active === "none",
    );
    streamDeck.press(uuid("toggle"), toggleKey, toggleSettings);
    await streamDeck.expect("showAlert", toggleKey);
    streamDeck.press(uuid("action"), actionKey, actionSettings);
    await streamDeck.expect("showAlert", actionKey);
    confirmed.push("without a connection a press shows the alert");

    return confirmed;
  } catch (error) {
    const exited =
      child && child.exitCode !== null ? ` (the plugin exited with ${child.exitCode})` : "";
    throw new Error(`${error.message}${exited}${output ? `\n${output}` : ""}`);
  } finally {
    if (child && child.exitCode === null && child.signalCode === null) {
      const gone = new Promise((resolve) => child.once("exit", resolve));
      child.kill();
      await gone;
    }
    await Promise.all([streamDeck.close(), app.close(), homeAssistant.close()]);
    // Windows keeps the folder locked for a moment after the process is gone.
    rmSync(scratch, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}

const manifest = readManifest();
const files = checkFiles(manifest);
console.log(`check-bundle: ${files.length} files the plugin needs are in ${PLUGIN_FOLDER}`);

try {
  for (const line of await checkBehaviour(manifest)) {
    console.log(`check-bundle: ${line}`);
  }
} catch (error) {
  fail(error.message);
}
