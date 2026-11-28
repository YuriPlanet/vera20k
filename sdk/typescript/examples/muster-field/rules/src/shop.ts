// The camp between battles. (Owner: shop)
//
// Every order for a camp unit is refused and queued (`judge`), and `apply`
// reads one frame's queue as one gesture of the player:
// - A group move arrives spread over the cells around the click
//   (`movement::group_destination`): the member nearest the group's middle
//   gets the clicked cell, so that one's cell says whether the group went
//   into the row or out of it.
// - A key sends one order per selected unit, so each key acts once.
// - What happened is one line, and the last line is the choice on the
//   field: the row left to right, or the supplies.

import {
  Action, Cell, Command, Mission, NO_HANDLE, Prop, SpeedType, Status, Techno,
  frame, get, lastStatus, log, random, set,
} from "@vera20k/sdk";
import * as camp from "../../data/camp";
import * as content from "../../data/content";
import { Merc } from "../../data/content";
import { MAX_COMPANY, MAX_OFFERS } from "../../data/index";
import * as proto from "../../data/proto";
import { INBOX, ORDER_CELL, ORDER_COMMAND, ORDER_UNIT, queue } from "./inbox";
import * as perks from "./perks";
import * as state from "./state";
import { Line, say } from "./text";
import * as world from "./world";

/** The quartermaster; armed, or the Guard key would send no order. */
export const OFFICER = "SSRV";
/** What stands for a supply to pick. */
export const CRATES: StaticArray<string> = ["TRUCKB", "PICK", "BUS"];

/** Cells between two offers. */
const SPACING: i32 = 3;
/** How far from the camp a member may stand, on either axis. */
const REACH: i32 = 9;
/** The row's dismiss zone on either side of its line. */
const ROW_DEPTH: i32 = 1;
const OFFICER_DY: i32 = -4;
const SUPPLY_DY: i32 = -7;

/** Orders in one batch at most. */
const BATCH: i32 = 32;
/**
 * Searches for a free cell that may fail in one frame: each is 121 host
 * calls, and the frame's operations are limited. The officer, created
 * first, may use them all.
 */
const MAX_FAILS: i64 = 4;
/** A unit needs this long to set off after it was ordered. */
const SET_OFF: i64 = 15;

// ------------------------------------------------------------------ State

// A place in the row. A kept one may have a kind and no unit: it comes
// back when there is room for it.
const OFFER_KIND = new StaticArray<i64>(MAX_OFFERS);
const OFFER_KEPT = new StaticArray<i64>(MAX_OFFERS);
const OFFER_UNIT = new StaticArray<i64>(MAX_OFFERS);
const OFFER_SPEED = new StaticArray<i64>(MAX_OFFERS);
const OFFER_FIRE = new StaticArray<i64>(MAX_OFFERS);
for (let at = 0; at < MAX_OFFERS; at++) OFFER_KIND[at] = -1;

let officerUnit: i64 = NO_HANDLE;
const TRUCKS = new StaticArray<i64>(3);
/**
 * The supply each truck stands for, or −1. A choice waits until it is
 * made, so waiting a muster does not draw another.
 */
const CHOICES: StaticArray<i64> = [-1, -1, -1];
/** The frame a member was last placed in this muster, or −1. */
const PLACED = new StaticArray<i64>(MAX_COMPANY);
for (let at = 0; at < MAX_COMPANY; at++) PLACED[at] = -1;
let musters: i64 = 0;
let fails: i64 = 0;

function isKept(at: i32): bool {
  return OFFER_KEPT[at] == 1;
}

function offerMerc(at: i32): Merc {
  return content.merc(<i32>max<i64>(OFFER_KIND[at], 0));
}

function clearOffer(at: i32): void {
  OFFER_KIND[at] = -1;
  OFFER_KEPT[at] = 0;
  OFFER_UNIT[at] = NO_HANDLE;
}

function takeAway(at: i32): void {
  const unit = OFFER_UNIT[at];
  if (unit != NO_HANDLE) Action.remove(unit);
  OFFER_UNIT[at] = NO_HANDLE;
}

// Roles: what a camp unit is. Offers 0..4, members 10..19, the officer,
// trucks 40..42; `NO_ROLE` for none.
const NO_ROLE: i32 = -1;
const ROLE_MEMBER: i32 = 10;
const ROLE_OFFICER: i32 = 30;
const ROLE_TRUCK: i32 = 40;

function isOffer(role: i32): bool {
  return role >= 0 && role < ROLE_MEMBER;
}
function isMember(role: i32): bool {
  return role >= ROLE_MEMBER && role < ROLE_OFFICER;
}
function isTruck(role: i32): bool {
  return role >= ROLE_TRUCK;
}

function role(unit: Techno): i32 {
  if (unit == NO_HANDLE) return NO_ROLE;
  if (officerUnit == unit) return ROLE_OFFICER;
  for (let at = 0; at < MAX_OFFERS; at++) if (OFFER_UNIT[at] == unit) return at;
  for (let at = 0; at < MAX_COMPANY; at++) {
    if (!state.isEmpty(at) && state.memberUnit[at] == unit) return ROLE_MEMBER + at;
  }
  for (let at = 0; at < 3; at++) if (TRUCKS[at] == unit) return ROLE_TRUCK + at;
  return NO_ROLE;
}

function campRank(): i32 {
  return camp.rank(state.campRank);
}

function places(): i32 {
  return camp.PLACES[campRank()];
}

function company(): i32 {
  let count = 0;
  for (let at = 0; at < MAX_COMPANY; at++) if (!state.isEmpty(at)) count++;
  return count;
}

/** The offers in the row: the first `row()` of them. */
function row(): i32 {
  return min(camp.ROW[campRank()], MAX_OFFERS);
}

function rowIsOut(): bool {
  for (let at = 0; at < row(); at++) if (OFFER_UNIT[at] != NO_HANDLE) return false;
  return true;
}

