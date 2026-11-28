// "IniAura", a demo `rules.wasm` for RFC 0001 section 6h, the same as the
// Rust SDK's `sdk/examples/ini-aura`: the module supplies building blocks,
// the modder writes every aura in INI. Nothing about a particular aura is
// in this code.
//
// - Registry `[Demo.AuraTypes]`: a list of aura sections. Each holds
//   `Radius=` (cells), `Property=` (a modifiable property the engine wires:
//   `FirepowerMultiplier`, `SpeedMultiplier`, `ArmorMultiplier`,
//   `ROFMultiplier`), `Op=` (`add`, `multiply`, `override`), `Value=` (a
//   decimal such as `1.25`), `Affects=` (`allies` or `enemies`) and
//   `Interval=` (frames between refreshes, at least 15).
// - Key `Demo.Aura=` on a TechnoType: the aura sections the type emits,
//   comma-separated.
// - Object event `Demo.AuraEntered`: raised on a unit the first time an
//   aura covers it (`value` is the aura's position in the list, `other`
//   the emitter). INI triggers listen with `Event=Wasm:Demo.AuraEntered`.
// - Module action `auraHello`: logs its parameter text once, for
//   `Action1=Wasm:auraHello(Subject,welcome)`.
//
// Each aura refreshes every `Interval` frames, staggered: a refresh walks
// the emitters of its types a few per frame, finds the allies or enemies
// in range and gives each a modifier lasting a little longer than the
// interval. The modifier group is the aura's, so a second emitter of the
// same aura replaces the modifier instead of stacking it.
//
// `onScenarioStart` places a few emitters and units of the human player
// and its enemy beside the player's first object, so the demo shows
// without any input. The first time each step happens it logs a `DEMO-OK`
// line (entry, key, apply, event, action).
import {
  Action, Category, Cell, EventRecord, EventScope, Find, House, Ini, IniEntryRecord, IniKeyRecord,
  IniSection, Modifier, ModifierOp, ModuleEvent, NO_HANDLE, NO_ID, Op, Prop, Relation, Rules,
  Status, Techno, TypeRef,
  Every, FIXED_ONE, fail, frame, get, lastStatus, log, namedAction, onFrame, onScenarioStart, propNamed,
} from "@vera20k/sdk";
// The auras that have covered an object: bit `i` for aura `i` (`vera.json`).
import { AuraSeen } from "./generated/vars";

// ------------------------------------------------------------------ State

const MAX_AURAS: i32 = 16;
const MAX_EMITTER_TYPES: i32 = 64;
const NAME_BYTES: i32 = 32;

/** Emitters handled per frame, over all auras. */
const EMITTERS_PER_FRAME: i32 = 8;
/** `find` pages of emitters per frame, empty ones included. */
const EMITTER_PAGES_PER_FRAME: i32 = 16;
/** Units one emitter reaches per refresh: pages of `TARGET_PAGE`. */
const TARGET_PAGES: i32 = 2;
const TARGET_PAGE: i32 = 32;
/** A modifier lasts its aura's interval plus this, so a refresh spread over a few frames never leaves a gap. */
const SLACK: i32 = 30;

const MIN_INTERVAL: i64 = 15;
const MAX_INTERVAL: i64 = 900;
const MAX_RADIUS: i64 = 20;
const LEPTONS_PER_CELL: i64 = 256;

/** The `DEMO-OK` marks, one bit each. */
const MARK_ENTRY = 1;
const MARK_KEY = 2;
const MARK_APPLY = 4;
const MARK_EVENT = 8;
const MARK_ACTION = 16;

class Aura {
  name: string = "";
  prop: Prop = 0;
  op: ModifierOp = ModifierOp.Multiply;
  value: i64 = 0;
  /** Leptons. */
  radius: i32 = 0;
  enemies: bool = false;
  interval: i32 = 0;
  /** The frame the next refresh starts. */
  nextDue: i32 = 0;
  /** A refresh is walking the emitters. */
  active: bool = false;
  /** The emitter type it is at, and the `find` position within it. */
  typeCursor: i32 = 0;
  after: i64 = 0;
}

const auras: Aura[] = [];
/** Emitting types and, bit `i`, whether they emit aura `i`. */
const emitterTypes: TypeRef[] = [];
const emitterMasks: u32[] = [];
let marks: i32 = 0;
/** `Demo.AuraEntered`, set in initialization (`NO_ID` when refused). */
let auraEntered: ModuleEvent = NO_ID;

