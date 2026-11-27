// Area `combat` of the bench (see `index.ts`), the port of the Rust
// `combat.rs`. Marks are named `combat.<point>`.
//
// Every combat decision, each on objects this area spawns in its arena and
// drives itself (a module call skips that module's links, so every
// decision here comes from a native fight):
// - the range: an enemy Rhino `T1` (weapons off, asleep) that four player
//   tanks attack. `T1` carries the `TakeDamage` decision (halved, checked
//   against its health on the next frame), the delivered `TakeDamage`
//   (counts the hits by shooter) and `FatalDamage` (the plan with the
//   Tiberium remains bit, checked as ore or overlay around its cell).
//   `SF` doubles its shot (`Fire` plan; two hits per shot), `SA` is refused
//   fire three times (`FireAllowed`) and then allowed, `SL`'s shells take a
//   `Launch` path (straight up first) and `SP`'s a `ProjectileFrame` step
//   (module data read back). `Launch` and `ProjectileFrame` bind the
//   shell's projectile type, learned from `SF`'s shot plan.
// - the shield: an enemy Rhino `T2` whose `DamageIncoming` a shield pool
//   absorbs; health stays full.
// - retaliation: an enemy Conscript `VR` (Sticky) hit by a player Rhino
//   `AR` from outside its own range; `Retaliate` answers 0 and `VR` must not
//   turn on `AR` where the engine would have (`native` 1).
// - crushing: a player Grizzly `CT` (weapons off) drives onto the cells of
//   two enemy GIs lined up east of it; `CrushAllowed` spares the first one
//   it meets and allows the other (its `Crushed` event follows).
//
// Units that die are respawned (up to `MAX_SPAWNS` each) until every mark
// is in. `Defeat` (`combat.defeat`) is not reached in the bench's scene.
//
// Each Rust key (300..310) is one handler here; each returns at once once
// the finale began.
import {
  Action, Cell, CrushAllowedDecision, CrushedEvent, DamageIncomingDecision, Event, FIXED_ONE,
  FatalDamageDecision, FireAllowedDecision, FireDecision, LaunchDecision, Mission, Modifier, NO_HANDLE,
  NameKind, PathKind, ProjectileFrameDecision, ProjectileStep, Prop, RetaliateDecision, Status,
  TakeDamageDecision, TakeDamageEvent, Techno, Trigger, TriggerRef, TypeRef, frame as clockFrame, get,
  lastStatus,
} from "@vera20k/sdk";
import { COMBAT, alive, made, arena, benchFail, enemy, mark, marked, offset, player, spawn } from "./bench";
import { finaleStarted } from "./finale";

/** Every mark this area reaches in the bench's scene. */
export const MARKS: StaticArray<string> = [
  "combat.take_damage",
  "combat.take_damage_event",
  "combat.damage_incoming",
  "combat.fatal",
  "combat.fire",
  "combat.fire_allowed",
  "combat.retaliate",
  "combat.launch",
  "combat.projectile_frame",
  "combat.crush",
];

// -------------------------------------------------------------- Triggers
// Indices of the Rust keys 300..310.

const TD_DECIDE: i32 = 0;
const TD_EVENT: i32 = 1;
const INCOMING: i32 = 2;
const FATAL: i32 = 3;
const FIRE: i32 = 4;
const FIRE_ALLOWED: i32 = 5;
const RETALIATE: i32 = 6;
const LAUNCH: i32 = 7;
const PFRAME: i32 = 8;
const CRUSH: i32 = 9;
const CRUSHED: i32 = 10;

const TRIGGERS = new StaticArray<i64>(11);

function make(key: i32, t: TriggerRef): void {
  // `made` reads the status `done` or `decideSync` left: call it first.
  if (made(t)) TRIGGERS[key] = t;
  else benchFail("combat.triggers", "trigger_create failed");
}

// ----------------------------------------------------------------- Units

/** What a slot's unit gets once spawned. */
const NONE: i32 = 0;
/** Weapons off. */
const DISARMED: i32 = 1;
/** Weapons off and asleep: a target. */
const TARGET: i32 = 2;
/** Sticky: holds its place, keeps its weapon and retaliates. */
const HOLD: i32 = 3;

