import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const ui = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "com.deskmate.streamdeck.sdPlugin",
  "ui",
);
const pages = readdirSync(ui).filter((name) => name.endsWith(".html"));

// The copy of Elgato's sdpi-components that ships with the plugin: release
// 4.0.1, the file dist/sdpi-components.js of the tag v4.0.1 of
// https://github.com/GeekyEggo/sdpi-components, unmodified. To move to
// another release, replace the file and both values together.
const SDPI_COMPONENTS_VERSION = "4.0.1";
const SDPI_COMPONENTS_SHA256 = "f6c0dfd2ed68e18084b9952842b86e3850cf837d674704700c2a0718e0a24f6b";

/** What a page loads: the values of its `src` and `href` attributes. */
function loaded(page: string): string[] {
  const html = readFileSync(join(ui, page), "utf8");
  return [...html.matchAll(/(?:src|href)\s*=\s*["']([^"']+)["']/g)].map((match) => match[1] ?? "");
}

test("there are property inspector pages to check", () => {
  assert.ok(pages.length >= 4);
});

test("a property inspector loads nothing from the network", () => {
  // A script from another address could read the plugin's settings, the
  // Home Assistant token among them.
  for (const page of pages) {
    for (const target of loaded(page)) {
      assert.doesNotMatch(target, /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i, `${page} loads ${target}`);
      assert.ok(existsSync(join(ui, target)), `${page} loads ${target}, which is missing`);
    }
  }
});

test("every property inspector loads the copy of sdpi-components in the plugin", () => {
  for (const page of pages) {
    assert.ok(loaded(page).includes("vendor/sdpi-components.js"), page);
  }
});

test("the copy of sdpi-components is the pinned release, unmodified", () => {
  const library = readFileSync(join(ui, "vendor", "sdpi-components.js"));
  assert.equal(createHash("sha256").update(library).digest("hex"), SDPI_COMPONENTS_SHA256);
  assert.ok(
    library
      .subarray(0, 400)
      .toString("utf8")
      .includes(`sdpi-components v${SDPI_COMPONENTS_VERSION},`),
  );
});

test("the licences of the shipped library are next to it", () => {
  const mit = readFileSync(join(ui, "vendor", "sdpi-components.LICENSE.md"), "utf8");
  assert.match(mit, /^MIT License/);
  assert.match(mit, /Corsair Memory Inc\./);
  // The library file contains Lit, which has its own licence.
  const bsd = readFileSync(join(ui, "vendor", "lit.LICENSE.txt"), "utf8");
  assert.match(bsd, /^BSD 3-Clause License/);
  assert.match(bsd, /Google LLC/);
});