/** `pair`'s two members. */
let pairFirst: i32 = -1;
let pairSecond: i32 = -1;

/** Whether two members of `kind` stand that a third would merge with (`pairFirst`, `pairSecond`). */
function pair(kind: i64): bool {
  pairFirst = -1;
  pairSecond = -1;
  for (let at = 0; at < MAX_COMPANY; at++) {
    if (state.isEmpty(at) || state.isChampion(at) || <i64>state.kindOf(at) != kind) continue;
    if (pairFirst < 0) pairFirst = at;
    else {
      pairSecond = at;
      return true;
    }
  }
  return false;
}

function unitRank(at: i32): i64 {
  // A rank outside 0..=2 would stop the module.
  const rank = perks.rank(at);
  return rank < 0 ? 0 : rank > 2 ? 2 : rank;
}

function chebyshev(dx: i32, dy: i32): i32 {
  return max(abs(dx), abs(dy));
}

// ----------------------------------------------------------------- Orders

/** Whether `unit` is one of the camp's: offer, member, officer or supply. */
export function owns(unit: Techno): bool {
  return role(unit) != NO_ROLE;
}

/** What a player's order means in a muster. */
export const ALLOW: i32 = 0;
export const REFUSE: i32 = 1;
export const QUEUE: i32 = 2;

/**
 * Runs inside `CommandAllowed`: it must not write to the game. Even a move
 * within the line is queued, as only the whole group tells a click into
 * the row from a destination spread into it.
 */
export function judge(unit: Techno): i32 {
  return role(unit) != NO_ROLE ? QUEUE : ALLOW;
}

// The units of one press of G.
const GUARDED_OFFERS = new StaticArray<bool>(MAX_OFFERS);
let guardedOffer = false;
let guardedOfficer = false;
let guardedMember = false;

function guard(role: i32): void {
  if (isOffer(role)) {
    GUARDED_OFFERS[role] = true;
    guardedOffer = true;
  } else if (role == ROLE_OFFICER) {
    guardedOfficer = true;
  } else if (isMember(role)) {
    guardedMember = true;
  }
}

/** Carries out the `count` orders in `ORDER_*`; true when the battle shall start. */
export function apply(count: i32): bool {
  const len = min(min(count, BATCH), INBOX);
  fails = 0;
  // A key is read against the camp as the player saw it.
  let first = -1;
  for (let at = 0; at < len; at++) {
    if (role(ORDER_UNIT[at]) != NO_ROLE) {
      first = at;
      break;
    }
  }
  if (first < 0) return false;
  const firstRole = role(ORDER_UNIT[first]);
  for (let at = 0; at < MAX_OFFERS; at++) GUARDED_OFFERS[at] = false;
  guardedOffer = false;
  guardedOfficer = false;
  guardedMember = false;
  let stop = false;
  let scatter = false;
  let canStart = false;
  let listedSupplies = false;
  for (let at = 0; at < len; at++) {
    const r = role(ORDER_UNIT[at]);
    if (r == NO_ROLE) continue;
    if (isTruck(r)) listedSupplies = true;
    const command = ORDER_COMMAND[at];
    if (command == Command.Guard && guardsItself(at)) {
      guard(r);
    } else if (command == Command.Stop) {
      if (isOffer(r) || r == ROLE_OFFICER) stop = true;
    } else if (command == Command.Scatter) {
      scatter = true;
      if (r == ROLE_OFFICER || isMember(r)) canStart = true;
    }
  }
  forgetTheLost();
  if (scatter && officerUnit == NO_HANDLE) canStart = true;

  let said = keep();
  const picks = state.picks;
  if (moves(len)) said = true;
  const fresh = refill();
  if (improveGuarded()) said = true;
  if (stop && !fresh) {
    if (redraw()) said = true;
  }
  if (canStart) {
    startLine();
    return true;
  }
  if (scatter && !said) {
    statusLine();
    said = true;
  }
  if (!said) refusal(ORDER_COMMAND[first], firstRole);
  showSupplies();
  ensureOfficer();
  const supplies = !fresh && (listedSupplies || state.picks != picks);
  if (!(supplies && choiceLine())) rowLine(fresh);
  return false;
}

/** G guards where the unit is; guarding a place elsewhere is another order. */
function guardsItself(at: i32): bool {
  const cell = ORDER_CELL[at];
  if (cell == NO_HANDLE) return true;
  const here = world.cellOf(ORDER_UNIT[at]);
  if (here == NO_HANDLE) return false;
  return chebyshev(world.deltaX(here, cell), world.deltaY(here, cell)) <= 1;
}

function refusal(command: i32, role: i32): void {
  const moving = command == Command.Move || command == Command.MoveToObject || command == Command.AttackMove;
  if (role == ROLE_OFFICER && moving) log("军需官不用移动：按 S 换货，G 升级，X 开战。");
  else if (isTruck(role) && command == Command.Move) log("选补给时只选一辆车，再把它移走。");
  else if (moving) log("请用普通移动，落点选营地里的空地。");
  else if (command == Command.Stop) log("S 换货：要选中货位佣兵或军需官。");
  else if (command == Command.Guard) log("G 只对货位佣兵（保留）和军需官（升级）有用。");
  else log("营地里不能攻击、部署或进入：只用移动和 S、G、X。");
}

function marked(at: i32): bool {
  return GUARDED_OFFERS[at] && OFFER_UNIT[at] != NO_HANDLE;
}

/** A group is all kept, or all let go once all of it is kept. */
function keep(): bool {
  if (!guardedOffer) return false;
  let any = false;
  let keep = false;
  for (let at = 0; at < MAX_OFFERS; at++) {
    if (!marked(at)) continue;
    any = true;
    if (!isKept(at)) keep = true;
  }
  if (!any) return false;
  const line = new Line().text(keep ? "保留" : "取消保留");
  let place = 0;
  for (let at = 0; at < MAX_OFFERS; at++) {
    if (!marked(at)) continue;
    OFFER_KEPT[at] = keep ? 1 : 0;
    const unit = OFFER_UNIT[at];
    // The chevron is the cue on the field.
    if (unit != NO_HANDLE) set(unit, Prop.Rank, keep ? 1 : 0);
    line.text(place == 0 ? " " : "、").text(offerMerc(at).name);
    place++;
  }
  line.text("，共留 ").num(<i64>kept()).text(" 个");
  line.text(guardedOfficer ? "；升级请只选军需官。" : keep ? "（带军衔标记）。" : "。");
  say(line);
  return true;
}

