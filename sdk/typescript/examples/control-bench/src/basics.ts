// Area `basics` of the bench (see `index.ts`), the port of the Rust
// `basics.rs`. Marks are named `basics.<point>`.
//
// Host calls, property reads (objects, houses, types, cells, the global
// reference, viewpoint reads), writes through `set`, modifiers of every
// wired stat, declared variables (`vera.json`), `find` in every scope, and
// INI registration (RFC 0001 section 6h).
//
// Three units of its own in its arena: two player MTNKs (`A` takes the
// reads and the modifiers, `B` the writes) and one enemy HTNK (`E`, for
// `Relation`, `Engage` and `DisableWeapons`, removed again after the
// reads). A short-lived unit with ammo and a powered building take the
// `Ammo` and `Online` writes. One check group runs per frame over the
// first ~45 frames after the arena opens; the enemy's factories and its
// build area are polled later. From the 47th frame the properties past the
// first set: `NearbyEnemyCount`, `Cloaked`, `TargetInRange`,
// `CanFireAtTarget` and `DoorOpen`.
//
// Not marked: see the Rust file's list (TeamType and ModuleAction names,
// `ControlGroup`, `Team`/`Tag`, a house's `Name`, `Animating` reading 1).
// `CritMultiplier`, `CritExtraChance` and `Unkillable` pass either way: in
// a Phobos build when the modifier reads back, otherwise when `hasKind`
// says 0 and `modify` answers `Unsupported`.
//
// `Find` has no constructor for the `Modifiers` (7) and `SuperWeapons` (8)
// scopes and no variable clause: `rawFind` (bench.ts) builds those.
import {
  Action, ActionKind, Category, Cell, Event, EventScope, FIXED_ONE, Find, Global, Ini, IniEntryRecord,
  IniKeyRecord, IniSection, Mission, Modifier, ModifierOp, ModuleEventRecord, NO_HANDLE, NO_ID, NameKind,
  Op, Prop, Relation, Rules, Status, Techno, Trigger, TypeRef, Waypoint,
  frame as sdkFrame, get, getMany, hasKind, lastStatus, log, lookup, nameOf, random, set, text,
} from "@vera20k/sdk";
import * as sys from "@vera20k/sdk/assembly/sys";
import {
  BASICS, alive, arena, benchFail, check, enemy, fails, getOr, holds, is, made, mark, marked, offset, ok,
  player, propClause, rawFind, rawFindAll, spawn, varClause, Clause,
} from "./bench";
import { finaleStarted } from "./finale";
import { BenchScore, BenchTag, BenchWave } from "./generated/vars";

/** Every mark this area reaches in the bench scene. */
export const MARKS: StaticArray<string> = [
  "basics.frame",
  "basics.random",
  "basics.log",
  "basics.has-kind",
  "basics.name-techno",
  "basics.name-weapon",
  "basics.name-warhead",
  "basics.name-projectile",
  "basics.name-anim",
  "basics.name-voxelanim",
  "basics.name-particles",
  "basics.name-super",
  "basics.name-country",
  "basics.name-variable",
  "basics.name-property",
  "basics.name-action",
  "basics.name-string",
  "basics.name-event",
  "basics.object-props",
  "basics.object-more",
  "basics.get-many",
  "basics.house-props",
  "basics.type-props",
  "basics.weapon-props",
  "basics.warhead-props",
  "basics.cell-props",
  "basics.waypoint-cell",
  "basics.global-props",
  "basics.relation",
  "basics.visible",
  "basics.engage",
  "basics.revealed",
  "basics.placeable",
  "basics.factory",
  "basics.set-health",
  "basics.set-rank",
  "basics.set-owner",
  "basics.set-mission",
  "basics.set-ammo",
  "basics.set-online",
  "basics.mod-compose",
  "basics.mod-group",
  "basics.mod-unmodify",
  "basics.mod-props",
  "basics.mod-expire",
  "basics.find-modifiers",
  "basics.mod-firepower",
  "basics.mod-armor",
  "basics.mod-speed",
  "basics.mod-rof",
  "basics.mod-range",
  "basics.mod-range-extra",
  "basics.mod-crit",
  "basics.mod-unkillable",
  "basics.mod-cloakable",
  "basics.mod-decloak",
  "basics.mod-noweapons",
  "basics.mod-immunity",
  "basics.mod-rot",
  "basics.var-defaults",
  "basics.var-global",
  "basics.var-house",
  "basics.var-object",
  "basics.var-lookup",
  "basics.find-var",
  "basics.find-objects",
  "basics.find-objects-area",
  "basics.find-houses",
  "basics.find-cells-map",
  "basics.find-cells-circle",
  "basics.find-cells-rect",
  "basics.find-supers",
  "basics.ini-entry",
  "basics.ini-key",
  "basics.module-event",
  "basics.type-name",
  "basics.points",
  "basics.game-mode",
  "basics.buildable",
  "basics.find-types",
  "basics.animating",
  "basics.nearby-enemies",
  "basics.cloaked",
  "basics.target-in-range",
  "basics.can-fire",
  "basics.door-open",
];

const EVENT_NAME = "Bench.BasicsEvent";
/** The TechnoType section that holds `Bench.Key=` in `rulesmd.ini`. */
const KEY_SECTION = "BFRT";
const KEY_VALUE = "Bench,7";

const ONE: i64 = FIXED_ONE;
const HALF: i64 = FIXED_ONE / 2;
const TWO: i64 = FIXED_ONE * 2;
const THREE: i64 = FIXED_ONE * 3;
const QUARTER: i64 = FIXED_ONE / 4;

// ----------------------------------------------------------------- State

/** The module event `Bench.BasicsEvent`, `NO_ID` until registered. */
let benchEvent: i32 = NO_ID;
/** The frame the arena opened. */
let openAt: i64 = 0;
/** Player MTNK for reads and modifiers. */
let unitA: Techno = NO_HANDLE;
/** Player MTNK for writes. */
let unitB: Techno = NO_HANDLE;
/** Enemy HTNK. */
let unitE: Techno = NO_HANDLE;
/** The building for the `Online` write. */
let building: Techno = NO_HANDLE;
/** The superweapon type granted to the player. */
let superKind: i64 = NO_HANDLE;
/** The modifier left to expire. */
let expiring: i64 = NO_HANDLE;
/** `IniEntry` records seen right: bit 0 the first entry, bit 1 the second. */
let entries: i32 = 0;
/** Player MTNK that cloaks and uncloaks, and whether it read cloaked (and `ForceDecloak` was added). */
let cloaker: Techno = NO_HANDLE;
let cloakSeen: bool = false;
/** Player MTNK for the target reads and its enemy target. */
let shooter: Techno = NO_HANDLE;
let foe: Techno = NO_HANDLE;
/** The player's own war factory, and whether its door read shut. */
let warFactory: Techno = NO_HANDLE;
let doorShut: bool = false;

const OUT = new StaticArray<i64>(16);
const OUT8 = new StaticArray<i64>(8);
const OUT4 = new StaticArray<i64>(4);
const OUT1 = new StaticArray<i64>(1);
const OUT64 = new StaticArray<i64>(64);
const RAW_VALUE: usize = memory.data(8, 8);

function tankType(): TypeRef {
  return Rules.technoType("MTNK");
}

/** `lookup(kind, name) == Ok(id)`. */
function lookupIs(kind: NameKind, name: string, id: i32): bool {
  const found = lookup(kind, name);
  return ok() && found == id;
}

/** `name_of(kind, id) == Ok(name)`: the name, `null` when it does not answer. */
function nameOrNull(kind: NameKind, id: i32): string | null {
  const name = nameOf(kind, id);
  return ok() ? name : null;
}

