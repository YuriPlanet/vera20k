// The records the area's triggers deliver (the Rust `triggers/records.rs`):
// each marks its event once its fields are what the scene made. The Rust
// `dispatch` matches the record's key; here each key is its own handler,
// which returns at once once the finale began.
import { Action, Command, Event, EventRecord, NO_HANDLE, Prop, Status, TriggerRef, Var, get, lastStatus } from "@vera20k/sdk";
import { benchFail, check, enemy, is, mark, marked, player } from "../bench";
import { finaleStarted } from "../finale";
import {
  BAIT, CELL_COLUMN, CELL_ROW, CONTROLLED, DOG, DUMMY, FOOT, HOUSE_NO, HOUSE_YES, IFV, KILLED, MODIFIER,
  ORDERED_1, ORDERED_2, OTHER_NO, OTHER_YES, PING_HEALTH, PING_MONEY, RIDER, SHOOTER, SUBJECT_NO,
  SUBJECT_YES, SUPER, SWITCHED, TR_PROJECTILE, TR_TIMER, TR_WARHEAD, TR_WEAPON, UNBIND_NO, UNBIND_YES,
  VICTIM, WARHEAD_AP, WEAPON_105, YURI, have, health, ld, money, retire, st, unit,
} from "./state";

/** A type slot's ID within its kind. */
function idOf(slot: i32): i32 {
  return <i32>ld(slot);
}

export function onTakeDamage(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(WARHEAD_AP) && r.subject == ld(DUMMY) && r.value > 0) {
    check("triggers.ev-take-damage", r.id == idOf(WARHEAD_AP), "the warhead is not AP");
  }
}

export function onKilled(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(WARHEAD_AP) && r.id == idOf(WARHEAD_AP)) {
    mark("triggers.ev-killed-warhead");
  } else if (r.subject == ld(KILLED)) {
    benchFail("triggers.ev-killed-warhead", "no AP warhead on the kill");
  }
}

export function onFire(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(SHOOTER) && have(WEAPON_105)) {
    check("triggers.ev-fire", r.id == idOf(WEAPON_105), "the weapon is not 105mm");
  }
}

/**
 * The global `Timer`: its record names the previous period's frame; then
 * the trigger is removed (`trigger_remove`) and no longer reads.
 */
export function onTimer(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ev-timer", r.value < <i64>r.frame, "the last frame is not before this one");
  if (marked("triggers.remove") || !have(TR_TIMER)) return;
  const timer: TriggerRef = ld(TR_TIMER);
  let removed = TriggerRef.remove(timer) == Status.Ok;
  if (removed) {
    get(timer, Prop.Enabled);
    removed = lastStatus() != Status.Ok;
  }
  check("triggers.remove", removed, "the removed trigger still reads");
}

/**
 * Both events latch on the dummy's instance; whichever comes second
 * completes it and is the record.
 */
export function onTimerObject(r: EventRecord): void {
  if (finaleStarted()) return;
  if ((r.kind == Event.TakeDamage || r.kind == Event.Timer) && r.subject == ld(DUMMY) && r.holder == ld(DUMMY)) {
    mark("triggers.ev-timer-object");
  }
}

export function onAttackedBy(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(VICTIM) && r.other != NO_HANDLE) mark("triggers.ev-attacked-by");
}

export function onBirth(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(BAIT) && r.subject == ld(BAIT)) {
    mark("triggers.ev-birth");
    retire(BAIT);
  }
}

/** A transport record on `subject` naming `other`. */
function transport(r: EventRecord, subject: i32, other: i32, name: string): void {
  if (have(subject) && r.subject == ld(subject)) {
    check(name, r.other == ld(other), "the other object is wrong");
  }
}

export function onEntered(r: EventRecord): void {
  if (finaleStarted()) return;
  transport(r, RIDER, IFV, "triggers.ev-transport-entered");
}

export function onAdded(r: EventRecord): void {
  if (finaleStarted()) return;
  transport(r, IFV, RIDER, "triggers.ev-passenger-added");
}

export function onExited(r: EventRecord): void {
  if (finaleStarted()) return;
  transport(r, RIDER, IFV, "triggers.ev-transport-exited");
}

export function onRemoved(r: EventRecord): void {
  if (finaleStarted()) return;
  transport(r, IFV, RIDER, "triggers.ev-passenger-removed");
}

/**
 * `HouseTransferred`: the GI given away (value 0, old house the player),
 * then the engineer the Yuri took (value 1, old house the player),
 * released at once. The release is a mind-control transfer too, which is
 * left alone.
 */
export function onTransferred(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(SWITCHED) && r.subject == ld(SWITCHED)) {
    check("triggers.ev-house-transferred", r.value == 0 && r.house == player(), "the cause or old house is wrong");
  } else if (have(CONTROLLED) && r.subject == ld(CONTROLLED) && r.value == 1 && r.house == player()) {
    mark("triggers.ev-mind-control");
    const controlled = unit(CONTROLLED);
    const freed = Action.clearMindControl(controlled) == Status.Ok && is(controlled, Prop.Owner, player());
    check("triggers.act-clear-mind-control", freed, "the engineer did not return");
    retire(YURI);
    retire(CONTROLLED);
  }
}

export function onSuperWeaponLaunched(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.house == player() && have(SUPER)) {
    check("triggers.ev-sw-launched", r.id == idOf(SUPER), "the superweapon type is wrong");
  }
}

export function onModStart(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(MODIFIER) && r.modifier == ld(MODIFIER)) {
    check("triggers.ev-mod-start", r.value == <i64>Prop.FirepowerMultiplier, "the property is wrong");
  }
}

export function onModStacks(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(DUMMY)) check("triggers.ev-mod-stacks", r.value >= 1, "the group count is wrong");
}

