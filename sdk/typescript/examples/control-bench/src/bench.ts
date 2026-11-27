// The bench's shared helpers, the part of the Rust `lib.rs` every area
// uses: the marks, the arena (the houses, the player's start, each area's
// corner, `spawn`) and a raw `find` for queries `Find` cannot build.
//
// Each area of the Rust bench owns a hundred of trigger keys, and `lib`
// routes records by key. Here each handler is its own function, which the
// SDK gives its own dispatch key; a handler of an area (not the finale)
// returns at once when `finaleStarted()` (see `finale.ts`), as the Rust
// `on_events` and `on_decide` skip the areas' records then.
import {
  Action, Cell, Find, House, NO_HANDLE, Op, Prop, Ref, Rules, Status, Techno, get, lastStatus, log,
} from "@vera20k/sdk";
import * as sys from "@vera20k/sdk/assembly/sys";

// ----------------------------------------------------------------- Areas

export const BASICS: i32 = 1;
export const TRIGGERS: i32 = 2;
export const COMBAT: i32 = 3;
export const MOVEMENT: i32 = 4;
export const FINALE: i32 = 5;

// ----------------------------------------------------------------- Marks

const MAX_MARKS: i32 = 1024;
/** An open-addressed set of name hashes (0 is a free slot). */
const MARKED = new StaticArray<i64>(MAX_MARKS);

/** FNV-1a over the name's (ASCII) bytes, never 0. */
export function hash(name: string): i64 {
  let h: u64 = 0xcbf29ce484222325;
  const chars = changetype<usize>(name);
  for (let i = 0; i < name.length; i++) {
    // `i` is below the length: read the UTF-16 unit without a bounds check.
    h ^= <u64>(load<u16>(chars + (<usize>i << 1)) & 0xff);
    h *= 0x100000001b3;
  }
  return <i64>(h | 1);
}

function firstSlot(h: i64): i32 {
  return <i32>(((<u64>h) >> 1) % <u64>MAX_MARKS);
}

/** Adds `h` to the set; false when it was there already (or the set is full). */
function insert(h: i64): bool {
  const first = firstSlot(h);
  for (let step = 0; step < MAX_MARKS; step++) {
    const slot = (first + step) % MAX_MARKS;
    const v = unchecked(MARKED[slot]);
    if (v == 0) {
      unchecked((MARKED[slot] = h));
      return true;
    }
    if (v == h) return false;
  }
  return false;
}

/** Whether hash `h` (`hash(name)`) is in the set. */
export function contains(h: i64): bool {
  const first = firstSlot(h);
  for (let step = 0; step < MAX_MARKS; step++) {
    const v = unchecked(MARKED[(first + step) % MAX_MARKS]);
    if (v == 0) return false;
    if (v == h) return true;
  }
  return false;
}

/** Logs `DEMO-OK <name>` the first time `name` is marked. */
export function mark(name: string): void {
  if (insert(hash(name))) log("DEMO-OK " + name);
}

/** Whether `name` was marked. */
export function marked(name: string): bool {
  return contains(hash(name));
}

/** Logs `BENCH-FAIL <name>: <why>` once per name. */
export function benchFail(name: string, why: string): void {
  if (insert(hash(name) ^ 0x5a5a)) log("BENCH-FAIL " + name + ": " + why);
}

/** Marks `name` when `ok`, otherwise logs a bench failure. */
export function check(name: string, ok: bool, why: string): void {
  if (ok) mark(name);
  else benchFail(name, why);
}

// ------------------------------------------------------- Result helpers
// The Rust bench compares `Result`s (`get(x, p) == Ok(v)`); a TypeScript
// read returns the value and leaves the status in `lastStatus()`.

/** Whether the latest read answered `Ok`. */
export function ok(): bool {
  return lastStatus() == Status.Ok;
}

/**
 * Whether `Trigger.done()` (or `decideSync`) answered as the Rust `Ok`:
 * the Rust `done` fails on a failed binding, the TypeScript one returns
 * the trigger with `lastStatus()` the binding's status. Call it right
 * after `done`.
 */
export function made(trigger: i64): bool {
  return trigger != NO_HANDLE && lastStatus() == Status.Ok;
}

/** `get(reference, prop) == Ok(value)`. */
export function is(reference: Ref, prop: Prop, value: i64): bool {
  const v = get(reference, prop);
  return lastStatus() == Status.Ok && v == value;
}

/** `get(reference, prop) == Err(status)`. */
export function fails(reference: Ref, prop: Prop, status: Status): bool {
  get(reference, prop);
  return lastStatus() == status;
}

/** `get(reference, prop).unwrap_or(otherwise)`. */
export function getOr(reference: Ref, prop: Prop, otherwise: i64): i64 {
  const v = get(reference, prop);
  return lastStatus() == Status.Ok ? v : otherwise;
}

/** Whether `out[..n]` holds `value`. */
export function holds(out: StaticArray<i64>, n: i32, value: i64): bool {
  for (let i = 0; i < n && i < out.length; i++) {
    if (unchecked(out[i]) == value) return true;
  }
  return false;
}

/** A decimal number (`ini::int`'s use here: digits only, trimmed), −1 for other text. */
export function parse(text: string): i64 {
  const s = text.trim();
  if (s.length == 0) return -1;
  let value: i64 = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 48 || c > 57) return -1;
    value = value * 10 + <i64>(c - 48);
  }
  return value;
}

// ----------------------------------------------------------------- Arena

let playerHouse: House = NO_HANDLE;
let enemyHouse: House = NO_HANDLE;
let baseCell: Cell = NO_HANDLE;

/** The one human house. */
export function player(): House {
  return playerHouse;
}

/** The first computer house. */
export function enemy(): House {
  return enemyHouse;
}

