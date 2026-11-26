//! "Veteran's Fury", a playtest `rules.wasm` (RFC 0001 preview) that
//! combines several module features in one skirmish:
//!
//! - Bounty: a human player's unit that kills a common infantry or tank
//!   earns its house 150 credits (a `Killed` trigger bound to types).
//! - Veterans: the third kill of a unit promotes it: firepower x1.5 and
//!   armor x1.25 for good, fully repaired (an object variable counts kills,
//!   modifiers apply the boost).
//! - Last stand: a human player's tank that drops below 30% health gets
//!   armor x3 for 6 s and 50% more speed for 10 s, once per tank (a
//!   `TakeDamage` trigger with property and house clauses).
//! - Waves: every minute each computer opponent gets a few tanks beside
//!   its base and sends them at the first human player's base with an
//!   attack-move order; each wave is larger (a global variable counts).
//! - Interest: every minute each human player earns 5% of their money, at
//!   most 500 credits.
//!
//! Build it with `cargo build --release`, rename the `.wasm` to
//! `rules.wasm` and put it in a mod folder (docs/wasm-modules/guide.md).
#![no_std]

use vera20k_sdk::prelude::*;
use vera20k_sdk::NO_HANDLE;

vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1, ON_LAST_STAND = "OnLastStand" => 2 }

vera20k_sdk::vars! {
    static WAVE: GlobalVar = ("Wave", Public, 0);
    static KILLS: ObjectVar = ("Kills", Public, 0);
    static RALLIED: ObjectVar = ("Rallied", Private, 0);
}

const BOUNTY: i32 = 150;
const PROMOTE_AT: i64 = 3;
/// Kills that pay a bounty: the common infantry and tanks of both sides.
const PREY: [&str; 12] = [
    "E1", "E2", "GGI", "FLAKT", "SHK", "INIT", "MTNK", "HTNK", "FV", "HTK", "APOC", "LTNK",
];
/// Tanks that make a last stand.
const TANKS: [&str; 6] = ["MTNK", "HTNK", "FV", "APOC", "TNKD", "LTNK"];
/// Frames between waves (15 frames a second).
const WAVE_EVERY: i32 = 900;

// ------------------------------------------------------------------ Start

fn start() {
    log("Veteran's Fury: +150 per kill; 3 kills make a veteran (x1.5 firepower).");
    log("Tanks below 30% make a last stand; enemy waves every minute; 5% interest a minute.");
    match Trigger::on(Event::KILLED).enqueue(ON_KILL).done() {
        Ok(trigger) => bind_types(trigger, &PREY),
        Err(_) => log("Veteran's Fury: the bounty trigger was refused"),
    }
    match Trigger::on(Event::TAKE_DAMAGE)
        .where_prop(Prop::HealthPercent, Op::Lt, 30)
        .where_house(Prop::IsHuman, Op::Eq, 1)
        .enqueue(ON_LAST_STAND)
        .done()
    {
        Ok(trigger) => bind_types(trigger, &TANKS),
        Err(_) => log("Veteran's Fury: the last-stand trigger was refused"),
    }
}

/// Binds `trigger` to each of `names` the rules know.
fn bind_types(trigger: TriggerRef, names: &[&str]) {
    for name in names {
        if let Ok(ty) = TypeRef::techno_type(name) {
            let _ = trigger.bind(ty);
        }
    }
}

// ----------------------------------------------------- Bounty and veterans

fn on_kill(event: KilledEvent) {
    let killer = event.killer();
    let Ok(house) = killer.owner() else {
        return; // No killer, or it died in the same frame.
    };
    if vera20k_sdk::get(house, Prop::IsHuman) != Ok(1) {
        return;
    }
    let _ = Action::add_money(house, BOUNTY);
    let Ok(kills) = KILLS.get(killer).map(|kills| kills + 1) else {
        return;
    };
    if KILLS.set(killer, kills).is_err() {
        return;
    }
    if kills == PROMOTE_AT {
        let _ = Modifier::multiply(killer, Prop::FirepowerMultiplier, fixed(1.5), -1);
        let _ = Modifier::multiply(killer, Prop::ArmorMultiplier, fixed(1.25), -1);
        if let Ok(strength) = vera20k_sdk::get(killer, Prop::Strength) {
            let _ = Action::heal(killer, strength as i32);
        }
        log("Veteran's Fury: a unit made its 3rd kill and became a veteran!");
    } else {
        Line::new()
            .text("Veteran's Fury: +150 bounty (that unit's kills: ")
            .num(kills)
            .text(")")
            .log();
    }
}

// -------------------------------------------------------------- Last stand

fn on_last_stand(event: TakeDamageEvent) {
    let tank = event.victim();
    // Gone when the hit killed it; 1 once it has made its stand.
    if RALLIED.get(tank) != Ok(0) || RALLIED.set(tank, 1).is_err() {
        return;
    }
    // Armor x3 rather than `Unkillable`, which needs a Phobos build.
    let _ = Modifier::multiply(tank, Prop::ArmorMultiplier, fixed(3.0), 90);
    let _ = Modifier::multiply(tank, Prop::SpeedMultiplier, fixed(1.5), 150);
    log("Veteran's Fury: last stand! A tank is armor x3 for 6 s, 50% faster for 10 s.");
}

// ------------------------------------------------------------------- Waves

