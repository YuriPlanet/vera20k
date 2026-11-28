// The client API (RFC 0001 section 10, appendix 2 and 3): one function per
// `vera:client@1.0` import, grouped as `Client`, `Ui`, `Node`, `Camera`,
// `Cursor`, `Local`, `Storage`, plus `ClientVar` and `NotifyName`.
//
// Only `vera:client` imports are called here. core.ts's `log`, `get`,
// `lookup`, `Var`, `Find.page`, ... call `vera:sim` imports, which the
// engine refuses in a client module: use `Client.log`, `Client.get`,
// `Client.lookup`, `ClientVar`, `Client.findPage` instead. The pure parts of
// core.ts (references, `Cell.at`, `TypeRef.make`, `Prop`, `Order`, `Find`'s
// builder) are shared.
//
// Errors as in core.ts: a call that writes returns its `Status`; a call
// that reads or creates returns the value (the empty value on failure) and
// `lastStatus()` gives the reason.

import * as sys from "../sys";
import {
  Status, check, OUT, OUT2, utf8, utf8Len, params, NO_HANDLE, NO_ID, Ref, House, Techno, Cell, Prop,
  NameKind, Order, ORDER_BUF, ORDER_UNITS, Find, FindPage, lastStatus,
} from "../core";
import { NodeKind, ClientEvent, ClientActionKind } from "../generated/client_tables";
import { ClientEventRecord } from "./events";

// ------------------------------------------------------------ References

const KIND_SHIFT: i64 = 48;
const KIND_MASK: i64 = (<i64>0x1f) << KIND_SHIFT;
/** Client node handles (kind 5). */
const KIND_NODE: i64 = (<i64>5) << KIND_SHIFT;
/** Client global references (kind 6): camera 1, local settings 2, cursor 3. */
const KIND_CLIENT: i64 = (<i64>6) << KIND_SHIFT;
/** The local settings reference (kind 6, 2). */
const SETTINGS: Ref = KIND_CLIENT | 2;

/**
 * A node of the match interface (kind 5): one a layout made or the module
 * created. It expires when removed or when the match interface closes.
 * Keep one between callbacks in a top-level `let`.
 */
export type Node = i64;

const NAME_BYTES: i32 = 1024;
const NAME_BUF: usize = memory.data(NAME_BYTES);

/** Reads a string the host writes with `(out, cap, len_out)`, asking again when it is longer than the buffer. */
function readName(kind: i32, id: i32): string {
  if (check(sys.client_name_of(kind, id, NAME_BUF, NAME_BYTES, OUT2)) != Status.Ok) return "";
  const len = load<i32>(OUT2);
  if (len <= NAME_BYTES) return String.UTF8.decodeUnsafe(NAME_BUF, <usize>len);
  const buffer = new ArrayBuffer(len);
  if (check(sys.client_name_of(kind, id, changetype<usize>(buffer), len, OUT2)) != Status.Ok) return "";
  return String.UTF8.decode(buffer);
}

const PAGE = new FindPage();
const ONE = new StaticArray<i64>(1);

// ---------------------------------------------------------------- Client

/** This machine's player and the client's basic calls. */
export namespace Client {
  /** The latest finished simulation frame (0 in `client_init`). */
  export function frame(): i32 {
    return sys.client_frame();
  }

  /** A local, display-only number in `lo..hi` inclusive. `lo > hi` disables the module. */
  export function random(lo: i32, hi: i32): i32 {
    sys.client_random(lo, hi, OUT);
    return load<i32>(OUT);
  }

  /** One line in the in-game console. */
  export function log(line: string): void {
    const at = utf8(line);
    sys.client_log(at, utf8Len);
  }

  /** Logs `why` and stops this client module (on this machine only). */
  export function fail(why: string): void {
    log(why);
    unreachable();
  }

  /** Whether entry `id` of `table` (appendix 8.3 `Table`) is available. */
  export function hasKind(table: i32, id: i32): bool {
    return sys.client_has_kind(table, id) != 0;
  }

  /**
   * The ID of `name` in name kind `kind`, or `NO_ID`. Variable and
   * notification names are the ones this match's simulation modules
   * declared; hotkey names this module's.
   */
  export function lookup(kind: NameKind, name: string): i32 {
    const at = utf8(name);
    if (check(sys.client_lookup(kind, at, utf8Len, OUT)) != Status.Ok) return NO_ID;
    return load<i32>(OUT);
  }

