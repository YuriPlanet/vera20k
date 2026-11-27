// "Muster Field"'s `ui.wasm`: the camp on panels, and a button for every
// key of the game (RFC 0001 section 10). The port of the Rust SDK's
// `sdk/examples/muster-field/ui`.
//
// - Top right, beside the sidebar: the wave and what it brings, banners,
//   camp, company and the supplies taken; in a battle, both sides'
//   standing and a bar of the raiders left.
// - Bottom left: the quartermaster's buttons (redraw, improve, battle),
//   then a card per offer: name, price, tier, how close it is to a merge,
//   hire and keep.
// - Top left, below the game's message lines: the company, a button per
//   member, filled downwards so a member keeps its place; a second click
//   within three seconds dismisses it.
// - Middle: a card per supply to pick from, while a pick is waiting.
//
// The pictures are the mod folder's PCX files: `mfp_<type>.pcx` portraits,
// `mfk_<n>.pcx` supply emblems, `mfcard<tier>.pcx`, `mfcardk.pcx` (kept)
// and `mfsup.pcx` frames, `mfb<width><colour>.pcx` button plates and
// `mfpanh.pcx`, `mfpanq.pcx` panels (made apart, not in the repository). The
// engine draws a button's label over its plate and dims or brightens the
// plate as the button is disabled or pointed at; texts on these pictures
// go without their black backdrop. A missing picture leaves its place
// empty, and a button without its plate falls back to the plain look.
//
// It reads only the player's private variables the simulation publishes
// (`data/proto.ts`, declared again in this project's `vera.json`) and the
// player's money, and changes the game only through two-byte messages,
// which the simulation turns into the orders the keys give. Spectators see
// nothing. What it shows is in Simplified Chinese.
import {
  ClickEvent, Client, ClientEvent, House, NO_HANDLE, Node, NodeKind, NodeProps, Prop, Status, Ui,
  lastStatus, onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";
import { IMPROVE, PLACES, REDRAW, ROW, rank as campRank, refund } from "../../data/camp";
import * as content from "../../data/content";
import { FRONT_NAMES, MAX_COMPANY, MAX_OFFERS } from "../../data/index";
import { Line } from "../../data/line";
import * as perks from "../../data/perks";
import * as proto from "../../data/proto";
import * as waves from "../../data/waves";
import {
  MfBanners, MfBattle, MfChoices, MfCompanyHigh, MfCompanyLow, MfFlags, MfFront, MfLast, MfOffers,
  MfPhase, MfPicks, MfRank, MfTaken, MfWave,
} from "./generated/vars";

// ----------------------------------------------------------------- Layout

const TOP_LEFT: i64 = 0;
const TOP_RIGHT: i64 = 2;
const MIDDLE: i64 = 4;
const BOTTOM_LEFT: i64 = 6;

const MARGIN: i64 = 8;
/** Below the match's top tabs. */
const TOP: i64 = 24;
/** Left of the sidebar: the top left holds the game's message lines. */
const SIDEBAR: i64 = 176;
/** Above the command bar along the battlefield's bottom. */
const BOTTOM: i64 = 44;
const GAP: i64 = 2;
const CARD_WIDTH: i64 = 136;
const CARD_GAP: i64 = 14;
/** How far a frame picture reaches past what it holds, each side. */
const FRAME: i64 = 5;
const BUTTON_HEIGHT: i64 = 20;
const CONTROL_WIDTH: i64 = 120;
const COMPANY_WIDTH: i64 = 150;
const SUPPLY_WIDTH: i64 = 150;
/** Between the middles of two supply cards. */
const SUPPLY_STEP: i64 = 176;
const BAR_WIDTH: i64 = 240;

/** Below the game's message lines. */
const COMPANY_TOP: i64 = 140;

/** A second dismiss click counts within so many seconds, whatever the game speed. */
const CONFIRM_SECONDS: f32 = 3.0;

// ------------------------------------------------------------------ Nodes

/** A node and what was last written to it, so that an unchanged frame writes nothing. */
class Item {
  node: Node = NO_HANDLE;
  private label: string = "";
  private labelled: bool = false;
  private picture: string = "";
  private pictured: bool = false;
  /** Bit 0 visible, bit 1 enabled; −1 before the first write. */
  private state: i32 = -1;
  private filled: i32 = -1;

  create(kind: NodeKind, parent: Node, props: NodeProps): Node {
    const node = Ui.createWith(kind, parent, props);
    if (kind == NodeKind.Button && node != NO_HANDLE) Node.setClickable(node, true);
    this.node = node;
    return node;
  }

  setLabel(text: string): void {
    if (this.labelled && this.label == text) return;
    if (Node.setLabel(this.node, text) == Status.Ok) {
      this.label = text;
      this.labelled = true;
    }
  }

  /** A picture the mod folder holds (`Art`, appendix 7.10). */
  art(name: string): void {
    if (this.pictured && this.picture == name) return;
    if (Node.setArt(this.node, name) == Status.Ok) {
      this.picture = name;
      this.pictured = true;
    }
  }

  show(visible: bool): void {
    const state = visible ? 1 : 0;
    if (this.state != state && Node.setVisible(this.node, visible) == Status.Ok) this.state = state;
  }

  /** A button: shown, and clickable or greyed. */
  button(visible: bool, enabled: bool): void {
    const state = (visible ? 1 : 0) | (enabled ? 2 : 0);
    if (this.state == state) return;
    if (Node.setVisible(this.node, visible) == Status.Ok && Node.setEnabled(this.node, enabled) == Status.Ok) {
      this.state = state;
    }
  }

  fill(percent: i32): void {
    if (this.filled != percent && Node.setFill(this.node, percent) == Status.Ok) this.filled = percent;
  }

  is(node: Node): bool {
    return node != NO_HANDLE && this.node == node;
  }
}

class Card {
  /** The frame picture behind the column, made before it. */
  frame: Item = new Item();
  column: Item = new Item();
  portrait: Item = new Item();
  name: Item = new Item();
  info: Item = new Item();
  merge: Item = new Item();
  hire: Item = new Item();
  keep: Item = new Item();
}

class SupplyCard {
  frame: Item = new Item();
  column: Item = new Item();
  emblem: Item = new Item();
  name: Item = new Item();
  effect: Item = new Item();
  pick: Item = new Item();
}

// The panel picture behind the column, the column and its rows.
const HUD_PANEL = new Item();
const HUD_COLUMN = new Item();
const HUD_WAVE = new Item();
const HUD_STATUS = new Item();
const HUD_PERKS = new Item();
const HUD_PHASE = new Item();
const HUD_RAIDERS = new Item();

const CARDS: StaticArray<Card> = [new Card(), new Card(), new Card(), new Card(), new Card()];

const CONTROLS_PANEL = new Item();
const CONTROLS_COLUMN = new Item();
const CONTROLS_TITLE = new Item();
const CONTROLS_REDRAW = new Item();
const CONTROLS_IMPROVE = new Item();
const CONTROLS_START = new Item();

const COMPANY_COLUMN = new Item();
const COMPANY_TITLE = new Item();
const MEMBERS: StaticArray<Item> = [
  new Item(), new Item(), new Item(), new Item(), new Item(),
  new Item(), new Item(), new Item(), new Item(), new Item(),
];

const SUPPLIES_TITLE = new Item();
const SUPPLY_CARDS: StaticArray<SupplyCard> = [new SupplyCard(), new SupplyCard(), new SupplyCard()];

/** The member a first dismiss click armed, and the seconds it has left; −1 for none. */
let armed: i32 = -1;
let armedLeft: f32 = 0;

// ------------------------------------------------------------------ State

const MUSTER: i64 = 1;
const IN_BATTLE: i64 = 2;

/** One render frame's reading of the camp. */
class Camp {
  phase: i64 = 0;
  wave: i64 = 0;
  banners: i64 = 0;
  rank: i32 = 0;
  picks: i64 = 0;
  flags: i64 = 0;
  /** A packed offer each (`proto.packOffer`). */
  offers: StaticArray<u8> = new StaticArray<u8>(MAX_OFFERS);
  /** The supply each truck stands for, −1 for none. */
  choices: StaticArray<i32> = new StaticArray<i32>(3);
  /** A packed member each (`proto.packMember`). */
  company: StaticArray<u8> = new StaticArray<u8>(MAX_COMPANY);
  taken: i64 = 0;
  battle: i64 = 0;
  front: i32 = 0;
  last: i64 = 0;
  money: i64 = 0;

  read(me: House): void {
    const offers = MfOffers.get(me);
    const choices = MfChoices.get(me);
    const low = MfCompanyLow.get(me);
    const high = MfCompanyHigh.get(me);
    for (let at = 0; at < MAX_COMPANY; at++) {
      this.company[at] = proto.byte(at < 5 ? low : high, at % 5);
    }
    for (let at = 0; at < MAX_OFFERS; at++) this.offers[at] = proto.byte(offers, at);
    for (let at = 0; at < 3; at++) {
      const byte = proto.byte(choices, at);
      this.choices[at] = byte > 0 ? <i32>byte - 1 : -1;
    }
    this.phase = MfPhase.get(me);
    this.wave = MfWave.get(me);
    this.banners = MfBanners.get(me);
    this.rank = campRank(MfRank.get(me));
    this.picks = MfPicks.get(me);
    this.flags = MfFlags.get(me);
    this.taken = MfTaken.get(me);
    this.battle = MfBattle.get(me);
    const front = MfFront.get(me);
    this.front = <i32>min<i64>(max<i64>(front, 0), <i64>(FRONT_NAMES.length - 1));
    this.last = MfLast.get(me);
    const money = Client.get(me, Prop.Money);
    this.money = lastStatus() == Status.Ok ? money : 0;
  }

  members(): i32 {
    let count = 0;
    for (let at = 0; at < MAX_COMPANY; at++) if (proto.kindOf(this.company[at]) >= 0) count++;
    return count;
  }

  places(): i32 {
    return PLACES[this.rank];
  }

  row(): i32 {
    return ROW[this.rank];
  }

  /** Non-champion members of `kind`. */
  same(kind: i32): i32 {
    let count = 0;
    for (let at = 0; at < MAX_COMPANY; at++) {
      const member = this.company[at];
      if (proto.kindOf(member) == kind && !proto.bitA(member)) count++;
    }
    return count;
  }
}

const CAMP = new Camp();

function tierName(tier: i32): string {
  return tier == 0 ? "一阶" : tier == 1 ? "二阶" : "三阶";
}

// ------------------------------------------------------------------ Start

function column(item: Item, hold: i64, x: i64, y: i64): Node {
  return item.create(NodeKind.Column, NO_HANDLE,
    new NodeProps().with(Prop.Hold, hold).with(Prop.X, x).with(Prop.Y, y).with(Prop.Gap, GAP));
}

function text(item: Item, parent: Node): Node {
  return item.create(NodeKind.Text, parent, new NodeProps());
}

/** A text on one of this module's pictures: no black under it. */
function plain(item: Item, parent: Node): Node {
  return item.create(NodeKind.Text, parent, new NodeProps().with(Prop.Backdrop, 0));
}

/** A frame picture, made before what it frames so that it is drawn under. */
function picture(item: Item, hold: i64, x: i64, y: i64): Node {
  return item.create(NodeKind.Image, NO_HANDLE, new NodeProps().with(Prop.Hold, hold).with(Prop.X, x).with(Prop.Y, y));
}

/** A button on its plate picture; the engine draws the label over it. */
function button(item: Item, parent: Node, width: i64, plate: string): void {
  item.create(NodeKind.Button, parent, new NodeProps().with(Prop.Width, width).with(Prop.Height, BUTTON_HEIGHT));
  item.art(plate);
}

function start(): void {
  picture(HUD_PANEL, TOP_RIGHT, SIDEBAR - FRAME, TOP - FRAME);
  HUD_PANEL.art("mfpanh.pcx");
  const hud = column(HUD_COLUMN, TOP_RIGHT, SIDEBAR, TOP);
  plain(HUD_WAVE, hud);
  plain(HUD_STATUS, hud);
  plain(HUD_PERKS, hud);
  plain(HUD_PHASE, hud);
  HUD_RAIDERS.create(NodeKind.Bar, hud, new NodeProps().with(Prop.Width, BAR_WIDTH).with(Prop.Height, 8));

  picture(CONTROLS_PANEL, BOTTOM_LEFT, MARGIN - FRAME, BOTTOM - FRAME);
  CONTROLS_PANEL.art("mfpanq.pcx");
  const controls = column(CONTROLS_COLUMN, BOTTOM_LEFT, MARGIN, BOTTOM);
  plain(CONTROLS_TITLE, controls);
  button(CONTROLS_REDRAW, controls, CONTROL_WIDTH, "mfb120s.pcx");
  button(CONTROLS_IMPROVE, controls, CONTROL_WIDTH, "mfb120s.pcx");
  button(CONTROLS_START, controls, CONTROL_WIDTH, "mfb120r.pcx");
  CONTROLS_TITLE.setLabel("军需官");

  for (let at = 0; at < MAX_OFFERS; at++) {
    const card = CARDS[at];
    const x = MARGIN + CONTROL_WIDTH + CARD_GAP + <i64>at * (CARD_WIDTH + CARD_GAP);
    picture(card.frame, BOTTOM_LEFT, x - FRAME, BOTTOM - FRAME);
    const node = column(card.column, BOTTOM_LEFT, x, BOTTOM);
    card.portrait.create(NodeKind.Image, node, new NodeProps());
    plain(card.name, node);
    plain(card.info, node);
    plain(card.merge, node);
    button(card.hire, node, CARD_WIDTH, "mfb136g.pcx");
    button(card.keep, node, CARD_WIDTH, "mfb136s.pcx");
  }

  const company = column(COMPANY_COLUMN, TOP_LEFT, MARGIN, COMPANY_TOP);
  text(COMPANY_TITLE, company);
  for (let at = 0; at < MAX_COMPANY; at++) button(MEMBERS[at], company, COMPANY_WIDTH, "mfb150s.pcx");

  SUPPLIES_TITLE.create(NodeKind.Text, NO_HANDLE, new NodeProps().with(Prop.Hold, MIDDLE).with(Prop.Y, -110));
  for (let at = 0; at < 3; at++) {
    const card = SUPPLY_CARDS[at];
    const x = (<i64>at - 1) * SUPPLY_STEP;
    picture(card.frame, MIDDLE, x, 0);
    card.frame.art("mfsup.pcx");
    const node = column(card.column, MIDDLE, x, 0);
    card.emblem.create(NodeKind.Image, node, new NodeProps());
    plain(card.name, node);
    plain(card.effect, node);
    button(card.pick, node, SUPPLY_WIDTH, "mfb150t.pcx");
    card.pick.setLabel("选这个");
  }
  if (HUD_COLUMN.node == NO_HANDLE) Client.log("佣兵点兵场界面：建不了面板。");
}

// ----------------------------------------------------------------- Render

function render(dt: f32, alpha: f32): void {
  const left = armedLeft - dt;
  armedLeft = left;
  if (left <= 0) armed = -1;
  const me = Client.viewer();
  const known = me != NO_HANDLE;
  if (known) CAMP.read(me);
  const playing = known && (CAMP.phase == MUSTER || CAMP.phase == IN_BATTLE);
  HUD_PANEL.show(playing);
  HUD_COLUMN.show(playing);
  const muster = known && CAMP.phase == MUSTER;
  CONTROLS_PANEL.show(muster);
  CONTROLS_COLUMN.show(muster);
  COMPANY_COLUMN.show(muster);
  for (let at = 0; at < MAX_OFFERS; at++) {
    const card = CARDS[at];
    const shown = known && muster && at < CAMP.row() && proto.kindOf(CAMP.offers[at]) >= 0;
    card.frame.show(shown);
    card.column.show(shown);
  }
  const picking = known && muster && (CAMP.flags & proto.FLAG_TRUCKS) != 0 &&
    (CAMP.choices[0] >= 0 || CAMP.choices[1] >= 0 || CAMP.choices[2] >= 0);
  SUPPLIES_TITLE.show(picking);
  for (let at = 0; at < 3; at++) {
    const card = SUPPLY_CARDS[at];
    const shown = picking && CAMP.choices[at] >= 0;
    card.frame.show(shown);
    card.column.show(shown);
  }
  if (!playing) return;
  hud(CAMP);
  if (muster) {
    controls(CAMP);
    cards(CAMP);
    companyPanel(CAMP);
  }
  if (picking) supplies(CAMP);
}

function hud(camp: Camp): void {
  const number = min<i64>(max<i64>(camp.wave, 1), <i64>waves.WAVES);
  const wave = waves.get(number);
  HUD_WAVE.setLabel(new Line()
    .text("第")
    .num(number)
    .text("/")
    .num(<i64>waves.WAVES)
    .text("波【")
    .text(wave.name)
    .text("】自")
    .text(FRONT_NAMES[camp.front])
    .text("来：")
    .text(wave.hint)
    .str);
  const status = new Line()
    .text("军旗 ")
    .num(camp.banners)
    .text("，营地 ")
    .num(<i64>(camp.rank + 1))
    .text(" 级，队伍 ")
    .num(<i64>camp.members())
    .text("/")
    .num(<i64>camp.places())
    .text("，每杀 ")
    .num(<i64>wave.bounty);
  if (camp.picks > 0) status.text("，待选补给 ").num(camp.picks);
  HUD_STATUS.setLabel(status.str);

  const taken = new Line().text("补给：");
  let any = false;
  for (let perk = 0; perk < perks.PERKS; perk++) {
    const count = proto.taken(camp.taken, perk);
    if (count > 0) {
      taken.textIf(any, " ").text(perks.name(perk));
      if (count > 1) taken.text("x").num(count);
      any = true;
    }
  }
  HUD_PERKS.setLabel(taken.str);
  HUD_PERKS.show(any);

  const ours = proto.unbattle(camp.battle, 0);
  const theirs = proto.unbattle(camp.battle, 1);
  const arrived = proto.unbattle(camp.battle, 2);
  if (camp.phase == IN_BATTLE) {
    HUD_PHASE.setLabel(new Line()
      .text("交战中：我方 ")
      .num(ours)
      .text(" 人，袭击者 ")
      .num(theirs)
      .text("/")
      .num(arrived)
      .str);
    const left = arrived > 0 ? (theirs * 100) / arrived : 0;
    HUD_RAIDERS.fill(<i32>min<i64>(max<i64>(left, 0), 100));
    HUD_RAIDERS.show(arrived > 0);
  } else {
    const result = proto.lastResult(camp.last);
    const line = new Line();
    if (result == 1 || result == 2) {
      line.text("点兵中，上一战")
        .text(result == 1 ? "胜" : "败")
        .text("，歼 ")
        .num(proto.lastKills(camp.last))
        .text("，得 ")
        .num(proto.lastPaid(camp.last))
        .textIf(proto.lastSupply(camp.last), "+补给");
    } else {
      line.text("点兵中：雇佣佣兵，三个同种合成精锐，准备好就开战");
    }
    HUD_PHASE.setLabel(line.str);
    HUD_RAIDERS.show(false);
  }
}

function controls(camp: Camp): void {
  const officer = (camp.flags & proto.FLAG_OFFICER) != 0;
  let free = false;
  for (let at = 0; at < camp.row(); at++) {
    const offer = camp.offers[at];
    if (proto.kindOf(offer) >= 0 && !proto.bitA(offer)) free = true;
  }
  CONTROLS_REDRAW.setLabel(new Line().text("换货 ¥").num(<i64>REDRAW).str);
  CONTROLS_REDRAW.button(true, free && camp.money >= <i64>REDRAW);
  if (camp.rank < IMPROVE.length) {
    const cost = IMPROVE[camp.rank];
    CONTROLS_IMPROVE.setLabel(new Line()
      .text("升到 ")
      .num(<i64>(camp.rank + 2))
      .text(" 级 ¥")
      .num(<i64>cost)
      .str);
    CONTROLS_IMPROVE.button(true, officer && camp.money >= <i64>cost);
  } else {
    CONTROLS_IMPROVE.setLabel("营地已满级");
    CONTROLS_IMPROVE.button(true, false);
  }
  let standing = 0;
  for (let at = 0; at < MAX_COMPANY; at++) if (proto.standing(camp.company[at])) standing++;
  CONTROLS_START.setLabel(new Line().text("开战（").num(<i64>standing).text(" 人）").str);
  CONTROLS_START.button(true, true);
}

function cards(camp: Camp): void {
  const room = camp.members() < camp.places();
  for (let at = 0; at < MAX_OFFERS && at < camp.row(); at++) {
    const card = CARDS[at];
    const offer = camp.offers[at];
    const kind = proto.kindOf(offer);
    if (kind < 0) continue;
    const kept = proto.bitA(offer);
    const standing = proto.standing(offer);
    const merc = content.merc(kind);
    card.portrait.art(new Line().text("mfp_").text(merc.id).text(".pcx").str);
    card.frame.art(kept ? "mfcardk.pcx" : merc.tier == 0 ? "mfcard0.pcx" : merc.tier == 1 ? "mfcard1.pcx" : "mfcard2.pcx");
    card.name.setLabel(new Line().text(merc.name).textIf(kept, "【留】").str);
    const sort = (merc.tags & content.VEHICLE) != 0 ? "载具" : "步兵";
    card.info.setLabel(new Line()
      .text("¥")
      .num(<i64>merc.price)
      .text(" ")
      .text(tierName(merc.tier))
      .text(sort)
      .str);
    const same = camp.same(kind);
    const merges = same >= 2;
    const merge = merges
      ? "再雇一个合成精锐！"
      : same == 1
        ? "已有 1 个，凑三合一"
        : !standing
          ? "保留中，等空位"
          : "";
    // Always a row, so every card is as tall as its frame.
    card.merge.setLabel(merge.length == 0 ? " " : merge);
    const affordable = camp.money >= <i64>merc.price;
    card.hire.setLabel(merges ? "雇佣并合成" : "雇佣");
    card.hire.button(true, standing && affordable && (room || merges));
    card.keep.setLabel(kept ? "取消保留" : "保留");
    card.keep.button(true, standing);
  }
}

function companyPanel(camp: Camp): void {
  const members = camp.members();
  COMPANY_TITLE.setLabel(new Line()
    .text("队伍 ")
    .num(<i64>members)
    .text("/")
    .num(<i64>camp.places())
    .text(members > 0 ? "（点两下遣散）" : "")
    .str);
  for (let at = 0; at < MAX_COMPANY; at++) {
    const item = MEMBERS[at];
    const member = camp.company[at];
    const kind = proto.kindOf(member);
    if (kind < 0) {
      item.button(false, false);
      continue;
    }
    const champion = proto.bitA(member);
    const standing = proto.standing(member);
    const merc = content.merc(kind);
    const label = new Line();
    if (armed == at) {
      label.text("遣散？退 ¥").num(<i64>refund(merc.price, champion));
    } else {
      label.textIf(champion, "精锐 ").text(merc.name).textIf(!standing, "（缺阵）");
    }
    item.setLabel(label.str);
    item.button(true, standing);
  }
}

function supplies(camp: Camp): void {
  SUPPLIES_TITLE.setLabel(new Line()
    .text("选一项补给，全队生效（还可选 ")
    .num(camp.picks)
    .text(" 次）")
    .str);
  for (let at = 0; at < 3; at++) {
    const card = SUPPLY_CARDS[at];
    const perk = camp.choices[at];
    if (perk < 0) continue;
    const taken = proto.taken(camp.taken, perk);
    card.emblem.art(new Line().text("mfk_").num(<i64>perk).text(".pcx").str);
    card.name.setLabel(new Line().text(perks.name(perk)).textIf(taken > 0, "（已有）").str);
    card.effect.setLabel(perks.effect(perk, taken));
    card.pick.button(true, true);
  }
}

// ----------------------------------------------------------------- Clicks

const MESSAGE = new Uint8Array(2);

function send(what: u8, index: i32): void {
  MESSAGE[0] = what;
  MESSAGE[1] = <u8>index;
  if (Client.sendMessage(MESSAGE) != Status.Ok) {
    // Spectators, a replay, or too many clicks at once.
    Client.log("佣兵点兵场界面：操作没有发出。");
  }
}

function onClick(event: ClickEvent): void {
  const node = event.node;
  if (CONTROLS_REDRAW.is(node)) {
    send(proto.MSG_REDRAW, 0);
    return;
  }
  if (CONTROLS_IMPROVE.is(node)) {
    send(proto.MSG_IMPROVE, 0);
    return;
  }
  if (CONTROLS_START.is(node)) {
    send(proto.MSG_START, 0);
    return;
  }
  for (let at = 0; at < MAX_OFFERS; at++) {
    const card = CARDS[at];
    if (card.hire.is(node)) {
      send(proto.MSG_HIRE, at);
      return;
    }
    if (card.keep.is(node)) {
      send(proto.MSG_KEEP, at);
      return;
    }
  }
  for (let at = 0; at < 3; at++) {
    if (SUPPLY_CARDS[at].pick.is(node)) {
      send(proto.MSG_PICK, at);
      return;
    }
  }
  for (let at = 0; at < MAX_COMPANY; at++) {
    if (MEMBERS[at].is(node)) {
      if (armed == at) {
        armed = -1;
        send(proto.MSG_DISMISS, at);
      } else {
        armed = at;
        armedLeft = CONFIRM_SECONDS;
      }
      return;
    }
  }
}

onStart(start);
onRender(render);
onClientEvent<ClickEvent>(ClientEvent.Click, onClick);
