// The state the parts share. Module memory is saved and hashed with the
// match, so plain top-level values do.

import { Cell, House, NO_HANDLE, Techno } from "@vera20k/sdk";
import { MAX_COMPANY, MAX_RAIDERS } from "../../data/index";

export const WAITING: i64 = 0;
export const MUSTER: i64 = 1;
export const BATTLE: i64 = 2;
export const OVER: i64 = 3;

export let phase: i64 = WAITING;
let playerHouse: House = NO_HANDLE;
let enemyHouse: House = NO_HANDLE;
let campCell: Cell = NO_HANDLE;

export function setPhase(value: i64): void {
  phase = value;
}

export function player(): House {
  return playerHouse;
}

export function enemy(): House {
  return enemyHouse;
}

/** The middle cell of the offer row; everything is placed from it. */
export function camp(): Cell {
  return campCell;
}

export function setSides(player: House, enemy: House, camp: Cell): void {
  playerHouse = player;
  enemyHouse = enemy;
  campCell = camp;
}

/** The wave the player faces next, from 1. `campaign` writes it. */
export let wave: i64 = 1;
/** Banners left; none is the end. `campaign` writes it. */
export let banners: i64 = 0;
/** The camp's rank, 0 to 2. `shop` writes it. */
export let campRank: i64 = 0;
/** Supply picks the player has yet to make. `campaign` adds for waves, `shop` for merges and takes them away. */
export let picks: i64 = 0;

export function setWave(value: i64): void {
  wave = value;
}
export function setBanners(value: i64): void {
  banners = value;
}
export function setCampRank(value: i64): void {
  campRank = value;
}
export function setPicks(value: i64): void {
  picks = value;
}

// One hired mercenary per place. It outlives its unit: the dead return
// next muster.
/** Index in `MERCS`; −1 for an empty place. */
export const memberKind = new StaticArray<i64>(MAX_COMPANY);
/** 1 for a champion. */
export const memberChampion = new StaticArray<i64>(MAX_COMPANY);
/** Its place in the line, a cell reference. */
export const memberHome = new StaticArray<i64>(MAX_COMPANY);
/** Its unit on the field, or `NO_HANDLE`. */
export const memberUnit = new StaticArray<i64>(MAX_COMPANY);
for (let at = 0; at < MAX_COMPANY; at++) memberKind[at] = -1;

export function isEmpty(at: i32): bool {
  return memberKind[at] < 0;
}

export function isChampion(at: i32): bool {
  return memberChampion[at] == 1;
}

export function kindOf(at: i32): i32 {
  const kind = memberKind[at];
  return kind < 0 ? 0 : <i32>kind;
}

/** Member `at`'s unit, or `NO_HANDLE`. */
export function unitOf(at: i32): Techno {
  return memberUnit[at];
}

export function clearMember(at: i32): void {
  memberKind[at] = -1;
  memberChampion[at] = 0;
  memberHome[at] = NO_HANDLE;
  memberUnit[at] = NO_HANDLE;
}

/** The raiders on the field, or `NO_HANDLE`. `battle` writes it. */
export const raiders = new StaticArray<i64>(MAX_RAIDERS);