const T1: i32 = 0;
const SF: i32 = 1;
const SL: i32 = 2;
const SP: i32 = 3;
const SA: i32 = 4;
const T2: i32 = 5;
const SDI: i32 = 6;
const VR: i32 = 7;
const AR: i32 = 8;
const CT: i32 = 9;
const I1: i32 = 10;
const I2: i32 = 11;
const UNITS: i32 = 12;

const SLOT_ID: StaticArray<string> = [
  "HTNK", "MTNK", "MTNK", "MTNK", "HTNK", "HTNK", "MTNK", "E2", "HTNK", "MTNK", "E1", "E1",
];
const SLOT_ENEMY: StaticArray<bool> = [true, false, false, false, false, true, false, true, false, false, true, true];
/** Cells from the arena's centre. */
const SLOT_DX: StaticArray<i32> = [0, -4, 0, 4, 0, -8, -5, -8, -4, -3, 2, 4];
const SLOT_DY: StaticArray<i32> = [0, 0, -4, 0, 4, -6, -6, 6, 3, 9, 0, 0];
const SLOT_SETUP: StaticArray<i32> = [
  TARGET, NONE, NONE, NONE, NONE, TARGET, NONE, HOLD, NONE, DISARMED, TARGET, TARGET,
];
/** The triggers bound to it, as bits of their indices. */
const SLOT_BINDS: StaticArray<i32> = [
  (1 << TD_DECIDE) | (1 << TD_EVENT) | (1 << FATAL),
  1 << FIRE,
  0,
  0,
  1 << FIRE_ALLOWED,
  1 << INCOMING,
  0,
  1 << RETALIATE,
  0,
  0,
  (1 << CRUSH) | (1 << CRUSHED),
  (1 << CRUSH) | (1 << CRUSHED),
];

/** Who attacks whom: (shooter, target). */
const ATTACK_SHOOTER: StaticArray<i32> = [SF, SL, SP, SA, SDI, AR];
const ATTACK_TARGET: StaticArray<i32> = [T1, T1, T1, T1, T2, VR];

/** Most respawns of one slot. */
const MAX_SPAWNS: i64 = 40;

const UNIT = new StaticArray<i64>(UNITS);
const SPAWNS = new StaticArray<i64>(UNITS);

function unit(slot: i32): Techno {
  return UNIT[slot];
}

function center(): Cell {
  return arena(COMBAT);
}

/**
 * Where a slot's offset counts from: the GIs line up east of the crusher
 * where it stands, the others from the arena's centre.
 */
function anchor(slot: i32): Cell {
  if (slot != I1 && slot != I2) return center();
  const tank = unit(CT);
  const cell = get(tank, Prop.Cell);
  if (lastStatus() == Status.Ok && alive(tank)) return cell;
  return offset(center(), -3, 9);
}

/** The current frame. */
function clock(): i64 {
  return <i64>clockFrame();
}

function allMarked(): bool {
  for (let i = 0; i < MARKS.length; i++) {
    if (!marked(MARKS[i])) return false;
  }
  return true;
}

/** Respawns the dead, rebinds their triggers and keeps the attackers on their targets; runs on a schedule. */
function upkeep(): void {
  if (allMarked()) return;
  for (let slot = 0; slot < UNITS; slot++) {
    if (alive(unit(slot)) || SPAWNS[slot] >= MAX_SPAWNS) continue;
    const house = SLOT_ENEMY[slot] ? enemy() : player();
    const made = spawn(offset(anchor(slot), SLOT_DX[slot], SLOT_DY[slot]), SLOT_ID[slot], house);
    if (made == NO_HANDLE) continue;
    SPAWNS[slot] = SPAWNS[slot] + 1;
    UNIT[slot] = made;
    if (slot == T2) shield = SHIELD_POOL; // a new shield for a new T2
    const binds = SLOT_BINDS[slot];
    for (let key = 0; key < TRIGGERS.length; key++) {
      if ((binds & (1 << key)) == 0) continue;
      const t = TRIGGERS[key];
      if (t != NO_HANDLE) TriggerRef.bind(t, made);
    }
    const setup = SLOT_SETUP[slot];
    if (setup == DISARMED || setup == TARGET) Modifier.overrideWith(made, Prop.DisableWeapons, 1, -1);
    if (setup == TARGET) {
      if (Action.setMission(made, Mission.Sleep, NO_HANDLE) != Status.Ok) {
        Action.setMission(made, Mission.Sticky, NO_HANDLE);
      }
    } else if (setup == HOLD) {
      Action.setMission(made, Mission.Sticky, NO_HANDLE);
    }
  }
  for (let i = 0; i < ATTACK_SHOOTER.length; i++) {
    const shooter = unit(ATTACK_SHOOTER[i]);
    const target = unit(ATTACK_TARGET[i]);
    if (alive(shooter) && alive(target)) {
      const current = get(shooter, Prop.Target);
      if (lastStatus() != Status.Ok || current != target) Action.setMissionOn(shooter, Mission.Attack, target);
    }
  }
  driveCrusher();
}

