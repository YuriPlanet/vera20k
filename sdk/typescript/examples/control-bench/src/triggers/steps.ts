// The actions, claims and orders (the Rust `triggers/steps.rs`), one group
// per scheduled frame after the arena opened; each checks what it changed
// where that is a read.
import {
  Action, Blocked, Cell, Command, FIXED_ONE, Global, House, ModifierOp, Mission, NO_HANDLE, Order, Prop,
  Status, Techno, TriggerRef, Var, get, lastStatus, set,
} from "@vera20k/sdk";
import { alive, benchFail, check, enemy, is, mark, marked, player, spawn } from "../bench";
import {
  ANIM, CLAIMED, CONTROLLED, CONVERTED, DOG, DUMMY, FOOT, GLOBAL_EVENT, IFV, KILLED, MIRAGE, MODIFIER, MOVER,
  MOVER_START, ORDERED_1, ORDERED_2, OTHER_NO, OTHER_YES, HOUSE_NO, HOUSE_YES, PING, PING_HEALTH, PING_MONEY,
  REMOVED, RIDER, SHOOTER, SUBJECT_NO, SUBJECT_YES, SUPER, SUPER_GRANTED, SWITCHED, TR_HOUSE_NO, TR_UNBIND,
  TYPE_E1, TYPE_E2, TYPE_FV, TYPE_HTNK, UNBIND_NO, UNBIND_YES, VICTIM, WARHEAD_AP, WEAPON_105, YURI,
  at, eventId, have, health, ld, money, retire, st, unit,
} from "./state";

/** The work of frame `t` after the arena opened. */
export function stepsStep(t: i64): void {
  if (t == 6) claim();
  else if (t == 8) stockActions();
  else if (t == 9) releaseClaim();
  else if (t == 10) damage();
  else if (t == 12) yuriAttack();
  else if (t == 14) heal();
  else if (t == 18) setProperty();
  else if (t == 22) detonateWarhead();
  else if (t == 26) detonateWeapon();
  else if (t == 30) raisePing();
  else if (t == 32) raiseGlobal();
  else if (t == 34) addMoney();
  else if (t == 36) score();
  else if (t == 38) allowType();
  else if (t == 40) animations();
  else if (t == 42) logAction();
  else if (t == 44) modify();
  else if (t == 50) putDog();
  else if (t == 60) removeModifiers();
  else if (t == 64) addExperience();
  else if (t == 66) ammoAndRemove();
  else if (t == 68) removedCheck();
  else if (t == 70) kill();
  else if (t == 72) killByDamage();
  else if (t == 74) setOwner();
  else if (t == 78) retire(SWITCHED);
  else if (t == 90) convert();
  else if (t == 96) convertFor();
  else if (t == 100) addPassenger();
  else if (t == 105) bindFoot();
  else if (t == 106) unbindFoot();
  else if (t == 110) filterSubjectHits();
  else if (t == 120) filterSubjectAndUnbindCheck();
  else if (t == 124) filterOtherHits();
  else if (t == 130) unload();
  else if (t == 134) filterOtherCheck();
  else if (t == 140) convertBackCheck();
  else if (t == 150) clearTarget();
  else if (t == 160) filterHouseCheck();
  else if (t == 200) {
    moveMover();
    order(Command.Move);
  } else if (t == 260) order(Command.Guard);
  else if (t == 300) clearDisguise();
  else if (t == 320) order(Command.Stop);
  else if (t == 380) order(Command.Scatter);
  else if (t == 420) teleport();
  else if (t == 440) order(Command.TakeProduct);
  else if (t == 500) {
    retire(ORDERED_1);
    retire(ORDERED_2);
  } else if (t == 2400) order(Command.QueueProduction);
  else if (t == 900) {
    retire(YURI);
    retire(CONTROLLED);
    retire(VICTIM);
  }
  // Repeated until they answer.
  if (t >= 6 && t <= 400 && t % 3 == 0) fireWeapon();
  if (t >= 8 && t <= 1800 && have(SUPER) && ld(SUPER_GRANTED) != 0) {
    const phase = t % 30;
    if (phase == 8) Action.superWeaponSetCharge(player(), ld(SUPER), 100);
    else if (phase == 10) launchSuperWeapon();
  }
  if (t >= 210 && t <= 400 && t % 10 == 0) setMissionCheck();
  if (t >= 430 && t <= 600 && t % 10 == 0) teleportCheck();
}

