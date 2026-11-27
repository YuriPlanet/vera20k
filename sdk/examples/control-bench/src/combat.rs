//! Area `combat` of the bench (see `lib.rs`). Marks are named `combat.<point>`.
//!
//! Every combat decision, each on objects this area spawns in its arena and
//! drives itself (its own decisions never come from its own actions: a
//! module call skips that module's links, so every decision here comes from
//! a native fight):
//! - the range: an enemy Rhino `T1` (weapons off, asleep) that four player
//!   tanks attack. `T1` carries the `TakeDamage` decision (halved, checked
//!   against its health on the next frame), the delivered `TakeDamage`
//!   (counts the hits by shooter) and `FatalDamage` (the plan with the
//!   Tiberium remains bit, checked as ore or overlay around its cell).
//!   `SF` doubles its shot (`Fire` plan; two hits per shot), `SA` is refused
//!   fire three times (`FireAllowed`; no hit meanwhile) and then allowed (a
//!   hit), `SL`'s shells take a `Launch` path (straight up first) and `SP`'s
//!   a `ProjectileFrame` step (module data read back). `Launch` and
//!   `ProjectileFrame` bind the shell's projectile type, learned from `SF`'s
//!   shot plan; the handlers answer only for their own shooter's bullets.
//! - the shield: an enemy Rhino `T2` (weapons off, asleep) whose
//!   `DamageIncoming` a shield pool absorbs; health stays full.
//! - retaliation: an enemy Conscript `VR` (Sticky) hit by a player Rhino
//!   `AR` from outside its own range; `Retaliate` answers 0 and `VR` must not
//!   turn on `AR` (no Attack mission, no target `AR`) where the engine would
//!   have (`native` 1).
//! - crushing: a player Grizzly `CT` (weapons off) drives onto the cells of
//!   two enemy GIs (weapons off, asleep) lined up east of it, one after the
//!   other; `CrushAllowed` spares the
//!   first one it meets (still alive next frame) and allows the other (its
//!   `Crushed` event follows).
//!
//! Units that die are respawned (up to `MAX_SPAWNS` each) until every mark
//! is in.
//!
//! Not reached in the bench's scene (left out of `MARKS`):
//! - `Defeat` (`combat.defeat`): a house is asked only once it holds
//!   nothing. The scene has one human house and one computer house, which
//!   the other areas need for the whole run, so no house may be brought
//!   there safely.

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::sys::ProjectileStep;
use vera20k_sdk::{
    CrushAllowedDecision, CrushedEvent, DamageIncomingDecision, FireAllowedDecision, NO_HANDLE,
    RetaliateDecision, get,
};

use crate::{COMBAT, alive, arena, bench_fail, enemy, mark, marked, offset, player, spawn};

/// Every mark this area reaches in the bench's scene.
pub const MARKS: &[&str] = &[
    "combat.take_damage",
    "combat.take_damage_event",
    "combat.damage_incoming",
    "combat.fatal",
    "combat.fire",
    "combat.fire_allowed",
    "combat.retaliate",
    "combat.launch",
    "combat.projectile_frame",
    "combat.crush",
];

// ------------------------------------------------------------------ Keys

const TD_DECIDE: i32 = 300;
const TD_EVENT: i32 = 301;
const INCOMING: i32 = 302;
const FATAL: i32 = 303;
const FIRE: i32 = 304;
const FIRE_ALLOWED: i32 = 305;
const RETALIATE: i32 = 306;
const LAUNCH: i32 = 307;
const PFRAME: i32 = 308;
const CRUSH: i32 = 309;
const CRUSHED: i32 = 310;

const KEYS: usize = 11;
static TRIGGERS: [AtomicI64; KEYS] = [const { AtomicI64::new(NO_HANDLE) }; KEYS];

fn trigger(key: i32) -> TriggerRef {
    TriggerRef(TRIGGERS[(key - TD_DECIDE) as usize].load(Relaxed))
}

fn make(key: i32, made: vera20k_sdk::Result<TriggerRef>) {
    match made {
        Ok(t) => TRIGGERS[(key - TD_DECIDE) as usize].store(t.0, Relaxed),
        Err(_) => bench_fail("combat.triggers", "trigger_create failed"),
    }
}