/** Costly and for good, so a group with more than the officer does not. */
function improveGuarded(): bool {
  if (!guardedOfficer || guardedOffer) return false;
  if (guardedMember) {
    log("升级营地请只选军需官，再按 G。");
    return true;
  }
  improve();
  return true;
}

// ------------------------------------------------------------------ Moves

const STEP_UNIT = new StaticArray<i64>(BATCH);
const STEP_ROLE = new StaticArray<i32>(BATCH);
const STEP_TO = new StaticArray<i64>(BATCH);
let steps: i32 = 0;

function moves(len: i32): bool {
  steps = 0;
  for (let at = 0; at < len; at++) {
    if (ORDER_COMMAND[at] != Command.Move || ORDER_CELL[at] == NO_HANDLE) continue;
    const unit = ORDER_UNIT[at];
    const r = role(unit);
    if (r == NO_ROLE) continue;
    // Of two orders to one unit the later is meant.
    let found = -1;
    for (let step = 0; step < steps; step++) {
      if (STEP_UNIT[step] == unit) {
        found = step;
        break;
      }
    }
    if (found >= 0) {
      STEP_TO[found] = ORDER_CELL[at];
    } else if (steps < BATCH) {
      STEP_UNIT[steps] = unit;
      STEP_ROLE[steps] = r;
      STEP_TO[steps] = ORDER_CELL[at];
      steps++;
    }
  }
  if (steps == 0) return false;
  if (steps == 1 && isTruck(STEP_ROLE[0])) return pick(STEP_ROLE[0] - ROLE_TRUCK);
  if (inRow(clicked())) {
    const dismissed = dismiss();
    let offered = false;
    for (let step = 0; step < steps; step++) if (isOffer(STEP_ROLE[step])) offered = true;
    if (offered && !dismissed) log("把佣兵移到货位外面才算雇佣。");
    return dismissed || offered;
  }
  const hired = hire();
  const placed = place(!hired);
  return placed || hired;
}

const AT_X = new StaticArray<i64>(BATCH);
const AT_Y = new StaticArray<i64>(BATCH);
const AT_TO = new StaticArray<i64>(BATCH);

/**
 * Where the player clicked: the destination of the unit nearest the middle
 * of the group, of equals the last, as the engine's group spread picks it.
 */
function clicked(): Cell {
  const last = STEP_TO[steps - 1];
  if (steps == 1) return last;
  let len = 0;
  let sumX: i64 = 0;
  let sumY: i64 = 0;
  for (let step = 0; step < steps; step++) {
    const x = get(STEP_UNIT[step], Prop.X);
    const xRead = lastStatus() == Status.Ok;
    const y = get(STEP_UNIT[step], Prop.Y);
    if (!xRead || lastStatus() != Status.Ok) continue;
    AT_X[len] = x;
    AT_Y[len] = y;
    AT_TO[len] = STEP_TO[step];
    len++;
    sumX += x;
    sumY += y;
  }
  if (len == 0) return last;
  const middleX = sumX / <i64>len;
  const middleY = sumY / <i64>len;
  let bestFar: i64 = i64.MAX_VALUE;
  let best = last;
  for (let at = len - 1; at >= 0; at--) {
    const dx = AT_X[at] - middleX;
    const dy = AT_Y[at] - middleY;
    const far = dx * dx + dy * dy;
    if (far < bestFar) {
      bestFar = far;
      best = AT_TO[at];
    }
  }
  return best;
}

function dismiss(): bool {
  let count: i64 = 0;
  let refund: i32 = 0;
  let oneKind = 0;
  let oneChampion: bool = false;
  let failed = false;
  for (let step = 0; step < steps; step++) {
    const unit = STEP_UNIT[step];
    const r = role(unit);
    if (!isMember(r)) continue;
    const at = r - ROLE_MEMBER;
    if (Action.remove(unit) != Status.Ok && world.alive(unit)) {
      failed = true;
      continue;
    }
    oneKind = state.kindOf(at);
    oneChampion = state.isChampion(at);
    refund += camp.refund(content.merc(state.kindOf(at)).price, state.isChampion(at));
    count++;
    state.clearMember(at);
    PLACED[at] = -1;
  }
  if (count == 0) {
    if (failed) log("这名队员现在不能遣散。");
    return failed;
  }
  const line = new Line().text("遣散 ");
  if (count == 1) line.text(oneChampion ? "精锐" : "").text(content.merc(oneKind).name);
  else line.num(count).text(" 人");
  if (world.pay(refund)) line.text("，退 +").num(<i64>refund);
  else line.text("，退款失败");
  say(purse(line).text("。"));
  return true;
}

/** Moves members in the line. `tell` is off when a hire says it already. */
function place(tell: bool): bool {
  let count = 0;
  let stuck = 0;
  let clamped = false;
  for (let step = 0; step < steps; step++) {
    // A unit hired from these same steps has had its order.
    const r = STEP_ROLE[step];
    if (!isMember(r)) continue;
    const at = r - ROLE_MEMBER;
    if (state.memberUnit[at] != STEP_UNIT[step]) continue; // Merged away by the same gesture.
    const to = stand(state.kindOf(at), STEP_TO[step]);
    if (send(at, to)) {
      count++;
      if (isOutside(STEP_TO[step])) clamped = true;
    } else {
      stuck++;
    }
  }
  if (!tell || count + stuck == 0) return false;
  const line = new Line();
  if (count == 0) line.text("走不到那里，队员留在原地");
  else line.text("列阵 ").num(<i64>count).text(" 人，下一轮仍站那里");
  if (clamped) line.text("（只能在营地 9 格内）");
  say(line.text("。"));
  return true;
}

