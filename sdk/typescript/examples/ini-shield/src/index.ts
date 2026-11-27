// "INI Shield", a demo `rules.wasm` for INI names a module registers
// (RFC 0001 section 6h), the same as the Rust SDK's `sdk/examples/ini-shield`.
// The modder configures everything in the mod's `rulesmd.ini`; the module
// only implements it:
//
// - `[Demo.ShieldTypes]`, a registry: each entry is a section with
//   `Strength=` (shield points), `Regen=` (points back every 15 frames)
//   and `Delay=` (frames after a hit before regeneration starts).
// - `Demo.Shield=<shield type>` on a TechnoType gives that type a shield.
//   The module binds a `TakeDamage` decision trigger to the type; the
//   shield, kept in module memory per object and filled on the first hit,
//   takes damage before the unit does.
// - `Event=Wasm:Demo.ShieldBroken`, an object event raised when a shield
//   drops to 0. A decision handler may not write, so the break is noted in
//   memory and raised in the next `onFrame`.
// - `Wasm:restoreShield(target,amount)`, a module action: gives back
//   `amount` shield points to its target.
//
// To exercise itself without a player, it stages a small fight of
// shielded tanks and infantry between the human player and the first
// computer opponent near the player's base. It logs `DEMO-OK <feature>`
// the first time each feature works (the engine test
// `sim::ext_modules::tests::ini_demo_mods_reach_their_marks` checks them).
import {
  Action, Cell, Event, EventRecord, EventScope, Find, House, Ini, IniEntryRecord, IniKeyRecord,
  IniSection, Mission, ModuleEvent, NO_HANDLE, NO_ID, Op, Prop, Rules, SpeedType, Status,
  TakeDamageDecision, Target, Techno, Trigger, TriggerRef, TypeRef,
  Every, fail, frame, get, lastStatus, log, namedAction, onFrame, onScenarioStart, text,
} from "@vera20k/sdk";

// ------------------------------------------------------------------ Memory
// Module memory is saved and hashed with the match.

/** Shield types from `[Demo.ShieldTypes]`, in list order. */
const MAX_KINDS: i32 = 16;
let kindCount: i32 = 0;
/** The entry section's name as a string ID. */
const kindName = new StaticArray<i32>(MAX_KINDS);
const kindStrength = new StaticArray<i32>(MAX_KINDS);
const kindRegen = new StaticArray<i32>(MAX_KINDS);
const kindDelay = new StaticArray<i32>(MAX_KINDS);

/** TechnoTypes with `Demo.Shield=`: type ID and shield type index. */
const MAX_TYPES: i32 = 64;
let typeCount: i32 = 0;
const typeId = new StaticArray<i32>(MAX_TYPES);
const typeKind = new StaticArray<i32>(MAX_TYPES);

/** The one `TakeDamage` decision trigger, bound to every shielded type. */
let decideTrigger: TriggerRef = NO_HANDLE;
/** `Demo.ShieldBroken`, set in initialization (`NO_ID` when refused). */
let shieldBroken: ModuleEvent = NO_ID;

/** Live shields, one slot per object that has been hit (`NO_HANDLE` is a free slot). */
const MAX_SHIELDS: i32 = 128;
const shieldObject = new StaticArray<i64>(MAX_SHIELDS);
const shieldKind = new StaticArray<i32>(MAX_SHIELDS);
const shieldPoints = new StaticArray<i32>(MAX_SHIELDS);
const shieldHit = new StaticArray<i32>(MAX_SHIELDS);

/** Shields broken in a decision, raised as `Demo.ShieldBroken` in the next `onFrame`. */
const MAX_PENDING: i32 = 32;
let pendingCount: i32 = 0;
const pending = new StaticArray<i64>(MAX_PENDING);

/** Set once a decision absorbed damage (logged from `onFrame`). */
let absorbed: bool = false;

/** The staged fight: done, and attempts so far. */
let staged: bool = false;
let stageTries: i32 = 0;
const MAX_STAGE_TRIES: i32 = 20;

/** Construction yards and MCVs of the three sides: where a house's base is. */
const BASES: StaticArray<string> = ["GACNST", "NACNST", "YACNST", "AMCV", "SMCV", "PCV"];