// ----------------------------------------------------------------- Units

/// What a slot's unit gets once spawned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Setup {
    /// Nothing: a player shooter.
    None,
    /// Weapons off.
    Disarmed,
    /// Weapons off and asleep (no scatter, no retaliation, never
    /// recruited): a target.
    Target,
    /// Sticky: holds its place, keeps its weapon and retaliates.
    Hold,
}

struct Slot {
    id: &'static str,
    enemy: bool,
    /// Cells from the arena's centre.
    at: (i32, i32),
    setup: Setup,
    /// The triggers bound to it.
    binds: &'static [i32],
}

const T1: usize = 0;
const SF: usize = 1;
const SL: usize = 2;
const SP: usize = 3;
const SA: usize = 4;
const T2: usize = 5;
const SDI: usize = 6;
const VR: usize = 7;
const AR: usize = 8;
const CT: usize = 9;
const I1: usize = 10;
const I2: usize = 11;
const UNITS: usize = 12;

const SLOTS: [Slot; UNITS] = [
    Slot {
        id: "HTNK",
        enemy: true,
        at: (0, 0),
        setup: Setup::Target,
        binds: &[TD_DECIDE, TD_EVENT, FATAL],
    },
    Slot {
        id: "MTNK",
        enemy: false,
        at: (-4, 0),
        setup: Setup::None,
        binds: &[FIRE],
    },
    Slot {
        id: "MTNK",
        enemy: false,
        at: (0, -4),
        setup: Setup::None,
        binds: &[],
    },
    Slot {
        id: "MTNK",
        enemy: false,
        at: (4, 0),
        setup: Setup::None,
        binds: &[],
    },
    Slot {
        id: "HTNK",
        enemy: false,
        at: (0, 4),
        setup: Setup::None,
        binds: &[FIRE_ALLOWED],
    },
    Slot {
        id: "HTNK",
        enemy: true,
        at: (-8, -6),
        setup: Setup::Target,
        binds: &[INCOMING],
    },
    Slot {
        id: "MTNK",
        enemy: false,
        at: (-5, -6),
        setup: Setup::None,
        binds: &[],
    },
    Slot {
        id: "E2",
        enemy: true,
        at: (-8, 6),
        setup: Setup::Hold,
        binds: &[RETALIATE],
    },
    Slot {
        id: "HTNK",
        enemy: false,
        at: (-4, 3),
        setup: Setup::None,
        binds: &[],
    },
    Slot {
        id: "MTNK",
        enemy: false,
        at: (-3, 9),
        setup: Setup::Disarmed,
        binds: &[],
    },
    Slot {
        id: "E1",
        enemy: true,
        at: (2, 0),
        setup: Setup::Target,
        binds: &[CRUSH, CRUSHED],
    },
    Slot {
        id: "E1",
        enemy: true,
        at: (4, 0),
        setup: Setup::Target,
        binds: &[CRUSH, CRUSHED],
    },
];

/// Who attacks whom: (shooter, target).
const ATTACKS: [(usize, usize); 6] = [(SF, T1), (SL, T1), (SP, T1), (SA, T1), (SDI, T2), (AR, VR)];

/// Most respawns of one slot.
const MAX_SPAWNS: i64 = 40;

static UNIT: [AtomicI64; UNITS] = [const { AtomicI64::new(NO_HANDLE) }; UNITS];
static SPAWNS: [AtomicI64; UNITS] = [const { AtomicI64::new(0) }; UNITS];

fn unit(slot: usize) -> Techno {
    Techno(UNIT[slot].load(Relaxed))
}

fn center() -> Cell {
    arena(COMBAT)
}

/// Where a slot's offset counts from: the GIs line up east of the crusher
/// where it stands (the arena's row may be clamped at the map's edge), the
/// others from the arena's centre.
fn anchor(slot: usize) -> Cell {
    let tank = unit(CT);
    match get(tank, Prop::Cell) {
        Ok(cell) if (slot == I1 || slot == I2) && alive(tank) => Cell(cell),
        _ if slot == I1 || slot == I2 => offset(center(), -3, 9),
        _ => center(),
    }
}