  /** The name of `id` in name kind `kind`, or `""`. */
  export function nameOf(kind: NameKind, id: i32): string {
    return readName(<i32>kind, id);
  }

  /** The text of string ID `id` (`""` for 0). */
  export function text(id: i32): string {
    if (id == 0) return "";
    return readName(<i32>NameKind.String, id);
  }

  /**
   * `prop` of `reference` as this player's interface may show it: an
   * object out of sight is `Gone`, a hidden property `Refused`, both 0.
   */
  export function get(reference: Ref, prop: Prop): i64 {
    check(sys.client_get(reference, prop, OUT));
    return load<i64>(OUT);
  }

  /** A string property: `get` plus `name_of(String, ..)`. */
  export function getText(reference: Ref, prop: Prop): string {
    const id = get(reference, prop);
    if (lastStatus() != Status.Ok) return "";
    return text(<i32>id);
  }

  /**
   * Reads `props` of every reference in `refs` into `out`, row-major
   * (`out[i * props.length + j]`), each under `get`'s visibility, and
   * returns the number of values written.
   */
  export function getMany(refs: StaticArray<i64>, props: StaticArray<i32>, out: StaticArray<i64>): i32 {
    if (out.length < refs.length * props.length) {
      check(Status.Refused);
      return 0;
    }
    check(sys.client_get_many(
      changetype<usize>(refs), refs.length,
      changetype<usize>(props), props.length,
      changetype<usize>(out), OUT2,
    ));
    return load<i32>(OUT2);
  }

  /** Writes a client-owned reference's property: a node, the camera, the cursor. */
  export function set(reference: Ref, prop: Prop, value: i64): Status {
    return check(sys.client_set(reference, prop, value));
  }

  /** Writes a string property (`Label`, `Art`, `Items`, ...). */
  export function setText(reference: Ref, prop: Prop, value: string): Status {
    const at = utf8(value);
    return check(sys.client_set_bytes(reference, prop, at, utf8Len));
  }

  /** Writes a bytes property (`ItemProgress`, ...). */
  export function setBytes(reference: Ref, prop: Prop, data: Uint8Array): Status {
    return check(sys.client_set_bytes(reference, prop, data.dataStart, data.length));
  }

  /** `client_var_get(scope, var)`: scope `NO_HANDLE` for a global variable. */
  export function varGet(scope: Ref, id: i32): i64 {
    check(sys.client_var_get(scope, id, OUT));
    return load<i64>(OUT);
  }

  /**
   * One page of `query` restricted to what this player sees, from sort key
   * `after` (0 first) into `out`. The returned page is shared: read it
   * before the next search.
   */
  export function findPage(query: Find, out: StaticArray<i64>, after: i64): FindPage {
    PAGE.count = 0;
    PAGE.next = 0;
    const ptr = query.raw(after);
    if (!ptr) {
      check(Status.Refused);
      return PAGE;
    }
    const len = query.rawLen;
    if (check(sys.client_find(ptr, len, changetype<usize>(out), out.length, OUT)) == Status.Ok) {
      PAGE.count = load<i32>(OUT);
      PAGE.next = load<i64>(OUT, 8);
    }
    return PAGE;
  }

  /** The first match of `query` this player sees, or `NO_HANDLE`. */
  export function findFirst(query: Find): i64 {
    let after: i64 = 0;
    while (true) {
      const page = findPage(query, ONE, after);
      if (lastStatus() != Status.Ok) return NO_HANDLE;
      if (page.count == 1) return unchecked(ONE[0]);
      if (page.next == 0) return NO_HANDLE;
      after = page.next;
    }
    return NO_HANDLE;
  }

  /** This player's house, or `NO_HANDLE` for a spectator. */
  export function viewer(): House {
    const house = get(SETTINGS, Prop.Viewer);
    return lastStatus() == Status.Ok ? house : NO_HANDLE;
  }

  export function isSpectator(): bool {
    const value = get(SETTINGS, Prop.IsSpectator);
    return lastStatus() == Status.Ok && value != 0;
  }

  /**
   * Sends `data` (at most 96 bytes) to this source's simulation module's
   * `on_message` through the command stream (RFC 10). `Refused` for
   * spectators and over the size and rate limits.
   */
  export function sendMessage(data: Uint8Array): Status {
    return check(sys.client_send_message(data.dataStart, data.length));
  }

