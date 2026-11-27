//! "Muster Field"'s `ui.wasm`: the camp on panels, and a button for every
//! key of the game (RFC 0001 section 10).
//!
//! - Top right, beside the sidebar: the wave and what it brings, banners, camp, company and the
//!   supplies taken; in a battle, both sides' standing and a bar of the
//!   raiders left.
//! - Bottom left: the quartermaster's buttons (redraw, improve, battle),
//!   then a card per offer: name, price, tier, how close it is to a merge,
//!   hire and keep.
//! - Top left, below the game's message lines: the company, a button per
//!   member, filled downwards so a member keeps its place; a second click
//!   within three seconds dismisses it.
//! - Middle: a card per supply to pick from, while a pick is waiting.
//!
//! The pictures are the mod folder's PCX files: `mfp_<type>.pcx` portraits,
//! `mfk_<n>.pcx` supply emblems, `mfcard<tier>.pcx`, `mfcardk.pcx` (kept)
//! and `mfsup.pcx` frames, `mfb<width><colour>.pcx` button plates and
//! `mfpanh.pcx`, `mfpanq.pcx` panels (made apart, not in the repository). The
//! engine draws a button's label over its plate and dims or brightens the
//! plate as the button is disabled or pointed at; texts on these pictures
//! go without their black backdrop. A missing picture leaves its place
//! empty, and a button without its plate falls back to the plain look.
//!
//! It reads only the player's private variables the simulation publishes
//! (`muster_field_data::proto`) and the player's money, and changes the
//! game only through two-byte messages, which the simulation turns into
//! the orders the keys give. Spectators see nothing. What it shows is in
//! Simplified Chinese.
#![no_std]

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering::Relaxed};

use muster_field_data::camp::{self, IMPROVE, PLACES, REDRAW, ROW};
use muster_field_data::content::{self, MERCS};
use muster_field_data::perks::{self, PERKS};
use muster_field_data::proto::{self, Last, Member, Offer};
use muster_field_data::waves::{self, WAVES};
use muster_field_data::{FRONT_NAMES, Line, MAX_COMPANY, MAX_OFFERS};
use vera20k_sdk::client::{self, prelude::*};

// ------------------------------------------------------------------ Layout

const TOP_LEFT: i64 = 0;
const TOP_RIGHT: i64 = 2;
const MIDDLE: i64 = 4;
const BOTTOM_LEFT: i64 = 6;

const MARGIN: i64 = 8;
/// Below the match's top tabs.
const TOP: i64 = 24;
/// Left of the sidebar: the top left holds the game's message lines.
const SIDEBAR: i64 = 176;
/// Above the command bar along the battlefield's bottom.
const BOTTOM: i64 = 44;
const GAP: i64 = 2;
const CARD_WIDTH: i64 = 136;
const CARD_GAP: i64 = 14;
/// How far a frame picture reaches past what it holds, each side.
const FRAME: i64 = 5;
const BUTTON_HEIGHT: i64 = 20;
const CONTROL_WIDTH: i64 = 120;
const COMPANY_WIDTH: i64 = 150;
const SUPPLY_WIDTH: i64 = 150;
/// Between the middles of two supply cards.
const SUPPLY_STEP: i64 = 176;
const BAR_WIDTH: i64 = 240;

/// Below the game's message lines.
const COMPANY_TOP: i64 = 140;

/// A second dismiss click counts within so many seconds, whatever the game
/// speed.
const CONFIRM_SECONDS: f32 = 3.0;

// ------------------------------------------------------------------- Nodes

/// A node and what was last written to it, so that an unchanged frame
/// writes nothing.
struct Item {
    node: NodeSlot,
    label: AtomicU32,
    art: AtomicU32,
    /// Bit 0 visible, bit 1 enabled; −1 before the first write.
    state: AtomicI32,
    fill: AtomicI32,
}

