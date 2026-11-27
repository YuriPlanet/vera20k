// The orders `CommandAllowed` refused and the interface's buttons stand
// for, kept for the next module phase (`index.ts` reads them, `shop.apply`
// carries them out).

import { NO_HANDLE } from "@vera20k/sdk";

export const INBOX: i32 = 32;
const INBOX_UNIT = new StaticArray<i64>(INBOX);
const INBOX_CELL = new StaticArray<i64>(INBOX);
const INBOX_COMMAND = new StaticArray<i64>(INBOX);
let inboxLen: i64 = 0;
for (let at = 0; at < INBOX; at++) {
  INBOX_UNIT[at] = NO_HANDLE;
  INBOX_CELL[at] = NO_HANDLE;
}

/** One frame's orders, as `take` copied them. */
export const ORDER_UNIT = new StaticArray<i64>(INBOX);
export const ORDER_COMMAND = new StaticArray<i32>(INBOX);
export const ORDER_CELL = new StaticArray<i64>(INBOX);

/**
 * Hands an order to the next `take`: a refused player order, or an
 * interface button's (`ui`). Writes module memory only, so a decision
 * handler may call it.
 */
export function queue(unit: i64, command: i32, cell: i64): void {
  const len = <i32>inboxLen;
  if (len < INBOX) {
    INBOX_UNIT[len] = unit;
    INBOX_COMMAND[len] = <i64>command;
    INBOX_CELL[len] = cell;
    inboxLen = <i64>(len + 1);
  }
}

/** Forgets the queued orders. */
export function clear(): void {
  inboxLen = 0;
}

/** Moves the queued orders to `ORDER_*` and returns how many there are. */
export function take(): i32 {
  const len = <i32>min<i64>(inboxLen, <i64>INBOX);
  if (len <= 0) return 0;
  inboxLen = 0;
  for (let at = 0; at < len; at++) {
    ORDER_UNIT[at] = INBOX_UNIT[at];
    ORDER_COMMAND[at] = <i32>INBOX_COMMAND[at];
    ORDER_CELL[at] = INBOX_CELL[at];
  }
  return len;
}