/** Orders member `at` to `to` and makes that its place in the line. */
function send(at: i32, to: Cell): bool {
  const unit = state.memberUnit[at];
  if (unit == NO_HANDLE) return false;
  if (Action.setMission(unit, Mission.Move, to) != Status.Ok) return false;
  state.memberHome[at] = to;
  PLACED[at] = <i64>frame();
  return true;
}

const WANTED_AT = new StaticArray<i32>(MAX_OFFERS);
const WANTED_TO = new StaticArray<i64>(MAX_OFFERS);
const HIRED = new StaticArray<i32>(MAX_OFFERS);

function hire(): bool {
  let len = 0;
  for (let step = 0; step < steps; step++) {
    const r = STEP_ROLE[step];
    if (isOffer(r) && len < MAX_OFFERS && OFFER_UNIT[r] != NO_HANDLE && OFFER_UNIT[r] == STEP_UNIT[step]) {
      WANTED_AT[len] = r;
      WANTED_TO[len] = STEP_TO[step];
      len++;
    }
  }
  if (len == 0) return false;
  let count = 0;
  let cost: i32 = 0;
  let merged = -1;
  let mergesMade: i64 = 0;
  let clamped = false;
  let payFailed = false;
  // Merges first: they free places, and one hire can complete a pair.
  while (len > 0) {
    const money = world.credits();
    const room = company() < places();
    let next = -1;
    for (let want = 0; want < len; want++) {
      const at = WANTED_AT[want];
      if (<i64>offerMerc(at).price <= money && pair(OFFER_KIND[at])) {
        next = want;
        break;
      }
    }
    if (next < 0) {
      for (let want = 0; want < len; want++) {
        if (room && <i64>offerMerc(WANTED_AT[want]).price <= money) {
          next = want;
          break;
        }
      }
    }
    if (next < 0) break;
    const at = WANTED_AT[next];
    const to = WANTED_TO[next];
    const kind = <i32>OFFER_KIND[at];
    const price = offerMerc(at).price;
    const made = enlist(at, kind, to);
    if (made == ENLIST_FAILED) {
      payFailed = true;
    } else {
      if (count < MAX_OFFERS) {
        HIRED[count] = kind;
        count++;
      }
      cost += price;
      if (isOutside(to)) clamped = true;
      if (made == ENLIST_CHAMPION) {
        merged = kind;
        mergesMade++;
      }
    }
    for (let want = next; want + 1 < len; want++) {
      WANTED_AT[want] = WANTED_AT[want + 1];
      WANTED_TO[want] = WANTED_TO[want + 1];
    }
    len--;
  }

  const left: Merc | null = len > 0 ? offerMerc(WANTED_AT[0]) : null;
  if (count == 0) {
    if (left != null && <i64>left.price > world.credits()) {
      short(new Line().text("钱不够，").text(left.name), left.price);
    } else if (left != null && !payFailed) {
      fullLine();
    } else {
      log("这名佣兵现在雇不了。");
    }
    return true;
  }
  const line = new Line();
  if (merged >= 0 && count == 1) {
    line.text("三合一！精锐").text(content.merc(HIRED[0]).name);
  } else if (count <= 2) {
    line.text("雇佣 ").text(content.merc(HIRED[0]).name);
    if (count == 2) line.text("、").text(content.merc(HIRED[1]).name);
  } else {
    line.text("雇佣 ").num(<i64>count).text(" 人");
  }
  line.text(" -").num(<i64>cost);
  if (merged >= 0) {
    if (count > 1) line.text("，合成精锐").text(content.merc(merged).name);
    let waiting = false;
    for (let at = 0; at < 3; at++) if (CHOICES[at] >= 0) waiting = true;
    if (waiting || anyChoice()) {
      line.text("，补给+").num(mergesMade);
      // The free row takes the last line from the supplies.
      if (rowIsOut()) line.text("（选车按 X 看）");
    }
  } else if (count == 1 && pair(<i64>HIRED[0])) {
    line.text("（同名 2/3）");
  }
  purse(line);
  // After a merge the line is long enough without it.
  if (left != null && merged < 0) {
    line.text("；").text(left.name);
    if (<i64>left.price > world.credits()) line.text("要 ").num(<i64>left.price).text("，钱不够");
    else line.text("没位置了");
  } else if (clamped) {
    line.text("（落点收回 9 格内）");
  }
  say(line.text("。"));
  return true;
}

function anyChoice(): bool {
  return perks.choices();
}

const ENLIST_FAILED: i32 = 0;
const ENLIST_HIRED: i32 = 1;
const ENLIST_CHAMPION: i32 = 2;

/** One hire and the merge it makes: `ENLIST_CHAMPION` when a champion came of it, `ENLIST_FAILED` when it could not be done. */
function enlist(offer: i32, kind: i32, to: Cell): i32 {
  const unit = OFFER_UNIT[offer];
  if (unit == NO_HANDLE) return ENLIST_FAILED;
  const paired = pair(<i64>kind);
  const first = pairFirst;
  const second = pairSecond;
  let slot = -1;
  if (paired) {
    slot = first;
  } else {
    for (let at = 0; at < MAX_COMPANY; at++) {
      if (state.isEmpty(at)) {
        slot = at;
        break;
      }
    }
    if (slot < 0) return ENLIST_FAILED;
  }
  if (!world.pay(-offerMerc(offer).price)) return ENLIST_FAILED;
  world.release(OFFER_SPEED[offer], OFFER_FIRE[offer]);
  clearOffer(offer);
  if (paired) {
    for (let n = 0; n < 2; n++) {
      const at = n == 0 ? first : second;
      const old = state.memberUnit[at];
      if (old != NO_HANDLE) Action.remove(old);
      state.clearMember(at);
      PLACED[at] = -1;
    }
  }
  state.memberKind[slot] = <i64>kind;
  state.memberChampion[slot] = paired ? 1 : 0;
  state.memberUnit[slot] = unit;
  const place = stand(kind, to);
  state.memberHome[slot] = place;
  PLACED[slot] = -1;
  set(unit, Prop.Rank, unitRank(slot));
  send(slot, place);
  if (paired) state.setPicks(state.picks + 1);
  return paired ? ENLIST_CHAMPION : ENLIST_HIRED;
}

