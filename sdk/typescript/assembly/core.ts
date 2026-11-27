// The simulation API's core (RFC 0001 appendix section 3): statuses,
// references, names, properties, modifiers, variables, searches, actions,
// orders and rules. Each wrapper makes the host calls appendix section 3
// lists, usually one.
//
// Errors: a call that writes returns its `Status`. A call that reads or
// creates returns the value itself, which on failure is the empty value the
// engine wrote (0, `NO_HANDLE` or −1); `lastStatus()` then gives the reason.

import * as sys from "./sys";
import { Prop } from "./generated/props";
import { ActionKind } from "./generated/actions";

export { Prop } from "./generated/props";
export { ActionKind } from "./generated/actions";

// ---------------------------------------------------------------- Status

/** A host call's result (appendix "Status"). */
export enum Status {
  Ok = 0,
  /** The reference is 0 or the object no longer exists. */
  Gone = 1,
  /** No matching name or ID, or a missing waypoint. */
  NotFound = 2,
  /** The owner refused: occupied cell, unwritable now, or quota full. */
  Refused = 3,
  /** Disabled in this match, an unknown entry, or unavailable here. */
  Unsupported = 4,
}

let last: Status = Status.Ok;

/** The status of the latest SDK call that returned a value. */
export function lastStatus(): Status {
  return last;
}

/** @internal Records `status` as the latest and returns it. */
export function check(status: i32): Status {
  last = <Status>(status < 0 || status > 4 ? Status.Unsupported : status);
  return last;
}

// -------------------------------------------------------------- Scratch

/** @internal Out pointers: 16 bytes, 8-byte aligned. */
export const OUT: usize = memory.data(16, 8);
/** @internal A second out slot (`len_out`, `count_out`). */
export const OUT2: usize = memory.data(8, 8);

const SLOT_BYTES: i32 = 512;
const SLOTS: usize = memory.data(SLOT_BYTES * 3, 8);
// A string too long for its slot is encoded into a buffer kept here until
// the slot is used again.
const kept: StaticArray<ArrayBuffer | null> = new StaticArray<ArrayBuffer | null>(3);

/** @internal Length in bytes of the latest `utf8` result. */
export let utf8Len: i32 = 0;

/**
 * @internal `s` as UTF-8 in module memory, in scratch slot `slot` (0..2),
 * valid until that slot is used again. Sets `utf8Len`.
 */
export function utf8(s: string, slot: i32 = 0): usize {
  if (s.length * 3 <= SLOT_BYTES) {
    const at = SLOTS + <usize>(slot * SLOT_BYTES);
    utf8Len = <i32>String.UTF8.encodeUnsafe(changetype<usize>(s), s.length, at);
    return at;
  }
  const buffer = String.UTF8.encode(s);
  unchecked((kept[slot] = buffer));
  utf8Len = buffer.byteLength;
  return changetype<usize>(buffer);
}

const NAME_BYTES: i32 = 1024;
const NAME_BUF: usize = memory.data(NAME_BYTES);

// ------------------------------------------------------------ References
// Every reference is an `i64` below 2^53 (appendix "References"): plain
// values, read and written through namespace functions taking the
// reference first, as `Techno.health(t)`.

/** No object. */
export const NO_HANDLE: i64 = 0;
/** No ID (`lookup` found nothing). */
export const NO_ID: i32 = -1;

const KIND_SHIFT: i64 = 48;
const KIND_TYPE: i64 = (<i64>1) << KIND_SHIFT;
const KIND_CELL: i64 = (<i64>2) << KIND_SHIFT;
const KIND_WAYPOINT: i64 = (<i64>3) << KIND_SHIFT;
const KIND_GLOBAL: i64 = (<i64>4) << KIND_SHIFT;

/** Any reference: what `get`, `set` and `find` take. */
export type Ref = i64;
/** A unit or building: its creation serial. */
export type Techno = i64;
/** A projectile: its creation serial (properties of appendix 7.4). */
export type Projectile = i64;
/** A house. */
export type House = i64;
/** A cell: kind 2, `x << 16 | y`. */
export type Cell = i64;
/** A waypoint: kind 3, its number. */
export type Waypoint = i64;
/** A type: kind 1, `name kind << 32 | ID`. */
export type TypeRef = i64;
/** A modifier handle. */
export type Modifier = i64;

/** Name kinds for `lookup` (appendix 8.1). */
export enum NameKind {
  TechnoType = 0,
  Weapon = 1,
  Warhead = 2,
  Projectile = 3,
  Anim = 4,
  VoxelAnim = 5,
  ParticleSystem = 6,
  SuperWeapon = 7,
  Armor = 8,
  Overlay = 9,
  Sound = 10,
  Country = 11,
  TeamType = 12,
  ScriptType = 13,
  AiTrigger = 14,
  Variable = 15,
  Notify = 16,
  ModuleAction = 17,
  /** Engine-held text by string ID (`text`). */
  String = 18,
  Property = 19,
  EventKind = 20,
  ActionKind = 21,
  /** A hotkey this client module registered. */
  Hotkey = 22,
  /** Module events any source registered; the ID is the event ID. */
  ModuleEvent = 24,
}

/** Filter clause comparison (appendix 8.3, enum 0). */
export enum Op {
  Eq = 0,
  Ne = 1,
  Lt = 2,
  Le = 3,
  Gt = 4,
  Ge = 5,
}

/** `Category` property values (appendix 8.3, enum 10). */
export enum Category {
  Infantry = 0,
  Vehicle = 1,
  Aircraft = 2,
  Building = 3,
  Projectile = 4,
}

export namespace TypeRef {
  /** The type reference of ID `id` in name kind `kind`. */
  export function make(kind: NameKind, id: i32): TypeRef {
    return KIND_TYPE | ((<i64>kind) << 32) | <i64>(<u32>id);
  }
  /** The type ID within its name kind. */
  export function id(t: TypeRef): i32 {
    return <i32>t;
  }
  /** The name kind of `t`. */
  export function kind(t: TypeRef): NameKind {
    return <NameKind>(<i32>((t >> 32) & 0xffff));
  }
  /** The wildcard of name kind `kind`, for `Find.types` only. */
  export function any(kind: NameKind): TypeRef {
    return make(kind, -1);
  }
}