// ----------------------------------------------------------------- State

/** `TakeDamage` decision: T1's expected health and the decision's frame. */
let tdExpect: i64 = 0;
let tdFrame: i64 = 0;

/** `DamageIncoming`: the shield pool, T2's health at a fully absorbed hit and its frame. */
const SHIELD_POOL: i64 = 5000;
let shield: i64 = SHIELD_POOL;
let diHealth: i64 = 0;
let diFrame: i64 = 0;

/** `FatalDamage`: the dead T1's cell, the ore and overlay count around it before, and the frame. */
let fatalCell: i64 = 0;
let fatalOre: i64 = 0;
let fatalOverlays: i64 = 0;
let fatalFrame: i64 = 0;

/** `Fire`: the doubled shot's frame and the hits counted since. */
let fireFrame: i64 = 0;
let fireHits: i64 = 0;
/** The shell's projectile type ID + 1 (0 unknown). */
let shellType: i64 = 0;
/** The shell type value `Launch` and `ProjectileFrame` are bound to. */
let shellBound: i64 = 0;

/** `FireAllowed`: refusals so far and the frame fire was allowed. */
let faRefusals: i64 = 0;
let faOpen: i64 = 0;
const FA_REFUSE: i64 = 3;

/** `Retaliate`: the frame of a refusal the engine would have retaliated for. */
let retFrame: i64 = 0;

/** `Launch`: the tracked bullet, its launch position and frame. */
let launchBullet: i64 = 0;
let launchX: i64 = 0;
let launchY: i64 = 0;
let launchZ: i64 = 0;
let launchFrame: i64 = 0;
/** Straight up first, this far, at this speed. */
const RISE: i64 = 1024;
const RISE_SPEED: i64 = 96;
/** The path's two points, reused: a decision handler allocates nothing. */
const PATH = new StaticArray<i64>(6);

/** `ProjectileFrame`: the bullets answered and not yet checked (a ring). */
const PF_RING: i32 = 4;
const PF_BULLETS = new StaticArray<i64>(PF_RING);
let pfNext: i32 = 0;
const PF_MARKER: i64 = 0x5eedc0de;

/** `CrushAllowed`: the spared victim and its frame, the allowed one, and what was seen (bit 0 spared alive, bit 1 allowed crushed). */
let spared: i64 = 0;
let sparedFrame: i64 = 0;
let allowed: i64 = 0;
let crushSeen: i64 = 0;
/** The crusher's leg: even east, odd west. */
let crushLeg: i64 = 0;
/** The frame the current leg started. */
let legSince: i64 = 0;

// --------------------------------------------------------------- Exports

/** The initialization. */
export function init(): void {}

/** At scenario start. */
export function start(): void {}

/** The frame the arena opens: the houses and the arena are known. */
export function open(): void {
  make(TD_DECIDE, Trigger.on(Event.TakeDamage).decideSync(takeDamage));
  make(TD_EVENT, Trigger.on(Event.TakeDamage).enqueue(onTakeDamage).done());
  make(INCOMING, Trigger.on(Event.DamageIncoming).decideSync(incoming));
  make(FATAL, Trigger.on(Event.FatalDamage).decideSync(fatal));
  make(FIRE, Trigger.on(Event.Fire).decideSync(fire));
  make(FIRE_ALLOWED, Trigger.on(Event.FireAllowed).decideSync(fireAllowed));
  make(RETALIATE, Trigger.on(Event.Retaliate).decideSync(retaliate));
  make(CRUSH, Trigger.on(Event.CrushAllowed).decideSync(crush));
  make(CRUSHED, Trigger.on(Event.Crushed).enqueue(onCrushed).done());
  upkeep();
}