function fullLine(): void {
  const line = new Line().text("队伍已满");
  fraction(line).text("：先遣散");
  const rank = campRank();
  if (rank < camp.IMPROVE.length) {
    line.text("，或选军需官按 G 升级（").num(<i64>camp.IMPROVE[rank]).text("）");
  }
  say(line.text("。"));
}

// --------------------------------------------------------------- Supplies

function pick(at: i32): bool {
  const perk = CHOICES[at];
  if (state.picks <= 0 || perk < 0) return false;
  // What this taking adds: after the grant it would be the next one's.
  const effect = perks.effect(<i32>perk);
  perks.grant(<i32>perk);
  state.setPicks(state.picks - 1);
  // Trucks anew: a second order to the old ones picks nothing.
  for (let truck = 0; truck < 3; truck++) {
    const unit = TRUCKS[truck];
    if (unit != NO_HANDLE) Action.remove(unit);
    TRUCKS[truck] = NO_HANDLE;
    CHOICES[truck] = -1;
  }
  // A supply can change a rank.
  for (let member = 0; member < MAX_COMPANY; member++) {
    if (state.isEmpty(member)) continue;
    const unit = state.memberUnit[member];
    if (unit != NO_HANDLE) set(unit, Prop.Rank, unitRank(member));
  }
  const line = new Line().text("获得补给：").text(perks.name(<i32>perk)).text("，").text(effect);
  if (state.picks > 0 && anyChoice()) line.text("；还可选 ").num(state.picks).text(" 次");
  say(line.text("。"));
  return true;
}

/** Three trucks while there is a pick and something left to pick. */
function showSupplies(): void {
  if (state.picks <= 0) return;
  if (CHOICES[0] < 0 && CHOICES[1] < 0 && CHOICES[2] < 0) {
    perks.choices();
    for (let at = 0; at < 3; at++) CHOICES[at] = <i64>perks.drawn[at];
  }
  for (let at = 0; at < 3; at++) {
    if (CHOICES[at] < 0 || TRUCKS[at] != NO_HANDLE) continue;
    const spot = world.offset(state.camp(), (at - 1) * SPACING, SUPPLY_DY);
    const unit = create(spot, CRATES[at]);
    if (unit != NO_HANDLE) {
      world.hold(unit);
      TRUCKS[at] = unit;
    }
  }
}

const UNITS = new StaticArray<i64>(MAX_OFFERS);

/** The choice, if one is on the field. */
function choiceLine(): bool {
  if (state.picks <= 0 || (TRUCKS[0] == NO_HANDLE && TRUCKS[1] == NO_HANDLE && TRUCKS[2] == NO_HANDLE)) return false;
  for (let at = 0; at < MAX_OFFERS; at++) UNITS[at] = NO_HANDLE;
  for (let at = 0; at < 3; at++) UNITS[at] = CHOICES[at] >= 0 ? TRUCKS[at] : NO_HANDLE;
  const len = leftToRight();
  if (len == 0) return false;
  // Names and effects on two lines: on one, three of them are too wide.
  const names = new Line().text("移走一辆车选补给，左起：");
  const effects = new Line().text("效果：");
  for (let place = 0; place < len; place++) {
    const perk = <i32>CHOICES[SHOWN_AT[place]];
    const gap = place == 0 ? "" : "｜";
    names.text(gap).text(perks.name(perk));
    effects.text(gap).text(perks.effect(perk));
  }
  say(names);
  say(effects);
  return true;
}

// ------------------------------------------------------------------- Camp

function kept(): i32 {
  let count = 0;
  for (let at = 0; at < row(); at++) if (isKept(at)) count++;
  return count;
}

function draw(): i32 {
  const odds = campRank() * 3;
  const roll = random(1, 100);
  let tier = 0;
  let sum = camp.ODDS[odds];
  while (roll > sum && tier + 1 < 3) {
    tier++;
    sum += camp.ODDS[odds + tier];
  }
  let count = 0;
  for (let kind = 0; kind < content.MERCS.length; kind++) if (content.MERCS[kind].tier == tier) count++;
  if (count == 0) return random(0, content.MERCS.length - 1);
  const nth = random(0, count - 1);
  let seen = 0;
  for (let kind = 0; kind < content.MERCS.length; kind++) {
    if (content.MERCS[kind].tier != tier) continue;
    if (seen == nth) return kind;
    seen++;
  }
  return 0;
}

/** Stands offer `at` on its spot: the one it keeps, or a draw. */
function stock(at: i32, drawNew: bool): void {
  if (drawNew && !isKept(at)) OFFER_KIND[at] = <i64>draw();
  if (OFFER_KIND[at] < 0) return;
  const count = row();
  const dx = at * SPACING - ((count - 1) * SPACING) / 2;
  const spot = world.offset(state.camp(), dx, 0);
  const unit = create(spot, offerMerc(at).id);
  if (unit == NO_HANDLE) {
    if (!isKept(at)) clearOffer(at);
    return;
  }
  OFFER_UNIT[at] = unit;
  world.hold(unit);
  OFFER_SPEED[at] = world.heldSpeed;
  OFFER_FIRE[at] = world.heldFire;
  if (isKept(at)) set(unit, Prop.Rank, 1);
}

/** A row that was hired out is drawn new for nothing. */
function refill(): bool {
  if (!rowIsOut()) return false;
  for (let at = 0; at < row(); at++) stock(at, true);
  return !rowIsOut();
}

