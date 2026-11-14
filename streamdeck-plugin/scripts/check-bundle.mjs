// Checks the built plugin folder. Runs at the end of `npm run build`.
//
// 1. Every file the manifest points at is there.
// 2. The bundle starts under this Node, registers with a stand-in for the
//    Stream Deck app and answers a key press on each action.
//
// Nothing here needs Stream Deck: the stand-in is a WebSocket server on the
// loopback interface that speaks the few messages the check uses.

import { spawn } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { WebSocketServer } from "ws";

const PLUGIN_FOLDER = "com.deskmate.streamdeck.sdPlugin";
const BOOT_TIMEOUT_MS = 20_000;
const DEVICE_ID = "check-device";

const pluginDir = join(dirname(fileURLToPath(import.meta.url)), "..", PLUGIN_FOLDER);

function fail(message) {
	console.error(`check-bundle: ${message}`);
	process.exit(1);
}

function readManifest() {
	const path = join(pluginDir, "manifest.json");
	if (!existsSync(path)) {
		fail(`${PLUGIN_FOLDER}/manifest.json is missing; run the build first`);
	}
	return JSON.parse(readFileSync(path, "utf8"));
}

/** The manifest names images without an extension; Stream Deck loads the PNG pair. */
function imageFiles(image) {
	return [`${image}.png`, `${image}@2x.png`];
}

function referencedFiles(manifest) {
	const files = [manifest.CodePath, ...imageFiles(manifest.Icon), ...imageFiles(manifest.CategoryIcon)];
	if (manifest.PropertyInspectorPath) {
		files.push(manifest.PropertyInspectorPath);
	}
	for (const action of manifest.Actions) {
		files.push(...imageFiles(action.Icon));
		if (action.PropertyInspectorPath) {
			files.push(action.PropertyInspectorPath);
		}
		for (const state of action.States) {
			files.push(...imageFiles(state.Image));
		}
	}
	// The bundle is an ES module only because of the package.json beside it.
	files.push(join(dirname(manifest.CodePath), "package.json"));
	return [...new Set(files)];
}

function checkFiles(manifest) {
	const files = referencedFiles(manifest);
	const missing = files.filter((file) => !existsSync(join(pluginDir, file)));
	if (missing.length > 0) {
		fail(`files the manifest needs are missing from ${PLUGIN_FOLDER}:\n  ${missing.join("\n  ")}`);
	}
	return files.length;
}

/**
 * Starts the bundle the way Stream Deck does and plays the application's side
 * of the conversation. Resolves once every action answered a key press with
 * an alert: no key is configured, so an alert is the correct answer.
 */
function checkBoot(manifest) {
	// The plugin reads manifest.json from its working directory and writes its
	// log there, so it runs in a copy that is thrown away afterwards.
	const scratch = mkdtempSync(join(tmpdir(), "deskmate-streamdeck-check-"));
	const cwd = join(scratch, PLUGIN_FOLDER);
	mkdirSync(cwd);
	cpSync(join(pluginDir, "manifest.json"), join(cwd, "manifest.json"));

	const contexts = new Map(manifest.Actions.map((action, index) => [`check-context-${index}`, action.UUID]));
	const waitingForAlert = new Set(contexts.keys());

	return new Promise((resolve, reject) => {
		const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
		let child;
		let output = "";

		let finished = false;
		const finish = (error) => {
			if (finished) {
				return;
			}
			finished = true;
			clearTimeout(timer);
			server.close();
			const settle = () => {
				// Windows keeps the folder locked until the process is gone.
				rmSync(scratch, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
				if (error) {
					reject(new Error(output ? `${error.message}\n${output}` : error.message));
				} else {
					resolve(contexts.size);
				}
			};
			if (child && child.exitCode === null && child.signalCode === null) {
				child.once("exit", settle);
				child.kill();
			} else {
				settle();
			}
		};
		const timer = setTimeout(() => {
			const pending = [...waitingForAlert].map((context) => contexts.get(context));
			finish(new Error(`the plugin did not answer in ${BOOT_TIMEOUT_MS} ms; waiting for: ${pending.join(", ")}`));
		}, BOOT_TIMEOUT_MS);

		server.on("connection", (socket) => {
			const send = (message) => socket.send(JSON.stringify(message));
			const pressEveryKey = () => {
				for (const [context, action] of contexts) {
					const key = { settings: {}, coordinates: { column: 0, row: 0 }, isInMultiAction: false, state: 0 };
					send({ event: "willAppear", action, context, device: DEVICE_ID, payload: { ...key, controller: "Keypad" } });
					send({ event: "keyDown", action, context, device: DEVICE_ID, payload: key });
				}
			};

			socket.on("message", (data) => {
				const message = JSON.parse(data.toString());
				if (message.event === "registerPlugin") {
					if (message.uuid !== manifest.UUID) {
						finish(new Error(`the plugin registered as ${message.uuid}, the manifest says ${manifest.UUID}`));
					}
				} else if (message.event === "getGlobalSettings") {
					send({ event: "didReceiveGlobalSettings", id: message.id, payload: { settings: {} } });
					pressEveryKey();
				} else if (message.event === "showAlert") {
					waitingForAlert.delete(message.context);
					if (waitingForAlert.size === 0) {
						finish();
					}
				}
			});
		});

		server.on("listening", () => {
			const info = {
				application: { font: "", language: "en", platform: "windows", platformVersion: "10", version: "7.1.0.0" },
				colors: {},
				devicePixelRatio: 1,
				devices: [{ id: DEVICE_ID, name: "Check", size: { columns: 5, rows: 3 }, type: 0 }],
				plugin: { uuid: manifest.UUID, version: manifest.Version }
			};
			const args = [
				join(pluginDir, manifest.CodePath),
				"-port",
				String(server.address().port),
				"-pluginUUID",
				manifest.UUID,
				"-registerEvent",
				"registerPlugin",
				"-info",
				JSON.stringify(info)
			];
			child = spawn(process.execPath, args, { cwd, stdio: ["ignore", "pipe", "pipe"] });
			child.stdout.on("data", (chunk) => {
				output += chunk;
			});
			child.stderr.on("data", (chunk) => {
				output += chunk;
			});
			child.on("error", (error) => finish(error));
			child.on("exit", (code) => {
				finish(new Error(`the plugin exited with code ${code} before it answered`));
			});
		});
		server.on("error", (error) => finish(error));
	});
}

const manifest = readManifest();
const fileCount = checkFiles(manifest);
console.log(`check-bundle: ${fileCount} files the manifest needs are in ${PLUGIN_FOLDER}`);

try {
	const actionCount = await checkBoot(manifest);
	console.log(`check-bundle: the bundle started, registered and answered a key press on ${actionCount} actions`);
} catch (error) {
	fail(error.message);
}