/// The current frame (this file's `frame` is the area's hook).
fn clock() -> i64 {
    i64::from(vera20k_sdk::frame())
}

/// Respawns the dead, rebinds their triggers and keeps the attackers on
/// their targets; runs on a schedule.
fn upkeep() {
    if MARKS.iter().all(|m| marked(m)) {
        return;
    }
    for (slot, spec) in SLOTS.iter().enumerate() {
        if alive(unit(slot)) || SPAWNS[slot].load(Relaxed) >= MAX_SPAWNS {
            continue;
        }
        let house = if spec.enemy { enemy() } else { player() };
        let Some(made) = spawn(offset(anchor(slot), spec.at.0, spec.at.1), spec.id, house) else {
            continue;
        };
        SPAWNS[slot].fetch_add(1, Relaxed);
        UNIT[slot].store(made.0, Relaxed);
        if slot == T2 {
            SHIELD.store(SHIELD_POOL, Relaxed); // a new shield for a new T2
        }
        for &key in spec.binds {
            let t = trigger(key);
            if t.0 != NO_HANDLE {
                let _ = t.bind(made);
            }
        }
        if matches!(spec.setup, Setup::Disarmed | Setup::Target) {
            let _ = Modifier::override_(made, Prop::DisableWeapons, 1, -1);
        }
        match spec.setup {
            Setup::Target => {
                if Action::set_mission(made, Mission::Sleep, Cell(NO_HANDLE)).is_err() {
                    let _ = Action::set_mission(made, Mission::Sticky, Cell(NO_HANDLE));
                }
            }
            Setup::Hold => {
                let _ = Action::set_mission(made, Mission::Sticky, Cell(NO_HANDLE));
            }
            Setup::None | Setup::Disarmed => {}
        }
    }
    for (shooter, target) in ATTACKS {
        let (shooter, target) = (unit(shooter), unit(target));
        if alive(shooter) && alive(target) && shooter.target() != Ok(target.0) {
            let _ = Action::set_mission_on(shooter, Mission::Attack, target);
        }
    }
    drive_crusher();
}

// ----------------------------------------------------------------- State

/// `TakeDamage` decision: T1's expected health and the decision's frame.
static TD_EXPECT: AtomicI64 = AtomicI64::new(0);
static TD_FRAME: AtomicI64 = AtomicI64::new(0);

/// `DamageIncoming`: the shield pool, T2's health at a fully absorbed hit
/// and its frame.
const SHIELD_POOL: i64 = 5000;
static SHIELD: AtomicI64 = AtomicI64::new(SHIELD_POOL);
static DI_HEALTH: AtomicI64 = AtomicI64::new(0);
static DI_FRAME: AtomicI64 = AtomicI64::new(0);

/// `FatalDamage`: the dead T1's cell, the ore and overlay count around it
/// before, and the frame.
static FATAL_CELL: AtomicI64 = AtomicI64::new(0);
static FATAL_ORE: AtomicI64 = AtomicI64::new(0);
static FATAL_OVERLAYS: AtomicI64 = AtomicI64::new(0);
static FATAL_FRAME: AtomicI64 = AtomicI64::new(0);

/// `Fire`: the doubled shot's frame and the hits counted since.
static FIRE_FRAME: AtomicI64 = AtomicI64::new(0);
static FIRE_HITS: AtomicI64 = AtomicI64::new(0);
/// The shell's projectile type ID + 1 (0 unknown), and whether `Launch`
/// and `ProjectileFrame` were bound to it.
static SHELL_TYPE: AtomicI64 = AtomicI64::new(0);
/// The shell type value `Launch` and `ProjectileFrame` are bound to.
static SHELL_BOUND: AtomicI64 = AtomicI64::new(0);

/// `FireAllowed`: refusals so far and the frame fire was allowed.
static FA_REFUSALS: AtomicI64 = AtomicI64::new(0);
static FA_OPEN: AtomicI64 = AtomicI64::new(0);
const FA_REFUSE: i64 = 3;

/// `Retaliate`: the frame of a refusal the engine would have retaliated
/// for.
static RET_FRAME: AtomicI64 = AtomicI64::new(0);

