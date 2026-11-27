// "Siege Doctrine", a playtest `rules.wasm` about the game's rules and its
// decisions, the same as the Rust SDK's `sdk/examples/siege-doctrine`:
//
// - `onRules`: reads and rewrites rules while they load. Base defenses
//   (pillbox, sentry gun, prism tower, tesla coil, flak cannon) get twice
//   their `Strength`.
// - `ProductionAllowed`: heavy tanks (Apocalypse, prism tank, tank
//   destroyer) leave the sidebar for everyone until minute 5.
// - `ProductionQueued`: superweapon buildings cannot be queued until
//   minute 10.
// - `CommandAllowed`: for the first 3 minutes engineers refuse any order
//   aimed at an object, so there is no early engineer rush.
// - `Defeat`: a human player who has lost everything is not defeated while
//   they still hold 3000 credits; the veto holds until the money drops.
//
// Decision handlers run synchronously while the engine decides and may
// only read; the announcements come from `onFrame` instead.
import {
  CommandAllowedDecision, DefeatDecision, Event, Every, Ini, NO_HANDLE, Prop,
  ProductionAllowedDecision, ProductionQueuedDecision, Rules, Status, Trigger, TriggerRef,
  TypeRef, frame, get, lastStatus, log, onFrame, onRules, onScenarioStart,
} from "@vera20k/sdk";

const MINUTE: i32 = 900;
const HEAVY_UNLOCK: i32 = 5 * MINUTE;
const SUPERWEAPON_UNLOCK: i32 = 10 * MINUTE;
const ENGINEER_UNLOCK: i32 = 3 * MINUTE;
const LAST_CHANCE_CREDITS: i64 = 3000;

const DEFENSES: StaticArray<string> = ["GAPILL", "NALASR", "ATESLA", "TESLA", "NAFLAK"];
const HEAVY: StaticArray<string> = ["APOC", "SREF", "TNKD"];
const SUPERWEAPON_BUILDINGS: StaticArray<string> =
  ["GAWEAT", "NAMISL", "GACSPH", "NAIRON", "YAPPET", "YAGNTC"];
const ENGINEER_TYPES: StaticArray<string> = ["ENGINEER", "SENGINEER", "YENGINEER"];

/** Bindings one trigger builder holds; more are bound after it is made. */
const BUILDER_BINDS: i32 = 8;
const late = new StaticArray<TypeRef>(BUILDER_BINDS);

/** The last unlock announced: 0 none, then 1, 2 and 3. */
let announced: i32 = 0;

// ------------------------------------------------------------------ Rules

/** Doubles each defense's `Strength` while the rules load. */
onRules((): void => {
  let doubled = 0;
  for (let i = 0; i < DEFENSES.length; i++) {
    const section = DEFENSES[i];
    const strength = Ini.int(Rules.get(section, "Strength"));
    // As Rust's `parse::<i32>()`: a decimal that fits an i32.
    if (lastStatus() != Status.Ok || strength < i32.MIN_VALUE || strength > i32.MAX_VALUE) continue;
    if (Rules.set(section, "Strength", (<i32>strength * 2).toString()) == Status.Ok) doubled++;
  }
  if (doubled == 0) log("Siege Doctrine: no defense found to strengthen");
});

// --------------------------------------------------------------- Decisions

function heavyTanks(d: ProductionAllowedDecision): void {
  if (frame() < HEAVY_UNLOCK) d.setResult(0);
}

function superweapons(d: ProductionQueuedDecision): void {
  if (frame() < SUPERWEAPON_UNLOCK) d.setResult(0);
}

function engineers(d: CommandAllowedDecision): void {
  if (frame() < ENGINEER_UNLOCK && d.target != NO_HANDLE) d.setResult(0);
}

function lastChance(d: DefeatDecision): void {
  const house = d.house;
  const isHuman = get(house, Prop.IsHuman);
  const human = lastStatus() == Status.Ok && isHuman == 1;
  let money = get(house, Prop.Money);
  if (lastStatus() != Status.Ok) money = 0;
  if (human && money >= LAST_CHANCE_CREDITS) d.setResult(0);
}

// ------------------------------------------------------------------ Start

/**
 * A decision trigger on `event` bound to each type of `names`; a trigger
 * builder holds 8 bindings, more are bound after it is made.
 */
function decide<T>(event: i32, names: StaticArray<string>, fn: (d: T) => void, what: string): void {
  const trigger = Trigger.on(event);
  let lateLen = 0;
  for (let i = 0; i < names.length; i++) {
    const ty = Rules.technoType(names[i]);
    if (ty == NO_HANDLE) continue;
    if (i < BUILDER_BINDS) trigger.bind(ty);
    else if (lateLen < late.length) late[lateLen++] = ty;
  }
  const made = trigger.decideSync<T>(fn);
  // A failed binding is a refusal too, as the Rust SDK's `decide_sync` answers.
  if (made == NO_HANDLE || lastStatus() != Status.Ok) {
    log("Siege Doctrine: the trigger for " + what + " was refused");
    return;
  }
  for (let i = 0; i < lateLen; i++) TriggerRef.bind(made, late[i]);
}

onScenarioStart((): void => {
  log("Siege Doctrine: base defenses have double strength.");
  log("Engineers can't capture before 3:00, heavy tanks unlock at 5:00, superweapons at 10:00.");
  decide<ProductionAllowedDecision>(Event.ProductionAllowed, HEAVY, heavyTanks, "heavy tanks");
  decide<ProductionQueuedDecision>(Event.ProductionQueued, SUPERWEAPON_BUILDINGS, superweapons, "superweapons");
  decide<CommandAllowedDecision>(Event.CommandAllowed, ENGINEER_TYPES, engineers, "engineers");
  if (Trigger.on(Event.Defeat).decideSync(lastChance) == NO_HANDLE) {
    log("Siege Doctrine: the defeat trigger was refused");
  }
});

// ----------------------------------------------------------- Announcements

onFrame(new Every(15), (): void => {
  const now = frame();
  let next = 0;
  let at = 0;
  let text = "";
  if (announced == 0) {
    next = 1;
    at = ENGINEER_UNLOCK;
    text = "Siege Doctrine: 3:00 - engineers may capture buildings now.";
  } else if (announced == 1) {
    next = 2;
    at = HEAVY_UNLOCK;
    text = "Siege Doctrine: 5:00 - heavy tanks are on the sidebar.";
  } else if (announced == 2) {
    next = 3;
    at = SUPERWEAPON_UNLOCK;
    text = "Siege Doctrine: 10:00 - superweapons can be built.";
  } else {
    return;
  }
  if (now >= at) {
    log(text);
    announced = next;
  }
});