/** Whether the latest call that wrote or created answered `status`. */
function answered(status: Status): bool {
  return lastStatus() == status;
}

// --------------------------------------------------------------- Exports

/** The initialization: the INI names (RFC 0001 section 6h). */
export function init(): void {
  if (Ini.registry("Bench.Registry", iniEntry) != Status.Ok) {
    benchFail("basics.ini-entry", "ini::registry failed");
  }
  if (Ini.key("Bench.Key", IniSection.TechnoType, iniKey) != Status.Ok) {
    benchFail("basics.ini-key", "ini::key failed");
  }
  const event = Ini.event(EVENT_NAME, EventScope.Object);
  if (event != NO_ID) benchEvent = event;
  else benchFail("basics.module-event", "ini::event failed");
}

export function start(): void {}

/** The frame the arena opens: the houses and the arena are known. */
export function open(): void {
  openAt = <i64>sdkFrame();
  const home = arena(BASICS);
  unitA = spawn(home, "MTNK", player());
  unitB = spawn(offset(home, 2, 0), "MTNK", player());
  // The enemy tank stands away from ours, its weapons off.
  const dxs: StaticArray<i32> = [-7, -7, 0, 4];
  const dys: StaticArray<i32> = [-7, 0, -7, -6];
  for (let i = 0; i < 4; i++) {
    const e = spawn(offset(home, dxs[i], dys[i]), "HTNK", enemy());
    if (e != NO_HANDLE) {
      unitE = e;
      break;
    }
  }
  if (unitE != NO_HANDLE) Modifier.create(unitE, Prop.DisableWeapons, ModifierOp.Override, 1, -1, 0, 99);
  // The module event's listener, on A.
  if (benchEvent >= 0 && unitA != NO_HANDLE) {
    const t = Trigger.onModuleEvent(benchEvent).bind(unitA).enqueue(moduleEvent).done();
    if (!made(t)) benchFail("basics.module-event", "its trigger could not be made");
  }
  // A one-time superweapon for the player, for the `SuperWeapons` scope.
  const supers: StaticArray<string> = ["ParaDropSpecial", "AmerParaDropSpecial", "NukeSpecial"];
  for (let i = 0; i < supers.length; i++) {
    const kind = Rules.superWeaponType(supers[i]);
    if (kind == NO_HANDLE) continue;
    if (hasKind(1, 33) && Action.oneTimeSuperWeapon(player(), kind) == Status.Ok) superKind = kind;
    break;
  }
}

/** Every later frame: one check group per frame, then the polls. */
export function frame(now: i32): void {
  const step = <i64>now - openAt;
  switch (<i32>step) {
    case 1: hostCalls(now); break;
    case 2: namesRules(); break;
    case 3: namesEngine(); break;
    case 4: objectReads(); break;
    case 5: houseAndTypeReads(); break;
    case 6: cellAndGlobalReads(now); break;
    case 7: viewpointReads(); break;
    case 8: revealed(); break;
    case 9: variables(); break;
    case 10: findObjectsAndHouses(); break;
    case 11: findCellsAndSupers(); break;
    case 12: writeHealthAndRank(); break;
    case 13: writeOwnerAway(); break;
    case 14: writeOwnerBack(); break;
    case 15: writeMission(); break;
    case 16: typeNameAndPoints(); break;
    case 17: writeAmmo(); break;
    case 18: spawnBuilding(); break;
    case 19: writeOnline(); break;
    case 20: modifierCompose(); break;
    case 21: modifierGroup(); break;
    case 22: modifierProps(); break;
    case 23: gameMode(); break;
    case 24: stat(Prop.ArmorMultiplier, "basics.mod-armor"); break;
    case 25: stat(Prop.SpeedMultiplier, "basics.mod-speed"); break;
    case 26: stat(Prop.ROFMultiplier, "basics.mod-rof"); break;
    case 27: stat(Prop.WeaponRangeMultiplier, "basics.mod-range"); break;
    case 28: rangeExtra(); break;
    case 29: phobosStats(); break;
    case 30: flag(Prop.Cloakable, "basics.mod-cloakable"); break;
    case 31: flag(Prop.ForceDecloak, "basics.mod-decloak"); break;
    case 32: immunity(); break;
    case 33: stat(Prop.ROTMultiplier, "basics.mod-rot"); break;
    case 34: modifierExpireStart(); break;
    case 35: findTypes(); break;
    case 40: raiseEvent(); break;
    case 42: modifierExpireCheck(); break;
    case 45: if (alive(unitE)) Action.remove(unitE); break;
    case 47: nearbyEnemies(); break;
    case 50: cloakStart(); break;
    case 60: targetStart(); break;
    default: break;
  }
  if (step >= 51 && step <= 900 && !marked("basics.cloaked")) cloakPoll(step == 900);
  if (step >= 61 && step <= 900 && !marked("basics.can-fire")) targetPoll(step, step == 900);
  if (step >= 35 && step % 100 == 35 && !marked("basics.buildable")) buildablePoll();
  if (step >= 100 && !marked("basics.door-open")) doorPoll(step);
  if (step >= 15 && step <= 60 && !marked("basics.set-mission")) missionPoll();
  if (step > 45 && step % 200 == 0 && !marked("basics.module-event") && step < 2000) raiseEvent();
  if (step >= 60 && step % 30 == 0 && !marked("basics.factory")) factoryPoll();
  if (step >= 100 && step % 50 == 0 && !marked("basics.placeable")) placeablePoll();
}

// ------------------------------------------------------------ Host calls

function hostCalls(now: i32): void {
  check("basics.frame", sdkFrame() == now && now > 0, "frame() differs");
  let inRange: bool = true;
  for (let i = 0; i < 8; i++) {
    const n = random(3, 9);
    inRange = inRange && n >= 3 && n <= 9;
  }
  check("basics.random", inRange, "random left its range");
  log("Control Bench basics: log line");
  mark("basics.log");
  const yes = hasKind(0, Event.TakeDamage)
    && hasKind(1, ActionKind.CreateObject)
    && hasKind(2, Prop.Health)
    && hasKind(2, Prop.FirepowerMultiplier)
    && hasKind(3, <i32>NameKind.TechnoType)
    && hasKind(4, 3)
    && hasKind(4, 7)
    && hasKind(4, 8);
  const no = !hasKind(4, 4) && !hasKind(3, 22) && !hasKind(1, 1999);
  check("basics.has-kind", yes && no, "has_kind answered a kind wrongly");
}

/** `lookup` then `nameOf` of the first of `names` that resolves. */
function roundTrip(kind: NameKind, names: StaticArray<string>, name: string): void {
  for (let i = 0; i < names.length; i++) {
    const candidate = names[i];
    const id = lookup(kind, candidate);
    if (!ok()) continue;
    const back = nameOrNull(kind, id);
    check(
      name,
      back != null && back.toLowerCase() == candidate.toLowerCase(),
      "name_of does not give the looked-up name back",
    );
    return;
  }
  benchFail(name, "no candidate name resolves");
}

function namesRules(): void {
  roundTrip(NameKind.TechnoType, ["MTNK", "HTNK"], "basics.name-techno");
  roundTrip(NameKind.Weapon, ["105mm", "120mm", "M60"], "basics.name-weapon");
  roundTrip(NameKind.Warhead, ["AP", "HE", "SA"], "basics.name-warhead");
  roundTrip(NameKind.Projectile, ["Cannon", "Invisible", "InvisibleLow", "InvisibleHigh"], "basics.name-projectile");
  roundTrip(NameKind.Anim, ["TWLT070", "EXPLOSML"], "basics.name-anim");
}

