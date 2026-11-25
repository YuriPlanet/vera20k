//! Module flight on a retail map (RFC 0001 section 9): the decidable `Fire`
//! (an empty plan launches nothing, a doubled native entry hits twice),
//! `ProjectileFrame` (a step moves the bullet and keeps its module data) and
//! `Launch` (a path steers the bullet and ends in an impact), plus the
//! path arithmetic.

use vera20k_wasm_ext::NAME_PROJECTILE;
use vera20k_wasm_ext::decide::{EVENT_LAUNCH, EVENT_PROJECTILE_FRAME, ProjectileStep};
use vera20k_wasm_ext::triggers::EVENT_FIRE;

use super::ModuleSource;
use super::projectiles::{MAX_STEP, tests_support};
use crate::headless_scenario::HeadlessScenario;

/// Handler keys: answer an empty plan; double the native plan entry; a
/// `ProjectileStep` setting velocity (0, 0, 64) and `Data0` 0x1234; a
/// `Waypoints` path to 1024 leptons east and 512 up of the bullet.
const EMPTY: i32 = 1;
const DOUBLE: i32 = 2;
const STEP: i32 = 3;
const PATH: i32 = 4;

/// A `TriggerDef`: house 0, repeating, every difficulty, decision handler
/// `key`, one event of `kind`, no actions.
fn decide_def(kind: i32, key: i32) -> Vec<u8> {
    let mut out = 0i64.to_le_bytes().to_vec();
    for value in [2, 7, 0, key, 1, 0, kind, 0, 0, -1] {
        out.extend(value.to_le_bytes());
    }
    out.extend(0i64.to_le_bytes());
    out.extend([0; 8]);
    out
}

/// Creates the trigger in `on_scenario_start` and binds it to `target`
/// (statuses at 72 and 76). `on_decide` copies its record to 512, counts
/// requests at 696 and answers by key. Its delivery area is 1024 bytes at
/// 1024: a record and the largest result area.
fn module(kind: i32, key: i32, target: i64) -> Vec<u8> {
    let def = decide_def(kind, key);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (memory (export "memory") 1)
  (data (i32.const 3072) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 3072) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (local $key i32) (local $area i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
    (local.set $key (i32.load (local.get $ptr)))
    (local.set $area (i32.load offset=148 (local.get $ptr)))
    (if (i32.eq (local.get $key) (i32.const {EMPTY}))
      (then (i64.store offset=136 (local.get $ptr) (i64.const 0))))
    (if (i32.eq (local.get $key) (i32.const {DOUBLE}))
      (then
        (memory.copy (i32.add (local.get $area) (i32.const 48)) (local.get $area) (i32.const 48))
        (i64.store offset=136 (local.get $ptr) (i64.const 2))))
    (if (i32.eq (local.get $key) (i32.const {STEP}))
      (then
        (i32.store (local.get $area) (i32.const 10))
        (i32.store offset=8 (local.get $area) (i32.const 0))
        (i32.store offset=12 (local.get $area) (i32.const 0))
        (i32.store offset=16 (local.get $area) (i32.const 4194304))
        (i64.store offset=40 (local.get $area) (i64.const 0x1234))
        (i64.store offset=136 (local.get $ptr) (i64.const 0))))
    (if (i32.eq (local.get $key) (i32.const {PATH}))
      (then
        (drop (call $get (i64.load offset=48 (local.get $ptr)) (i32.const 6) (i32.const 800)))
        (drop (call $get (i64.load offset=48 (local.get $ptr)) (i32.const 7) (i32.const 808)))
        (drop (call $get (i64.load offset=48 (local.get $ptr)) (i32.const 8) (i32.const 816)))
        (i32.store (local.get $area) (i32.const 3))
        (i32.store offset=4 (local.get $area) (i32.const 1))
        (i64.store offset=8 (local.get $area) (i64.const 8388608))
        (i64.store offset=32 (local.get $area) (i64.add (i64.load (i32.const 800)) (i64.const 1024)))
        (i64.store offset=40 (local.get $area) (i64.load (i32.const 808)))
        (i64.store offset=48 (local.get $area) (i64.add (i64.load (i32.const 816)) (i64.const 512)))
        (i64.store offset=136 (local.get $ptr) (i64.const 56))))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#,
        len = def.len(),
    ))
    .expect("test module parses")
}

