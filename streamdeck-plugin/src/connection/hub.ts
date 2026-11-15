import {
  type ConnectionMode,
  DEFAULT_CONNECTION_MODE,
  type GlobalSettings,
  parseConnectionMode,
  parseDirectConfig,
} from "../settings.ts";
import { homeAssistantKeysNeedChannel, type Selection, selectTransport } from "./select.ts";
import type { ConnectionStatus } from "./status.ts";
import {
  ConnectionError,
  type DeskmateAction,
  type DeskmateLink,
  type DirectLink,
  type EntityInfo,
  type EntityState,
  type HaTransport,
  type ServiceCall,
} from "./transport.ts";

/** What a visible key needs: Home Assistant, or the Deskmate app itself. */
export type KeyNeed = "homeAssistant" | "deskmate";

/** Gets the state of a watched entity; `undefined` while it is not known. */
export type StateListener = (state: string | undefined) => void;

/**
 * The one place the actions talk to. It owns both transports, sends every
 * Home Assistant operation over the one the `connection` setting selects,
 * and moves the watched entities along when the selection changes.
 */
export class ConnectionHub {
  private readonly deskmate: DeskmateLink;
  private readonly direct: DirectLink;
  private readonly watchers = new Map<string, Set<StateListener>>();
  private readonly states = new Map<string, string>();
  private readonly statusListeners = new Set<() => void>();
  private readonly keys: Record<KeyNeed, number> = { homeAssistant: 0, deskmate: 0 };

  private mode: ConnectionMode = DEFAULT_CONNECTION_MODE;
  private active: Selection = "none";
  private lastStatusKey = "";

  constructor(deskmate: DeskmateLink, direct: DirectLink) {
    this.deskmate = deskmate;
    this.direct = direct;
    deskmate.onStatus(() => this.refresh());
    deskmate.onState((state) => this.receiveState("deskmate", state));
    direct.onState((state) => this.receiveState("direct", state));
    direct.onProblem(() => this.notifyStatus());
    this.lastStatusKey = JSON.stringify(this.status);
  }

  get status(): ConnectionStatus {
    return {
      mode: this.mode,
      active: this.select(),
      channel: this.deskmate.status,
      directConfigured: this.direct.configured,
      directProblem: this.direct.problem,
    };
  }

  /** Called with no argument whenever `status` has changed. */
  onStatus(listener: () => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  /** Applies the plugin's global settings. */
  configure(settings: GlobalSettings): void {
    this.mode = parseConnectionMode(settings.connection);
    this.direct.configure(parseDirectConfig(settings));
    this.refresh();
  }

  /**
   * Registers a visible key. While a key needs the Deskmate app, the channel
   * to it is kept open and retried. Returns the function that unregisters.
   */
  retain(need: KeyNeed): () => void {
    let released = false;
    this.keys[need] += 1;
    this.refresh();
    return () => {
      if (released) {
        return;
      }
      released = true;
      this.keys[need] -= 1;
      this.refresh();
    };
  }

  /**
   * Watches an entity. The listener gets the state known now, then every
   * change; it gets `undefined` when the connection in use changes and the
   * state has to be learned again. Returns the function that stops watching.
   */
  watchEntity(entityId: string, listener: StateListener): () => void {
    let listeners = this.watchers.get(entityId);
    if (!listeners) {
      listeners = new Set();
      this.watchers.set(entityId, listeners);
    }
    listeners.add(listener);
    listener(this.states.get(entityId));
    this.syncWatch();
    return () => {
      const current = this.watchers.get(entityId);
      if (!current?.delete(listener) || current.size > 0) {
        return;
      }
      this.watchers.delete(entityId);
      this.states.delete(entityId);
      this.syncWatch();
    };
  }

  async toggle(entityId: string): Promise<void> {
    const transport = await this.transport();
    await transport.toggle(entityId);
  }

  async callService(call: ServiceCall): Promise<void> {
    const transport = await this.transport();
    await transport.callService(call);
  }

  async listEntities(domains: readonly string[] | undefined): Promise<EntityInfo[]> {
    const transport = await this.transport();
    return transport.listEntities(domains);
  }

  /** The actions defined in Deskmate. Only the app has them. */
  listActions(): Promise<DeskmateAction[]> {
    return this.deskmate.listActions();
  }

  runAction(actionId: string): Promise<void> {
    return this.deskmate.runAction(actionId);
  }

  private select(): Selection {
    return selectTransport(this.mode, this.deskmate.status, this.direct.configured);
  }

  /** The transport for one operation. */
  private async transport(): Promise<HaTransport> {
    if (this.mode === "auto" && this.deskmate.status.state !== "ready") {
      // The app may have been started since the last attempt, so it is tried
      // once more before settling for Direct. A missing pipe fails at once.
      await this.deskmate.ensureConnected().catch(() => undefined);
    }
    const selection = this.select();
    if (selection === "deskmate") {
      return this.deskmate;
    }
    if (selection === "direct") {
      return this.direct;
    }
    throw new ConnectionError(
      "no_connection",
      "Neither the Deskmate app nor a direct connection is available",
    );
  }

  /** Brings everything in line after a setting, a key or the channel changed. */
  private refresh(): void {
    const channelWanted =
      this.keys.deskmate > 0 ||
      (this.keys.homeAssistant > 0 && homeAssistantKeysNeedChannel(this.mode));
    this.deskmate.setWanted(channelWanted);

    const selection = this.select();
    if (selection !== this.active) {
      this.active = selection;
      // What the other connection reported may be stale by now.
      this.states.clear();
      for (const listeners of this.watchers.values()) {
        for (const listener of listeners) {
          listener(undefined);
        }
      }
    }
    this.syncWatch();
    this.notifyStatus();
  }

  private syncWatch(): void {
    const entityIds = [...this.watchers.keys()];
    this.deskmate.watch(this.active === "deskmate" ? entityIds : []);
    this.direct.watch(this.active === "direct" ? entityIds : []);
  }

  private receiveState(from: Selection, state: EntityState): void {
    const listeners = this.watchers.get(state.entityId);
    if (from !== this.active || !listeners) {
      return;
    }
    this.states.set(state.entityId, state.state);
    for (const listener of listeners) {
      listener(state.state);
    }
  }

  private notifyStatus(): void {
    const key = JSON.stringify(this.status);
    if (key === this.lastStatusKey) {
      return;
    }
    this.lastStatusKey = key;
    for (const listener of this.statusListeners) {
      listener();
    }
  }
}