function namesEngine(): void {
  roundTrip(NameKind.VoxelAnim, ["TIRE", "PIECE", "PEBBLE"], "basics.name-voxelanim");
  roundTrip(NameKind.ParticleSystem, ["SparkSys", "FireStreamSys", "GasCloudSys"], "basics.name-particles");
  roundTrip(NameKind.SuperWeapon, ["NukeSpecial", "LightningStormSpecial", "ParaDropSpecial"], "basics.name-super");
  // Properties and action kinds have fixed numbers.
  const health = lookupIs(NameKind.Property, "Health", Prop.Health);
  const passable = lookupIs(NameKind.Property, "Passable.Foot", Prop.Passable_Foot);
  const rot = nameOrNull(NameKind.Property, Prop.ROTMultiplier);
  check(
    "basics.name-property",
    health && passable && rot != null && rot == "ROTMultiplier",
    "Property names and IDs differ",
  );
  const create = lookupIs(NameKind.ActionKind, "CreateObject", ActionKind.CreateObject);
  const logName = nameOrNull(NameKind.ActionKind, ActionKind.Log);
  check("basics.name-action", create && logName != null && logName == "Log", "ActionKind names and IDs differ");
  // The module event this area registered.
  const event = benchEvent;
  const id = lookupIs(NameKind.ModuleEvent, EVENT_NAME, event);
  const back = nameOrNull(NameKind.ModuleEvent, event);
  check(
    "basics.name-event",
    event >= 0 && id && back != null && back == EVENT_NAME,
    "ModuleEvent lookup and name_of disagree",
  );
}

// ------------------------------------------------------------ Properties

const OBJECT_PROPS: StaticArray<i32> = [
  Prop.Type, Prop.Owner, Prop.Category, Prop.Health, Prop.Strength, Prop.HealthPercent,
  Prop.X, Prop.Y, Prop.Z, Prop.Cell, Prop.Rank,
];
const OBJECT_MORE: StaticArray<i32> = [
  Prop.Layer, Prop.Target, Prop.Experience, Prop.Storage, Prop.Transport, Prop.PassengerCount,
  Prop.TransportSpaceLeft, Prop.DisguiseType, Prop.DisguiseOwner, Prop.Moving, Prop.Facing,
  Prop.TurretFacing, Prop.Armor, Prop.Destination, Prop.WeaponReady, Prop.Deployed,
  Prop.UnderConstruction, Prop.Parasite,
];
const OWNER_HEALTH: StaticArray<i32> = [Prop.Owner, Prop.Health];
const V11 = new StaticArray<i64>(11);
const V18 = new StaticArray<i64>(18);
const V4 = new StaticArray<i64>(4);
const ONE_REF = new StaticArray<i64>(1);
const TWO_REFS = new StaticArray<i64>(2);

/** `get_many(&[reference], props, out).is_ok()`. */
function readOne(reference: i64, props: StaticArray<i32>, out: StaticArray<i64>): bool {
  ONE_REF[0] = reference;
  getMany(ONE_REF, props, out);
  return ok();
}

function objectReads(): void {
  const a = unitA;
  const mtnk = tankType();
  if (mtnk == NO_HANDLE || !alive(a)) {
    benchFail("basics.object-props", "unit A is missing");
    return;
  }
  const v = V11;
  let read = readOne(a, OBJECT_PROPS, v);
  const strength = getOr(mtnk, Prop.Strength, -1);
  const cell: Cell = v[9];
  check(
    "basics.object-props",
    read
      && v[0] == mtnk
      && v[1] == player()
      && v[2] == <i64>Category.Vehicle
      && v[3] > 0
      && v[4] == strength
      && v[5] >= 1 && v[5] <= 100
      && v[6] / 256 == <i64>Cell.x(cell)
      && v[7] / 256 == <i64>Cell.y(cell)
      && v[10] == 0,
    "unit A's basic properties read wrong",
  );
  const m = V18;
  read = readOne(a, OBJECT_MORE, m);
  const armor = getOr(mtnk, Prop.Armor, -1);
  check(
    "basics.object-more",
    read
      && m[4] == NO_HANDLE
      && m[5] == 0
      && m[7] == NO_HANDLE
      && m[8] == NO_HANDLE
      && m[10] >= 0 && m[10] <= 255
      && m[11] >= 0 && m[11] <= 255
      && m[12] == armor
      && m[16] == 0
      && m[17] == NO_HANDLE,
    "unit A's 7.1 properties read wrong",
  );
  // Two objects, two properties, row-major.
  const b = unitB;
  TWO_REFS[0] = a;
  TWO_REFS[1] = b;
  getMany(TWO_REFS, OWNER_HEALTH, V4);
  read = ok();
  const two = V4;
  const healthA = get(a, Prop.Health);
  const okA = ok();
  const healthB = get(b, Prop.Health);
  const okB = ok();
  check(
    "basics.get-many",
    read && two[0] == player() && two[2] == player() && okA && two[1] == healthA && okB && two[3] == healthB,
    "get_many differs from get",
  );
}

const HOUSE_PROPS: StaticArray<i32> = [
  Prop.Index, Prop.Money, Prop.PowerOutput, Prop.PowerDrain, Prop.LowPower, Prop.IsDefeated,
  Prop.IsHuman, Prop.TechLevel, Prop.Country, Prop.ControlDomains,
];
const TYPE_KEYS: StaticArray<i32> = [
  Prop.Category, Prop.Strength, Prop.Cost, Prop.Soylent, Prop.Speed, Prop.ROT, Prop.Crushable,
  Prop.Selfhealing, Prop.Explodes, Prop.Power, Prop.Sight, Prop.Ammo, Prop.InitialAmmo, Prop.Storage,
  Prop.Passengers, Prop.SizeLimit, Prop.TechLevel, Prop.Armor,
];
const WEAPON_KEYS: StaticArray<i32> = [Prop.Damage, Prop.ROF, Prop.Burst, Prop.MinimumRange, Prop.Range];
const WARHEAD_KEYS: StaticArray<i32> = [
  Prop.CellSpread, Prop.PercentAtMax, Prop.Wall, Prop.Wood, Prop.Sparky, Prop.Fire, Prop.Conventional,
  Prop.Rocker, Prop.InfDeath,
];
const V20 = new StaticArray<i64>(20);
const V5 = new StaticArray<i64>(5);
const V9 = new StaticArray<i64>(9);

function houseAndTypeReads(): void {
  const v = V20;
  TWO_REFS[0] = player();
  TWO_REFS[1] = enemy();
  getMany(TWO_REFS, HOUSE_PROPS, v);
  const read = ok();
  check(
    "basics.house-props",
    read && v[6] == 1 && v[16] == 0 && v[0] != v[10] && v[5] == 0 && v[15] == 0,
    "house properties read wrong",
  );
  // The player's country: a Country type reference, named and looked up.
  const country = TypeRef.id(v[8]);
  const name = nameOrNull(NameKind.Country, country);
  if (name != null) {
    check("basics.name-country", lookupIs(NameKind.Country, name, country), "Country lookup differs from name_of");
  } else {
    benchFail("basics.name-country", "the house's country has no name");
  }
  // 7.7: TechnoType, Weapon and Warhead keys.
  const mtnk = tankType();
  if (mtnk != NO_HANDLE) {
    const t = V18;
    const typeRead = readOne(mtnk, TYPE_KEYS, t);
    check(
      "basics.type-props",
      typeRead && t[0] == <i64>Category.Vehicle && t[1] > 0 && t[2] > 0 && t[4] > 0,
      "MTNK's type keys read wrong",
    );
  }
  const weapons: StaticArray<string> = ["105mm", "120mm", "M60"];
  for (let i = 0; i < weapons.length; i++) {
    const weapon = Rules.weapon(weapons[i]);
    if (weapon == NO_HANDLE) continue;
    const w = V5;
    const weaponRead = readOne(weapon, WEAPON_KEYS, w);
    check("basics.weapon-props", weaponRead && w[0] > 0 && w[4] > 0, "the weapon's keys read wrong");
    break;
  }
  const warheads: StaticArray<string> = ["AP", "HE", "SA"];
  for (let i = 0; i < warheads.length; i++) {
    const warhead = Rules.warhead(warheads[i]);
    if (warhead == NO_HANDLE) continue;
    const w = V9;
    const warheadRead = readOne(warhead, WARHEAD_KEYS, w);
    check("basics.warhead-props", warheadRead && w[0] >= 0, "the warhead's keys read wrong");
    break;
  }
}