/// `Launch`: the tracked bullet, its launch position and frame.
static LAUNCH_BULLET: AtomicI64 = AtomicI64::new(0);
static LAUNCH_AT: [AtomicI64; 3] = [const { AtomicI64::new(0) }; 3];
static LAUNCH_FRAME: AtomicI64 = AtomicI64::new(0);
/// Straight up first, this far, at this speed.
const RISE: i64 = 1024;
const RISE_SPEED: i64 = 96;

/// `ProjectileFrame`: the answered bullet.
/// `ProjectileFrame`: the bullets answered and not yet checked (a ring).
const PF_RING: usize = 4;
static PF_BULLETS: [AtomicI64; PF_RING] = [const { AtomicI64::new(0) }; PF_RING];
static PF_NEXT: AtomicI64 = AtomicI64::new(0);
const PF_MARKER: i64 = 0x5EED_C0DE;

/// `CrushAllowed`: the spared victim and its frame, the allowed one, and
/// what was seen (bit 0 spared alive, bit 1 allowed crushed).
static SPARED: AtomicI64 = AtomicI64::new(0);
static SPARED_FRAME: AtomicI64 = AtomicI64::new(0);
static ALLOWED: AtomicI64 = AtomicI64::new(0);
static CRUSH_SEEN: AtomicI64 = AtomicI64::new(0);
/// The crusher's leg: even east, odd west.
static CRUSH_LEG: AtomicI64 = AtomicI64::new(0);
/// The frame the current leg started.
static LEG_SINCE: AtomicI64 = AtomicI64::new(0);

// --------------------------------------------------------------- Exports

/// `vera_init`.
pub fn init() {}

/// `on_scenario_start`.
pub fn start() {}

/// The frame the arena opens: the houses and `crate::arena` are known.
pub fn open() {
    make(
        TD_DECIDE,
        Trigger::on(Event::TAKE_DAMAGE).decide_sync(TD_DECIDE),
    );
    make(
        TD_EVENT,
        Trigger::on(Event::TAKE_DAMAGE).enqueue(TD_EVENT).done(),
    );
    make(
        INCOMING,
        Trigger::on(Event::DAMAGE_INCOMING).decide_sync(INCOMING),
    );
    make(FATAL, Trigger::on(Event::FATAL_DAMAGE).decide_sync(FATAL));
    make(FIRE, Trigger::on(Event::FIRE).decide_sync(FIRE));
    make(
        FIRE_ALLOWED,
        Trigger::on(Event::FIRE_ALLOWED).decide_sync(FIRE_ALLOWED),
    );
    make(
        RETALIATE,
        Trigger::on(Event::RETALIATE).decide_sync(RETALIATE),
    );
    make(CRUSH, Trigger::on(Event::CRUSH_ALLOWED).decide_sync(CRUSH));
    make(CRUSHED, Trigger::on(Event::CRUSHED).enqueue(CRUSHED).done());
    upkeep();
}

/// Every later frame.
pub fn frame(now: i32) {
    let now = i64::from(now);
    if now % 30 == 0 {
        upkeep();
    }
    bind_shell_type();
    check_take_damage(now);
    check_incoming(now);
    check_fatal(now);
    check_retaliate(now);
    check_launch(now);
    check_projectile_frame();
    check_crush(now);
}

/// Records whose key is in this area's range.
pub fn events(record: &EventRecord) {
    match record.key {
        TD_EVENT => on_take_damage(TakeDamageEvent::from(record)),
        CRUSHED => on_crushed(CrushedEvent::from(record)),
        _ => {}
    }
}

/// Decisions whose key is in this area's range.
pub fn decide(record: &mut DecideRecord) {
    match record.event.key {
        TD_DECIDE => take_damage(TakeDamageDecision::from(record)),
        INCOMING => incoming(DamageIncomingDecision::from(record)),
        FATAL => fatal(FatalDamageDecision::from(record)),
        FIRE => fire(FireDecision::from(record)),
        FIRE_ALLOWED => fire_allowed(FireAllowedDecision::from(record)),
        RETALIATE => retaliate(RetaliateDecision::from(record)),
        LAUNCH => launch(LaunchDecision::from(record)),
        PFRAME => projectile_frame(ProjectileFrameDecision::from(record)),
        CRUSH => crush(CrushAllowedDecision::from(record)),
        _ => {}
    }
}