function redraw(): bool {
  const free = row() - kept();
  if (free == 0) {
    log("货位全都保留着：先选中按 G 取消保留。");
    return true;
  }
  if (world.credits() < <i64>camp.REDRAW) {
    short(new Line().text("钱不够，换货"), camp.REDRAW);
    return true;
  }
  if (!world.pay(-camp.REDRAW)) {
    log("现在不能换货。");
    return true;
  }
  for (let at = 0; at < row(); at++) {
    if (isKept(at)) continue;
    takeAway(at);
    clearOffer(at);
  }
  let drawn = false;
  for (let at = 0; at < row(); at++) {
    if (isKept(at)) continue;
    stock(at, true);
    if (OFFER_UNIT[at] != NO_HANDLE) drawn = true;
  }
  if (!drawn) {
    world.pay(camp.REDRAW);
    log("货位放不下新佣兵，换货的钱已退回。");
    return true;
  }
  const line = new Line().text("换货 -").num(<i64>camp.REDRAW);
  if (kept() > 0) line.text("（保留的 ").num(<i64>kept()).text(" 个不变）");
  say(purse(line).text("。"));
  return true;
}

function improve(): void {
  const rank = campRank();
  if (rank >= camp.IMPROVE.length) {
    say(fraction(new Line().text("营地已是最高 3 级：5 个货位，队伍")).text("。"));
    return;
  }
  const cost = camp.IMPROVE[rank];
  if (world.credits() < <i64>cost) {
    short(new Line().text("钱不够，升级营地"), cost);
    return;
  }
  if (!world.pay(-cost)) {
    log("现在不能升级营地。");
    return;
  }
  // The row is centred, so every offer moves.
  for (let at = 0; at < MAX_OFFERS; at++) takeAway(at);
  const old = row();
  state.setCampRank(<i64>(rank + 1));
  for (let at = 0; at < row(); at++) stock(at, at >= old);
  // The row's zone grew over some of the line.
  for (let at = 0; at < MAX_COMPANY; at++) {
    if (state.isEmpty(at)) continue;
    const home = state.memberHome[at];
    if (inRow(home)) {
      const to = stand(state.kindOf(at), home);
      if (!send(at, to)) state.memberHome[at] = to;
    }
  }
  const line = new Line().text("营地 ").num(<i64>(rank + 2)).text(" 级 -").num(<i64>cost);
  purse(line);
  if (rank + 1 < camp.IMPROVE.length) line.text("，下次升级 ").num(<i64>camp.IMPROVE[rank + 1]);
  say(line.text("。"));
}

function ensureOfficer(): void {
  if (officerUnit != NO_HANDLE) return;
  // The camp's own cell held the player's first unit; and any armed unit of
  // the bound types will do rather than none.
  for (let n = 0; n < 4; n++) {
    const id = n < 3 ? OFFICER : content.MERCS[0].id;
    const dy = n == 0 ? OFFICER_DY : n == 1 ? -OFFICER_DY : 0;
    const unit = create(world.offset(state.camp(), 0, dy), id);
    if (unit != NO_HANDLE) {
      world.hold(unit);
      officerUnit = unit;
      return;
    }
  }
}

function create(spot: Cell, id: string): Techno {
  if (fails >= MAX_FAILS) return NO_HANDLE;
  const made = world.createNear(spot, id, state.player());
  if (made == NO_HANDLE) fails++;
  return made;
}

function lost(unit: Techno): bool {
  return unit != NO_HANDLE && !world.alive(unit);
}

function forgetTheLost(): void {
  for (let at = 0; at < MAX_OFFERS; at++) {
    if (lost(OFFER_UNIT[at])) {
      if (isKept(at)) OFFER_UNIT[at] = NO_HANDLE;
      else clearOffer(at);
    }
  }
  for (let at = 0; at < 3; at++) if (lost(TRUCKS[at])) TRUCKS[at] = NO_HANDLE;
  if (lost(officerUnit)) officerUnit = NO_HANDLE;
  for (let at = 0; at < MAX_COMPANY; at++) {
    if (lost(state.memberUnit[at])) state.memberUnit[at] = NO_HANDLE;
  }
}

// ----------------------------------------------------------------- Ground

function inRow(cell: Cell): bool {
  if (cell == NO_HANDLE) return false;
  const dx = world.deltaX(state.camp(), cell);
  const dy = world.deltaY(state.camp(), cell);
  const half = ((row() - 1) * SPACING + 1) / 2;
  return abs(dy) <= ROW_DEPTH && abs(dx) <= half + 1;
}

function isOutside(cell: Cell): bool {
  return chebyshev(world.deltaX(state.camp(), cell), world.deltaY(state.camp(), cell)) > REACH;
}

function clampReach(value: i32): i32 {
  return value < -REACH ? -REACH : value > REACH ? REACH : value;
}

/** The closest place in the line to `wanted` that `kind` can stand on. */
function stand(kind: i32, wanted: Cell): Cell {
  const at = state.camp();
  const dx = world.deltaX(at, wanted);
  let dy = world.deltaY(at, wanted);
  if (inRow(wanted)) dy = dy < 0 ? -ROW_DEPTH - 1 : ROW_DEPTH + 1;
  const place = world.offset(at, clampReach(dx), clampReach(dy));
  const speed = (content.merc(kind).tags & content.FOOT) != 0 ? SpeedType.Foot : SpeedType.Track;
  for (let ring = 0; ring <= 3; ring++) {
    for (let y = -ring; y <= ring; y++) {
      for (let x = -ring; x <= ring; x++) {
        const cell = world.offset(place, x, y);
        if (chebyshev(x, y) == ring && !isOutside(cell) && !inRow(cell) && Cell.passable(cell, speed) &&
          lastStatus() == Status.Ok) {
          return cell;
        }
      }
    }
  }
  return place;
}

// -------------------------------------------------------------- Interface

function firstRowUnit(): Techno {
  for (let at = 0; at < row(); at++) if (OFFER_UNIT[at] != NO_HANDLE) return OFFER_UNIT[at];
  return NO_HANDLE;
}

