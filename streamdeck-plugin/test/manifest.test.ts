import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { ACTION_UUID, PLUGIN_UUID, PLUGIN_VERSION } from "../src/plugin-info.ts";

type ManifestAction = {
  Name: string;
  UUID: string;
  Icon: string;
  PropertyInspectorPath?: string;
  States: { Image: string }[];
};

type Manifest = {
  UUID: string;
  Version: string;
  Icon: string;
  CategoryIcon: string;
  CodePath: string;
  SDKVersion: number;
  Software: { MinimumVersion: string };
  Nodejs: { Version: string };
  Actions: ManifestAction[];
};

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pluginFolder = join(root, "com.deskmate.streamdeck.sdPlugin");

function readJson<T>(path: string): T {
  // The files are the repository's own, so their shape is trusted here.
  return JSON.parse(readFileSync(join(root, path), "utf8")) as T;
}

const manifest = readJson<Manifest>("manifest.template.json");
const packageJson = readJson<{ version: string }>("package.json");

test("the manifest carries the package version", () => {
  assert.equal(manifest.Version, `${packageJson.version}.0`);
});

test("the version sent to the Deskmate app is the package version", () => {
  assert.equal(PLUGIN_VERSION, packageJson.version);
});

test("the plugin folder is named after the plugin UUID", () => {
  assert.equal(manifest.UUID, PLUGIN_UUID);
  assert.ok(existsSync(pluginFolder));
});

test("the code and the manifest name the same actions", () => {
  const inManifest = manifest.Actions.map((action) => action.UUID).sort();
  assert.deepEqual(inManifest, Object.values(ACTION_UUID).sort());
});

test("every action UUID sits under the plugin UUID and is unique", () => {
  const uuids = manifest.Actions.map((action) => action.UUID);
  for (const uuid of uuids) {
    assert.match(uuid, /^com\.deskmate\.streamdeck\.[a-z0-9-]+$/);
  }
  assert.equal(new Set(uuids).size, uuids.length);
});

test("the manifest asks for what SDK 3 needs", () => {
  assert.equal(manifest.SDKVersion, 3);
  assert.equal(manifest.Software.MinimumVersion, "7.1");
  assert.equal(manifest.Nodejs.Version, "24");
});

test("every image and property inspector in the manifest is in the plugin folder", () => {
  const images = [manifest.Icon, manifest.CategoryIcon];
  const pages: string[] = [];
  for (const action of manifest.Actions) {
    images.push(action.Icon, ...action.States.map((state) => state.Image));
    if (action.PropertyInspectorPath) {
      pages.push(action.PropertyInspectorPath);
    }
  }
  const files = [...images.flatMap((image) => [`${image}.png`, `${image}@2x.png`]), ...pages];
  const missing = files.filter((file) => !existsSync(join(pluginFolder, file)));
  assert.deepEqual(missing, []);
});
