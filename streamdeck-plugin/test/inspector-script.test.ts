import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";

const scriptPath = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "com.deskmate.streamdeck.sdPlugin",
  "ui",
  "inspector.js",
);
const script = readFileSync(scriptPath, "utf8");

/** As much of an element as the script touches. */
class FakeElement {
  hidden = false;
  textContent = "";
  refreshes = 0;
  private current: string | undefined;
  private readonly attributes: Record<string, string>;
  private readonly listeners: (() => void)[] = [];

  constructor(attributes: Record<string, string> = {}) {
    this.attributes = attributes;
  }

  get value(): string | undefined {
    return this.current;
  }

  /** Like the sdpi components: a new value announces itself. */
  set value(value: string | undefined) {
    if (value !== this.current) {
      this.current = value;
      for (const listener of this.listeners) {
        listener();
      }
    }
  }

  getAttribute(name: string): string | null {
    return this.attributes[name] ?? null;
  }

  addEventListener(event: string, listener: () => void): void {
    assert.equal(event, "valuechange");
    this.listeners.push(listener);
  }

  refresh(): void {
    this.refreshes += 1;
  }
}

type Page = {
  elements: Record<string, FakeElement>;
  sent: { event: string; payload: unknown }[];
  /** Delivers a message from the plugin. */
  receive: (payload: unknown) => void;
  /** Runs the timers the script started. */
  runTimers: () => void;
  timerDelays: () => number[];
};

function loadPage(ids: string[], withComponents = true): Page {
  const elements: Record<string, FakeElement> = {};
  for (const id of ids) {
    elements[id] = new FakeElement(id === "picker" ? { datasource: "getEntities" } : {});
  }
  const sent: Page["sent"] = [];
  const handlers: ((message: { payload: unknown }) => void)[] = [];
  const timers = new Map<number, { callback: () => void; delay: number }>();
  let nextTimer = 1;

  const streamDeckClient = {
    sendToPropertyInspector: {
      subscribe: (handler: (message: { payload: unknown }) => void) => handlers.push(handler),
    },
    // The payload is built inside the script's own context; the round trip
    // through JSON turns it into plain objects of this one.
    send: (event: string, payload: unknown) =>
      sent.push({ event, payload: JSON.parse(JSON.stringify(payload)) }),
  };
  runInNewContext(script, {
    window: withComponents ? { SDPIComponents: { streamDeckClient } } : {},
    document: { getElementById: (id: string) => elements[id] ?? null },
    setTimeout: (callback: () => void, delay: number) => {
      timers.set(nextTimer, { callback, delay });
      nextTimer += 1;
      return nextTimer - 1;
    },
    clearTimeout: (timer: number) => timers.delete(timer),
  });

  return {
    elements,
    sent,
    receive: (payload) => {
      for (const handler of handlers) {
        handler({ payload });
      }
    },
    runTimers: () => {
      for (const [timer, { callback }] of [...timers]) {
        timers.delete(timer);
        callback();
      }
    },
    timerDelays: () => [...timers.values()].map((timer) => timer.delay),
  };
}

const STATUS_IDS = ["status-text", "status-hint", "direct-settings"];
const PICKER_IDS = ["picker", "picker-item", "picker-target"];

function element(page: Page, id: string): FakeElement {
  const found = page.elements[id];
  assert.ok(found, `no element ${id}`);
  return found;
}

test("the inspector asks the plugin for the connection status when it opens", () => {
  const page = loadPage(STATUS_IDS);
  assert.deepEqual(page.sent, [{ event: "sendToPlugin", payload: { event: "getStatus" } }]);
});

test("the status and its hint are shown as the plugin words them", () => {
  const page = loadPage(STATUS_IDS);
  page.receive({
    event: "status",
    mode: "auto",
    active: "none",
    text: "Not connected.",
    hint: "Open Deskmate, go to Settings and turn on Stream Deck.",
  });
  assert.equal(element(page, "status-text").textContent, "Not connected.");
  assert.equal(
    element(page, "status-hint").textContent,
    "Open Deskmate, go to Settings and turn on Stream Deck.",
  );
  assert.equal(element(page, "status-hint").hidden, false);
  assert.equal(element(page, "direct-settings").hidden, false);
});

test("an empty hint is hidden", () => {
  const page = loadPage(STATUS_IDS);
  page.receive({ event: "status", mode: "auto", active: "deskmate", text: "Connected.", hint: "" });
  assert.equal(element(page, "status-hint").hidden, true);
});

test("the address and token are hidden while only the app is used", () => {
  const page = loadPage(STATUS_IDS);
  page.receive({ event: "status", mode: "deskmate", active: "deskmate", text: "x", hint: "" });
  assert.equal(element(page, "direct-settings").hidden, true);
  page.receive({ event: "status", mode: "direct", active: "direct", text: "x", hint: "" });
  assert.equal(element(page, "direct-settings").hidden, false);
});

test("the dropdown shows only when the plugin has something to pick from", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS]);
  element(page, "picker-item").hidden = true;
  page.receive({ event: "getEntities", items: [{ value: "light.desk", label: "Desk" }] });
  assert.equal(element(page, "picker-item").hidden, false);
  page.receive({ event: "getEntities", items: [] });
  assert.equal(element(page, "picker-item").hidden, true);
  page.receive({ event: "getEntities" });
  assert.equal(element(page, "picker-item").hidden, true);
});

test("a list for another dropdown is ignored", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS]);
  element(page, "picker-item").hidden = true;
  page.receive({ event: "getSomethingElse", items: [{ value: "a", label: "A" }] });
  assert.equal(element(page, "picker-item").hidden, true);
});

test("picking an option types it into the field that holds the setting", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS]);
  element(page, "picker").value = "light.desk";
  assert.equal(element(page, "picker-target").value, "light.desk");
});

test("typing into the field selects the matching option", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS]);
  element(page, "picker-target").value = "switch.fan";
  assert.equal(element(page, "picker").value, "switch.fan");
});

test("a dropdown with nothing selected does not clear the field", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS]);
  element(page, "picker-target").value = "light.typed_by_hand";
  element(page, "picker").value = undefined;
  assert.equal(element(page, "picker-target").value, "light.typed_by_hand");
});

test("a change of the scope field makes the dropdown ask again, once", () => {
  const page = loadPage([...STATUS_IDS, ...PICKER_IDS, "picker-scope"]);
  element(page, "picker-scope").value = "light.turn_o";
  element(page, "picker-scope").value = "light.turn_on";
  assert.equal(element(page, "picker").refreshes, 0);
  assert.deepEqual(page.timerDelays(), [500]);
  page.runTimers();
  assert.equal(element(page, "picker").refreshes, 1);
});

test("a page without a dropdown works", () => {
  const page = loadPage(STATUS_IDS);
  page.receive({ event: "getEntities", items: [{ value: "light.desk", label: "Desk" }] });
  page.receive({ event: "status", mode: "auto", active: "direct", text: "Direct.", hint: "" });
  assert.equal(element(page, "status-text").textContent, "Direct.");
});

test("without the components the script does nothing", () => {
  const page = loadPage(STATUS_IDS, false);
  assert.deepEqual(page.sent, []);
});

test("a message without a payload is ignored", () => {
  const page = loadPage(STATUS_IDS);
  page.receive(undefined);
  page.receive(null);
  assert.equal(element(page, "status-text").textContent, "");
});
