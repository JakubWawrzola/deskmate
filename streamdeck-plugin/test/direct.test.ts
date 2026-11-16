import assert from "node:assert/strict";
import { createServer } from "node:net";
import { test } from "node:test";
import { DirectTransport } from "../src/connection/direct.ts";

/** A local port that nothing listens on: one that was free a moment ago. */
async function closedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address !== null && typeof address === "object");
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return address.port;
}

test("a WebSocket that cannot connect is logged with its code, not the address", async () => {
  const port = await closedPort();
  const lines: string[] = [];
  let socketFailed: () => void = () => {};
  const failed = new Promise<void>((resolve) => {
    socketFailed = resolve;
  });
  const transport = new DirectTransport({
    info: (message) => lines.push(message),
    error: (message) => {
      lines.push(message);
      if (message.startsWith("WebSocket error")) {
        socketFailed();
      }
    },
  });
  try {
    transport.configure({ baseUrl: `http://127.0.0.1:${port}`, token: "test-token" });
    transport.watch(["light.desk"]);
    await failed;
  } finally {
    transport.configure(undefined);
  }
  assert.ok(lines.includes("WebSocket error: ECONNREFUSED"), JSON.stringify(lines));
  for (const line of lines) {
    assert.ok(!line.includes("127.0.0.1") && !line.includes(String(port)), line);
  }
});
