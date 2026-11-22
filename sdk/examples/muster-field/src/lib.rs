//! "Muster Field", a playtest `rules.wasm` (RFC 0001 preview): a small
//! auto-battler against one computer house, played with ordinary unit orders
//! and no interface code.
//!
//! - Camp: the starting forces leave, and three mercenaries stand in a row
//!   where the player started. They cannot move or fire until hired.
//! - Hire: order a mercenary to move out of the row. It pays its price and
//!   walks to the clicked cell, so hiring also sets the battle line.
//! - Dismiss: order a hired unit back into the row for 3/4 of its price.
//! - Redraw: press Stop on a waiting mercenary to replace all three for a
//!   fee. A row that was hired out refills free.
//! - Champion: three hired units of a type merge into one elite unit with
//!   2.5x firepower and 3x armor, at full health.
//! - Battle: press Scatter on a hired unit. The muster has no time limit: it
//!   waits for the player, and frames measure no time to think at unlimited
//!   game speed. Both sides then fight by themselves until one is gone or the
//!   battle's time is up.
//!
//! What it shows of the API: `CommandAllowed` as player input (the handler
//! may not write, so it queues the order in module memory and `on_frame`
//! carries it out), `SetMission` for a human house's units (`House::order`
//! takes computer houses only), timed holds through modifiers, and a
//! `Defeat` veto while both houses have nothing on the field.
//!
//! It is made for one human player against computer opponents, with Bases on.
//! What it tells the player is in Simplified Chinese.
//! Build it with `cargo build --release`, rename the `.wasm` to `rules.wasm`
//! and put it in a mod folder (docs/wasm-modules.md).
#![no_std]

use core::sync::atomic::{AtomicI32, AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get, set};

const ON_COMMAND: i32 = 1;
const ON_DEFEAT: i32 = 2;

struct Mercenary {
    id: &'static str,
    name: &'static str,
    price: i32,
}

const POOL: [Mercenary; 4] = [
    Mercenary {
        id: "E1",
        name: "美国大兵",
        price: 200,
    },
    Mercenary {
        id: "GGI",
        name: "重装大兵",
        price: 340,
    },
    Mercenary {
        id: "HTK",
        name: "防空履带车",
        price: 460,
    },
    Mercenary {
        id: "MTNK",
        name: "灰熊坦克",
        price: 680,
    },
];
/// The raiders, strongest first.
const RAIDERS: [&str; 7] = ["HTNK", "HTNK", "E2", "E2", "E2", "E2", "E2"];
/// What a computer opponent starts with or deploys into.
const BASE_TYPES: [&str; 6] = ["AMCV", "SMCV", "PCV", "GACNST", "NACNST", "YACNST"];

const PURSE: i64 = 2400;
const REDRAW: i32 = 60;
const OFFERS: usize = 3;
const SQUAD: usize = 10;
/// Cells between two mercenaries of the row.
const SPACING: i32 = 3;
/// Cells from the camp to the raiders.
const FIELD: i32 = 14;
/// The battle's length in mission timer seconds (15 frames each).
const BATTLE_SECONDS: i32 = 240;

const WAITING: i32 = 0;
const MUSTER: i32 = 1;
const BATTLE: i32 = 2;
const OVER: i32 = 3;

// ------------------------------------------------------------------- State

// Module memory is saved and hashed with the match, so plain statics do.
static PHASE: AtomicI32 = AtomicI32::new(WAITING);
static DEADLINE: AtomicI32 = AtomicI32::new(0);
static PLAYER: AtomicI64 = AtomicI64::new(NO_HANDLE);
static ENEMY: AtomicI64 = AtomicI64::new(NO_HANDLE);
static CAMP: AtomicI64 = AtomicI64::new(NO_HANDLE);

/// One unit the module follows, with its index in [`POOL`].
struct Slot {
    unit: AtomicI64,
    kind: AtomicI32,
}

