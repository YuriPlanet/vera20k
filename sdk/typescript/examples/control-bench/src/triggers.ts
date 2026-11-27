// Area `triggers` of the bench (see `index.ts`), the port of the Rust
// `triggers.rs`. Marks are named `triggers.<point>`.
//
// Module triggers (RFC 0001 sections 6a and 7): their lifecycle and
// bindings, the delivered, stock and polling events, filter clauses,
// immediate actions and chains; the `action` import (every atomic action
// the SDK has a function for, a few stock ones); house orders and their
// results (6g, 6i); a domain claim (6i); an INI trigger (6h).
//
// Its own units stand in its arena (`arena(TRIGGERS)`): player units for
// the actions and events, a few enemy-owned ones (a weaponless Rhino the
// duel's Grizzly shoots, two Rhinos the house orders drive, a Yuri that
// mind-controls a player engineer), each removed again once its part is
// done. The work runs on a schedule of frames after the arena opens
// (`triggers/setup.ts` for the units and triggers, `triggers/steps.ts` for
// the actions); `triggers/records.ts` reads the delivered records;
// `triggers/teams.ts` the TeamTypes and the objects stock actions make;
// `triggers/state.ts` holds the state they share.
//
// Not marked (left out of `MARKS`):
// - `EndFor` and the `SuspendProduction` order: the finale runs them.
// - `Push` and `SetFacing`: the `movement` area marks them.
// - `Team` of a reinforcement's member: marked `triggers.prop-team` when
//   the engine reads it, which this list does not count on.
// - The stock cell events the engine does not take (24, 31, 33, 51, 53,
//   54, 59) are not tried.
import { EventScope, Ini, NO_ID, frame as now, namedAction } from "@vera20k/sdk";
import { TRIGGERS, arena, benchFail } from "./bench";
import { ARENA, GLOBAL_EVENT, OPENED, PING, PONG, ld, st } from "./triggers/state";
import { iniDog } from "./triggers/records";
import { lookups, setupStep } from "./triggers/setup";
import { stepsStep } from "./triggers/steps";
import { teamsStep } from "./triggers/teams";

/** Every mark this area reaches in the bench scene. */
export const MARKS: StaticArray<string> = [
  // Lifecycle and bindings.
  "triggers.create",
  "triggers.bind",
  "triggers.unbind",
  "triggers.remove",
  "triggers.bind-type",
  "triggers.bind-weapon",
  "triggers.bind-warhead",
  "triggers.bind-projectile",
  "triggers.bind-cell",
  "triggers.prop-enabled",
  "triggers.prop-house",
  "triggers.prop-repeat",
  // Delivered events.
  "triggers.ev-take-damage",
  "triggers.ev-killed-warhead",
  "triggers.ev-fire",
  "triggers.ev-timer",
  "triggers.ev-timer-object",
  "triggers.ev-attacked-by",
  "triggers.ev-birth",
  "triggers.ev-transport-entered",
  "triggers.ev-passenger-added",
  "triggers.ev-transport-exited",
  "triggers.ev-passenger-removed",
  "triggers.ev-house-transferred",
  "triggers.ev-mind-control",
  "triggers.ev-sw-launched",
  "triggers.ev-mod-start",
  "triggers.ev-mod-stacks",
  "triggers.ev-mod-expire",
  "triggers.ev-module-event",
  "triggers.ev-module-global",
  // Stock events.
  "triggers.ev-stock-attacked",
  "triggers.ev-stock-destroyed",
  "triggers.ev-stock-house",
  "triggers.ev-stock-logic",
  "triggers.ev-cell-row",
  "triggers.ev-cell-column",
  // Polls.
  "triggers.poll-property",
  "triggers.poll-money",
  "triggers.poll-variable",
  "triggers.poll-power",
  "triggers.poll-modifier-count",
  "triggers.poll-distance",
  // Filter clauses.
  "triggers.filter-subject",
  "triggers.filter-other",
  "triggers.filter-house",
  // Immediate actions and chains.
  "triggers.imm-heal",
  "triggers.imm-add-money",
  "triggers.chain",
  // Atomic actions.
  "triggers.act-launch-sw",
  "triggers.act-remove",
  "triggers.act-kill",
  "triggers.act-damage",
  "triggers.act-create",
  "triggers.act-clear-target",
  "triggers.act-add-money",
  "triggers.act-set-owner",
  "triggers.act-set-var",
  "triggers.act-detonate-warhead",
  "triggers.act-log",
  "triggers.act-heal",
  "triggers.act-set-mission",
  "triggers.act-spawn-anim",
  "triggers.act-detonate-weapon",
  "triggers.act-add-experience",
  "triggers.act-add-ammo",
  "triggers.act-teleport",
  "triggers.act-clear-disguise",
  "triggers.act-clear-mind-control",
  "triggers.act-convert",
  "triggers.act-convert-timed",
  "triggers.act-set-score",
  "triggers.act-add-score",
  "triggers.act-set-property",
  "triggers.act-modify",
  "triggers.act-remove-modifiers",
  "triggers.act-fire-weapon",
  "triggers.act-attach-anim",
  "triggers.act-allow-type",
  "triggers.act-add-passenger",
  "triggers.act-raise-event",
  // Stock actions.
  "triggers.stock-timer",
  "triggers.stock-local",
  "triggers.stock-reveal",
  // House orders and their results.
  "triggers.order-move",
  "triggers.order-guard",
  "triggers.order-attack",
  "triggers.order-stop",
  "triggers.order-scatter",
  "triggers.order-queue",
  "triggers.order-rejected",
  // Claims.
  "triggers.claim",
  "triggers.claim-human",
  // INI triggers.
  "triggers.ini-trigger",
  // TeamTypes and the objects stock actions make (`teams`).
  "triggers.team-lookup",
  "triggers.ev-birth-reinforce",
  "triggers.ev-birth-reinforce-at",
  "triggers.ev-birth-chrono",
  "triggers.ev-birth-building",
];

// The module's one named action: the INI trigger's `Wasm:benchIniDog`.
namedAction("benchIniDog", iniDog);

/** Registers module event `name` into `slot`. */
function event(slot: i32, name: string, scope: EventScope): void {
  const id = Ini.event(name, scope);
  if (id != NO_ID) st(slot, id);
  else benchFail("triggers.ev-module-event", "ini::event failed");
}

/** The initialization: the module events this area raises (RFC 0001 section 6h). */
export function init(): void {
  event(PING, "Bench.TrigPing", EventScope.Object);
  event(PONG, "Bench.TrigPong", EventScope.Object);
  event(GLOBAL_EVENT, "Bench.TrigAll", EventScope.Global);
}

/** `onScenarioStart`. */
export function start(): void {}

/** The frame the arena opens: the houses and `arena` are known. */
export function open(): void {
  st(OPENED, <i64>now());
  st(ARENA, arena(TRIGGERS));
  lookups();
}

/** Every later frame. */
export function frame(at: i32): void {
  const t = <i64>at - ld(OPENED);
  setupStep(t);
  stepsStep(t);
  teamsStep(t);
}
