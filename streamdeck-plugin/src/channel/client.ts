import { reconnectDelayMs } from "./backoff.ts";
import { encodeLine, FramingError, LineDecoder } from "./framing.ts";
import {
  CLIENT_NAME,
  type EntityState,
  type HelloInfo,
  type JsonRecord,
  MAX_WATCHED_ENTITIES,
  PROTOCOL_VERSION,
  ProtocolError,
  parseServerLine,
  readHello,
  readStateEvent,
  readStates,
  type ServerMessage,
  type ServerResponse,
} from "./protocol.ts";

/** How long a request waits for its response. */
export const REQUEST_TIMEOUT_MS = 10_000;

/** Why there is no working connection. */
export type ChannelFailure = "unavailable" | "version" | "protocol";

export type ChannelStatus =
  | { state: "idle" }
  | { state: "connecting" }
  | { state: "ready"; app: string; ha: boolean }
  | { state: "down"; reason: ChannelFailure };

/**
 * A request that did not succeed. `code` is the app's error code, or one of
 * the client's own: `not_connected`, `disconnected`, `timeout`, `protocol`,
 * `too_large`.
 */
export class ChannelError extends Error {
  readonly code: string;

  constructor(code: string, message?: string) {
    super(message ?? code);
    this.name = "ChannelError";
    this.code = code;
  }
}

/** The part of a `net.Socket` the client uses. */
export interface ChannelSocket {
  on(event: "connect", listener: () => void): unknown;
  on(event: "data", listener: (chunk: Uint8Array) => void): unknown;
  on(event: "error", listener: (error: Error) => void): unknown;
  on(event: "close", listener: () => void): unknown;
  write(data: Uint8Array): unknown;
  destroy(): void;
}

/** Runs the callback after the delay; the returned function cancels it. */
export type Schedule = (callback: () => void, delayMs: number) => () => void;

export type ChannelLog = {
  info(message: string): void;
  warn(message: string): void;
};

export type ChannelClientOptions = {
  /** Path of the pipe. */
  path: string;
  /** Plugin version, sent in `hello`. */
  version: string;
  /** Opens the pipe. `net.connect` in the plugin, a fake in tests. */
  connect: (path: string) => ChannelSocket;
  schedule?: Schedule;
  log?: ChannelLog;
};

type PendingRequest = {
  resolve: (body: JsonRecord) => void;
  reject: (error: ChannelError) => void;
  cancelTimeout: () => void;
};

type ConnectWaiter = { resolve: () => void; reject: (error: ChannelError) => void };

const systemSchedule: Schedule = (callback, delayMs) => {
  const timer = setTimeout(callback, delayMs);
  return () => clearTimeout(timer);
};