/**
 * Queues the order a player would give a camp unit for the interface's
 * message `what` (`proto.MSG_*`) about offer, truck or member `index`: a
 * button does what the order does, through `apply`. Nothing when the unit
 * it needs is not in camp.
 */
export function uiOrder(what: u8, index: i32): void {
  const offer = index < row() ? OFFER_UNIT[index] : NO_HANDLE;
  if (what == proto.MSG_HIRE) {
    if (offer != NO_HANDLE) queue(offer, Command.Move, freePlace());
  } else if (what == proto.MSG_KEEP) {
    if (offer != NO_HANDLE) queue(offer, Command.Guard, NO_HANDLE);
  } else if (what == proto.MSG_REDRAW) {
    // The officer, or any camp unit the same key works on.
    const unit = officerUnit != NO_HANDLE ? officerUnit : firstRowUnit();
    if (unit != NO_HANDLE) queue(unit, Command.Stop, NO_HANDLE);
  } else if (what == proto.MSG_IMPROVE) {
    if (officerUnit != NO_HANDLE) queue(officerUnit, Command.Guard, NO_HANDLE);
  } else if (what == proto.MSG_START) {
    let unit = officerUnit;
    for (let at = 0; at < MAX_COMPANY && unit == NO_HANDLE; at++) unit = state.memberUnit[at];
    if (unit == NO_HANDLE) unit = firstRowUnit();
    if (unit != NO_HANDLE) queue(unit, Command.Scatter, NO_HANDLE);
  } else if (what == proto.MSG_PICK) {
    const truck = index < 3 ? TRUCKS[index] : NO_HANDLE;
    if (truck != NO_HANDLE) queue(truck, Command.Move, world.offset(state.camp(), 0, SUPPLY_DY - 2));
  } else if (what == proto.MSG_DISMISS) {
    // Into the row: a dismissal.
    const unit = index < MAX_COMPANY ? state.memberUnit[index] : NO_HANDLE;
    if (unit != NO_HANDLE) queue(unit, Command.Move, state.camp());
  }
}

/** The first place of the starting line that no member calls home. */
function freePlace(): Cell {
  const at = state.camp();
  for (let n = 0; n < MAX_COMPANY; n++) {
    const spot = world.offset(at, ((n % 5) - 2) * 2, 4 + (n / 5) * 2);
    let free = true;
    for (let member = 0; member < MAX_COMPANY; member++) if (state.memberHome[member] == spot) free = false;
    if (free) return spot;
  }
  return world.offset(at, 0, 4);
}

// The camp as `proto` publishes it, written by `uiView`.
export let viewOffers: i64 = 0;
export let viewChoices: i64 = 0;
export let viewCompanyLow: i64 = 0;
export let viewCompanyHigh: i64 = 0;
export let viewFlags: i64 = 0;

export function uiView(): void {
  let offers: i64 = 0;
  for (let at = 0; at < row(); at++) {
    const kind = OFFER_KIND[at];
    const packed = proto.packOffer(kind >= 0 ? <i32>kind : -1, isKept(at), OFFER_UNIT[at] != NO_HANDLE);
    offers = proto.withByte(offers, at, packed);
  }
  const trucks = TRUCKS[0] != NO_HANDLE || TRUCKS[1] != NO_HANDLE || TRUCKS[2] != NO_HANDLE;
  let choices: i64 = 0;
  for (let at = 0; at < 3; at++) {
    const perk = CHOICES[at];
    if (TRUCKS[at] != NO_HANDLE && perk >= 0 && state.picks > 0) {
      choices = proto.withByte(choices, at, <u8>(perk + 1));
    }
  }
  let low: i64 = 0;
  let high: i64 = 0;
  for (let at = 0; at < MAX_COMPANY; at++) {
    const packed = proto.packMember(
      state.isEmpty(at) ? -1 : state.kindOf(at), state.isChampion(at), state.memberUnit[at] != NO_HANDLE,
    );
    if (at < 5) low = proto.withByte(low, at % 5, packed);
    else high = proto.withByte(high, at % 5, packed);
  }
  let flags: i64 = 0;
  if (officerUnit != NO_HANDLE) flags |= proto.FLAG_OFFICER;
  if (trucks && state.picks > 0) flags |= proto.FLAG_TRUCKS;
  viewOffers = offers;
  viewChoices = choices;
  viewCompanyLow = low;
  viewCompanyHigh = high;
  viewFlags = flags;
}

// ----------------------------------------------------------------- Phases

/** Puts the camp and the company on the cleared field. */
export function open(): void {
  fails = 0;
  musters++;
  officerUnit = NO_HANDLE;
  for (let at = 0; at < 3; at++) TRUCKS[at] = NO_HANDLE;
  for (let at = 0; at < MAX_OFFERS; at++) OFFER_UNIT[at] = NO_HANDLE;
  ensureOfficer();
  for (let at = 0; at < row(); at++) stock(at, true);
  showSupplies();
  let missing: i64 = 0;
  const at0 = state.camp();
  for (let at = 0; at < MAX_COMPANY; at++) {
    PLACED[at] = -1;
    state.memberUnit[at] = NO_HANDLE;
    if (state.isEmpty(at)) continue;
    if (state.memberHome[at] == NO_HANDLE) {
      state.memberHome[at] = world.offset(at0, ((at % 5) - 2) * 2, 4 + (at / 5) * 2);
    }
    const id = content.merc(state.kindOf(at)).id;
    let made = create(state.memberHome[at], id);
    if (made == NO_HANDLE) made = create(world.offset(at0, 0, 4), id);
    if (made != NO_HANDLE) {
      state.memberUnit[at] = made;
      set(made, Prop.Rank, unitRank(at));
    } else {
      missing++;
    }
  }
  opening(missing);
}

/**
 * At most five lines, the row last: the newest line stays longest. The
 * first muster opens with the credit (README.md).
 */
