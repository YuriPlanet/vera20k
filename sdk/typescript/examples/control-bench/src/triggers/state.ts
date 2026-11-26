// The `triggers` area's state (the Rust `triggers.rs` slots, `ld`/`st`):
// handles, type references and readings kept between frames, in module
// memory, so a loaded game keeps them. One file the area's parts share.
import { Action, Cell, NO_HANDLE, Prop, Status, Techno, get, lastStatus } from "@vera20k/sdk";
import { alive, offset, player } from "../bench";

export const OPENED: i32 = 0;
export const ARENA: i32 = 1;
// Type references (0: not in the rules).
export const WEAPON_105: i32 = 2;
export const WARHEAD_AP: i32 = 3;
export const PROJECTILE_CANNON: i32 = 4;
/** The `PIFF` animation's ID plus one. */
export const ANIM: i32 = 5;
export const SUPER: i32 = 6;
export const TYPE_FV: i32 = 7;
export const TYPE_HTNK: i32 = 8;
export const TYPE_E1: i32 = 9;
export const TYPE_E2: i32 = 10;
export const TYPE_ENGINEER: i32 = 11;
// Units.
export const SHOOTER: i32 = 12;
export const VICTIM: i32 = 13;
export const DUMMY: i32 = 14;
export const FOOT: i32 = 15;
export const KILLED: i32 = 16;
export const MOVER: i32 = 17;
export const CONVERTED: i32 = 18;
export const IFV: i32 = 19;
export const RIDER: i32 = 20;
export const SWITCHED: i32 = 21;
export const BAIT: i32 = 22;
export const ORDERED_1: i32 = 23;
export const ORDERED_2: i32 = 24;
export const YURI: i32 = 25;
export const CONTROLLED: i32 = 26;
export const MIRAGE: i32 = 27;
export const DOG: i32 = 28;
// Triggers.
export const TR_TIMER: i32 = 29;
export const TR_WEAPON: i32 = 30;
export const TR_WARHEAD: i32 = 31;
export const TR_PROJECTILE: i32 = 32;
export const TR_UNBIND: i32 = 33;
export const TR_HOUSE_NO: i32 = 34;
export const TR_SET_VAR: i32 = 35;
export const TR_TRANSFERRED: i32 = 36;
// Readings and progress.
export const PING_HEALTH: i32 = 37;
export const PING_MONEY: i32 = 38;
export const MODIFIER: i32 = 39;
export const CELL_ROW: i32 = 40;
export const CELL_COLUMN: i32 = 41;
export const MOVER_START: i32 = 42;
export const CLAIMED: i32 = 43;
export const SUPER_GRANTED: i32 = 44;
export const SUBJECT_YES: i32 = 45;
export const SUBJECT_NO: i32 = 46;
export const OTHER_YES: i32 = 47;
export const OTHER_NO: i32 = 48;
export const HOUSE_YES: i32 = 49;
export const HOUSE_NO: i32 = 50;
export const UNBIND_YES: i32 = 51;
export const UNBIND_NO: i32 = 52;
/** The unit `Remove` took, checked on a later frame. */
export const REMOVED: i32 = 53;
// TeamTypes (type references) and the types their `Birth` is bound to.
export const TEAM: i32 = 54;
export const CHRONO_TEAM: i32 = 55;
export const TYPE_TNKD: i32 = 56;
export const TYPE_SNIPE: i32 = 57;
export const TYPE_GAP: i32 = 58;
/** Which reinforcement the next Tank Destroyer's `Birth` belongs to. */
export const TEAM_PHASE: i32 = 59;
/** The module events this area registers (the Rust `EventSlot`s): −1 until registered. */
export const PING: i32 = 60;
export const PONG: i32 = 61;
export const GLOBAL_EVENT: i32 = 62;
const SLOTS: i32 = 63;

const STATE = new StaticArray<i64>(SLOTS);
STATE[PING] = -1;
STATE[PONG] = -1;
STATE[GLOBAL_EVENT] = -1;

export function ld(slot: i32): i64 {
  return unchecked(STATE[slot]);
}

export function st(slot: i32, value: i64): void {
  unchecked((STATE[slot] = value));
}

/** The cell `dx`, `dy` from the arena's corner. */
export function at(dx: i32, dy: i32): Cell {
  return offset(ld(ARENA), dx, dy);
}

export function unit(slot: i32): Techno {
  return ld(slot);
}

export function have(slot: i32): bool {
  return ld(slot) != NO_HANDLE;
}

/** An event slot's ID (−1 for none). */
export function eventId(slot: i32): i32 {
  return <i32>ld(slot);
}

/** A unit's health, −1 once it is gone. */
export function health(t: Techno): i64 {
  const v = get(t, Prop.Health);
  return lastStatus() == Status.Ok ? v : -1;
}

/** The player's money, −1 when it cannot be read. */
export function money(): i64 {
  const v = get(player(), Prop.Money);
  return lastStatus() == Status.Ok ? v : -1;
}

/** Removes one of this area's units once its part is done. */
export function retire(slot: i32): void {
  if (have(slot) && alive(unit(slot))) Action.remove(unit(slot));
}
