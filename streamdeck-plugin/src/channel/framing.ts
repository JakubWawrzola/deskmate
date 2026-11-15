/** Longest line either side may send, in bytes, not counting the newline. */
export const MAX_LINE_BYTES = 65_536;

const NEWLINE = 0x0a;

/** A line that breaks the framing rules. The connection is closed over it. */
export class FramingError extends Error {
  readonly code: "line_too_long" | "invalid_utf8";

  constructor(code: "line_too_long" | "invalid_utf8") {
    super(code === "line_too_long" ? "A line is longer than the limit" : "A line is not UTF-8");
    this.name = "FramingError";
    this.code = code;
  }
}

/**
 * Cuts the byte stream of the pipe into lines. A chunk can hold several lines
 * or a part of one, and a multi-byte character can be split across chunks, so
 * the bytes are kept as they arrive and decoded one whole line at a time.
 */
export class LineDecoder {
  private readonly decoder = new TextDecoder("utf-8", { fatal: true });
  private pending: Uint8Array[] = [];
  private pendingBytes = 0;

  /** Returns the lines completed by this chunk, without their newline. */
  push(chunk: Uint8Array): string[] {
    const lines: string[] = [];
    let start = 0;
    let end = chunk.indexOf(NEWLINE, start);
    while (end !== -1) {
      lines.push(this.takeLine(chunk.subarray(start, end)));
      start = end + 1;
      end = chunk.indexOf(NEWLINE, start);
    }
    if (start < chunk.length) {
      this.keep(chunk.subarray(start));
    }
    return lines;
  }

  private keep(bytes: Uint8Array): void {
    if (this.pendingBytes + bytes.length > MAX_LINE_BYTES) {
      throw new FramingError("line_too_long");
    }
    // A copy: the caller may reuse the chunk's buffer.
    this.pending.push(bytes.slice());
    this.pendingBytes += bytes.length;
  }

  private takeLine(tail: Uint8Array): string {
    if (this.pendingBytes + tail.length > MAX_LINE_BYTES) {
      throw new FramingError("line_too_long");
    }
    const line = new Uint8Array(this.pendingBytes + tail.length);
    let offset = 0;
    for (const part of this.pending) {
      line.set(part, offset);
      offset += part.length;
    }
    line.set(tail, offset);
    this.pending = [];
    this.pendingBytes = 0;
    try {
      return this.decoder.decode(line);
    } catch {
      throw new FramingError("invalid_utf8");
    }
  }
}

/** One message as the bytes of one line, newline included. */
export function encodeLine(message: Record<string, unknown>): Uint8Array {
  // JSON.stringify escapes line breaks inside strings, so the only newline is
  // the one added here.
  const bytes = new TextEncoder().encode(JSON.stringify(message));
  if (bytes.length > MAX_LINE_BYTES) {
    throw new FramingError("line_too_long");
  }
  const line = new Uint8Array(bytes.length + 1);
  line.set(bytes);
  line[bytes.length] = NEWLINE;
  return line;
}
