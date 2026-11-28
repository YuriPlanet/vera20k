// The area's units and triggers (the Rust `triggers/setup.rs`), made over
// the first frames after the arena opens (a few per frame, so no frame
// carries all of it).
import {
  Action, ActionDef, Category, Event, House, Modifier, Mission, NO_HANDLE, Op, Prop, Rules,
  Status, Target, Trigger, TriggerRef, Var, get, lastStatus, set,
} from "@vera20k/sdk";
import { benchFail, check, enemy, is, made, mark, player, spawn } from "../bench";
import {
  ANIM, CELL_COLUMN, CELL_ROW, CONTROLLED, DUMMY, FOOT, GLOBAL_EVENT, IFV, KILLED, MIRAGE, MOVER,
  CONVERTED, ORDERED_1, ORDERED_2, PING, PONG, PROJECTILE_CANNON, RIDER, SHOOTER, SUPER, SUPER_GRANTED,
  SWITCHED, BAIT, TR_HOUSE_NO, TR_PROJECTILE, TR_SET_VAR, TR_TIMER, TR_TRANSFERRED, TR_UNBIND, TR_WARHEAD,
  TR_WEAPON, TYPE_E1, TYPE_E2, TYPE_ENGINEER, TYPE_FV, TYPE_HTNK, VICTIM, WARHEAD_AP, WEAPON_105, YURI,
  at, eventId, have, ld, st, unit,
} from "./state";
import {
  onAdded, onAttackedBy, onBirth, onCellColumn, onCellRow, onCommanded, onEntered, onExited, onFilterHouseNo,
  onFilterHouseYes, onFilterOther, onFilterSubject, onFire, onFireProjectileType, onFireWarheadType,
  onFireWeaponType, onGlobalEvent, onKilled, onModExpire, onModStacks, onModStart, onOrderRefused, onPing,
  onPollDistance, onPollModCount, onPollMoney, onPollPower, onPollProperty, onPollVariable, onPong, onRemoved,
  onSetVar, onStockAttacked, onStockDestroyed, onStockHouse, onStockLogic, onSuperWeaponLaunched, onTakeDamage,
  onTimer, onTimerObject, onTransferred, onUnbind,
} from "./records";

/** Keeps a type reference in `slot`, or fails `why`. */
function keepType(slot: i32, ty: i64, why: string): void {
  if (ty != NO_HANDLE) st(slot, ty);
  else benchFail("triggers.setup", why);
}

/** Resolves the rules names the area uses, once. */
export function lookups(): void {
  const slots: StaticArray<i32> = [TYPE_FV, TYPE_HTNK, TYPE_E1, TYPE_E2, TYPE_ENGINEER];
  const names: StaticArray<string> = ["FV", "HTNK", "E1", "E2", "ENGINEER"];
  for (let i = 0; i < slots.length; i++) {
    keepType(slots[i], Rules.technoType(names[i]), "a retail TechnoType is missing");
  }
  keepType(WEAPON_105, Rules.weapon("105mm"), "weapon 105mm is missing");
  keepType(WARHEAD_AP, Rules.warhead("AP"), "warhead AP is missing");
  keepType(PROJECTILE_CANNON, Rules.projectile("Cannon"), "projectile Cannon is missing");
  const anim = Rules.anim("PIFF");
  if (anim >= 0) st(ANIM, <i64>anim + 1);
  else benchFail("triggers.setup", "animation PIFF is missing");
  keepType(SUPER, Rules.superWeaponType("PsychicRevealSpecial"), "PsychicRevealSpecial is missing");
}

/** The setup work of frame `t` after the arena opened. */
export function setupStep(t: i64): void {
  if (t == 1) firstUnits();
  else if (t == 2) objectTriggers();
  else if (t == 3) globalTriggers();
  else if (t == 4) secondUnits();
  else if (t == 5) triggerProperties();
}

/** Creates a `name` for `house` at `dx`, `dy` from the arena's corner. */
function put(slot: i32, dx: i32, dy: i32, name: string, house: House): void {
  const made = spawn(at(dx, dy), name, house);
  if (made != NO_HANDLE) st(slot, made);
  else benchFail("triggers.spawn", "a unit could not be placed");
}

