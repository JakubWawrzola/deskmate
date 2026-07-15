import streamDeck from "@elgato/streamdeck";
import WebSocket from "ws";

/** Global plugin settings (entered in the Property Inspector, stored in global settings). */
export type HaGlobalSettings = {
	haUrl?: string;
	haToken?: string;
};

/** Snapshot of a Home Assistant entity state. */
export type HaEntityState = {
	entity_id: string;
	state: string;
	attributes: Record<string, unknown>;
};

type StateListener = (state: HaEntityState | undefined) => void;

const logger = streamDeck.logger.createScope("ha-client");

const RECONNECT_MIN_MS = 2_000;
const RECONNECT_MAX_MS = 30_000;
const PING_INTERVAL_MS = 30_000;
const PONG_TIMEOUT_MS = 10_000;
const REST_TIMEOUT_MS = 10_000;

/**
 * Single shared Home Assistant client for the whole plugin.
 * REST (fetch) to call services, WebSocket to subscribe to state changes.
 * Token and URL come EXCLUSIVELY from global settings — never from code.
 */
class HaClient {
	private baseUrl = "";
	private token = "";
	private ws: WebSocket | undefined;
	private msgId = 0;
	private authFailed = false;
	private reconnectTimer: NodeJS.Timeout | undefined;
	private reconnectDelay = RECONNECT_MIN_MS;
	private pingTimer: NodeJS.Timeout | undefined;
	private pongTimer: NodeJS.Timeout | undefined;
	private getStatesId = -1;

	/** State cache — only for entities that someone subscribes to. */
	private readonly states = new Map<string, HaEntityState>();
	private readonly listeners = new Map<string, Set<StateListener>>();

	public get isConfigured(): boolean {
		return this.baseUrl !== "" && this.token !== "";
	}

	/** Apply global settings; restarts the WS connection when the URL/token changes. */
	public configure(settings: HaGlobalSettings): void {
		const baseUrl = (settings.haUrl ?? "").trim().replace(/\/+$/, "");
		const token = (settings.haToken ?? "").trim();
		if (baseUrl === this.baseUrl && token === this.token) {
			return;
		}
		this.baseUrl = baseUrl;
		this.token = token;
		this.authFailed = false;
		this.states.clear();
		this.restartConnection();
	}

	/** Call an HA service via REST. Throws an Error if not configured or on HTTP error. */
	public async callService(domain: string, service: string, data?: Record<string, unknown>): Promise<void> {
		if (!this.isConfigured) {
			throw new Error("Home Assistant URL or token not configured");
		}
		const res = await fetch(
			`${this.baseUrl}/api/services/${encodeURIComponent(domain)}/${encodeURIComponent(service)}`,
			{
				method: "POST",
				headers: {
					Authorization: `Bearer ${this.token}`,
					"Content-Type": "application/json"
				},
				body: JSON.stringify(data ?? {}),
				signal: AbortSignal.timeout(REST_TIMEOUT_MS)
			}
		);
		if (!res.ok) {
			const body = await res.text().catch(() => "");
			throw new Error(`HA responded ${res.status} ${res.statusText}: ${body.slice(0, 200)}`);
		}
	}

	/**
	 * Subscribe to an entity's state. The listener immediately gets the cached state
	 * (or undefined), then every subsequent change. Returns an unsubscribe function.
	 */
	public onEntityState(entityId: string, listener: StateListener): () => void {
		let set = this.listeners.get(entityId);
		if (!set) {
			set = new Set();
			this.listeners.set(entityId, set);
		}
		set.add(listener);
		listener(this.states.get(entityId));
		this.ensureConnected();
		if (!this.states.has(entityId)) {
			// fetch the initial state via REST (WS get_states only covers the moment of login)
			void this.fetchState(entityId);
		}
		return () => {
			const s = this.listeners.get(entityId);
			if (!s) {
				return;
			}
			s.delete(listener);
			if (s.size === 0) {
				this.listeners.delete(entityId);
				this.states.delete(entityId);
			}
		};
	}

	// ----------------- WebSocket connection -----------------