impl Item {
    const NEW: Item = Item {
        node: NodeSlot::new(),
        label: AtomicU32::new(0),
        art: AtomicU32::new(0),
        state: AtomicI32::new(-1),
        fill: AtomicI32::new(-1),
    };

    fn node(&self) -> Node {
        self.node.get()
    }

    fn create(&self, kind: NodeKind, parent: Node, props: &[(Prop, i64)]) -> Node {
        let node = Ui::create_with(kind, parent, props).unwrap_or(Node(NO_HANDLE));
        if kind == NodeKind::Button && !node.is_none() {
            let _ = node.set_clickable(true);
        }
        self.node.set(node);
        node
    }

    fn label(&self, text: &str) {
        let hash = fnv(text);
        if self.label.load(Relaxed) != hash && self.node().set_label(text).is_ok() {
            self.label.store(hash, Relaxed);
        }
    }

    /// A picture the mod folder holds (`Art`, appendix 7.10).
    fn art(&self, name: &str) {
        let hash = fnv(name);
        if self.art.load(Relaxed) != hash && self.node().set_art(name).is_ok() {
            self.art.store(hash, Relaxed);
        }
    }

    fn show(&self, visible: bool) {
        let state = i32::from(visible);
        if self.state.load(Relaxed) != state && self.node().set_visible(visible).is_ok() {
            self.state.store(state, Relaxed);
        }
    }

    /// A button: shown, and clickable or greyed.
    fn button(&self, visible: bool, enabled: bool) {
        let state = i32::from(visible) | i32::from(enabled) << 1;
        if self.state.load(Relaxed) == state {
            return;
        }
        let node = self.node();
        if node.set_visible(visible).is_ok() && node.set_enabled(enabled).is_ok() {
            self.state.store(state, Relaxed);
        }
    }

    fn fill(&self, percent: i32) {
        if self.fill.load(Relaxed) != percent && self.node().set_fill(percent).is_ok() {
            self.fill.store(percent, Relaxed);
        }
    }

    fn is(&self, node: Node) -> bool {
        !node.is_none() && self.node() == node
    }
}

/// 32-bit FNV-1a, never 0 (the "nothing written" mark).
fn fnv(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in text.as_bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash.max(1)
}

struct Hud {
    /// The panel picture behind the column.
    panel: Item,
    column: Item,
    wave: Item,
    status: Item,
    perks: Item,
    phase: Item,
    raiders: Item,
}

struct Card {
    /// The frame picture behind the column, made before it.
    frame: Item,
    column: Item,
    portrait: Item,
    name: Item,
    info: Item,
    merge: Item,
    hire: Item,
    keep: Item,
}

impl Card {
    const NEW: Card = Card {
        frame: Item::NEW,
        column: Item::NEW,
        portrait: Item::NEW,
        name: Item::NEW,
        info: Item::NEW,
        merge: Item::NEW,
        hire: Item::NEW,
        keep: Item::NEW,
    };
}

struct Controls {
    panel: Item,
    column: Item,
    title: Item,
    redraw: Item,
    improve: Item,
    start: Item,
}

struct Company {
    column: Item,
    title: Item,
    members: [Item; MAX_COMPANY],
}

struct Supplies {
    title: Item,
    cards: [SupplyCard; 3],
}

struct SupplyCard {
    frame: Item,
    column: Item,
    emblem: Item,
    name: Item,
    effect: Item,
    pick: Item,
}

impl SupplyCard {
    const NEW: SupplyCard = SupplyCard {
        frame: Item::NEW,
        column: Item::NEW,
        emblem: Item::NEW,
        name: Item::NEW,
        effect: Item::NEW,
        pick: Item::NEW,
    };
}