  /**
   * A standard player order for this player, built with `Order`'s static
   * functions and checked like player input. `Refused` unless the room
   * allows automation (`AutomationAllowed`) and within the rate limits.
   */
  export function order(order: Order): Status {
    const len = order.write();
    if (len < 0) return check(Status.Refused);
    return check(sys.client_send_order(ORDER_BUF, len));
  }
}

// -------------------------------------------------------------------- Ui

/** Initial scalar properties for `Ui.createWith`: at most 16, set in the one `client_node_create` call. */
export class NodeProps {
  /** Most entries one `NodeProps` holds; more make the create `Refused`. */
  static readonly MAX: i32 = 16;
  private bytes: StaticArray<u8> = new StaticArray<u8>(NodeProps.MAX * sys.PROP_VALUE_BYTES);
  private count: i32 = 0;
  private overflow: bool = false;

  /** Adds `prop = value`. */
  with(prop: Prop, value: i64): NodeProps {
    if (this.count >= NodeProps.MAX) {
      this.overflow = true;
      return this;
    }
    const at = changetype<usize>(this.bytes) + <usize>(this.count * sys.PROP_VALUE_BYTES);
    store<i32>(at, prop, 0);
    store<i32>(at, 0, 4);
    store<i64>(at, value, 8);
    this.count++;
    return this;
  }

  /** Position in pixels. */
  at(x: i32, y: i32): NodeProps {
    return this.with(Prop.X, x).with(Prop.Y, y);
  }

  size(width: i32, height: i32): NodeProps {
    return this.with(Prop.Width, width).with(Prop.Height, height);
  }

  /** @internal */
  get ptr(): usize {
    return changetype<usize>(this.bytes);
  }
  /** @internal */
  get len(): i32 {
    return this.count * sys.PROP_VALUE_BYTES;
  }
  /** @internal */
  get refused(): bool {
    return this.overflow;
  }
}

/** The interface tree: finding, creating and removing nodes (RFC 10). */
export namespace Ui {
  /**
   * The node named `name`: a layout part's name (without the layout
   * prefix) or a created node's `Name`. This module's source first, then
   * engine, mod and map; `NO_HANDLE` (`NotFound`) without one.
   */
  export function find(name: string): Node {
    const at = utf8(name);
    if (check(sys.client_node_find(at, utf8Len, OUT)) != Status.Ok) return NO_HANDLE;
    return load<i64>(OUT);
  }

  /** A new node of `kind` under `parent` (`NO_HANDLE`: top level, in the overlay for a map module). */
  export function create(kind: NodeKind, parent: Node): Node {
    return created(sys.client_node_create(kind, parent, 0, 0, OUT));
  }

  /** A new node with initial scalar properties, set in one call. */
  export function createWith(kind: NodeKind, parent: Node, props: NodeProps): Node {
    if (props.refused) {
      check(Status.Refused);
      return NO_HANDLE;
    }
    return created(sys.client_node_create(kind, parent, props.ptr, props.len, OUT));
  }

  /** A new node named `name` (see `Node.setName`); removed again when the name is refused. */
  export function createNamed(kind: NodeKind, parent: Node, name: string): Node {
    const node = create(kind, parent);
    if (node == NO_HANDLE) return NO_HANDLE;
    const status = Node.setName(node, name);
    if (status != Status.Ok) {
      sys.client_node_remove(node);
      // The refused name is what to report, not the removal.
      check(status);
      return NO_HANDLE;
    }
    return node;
  }

  /** Removes `node` and its children. */
  export function remove(node: Node): Status {
    return check(sys.client_node_remove(node));
  }

  function created(status: i32): Node {
    if (check(status) != Status.Ok) return NO_HANDLE;
    return load<i64>(OUT);
  }
}

