//! The bench's finale (see `lib.rs`), run once every area's `MARKS` is
//! reached, since its points pause or end the match the areas play in.
//! Marks are named `finale.<point>`; `bench.all` follows the last.
//!
//! In order:
//! 1. `SuspendProduction`: a house order for the computer house on a type
//!    its factory is building; the factory's `Progress` stands still, then
//!    `QueueProduction` of the same type resumes it (`Progress` moves on).
//! 2. `Defeat`: every object of the computer house is removed
//!    (`Find` + `Action::remove`). Its defeat gate then asks; the first
//!    answer vetoes (`defeat-veto`: the house is still not defeated frames
//!    later, the gate asking again), the next allows (`defeat`: the house
//!    reads `IsDefeated`).
//! 3. `EndFor` the player as the winner (`end`, logged before the call);
//!    `after-end` once the module has run 30 more frames.
//!
//! Lib stops the areas' frame work when the finale starts (their triggers
//! and records stay), so nothing works on the house once it is gone.

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

/// Every mark of the finale.
pub const MARKS: &[&str] = &[
    "finale.suspend",
    "finale.resume",
    "finale.defeat-veto",
    "finale.defeat",
    "finale.end",
    "finale.after-end",
];

/// The `Defeat` decision's key (the finale's range is 500..599).
const K_DEFEAT: i32 = 500;

// Stages.
const PICK: i64 = 1;
const SUSPENDED: i64 = 2;
const RESUMED: i64 = 3;
const REMOVE: i64 = 4;
const DEFEAT: i64 = 5;
const END: i64 = 6;
const AFTER: i64 = 7;
const DONE: i64 = 8;

/// Frames production is watched after an order.
const WATCH: i64 = 45;
/// Longest wait for something in production, then for the defeat.
const PICK_LIMIT: i64 = 1500;
const REMOVE_LIMIT: i64 = 300;
const DEFEAT_LIMIT: i64 = 900;
/// Frames the house must stay undefeated after the veto.
const VETO_HOLD: i64 = 10;

static STAGE: AtomicI64 = AtomicI64::new(0);
/// The frame the stage began.
static AT: AtomicI64 = AtomicI64::new(0);
/// The watched factory (index into `FACTORIES`), the type it builds and
/// its last `Progress` reading.
static FACTORY: AtomicI64 = AtomicI64::new(0);
static TYPE: AtomicI64 = AtomicI64::new(0);
static PROGRESS: AtomicI64 = AtomicI64::new(-1);
/// The frame of the first veto, the gate's asks since, and whether the
/// next ask is allowed.
static VETOED_AT: AtomicI64 = AtomicI64::new(0);
static ASKS: AtomicI64 = AtomicI64::new(0);
static ALLOW: AtomicI64 = AtomicI64::new(0);

/// The computer house's factories: `Type`, `Progress` and `Ready` of each
/// kind.
const FACTORIES: [(Prop, Prop, Prop); 4] = [
    (
        Prop::Factory_Infantry_Type,
        Prop::Factory_Infantry_Progress,
        Prop::Factory_Infantry_Ready,
    ),
    (
        Prop::Factory_Vehicle_Type,
        Prop::Factory_Vehicle_Progress,
        Prop::Factory_Vehicle_Ready,
    ),
    (
        Prop::Factory_Building_Type,
        Prop::Factory_Building_Progress,
        Prop::Factory_Building_Ready,
    ),
    (
        Prop::Factory_Defense_Type,
        Prop::Factory_Defense_Progress,
        Prop::Factory_Defense_Ready,
    ),
];

fn stage(next: i64, now: i64) {
    STAGE.store(next, Relaxed);
    AT.store(now, Relaxed);
}

/// Whether the finale began (the areas' frame work stops).
pub fn started() -> bool {
    STAGE.load(Relaxed) != 0
}

/// Whether every step ran.
pub fn done() -> bool {
    STAGE.load(Relaxed) == DONE
}

/// Starts the finale on frame `now`: the computer house can pay for what
/// it builds, and the `Defeat` decision is made before its objects go.
pub fn begin(now: i32) {
    if started() {
        return;
    }
    stage(PICK, i64::from(now));
    let _ = Action::add_money(crate::enemy(), 10_000);
    if Trigger::on(Event::DEFEAT).decide_sync(K_DEFEAT).is_err() {
        crate::bench_fail("finale.defeat", "the Defeat trigger could not be made");
    }
}

/// A factory reading of the computer house, 0 when it cannot be read.
fn read(prop: Prop) -> i64 {
    get(crate::enemy(), prop).unwrap_or(0)
}

fn factory() -> (Prop, Prop, Prop) {
    FACTORIES[FACTORY.load(Relaxed) as usize]
}

/// Every frame once the finale began.
pub fn frame(now: i32) {
    let now = i64::from(now);
    let since = now - AT.load(Relaxed);
    match STAGE.load(Relaxed) {
        PICK => pick(now, since),
        SUSPENDED => suspended(now, since),
        RESUMED => resumed(now, since),
        REMOVE => remove(now, since),
        DEFEAT => defeat(now, since),
        END => {
            // Logged before the call, so the line exists if the call
            // stops the module.
            crate::mark("finale.end");
            if Action::end_for(crate::player(), Outcome::Win).is_err() {
                crate::bench_fail("finale.end", "EndFor answered an error");
            }
            stage(AFTER, now);
        }
        AFTER if since >= 30 => {
            crate::mark("finale.after-end");
            stage(DONE, now);
        }
        _ => {}
    }
}

