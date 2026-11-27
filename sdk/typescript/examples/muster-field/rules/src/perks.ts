// Company-wide supplies and what a champion is worth. (Owner: content)
//
// Stacks last the campaign. What they do to units lives one battle: units
// are fresh each muster, and each effect has a modifier group of its own,
// so applying one again replaces it.

import {
  Action, FIXED_ONE, Modifier, ModifierOp, NO_HANDLE, Prop, Rules, Status, Techno,
  frame, get, lastStatus, random, set,
} from "@vera20k/sdk";
import * as content from "../../data/content";
import * as table from "../../data/perks";
import { MAX_COMPANY, MAX_RAIDERS } from "../../data/index";
import * as state from "./state";
import * as world from "./world";

export const PERKS: i32 = table.PERKS;

const SECOND: i64 = 15;
/** How long the horn holds: the approach and the first exchange. */
const HORN_FRAMES: i32 = 20 * <i32>SECOND;
const HEAL_EVERY: i64 = 3 * SECOND;
const SHELL_EVERY: i64 = 6 * SECOND;

// One modifier group per effect and property: a group holds one modifier
// on an object, whatever its property.
const GROUP_CHAMPION_FIRE: i32 = 4101;
const GROUP_CHAMPION_ARMOR: i32 = 4102;
const GROUP_PLATES: i32 = 4103;
const GROUP_BARRELS: i32 = 4104;
const GROUP_HORN: i32 = 4105;
const GROUP_GRUDGE_FIRE: i32 = 4106;
const GROUP_LAST_FIRE: i32 = 4107;
const GROUP_LAST_ARMOR: i32 = 4108;

const TAKEN = new StaticArray<i64>(table.PERKS);
// This battle: its first frame, who was fielded, how many of them fell and
// whether the last of them made the stand.
let started: i64 = 0;
let fieldedCount: i64 = 0;
let fallenCount: i64 = 0;
let stood: i64 = 0;

function taken(perk: i32): i64 {
  if (perk < 0 || perk >= table.PERKS) return 0;
  const count = TAKEN[perk];
  const most = table.TABLE[perk].most;
  return count < 0 ? 0 : count > most ? most : count;
}

export function name(perk: i32): string {
  return table.name(perk);
}

/** A few words for the choice line: what taking it now adds. */
export function effect(perk: i32): string {
  return table.effect(perk, taken(perk));
}

/** How often supply `perk` was taken, for the interface. */
export function takenOf(perk: i32): i64 {
  return taken(perk);
}

/** `choices`' draw: up to three different supplies, −1 for none. */
export const drawn = new StaticArray<i32>(3);
const OPEN = new StaticArray<i32>(table.PERKS);

/** Draws up to three different supplies that can still be taken into `drawn`; whether any was. */
export function choices(): bool {
  let count = 0;
  for (let perk = 0; perk < table.PERKS; perk++) {
    if (taken(perk) < table.TABLE[perk].most) {
      OPEN[count] = perk;
      count++;
    }
  }
  for (let at = 0; at < 3; at++) drawn[at] = -1;
  for (let at = 0; at < 3 && at < count; at++) {
    let pick = random(at, count - 1);
    if (pick < 0) pick = 0;
    pick = min(pick, count - 1);
    pick = max(pick, at);
    const swap = OPEN[at];
    OPEN[at] = OPEN[pick];
    OPEN[pick] = swap;
    drawn[at] = OPEN[at];
  }
  return count > 0;
}

export function grant(perk: i32): void {
  if (perk < 0 || perk >= table.PERKS) return;
  if (TAKEN[perk] >= table.TABLE[perk].most) return;
  TAKEN[perk] += 1;
}

/** The rank member `at`'s fresh unit gets: 0 rookie, 1 veteran, 2 elite. */
export function rank(at: i32): i64 {
  if (state.isEmpty(at)) return 0;
  const merc = content.MERCS[state.kindOf(at)];
  if (state.isChampion(at)) return merc.champion.rank;
  return <i64>merc.tier < taken(table.DRILL) ? 1 : 0;
}

/** Member `at`'s unit if it is on the field and alive, else `NO_HANDLE`. */
function fielded(at: i32): Techno {
  if (state.isEmpty(at)) return NO_HANDLE;
  const unit = state.unitOf(at);
  if (unit == NO_HANDLE || !world.alive(unit)) return NO_HANDLE;
  return unit;
}

function promote(at: i32, unit: Techno): void {
  const due = rank(at);
  const now = get(unit, Prop.Rank);
  if (lastStatus() == Status.Ok && now < due) set(unit, Prop.Rank, due);
}

/** Multiplies `prop` by `percent` until the unit is gone. */
function scale(unit: Techno, prop: Prop, percent: i64, group: i32): void {
  scaleFor(unit, prop, percent, -1, group);
}

function scaleFor(unit: Techno, prop: Prop, percent: i64, frames: i32, group: i32): void {
  if (percent != 100) {
    Modifier.create(unit, prop, ModifierOp.Multiply, (percent * FIXED_ONE) / 100, frames, 0, group);
  }
}

function spark(unit: Techno, anim: string): void {
  const id = Rules.anim(anim);
  if (lastStatus() == Status.Ok) Action.attachAnim(unit, id);
}

/** After the raiders are placed and before anyone is ordered. */
export function battleStarted(): void {
  started = <i64>frame();
  fallenCount = 0;
  stood = 0;
  let count: i64 = 0;
  for (let at = 0; at < MAX_COMPANY; at++) {
    const unit = fielded(at);
    if (unit == NO_HANDLE) continue;
    count++;
    outfit(at, unit);
  }
  fieldedCount = count;
  barrage();
}