/** Every later frame. */
export function frame(at: i32): void {
  const now = <i64>at;
  if (now % 30 == 0) upkeep();
  bindShellType();
  checkTakeDamage(now);
  checkIncoming(now);
  checkFatal(now);
  checkRetaliate(now);
  checkLaunch(now);
  checkProjectileFrame();
  checkCrush(now);
}

// ----------------------------------------------------------- TakeDamage

/** Halves T1's damage and notes the health it must have afterwards. */
function takeDamage(d: TakeDamageDecision): void {
  if (finaleStarted()) return;
  const victim = d.victim;
  const damage = d.previous;
  if (victim != unit(T1) || damage <= 0) return;
  const answer = damage / 2;
  d.setResult(answer);
  const health = get(victim, Prop.Health);
  if (lastStatus() != Status.Ok) return;
  tdExpect = health - min(answer, health);
  tdFrame = clock();
}

function checkTakeDamage(now: i64): void {
  const at = tdFrame;
  if (at == 0 || now <= at || marked("combat.take_damage")) return;
  tdFrame = 0;
  const expect = tdExpect;
  if (expect <= 0) return; // the hit killed it
  const health = get(unit(T1), Prop.Health);
  if (lastStatus() != Status.Ok) return;
  if (health == expect) mark("combat.take_damage");
  else benchFail("combat.take_damage", "health after the halved hit differs");
}

/** The delivered `TakeDamage` on T1: the mark, and the hits by shooter for `Fire` and `FireAllowed`. */
function onTakeDamage(e: TakeDamageEvent): void {
  if (finaleStarted()) return;
  if (e.damage > 0 && e.victim != NO_HANDLE) mark("combat.take_damage_event");
  const attacker = e.attacker;
  if (attacker == NO_HANDLE) return;
  const now = clock();
  if (attacker == unit(SF)) {
    const shot = fireFrame;
    if (shot > 0 && now >= shot && now - shot < 50) {
      fireHits++;
      if (fireHits >= 2) mark("combat.fire");
    }
  }
  if (attacker == unit(SA)) {
    if (faOpen == 0) {
      if (faRefusals > 0) benchFail("combat.fire_allowed", "the shooter fired while refused");
    } else if (faRefusals >= FA_REFUSE) {
      mark("combat.fire_allowed");
    }
  }
}

// ------------------------------------------------------- DamageIncoming

/** A shield on T2: the pool absorbs incoming damage at the entry. */
function incoming(d: DamageIncomingDecision): void {
  if (finaleStarted()) return;
  const victim = d.victim;
  const damage = d.incoming;
  if (victim != unit(T2) || damage <= 0 || d.preview < 0) return;
  const pool = shield;
  const absorbed = min(damage, pool);
  shield = pool - absorbed;
  d.setResult(damage - absorbed);
  const health = get(victim, Prop.Health);
  if (lastStatus() == Status.Ok && absorbed == damage) {
    diHealth = health;
    diFrame = clock();
  } else {
    diFrame = 0;
  }
}

function checkIncoming(now: i64): void {
  const at = diFrame;
  if (at == 0 || now <= at || marked("combat.damage_incoming")) return;
  diFrame = 0;
  const health = get(unit(T2), Prop.Health);
  if (lastStatus() != Status.Ok) return;
  if (health == diHealth) mark("combat.damage_incoming");
  else benchFail("combat.damage_incoming", "an absorbed hit still took health");
}

// ---------------------------------------------------------- FatalDamage

let aroundOre: i64 = 0;
let aroundOverlays: i64 = 0;

/** Ore and the number of overlaid cells on and around `cell`, into `aroundOre` and `aroundOverlays`. */
function tiberiumAround(cell: Cell): void {
  let ore: i64 = 0;
  let overlays: i64 = 0;
  for (let dy = -1; dy <= 1; dy++) {
    for (let dx = -1; dx <= 1; dx++) {
      const c = offset(cell, dx, dy);
      const amount = Cell.ore(c);
      if (lastStatus() == Status.Ok) ore += amount;
      const overlay = Cell.overlay(c);
      if (lastStatus() == Status.Ok && overlay != NO_HANDLE) overlays++;
    }
  }
  aroundOre = ore;
  aroundOverlays = overlays;
}