/// Every 15 frames: the first factory building something halfway gets
/// `SuspendProduction` on its type.
fn pick(now: i64, since: i64) {
    if since > PICK_LIMIT {
        crate::bench_fail("finale.suspend", "the computer house built nothing");
        return stage(REMOVE, now);
    }
    if since % 15 != 0 {
        return;
    }
    for (index, &(ty, progress, _)) in FACTORIES.iter().enumerate() {
        let kind = read(ty);
        if kind == 0 || !(1..=90).contains(&read(progress)) {
            continue;
        }
        let order = Order::new(Command::SuspendProduction, 0, kind, &[]);
        if crate::enemy().order(&order).is_err() {
            crate::bench_fail("finale.suspend", "the order was turned down at once");
            return stage(REMOVE, now);
        }
        FACTORY.store(index as i64, Relaxed);
        TYPE.store(kind, Relaxed);
        PROGRESS.store(-1, Relaxed);
        return stage(SUSPENDED, now);
    }
}

/// The order ran the frame after it was given: `Progress` is read then,
/// and must be the same `WATCH` frames later. Then the same type is queued
/// again, which resumes it.
fn suspended(now: i64, since: i64) {
    let (ty, progress, _) = factory();
    if since == 3 {
        PROGRESS.store(read(progress), Relaxed);
    }
    if since < WATCH {
        return;
    }
    let kind = TYPE.load(Relaxed);
    let held = read(ty) == kind && read(progress) == PROGRESS.load(Relaxed);
    if !held {
        crate::bench_fail("finale.suspend", "the factory went on building");
        return stage(REMOVE, now);
    }
    crate::mark("finale.suspend");
    if crate::enemy()
        .order(&Order::queue_production(TypeRef(kind)))
        .is_err()
    {
        crate::bench_fail("finale.resume", "the order was turned down at once");
        return stage(REMOVE, now);
    }
    stage(RESUMED, now);
}

/// The resumed factory moved on: further, finished, or done with the type.
fn resumed(now: i64, since: i64) {
    if since < WATCH {
        return;
    }
    let (ty, progress, ready) = factory();
    let moved = read(progress) > PROGRESS.load(Relaxed)
        || read(ready) != 0
        || read(ty) != TYPE.load(Relaxed);
    if moved {
        crate::mark("finale.resume");
    } else {
        crate::bench_fail("finale.resume", "the factory stood still");
    }
    stage(REMOVE, now);
}

/// Removes the computer house's units and buildings, a page a frame, until
/// none is left.
fn remove(now: i64, since: i64) {
    let mut page = [NO_HANDLE; 32];
    // Units and buildings: `remove` takes no projectile.
    let mut find = Find::objects().owned_by(crate::enemy()).where_prop(
        Prop::Category,
        Op::Lt,
        Category::Projectile as i64,
    );
    // A page can be empty with more to examine after it (`next`).
    let mut after = 0;
    let count = loop {
        match find.page(&mut page, after) {
            Ok((0, next)) if next != 0 => after = next,
            Ok((count, _)) => break count,
            Err(_) => break 0,
        }
    };
    for &object in &page[..count] {
        let _ = Action::remove(Techno(object));
    }
    if count == 0 || since > REMOVE_LIMIT {
        stage(DEFEAT, now);
    }
}

/// Waits for the gate's veto to hold, then for the allowed defeat.
fn defeat(now: i64, since: i64) {
    let defeated = get(crate::enemy(), Prop::IsDefeated) == Ok(1);
    let vetoed_at = VETOED_AT.load(Relaxed);
    if ALLOW.load(Relaxed) == 0 {
        if vetoed_at != 0 && now >= vetoed_at + VETO_HOLD && ASKS.load(Relaxed) >= 2 {
            if defeated {
                crate::bench_fail("finale.defeat-veto", "the house was defeated anyway");
            } else {
                crate::mark("finale.defeat-veto");
            }
            ALLOW.store(1, Relaxed);
        }
    } else if defeated {
        crate::mark("finale.defeat");
        return stage(END, now);
    }
    if since > DEFEAT_LIMIT {
        crate::bench_fail(
            "finale.defeat",
            "the defeat gate never asked or never defeated",
        );
        stage(END, now);
    }
}

/// Records of the finale's range: none are delivered.
pub fn events(_record: &EventRecord) {}

/// The `Defeat` gate for the computer house: vetoed until the veto held,
/// then allowed.
pub fn decide(record: &mut DecideRecord) {
    if record.event.key != K_DEFEAT || record.event.house != crate::enemy().0 {
        return;
    }
    if ALLOW.load(Relaxed) != 0 {
        record.set_result(1);
        return;
    }
    record.set_result(0);
    ASKS.fetch_add(1, Relaxed);
    if VETOED_AT.load(Relaxed) == 0 {
        VETOED_AT.store(i64::from(frame()), Relaxed);
    }
}