// ----------------------------------------------------------- TakeDamage

/// Halves T1's damage and notes the health it must have afterwards.
fn take_damage(mut d: TakeDamageDecision) {
    let victim = d.victim();
    let damage = d.previous();
    if victim.0 != unit(T1).0 || damage <= 0 {
        return;
    }
    let answer = damage / 2;
    d.set_result(answer);
    let Ok(health) = victim.health() else {
        return;
    };
    TD_EXPECT.store(health - answer.min(health), Relaxed);
    TD_FRAME.store(clock(), Relaxed);
}

fn check_take_damage(now: i64) {
    let at = TD_FRAME.load(Relaxed);
    if at == 0 || now <= at || marked("combat.take_damage") {
        return;
    }
    TD_FRAME.store(0, Relaxed);
    let expect = TD_EXPECT.load(Relaxed);
    if expect <= 0 {
        return; // the hit killed it
    }
    match unit(T1).health() {
        Ok(health) if health == expect => mark("combat.take_damage"),
        Ok(_) => bench_fail("combat.take_damage", "health after the halved hit differs"),
        Err(_) => {}
    }
}

/// The delivered `TakeDamage` on T1: the mark, and the hits by shooter for
/// `Fire` and `FireAllowed`.
fn on_take_damage(e: TakeDamageEvent) {
    if e.damage() > 0 && !e.victim().is_none() {
        mark("combat.take_damage_event");
    }
    let attacker = e.attacker().0;
    if attacker == NO_HANDLE {
        return;
    }
    let now = clock();
    if attacker == unit(SF).0 {
        let shot = FIRE_FRAME.load(Relaxed);
        if shot > 0 && now >= shot && now - shot < 50 {
            let hits = FIRE_HITS.fetch_add(1, Relaxed) + 1;
            if hits >= 2 {
                mark("combat.fire");
            }
        }
    }
    if attacker == unit(SA).0 {
        if FA_OPEN.load(Relaxed) == 0 {
            if FA_REFUSALS.load(Relaxed) > 0 {
                bench_fail("combat.fire_allowed", "the shooter fired while refused");
            }
        } else if FA_REFUSALS.load(Relaxed) >= FA_REFUSE {
            mark("combat.fire_allowed");
        }
    }
}

// ------------------------------------------------------- DamageIncoming

/// A shield on T2: the pool absorbs incoming damage at the entry.
fn incoming(mut d: DamageIncomingDecision) {
    let victim = d.victim();
    let damage = d.incoming();
    if victim.0 != unit(T2).0 || damage <= 0 || d.preview() < 0 {
        return;
    }
    let pool = SHIELD.load(Relaxed);
    let absorbed = damage.min(pool);
    SHIELD.store(pool - absorbed, Relaxed);
    d.set_result(damage - absorbed);
    match victim.health() {
        Ok(health) if absorbed == damage => {
            DI_HEALTH.store(health, Relaxed);
            DI_FRAME.store(clock(), Relaxed);
        }
        _ => DI_FRAME.store(0, Relaxed),
    }
}

fn check_incoming(now: i64) {
    let at = DI_FRAME.load(Relaxed);
    if at == 0 || now <= at || marked("combat.damage_incoming") {
        return;
    }
    DI_FRAME.store(0, Relaxed);
    match unit(T2).health() {
        Ok(health) if health == DI_HEALTH.load(Relaxed) => mark("combat.damage_incoming"),
        Ok(_) => bench_fail(
            "combat.damage_incoming",
            "an absorbed hit still took health",
        ),
        Err(_) => {}
    }
}

// ---------------------------------------------------------- FatalDamage

/// Ore and the number of overlaid cells on and around `cell`.
fn tiberium_around(cell: Cell) -> (i64, i64) {
    let (mut ore, mut overlays) = (0, 0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let c = offset(cell, dx, dy);
            ore += c.ore().unwrap_or(0);
            if c.overlay().is_ok_and(|t| t.0 != NO_HANDLE) {
                overlays += 1;
            }
        }
    }
    (ore, overlays)
}

