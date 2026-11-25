//! "Meteor Storm", a playtest `rules.wasm` (RFC 0001 preview) about the
//! map itself: cells, timers, randomness and the stock trigger actions. It
//! covers what the other playtest mods leave out:
//!
//! - A `Timer` trigger (`Trigger::every`) delivers a record every 45
//!   seconds; the module answers it in `on_events`.
//! - A cell search (`Find::cells`) with a property clause finds ore cells,
//!   starting at a random cell index (`random`, the module's own
//!   deterministic stream, the same on every machine).
//! - Cell properties (`Cell::ore`, `Cell::height`) and an object search
//!   around a cell (`Find::near`).
//! - Actions on cells: `spawn_anim` and `detonate_warhead`.
//! - Stock trigger actions: the mission timer counts down to the next storm
//!   (`timer_set`, `timer_start`, read back with `Global::mission_timer`),
//!   and every fourth storm starts a lightning storm (`ion_storm_start`,
//!   `Global::ion_storm_active`).
//! - Stock Red Alert 2 global variable 20 counts the storms (`Var::global`).
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::named_actions! { ON_STORM = "OnStorm" => 1 }

/// Seconds between storms.
const STORM_SECONDS: i32 = 45;
/// Meteors per storm.
const METEORS: usize = 3;
/// Damage at the impact cell.
const DAMAGE: i32 = 120;
/// Red Alert 2 global variable that counts the storms.
const STORMS: Var = Var::global(20);
/// Explosion animations to try, in order.
const ANIMS: [&str; 3] = ["TWLT070", "S_BANG48", "FIRE01"];

fn start() {
    log("Meteor Storm: every 45 s meteors strike the ore fields; the timer counts down.");
    log("Every 4th storm brings lightning. Keep your harvesters moving!");
    if Trigger::every(STORM_SECONDS * 15)
        .enqueue(ON_STORM)
        .done()
        .is_err()
    {
        log("Meteor Storm: the timer trigger was refused");
    }
    restart_timer();
}

/// The mission timer shows the time to the next storm.
fn restart_timer() {
    let _ = Action::timer_set(STORM_SECONDS);
    let _ = Action::timer_start();
}

fn on_storm(_event: TimerEvent) {
    let storm = STORMS.get() + 1;
    let _ = STORMS.set(storm);
    let Ok(warhead) = TypeRef::warhead("HE") else {
        log("Meteor Storm: warhead HE not found");
        return;
    };
    let anim = ANIMS.iter().find_map(|name| anim_id(name).ok());

    // Up to 64 ore cells from a random place on the map; wrap to the start
    // when the random place is past the last ore cell.
    let mut cells = [0i64; 64];
    let mut count = ore_cells(&mut cells, i64::from(random(0, 60000)));
    if count == 0 {
        count = ore_cells(&mut cells, 0);
    }
    if count == 0 {
        log("Meteor Storm: no ore left on the map - the sky stays clear");
        restart_timer();
        return;
    }

    for _ in 0..METEORS {
        let cell = Cell(cells[random(0, count as i32 - 1) as usize]);
        let caught = Find::objects()
            .near(cell, 2)
            .page(&mut [0i64; 32], 0)
            .map_or(0, |(found, _)| found);
        if let Some(anim) = anim {
            let _ = Action::spawn_anim(cell, anim);
        }
        let _ = Action::detonate_warhead(cell, warhead, DAMAGE);
        Line::new()
            .text("Meteor Storm: impact at ")
            .num(i64::from(cell.x()))
            .text(",")
            .num(i64::from(cell.y()))
            .text(" (ore ")
            .num(cell.ore().unwrap_or(0))
            .text(", height ")
            .num(cell.height().unwrap_or(0))
            .text(") - ")
            .num(caught as i64)
            .text(" units in the blast")
            .log();
    }

    if storm % 4 == 0 && Global::ion_storm_active() == Ok(false) {
        if Action::ion_storm_start(450).is_ok() {
            log("Meteor Storm: a lightning storm follows the meteors!");
        }
    }
    restart_timer();
    Line::new()
        .text("Meteor Storm: storm ")
        .num(storm)
        .text(" over; next in ")
        .num(Global::mission_timer().unwrap_or(0) / 15)
        .text(" s")
        .log();
}

/// Ore cells after cell index `after`, written to `out`; how many.
fn ore_cells(out: &mut [i64], after: i64) -> usize {
    Find::cells()
        .where_prop(Prop::Ore, Op::Gt, 0)
        .page(out, after)
        .map_or(0, |(count, _)| count)
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
vera20k_sdk::on_events!(ON_STORM => on_storm);
