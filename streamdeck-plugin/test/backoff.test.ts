import assert from "node:assert/strict";
import { test } from "node:test";
import { reconnectDelayMs } from "../src/channel/backoff.ts";

test("the delay starts at one second and doubles up to thirty", () => {
  const delays = [0, 1, 2, 3, 4, 5, 6, 7].map(reconnectDelayMs);
  assert.deepEqual(delays, [1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
});

test("the delay stays at the cap however long the outage lasts", () => {
  assert.equal(reconnectDelayMs(1000), 30000);
  assert.equal(reconnectDelayMs(Number.MAX_SAFE_INTEGER), 30000);
});

test("a nonsense attempt number gives the first delay", () => {
  assert.equal(reconnectDelayMs(-3), 1000);
  assert.equal(reconnectDelayMs(0.9), 1000);
});
