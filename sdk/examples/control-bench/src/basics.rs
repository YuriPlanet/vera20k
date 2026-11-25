//! Area `basics` of the bench (see `lib.rs`). Marks are named `basics.<point>`.
//!
//! Host calls, property reads (objects, houses, types, cells, the global
//! reference, viewpoint reads), writes through `set`, modifiers of every
//! wired stat, declared variables (this crate's one `vars!`), `find` in
//! every scope, and INI registration (RFC 0001 section 6h).
//!
//! Three units of its own in its arena: two player MTNKs (`A` takes the
//! reads and the modifiers, `B` the writes) and one enemy HTNK (`E`, for
//! `Relation`, `Engage` and `DisableWeapons`, removed again after the
//! reads). A short-lived unit with ammo and a powered building take the
//! `Ammo` and `Online` writes and are removed after them. One check group
//! runs per frame over the first ~45 frames after the arena opens; the
//! enemy's factories and its build area are polled later.
//!
//! Not marked (left out of `MARKS`):
//! - `lookup`/`name_of(TeamType)`: TeamType IDs are the map's own team
//!   definitions and XMP03T4 has none to look up; `name_of` with an ID that
//!   names nothing stops the module, so there is nothing safe to try.
//! - `lookup`/`name_of(ModuleAction)`: the crate's `named_actions!` is
//!   `triggers`'s, whose names this area does not know.
//! - `ControlGroup` (client only), `GameMode`, `Team`/`Tag` on a member,
//!   `Cloaked`, `NearbyEnemyCount` and the other properties the engine does
//!   not read yet (`src/sim/ext_modules/props.rs` residuals).
//! - `CritMultiplier`, `CritExtraChance` and `Unkillable` work only in a
//!   Phobos build. Their marks pass either way: in a Phobos build when the
//!   modifier reads back, otherwise when `has_kind` says 0 and `modify`
//!   answers `Unsupported`, which is the correct answer there.
//!
//! SDK gaps worked around here: `Find` has no constructor for the
//! `Modifiers` (7) and `SuperWeapons` (8) scopes and no variable clause, so
//! `raw_find` builds those queries over `sys::find`; `has_kind` has no
//! wrapper (raw `sys::has_kind`).

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get, get_many, name_of, set, sys};

use crate::{BASICS, Line, alive, arena, bench_fail, enemy, mark, marked, offset, player, spawn};

/// Every mark this area reaches in the bench scene.
pub const MARKS: &[&str] = &[
    "basics.frame",
    "basics.random",
    "basics.log",
    "basics.has-kind",
    "basics.name-techno",
    "basics.name-weapon",
    "basics.name-warhead",
    "basics.name-projectile",
    "basics.name-anim",
    "basics.name-voxelanim",
    "basics.name-particles",
    "basics.name-super",
    "basics.name-country",
    "basics.name-variable",
    "basics.name-property",
    "basics.name-action",
    "basics.name-string",
    "basics.name-event",
    "basics.object-props",
    "basics.object-more",
    "basics.get-many",
    "basics.house-props",
    "basics.type-props",
    "basics.weapon-props",
    "basics.warhead-props",
    "basics.cell-props",
    "basics.waypoint-cell",
    "basics.global-props",
    "basics.relation",
    "basics.visible",
    "basics.engage",
    "basics.revealed",
    "basics.placeable",
    "basics.factory",
    "basics.set-health",
    "basics.set-rank",
    "basics.set-owner",
    "basics.set-mission",
    "basics.set-ammo",
    "basics.set-online",
    "basics.mod-compose",
    "basics.mod-group",
    "basics.mod-unmodify",
    "basics.mod-props",
    "basics.mod-expire",
    "basics.find-modifiers",
    "basics.mod-firepower",
    "basics.mod-armor",
    "basics.mod-speed",
    "basics.mod-rof",
    "basics.mod-range",
    "basics.mod-range-extra",
    "basics.mod-crit",
    "basics.mod-unkillable",
    "basics.mod-cloakable",
    "basics.mod-decloak",
    "basics.mod-noweapons",
    "basics.mod-immunity",
    "basics.mod-rot",
    "basics.var-defaults",
    "basics.var-global",
    "basics.var-house",
    "basics.var-object",
    "basics.var-lookup",
    "basics.find-var",
    "basics.find-objects",
    "basics.find-objects-area",
    "basics.find-houses",
    "basics.find-cells-map",
    "basics.find-cells-circle",
    "basics.find-cells-rect",
    "basics.find-supers",
    "basics.ini-entry",
    "basics.ini-key",
    "basics.module-event",
];

// ------------------------------------------------------------------ Keys

/// The registry `[Bench.Registry]`'s `IniEntry` records.
const KEY_REGISTRY: i32 = 101;
/// `Bench.Key=` on TechnoType sections: `IniKey` records.
const KEY_INI_KEY: i32 = 102;
/// The code trigger on `Bench.BasicsEvent`.
const KEY_EVENT: i32 = 103;

const EVENT_NAME: &str = "Bench.BasicsEvent";
/// The TechnoType section that holds `Bench.Key=` in `rulesmd.ini`.
const KEY_SECTION: &str = "BFRT";
const KEY_VALUE: &str = "Bench,7";

static EVENT: ini::EventSlot = ini::EventSlot::new();

// ------------------------------------------------------------- Variables

vera20k_sdk::vars! {
    static BENCH_WAVE: GlobalVar = ("BenchWave", Public, 7);
    static BENCH_SCORE: HouseVar = ("BenchScore", Allies, 3);
    static BENCH_TAG: ObjectVar = ("BenchTag", Private, 5);
}

// ----------------------------------------------------------------- State

/// The frame the arena opened.
static OPEN_AT: AtomicI64 = AtomicI64::new(0);
/// Player MTNK for reads and modifiers.
static UNIT_A: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// Player MTNK for writes.
static UNIT_B: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// Enemy HTNK.
static UNIT_E: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// The building for the `Online` write.
static BUILDING: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// The superweapon type granted to the player (raw type reference).
static SUPER: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// The modifier left to expire.
static EXPIRING: AtomicI64 = AtomicI64::new(NO_HANDLE);
/// `IniEntry` records seen right: bit 0 the first entry, bit 1 the second.
static ENTRIES: AtomicI64 = AtomicI64::new(0);

fn unit(slot: &AtomicI64) -> Techno {
    Techno(slot.load(Relaxed))
}

fn tank_type() -> Option<TypeRef> {
    TypeRef::techno_type("MTNK").ok()
}

/// Marks `name` when `ok`, otherwise logs a bench failure.
fn check(name: &str, ok: bool, why: &str) {
    if ok {
        mark(name);
    } else {
        bench_fail(name, why);
    }
}

