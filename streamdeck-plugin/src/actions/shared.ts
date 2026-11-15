/** The part of the plugin's logger the actions use. */
export type ActionLog = {
  error(message: string): void;
};

/**
 * Keeps one registration per visible key, so that the connection the keys
 * need stays up while they are on screen and no longer.
 */
export class VisibleKeys {
  private readonly retain: () => () => void;
  private readonly releases = new Map<string, () => void>();

  constructor(retain: () => () => void) {
    this.retain = retain;
  }

  appeared(keyId: string): void {
    if (!this.releases.has(keyId)) {
      this.releases.set(keyId, this.retain());
    }
  }

  disappeared(keyId: string): void {
    this.releases.get(keyId)?.();
    this.releases.delete(keyId);
  }
}