const CELL_PROPS: StaticArray<i32> = [
  Prop.LandType, Prop.Height, Prop.Overlay, Prop.Ore, Prop.Waypoint, Prop.CellTag,
  Prop.Passable_Foot, Prop.Passable_Track, Prop.Passable_Wheel, Prop.Passable_Hover,
  Prop.Passable_Winged, Prop.Passable_Float, Prop.Passable_Amphibious, Prop.Passable_FloatBeach,
  Prop.Obstructed,
];
const GLOBAL_PROPS: StaticArray<i32> = [
  Prop.Frame, Prop.PlayerCount, Prop.GameSpeed, Prop.Difficulty, Prop.MissionTimer,
  Prop.MissionTimerRunning, Prop.AmbientLight, Prop.IonStormActive,
];
const V15 = new StaticArray<i64>(15);
const V8 = new StaticArray<i64>(8);

/** Unit A's cell, the arena's corner when it does not read. */
function homeOfA(): Cell {
  const cell = get(unitA, Prop.Cell);
  return ok() ? cell : arena(BASICS);
}

function cellAndGlobalReads(now: i32): void {
  const cell = homeOfA();
  const v = V15;
  const read = readOne(cell, CELL_PROPS, v);
  // A tank stands there: tracks pass.
  check("basics.cell-props", read && v[7] == 1 && v[3] >= 0, "the tank's cell reads wrong");
  const start = Waypoint.number(0);
  get(start, Prop.LandType);
  const land = ok();
  check(
    "basics.waypoint-cell",
    land && is(start, Prop.Waypoint, 0),
    "waypoint 0 does not read as its cell",
  );
  const g = V8;
  const globalRead = readOne(Global.REF, GLOBAL_PROPS, g);
  const globalFrame = Global.frame();
  const frameOk = ok();
  check(
    "basics.global-props",
    globalRead
      && g[0] == <i64>now
      && g[1] >= 1
      && g[3] >= 0 && g[3] <= 2
      && frameOk && globalFrame == <i64>now,
    "global properties read wrong",
  );
}

function viewpointReads(): void {
  const a = unitA;
  const e = unitE;
  const mtnk = tankType();
  if (mtnk == NO_HANDLE) return;
  const home = arena(BASICS);
  const out = OUT8;
  // The player sees its own house as itself and the enemy's tank as an enemy.
  const me = Find.houses()
    .seenBy(player())
    .where(Prop.Relation, Op.Eq, <i64>Relation.SameHouse)
    .page(out, 0);
  const own = ok() && me.count == 1 && out[0] == player();
  let hostile: bool = false;
  const htnk = Rules.technoType("HTNK");
  if (htnk != NO_HANDLE) {
    const found = Find.objects()
      .ownedBy(enemy())
      .ofType(htnk)
      .seenBy(player())
      .where(Prop.Relation, Op.Eq, <i64>Relation.Enemy)
      .page(out, 0);
    hostile = ok() && holds(out, found.count, e);
  }
  if (alive(e)) check("basics.relation", own && hostile, "Relation read wrong");
  // The player sees its own tank.
  const seen = Find.objects()
    .ownedBy(player())
    .ofType(mtnk)
    .near(home, 8 * 256)
    .seenBy(player())
    .where(Prop.Visible, Op.Eq, 1)
    .page(out, 0);
  check("basics.visible", ok() && holds(out, seen.count, a), "the player does not see its own tank");
  // Engage with A as the shooter: any EngageResult reads.
  if (alive(a) && alive(e)) {
    const engaged = Find.objects()
      .ownedBy(enemy())
      .near(home, 20 * 256)
      .seenBy(a)
      .where(Prop.Engage, Op.Ge, 0)
      .page(out, 0);
    check("basics.engage", ok() && holds(out, engaged.count, e), "Engage did not read for the enemy tank");
    check("basics.mod-noweapons", is(e, Prop.DisableWeapons, 1), "DisableWeapons does not read back");
  }
}

function revealed(): void {
  const cell = get(unitA, Prop.Cell);
  if (!ok()) return;
  const out = OUT;
  const found = Find.cells()
    .near(cell, 512)
    .seenBy(player())
    .where(Prop.Revealed, Op.Eq, 1)
    .page(out, 0);
  check("basics.revealed", ok() && holds(out, found.count, cell), "the tank's own cell is not revealed");
}

const FACTORY_PROPS: StaticArray<i32> = [
  Prop.Factory_Building_Type, Prop.Factory_Building_Progress, Prop.Factory_Building_Ready,
  Prop.Factory_Building_Queued, Prop.Factory_Infantry_Type, Prop.Factory_Vehicle_Type,
];
const V6 = new StaticArray<i64>(6);

/** The enemy's factories, once it builds something. */
function factoryPoll(): void {
  const v = V6;
  if (!readOne(enemy(), FACTORY_PROPS, v)) {
    benchFail("basics.factory", "Factory properties do not read");
    return;
  }
  if (v[0] != NO_HANDLE) {
    check("basics.factory", v[1] >= 0 && v[1] <= 100 && v[3] >= 1, "the enemy's building factory reads wrong");
  }
}

/** `Placeable` for the enemy around its first building, with a power plant's footprint. */
function placeablePoll(): void {
  const first = Find.objects()
    .ownedBy(enemy())
    .where(Prop.Category, Op.Eq, <i64>Category.Building)
    .first();
  const cell = get(first, Prop.Cell);
  if (!ok()) return;
  let plant = Rules.technoType("GAPOWR");
  if (plant == NO_HANDLE) plant = Rules.technoType("NAPOWR");
  if (plant == NO_HANDLE) return;
  const found = Find.cells()
    .within(offset(cell, -8, -8), 17, 17)
    .ofType(plant)
    .seenBy(enemy())
    .where(Prop.Placeable, Op.Eq, 1)
    .page(OUT4, 0);
  if (ok() && found.count > 0) mark("basics.placeable");
}

// ------------------------------------------------------------- Variables

function variables(): void {
  const a = unitA;
  const wave = BenchWave.get();
  const waveOk = ok();
  const score = BenchScore.get(player());
  const scoreOk = ok();
  const tag = BenchTag.get(a);
  const tagOk = ok();
  check(
    "basics.var-defaults",
    waveOk && wave == 7 && scoreOk && score == 3 && tagOk && tag == 5,
    "declared defaults read wrong",
  );
  let global: bool = BenchWave.set(11) == Status.Ok;
  if (global) {
    const back = BenchWave.get();
    global = ok() && back == 11;
  }
  check("basics.var-global", global, "the global variable does not read back");
  let house: bool = BenchScore.set(enemy(), 9) == Status.Ok;
  if (house) {
    const theirs = BenchScore.get(enemy());
    const theirsOk = ok();
    const ours = BenchScore.get(player());
    house = theirsOk && theirs == 9 && ok() && ours == 3;
  }
  check("basics.var-house", house, "the house variable does not read back");
  let object: bool = BenchTag.set(a, 42) == Status.Ok;
  if (object) {
    const onA = BenchTag.get(a);
    const onAOk = ok();
    const onB = BenchTag.get(unitB);
    object = onAOk && onA == 42 && ok() && onB == 5;
  }
  check("basics.var-object", object, "the object variable does not read back");
  // `mod.` names this module's own declarations; raw `var_get` agrees.
  const plain = lookup(NameKind.Variable, "BenchWave");
  const plainOk = ok();
  const prefixed = lookup(NameKind.Variable, "mod.BenchWave");
  const prefixedOk = ok();
  let rawOk: bool = false;
  if (plainOk) rawOk = sys.var_get(0, plain, RAW_VALUE) == 0 && load<i64>(RAW_VALUE) == 11;
  check(
    "basics.var-lookup",
    plainOk && plain >= 1000 && prefixedOk && plain == prefixed && rawOk,
    "lookup(Variable) with mod. differs",
  );
  if (plainOk) {
    const name = nameOrNull(NameKind.Variable, plain);
    check("basics.name-variable", name != null && name == "BenchWave", "name_of(Variable) differs");
  }
}

