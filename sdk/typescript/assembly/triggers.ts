// Runtime triggers (RFC 0001 sections 6a and 7, appendix section 3) and
// the module's callbacks: `Trigger` builds a `TriggerDef` and calls
// `trigger_create` and `trigger_bind`, `ActionDef` builds trigger actions,
// and `on_events` / `on_decide` hand each record to the handler its
// dispatch key names.
//
//   function onHit(e: TakeDamageEvent): void { ... }
//   namedAction("OnHit", onHit);           // INI: Action1=Wasm:OnHit
//   onScenarioStart((): void => {
//     Trigger.on(Event.TakeDamage).bind(unit).enqueue(onHit).done();
//   });

import * as sys from "./sys";
import {
  Status, check, OUT, utf8, utf8Len, NO_HANDLE, Ref, House, Cell, Prop, Op, Var, ActionKind,
  frame, set, lastStatus, Find, TypeRef, NameKind,
} from "./core";
import { Event } from "./generated/events";
import { Target, EventRecord, DecideRecord } from "./records";

export { Event } from "./generated/events";

// ------------------------------------------------------------- Handlers

// Dispatch keys (RFC 12): a named action's key is a hash of its name, so
// the build writes the same key to `vera.actions` without running the
// module; other handlers get small keys in registration order. Both live
// in module memory, which saves carry, so a loaded game dispatches as the
// saved one did.
const NAMED: i32 = 0x40000000;
const handlerKeys = new Array<i32>();
const handlerFns = new Array<usize>();
let nextKey: i32 = 1;

/** The dispatch key of the action named `name` (FNV-1a over its ASCII-lowercased bytes). */
export function actionKey(name: string): i32 {
  let hash: u32 = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    let c = <u32>name.charCodeAt(i);
    if (c >= 65 && c <= 90) c += 32;
    hash = (hash ^ (c & 0xff)) * 0x01000193;
  }
  return <i32>(hash & 0x3fffffff) | NAMED;
}

/** @internal The dispatch key of handler `fn`, registering it under a new key if it has none. */
export function keyOf(fn: usize): i32 {
  let unnamed = -1;
  for (let i = 0; i < handlerFns.length; i++) {
    if (unchecked(handlerFns[i]) == fn) {
      const key = unchecked(handlerKeys[i]);
      if (key & NAMED) return key;
      unnamed = key;
    }
  }
  if (unnamed >= 0) return unnamed;
  const key = nextKey++;
  handlerKeys.push(key);
  handlerFns.push(fn);
  return key;
}

function handlerFor(key: i32): usize {
  for (let i = 0; i < handlerKeys.length; i++) {
    if (unchecked(handlerKeys[i]) == key) return unchecked(handlerFns[i]);
  }
  return 0;
}

/**
 * Names handler `fn` so INI can call it (`Action=Wasm:<name>`,
 * `Decide=Wasm:<name>`). Write `name` as a string literal: the build copies
 * it into the module's `vera.actions` section. Returns the dispatch key.
 */
export function namedAction<T>(name: string, fn: (e: T) => void): i32 {
  const key = actionKey(name);
  if (handlerFor(key) == 0) {
    handlerKeys.push(key);
    handlerFns.push(changetype<usize>(fn));
  }
  return key;
}

// ----------------------------------------------------------- Lifecycle

// Each registration runs, in registration order.
const scenarioStart = new Array<usize>();
const rulesHooks = new Array<usize>();
const messageHooks = new Array<usize>();
const frameFns = new Array<usize>();
const frameEvery = new Array<i32>();

/** How often an `onFrame` callback runs: on frames divisible by `frames`. */
export class Every {
  constructor(public readonly frames: i32) {}
}

/** Runs `fn` once at scenario start. */
export function onScenarioStart(fn: () => void): void {
  scenarioStart.push(changetype<usize>(fn));
}

/** Runs `fn` once at this module's layer of rules processing, where `Rules.set` writes. */
export function onRules(fn: () => void): void {
  rulesHooks.push(changetype<usize>(fn));
}

/** Runs `fn` on every frame divisible by `every.frames`, at the end of the module phase. */
export function onFrame(every: Every, fn: () => void): void {
  frameFns.push(changetype<usize>(fn));
  frameEvery.push(max(every.frames, 1));
}