export namespace Cell {
  /** The cell at `x`, `y`. No host call. */
  export function at(x: i32, y: i32): Cell {
    return KIND_CELL | ((<i64>(x & 0xffff)) << 16) | <i64>(y & 0xffff);
  }
  export function x(c: Cell): i32 {
    return <i32>((c >> 16) & 0xffff);
  }
  export function y(c: Cell): i32 {
    return <i32>(c & 0xffff);
  }
  export function landType(c: Cell): i64 {
    return get(c, Prop.LandType);
  }
  export function height(c: Cell): i64 {
    return get(c, Prop.Height);
  }
  export function overlay(c: Cell): TypeRef {
    return get(c, Prop.Overlay);
  }
  export function ore(c: Cell): i64 {
    return get(c, Prop.Ore);
  }
  /** The waypoint number on `c`, −1 for none. */
  export function waypoint(c: Cell): i64 {
    return get(c, Prop.Waypoint);
  }
  /** Whether `speed` can pass `c`. */
  export function passable(c: Cell, speed: SpeedType): bool {
    return get(c, SpeedType.passable(speed)) != 0;
  }
}

export namespace Waypoint {
  /** Waypoint `n`. No host call; functions taking cells resolve it. */
  export function number(n: i32): Waypoint {
    return KIND_WAYPOINT | <i64>(<u32>n);
  }
  /** The same as `number`, the name appendix section 3 uses. */
  export function cell(n: i32): Waypoint {
    return number(n);
  }
}

// ----------------------------------------------------------- Basic calls

/** The current frame (15 per game second). */
export function frame(): i32 {
  return sys.frame();
}

/** A number in `lo..hi` inclusive from this module's own random stream. */
export function random(lo: i32, hi: i32): i32 {
  sys.random(lo, hi, OUT);
  return load<i32>(OUT);
}

/** One line in the in-game log. */
export function log(line: string): void {
  const at = utf8(line);
  sys.log(at, utf8Len);
}

/**
 * Logs `why` and stops the module for good: the engine disables it on
 * every machine and tells the players (RFC section 3).
 */
export function fail(why: string): void {
  log(why);
  unreachable();
}

/** 1 if `id` in table `table` is available in this match, else 0. */
export function hasKind(table: i32, id: i32): bool {
  return sys.has_kind(table, id) != 0;
}

/** The ID of `name` in name kind `kind`, or `NO_ID`. */
export function lookup(kind: NameKind, name: string): i32 {
  const at = utf8(name);
  if (check(sys.lookup(kind, at, utf8Len, OUT)) != Status.Ok) return NO_ID;
  return load<i32>(OUT);
}

/** The name of `id` in name kind `kind`, or `""`. */
export function nameOf(kind: NameKind, id: i32): string {
  return readName(<i32>kind, id);
}

/**
 * The text of string ID `id`: a module action's parameter text, an INI
 * entry name or value, a `string` property. ID 0 is no text, `""`.
 */
export function text(id: i32): string {
  if (id == 0) return "";
  return readName(<i32>NameKind.String, id);
}

function readName(kind: i32, id: i32): string {
  if (check(sys.name_of(kind, id, NAME_BUF, NAME_BYTES, OUT2)) != Status.Ok) return "";
  const len = load<i32>(OUT2);
  if (len <= NAME_BYTES) return String.UTF8.decodeUnsafe(NAME_BUF, <usize>len);
  // Too long for the buffer: the engine wrote nothing; ask again.
  const buffer = new ArrayBuffer(len);
  if (check(sys.name_of(kind, id, changetype<usize>(buffer), len, OUT2)) != Status.Ok) return "";
  return String.UTF8.decode(buffer);
}

// ------------------------------------------------------------ Properties

/** The effective value of `prop` on `reference`, including modifiers. */
export function get(reference: Ref, prop: Prop): i64 {
  check(sys.get(reference, prop, OUT));
  return load<i64>(OUT);
}

/** Writes `prop` through its owner. */
export function set(reference: Ref, prop: Prop, value: i64): Status {
  return check(sys.set(reference, prop, value));
}

/** A string property (`House.name`): `get` plus `name_of(String, ..)`. */
export function getText(reference: Ref, prop: Prop): string {
  const id = get(reference, prop);
  if (last != Status.Ok) return "";
  return text(<i32>id);
}

/**
 * Reads `props` of every reference in `refs` into `out`, row-major
 * (`out[i * props.length + j]`), and returns the number of values written.
 * `out` must hold `refs.length * props.length` values.
 */
export function getMany(refs: StaticArray<i64>, props: StaticArray<i32>, out: StaticArray<i64>): i32 {
  if (out.length < refs.length * props.length) {
    last = Status.Refused;
    return 0;
  }
  check(sys.get_many(
    changetype<usize>(refs), refs.length,
    changetype<usize>(props), props.length,
    changetype<usize>(out), OUT2,
  ));
  return load<i32>(OUT2);
}

/** Whether property `name` exists: its ID, or `NO_ID`. */
export function propNamed(name: string): Prop {
  return lookup(NameKind.Property, name);
}

// ------------------------------------------------------------- Modifiers

/** `1.0` as a `fixed` value (raw `I16F16`, appendix "Values"). */
export const FIXED_ONE: i64 = 65536;

/** `value` as a `fixed` value: `fixed(0.5)` is half. */
export function fixed(value: f64): i64 {
  return <i64>(value * 65536.0);
}

export enum ModifierOp {
  Add = 0,
  Multiply = 1,
  Override = 2,
}

export namespace Modifier {
  /**
   * Adds a modifier and returns its handle (`NO_HANDLE` on failure).
   * Negative `frames` lasts until removed; `group` 0 is ungrouped.
   */
  export function create(target: Ref, prop: Prop, op: ModifierOp, value: i64, frames: i32, source: Ref = 0, group: i32 = 0): Modifier {
    check(sys.modify(target, prop, op, value, frames, source, group, OUT));
    return load<i64>(OUT);
  }
  export function add(target: Ref, prop: Prop, value: i64, frames: i32): Modifier {
    return create(target, prop, ModifierOp.Add, value, frames);
  }
  /** Multiplies by `factor`, a `fixed` value. */
  export function multiply(target: Ref, prop: Prop, factor: i64, frames: i32): Modifier {
    return create(target, prop, ModifierOp.Multiply, factor, frames);
  }
  export function overrideWith(target: Ref, prop: Prop, value: i64, frames: i32): Modifier {
    return create(target, prop, ModifierOp.Override, value, frames);
  }
  /** Removes `m` early; `Gone` if it expired. */
  export function remove(m: Modifier): Status {
    return check(sys.unmodify(m));
  }
}

// ------------------------------------------------------------- Variables

/**
 * A variable (appendix "Variable IDs"): a stock one by number, or one this
 * module declares in `vera.json` (`GlobalVar`, `HouseVar`, `ObjectVar`),
 * resolved by name on first use. Filter clauses and `ActionDef.setVar`
 * take any of them.
 */
