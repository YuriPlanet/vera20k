// Decision views with a result area (RFC 0001 section 6f, appendix 4.3 and
// 4.5), the hand-written part of the Rust SDK's `decide.rs`. The simple
// views are generated into `generated/decisions.ts`; `scripts/tables.js`
// skips the ones here.
//
// A decision handler must not allocate: everything here reads and writes
// the record's result area in place, through `@unmanaged` struct views
// (`ProjectileSpec`, `ProjectileStep`, `MissionStepResult`, `MoveStep`,
// `PathCell`) over module memory. A view is valid only during the handler.
// A struct view of your own (to keep or copy from) goes over memory you
// reserve once: `ProjectileStep.at(memory.data(ProjectileStep.BYTES, 8))`.
//
//   function doubleShot(d: FireDecision): void {
//     if (d.previousCount < 1) return;
//     const second = d.planEntry(1);
//     if (second == null) return;
//     second.copyFrom(d.planEntry(0)!);
//     d.setPlanLength(2);
//   }
//   Trigger.on(Event.Fire).bind(gi).decideSync(doubleShot);

import { Techno, Cell, Ref, Projectile, Mission } from "./core";
import { EventRecord, DecideRecord } from "./records";

/** Whether `size` bytes at `at` fit `d`'s result area. */
function fits(d: DecideRecord, at: i32, size: i32): bool {
  return d.area != 0 && at >= 0 && size >= 0 && at <= d.areaCap - size;
}

/** The previous link's entry count, as far as its bytes in the area hold `size`-byte entries. */
function previousEntries(d: DecideRecord, size: i32): i32 {
  const count = d.previous;
  const held = d.previousLen / size;
  if (count <= 0 || held <= 0) return 0;
  return count < <i64>held ? <i32>count : held;
}

// ---------------------------------------------------------- Struct views

/** `ProjectileSpec`, 48 bytes: one entry of a `Fire` shot plan. */
@unmanaged
export class ProjectileSpec {
  static readonly BYTES: i32 = 48;
  /** `flags` bit 0: `origin` is absolute, otherwise an offset from the native launch position. */
  static readonly ORIGIN_ABSOLUTE: i32 = 1;

  /** The spec at `ptr` in module memory. */
  static at(ptr: usize): ProjectileSpec {
    return changetype<ProjectileSpec>(ptr);
  }
  get ptr(): usize {
    return changetype<usize>(this);
  }
  /** Projectile type ID. */
  get projectileType(): i32 { return load<i32>(this.ptr, 0); }
  set projectileType(v: i32) { store<i32>(this.ptr, v, 0); }
  /** Weapon ID giving damage and warhead; −1 for the shot's own weapon. */
  get weapon(): i32 { return load<i32>(this.ptr, 4); }
  set weapon(v: i32) { store<i32>(this.ptr, v, 4); }
  /** Object or cell reference; 0 for the shot's target. */
  get target(): Ref { return load<i64>(this.ptr, 8); }
  set target(v: Ref) { store<i64>(this.ptr, v, 8); }
  /** Initial `Data0` (data bytes 0..7). */
  get data0(): i64 { return load<i64>(this.ptr, 16); }
  set data0(v: i64) { store<i64>(this.ptr, v, 16); }
  /** Initial `Data1` (data bytes 8..15). */
  get data1(): i64 { return load<i64>(this.ptr, 24); }
  set data1(v: i64) { store<i64>(this.ptr, v, 24); }
  /** Bit 0 `ORIGIN_ABSOLUTE`. */
  get flags(): i32 { return load<i32>(this.ptr, 32); }
  set flags(v: i32) { store<i32>(this.ptr, v, 32); }
  /** Origin x, y, z in leptons. */
  get originX(): i32 { return load<i32>(this.ptr, 36); }
  set originX(v: i32) { store<i32>(this.ptr, v, 36); }
  get originY(): i32 { return load<i32>(this.ptr, 40); }
  set originY(v: i32) { store<i32>(this.ptr, v, 40); }
  get originZ(): i32 { return load<i32>(this.ptr, 44); }
  set originZ(v: i32) { store<i32>(this.ptr, v, 44); }
  /** Zeroes all 48 bytes. */
  clear(): void { memory.fill(this.ptr, 0, 48); }
  /** Copies `other`'s 48 bytes here. */
  copyFrom(other: ProjectileSpec): void { memory.copy(this.ptr, changetype<usize>(other), 48); }
}

