//! "Siege Doctrine", a playtest `rules.wasm` (RFC 0001 preview) about the
//! game's rules and its decisions. It covers what the other playtest mods
//! leave out:
//!
//! - `on_rules`: reads and rewrites rules while they load. Base defenses
//!   (pillbox, sentry gun, prism tower, tesla coil, flak cannon) get twice
//!   their `Strength`.
//! - `ProductionAllowed`: heavy tanks (Apocalypse, prism tank, tank
//!   destroyer) leave the sidebar for everyone until minute 5.
//! - `ProductionQueued`: superweapon buildings cannot be queued until
//!   minute 10.
//! - `CommandAllowed`: for the first 3 minutes engineers refuse any order
//!   aimed at an object, so there is no early engineer rush.
//! - `Defeat`: a human player who has lost everything is not defeated while
//!   they still hold 3000 credits; the veto holds until the money drops.
//!
//! Decision handlers run synchronously while the engine decides and may
//! only read; the announcements come from `on_frame` instead.
#![no_std]

use core::sync::atomic::{AtomicI32, Ordering};

use vera20k_sdk::prelude::*;
use vera20k_sdk::NO_HANDLE;

const HEAVY_TANKS: i32 = 1;
const SUPERWEAPONS: i32 = 2;
const ENGINEERS: i32 = 3;
const LAST_CHANCE: i32 = 4;

const MINUTE: i32 = 900;
const HEAVY_UNLOCK: i32 = 5 * MINUTE;
const SUPERWEAPON_UNLOCK: i32 = 10 * MINUTE;
const ENGINEER_UNLOCK: i32 = 3 * MINUTE;
const LAST_CHANCE_CREDITS: i64 = 3000;

const DEFENSES: [&str; 5] = ["GAPILL", "NALASR", "ATESLA", "TESLA", "NAFLAK"];
const HEAVY: [&str; 3] = ["APOC", "SREF", "TNKD"];
const SUPERWEAPON_BUILDINGS: [&str; 6] =
    ["GAWEAT", "NAMISL", "GACSPH", "NAIRON", "YAPPET", "YAGNTC"];
const ENGINEER_TYPES: [&str; 3] = ["ENGINEER", "SENGINEER", "YENGINEER"];

// ------------------------------------------------------------------ Rules

/// Doubles each defense's `Strength` while the rules load.
fn rules() {
    let mut doubled = 0;
    for section in DEFENSES {
        let mut buf = [0u8; 16];
        let Some(strength) = Rules::get(section, "Strength", &mut buf)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
        else {
            continue;
        };
        let mut text = [0u8; 12];
        if Rules::set(section, "Strength", format(strength * 2, &mut text)).is_ok() {
            doubled += 1;
        }
    }
    if doubled == 0 {
        log("Siege Doctrine: no defense found to strengthen");
    }
}

// ------------------------------------------------------------------ Start

fn start() {
    log("Siege Doctrine: base defenses have double strength.");
    log("Engineers can't capture before 3:00, heavy tanks unlock at 5:00, superweapons at 10:00.");
    decide(Event::PRODUCTION_ALLOWED, &HEAVY, HEAVY_TANKS, "heavy tanks");
    decide(Event::PRODUCTION_QUEUED, &SUPERWEAPON_BUILDINGS, SUPERWEAPONS, "superweapons");
    decide(Event::COMMAND_ALLOWED, &ENGINEER_TYPES, ENGINEERS, "engineers");
    if Trigger::on(Event::DEFEAT).decide_sync(LAST_CHANCE).is_err() {
        log("Siege Doctrine: the defeat trigger was refused");
    }
}

/// A decision trigger on `event` bound to each type of `names`; a trigger
/// holds 8 bindings, more are bound after it is made.
fn decide(event: i32, names: &[&str], key: i32, what: &str) {
    let mut trigger = Trigger::on(event);
    let mut late = [TypeRef(NO_HANDLE); 8];
    let mut late_len = 0;
    for (i, name) in names.iter().enumerate() {
        let Ok(ty) = TypeRef::techno_type(name) else {
            continue;
        };
        if i < 8 {
            trigger = trigger.bind(ty);
        } else if late_len < late.len() {
            late[late_len] = ty;
            late_len += 1;
        }
    }
    match trigger.decide_sync(key) {
        Ok(made) => {
            for ty in &late[..late_len] {
                let _ = made.bind(*ty);
            }
        }
        Err(_) => {
            let mut line = Line::new().text("Siege Doctrine: the trigger for ");
            line = line.text(what).text(" was refused");
            line.log();
        }
    }
}

// --------------------------------------------------------------- Decisions

fn heavy_tanks(mut decision: ProductionAllowedDecision) {
    if frame() < HEAVY_UNLOCK {
        decision.set_result(0);
    }
}

fn superweapons(mut decision: ProductionQueuedDecision) {
    if frame() < SUPERWEAPON_UNLOCK {
        decision.set_result(0);
    }
}

fn engineers(mut decision: CommandAllowedDecision) {
    if frame() < ENGINEER_UNLOCK && !decision.target().is_none() {
        decision.set_result(0);
    }
}

fn last_chance(mut decision: DefeatDecision) {
    let house = decision.house();
    let human = vera20k_sdk::get(house, Prop::IsHuman) == Ok(1);
    let money = vera20k_sdk::get(house, Prop::Money).unwrap_or(0);
    if human && money >= LAST_CHANCE_CREDITS {
        decision.set_result(0);
    }
}

// ----------------------------------------------------------- Announcements

/// The last unlock announced: 0 none, then 1, 2 and 3.
static ANNOUNCED: AtomicI32 = AtomicI32::new(0);

fn tick() {
    let now = frame();
    let announced = ANNOUNCED.load(Ordering::Relaxed);
    let (next, at, text) = match announced {
        0 => (1, ENGINEER_UNLOCK, "Siege Doctrine: 3:00 - engineers may capture buildings now."),
        1 => (2, HEAVY_UNLOCK, "Siege Doctrine: 5:00 - heavy tanks are on the sidebar."),
        2 => (3, SUPERWEAPON_UNLOCK, "Siege Doctrine: 10:00 - superweapons can be built."),
        _ => return,
    };
    if now >= at {
        log(text);
        ANNOUNCED.store(next, Ordering::Relaxed);
    }
}

// ------------------------------------------------------------------- Text

/// `value` in decimal, written into `buf`.
fn format(value: i32, buf: &mut [u8; 12]) -> &str {
    let mut at = buf.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        buf[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

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

    fn log(&self) {
        if let Ok(text) = core::str::from_utf8(&self.buf[..self.len]) {
            log(text);
        }
    }
}

vera20k_sdk::on_rules!(rules);
vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(every = 15, tick);
vera20k_sdk::on_decide!(
    HEAVY_TANKS => heavy_tanks,
    SUPERWEAPONS => superweapons,
    ENGINEERS => engineers,
    LAST_CHANCE => last_chance,
);
