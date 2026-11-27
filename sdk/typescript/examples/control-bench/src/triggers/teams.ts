// TeamTypes and the objects stock actions make (the Rust
// `triggers/teams.rs`): the bench's map (`map.ini`, merged into XMP03T4)
// adds two TeamTypes for the player's house: `BenchTeam`, one Tank
// Destroyer, and `BenchChronoTeam`, one Sniper. Neither type is in the
// scene otherwise, so a `Birth` bound to the type is the reinforcement's.
// A Gap Generator (no one else builds one) is the trigger-made building.
// Each new object is removed once its `Birth` is read.
//
// `Team` of a member is marked (`triggers.prop-team`, not in `MARKS`) when
// the engine reads it.
import {
  Action, Event, EventRecord, NO_HANDLE, NO_ID, NameKind, Prop, Rules, Status, Trigger, TypeRef, Waypoint,
  get, lastStatus, lookup, nameOf,
} from "@vera20k/sdk";
import { benchFail, is, made, mark, marked, player } from "../bench";
import { finaleStarted } from "../finale";
import { CHRONO_TEAM, TEAM, TEAM_PHASE, TYPE_GAP, TYPE_SNIPE, TYPE_TNKD, have, ld, st } from "./state";

/** The schedule, in frames after the arena opened. */
const LOOKUP: i64 = 600;
const REINFORCE: i64 = 610;
const REINFORCE_AT: i64 = 700;
const CHRONO: i64 = 800;
const BUILDING: i64 = 900;
/** Frames between the building's tries. */
const BUILDING_EVERY: i64 = 30;

/** Reinforcement phases (`TEAM_PHASE`): which action the next Tank Destroyer's `Birth` belongs to. */
const PHASE_REINFORCE: i64 = 1;
const PHASE_REINFORCE_AT: i64 = 2;

/** The work of frame `t` after the arena opened. */
export function teamsStep(t: i64): void {
  if (t == LOOKUP) lookups();
  if (!have(TEAM) || !have(CHRONO_TEAM)) return;
  if (t == REINFORCE) {
    st(TEAM_PHASE, PHASE_REINFORCE);
    if (Action.reinforce(ld(TEAM)) != Status.Ok) {
      benchFail("triggers.ev-birth-reinforce", "stock 7 answered an error");
    }
  } else if (t == REINFORCE_AT) {
    st(TEAM_PHASE, PHASE_REINFORCE_AT);
    if (Action.reinforceAt(ld(TEAM), Waypoint.number(1)) != Status.Ok) {
      benchFail("triggers.ev-birth-reinforce-at", "stock 80 answered an error");
    }
  } else if (t == CHRONO) {
    if (Action.reinforceByChrono(ld(CHRONO_TEAM), Waypoint.number(2)) != Status.Ok) {
      benchFail("triggers.ev-birth-chrono", "stock 107 answered an error");
    }
  }
  // The building, at start waypoint 0, 1, 2 and 3 until one takes it.
  const tries = t - BUILDING;
  if (have(TYPE_GAP) && tries >= 0 && tries < 4 * BUILDING_EVERY && tries % BUILDING_EVERY == 0
    && !marked("triggers.ev-birth-building")) {
    const at = Waypoint.number(<i32>(tries / BUILDING_EVERY));
    Action.createBuildingAt(at, ld(TYPE_GAP), player());
  }
}

/** Keeps a TeamType of `name` in `slot`; false (failed) when the map has none or `name_of` differs. */
function team(slot: i32, name: string): bool {
  const id = lookup(NameKind.TeamType, name);
  if (id == NO_ID) {
    benchFail("triggers.team-lookup", "the map has no bench TeamType");
    return false;
  }
  const back = nameOf(NameKind.TeamType, id);
  if (lastStatus() != Status.Ok || back.toLowerCase() != name.toLowerCase()) {
    benchFail("triggers.team-lookup", "name_of names another TeamType");
    return false;
  }
  // A type reference (never 0), which the stock actions take.
  st(slot, TypeRef.make(NameKind.TeamType, id));
  return true;
}

/** Keeps TechnoType `name` in `slot` with a `Birth` trigger on it to `fn`. */
function birth(slot: i32, name: string, fn: (r: EventRecord) => void): void {
  const ty = Rules.technoType(name);
  if (ty == NO_HANDLE) {
    benchFail("triggers.setup", "a retail TechnoType is missing");
    return;
  }
  if (made(Trigger.on(Event.Birth).bind(ty).enqueue(fn).done())) st(slot, ty);
  else benchFail("triggers.create", "a Birth trigger could not be made");
}

/** The TeamTypes by name, with their names read back, and the `Birth` triggers on the members' and the building's types. */
function lookups(): void {
  if (!team(TEAM, "BenchTeam")) return;
  if (!team(CHRONO_TEAM, "BenchChronoTeam")) return;
  mark("triggers.team-lookup");
  birth(TYPE_TNKD, "TNKD", onBirthTeam);
  birth(TYPE_SNIPE, "SNIPE", onBirthChrono);
  birth(TYPE_GAP, "GAGAP", onBirthBuilding);
}

// Which `Birth` trigger delivered the record (the Rust keys).
const BIRTH_TEAM: i32 = 0;
const BIRTH_CHRONO: i32 = 1;
const BIRTH_BUILDING: i32 = 2;

function onBirthTeam(r: EventRecord): void {
  if (finaleStarted()) return;
  record(r, BIRTH_TEAM);
}

function onBirthChrono(r: EventRecord): void {
  if (finaleStarted()) return;
  record(r, BIRTH_CHRONO);
}

function onBirthBuilding(r: EventRecord): void {
  if (finaleStarted()) return;
  record(r, BIRTH_BUILDING);
}

/** A `Birth` of this file's triggers: the player's new object. */
function record(r: EventRecord, which: i32): void {
  const made = r.subject;
  if (!is(made, Prop.Owner, player())) {
    benchFail("triggers.ev-birth-team", "the new object is not the player's");
    return;
  }
  let name = "";
  if (which == BIRTH_TEAM && ld(TEAM_PHASE) == PHASE_REINFORCE) name = "triggers.ev-birth-reinforce";
  else if (which == BIRTH_TEAM && ld(TEAM_PHASE) == PHASE_REINFORCE_AT) name = "triggers.ev-birth-reinforce-at";
  else if (which == BIRTH_CHRONO) name = "triggers.ev-birth-chrono";
  else if (which == BIRTH_BUILDING) name = "triggers.ev-birth-building";
  else return;
  mark(name);
  // A member's team, where the engine reads it.
  if (which != BIRTH_BUILDING) {
    const team = get(made, Prop.Team);
    if (lastStatus() == Status.Ok && team != 0 && team != NO_HANDLE) mark("triggers.prop-team");
  }
  Action.remove(made);
}