impl Slot {
    const EMPTY: Slot = Slot {
        unit: AtomicI64::new(NO_HANDLE),
        kind: AtomicI32::new(0),
    };

    fn unit(&self) -> Techno {
        Techno(self.unit.load(Relaxed))
    }

    fn kind(&self) -> usize {
        (self.kind.load(Relaxed) as usize).min(POOL.len() - 1)
    }

    fn is_empty(&self) -> bool {
        self.unit.load(Relaxed) == NO_HANDLE
    }

    fn put(&self, unit: Techno, kind: usize) {
        self.unit.store(unit.0, Relaxed);
        self.kind.store(kind as i32, Relaxed);
    }

    fn clear(&self) {
        self.unit.store(NO_HANDLE, Relaxed);
    }
}

static ROW: [Slot; OFFERS] = [Slot::EMPTY; OFFERS];
/// The holds on each mercenary of the row: speed, then weapons.
static HOLDS: [[AtomicI64; 2]; OFFERS] =
    [const { [AtomicI64::new(NO_HANDLE), AtomicI64::new(NO_HANDLE)] }; OFFERS];
static HIRED: [Slot; SQUAD] = [Slot::EMPTY; SQUAD];
static CHAMPION: [AtomicI32; SQUAD] = [const { AtomicI32::new(0) }; SQUAD];
static RAID: [Slot; RAIDERS.len()] = [Slot::EMPTY; RAIDERS.len()];

fn position(slots: &[Slot], unit: Techno) -> Option<usize> {
    slots
        .iter()
        .position(|slot| !slot.is_empty() && slot.unit() == unit)
}

// ------------------------------------------------------------------- Input

/// Player orders waiting for the next module phase.
struct Pending {
    unit: AtomicI64,
    cell: AtomicI64,
    command: AtomicI32,
}

const INBOX: usize = 32;
static PENDING: [Pending; INBOX] = [const {
    Pending {
        unit: AtomicI64::new(NO_HANDLE),
        cell: AtomicI64::new(NO_HANDLE),
        command: AtomicI32::new(0),
    }
}; INBOX];
static PENDING_LEN: AtomicI32 = AtomicI32::new(0);

/// Runs as the engine executes a player's order. It may only read the game,
/// so it refuses the order and notes it for [`tick`].
fn command(mut decision: CommandAllowedDecision) {
    let phase = PHASE.load(Relaxed);
    let unit = decision.unit();
    let offered = position(&ROW, unit).is_some();
    if !offered && position(&HIRED, unit).is_none() {
        return;
    }
    let command = decision.command();
    let is_move = command == Command::Move as i32;
    if phase == MUSTER && !offered && is_move && !in_row(decision.target_cell()) {
        return; // A hired unit takes its place in the line.
    }
    decision.set_result(0);
    let wanted = is_move
        || (offered && command == Command::Stop as i32)
        || (!offered && command == Command::Scatter as i32);
    let len = PENDING_LEN.load(Relaxed) as usize;
    if phase != MUSTER || !wanted || len >= INBOX {
        return;
    }
    PENDING[len].unit.store(unit.0, Relaxed);
    PENDING[len].cell.store(decision.target_cell().0, Relaxed);
    PENDING[len].command.store(command, Relaxed);
    PENDING_LEN.store(len as i32 + 1, Relaxed);
}

fn read_orders() {
    let len = (PENDING_LEN.swap(0, Relaxed) as usize).min(INBOX);
    // A selected group sends one order per unit; redraw once for all of them.
    let mut redrawn = false;
    for order in &PENDING[..len] {
        if PHASE.load(Relaxed) != MUSTER {
            return;
        }
        let unit = Techno(order.unit.load(Relaxed));
        let cell = Cell(order.cell.load(Relaxed));
        let command = order.command.load(Relaxed);
        if let Some(at) = position(&ROW, unit) {
            if command == Command::Move as i32 {
                hire(at, cell);
            } else if !redrawn {
                redrawn = true;
                redraw();
            }
        } else if let Some(at) = position(&HIRED, unit) {
            if command == Command::Move as i32 {
                dismiss(at);
            } else {
                start_battle();
            }
        }
    }
}