export class Var {
  private resolved: i32;

  constructor(id: i32, public readonly name: string = "") {
    this.resolved = id;
  }

  /** Red Alert 2 local variable `n` (0..99). */
  static local(n: i32): GlobalVar {
    return new GlobalVar("", n);
  }
  /** Red Alert 2 global variable `n` (0..49). */
  static global(n: i32): GlobalVar {
    return new GlobalVar("", 100 + n);
  }

  /** The variable ID, or `NO_ID` for an unknown name. */
  get id(): i32 {
    if (this.resolved < 0 && this.name.length) this.resolved = lookup(NameKind.Variable, this.name);
    return this.resolved;
  }

  /** The value in `scope`: 0 for global, else a house or object. */
  read(scope: Ref): i64 {
    check(sys.var_get(scope, this.id, OUT));
    return load<i64>(OUT);
  }

  /** Writes the value in `scope`. */
  write(scope: Ref, value: i64): Status {
    return check(sys.var_set(scope, this.id, value));
  }
}

/** A global variable: a stock one (`Var.local`, `Var.global`) or a declared one. */
export class GlobalVar extends Var {
  constructor(name: string, id: i32 = -1) {
    super(id, name);
  }
  get(): i64 {
    return this.read(0);
  }
  set(value: i64): Status {
    return this.write(0, value);
  }
}

/** A declared per-house variable. */
export class HouseVar extends Var {
  constructor(name: string) {
    super(-1, name);
  }
  get(house: House): i64 {
    return this.read(house);
  }
  set(house: House, value: i64): Status {
    return this.write(house, value);
  }
}

/** A declared per-object variable. */
export class ObjectVar extends Var {
  constructor(name: string) {
    super(-1, name);
  }
  get(object: Ref): i64 {
    return this.read(object);
  }
  set(object: Ref, value: i64): Status {
    return this.write(object, value);
  }
}

// ---------------------------------------------------------------- Search

/** Most filter clauses one `Find` holds; more make the search `Refused`. */
export const MAX_CLAUSES: i32 = 16;

const Q_SCOPE: i32 = 0;
const Q_AREA: i32 = 4;
const Q_CELL: i32 = 8;
const Q_RADIUS: i32 = 16;
const Q_HEIGHT: i32 = 20;
const Q_TYPE: i32 = 24;
const Q_OWNER: i32 = 32;
const Q_VIEWPOINT: i32 = 40;
const Q_AFTER: i32 = 48;
const Q_CLAUSES: i32 = 56;

/** One page of a search: `count` entries written, `next` for the next page (0 on the last). */
export class FindPage {
  count: i32 = 0;
  next: i64 = 0;
}

const PAGE = new FindPage();
const ONE = new StaticArray<i64>(1);

/**
 * A search (`Query`, appendix 8.2). Build it, then call `.first()` or
 * `.page(buffer, after)`; each page is one `find` call.
 */
export class Find {
  private buf: StaticArray<u8> = new StaticArray<u8>(sys.QUERY_BYTES + MAX_CLAUSES * sys.CLAUSE_BYTES);
  private overflow: bool = false;

  private constructor(scope: i32) {
    store<i32>(changetype<usize>(this.buf), scope, Q_SCOPE);
  }

  private get ptr(): usize {
    return changetype<usize>(this.buf);
  }

  /** Units and buildings, by creation serial. */
  static objects(): Find {
    return new Find(0);
  }
  /** Houses, by house index. */
  static houses(): Find {
    return new Find(1);
  }
  /** Cells, by cell index. */
  static cells(): Find {
    return new Find(2);
  }
  /** Types matching `t`: a type reference or `TypeRef.any(kind)`. */
  static types(t: TypeRef): Find {
    return new Find(3).ofType(t);
  }

  /** Only within `radius` leptons (256 to a cell) of `center`. */
  near(center: Cell, radius: i32): Find {
    store<i32>(this.ptr, 1, Q_AREA);
    store<i64>(this.ptr, center, Q_CELL);
    store<i32>(this.ptr, radius, Q_RADIUS);
    return this;
  }
  /** Only within the `width` × `height` rectangle from `topLeft`. */
  within(topLeft: Cell, width: i32, height: i32): Find {
    store<i32>(this.ptr, 2, Q_AREA);
    store<i64>(this.ptr, topLeft, Q_CELL);
    store<i32>(this.ptr, width, Q_RADIUS);
    store<i32>(this.ptr, height, Q_HEIGHT);
    return this;
  }
  ofType(t: TypeRef): Find {
    store<i64>(this.ptr, t, Q_TYPE);
    return this;
  }
  ownedBy(house: House): Find {
    store<i64>(this.ptr, house, Q_OWNER);
    return this;
  }
  /** Only what `house` sees. */
  seenBy(house: House): Find {
    store<i64>(this.ptr, house, Q_VIEWPOINT);
    return this;
  }
  /** Only entries of a country (a type reference from `Rules.country`). */
  country(country: TypeRef): Find {
    return this.where(Prop.Country, Op.Eq, <i64>TypeRef.id(country));
  }
  /** Only entries whose `prop op value` holds. */
  where(prop: Prop, op: Op, value: i64): Find {
    const count = load<i32>(this.ptr, Q_CLAUSES);
    if (count >= MAX_CLAUSES) {
      this.overflow = true;
      return this;
    }
    const at = this.ptr + <usize>(sys.QUERY_BYTES + count * sys.CLAUSE_BYTES);
    store<u8>(at, <u8>op, 1);
    store<i32>(at, prop, 4);
    store<i64>(at, value, 8);
    store<i32>(this.ptr, count + 1, Q_CLAUSES);
    return this;
  }

  /**
   * @internal The query bytes with `after` set, or 0 for a query with too
   * many clauses; `rawLen` is their length. `Client.findPage` sends them
   * with `client_find`.
   */
  raw(after: i64): usize {
    if (this.overflow) return 0;
    store<i64>(this.ptr, after, Q_AFTER);
    return this.ptr;
  }

  /** @internal The byte length of `raw`'s query. */
  get rawLen(): i32 {
    return sys.QUERY_BYTES + load<i32>(this.ptr, Q_CLAUSES) * sys.CLAUSE_BYTES;
  }

