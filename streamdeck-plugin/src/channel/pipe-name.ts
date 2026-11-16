/** Every pipe of the channel starts with this; the sanitized user name follows. */
export const PIPE_NAME_PREFIX = "deskmate-streamdeck-";

/**
 * Environment variable that replaces the pipe path. It exists for the build
 * check and for development, where a stand-in for the app listens elsewhere.
 */
export const PIPE_PATH_VARIABLE = "DESKMATE_STREAMDECK_PIPE";

const WINDOWS_PIPE_ROOT = "\\\\.\\pipe\\";

/**
 * The user name as it appears in the pipe name: lowercased, then every
 * character outside a-z and 0-9 written as its code point in lowercase hex
 * between two underscores (`é` is `_e9_`). A character is a Unicode code
 * point, so one emoji gives one escape; lowercasing happens first, so a
 * letter whose lowercase form is two code points gives two. Every underscore
 * opens or closes such an escape, so two names that differ after lowercasing
 * never share a pipe.
 */
export function sanitizeUserName(userName: string): string {
  return userName
    .toLowerCase()
    .replace(/[^a-z0-9]/gu, (character) => `_${codePointOf(character).toString(16)}_`);
}

function codePointOf(character: string): number {
  // With the `u` flag a match is one whole code point, never empty.
  return character.codePointAt(0) ?? 0;
}

/** Full path of the Deskmate app's pipe for a Windows user. */
export function pipePathForUser(userName: string): string {
  return `${WINDOWS_PIPE_ROOT}${PIPE_NAME_PREFIX}${sanitizeUserName(userName)}`;
}

/**
 * The path the plugin connects to: the override from the environment when
 * one is set, otherwise the pipe of the current user. `currentUserName` is
 * only called when it is needed; if it throws, the path is one that nothing
 * listens on, so the channel reports the app as not reachable.
 */
export function resolvePipePath(
  environment: Record<string, string | undefined>,
  currentUserName: () => string,
): string {
  const override = environment[PIPE_PATH_VARIABLE]?.trim();
  if (override) {
    return override;
  }
  try {
    return pipePathForUser(currentUserName());
  } catch {
    return `${WINDOWS_PIPE_ROOT}${PIPE_NAME_PREFIX}`;
  }
}