/**
 * Keeps a made trigger's handle in `slot` (0 for none); a trigger the
 * engine turned down is a wrong answer. Call it right after `done`.
 */
function keep(slot: i32, trigger: TriggerRef): bool {
  if (made(trigger)) {
    if (slot != 0) st(slot, trigger);
    return true;
  }
  benchFail("triggers.create", "trigger_create or its binding failed");
  return false;
}

/** Frame 1: the units the object triggers bind. */
function firstUnits(): void {
  const p = player();
  const e = enemy();
  // The duel: a Grizzly and, three cells off, a weaponless enemy Rhino
  // with little health left.
  put(SHOOTER, 0, 0, "MTNK", p);
  put(VICTIM, 3, 0, "HTNK", e);
  if (have(VICTIM)) {
    const victim = unit(VICTIM);
    set(victim, Prop.Health, 130);
    Modifier.overrideWith(victim, Prop.DisableWeapons, 1, -1);
  }
  // The dummy every damage, heal and modifier action works on.
  put(DUMMY, -4, -5, "HTNK", p);
  put(FOOT, -8, -8, "E1", p);
  put(KILLED, -4, 2, "E1", p);
  put(MOVER, 4, 5, "MTNK", p);
  put(CONVERTED, -2, -9, "MTNK", p);
  put(IFV, -8, -2, "FV", p);
  put(RIDER, -10, -3, "E1", p);
  put(SWITCHED, -2, 4, "E1", p);
}

/** Frame 2: triggers bound to the first units. */
function objectTriggers(): void {
  const p = player();
  const dummy = unit(DUMMY);
  if (have(DUMMY)) {
    if (keep(0, Trigger.on(Event.TakeDamage).bind(dummy).enqueue(onTakeDamage).done())) {
      mark("triggers.create");
    }
    // Timer beside an object event: a period, then the next hit.
    keep(0, Trigger.every(10).and(Event.TakeDamage).bind(dummy).once().enqueue(onTimerObject).done());
    keep(0, Trigger.on(Event.ModifierStart).bind(dummy).enqueue(onModStart).done());
    keep(0, Trigger.on(Event.ModifierStacks).bind(dummy).enqueue(onModStacks).done());
    keep(0, Trigger.on(Event.ModifierExpire).bind(dummy).enqueue(onModExpire).done());
    keep(0, Trigger.onProperty(Prop.Health, Op.Lt, 300, 3).bind(dummy).once().enqueue(onPollProperty).done());
    keep(0, Trigger.onModifierCount(5, Op.Ge, 1, 3).bind(dummy).once().enqueue(onPollModCount).done());
    // Filter clauses: Strength above 300 (the Rhino, not the GI); an
    // infantry source.
    const subject = Trigger.on(Event.TakeDamage).where(Prop.Strength, Op.Gt, 300).bind(dummy);
    if (have(FOOT)) subject.bind(unit(FOOT));
    keep(0, subject.enqueue(onFilterSubject).done());
    keep(0, Trigger.on(Event.TakeDamage)
      .whereOther(Prop.Category, Op.Eq, <i64>Category.Infantry)
      .bind(dummy)
      .enqueue(onFilterOther)
      .done());
    // `bind` and `unbind` add and take away the GI later.
    keep(TR_UNBIND, Trigger.on(Event.TakeDamage).bind(dummy).enqueue(onUnbind).done());
    // A module event with immediate actions, one of which raises another
    // module event: the chain.
    const ping = eventId(PING);
    const pong = eventId(PONG);
    if (ping >= 0 && pong >= 0) {
      keep(0, Trigger.onModuleEvent(ping)
        .bind(dummy)
        .immediate(ActionDef.heal(Target.Subject, 30))
        .immediate(ActionDef.addMoney(7, p))
        .immediate(ActionDef.raiseEvent(Target.Subject, pong, 5, 0))
        .enqueue(onPing)
        .done());
      keep(0, Trigger.onModuleEvent(pong).bind(dummy).enqueue(onPong).done());
    }
  }
  if (have(KILLED) || have(VICTIM)) {
    const killed = Trigger.on(Event.Killed);
    if (have(KILLED)) killed.bind(unit(KILLED));
    if (have(VICTIM)) killed.bind(unit(VICTIM));
    keep(0, killed.enqueue(onKilled).done());
  }
  if (have(SHOOTER)) {
    keep(0, Trigger.on(Event.Fire).bind(unit(SHOOTER)).enqueue(onFire).done());
  }
  if (have(VICTIM)) {
    const victim = unit(VICTIM);
    keep(0, Trigger.on(Event.AttackedBy).bind(victim).enqueue(onAttackedBy).done());
    keep(0, Trigger.on(Event.AttackedByAnyHouse).bind(victim).enqueue(onStockAttacked).done());
    keep(0, Trigger.on(Event.DestroyedByAnyHouse).bind(victim).enqueue(onStockDestroyed).done());
  }
  if (have(RIDER) && have(IFV)) {
    const rider = unit(RIDER);
    const ifv = unit(IFV);
    keep(0, Trigger.on(Event.TransportEntered).bind(rider).enqueue(onEntered).done());
    keep(0, Trigger.on(Event.PassengerAdded).bind(ifv).enqueue(onAdded).done());
    keep(0, Trigger.on(Event.TransportExited).bind(rider).enqueue(onExited).done());
    keep(0, Trigger.on(Event.PassengerRemoved).bind(ifv).enqueue(onRemoved).done());
  }
  if (have(SWITCHED)) {
    keep(TR_TRANSFERRED, Trigger.on(Event.HouseTransferred).bind(unit(SWITCHED)).enqueue(onTransferred).done());
  }
  if (have(MOVER)) {
    // Where it stands (a ring cell when its own was taken).
    let start = get(unit(MOVER), Prop.Cell);
    if (lastStatus() != Status.Ok) start = at(4, 5);
    keep(0, Trigger.onDistance(start, Op.Gt, 512, 5).bind(unit(MOVER)).once().enqueue(onPollDistance).done());
  }
  // Stock cell events: a foot arriving on the row (25) or the column (26)
  // of a bound cell, of any house (−1). The mover crosses both.
  const row = at(0, 7);
  const column = at(6, 0);
  st(CELL_ROW, row);
  st(CELL_COLUMN, column);
  const rows = keep(0, Trigger.on(Event.CrossesHorizontalLine).value(-1).bind(row).once().enqueue(onCellRow).done());
  const columns = keep(0, Trigger.on(Event.CrossesVerticalLine).value(-1).bind(column).once().enqueue(onCellColumn).done());
  if (rows && columns) mark("triggers.bind-cell");
}