/** The fight, interleaved so enemies stand next to each other; `true` for the human player's units. */
const ROSTER_LEN: i32 = 10;
const ROSTER_TYPES: StaticArray<string> = ["MTNK", "HTNK", "MTNK", "HTNK", "MTNK", "HTNK", "E1", "E2", "E1", "E2"];
const ROSTER_OURS: StaticArray<bool> = [true, false, true, false, true, false, true, false, true, false];
const ours = new StaticArray<Techno>(ROSTER_LEN);
const theirs = new StaticArray<Techno>(ROSTER_LEN);
const HOUSES = new StaticArray<i64>(16);

/** `DEMO-OK` lines already logged, one bit each. */
let marks: i32 = 0;
const MARK_ENTRY = 1;
const MARK_KEY = 2;
const MARK_ABSORB = 4;
const MARK_BROKEN = 8;
const MARK_RESTORE = 16;

/** Logs `line` the first time `bit` is marked. */
function mark(bit: i32, line: string): void {
  if ((marks & bit) == 0) {
    marks |= bit;
    log(line);
  }
}

// --------------------------------------------------- INI records at start

/** `IniEntry`: one shield type of `[Demo.ShieldTypes]`. */
function loadShieldType(r: IniEntryRecord): void {
  const section = r.entryName;
  const count = kindCount;
  if (count == MAX_KINDS) bad(section, "", "is past the 16 shield types this module holds");
  const strength = readInt(section, "Strength", false, 0, 1, 1000000);
  const regen = readInt(section, "Regen", true, 0, 0, 1000000);
  const delay = readInt(section, "Delay", true, 0, 0, 1000000);
  kindName[count] = r.record.args;
  kindStrength[count] = <i32>strength;
  kindRegen[count] = <i32>regen;
  kindDelay[count] = <i32>delay;
  kindCount = count + 1;
  log("ini-shield: shield type " + section + " strength " + strength.toString() + ", regen " +
    regen.toString() + ", delay " + delay.toString());
  mark(MARK_ENTRY, "DEMO-OK entry");
}

/** `IniKey`: a TechnoType with `Demo.Shield=`. */
function attachShield(r: IniKeyRecord): void {
  let section = r.sectionName;
  if (!section.length) section = "?";
  const kind = kindNamed(r.record.args);
  if (kind < 0) bad(section, "Demo.Shield", "names no section listed in [Demo.ShieldTypes]");
  const count = typeCount;
  if (count == MAX_TYPES) bad(section, "Demo.Shield", "is past the 64 shielded types this module holds");
  const t = r.typeRef;
  if (decideTrigger == NO_HANDLE) {
    decideTrigger = Trigger.on(Event.TakeDamage).decideSync(absorb);
    if (decideTrigger == NO_HANDLE) fail("ini-shield: the TakeDamage decision trigger could not be made");
  }
  if (TriggerRef.bind(decideTrigger, t) != Status.Ok) {
    bad(section, "Demo.Shield", "could not bind the shield to this type");
  }
  typeId[count] = TypeRef.id(t);
  typeKind[count] = kind;
  typeCount = count + 1;
  mark(MARK_KEY, "DEMO-OK key");
}

/** The shield type whose entry name is string `id`, ignoring case; −1 for none. */
function kindNamed(id: i32): i32 {
  for (let i = 0; i < kindCount; i++) if (kindName[i] == id) return i;
  const want = trim(text(id));
  if (lastStatus() != Status.Ok) return -1;
  for (let i = 0; i < kindCount; i++) {
    const have = text(kindName[i]);
    if (lastStatus() == Status.Ok && sameAscii(have, want)) return i;
  }
  return -1;
}

/**
 * `[section] key` as a whole number in `min..=max`; `otherwise` when the
 * key is missing and `optional` (else it is required). Stops the module on
 * a bad value.
 */
function readInt(section: string, key: string, optional: bool, otherwise: i64, min: i64, max: i64): i64 {
  const value = Rules.get(section, key);
  const status = lastStatus();
  let n: i64 = 0;
  let good = false;
  if (status == Status.Ok) {
    n = Ini.int(value, 0);
    good = lastStatus() == Status.Ok;
  } else if (status == Status.NotFound && optional) {
    n = otherwise;
    good = true;
  }
  if (!good || n < min || n > max) {
    fail("ini-shield: [" + section + "] " + key + "= must be a whole number from " + min.toString() +
      " to " + max.toString());
  }
  return n;
}

/** Logs what is wrong with `[section] key=` and stops the module (a trap stops only this module, RFC section 3). */
function bad(section: string, key: string, what: string): void {
  fail("ini-shield: [" + section + "] " + (key.length ? key + "= " : "") + what);
}

// ------------------------------------------------------------------ Start

function start(): void {
  log("ini-shield: " + kindCount.toString() + " shield types on " + typeCount.toString() + " unit types");
}

