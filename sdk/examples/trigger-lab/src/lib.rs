//! "Trigger Lab", a playtest `rules.wasm` (RFC 0001 preview): a tour of the
//! trigger system for the human player, each trigger announcing itself in
//! the message list. It covers what the other playtest mods leave out:
//!
//! - `Birth` bound to types, with a subject clause on `Owner`, an immediate
//!   action (`ActionDef::log`, run by the engine as the trigger fires) and a
//!   module action that counts in a house variable (`HouseVar`).
//! - `Fire` bound to a weapon (the GI's `M60`), `once()`: only the first
//!   shot of the match reports.
//! - `PassengerAdded` bound to transports: who boarded and the space left.
//! - `PropertyPoll` on the house's `Money` (`Trigger::on_property` with
//!   `house`): fires each time the money rises to 5000.
//! - `HousePowerPoll`: warns when the house's power runs short.
//! - `VariablePoll` on Red Alert 2 local variable 10, which the money
//!   trigger sets: one trigger driving another. It also disables the money
//!   trigger for a minute (`TriggerRef::set_enabled`).
//! - `DistancePoll` bound to one object, the starting MCV: reports once it
//!   is 6 cells from where it started.
//! - Every minute an army report reads every unit's health in one call
//!   (`get_many`).
#![no_std]

use core::sync::atomic::{AtomicI64, Ordering};

use vera20k_sdk::prelude::*;
use vera20k_sdk::NO_HANDLE;
use vera20k_sdk::triggers::{BirthEvent, PassengerAddedEvent, PollEvent};

vera20k_sdk::named_actions! {
    ON_BIRTH = "OnBirth" => 1,
    ON_FIRST_SHOT = "OnFirstShot" => 2,
    ON_BOARD = "OnBoard" => 3,
    ON_RICH = "OnRich" => 4,
    ON_FLAG = "OnFlag" => 5,
    ON_MCV_MOVED = "OnMcvMoved" => 6,
}

vera20k_sdk::vars! {
    static BUILT: HouseVar = ("Built", Public, 0);
}

const TANKS: [&str; 8] = ["MTNK", "HTNK", "FV", "APOC", "SREF", "TNKD", "LTNK", "YTNK"];
const TRANSPORTS: [&str; 4] = ["FV", "BFRT", "LCRF", "SAPC"];
const MCVS: [&str; 3] = ["AMCV", "SMCV", "PCV"];
/// Red Alert 2 local variable 10: set when the player gets rich.
const FLAG: Var = Var::local(10);
const RICH: i64 = 5000;

/// The human player, the money trigger and the frame it was paused.
static HUMAN: AtomicI64 = AtomicI64::new(0);
static RICH_TRIGGER: AtomicI64 = AtomicI64::new(0);
static PAUSED_AT: AtomicI64 = AtomicI64::new(-1);

// ------------------------------------------------------------------ Start

fn start() {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        log("Trigger Lab: no human player");
        return;
    }
    HUMAN.store(human, Ordering::Relaxed);
    let human = House(human);
    log("Trigger Lab: build tanks, fire a GI, fill a transport, save 5000, move your MCV...");

    // Birth: an immediate action and a module action on one trigger.
    let birth = Trigger::on(Event::BIRTH)
        .where_prop(Prop::Owner, Op::Eq, human.0)
        .immediate(ActionDef::log("Trigger Lab: [Birth] a tank rolled out (immediate action)"))
        .enqueue(ON_BIRTH);
    report("Birth", bind_all(birth, &TANKS));

    // Fire bound to a weapon, once.
    match TypeRef::weapon("M60") {
        Ok(m60) => report(
            "Fire",
            Trigger::on(Event::FIRE).bind(m60).once().enqueue(ON_FIRST_SHOT).done(),
        ),
        Err(_) => log("Trigger Lab: weapon M60 not found"),
    }

    // PassengerAdded on transports.
    report(
        "PassengerAdded",
        bind_all(Trigger::on(Event::PASSENGER_ADDED).enqueue(ON_BOARD), &TRANSPORTS),
    );

    // PropertyPoll on the house's money, checked every 2 seconds.
    match Trigger::on_property(Prop::Money, Op::Ge, RICH, 30)
        .house(human)
        .enqueue(ON_RICH)
        .done()
    {
        Ok(trigger) => RICH_TRIGGER.store(trigger.0, Ordering::Relaxed),
        Err(_) => log("Trigger Lab: the PropertyPoll trigger was refused"),
    }

    // HousePowerPoll with only an immediate action.
    report(
        "HousePowerPoll",
        Trigger::on_house_power(Op::Lt, 0, 30)
            .house(human)
            .immediate(ActionDef::log("Trigger Lab: [HousePowerPoll] low power! Build a power plant."))
            .done(),
    );

    // VariablePoll on local 10.
    report(
        "VariablePoll",
        Trigger::on_variable(FLAG, Op::Eq, 1, 15)
            .house(human)
            .enqueue(ON_FLAG)
            .done(),
    );

    // DistancePoll bound to one object: the starting MCV.
    if let Some(mcv) = starting_mcv(human) {
        if let Ok(cell) = vera20k_sdk::get(mcv, Prop::Cell).map(Cell) {
            report(
                "DistancePoll",
                Trigger::on_distance(cell, Op::Gt, 6 * 256, 15)
                    .bind(mcv)
                    .once()
                    .enqueue(ON_MCV_MOVED)
                    .done(),
            );
        }
    }
}

