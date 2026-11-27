//! "Muster Field", a playtest `rules.wasm` (RFC 0001 preview): a small
//! auto-battler against one computer house, played with ordinary unit orders
//! or with its `ui.wasm`'s panels. Its play follows the 红警酒馆 of MK,
//! a Bilibili creator (README.md); it is a demonstration, not to be
//! published as a mod of its own.
//!
//! - A campaign of waves: between battles the company stands in camp, the
//!   dead return, and the player spends the pay.
//! - Hire by ordering a mercenary out of the row, dismiss by ordering one
//!   back into it. Stop (S) redraws the row, Guard (G) keeps an offer.
//! - The quartermaster: Guard improves the camp, Scatter (X) starts the
//!   battle, as Scatter on any hired unit does.
//! - Three of a type merge into a champion. Merges and won waves bring
//!   supplies: order one of three trucks away to pick one for the company.
//!
//! What it shows of the API: `CommandAllowed` as player input (the handler
//! may not write, so it queues the order in module memory and `on_frame`
//! carries it out), `SetMission` for a human house's units (`House::order`
//! takes computer houses only), modifiers, searches, and a `Defeat` veto
//! while both houses have nothing on the field. Nothing waits on frames for
//! the player: game speed 0 is unlimited.
//!
//! It is made for one human player against computer opponents, with Bases on.
//! What it tells the player is in Simplified Chinese.
//! Build it with `cargo build --release`, rename `muster_field.wasm` to
//! `rules.wasm` and `muster_field_ui.wasm` to `ui.wasm`, and put both in a
//! mod folder (docs/wasm-modules/guide.md).
//!
//! The interface (`ui/`) shows the camp and has a button for every key:
//! the simulation publishes the camp as variables and turns each button's
//! message into the order that key gives (`ui`). The orders keep working
//! without it.
//!
//! `lib` owns input and the phases, `shop` the camp, `battle` one fight with
//! `waves`, `campaign` the pay and the end, `content` and `perks` what can be
//! hired and won, `state` what they share, `ui` the interface's side; the
//! tables live in `data/`, which `ui.wasm` links too.
#![no_std]

mod battle;
mod campaign;
mod perks;
mod shop;
mod state;
mod text;
mod ui;
mod world;

use muster_field_data::{content, waves};

use state::{BATTLE, MUSTER, Num, OVER, PHASE, WAITING};
use text::Say;
use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

const ON_COMMAND: i32 = 1;
const ON_DEFEAT: i32 = 2;
/// What a computer opponent starts with or deploys into.
const BASE_TYPES: [&str; 6] = ["AMCV", "SMCV", "PCV", "GACNST", "NACNST", "YACNST"];

const INBOX: usize = 32;
static INBOX_UNIT: [Num; INBOX] = [const { Num::new(NO_HANDLE) }; INBOX];
static INBOX_CELL: [Num; INBOX] = [const { Num::new(NO_HANDLE) }; INBOX];
static INBOX_COMMAND: [Num; INBOX] = [const { Num::new(0) }; INBOX];
static INBOX_LEN: Num = Num::new(0);

/// Runs as the engine executes a player's order. It may only read the game.
fn command(mut decision: CommandAllowedDecision) {
    let order = shop::Order {
        unit: decision.unit(),
        command: decision.command(),
        cell: decision.target_cell(),
    };
    let phase = PHASE.get();
    if phase == WAITING || !shop::owns(order.unit) {
        return;
    }
    let verdict = if phase == MUSTER {
        shop::judge(order)
    } else {
        shop::Verdict::Refuse
    };
    if matches!(verdict, shop::Verdict::Allow) {
        return;
    }
    decision.set_result(0);
    if matches!(verdict, shop::Verdict::Queue) {
        queue(order);
    }
}

/// Hands `order` to the next [`read_orders`]: a refused player order, or
/// an interface button's (`ui`).
fn queue(order: shop::Order) {
    let len = INBOX_LEN.get() as usize;
    if len < INBOX {
        INBOX_UNIT[len].set(order.unit.0);
        INBOX_COMMAND[len].set(i64::from(order.command));
        INBOX_CELL[len].set(order.cell.0);
        INBOX_LEN.set(len as i64 + 1);
    }
}