// ----------------------------------------------------------------- Claims

/**
 * Claims `SuperWeapons` (bit 3) on the computer house, which leaves its
 * production and forces to the native AI; a human house takes no claim.
 */
function claim(): void {
  const e = enemy();
  if (set(e, Prop.ControlDomains, 8) == Status.Ok && is(e, Prop.ControlDomains, 8)) {
    st(CLAIMED, 1);
  } else {
    benchFail("triggers.claim", "the claim did not read back");
  }
  const p = player();
  const status = set(p, Prop.ControlDomains, 8);
  if (status == Status.Refused) {
    mark("triggers.claim-human");
  } else if (status == Status.Ok) {
    set(p, Prop.ControlDomains, 0);
    benchFail("triggers.claim-human", "a human house took a claim");
  } else {
    benchFail("triggers.claim-human", "wrong status");
  }
}

/** Three frames later: the claim goes again. */
function releaseClaim(): void {
  if (ld(CLAIMED) == 0) return;
  const e = enemy();
  const released = set(e, Prop.ControlDomains, 0) == Status.Ok && is(e, Prop.ControlDomains, 0);
  check("triggers.claim", released, "the release did not read back");
}

// ----------------------------------------------------------------- Stock

/** `Global.missionTimerRunning()`, `otherwise` when it does not read. */
function timerRunning(otherwise: bool): bool {
  const v = get(Global.REF, Prop.MissionTimerRunning);
  return lastStatus() == Status.Ok ? v != 0 : otherwise;
}

/**
 * Stock actions through `action`: the mission timer (27 set, 23 start, 24
 * stop), a local variable (56 set, 57 clear), and 16 (reveal the map).
 */
function stockActions(): void {
  const started = Action.timerSet(60) == Status.Ok && Action.timerStart() == Status.Ok;
  const running = timerRunning(false);
  const stopped = Action.timerStop() == Status.Ok;
  const idle = !timerRunning(true);
  check("triggers.stock-timer", started && running && stopped && idle, "the mission timer did not run and stop");
  const local = Var.local(88);
  const setOk = Action.setLocal(88) == Status.Ok && local.get() == 1;
  const cleared = Action.clearLocal(88) == Status.Ok && local.get() == 0;
  check("triggers.stock-local", setOk && cleared, "local 88 did not read back");
  check("triggers.stock-reveal", Action.revealAll() == Status.Ok, "stock 16 answered an error");
}

// ---------------------------------------------------- The dummy's actions

function damage(): void {
  if (!have(DUMMY) || !have(WARHEAD_AP)) return;
  const dummy = unit(DUMMY);
  const before = health(dummy);
  const ok = Action.damage(dummy, 150, ld(WARHEAD_AP)) == Status.Ok;
  const after = health(dummy);
  check("triggers.act-damage", ok && after >= 0 && after < before, "health did not drop");
}

function heal(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !alive(dummy)) return;
  set(dummy, Prop.Health, 200);
  const ok = Action.heal(dummy, 50) == Status.Ok;
  check("triggers.act-heal", ok && health(dummy) == 250, "health did not rise by the amount");
}

function setProperty(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !alive(dummy)) return;
  const ok = Action.setProperty(dummy, Prop.Health, 350) == Status.Ok;
  check("triggers.act-set-property", ok && health(dummy) == 350, "Health did not read back");
}

function detonateWarhead(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !have(WARHEAD_AP) || !alive(dummy)) return;
  const before = health(dummy);
  const ok = Action.detonateWarhead(dummy, ld(WARHEAD_AP), 60) == Status.Ok;
  check("triggers.act-detonate-warhead", ok && health(dummy) < before, "the warhead did not hurt the dummy");
}

function detonateWeapon(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !have(WEAPON_105) || !alive(dummy)) return;
  set(dummy, Prop.Health, 380);
  const ok = Action.detonateWeapon(dummy, ld(WEAPON_105), NO_HANDLE) == Status.Ok;
  check("triggers.act-detonate-weapon", ok && health(dummy) < 380, "the weapon did not hurt the dummy");
}

/**
 * Raises the object module event the dummy's chain trigger hears; the
 * trigger heals it by 30 and pays the player 7 at once (`records`).
 */