/** `find` buffers: emitter pages of 1 to `EMITTERS_PER_FRAME`, by length, and one target page. */
const EMITTER_BUFS = new StaticArray<StaticArray<i64>>(EMITTERS_PER_FRAME + 1);
for (let i = 0; i <= EMITTERS_PER_FRAME; i++) EMITTER_BUFS[i] = new StaticArray<i64>(i);
const TARGETS = new StaticArray<i64>(TARGET_PAGE);
const HOUSES = new StaticArray<i64>(8);

/** The properties whose owners read modifiers (status document, "Wired"). */
const WIRED: StaticArray<Prop> = [
  Prop.FirepowerMultiplier, Prop.SpeedMultiplier, Prop.ArmorMultiplier, Prop.ROFMultiplier,
];

/** What `stage` places: type, the enemy's (else the player's), cells to the side of the player's first object. */
const STAGED_TYPES: StaticArray<string> = ["MTNK", "E1", "E1", "HTNK", "E2", "E2"];
const STAGED_ENEMY: StaticArray<bool> = [false, false, false, true, true, true];
const STAGED_DX: StaticArray<i32> = [3, 3, 3, 6, 6, 6];

/** Cells tried around a cluster's center, nearest first (x, y pairs). */
const OFFSETS: StaticArray<i32> = [
  0, 0, 1, 0, 0, 1, -1, 0, 0, -1, 1, 1, -1, 1, 1, -1, -1, -1, 2, 0, 0, 2,
  -2, 0, 0, -2, 2, 1, 1, 2, -2, -1, -1, -2, 2, 2, -2, 2, 2, -2, -2, -2,
];

/** Whether `mark` is new; records it. */
function first(mark: i32): bool {
  const fresh = (marks & mark) == 0;
  marks |= mark;
  return fresh;
}

// ------------------------------------------------------- INI: aura types

/** One `[Demo.AuraTypes]` entry: reads and checks its section. */
function loadAura(record: IniEntryRecord): void {
  const section = record.entryName;
  if (String.UTF8.byteLength(section) > NAME_BYTES) stop(bad(section, "", "the section name is over 32 characters"));
  if (auras.length == MAX_AURAS) stop(bad(section, "", "more than 16 aura types"));

  let text = Rules.get(section, "Radius");
  let found = lastStatus() == Status.Ok;
  const radius = Ini.int(text, 0);
  if (!found || lastStatus() != Status.Ok || radius < 1 || radius > MAX_RADIUS) {
    stop(bad(section, "Radius", "needs a whole number of cells, 1 to 20"));
  }

  text = Rules.get(section, "Property");
  if (lastStatus() != Status.Ok) stop(bad(section, "Property", "is missing"));
  const prop = propNamed(trim(text));
  if (prop == NO_ID || !WIRED.includes(prop)) {
    stop(bad(section, "Property", "needs FirepowerMultiplier, SpeedMultiplier, ArmorMultiplier or ROFMultiplier"));
  }

  let op = ModifierOp.Multiply;
  text = Rules.get(section, "Op");
  if (lastStatus() == Status.Ok) {
    text = trim(text);
    if (!text.length || sameAscii(text, "multiply")) op = ModifierOp.Multiply;
    else if (sameAscii(text, "add")) op = ModifierOp.Add;
    else if (sameAscii(text, "override")) op = ModifierOp.Override;
    else stop(bad(section, "Op", "needs add, multiply or override"));
  }

  text = Rules.get(section, "Value");
  found = lastStatus() == Status.Ok;
  const value = parseFixed(text);
  if (!found || !parsed) stop(bad(section, "Value", "needs a decimal number such as 1.25"));

  let enemies = false;
  text = Rules.get(section, "Affects");
  if (lastStatus() == Status.Ok) {
    text = trim(text);
    if (!text.length || sameAscii(text, "allies")) enemies = false;
    else if (sameAscii(text, "enemies")) enemies = true;
    else stop(bad(section, "Affects", "needs allies or enemies"));
  }

  let interval: i64 = 30;
  text = Rules.get(section, "Interval");
  if (lastStatus() == Status.Ok) {
    interval = Ini.int(text, 0);
    if (lastStatus() != Status.Ok || interval < MIN_INTERVAL || interval > MAX_INTERVAL) {
      stop(bad(section, "Interval", "needs 15 to 900 frames"));
    }
  }

  const aura = new Aura();
  aura.name = section;
  aura.prop = prop;
  aura.op = op;
  aura.value = value;
  aura.radius = <i32>(radius * LEPTONS_PER_CELL);
  aura.enemies = enemies;
  aura.interval = <i32>interval;
  auras.push(aura);

  if (first(MARK_ENTRY)) {
    log("DEMO-OK entry " + section + " radius " + radius.toString() + " interval " + interval.toString());
  }
}

