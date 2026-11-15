export const RECONNECT_MIN_DELAY_MS = 1_000;
export const RECONNECT_MAX_DELAY_MS = 30_000;

// 2^5 seconds is already past the cap; stopping there keeps the number small.
const MAX_DOUBLINGS = 5;

/**
 * Wait before a reconnect attempt. `attempt` counts the waits since the last
 * working connection, from 0: 1 s, 2 s, 4 s, 8 s, 16 s, then 30 s every time.
 */
export function reconnectDelayMs(attempt: number): number {
  const doublings = Math.min(Math.max(0, Math.floor(attempt)), MAX_DOUBLINGS);
  return Math.min(RECONNECT_MIN_DELAY_MS * 2 ** doublings, RECONNECT_MAX_DELAY_MS);
}