fn install(scene: &mut HeadlessScenario, kind: i32, key: i32, target: i64) {
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(kind, key, target),
        )
        .expect("module installs");
    scene.tick();
    assert_eq!(&memory(scene)[72..80], &[0; 8], "create and bind answer Ok");
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn i64_at(bytes: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

fn requests(scene: &HeadlessScenario) -> i32 {
    i32_at(&memory(scene), 696)
}

/// An empty plan: the GI keeps shooting (it is asked again and again) and
/// the target is never hit. The first link saw the native entry.
#[test]
fn an_empty_fire_plan_launches_nothing() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some((mut scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    install(&mut scene, EVENT_FIRE, EMPTY, attacker as i64);
    let full = scene.sim().entities().get(target).expect("target").health.current();
    let order = CommandEnvelope::new(
        scene.sim().interner.get("Americans").expect("Americans house"),
        scene.sim().session.tick() + 1,
        Command::clicked_mission(
            attacker,
            crate::sim::mission::MissionType::Attack,
            crate::sim::combat::TargetKind::Entity(target),
        ),
    );
    let mut orders = vec![order];
    for _ in 0..200 {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
    }
    assert!(requests(&scene) >= 2, "the GI fired more than once");
    let now = scene.sim().entities().get(target).expect("target").health.current();
    assert_eq!(now, full, "no bullet, no hit");
    let record = &memory(&scene)[512..672];
    assert_eq!(i32_at(record, 4), EVENT_FIRE);
    assert_eq!(i64_at(record, 24), attacker as i64, "subject");
    assert_eq!(i64_at(record, 32), target as i64, "other");
    // native 1, one 48-byte entry passed on, a 768-byte area.
    assert_eq!(i64_at(record, 120), 1);
    assert_eq!((i32_at(record, 152), i32_at(record, 156)), (768, 48));
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// The native entry twice: two bullets of the shot's weapon. The second one
/// enters the logic list behind the walk (as a second native bullet would)
/// and lands within the next frames; a native shot lands once.
#[test]
fn a_doubled_native_entry_hits_twice() {
    let Some((mut plain, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let plain_hits = hits_after_first(&mut plain, attacker, target);
    let (mut scene, attacker, target) = super::tests::gi_duel().expect("second duel");
    install(&mut scene, EVENT_FIRE, DOUBLE, attacker as i64);
    let doubled_hits = hits_after_first(&mut scene, attacker, target);
    assert_eq!((plain_hits, doubled_hits), (1, 2));
    assert_eq!(requests(&scene), 1);
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// The hits the target takes from the frame of the first one through the
/// next two frames (the GI's rearm is longer than that).
fn hits_after_first(scene: &mut HeadlessScenario, attacker: u64, target: u64) -> u32 {
    let health = |scene: &HeadlessScenario| {
        scene.sim().entities().get(target).expect("target lives").health.current()
    };
    super::tests::first_hit(scene, attacker, target, |_| {});
    let mut hits = 1;
    let mut last = health(scene);
    for _ in 0..2 {
        scene.tick();
        let now = health(scene);
        if now < last {
            hits += 1;
        }
        last = now;
    }
    hits
}

/// A chemical missile strike (TAction 96) at the first waypoint the map
/// has: its bullet and BulletType ID.
fn chem_missile(scene: &mut HeadlessScenario) -> Option<(u64, u32)> {
    let rules = &scene.runtime.resources.rules;
    let projectile = rules
        .weapon("ChemLauncher")
        .and_then(|weapon| weapon.projectile.clone())?;
    let type_id = rules.projectile_index(&projectile)? as u32;
    let waypoint = (0..100).find(|&w| scene.sim().session.waypoint_cell(w).is_some())?;
    let before: Vec<u64> = scene.sim().projectiles.iter().map(|(id, _)| *id).collect();
    let runtime = &mut scene.runtime;
    assert!(
        runtime
            .simulation
            .trigger_chemical_strike(&runtime.resources.rules, waypoint)
    );
    let bullet = scene
        .sim()
        .projectiles
        .iter()
        .map(|(id, _)| *id)
        .find(|id| !before.contains(id))?;
    Some((bullet, type_id))
}

fn chem_type(scene: &HeadlessScenario) -> Option<u32> {
    let rules = &scene.runtime.resources.rules;
    let projectile = rules
        .weapon("ChemLauncher")
        .and_then(|weapon| weapon.projectile.clone())?;
    rules.projectile_index(&projectile).map(|id| id as u32)
}

/// `ProjectileFrame`: the step's velocity replaces the guided flight and
/// its module data stays with the bullet.
#[test]
fn a_projectile_frame_step_moves_the_bullet_and_keeps_its_data() {
    let Some(mut scene) = super::tests::scenario() else {
        return;
    };
    let Some(type_id) = chem_type(&scene) else {
        return;
    };
    let target = super::type_ref(NAME_PROJECTILE, type_id);
    install(&mut scene, EVENT_PROJECTILE_FRAME, STEP, target);
    let (bullet, _) = chem_missile(&mut scene).expect("the strike launches");
    let start = scene.sim().projectiles.get(bullet).expect("bullet").position();
    scene.tick();
    let bullet_now = scene.sim().projectiles.get(bullet).expect("still flying");
    let at = bullet_now.position();
    assert_eq!((at.x, at.y, at.z), (start.x, start.y, start.z + 64));
    let record = &memory(&scene)[512..672];
    assert_eq!(i32_at(record, 4), EVENT_PROJECTILE_FRAME);
    assert_eq!(i64_at(record, 48), bullet as i64, "projectile");
    assert_eq!(i32_at(record, 152), ProjectileStep::SIZE as i32);
    assert_eq!(scene.sim().ext_modules.projectiles.data(bullet, 0), 0x1234);
    // The bullet reads back its velocity in 16.16.
    let vz = property(&mut scene, bullet, "VZ");
    assert_eq!(vz, 64 << 16);
    assert_eq!(property(&mut scene, bullet, "Data0"), 0x1234);
}

/// `Launch`: a one-point `Waypoints` path at 128 leptons a frame steers the
/// bullet east and up, and its end is an impact.
#[test]
fn a_launch_path_steers_the_bullet_to_its_end() {
    let Some(mut scene) = super::tests::scenario() else {
        return;
    };
    let Some(type_id) = chem_type(&scene) else {
        return;
    };
    let target = super::type_ref(NAME_PROJECTILE, type_id);
    install(&mut scene, EVENT_LAUNCH, PATH, target);
    let (bullet, _) = chem_missile(&mut scene).expect("the strike launches");
    assert_eq!(requests(&scene), 1, "asked once, at launch");
    assert_eq!(i64_at(&memory(&scene)[512..672], 48), bullet as i64);
    let start = scene.sim().projectiles.get(bullet).expect("bullet").position();
    scene.tick();
    let at = scene.sim().projectiles.get(bullet).expect("on its path").position();
    let expected = tests_support::waypoint_step(
        [start.x, start.y, start.z],
        [start.x + 1024, start.y, start.z + 512],
        128,
    );
    assert_eq!([at.x, at.y, at.z], expected);
    for _ in 0..12 {
        scene.tick();
    }
    assert!(
        scene.sim().projectiles.get(bullet).is_none(),
        "the path's end detonated it"
    );
    assert!(scene.sim().ext_modules.projectiles.is_empty());
    assert_eq!(requests(&scene), 1);
}

/// A projectile property as the module reads it.
fn property(scene: &mut HeadlessScenario, id: u64, name: &str) -> i64 {
    let prop = vera20k_wasm_ext::property_id(name).expect("property");
    let runtime = &mut scene.runtime;
    runtime
        .simulation
        .with_module_host(&runtime.resources.rules, ModuleSource::Map, |host| {
            host.reader().read(id as i64, prop)
        })
        .expect("a valid read")
        .0
}

#[test]
fn path_arithmetic() {
    tests_support::check_arithmetic();
    assert!(MAX_STEP > 0);
}
