/** The plugin's version, as sent to the Deskmate app. Kept equal to package.json by a test. */
export const PLUGIN_VERSION = "0.2.0";

export const PLUGIN_UUID = "com.deskmate.streamdeck";

/** Action identifiers, the same as in manifest.template.json. */
export const ACTION_UUID = {
  toggleEntity: `${PLUGIN_UUID}.toggle`,
  callService: `${PLUGIN_UUID}.call-service`,
  activateScene: `${PLUGIN_UUID}.scene`,
} as const;