static HUD: Hud = Hud {
    panel: Item::NEW,
    column: Item::NEW,
    wave: Item::NEW,
    status: Item::NEW,
    perks: Item::NEW,
    phase: Item::NEW,
    raiders: Item::NEW,
};
static CARDS: [Card; MAX_OFFERS] = [Card::NEW; MAX_OFFERS];
static CONTROLS: Controls = Controls {
    panel: Item::NEW,
    column: Item::NEW,
    title: Item::NEW,
    redraw: Item::NEW,
    improve: Item::NEW,
    start: Item::NEW,
};
static COMPANY: Company = Company {
    column: Item::NEW,
    title: Item::NEW,
    members: [Item::NEW; MAX_COMPANY],
};
static SUPPLIES: Supplies = Supplies {
    title: Item::NEW,
    cards: [SupplyCard::NEW; 3],
};

/// The member a first dismiss click armed, and the seconds it has left
/// (f32 bits); −1 for none.
static ARMED: AtomicI32 = AtomicI32::new(-1);
static ARMED_LEFT: AtomicU32 = AtomicU32::new(0);

// ------------------------------------------------------------------- State

static PHASE: ClientVar = ClientVar::new(proto::PHASE);
static WAVE: ClientVar = ClientVar::new(proto::WAVE);
static BANNERS: ClientVar = ClientVar::new(proto::BANNERS);
static RANK: ClientVar = ClientVar::new(proto::RANK);
static PICKS: ClientVar = ClientVar::new(proto::PICKS);
static FLAGS: ClientVar = ClientVar::new(proto::FLAGS);
static OFFERS: ClientVar = ClientVar::new(proto::OFFERS);
static CHOICES: ClientVar = ClientVar::new(proto::CHOICES);
static COMPANY_LOW: ClientVar = ClientVar::new(proto::COMPANY_LOW);
static COMPANY_HIGH: ClientVar = ClientVar::new(proto::COMPANY_HIGH);
static TAKEN: ClientVar = ClientVar::new(proto::TAKEN);
static BATTLE: ClientVar = ClientVar::new(proto::BATTLE);
static FRONT: ClientVar = ClientVar::new(proto::FRONT);
static LAST: ClientVar = ClientVar::new(proto::LAST);

const MUSTER: i64 = 1;
const IN_BATTLE: i64 = 2;

/// One render frame's reading of the camp.
struct Camp {
    phase: i64,
    wave: i64,
    banners: i64,
    rank: usize,
    picks: i64,
    flags: i64,
    offers: [Offer; MAX_OFFERS],
    choices: [Option<usize>; 3],
    company: [Member; MAX_COMPANY],
    taken: i64,
    battle: (i64, i64, i64),
    front: usize,
    last: Last,
    money: i64,
}

impl Camp {
    fn read(me: House) -> Self {
        let read = |var: &ClientVar| var.get_house(me).unwrap_or(0);
        let offers = read(&OFFERS);
        let choices = read(&CHOICES);
        let low = read(&COMPANY_LOW);
        let high = read(&COMPANY_HIGH);
        let mut company = [Member::default(); MAX_COMPANY];
        for (at, member) in company.iter_mut().enumerate() {
            let half = if at < 5 { low } else { high };
            *member = Member::unpack(proto::byte(half, at % 5));
        }
        let mut offer = [Offer::default(); MAX_OFFERS];
        for (at, slot) in offer.iter_mut().enumerate() {
            *slot = Offer::unpack(proto::byte(offers, at));
        }
        let mut choice = [None; 3];
        for (at, slot) in choice.iter_mut().enumerate() {
            let byte = proto::byte(choices, at);
            *slot = (byte > 0).then(|| usize::from(byte - 1));
        }
        Self {
            phase: read(&PHASE),
            wave: read(&WAVE),
            banners: read(&BANNERS),
            rank: camp::rank(read(&RANK)),
            picks: read(&PICKS),
            flags: read(&FLAGS),
            offers: offer,
            choices: choice,
            company,
            taken: read(&TAKEN),
            battle: proto::unbattle(read(&BATTLE)),
            front: (read(&FRONT).max(0) as usize).min(FRONT_NAMES.len() - 1),
            last: Last::unpack(read(&LAST)),
            money: client::get(me, Prop::Money).unwrap_or(0),
        }
    }