/**
 * Runs `fn` for each message a same-origin client module sent
 * (`Client.sendMessage`), with the sender's house and a copy of the bytes.
 */
export function onMessage(fn: (sender: House, data: Uint8Array) => void): void {
  messageHooks.push(changetype<usize>(fn));
}

/** @internal */
export function runScenarioStart(): void {
  for (let i = 0; i < scenarioStart.length; i++) changetype<() => void>(unchecked(scenarioStart[i]))();
}

/** @internal */
export function runRules(): void {
  for (let i = 0; i < rulesHooks.length; i++) changetype<() => void>(unchecked(rulesHooks[i]))();
}

/** @internal */
export function runFrame(): void {
  const now = frame();
  for (let i = 0; i < frameFns.length; i++) {
    if (now % unchecked(frameEvery[i]) == 0) changetype<() => void>(unchecked(frameFns[i]))();
  }
}

/** @internal `on_events(ptr, count)`. */
export function runEvents(ptr: usize, count: i32): void {
  for (let i = 0; i < count; i++) {
    const record = ptr + <usize>(i * sys.EVENT_RECORD_BYTES);
    const fn = handlerFor(load<i32>(record, sys.ER_KEY));
    if (fn) changetype<(e: usize) => void>(fn)(record);
  }
}

/** @internal `on_decide(ptr, count)`. */
export function runDecide(ptr: usize, count: i32): void {
  for (let i = 0; i < count; i++) {
    const record = ptr + <usize>(i * sys.DECIDE_RECORD_BYTES);
    const fn = handlerFor(load<i32>(record, sys.ER_KEY));
    if (fn) changetype<(e: usize) => void>(fn)(record);
  }
}

/** @internal `on_message(sender, ptr, len)`. */
export function runMessage(sender: i64, ptr: usize, len: i32): void {
  if (!messageHooks.length) return;
  // A copy: the delivery area is reused by the next call.
  const data = new Uint8Array(len);
  memory.copy(data.dataStart, ptr, <usize>len);
  for (let i = 0; i < messageHooks.length; i++) {
    changetype<(sender: House, data: Uint8Array) => void>(unchecked(messageHooks[i]))(sender, data);
  }
}

// --------------------------------------------------------- Trigger actions

/** Whose variable a filter clause reads (`VarTarget`, appendix 8.3). */
export enum VarTarget {
  Global = 0,
  Subject = 1,
  Other = 2,
  House = 3,
}

/**
 * A trigger action (`ActionDef` with its `ActionParams`), only constructed
 * here; `Trigger.immediate` adds it.
 */
export class ActionDef {
  readonly bytes: StaticArray<u8> = new StaticArray<u8>(sys.ACTION_DEF_BYTES);
  // A text the action carries, kept alive until `trigger_create` copies it.
  private text: ArrayBuffer | null = null;

  constructor(kind: i32, target: Target) {
    store<i32>(this.ptr, kind, 0);
    store<i32>(this.ptr, <i32>target, 4);
  }

  /** @internal */
  get ptr(): usize {
    return changetype<usize>(this.bytes);
  }

  private param<T>(value: T, at: i32): ActionDef {
    store<T>(this.ptr + 8 + <usize>at, value);
    return this;
  }

  private withText(s: string): ActionDef {
    const buffer = String.UTF8.encode(s);
    this.text = buffer;
    store<i32>(this.ptr + 8, <i32>changetype<usize>(buffer), sys.AP_TEXT);
    store<i32>(this.ptr + 8, buffer.byteLength, sys.AP_TEXT_LEN);
    return this;
  }