/// `has_kind(table, id)`: 0 events, 1 actions, 2 properties, 3 name kinds,
/// 4 search scopes.
fn has_kind(table: i32, id: i32) -> bool {
    unsafe { sys::has_kind(table, id) != 0 }
}

/// A `find` the SDK's `Find` cannot build: any scope, an owner, a type and
/// up to two raw clauses. `None` unless the call answered `Ok`.
fn raw_find(
    scope: i32,
    owner: i64,
    type_ref: i64,
    clauses: &[sys::Clause],
    out: &mut [i64],
) -> Option<usize> {
    #[repr(C)]
    struct Buffer {
        query: sys::Query,
        clauses: [sys::Clause; 2],
    }
    let count = clauses.len().min(2);
    let mut buffer = Buffer {
        query: sys::Query {
            scope,
            owner,
            type_: type_ref,
            clause_count: count as i32,
            ..Default::default()
        },
        clauses: [sys::Clause::default(); 2],
    };
    buffer.clauses[..count].copy_from_slice(&clauses[..count]);
    let len = size_of::<sys::Query>() + size_of::<sys::Clause>() * count;
    let mut result = sys::FindResult::default();
    let status = unsafe {
        sys::find(
            (&raw const buffer).cast(),
            len as i32,
            out.as_mut_ptr(),
            out.len() as i32,
            &mut result,
        )
    };
    (status == 0).then_some(result.count as usize)
}

/// A clause on the candidate's property `prop`.
fn prop_clause(prop: Prop, op: Op, value: i64) -> sys::Clause {
    sys::Clause {
        op: op as u8,
        prop: prop.0,
        value,
        ..Default::default()
    }
}

/// A clause on declared variable `var` of the candidate (subject 3, scope
/// 1 Subject).
fn var_clause(var: i32, op: Op, value: i64) -> sys::Clause {
    sys::Clause {
        subject: 3,
        op: op as u8,
        scope: 1,
        prop: var,
        value,
        ..Default::default()
    }
}

// --------------------------------------------------------------- Exports

/// `vera_init`: the INI names (RFC 0001 section 6h).
pub fn init() {
    if ini::registry("Bench.Registry", KEY_REGISTRY).is_err() {
        bench_fail("basics.ini-entry", "ini::registry failed");
    }
    if ini::key("Bench.Key", IniSection::TechnoType, KEY_INI_KEY).is_err() {
        bench_fail("basics.ini-key", "ini::key failed");
    }
    match ini::event(EVENT_NAME, EventScope::Object) {
        Ok(event) => EVENT.set(event),
        Err(_) => bench_fail("basics.module-event", "ini::event failed"),
    }
}

/// `on_scenario_start`.
pub fn start() {}

/// The frame the arena opens: the houses and `crate::arena` are known.
pub fn open() {
    OPEN_AT.store(i64::from(vera20k_sdk::frame()), Relaxed);
    let home = arena(BASICS);
    if let Some(a) = spawn(home, "MTNK", player()) {
        UNIT_A.store(a.0, Relaxed);
    }
    if let Some(b) = spawn(offset(home, 2, 0), "MTNK", player()) {
        UNIT_B.store(b.0, Relaxed);
    }
    // The enemy tank stands away from ours, its weapons off.
    let spots = [(-7, -7), (-7, 0), (0, -7), (4, -6)];
    for (dx, dy) in spots {
        if let Some(e) = spawn(offset(home, dx, dy), "HTNK", enemy()) {
            UNIT_E.store(e.0, Relaxed);
            break;
        }
    }
    let e = unit(&UNIT_E);
    if !e.is_none() {
        let _ = Modifier::new(e, Prop::DisableWeapons, ModifierOp::Override, 1, -1, 0, 99);
    }
    // The module event's listener, on A.
    let a = unit(&UNIT_A);
    let event = EVENT.get();
    if event.0 >= 0 && !a.is_none() {
        if Trigger::on_module_event(event)
            .bind(a)
            .enqueue(KEY_EVENT)
            .done()
            .is_err()
        {
            bench_fail("basics.module-event", "its trigger could not be made");
        }
    }
    // A one-time superweapon for the player, for the `SuperWeapons` scope.
    for name in ["ParaDropSpecial", "AmerParaDropSpecial", "NukeSpecial"] {
        if let Ok(kind) = TypeRef::super_weapon(name) {
            if has_kind(1, 33) && Action::one_time_super_weapon(player(), kind).is_ok() {
                SUPER.store(kind.0, Relaxed);
            }
            break;
        }
    }
}

/// Every later frame: one check group per frame, then the polls.
pub fn frame(now: i32) {
    let step = i64::from(now) - OPEN_AT.load(Relaxed);
    match step {
        1 => host_calls(now),
        2 => names_rules(),
        3 => names_engine(),
        4 => object_reads(),
        5 => house_and_type_reads(),
        6 => cell_and_global_reads(now),
        7 => viewpoint_reads(),
        8 => revealed(),
        9 => variables(),
        10 => find_objects_and_houses(),
        11 => find_cells_and_supers(),
        12 => write_health_and_rank(),
        13 => write_owner_away(),
        14 => write_owner_back(),
        15 => write_mission(),
        17 => write_ammo(),
        18 => spawn_building(),
        19 => write_online(),
        20 => modifier_compose(),
        21 => modifier_group(),
        22 => modifier_props(),
        24 => stat(Prop::ArmorMultiplier, "basics.mod-armor"),
        25 => stat(Prop::SpeedMultiplier, "basics.mod-speed"),
        26 => stat(Prop::ROFMultiplier, "basics.mod-rof"),
        27 => stat(Prop::WeaponRangeMultiplier, "basics.mod-range"),
        28 => range_extra(),
        29 => phobos_stats(),
        30 => flag(Prop::Cloakable, "basics.mod-cloakable"),
        31 => flag(Prop::ForceDecloak, "basics.mod-decloak"),
        32 => immunity(),
        33 => stat(Prop::ROTMultiplier, "basics.mod-rot"),
        34 => modifier_expire_start(),
        40 => raise_event(),
        42 => modifier_expire_check(),
        45 => {
            let e = unit(&UNIT_E);
            if alive(e) {
                let _ = Action::remove(e);
            }
        }
        _ => {}
    }
    if (15..=60).contains(&step) && !marked("basics.set-mission") {
        mission_poll();
    }
    if step > 45 && step % 200 == 0 && !marked("basics.module-event") && step < 2000 {
        raise_event();
    }
    if step >= 60 && step % 30 == 0 && !marked("basics.factory") {
        factory_poll();
    }
    if step >= 100 && step % 50 == 0 && !marked("basics.placeable") {
        placeable_poll();
    }
}