    fn members(&self) -> usize {
        self.company.iter().filter(|member| member.kind.is_some()).count()
    }

    fn places(&self) -> usize {
        PLACES[self.rank]
    }

    fn row(&self) -> usize {
        ROW[self.rank]
    }

    /// Non-champion members of `kind`.
    fn same(&self, kind: usize) -> usize {
        self.company
            .iter()
            .filter(|member| member.kind == Some(kind) && !member.champion)
            .count()
    }
}

fn merc(kind: usize) -> &'static content::Merc {
    MERCS.get(kind).unwrap_or(&MERCS[0])
}

fn tier_name(tier: usize) -> &'static str {
    match tier {
        0 => "一阶",
        1 => "二阶",
        _ => "三阶",
    }
}

// ------------------------------------------------------------------- Start

fn start() {
    let root = Node(NO_HANDLE);
    let column = |item: &Item, hold: i64, x: i64, y: i64| {
        item.create(
            NodeKind::Column,
            root,
            &[
                (Prop::Hold, hold),
                (Prop::X, x),
                (Prop::Y, y),
                (Prop::Gap, GAP),
            ],
        )
    };
    let text = |item: &Item, parent: Node| item.create(NodeKind::Text, parent, &[]);
    // A text on one of this module's pictures: no black under it.
    let plain = |item: &Item, parent: Node| {
        item.create(NodeKind::Text, parent, &[(Prop::Backdrop, 0)])
    };
    // A frame picture, made before what it frames so that it is drawn under.
    let picture = |item: &Item, hold: i64, x: i64, y: i64| {
        item.create(
            NodeKind::Image,
            root,
            &[(Prop::Hold, hold), (Prop::X, x), (Prop::Y, y)],
        )
    };
    // A button on its plate picture; the engine draws the label over it.
    let button = |item: &Item, parent: Node, width: i64, plate: &str| {
        item.create(
            NodeKind::Button,
            parent,
            &[(Prop::Width, width), (Prop::Height, BUTTON_HEIGHT)],
        );
        item.art(plate);
    };

    picture(&HUD.panel, TOP_RIGHT, SIDEBAR - FRAME, TOP - FRAME);
    HUD.panel.art("mfpanh.pcx");
    let hud = column(&HUD.column, TOP_RIGHT, SIDEBAR, TOP);
    for item in [&HUD.wave, &HUD.status, &HUD.perks, &HUD.phase] {
        plain(item, hud);
    }
    HUD.raiders.create(
        NodeKind::Bar,
        hud,
        &[(Prop::Width, BAR_WIDTH), (Prop::Height, 8)],
    );

    picture(&CONTROLS.panel, BOTTOM_LEFT, MARGIN - FRAME, BOTTOM - FRAME);
    CONTROLS.panel.art("mfpanq.pcx");
    let controls = column(&CONTROLS.column, BOTTOM_LEFT, MARGIN, BOTTOM);
    plain(&CONTROLS.title, controls);
    button(&CONTROLS.redraw, controls, CONTROL_WIDTH, "mfb120s.pcx");
    button(&CONTROLS.improve, controls, CONTROL_WIDTH, "mfb120s.pcx");
    button(&CONTROLS.start, controls, CONTROL_WIDTH, "mfb120r.pcx");
    CONTROLS.title.label("军需官");

    for (at, card) in CARDS.iter().enumerate() {
        let x = MARGIN + CONTROL_WIDTH + CARD_GAP + at as i64 * (CARD_WIDTH + CARD_GAP);
        picture(&card.frame, BOTTOM_LEFT, x - FRAME, BOTTOM - FRAME);
        let node = column(&card.column, BOTTOM_LEFT, x, BOTTOM);
        card.portrait.create(NodeKind::Image, node, &[]);
        for item in [&card.name, &card.info, &card.merge] {
            plain(item, node);
        }
        button(&card.hire, node, CARD_WIDTH, "mfb136g.pcx");
        button(&card.keep, node, CARD_WIDTH, "mfb136s.pcx");
    }

    let company = column(&COMPANY.column, TOP_LEFT, MARGIN, COMPANY_TOP);
    text(&COMPANY.title, company);
    for item in &COMPANY.members {
        button(item, company, COMPANY_WIDTH, "mfb150s.pcx");
    }

    SUPPLIES.title.create(
        NodeKind::Text,
        root,
        &[(Prop::Hold, MIDDLE), (Prop::Y, -110)],
    );
    for (at, card) in SUPPLIES.cards.iter().enumerate() {
        let x = (at as i64 - 1) * SUPPLY_STEP;
        picture(&card.frame, MIDDLE, x, 0);
        card.frame.art("mfsup.pcx");
        let node = column(&card.column, MIDDLE, x, 0);
        card.emblem.create(NodeKind::Image, node, &[]);
        plain(&card.name, node);
        plain(&card.effect, node);
        button(&card.pick, node, SUPPLY_WIDTH, "mfb150t.pcx");
        card.pick.label("选这个");
    }
    if HUD.column.node().is_none() {
        client::log("佣兵点兵场界面：建不了面板。");
    }
}