  /**
   * One page from sort key `after` (0 first) into `out`. The returned page
   * is shared: read it before the next search.
   */
  page(out: StaticArray<i64>, after: i64): FindPage {
    PAGE.count = 0;
    PAGE.next = 0;
    const query = this.raw(after);
    if (!query) {
      last = Status.Refused;
      return PAGE;
    }
    if (check(sys.find(query, this.rawLen, changetype<usize>(out), out.length, OUT)) == Status.Ok) {
      PAGE.count = load<i32>(OUT);
      PAGE.next = load<i64>(OUT, 8);
    }
    return PAGE;
  }

  /** The first match, or `NO_HANDLE`. */
  first(): i64 {
    let after: i64 = 0;
    while (true) {
      const page = this.page(ONE, after);
      if (last != Status.Ok) return NO_HANDLE;
      if (page.count == 1) return unchecked(ONE[0]);
      if (page.next == 0) return NO_HANDLE;
      after = page.next;
    }
    return NO_HANDLE;
  }
}

// --------------------------------------------------------------- Objects

export enum Mission {
  Sleep = 0,
  Attack = 1,
  Move = 2,
  QMove = 3,
  Retreat = 4,
  Guard = 5,
  Sticky = 6,
  Enter = 7,
  Capture = 8,
  Eaten = 9,
  Harvest = 10,
  AreaGuard = 11,
  Return = 12,
  Stop = 13,
  Ambush = 14,
  Hunt = 15,
  Unload = 16,
  Sabotage = 17,
  Construction = 18,
  Selling = 19,
  Repair = 20,
  Rescue = 21,
  Missile = 22,
  Harmless = 23,
  Open = 24,
  Patrol = 25,
  ParadropApproach = 26,
  ParadropOverfly = 27,
  Deliberate = 28,
  AttackMove = 29,
  SpyplaneApproach = 30,
  SpyplaneOverfly = 31,
}

/** What `Action.teleport` does when the destination is taken. */
export enum Blocked {
  Nearby = 0,
  Fail = 1,
}

export enum Layer {
  Underground = 0,
  Surface = 1,
  Ground = 2,
  Air = 3,
  Top = 4,
}

export enum Relation {
  SameHouse = 0,
  Ally = 1,
  Enemy = 2,
  Neutral = 3,
}

export enum SpeedType {
  Foot = 0,
  Track = 1,
  Wheel = 2,
  Hover = 3,
  Winged = 4,
  Float = 5,
  Amphibious = 6,
  FloatBeach = 7,
}

export namespace SpeedType {
  /** The `Passable.<speed>` cell property. */
  export function passable(speed: SpeedType): Prop {
    return Prop.Passable_Foot + <i32>speed;
  }
}

export namespace Techno {
  export function health(t: Techno): i64 {
    return get(t, Prop.Health);
  }
  export function setHealth(t: Techno, health: i64): Status {
    return set(t, Prop.Health, health);
  }
  export function owner(t: Techno): House {
    return get(t, Prop.Owner);
  }
  export function setOwner(t: Techno, house: House): Status {
    return set(t, Prop.Owner, house);
  }
  export function typeRef(t: Techno): TypeRef {
    return get(t, Prop.Type);
  }
  export function mission(t: Techno): Mission {
    return <Mission>(<i32>get(t, Prop.Mission));
  }
  /** Gives `t` `mission` as its owner's player would. */
  export function setMission(t: Techno, mission: Mission): Status {
    return set(t, Prop.Mission, <i64>mission);
  }
  export function ammo(t: Techno): i64 {
    return get(t, Prop.Ammo);
  }
  export function setAmmo(t: Techno, ammo: i64): Status {
    return set(t, Prop.Ammo, ammo);
  }
  export function setOnline(t: Techno, online: bool): Status {
    return set(t, Prop.Online, online ? 1 : 0);
  }
  export function layer(t: Techno): Layer {
    return <Layer>(<i32>get(t, Prop.Layer));
  }
  /** The current target (object or cell reference, 0 for none). */
  export function target(t: Techno): Ref {
    return get(t, Prop.Target);
  }
  export function experience(t: Techno): i64 {
    return get(t, Prop.Experience);
  }
  export function storage(t: Techno): i64 {
    return get(t, Prop.Storage);
  }
  export function transport(t: Techno): Techno {
    return get(t, Prop.Transport);
  }
  export function passengerCount(t: Techno): i64 {
    return get(t, Prop.PassengerCount);
  }
  export function transportSpaceLeft(t: Techno): i64 {
    return get(t, Prop.TransportSpaceLeft);
  }
  export function disguiseType(t: Techno): TypeRef {
    return get(t, Prop.DisguiseType);
  }
  export function disguiseOwner(t: Techno): House {
    return get(t, Prop.DisguiseOwner);
  }
  export function moving(t: Techno): bool {
    return get(t, Prop.Moving) != 0;
  }
  export function facing(t: Techno): i64 {
    return get(t, Prop.Facing);
  }
  export function turretFacing(t: Techno): i64 {
    return get(t, Prop.TurretFacing);
  }
  export function armor(t: Techno): i64 {
    return get(t, Prop.Armor);
  }
  export function cell(t: Techno): Cell {
    return get(t, Prop.Cell);
  }
  /**
   * Reads `props` of every object in `list` (one `get_many`), row-major:
   * value `j` of object `i` is at `i * props.length + j`.
   */
  export function read(list: StaticArray<Techno>, props: StaticArray<i32>): StaticArray<i64> {
    const out = new StaticArray<i64>(list.length * props.length);
    getMany(list, props, out);
    return out;
  }
}

export namespace House {
  /**
   * The house at array index `index`. Provisional: the engine preview
   * encodes houses as bit 47 over the index.
   */
  export function fromIndex(index: i32): House {
    return ((<i64>1) << 47) | <i64>(<u32>index);
  }
  export function money(h: House): i64 {
    return get(h, Prop.Money);
  }
  export function name(h: House): string {
    return getText(h, Prop.Name);
  }
  export function isHuman(h: House): bool {
    return get(h, Prop.IsHuman) != 0;
  }
  export function isDefeated(h: House): bool {
    return get(h, Prop.IsDefeated) != 0;
  }
  /** Queues `order` for computer house `h`; it runs next frame. */
  export function order(h: House, order: Order): Status {
    const len = order.write();
    if (len < 0) {
      last = Status.Refused;
      return Status.Refused;
    }
    return check(sys.order(h, ORDER_BUF, len));
  }
}