// --------------------------------------------------------------- Shields

/** The shield type of TechnoType `id`, or −1 when it has none. */
function kindOfType(id: i32): i32 {
  for (let i = 0; i < typeCount; i++) if (typeId[i] == id) return typeKind[i];
  return -1;
}

/** `object`'s shield slot, made full when it has none; −1 when every slot is taken or its type has no shield. */
function shieldOf(object: Techno): i32 {
  for (let i = 0; i < MAX_SHIELDS; i++) if (shieldObject[i] == object) return i;
  const t = Techno.typeRef(object);
  if (lastStatus() != Status.Ok) return -1;
  const kind = kindOfType(TypeRef.id(t));
  if (kind < 0) return -1;
  for (let slot = 0; slot < MAX_SHIELDS; slot++) {
    if (shieldObject[slot] != NO_HANDLE) continue;
    shieldObject[slot] = object;
    shieldKind[slot] = kind;
    shieldPoints[slot] = kindStrength[kind];
    shieldHit[slot] = frame();
    return slot;
  }
  return -1;
}

/** The `TakeDamage` decision: the shield takes the final damage first. Reads only, and writes module memory. */
function absorb(d: TakeDamageDecision): void {
  const damage = d.previous;
  if (damage <= 0) return;
  const victim = d.victim;
  const slot = shieldOf(victim);
  if (slot < 0) return;
  shieldHit[slot] = frame();
  const points = <i64>shieldPoints[slot];
  if (points <= 0) return;
  const taken = min(points, damage);
  const left = points - taken;
  shieldPoints[slot] = <i32>left;
  d.setResult(damage - taken);
  absorbed = true;
  if (left == 0 && pendingCount < MAX_PENDING) {
    pending[pendingCount] = victim;
    pendingCount++;
  }
}

/** Raises `Demo.ShieldBroken` for the shields the decisions broke. */
function raiseBroken(): void {
  const count = pendingCount;
  pendingCount = 0;
  for (let i = 0; i < count; i++) {
    // `Gone` when the hit that broke the shield also killed the unit.
    if (Action.raiseEvent(shieldBroken, pending[i], NO_HANDLE, 0, 0) == Status.Ok) {
      mark(MARK_BROKEN, "DEMO-OK broken");
    }
  }
}

/** Every 15 frames: shields not hit for their `Delay=` regain `Regen=`; slots of dead objects are freed. */
function regenerate(now: i32): void {
  for (let slot = 0; slot < MAX_SHIELDS; slot++) {
    const object = shieldObject[slot];
    if (object == NO_HANDLE) continue;
    Techno.health(object);
    if (lastStatus() != Status.Ok) {
      shieldObject[slot] = NO_HANDLE;
      continue;
    }
    const kind = shieldKind[slot];
    const strength = kindStrength[kind];
    const points = shieldPoints[slot];
    if (points < strength && now - shieldHit[slot] >= kindDelay[kind]) {
      shieldPoints[slot] = min(points + kindRegen[kind], strength);
    }
  }
}

/** Module action `restoreShield(target,amount)`. */
function restoreShield(r: EventRecord): void {
  const amount = Ini.int(r.argsText, 0);
  if (lastStatus() != Status.Ok) {
    log("ini-shield: restoreShield wants a whole number, as in restoreShield(Subject,40)");
    return;
  }
  mark(MARK_RESTORE, "DEMO-OK restore");
  const target = r.target;
  let object: Techno = NO_HANDLE;
  if (target == Target.Subject) object = r.subject;
  else if (target == Target.Other) object = r.other;
  else if (target == Target.TagHolder) object = r.holder;
  else return;
  if (amount <= 0) return;
  Techno.health(object);
  if (lastStatus() != Status.Ok) return;
  const slot = shieldOf(object);
  if (slot < 0) return;
  const strength = <i64>kindStrength[shieldKind[slot]];
  const points = <i64>shieldPoints[slot];
  shieldPoints[slot] = <i32>min(points + amount, strength);
}

// ------------------------------------------------------- The staged fight

/** The cell of `house`'s construction yard or MCV, or `NO_HANDLE`. */
function baseOf(house: House): Cell {
  for (let i = 0; i < BASES.length; i++) {
    const t = Rules.technoType(BASES[i]);
    if (t == NO_HANDLE) continue;
    const object = Find.objects().ownedBy(house).ofType(t).first();
    if (object != NO_HANDLE) {
      const cell = get(object, Prop.Cell);
      if (lastStatus() == Status.Ok) return cell;
    }
  }
  return NO_HANDLE;
}