/** A node's properties (appendix 7.10); each is one `client_set`, `client_set_bytes` or `client_get`. */
export namespace Node {
  /** Whether `n` is a node handle (kind 5). No host call. */
  export function isNode(n: Node): bool {
    return (n & KIND_MASK) == KIND_NODE;
  }
  export function get(n: Node, prop: Prop): i64 {
    return Client.get(n, prop);
  }
  export function set(n: Node, prop: Prop, value: i64): Status {
    return Client.set(n, prop, value);
  }
  /**
   * Names a node this module created, for `Ui.find`: once per node, unique
   * among this source's nodes. `Refused` for an empty name, a second name,
   * a name taken, or a node of the engine, a layout or the other module.
   */
  export function setName(n: Node, name: string): Status {
    return Client.setText(n, Prop.Name, name);
  }
  /** A CSF entry name or literal text (`Text`, `Button`). */
  export function setLabel(n: Node, text: string): Status {
    return Client.setText(n, Prop.Label, text);
  }
  /** An image by name (`Button`, `Image`). */
  export function setArt(n: Node, name: string): Status {
    return Client.setText(n, Prop.Art, name);
  }
  export function visible(n: Node): bool {
    return Client.get(n, Prop.Visible) != 0;
  }
  export function setVisible(n: Node, visible: bool): Status {
    return Client.set(n, Prop.Visible, visible ? 1 : 0);
  }
  /** `Button`, `List`: drawn disabled and without `Click` at false. */
  export function setEnabled(n: Node, enabled: bool): Status {
    return Client.set(n, Prop.Enabled, enabled ? 1 : 0);
  }
  /** Emits `Click` events. */
  export function setClickable(n: Node, clickable: bool): Status {
    return Client.set(n, Prop.Clickable, clickable ? 1 : 0);
  }
  /** Emits `Hover` events. */
  export function setHoverable(n: Node, hoverable: bool): Status {
    return Client.set(n, Prop.Hoverable, hoverable ? 1 : 0);
  }
  /** Emits `Dismiss` on a press outside it or Esc, while visible. */
  export function setDismissable(n: Node, dismissable: bool): Status {
    return Client.set(n, Prop.Dismissable, dismissable ? 1 : 0);
  }
  /** Position in pixels (sidebar or screen, by where the node is). */
  export function setPosition(n: Node, x: i32, y: i32): Status {
    const status = Client.set(n, Prop.X, x);
    if (status != Status.Ok) return status;
    return Client.set(n, Prop.Y, y);
  }
  export function setSize(n: Node, width: i32, height: i32): Status {
    const status = Client.set(n, Prop.Width, width);
    if (status != Status.Ok) return status;
    return Client.set(n, Prop.Height, height);
  }
  /** A `Bar`'s fill, 0..100. */
  export function setFill(n: Node, percent: i32): Status {
    return Client.set(n, Prop.Fill, percent);
  }
  /** Whether a `Text` over the battlefield stands on black (the default); off for a text on the module's own picture. */
  export function setBackdrop(n: Node, backdrop: bool): Status {
    return Client.set(n, Prop.Backdrop, backdrop ? 1 : 0);
  }
  /** A `List`'s rows: the entries joined with `\n` in one call. `Refused` past 1 KiB in all. */
  export function setItems(n: Node, items: string[]): Status {
    let len: i32 = 0;
    for (let i = 0; i < items.length; i++) {
      const bytes = String.UTF8.byteLength(unchecked(items[i]));
      len += bytes + (i > 0 ? 1 : 0);
      if (len > ITEMS_BYTES) return check(Status.Refused);
    }
    let at = ITEMS_BUF;
    for (let i = 0; i < items.length; i++) {
      if (i > 0) store<u8>(at++, 0x0a);
      const item = unchecked(items[i]);
      at += String.UTF8.encodeUnsafe(changetype<usize>(item), item.length, at);
    }
    return check(sys.client_set_bytes(n, Prop.Items, ITEMS_BUF, len));
  }
  /** Removes the node and its children. */
  export function remove(n: Node): Status {
    return Ui.remove(n);
  }
}

const ITEMS_BYTES: i32 = 1024;
const ITEMS_BUF: usize = memory.data(ITEMS_BYTES);

// --------------------------------------------------------- Client globals

/** The local camera (kind 6, 1). */
export namespace Camera {
  export const REF: Ref = KIND_CLIENT | 1;
  /** The cell at the camera's centre. */
  export function cell(): Cell {
    return Client.get(REF, Prop.Cell);
  }
  /** Jumps the local camera to `cell` (local action `JumpCamera`). */
  export function jumpTo(cell: Cell): Status {
    return Local.run(ClientActionKind.JumpCamera, cell, params());
  }
  /** Moves the local camera to `cell` at `speed`, as stock action 48. */
  export function centerOn(cell: Cell, speed: i64): Status {
    const p = params();
    store<i64>(p, speed, sys.AP_VALUE);
    return Local.run(ClientActionKind.CenterCamera, cell, p);
  }
}