function raisePing(): void {
  const dummy = unit(DUMMY);
  const ping = eventId(PING);
  if (!have(DUMMY) || ping < 0 || !alive(dummy)) return;
  set(dummy, Prop.Health, 300);
  st(PING_HEALTH, health(dummy));
  st(PING_MONEY, money());
  check("triggers.act-raise-event", Action.raiseEvent(ping, dummy, NO_HANDLE, 1, 2) == Status.Ok, "RaiseEvent answered an error");
}

/** Raises the global module event. */
function raiseGlobal(): void {
  const event = eventId(GLOBAL_EVENT);
  if (event >= 0 && Action.raiseEvent(event, Global.REF, NO_HANDLE, 3, 4) != Status.Ok) {
    benchFail("triggers.ev-module-global", "RaiseEvent answered an error");
  }
}

function addMoney(): void {
  const before = money();
  const ok = Action.addMoney(player(), 100) == Status.Ok;
  check("triggers.act-add-money", ok && before >= 0 && money() == before + 100, "Money did not rise by 100");
}

function score(): void {
  const p = player();
  check("triggers.act-set-score", Action.setScore(p, 10) == Status.Ok, "SetScore answered an error");
  check("triggers.act-add-score", Action.addScore(p, 5) == Status.Ok, "AddScore answered an error");
}

/** Forbids the GI to the player, then lifts the forbid. */
function allowType(): void {
  if (!have(TYPE_E1)) return;
  const p = player();
  const forbid = Action.allowType(p, ld(TYPE_E1), false) == Status.Ok;
  const allow = Action.allowType(p, ld(TYPE_E1), true) == Status.Ok;
  check("triggers.act-allow-type", forbid && allow, "AllowType answered an error");
}

function animations(): void {
  if (!have(ANIM)) return;
  const anim = <i32>(ld(ANIM) - 1);
  check("triggers.act-spawn-anim", Action.spawnAnim(at(0, -3), anim) == Status.Ok, "SpawnAnim answered an error");
  if (have(DUMMY) && alive(unit(DUMMY))) {
    check("triggers.act-attach-anim", Action.attachAnim(unit(DUMMY), anim) == Status.Ok, "AttachAnim answered an error");
  }
}

function logAction(): void {
  check("triggers.act-log", Action.log("Control Bench triggers: the Log action") == Status.Ok, "Log answered an error");
}

/** A grouped modifier (group 5) on the dummy: `ModifierStart`, `ModifierStacks` and the `ModifierCountPoll` follow. */
function modify(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !alive(dummy)) return;
  const modifier = Action.modify(dummy, Prop.FirepowerMultiplier, ModifierOp.Multiply, FIXED_ONE, 900, 5);
  if (modifier != NO_HANDLE && lastStatus() == Status.Ok) {
    st(MODIFIER, modifier);
    const left = get(modifier, Prop.Remaining);
    check("triggers.act-modify", lastStatus() == Status.Ok && left > 0, "the modifier did not read back");
  } else {
    benchFail("triggers.act-modify", "Modify answered an error");
  }
}

/** Group 5 goes: the modifier reads as gone, `ModifierExpire` follows. */
function removeModifiers(): void {
  if (!have(MODIFIER)) return;
  const ok = Action.removeModifiers(unit(DUMMY), 5) == Status.Ok;
  let gone = false;
  if (ok) {
    get(ld(MODIFIER), Prop.Remaining);
    gone = lastStatus() != Status.Ok;
  }
  check("triggers.act-remove-modifiers", ok && gone, "the modifier still reads");
}

/** `Techno.experience`, −1 when it does not read. */
function experience(t: Techno): i64 {
  const v = get(t, Prop.Experience);
  return lastStatus() == Status.Ok ? v : -1;
}

function addExperience(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !alive(dummy)) return;
  const before = experience(dummy);
  const ok = Action.addExperience(dummy, 1000) == Status.Ok;
  const after = experience(dummy);
  check("triggers.act-add-experience", ok && before >= 0 && after > before, "Experience did not rise");
}

/**
 * A Harrier (`Ammo=1`) made for one call: ammo down and up again, then
 * removed. Without one a GI takes the removal.
 */