/** Whether the latest `parseFixed` read a number. */
let parsed: bool = false;

/** A decimal such as `1.25` or `-0.5` as a `fixed` value (16 fraction bits), rounded; integer arithmetic only. Sets `parsed`. */
function parseFixed(input: string): i64 {
  parsed = false;
  const text = trim(input);
  if (!text.length) return 0;
  let at = 0;
  let negative = false;
  const c = text.charCodeAt(0);
  if (c == 45 /* - */ || c == 43 /* + */) {
    negative = c == 45;
    at = 1;
  }
  const rest = text.substring(at);
  const dot = rest.indexOf(".");
  const whole = dot < 0 ? rest : rest.substring(0, dot);
  const fraction = dot < 0 ? "" : rest.substring(dot + 1);
  if (!whole.length && !fraction.length) return 0;
  let int: i64 = 0;
  for (let i = 0; i < whole.length; i++) {
    const digit = whole.charCodeAt(i) - 48;
    if (digit < 0 || digit > 9) return 0;
    int = int * 10 + <i64>digit;
    if (int > 32767) return 0;
  }
  let num: i64 = 0;
  let den: i64 = 1;
  for (let i = 0; i < fraction.length; i++) {
    const digit = fraction.charCodeAt(i) - 48;
    if (digit < 0 || digit > 9) return 0;
    // Six digits are finer than 1/65536; the rest are checked only.
    if (den < 1000000) {
      num = num * 10 + <i64>digit;
      den *= 10;
    }
  }
  const value = int * FIXED_ONE + (num * FIXED_ONE + den / 2) / den;
  parsed = true;
  return negative ? -value : value;
}

// ------------------------------------------------------ INI: Demo.Aura=

/** One TechnoType with `Demo.Aura=`: the auras it emits. */
function loadEmitter(record: IniKeyRecord): void {
  const value = record.value;
  let mask: u32 = 0;
  const parts = Ini.split(value);
  for (let p = 0; p < parts.length; p++) {
    const part = parts[p];
    if (!part.length) continue;
    let index = -1;
    for (let i = 0; i < auras.length; i++) {
      if (sameAscii(auras[i].name, part)) {
        index = i;
        break;
      }
    }
    if (index < 0) {
      stop("ini-aura: [" + sectionOf(record) + "] Demo.Aura: " + part + " is not listed in [Demo.AuraTypes]");
    }
    mask |= (<u32>1) << index;
  }
  if (mask == 0) return;
  if (emitterTypes.length == MAX_EMITTER_TYPES) stop(bad(sectionOf(record), "Demo.Aura", "more than 64 types emit auras"));
  emitterTypes.push(record.typeRef);
  emitterMasks.push(mask);

  if (first(MARK_KEY)) log("DEMO-OK key " + sectionOf(record) + " emits " + value);
}

function sectionOf(record: IniKeyRecord): string {
  const name = record.sectionName;
  return name.length ? name : "?";
}

// ------------------------------------------------------------------ Start

function start(): void {
  const now = frame();
  // Staggered, so the auras do not all refresh on one frame.
  for (let i = 0; i < auras.length; i++) auras[i].nextDue = now + 1 + 7 * i;
  log("ini-aura: " + auras.length.toString() + " aura types, " + emitterTypes.length.toString() + " emitting types");
  stage();
}