/** The mouse cursor (kind 6, 3). */
export namespace Cursor {
  export const REF: Ref = KIND_CLIENT | 3;
  /** Screen pixels, as node `X` counts. */
  export function x(): i64 {
    return Client.get(REF, Prop.X);
  }
  /** Screen pixels, as node `Y` counts. */
  export function y(): i64 {
    return Client.get(REF, Prop.Y);
  }
  /** The cell under the cursor, `NO_HANDLE` off the map. */
  export function cell(): Cell {
    return Client.get(REF, Prop.Cell);
  }
  /** At true, map clicks reach only this module (`MapClick`), not the game. */
  export function captureMapClicks(capture: bool): Status {
    return Client.set(REF, Prop.CaptureMapClick, capture ? 1 : 0);
  }
}

// ---------------------------------------------------------- Local actions

/** Local actions (appendix 5.4): effects on this machine only. */
export namespace Local {
  /** Any local action `kind` on `target` with the `ActionParams` at `p` (`params()` clears the shared one). */
  export function run(kind: i32, target: i64, p: usize): Status {
    return check(sys.client_action(kind, target, p, sys.ACTION_PARAMS_BYTES));
  }

  function withText(kind: i32, text: string, extra: usize, extraLen: i32, value: i64): Status {
    const p = params();
    const at = utf8(text, 0);
    store<i32>(p, <i32>at, sys.AP_TEXT);
    store<i32>(p, utf8Len, sys.AP_TEXT_LEN);
    store<i32>(p, <i32>extra, sys.AP_EXTRA);
    store<i32>(p, extraLen, sys.AP_EXTRA_LEN);
    store<i64>(p, value, sys.AP_VALUE);
    return run(kind, NO_HANDLE, p);
  }

  /** Plays sound `id` (`Client.lookup(NameKind.Sound, ..)`) at `cell`, or not positioned with `NO_HANDLE`. */
  export function playSound(id: i32, cell: Cell): Status {
    const p = params();
    store<i32>(p, id, sys.AP_ID);
    return run(ClientActionKind.PlaySound, cell, p);
  }

  /** Plays EVA speech `name`. */
  export function playSpeech(name: string): Status {
    return withText(ClientActionKind.PlaySpeech, name, 0, 0, 0);
  }

  /** The stock home-camera action (145); a later tier, `Unsupported` until the engine has it. */
  export function homeCamera(): Status {
    return run(ClientActionKind.HomeCamera, NO_HANDLE, params());
  }

  /**
   * Selects `units` locally (only what this player may select); `append`
   * keeps the current selection. `Refused` past `ORDER_UNITS` units.
   */
  export function select(units: StaticArray<Techno>, append: bool): Status {
    if (units.length > ORDER_UNITS) return check(Status.Refused);
    // `extra` is the array of i64 handles itself.
    return withText(ClientActionKind.Select, "", changetype<usize>(units), units.length * 8, append ? 1 : 0);
  }

  /** Flashes `object` for `frames` frames. */
  export function flash(object: Techno, frames: i64): Status {
    const p = params();
    store<i64>(p, frames, sys.AP_VALUE);
    return run(ClientActionKind.FlashObject, object, p);
  }

  /** Shows one line: a CSF entry name or literal text, in `color`. */
  export function showMessage(text: string, color: i64): Status {
    return withText(ClientActionKind.ShowMessage, text, 0, 0, color);
  }

  /**
   * Registers hotkey `name` with its default chord (`"Ctrl+Alt+K"`); read
   * its ID with `Hotkey.id`. `Refused` when a game hotkey already has the
   * chord: the hotkey exists, without a key.
   */
  export function registerHotkey(name: string, chord: string): Status {
    const extra = utf8(chord, 1);
    return withText(ClientActionKind.RegisterHotkey, name, extra, utf8Len, 0);
  }
}

/** Hotkeys this module registered. */
export namespace Hotkey {
  /** The ID a `Hotkey` client event carries for `name`, or `NO_ID`. */
  export function id(name: string): i32 {
    return Client.lookup(NameKind.Hotkey, name);
  }
}

// ------------------------------------------------- Variables, notifications

/**
 * A simulation module's declared variable, read under its visibility (RFC
 * 6e): resolved by name on first use with `client_lookup`. Clients only
 * read; `client_var_get` is the one variable call.
 */
export class ClientVar {
  private resolved: i32 = NO_ID;

  /** `name` as the simulation module's `vera.json` declares it (`mod.` or `map.` addresses the other source's). */
  constructor(public readonly name: string) {}

  /** The variable ID, or `NO_ID` for a name no simulation module of this match declared. */
  get id(): i32 {
    if (this.resolved < 0) this.resolved = Client.lookup(NameKind.Variable, this.name);
    return this.resolved;
  }

