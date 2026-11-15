import type { ChannelStatus } from "../channel/client.ts";
import type { DeskmateAction, EntityState, JsonRecord } from "../channel/protocol.ts";
import type { DirectConfig } from "../settings.ts";

export type { DeskmateAction, EntityState };

/** An entity as offered in a property inspector. */
export type EntityInfo = { entityId: string; name: string; state: string };

export type ServiceCall = {
  /** `domain.service`, for example `scene.turn_on`. */
  service: string;
  entityId: string | undefined;
  data: JsonRecord | undefined;
};

/**
 * One way of reaching Home Assistant. The plugin has two: through the
 * Deskmate app and directly. Every action works over either.
 */
export interface HaTransport {
  toggle(entityId: string): Promise<void>;
  callService(call: ServiceCall): Promise<void>;
  /** Entities of the given domains, or of every domain. */
  listEntities(domains: readonly string[] | undefined): Promise<EntityInfo[]>;
  /** Replaces the set of entities whose state is reported through `onState`. */
  watch(entityIds: readonly string[]): void;
  /** The state of a watched entity: once after `watch`, then on every change. */
  onState(listener: (state: EntityState) => void): () => void;
}

/** The transport through the Deskmate app, and what only the app can do. */
export interface DeskmateLink extends HaTransport {
  readonly status: ChannelStatus;
  onStatus(listener: (status: ChannelStatus) => void): () => void;
  /** Whether any key needs the app right now; drives connecting and retrying. */
  setWanted(wanted: boolean): void;
  ensureConnected(): Promise<void>;
  /** The actions the user defined in Deskmate. */
  listActions(): Promise<DeskmateAction[]>;
  runAction(actionId: string): Promise<void>;
}

/** Why the Direct connection is set up but not usable. */
export type DirectProblem = "auth";

/** The transport that talks to Home Assistant itself. */
export interface DirectLink extends HaTransport {
  readonly configured: boolean;
  readonly problem: DirectProblem | undefined;
  configure(config: DirectConfig | undefined): void;
  onProblem(listener: () => void): () => void;
}

/** An operation that could not be carried out; `code` says why, in one word. */
export class ConnectionError extends Error {
  readonly code: string;

  constructor(code: string, message?: string) {
    super(message ?? code);
    this.name = "ConnectionError";
    this.code = code;
  }
}