/** Frame 3: global triggers and the type-bound ones. */
function globalTriggers(): void {
  const p = player();
  const e = enemy();
  keep(TR_TIMER, Trigger.every(30).enqueue(onTimer).done());
  keep(0, Trigger.on(Event.SuperWeaponLaunched).enqueue(onSuperWeaponLaunched).done());
  const global = eventId(GLOBAL_EVENT);
  if (global >= 0) keep(0, Trigger.onModuleEvent(global).enqueue(onGlobalEvent).done());
  // Stock House event (polled every eighth frame) and Logic event.
  keep(0, Trigger.on(Event.CreditsExceed).value(0).house(p).once().enqueue(onStockHouse).done());
  keep(0, Trigger.on(Event.ElapsedTime).value(1).once().enqueue(onStockLogic).done());
  // Polls: a Red Alert 2 local variable a trigger's SetVar sets, the
  // player's power and money.
  keep(0, Trigger.onVariable(Var.local(87), Op.Eq, 1, 5).house(p).once().enqueue(onPollVariable).done());
  keep(TR_SET_VAR, Trigger.every(40)
    .once()
    .immediate(ActionDef.setVar(Target.Subject, Var.local(87), 1))
    .enqueue(onSetVar)
    .done());
  keep(0, Trigger.onHousePower(Op.Ge, -1000000, 15).house(p).once().enqueue(onPollPower).done());
  keep(0, Trigger.onProperty(Prop.Money, Op.Ge, 0, 15).house(p).once().enqueue(onPollMoney).done());
  // A house clause: true for the human player's house only.
  keep(0, Trigger.every(20).house(p).whereHouse(Prop.IsHuman, Op.Eq, 1).once().enqueue(onFilterHouseYes).done());
  keep(TR_HOUSE_NO, Trigger.every(20).house(e).whereHouse(Prop.IsHuman, Op.Eq, 1).enqueue(onFilterHouseNo).done());
  keep(0, Trigger.on(Event.OrderRefused).enqueue(onOrderRefused).done());
  // Birth bound to a type (the engineer the area creates next frame).
  if (have(TYPE_ENGINEER) && keep(0, Trigger.on(Event.Birth).bind(ld(TYPE_ENGINEER)).enqueue(onBirth).done())) {
    mark("triggers.bind-type");
  }
  // `Fire` bound to the Grizzly's weapon, its warhead and its projectile
  // type; each is removed after its first record.
  if (have(WEAPON_105)) {
    keep(TR_WEAPON, Trigger.on(Event.Fire).bind(ld(WEAPON_105)).enqueue(onFireWeaponType).done());
  }
  if (have(WARHEAD_AP)) {
    keep(TR_WARHEAD, Trigger.on(Event.Fire).bind(ld(WARHEAD_AP)).enqueue(onFireWarheadType).done());
  }
  if (have(PROJECTILE_CANNON)) {
    keep(TR_PROJECTILE, Trigger.on(Event.Fire).bind(ld(PROJECTILE_CANNON)).enqueue(onFireProjectileType).done());
  }
}