/** `ProjectileStep`, 56 bytes: `ProjectileFrame`'s result. */
@unmanaged
export class ProjectileStep {
  static readonly BYTES: i32 = 56;
  /** `flags` bits. */
  static readonly DETONATE: i32 = 1;
  static readonly SET_VELOCITY: i32 = 2;
  static readonly SET_POSITION: i32 = 4;
  static readonly SET_DATA: i32 = 8;

  static at(ptr: usize): ProjectileStep {
    return changetype<ProjectileStep>(ptr);
  }
  get ptr(): usize {
    return changetype<usize>(this);
  }
  get flags(): i32 { return load<i32>(this.ptr, 0); }
  set flags(v: i32) { store<i32>(this.ptr, v, 0); }
  /** Velocity x, y, z: leptons per frame with 16 fraction bits. */
  get velocityX(): i32 { return load<i32>(this.ptr, 8); }
  set velocityX(v: i32) { store<i32>(this.ptr, v, 8); }
  get velocityY(): i32 { return load<i32>(this.ptr, 12); }
  set velocityY(v: i32) { store<i32>(this.ptr, v, 12); }
  get velocityZ(): i32 { return load<i32>(this.ptr, 16); }
  set velocityZ(v: i32) { store<i32>(this.ptr, v, 16); }
  /** Position x, y, z in leptons. */
  get positionX(): i32 { return load<i32>(this.ptr, 24); }
  set positionX(v: i32) { store<i32>(this.ptr, v, 24); }
  get positionY(): i32 { return load<i32>(this.ptr, 28); }
  set positionY(v: i32) { store<i32>(this.ptr, v, 28); }
  get positionZ(): i32 { return load<i32>(this.ptr, 32); }
  set positionZ(v: i32) { store<i32>(this.ptr, v, 32); }
  /** New `Data0` (with `SET_DATA`). */
  get data0(): i64 { return load<i64>(this.ptr, 40); }
  set data0(v: i64) { store<i64>(this.ptr, v, 40); }
  /** New `Data1` (with `SET_DATA`). */
  get data1(): i64 { return load<i64>(this.ptr, 48); }
  set data1(v: i64) { store<i64>(this.ptr, v, 48); }
  clear(): void { memory.fill(this.ptr, 0, 56); }
  copyFrom(other: ProjectileStep): void { memory.copy(this.ptr, changetype<usize>(other), 56); }
}

/** `MissionStepResult`, 24 bytes: `MissionStep`'s result, arriving with the mission body's outcome. */
@unmanaged
export class MissionStepResult {
  static readonly BYTES: i32 = 24;

  static at(ptr: usize): MissionStepResult {
    return changetype<MissionStepResult>(ptr);
  }
  get ptr(): usize {
    return changetype<usize>(this);
  }
  /** Object or cell reference, 0 for none. */
  get target(): Ref { return load<i64>(this.ptr, 0); }
  set target(v: Ref) { store<i64>(this.ptr, v, 0); }
  /** Object or cell reference, 0 for none. */
  get destination(): Ref { return load<i64>(this.ptr, 8); }
  set destination(v: Ref) { store<i64>(this.ptr, v, 8); }
  /** `Mission` number to queue, −1 for none. */
  get mission(): i32 { return load<i32>(this.ptr, 16); }
  set mission(v: i32) { store<i32>(this.ptr, v, 16); }
  /** Frames until the mission body runs again (at least 1). */
  get delay(): i32 { return load<i32>(this.ptr, 20); }
  set delay(v: i32) { store<i32>(this.ptr, v, 20); }
  copyFrom(other: MissionStepResult): void { memory.copy(this.ptr, changetype<usize>(other), 24); }
}

/** `MoveStep`, 16 bytes: `MoveFrame`'s result. */
@unmanaged
export class MoveStep {
  static readonly BYTES: i32 = 16;

