// The interface's side of the simulation (`ui.wasm`, `data/proto.ts`): the
// camp published as the player's private variables (declared in
// `vera.json`), and the buttons' messages turned into the orders they
// stand for. (Owner: lead)

import { House, HouseVar, Status } from "@vera20k/sdk";
import * as proto from "../../data/proto";
import * as battle from "./battle";
import * as campaign from "./campaign";
import * as perks from "./perks";
import * as shop from "./shop";
import * as state from "./state";
import {
  MfBanners, MfBattle, MfChoices, MfCompanyHigh, MfCompanyLow, MfFlags, MfFront, MfLast, MfOffers,
  MfPhase, MfPicks, MfRank, MfTaken, MfWave,
} from "./generated/vars";

const VARS: i32 = 14;
/** The variables in `values` order. */
const ALL: StaticArray<HouseVar> = [
  MfPhase, MfWave, MfBanners, MfRank, MfPicks, MfFlags, MfOffers, MfChoices, MfCompanyLow,
  MfCompanyHigh, MfTaken, MfBattle, MfFront, MfLast,
];
/** What each variable was last written as, so an unchanged one costs no host call. */
const WRITTEN = new StaticArray<i64>(VARS);
const VALUES = new StaticArray<i64>(VARS);

/** Writes the variables that changed. Every frame once the camp is open. */
export function publish(): void {
  if (state.phase == state.WAITING) return;
  const player = state.player();
  shop.uiView();
  battle.standing();
  let taken: i64 = 0;
  for (let perk = 0; perk < perks.PERKS; perk++) taken = proto.withTaken(taken, perk, perks.takenOf(perk));
  VALUES[0] = state.phase;
  VALUES[1] = state.wave;
  VALUES[2] = state.banners;
  VALUES[3] = state.campRank;
  VALUES[4] = state.picks;
  VALUES[5] = shop.viewFlags;
  VALUES[6] = shop.viewOffers;
  VALUES[7] = shop.viewChoices;
  VALUES[8] = shop.viewCompanyLow;
  VALUES[9] = shop.viewCompanyHigh;
  VALUES[10] = taken;
  VALUES[11] = proto.battle(battle.standingOurs, battle.standingTheirs, battle.standingArrived);
  VALUES[12] = battle.frontIndex();
  VALUES[13] = campaign.last();
  for (let at = 0; at < VARS; at++) {
    const value = VALUES[at];
    if (WRITTEN[at] != value && ALL[at].set(player, value) == Status.Ok) WRITTEN[at] = value;
  }
}

/**
 * A button's message: `[what, index]` from the player, in a muster. The
 * order it stands for joins the frame's orders.
 */
export function message(sender: House, data: Uint8Array): void {
  if (data.length != 2) return;
  if (sender != state.player() || state.phase != state.MUSTER) return;
  shop.uiOrder(data[0], <i32>data[1]);
}