/// Records whose key is in this area's range.
pub fn events(record: &EventRecord) {
    match record.key {
        KEY_REGISTRY => ini_entry(IniEntryRecord::from(record)),
        KEY_INI_KEY => ini_key(IniKeyRecord::from(record)),
        KEY_EVENT => module_event(ModuleEventRecord::from(record)),
        _ => {}
    }
}

/// Decisions whose key is in this area's range.
pub fn decide(_record: &mut DecideRecord) {}

// ------------------------------------------------------------ Host calls

fn host_calls(now: i32) {
    check(
        "basics.frame",
        vera20k_sdk::frame() == now && now > 0,
        "frame() differs",
    );
    let mut in_range = true;
    for _ in 0..8 {
        let n = random(3, 9);
        in_range &= (3..=9).contains(&n);
    }
    check("basics.random", in_range, "random left its range");
    log("Control Bench basics: log line");
    mark("basics.log");
    let yes = has_kind(0, Event::TAKE_DAMAGE)
        && has_kind(1, Action::CREATE_OBJECT)
        && has_kind(2, Prop::Health.0)
        && has_kind(2, Prop::FirepowerMultiplier.0)
        && has_kind(3, NameKind::TechnoType as i32)
        && has_kind(4, 7)
        && has_kind(4, 8);
    let no = !has_kind(4, 3) && !has_kind(3, 22) && !has_kind(1, 1999);
    check(
        "basics.has-kind",
        yes && no,
        "has_kind answered a kind wrongly",
    );
}

/// `lookup` then `name_of` of the first of `names` that resolves.
fn round_trip(kind: NameKind, names: &[&str], name: &str) {
    for candidate in names {
        let Ok(id) = lookup(kind, candidate) else {
            continue;
        };
        let mut buf = [0u8; 64];
        let back = name_of(kind, id, &mut buf);
        check(
            name,
            back.is_ok_and(|back| back.eq_ignore_ascii_case(candidate)),
            "name_of does not give the looked-up name back",
        );
        return;
    }
    bench_fail(name, "no candidate name resolves");
}

fn names_rules() {
    round_trip(
        NameKind::TechnoType,
        &["MTNK", "HTNK"],
        "basics.name-techno",
    );
    round_trip(
        NameKind::Weapon,
        &["105mm", "120mm", "M60"],
        "basics.name-weapon",
    );
    round_trip(
        NameKind::Warhead,
        &["AP", "HE", "SA"],
        "basics.name-warhead",
    );
    round_trip(
        NameKind::Projectile,
        &["Cannon", "Invisible", "InvisibleLow", "InvisibleHigh"],
        "basics.name-projectile",
    );
    round_trip(NameKind::Anim, &["TWLT070", "EXPLOSML"], "basics.name-anim");
}

fn names_engine() {
    round_trip(
        NameKind::VoxelAnim,
        &["TIRE", "PIECE", "PEBBLE"],
        "basics.name-voxelanim",
    );
    round_trip(
        NameKind::ParticleSystem,
        &["SparkSys", "FireStreamSys", "GasCloudSys"],
        "basics.name-particles",
    );
    round_trip(
        NameKind::SuperWeapon,
        &["NukeSpecial", "LightningStormSpecial", "ParaDropSpecial"],
        "basics.name-super",
    );
    // Properties and action kinds have fixed numbers.
    let health = lookup(NameKind::Property, "Health");
    let passable = lookup(NameKind::Property, "Passable.Foot");
    let mut buf = [0u8; 64];
    let back = name_of(NameKind::Property, Prop::ROTMultiplier.0, &mut buf)
        .is_ok_and(|n| n == "ROTMultiplier");
    check(
        "basics.name-property",
        health == Ok(Prop::Health.0) && passable == Ok(Prop::Passable_Foot.0) && back,
        "Property names and IDs differ",
    );
    let create = lookup(NameKind::ActionKind, "CreateObject");
    let back = name_of(NameKind::ActionKind, Action::LOG, &mut buf).is_ok_and(|n| n == "Log");
    check(
        "basics.name-action",
        create == Ok(Action::CREATE_OBJECT) && back,
        "ActionKind names and IDs differ",
    );
    // The module event this area registered.
    let event = EVENT.get();
    let id = lookup(NameKind::ModuleEvent, EVENT_NAME);
    let back = name_of(NameKind::ModuleEvent, event.0, &mut buf).is_ok_and(|n| n == EVENT_NAME);
    check(
        "basics.name-event",
        event.0 >= 0 && id == Ok(event.0) && back,
        "ModuleEvent lookup and name_of disagree",
    );
}

// ------------------------------------------------------------ Properties

fn object_reads() {
    let a = unit(&UNIT_A);
    let Some(mtnk) = tank_type().filter(|_| alive(a)) else {
        return bench_fail("basics.object-props", "unit A is missing");
    };
    let props = [
        Prop::Type.0,
        Prop::Owner.0,
        Prop::Category.0,
        Prop::Health.0,
        Prop::Strength.0,
        Prop::HealthPercent.0,
        Prop::X.0,
        Prop::Y.0,
        Prop::Z.0,
        Prop::Cell.0,
        Prop::Rank.0,
    ];
    let mut v = [0i64; 11];
    let read = get_many(&[a.0], &props, &mut v);
    let strength = get(mtnk, Prop::Strength).unwrap_or(-1);
    let cell = Cell(v[9]);
    check(
        "basics.object-props",
        read.is_ok()
            && v[0] == mtnk.0
            && v[1] == player().0
            && v[2] == Category::Vehicle as i64
            && v[3] > 0
            && v[4] == strength
            && (1..=100).contains(&v[5])
            && v[6] / 256 == i64::from(cell.x())
            && v[7] / 256 == i64::from(cell.y())
            && v[10] == 0,
        "unit A's basic properties read wrong",
    );
    let more = [
        Prop::Layer.0,
        Prop::Target.0,
        Prop::Experience.0,
        Prop::Storage.0,
        Prop::Transport.0,
        Prop::PassengerCount.0,
        Prop::TransportSpaceLeft.0,
        Prop::DisguiseType.0,
        Prop::DisguiseOwner.0,
        Prop::Moving.0,
        Prop::Facing.0,
        Prop::TurretFacing.0,
        Prop::Armor.0,
        Prop::Destination.0,
        Prop::WeaponReady.0,
        Prop::Deployed.0,
        Prop::UnderConstruction.0,
        Prop::Parasite.0,
    ];
    let mut m = [0i64; 18];
    let read = get_many(&[a.0], &more, &mut m);
    let armor = get(mtnk, Prop::Armor).unwrap_or(-1);
    check(
        "basics.object-more",
        read.is_ok()
            && m[4] == NO_HANDLE
            && m[5] == 0
            && m[7] == NO_HANDLE
            && m[8] == NO_HANDLE
            && (0..=255).contains(&m[10])
            && (0..=255).contains(&m[11])
            && m[12] == armor
            && m[16] == 0
            && m[17] == NO_HANDLE,
        "unit A's 7.1 properties read wrong",
    );
    // Two objects, two properties, row-major.
    let b = unit(&UNIT_B);
    let mut two = [0i64; 4];
    let read = get_many(&[a.0, b.0], &[Prop::Owner.0, Prop::Health.0], &mut two);
    check(
        "basics.get-many",
        read.is_ok()
            && two[0] == player().0
            && two[2] == player().0
            && Ok(two[1]) == a.health()
            && Ok(two[3]) == b.health(),
        "get_many differs from get",
    );
}