fn read_orders() -> bool {
    let len = (INBOX_LEN.get() as usize).min(INBOX);
    if len == 0 {
        return false;
    }
    INBOX_LEN.set(0);
    let mut orders = [shop::Order {
        unit: Techno(NO_HANDLE),
        command: 0,
        cell: Cell(NO_HANDLE),
    }; INBOX];
    for (at, order) in orders.iter_mut().take(len).enumerate() {
        order.unit = Techno(INBOX_UNIT[at].get());
        order.command = INBOX_COMMAND[at].get() as i32;
        order.cell = Cell(INBOX_CELL[at].get());
    }
    shop::apply(&orders[..len])
}

/// No house is out while the field is empty between the phases.
fn defeat(mut decision: DefeatDecision) {
    let house = decision.house();
    let playing = matches!(PHASE.get(), MUSTER | BATTLE);
    if playing && (house == state::player() || house == state::enemy()) {
        decision.set_result(0);
    }
}

fn start() {
    let Ok(trigger) = Trigger::on(Event::COMMAND_ALLOWED).decide_sync(ON_COMMAND) else {
        return log("佣兵点兵场：触发器创建失败。");
    };
    let camp_types = shop::CRATES.iter().copied().chain([shop::OFFICER]);
    for id in content::MERCS.iter().map(|merc| merc.id).chain(camp_types) {
        match TypeRef::techno_type(id) {
            Ok(ty) if trigger.bind(ty).is_ok() => {}
            _ => text::Line::new()
                .text("佣兵点兵场：规则里缺少单位 ")
                .text(id)
                .log(),
        }
    }
    if Trigger::on(Event::DEFEAT).decide_sync(ON_DEFEAT).is_err() {
        log("佣兵点兵场：触发器创建失败。");
    }
}

fn has_base(house: House) -> bool {
    BASE_TYPES.iter().any(|id| {
        TypeRef::techno_type(id)
            .is_ok_and(|ty| Find::objects().owned_by(house).of_type(ty).first() != NO_HANDLE)
    })
}

fn open_camp() {
    let mut humans = [0i64; 2];
    let found = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 1)
        .page(&mut humans, 0);
    let Ok((1, _)) = found else {
        PHASE.set(OVER);
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
            world::clear_forces(house);
        }
    }
    let (Ok(camp), false) = (camp, enemy.0 == NO_HANDLE) else {
        PHASE.set(OVER);
        return log("佣兵点兵场需要一个电脑对手，并打开“基地”选项。");
    };
    state::set_sides(player, enemy, Cell(camp));
    world::clear_forces(player);
    let _ = Action::reveal_all();
    campaign::begin();
    muster();
}

fn muster() {
    PHASE.set(MUSTER);
    campaign::announce();
    shop::open();
}

fn start_battle() {
    INBOX_LEN.set(0);
    shop::close();
    PHASE.set(BATTLE);
    battle::start();
}

fn after_battle(report: &battle::Report) {
    match campaign::settle(report) {
        campaign::Next::Muster => muster(),
        campaign::Next::Victory => finish(Outcome::Win, "八波袭击全部击退，佣兵团名扬天下！"),
        campaign::Next::Defeat => finish(Outcome::Lose, "最后一面军旗倒下，营地失守。"),
    }
}

fn finish(outcome: Outcome, text: &str) {
    PHASE.set(OVER);
    log(text);
    let _ = Action::end_for(state::player(), outcome);
}

fn tick() {
    let now = frame();
    match PHASE.get() {
        // The starting units are on the map by the second frame.
        WAITING if now >= 2 => open_camp(),
        MUSTER => {
            if read_orders() {
                start_battle();
            }
        }
        BATTLE => {
            perks::battle_tick(now);
            if let Some(report) = battle::tick(now) {
                after_battle(&report);
            }
        }
        _ => {}
    }
    ui::publish();
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
vera20k_sdk::on_decide!(ON_COMMAND => command, ON_DEFEAT => defeat);