	private restartConnection(): void {
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = undefined;
		}
		this.stopPing();
		if (this.ws) {
			const old = this.ws;
			this.ws = undefined; // close handler will ignore the old client
			try {
				old.terminate();
			} catch {
				// ignore — the socket may already be closed
			}
		}
		this.reconnectDelay = RECONNECT_MIN_MS;
		this.ensureConnected();
	}

	private ensureConnected(): void {
		if (!this.isConfigured || this.authFailed || this.ws || this.reconnectTimer) {
			return;
		}
		this.openSocket();
	}

	private openSocket(): void {
		const wsUrl = `${this.baseUrl.replace(/^http/i, "ws")}/api/websocket`;
		logger.info(`Connecting to ${wsUrl}`);
		const ws = new WebSocket(wsUrl, { handshakeTimeout: 10_000 });
		this.ws = ws;
		ws.on("message", (raw) => {
			if (this.ws === ws) {
				this.handleMessage(raw);
			}
		});
		ws.on("error", (err) => logger.error(`WebSocket error: ${err.message}`));
		ws.on("close", () => {
			if (this.ws !== ws) {
				return;
			}
			this.ws = undefined;
			this.stopPing();
			this.scheduleReconnect();
		});
	}

	private scheduleReconnect(): void {
		if (!this.isConfigured || this.authFailed || this.reconnectTimer) {
			return;
		}
		logger.info(`Reconnecting in ${this.reconnectDelay} ms`);
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = undefined;
			this.ensureConnected();
		}, this.reconnectDelay);
		this.reconnectDelay = Math.min(this.reconnectDelay * 2, RECONNECT_MAX_MS);
	}

	private handleMessage(raw: WebSocket.RawData): void {
		let msg: {
			type?: string;
			id?: number;
			success?: boolean;
			result?: unknown;
			message?: string;
			event?: {
				event_type?: string;
				data?: { entity_id?: string; new_state?: HaEntityState | null };
			};
		};
		try {
			msg = JSON.parse(raw.toString());
		} catch {
			return;
		}
		switch (msg.type) {
			case "auth_required":
				this.send({ type: "auth", access_token: this.token });
				break;
			case "auth_ok":
				logger.info("WebSocket authenticated");
				this.reconnectDelay = RECONNECT_MIN_MS;
				this.send({ id: ++this.msgId, type: "subscribe_events", event_type: "state_changed" });
				this.getStatesId = ++this.msgId;
				this.send({ id: this.getStatesId, type: "get_states" });
				this.startPing();
				break;
			case "auth_invalid":
				logger.error(`Authentication rejected by HA: ${msg.message ?? "invalid token"}`);
				this.authFailed = true; // don't keep retrying with a bad token
				this.ws?.close();
				break;
			case "result":
				if (msg.id === this.getStatesId && msg.success && Array.isArray(msg.result)) {
					for (const st of msg.result as HaEntityState[]) {
						if (this.listeners.has(st.entity_id)) {
							this.updateState(st);
						}
					}
				}
				break;
			case "event": {
				const data = msg.event?.data;
				if (msg.event?.event_type === "state_changed" && data?.entity_id && this.listeners.has(data.entity_id)) {
					if (data.new_state) {
						this.updateState(data.new_state);
					} else {
						// entity removed from HA
						this.states.delete(data.entity_id);
						this.notify(data.entity_id, undefined);
					}
				}
				break;
			}
			case "pong":
				if (this.pongTimer) {
					clearTimeout(this.pongTimer);
					this.pongTimer = undefined;
				}
				break;
		}
	}

	private send(payload: Record<string, unknown>): void {
		if (this.ws && this.ws.readyState === WebSocket.OPEN) {
			this.ws.send(JSON.stringify(payload));
		}
	}

	private startPing(): void {
		this.stopPing();
		this.pingTimer = setInterval(() => {
			this.send({ id: ++this.msgId, type: "ping" });
			if (!this.pongTimer) {
				this.pongTimer = setTimeout(() => {
					logger.error("Ping timeout — terminating WebSocket");
					this.pongTimer = undefined;
					this.ws?.terminate();
				}, PONG_TIMEOUT_MS);
			}
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

	private updateState(state: HaEntityState): void {
		this.states.set(state.entity_id, state);
		this.notify(state.entity_id, state);
	}

	private notify(entityId: string, state: HaEntityState | undefined): void {
		const set = this.listeners.get(entityId);
		if (!set) {
			return;
		}
		for (const listener of set) {
			try {
				listener(state);
			} catch (err) {
				logger.error(`State listener failed: ${err instanceof Error ? err.message : String(err)}`);
			}
		}
	}

	/** Fetch a single entity's state via REST (initial state for the key). */
	private async fetchState(entityId: string): Promise<void> {
		if (!this.isConfigured) {
			return;
		}
		try {
			const res = await fetch(`${this.baseUrl}/api/states/${encodeURIComponent(entityId)}`, {
				headers: { Authorization: `Bearer ${this.token}` },
				signal: AbortSignal.timeout(REST_TIMEOUT_MS)
			});
			if (res.status === 404) {
				this.notify(entityId, undefined);
				return;
			}
			if (!res.ok) {
				logger.error(`fetchState(${entityId}) HTTP ${res.status}`);
				return;
			}
			const state = (await res.json()) as HaEntityState;
			if (this.listeners.has(entityId)) {
				this.updateState(state);
			}
		} catch (err) {
			logger.error(`fetchState(${entityId}) failed: ${err instanceof Error ? err.message : String(err)}`);
		}
	}
}

/** Singleton — one HA client for the whole plugin. */
export const haClient = new HaClient();