fn house_and_type_reads() {
    let props = [
        Prop::Index.0,
        Prop::Money.0,
        Prop::PowerOutput.0,
        Prop::PowerDrain.0,
        Prop::LowPower.0,
        Prop::IsDefeated.0,
        Prop::IsHuman.0,
        Prop::TechLevel.0,
        Prop::Country.0,
        Prop::ControlDomains.0,
    ];
    let mut v = [0i64; 20];
    let read = get_many(&[player().0, enemy().0], &props, &mut v);
    check(
        "basics.house-props",
        read.is_ok() && v[6] == 1 && v[16] == 0 && v[0] != v[10] && v[5] == 0 && v[15] == 0,
        "house properties read wrong",
    );
    // The player's country: a Country type reference, named and looked up.
    let country = TypeRef(v[8]).id();
    let mut buf = [0u8; 64];
    match name_of(NameKind::Country, country, &mut buf) {
        Ok(name) => check(
            "basics.name-country",
            lookup(NameKind::Country, name) == Ok(country),
            "Country lookup differs from name_of",
        ),
        Err(_) => bench_fail("basics.name-country", "the house's country has no name"),
    }
    // 7.7: TechnoType, Weapon and Warhead keys.
    if let Some(mtnk) = tank_type() {
        let keys = [
            Prop::Category.0,
            Prop::Strength.0,
            Prop::Cost.0,
            Prop::Soylent.0,
            Prop::Speed.0,
            Prop::ROT.0,
            Prop::Crushable.0,
            Prop::Selfhealing.0,
            Prop::Explodes.0,
            Prop::Power.0,
            Prop::Sight.0,
            Prop::Ammo.0,
            Prop::InitialAmmo.0,
            Prop::Storage.0,
            Prop::Passengers.0,
            Prop::SizeLimit.0,
            Prop::TechLevel.0,
            Prop::Armor.0,
        ];
        let mut t = [0i64; 18];
        let read = get_many(&[mtnk.0], &keys, &mut t);
        check(
            "basics.type-props",
            read.is_ok() && t[0] == Category::Vehicle as i64 && t[1] > 0 && t[2] > 0 && t[4] > 0,
            "MTNK's type keys read wrong",
        );
    }
    for name in ["105mm", "120mm", "M60"] {
        let Ok(weapon) = TypeRef::weapon(name) else {
            continue;
        };
        let keys = [
            Prop::Damage.0,
            Prop::ROF.0,
            Prop::Burst.0,
            Prop::MinimumRange.0,
            Prop::Range.0,
        ];
        let mut w = [0i64; 5];
        let read = get_many(&[weapon.0], &keys, &mut w);
        check(
            "basics.weapon-props",
            read.is_ok() && w[0] > 0 && w[4] > 0,
            "the weapon's keys read wrong",
        );
        break;
    }
    for name in ["AP", "HE", "SA"] {
        let Ok(warhead) = TypeRef::warhead(name) else {
            continue;
        };
        let keys = [
            Prop::CellSpread.0,
            Prop::PercentAtMax.0,
            Prop::Wall.0,
            Prop::Wood.0,
            Prop::Sparky.0,
            Prop::Fire.0,
            Prop::Conventional.0,
            Prop::Rocker.0,
            Prop::InfDeath.0,
        ];
        let mut w = [0i64; 9];
        let read = get_many(&[warhead.0], &keys, &mut w);
        check(
            "basics.warhead-props",
            read.is_ok() && w[0] >= 0,
            "the warhead's keys read wrong",
        );
        break;
    }
}

fn cell_and_global_reads(now: i32) {
    let a = unit(&UNIT_A);
    let cell = match get(a, Prop::Cell) {
        Ok(cell) => Cell(cell),
        Err(_) => arena(BASICS),
    };
    let props = [
        Prop::LandType.0,
        Prop::Height.0,
        Prop::Overlay.0,
        Prop::Ore.0,
        Prop::Waypoint.0,
        Prop::CellTag.0,
        Prop::Passable_Foot.0,
        Prop::Passable_Track.0,
        Prop::Passable_Wheel.0,
        Prop::Passable_Hover.0,
        Prop::Passable_Winged.0,
        Prop::Passable_Float.0,
        Prop::Passable_Amphibious.0,
        Prop::Passable_FloatBeach.0,
        Prop::Obstructed.0,
    ];
    let mut v = [0i64; 15];
    let read = get_many(&[cell.0], &props, &mut v);
    // A tank stands there: tracks pass.
    check(
        "basics.cell-props",
        read.is_ok() && v[7] == 1 && v[3] >= 0,
        "the tank's cell reads wrong",
    );
    let start = Waypoint::number(0);
    check(
        "basics.waypoint-cell",
        get(start, Prop::LandType).is_ok() && get(start, Prop::Waypoint) == Ok(0),
        "waypoint 0 does not read as its cell",
    );
    let globals = [
        Prop::Frame.0,
        Prop::PlayerCount.0,
        Prop::GameSpeed.0,
        Prop::Difficulty.0,
        Prop::MissionTimer.0,
        Prop::MissionTimerRunning.0,
        Prop::AmbientLight.0,
        Prop::IonStormActive.0,
    ];
    let mut g = [0i64; 8];
    let read = get_many(&[Global.raw()], &globals, &mut g);
    check(
        "basics.global-props",
        read.is_ok()
            && g[0] == i64::from(now)
            && g[1] >= 1
            && (0..=2).contains(&g[3])
            && Global::frame() == Ok(i64::from(now)),
        "global properties read wrong",
    );
}

