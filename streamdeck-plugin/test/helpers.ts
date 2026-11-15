import type { ChannelSocket, Schedule } from "../src/channel/client.ts";
import type { JsonRecord } from "../src/channel/protocol.ts";

type Listeners = {
  connect: (() => void)[];
  data: ((chunk: Uint8Array) => void)[];
  error: ((error: Error) => void)[];
  close: (() => void)[];
};

/** A socket the test drives by hand. */
export class FakeSocket implements ChannelSocket {
  destroyed = false;
  private readonly listeners: Listeners = { connect: [], data: [], error: [], close: [] };
  private readonly writtenLines: string[] = [];

  on(event: "connect", listener: () => void): this;
  on(event: "data", listener: (chunk: Uint8Array) => void): this;
  on(event: "error", listener: (error: Error) => void): this;
  on(event: "close", listener: () => void): this;
  on(event: keyof Listeners, listener: never): this {
    this.listeners[event].push(listener);
    return this;
  }

  write(data: Uint8Array): boolean {
    this.writtenLines.push(new TextDecoder().decode(data));
    return true;
  }

  destroy(): void {
    this.destroyed = true;
  }

  /** The pipe opened. */
  open(): void {
    for (const listener of this.listeners.connect) {
      listener();
    }
  }

  /** The server sent these bytes; a string is sent as UTF-8. */
  receive(data: string | Uint8Array): void {
    const chunk = typeof data === "string" ? new TextEncoder().encode(data) : data;
    for (const listener of this.listeners.data) {
      listener(chunk);
    }
  }

  /** The server sent this message as one line. */
  reply(message: JsonRecord): void {
    this.receive(`${JSON.stringify(message)}\n`);
  }

  /** The connection ended, the way Node reports it. */
  close(): void {
    for (const listener of this.listeners.close) {
      listener();
    }
  }

  /** Connecting failed: Node emits "error", then "close". */
  fail(): void {
    for (const listener of this.listeners.error) {
      listener(new Error("ENOENT"));
    }
    this.close();
  }

  /** Every request written so far. */
  get sent(): JsonRecord[] {
    return this.writtenLines.map((line) => {
      if (!line.endsWith("\n")) {
        throw new Error("A request without its newline");
      }
      return JSON.parse(line) as JsonRecord;
    });
  }

  get lastSent(): JsonRecord {
    const last = this.sent.at(-1);
    if (!last) {
      throw new Error("Nothing was sent");
    }
    return last;
  }
}

type Timer = { at: number; callback: () => void; cancelled: boolean };

/** Timers that move only when the test says so. */
export class FakeClock {
  private now = 0;
  private timers: Timer[] = [];

  readonly schedule: Schedule = (callback, delayMs) => {
    const timer: Timer = { at: this.now + delayMs, callback, cancelled: false };
    this.timers.push(timer);
    return () => {
      timer.cancelled = true;
    };
  };

  /** Delays of the timers still waiting, shortest first. */
  get waiting(): number[] {
    return this.timers
      .filter((timer) => !timer.cancelled)
      .map((timer) => timer.at - this.now)
      .sort((a, b) => a - b);
  }

  advance(ms: number): void {
    const target = this.now + ms;
    for (;;) {
      const due = this.timers
        .filter((timer) => !timer.cancelled && timer.at <= target)
        .sort((a, b) => a.at - b.at)[0];
      if (!due) {
        break;
      }
      due.cancelled = true;
      this.now = due.at;
      due.callback();
    }
    this.now = target;
  }
}

/** Lets every promise callback that is already queued run. */
export function settle(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}
