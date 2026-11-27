// "Muster Field", a playtest `rules.wasm` (RFC 0001 preview), the port of
// the Rust SDK's `sdk/examples/muster-field`: a small auto-battler against
// one computer house, played with ordinary unit orders or with its
// `ui.wasm`'s panels. Its play follows the 红警酒馆 of MK, a Bilibili
// creator (README.md); it is a demonstration, not to be published as a mod
// of its own.
//
// - A campaign of waves: between battles the company stands in camp, the
//   dead return, and the player spends the pay.
// - Hire by ordering a mercenary out of the row, dismiss by ordering one
//   back into it. Stop (S) redraws the row, Guard (G) keeps an offer.
// - The quartermaster: Guard improves the camp, Scatter (X) starts the
//   battle, as Scatter on any hired unit does.
// - Three of a type merge into a champion. Merges and won waves bring
//   supplies: order one of three trucks away to pick one for the company.
//
// What it shows of the API: `CommandAllowed` as player input (the handler
// may not write, so it queues the order in module memory and `onFrame`
// carries it out), `SetMission` for a human house's units (`House.order`
// takes computer houses only), modifiers, searches, and a `Defeat` veto
// while both houses have nothing on the field. Nothing waits on frames for
// the player: game speed 0 is unlimited.
//
// It is made for one human player against computer opponents, with Bases
// on. What it tells the player is in Simplified Chinese.
//
// The interface (`../ui`) shows the camp and has a button for every key:
// the simulation publishes the camp as variables and turns each button's
// message into the order that key gives (`ui.ts`). The orders keep working
// without it.
//
// `index` owns input and the phases, `shop` the camp, `battle` one fight
// with `waves`, `campaign` the pay and the end, `content` and `perks` what
// can be hired and won, `state` what they share, `ui` the interface's side;
// the tables live in `../../data/`, which `ui.wasm` imports too.
import {
  Action, CommandAllowedDecision, DefeatDecision, Event, Every, Find, House, NO_HANDLE, Op, Outcome,
  Prop, Rules, Status, Trigger, TriggerRef, frame, get, lastStatus, log, onFrame, onMessage,
  onScenarioStart,
} from "@vera20k/sdk";
import * as content from "../../data/content";
import * as battle from "./battle";
import * as campaign from "./campaign";
import * as inbox from "./inbox";
import * as perks from "./perks";
import * as shop from "./shop";
import * as state from "./state";
import { Line, say } from "./text";
import * as ui from "./ui";
import * as world from "./world";

/** What a computer opponent starts with or deploys into. */
const BASE_TYPES: StaticArray<string> = ["AMCV", "SMCV", "PCV", "GACNST", "NACNST", "YACNST"];
const HUMANS = new StaticArray<i64>(2);
const HOUSES = new StaticArray<i64>(16);

/** Runs as the engine executes a player's order. It may only read the game. */
function command(decision: CommandAllowedDecision): void {
  const unit = decision.unit;
  const phase = state.phase;
  if (phase == state.WAITING || !shop.owns(unit)) return;
  const verdict = phase == state.MUSTER ? shop.judge(unit) : shop.REFUSE;
  if (verdict == shop.ALLOW) return;
  decision.setResult(0);
  if (verdict == shop.QUEUE) inbox.queue(unit, decision.command, decision.targetCell);
}

/** No house is out while the field is empty between the phases. */
function defeat(decision: DefeatDecision): void {
  const house = decision.house;
  const playing = state.phase == state.MUSTER || state.phase == state.BATTLE;
  if (playing && (house == state.player() || house == state.enemy())) decision.setResult(0);
}

function start(): void {
  const trigger = Trigger.on(Event.CommandAllowed).decideSync<CommandAllowedDecision>(command);
  if (trigger == NO_HANDLE) {
    log("佣兵点兵场：触发器创建失败。");
    return;
  }
  const count = content.MERCS.length + shop.CRATES.length + 1;
  for (let at = 0; at < count; at++) {
    const id = at < content.MERCS.length
      ? content.MERCS[at].id
      : at < content.MERCS.length + shop.CRATES.length
        ? shop.CRATES[at - content.MERCS.length]
        : shop.OFFICER;
    const ty = Rules.technoType(id);
    if (ty == NO_HANDLE || TriggerRef.bind(trigger, ty) != Status.Ok) {
      say(new Line().text("佣兵点兵场：规则里缺少单位 ").text(id));
    }
  }
  if (Trigger.on(Event.Defeat).decideSync<DefeatDecision>(defeat) == NO_HANDLE) {
    log("佣兵点兵场：触发器创建失败。");
  }
}

function hasBase(house: House): bool {
  for (let at = 0; at < BASE_TYPES.length; at++) {
    const ty = Rules.technoType(BASE_TYPES[at]);
    if (ty != NO_HANDLE && Find.objects().ownedBy(house).ofType(ty).first() != NO_HANDLE) return true;
  }
  return false;
}

function openCamp(): void {
  const found = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(HUMANS, 0);
  if (lastStatus() != Status.Ok || found.count != 1) {
    state.setPhase(state.OVER);
    log("佣兵点兵场只支持一名玩家。");
    return;
  }
  const player: House = HUMANS[0];
  const first = Find.objects().ownedBy(player).first();
  const campCell = get(first, Prop.Cell);
  const campRead = lastStatus() == Status.Ok;
  const page = Find.houses().where(Prop.IsHuman, Op.Eq, 0).page(HOUSES, 0);
  const computers = lastStatus() == Status.Ok ? page.count : 0;
  let enemy: House = NO_HANDLE;
  for (let at = 0; at < computers; at++) {
    const house: House = HOUSES[at];
    if (hasBase(house)) {
      if (enemy == NO_HANDLE) enemy = house;
      world.clearForces(house);
    }
  }
  if (!campRead || enemy == NO_HANDLE) {
    state.setPhase(state.OVER);
    log("佣兵点兵场需要一个电脑对手，并打开“基地”选项。");
    return;
  }
  state.setSides(player, enemy, campCell);
  world.clearForces(player);
  Action.revealAll();
  campaign.begin();
  muster();
}

function muster(): void {
  state.setPhase(state.MUSTER);
  campaign.announce();
  shop.open();
}

function startBattle(): void {
  inbox.clear();
  shop.close();
  state.setPhase(state.BATTLE);
  battle.start();
}

function afterBattle(won: bool, kills: i64): void {
  const next = campaign.settle(won, kills);
  if (next == campaign.NEXT_MUSTER) muster();
  else if (next == campaign.NEXT_VICTORY) finish(Outcome.Win, "八波袭击全部击退，佣兵团名扬天下！");
  else finish(Outcome.Lose, "最后一面军旗倒下，营地失守。");
}

function finish(outcome: Outcome, text: string): void {
  state.setPhase(state.OVER);
  log(text);
  Action.endFor(state.player(), outcome);
}

function tick(): void {
  const now = frame();
  const phase = state.phase;
  if (phase == state.WAITING) {
    // The starting units are on the map by the second frame.
    if (now >= 2) openCamp();
  } else if (phase == state.MUSTER) {
    const count = inbox.take();
    if (count > 0 && shop.apply(count)) startBattle();
  } else if (phase == state.BATTLE) {
    perks.battleTick(now);
    const report = battle.tick(now);
    if (report != battle.UNDECIDED) afterBattle(report == battle.REPORT_WON, battle.reportKills);
  }
  ui.publish();
}

onScenarioStart(start);
onFrame(new Every(1), tick);
onMessage(ui.message);