fn viewpoint_reads() {
    let (a, e) = (unit(&UNIT_A), unit(&UNIT_E));
    let Some(mtnk) = tank_type() else {
        return;
    };
    let home = arena(BASICS);
    let mut out = [NO_HANDLE; 8];
    // The player sees its own house as itself and the enemy's tank as an
    // enemy.
    let me = Find::houses()
        .seen_by(player())
        .where_prop(Prop::Relation, Op::Eq, Relation::SameHouse as i64)
        .page(&mut out, 0);
    let own = me.is_ok_and(|(n, _)| n == 1) && out[0] == player().0;
    let hostile = TypeRef::techno_type("HTNK").ok().is_some_and(|htnk| {
        let found = Find::objects()
            .owned_by(enemy())
            .of_type(htnk)
            .seen_by(player())
            .where_prop(Prop::Relation, Op::Eq, Relation::Enemy as i64)
            .page(&mut out, 0);
        found.is_ok_and(|(n, _)| out[..n].contains(&e.0))
    });
    if alive(e) {
        check("basics.relation", own && hostile, "Relation read wrong");
    }
    // The player sees its own tank.
    let found = Find::objects()
        .owned_by(player())
        .of_type(mtnk)
        .near(home, 8 * 256)
        .seen_by(player())
        .where_prop(Prop::Visible, Op::Eq, 1)
        .page(&mut out, 0);
    check(
        "basics.visible",
        found.is_ok_and(|(n, _)| out[..n].contains(&a.0)),
        "the player does not see its own tank",
    );
    // Engage with A as the shooter: any EngageResult reads.
    if alive(a) && alive(e) {
        let found = Find::objects()
            .owned_by(enemy())
            .near(home, 20 * 256)
            .seen_by(House(a.0))
            .where_prop(Prop::Engage, Op::Ge, 0)
            .page(&mut out, 0);
        check(
            "basics.engage",
            found.is_ok_and(|(n, _)| out[..n].contains(&e.0)),
            "Engage did not read for the enemy tank",
        );
        check(
            "basics.mod-noweapons",
            get(e, Prop::DisableWeapons) == Ok(1),
            "DisableWeapons does not read back",
        );
    }
}

fn revealed() {
    let a = unit(&UNIT_A);
    let Ok(cell) = get(a, Prop::Cell) else {
        return;
    };
    let mut out = [NO_HANDLE; 16];
    let found = Find::cells()
        .near(Cell(cell), 512)
        .seen_by(player())
        .where_prop(Prop::Revealed, Op::Eq, 1)
        .page(&mut out, 0);
    check(
        "basics.revealed",
        found.is_ok_and(|(n, _)| out[..n].contains(&cell)),
        "the tank's own cell is not revealed",
    );
}

/// The enemy's factories, once it builds something.
fn factory_poll() {
    let props = [
        Prop::Factory_Building_Type.0,
        Prop::Factory_Building_Progress.0,
        Prop::Factory_Building_Ready.0,
        Prop::Factory_Building_Queued.0,
        Prop::Factory_Infantry_Type.0,
        Prop::Factory_Vehicle_Type.0,
    ];
    let mut v = [0i64; 6];
    let read = get_many(&[enemy().0], &props, &mut v);
    if read.is_err() {
        return bench_fail("basics.factory", "Factory properties do not read");
    }
    if v[0] != NO_HANDLE {
        check(
            "basics.factory",
            (0..=100).contains(&v[1]) && v[3] >= 1,
            "the enemy's building factory reads wrong",
        );
    }
}

/// `Placeable` for the enemy around its first building, with a power
/// plant's footprint.
fn placeable_poll() {
    let first = Find::objects()
        .owned_by(enemy())
        .where_prop(Prop::Category, Op::Eq, Category::Building as i64)
        .first();
    let Ok(cell) = get(Techno(first), Prop::Cell) else {
        return;
    };
    let Some(plant) = ["GAPOWR", "NAPOWR"]
        .into_iter()
        .find_map(|name| TypeRef::techno_type(name).ok())
    else {
        return;
    };
    let mut out = [NO_HANDLE; 4];
    let found = Find::cells()
        .within(crate::offset(Cell(cell), -8, -8), 17, 17)
        .of_type(plant)
        .seen_by(enemy())
        .where_prop(Prop::Placeable, Op::Eq, 1)
        .page(&mut out, 0);
    if found.is_ok_and(|(n, _)| n > 0) {
        mark("basics.placeable");
    }
}

// ------------------------------------------------------------- Variables

fn variables() {
    let a = unit(&UNIT_A);
    check(
        "basics.var-defaults",
        BENCH_WAVE.get() == Ok(7)
            && BENCH_SCORE.get(player()) == Ok(3)
            && BENCH_TAG.get(a) == Ok(5),
        "declared defaults read wrong",
    );
    let global = BENCH_WAVE.set(11).is_ok() && BENCH_WAVE.get() == Ok(11);
    check(
        "basics.var-global",
        global,
        "the global variable does not read back",
    );
    let house = BENCH_SCORE.set(enemy(), 9).is_ok()
        && BENCH_SCORE.get(enemy()) == Ok(9)
        && BENCH_SCORE.get(player()) == Ok(3);
    check(
        "basics.var-house",
        house,
        "the house variable does not read back",
    );
    let object = BENCH_TAG.set(a, 42).is_ok()
        && BENCH_TAG.get(a) == Ok(42)
        && BENCH_TAG.get(unit(&UNIT_B)) == Ok(5);
    check(
        "basics.var-object",
        object,
        "the object variable does not read back",
    );
    // `mod.` names this module's own declarations; raw `var_get` agrees.
    let plain = lookup(NameKind::Variable, "BenchWave");
    let prefixed = lookup(NameKind::Variable, "mod.BenchWave");
    let mut raw = 0;
    let raw_ok = plain.is_ok_and(|id| unsafe { sys::var_get(0, id, &mut raw) } == 0 && raw == 11);
    check(
        "basics.var-lookup",
        plain.is_ok_and(|id| id >= 1000) && plain == prefixed && raw_ok,
        "lookup(Variable) with mod. differs",
    );
    if let Ok(id) = plain {
        let mut buf = [0u8; 32];
        check(
            "basics.name-variable",
            name_of(NameKind::Variable, id, &mut buf).is_ok_and(|n| n == "BenchWave"),
            "name_of(Variable) differs",
        );
    }
}

// ---------------------------------------------------------------- Search