/** The scenario and global rules (appendix 7.8). */
export namespace Global {
  /** The global rules reference. */
  export const REF: Ref = KIND_GLOBAL;
  export function frame(): i64 {
    return get(REF, Prop.Frame);
  }
  export function difficulty(): i64 {
    return get(REF, Prop.Difficulty);
  }
  export function missionTimer(): i64 {
    return get(REF, Prop.MissionTimer);
  }
  export function missionTimerRunning(): bool {
    return get(REF, Prop.MissionTimerRunning) != 0;
  }
  export function ambientLight(): i64 {
    return get(REF, Prop.AmbientLight);
  }
  export function ionStormActive(): bool {
    return get(REF, Prop.IonStormActive) != 0;
  }
}

// ----------------------------------------------------------------- Rules

export namespace Rules {
  function typeNamed(kind: NameKind, name: string): TypeRef {
    const id = lookup(kind, name);
    return id == NO_ID ? NO_HANDLE : TypeRef.make(kind, id);
  }
  /** TechnoType `name`; resolve once, at initialization or scenario start. */
  export function technoType(name: string): TypeRef {
    return typeNamed(NameKind.TechnoType, name);
  }
  export function weapon(name: string): TypeRef {
    return typeNamed(NameKind.Weapon, name);
  }
  export function warhead(name: string): TypeRef {
    return typeNamed(NameKind.Warhead, name);
  }
  /** Projectile (BulletType) `name`. */
  export function projectile(name: string): TypeRef {
    return typeNamed(NameKind.Projectile, name);
  }
  export function superWeaponType(name: string): TypeRef {
    return typeNamed(NameKind.SuperWeapon, name);
  }
  export function country(name: string): TypeRef {
    return typeNamed(NameKind.Country, name);
  }
  export function teamType(name: string): TypeRef {
    return typeNamed(NameKind.TeamType, name);
  }
  /** The ID of animation `name`, which `Action.spawnAnim` takes. */
  export function anim(name: string): i32 {
    return lookup(NameKind.Anim, name);
  }

  /** The raw value of `key` in `section`, or `""` (`NotFound` for no key). */
  export function get(section: string, key: string): string {
    const sec = utf8(section, 0);
    const secLen = utf8Len;
    const k = utf8(key, 1);
    const kLen = utf8Len;
    if (check(sys.rules_get(sec, secLen, k, kLen, NAME_BUF, NAME_BYTES, OUT2)) != Status.Ok) return "";
    const len = load<i32>(OUT2);
    if (len <= NAME_BYTES) return String.UTF8.decodeUnsafe(NAME_BUF, <usize>len);
    const buffer = new ArrayBuffer(len);
    if (check(sys.rules_get(sec, secLen, k, kLen, changetype<usize>(buffer), len, OUT2)) != Status.Ok) return "";
    return String.UTF8.decode(buffer);
  }

  /** Writes `key` in `section`; only in `onRules`. */
  export function set(section: string, key: string, value: string): Status {
    const sec = utf8(section, 0);
    const secLen = utf8Len;
    const k = utf8(key, 1);
    const kLen = utf8Len;
    const v = utf8(value, 2);
    return check(sys.rules_set(sec, secLen, k, kLen, v, utf8Len));
  }
}

// --------------------------------------------------------------- Actions

export enum Outcome {
  Win = 0,
  Lose = 1,
}

const PARAMS: usize = memory.data(sys.ACTION_PARAMS_BYTES, 8);

/** @internal Clears the shared `ActionParams`. */
export function params(): usize {
  memory.fill(PARAMS, 0, <usize>sys.ACTION_PARAMS_BYTES);
  return PARAMS;
}

function textParams(s: string): usize {
  const p = params();
  const at = utf8(s);
  store<i32>(p, <i32>at, sys.AP_TEXT);
  store<i32>(p, utf8Len, sys.AP_TEXT_LEN);
  return p;
}

function run(kind: i32, target: i64, p: usize): Status {
  return check(sys.action(kind, target, p, sys.ACTION_PARAMS_BYTES, OUT));
}

function created(kind: i32, target: i64, p: usize): i64 {
  if (run(kind, target, p) != Status.Ok) return NO_HANDLE;
  return load<i64>(OUT);
}

/** One function per action kind (appendix 5); each is one `action` call. */
export namespace Action {
  /** Any action `kind` on `target` with the `ActionParams` at `p`, returning the created object (or 0). */
  export function runRaw(kind: i32, target: i64, p: usize): i64 {
    run(kind, target, p);
    return load<i64>(OUT);
  }
  /** Any action `kind` on `target` taking a text. */
  export function runText(kind: i32, target: i64, s: string): Status {
    return run(kind, target, textParams(s));
  }