// ------------------------------------------------------------------ Render

fn render(dt: f32, _alpha: f32) {
    let left = f32::from_bits(ARMED_LEFT.load(Relaxed)) - dt;
    ARMED_LEFT.store(left.to_bits(), Relaxed);
    if left <= 0.0 {
        ARMED.store(-1, Relaxed);
    }
    let me = Client::viewer();
    let camp = (!me.is_none()).then(|| Camp::read(me));
    let playing = camp
        .as_ref()
        .is_some_and(|camp| camp.phase == MUSTER || camp.phase == IN_BATTLE);
    HUD.panel.show(playing);
    HUD.column.show(playing);
    let muster = camp.as_ref().is_some_and(|camp| camp.phase == MUSTER);
    CONTROLS.panel.show(muster);
    CONTROLS.column.show(muster);
    COMPANY.column.show(muster);
    for (at, card) in CARDS.iter().enumerate() {
        let shown = camp
            .as_ref()
            .is_some_and(|camp| muster && at < camp.row() && camp.offers[at].kind.is_some());
        card.frame.show(shown);
        card.column.show(shown);
    }
    let picking = camp.as_ref().is_some_and(|camp| {
        muster && camp.flags & proto::FLAG_TRUCKS != 0 && camp.choices.iter().any(Option::is_some)
    });
    SUPPLIES.title.show(picking);
    for (at, card) in SUPPLIES.cards.iter().enumerate() {
        let shown = picking && camp.as_ref().is_some_and(|camp| camp.choices[at].is_some());
        card.frame.show(shown);
        card.column.show(shown);
    }
    let Some(camp) = camp.filter(|_| playing) else {
        return;
    };
    hud(&camp);
    if muster {
        controls(&camp);
        cards(&camp);
        company(&camp);
    }
    if picking {
        supplies(&camp);
    }
}