/** T1's death plan with the Tiberium remains bit. */
function fatal(d: FatalDamageDecision): void {
  if (finaleStarted()) return;
  if (d.victim != unit(T1)) return;
  const x = d.x;
  const y = d.y;
  if (x < 0 || y < 0) return;
  const cell = Cell.at((x / 256) & 0xffff, (y / 256) & 0xffff);
  tiberiumAround(cell);
  d.setResult(d.with(FatalDamageDecision.TIBERIUM_REMAINS, true));
  fatalCell = cell;
  fatalOre = aroundOre;
  fatalOverlays = aroundOverlays;
  fatalFrame = clock();
}

function checkFatal(now: i64): void {
  const at = fatalFrame;
  if (at == 0 || now <= at || marked("combat.fatal")) return;
  tiberiumAround(fatalCell);
  if (aroundOre > fatalOre || aroundOverlays > fatalOverlays) {
    fatalFrame = 0;
    mark("combat.fatal");
  } else if (now - at > 10) {
    // Nothing grew (land that takes no Tiberium, or three zero amounts):
    // the next T1 tries again.
    fatalFrame = 0;
  }
}

// ----------------------------------------------------------------- Fire

/** SF's shot plan: the native entry twice. Learns the shell's projectile type for `Launch` and `ProjectileFrame`. */
function fire(d: FireDecision): void {
  if (finaleStarted()) return;
  if (d.shooter != unit(SF)) return;
  const native = d.previousEntry(0);
  if (native == null) return;
  if (native.projectileType >= 0) shellType = <i64>native.projectileType + 1;
  // The native entry is the area's entry 0: copy it into entry 1, answer 2.
  const second = d.planEntry(1);
  if (second == null) return;
  second.copyFrom(native);
  if (d.setPlanLength(2)) {
    fireFrame = clock();
    fireHits = 0;
  }
}

/** SA's first shots refused, then allowed. */
function fireAllowed(d: FireAllowedDecision): void {
  if (finaleStarted()) return;
  if (d.shooter != unit(SA)) return;
  if (faOpen == 0 && faRefusals < FA_REFUSE) {
    d.setResult(0);
    faRefusals++;
  } else {
    d.setResult(1);
    if (faOpen == 0) faOpen = clock();
  }
}

// ------------------------------------------------------------ Retaliate

/** VR never retaliates; a refusal where the engine would have is checked. */
function retaliate(d: RetaliateDecision): void {
  if (finaleStarted()) return;
  const victim = d.victim;
  if (victim != unit(VR)) return;
  const source = d.source;
  const native = d.native;
  d.setResult(0);
  if (native == 1 && source != NO_HANDLE && source == unit(AR) && !targets(victim, source) && !attacking(victim)) {
    retFrame = clock();
  }
}

/** `victim.target() == Ok(target)`. */
function targets(victim: Techno, target: i64): bool {
  const t = get(victim, Prop.Target);
  return lastStatus() == Status.Ok && t == target;
}

/** `victim.mission() == Ok(Mission::Attack)`. */
function attacking(victim: Techno): bool {
  const m = get(victim, Prop.Mission);
  return lastStatus() == Status.Ok && m == <i64>Mission.Attack;
}

function checkRetaliate(now: i64): void {
  const at = retFrame;
  if (at == 0 || now <= at || marked("combat.retaliate")) return;
  retFrame = 0;
  const victim = unit(VR);
  if (!alive(victim)) return;
  // Its own scan may still pick the attacker; only a clean frame counts.
  if (!targets(victim, unit(AR)) && !attacking(victim)) mark("combat.retaliate");
}

// ------------------------------------------------------ Launch and frame