/** The player's starting cell (their first object, the MCV). */
export function base(): Cell {
  return baseCell;
}

function clamp(v: i32, lo: i32, hi: i32): i32 {
  return v < lo ? lo : v > hi ? hi : v;
}

export function offset(cell: Cell, dx: i32, dy: i32): Cell {
  return Cell.at(clamp(Cell.x(cell) + dx, 1, 510), clamp(Cell.y(cell) + dy, 1, 510));
}

/** Each area's own corner of the map, a few cells out from the player's start. */
export function arena(area: i32): Cell {
  if (area == BASICS) return offset(base(), -10, -10);
  if (area == TRIGGERS) return offset(base(), 10, -10);
  if (area == COMBAT) return offset(base(), -10, 10);
  return offset(base(), 10, 10);
}

/** Creates a unit of type `id` for `house` on `spot` or the closest ring of cells that takes it; `NO_HANDLE` if none. */
export function spawn(spot: Cell, id: string, house: House): Techno {
  const ty = Rules.technoType(id);
  if (ty == NO_HANDLE) return NO_HANDLE;
  for (let ring = 0; ring <= 6; ring++) {
    for (let dy = -ring; dy <= ring; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (max(abs(dx), abs(dy)) != ring) continue;
        const unit = Action.createObject(offset(spot, dx, dy), ty, house);
        if (unit != NO_HANDLE && lastStatus() == Status.Ok) return unit;
      }
    }
  }
  return NO_HANDLE;
}

export function alive(unit: Techno): bool {
  if (unit == NO_HANDLE) return false;
  const health = get(unit, Prop.Health);
  return lastStatus() == Status.Ok && health > 0;
}

const HUMANS = new StaticArray<i64>(2);
const COMPUTERS = new StaticArray<i64>(16);

/** Finds the player, the opponent and the player's start; false without them. */
export function findSides(): bool {
  const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(HUMANS, 0);
  if (lastStatus() != Status.Ok || humans.count != 1) return false;
  const human = HUMANS[0];
  const first = Find.objects().ownedBy(human).first();
  const cell = get(first, Prop.Cell);
  if (lastStatus() != Status.Ok) return false;
  const page = Find.houses()
    .where(Prop.IsHuman, Op.Eq, 0)
    .where(Prop.IsDefeated, Op.Eq, 0)
    .page(COMPUTERS, 0);
  const count = lastStatus() == Status.Ok ? page.count : 0;
  // The first computer house that owns something: the opponent, not a
  // neutral or special house.
  let foe: House = NO_HANDLE;
  for (let i = 0; i < count; i++) {
    const h = COMPUTERS[i];
    if (Find.objects().ownedBy(h).first() != NO_HANDLE) {
      foe = h;
      break;
    }
  }
  if (foe == NO_HANDLE) return false;
  playerHouse = human;
  enemyHouse = foe;
  baseCell = cell;
  return true;
}

// ------------------------------------------------------------- Raw find
// `Find` has no constructor for the `Modifiers` (7) and `SuperWeapons` (8)
// scopes and no variable clause; `rawFind` builds such a query over
// `sys.find`, as the Rust bench's `raw_find` does.

const RAW_QUERY: usize = memory.data(sys.QUERY_BYTES + 2 * sys.CLAUSE_BYTES, 8);
const RAW_RESULT: usize = memory.data(sys.FIND_RESULT_BYTES, 8);

/** One raw clause (`Clause`): `subject` 0 the candidate's property, 3 a variable of `scope`. */
export class Clause {
  constructor(
    public subject: u8,
    public scope: u8,
    public prop: i32,
    public op: Op,
    public value: i64,
  ) {}
}

/** A clause on the candidate's property `prop`. */
export function propClause(prop: Prop, op: Op, value: i64): Clause {
  return new Clause(0, 0, prop, op, value);
}

/** A clause on declared variable `v` of the candidate (subject 3, scope 1 Subject). */
export function varClause(v: i32, op: Op, value: i64): Clause {
  return new Clause(3, 1, v, op, value);
}

const NO_CLAUSES = new StaticArray<Clause>(0);

/** A `find` of any scope with an owner, a type and up to two clauses: the count found, −1 unless it answered `Ok`. */
export function rawFind(scope: i32, owner: i64, typeRef: i64, clauses: StaticArray<Clause>, out: StaticArray<i64>): i32 {
  const count = min(clauses.length, 2);
  memory.fill(RAW_QUERY, 0, <usize>(sys.QUERY_BYTES + 2 * sys.CLAUSE_BYTES));
  store<i32>(RAW_QUERY, scope, 0);
  store<i64>(RAW_QUERY, typeRef, 24);
  store<i64>(RAW_QUERY, owner, 32);
  store<i32>(RAW_QUERY, count, 56);
  for (let i = 0; i < count; i++) {
    const c = clauses[i];
    const at = RAW_QUERY + <usize>(sys.QUERY_BYTES + i * sys.CLAUSE_BYTES);
    store<u8>(at, c.subject, 0);
    store<u8>(at, <u8>c.op, 1);
    store<u8>(at, c.scope, 2);
    store<i32>(at, c.prop, 4);
    store<i64>(at, c.value, 8);
  }
  const len = sys.QUERY_BYTES + count * sys.CLAUSE_BYTES;
  const status = sys.find(RAW_QUERY, len, changetype<usize>(out), out.length, RAW_RESULT);
  return status == 0 ? load<i32>(RAW_RESULT) : -1;
}

/** `rawFind` without clauses. */
export function rawFindAll(scope: i32, owner: i64, typeRef: i64, out: StaticArray<i64>): i32 {
  return rawFind(scope, owner, typeRef, NO_CLAUSES, out);
}
