import fs from "node:fs";
import path from "node:path";
import url from "node:url";
import commonjs from "@rollup/plugin-commonjs";
import nodeResolve from "@rollup/plugin-node-resolve";
import typescript from "@rollup/plugin-typescript";

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
    },
  },
  // A warning about code inside a dependency cannot be fixed here; one about
  // the plugin's own code still shows.
  onwarn: (warning, warn) => {
    const modules = warning.ids ?? [warning.id];
    if (modules.every((id) => id?.includes("node_modules"))) return;
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
      },
    },
    typescript({
      mapRoot: isWatching ? "./" : undefined,
    }),
    nodeResolve({
      browser: false,
      exportConditions: ["node"],
      preferBuiltins: true,
    }),
    commonjs(),
    {
      name: "emit-module-package-file",
      generateBundle() {
        this.emitFile({ fileName: "package.json", source: `{ "type": "module" }`, type: "asset" });
      },
    },
  ],
};

export default config;