/** Binds `Launch` and `ProjectileFrame` to the shell's projectile type once `SF`'s shot plan named it. */
function bindShellType(): void {
  const shell = shellType;
  if (shell == 0) return;
  const was = shellBound;
  shellBound = shell;
  if (was == shell) return;
  if (TRIGGERS[LAUNCH] != NO_HANDLE) TriggerRef.remove(TRIGGERS[LAUNCH]);
  if (TRIGGERS[PFRAME] != NO_HANDLE) TriggerRef.remove(TRIGGERS[PFRAME]);
  const ty = TypeRef.make(NameKind.Projectile, <i32>(shell - 1));
  make(LAUNCH, Trigger.on(Event.Launch).bind(ty).decideSync(launch));
  make(PFRAME, Trigger.on(Event.ProjectileFrame).bind(ty).decideSync(projectileFrame));
}

/** SL's shells: straight up first, then down onto the target. */
function launch(d: LaunchDecision): void {
  if (finaleStarted()) return;
  const shooter = unit(SL);
  if (shooter == NO_HANDLE || d.launcher != shooter) return;
  const bullet = d.projectile;
  const x = get(bullet, Prop.X);
  if (lastStatus() != Status.Ok) return;
  const y = get(bullet, Prop.Y);
  if (lastStatus() != Status.Ok) return;
  const z = get(bullet, Prop.Z);
  if (lastStatus() != Status.Ok) return;
  const target = d.target;
  let endX: i64 = 0;
  let endY: i64 = 0;
  let endZ: i64 = 0;
  let onTarget = false;
  if (target != NO_HANDLE) {
    endX = get(target, Prop.X);
    let good = lastStatus() == Status.Ok;
    endY = get(target, Prop.Y);
    good = good && lastStatus() == Status.Ok;
    endZ = get(target, Prop.Z);
    onTarget = good && lastStatus() == Status.Ok;
  }
  if (!onTarget) {
    const cell = d.targetCell;
    endX = <i64>Cell.x(cell) * 256 + 128;
    endY = <i64>Cell.y(cell) * 256 + 128;
    endZ = z;
  }
  PATH[0] = x;
  PATH[1] = y;
  PATH[2] = z + RISE;
  PATH[3] = endX;
  PATH[4] = endY;
  PATH[5] = endZ;
  if (d.setPath(PathKind.Waypoints, RISE_SPEED * FIXED_ONE, 0, 0, PATH) && launchBullet == 0) {
    launchBullet = bullet;
    launchX = x;
    launchY = y;
    launchZ = z;
    launchFrame = clock();
  }
}

function checkLaunch(now: i64): void {
  const bullet = launchBullet;
  if (bullet == NO_HANDLE || marked("combat.launch")) return;
  const x = get(bullet, Prop.X);
  let good = lastStatus() == Status.Ok;
  const y = get(bullet, Prop.Y);
  good = good && lastStatus() == Status.Ok;
  const z = get(bullet, Prop.Z);
  good = good && lastStatus() == Status.Ok;
  if (!good) {
    launchBullet = 0; // gone: track the next one
    return;
  }
  const dx = abs(x - launchX);
  const dy = abs(y - launchY);
  const rose = z > launchZ + 16;
  if (dx <= 32 && dy <= 32 && rose) {
    mark("combat.launch");
  } else if ((dx > 64 || dy > 64) && !rose) {
    launchBullet = 0;
    benchFail("combat.launch", "the bullet left its path");
  } else if (now - launchFrame > 40) {
    launchBullet = 0;
  }
}

/** Shooters whose shells `ProjectileFrame` steps: every player tank on the range but SL. */
const PF_SHOOTERS: StaticArray<i32> = [SP, SF, SA, AR];

function velocity(bullet: i64, prop: Prop): i32 {
  let v = get(bullet, prop);
  if (lastStatus() != Status.Ok) v = 0;
  if (v < <i64>i32.MIN_VALUE) return i32.MIN_VALUE;
  if (v > <i64>i32.MAX_VALUE) return i32.MAX_VALUE;
  return <i32>v;
}