/** Places a few emitters and units of the human player and its enemy close enough for both auras to reach across. */
function stage(): void {
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human == NO_HANDLE) {
    log("ini-aura: no human player, nothing staged");
    return;
  }
  const enemy = enemyOf(human);
  const anchor = Find.objects().ownedBy(human).first();
  const at = get(anchor, Prop.Cell);
  if (lastStatus() != Status.Ok) {
    log("ini-aura: the player has no object, nothing staged");
    return;
  }
  const x = Cell.x(at);
  const y = Cell.y(at);
  let placed = 0;
  for (let i = 0; i < STAGED_TYPES.length; i++) {
    const house = STAGED_ENEMY[i] ? enemy : human;
    if (house == NO_HANDLE) continue;
    const t = Rules.technoType(STAGED_TYPES[i]);
    if (t == NO_HANDLE) continue;
    const dx = STAGED_DX[i];
    // To the right of the anchor, else to its left.
    if (place(t, house, x + dx, y + 1) || place(t, house, x - dx, y + 1)) placed++;
  }
  log("ini-aura: staged " + placed.toString() + " units beside the player");
}

/** The first undefeated enemy house of `human` that owns an object. */
function enemyOf(human: House): House {
  const find = Find.houses()
    .seenBy(human)
    .where(Prop.Relation, Op.Eq, <i64>Relation.Enemy)
    .where(Prop.IsDefeated, Op.Eq, 0);
  let after: i64 = 0;
  for (let n = 0; n < 8; n++) {
    const page = find.page(HOUSES, after);
    if (lastStatus() != Status.Ok) break;
    const count = page.count;
    const next = page.next;
    for (let i = 0; i < count; i++) {
      const house: House = HOUSES[i];
      if (Find.objects().ownedBy(house).first() != NO_HANDLE) return house;
    }
    if (next == 0) break;
    after = next;
  }
  return NO_HANDLE;
}

/** Creates `t` for `house` on the first free cell around (`cx`, `cy`). */
function place(t: TypeRef, house: House, cx: i32, cy: i32): bool {
  for (let i = 0; i < OFFSETS.length; i += 2) {
    const x = cx + OFFSETS[i];
    const y = cy + OFFSETS[i + 1];
    if (x < 0 || x > 65535 || y < 0 || y > 65535) continue;
    if (Action.createObject(Cell.at(x, y), t, house) != NO_HANDLE) return true;
  }
  return false;
}

// ------------------------------------------------------------------ Frame

function tick(): void {
  const now = frame();
  const count = auras.length;
  if (count == 0 || emitterTypes.length == 0) return;
  let emittersLeft = EMITTERS_PER_FRAME;
  let pagesLeft = EMITTER_PAGES_PER_FRAME;
  const begin = now % count;
  for (let k = 0; k < count; k++) {
    const i = (begin + k) % count;
    const aura = auras[i];
    if (!aura.active && now >= aura.nextDue) {
      aura.active = true;
      aura.typeCursor = 0;
      aura.after = 0;
      aura.nextDue = now + aura.interval;
    }
    while (aura.active && emittersLeft > 0 && pagesLeft > 0) {
      pagesLeft--;
      emittersLeft -= refreshPage(i, emittersLeft);
    }
  }
}

/** One page of aura `i`'s emitters, at most `room`; returns how many. */
function refreshPage(i: i32, room: i32): i32 {
  const aura = auras[i];
  const bit: u32 = (<u32>1) << i;
  let cursor = aura.typeCursor;
  while (cursor < emitterTypes.length && (emitterMasks[cursor] & bit) == 0) cursor++;
  if (cursor >= emitterTypes.length) {
    aura.active = false;
    return 0;
  }
  const buf = EMITTER_BUFS[min(room, EMITTERS_PER_FRAME)];
  const page = Find.objects().ofType(emitterTypes[cursor]).page(buf, aura.after);
  let count = 0;
  let next: i64 = 0;
  if (lastStatus() == Status.Ok) {
    count = page.count;
    next = page.next;
  }
  if (next == 0) {
    aura.typeCursor = cursor + 1;
    aura.after = 0;
  } else {
    aura.typeCursor = cursor;
    aura.after = next;
  }
  for (let e = 0; e < count; e++) cover(i, aura, buf[e]);
  return count;
}