// ---------------------------------------------------------------- Search

function findObjectsAndHouses(): void {
  const a = unitA;
  const b = unitB;
  const out = OUT;
  // A variable clause on the candidate: only A holds 42.
  const tag = lookup(NameKind.Variable, "BenchTag");
  if (ok()) {
    const clauses: StaticArray<Clause> = [varClause(tag, Op.Eq, 42)];
    const found = rawFind(0, player(), 0, clauses, out);
    check("basics.find-var", found == 1 && out[0] == a, "a variable clause found the wrong objects");
  }
  const mtnk = tankType();
  if (mtnk != NO_HANDLE) {
    const found = Find.objects()
      .ownedBy(player())
      .ofType(mtnk)
      .where(Prop.Health, Op.Gt, 0)
      .page(out, 0);
    check(
      "basics.find-objects",
      ok() && holds(out, found.count, a) && holds(out, found.count, b),
      "owned_by/of_type/where_prop missed our tanks",
    );
  }
  const home = homeOfA();
  const inCircle = Find.objects().ownedBy(player()).near(home, 4 * 256).page(out, 0);
  const circle = ok() && holds(out, inCircle.count, a);
  const inRect = Find.objects().ownedBy(player()).within(offset(home, -3, -3), 7, 7).page(out, 0);
  const rect = ok() && holds(out, inRect.count, a);
  check("basics.find-objects-area", circle && rect, "a circle or rectangle missed tank A");
  const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(out, 0);
  const human = ok() && humans.count == 1 && out[0] == player();
  const houses = Find.houses().page(out, 0);
  const all = ok() && holds(out, houses.count, player()) && holds(out, houses.count, enemy());
  check("basics.find-houses", human && all, "the house search is wrong");
}

const SUPER_PROPS: StaticArray<i32> = [Prop.Type, Prop.Ready, Prop.ChargePercent];
const V3 = new StaticArray<i64>(3);

function findCellsAndSupers(): void {
  const home = homeOfA();
  const one = OUT1;
  const first = Find.cells().page(one, 0);
  check("basics.find-cells-map", ok() && first.count == 1 && one[0] != NO_HANDLE, "the map has no first cell");
  // Within one cell of the centre: the cell and its four neighbours.
  const out = OUT;
  const circle = Find.cells().near(home, 256).page(out, 0);
  let circleOk = ok() && circle.count >= 1 && circle.count <= 5 && holds(out, circle.count, home);
  for (let i = 0; circleOk && i < circle.count; i++) {
    const dx = Cell.x(out[i]) - Cell.x(home);
    const dy = Cell.y(out[i]) - Cell.y(home);
    if (dx * dx + dy * dy > 1) circleOk = false;
  }
  check("basics.find-cells-circle", circleOk, "the circle of cells is wrong");
  const rect = Find.cells().within(home, 3, 3).page(out, 0);
  let rectOk = ok() && rect.count >= 1 && rect.count <= 9;
  for (let i = 0; rectOk && i < rect.count; i++) {
    const x = Cell.x(out[i]);
    const y = Cell.y(out[i]);
    if (x < Cell.x(home) || x >= Cell.x(home) + 3 || y < Cell.y(home) || y >= Cell.y(home) + 3) rectOk = false;
  }
  check("basics.find-cells-rect", rectOk, "the rectangle of cells is wrong");
  const kind = superKind;
  if (kind == NO_HANDLE) {
    benchFail("basics.find-supers", "no superweapon could be granted");
    return;
  }
  const found = rawFindAll(8, player(), kind, out);
  let ownerOk: bool = false;
  let read: bool = false;
  const v = V3;
  if (found == 1) {
    ownerOk = is(out[0], Prop.Owner, player());
    read = readOne(out[0], SUPER_PROPS, v);
  }
  check(
    "basics.find-supers",
    read && ownerOk && v[2] >= 0 && v[2] <= 100,
    "the granted superweapon was not found",
  );
}

// ---------------------------------------------------------------- Writes

function writeHealthAndRank(): void {
  const b = unitB;
  const strength = get(b, Prop.Strength);
  if (!ok()) {
    benchFail("basics.set-health", "unit B is missing");
    return;
  }
  const half = max<i64>(strength / 2, 1);
  const wrote = Techno.setHealth(b, half) == Status.Ok && is(b, Prop.Health, half);
  const zero = Techno.setHealth(b, 0) == Status.Refused;
  Techno.setHealth(b, strength);
  check("basics.set-health", wrote && zero, "Health does not read back");
  const elite = set(b, Prop.Rank, 2) == Status.Ok && is(b, Prop.Rank, 2);
  const rookie = set(b, Prop.Rank, 0) == Status.Ok && is(b, Prop.Rank, 0);
  check("basics.set-rank", elite && rookie, "Rank does not read back");
}

function writeOwnerAway(): void {
  const b = unitB;
  if (Techno.setOwner(b, enemy()) != Status.Ok || !is(b, Prop.Owner, enemy())) {
    benchFail("basics.set-owner", "Owner does not read back");
  }
}

function writeOwnerBack(): void {
  const b = unitB;
  const wasEnemy = is(b, Prop.Owner, enemy());
  const back = Techno.setOwner(b, player()) == Status.Ok && is(b, Prop.Owner, player());
  check("basics.set-owner", wasEnemy && back, "Owner does not read back");
}

function writeMission(): void {
  const b = unitB;
  if (alive(b) && Techno.setMission(b, Mission.AreaGuard) != Status.Ok) {
    benchFail("basics.set-mission", "the Mission write failed");
  }
}

function missionPoll(): void {
  if (is(unitB, Prop.Mission, <i64>Mission.AreaGuard)) mark("basics.set-mission");
}

/** A unit with ammo (whichever candidate type has `Ammo=`), emptied and refilled, then removed. */
function writeAmmo(): void {
  const spot = offset(arena(BASICS), 0, 4);
  const names: StaticArray<string> = ["V3", "ORCA", "BEAG"];
  for (let i = 0; i < names.length; i++) {
    const kind = Rules.technoType(names[i]);
    if (kind == NO_HANDLE) continue;
    if (getOr(kind, Prop.Ammo, 0) <= 0) continue;
    const armed = spawn(spot, names[i], player());
    if (armed == NO_HANDLE) continue;
    const empty = Techno.setAmmo(armed, 0) == Status.Ok && is(armed, Prop.Ammo, 0);
    const one = Techno.setAmmo(armed, 1) == Status.Ok && is(armed, Prop.Ammo, 1);
    Action.remove(armed);
    check("basics.set-ammo", empty && one, "Ammo does not read back");
    return;
  }
  benchFail("basics.set-ammo", "no unit with ammo could be made");
}