  /** Any action `kind` on `target`; set its parameters with the raw writers. */
  static raw(kind: i32, target: Target): ActionDef {
    return new ActionDef(kind, target);
  }
  /** Sets `ActionParams.id`. */
  id(id: i32): ActionDef { return this.param<i32>(id, sys.AP_ID); }
  /** Sets `ActionParams.arg`. */
  arg(arg: i32): ActionDef { return this.param<i32>(arg, sys.AP_ARG); }
  /** Sets `ActionParams.arg2`. */
  arg2(arg: i32): ActionDef { return this.param<i32>(arg, sys.AP_ARG2); }
  /** Sets `ActionParams.value`. */
  value(value: i64): ActionDef { return this.param<i64>(value, sys.AP_VALUE); }
  /** Sets `ActionParams.value2`. */
  value2(value: i64): ActionDef { return this.param<i64>(value, sys.AP_VALUE2); }
  /** Sets `ActionParams.cell`. */
  cell(cell: Ref): ActionDef { return this.param<i64>(cell, sys.AP_CELL); }
  /** Sets `ActionParams.house`. */
  house(house: House): ActionDef { return this.param<i64>(house, sys.AP_HOUSE); }
  /** Sets `ActionParams.other`. */
  other(other: Ref): ActionDef { return this.param<i64>(other, sys.AP_OTHER); }

  /** Gives `house` (the trigger's house for `NO_HANDLE`) `amount` credits. */
  static addMoney(amount: i32, house: House = NO_HANDLE): ActionDef {
    return new ActionDef(ActionKind.AddMoney, Target.House).value(amount).house(house);
  }
  /** Writes `value` to variable `v` of `target` (a Red Alert 2 variable ignores the target). */
  static setVar(target: Target, v: Var, value: i64): ActionDef {
    return new ActionDef(ActionKind.SetVar, target).id(v.id).value(value);
  }
  /** Queues a record for `fn` in the module phase, as `Trigger.enqueue` does. */
  static module<T>(fn: (e: T) => void, target: Target = Target.Subject, text: string = ""): ActionDef {
    const def = new ActionDef(ActionKind.Module, target).id(keyOf(changetype<usize>(fn)));
    return text.length ? def.withText(text) : def;
  }
  /** Raises module event `event` on `target` with `value` and `value2`. */
  static raiseEvent(target: Target, event: i32, value: i64, value2: i64): ActionDef {
    return new ActionDef(ActionKind.RaiseEvent, target).id(event).value(value).value2(value2);
  }
  /** Writes `line` to the in-game log. */
  static log(line: string): ActionDef {
    return new ActionDef(ActionKind.Log, Target.Subject).withText(line);
  }
  /** Restores `amount` health to `target`. */
  static heal(target: Target, amount: i32): ActionDef {
    return new ActionDef(ActionKind.Heal, target).value(amount);
  }
  /** Damages `target` by `amount` with `warhead` (a type reference). */
  static damage(target: Target, warhead: i64, amount: i32): ActionDef {
    return new ActionDef(ActionKind.Damage, target).id(<i32>warhead).value(amount);
  }
  /** Writes `prop` of `target`, as `set` does. */
  static setProperty(target: Target, prop: Prop, value: i64): ActionDef {
    return new ActionDef(ActionKind.SetProperty, target).id(prop).value(value);
  }
}

// -------------------------------------------------------------- Triggers

/** Most events, clauses per event, actions and bindings one `Trigger` holds; more make `done` answer `NO_HANDLE` (`Refused`). */
export const MAX_EVENTS: i32 = 4;
export const MAX_EVENT_CLAUSES: i32 = 4;
export const MAX_ACTIONS: i32 = 8;
export const MAX_BINDS: i32 = 8;

const EVENT_BYTES: i32 = sys.EVENT_DEF_BYTES + MAX_EVENT_CLAUSES * sys.CLAUSE_BYTES;
const BUFFER_BYTES: i32 = sys.TRIGGER_DEF_BYTES + MAX_EVENTS * EVENT_BYTES + MAX_ACTIONS * sys.ACTION_DEF_BYTES;
const BUFFER: usize = memory.data(BUFFER_BYTES, 8);

// TriggerDef
const TD_HOUSE: i32 = 0;
const TD_REPEAT: i32 = 8;
const TD_DIFFICULTY: i32 = 12;
const TD_FLAGS: i32 = 16;
const TD_DECIDE_KEY: i32 = 20;
const TD_EVENT_COUNT: i32 = 24;
const TD_ACTION_COUNT: i32 = 28;
// EventDef
const ED_KIND: i32 = 0;
const ED_OP: i32 = 4;
const ED_INTERVAL: i32 = 8;
const ED_ID: i32 = 12;
const ED_VALUE: i32 = 16;
const ED_CLAUSE_COUNT: i32 = 24;

