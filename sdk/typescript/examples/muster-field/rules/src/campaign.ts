// The waves as a campaign: pay, banners and the end. (Owner: battle)

import * as waves from "../../data/waves";
import { packLast } from "../../data/proto";
import * as battle from "./battle";
import * as perks from "./perks";
import * as state from "./state";
import { Line, say } from "./text";
import * as world from "./world";

const PURSE: i64 = 1600;
const START_BANNERS: i64 = 3;
const WAGE: i64 = 420;
/** Added to the wage per wave. */
const WAGE_STEP: i64 = 115;

const NOTHING: i64 = 0;
const WON: i64 = 1;
const LOST: i64 = 2;
/** What `settle` leaves for the next muster's one line. */
let result: i64 = NOTHING;
let kills: i64 = 0;
let paid: i64 = 0;
let supply: i64 = 0;
/** The last battle as `packLast` packs it; unlike `result` it stays. */
let lastPacked: i64 = 0;

export const NEXT_MUSTER: i32 = 0;
export const NEXT_VICTORY: i32 = 1;
export const NEXT_DEFEAT: i32 = 2;

function clamp(value: i64, lo: i64, hi: i64): i64 {
  return value < lo ? lo : value > hi ? hi : value;
}

/** The purse and the banners of a new campaign. */
export function begin(): void {
  state.setWave(1);
  state.setBanners(START_BANNERS);
  state.setPicks(0);
  result = NOTHING;
  lastPacked = 0;
  const change = clamp(PURSE - world.credits(), <i64>i32.MIN_VALUE, <i64>i32.MAX_VALUE);
  world.pay(<i32>change);
}

/**
 * Pays for the battle, win or lose. It prints only when the campaign is
 * over: otherwise the muster follows in the same frame, whose one campaign
 * line is `announce`'s.
 */
export function settle(won: bool, reportKills: i64): i32 {
  const number = clamp(state.wave, 1, <i64>waves.WAVES);
  const wave = waves.get(number);
  const killed = clamp(reportKills, 0, <i64>wave.size());
  const wage = WAGE + WAGE_STEP * number;
  const bounty = killed * <i64>wave.bounty;
  const bonus = <i64>perks.bonusPay(killed, won);
  const due = clamp(wage + bounty + bonus, 0, <i64>i32.MAX_VALUE);
  const got = world.pay(<i32>due) ? due : 0;
  result = won ? WON : LOST;
  kills = killed;
  paid = got;
  supply = 0;

  let next = NEXT_MUSTER;
  if (!won) {
    const left = max<i64>(state.banners - 1, 0);
    state.setBanners(left);
    next = left == 0 ? NEXT_DEFEAT : NEXT_MUSTER;
  } else if (number >= <i64>waves.WAVES) {
    next = NEXT_VICTORY;
  } else {
    state.setWave(number + 1);
    if (number == 2 || number == 4 || number == 6) {
      state.setPicks(state.picks + 1);
      supply = 1;
    }
  }
  lastPacked = packLast(won ? 1 : 2, supply == 1, killed, got);
  if (next != NEXT_MUSTER) {
    say(outcome(new Line()));
    result = NOTHING;
  }
  return next;
}

/** The last battle in a few words, if there was one. */
function outcome(line: Line): Line {
  let word: string;
  if (result == WON) word = "胜，歼";
  else if (result == LOST) word = "败，歼";
  else return line;
  line.text(word).num(kills).text("，得").num(paid);
  return supply == 1 ? line.text("＋补给。") : line.text("。");
}

/**
 * The one campaign line of a muster: how the last battle went, then the
 * wave, where it comes from, what it is and the banners.
 */
export function announce(): void {
  const number = state.wave;
  const wave = waves.get(number);
  say(outcome(new Line())
    .text("第")
    .num(number)
    .text("/")
    .num(<i64>waves.WAVES)
    .text("波【")
    .text(wave.name)
    .text("】自")
    .text(battle.frontName())
    .text("来：")
    .text(wave.hint)
    .text("。军旗")
    .num(state.banners));
  result = NOTHING;
}

/** The last battle, packed, for the interface. */
export function last(): i64 {
  return lastPacked;
}