/** A powered building of the player's (a type that drains power), for the `Online` write next frame. */
function spawnBuilding(): void {
  const spot = offset(arena(BASICS), 4, 4);
  const names: StaticArray<string> = ["TESLA", "ATESLA", "NASAM", "GAPILE", "NAHAND"];
  for (let i = 0; i < names.length; i++) {
    const kind = Rules.technoType(names[i]);
    if (kind == NO_HANDLE) continue;
    if (getOr(kind, Prop.Power, 0) >= 0) continue;
    const made = spawn(spot, names[i], player());
    if (made != NO_HANDLE) {
      building = made;
      return;
    }
  }
  benchFail("basics.set-online", "no powered building could be made");
}

function writeOnline(): void {
  const b = building;
  if (b == NO_HANDLE) return;
  const off = Techno.setOnline(b, false) == Status.Ok && is(b, Prop.Online, 0);
  const on = Techno.setOnline(b, true) == Status.Ok && is(b, Prop.Online, 1);
  // A tank has no Online.
  const tank = fails(unitA, Prop.Online, Status.Unsupported);
  // Not in the working body mode, not a factory; a tank has neither.
  check(
    "basics.animating",
    is(b, Prop.Animating, 0)
      && fails(b, Prop.DoorOpen, Status.Unsupported)
      && fails(unitA, Prop.Animating, Status.Unsupported),
    "Animating reads wrong",
  );
  Action.remove(b);
  check("basics.set-online", off && on && tank, "Online does not read back");
}

// ------------------------------------------------------------- Modifiers

/** `modifier.remove().is_ok()` for a modifier whose creation answered `created`. */
function removed(created: bool, m: i64): bool {
  return created && Modifier.remove(m) == Status.Ok;
}

/**
 * Add, Multiply and Override on `FirepowerMultiplier` of A: (1 + 0.5) * 2
 * = 3, the override 0.25 replaces it, and removing each restores.
 */
function modifierCompose(): void {
  const a = unitA;
  const fp = Prop.FirepowerMultiplier;
  const base = get(a, fp);
  const baseOk = ok();
  const add = Modifier.add(a, fp, HALF, 300);
  const addOk = ok();
  const afterAdd = is(a, fp, ONE + HALF);
  const mul = Modifier.multiply(a, fp, TWO, 300);
  const mulOk = ok();
  const afterMul = is(a, fp, THREE);
  const over = Modifier.overrideWith(a, fp, QUARTER, 300);
  const overOk = ok();
  const afterOver = is(a, fp, QUARTER);
  const overGone = removed(overOk, over) && is(a, fp, THREE);
  const addGone = removed(addOk, add);
  const restGone = addGone && removed(mulOk, mul);
  const restored = baseOk && is(a, fp, base);
  const composed = baseOk && base == ONE && afterAdd && afterMul && afterOver;
  check(
    "basics.mod-compose",
    composed && overGone && restGone && restored,
    "Add/Multiply/Override compose wrong",
  );
  check("basics.mod-firepower", afterMul && restored, "FirepowerMultiplier does not read back");
}

/**
 * Two modifiers of one group on A: the second replaces the first; then
 * `unmodify` removes it, and a second `unmodify` answers `Gone`.
 */
function modifierGroup(): void {
  const a = unitA;
  const fp = Prop.FirepowerMultiplier;
  const first = Modifier.create(a, fp, ModifierOp.Multiply, TWO, -1, 0, 9);
  const firstOk = ok();
  const second = Modifier.create(a, fp, ModifierOp.Multiply, THREE, -1, 0, 9);
  if (!firstOk || !ok()) {
    benchFail("basics.mod-group", "modify failed");
    return;
  }
  const replaced = fails(first, Prop.Value, Status.Gone) && is(second, Prop.Value, THREE) && is(a, fp, THREE);
  check("basics.mod-group", replaced, "the group did not replace");
  const gone = Modifier.remove(second) == Status.Ok
    && Modifier.remove(second) == Status.Gone
    && fails(second, Prop.Value, Status.Gone)
    && is(a, fp, ONE);
  check("basics.mod-unmodify", gone, "unmodify did not remove");
}

const MODIFIER_PROPS: StaticArray<i32> = [
  Prop.Target, Prop.Property, Prop.Op, Prop.Value, Prop.Remaining, Prop.Source, Prop.Group, Prop.Origin,
];

/** The 7.3 properties of a modifier with a source and a group, and the `Modifiers` scope. */
function modifierProps(): void {
  const a = unitA;
  const b = unitB;
  const m = Modifier.create(a, Prop.ArmorMultiplier, ModifierOp.Multiply, ONE + HALF, 100, b, 5);
  if (!ok()) {
    benchFail("basics.mod-props", "modify failed");
    return;
  }
  const v = V8;
  const read = readOne(m, MODIFIER_PROPS, v);
  check(
    "basics.mod-props",
    read
      && v[0] == a
      && v[1] == <i64>Prop.ArmorMultiplier
      && v[2] == <i64>ModifierOp.Multiply
      && v[3] == ONE + HALF
      && v[4] >= 99 && v[4] <= 100
      && v[5] == b
      && v[6] == 5,
    "the modifier's properties read wrong",
  );
  const out = OUT;
  const clauses: StaticArray<Clause> = [propClause(Prop.Target, Op.Eq, a)];
  const found = rawFind(7, 0, 0, clauses, out);
  check("basics.find-modifiers", found >= 0 && holds(out, found, m), "the Modifiers scope missed the modifier");
  Modifier.remove(m);
}

/** A 5-frame `SpeedMultiplier` on A, checked after it ran out. */
function modifierExpireStart(): void {
  const a = unitA;
  const m = Modifier.multiply(a, Prop.SpeedMultiplier, HALF, 5);
  if (ok() && is(a, Prop.SpeedMultiplier, HALF)) expiring = m;
  else benchFail("basics.mod-expire", "the timed modifier did not apply");
}

function modifierExpireCheck(): void {
  const m = expiring;
  if (m == NO_HANDLE) return;
  check(
    "basics.mod-expire",
    fails(m, Prop.Remaining, Status.Gone) && is(unitA, Prop.SpeedMultiplier, ONE),
    "the timed modifier did not expire",
  );
}

/** A factor stat: doubled while the modifier lasts, back after `unmodify`. */
function stat(prop: Prop, name: string): void {
  const a = unitA;
  const base = get(a, prop);
  if (!ok()) {
    benchFail(name, "the stat does not read");
    return;
  }
  const m = Modifier.multiply(a, prop, TWO, 60);
  if (!ok()) {
    benchFail(name, "modify failed");
    return;
  }
  let doubled = get(a, prop);
  const doubledOk = ok();
  if (!doubledOk) doubled = -1;
  const gone = Modifier.remove(m) == Status.Ok;
  const good = doubledOk && doubled == base * 2 && gone && is(a, prop, base);
  if (!good) {
    log("basics: " + name + " base " + base.toString() + " doubled " + doubled.toString());
  }
  check(name, good, "the stat does not read back");
}

/** A flag stat: an override of 1 reads 1. */
function flag(prop: Prop, name: string): void {
  const a = unitA;
  const m = Modifier.overrideWith(a, prop, 1, 60);
  if (!ok()) {
    benchFail(name, "modify failed");
    return;
  }
  const on = is(a, prop, 1);
  const gone = Modifier.remove(m) == Status.Ok;
  check(name, on && gone, "the flag does not read back");
}

function rangeExtra(): void {
  const a = unitA;
  const extra = Prop.WeaponRangeExtra;
  const base = get(a, extra);
  if (!ok()) {
    benchFail("basics.mod-range-extra", "the stat does not read");
    return;
  }
  const m = Modifier.add(a, extra, 256, 60);
  if (!ok()) {
    benchFail("basics.mod-range-extra", "modify failed");
    return;
  }
  const added = is(a, extra, base + 256);
  const gone = Modifier.remove(m) == Status.Ok && is(a, extra, base);
  check("basics.mod-range-extra", added && gone, "the extra range does not read back");
}