  // Atomic actions (5.3).
  export function launchSuperWeapon(house: House, kind: TypeRef, at: Ref): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, at, sys.AP_CELL);
    return run(ActionKind.LaunchSuperWeapon, house, p);
  }
  export function remove(target: Techno): Status {
    return run(ActionKind.Remove, target, params());
  }
  /** Kills `target`, crediting `source` (`NO_HANDLE` for none). */
  export function kill(target: Techno, source: Techno): Status {
    const p = params();
    store<i64>(p, source, sys.AP_OTHER);
    return run(ActionKind.Kill, target, p);
  }
  export function damage(target: Techno, amount: i32, warhead: TypeRef, source: Techno = NO_HANDLE): Status {
    const p = params();
    store<i32>(p, TypeRef.id(warhead), sys.AP_ID);
    store<i64>(p, <i64>amount, sys.AP_VALUE);
    store<i64>(p, source, sys.AP_OTHER);
    return run(ActionKind.Damage, target, p);
  }
  /** Creates `kind` for `house` at `at` and returns it, or `NO_HANDLE`. */
  export function createObject(at: Ref, kind: TypeRef, house: House): Techno {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i32>(p, 255, sys.AP_ARG);
    store<i64>(p, house, sys.AP_HOUSE);
    return created(ActionKind.CreateObject, at, p);
  }
  export function clearTarget(target: Techno): Status {
    return run(ActionKind.ClearTarget, target, params());
  }
  /** Gives `house` `amount` credits (negative takes). */
  export function addMoney(house: House, amount: i32): Status {
    const p = params();
    store<i64>(p, <i64>amount, sys.AP_VALUE);
    return run(ActionKind.AddMoney, house, p);
  }
  export function setOwner(target: Techno, house: House): Status {
    const p = params();
    store<i64>(p, house, sys.AP_HOUSE);
    return run(ActionKind.SetOwner, target, p);
  }
  export function detonateWarhead(at: Ref, warhead: TypeRef, damage: i32): Status {
    const p = params();
    store<i32>(p, TypeRef.id(warhead), sys.AP_ID);
    store<i64>(p, <i64>damage, sys.AP_VALUE);
    return run(ActionKind.DetonateWarhead, at, p);
  }
  export function endFor(house: House, outcome: Outcome): Status {
    const p = params();
    store<i32>(p, <i32>outcome, sys.AP_ARG);
    return run(ActionKind.EndFor, house, p);
  }
  /** Writes `line` to the in-game log as an action. */
  export function log(line: string): Status {
    return run(ActionKind.Log, 0, textParams(line));
  }
  export function spawnAnim(at: Ref, anim: i32): Status {
    const p = params();
    store<i32>(p, anim, sys.AP_ID);
    return run(ActionKind.SpawnAnim, at, p);
  }
  export function heal(target: Techno, amount: i32): Status {
    const p = params();
    store<i64>(p, <i64>amount, sys.AP_VALUE);
    return run(ActionKind.Heal, target, p);
  }
  /** Gives `target` `mission` toward cell or waypoint `at` (0 for none). */
  export function setMission(target: Techno, mission: Mission, at: Ref = 0): Status {
    const p = params();
    store<i32>(p, <i32>mission, sys.AP_ARG);
    store<i64>(p, at, sys.AP_CELL);
    return run(ActionKind.SetMission, target, p);
  }
  /** Gives `target` `mission` on object `on`. */
  export function setMissionOn(target: Techno, mission: Mission, on: Techno): Status {
    const p = params();
    store<i32>(p, <i32>mission, sys.AP_ARG);
    store<i64>(p, on, sys.AP_OTHER);
    return run(ActionKind.SetMission, target, p);
  }
  export function teleport(target: Techno, to: Ref, blocked: Blocked): Status {
    const p = params();
    store<i32>(p, <i32>blocked, sys.AP_ARG);
    store<i64>(p, to, sys.AP_CELL);
    return run(ActionKind.Teleport, target, p);
  }
  export function clearDisguise(target: Techno): Status {
    return run(ActionKind.ClearDisguise, target, params());
  }
  export function clearMindControl(target: Techno): Status {
    return run(ActionKind.ClearMindControl, target, params());
  }
  export function addExperience(target: Techno, points: i32): Status {
    const p = params();
    store<i64>(p, <i64>points, sys.AP_VALUE);
    return run(ActionKind.AddExperience, target, p);
  }
  export function addAmmo(target: Techno, amount: i32): Status {
    const p = params();
    store<i64>(p, <i64>amount, sys.AP_VALUE);
    return run(ActionKind.AddAmmo, target, p);
  }
  export function setProperty(target: Ref, prop: Prop, value: i64): Status {
    const p = params();
    store<i32>(p, prop, sys.AP_ID);
    store<i64>(p, value, sys.AP_VALUE);
    return run(ActionKind.SetProperty, target, p);
  }
  /** Adds a modifier as an action and returns it, or `NO_HANDLE`. */
  export function modify(target: Ref, prop: Prop, op: ModifierOp, value: i64, frames: i32, group: i32): Modifier {
    const p = params();
    store<i32>(p, prop, sys.AP_ID);
    store<i32>(p, <i32>op, sys.AP_ARG);
    store<i32>(p, group, sys.AP_ARG2);
    store<i64>(p, value, sys.AP_VALUE);
    store<i64>(p, <i64>frames, sys.AP_VALUE2);
    return created(ActionKind.Modify, target, p);
  }
  /** Removes this module's modifiers in `group` on `target`, of `prop` only unless `prop` is −1. */
  export function removeModifiers(target: Ref, group: i32, prop: Prop = -1): Status {
    const p = params();
    store<i32>(p, prop, sys.AP_ID);
    store<i32>(p, group, sys.AP_ARG2);
    return run(ActionKind.RemoveModifiers, target, p);
  }
  export function detonateWeapon(at: Ref, weapon: TypeRef, source: Techno): Status {
    const p = params();
    store<i32>(p, TypeRef.id(weapon), sys.AP_ID);
    store<i64>(p, source, sys.AP_OTHER);
    return run(ActionKind.DetonateWeapon, at, p);
  }
  export function setScore(house: House, score: i32): Status {
    const p = params();
    store<i64>(p, <i64>score, sys.AP_VALUE);
    return run(ActionKind.SetScore, house, p);
  }
  export function addScore(house: House, score: i32): Status {
    const p = params();
    store<i64>(p, <i64>score, sys.AP_VALUE);
    return run(ActionKind.AddScore, house, p);
  }
  /** Converts `target` to `kind`; for `frames` frames, or for good with 0. */
  export function convert(target: Techno, kind: TypeRef, frames: i32 = 0): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, <i64>frames, sys.AP_VALUE2);
    return run(ActionKind.Convert, target, p);
  }
  export function push(target: Techno, toward: Cell, distance: i32, frames: i32): Status {
    const p = params();
    store<i64>(p, toward, sys.AP_CELL);
    store<i64>(p, <i64>distance, sys.AP_VALUE);
    store<i64>(p, <i64>frames, sys.AP_VALUE2);
    return run(ActionKind.Push, target, p);
  }
  export function setFacing(target: Techno, facing: i32): Status {
    const p = params();
    store<i64>(p, <i64>(facing & 0xff), sys.AP_VALUE);
    return run(ActionKind.SetFacing, target, p);
  }
  /** Fires weapon `slot` of `shooter` at its target, object `at` or cell `cell`. */
  export function fireWeapon(shooter: Techno, slot: i32, at: Techno = NO_HANDLE, cell: Ref = 0): Status {
    const p = params();
    store<i32>(p, -1, sys.AP_ID);
    store<i64>(p, <i64>slot, sys.AP_VALUE);
    store<i64>(p, at, sys.AP_OTHER);
    store<i64>(p, cell, sys.AP_CELL);
    return run(ActionKind.FireWeapon, shooter, p);
  }
  /** Plays animation `anim` attached to `target`: once with `frames` 0,
   *  looping for `frames` frames, or until `detachAnim` when negative. */
  export function attachAnim(target: Techno, anim: i32, frames: i64 = 0): Status {
    const p = params();
    store<i32>(p, anim, sys.AP_ID);
    store<i64>(p, frames, sys.AP_VALUE2);
    return run(ActionKind.AttachAnim, target, p);
  }
  /** Removes the animations `attachAnim` with frames kept on `target`:
   *  those of `anim`, or all with -1. */
  export function detachAnim(target: Techno, anim: i32 = -1): Status {
    const p = params();
    store<i32>(p, anim, sys.AP_ID);
    return run(ActionKind.DetachAnim, target, p);
  }
  export function allowType(house: House, kind: TypeRef, allow: bool): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, allow ? 1 : 0, sys.AP_VALUE);
    return run(ActionKind.AllowType, house, p);
  }
  export function addPassenger(transport: Techno, passenger: Techno): Status {
    const p = params();
    store<i64>(p, passenger, sys.AP_OTHER);
    return run(ActionKind.AddPassenger, transport, p);
  }
  /** Raises module event `event` on `target` (anything for a global event). */
  export function raiseEvent(event: i32, target: Ref, other: Techno, value: i64, value2: i64, house: House = NO_HANDLE): Status {
    const p = params();
    store<i32>(p, event, sys.AP_ID);
    store<i64>(p, other, sys.AP_OTHER);
    store<i64>(p, value, sys.AP_VALUE);
    store<i64>(p, value2, sys.AP_VALUE2);
    store<i64>(p, house, sys.AP_HOUSE);
    return run(ActionKind.RaiseEvent, target, p);
  }

  // Stock actions (5.2) taking a string.
  export function declareWinner(country: string): Status { return runText(1, 0, country); }
  export function declareLoser(country: string): Status { return runText(2, 0, country); }
  export function showText(label: string): Status { return runText(11, 0, label); }
  export function destroyTrigger(id: string): Status { return runText(12, 0, id); }
  export function playTheme(theme: string): Status { return runText(20, 0, theme); }
  export function playSpeech(eva: string): Status { return runText(21, 0, eva); }
  export function forceTrigger(id: string): Status { return runText(22, 0, id); }
  export function enableTrigger(id: string): Status { return runText(53, 0, id); }
  export function disableTrigger(id: string): Status { return runText(54, 0, id); }
  export function destroyTag(id: string): Status { return runText(70, 0, id); }
  export function timerText(label: string): Status { return runText(103, 0, label); }

  // Stock actions (5.2) with no target.
  function bare(kind: i32): Status { return run(kind, 0, params()); }
  export function revealAll(): Status { return bare(16); }
  export function timerStart(): Status { return bare(23); }
  export function timerStop(): Status { return bare(24); }
  export function growShroud(): Status { return bare(31); }
  export function ionStormStop(): Status { return bare(45); }
  export function lockInput(): Status { return bare(46); }
  export function unlockInput(): Status { return bare(47); }
  export function announceWin(): Status { return bare(67); }
  export function announceLose(): Status { return bare(68); }
  export function endScenario(): Status { return bare(69); }
  export function toggleTrainCargo(): Status { return bare(97); }
  export function clearSmudges(): Status { return bare(118); }
  export function homeCamera(): Status { return bare(145); }

  // A value.
  function valued(kind: i32, target: i64, value: i64): Status {
    const p = params();
    store<i64>(p, value, sys.AP_VALUE);
    return run(kind, target, p);
  }
  export function playMovie(movie: i32): Status { return valued(10, 0, movie); }
  export function timerExtend(seconds: i32): Status { return valued(25, 0, seconds); }
  export function timerShorten(seconds: i32): Status { return valued(26, 0, seconds); }
  export function timerSet(seconds: i32): Status { return valued(27, 0, seconds); }
  export function setGlobal(n: i32): Status { return valued(28, 0, n); }
  export function clearGlobal(n: i32): Status { return valued(29, 0, n); }
  export function ionStormStart(duration: i32): Status { return valued(44, 0, duration); }
  export function setLocal(n: i32): Status { return valued(56, 0, n); }
  export function clearLocal(n: i32): Status { return valued(57, 0, n); }
  export function setAmbientLight(value: i32): Status { return valued(73, 0, value); }
  export function wakeGroup(group: i32): Status { return valued(84, 0, group); }
  export function playRadarMovie(movie: i32): Status { return valued(100, 0, movie); }
  export function pauseGame(duration: i32): Status { return valued(110, 0, duration); }
  export function setSidebarTab(tab: i32): Status { return valued(114, 0, tab); }
  export function chronoScreenEffect(duration: i32): Status { return valued(127, 0, duration); }

  // TeamTypes (`Rules.teamType`).
  function teamed(kind: i32, target: i64, team: TypeRef): Status {
    const p = params();
    store<i32>(p, TypeRef.id(team), sys.AP_ID);
    return run(kind, target, p);
  }
  export function createTeam(team: TypeRef): Status { return teamed(4, 0, team); }
  export function destroyTeams(team: TypeRef): Status { return teamed(5, 0, team); }
  export function reinforce(team: TypeRef): Status { return teamed(7, 0, team); }
  export function reinforceAt(team: TypeRef, at: Waypoint): Status { return teamed(80, at, team); }
  export function reinforceByChrono(team: TypeRef, at: Waypoint): Status { return teamed(107, at, team); }

  // A waypoint.
  export function dropZoneFlare(at: Waypoint): Status { return run(8, at, params()); }
  export function revealAround(at: Waypoint): Status { return run(17, at, params()); }
  export function revealZone(at: Waypoint): Status { return run(18, at, params()); }
  export function lightningStrike(at: Waypoint): Status { return run(90, at, params()); }
  export function ionCannonStrike(at: Waypoint): Status { return run(94, at, params()); }
  export function nukeStrike(at: Waypoint): Status { return run(95, at, params()); }
  export function chemStrike(at: Waypoint): Status { return run(96, at, params()); }
  export function reshroudAt(at: Waypoint): Status { return run(101, at, params()); }
  export function ironCurtainAt(at: Waypoint): Status { return run(109, at, params()); }
  export function jumpCamera(at: Waypoint): Status { return run(112, at, params()); }
  export function playAnimAt(at: Waypoint, anim: i32): Status {
    const p = params();
    store<i32>(p, anim, sys.AP_ID);
    return run(41, at, p);
  }
  export function detonateWeaponAt(at: Waypoint, weapon: i32): Status {
    const p = params();
    store<i32>(p, weapon, sys.AP_ID);
    return run(42, at, p);
  }
  export function centerCamera(at: Waypoint, speed: i32): Status { return valued(48, at, speed); }
  export function meteorShower(at: Waypoint, intensity: i32): Status { return valued(58, at, intensity); }
  export function createCrate(at: Waypoint, kind: i32): Status { return valued(108, at, kind); }
  export function lightningStormStrike(at: Waypoint, house: House): Status {
    const p = params();
    store<i64>(p, house, sys.AP_HOUSE);
    return run(102, at, p);
  }
  export function createBuildingAt(at: Waypoint, kind: TypeRef, house: House): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, house, sys.AP_HOUSE);
    return run(125, at, p);
  }

  // Types.
  export function setTechLevel(kind: TypeRef, level: i32): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, <i64>level, sys.AP_VALUE);
    return run(106, 0, p);
  }
  export function flashCameo(kind: TypeRef, frames: i32): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, <i64>frames, sys.AP_VALUE);
    return run(115, 0, p);
  }

  // A house.
  export function autoBaseBuilding(house: House, on: bool): Status { return valued(30, house, on ? 1 : 0); }
  export function reshroud(house: House): Status { return run(51, house, params()); }
  export function wakeSleepers(house: House): Status { return run(82, house, params()); }
  export function wakeHarmless(house: House): Status { return run(83, house, params()); }
  export function radarBlackout(house: House, frames: i32): Status { return valued(139, house, frames); }
  export function oneTimeSuperWeapon(house: House, kind: TypeRef): Status { return teamed(33, house, kind); }
  export function repeatingSuperWeapon(house: House, kind: TypeRef): Status { return teamed(34, house, kind); }
  export function superWeaponSetCharge(house: House, kind: TypeRef, percent: i32): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, <i64>percent, sys.AP_VALUE);
    return run(129, house, p);
  }
  export function superWeaponReset(house: House, kind: TypeRef): Status { return teamed(134, house, kind); }
  export function teleportAll(house: House, to: Waypoint): Status {
    const p = params();
    store<i64>(p, to, sys.AP_CELL);
    return run(128, house, p);
  }
  export function setAlternateBase(house: House, at: Waypoint): Status {
    const p = params();
    store<i64>(p, at, sys.AP_CELL);
    return run(137, house, p);
  }
  export function clearAlternateBase(house: House): Status { return run(138, house, params()); }
  export function flashBuildings(house: House, kind: TypeRef, frames: i32): Status {
    const p = params();
    store<i32>(p, TypeRef.id(kind), sys.AP_ID);
    store<i64>(p, <i64>frames, sys.AP_VALUE);
    return run(131, house, p);
  }

  // An object.
  export function changeHouse(target: Techno, house: House): Status {
    const p = params();
    store<i64>(p, house, sys.AP_HOUSE);
    return run(14, target, p);
  }
  export function destroyObject(target: Techno): Status { return run(32, target, params()); }
  export function sellBuilding(target: Techno): Status { return run(60, target, params()); }
  export function turnOffBuilding(target: Techno): Status { return run(61, target, params()); }
  export function turnOnBuilding(target: Techno): Status { return run(62, target, params()); }
}