fn find_objects_and_houses() {
    let (a, b) = (unit(&UNIT_A), unit(&UNIT_B));
    let mut out = [NO_HANDLE; 16];
    // A variable clause on the candidate: only A holds 42.
    if let Ok(tag) = lookup(NameKind::Variable, "BenchTag") {
        let found = raw_find(0, player().0, 0, &[var_clause(tag, Op::Eq, 42)], &mut out);
        check(
            "basics.find-var",
            found == Some(1) && out[0] == a.0,
            "a variable clause found the wrong objects",
        );
    }
    if let Some(mtnk) = tank_type() {
        let found = Find::objects()
            .owned_by(player())
            .of_type(mtnk)
            .where_prop(Prop::Health, Op::Gt, 0)
            .page(&mut out, 0);
        check(
            "basics.find-objects",
            found.is_ok_and(|(n, _)| out[..n].contains(&a.0) && out[..n].contains(&b.0)),
            "owned_by/of_type/where_prop missed our tanks",
        );
    }
    let home = get(a, Prop::Cell).map_or(arena(BASICS), Cell);
    let circle = Find::objects()
        .owned_by(player())
        .near(home, 4 * 256)
        .page(&mut out, 0)
        .is_ok_and(|(n, _)| out[..n].contains(&a.0));
    let rect = Find::objects()
        .owned_by(player())
        .within(offset(home, -3, -3), 7, 7)
        .page(&mut out, 0)
        .is_ok_and(|(n, _)| out[..n].contains(&a.0));
    check(
        "basics.find-objects-area",
        circle && rect,
        "a circle or rectangle missed tank A",
    );
    let humans = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 1)
        .page(&mut out, 0);
    let human = humans.is_ok_and(|(n, _)| n == 1) && out[0] == player().0;
    let all = Find::houses()
        .page(&mut out, 0)
        .is_ok_and(|(n, _)| out[..n].contains(&player().0) && out[..n].contains(&enemy().0));
    check(
        "basics.find-houses",
        human && all,
        "the house search is wrong",
    );
}

fn find_cells_and_supers() {
    let home = get(unit(&UNIT_A), Prop::Cell).map_or(arena(BASICS), Cell);
    let mut one = [NO_HANDLE; 1];
    check(
        "basics.find-cells-map",
        Find::cells().page(&mut one, 0).is_ok_and(|(n, _)| n == 1) && one[0] != NO_HANDLE,
        "the map has no first cell",
    );
    // Within one cell of the centre: the cell and its four neighbours.
    let mut out = [NO_HANDLE; 16];
    let circle = Find::cells().near(home, 256).page(&mut out, 0);
    let near = |c: &i64| {
        let c = Cell(*c);
        let dx = i32::from(c.x()) - i32::from(home.x());
        let dy = i32::from(c.y()) - i32::from(home.y());
        dx * dx + dy * dy <= 1
    };
    check(
        "basics.find-cells-circle",
        circle.is_ok_and(|(n, _)| {
            (1..=5).contains(&n) && out[..n].iter().all(near) && out[..n].contains(&home.0)
        }),
        "the circle of cells is wrong",
    );
    let rect = Find::cells().within(home, 3, 3).page(&mut out, 0);
    let inside = |c: &i64| {
        let c = Cell(*c);
        (home.x()..home.x() + 3).contains(&c.x()) && (home.y()..home.y() + 3).contains(&c.y())
    };
    check(
        "basics.find-cells-rect",
        rect.is_ok_and(|(n, _)| (1..=9).contains(&n) && out[..n].iter().all(inside)),
        "the rectangle of cells is wrong",
    );
    let kind = SUPER.load(Relaxed);
    if kind == NO_HANDLE {
        return bench_fail("basics.find-supers", "no superweapon could be granted");
    }
    let found = raw_find(8, player().0, kind, &[], &mut out);
    let owner = if found == Some(1) {
        get(Techno(out[0]), Prop::Owner)
    } else {
        Err(Status::NotFound)
    };
    let mut v = [0i64; 3];
    let read = found == Some(1)
        && get_many(
            &[out[0]],
            &[Prop::Type.0, Prop::Ready.0, Prop::ChargePercent.0],
            &mut v,
        )
        .is_ok();
    check(
        "basics.find-supers",
        read && owner == Ok(player().0) && (0..=100).contains(&v[2]),
        "the granted superweapon was not found",
    );
}

// ---------------------------------------------------------------- Writes

fn write_health_and_rank() {
    let b = unit(&UNIT_B);
    let Ok(strength) = get(b, Prop::Strength) else {
        return bench_fail("basics.set-health", "unit B is missing");
    };
    let half = (strength / 2).max(1);
    let wrote = b.set_health(half).is_ok() && b.health() == Ok(half);
    let zero = b.set_health(0) == Err(Status::Refused);
    let _ = b.set_health(strength);
    check(
        "basics.set-health",
        wrote && zero,
        "Health does not read back",
    );
    let elite = set(b, Prop::Rank, 2).is_ok() && get(b, Prop::Rank) == Ok(2);
    let rookie = set(b, Prop::Rank, 0).is_ok() && get(b, Prop::Rank) == Ok(0);
    check(
        "basics.set-rank",
        elite && rookie,
        "Rank does not read back",
    );
}

fn write_owner_away() {
    let b = unit(&UNIT_B);
    if b.set_owner(enemy()).is_err() || b.owner() != Ok(enemy()) {
        bench_fail("basics.set-owner", "Owner does not read back");
    }
}

fn write_owner_back() {
    let b = unit(&UNIT_B);
    let was_enemy = b.owner() == Ok(enemy());
    let back = b.set_owner(player()).is_ok() && b.owner() == Ok(player());
    check(
        "basics.set-owner",
        was_enemy && back,
        "Owner does not read back",
    );
}

fn write_mission() {
    let b = unit(&UNIT_B);
    if alive(b) && b.set_mission(Mission::AreaGuard).is_err() {
        bench_fail("basics.set-mission", "the Mission write failed");
    }
}

fn mission_poll() {
    if unit(&UNIT_B).mission() == Ok(Mission::AreaGuard as i64) {
        mark("basics.set-mission");
    }
}

/// A unit with ammo (whichever candidate type has `Ammo=`), emptied and
/// refilled, then removed.
fn write_ammo() {
    let spot = offset(arena(BASICS), 0, 4);
    for name in ["V3", "ORCA", "BEAG"] {
        let Ok(kind) = TypeRef::techno_type(name) else {
            continue;
        };
        if get(kind, Prop::Ammo).unwrap_or(0) <= 0 {
            continue;
        }
        let Some(armed) = spawn(spot, name, player()) else {
            continue;
        };
        let empty = armed.set_ammo(0).is_ok() && armed.ammo() == Ok(0);
        let one = armed.set_ammo(1).is_ok() && armed.ammo() == Ok(1);
        let _ = Action::remove(armed);
        return check("basics.set-ammo", empty && one, "Ammo does not read back");
    }
    bench_fail("basics.set-ammo", "no unit with ammo could be made");
}

