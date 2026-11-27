// Notifications from the simulation module to the client modules of its
// source (RFC 0001 section 10 "Communication", appendix 1.1 `send_notify`).
// Messages the other way arrive at `onMessage` (triggers.ts).
//
// Declare every name the module sends in `vera.json`:
//
//   { "notify": ["Reinforced"] }
//
// The build writes the `vera.notify` section and generates
// `src/generated/notify.ts` with `export const Reinforced = new Notify("Reinforced");`.
//
//   import { Reinforced } from "./generated/notify";
//   onMessage((sender: House, data: Uint8Array): void => {
//     if (data.length == 1 && data[0] == 1) Reinforced.send(sender);
//   });

import * as sys from "./sys";
import { Status, check, NO_HANDLE, NO_ID, NameKind, House, lookup } from "./core";

/** The largest message (RFC 15.4 "Messages"): what `onMessage` receives at most. */
export const MAX_MESSAGE_BYTES: i32 = 96;
/** The largest notification (RFC 15.4 "Notifications"). */
export const MAX_NOTIFY_BYTES: i32 = 256;

const EMPTY = new Uint8Array(0);

/**
 * A notification name this module declared in `vera.json`. Its ID (the
 * name's FNV-1a hash, RFC 10) is resolved with `lookup(Notify, name)` on
 * first use.
 */
export class Notify {
  // −1 is no declared name's hash (RFC 3, step 7).
  private resolved: i32 = NO_ID;

  constructor(public readonly name: string) {}

  /**
   * The notification's ID: the same on every machine and in the client
   * world, where a `Notify` event's `id` carries it. `NO_ID` for a name
   * not declared (`lastStatus()` is `NotFound`).
   */
  get id(): i32 {
    if (this.resolved == NO_ID) this.resolved = lookup(NameKind.Notify, this.name);
    return this.resolved;
  }

  /**
   * Notifies the player of `to` with `data` (at most `MAX_NOTIFY_BYTES`).
   * `Refused` over the size or per-frame count limit.
   */
  send(to: House, data: Uint8Array = EMPTY): Status {
    const id = this.id;
    if (id == NO_ID) return Status.NotFound;
    return sendNotify(to, id, data);
  }

  /** Notifies every observer client with `data`. */
  sendObservers(data: Uint8Array = EMPTY): Status {
    return this.send(NO_HANDLE, data);
  }
}

/** `send_notify(to, id, ptr, len)`: `to` 0 addresses the observers. */
export function sendNotify(to: i64, id: i32, data: Uint8Array): Status {
  return check(sys.send_notify(to, id, data.dataStart, data.length));
}