  static at(ptr: usize): MoveStep {
    return changetype<MoveStep>(ptr);
  }
  get ptr(): usize {
    return changetype<usize>(this);
  }
  /** `MoveFrameDecision.MOVE`, `.FACING`, `.STOP`. */
  get flags(): i32 { return load<i32>(this.ptr, 0); }
  set flags(v: i32) { store<i32>(this.ptr, v, 0); }
  /** Body facing 0..255. */
  get facing(): i32 { return load<i32>(this.ptr, 4); }
  set facing(v: i32) { store<i32>(this.ptr, v, 4); }
  /** Displacement x, y in leptons, each at most 2,048 either way. */
  get dx(): i32 { return load<i32>(this.ptr, 8); }
  set dx(v: i32) { store<i32>(this.ptr, v, 8); }
  get dy(): i32 { return load<i32>(this.ptr, 12); }
  set dy(v: i32) { store<i32>(this.ptr, v, 12); }
  clear(): void { memory.fill(this.ptr, 0, 16); }
  copyFrom(other: MoveStep): void { memory.copy(this.ptr, changetype<usize>(other), 16); }
}

/** `PathCell`, 8 bytes: one cell of a `PathFound` route. */
@unmanaged
export class PathCell {
  static readonly BYTES: i32 = 8;

  static at(ptr: usize): PathCell {
    return changetype<PathCell>(ptr);
  }
  get x(): i32 { return load<i32>(changetype<usize>(this), 0); }
  set x(v: i32) { store<i32>(changetype<usize>(this), v, 0); }
  get y(): i32 { return load<i32>(changetype<usize>(this), 4); }
  set y(v: i32) { store<i32>(changetype<usize>(this), v, 4); }
  /** The cell reference (`Cell.at(x, y)`). */
  get cell(): Cell { return Cell.at(this.x, this.y); }
}

// ---------------------------------------------------------------- Fire

/**
 * `Fire` (appendix 4.3, preview): the shot plan, just before the shooter's
 * weapon launches its bullet. The result is the number of `ProjectileSpec`
 * entries in the result area (0..16); the first link gets the native shot
 * as one entry (its projectile type and weapon, target and offset 0). The
 * engine launches the plan's projectiles in place of the native one; an
 * empty plan launches nothing. Every other effect of the shot (rearm,
 * ammo, burst, report, recoil) stays.
 *
 * The plan is edited in place: the previous link's entries are the area's
 * first entries, so writing `planEntry(i)` replaces `previousEntry(i)`.
 */