fn hud(camp: &Camp) {
    let number = camp.wave.clamp(1, WAVES as i64);
    let wave = waves::get(number);
    HUD.wave.label(
        Line::new()
            .text("第")
            .num(number)
            .text("/")
            .num(WAVES as i64)
            .text("波【")
            .text(wave.name)
            .text("】自")
            .text(FRONT_NAMES[camp.front])
            .text("来：")
            .text(wave.hint)
            .as_str(),
    );
    let mut status = Line::new()
        .text("军旗 ")
        .num(camp.banners)
        .text("，营地 ")
        .num(camp.rank as i64 + 1)
        .text(" 级，队伍 ")
        .num(camp.members() as i64)
        .text("/")
        .num(camp.places() as i64)
        .text("，每杀 ")
        .num(i64::from(wave.bounty));
    if camp.picks > 0 {
        status = status.text("，待选补给 ").num(camp.picks);
    }
    HUD.status.label(status.as_str());

    let mut taken = Line::new().text("补给：");
    let mut any = false;
    for perk in 0..PERKS {
        let count = proto::taken(camp.taken, perk);
        if count > 0 {
            taken = taken.text_if(any, " ").text(perks::name(perk));
            if count > 1 {
                taken = taken.text("x").num(count);
            }
            any = true;
        }
    }
    HUD.perks.label(taken.as_str());
    HUD.perks.show(any);

    let (ours, theirs, arrived) = camp.battle;
    if camp.phase == IN_BATTLE {
        HUD.phase.label(
            Line::new()
                .text("交战中：我方 ")
                .num(ours)
                .text(" 人，袭击者 ")
                .num(theirs)
                .text("/")
                .num(arrived)
                .as_str(),
        );
        let left = if arrived > 0 { theirs * 100 / arrived } else { 0 };
        HUD.raiders.fill(left.clamp(0, 100) as i32);
        HUD.raiders.show(arrived > 0);
    } else {
        let last = camp.last;
        let line = match last.result {
            1 | 2 => Line::new()
                .text("点兵中，上一战")
                .text(if last.result == 1 { "胜" } else { "败" })
                .text("，歼 ")
                .num(last.kills)
                .text("，得 ")
                .num(last.paid)
                .text_if(last.supply, "+补给"),
            _ => Line::new().text("点兵中：雇佣佣兵，三个同种合成精锐，准备好就开战"),
        };
        HUD.phase.label(line.as_str());
        HUD.raiders.show(false);
    }
}

fn controls(camp: &Camp) {
    let officer = camp.flags & proto::FLAG_OFFICER != 0;
    let free = camp.offers[..camp.row()]
        .iter()
        .any(|offer| offer.kind.is_some() && !offer.kept);
    CONTROLS.redraw.label(Line::new().text("换货 ¥").num(i64::from(REDRAW)).as_str());
    CONTROLS
        .redraw
        .button(true, free && camp.money >= i64::from(REDRAW));
    match IMPROVE.get(camp.rank) {
        Some(&cost) => {
            CONTROLS.improve.label(
                Line::new()
                    .text("升到 ")
                    .num(camp.rank as i64 + 2)
                    .text(" 级 ¥")
                    .num(i64::from(cost))
                    .as_str(),
            );
            CONTROLS
                .improve
                .button(true, officer && camp.money >= i64::from(cost));
        }
        None => {
            CONTROLS.improve.label("营地已满级");
            CONTROLS.improve.button(true, false);
        }
    }
    CONTROLS.start.label(
        Line::new()
            .text("开战（")
            .num(camp.company.iter().filter(|m| m.standing).count() as i64)
            .text(" 人）")
            .as_str(),
    );
    CONTROLS.start.button(true, true);
}

fn cards(camp: &Camp) {
    let room = camp.members() < camp.places();
    for (at, card) in CARDS.iter().enumerate().take(camp.row()) {
        let offer = camp.offers[at];
        let Some(kind) = offer.kind else { continue };
        let merc = merc(kind);
        card.portrait.art(Line::new().text("mfp_").text(merc.id).text(".pcx").as_str());
        card.frame.art(if offer.kept {
            "mfcardk.pcx"
        } else {
            match merc.tier {
                0 => "mfcard0.pcx",
                1 => "mfcard1.pcx",
                _ => "mfcard2.pcx",
            }
        });
        card.name.label(
            Line::new()
                .text(merc.name)
                .text_if(offer.kept, "【留】")
                .as_str(),
        );
        let sort = if merc.tags & content::VEHICLE != 0 {
            "载具"
        } else {
            "步兵"
        };
        card.info.label(
            Line::new()
                .text("¥")
                .num(i64::from(merc.price))
                .text(" ")
                .text(tier_name(merc.tier))
                .text(sort)
                .as_str(),
        );
        let same = camp.same(kind);
        let merges = same >= 2;
        let merge = if merges {
            "再雇一个合成精锐！"
        } else if same == 1 {
            "已有 1 个，凑三合一"
        } else if !offer.standing {
            "保留中，等空位"
        } else {
            ""
        };
        // Always a row, so every card is as tall as its frame.
        card.merge.label(if merge.is_empty() { " " } else { merge });
        let affordable = camp.money >= i64::from(merc.price);
        card.hire.label(if merges { "雇佣并合成" } else { "雇佣" });
        card.hire
            .button(true, offer.standing && affordable && (room || merges));
        card.keep.label(if offer.kept { "取消保留" } else { "保留" });
        card.keep.button(true, offer.standing);
    }
}