fn waves() {
    if frame() < WAVE_EVERY {
        return;
    }
    let wave = WAVE.get().unwrap_or(0) + 1;
    let _ = WAVE.set(wave);
    let Some(target) = human_base() else {
        log("Veteran's Fury: wave skipped - no human building found");
        return;
    };
    let size = (1 + wave).min(6) as usize;
    let tank = if wave % 2 == 1 { "MTNK" } else { "HTNK" };
    let Ok(kind) = TypeRef::techno_type(tank) else {
        log("Veteran's Fury: wave skipped - tank type not found");
        return;
    };
    let mut houses = [0i64; 16];
    let Ok((count, _)) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .where_prop(Prop::IsDefeated, Op::Eq, 0)
        .page(&mut houses, 0)
    else {
        log("Veteran's Fury: wave skipped - house search failed");
        return;
    };
    // Computer opponents are the computer houses with a construction yard;
    // that leaves out the neutral and civilian houses (the engine does not
    // answer `IsCivilian` yet).
    let mut opponents = 0;
    let mut sent = 0;
    for &house in &houses[..count] {
        let house = House(house);
        if let Some(base) = yard_cell(house) {
            opponents += 1;
            sent += send_wave(house, base, kind, size, target);
        }
    }
    if opponents == 0 {
        log("Veteran's Fury: wave skipped - no computer opponent has a construction yard yet");
    }
    if sent > 0 {
        Line::new()
            .text("Veteran's Fury: wave ")
            .num(wave)
            .text(" - ")
            .num(sent as i64)
            .text(" enemy tanks are heading for your base!")
            .log();
    }
}

/// Creates up to `size` tanks of `kind` around `base` for `house` and
/// orders them to attack-move to `target`; returns how many it made.
fn send_wave(house: House, base: Cell, kind: TypeRef, size: usize, target: Cell) -> usize {
    let mut refused = None;
    let mut tanks = [Techno(NO_HANDLE); 6];
    let mut made = 0;
    // A ring of cells around the base; occupied ones refuse the object.
    for dy in -4i32..=4 {
        for dx in -4i32..=4 {
            if made == size {
                break;
            }
            if dx.abs() < 3 && dy.abs() < 3 {
                continue; // Inside the building's footprint, probably.
            }
            let x = (i32::from(base.x()) + dx).clamp(1, 500) as u16;
            let y = (i32::from(base.y()) + dy).clamp(1, 500) as u16;
            match Action::create_object(Cell::at(x, y), kind, house) {
                Ok(tank) => {
                    tanks[made] = tank;
                    made += 1;
                }
                Err(status) => refused = Some(status),
            }
        }
    }
    if made == 0 {
        let mut line = Line::new().text("Veteran's Fury: no tank could be placed near (")
            .num(i64::from(base.x())).text(",").num(i64::from(base.y())).text("): ");
        line = line.text(refused.map_or("no cell tried", status_name));
        line.log();
    } else if let Err(status) = house.order(&Order::attack_move(&tanks[..made], target)) {
        Line::new()
            .text("Veteran's Fury: attack order refused: ")
            .text(status_name(status))
            .log();
    }
    made
}

/// The cell of the first human player's construction yard, or else of
/// their first building.
fn human_base() -> Option<Cell> {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        return None;
    }
    let human = House(human);
    yard_cell(human).or_else(|| {
        let building = Find::objects()
            .owned_by(human)
            .where_prop(Prop::Category, Op::Eq, Category::Building as i64)
            .first();
        cell_of(building)
    })
}

/// Construction yards of the three sides.
const YARDS: [&str; 3] = ["GACNST", "NACNST", "YACNST"];

/// The cell of `house`'s first construction yard.
fn yard_cell(house: House) -> Option<Cell> {
    YARDS.iter().find_map(|name| {
        let ty = TypeRef::techno_type(name).ok()?;
        cell_of(Find::objects().owned_by(house).of_type(ty).first())
    })
}

fn cell_of(object: i64) -> Option<Cell> {
    if object == NO_HANDLE {
        return None;
    }
    vera20k_sdk::get(Techno(object), Prop::Cell).ok().map(Cell)
}

// ---------------------------------------------------------------- Interest

fn interest() {
    if frame() == 0 {
        return;
    }
    let mut houses = [0i64; 16];
    let Ok((count, _)) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 1)
        .page(&mut houses, 0)
    else {
        return;
    };
    for &house in &houses[..count] {
        let house = House(house);
        let Ok(money) = vera20k_sdk::get(house, Prop::Money) else {
            continue;
        };
        let paid = (money / 20).min(500);
        if paid > 0 && Action::add_money(house, paid as i32).is_ok() {
            Line::new()
                .text("Veteran's Fury: interest +")
                .num(paid)
                .log();
        }
    }
}

// ------------------------------------------------------------------- Text

/// A log line built without an allocator.
struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 160],
            len: 0,
        }
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

/// A status's name; `core::fmt` would pull in code the engine refuses.
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Gone => "Gone",
        Status::NotFound => "NotFound",
        Status::Refused => "Refused",
        Status::Unsupported => "Unsupported",
    }
}

vera20k_sdk::on_scenario_start!(start);
/// One `on_frame` export per module: every 900 frames it pays interest
/// and sends a wave.
fn tick() {
    interest();
    if frame() % WAVE_EVERY == 0 {
        waves();
    }
}

vera20k_sdk::on_frame!(every = 900, tick);
vera20k_sdk::on_events!(ON_KILL => on_kill, ON_LAST_STAND => on_last_stand);