function ammoAndRemove(): void {
  const p = player();
  let made = spawn(at(2, -12), "ORCA", p);
  if (made != NO_HANDLE) {
    const plane = made;
    let full = get(plane, Prop.Ammo);
    if (lastStatus() != Status.Ok) full = -1;
    const down = Action.addAmmo(plane, -1) == Status.Ok && is(plane, Prop.Ammo, full - 1);
    const up = Action.addAmmo(plane, 1) == Status.Ok && is(plane, Prop.Ammo, full);
    check("triggers.act-add-ammo", full >= 1 && down && up, "Ammo did not read back");
  } else {
    benchFail("triggers.act-add-ammo", "no Harrier could be placed");
    made = spawn(at(2, -12), "E1", p);
  }
  // Remove queues the object's deletion: it reads as gone from the next
  // frame on.
  if (made != NO_HANDLE) {
    if (Action.remove(made) == Status.Ok) st(REMOVED, made);
    else benchFail("triggers.act-remove", "Remove answered an error");
  }
}

/** Two frames after `Remove`: the unit reads as gone. */
function removedCheck(): void {
  if (!have(REMOVED)) return;
  get(unit(REMOVED), Prop.Health);
  check("triggers.act-remove", lastStatus() != Status.Ok, "the removed unit still reads");
}

/** A GI made to be killed (`C4Warhead=`, defenses ignored). */
function kill(): void {
  const victim = spawn(at(-6, 2), "E1", player());
  if (victim == NO_HANDLE) {
    benchFail("triggers.act-kill", "no GI could be placed");
    return;
  }
  mark("triggers.act-create");
  const ok = Action.kill(victim, NO_HANDLE) == Status.Ok;
  check("triggers.act-kill", ok && !alive(victim), "the GI still lives");
}

/** Kills the `Killed` trigger's GI with the AP warhead. */
function killByDamage(): void {
  if (have(KILLED) && have(WARHEAD_AP) && alive(unit(KILLED))) {
    Action.damage(unit(KILLED), 1000, ld(WARHEAD_AP));
  }
}

/** Gives a GI to the computer house; `HouseTransferred` follows. */
function setOwner(): void {
  const switched = unit(SWITCHED);
  if (!have(SWITCHED) || !alive(switched)) return;
  const e = enemy();
  const ok = Action.setOwner(switched, e) == Status.Ok;
  check("triggers.act-set-owner", ok && is(switched, Prop.Owner, e), "Owner did not read back");
}

/** The computer house's Yuri takes the player's engineer (`records` releases it). */
function yuriAttack(): void {
  if (have(YURI) && have(CONTROLLED)) {
    const units: StaticArray<Techno> = [unit(YURI)];
    if (House.order(enemy(), Order.attack(units, unit(CONTROLLED))) != Status.Ok) {
      benchFail("triggers.order-attack", "the order was turned down");
    }
  }
}

// ------------------------------------------------------------- Conversion

function convert(): void {
  const converted = unit(CONVERTED);
  if (!have(CONVERTED) || !have(TYPE_FV) || !alive(converted)) return;
  const ok = Action.convert(converted, ld(TYPE_FV)) == Status.Ok;
  check("triggers.act-convert", ok && is(converted, Prop.Type, ld(TYPE_FV)), "Type did not read back");
}

/** For 30 frames a Rhino, then the IFV again. */
function convertFor(): void {
  const converted = unit(CONVERTED);
  if (!have(CONVERTED) || !have(TYPE_HTNK) || !alive(converted)) return;
  const ok = Action.convert(converted, ld(TYPE_HTNK), 30) == Status.Ok;
  if (!ok || !is(converted, Prop.Type, ld(TYPE_HTNK))) {
    benchFail("triggers.act-convert-timed", "Type did not read back");
  }
}

function convertBackCheck(): void {
  const converted = unit(CONVERTED);
  if (have(CONVERTED) && have(TYPE_FV) && alive(converted)) {
    check("triggers.act-convert-timed", is(converted, Prop.Type, ld(TYPE_FV)), "the conversion did not revert");
  }
}

// -------------------------------------------------------------- Transport

function addPassenger(): void {
  const ifv = unit(IFV);
  const rider = unit(RIDER);
  if (!have(IFV) || !have(RIDER) || !alive(ifv) || !alive(rider)) return;
  const ok = Action.addPassenger(ifv, rider) == Status.Ok;
  check("triggers.act-add-passenger", ok && is(rider, Prop.Transport, ifv), "Transport did not read back");
}