/** A trigger handle. */
export type TriggerRef = i64;

export namespace TriggerRef {
  export function bind(t: TriggerRef, reference: Ref): Status {
    return check(sys.trigger_bind(t, reference));
  }
  export function unbind(t: TriggerRef, reference: Ref): Status {
    return check(sys.trigger_unbind(t, reference));
  }
  /** Removes the trigger and every instance of it. */
  export function remove(t: TriggerRef): Status {
    return check(sys.trigger_remove(t));
  }
  /** Writes the trigger's `Enabled` property. */
  export function setEnabled(t: TriggerRef, enabled: bool): Status {
    return set(t, Prop.Enabled, enabled ? 1 : 0);
  }
  /**
   * Binds `t` to every type of name kind `kind` (`find` with scope `Types`
   * and the kind's wildcard, then `trigger_bind` once per type). Returns
   * the number bound; `lastStatus()` is the first failure, if any.
   */
  export function bindAllTypes(t: TriggerRef, kind: NameKind): i32 {
    const find = Find.types(TypeRef.any(kind));
    let bound = 0;
    let failed = Status.Ok;
    let after: i64 = 0;
    do {
      const page = find.page(TYPE_PAGE, after);
      if (lastStatus() != Status.Ok) return bound;
      const count = page.count;
      after = page.next;
      for (let i = 0; i < count; i++) {
        const status = check(sys.trigger_bind(t, unchecked(TYPE_PAGE[i])));
        if (status == Status.Ok) bound++;
        else if (failed == Status.Ok) failed = status;
      }
    } while (after != 0);
    check(failed);
    return bound;
  }
}

const TYPE_PAGE = new StaticArray<i64>(64);

/**
 * A trigger under construction (appendix section 3, "Triggers"). Nothing
 * reaches the engine before `done` or `decideSync`.
 */
export class Trigger {
  private header: StaticArray<u8> = new StaticArray<u8>(sys.TRIGGER_DEF_BYTES);
  private events: StaticArray<u8> = new StaticArray<u8>(MAX_EVENTS * EVENT_BYTES);
  private actions: Array<ActionDef> = new Array<ActionDef>();
  private binds: Array<i64> = new Array<i64>();
  private overflow: bool = false;

  private constructor() {
    const h = changetype<usize>(this.header);
    store<i32>(h, 2, TD_REPEAT);
    store<i32>(h, 0b111, TD_DIFFICULTY);
    store<i32>(h, -1, TD_DECIDE_KEY);
  }

  private get eventCount(): i32 {
    return load<i32>(changetype<usize>(this.header), TD_EVENT_COUNT);
  }

  /** The latest event's `EventDef`, or 0 before any. */
  private get latest(): usize {
    const count = this.eventCount;
    if (count == 0) return 0;
    return changetype<usize>(this.events) + <usize>((count - 1) * EVENT_BYTES);
  }

  // Handle functions (appendix section 3, "Bindings"), the same as `TriggerRef`'s.

  /** Binds trigger `t` to `reference`. */
  static bind(t: TriggerRef, reference: Ref): Status {
    return TriggerRef.bind(t, reference);
  }
  /** Unbinds trigger `t` from `reference`. */
  static unbind(t: TriggerRef, reference: Ref): Status {
    return TriggerRef.unbind(t, reference);
  }
  /** Removes trigger `t` and every instance of it. */
  static remove(t: TriggerRef): Status {
    return TriggerRef.remove(t);
  }
  /** Writes trigger `t`'s `Enabled` property. */
  static setEnabled(t: TriggerRef, enabled: bool): Status {
    return TriggerRef.setEnabled(t, enabled);
  }
  /** Binds trigger `t` to every type of `kind`; see `TriggerRef.bindAllTypes`. */
  static bindAllTypes(t: TriggerRef, kind: NameKind): i32 {
    return TriggerRef.bindAllTypes(t, kind);
  }

