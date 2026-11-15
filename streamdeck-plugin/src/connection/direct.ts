import WebSocket from "ws";
import { reconnectDelayMs } from "../channel/backoff.ts";
import type { EntityState, JsonRecord } from "../channel/protocol.ts";
import { parseService } from "../ha-ids.ts";
import type { DirectConfig } from "../settings.ts";
import {
  entitiesFromStates,
  readEntity,
  readSocketMessage,
  UNAVAILABLE_STATE,
  webSocketUrl,
} from "./ha-api.ts";
import {
  ConnectionError,
  type DirectLink,
  type DirectProblem,
  type EntityInfo,
  type ServiceCall,
} from "./transport.ts";

const REST_TIMEOUT_MS = 10_000;
const HANDSHAKE_TIMEOUT_MS = 10_000;
const PING_INTERVAL_MS = 30_000;
const PONG_TIMEOUT_MS = 10_000;
const HTTP_UNAUTHORIZED = 401;
const HTTP_NOT_FOUND = 404;

export type DirectLog = {
  info(message: string): void;
  error(message: string): void;
};

/**
 * Home Assistant without the Deskmate app: service calls and entity lists go
 * over its REST API, state changes come over its WebSocket API. The address
 * and the token come from the plugin's global settings.
 *
 * The WebSocket is open only while entities are watched, so a plugin that
 * works through the app holds no connection of its own.
 */
export class DirectTransport implements DirectLink {
  private readonly log: DirectLog;
  private readonly watched = new Set<string>();
  private readonly stateListeners = new Set<(state: EntityState) => void>();
  private readonly problemListeners = new Set<() => void>();

  private config: DirectConfig | undefined;
  private currentProblem: DirectProblem | undefined;
  private socket: WebSocket | undefined;
  private messageId = 0;
  private statesRequestId = -1;
  private reconnectAttempt = 0;
  private reconnectTimer: NodeJS.Timeout | undefined;
  private pingTimer: NodeJS.Timeout | undefined;
  private pongTimer: NodeJS.Timeout | undefined;

  constructor(log: DirectLog) {
    this.log = log;
  }

  get configured(): boolean {
    return this.config !== undefined;
  }

  get problem(): DirectProblem | undefined {
    return this.currentProblem;
  }

  onState(listener: (state: EntityState) => void): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  onProblem(listener: () => void): () => void {
    this.problemListeners.add(listener);
    return () => this.problemListeners.delete(listener);
  }

  /** Applies the address and token; a change restarts the WebSocket. */
  configure(config: DirectConfig | undefined): void {
    if (config?.baseUrl === this.config?.baseUrl && config?.token === this.config?.token) {
      return;
    }
    this.config = config;
    this.setProblem(undefined);
    this.closeSocket();
    this.reconnectAttempt = 0;
    this.syncSocket();
    for (const entityId of this.watched) {
      void this.fetchState(entityId);
    }
  }

  watch(entityIds: readonly string[]): void {
    const added = entityIds.filter((entityId) => !this.watched.has(entityId));
    this.watched.clear();
    for (const entityId of entityIds) {
      this.watched.add(entityId);
    }
    this.syncSocket();
    // The WebSocket reports changes; the state right now comes over REST.
    for (const entityId of new Set(added)) {
      void this.fetchState(entityId);
    }
  }

  async toggle(entityId: string): Promise<void> {
    await this.post("homeassistant", "toggle", { entity_id: entityId });
  }

  async callService(call: ServiceCall): Promise<void> {
    const service = parseService(call.service);
    if (!service) {
      throw new ConnectionError("bad_request", "The service is not written as domain.service");
    }
    const body: JsonRecord = { ...call.data };
    if (call.entityId !== undefined) {
      body.entity_id = call.entityId;
    }
    await this.post(service.domain, service.service, body);
  }

  async listEntities(domains: readonly string[] | undefined): Promise<EntityInfo[]> {
    const response = await this.send("/api/states");
    if (!response.ok) {
      throw new ConnectionError("ha_error", `Home Assistant answered ${response.status}`);
    }
    return entitiesFromStates(await response.json(), domains);
  }

  private async post(domain: string, service: string, body: JsonRecord): Promise<void> {
    const path = `/api/services/${encodeURIComponent(domain)}/${encodeURIComponent(service)}`;
    const response = await this.send(path, body);
    if (!response.ok) {
      throw new ConnectionError("ha_error", `Home Assistant answered ${response.status}`);
    }
  }

  /**
   * A GET, or a POST when there is a body. Throws when Home Assistant does
   * not answer or refuses the token; any other status is the caller's to read.
   */
  private async send(path: string, body?: JsonRecord): Promise<Response> {
    const config = this.config;
    if (!config) {
      throw new ConnectionError("ha_not_configured", "No Home Assistant address and token are set");
    }
    let response: Response;
    try {
      const authorization = { Authorization: `Bearer ${config.token}` };
      response = await fetch(`${config.baseUrl}${path}`, {
        method: body ? "POST" : "GET",
        headers: body ? { ...authorization, "Content-Type": "application/json" } : authorization,
        body: body ? JSON.stringify(body) : undefined,
        signal: AbortSignal.timeout(REST_TIMEOUT_MS),
      });
    } catch {
      throw new ConnectionError("ha_unreachable", "Home Assistant did not answer");
    }
    if (response.status === HTTP_UNAUTHORIZED) {
      this.setProblem("auth");
      throw new ConnectionError("ha_auth", "Home Assistant refused the access token");
    }
    return response;
  }