export function onModExpire(r: EventRecord): void {
  if (finaleStarted()) return;
  if (have(MODIFIER) && r.modifier == ld(MODIFIER)) mark("triggers.ev-mod-expire");
}

/**
 * The ping trigger's immediate actions ran before its module action: the
 * dummy healed by 30, the player paid 7.
 */
export function onPing(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject != ld(DUMMY)) return;
  check("triggers.ev-module-event", r.value == 1 && r.value2 == 2, "the raised values are wrong");
  check("triggers.imm-heal", health(unit(DUMMY)) == ld(PING_HEALTH) + 30, "Heal did not add 30");
  check("triggers.imm-add-money", money() == ld(PING_MONEY) + 7, "AddMoney did not pay 7");
}

export function onPong(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(DUMMY)) check("triggers.chain", r.value == 5, "the chained value is wrong");
}

export function onGlobalEvent(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ev-module-global", r.value == 3 && r.value2 == 4, "the values are wrong");
}

export function onStockAttacked(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(VICTIM)) mark("triggers.ev-stock-attacked");
}

export function onStockDestroyed(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(VICTIM)) mark("triggers.ev-stock-destroyed");
}

export function onStockHouse(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ev-stock-house", r.frame % 8 == 0 && r.house == player(), "not the player on an eighth frame");
}

export function onStockLogic(r: EventRecord): void {
  if (finaleStarted()) return;
  mark("triggers.ev-stock-logic");
}

export function onCellRow(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ev-cell-row", r.holder == ld(CELL_ROW), "not the bound cell");
}

export function onCellColumn(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ev-cell-column", r.holder == ld(CELL_COLUMN), "not the bound cell");
}

export function onPollProperty(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.poll-property", r.value < 300, "the reading is not below 300");
}

export function onPollVariable(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.poll-variable", r.value == 1, "the reading is not 1");
}

export function onPollPower(r: EventRecord): void {
  if (finaleStarted()) return;
  mark("triggers.poll-power");
}

export function onPollModCount(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.poll-modifier-count", r.value >= 1, "the count is below 1");
}

export function onPollDistance(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.poll-distance", r.value > 512, "the reading is not past 512");
}

export function onPollMoney(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.poll-money", r.value >= 0, "the reading is negative");
}

export function onFilterSubject(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(DUMMY)) {
    st(SUBJECT_YES, 1);
  } else if (r.subject == ld(FOOT)) {
    st(SUBJECT_NO, 1);
    benchFail("triggers.filter-subject", "the clause let the GI through");
  }
}

export function onFilterOther(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.other == ld(FOOT)) {
    st(OTHER_YES, 1);
  } else if (r.other == ld(DUMMY)) {
    st(OTHER_NO, 1);
    benchFail("triggers.filter-other", "the clause let a vehicle through");
  }
}

export function onFilterHouseYes(r: EventRecord): void {
  if (finaleStarted()) return;
  st(HOUSE_YES, 1);
}

export function onFilterHouseNo(r: EventRecord): void {
  if (finaleStarted()) return;
  st(HOUSE_NO, 1);
  benchFail("triggers.filter-house", "the clause let a computer house through");
}

export function onUnbind(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.subject == ld(DUMMY)) {
    st(UNBIND_YES, 1);
  } else if (r.subject == ld(FOOT)) {
    st(UNBIND_NO, 1);
    benchFail("triggers.unbind", "an unbound object raised the event");
  }
}

export function onSetVar(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.act-set-var", Var.local(87).get() == 1, "local 87 did not read back");
}

/** A `Fire` record of a weapon, warhead or projectile binding: marked, then that trigger is removed. */
function typedFire(r: EventRecord, slot: i32, name: string): void {
  if (r.subject != NO_HANDLE) mark(name);
  if (have(slot)) {
    TriggerRef.remove(ld(slot));
    st(slot, 0);
  }
}

export function onFireWeaponType(r: EventRecord): void {
  if (finaleStarted()) return;
  typedFire(r, TR_WEAPON, "triggers.bind-weapon");
}

export function onFireWarheadType(r: EventRecord): void {
  if (finaleStarted()) return;
  typedFire(r, TR_WARHEAD, "triggers.bind-warhead");
}

export function onFireProjectileType(r: EventRecord): void {
  if (finaleStarted()) return;
  typedFire(r, TR_PROJECTILE, "triggers.bind-projectile");
}

/** `Commanded` on the ordered Rhinos and the Yuri, by command. */
export function onCommanded(r: EventRecord): void {
  if (finaleStarted()) return;
  const ours = (have(ORDERED_1) && r.subject == ld(ORDERED_1))
    || (have(ORDERED_2) && r.subject == ld(ORDERED_2))
    || (have(YURI) && r.subject == ld(YURI));
  if (!ours) return;
  const v = r.value;
  if (v == <i64>Command.Move) mark("triggers.order-move");
  else if (v == <i64>Command.Guard) mark("triggers.order-guard");
  else if (v == <i64>Command.Attack) mark("triggers.order-attack");
  else if (v == <i64>Command.Stop) mark("triggers.order-stop");
  else if (v == <i64>Command.Scatter) mark("triggers.order-scatter");
}

export function onOrderRefused(r: EventRecord): void {
  if (finaleStarted()) return;
  if (r.house == enemy() && r.value == <i64>Command.TakeProduct) mark("triggers.order-rejected");
}

/** The INI trigger's `Wasm:benchIniDog(Subject,dog)` action. */
export function iniDog(r: EventRecord): void {
  if (finaleStarted()) return;
  check("triggers.ini-trigger", r.argsText == "dog", "the action text is wrong");
  if (have(DOG) && r.subject == ld(DOG)) retire(DOG);
}