  /** A repeating trigger on event `kind` for every difficulty. */
  static on(kind: i32): Trigger {
    return new Trigger().and(kind);
  }
  /** A trigger that fires every `interval` frames (`Timer`), counted from its creation. */
  static every(interval: i32): Trigger {
    const t = Trigger.on(Event.Timer);
    store<i32>(t.latest, interval, ED_INTERVAL);
    return t;
  }
  /** A trigger on module event `event` (`Ini.event`). */
  static onModuleEvent(event: i32): Trigger {
    return Trigger.on(Event.ModuleEvent).id(event);
  }
  /** `PropertyPoll`: fires when `prop op value` becomes true, checked every `interval` frames. */
  static onProperty(prop: Prop, op: Op, value: i64, interval: i32): Trigger {
    return Trigger.on(Event.PropertyPoll).poll(prop, op, value, interval);
  }
  /** `VariablePoll`: fires when `v op value` becomes true, checked every `interval` frames. */
  static onVariable(v: Var, op: Op, value: i64, interval: i32): Trigger {
    return Trigger.on(Event.VariablePoll).poll(v.id, op, value, interval);
  }
  /** `HousePowerPoll`: the trigger's house's power output minus drain `op value`. */
  static onHousePower(op: Op, value: i64, interval: i32): Trigger {
    return Trigger.on(Event.HousePowerPoll).poll(-1, op, value, interval);
  }
  /** `ModifierCountPoll`: this module's modifiers in `group` on a bound object `op count`. */
  static onModifierCount(group: i32, op: Op, count: i64, interval: i32): Trigger {
    return Trigger.on(Event.ModifierCountPoll).poll(group, op, count, interval);
  }
  /** `DistancePoll`: a bound object's distance in leptons to `cell` `op leptons`. */
  static onDistance(cell: Ref, op: Op, leptons: i32, interval: i32): Trigger {
    return Trigger.on(Event.DistancePoll).poll(leptons, op, cell, interval);
  }

  private poll(id: i32, op: Op, value: i64, interval: i32): Trigger {
    const e = this.latest;
    store<i32>(e, id, ED_ID);
    store<i32>(e, <i32>op, ED_OP);
    store<i64>(e, value, ED_VALUE);
    store<i32>(e, interval, ED_INTERVAL);
    return this;
  }

  /** Sets the latest event's `value` argument (a stock event's house index, credits, seconds, ...). */
  value(value: i64): Trigger {
    const e = this.latest;
    if (e) store<i64>(e, value, ED_VALUE);
    else this.overflow = true;
    return this;
  }
  /** Sets the latest event's `id` argument. */
  id(id: i32): Trigger {
    const e = this.latest;
    if (e) store<i32>(e, id, ED_ID);
    else this.overflow = true;
    return this;
  }
  /** Adds another event; all must occur before the trigger fires. */
  and(kind: i32): Trigger {
    const count = this.eventCount;
    if (count >= MAX_EVENTS) {
      this.overflow = true;
      return this;
    }
    const e = changetype<usize>(this.events) + <usize>(count * EVENT_BYTES);
    store<i32>(e, kind, ED_KIND);
    store<i32>(e, -1, ED_ID);
    store<i32>(changetype<usize>(this.header), count + 1, TD_EVENT_COUNT);
    return this;
  }
  /** Fires once per instance instead of every time. */
  once(): Trigger {
    store<i32>(changetype<usize>(this.header), 0, TD_REPEAT);
    return this;
  }
  /** The trigger's house, in place of the subject's owner. */
  house(house: House): Trigger {
    store<i64>(changetype<usize>(this.header), house, TD_HOUSE);
    return this;
  }
  /** Only on these difficulties: bit 0 easy, 1 normal, 2 hard. */
  difficulty(mask: i32): Trigger {
    store<i32>(changetype<usize>(this.header), mask, TD_DIFFICULTY);
    return this;
  }
  /** Starts disabled (`TriggerRef.setEnabled` turns it on). */
  disabled(): Trigger {
    const h = changetype<usize>(this.header);
    store<i32>(h, load<i32>(h, TD_FLAGS) | 1, TD_FLAGS);
    return this;
  }