/** Gives every unit aura `i` of `emitter` reaches its modifier. */
function cover(i: i32, aura: Aura, emitter: Techno): void {
  const owner = Techno.owner(emitter);
  if (lastStatus() != Status.Ok) return;
  const at = get(emitter, Prop.Cell);
  if (lastStatus() != Status.Ok) return;
  // Self (0) and allies (1), or enemies (2); never neutral houses.
  const op = aura.enemies ? Op.Eq : Op.Le;
  const relation = aura.enemies ? Relation.Enemy : Relation.Ally;
  const find = Find.objects()
    .near(at, aura.radius)
    .seenBy(owner)
    .where(Prop.Category, Op.Le, <i64>Category.Vehicle)
    .where(Prop.Relation, op, <i64>relation);
  let after: i64 = 0;
  for (let n = 0; n < TARGET_PAGES; n++) {
    const page = find.page(TARGETS, after);
    if (lastStatus() != Status.Ok) return;
    // The page is shared and `touch` makes calls: keep its values first.
    const count = page.count;
    const next = page.next;
    for (let u = 0; u < count; u++) touch(i, aura, emitter, TARGETS[u]);
    if (next == 0) return;
    after = next;
  }
}

/** Applies aura `i` to `unit`; on the first cover, raises `Demo.AuraEntered` on it. */
function touch(i: i32, aura: Aura, emitter: Techno, unit: Techno): void {
  // The group is the aura's: re-applying replaces, it never stacks.
  Modifier.create(unit, aura.prop, aura.op, aura.value, aura.interval + SLACK, emitter, i + 1);
  if (lastStatus() == Status.Ok && first(MARK_APPLY)) log("DEMO-OK apply " + aura.name);
  // Registration failed in initialization (logged there): an unknown event
  // ID would stop the module.
  if (auraEntered == NO_ID) return;
  const bit: i64 = (<i64>1) << i;
  const seen = AuraSeen.get(unit);
  if (lastStatus() != Status.Ok) return;
  if ((seen & bit) != 0 || AuraSeen.set(unit, seen | bit) != Status.Ok) return;
  const raised = Action.raiseEvent(auraEntered, unit, emitter, <i64>i, 0);
  if (raised == Status.Ok && first(MARK_EVENT)) log("DEMO-OK event Demo.AuraEntered for " + aura.name);
}

// ----------------------------------------------------------------- Action

/** `Wasm:auraHello(target,text)`: logs the parameter text the first time. */
function auraHello(record: EventRecord): void {
  const words = record.argsText;
  if (words.length && first(MARK_ACTION)) log("DEMO-OK action auraHello(" + words + ")");
}

// ---------------------------------------------------------------- Helpers

/** A message naming `[section] key` for a bad value. */
function bad(section: string, key: string, why: string): string {
  return "ini-aura: [" + section + "] " + (key.length ? key + " " : "") + why;
}

/** Logs `line` and stops the module: a wrong INI value stops it, as RFC 0001 section 6h asks. */
function stop(line: string): void {
  fail(line);
}

function isSpace(c: i32): bool {
  return c == 32 || c == 9 || c == 10 || c == 12 || c == 13;
}

/** `s` without surrounding ASCII spaces (Rust's `trim_ascii`). */
function trim(s: string): string {
  let begin = 0;
  let end = s.length;
  while (begin < end && isSpace(s.charCodeAt(begin))) begin++;
  while (end > begin && isSpace(s.charCodeAt(end - 1))) end--;
  return s.substring(begin, end);
}

/** Whether `a` and `b` are equal ignoring ASCII case. */
function sameAscii(a: string, b: string): bool {
  if (a.length != b.length) return false;
  for (let i = 0; i < a.length; i++) {
    let x = a.charCodeAt(i);
    let y = b.charCodeAt(i);
    if (x >= 65 && x <= 90) x += 32;
    if (y >= 65 && y <= 90) y += 32;
    if (x != y) return false;
  }
  return true;
}

// ------------------------------------------------------------ Registration
// Top-level code is the module's initialization, where INI names register.

if (Ini.registry("Demo.AuraTypes", loadAura) != Status.Ok) log("ini-aura: Demo.AuraTypes is already taken");
if (Ini.key("Demo.Aura", IniSection.TechnoType, loadEmitter) != Status.Ok) log("ini-aura: Demo.Aura is already taken");
auraEntered = Ini.event("Demo.AuraEntered", EventScope.Object);
if (auraEntered == NO_ID) log("ini-aura: Demo.AuraEntered is already taken");

// The INI trigger `[AuraHello]` calls it (`Action2=Wasm:auraHello(Subject,welcome)`).
namedAction("auraHello", auraHello);
onScenarioStart(start);
onFrame(new Every(1), tick);