/// T1's death plan with the Tiberium remains bit.
fn fatal(mut d: FatalDamageDecision) {
    if d.victim().0 != unit(T1).0 {
        return;
    }
    let [x, y, _] = d.position();
    if x < 0 || y < 0 {
        return;
    }
    let cell = Cell::at((x / 256) as u16, (y / 256) as u16);
    let (ore, overlays) = tiberium_around(cell);
    let plan = d.with(FatalDamageDecision::TIBERIUM_REMAINS, true);
    d.set_result(plan);
    FATAL_CELL.store(cell.0, Relaxed);
    FATAL_ORE.store(ore, Relaxed);
    FATAL_OVERLAYS.store(overlays, Relaxed);
    FATAL_FRAME.store(clock(), Relaxed);
}

fn check_fatal(now: i64) {
    let at = FATAL_FRAME.load(Relaxed);
    if at == 0 || now <= at || marked("combat.fatal") {
        return;
    }
    let (ore, overlays) = tiberium_around(Cell(FATAL_CELL.load(Relaxed)));
    if ore > FATAL_ORE.load(Relaxed) || overlays > FATAL_OVERLAYS.load(Relaxed) {
        FATAL_FRAME.store(0, Relaxed);
        mark("combat.fatal");
    } else if now - at > 10 {
        // Nothing grew (land that takes no Tiberium, or three zero
        // amounts): the next T1 tries again.
        FATAL_FRAME.store(0, Relaxed);
    }
}

// ----------------------------------------------------------------- Fire

/// SF's shot plan: the native entry twice. Learns the shell's projectile
/// type for `Launch` and `ProjectileFrame`.
fn fire(mut d: FireDecision) {
    if d.shooter().0 != unit(SF).0 {
        return;
    }
    let Some(native) = d.previous_entry(0) else {
        return;
    };
    if native.projectile_type >= 0 {
        SHELL_TYPE.store(i64::from(native.projectile_type) + 1, Relaxed);
    }
    if d.set_plan(&[native, native]) {
        FIRE_FRAME.store(clock(), Relaxed);
        FIRE_HITS.store(0, Relaxed);
    }
}

/// SA's first shots refused, then allowed.
fn fire_allowed(mut d: FireAllowedDecision) {
    if d.shooter().0 != unit(SA).0 {
        return;
    }
    if FA_OPEN.load(Relaxed) == 0 && FA_REFUSALS.load(Relaxed) < FA_REFUSE {
        d.set_result(0);
        FA_REFUSALS.fetch_add(1, Relaxed);
    } else {
        d.set_result(1);
        if FA_OPEN.load(Relaxed) == 0 {
            FA_OPEN.store(clock(), Relaxed);
        }
    }
}

// ------------------------------------------------------------ Retaliate

/// VR never retaliates; a refusal where the engine would have is checked.
fn retaliate(mut d: RetaliateDecision) {
    let victim = d.victim();
    if victim.0 != unit(VR).0 {
        return;
    }
    let source = d.source().0;
    let native = d.native();
    d.set_result(0);
    if native == 1
        && source != NO_HANDLE
        && source == unit(AR).0
        && victim.target() != Ok(source)
        && victim.mission() != Ok(Mission::Attack as i64)
    {
        RET_FRAME.store(clock(), Relaxed);
    }
}

fn check_retaliate(now: i64) {
    let at = RET_FRAME.load(Relaxed);
    if at == 0 || now <= at || marked("combat.retaliate") {
        return;
    }
    RET_FRAME.store(0, Relaxed);
    let victim = unit(VR);
    if !alive(victim) {
        return;
    }
    // Its own scan may still pick the attacker; only a clean frame counts.
    if victim.target() != Ok(unit(AR).0) && victim.mission() != Ok(Mission::Attack as i64) {
        mark("combat.retaliate");
    }
}

// ------------------------------------------------------ Launch and frame