/** One step for each shell of the shooters above, once per shell: its own velocity, and module data to read back. */
function projectileFrame(d: ProjectileFrameDecision): void {
  if (finaleStarted()) return;
  const bullet = d.projectile;
  const source = get(bullet, Prop.Source);
  if (lastStatus() != Status.Ok || source == NO_HANDLE) return;
  let ours = false;
  for (let i = 0; i < PF_SHOOTERS.length; i++) {
    if (unit(PF_SHOOTERS[i]) == source) ours = true;
  }
  if (!ours || d.frames < 0) return;
  for (let i = 0; i < PF_RING; i++) {
    if (PF_BULLETS[i] == bullet) return;
  }
  const vx = velocity(bullet, Prop.VX);
  const vy = velocity(bullet, Prop.VY);
  const vz = velocity(bullet, Prop.VZ);
  const step = d.newStep();
  if (step == null) return;
  step.flags = ProjectileStep.SET_VELOCITY | ProjectileStep.SET_DATA;
  step.velocityX = vx;
  step.velocityY = vy;
  step.velocityZ = vz;
  step.data0 = PF_MARKER;
  PF_BULLETS[pfNext % PF_RING] = bullet;
  pfNext++;
}

function checkProjectileFrame(): void {
  if (marked("combat.projectile_frame")) return;
  for (let i = 0; i < PF_RING; i++) {
    const bullet = PF_BULLETS[i];
    if (bullet == NO_HANDLE) continue;
    const data = get(bullet, Prop.Data0);
    if (lastStatus() != Status.Ok) {
      PF_BULLETS[i] = 0; // gone: the next shell is answered
    } else if (data == PF_MARKER) {
      mark("combat.projectile_frame");
      return;
    } else {
      PF_BULLETS[i] = 0;
      benchFail("combat.projectile_frame", "the step's module data was not kept");
    }
  }
}

// ---------------------------------------------------------------- Crush

/** The first GI the crusher meets is spared (each time), any other is crushed. */
function crush(d: CrushAllowedDecision): void {
  if (finaleStarted()) return;
  const victim = d.victim;
  if (victim == NO_HANDLE || (victim != unit(I1) && victim != unit(I2))) return;
  if (spared == NO_HANDLE || spared == victim) {
    d.setResult(0);
    spared = victim;
    sparedFrame = clock();
  } else {
    d.setResult(1);
    allowed = victim;
  }
}

function onCrushed(e: CrushedEvent): void {
  if (finaleStarted()) return;
  const victim = e.victim;
  if (victim == NO_HANDLE) return;
  if (victim == spared) benchFail("combat.crush", "a refused crush still crushed");
  else if (victim == allowed) crushSeen |= 2;
}

function checkCrush(now: i64): void {
  if (marked("combat.crush")) return;
  if ((crushSeen & 1) == 0 && !alive(spared)) spared = NO_HANDLE; // spare the next GI met instead
  const at = sparedFrame;
  if (at != 0 && now > at) {
    sparedFrame = 0;
    if (alive(spared)) crushSeen |= 1;
  }
  if (crushSeen == 3) mark("combat.crush");
}

/** The cell of the leg's GI (the other if that one is dead), `NO_HANDLE` for none. */
function goal(leg: i64): Cell {
  const first = leg % 2 == 0 ? I1 : I2;
  const second = first ^ I1 ^ I2;
  let gi: Techno = NO_HANDLE;
  if (alive(unit(first))) gi = unit(first);
  else if (alive(unit(second))) gi = unit(second);
  if (gi == NO_HANDLE) return NO_HANDLE;
  const cell = get(gi, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_HANDLE;
}

/**
 * Drives the crusher onto the GIs' cells in turn, one leg each: the crush
 * is checked in the cell the crusher stands in, so its legs end on the GIs
 * themselves. A leg ends when the crusher reaches that cell or after 300
 * frames.
 */
function driveCrusher(): void {
  const tank = unit(CT);
  if (marked("combat.crush")) return;
  if (!alive(tank)) return;
  const now = clock();
  let leg = crushLeg;
  const here = get(tank, Prop.Cell);
  const hereOk = lastStatus() == Status.Ok;
  const target = goal(leg);
  const reached = target != NO_HANDLE && hereOk && here == target;
  if (reached || now - legSince > 300) {
    leg++;
    crushLeg = leg;
    legSince = now;
  } else {
    const moving = get(tank, Prop.Moving);
    if (lastStatus() == Status.Ok && moving != 0) return;
  }
  const cell = goal(leg);
  if (cell != NO_HANDLE) Action.setMission(tank, Mission.Move, cell);
}