// ---------------------------------------------------------------- Orders

/** Standard player commands (`Order.command`, appendix 8.3). */
export enum Command {
  Move = 0,
  MoveToObject = 1,
  Stop = 2,
  Attack = 3,
  AttackMove = 4,
  Guard = 5,
  DeployMcv = 6,
  UndeployBuilding = 7,
  SetRally = 8,
  QueueProduction = 9,
  SuspendProduction = 10,
  CancelProductionByType = 11,
  PlaceReadyBuilding = 12,
  SellBuilding = 13,
  ToggleRepair = 14,
  EjectBunker = 15,
  UnloadPassengers = 16,
  CaptureBuilding = 17,
  PlantC4 = 18,
  LaunchSuperWeapon = 19,
  ToggleInfantryDeploy = 20,
  ForceAttackCell = 21,
  SellWallAtCell = 22,
  Scatter = 23,
  TogglePrimaryFactory = 24,
  Enter = 25,
  Harvest = 26,
  TakeProduct = 27,
}

/** Most units one order carries. */
export const ORDER_UNITS: i32 = 64;

/** @internal The words of the latest `Order.write`. */
export const ORDER_BUF: usize = memory.data((3 + ORDER_UNITS) * 8, 8);

const NO_UNITS = new StaticArray<Techno>(0);