// --------------------------------------------------------------------- Row

fn in_row(cell: Cell) -> bool {
    let camp = Cell(CAMP.load(Relaxed));
    let dx = i32::from(cell.x()) - i32::from(camp.x());
    let dy = i32::from(cell.y()) - i32::from(camp.y());
    dx.abs() <= SPACING + 1 && dy.abs() <= 1
}

fn fill_row() {
    let camp = Cell(CAMP.load(Relaxed));
    let house = House(PLAYER.load(Relaxed));
    for (at, slot) in ROW.iter().enumerate() {
        if !slot.is_empty() {
            continue;
        }
        let kind = random(0, POOL.len() as i32 - 1) as usize;
        let Ok(ty) = TypeRef::techno_type(POOL[kind].id) else {
            continue;
        };
        let spot = offset(camp, (at as i32 - 1) * SPACING, 0);
        let Some(unit) = create_near(spot, ty, house) else {
            continue;
        };
        slot.put(unit, kind);
        for (hold, (prop, op, value)) in HOLDS[at].iter().zip([
            (Prop::SpeedMultiplier, ModifierOp::Multiply, 0),
            (Prop::DisableWeapons, ModifierOp::Override, 1),
        ]) {
            let held = Modifier::new(unit, prop, op, value, -1, 0, 0);
            hold.store(held.map_or(NO_HANDLE, |modifier| modifier.0), Relaxed);
        }
    }
    let mut line = Line::new().text("待雇佣：");
    for slot in ROW.iter().filter(|slot| !slot.is_empty()) {
        let offer = &POOL[slot.kind()];
        line = line
            .text("  ")
            .text(offer.name)
            .text(" ")
            .num(i64::from(offer.price));
    }
    line.log();
}

fn empty_row() {
    for slot in &ROW {
        if !slot.is_empty() {
            let _ = Action::remove(slot.unit());
            slot.clear();
        }
    }
}

fn redraw() {
    if credits() < i64::from(REDRAW) {
        log("资金不足，无法刷新。");
        return;
    }
    if Action::add_money(House(PLAYER.load(Relaxed)), -REDRAW).is_ok() {
        empty_row();
        fill_row();
    }
}

fn hire(at: usize, to: Cell) {
    if in_row(to) {
        log("把佣兵移到货位外面才算雇佣。");
        return;
    }
    let unit = ROW[at].unit();
    let kind = ROW[at].kind();
    let offer = &POOL[kind];
    let Some(free) = HIRED.iter().position(Slot::is_empty) else {
        log("队伍已满，请先遣散一名佣兵。");
        return;
    };
    if credits() < i64::from(offer.price) {
        Line::new()
            .text("资金不足：")
            .text(offer.name)
            .text("需要 ")
            .num(i64::from(offer.price))
            .log();
        return;
    }
    if Action::add_money(House(PLAYER.load(Relaxed)), -offer.price).is_err() {
        return;
    }
    for hold in &HOLDS[at] {
        let _ = Modifier(hold.swap(NO_HANDLE, Relaxed)).remove();
    }
    ROW[at].clear();
    HIRED[free].put(unit, kind);
    CHAMPION[free].store(0, Relaxed);
    let _ = Action::set_mission(unit, Mission::Move, to);
    Line::new()
        .text("已雇佣：")
        .text(offer.name)
        .text("，剩余资金 ")
        .num(credits())
        .log();
    merge(free);
    if ROW.iter().all(Slot::is_empty) {
        fill_row();
    }
}