/** `CritMultiplier`, `CritExtraChance` and `Unkillable`: modifiers in a Phobos build, `Unsupported` (and `hasKind` 0) without one. */
function phobosStats(): void {
  const a = unitA;
  const crit = Prop.CritMultiplier;
  if (hasKind(2, crit)) {
    stat(crit, "basics.mod-crit");
    const extra = Prop.CritExtraChance;
    const m = Modifier.add(a, extra, QUARTER, 60);
    if (ok()) {
      if (!is(a, extra, QUARTER)) benchFail("basics.mod-crit", "CritExtraChance does not read back");
      Modifier.remove(m);
    }
  } else {
    Modifier.multiply(a, crit, TWO, 60);
    const multiplyRefused = answered(Status.Unsupported);
    Modifier.add(a, Prop.CritExtraChance, QUARTER, 60);
    const refused = multiplyRefused && answered(Status.Unsupported);
    check("basics.mod-crit", refused, "crit without Phobos answered wrongly");
  }
  if (hasKind(2, Prop.Unkillable)) {
    flag(Prop.Unkillable, "basics.mod-unkillable");
  } else {
    Modifier.overrideWith(a, Prop.Unkillable, 1, 60);
    check("basics.mod-unkillable", answered(Status.Unsupported), "Unkillable without Phobos answered wrongly");
  }
}

/** `ImmunityFlags`: an override mask reads back as itself; `Add` is not taken. */
function immunity(): void {
  const a = unitA;
  const prop = Prop.ImmunityFlags;
  const base = get(a, prop);
  if (!ok()) {
    benchFail("basics.mod-immunity", "ImmunityFlags does not read");
    return;
  }
  const m = Modifier.overrideWith(a, prop, 0x55, 60);
  if (!ok()) {
    benchFail("basics.mod-immunity", "modify failed");
    return;
  }
  const on = is(a, prop, 0x55);
  Modifier.add(a, prop, 1, 60);
  const add = answered(Status.Refused);
  const gone = Modifier.remove(m) == Status.Ok && is(a, prop, base);
  check("basics.mod-immunity", on && add && gone, "ImmunityFlags does not read back");
}

// ---------------------------------------------------------------- INI

/** `[Bench.Registry]`: `BenchAlpha` (Value=11) then `BenchBeta` (22). */
function iniEntry(record: IniEntryRecord): void {
  if (finaleStarted()) return;
  const section = record.entryName;
  if (!ok()) {
    benchFail("basics.ini-entry", "the entry has no name");
    return;
  }
  const position = record.position;
  let expectedName: string;
  let expectedValue: i64;
  let bit: i32;
  if (position == 0) {
    expectedName = "BenchAlpha";
    expectedValue = 11;
    bit = 1;
  } else if (position == 1) {
    expectedName = "BenchBeta";
    expectedValue = 22;
    bit = 2;
  } else {
    benchFail("basics.ini-entry", "an unexpected entry");
    return;
  }
  const valueText = Rules.get(section, "Value");
  let read = ok();
  const value = Ini.int(valueText, -1);
  read = read && ok();
  if (section != expectedName || !read || value != expectedValue) {
    benchFail("basics.ini-entry", "the entry or its Value reads wrong");
    return;
  }
  // The entry name is an engine string: name_of and lookup agree.
  const id = record.record.args;
  const back = text(id);
  const backOk = ok();
  check(
    "basics.name-string",
    backOk && back == expectedName && lookupIs(NameKind.String, expectedName, id),
    "String name_of and lookup disagree",
  );
  entries |= bit;
  if (entries == 3) mark("basics.ini-entry");
}

/** `Bench.Key=Bench,7` on `[BFRT]`. */
function iniKey(record: IniKeyRecord): void {
  if (finaleStarted()) return;
  const value = record.value;
  const valueOk = ok();
  const section = Rules.technoType(KEY_SECTION);
  const name = record.sectionName;
  const named = ok() && name.toLowerCase() == KEY_SECTION.toLowerCase();
  check(
    "basics.ini-key",
    valueOk && value == KEY_VALUE
      && !record.isGeneral
      && section != NO_HANDLE && section == record.typeRef
      && named,
    "the IniKey record reads wrong",
  );
}

function raiseEvent(): void {
  const a = unitA;
  const event = benchEvent;
  if (event < 0 || !alive(a)) return;
  if (Action.raiseEvent(event, a, NO_HANDLE, 77, 88) != Status.Ok) {
    benchFail("basics.module-event", "RaiseEvent failed");
  }
}

function moduleEvent(record: ModuleEventRecord): void {
  if (finaleStarted()) return;
  check(
    "basics.module-event",
    record.event == benchEvent && record.subject == unitA && record.value == 77 && record.value2 == 88,
    "the module event's record reads wrong",
  );
}

// ---------------------------------------------- Properties past the first set

/**
 * A type's `Name` round trip (string ID, `nameOf`, `lookup(String)`), and
 * TechnoType `Points`: the rules' `Points=` where the merged text holds
 * it, and a `Types` query on the value finds the type.
 */
function typeNameAndPoints(): void {
  const mtnk = tankType();
  if (mtnk == NO_HANDLE) {
    benchFail("basics.type-name", "MTNK does not resolve");
    return;
  }
  const id = getOr(mtnk, Prop.Name, 0);
  let named: bool = false;
  if (id > 0) {
    const name = nameOrNull(NameKind.String, <i32>id);
    named = name != null && name == "MTNK";
  }
  check(
    "basics.type-name",
    named && lookupIs(NameKind.String, "MTNK", <i32>id) && is(mtnk, Prop.Name, id),
    "a type's Name does not round-trip",
  );
  const points = get(mtnk, Prop.Points);
  if (!ok()) {
    benchFail("basics.points", "Points does not read");
    return;
  }
  const rulesText = Rules.get("MTNK", "Points");
  let haveRules = ok();
  const rules = Ini.int(rulesText, 0);
  haveRules = haveRules && ok();
  const found = Find.types(mtnk).where(Prop.Points, Op.Eq, points).page(OUT1, 0);
  const hit = ok() && found.count == 1 && OUT1[0] == mtnk;
  check(
    "basics.points",
    points >= 0 && hit && (!haveRules || rules == points),
    "Points differs from the rules",
  );
}

/** The global `GameMode`: 0 without a multiplayer mode, else a string the engine holds. */
function gameMode(): void {
  const id = get(Global.REF, Prop.GameMode);
  if (!ok()) {
    benchFail("basics.game-mode", "GameMode does not read");
    return;
  }
  let good: bool = id == 0;
  if (!good) {
    const mode = nameOrNull(NameKind.String, <i32>id);
    good = mode != null && mode.length > 0 && lookupIs(NameKind.String, mode, <i32>id);
  }
  check("basics.game-mode", good, "GameMode is not a held string");
}

/** `find` scope 3: every TechnoType for the wildcard, page by page, and a `Category` clause that keeps MTNK among the vehicles. */
function findTypes(): void {
  const mtnk = tankType();
  if (mtnk == NO_HANDLE) return;
  const out = OUT64;
  let total = 0;
  let vehicles = 0;
  let after: i64 = 0;
  let hasTank: bool = false;
  const all = Find.types(TypeRef.any(NameKind.TechnoType));
  while (true) {
    const page = all.page(out, after);
    if (!ok()) {
      benchFail("basics.find-types", "a Types page failed");
      return;
    }
    total += page.count;
    if (page.next == 0) break;
    after = page.next;
  }
  const query = Find.types(TypeRef.any(NameKind.TechnoType)).where(Prop.Category, Op.Eq, <i64>Category.Vehicle);
  after = 0;
  while (true) {
    const page = query.page(out, after);
    if (!ok()) {
      benchFail("basics.find-types", "a Types page failed");
      return;
    }
    vehicles += page.count;
    hasTank = hasTank || holds(out, page.count, mtnk);
    if (page.next == 0) break;
    after = page.next;
  }
  check("basics.find-types", hasTank && vehicles > 0 && vehicles < total, "the Types search is wrong");
}