  /**
   * The value in `scope`: `NO_HANDLE` for a global variable, a house (a
   * private one only this player's own) or an object. 0 on failure, with
   * `lastStatus()` the reason.
   */
  get(scope: Ref = NO_HANDLE): i64 {
    const id = this.id;
    if (id < 0) return 0;
    return Client.varGet(scope, id);
  }
}

/**
 * A notification name some simulation module of this match declared,
 * resolved once with `client_lookup(Notify, ..)`: compare with a `Notify`
 * event's `id`, or test the event with `is`.
 */
export class NotifyName {
  private resolved: i32 = NO_ID;

  constructor(public readonly name: string) {}

  /** The notification ID, or `NO_ID` when no simulation module declared the name. */
  get id(): i32 {
    if (this.resolved == NO_ID) this.resolved = Client.lookup(NameKind.Notify, this.name);
    return this.resolved;
  }

  /** Whether `event` is this notification. */
  is(event: ClientEventRecord): bool {
    if (event.kind != ClientEvent.Notify) return false;
    const id = this.id;
    return id != NO_ID && id == event.id;
  }
}

// ---------------------------------------------------------------- Storage

const STORAGE_BYTES: i32 = 1024;
const STORAGE_BUF: usize = memory.data(STORAGE_BYTES, 8);

/**
 * This module source's local key-value store (RFC 10, "Storage"): ASCII
 * keys of 1..64 bytes, values up to 16 KiB, 256 keys and 256 KiB in all.
 */
export namespace Storage {
  /** The value of `key`, empty with `NotFound` without one. Calls twice for a value past 1 KiB. */
  export function read(key: string): Uint8Array {
    const k = utf8(key, 0);
    const kLen = utf8Len;
    if (check(sys.client_storage_read(k, kLen, STORAGE_BUF, STORAGE_BYTES, OUT2)) != Status.Ok) return new Uint8Array(0);
    const len = load<i32>(OUT2);
    const out = new Uint8Array(max(len, 0));
    if (len <= STORAGE_BYTES) {
      memory.copy(out.dataStart, STORAGE_BUF, <usize>len);
      return out;
    }
    // Too long for the buffer: the engine wrote nothing; ask again.
    if (check(sys.client_storage_read(k, kLen, out.dataStart, len, OUT2)) != Status.Ok) return new Uint8Array(0);
    return out;
  }

  /** The value of `key` as UTF-8 text, `""` without one. */
  export function readString(key: string): string {
    const data = read(key);
    return String.UTF8.decodeUnsafe(data.dataStart, <usize>data.length);
  }

  /** The length of `key`'s value, −1 (`NotFound`) without one. */
  export function len(key: string): i32 {
    const k = utf8(key, 0);
    if (check(sys.client_storage_read(k, utf8Len, 0, 0, OUT2)) != Status.Ok) return -1;
    return max(load<i32>(OUT2), 0);
  }

  /** Whether `key` has a value. */
  export function has(key: string): bool {
    return len(key) >= 0;
  }

  /** Writes `key`. `Refused` for empty data or over a limit, storage unchanged. */
  export function write(key: string, data: Uint8Array): Status {
    if (data.length == 0) return check(Status.Refused);
    const k = utf8(key, 0);
    return check(sys.client_storage_write(k, utf8Len, data.dataStart, data.length));
  }

  /** Writes `key` as UTF-8 text. `Refused` for `""` or over a limit. */
  export function writeString(key: string, value: string): Status {
    if (value.length == 0) return check(Status.Refused);
    const k = utf8(key, 0);
    const kLen = utf8Len;
    const v = utf8(value, 1);
    return check(sys.client_storage_write(k, kLen, v, utf8Len));
  }

  /** Deletes `key`. */
  export function remove(key: string): Status {
    const k = utf8(key, 0);
    return check(sys.client_storage_write(k, utf8Len, 0, 0));
  }
}

/** Status templates over the battlefield (`client_status`): the keys of a
 *  `veraui.ini` `[VeraStatus.<name>]` section, `Name=` first. */
export namespace StatusTemplates {
  /** Registers `template`, replacing this module's one of the same name.
   *  `Refused` when a key does not parse or is over a limit. */
  export function set(template: string): Status {
    const at = utf8(template, 0);
    return check(sys.client_status(at, utf8Len));
  }

  /** Removes this module's template `name`. */
  export function remove(name: string): Status {
    return set("Name=" + name);
  }
}
