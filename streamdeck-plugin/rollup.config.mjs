import commonjs from "@rollup/plugin-commonjs";
import nodeResolve from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";
import fs from "node:fs";
import path from "node:path";
import url from "node:url";

const isWatching = !!process.env.ROLLUP_WATCH;
const sdPlugin = "com.deskmate.streamdeck.sdPlugin";

/**
 * Modelled on Elgato's streamdeck-plugin-template. Bundles src/plugin.ts,
 * together with @elgato/streamdeck and ws, into bin/plugin.js.
 * @type {import('rollup').RollupOptions}
 */
const config = {
	input: "src/plugin.ts",
	output: {
		file: `${sdPlugin}/bin/plugin.js`,
		format: "es",
		sourcemap: isWatching,
		sourcemapPathTransform: (relativeSourcePath, sourcemapPath) => {
			return url.pathToFileURL(path.resolve(path.dirname(sourcemapPath), relativeSourcePath)).href;
		}
	},
	// ws requires 'bufferutil' and 'utf-8-validate' inside try/catch; they are
	// optional, and without them it falls back to its JavaScript code.
	onwarn: (warning, warn) => {
		if (warning.code === "CIRCULAR_DEPENDENCY") return;
		warn(warning);
	},
	plugins: [
		{
			// The repository keeps the plugin manifest as manifest.template.json:
			// HACS checks need exactly one file named manifest.json in the repo
			// (the Home Assistant integration's). The build writes the real one.
			name: "write-manifest",
			buildStart: function () {
				this.addWatchFile("manifest.template.json");
				fs.copyFileSync("manifest.template.json", `${sdPlugin}/manifest.json`);
			}
		},
		typescript({
			mapRoot: isWatching ? "./" : undefined
		}),
		nodeResolve({
			browser: false,
			exportConditions: ["node"],
			preferBuiltins: true
		}),
		commonjs(),
		{
			name: "emit-module-package-file",
			generateBundle() {
				this.emitFile({ fileName: "package.json", source: `{ "type": "module" }`, type: "asset" });
			}
		}
	]
};

export default config;