/** The IFV's Unload mission lets the GI out (`TransportExited`, `PassengerRemoved`). */
function unload(): void {
  if (have(IFV) && alive(unit(IFV))) Action.setMission(unit(IFV), Mission.Unload, NO_HANDLE);
}

// --------------------------------------------------------- Filter clauses

/** `trigger_bind` adds the GI to the unbind trigger... */
function bindFoot(): void {
  if (have(TR_UNBIND) && have(FOOT) && TriggerRef.bind(ld(TR_UNBIND), unit(FOOT)) != Status.Ok) {
    benchFail("triggers.unbind", "trigger_bind failed");
  }
}

/** ...and `trigger_unbind` takes it away again. */
function unbindFoot(): void {
  if (have(TR_UNBIND) && have(FOOT) && TriggerRef.unbind(ld(TR_UNBIND), unit(FOOT)) != Status.Ok) {
    benchFail("triggers.unbind", "trigger_unbind failed");
  }
}

/**
 * The GI (Strength 125) and the Rhino (400) take a hit: the subject clause
 * lets only the Rhino's through, and the unbound GI raises nothing on the
 * unbind trigger.
 */
function filterSubjectHits(): void {
  if (!have(WARHEAD_AP)) return;
  if (have(FOOT) && alive(unit(FOOT))) Action.damage(unit(FOOT), 5, ld(WARHEAD_AP));
  if (have(DUMMY) && alive(unit(DUMMY))) Action.damage(unit(DUMMY), 5, ld(WARHEAD_AP));
}

function filterSubjectAndUnbindCheck(): void {
  if (ld(SUBJECT_YES) != 0 && ld(SUBJECT_NO) == 0) mark("triggers.filter-subject");
  if (ld(UNBIND_YES) != 0 && ld(UNBIND_NO) == 0) mark("triggers.unbind");
}

/** The Rhino takes a hit from itself (a vehicle) and from the GI: the clause on the other object lets only the GI's through. */
function filterOtherHits(): void {
  const dummy = unit(DUMMY);
  if (!have(DUMMY) || !have(FOOT) || !have(WARHEAD_AP) || !alive(dummy)) return;
  Action.damage(dummy, 5, ld(WARHEAD_AP), dummy);
  Action.damage(dummy, 5, ld(WARHEAD_AP), unit(FOOT));
}

function filterOtherCheck(): void {
  if (ld(OTHER_YES) != 0 && ld(OTHER_NO) == 0) mark("triggers.filter-other");
}

/** The player's house passes the house clause, the computer's never does. */
function filterHouseCheck(): void {
  if (ld(HOUSE_YES) != 0 && ld(HOUSE_NO) == 0) mark("triggers.filter-house");
  if (have(TR_HOUSE_NO)) TriggerRef.remove(ld(TR_HOUSE_NO));
}

// ------------------------------------------------------------- The duel

/** The Grizzly's target goes; it is sent at the Rhino again. */
function clearTarget(): void {
  const shooter = unit(SHOOTER);
  if (!have(SHOOTER) || !alive(shooter)) return;
  const ok = Action.clearTarget(shooter) == Status.Ok;
  check("triggers.act-clear-target", ok && is(shooter, Prop.Target, NO_HANDLE), "Target did not clear");
  if (have(VICTIM) && alive(unit(VICTIM))) Action.setMissionOn(shooter, Mission.Attack, unit(VICTIM));
}

/** A GI fires at the ground two cells off once it is ready; the Grizzly at its current target is the other try. */
function fireWeapon(): void {
  if (marked("triggers.act-fire-weapon")) return;
  const foot = have(FOOT) && alive(unit(FOOT)) && Action.fireWeapon(unit(FOOT), 0, NO_HANDLE, at(-10, -8)) == Status.Ok;
  const shooter = !foot && have(SHOOTER) && alive(unit(SHOOTER)) && Action.fireWeapon(unit(SHOOTER), 0) == Status.Ok;
  if (foot || shooter) mark("triggers.act-fire-weapon");
}

/** The player's one-time Psychic Reveal, charged, at a cell north of the arena. */
function launchSuperWeapon(): void {
  if (marked("triggers.act-launch-sw")) return;
  if (Action.launchSuperWeapon(player(), ld(SUPER), at(0, -14)) == Status.Ok) mark("triggers.act-launch-sw");
}