fn dismiss(at: usize) {
    let slot = &HIRED[at];
    let offer = &POOL[slot.kind()];
    // A champion is three mercenaries.
    let count = if CHAMPION[at].load(Relaxed) == 1 {
        3
    } else {
        1
    };
    let refund = offer.price * count * 3 / 4;
    if Action::remove(slot.unit()).is_err() {
        return;
    }
    slot.clear();
    CHAMPION[at].store(0, Relaxed);
    let _ = Action::add_money(House(PLAYER.load(Relaxed)), refund);
    Line::new()
        .text("已遣散：")
        .text(offer.name)
        .text("，退回 ")
        .num(i64::from(refund))
        .log();
}

/// Merges the newly hired unit at `newest` with two of its type.
fn merge(newest: usize) {
    let kind = HIRED[newest].kind();
    let mut others = [0usize; 2];
    let mut found = 0;
    for (at, slot) in HIRED.iter().enumerate() {
        if found < others.len()
            && at != newest
            && !slot.is_empty()
            && slot.kind() == kind
            && CHAMPION[at].load(Relaxed) == 0
        {
            others[found] = at;
            found += 1;
        }
    }
    if found < others.len() {
        return;
    }
    for at in others {
        let _ = Action::remove(HIRED[at].unit());
        HIRED[at].clear();
    }
    let unit = HIRED[newest].unit();
    CHAMPION[newest].store(1, Relaxed);
    let _ = set(unit, Prop::Rank, 2);
    let _ = Modifier::multiply(unit, Prop::FirepowerMultiplier, fixed(2.5), -1);
    let _ = Modifier::multiply(unit, Prop::ArmorMultiplier, fixed(3.0), -1);
    if let Ok(strength) = get(unit, Prop::Strength) {
        let _ = Action::heal(unit, strength as i32);
    }
    Line::new()
        .text("三合一！晋升精英：")
        .text(POOL[kind].name)
        .log();
}

/// With nobody hired and less money than the cheapest price, no order can
/// start the battle any more.
fn is_broke() -> bool {
    HIRED.iter().all(Slot::is_empty) && POOL.iter().all(|offer| credits() < i64::from(offer.price))
}

// ------------------------------------------------------------------ Phases

fn start() {
    let mut trigger = Trigger::on(Event::COMMAND_ALLOWED);
    for offer in &POOL {
        match TypeRef::techno_type(offer.id) {
            Ok(ty) => trigger = trigger.bind(ty),
            Err(_) => return log("佣兵点兵场：规则里缺少佣兵所用的单位类型。"),
        }
    }
    if trigger.decide_sync(ON_COMMAND).is_err()
        || Trigger::on(Event::DEFEAT).decide_sync(ON_DEFEAT).is_err()
    {
        log("佣兵点兵场：触发器创建失败。");
    }
}

/// No house is out while the field is empty between the phases.
fn defeat(mut decision: DefeatDecision) {
    let house = decision.house().0;
    let playing = matches!(PHASE.load(Relaxed), MUSTER | BATTLE);
    if playing && (house == PLAYER.load(Relaxed) || house == ENEMY.load(Relaxed)) {
        decision.set_result(0);
    }
}

fn open_camp() {
    let mut humans = [0i64; 2];
    let found = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 1)
        .page(&mut humans, 0);
    let Ok((1, _)) = found else {
        PHASE.store(OVER, Relaxed);
        return log("佣兵点兵场只支持一名玩家。");
    };
    let player = House(humans[0]);
    let camp = get(Techno(Find::objects().owned_by(player).first()), Prop::Cell);
    let mut houses = [0i64; 16];
    let computers = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .page(&mut houses, 0)
        .map_or(0, |(count, _)| count);
    let mut enemy = House(NO_HANDLE);
    for &house in &houses[..computers] {
        let house = House(house);
        if has_base(house) {
            if enemy.0 == NO_HANDLE {
                enemy = house;
            }
            clear_forces(house);
        }
    }
    let (Ok(camp), false) = (camp, enemy.0 == NO_HANDLE) else {
        PHASE.store(OVER, Relaxed);
        return log("佣兵点兵场需要一个电脑对手，并打开“基地”选项。");
    };
    PLAYER.store(player.0, Relaxed);
    ENEMY.store(enemy.0, Relaxed);
    CAMP.store(camp, Relaxed);
    PHASE.store(MUSTER, Relaxed);
    clear_forces(player);
    let _ = Action::add_money(player, (PURSE - credits()) as i32);
    let _ = Action::reveal_all();
    log("佣兵点兵场：把佣兵移出货位即雇佣，移回货位即遣散。");
    log("选中待雇佣兵按 S 刷新（60），选中已雇佣兵按 X 开战。");
    fill_row();
}

