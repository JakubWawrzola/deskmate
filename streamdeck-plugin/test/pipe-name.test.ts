import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { pipePathForUser, resolvePipePath, sanitizeUserName } from "../src/channel/pipe-name.ts";

test("every name of the table shared with the Deskmate app is derived the same way", () => {
  // The app's own test reads the same file (src-tauri/src/streamdeck/pipe_name.rs).
  const table: unknown = JSON.parse(
    readFileSync(new URL("./pipe-names.json", import.meta.url), "utf8"),
  );
  assert.ok(Array.isArray(table) && table.length > 0);
  for (const { user, name } of table as { user: string; name: string }[]) {
    assert.equal(sanitizeUserName(user), name, JSON.stringify(user));
  }
});

test("names that differ get different pipes", () => {
  const table = JSON.parse(readFileSync(new URL("./pipe-names.json", import.meta.url), "utf8")) as {
    user: string;
    name: string;
  }[];
  const names = new Set(table.map(({ name }) => name));
  assert.equal(names.size, table.length);
  for (const [one, other] of [
    ["Иван", "Петр"],
    ["张伟", "李娜"],
    ["Zoë", "Zoé"],
    ["jan.kowalski", "jan kowalski"],
    ["a_b", "a b"],
    ["_5f_", "_"],
  ] as const) {
    assert.notEqual(sanitizeUserName(one), sanitizeUserName(other), one);
  }
});

test("a plain user name is lowercased", () => {
  assert.equal(sanitizeUserName("Admin"), "admin");
  assert.equal(sanitizeUserName("user42"), "user42");
});

test("every character outside a-z and 0-9 becomes its code point in hex", () => {
  assert.equal(sanitizeUserName("Jan Kowalski"), "jan_20_kowalski");
  assert.equal(sanitizeUserName("anna.nowak-2"), "anna_2e_nowak_2d_2");
  assert.equal(sanitizeUserName("DOMAIN\\Admin"), "domain_5c_admin");
  assert.equal(sanitizeUserName("a_b"), "a_5f_b");
});

test("letters outside ASCII become one escape per code point", () => {
  assert.equal(sanitizeUserName("Łukasz"), "_142_ukasz");
  assert.equal(sanitizeUserName("Zoë"), "zo_eb_");
  // One emoji is one code point but two UTF-16 units.
  assert.equal(sanitizeUserName("pc\u{1F600}"), "pc_1f600_");
});

test("lowercasing happens before the replacement", () => {
  // The Kelvin sign lowercases to an ASCII k.
  assert.equal(sanitizeUserName("K"), "k");
  // A capital I with a dot lowercases to "i" plus a combining dot.
  assert.equal(sanitizeUserName("İ"), "i_307_");
});

test("an empty name stays empty", () => {
  assert.equal(sanitizeUserName(""), "");
});

test("the pipe path is the fixed prefix plus the sanitized name", () => {
  const path = pipePathForUser("My-PC User");
  assert.equal(path, "\\\\.\\pipe\\deskmate-streamdeck-my_2d_pc_20_user");
  // Spelled out, so that an escaping mistake in either form shows.
  assert.deepEqual(
    [...path].slice(0, 9).map((character) => character.codePointAt(0)),
    [0x5c, 0x5c, 0x2e, 0x5c, 0x70, 0x69, 0x70, 0x65, 0x5c],
  );
});

test("the plugin connects to the pipe of the current user", () => {
  assert.equal(
    resolvePipePath({}, () => "My-PC User"),
    pipePathForUser("My-PC User"),
  );
});

test("the environment can point the plugin at another pipe", () => {
  let asked = false;
  const path = resolvePipePath({ DESKMATE_STREAMDECK_PIPE: " /tmp/stand-in.sock " }, () => {
    asked = true;
    return "my-pc";
  });
  assert.equal(path, "/tmp/stand-in.sock");
  assert.equal(asked, false);
});

test("an empty override is not an override", () => {
  assert.equal(
    resolvePipePath({ DESKMATE_STREAMDECK_PIPE: "   " }, () => "my-pc"),
    pipePathForUser("my-pc"),
  );
});

test("an unknown user name gives a path nothing listens on", () => {
  const path = resolvePipePath({}, () => {
    throw new Error("no user information");
  });
  assert.equal(path, pipePathForUser(""));
});