  private async fetchState(entityId: string): Promise<void> {
    if (!this.config) {
      return;
    }
    try {
      const response = await this.send(`/api/states/${encodeURIComponent(entityId)}`);
      if (response.status === HTTP_NOT_FOUND) {
        this.emitState({ entityId, state: UNAVAILABLE_STATE });
        return;
      }
      if (!response.ok) {
        this.log.error(`Reading an entity state failed: HTTP ${response.status}`);
        return;
      }
      const entity = readEntity(await response.json());
      if (entity) {
        this.emitState({ entityId, state: entity.state });
      }
    } catch (error) {
      const reason = error instanceof ConnectionError ? error.code : "unexpected error";
      this.log.error(`Reading an entity state failed: ${reason}`);
    }
  }

  /** Opens or closes the WebSocket to match what is configured and watched. */
  private syncSocket(): void {
    const wanted = this.config !== undefined && this.watched.size > 0 && !this.currentProblem;
    if (!wanted) {
      this.closeSocket();
      return;
    }
    if (!this.socket && !this.reconnectTimer) {
      this.openSocket();
    }
  }

  private openSocket(): void {
    const config = this.config;
    if (!config) {
      return;
    }
    this.log.info("Connecting to Home Assistant");
    const socket = new WebSocket(webSocketUrl(config.baseUrl), {
      handshakeTimeout: HANDSHAKE_TIMEOUT_MS,
    });
    this.socket = socket;
    socket.on("message", (raw) => {
      if (this.socket === socket) {
        this.handleMessage(socket, config, raw.toString());
      }
    });
    socket.on("error", (error) => this.log.error(`WebSocket error: ${error.message}`));
    socket.on("close", () => {
      if (this.socket !== socket) {
        return;
      }
      this.socket = undefined;
      this.stopPing();
      this.scheduleReconnect();
    });
  }

  private closeSocket(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
    this.stopPing();
    const socket = this.socket;
    // Cleared first, so that the close handler ignores this socket.
    this.socket = undefined;
    socket?.terminate();
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) {
      return;
    }
    const delay = reconnectDelayMs(this.reconnectAttempt);
    this.reconnectAttempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      this.syncSocket();
    }, delay);
  }

  private handleMessage(socket: WebSocket, config: DirectConfig, raw: string): void {
    const message = readSocketMessage(raw);
    switch (message.type) {
      case "auth_required":
        this.write(socket, { type: "auth", access_token: config.token });
        break;
      case "auth_ok":
        this.log.info("Home Assistant accepted the connection");
        this.reconnectAttempt = 0;
        this.write(socket, {
          id: this.nextMessageId(),
          type: "subscribe_events",
          event_type: "state_changed",
        });
        this.statesRequestId = this.nextMessageId();
        this.write(socket, { id: this.statesRequestId, type: "get_states" });
        this.startPing(socket);
        break;
      case "auth_invalid":
        this.log.error("Home Assistant refused the access token");
        // Retrying with a refused token would only fill Home Assistant's log.
        this.setProblem("auth");
        this.closeSocket();
        break;
      case "result":
        if (message.id === this.statesRequestId) {
          for (const entity of message.states) {
            this.emitState({ entityId: entity.entityId, state: entity.state });
          }
        }
        break;
      case "state_changed":
        this.emitState({ entityId: message.entityId, state: message.state });
        break;
      case "pong":
        if (this.pongTimer) {
          clearTimeout(this.pongTimer);
          this.pongTimer = undefined;
        }
        break;
      case "other":
        break;
    }
  }

  private nextMessageId(): number {
    this.messageId += 1;
    return this.messageId;
  }

  private write(socket: WebSocket, payload: JsonRecord): void {
    if (socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(payload));
    }
  }

  private startPing(socket: WebSocket): void {
    this.stopPing();
    this.pingTimer = setInterval(() => {
      this.write(socket, { id: this.nextMessageId(), type: "ping" });
      this.pongTimer ??= setTimeout(() => {
        this.log.error("Home Assistant stopped answering; reconnecting");
        this.pongTimer = undefined;
        socket.terminate();
      }, PONG_TIMEOUT_MS);
    }, PING_INTERVAL_MS);
  }

  private stopPing(): void {
    if (this.pingTimer) {
      clearInterval(this.pingTimer);
      this.pingTimer = undefined;
    }
    if (this.pongTimer) {
      clearTimeout(this.pongTimer);
      this.pongTimer = undefined;
    }
  }

  private setProblem(problem: DirectProblem | undefined): void {
    if (this.currentProblem === problem) {
      return;
    }
    this.currentProblem = problem;
    for (const listener of this.problemListeners) {
      listener();
    }
  }

  private emitState(state: EntityState): void {
    if (!this.watched.has(state.entityId)) {
      return;
    }
    for (const listener of this.stateListeners) {
      listener(state);
    }
  }
}