/// A powered building of the player's (a type that drains power), for the
/// `Online` write next frame.
fn spawn_building() {
    let spot = offset(arena(BASICS), 4, 4);
    for name in ["TESLA", "ATESLA", "NASAM", "GAPILE", "NAHAND"] {
        let Ok(kind) = TypeRef::techno_type(name) else {
            continue;
        };
        if get(kind, Prop::Power).unwrap_or(0) >= 0 {
            continue;
        }
        if let Some(building) = spawn(spot, name, player()) {
            BUILDING.store(building.0, Relaxed);
            return;
        }
    }
    bench_fail("basics.set-online", "no powered building could be made");
}

fn write_online() {
    let building = unit(&BUILDING);
    if building.is_none() {
        return;
    }
    let off = building.set_online(false).is_ok() && get(building, Prop::Online) == Ok(0);
    let on = building.set_online(true).is_ok() && get(building, Prop::Online) == Ok(1);
    // A tank has no Online.
    let tank = get(unit(&UNIT_A), Prop::Online) == Err(Status::Unsupported);
    let _ = Action::remove(building);
    check(
        "basics.set-online",
        off && on && tank,
        "Online does not read back",
    );
}

// ------------------------------------------------------------- Modifiers

const ONE: i64 = FIXED_ONE;
const HALF: i64 = FIXED_ONE / 2;
const TWO: i64 = FIXED_ONE * 2;
const THREE: i64 = FIXED_ONE * 3;
const QUARTER: i64 = FIXED_ONE / 4;

/// Add, Multiply and Override on `FirepowerMultiplier` of A: (1 + 0.5) * 2
/// = 3, the override 0.25 replaces it, and removing each restores.
fn modifier_compose() {
    let a = unit(&UNIT_A);
    let fp = Prop::FirepowerMultiplier;
    let base = get(a, fp);
    let add = Modifier::add(a, fp, HALF, 300);
    let after_add = get(a, fp);
    let mul = Modifier::multiply(a, fp, TWO, 300);
    let after_mul = get(a, fp);
    let over = Modifier::override_(a, fp, QUARTER, 300);
    let after_over = get(a, fp);
    let over_gone = over.is_ok_and(|m| m.remove().is_ok()) && get(a, fp) == Ok(THREE);
    let rest_gone = add.is_ok_and(|m| m.remove().is_ok()) && mul.is_ok_and(|m| m.remove().is_ok());
    let restored = get(a, fp) == base;
    let composed = base == Ok(ONE)
        && after_add == Ok(ONE + HALF)
        && after_mul == Ok(THREE)
        && after_over == Ok(QUARTER);
    check(
        "basics.mod-compose",
        composed && over_gone && rest_gone && restored,
        "Add/Multiply/Override compose wrong",
    );
    check(
        "basics.mod-firepower",
        after_mul == Ok(THREE) && restored,
        "FirepowerMultiplier does not read back",
    );
}

/// Two modifiers of one group on A: the second replaces the first; then
/// `unmodify` removes it, and a second `unmodify` answers `Gone`.
fn modifier_group() {
    let a = unit(&UNIT_A);
    let fp = Prop::FirepowerMultiplier;
    let first = Modifier::new(a, fp, ModifierOp::Multiply, TWO, -1, 0, 9);
    let second = Modifier::new(a, fp, ModifierOp::Multiply, THREE, -1, 0, 9);
    let (Ok(first), Ok(second)) = (first, second) else {
        return bench_fail("basics.mod-group", "modify failed");
    };
    let replaced = get(first, Prop::Value) == Err(Status::Gone)
        && get(second, Prop::Value) == Ok(THREE)
        && get(a, fp) == Ok(THREE);
    check("basics.mod-group", replaced, "the group did not replace");
    let removed = second.remove().is_ok()
        && second.remove() == Err(Status::Gone)
        && get(second, Prop::Value) == Err(Status::Gone)
        && get(a, fp) == Ok(ONE);
    check("basics.mod-unmodify", removed, "unmodify did not remove");
}

/// The 7.3 properties of a modifier with a source and a group, and the
/// `Modifiers` scope.
fn modifier_props() {
    let (a, b) = (unit(&UNIT_A), unit(&UNIT_B));
    let Ok(m) = Modifier::new(
        a,
        Prop::ArmorMultiplier,
        ModifierOp::Multiply,
        ONE + HALF,
        100,
        b.0,
        5,
    ) else {
        return bench_fail("basics.mod-props", "modify failed");
    };
    let props = [
        Prop::Target.0,
        Prop::Property.0,
        Prop::Op.0,
        Prop::Value.0,
        Prop::Remaining.0,
        Prop::Source.0,
        Prop::Group.0,
        Prop::Origin.0,
    ];
    let mut v = [0i64; 8];
    let read = get_many(&[m.0], &props, &mut v);
    check(
        "basics.mod-props",
        read.is_ok()
            && v[0] == a.0
            && v[1] == i64::from(Prop::ArmorMultiplier.0)
            && v[2] == ModifierOp::Multiply as i64
            && v[3] == ONE + HALF
            && (99..=100).contains(&v[4])
            && v[5] == b.0
            && v[6] == 5,
        "the modifier's properties read wrong",
    );
    let mut out = [NO_HANDLE; 16];
    let found = raw_find(7, 0, 0, &[prop_clause(Prop::Target, Op::Eq, a.0)], &mut out);
    check(
        "basics.find-modifiers",
        found.is_some_and(|n| out[..n].contains(&m.0)),
        "the Modifiers scope missed the modifier",
    );
    let _ = m.remove();
}

/// A 5-frame `SpeedMultiplier` on A, checked after it ran out.
fn modifier_expire_start() {
    let a = unit(&UNIT_A);
    match Modifier::multiply(a, Prop::SpeedMultiplier, HALF, 5) {
        Ok(m) if get(a, Prop::SpeedMultiplier) == Ok(HALF) => EXPIRING.store(m.0, Relaxed),
        _ => bench_fail("basics.mod-expire", "the timed modifier did not apply"),
    }
}

fn modifier_expire_check() {
    let m = Modifier(EXPIRING.load(Relaxed));
    if m.is_none() {
        return;
    }
    check(
        "basics.mod-expire",
        get(m, Prop::Remaining) == Err(Status::Gone)
            && get(unit(&UNIT_A), Prop::SpeedMultiplier) == Ok(ONE),
        "the timed modifier did not expire",
    );
}