/** The first computer house with a base, or `NO_HANDLE`. */
function enemySide(): House {
  const page = Find.houses().where(Prop.IsHuman, Op.Eq, 0).page(HOUSES, 0);
  if (lastStatus() != Status.Ok) return NO_HANDLE;
  const count = page.count;
  for (let i = 0; i < count; i++) {
    if (baseOf(HOUSES[i]) != NO_HANDLE) return HOUSES[i];
  }
  return NO_HANDLE;
}

/** Creates the roster a quarter of the way from the human player's base towards the computer's, on passable cells in rings around that point, and sets each unit on an enemy. */
function stage(): void {
  const tries = stageTries + 1;
  if (tries > MAX_STAGE_TRIES) return;
  stageTries = tries;
  const placed = placeRoster();
  if (placed == 0 && tries == MAX_STAGE_TRIES) log("ini-shield: found no place to stage the fight");
}

/** Places the roster; how many units it made once both sides have some. */
function placeRoster(): i32 {
  const human: House = Find.houses().where(Prop.IsHuman, Op.Eq, 1).first();
  if (human == NO_HANDLE) return 0;
  const enemy = enemySide();
  if (enemy == NO_HANDLE) return 0;
  const home = baseOf(human);
  if (home == NO_HANDLE) return 0;
  let away = baseOf(enemy);
  if (away == NO_HANDLE) away = home;
  const hx = Cell.x(home);
  const hy = Cell.y(home);
  const ax = hx + (Cell.x(away) - hx) / 4;
  const ay = hy + (Cell.y(away) - hy) / 4;
  let nOurs = 0;
  let nTheirs = 0;
  let next = 0;
  for (let ring = 0; ring <= 7 && next < ROSTER_LEN; ring++) {
    for (let dy = -ring; dy <= ring && next < ROSTER_LEN; dy++) {
      for (let dx = -ring; dx <= ring && next < ROSTER_LEN; dx++) {
        if (max(abs(dx), abs(dy)) != ring) continue;
        const x = ax + dx;
        const y = ay + dy;
        if (x < 1 || x > 500 || y < 1 || y > 500) continue;
        const cell = Cell.at(x, y);
        if (!Cell.passable(cell, SpeedType.Track) || lastStatus() != Status.Ok) continue;
        const kind = Rules.technoType(ROSTER_TYPES[next]);
        if (kind == NO_HANDLE) {
          next++;
          continue;
        }
        const allied = ROSTER_OURS[next];
        // An occupied cell takes no unit; the next one is tried.
        const unit = Action.createObject(cell, kind, allied ? human : enemy);
        if (unit != NO_HANDLE) {
          if (allied) ours[nOurs++] = unit;
          else theirs[nTheirs++] = unit;
          next++;
        }
      }
    }
  }
  if (nOurs == 0 || nTheirs == 0) return 0;
  staged = true;
  for (let i = 0; i < nOurs; i++) Action.setMissionOn(ours[i], Mission.Attack, theirs[i % nTheirs]);
  for (let i = 0; i < nTheirs; i++) Action.setMissionOn(theirs[i], Mission.Attack, ours[i % nOurs]);
  log("ini-shield: staged a fight of " + (nOurs + nTheirs).toString() + " shielded units near (" +
    ax.toString() + "," + ay.toString() + ")");
  return nOurs + nTheirs;
}

// ------------------------------------------------------------------ Frame

function tick(): void {
  if (absorbed) mark(MARK_ABSORB, "DEMO-OK absorb");
  raiseBroken();
  const now = frame();
  if (now % 15 == 0) {
    regenerate(now);
    if (!staged && now >= 30) stage();
  }
}

// ------------------------------------------------------------------- Text

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

if (Ini.registry("Demo.ShieldTypes", loadShieldType) != Status.Ok) log("ini-shield: the name Demo.ShieldTypes is taken");
if (Ini.key("Demo.Shield", IniSection.TechnoType, attachShield) != Status.Ok) log("ini-shield: the name Demo.Shield is taken");
shieldBroken = Ini.event("Demo.ShieldBroken", EventScope.Object);
if (shieldBroken == NO_ID) log("ini-shield: the name Demo.ShieldBroken is taken");

// The INI trigger `[DemoShieldRecharge]` calls it (`Action1=Wasm:restoreShield(Subject,40)`).
namedAction("restoreShield", restoreShield);
onScenarioStart(start);
onFrame(new Every(1), tick);