fn start_battle() {
    PHASE.store(BATTLE, Relaxed);
    empty_row();
    let hired = HIRED.iter().filter(|slot| !slot.is_empty()).count();
    if hired == 0 {
        return finish(Outcome::Lose, "没有雇佣任何人，营地失守。");
    }
    let enemy = House(ENEMY.load(Relaxed));
    let front = front();
    let mut raiders = 0;
    for (at, (slot, id)) in RAID.iter().zip(RAIDERS).enumerate() {
        let Ok(ty) = TypeRef::techno_type(id) else {
            continue;
        };
        let spot = offset(front, (at % 3) as i32 - 1, (at / 3) as i32 - 1);
        if let Some(unit) = create_near(spot, ty, enemy) {
            slot.put(unit, 0);
            raiders += 1;
        }
    }
    if raiders == 0 {
        return finish(Outcome::Win, "袭击者找不到进攻营地的路，你赢了！");
    }
    count_down(BATTLE_SECONDS);
    Line::new()
        .text("开战！我方 ")
        .num(hired as i64)
        .text(" 人，敌方 ")
        .num(raiders)
        .text(" 人，战斗将自动进行。")
        .log();
    engage(&HIRED, &RAID);
    engage(&RAID, &HIRED);
}

/// The open ground the raiders arrive on: of the four sides of the camp, the
/// one with the most passable cells on the way.
fn front() -> Cell {
    let camp = Cell(CAMP.load(Relaxed));
    let mut best = (camp, -1);
    for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
        let open = (1..=FIELD + 2)
            .filter(|&step| {
                offset(camp, dx * step, dy * step).passable(SpeedType::Track) == Ok(true)
            })
            .count() as i32;
        if open > best.1 {
            best = (offset(camp, dx * FIELD, dy * FIELD), open);
        }
    }
    best.0
}

/// Sends each unit of `side` that has no living target of `foes` at the
/// nearest one. The order passes `CommandAllowed` unasked: a module's own
/// writes skip its own deciders.
fn engage(side: &[Slot], foes: &[Slot]) {
    for slot in side.iter().filter(|slot| !slot.is_empty()) {
        let unit = slot.unit();
        if let Ok(target) = unit.target() {
            if position(foes, Techno(target)).is_some() {
                continue;
            }
        }
        let Ok(from) = get(unit, Prop::Cell).map(Cell) else {
            continue;
        };
        let mut nearest = None;
        for foe in foes.iter().filter(|foe| !foe.is_empty()) {
            let Ok(to) = get(foe.unit(), Prop::Cell).map(Cell) else {
                continue;
            };
            let dx = i32::from(from.x()) - i32::from(to.x());
            let dy = i32::from(from.y()) - i32::from(to.y());
            let distance = dx * dx + dy * dy;
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, foe.unit()));
            }
        }
        if let Some((_, foe)) = nearest {
            let _ = Action::set_mission_on(unit, Mission::Attack, foe);
        }
    }
}

/// Forgets the dead of `slots` and counts the living.
fn survivors(slots: &[Slot]) -> usize {
    let mut living = 0;
    for slot in slots.iter().filter(|slot| !slot.is_empty()) {
        if slot.unit().health().is_ok_and(|health| health > 0) {
            living += 1;
        } else {
            slot.clear();
        }
    }
    living
}