/// Binds `Launch` and `ProjectileFrame` to the shell's projectile type once
/// `SF`'s shot plan named it.
fn bind_shell_type() {
    let shell = SHELL_TYPE.load(Relaxed);
    if shell == 0 || SHELL_BOUND.swap(shell, Relaxed) == shell {
        return;
    }
    for key in [LAUNCH, PFRAME] {
        let old = trigger(key);
        if old.0 != NO_HANDLE {
            let _ = old.remove();
        }
    }
    let ty = TypeRef::new(NameKind::Projectile, (shell - 1) as i32);
    make(
        LAUNCH,
        Trigger::on(Event::LAUNCH).bind(ty).decide_sync(LAUNCH),
    );
    make(
        PFRAME,
        Trigger::on(Event::PROJECTILE_FRAME)
            .bind(ty)
            .decide_sync(PFRAME),
    );
}

/// SL's shells: straight up first, then down onto the target.
fn launch(mut d: LaunchDecision) {
    let shooter = unit(SL).0;
    if shooter == NO_HANDLE || d.launcher().0 != shooter {
        return;
    }
    let bullet = d.projectile();
    let (Ok(x), Ok(y), Ok(z)) = (
        get(bullet, Prop::X),
        get(bullet, Prop::Y),
        get(bullet, Prop::Z),
    ) else {
        return;
    };
    let target = d.target();
    let end = match (
        get(target, Prop::X),
        get(target, Prop::Y),
        get(target, Prop::Z),
    ) {
        (Ok(tx), Ok(ty), Ok(tz)) if !target.is_none() => [tx, ty, tz],
        _ => {
            let cell = d.target_cell();
            [
                i64::from(cell.x()) * 256 + 128,
                i64::from(cell.y()) * 256 + 128,
                z,
            ]
        }
    };
    let path = [[x, y, z + RISE], end];
    if d.set_path(PathKind::Waypoints, RISE_SPEED * FIXED_ONE, [0, 0], &path)
        && LAUNCH_BULLET.load(Relaxed) == 0
    {
        LAUNCH_BULLET.store(bullet.0, Relaxed);
        for (slot, value) in LAUNCH_AT.iter().zip([x, y, z]) {
            slot.store(value, Relaxed);
        }
        LAUNCH_FRAME.store(clock(), Relaxed);
    }
}

fn check_launch(now: i64) {
    let bullet = Projectile(LAUNCH_BULLET.load(Relaxed));
    if bullet.is_none() || marked("combat.launch") {
        return;
    }
    let at = |i: usize| LAUNCH_AT[i].load(Relaxed);
    let (Ok(x), Ok(y), Ok(z)) = (
        get(bullet, Prop::X),
        get(bullet, Prop::Y),
        get(bullet, Prop::Z),
    ) else {
        LAUNCH_BULLET.store(0, Relaxed); // gone: track the next one
        return;
    };
    let (dx, dy) = ((x - at(0)).abs(), (y - at(1)).abs());
    let rose = z > at(2) + 16;
    if dx <= 32 && dy <= 32 && rose {
        mark("combat.launch");
    } else if (dx > 64 || dy > 64) && !rose {
        LAUNCH_BULLET.store(0, Relaxed);
        bench_fail("combat.launch", "the bullet left its path");
    } else if now - LAUNCH_FRAME.load(Relaxed) > 40 {
        LAUNCH_BULLET.store(0, Relaxed);
    }
}

/// Shooters whose shells `ProjectileFrame` steps: every player tank on the
/// range but SL (whose shells follow their `Launch` path).
const PF_SHOOTERS: [usize; 4] = [SP, SF, SA, AR];