@unmanaged
export class FireDecision {
  get shooter(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** The target object; `NO_HANDLE` for a cell. */
  get target(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }
  get targetCell(): Cell {
    return load<i64>(changetype<usize>(this), 64);
  }
  /** The weapon slot. */
  get slot(): i64 {
    return load<i64>(changetype<usize>(this), 72);
  }
  /** The weapon's type ID. */
  get weapon(): i32 {
    return load<i32>(changetype<usize>(this), 88);
  }

  /** The number of entries the previous link's plan has (1 for the first link). */
  get previousCount(): i32 {
    return previousEntries(this.request, ProjectileSpec.BYTES);
  }
  /** The previous link's entry `index`, or `null` past its plan. Read it before writing `planEntry(index)`. */
  previousEntry(index: i32): ProjectileSpec | null {
    if (index < 0 || index >= this.previousCount) return null;
    return ProjectileSpec.at(this.request.area + <usize>(index * ProjectileSpec.BYTES));
  }
  /** How many entries the result area holds. */
  get planCapacity(): i32 {
    const d = this.request;
    return d.area == 0 ? 0 : d.areaCap / ProjectileSpec.BYTES;
  }
  /** Entry `index` of the answer, written in place; `null` when it does not fit the area. */
  planEntry(index: i32): ProjectileSpec | null {
    if (!fits(this.request, index * ProjectileSpec.BYTES, ProjectileSpec.BYTES)) return null;
    return ProjectileSpec.at(this.request.area + <usize>(index * ProjectileSpec.BYTES));
  }
  /** Answers the plan's first `count` entries; false (answering nothing) when they do not fit. */
  setPlanLength(count: i32): bool {
    if (!fits(this.request, 0, count * ProjectileSpec.BYTES)) return false;
    this.request.setResult(count);
    return true;
  }
  /** Answers `plan`, copied into the area; false (answering nothing) when it does not fit. */
  setPlan(plan: StaticArray<ProjectileSpec>): bool {
    if (!fits(this.request, 0, plan.length * ProjectileSpec.BYTES)) return false;
    for (let i = 0; i < plan.length; i++) {
      memory.copy(this.request.area + <usize>(i * ProjectileSpec.BYTES), changetype<usize>(unchecked(plan[i])), ProjectileSpec.BYTES);
    }
    this.request.setResult(plan.length);
    return true;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// --------------------------------------------------------- FatalDamage

/**
 * `FatalDamage` (appendix 4.3, preview): the death plan, at the entry of
 * the death arm of a unit or building a hit killed. The result is the
 * plan's bits (the constants here); `native` is the engine's own plan.
 */
@unmanaged
export class FatalDamageDecision {
  /** Passengers die with it and the death weapon may fire. */
  static readonly EXPLODE: i64 = 1;
  /** The death weapon fires (with `EXPLODE`). */
  static readonly DEATH_WEAPON: i64 = 1 << 1;
  /** Debris is thrown. */
  static readonly DEBRIS: i64 = 1 << 2;
  /** A building's survivors appear and a vehicle's passengers escape. */
  static readonly SURVIVORS: i64 = 1 << 3;
  /** A vehicle's crew escapes. */
  static readonly CREW: i64 = 1 << 4;
  /** The unit-lost announcement plays. */
  static readonly ANNOUNCE: i64 = 1 << 5;
  /** Tiberium remains: Riparius on and around the cell (Ares' `TiberiumRemains`; natively set only under Ares). */
  static readonly TIBERIUM_REMAINS: i64 = 1 << 6;

  /** `plan` with infantry death `infDeath` (0..255). */
  static withInfDeath(plan: i64, infDeath: u8): i64 {
    return (plan & ~(<i64>0xff << 8)) | ((<i64>infDeath) << 8);
  }
  /** `plan` firing weapon ID `weapon` as the death weapon (−1 for the type's own). */
  static withDeathWeapon(plan: i64, weapon: i32): i64 {
    return (plan & 0xffffffff) | (((<i64>weapon) + 1) << 32);
  }

  get victim(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** The killer; `NO_HANDLE` without one. */
  get killer(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }
  /** The killing hit's damage. */
  get damage(): i64 {
    return load<i64>(changetype<usize>(this), 72);
  }
  /** The killing warhead's ID, −1 when unknown. */
  get warhead(): i32 {
    return load<i32>(changetype<usize>(this), 88);
  }
  /** Position in leptons. */
  get x(): i32 {
    return load<i32>(changetype<usize>(this), 96);
  }
  get y(): i32 {
    return load<i32>(changetype<usize>(this), 100);
  }
  get z(): i32 {
    return load<i32>(changetype<usize>(this), 104);
  }

  /** `previous` with `bit` set or cleared. */
  with(bit: i64, on: bool): i64 {
    const plan = this.request.previous;
    return on ? plan | bit : plan & ~bit;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// --------------------------------------------------------- MissionStep

/**
 * `MissionStep` (appendix 4.5, preview): after a unit's or infantryman's
 * Guard, Area Guard, Hunt or Attack mission body ran. The result area
 * holds its outcome (the previous link's answer for a later link): change
 * what you want with the setters, each of which answers. Without an answer
 * the outcome stands.
 */
@unmanaged
export class MissionStepDecision {
  get unit(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** The mission whose body ran (`Mission` number). */
  get mission(): i64 {
    return load<i64>(changetype<usize>(this), 72);
  }
  /** The target the body left; `NO_HANDLE` for none or a cell. */
  get target(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }

  /** The outcome so far, in place (the body's, or the previous link's answer); `null` without an area. */
  get step(): MissionStepResult | null {
    if (!fits(this.request, 0, MissionStepResult.BYTES)) return null;
    return MissionStepResult.at(this.request.area);
  }
  /** Answers `step` whole. */
  setStep(step: MissionStepResult): bool {
    if (!fits(this.request, 0, MissionStepResult.BYTES)) return false;
    memory.copy(this.request.area, changetype<usize>(step), MissionStepResult.BYTES);
    this.request.setResult(0);
    return true;
  }
  /** Answers with the target `target`, an object or a cell (0 for none). */
  setTarget(target: Ref): bool {
    return this.answer64(0, target);
  }
  /** Answers with the destination `destination`, an object or a cell (0 for none). */
  setDestination(destination: Ref): bool {
    return this.answer64(8, destination);
  }
  /** Answers with `mission` to queue. */
  setMission(mission: Mission): bool {
    return this.answer32(16, <i32>mission);
  }
  /** Answers with no mission to queue. */
  setNoMission(): bool {
    return this.answer32(16, -1);
  }
  /** Answers with the frames until the body runs again (at least 1). */
  setDelay(frames: i32): bool {
    return this.answer32(20, frames);
  }

  private answer64(at: i32, value: i64): bool {
    if (!fits(this.request, at, 8)) return false;
    store<i64>(this.request.area + <usize>at, value);
    this.request.setResult(0);
    return true;
  }
  private answer32(at: i32, value: i32): bool {
    if (!fits(this.request, at, 4)) return false;
    store<i32>(this.request.area + <usize>at, value);
    this.request.setResult(0);
    return true;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// -------------------------------------------------------------- Launch

/** `PathKind` (appendix 8.3, enum 22). */
export enum PathKind {
  /** Control points after the launch position; ends on the last. */
  Bezier = 0,
  /** To the one point; `param0` is the apex height in leptons. */
  Parabola = 1,
  /** Along the line to the one point; `param0` radius in leptons, `param1` frames per turn. */
  Spiral = 2,
  /** Straight through each point in turn. */
  Waypoints = 3,
}

/** `PathSpec`'s header bytes; 24-byte points (three `i64` leptons) follow it. */
export const PATH_HEADER_BYTES: i32 = 32;
export const PATH_POINT_BYTES: i32 = 24;

/**
 * `Launch` (appendix 4.5, preview): the bullet's path, once, when a bullet
 * of a bound type is launched. Without an answer the bullet flies natively.
 */
@unmanaged
export class LaunchDecision {
  get projectile(): Projectile {
    return load<i64>(changetype<usize>(this), 48);
  }
  get launcher(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  get target(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }
  get targetCell(): Cell {
    return load<i64>(changetype<usize>(this), 64);
  }

  /**
   * Answers a path of `kind` through `points` (flat `x, y, z` leptons, 1 to
   * 16 points; exactly 1 for `Parabola` and `Spiral`) at `speed` leptons
   * per frame with 16 fraction bits (0 for the bullet's own). False
   * (answering nothing) when it does not fit the result area.
   */
  setPath(kind: PathKind, speed: i64, param0: i64, param1: i64, points: StaticArray<i64>): bool {
    const count = points.length / 3;
    const area = this.header(kind, speed, param0, param1, count);
    if (area == 0) return false;
    memory.copy(area + <usize>PATH_HEADER_BYTES, changetype<usize>(points), <usize>(count * PATH_POINT_BYTES));
    this.request.setResult(PATH_HEADER_BYTES + count * PATH_POINT_BYTES);
    return true;
  }
  /** As `setPath`, through the one point `x`, `y`, `z`. */
  setPathTo(kind: PathKind, speed: i64, param0: i64, param1: i64, x: i64, y: i64, z: i64): bool {
    const area = this.header(kind, speed, param0, param1, 1);
    if (area == 0) return false;
    store<i64>(area, x, PATH_HEADER_BYTES);
    store<i64>(area, y, PATH_HEADER_BYTES + 8);
    store<i64>(area, z, PATH_HEADER_BYTES + 16);
    this.request.setResult(PATH_HEADER_BYTES + PATH_POINT_BYTES);
    return true;
  }

  private header(kind: PathKind, speed: i64, param0: i64, param1: i64, count: i32): usize {
    if (!fits(this.request, 0, PATH_HEADER_BYTES + count * PATH_POINT_BYTES)) return 0;
    const area = this.request.area;
    store<i32>(area, <i32>kind, 0);
    store<i32>(area, count, 4);
    store<i64>(area, speed, 8);
    store<i64>(area, param0, 16);
    store<i64>(area, param1, 24);
    return area;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// ----------------------------------------------------- ProjectileFrame

/**
 * `ProjectileFrame` (appendix 4.5, preview): before a bound bullet's
 * candidate position is computed, every frame it moves. Read its `X`..`Z`,
 * `VX`..`VZ`, `Data0`, `Data1`; answer a step. Without an answer it moves
 * natively (or along its `Launch` path).
 */
@unmanaged
export class ProjectileFrameDecision {
  get projectile(): Projectile {
    return load<i64>(changetype<usize>(this), 48);
  }
  /** Frames since it was launched. */
  get frames(): i64 {
    return load<i64>(changetype<usize>(this), 72);
  }

  /** The previous link's step, when one answered (`null` otherwise). Read it before `newStep`. */
  get previousStep(): ProjectileStep | null {
    const d = this.request;
    if (d.previousLen <= 0 || !fits(d, 0, ProjectileStep.BYTES)) return null;
    return ProjectileStep.at(d.area);
  }
  /** Answers with a zeroed step in the area and returns it to fill in place; `null` (no answer) without room. */
  newStep(): ProjectileStep | null {
    if (!fits(this.request, 0, ProjectileStep.BYTES)) return null;
    const step = ProjectileStep.at(this.request.area);
    step.clear();
    this.request.setResult(0);
    return step;
  }
  /** Answers `step`, copied (the previous link's, if any, is replaced). */
  setStep(step: ProjectileStep): bool {
    if (!fits(this.request, 0, ProjectileStep.BYTES)) return false;
    memory.copy(this.request.area, changetype<usize>(step), ProjectileStep.BYTES);
    this.request.setResult(0);
    return true;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// ----------------------------------------------------------- MoveFrame

/**
 * `MoveFrame` (appendix 4.5, preview): each frame a bound vehicle or
 * infantryman's locomotor (Drive, Ship, Walk, Mech, Hover) could start a
 * step. Answer a `MoveStep`: `MOVE` takes the frame and moves it by the
 * displacement (the engine checks the cell it enters), `FACING` turns it,
 * `STOP` drops its destination. Without an answer, or without `MOVE`, its
 * locomotor moves it natively.
 */
@unmanaged
export class MoveFrameDecision {
  static readonly MOVE: i32 = 1;
  static readonly FACING: i32 = 2;
  static readonly STOP: i32 = 4;

  get unit(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** 1 while its locomotor is moving. */
  get moving(): bool {
    return load<i64>(changetype<usize>(this), 72) == 1;
  }
  /** Its NavCom object; `NO_HANDLE` for a cell or none. */
  get destination(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }
  /** Its NavCom cell; `NO_HANDLE` for an object or none. */
  get destinationCell(): Cell {
    return load<i64>(changetype<usize>(this), 64);
  }
  /** Position in leptons. */
  get x(): i32 {
    return load<i32>(changetype<usize>(this), 96);
  }
  get y(): i32 {
    return load<i32>(changetype<usize>(this), 100);
  }
  get z(): i32 {
    return load<i32>(changetype<usize>(this), 104);
  }

  /** The previous link's step (`null` for the first link, whose area is zero). Read it before `newStep`. */
  get previousStep(): MoveStep | null {
    const d = this.request;
    if (d.previousLen <= 0 || !fits(d, 0, MoveStep.BYTES)) return null;
    return MoveStep.at(d.area);
  }
  /** Answers with a zeroed step in the area and returns it to fill in place; `null` (no answer) without room. */
  newStep(): MoveStep | null {
    if (!fits(this.request, 0, MoveStep.BYTES)) return null;
    const step = MoveStep.at(this.request.area);
    step.clear();
    this.request.setResult(0);
    return step;
  }
  /** Answers `step`, copied (the previous link's is replaced). */
  setStep(step: MoveStep): bool {
    if (!fits(this.request, 0, MoveStep.BYTES)) return false;
    memory.copy(this.request.area, changetype<usize>(step), MoveStep.BYTES);
    this.request.setResult(0);
    return true;
  }
  /** Takes the frame and moves by `dx`, `dy` leptons. */
  stepBy(dx: i32, dy: i32): bool {
    const step = this.newStep();
    if (step == null) return false;
    step.flags = MoveFrameDecision.MOVE;
    step.dx = dx;
    step.dy = dy;
    return true;
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// ----------------------------------------------------------- PathFound

/**
 * `PathFound` (appendix 4.5, preview): when a path search's route is
 * adopted. The area holds the route's cells (up to 24), the first link the
 * native ones; `native` is their count, 0 for no route. Answer up to 24
 * cells, each next to the one before (the first next to the unit's cell),
 * or no route. The route is edited in place: writing `cell(i)` replaces
 * `previousCell(i)`.
 */
@unmanaged
export class PathFoundDecision {
  /** Most cells a route holds. */
  static readonly MAX_CELLS: i32 = 24;

  get unit(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** The search's goal cell. */
  get goal(): Cell {
    return load<i64>(changetype<usize>(this), 64);
  }
  /** Position in leptons. */
  get x(): i32 {
    return load<i32>(changetype<usize>(this), 96);
  }
  get y(): i32 {
    return load<i32>(changetype<usize>(this), 100);
  }
  get z(): i32 {
    return load<i32>(changetype<usize>(this), 104);
  }

  /** The previous link's route length. */
  get previousCount(): i32 {
    return previousEntries(this.request, PathCell.BYTES);
  }
  /** Cell `index` of the previous link's route, or `null` past it. */
  previousCell(index: i32): PathCell | null {
    if (index < 0 || index >= this.previousCount) return null;
    return PathCell.at(this.request.area + <usize>(index * PathCell.BYTES));
  }
  /** Cell `index` of the answer, written in place; `null` when it does not fit. */
  cell(index: i32): PathCell | null {
    if (!fits(this.request, index * PathCell.BYTES, PathCell.BYTES)) return null;
    return PathCell.at(this.request.area + <usize>(index * PathCell.BYTES));
  }
  /** Answers the route's first `count` cells; false (answering nothing) when they do not fit. */
  setCellCount(count: i32): bool {
    if (!fits(this.request, 0, count * PathCell.BYTES)) return false;
    this.request.setResult(count);
    return true;
  }
  /** Answers the route `xy` (flat `x, y` cell pairs), copied; false (answering nothing) when it does not fit. */
  setCells(xy: StaticArray<i32>): bool {
    const count = xy.length / 2;
    if (!fits(this.request, 0, count * PathCell.BYTES)) return false;
    memory.copy(this.request.area, changetype<usize>(xy), <usize>(count * PathCell.BYTES));
    this.request.setResult(count);
    return true;
  }
  /** Answers no route: the search fails. */
  noRoute(): void {
    this.request.setResult(0);
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}

// ------------------------------------------------------- ApproachPoint

/**
 * `ApproachPoint` (appendix 4.5, preview): when a unit approaching its
 * target has chosen the cell to move to, before it is set as the
 * destination. The result is a cell reference on the map; anything else
 * (0 included) keeps the previous link's cell, the native one (`native`)
 * for the first.
 */
@unmanaged
export class ApproachPointDecision {
  get unit(): Techno {
    return load<i64>(changetype<usize>(this), 24);
  }
  /** The cell the engine chose. */
  get chosen(): Cell {
    return load<i64>(changetype<usize>(this), 64);
  }
  /** The target object; `NO_HANDLE` for a cell target. */
  get target(): Techno {
    return load<i64>(changetype<usize>(this), 32);
  }
  /** The target's cell. */
  get targetCell(): Cell {
    return load<i64>(changetype<usize>(this), 80);
  }
  /** Position in leptons. */
  get x(): i32 {
    return load<i32>(changetype<usize>(this), 96);
  }
  get y(): i32 {
    return load<i32>(changetype<usize>(this), 100);
  }
  get z(): i32 {
    return load<i32>(changetype<usize>(this), 104);
  }

  /** Heads for `cell` instead. */
  setCell(cell: Cell): void {
    this.request.setResult(cell);
  }

  /** The whole record. */
  get record(): EventRecord {
    return changetype<EventRecord>(this);
  }
  /** The native result, before any module. */
  get native(): i64 {
    return this.request.native;
  }
  /** The result so far: the previous link's, or the native one. */
  get previous(): i64 {
    return this.request.previous;
  }
  /** Answers `value` (and marks the request handled). */
  setResult(value: i64): void {
    this.request.setResult(value);
  }
  /** The whole request. */
  get request(): DecideRecord {
    return changetype<DecideRecord>(this);
  }
}