function sameList(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

/**
 * Client of the Deskmate app's local channel: one JSON object per line over a
 * named pipe. It connects when asked to, says `hello` first, matches each
 * response to its request, keeps the watch list across reconnects and retries
 * with a growing delay for as long as the channel is wanted.
 *
 * Messages are never logged: they name the user's entities and actions.
 */
export class ChannelClient {
  private readonly options: ChannelClientOptions;
  private readonly schedule: Schedule;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly statusListeners = new Set<(status: ChannelStatus) => void>();
  private readonly stateListeners = new Set<(state: EntityState) => void>();

  private socket: ChannelSocket | undefined;
  private decoder = new LineDecoder();
  private nextId = 1;
  private hello: HelloInfo | undefined;
  private failure: ChannelFailure | undefined;
  private wanted = false;
  private reconnectAttempt = 0;
  private cancelReconnect: (() => void) | undefined;
  private connectWaiters: ConnectWaiter[] = [];
  private lastStatusKey = JSON.stringify({ state: "idle" });

  private watchList: string[] = [];
  private watchDirty = false;
  private watchInFlight = false;

  constructor(options: ChannelClientOptions) {
    this.options = options;
    this.schedule = options.schedule ?? systemSchedule;
  }

  get status(): ChannelStatus {
    if (this.hello) {
      return { state: "ready", app: this.hello.app, ha: this.hello.ha };
    }
    // While retrying, the status stays "down": flipping to "connecting" for
    // every attempt would only make the listeners redraw.
    if (this.failure) {
      return { state: "down", reason: this.failure };
    }
    return this.socket ? { state: "connecting" } : { state: "idle" };
  }

  onStatus(listener: (status: ChannelStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  /** States of watched entities: the answer to `watch`, then every change. */
  onState(listener: (state: EntityState) => void): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  /**
   * Says whether anything needs the channel right now. While it does, the
   * client connects and keeps reconnecting; when it no longer does, the
   * connection is closed.
   */
  setWanted(wanted: boolean): void {
    if (this.wanted === wanted) {
      return;
    }
    this.wanted = wanted;
    if (wanted) {
      if (!this.socket && !this.cancelReconnect) {
        this.open();
      }
      return;
    }
    this.stopReconnecting();
    this.reconnectAttempt = 0;
    this.failure = undefined;
    if (this.socket) {
      this.drop(undefined);
    } else {
      this.emitStatus();
    }
  }

  /** Resolves once `hello` has succeeded, connecting now if necessary. */
  ensureConnected(): Promise<void> {
    if (this.hello) {
      return Promise.resolve();
    }
    return new Promise((resolve, reject) => {
      this.connectWaiters.push({ resolve, reject });
      if (!this.socket) {
        this.open();
      }
    });
  }

  /** Sends one request and returns the body of its response. */
  async request(op: string, params: JsonRecord = {}): Promise<JsonRecord> {
    await this.ensureConnected();
    return this.send(op, params);
  }

  /**
   * Replaces the list of watched entities. It is sent when the connection is
   * up and again after every reconnect. Entities past the app's limit are
   * left out.
   */
  setWatch(entityIds: readonly string[]): void {
    const unique = [...new Set(entityIds)];
    if (unique.length > MAX_WATCHED_ENTITIES) {
      this.options.log?.warn(
        `More than ${MAX_WATCHED_ENTITIES} entities to watch; the rest get no live state`,
      );
    }
    const list = unique.slice(0, MAX_WATCHED_ENTITIES);
    if (sameList(list, this.watchList)) {
      return;
    }
    this.watchList = list;
    this.watchDirty = true;
    this.flushWatch();
  }

  private open(): void {
    this.stopReconnecting();
    let socket: ChannelSocket;
    try {
      socket = this.options.connect(this.options.path);
    } catch {
      this.connectionEnded("unavailable");
      return;
    }
    this.socket = socket;
    this.decoder = new LineDecoder();
    this.nextId = 1;
    socket.on("connect", () => {
      if (this.socket === socket) {
        this.sayHello();
      }
    });
    socket.on("data", (chunk) => {
      if (this.socket === socket) {
        this.receive(chunk);
      }
    });
    // Node reports a failed connect as "error" followed by "close". Without a
    // listener the error would crash the process; "close" does the work.
    socket.on("error", () => {});
    socket.on("close", () => {
      if (this.socket === socket) {
        this.socket = undefined;
        this.connectionEnded("unavailable");
      }
    });
    this.emitStatus();
  }

  private sayHello(): void {
    this.send("hello", {
      v: PROTOCOL_VERSION,
      client: CLIENT_NAME,
      version: this.options.version,
    }).then(
      (body) => {
        let info: HelloInfo;
        try {
          info = readHello(body);
        } catch {
          // Nothing falls back to another version: the connection is closed.
          this.drop(body.v === PROTOCOL_VERSION ? "protocol" : "version");
          return;
        }
        this.hello = info;
        this.failure = undefined;
        this.reconnectAttempt = 0;
        this.options.log?.info("Connected to the Deskmate app");
        for (const waiter of this.connectWaiters.splice(0)) {
          waiter.resolve();
        }
        this.emitStatus();
        this.watchDirty = this.watchList.length > 0;
        this.flushWatch();
      },
      (error: ChannelError) => {
        if (error.code === "disconnected") {
          return;
        }
        if (error.code === "version") {
          this.drop("version");
        } else {
          this.drop(error.code === "timeout" ? "unavailable" : "protocol");
        }
      },
    );
  }

  private send(op: string, params: JsonRecord): Promise<JsonRecord> {
    const socket = this.socket;
    if (!socket) {
      return Promise.reject(new ChannelError("not_connected"));
    }
    const id = this.nextId;
    this.nextId += 1;
    let line: Uint8Array;
    try {
      line = encodeLine({ id, op, ...params });
    } catch (error) {
      if (error instanceof FramingError) {
        return Promise.reject(new ChannelError("too_large"));
      }
      throw error;
    }
    return new Promise((resolve, reject) => {
      const cancelTimeout = this.schedule(() => {
        this.pending.delete(id);
        reject(new ChannelError("timeout"));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(id, { resolve, reject, cancelTimeout });
      socket.write(line);
    });
  }

  private receive(chunk: Uint8Array): void {
    const socket = this.socket;
    let lines: string[];
    try {
      lines = this.decoder.push(chunk);
    } catch {
      this.drop("protocol");
      return;
    }
    for (const line of lines) {
      let message: ServerMessage;
      try {
        message = parseServerLine(line);
      } catch {
        this.drop("protocol");
        return;
      }
      this.dispatch(message);
      if (this.socket !== socket) {
        return;
      }
    }
  }

  private dispatch(message: ServerMessage): void {
    if (message.kind === "response") {
      this.settle(message);
    } else if (message.kind === "event" && message.ev === "state") {
      let state: EntityState;
      try {
        state = readStateEvent(message.body);
      } catch (error) {
        if (error instanceof ProtocolError) {
          return;
        }
        throw error;
      }
      this.emitState(state);
    }
  }

  private settle(response: ServerResponse): void {
    let id = response.id;
    if (id === undefined) {
      // A refusal with no id can only be matched while `hello` is the one
      // request in flight; anywhere else it is ambiguous and is ignored.
      const [onlyId] = this.pending.keys();
      if (response.ok || this.hello || this.pending.size !== 1 || onlyId === undefined) {
        return;
      }
      id = onlyId;
    }
    const request = this.pending.get(id);
    if (!request) {
      return;
    }
    this.pending.delete(id);
    request.cancelTimeout();
    if (response.ok) {
      request.resolve(response.body);
    } else {
      request.reject(new ChannelError(response.error, response.message));
    }
  }

  private flushWatch(): void {
    if (!this.hello || !this.watchDirty || this.watchInFlight) {
      return;
    }
    this.watchDirty = false;
    this.watchInFlight = true;
    this.send("watch", { entities: this.watchList }).then(
      (body) => {
        this.watchInFlight = false;
        if (this.watchDirty) {
          // The list changed meanwhile; this answer describes the old one.
          this.flushWatch();
          return;
        }
        let states: EntityState[];
        try {
          states = readStates(body);
        } catch (error) {
          if (error instanceof ProtocolError) {
            this.drop("protocol");
            return;
          }
          throw error;
        }
        for (const state of states) {
          this.emitState(state);
        }
      },
      () => {
        // Refused or lost. The list goes out again with the next change or
        // the next connection.
        this.watchInFlight = false;
        this.flushWatch();
      },
    );
  }

  /** Closes the connection from this side. */
  private drop(failure: ChannelFailure | undefined): void {
    const socket = this.socket;
    this.socket = undefined;
    socket?.destroy();
    this.connectionEnded(failure);
  }

  private connectionEnded(failure: ChannelFailure | undefined): void {
    const wasReady = this.hello !== undefined;
    this.hello = undefined;
    this.watchInFlight = false;
    for (const [id, request] of this.pending) {
      this.pending.delete(id);
      request.cancelTimeout();
      request.reject(new ChannelError("disconnected"));
    }
    for (const waiter of this.connectWaiters.splice(0)) {
      waiter.reject(new ChannelError("not_connected"));
    }

    const previous = this.failure;
    this.failure = this.wanted ? failure : undefined;
    if (wasReady) {
      this.options.log?.info("The connection to the Deskmate app closed");
    } else if (this.failure && this.failure !== previous) {
      this.options.log?.info(`No connection to the Deskmate app (${this.failure})`);
    }

    if (this.wanted) {
      const delay = reconnectDelayMs(this.reconnectAttempt);
      this.reconnectAttempt += 1;
      this.cancelReconnect = this.schedule(() => {
        this.cancelReconnect = undefined;
        if (this.wanted && !this.socket) {
          this.open();
        }
      }, delay);
    }
    this.emitStatus();
  }

  private stopReconnecting(): void {
    this.cancelReconnect?.();
    this.cancelReconnect = undefined;
  }

  private emitStatus(): void {
    const status = this.status;
    const key = JSON.stringify(status);
    if (key === this.lastStatusKey) {
      return;
    }
    this.lastStatusKey = key;
    for (const listener of this.statusListeners) {
      listener(status);
    }
  }

  private emitState(state: EntityState): void {
    for (const listener of this.stateListeners) {
      listener(state);
    }
  }
}
