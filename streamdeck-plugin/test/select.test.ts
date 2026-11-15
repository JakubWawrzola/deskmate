import assert from "node:assert/strict";
import { test } from "node:test";
import type { ChannelStatus } from "../src/channel/client.ts";
import { homeAssistantKeysNeedChannel, selectTransport } from "../src/connection/select.ts";
import { type ConnectionStatus, describeStatus } from "../src/connection/status.ts";
import { parseConnectionMode, parseDirectConfig } from "../src/settings.ts";

const READY: ChannelStatus = { state: "ready", app: "0.9.0", ha: true };
const READY_WITHOUT_HA: ChannelStatus = { state: "ready", app: "0.9.0", ha: false };
const DOWN: ChannelStatus = { state: "down", reason: "unavailable" };
const WRONG_VERSION: ChannelStatus = { state: "down", reason: "version" };
const IDLE: ChannelStatus = { state: "idle" };
const CONNECTING: ChannelStatus = { state: "connecting" };

test("auto uses the app when its channel is up", () => {
  assert.equal(selectTransport("auto", READY, false), "deskmate");
  assert.equal(selectTransport("auto", READY, true), "deskmate");
});

test("auto falls back to Direct when the channel is not up and Direct is set", () => {
  for (const channel of [DOWN, WRONG_VERSION, IDLE, CONNECTING]) {
    assert.equal(selectTransport("auto", channel, true), "direct");
  }
});

test("auto has nothing to use when the channel is not up and Direct is not set", () => {
  for (const channel of [DOWN, WRONG_VERSION, IDLE, CONNECTING]) {
    assert.equal(selectTransport("auto", channel, false), "none");
  }
});

test("auto prefers Direct over an app that has no Home Assistant set up", () => {
  assert.equal(selectTransport("auto", READY_WITHOUT_HA, true), "direct");
  // With no Direct either, the app is asked, so its answer says what is missing.
  assert.equal(selectTransport("auto", READY_WITHOUT_HA, false), "deskmate");
});

test("deskmate never falls back, connected or not", () => {
  for (const channel of [READY, READY_WITHOUT_HA, DOWN, IDLE, CONNECTING]) {
    assert.equal(selectTransport("deskmate", channel, true), "deskmate");
    assert.equal(selectTransport("deskmate", channel, false), "deskmate");
  }
});

test("direct ignores the app and needs an address and token", () => {
  assert.equal(selectTransport("direct", READY, true), "direct");
  assert.equal(selectTransport("direct", READY, false), "none");
  assert.equal(selectTransport("direct", DOWN, true), "direct");
});

test("Home Assistant keys need the channel in every mode but direct", () => {
  assert.equal(homeAssistantKeysNeedChannel("auto"), true);
  assert.equal(homeAssistantKeysNeedChannel("deskmate"), true);
  assert.equal(homeAssistantKeysNeedChannel("direct"), false);
});

test("the connection setting defaults to auto", () => {
  assert.equal(parseConnectionMode(undefined), "auto");
  assert.equal(parseConnectionMode(""), "auto");
  assert.equal(parseConnectionMode("Deskmate"), "auto");
  assert.equal(parseConnectionMode(7), "auto");
  assert.equal(parseConnectionMode("auto"), "auto");
  assert.equal(parseConnectionMode("deskmate"), "deskmate");
  assert.equal(parseConnectionMode("direct"), "direct");
});

test("Direct is set only with a web address and a token", () => {
  assert.deepEqual(
    parseDirectConfig({ haUrl: " http://homeassistant.local:8123/ ", haToken: " token " }),
    { baseUrl: "http://homeassistant.local:8123", token: "token" },
  );
  assert.deepEqual(parseDirectConfig({ haUrl: "https://example.com/ha//", haToken: "token" }), {
    baseUrl: "https://example.com/ha",
    token: "token",
  });
  assert.equal(parseDirectConfig({}), undefined);
  assert.equal(parseDirectConfig({ haUrl: "http://homeassistant.local:8123" }), undefined);
  assert.equal(parseDirectConfig({ haToken: "token" }), undefined);
  assert.equal(parseDirectConfig({ haUrl: "   ", haToken: "token" }), undefined);
});

test("an address that is not http or https does not count", () => {
  assert.equal(parseDirectConfig({ haUrl: "homeassistant.local:8123", haToken: "t" }), undefined);
  assert.equal(parseDirectConfig({ haUrl: "ftp://homeassistant.local", haToken: "t" }), undefined);
  assert.equal(parseDirectConfig({ haUrl: "file:///C:/secrets", haToken: "t" }), undefined);
  assert.equal(parseDirectConfig({ haUrl: "not a url", haToken: "t" }), undefined);
});

function status(overrides: Partial<ConnectionStatus>): ConnectionStatus {
  return {
    mode: "auto",
    active: "none",
    channel: DOWN,
    directConfigured: false,
    directProblem: undefined,
    ...overrides,
  };
}

test("the status names the connection in use", () => {
  assert.deepEqual(describeStatus(status({ active: "deskmate", channel: READY })), {
    text: "Connected through the Deskmate app.",
    hint: undefined,
  });
  assert.deepEqual(describeStatus(status({ active: "direct", directConfigured: true })), {
    text: "Using a direct connection to Home Assistant.",
    hint: undefined,
  });
});

test("with neither connection available the hint says how to get one", () => {
  assert.deepEqual(describeStatus(status({})), {
    text: "Not connected.",
    hint: "Open Deskmate, go to Settings and turn on Stream Deck, or enter a Home Assistant address and token here.",
  });
});

test("each mode gets the hint that fits it", () => {
  assert.deepEqual(describeStatus(status({ mode: "deskmate", active: "deskmate" })), {
    text: "Not connected to the Deskmate app.",
    hint: "Open Deskmate, go to Settings and turn on Stream Deck.",
  });
  assert.deepEqual(describeStatus(status({ mode: "direct" })), {
    text: "Not connected.",
    hint: "Enter a Home Assistant address and token here.",
  });
});

test("an app without Home Assistant set up is pointed out", () => {
  const auto = describeStatus(status({ active: "deskmate", channel: READY_WITHOUT_HA }));
  assert.equal(auto.text, "Connected through the Deskmate app.");
  assert.match(auto.hint ?? "", /^Deskmate has no Home Assistant address and token yet\./);
  assert.match(auto.hint ?? "", /or enter a Home Assistant address and token here\.$/);

  const forced = describeStatus(
    status({ mode: "deskmate", active: "deskmate", channel: READY_WITHOUT_HA }),
  );
  assert.equal(
    forced.hint,
    "Deskmate has no Home Assistant address and token yet. Add them in Deskmate.",
  );
});

test("a version mismatch asks for an update instead of a setting", () => {
  const forced = describeStatus(
    status({ mode: "deskmate", active: "deskmate", channel: WRONG_VERSION }),
  );
  assert.match(forced.hint ?? "", /Update both\.$/);
  const auto = describeStatus(status({ channel: WRONG_VERSION }));
  assert.match(
    auto.hint ?? "",
    /Update both\. Or enter a Home Assistant address and token here\.$/,
  );
});

test("a refused token is pointed out on the Direct connection", () => {
  const refused = describeStatus(
    status({ active: "direct", directConfigured: true, directProblem: "auth" }),
  );
  assert.equal(
    refused.hint,
    "Home Assistant refused the access token. Create a new one and paste it here.",
  );
});