fn fight() {
    let ours = survivors(&HIRED);
    let theirs = survivors(&RAID);
    if ours == 0 {
        finish(Outcome::Lose, "队伍全军覆没，营地失守。");
    } else if theirs == 0 {
        finish(Outcome::Win, "击退了袭击者，营地守住了！");
    } else if frame() >= DEADLINE.load(Relaxed) {
        finish(Outcome::Lose, "时间耗尽，未能击退袭击者。");
    } else {
        engage(&HIRED, &RAID);
        engage(&RAID, &HIRED);
    }
}

fn finish(outcome: Outcome, text: &str) {
    PHASE.store(OVER, Relaxed);
    let _ = Action::timer_stop();
    log(text);
    let _ = Action::end_for(House(PLAYER.load(Relaxed)), outcome);
}

fn count_down(seconds: i32) {
    DEADLINE.store(frame() + seconds * 15, Relaxed);
    let _ = Action::timer_set(seconds);
    let _ = Action::timer_start();
}

fn tick() {
    let now = frame();
    match PHASE.load(Relaxed) {
        // The starting units are on the map by the second frame.
        WAITING if now >= 2 => open_camp(),
        MUSTER => {
            read_orders();
            if PHASE.load(Relaxed) == MUSTER && is_broke() {
                finish(Outcome::Lose, "资金耗尽，已无人可雇，营地失守。");
            }
        }
        BATTLE if now % 15 == 0 => fight(),
        _ => {}
    }
}

// ----------------------------------------------------------------- Helpers

fn credits() -> i64 {
    get(House(PLAYER.load(Relaxed)), Prop::Money).unwrap_or(0)
}

fn has_base(house: House) -> bool {
    BASE_TYPES.iter().any(|id| {
        TypeRef::techno_type(id)
            .is_ok_and(|ty| Find::objects().owned_by(house).of_type(ty).first() != NO_HANDLE)
    })
}

fn clear_forces(house: House) {
    let mut found = [0i64; 64];
    let mut after = 0;
    loop {
        let Ok((count, next)) = Find::objects().owned_by(house).page(&mut found, after) else {
            return;
        };
        for &object in &found[..count] {
            let _ = Action::remove(Techno(object));
        }
        if next == 0 {
            return;
        }
        after = next;
    }
}

fn offset(cell: Cell, dx: i32, dy: i32) -> Cell {
    let x = (i32::from(cell.x()) + dx).clamp(1, 510) as u16;
    let y = (i32::from(cell.y()) + dy).clamp(1, 510) as u16;
    Cell::at(x, y)
}

/// Creates a unit on `spot` or the closest ring of cells that takes it.
fn create_near(spot: Cell, ty: TypeRef, house: House) -> Option<Techno> {
    for ring in 0..=4i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                if let Ok(unit) = Action::create_object(offset(spot, dx, dy), ty, house) {
                    return Some(unit);
                }
            }
        }
    }
    None
}

// -------------------------------------------------------------------- Text

/// A log line built without an allocator (`core::fmt` would pull in code
/// the engine refuses).
struct Line {
    buf: [u8; 240],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 240],
            len: 0,
        }
    }

    fn text(mut self, text: &str) -> Self {
        // Cut between characters: a line that is not UTF-8 is not shown.
        let mut end = text.len().min(self.buf.len() - self.len);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        for &byte in &text.as_bytes()[..end] {
            self.buf[self.len] = byte;
            self.len += 1;
        }
        self
    }

    fn num(self, value: i64) -> Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let line = if value < 0 { self.text("-") } else { self };
        line.text(core::str::from_utf8(&digits[at..]).unwrap_or("?"))
    }

    fn log(&self) {
        if let Ok(text) = core::str::from_utf8(&self.buf[..self.len]) {
            log(text);
        }
    }
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
vera20k_sdk::on_decide!(ON_COMMAND => command, ON_DEFEAT => defeat);
