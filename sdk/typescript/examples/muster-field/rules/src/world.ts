// What several parts ask of the map and the engine.

import {
  Action, Cell, Find, House, Modifier, NO_HANDLE, Prop, Rules, Status, Techno, get, lastStatus,
} from "@vera20k/sdk";
import * as state from "./state";

function clamp(value: i32, lo: i32, hi: i32): i32 {
  return value < lo ? lo : value > hi ? hi : value;
}

export function offset(cell: Cell, dx: i32, dy: i32): Cell {
  const x = clamp(Cell.x(cell) + dx, 1, 510);
  const y = clamp(Cell.y(cell) + dy, 1, 510);
  return Cell.at(x, y);
}

/** `to` minus `from` on the x axis. */
export function deltaX(from: Cell, to: Cell): i32 {
  return Cell.x(to) - Cell.x(from);
}

/** `to` minus `from` on the y axis. */
export function deltaY(from: Cell, to: Cell): i32 {
  return Cell.y(to) - Cell.y(from);
}

/** The cell `unit` stands on, or `NO_HANDLE`. */
export function cellOf(unit: Techno): Cell {
  const cell = get(unit, Prop.Cell);
  return lastStatus() == Status.Ok ? cell : NO_HANDLE;
}

export function alive(unit: Techno): bool {
  if (unit == NO_HANDLE) return false;
  const health = get(unit, Prop.Health);
  return lastStatus() == Status.Ok && health > 0;
}

/** Creates a unit on `spot` or the closest ring of cells that takes it; `NO_HANDLE` when none does. */
export function createNear(spot: Cell, id: string, house: House): Techno {
  const ty = Rules.technoType(id);
  if (ty == NO_HANDLE) return NO_HANDLE;
  for (let ring = 0; ring <= 5; ring++) {
    for (let dy = -ring; dy <= ring; dy++) {
      for (let dx = -ring; dx <= ring; dx++) {
        if (max(abs(dx), abs(dy)) != ring) continue;
        const unit = Action.createObject(offset(spot, dx, dy), ty, house);
        if (lastStatus() == Status.Ok) return unit;
      }
    }
  }
  return NO_HANDLE;
}

const FOUND = new StaticArray<i64>(64);

/** Removes everything `house` has on the map, strays such as a tank's crew included. */
export function clearForces(house: House): void {
  let after: i64 = 0;
  while (true) {
    const page = Find.objects().ownedBy(house).page(FOUND, after);
    if (lastStatus() != Status.Ok) return;
    const count = page.count;
    const next = page.next;
    for (let at = 0; at < count; at++) Action.remove(FOUND[at]);
    if (next == 0) return;
    after = next;
  }
}

export function credits(): i64 {
  const money = get(state.player(), Prop.Money);
  return lastStatus() == Status.Ok ? money : 0;
}

/** Pays the player; a negative amount charges them. */
export function pay(amount: i32): bool {
  return Action.addMoney(state.player(), amount) == Status.Ok;
}

/** What keeps a unit still and harmless: its speed modifier (`heldFire` the weapon one). */
export let heldSpeed: i64 = NO_HANDLE;
export let heldFire: i64 = NO_HANDLE;

/** Holds `unit`; the two modifiers are `heldSpeed` and `heldFire` after. */
export function hold(unit: Techno): void {
  const speed = Modifier.multiply(unit, Prop.SpeedMultiplier, 0, -1);
  heldSpeed = lastStatus() == Status.Ok ? speed : NO_HANDLE;
  const fire = Modifier.overrideWith(unit, Prop.DisableWeapons, 1, -1);
  heldFire = lastStatus() == Status.Ok ? fire : NO_HANDLE;
}

export function release(speed: i64, fire: i64): void {
  if (speed != NO_HANDLE) Modifier.remove(speed);
  if (fire != NO_HANDLE) Modifier.remove(fire);
}