/**
 * Frame 4: the units of the orders, the mind control, the disguise and the
 * `Birth`; the duel starts and the player gets a superweapon.
 */
function secondUnits(): void {
  const p = player();
  const e = enemy();
  put(BAIT, -4, -12, "ENGINEER", p);
  put(ORDERED_1, 8, -12, "HTNK", e);
  put(ORDERED_2, 10, -12, "HTNK", e);
  // Ten cells apart, beyond the Yuri's reach, until its attack order.
  put(YURI, 14, 8, "YURI", e);
  put(CONTROLLED, 14, -2, "SENGINEER", p);
  put(MIRAGE, -8, -12, "MGTK", p);
  const commanded = Trigger.on(Event.Commanded);
  if (have(ORDERED_1)) commanded.bind(unit(ORDERED_1));
  if (have(ORDERED_2)) commanded.bind(unit(ORDERED_2));
  if (have(YURI)) commanded.bind(unit(YURI));
  keep(0, commanded.enqueue(onCommanded).done());
  // `trigger_bind` on a made trigger: the mind control's victim.
  if (have(TR_TRANSFERRED) && have(CONTROLLED)) {
    check("triggers.bind", TriggerRef.bind(ld(TR_TRANSFERRED), unit(CONTROLLED)) == Status.Ok, "trigger_bind failed");
  }
  if (have(SHOOTER) && have(VICTIM)) Action.setMissionOn(unit(SHOOTER), Mission.Attack, unit(VICTIM));
  if (have(SUPER) && Action.oneTimeSuperWeapon(p, ld(SUPER)) == Status.Ok) st(SUPER_GRANTED, 1);
}

/** Frame 5: the trigger properties `Enabled` (read and write), `House` and `Repeat` (read; a write answers `Refused`). */
function triggerProperties(): void {
  const no: TriggerRef = ld(TR_HOUSE_NO);
  if (have(TR_HOUSE_NO)) {
    const off = set(no, Prop.Enabled, 0) == Status.Ok && is(no, Prop.Enabled, 0);
    const on = set(no, Prop.Enabled, 1) == Status.Ok && is(no, Prop.Enabled, 1);
    check("triggers.prop-enabled", off && on, "Enabled did not read back");
    const foe = enemy();
    const house = is(no, Prop.House, foe)
      && set(no, Prop.House, player()) == Status.Refused
      && is(no, Prop.House, foe);
    check("triggers.prop-house", house, "House read wrong");
  }
  if (have(TR_TIMER) && have(TR_SET_VAR)) {
    const timer: TriggerRef = ld(TR_TIMER);
    const repeat = is(timer, Prop.Repeat, 2)
      && is(ld(TR_SET_VAR), Prop.Repeat, 0)
      && set(timer, Prop.Repeat, 0) == Status.Refused
      && is(timer, Prop.Repeat, 2);
    check("triggers.prop-repeat", repeat, "Repeat read wrong");
  }
}
