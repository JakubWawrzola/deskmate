import assert from "node:assert/strict";
import { test } from "node:test";
import { encodeLine, FramingError, LineDecoder, MAX_LINE_BYTES } from "../src/channel/framing.ts";

const bytes = (text: string) => new TextEncoder().encode(text);

test("one chunk with one line gives that line", () => {
  assert.deepEqual(new LineDecoder().push(bytes('{"id":1,"ok":true}\n')), ['{"id":1,"ok":true}']);
});

test("several lines merged into one chunk come out one by one", () => {
  const lines = new LineDecoder().push(bytes('{"id":1}\n{"id":2}\n{"ev":"state"}\n'));
  assert.deepEqual(lines, ['{"id":1}', '{"id":2}', '{"ev":"state"}']);
});

test("a line split across chunks is put back together", () => {
  const decoder = new LineDecoder();
  assert.deepEqual(decoder.push(bytes('{"id":1,"o')), []);
  assert.deepEqual(decoder.push(bytes('k":tr')), []);
  assert.deepEqual(decoder.push(bytes('ue}\n{"id"')), ['{"id":1,"ok":true}']);
  assert.deepEqual(decoder.push(bytes(":2}\n")), ['{"id":2}']);
});

test("a chunk that ends exactly on the newline leaves nothing behind", () => {
  const decoder = new LineDecoder();
  assert.deepEqual(decoder.push(bytes("a\n")), ["a"]);
  assert.deepEqual(decoder.push(bytes("b\n")), ["b"]);
});

test("a multi-byte character split across chunks survives", () => {
  const whole = bytes('{"name":"Żółw \u{1F422}"}\n');
  const decoder = new LineDecoder();
  const lines: string[] = [];
  // One byte at a time splits every multi-byte sequence in the line.
  for (const byte of whole) {
    lines.push(...decoder.push(Uint8Array.of(byte)));
  }
  assert.deepEqual(lines, ['{"name":"Żółw \u{1F422}"}']);
});

test("the decoder keeps its own copy of a partial line", () => {
  const decoder = new LineDecoder();
  const chunk = bytes("abc");
  decoder.push(chunk);
  chunk.fill(0x78);
  assert.deepEqual(decoder.push(bytes("\n")), ["abc"]);
});

test("an empty line is passed on as an empty string", () => {
  assert.deepEqual(new LineDecoder().push(bytes("\n")), [""]);
});

test("a line of exactly the limit is accepted", () => {
  const line = "x".repeat(MAX_LINE_BYTES);
  assert.deepEqual(new LineDecoder().push(bytes(`${line}\n`)), [line]);
});

test("a line one byte over the limit is refused", () => {
  const decoder = new LineDecoder();
  assert.throws(
    () => decoder.push(bytes(`${"x".repeat(MAX_LINE_BYTES + 1)}\n`)),
    (error) => error instanceof FramingError && error.code === "line_too_long",
  );
});

test("an oversize line is refused before its newline arrives", () => {
  const decoder = new LineDecoder();
  decoder.push(bytes("x".repeat(MAX_LINE_BYTES)));
  assert.throws(() => decoder.push(bytes("x")), FramingError);
});

test("the limit counts bytes, not characters", () => {
  // Each "ż" is two bytes, so half the limit plus one is already too long.
  const decoder = new LineDecoder();
  assert.throws(() => decoder.push(bytes(`${"ż".repeat(MAX_LINE_BYTES / 2 + 1)}\n`)), FramingError);
});

test("bytes that are not UTF-8 are refused", () => {
  const decoder = new LineDecoder();
  assert.throws(
    () => decoder.push(Uint8Array.of(0x7b, 0xff, 0xfe, 0x7d, 0x0a)),
    (error) => error instanceof FramingError && error.code === "invalid_utf8",
  );
});

test("a message is encoded as one line that ends in a newline", () => {
  const line = new TextDecoder().decode(encodeLine({ id: 1, op: "run", action: "hotkey:lights" }));
  assert.equal(line, '{"id":1,"op":"run","action":"hotkey:lights"}\n');
});

test("a line break inside a value does not break the framing", () => {
  const line = new TextDecoder().decode(encodeLine({ op: "call", data: { text: "a\nb" } }));
  assert.equal(line.indexOf("\n"), line.length - 1);
  assert.deepEqual(new LineDecoder().push(bytes(line)), [line.slice(0, -1)]);
});

test("a message over the limit is not encoded", () => {
  assert.throws(
    () => encodeLine({ op: "call", data: { text: "x".repeat(MAX_LINE_BYTES) } }),
    (error) => error instanceof FramingError && error.code === "line_too_long",
  );
});