/// Makes `trigger` and binds it to each of `names` the rules know.
fn bind_all(trigger: Trigger, names: &[&str]) -> vera20k_sdk::Result<TriggerRef> {
    let made = trigger.done()?;
    for name in names {
        if let Ok(ty) = TypeRef::techno_type(name) {
            let _ = made.bind(ty);
        }
    }
    Ok(made)
}

fn report(what: &str, result: vera20k_sdk::Result<TriggerRef>) {
    if result.is_err() {
        Line::new()
            .text("Trigger Lab: the ")
            .text(what)
            .text(" trigger was refused")
            .log();
    }
}

fn starting_mcv(human: House) -> Option<Techno> {
    MCVS.iter().find_map(|name| {
        let ty = TypeRef::techno_type(name).ok()?;
        let mcv = Find::objects().owned_by(human).of_type(ty).first();
        (mcv != NO_HANDLE).then_some(Techno(mcv))
    })
}

fn human() -> House {
    House(HUMAN.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------- Handlers

fn on_birth(_event: BirthEvent) {
    let house = human();
    let Ok(built) = BUILT.get(house).map(|built| built + 1) else {
        return;
    };
    if BUILT.set(house, built).is_ok() {
        Line::new()
            .text("Trigger Lab: [Birth] tanks built so far (house variable): ")
            .num(built)
            .log();
    }
}

fn on_first_shot(event: FireEvent) {
    let target = event.target_cell();
    Line::new()
        .text("Trigger Lab: [Fire, once] the first GI shot of the match, at cell ")
        .num(i64::from(target.x()))
        .text(",")
        .num(i64::from(target.y()))
        .log();
}

fn on_board(event: PassengerAddedEvent) {
    Line::new()
        .text("Trigger Lab: [PassengerAdded] a soldier boarded; space left: ")
        .num(event.space())
        .log();
}

fn on_rich(event: PollEvent) {
    Line::new()
        .text("Trigger Lab: [PropertyPoll] money reached ")
        .num(event.reading())
        .text(" - setting local 10")
        .log();
    let _ = FLAG.set(1);
}

fn on_flag(_event: PollEvent) {
    log("Trigger Lab: [VariablePoll] local 10 is set; pausing the money trigger for a minute");
    let _ = FLAG.set(0);
    let rich = TriggerRef(RICH_TRIGGER.load(Ordering::Relaxed));
    if rich.set_enabled(false).is_ok() {
        PAUSED_AT.store(i64::from(frame()), Ordering::Relaxed);
    }
}

fn on_mcv_moved(_event: PollEvent) {
    log("Trigger Lab: [DistancePoll, once] your MCV is 6 cells from where it started");
}

// ----------------------------------------------------------- Every second

fn tick() {
    let now = i64::from(frame());
    let paused = PAUSED_AT.load(Ordering::Relaxed);
    if paused >= 0 && now - paused >= 900 {
        let rich = TriggerRef(RICH_TRIGGER.load(Ordering::Relaxed));
        if rich.set_enabled(true).is_ok() {
            log("Trigger Lab: the money trigger is enabled again");
        }
        PAUSED_AT.store(-1, Ordering::Relaxed);
    }
    if now > 0 && now % 900 == 0 {
        army_report();
    }
}

/// The human player's units and their average health, read in one call.
fn army_report() {
    let house = human();
    let mut units = [0i64; 64];
    let Ok((count, _)) = Find::objects()
        .owned_by(house)
        .where_prop(Prop::Category, Op::Ne, Category::Building as i64)
        .page(&mut units, 0)
    else {
        return;
    };
    if count == 0 {
        return;
    }
    let props = [Prop::HealthPercent.0];
    let mut health = [0i64; 64];
    if vera20k_sdk::get_many(&units[..count], &props, &mut health[..count]).is_err() {
        return;
    }
    let total: i64 = health[..count].iter().sum();
    Line::new()
        .text("Trigger Lab: [get_many] army report - ")
        .num(count as i64)
        .text(if count == units.len() { "+ units" } else { " units" })
        .text(", average health ")
        .num(total / count as i64)
        .text("%")
        .log();
}

// ------------------------------------------------------------------- Text

/// A log line built without an allocator (`core::fmt` would pull in code
/// the engine refuses).
struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self { buf: [0; 160], len: 0 }
    }

    fn text(mut self, text: &str) -> Self {
        for &byte in text.as_bytes() {
            if self.len == self.buf.len() {
                break;
            }
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
vera20k_sdk::on_frame!(every = 15, tick);
vera20k_sdk::on_events!(
    ON_BIRTH => on_birth,
    ON_FIRST_SHOT => on_first_shot,
    ON_BOARD => on_board,
    ON_RICH => on_rich,
    ON_FLAG => on_flag,
    ON_MCV_MOVED => on_mcv_moved,
);