/**
 * A standard player command (`Order`), for `House.order` and
 * `Client.order`. Build one with the static functions.
 */
export class Order {
  constructor(
    public command: Command,
    public target: i64,
    public value: i64,
    public units: StaticArray<Techno>,
  ) {}

  static moveTo(units: StaticArray<Techno>, cell: Cell): Order {
    return new Order(Command.Move, cell, 0, units);
  }
  static moveToObject(units: StaticArray<Techno>, target: Techno): Order {
    return new Order(Command.MoveToObject, target, 0, units);
  }
  static stop(units: StaticArray<Techno>): Order {
    return new Order(Command.Stop, 0, 0, units);
  }
  static attack(units: StaticArray<Techno>, target: Techno): Order {
    return new Order(Command.Attack, target, 0, units);
  }
  static attackMove(units: StaticArray<Techno>, cell: Cell): Order {
    return new Order(Command.AttackMove, cell, 0, units);
  }
  static guard(units: StaticArray<Techno>, target: Techno): Order {
    return new Order(Command.Guard, target, 0, units);
  }
  static deployMcv(units: StaticArray<Techno>): Order {
    return new Order(Command.DeployMcv, 0, 0, units);
  }
  static setRally(factories: StaticArray<Techno>, cell: Cell): Order {
    return new Order(Command.SetRally, cell, 0, factories);
  }
  static queueProduction(t: TypeRef): Order {
    return new Order(Command.QueueProduction, 0, t, NO_UNITS);
  }
  static cancelProduction(t: TypeRef): Order {
    return new Order(Command.CancelProductionByType, 0, t, NO_UNITS);
  }
  static placeReadyBuilding(t: TypeRef, cell: Cell): Order {
    return new Order(Command.PlaceReadyBuilding, cell, t, NO_UNITS);
  }
  static sell(buildings: StaticArray<Techno>): Order {
    return new Order(Command.SellBuilding, 0, 0, buildings);
  }
  static launchSuperWeapon(t: TypeRef, cell: Cell): Order {
    return new Order(Command.LaunchSuperWeapon, cell, t, NO_UNITS);
  }
  /** Takes a finished product of `kind` (a factory kind, appendix 8.3). */
  static takeProduct(kind: i64): Order {
    return new Order(Command.TakeProduct, 0, kind, NO_UNITS);
  }

  /** @internal Writes the order's words to `ORDER_BUF`; its byte length, or −1 for too many units. */
  write(): i32 {
    const count = this.units.length;
    if (count > ORDER_UNITS) return -1;
    store<i64>(ORDER_BUF, (<i64>(<u32>this.command)) | ((<i64>count) << 32), 0);
    store<i64>(ORDER_BUF, this.target, 8);
    store<i64>(ORDER_BUF, this.value, 16);
    for (let i = 0; i < count; i++) {
      store<i64>(ORDER_BUF + <usize>((3 + i) * 8), unchecked(this.units[i]));
    }
    return (3 + count) * 8;
  }
}