/// One step for each shell of the shooters above, once per shell: its own
/// velocity, and module data to read back.
fn projectile_frame(mut d: ProjectileFrameDecision) {
    let bullet = d.projectile();
    let Ok(source) = get(bullet, Prop::Source) else {
        return;
    };
    if source == NO_HANDLE || !PF_SHOOTERS.iter().any(|&slot| unit(slot).0 == source) {
        return;
    }
    if d.frames() < 0 || PF_BULLETS.iter().any(|b| b.load(Relaxed) == bullet.0) {
        return;
    }
    let velocity = |prop: Prop| {
        get(bullet, prop)
            .unwrap_or(0)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    let mut data = [0u8; 16];
    data[..8].copy_from_slice(&PF_MARKER.to_le_bytes());
    let step = ProjectileStep {
        flags: 2 | 8, // set velocity, set module data
        velocity: [velocity(Prop::VX), velocity(Prop::VY), velocity(Prop::VZ)],
        data,
        ..ProjectileStep::default()
    };
    if d.set_step(&step) {
        let at = PF_NEXT.fetch_add(1, Relaxed) as usize % PF_RING;
        PF_BULLETS[at].store(bullet.0, Relaxed);
    }
}

fn check_projectile_frame() {
    if marked("combat.projectile_frame") {
        return;
    }
    for slot in &PF_BULLETS {
        let bullet = Projectile(slot.load(Relaxed));
        if bullet.is_none() {
            continue;
        }
        match get(bullet, Prop::Data0) {
            Ok(PF_MARKER) => {
                mark("combat.projectile_frame");
                return;
            }
            Ok(_) => {
                slot.store(0, Relaxed);
                bench_fail(
                    "combat.projectile_frame",
                    "the step's module data was not kept",
                );
            }
            Err(_) => {
                slot.store(0, Relaxed); // gone: the next shell is answered
            }
        }
    }
}

// ---------------------------------------------------------------- Crush

/// The first GI the crusher meets is spared (each time), any other is
/// crushed.
fn crush(mut d: CrushAllowedDecision) {
    let victim = d.victim().0;
    if victim == NO_HANDLE || (victim != unit(I1).0 && victim != unit(I2).0) {
        return;
    }
    let spared = SPARED.load(Relaxed);
    if spared == NO_HANDLE || spared == victim {
        d.set_result(0);
        SPARED.store(victim, Relaxed);
        SPARED_FRAME.store(clock(), Relaxed);
    } else {
        d.set_result(1);
        ALLOWED.store(victim, Relaxed);
    }
}

fn on_crushed(e: CrushedEvent) {
    let victim = e.victim().0;
    if victim == NO_HANDLE {
        return;
    }
    if victim == SPARED.load(Relaxed) {
        bench_fail("combat.crush", "a refused crush still crushed");
    } else if victim == ALLOWED.load(Relaxed) {
        CRUSH_SEEN.fetch_or(2, Relaxed);
    }
}

fn check_crush(now: i64) {
    if marked("combat.crush") {
        return;
    }
    if CRUSH_SEEN.load(Relaxed) & 1 == 0 && !alive(Techno(SPARED.load(Relaxed))) {
        SPARED.store(NO_HANDLE, Relaxed); // spare the next GI met instead
    }
    let at = SPARED_FRAME.load(Relaxed);
    if at != 0 && now > at {
        SPARED_FRAME.store(0, Relaxed);
        if alive(Techno(SPARED.load(Relaxed))) {
            CRUSH_SEEN.fetch_or(1, Relaxed);
        }
    }
    if CRUSH_SEEN.load(Relaxed) == 3 {
        mark("combat.crush");
    }
}

/// Drives the crusher onto the GIs' cells in turn, one leg each: the crush
/// is checked in the cell the crusher stands in, against occupants within
/// half a cell of its centre, so its legs end on the GIs themselves. A leg
/// ends when the crusher reaches that cell or after 300 frames.
fn drive_crusher() {
    let tank = unit(CT);
    if marked("combat.crush") {
        return;
    }
    if !alive(tank) {
        return;
    }
    let now = clock();
    let mut leg = CRUSH_LEG.load(Relaxed);
    let goal = |leg: i64| {
        let first = if leg % 2 == 0 { I1 } else { I2 };
        [first, first ^ I1 ^ I2]
            .into_iter()
            .map(unit)
            .find(|gi| alive(*gi))
            .and_then(|gi| get(gi, Prop::Cell).ok())
    };
    let here = get(tank, Prop::Cell).ok();
    let reached = goal(leg).is_some() && here == goal(leg);
    if reached || now - LEG_SINCE.load(Relaxed) > 300 {
        leg += 1;
        CRUSH_LEG.store(leg, Relaxed);
        LEG_SINCE.store(now, Relaxed);
    } else if tank.moving().unwrap_or(false) {
        return;
    }
    if let Some(cell) = goal(leg) {
        let _ = Action::set_mission(tank, Mission::Move, Cell(cell));
    }
}