function opening(missing: i64): void {
  const muster = musters;
  if (muster == 1) {
    log("玩法参考 B 站 UP 主 MK 的红警酒馆，致谢。");
    log("把货位佣兵移到空地＝雇佣；队员移动＝列阵，移回货位＝遣散退半价。");
    say(new Line()
      .text("军需官：S 换货 ")
      .num(<i64>camp.REDRAW)
      .text("、G 升级 ")
      .num(<i64>camp.IMPROVE[0])
      .text("、X 开战（队员也行）；货位：G 保留、X 查看。"));
  }
  const noOfficer = officerUnit == NO_HANDLE;
  if (noOfficer) {
    log("军需官进不了场：选中任意单位按 X 开战。");
  } else if (missing > 0) {
    say(fraction(new Line().text("有 ").num(missing).text(" 人进不了场，本轮缺阵；队伍")).text("。"));
  } else if (state.picks > 0) {
    // The supplies take two lines.
  } else if (muster == 2) {
    log("三名同种普通队员合成一名精锐并得一次补给，满员也能雇第三名。");
  } else if (muster == 3) {
    log("G 保留的佣兵换货和下轮都不走；队员站位会一直沿用。");
  }
  if (muster > 1 || (!noOfficer && missing == 0)) statusLine();
  choiceLine();
  rowLine(false);
}

/** Takes down all but the company, and settles where each member stands. */
export function close(): void {
  for (let at = 0; at < 4; at++) {
    const unit = at < 3 ? TRUCKS[at] : officerUnit;
    if (unit != NO_HANDLE) Action.remove(unit);
    if (at < 3) TRUCKS[at] = NO_HANDLE;
    else officerUnit = NO_HANDLE;
  }
  for (let at = 0; at < MAX_OFFERS; at++) {
    takeAway(at);
    if (!isKept(at)) clearOffer(at);
  }
  const now = <i64>frame();
  for (let at = 0; at < MAX_COMPANY; at++) {
    const placed = PLACED[at];
    PLACED[at] = -1;
    // One who was not ordered keeps the place, not where it was squeezed.
    const unit = state.memberUnit[at];
    if (unit == NO_HANDLE || placed < 0) continue;
    // One still on the way will stand where it was sent.
    if (now - placed < SET_OFF) continue;
    const moving = get(unit, Prop.Moving);
    if (lastStatus() == Status.Ok && moving == 1) continue;
    const cell = world.cellOf(unit);
    if (cell != NO_HANDLE && cell != state.memberHome[at]) {
      state.memberHome[at] = stand(state.kindOf(at), cell);
    }
  }
}

// ------------------------------------------------------------------- Text

function fraction(line: Line): Line {
  return line.text(" ").num(<i64>company()).text("/").num(<i64>places());
}

/** The money left and the places used, after every action. */
function purse(line: Line): Line {
  return fraction(line.text("，余 ").num(world.credits()).text("，队伍"));
}

function short(line: Line, cost: i32): void {
  const money = world.credits();
  say(line.text("要 ")
    .num(<i64>cost)
    .text("，余 ")
    .num(money)
    .text("（还差 ")
    .num(<i64>cost - money)
    .text("）。"));
}

function startLine(): void {
  let ready: i64 = 0;
  for (let at = 0; at < MAX_COMPANY; at++) if (state.memberUnit[at] != NO_HANDLE) ready++;
  say(purse(new Line().text("点兵结束，").num(ready).text(" 人出战")).text("。"));
}

function statusLine(): void {
  const rank = campRank();
  const line = new Line().text("营地 ").num(<i64>(rank + 1)).text(" 级");
  purse(line).text("；换货 ").num(<i64>camp.REDRAW);
  if (rank < camp.IMPROVE.length) line.text("，升级 ").num(<i64>camp.IMPROVE[rank]);
  else line.text("，已满级");
  say(line.text("。"));
}

// `leftToRight`'s result: the places of `UNITS` as they stand on the
// screen, where x − y grows to the right.
const SHOWN_KEY = new StaticArray<i32>(MAX_OFFERS);
const SHOWN_AT = new StaticArray<i32>(MAX_OFFERS);

/** Sorts the units in `UNITS` left to right into `SHOWN_AT`; how many there are. */
function leftToRight(): i32 {
  let len = 0;
  for (let at = 0; at < MAX_OFFERS; at++) {
    const unit = UNITS[at];
    if (unit == NO_HANDLE) continue;
    const cell = world.cellOf(unit);
    if (cell == NO_HANDLE) continue;
    const x = world.deltaX(state.camp(), cell);
    const y = world.deltaY(state.camp(), cell);
    if (len < MAX_OFFERS) {
      SHOWN_KEY[len] = x - y;
      SHOWN_AT[len] = at;
      len++;
    }
  }
  for (let end = 1; end < len; end++) {
    let at = end;
    while (at > 0 && SHOWN_KEY[at] < SHOWN_KEY[at - 1]) {
      const key = SHOWN_KEY[at];
      SHOWN_KEY[at] = SHOWN_KEY[at - 1];
      SHOWN_KEY[at - 1] = key;
      const index = SHOWN_AT[at];
      SHOWN_AT[at] = SHOWN_AT[at - 1];
      SHOWN_AT[at - 1] = index;
      at--;
    }
  }
  return len;
}

function rowLine(fresh: bool): void {
  for (let at = 0; at < MAX_OFFERS; at++) UNITS[at] = at < row() ? OFFER_UNIT[at] : NO_HANDLE;
  const len = leftToRight();
  if (len == 0) {
    log("货位空着：选中军需官按 S 换货。");
    return;
  }
  const line = new Line().text(fresh ? "免费新货左起：" : "货位左起：");
  for (let place = 0; place < len; place++) {
    const at = SHOWN_AT[place];
    const merc = offerMerc(at);
    line.text(place == 0 ? "" : " ").text(merc.name).num(<i64>merc.price);
    if (isKept(at)) line.text("[留]");
  }
  say(line);
}
