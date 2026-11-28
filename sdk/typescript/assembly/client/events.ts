// Client callbacks (appendix 2.2) and client event views (appendix 4.6).
// `onStart`, `onRender` and `onClientEvent` register at top level; the
// SDK's `client_on_start`, `client_on_render` and `client_on_events`
// exports call them. Each render frame delivers the frame's events first,
// then renders.
//
//   let buy: Node = NO_HANDLE;
//   onStart((): void => { buy = Ui.find("Buy"); Node.setClickable(buy, true); });
//   onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
//     if (e.node == buy) Client.sendMessage(BUY);
//   });
//
// Event views read the `ClientEventRecord` (32 bytes) in place and are
// valid only during their callback; keep the values, not the view.

import * as sys from "../sys";
import { Cell } from "../core";
import { EventRecord } from "../records";
import { ClientEvent } from "../generated/client_tables";
import { Node } from "./api";

// ------------------------------------------------------------ Callbacks

// Each registration runs, in registration order.
const startHooks = new Array<usize>();
const renderHooks = new Array<usize>();
const eventKinds = new Array<i32>();
const eventFns = new Array<usize>();

/** Runs `fn` once the match interface is built (and again after a rejoining player's catch-up). */
export function onStart(fn: () => void): void {
  startHooks.push(changetype<usize>(fn));
}

/** Runs `fn(dt, alpha)` once per render frame, after the frame's events. Both serve presentation only. */
export function onRender(fn: (dt: f32, alpha: f32) => void): void {
  renderHooks.push(changetype<usize>(fn));
}

/**
 * Runs `fn` for every client event of `kind`, in delivery order. `T` is
 * the kind's view (`ClickEvent`, `NotifyEvent`, ...) or `ClientEventRecord`.
 * Several handlers of one kind run in registration order.
 */
export function onClientEvent<T>(kind: ClientEvent, fn: (e: T) => void): void {
  eventKinds.push(<i32>kind);
  eventFns.push(changetype<usize>(fn));
}

/** @internal `client_on_start`. */
export function runStart(): void {
  for (let i = 0; i < startHooks.length; i++) changetype<() => void>(unchecked(startHooks[i]))();
}

/** @internal `client_on_render(dt, alpha)`. */
export function runRender(dt: f32, alpha: f32): void {
  for (let i = 0; i < renderHooks.length; i++) {
    changetype<(dt: f32, alpha: f32) => void>(unchecked(renderHooks[i]))(dt, alpha);
  }
}

/** @internal `client_on_events(ptr, count)`. */
export function runClientEvents(ptr: usize, count: i32): void {
  for (let i = 0; i < count; i++) {
    const record = ptr + <usize>(i * sys.CLIENT_EVENT_RECORD_BYTES);
    const kind = load<i32>(record, sys.CR_KIND);
    for (let j = 0; j < eventKinds.length; j++) {
      if (unchecked(eventKinds[j]) == kind) changetype<(e: usize) => void>(unchecked(eventFns[j]))(record);
    }
  }
}

// ---------------------------------------------------------------- Views

/** One client event (`ClientEventRecord`, 32 bytes), every field. */
@unmanaged
export class ClientEventRecord {
  /** The event kind (`ClientEvent`). */
  get kind(): ClientEvent {
    return <ClientEvent>load<i32>(changetype<usize>(this), sys.CR_KIND);
  }
  /** `Notify`: the sending module's source, 0 mod, 1 map. */
  get source(): i32 {
    return load<i32>(changetype<usize>(this), sys.CR_SOURCE);
  }
  /** The node (`Click`, `Hover`, `Dismiss`) or cell (`MapClick`). */
  get ref(): i64 {
    return load<i64>(changetype<usize>(this), sys.CR_REF);
  }
  /** `Hotkey`: the hotkey ID; `Notify`: the notification ID. */
  get id(): i32 {
    return load<i32>(changetype<usize>(this), sys.CR_ID);
  }
  get value(): i32 {
    return load<i32>(changetype<usize>(this), sys.CR_VALUE);
  }
  /** The address of `Notify` bytes or a `SimEvent`'s `EventRecord` in the delivery area (0 for none). */
  get dataPtr(): usize {
    return <usize>load<i32>(changetype<usize>(this), sys.CR_DATA);
  }
  get dataLen(): i32 {
    return load<i32>(changetype<usize>(this), sys.CR_DATA_LEN);
  }

  /** `Click`, `Hover`, `Dismiss`: the node. */
  get node(): Node {
    return this.ref;
  }
  /** `MapClick`: the cell. */
  get cell(): Cell {
    return this.ref;
  }
  /** `Notify`: whether the sender is the map's simulation module (else the mod's). */
  get fromMap(): bool {
    return this.source == 1;
  }
  /** `Notify`: a copy of the notification's bytes; empty for other events. */
  get data(): Uint8Array {
    const len = this.kind == ClientEvent.Notify ? max(this.dataLen, 0) : 0;
    const out = new Uint8Array(len);
    if (len > 0) memory.copy(out.dataStart, this.dataPtr, <usize>len);
    return out;
  }
  /** `Notify`: byte `i` of the data, read in place (0 past the end). */
  dataAt(i: i32): u8 {
    if (this.kind != ClientEvent.Notify || i < 0 || i >= this.dataLen) return 0;
    return load<u8>(this.dataPtr + <usize>i);
  }
  /**
   * `SimEvent`: the forwarded `EventRecord`, read in place (`other` is 0
   * when this player cannot see it); `null` for other events.
   */
  get simEvent(): EventRecord | null {
    if (this.kind != ClientEvent.SimEvent || this.dataLen != sys.EVENT_RECORD_BYTES) return null;
    const at = this.dataPtr;
    if (at == 0 || (at & 7) != 0) return null;
    return changetype<EventRecord>(at);
  }
}

/** `Click` on a clickable node. */
@unmanaged
export class ClickEvent extends ClientEventRecord {
  /** The zero-based `List` row or `CameoGrid` item, −1 for other nodes. */
  get itemIndex(): i32 {
    return this.value;
  }
}

/** `Hover`: the cursor entered or left a hoverable node. */
@unmanaged
export class HoverEvent extends ClientEventRecord {
  /** True on entering, false on leaving. */
  get entered(): bool {
    return this.value != 0;
  }
}

/** `Hotkey`: a hotkey this module registered was pressed (`id`, compare with `Hotkey.id(name)`). */
@unmanaged
export class HotkeyEvent extends ClientEventRecord {}

/** `Dismiss`: a press outside a dismissable node, or Esc. */
@unmanaged
export class DismissEvent extends ClientEventRecord {}

/** `MapClick`: a click on the map (`cell`). */
@unmanaged
export class MapClickEvent extends ClientEventRecord {
  /** True for the right button, false for the left. */
  get right(): bool {
    return this.value == 1;
  }
}

/** `SelectionChanged`: read the selection with `Client.findPage` and a `Selected = 1` clause. */
@unmanaged
export class SelectionChangedEvent extends ClientEventRecord {}

/** `Notify`: a simulation module's notification (`source`, `id`, `data`); test it with `NotifyName.is`. */
@unmanaged
export class NotifyEvent extends ClientEventRecord {}

/** `SimEvent`: one of this player's simulation events (`simEvent`). */
@unmanaged
export class ForwardedEvent extends ClientEventRecord {}
