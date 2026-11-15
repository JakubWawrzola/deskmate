import assert from "node:assert/strict";
import { test } from "node:test";
import { pipePathForUser, resolvePipePath, sanitizeUserName } from "../src/channel/pipe-name.ts";

test("a plain user name is lowercased", () => {
  assert.equal(sanitizeUserName("Admin"), "admin");
  assert.equal(sanitizeUserName("user42"), "user42");
});

test("every character outside a-z and 0-9 becomes an underscore", () => {
  assert.equal(sanitizeUserName("Jan Kowalski"), "jan_kowalski");
  assert.equal(sanitizeUserName("anna.nowak-2"), "anna_nowak_2");
  assert.equal(sanitizeUserName("DOMAIN\\Admin"), "domain_admin");
  assert.equal(sanitizeUserName("a_b"), "a_b");
});

test("letters outside ASCII become underscores, one per code point", () => {
  assert.equal(sanitizeUserName("Łukasz"), "_ukasz");
  assert.equal(sanitizeUserName("Zoë"), "zo_");
  // One emoji is one code point but two UTF-16 units.
  assert.equal(sanitizeUserName("pc\u{1F600}"), "pc_");
});

test("lowercasing happens before the replacement", () => {
  // The Kelvin sign lowercases to an ASCII k.
  assert.equal(sanitizeUserName("K"), "k");
  // A capital I with a dot lowercases to "i" plus a combining dot.
  assert.equal(sanitizeUserName("İ"), "i_");
});

test("an empty name stays empty", () => {
  assert.equal(sanitizeUserName(""), "");
});

test("the pipe path is the fixed prefix plus the sanitized name", () => {
  const path = pipePathForUser("My-PC User");
  assert.equal(path, "\\\\.\\pipe\\deskmate-streamdeck-my_pc_user");
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