// ----------------------------------------------------------------- Motion

/** The mover drives three cells on; `DistancePoll` and the stock cell events follow. */
function moveMover(): void {
  const mover = unit(MOVER);
  if (have(MOVER) && alive(mover)) {
    let start = get(mover, Prop.Cell);
    if (lastStatus() != Status.Ok) start = NO_HANDLE;
    st(MOVER_START, start);
    Action.setMission(mover, Mission.Move, at(7, 8));
  }
}

function setMissionCheck(): void {
  const mover = unit(MOVER);
  if (marked("triggers.act-set-mission") || !have(MOVER) || !alive(mover)) return;
  const cell = get(mover, Prop.Cell);
  const moved = lastStatus() == Status.Ok && cell != ld(MOVER_START);
  if (moved || is(mover, Prop.Mission, <i64>Mission.Move)) mark("triggers.act-set-mission");
}

function teleport(): void {
  const mover = unit(MOVER);
  if (have(MOVER) && alive(mover)) {
    if (Action.teleport(mover, at(6, 2), Blocked.Nearby) != Status.Ok) {
      benchFail("triggers.act-teleport", "Teleport answered an error");
    }
  }
}

/** The mover stands within two cells of the teleport's cell. */
function teleportCheck(): void {
  const mover = unit(MOVER);
  if (marked("triggers.act-teleport") || !have(MOVER) || !alive(mover)) return;
  const cell: Cell = get(mover, Prop.Cell);
  if (lastStatus() != Status.Ok) return;
  const goal = at(6, 2);
  const near = abs(Cell.x(cell) - Cell.x(goal)) <= 2 && abs(Cell.y(cell) - Cell.y(goal)) <= 2;
  if (near) mark("triggers.act-teleport");
}

/** The Mirage tank's disguise (if it wears one by now) goes. */
function clearDisguise(): void {
  const mirage = unit(MIRAGE);
  if (!have(MIRAGE) || !alive(mirage)) return;
  const ok = Action.clearDisguise(mirage) == Status.Ok;
  const ty = get(mirage, Prop.DisguiseType);
  const bare = lastStatus() != Status.Ok || ty == NO_HANDLE;
  check("triggers.act-clear-disguise", ok && bare, "the disguise stayed");
}

// ---------------------------------------------------------------- Orders

const NO_UNITS = new StaticArray<Techno>(0);

/**
 * House orders for the computer house on the area's two Rhinos (its
 * `Commanded` and `OrderRefused` records are read in `records`).
 */
function order(command: Command): void {
  const units: StaticArray<Techno> = [unit(ORDERED_1), unit(ORDERED_2)];
  const both = have(ORDERED_1) && have(ORDERED_2) && alive(units[0]) && alive(units[1]);
  const second: StaticArray<Techno> = [unit(ORDERED_2)];
  let o: Order | null = null;
  if (command == Command.Move && both) o = Order.moveTo(units, at(8, -9));
  else if (command == Command.Guard && both) o = new Order(Command.Guard, at(8, -9), 0, units);
  else if (command == Command.Stop && both) o = Order.stop(units);
  else if (command == Command.Scatter && both) o = new Order(Command.Scatter, 0, 0, second);
  // No naval factory has a product ready: refused (`NotReady`).
  else if (command == Command.TakeProduct) o = Order.takeProduct(4);
  else if (command == Command.QueueProduction && have(TYPE_E2)) o = Order.queueProduction(ld(TYPE_E2));
  if (o == null) return;
  if (House.order(enemy(), o) == Status.Ok) {
    if (command == Command.QueueProduction) mark("triggers.order-queue");
  } else {
    benchFail("triggers.order", "an order was turned down at once");
  }
}

// -------------------------------------------------------------------- INI

/**
 * An Allied attack dog: its `Birth` reaches the INI trigger of the area's
 * `rulesmd.ini` block, whose `Wasm:benchIniDog` action `records` hears.
 */
function putDog(): void {
  const dog = spawn(at(-11, -12), "ADOG", player());
  if (dog != NO_HANDLE) st(DOG, dog);
  else benchFail("triggers.ini-trigger", "no dog could be placed");
}