/**
 * `Buildable` for the enemy (a computer house: CanBuild passes, the
 * requirements and prerequisites decide): some type it can build, and the
 * other sides' battle labs, which it cannot.
 */
function buildablePoll(): void {
  const some = Find.types(TypeRef.any(NameKind.TechnoType))
    .seenBy(enemy())
    .where(Prop.Buildable, Op.Eq, 1)
    .page(OUT8, 0);
  if (!ok() || some.count == 0) return;
  let labs = 0;
  let refused = 0;
  const names: StaticArray<string> = ["GATECH", "NATECH", "YATECH"];
  for (let i = 0; i < names.length; i++) {
    const lab = Rules.technoType(names[i]);
    if (lab == NO_HANDLE) continue;
    labs++;
    const no = Find.types(lab).seenBy(enemy()).where(Prop.Buildable, Op.Eq, 0).first();
    if (no == lab) refused++;
  }
  const plain = fails(unitA, Prop.Buildable, Status.Unsupported);
  // At least two of the three labs belong to another side.
  check("basics.buildable", labs == 3 && refused >= 2 && plain, "Buildable read wrong");
}

/** `NearbyEnemyCount` of A rises by one for an enemy made beside it and falls back once that one is removed, all within this frame. */
function nearbyEnemies(): void {
  const a = unitA;
  const cell = get(a, Prop.Cell);
  if (!ok()) {
    benchFail("basics.nearby-enemies", "unit A is missing");
    return;
  }
  const before = get(a, Prop.NearbyEnemyCount);
  if (!ok()) {
    benchFail("basics.nearby-enemies", "NearbyEnemyCount does not read");
    return;
  }
  const near = spawn(offset(cell, 1, 1), "HTNK", enemy());
  if (near == NO_HANDLE) {
    benchFail("basics.nearby-enemies", "no enemy could be made beside A");
    return;
  }
  const withIt = is(a, Prop.NearbyEnemyCount, before + 1);
  Action.remove(near);
  const after = is(a, Prop.NearbyEnemyCount, before);
  check("basics.nearby-enemies", withIt && after, "NearbyEnemyCount does not follow the enemy");
}

/** A player MTNK with a `Cloakable` modifier, apart from the others. */
function cloakStart(): void {
  const c = spawn(offset(arena(BASICS), -3, 3), "MTNK", player());
  if (c == NO_HANDLE) {
    benchFail("basics.cloaked", "no cloaker could be made");
    return;
  }
  cloaker = c;
  let good: bool = is(c, Prop.Cloaked, 0);
  if (good) {
    Modifier.create(c, Prop.Cloakable, ModifierOp.Override, 1, -1, 0, 97);
    good = ok();
  }
  if (!good) benchFail("basics.cloaked", "the cloaker starts wrong");
}

/** Until the cloaker reads cloaked, then `ForceDecloak` until it reads 0 again; removed when done or at the `last` poll. */
function cloakPoll(last: bool): void {
  const c = cloaker;
  if (!alive(c)) return;
  if (!cloakSeen) {
    if (is(c, Prop.Cloaked, 1)) {
      cloakSeen = true;
      Modifier.create(c, Prop.ForceDecloak, ModifierOp.Override, 1, -1, 0, 96);
      if (!ok()) benchFail("basics.cloaked", "ForceDecloak could not be added");
    }
  } else if (is(c, Prop.Cloaked, 0)) {
    mark("basics.cloaked");
    Action.remove(c);
    return;
  }
  if (last) Action.remove(c);
}

/** A player MTNK beside an enemy HTNK (weapons off, so it does not fight back), ordered to attack it. */
function targetStart(): void {
  const spot = offset(arena(BASICS), 3, 6);
  const s = spawn(spot, "MTNK", player());
  if (s == NO_HANDLE) {
    benchFail("basics.target-in-range", "no shooter could be made");
    return;
  }
  shooter = s;
  let at = get(s, Prop.Cell);
  if (!ok()) at = spot;
  const f = spawn(offset(at, 2, 0), "HTNK", enemy());
  if (f == NO_HANDLE) {
    benchFail("basics.target-in-range", "no target could be made");
    return;
  }
  foe = f;
  Modifier.create(f, Prop.DisableWeapons, ModifierOp.Override, 1, -1, 0, 99);
  const idle = is(s, Prop.TargetInRange, 0) && is(s, Prop.CanFireAtTarget, 0);
  if (!idle) benchFail("basics.target-in-range", "a shooter without a target reads one");
  Action.setMissionOn(s, Mission.Attack, f);
}

/**
 * Until the foe is the shooter's target (ordered again every 15 frames):
 * then it reads in range, out of range while a `WeaponRangeMultiplier`
 * override shrinks the weapon to nothing, and in range again once that is
 * removed (all in one frame); the shooter can fire at some poll. Both are
 * removed when done or at the `last` poll.
 */
function targetPoll(step: i64, last: bool): void {
  const s = shooter;
  const f = foe;
  if (!alive(s) || !alive(f)) return;
  if (!is(s, Prop.Target, f)) {
    if (step % 15 == 0) Action.setMissionOn(s, Mission.Attack, f);
  } else {
    if (!marked("basics.target-in-range") && is(s, Prop.TargetInRange, 1)) {
      const m = Modifier.create(s, Prop.WeaponRangeMultiplier, ModifierOp.Override, 1, -1, 0, 95);
      if (ok()) {
        const out = is(s, Prop.TargetInRange, 0) && is(s, Prop.CanFireAtTarget, 0);
        const gone = Modifier.remove(m) == Status.Ok;
        check(
          "basics.target-in-range",
          out && gone && is(s, Prop.TargetInRange, 1),
          "TargetInRange does not follow the weapon's range",
        );
      }
    }
    if (marked("basics.target-in-range") && is(s, Prop.CanFireAtTarget, 1)) mark("basics.can-fire");
  }
  if (last || marked("basics.can-fire")) {
    Action.remove(s);
    Action.remove(f);
  }
}

/**
 * A war factory of the player's own, ordered to Unload once built up
 * (again every 30 frames): Mission_Unload's factory arm opens its door
 * whether or not a product waits. Its door reads shut first, then open; a
 * tank has no door.
 */
function doorPoll(step: i64): void {
  let factory = warFactory;
  if (factory == NO_HANDLE) {
    const spot = offset(arena(BASICS), -6, 6);
    const names: StaticArray<string> = ["GAWEAP", "NAWEAP", "YAWEAP"];
    for (let i = 0; i < names.length; i++) {
      const made = spawn(spot, names[i], player());
      if (made != NO_HANDLE) {
        factory = made;
        break;
      }
    }
    // A busy patch of ground can clear before the next placement attempt.
    if (factory == NO_HANDLE) return;
    warFactory = factory;
  }
  if (!alive(factory)) return;
  const door = get(factory, Prop.DoorOpen);
  if (!ok()) {
    benchFail("basics.door-open", "the war factory has no door");
    return;
  }
  if (door == 0) {
    doorShut = true;
    if (step % 30 == 0 && is(factory, Prop.UnderConstruction, 0)) Techno.setMission(factory, Mission.Unload);
  } else if (door == 1 && doorShut) {
    check("basics.door-open", fails(unitA, Prop.DoorOpen, Status.Unsupported), "a tank reads a door");
    Action.remove(factory);
  }
}