function outfit(at: i32, unit: Techno): void {
  if (state.isEmpty(at)) return;
  const merc = content.MERCS[state.kindOf(at)];
  promote(at, unit);
  if (state.isChampion(at)) {
    scale(unit, Prop.FirepowerMultiplier, merc.champion.fire, GROUP_CHAMPION_FIRE);
    scale(unit, Prop.ArmorMultiplier, merc.champion.armor, GROUP_CHAMPION_ARMOR);
  }
  if ((merc.tags & content.VEHICLE) != 0) {
    const percent = 100 + 40 * taken(table.PLATES);
    scale(unit, Prop.ArmorMultiplier, percent, GROUP_PLATES);
  }
  if ((merc.tags & content.ASSAULT) != 0) {
    const percent = 100 + 60 * taken(table.HORN);
    scaleFor(unit, Prop.ArmorMultiplier, percent, HORN_FRAMES, GROUP_HORN);
  }
  const cells = taken(table.BARRELS);
  if (cells > 0) {
    Modifier.create(unit, Prop.WeaponRangeExtra, ModifierOp.Add, 256 * cells, -1, 0, GROUP_BARRELS);
  }
}

function barrage(): void {
  const left = 100 - 15 * taken(table.BARRAGE);
  if (left == 100) return;
  for (let at = 0; at < MAX_RAIDERS; at++) {
    const unit = state.raiders[at];
    if (unit == NO_HANDLE) continue;
    const full = get(unit, Prop.Strength);
    const fullRead = lastStatus() == Status.Ok;
    const now = get(unit, Prop.Health);
    if (!fullRead || lastStatus() != Status.Ok) continue;
    const to = max<i64>((full * left) / 100, 1);
    if (to < now && set(unit, Prop.Health, to) == Status.Ok) spark(unit, "EXPLOSML");
  }
}

/** Every frame of a battle: work once a second. */
export function battleTick(now: i32): void {
  const since = <i64>now - started;
  if (since <= 0 || since % SECOND != 0) return;
  const medics = taken(table.MEDICS);
  const guns = taken(table.GUNS);
  const watched = taken(table.GRUDGE) > 0 || (taken(table.LAST_STAND) > 0 && stood == 0);
  const heals = medics > 0 && since % HEAL_EVERY == 0;
  if (watched || heals) {
    let living: i64 = 0;
    let last: Techno = NO_HANDLE;
    for (let at = 0; at < MAX_COMPANY; at++) {
      const unit = fielded(at);
      if (unit == NO_HANDLE) continue;
      living++;
      last = unit;
      if (heals) heal(unit, 4 * medics);
    }
    if (watched) fell(living, last);
  }
  if (guns > 0 && since % SHELL_EVERY == 0) shell(guns);
}

function heal(unit: Techno, percent: i64): void {
  const full = get(unit, Prop.Strength);
  const fullRead = lastStatus() == Status.Ok;
  const now = get(unit, Prop.Health);
  if (!fullRead || lastStatus() != Status.Ok) return;
  if (now < full) {
    let amount = (full * percent) / 100;
    amount = amount < 1 ? 1 : amount > <i64>i32.MAX_VALUE ? <i64>i32.MAX_VALUE : amount;
    Action.heal(unit, <i32>amount);
  }
}

function fell(living: i64, last: Techno): void {
  const fallen = max<i64>(fieldedCount - living, 0);
  if (fallen > fallenCount) {
    fallenCount = fallen;
    const percent = 100 + 20 * taken(table.GRUDGE) * fallen;
    for (let at = 0; at < MAX_COMPANY; at++) {
      const unit = fielded(at);
      if (unit != NO_HANDLE) scale(unit, Prop.FirepowerMultiplier, percent, GROUP_GRUDGE_FIRE);
    }
  }
  // Someone has to have fallen: a company of one makes no stand.
  if (last == NO_HANDLE || living != 1 || fallen <= 0) return;
  const unit = last;
  if (taken(table.LAST_STAND) > 0 && stood == 0) {
    stood = 1;
    const full = get(unit, Prop.Strength);
    if (lastStatus() == Status.Ok) set(unit, Prop.Health, full);
    scale(unit, Prop.FirepowerMultiplier, 200, GROUP_LAST_FIRE);
    scale(unit, Prop.ArmorMultiplier, 200, GROUP_LAST_ARMOR);
    spark(unit, "RING1");
  }
}

/** A shell on the raider with the most health, through armor of every kind. */
function shell(guns: i64): void {
  let target: Techno = NO_HANDLE;
  let most: i64 = 0;
  for (let at = 0; at < MAX_RAIDERS; at++) {
    const unit = state.raiders[at];
    if (unit == NO_HANDLE) continue;
    const hp = get(unit, Prop.Health);
    if (lastStatus() != Status.Ok || hp <= 0) continue;
    // Of equals the last, as Rust's `max_by_key` picks.
    if (target == NO_HANDLE || hp >= most) {
      target = unit;
      most = hp;
    }
  }
  const warhead = Rules.warhead("Super");
  if (target == NO_HANDLE || warhead == NO_HANDLE) return;
  spark(target, "EXPLOMED");
  Action.damage(target, <i32>(100 * guns), warhead);
}

/** Extra pay for a finished wave, won or lost. */
export function bonusPay(kills: i64, won: bool): i32 {
  const held = kills < 0 ? 0 : kills > <i64>MAX_RAIDERS ? <i64>MAX_RAIDERS : kills;
  return <i32>(taken(table.LEDGER) * (100 + 15 * held));
}
