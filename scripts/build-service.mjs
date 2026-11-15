// Builds deskmate-service.exe in release mode for the installer's target and
// puts it where the bundler picks it up: src-tauri/service-bin, which
// tauri.conf.json lists as a resource folder.
//
// `tauri build` runs this through beforeBuildCommand and says which target it
// builds for in TAURI_ENV_TARGET_TRIPLE. By hand:
//   node scripts/build-service.mjs [--target <triple>]

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const WINDOWS_TARGETS = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];
const PACKAGE = "deskmate-service";
const EXECUTABLE = "deskmate-service.exe";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const tauriDir = join(root, "src-tauri");

function fail(message) {
  console.error(`build-service: ${message}`);
  process.exit(1);
}

function cargo(args, options) {
  const result = spawnSync("cargo", args, { cwd: tauriDir, ...options });
  if (result.error) fail(`cannot run cargo: ${result.error.message}`);
  if (result.status !== 0) fail(`cargo ${args[0]} failed`);
  return result;
}

function hostTarget() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  if (result.error || result.status !== 0) fail("cannot ask rustc for the host target");
  const host = /^host: (.+)$/m.exec(result.stdout);
  if (!host) fail("rustc did not name its host target");
  return host[1].trim();
}

const flag = process.argv.indexOf("--target");
const target =
  (flag === -1 ? undefined : process.argv[flag + 1]) ??
  process.env.TAURI_ENV_TARGET_TRIPLE ??
  hostTarget();

if (!WINDOWS_TARGETS.includes(target)) {
  // The service exists for Windows only; another target bundles none.
  console.log(`build-service: no service for ${target}, nothing to build`);
  process.exit(0);
}

// --locked: the installer ships what Cargo.lock names, like the app itself.
cargo(["build", "--release", "--locked", "-p", PACKAGE, "--target", target], {
  stdio: "inherit",
});

const metadata = cargo(["metadata", "--format-version", "1", "--no-deps"], {
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});
const targetDir = JSON.parse(metadata.stdout).target_directory;
const built = join(targetDir, target, "release", EXECUTABLE);
if (!existsSync(built)) fail(`${built} was not built`);

const bundleDir = join(tauriDir, "service-bin");
mkdirSync(bundleDir, { recursive: true });
copyFileSync(built, join(bundleDir, EXECUTABLE));
console.log(`build-service: ${EXECUTABLE} for ${target} is in ${bundleDir}`);