fn company(camp: &Camp) {
    let members = camp.members();
    COMPANY.title.label(
        Line::new()
            .text("队伍 ")
            .num(members as i64)
            .text("/")
            .num(camp.places() as i64)
            .text(if members > 0 {
                "（点两下遣散）"
            } else {
                ""
            })
            .as_str(),
    );
    let armed = ARMED.load(Relaxed);
    for (at, (item, member)) in COMPANY.members.iter().zip(&camp.company).enumerate() {
        let Some(kind) = member.kind else {
            item.button(false, false);
            continue;
        };
        let merc = merc(kind);
        let label = if armed == at as i32 {
            Line::new()
                .text("遣散？退 ¥")
                .num(i64::from(camp::refund(merc.price, member.champion)))
        } else {
            Line::new()
                .text_if(member.champion, "精锐 ")
                .text(merc.name)
                .text_if(!member.standing, "（缺阵）")
        };
        item.label(label.as_str());
        item.button(true, member.standing);
    }
}

fn supplies(camp: &Camp) {
    SUPPLIES.title.label(
        Line::new()
            .text("选一项补给，全队生效（还可选 ")
            .num(camp.picks)
            .text(" 次）")
            .as_str(),
    );
    for (card, choice) in SUPPLIES.cards.iter().zip(camp.choices) {
        let Some(perk) = choice else { continue };
        let taken = proto::taken(camp.taken, perk);
        card.emblem.art(Line::new().text("mfk_").num(perk as i64).text(".pcx").as_str());
        card.name.label(
            Line::new()
                .text(perks::name(perk))
                .text_if(taken > 0, "（已有）")
                .as_str(),
        );
        card.effect.label(perks::effect(perk, taken));
        card.pick.button(true, true);
    }
}

// ------------------------------------------------------------------ Clicks

fn send(what: u8, index: usize) {
    if Client::send_message(&[what, index as u8]).is_err() {
        // Spectators, a replay, or too many clicks at once.
        client::log("佣兵点兵场界面：操作没有发出。");
    }
}

fn on_click(event: &ClientEventRecord) {
    let node = event.node();
    if CONTROLS.redraw.is(node) {
        return send(proto::MSG_REDRAW, 0);
    }
    if CONTROLS.improve.is(node) {
        return send(proto::MSG_IMPROVE, 0);
    }
    if CONTROLS.start.is(node) {
        return send(proto::MSG_START, 0);
    }
    for (at, card) in CARDS.iter().enumerate() {
        if card.hire.is(node) {
            return send(proto::MSG_HIRE, at);
        }
        if card.keep.is(node) {
            return send(proto::MSG_KEEP, at);
        }
    }
    for (at, card) in SUPPLIES.cards.iter().enumerate() {
        if card.pick.is(node) {
            return send(proto::MSG_PICK, at);
        }
    }
    for (at, item) in COMPANY.members.iter().enumerate() {
        if item.is(node) {
            if ARMED.load(Relaxed) == at as i32 {
                ARMED.store(-1, Relaxed);
                send(proto::MSG_DISMISS, at);
            } else {
                ARMED.store(at as i32, Relaxed);
                ARMED_LEFT.store(CONFIRM_SECONDS.to_bits(), Relaxed);
            }
            return;
        }
    }
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_render!(render);
vera20k_sdk::on_client_event!(ClientEvent::Click => on_click);