  private clause(subject: u8, scope: u8, prop: i32, op: Op, value: i64): Trigger {
    const e = this.latest;
    if (!e) {
      this.overflow = true;
      return this;
    }
    const count = load<i32>(e, ED_CLAUSE_COUNT);
    if (count >= MAX_EVENT_CLAUSES) {
      this.overflow = true;
      return this;
    }
    const c = e + <usize>(sys.EVENT_DEF_BYTES + count * sys.CLAUSE_BYTES);
    store<u8>(c, subject, 0);
    store<u8>(c, <u8>op, 1);
    store<u8>(c, scope, 2);
    store<i32>(c, prop, 4);
    store<i64>(c, value, 8);
    store<i32>(e, count + 1, ED_CLAUSE_COUNT);
    return this;
  }

  /** The latest event counts only when the subject's `prop op value` holds. */
  where(prop: Prop, op: Op, value: i64): Trigger {
    return this.clause(0, 0, prop, op, value);
  }
  /** As `where`, on the event's other object. */
  whereOther(prop: Prop, op: Op, value: i64): Trigger {
    return this.clause(1, 0, prop, op, value);
  }
  /** As `where`, on the trigger's house. */
  whereHouse(prop: Prop, op: Op, value: i64): Trigger {
    return this.clause(2, 0, prop, op, value);
  }
  /** As `where`, on variable `v` of `target`. */
  whereVar(target: VarTarget, v: Var, op: Op, value: i64): Trigger {
    return this.clause(3, <u8>target, v.id, op, value);
  }

  /** Binds the trigger to `reference` (an object, a type, a weapon, warhead or projectile type, or a cell) at `done`. */
  bind(reference: Ref): Trigger {
    if (this.binds.length >= MAX_BINDS) this.overflow = true;
    else this.binds.push(reference);
    return this;
  }
  /** Runs `action` when the trigger fires. */
  immediate(action: ActionDef): Trigger {
    if (this.actions.length >= MAX_ACTIONS) this.overflow = true;
    else this.actions.push(action);
    return this;
  }
  /** Delivers the event to `fn` in the module phase. */
  enqueue<T>(fn: (e: T) => void): Trigger {
    return this.immediate(ActionDef.module<T>(fn));
  }
  /** As `enqueue`, with the record's target and parameter text (`e.record.argsText`). */
  enqueueWith<T>(fn: (e: T) => void, target: Target, text: string): Trigger {
    return this.immediate(ActionDef.module<T>(fn, target, text));
  }

  /**
   * Makes this a decision trigger (RFC 6f) whose requests reach `fn`
   * synchronously, and creates it as `done` does. Its event must have a
   * decision form; such a trigger takes no actions.
   */
  decideSync<T>(fn: (d: T) => void): TriggerRef {
    store<i32>(changetype<usize>(this.header), keyOf(changetype<usize>(fn)), TD_DECIDE_KEY);
    return this.done();
  }

  /**
   * Creates the trigger, then binds it. Returns the trigger, or `NO_HANDLE`
   * when none was created (`lastStatus()` says why; an overflowing builder
   * is `Refused`). A failed binding still returns the trigger, with
   * `lastStatus()` the binding's status.
   */
  done(): TriggerRef {
    if (this.overflow) {
      check(Status.Refused);
      return NO_HANDLE;
    }
    store<i32>(changetype<usize>(this.header), this.actions.length, TD_ACTION_COUNT);
    let at = BUFFER;
    memory.copy(at, changetype<usize>(this.header), sys.TRIGGER_DEF_BYTES);
    at += sys.TRIGGER_DEF_BYTES;
    for (let i = 0; i < this.eventCount; i++) {
      const e = changetype<usize>(this.events) + <usize>(i * EVENT_BYTES);
      const len = sys.EVENT_DEF_BYTES + load<i32>(e, ED_CLAUSE_COUNT) * sys.CLAUSE_BYTES;
      memory.copy(at, e, len);
      at += len;
    }
    for (let i = 0; i < this.actions.length; i++) {
      memory.copy(at, unchecked(this.actions[i]).ptr, sys.ACTION_DEF_BYTES);
      at += sys.ACTION_DEF_BYTES;
    }
    if (check(sys.trigger_create(BUFFER, <i32>(at - BUFFER), OUT)) != Status.Ok) return NO_HANDLE;
    const trigger = load<i64>(OUT);
    for (let i = 0; i < this.binds.length; i++) {
      if (check(sys.trigger_bind(trigger, unchecked(this.binds[i]))) != Status.Ok) break;
    }
    return trigger;
  }
}