/// A factor stat: doubled while the modifier lasts, back after `unmodify`.
fn stat(prop: Prop, name: &str) {
    let a = unit(&UNIT_A);
    let Ok(base) = get(a, prop) else {
        return bench_fail(name, "the stat does not read");
    };
    let Ok(m) = Modifier::multiply(a, prop, TWO, 60) else {
        return bench_fail(name, "modify failed");
    };
    let doubled = get(a, prop);
    let removed = m.remove().is_ok();
    let ok = doubled == Ok(base * 2) && removed && get(a, prop) == Ok(base);
    if !ok {
        Line::new()
            .text("basics: ")
            .text(name)
            .text(" base ")
            .num(base)
            .text(" doubled ")
            .num(doubled.unwrap_or(-1))
            .log();
    }
    check(name, ok, "the stat does not read back");
}

/// A flag stat: an override of 1 reads 1.
fn flag(prop: Prop, name: &str) {
    let a = unit(&UNIT_A);
    let Ok(m) = Modifier::override_(a, prop, 1, 60) else {
        return bench_fail(name, "modify failed");
    };
    let set = get(a, prop) == Ok(1);
    let removed = m.remove().is_ok();
    check(name, set && removed, "the flag does not read back");
}

fn range_extra() {
    let a = unit(&UNIT_A);
    let extra = Prop::WeaponRangeExtra;
    let Ok(base) = get(a, extra) else {
        return bench_fail("basics.mod-range-extra", "the stat does not read");
    };
    let Ok(m) = Modifier::add(a, extra, 256, 60) else {
        return bench_fail("basics.mod-range-extra", "modify failed");
    };
    let added = get(a, extra) == Ok(base + 256);
    let removed = m.remove().is_ok() && get(a, extra) == Ok(base);
    check(
        "basics.mod-range-extra",
        added && removed,
        "the extra range does not read back",
    );
}

/// `CritMultiplier`, `CritExtraChance` and `Unkillable`: modifiers in a
/// Phobos build, `Unsupported` (and `has_kind` 0) without one.
fn phobos_stats() {
    let a = unit(&UNIT_A);
    let crit = Prop::CritMultiplier;
    if has_kind(2, crit.0) {
        stat(crit, "basics.mod-crit");
        let extra = Prop::CritExtraChance;
        if let Ok(m) = Modifier::add(a, extra, QUARTER, 60) {
            if get(a, extra) != Ok(QUARTER) {
                bench_fail("basics.mod-crit", "CritExtraChance does not read back");
            }
            let _ = m.remove();
        }
    } else {
        let refused = Modifier::multiply(a, crit, TWO, 60).err() == Some(Status::Unsupported)
            && Modifier::add(a, Prop::CritExtraChance, QUARTER, 60).err()
                == Some(Status::Unsupported);
        check(
            "basics.mod-crit",
            refused,
            "crit without Phobos answered wrongly",
        );
    }
    if has_kind(2, Prop::Unkillable.0) {
        flag(Prop::Unkillable, "basics.mod-unkillable");
    } else {
        let refused =
            Modifier::override_(a, Prop::Unkillable, 1, 60).err() == Some(Status::Unsupported);
        check(
            "basics.mod-unkillable",
            refused,
            "Unkillable without Phobos answered wrongly",
        );
    }
}

/// `ImmunityFlags`: an override mask reads back as itself; `Add` is not
/// taken.
fn immunity() {
    let a = unit(&UNIT_A);
    let prop = Prop::ImmunityFlags;
    let Ok(base) = get(a, prop) else {
        return bench_fail("basics.mod-immunity", "ImmunityFlags does not read");
    };
    let Ok(m) = Modifier::override_(a, prop, 0x55, 60) else {
        return bench_fail("basics.mod-immunity", "modify failed");
    };
    let set = get(a, prop) == Ok(0x55);
    let add = Modifier::add(a, prop, 1, 60).err() == Some(Status::Refused);
    let removed = m.remove().is_ok() && get(a, prop) == Ok(base);
    check(
        "basics.mod-immunity",
        set && add && removed,
        "ImmunityFlags does not read back",
    );
}

// ---------------------------------------------------------------- INI

/// `[Bench.Registry]`: `BenchAlpha` (Value=11) then `BenchBeta` (22).
fn ini_entry(record: IniEntryRecord) {
    let mut name = [0u8; 32];
    let Ok(section) = record.entry_name(&mut name) else {
        return bench_fail("basics.ini-entry", "the entry has no name");
    };
    let (expected_name, expected_value, bit) = match record.position() {
        0 => ("BenchAlpha", 11, 1),
        1 => ("BenchBeta", 22, 2),
        _ => return bench_fail("basics.ini-entry", "an unexpected entry"),
    };
    let mut value = [0u8; 16];
    let read = Rules::get(section, "Value", &mut value)
        .ok()
        .and_then(ini::int);
    if section != expected_name || read != Some(expected_value) {
        return bench_fail("basics.ini-entry", "the entry or its Value reads wrong");
    }
    // The entry name is an engine string: name_of and lookup agree.
    let id = record.record().args;
    let mut text = [0u8; 32];
    check(
        "basics.name-string",
        vera20k_sdk::text(id, &mut text).is_ok_and(|t| t == expected_name)
            && lookup(NameKind::String, expected_name) == Ok(id),
        "String name_of and lookup disagree",
    );
    if ENTRIES.fetch_or(bit, Relaxed) | bit == 3 {
        mark("basics.ini-entry");
    }
}

/// `Bench.Key=Bench,7` on `[BFRT]`.
fn ini_key(record: IniKeyRecord) {
    let mut value = [0u8; 32];
    let text = record.value(&mut value);
    let section = TypeRef::techno_type(KEY_SECTION);
    let mut name = [0u8; 32];
    let named = record
        .section_name(&mut name)
        .is_ok_and(|n| n.eq_ignore_ascii_case(KEY_SECTION));
    check(
        "basics.ini-key",
        text == Ok(KEY_VALUE)
            && !record.is_general()
            && section.is_ok_and(|s| s.0 == record.type_ref().0)
            && named,
        "the IniKey record reads wrong",
    );
}

fn raise_event() {
    let a = unit(&UNIT_A);
    let event = EVENT.get();
    if event.0 < 0 || !alive(a) {
        return;
    }
    if Action::raise_event(event, a, Techno(NO_HANDLE), 77, 88).is_err() {
        bench_fail("basics.module-event", "RaiseEvent failed");
    }
}

fn module_event(record: ModuleEventRecord) {
    check(
        "basics.module-event",
        record.event() == EVENT.get()
            && record.subject() == unit(&UNIT_A)
            && record.value() == 77
            && record.value2() == 88,
        "the module event's record reads wrong",
    );
}
