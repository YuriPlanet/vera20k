//! Module triggers on a retail map: a real hit raises `TakeDamage`, the
//! record reaches `on_events` byte for byte, a filter clause holds one back,
//! queued records survive a save, and a removed trigger delivers nothing.

use std::hash::Hasher;

use vera20k_wasm_ext::EventRecord;
use vera20k_wasm_ext::triggers::{ACTION_MODULE, ACTION_SET_VAR, EVENT_TAKE_DAMAGE};

use super::{Host, ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;
/// The module action's dispatch key, and the local variable its `SetVar`
/// action sets.
const KEY: i32 = 7;
const LOCAL: u32 = 9;

pub(super) fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

/// A `TriggerDef`: house 0, repeating, every difficulty, one `TakeDamage`
/// event with `clauses` (16 bytes each), then a `Module` action (key
/// [`KEY`]) and a `SetVar` of local [`LOCAL`].
fn trigger_def(clauses: &[[u8; 16]]) -> Vec<u8> {
    let i32s = |values: &[i32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut out = 0i64.to_le_bytes().to_vec();
    out.extend(i32s(&[2, 7, 0, -1, 1, 2]));
    out.extend(i32s(&[EVENT_TAKE_DAMAGE, 0, 0, -1]));
    out.extend(0i64.to_le_bytes());
    out.extend(i32s(&[clauses.len() as i32, 0]));
    out.extend(clauses.concat());
    for (kind, id, value) in [
        (ACTION_MODULE, KEY, 0),
        (ACTION_SET_VAR, LOCAL as i32, 1i64),
    ] {
        out.extend(i32s(&[kind, 0, id, 0, 0, 0]));
        out.extend(value.to_le_bytes());
        out.extend([0; 72 - 24]);
    }
    out
}

/// `Subject.HealthPercent < 30`.
fn health_below_30_percent() -> [u8; 16] {
    let prop = vera20k_wasm_ext::property_id("HealthPercent").expect("known property");
    let mut clause = [0u8; 16];
    clause[1] = 2; // Lt
    clause[4..8].copy_from_slice(&prop.to_le_bytes());
    clause[8..].copy_from_slice(&30i64.to_le_bytes());
    clause
}

/// Creates the trigger in `on_scenario_start` and binds it to `target`
/// (handle at 64, statuses at 72 and 76); `on_events` copies the first
/// record it ever gets to 512 and counts records at 80. Its delivery area is
/// eight records at 1024.
fn module(def: &[u8], target: u64) -> Vec<u8> {
    let bytes = |b: &[u8]| b.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\80\03\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{def}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (if (i32.eqz (i32.load (i32.const 80)))
      (then (memory.copy (i32.const 512) (local.get $ptr) (i32.const 112))))
    (i32.store (i32.const 80) (i32.add (i32.load (i32.const 80)) (local.get $count)))))"#,
        def = bytes(def),
        len = def.len(),
    ))
    .expect("test module parses")
}

/// The map's first object on the map with more than 100 Strength left.
pub(super) fn sturdy_object(scene: &HeadlessScenario) -> u64 {
    scene
        .sim()
        .entities()
        .iter_sorted()
        .find(|(_, e)| e.health.current() > 100 && !e.lifecycle.in_limbo())
        .map(|(id, _)| id)
        .expect("the map has a sturdy object")
}

/// A scene whose module bound `def` to [`sturdy_object`], after the frame
/// that ran `on_scenario_start`.
fn bound_scene(def: &[u8]) -> Option<(HeadlessScenario, u64)> {
    let mut scene = scenario()?;
    let victim = sturdy_object(&scene);
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(def, victim),
        )
        .expect("module installs");
    scene.tick();
    let memory = memory(&scene);
    assert_eq!(&memory[72..80], &[0; 8], "create and bind answer Ok");
    Some((scene, victim))
}

pub(super) fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

pub(super) fn received(scene: &HeadlessScenario) -> i32 {
    i32::from_le_bytes(memory(scene)[80..84].try_into().expect("4 bytes"))
}

/// One direct hit of `damage` with the `SA` warhead and no attacker.
pub(super) fn hit(scene: &mut HeadlessScenario, victim: u64, damage: i32) {
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let warhead = runtime.simulation.interner.intern("SA");
    let event = EntityDamageEvent::direct_receiver(
        victim,
        damage,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    runtime.simulation.commit_direct_damage_receiver(
        rules,
        Some(rules.overlay_registry()),
        event,
        &mut FrameEffects::default(),
    );
}

#[test]
fn a_hit_fires_the_bound_trigger_and_on_events_gets_its_record() {
    let Some((mut scene, victim)) = bound_scene(&trigger_def(&[])) else {
        return;
    };
    let handle = i64::from_le_bytes(memory(&scene)[64..72].try_into().expect("8 bytes"));
    let frame = scene.sim().session.binary_frame();
    let health = scene
        .sim()
        .entities()
        .get(victim)
        .expect("victim")
        .health
        .current();
    hit(&mut scene, victim, 10);
    // SetVar ran at the hit; the module action waits for the module phase.
    assert!(scene.sim().trigger_runtime.local_variable_is_set(LOCAL));
    assert_eq!(received(&scene), 0);
    let sim = scene.sim();
    let entity = sim.entities().get(victim).expect("victim lives");
    assert_eq!(entity.health.current(), health - 10);
    let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
    let z =
        crate::sim::movement::ground_pose::object_world_z_leptons(entity, sim.resolved_terrain());
    let owner = sim.houses.index_of(entity.owner()).expect("owner house");
    let expected = EventRecord {
        key: KEY,
        trigger: handle,
        holder: victim as i64,
        subject: victim as i64,
        house: house_ref(owner),
        value: 10,
        id: scene
            .runtime
            .resources
            .rules
            .warhead_index("SA")
            .expect("SA") as i32,
        frame: frame as i32,
        x,
        y,
        z,
        ..EventRecord::empty(EVENT_TAKE_DAMAGE)
    };
    scene.tick();
    assert_eq!(received(&scene), 1);
    assert_eq!(&memory(&scene)[512..624], &expected.to_bytes());
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

#[test]
fn a_filter_clause_holds_back_a_hit_it_rejects() {
    let Some((mut scene, victim)) = bound_scene(&trigger_def(&[health_below_30_percent()])) else {
        return;
    };
    hit(&mut scene, victim, 10);
    scene.tick();
    assert_eq!(received(&scene), 0, "the victim is above 30%");
    assert!(!scene.sim().trigger_runtime.local_variable_is_set(LOCAL));
    // A hit that leaves it at 20% passes.
    let sim = scene.sim();
    let entity = sim.entities().get(victim).expect("victim");
    let strength = sim
        .entity_object_type(entity, &scene.runtime.resources.rules)
        .expect("type")
        .strength;
    let damage = entity.health.current() - strength / 5;
    hit(&mut scene, victim, damage);
    scene.tick();
    assert_eq!(received(&scene), 1);
    assert!(scene.sim().trigger_runtime.local_variable_is_set(LOCAL));
}

#[test]
fn a_queued_record_is_hashed_and_survives_a_save() {
    use crate::sim::snapshot::GameSnapshot;
    let Some((mut live, victim)) = bound_scene(&trigger_def(&[])) else {
        return;
    };
    let triggers_hash = |scene: &HeadlessScenario| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        scene.sim().trigger_runtime.hash_state(&mut hasher);
        hasher.finish()
    };
    let before = triggers_hash(&live);
    hit(&mut live, victim, 10);
    assert_ne!(triggers_hash(&live), before, "the queued record is hashed");
    let rules = &live.runtime.resources.rules;
    let bytes = GameSnapshot::save_validated(
        live.sim(),
        0,
        rules.simulation_config_hash(),
        "module triggers",
        0,
    );
    let mut loaded = GameSnapshot::load(&bytes).expect("save loads").sim;
    loaded
        .restore_after_snapshot_load()
        .expect("modules restore");
    loaded.rebuild_caches_after_load(
        live.sim().resolved_terrain().expect("terrain").clone(),
        Default::default(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
        .expect("map authority restores");
    loaded.resolve_type_handles(rules);
    let mut resumed = scenario().expect("second load");
    resumed.runtime.simulation = loaded;
    assert_eq!(triggers_hash(&resumed), triggers_hash(&live));
    live.tick();
    resumed.tick();
    assert_eq!(received(&resumed), 1);
    assert_eq!(memory(&resumed)[512..624], memory(&live)[512..624]);
    assert_eq!(triggers_hash(&resumed), triggers_hash(&live));
}

#[test]
fn a_trigger_removed_before_the_module_phase_delivers_nothing() {
    let Some((mut scene, victim)) = bound_scene(&trigger_def(&[])) else {
        return;
    };
    let handle = i64::from_le_bytes(memory(&scene)[64..72].try_into().expect("8 bytes"));
    hit(&mut scene, victim, 10);
    // What `trigger_remove` runs, from the module's own host.
    let runtime = &mut scene.runtime;
    let mut rng = crate::sim::rng::SimRng::new(0);
    let mut host = Host {
        sim: &mut runtime.simulation,
        rules: &runtime.resources.rules,
        rng: &mut rng,
        source: ModuleSource::Map,
        orders: &mut Vec::new(),
        rules_ini: None,
    };
    assert_eq!(
        host.remove_trigger(handle),
        Ok(vera20k_wasm_ext::Status::Ok)
    );
    scene.tick();
    assert_eq!(received(&scene), 0);
    hit(&mut scene, victim, 10);
    scene.tick();
    assert_eq!(received(&scene), 0);
    assert!(scene.sim().trigger_runtime.module_triggers().is_empty());
}

/// Runs the SDK's trigger example when `VERA20K_TEST_TRIGGERS_WASM` names
/// its build (`sdk/examples/panic-triggers`): it binds a `TakeDamage`
/// trigger to every TechnoType and, through `on_events`, sets local
/// variable 7 once a record arrives for the hit object.
#[test]
fn a_rust_built_trigger_module_receives_its_records() {
    let Some(path) = std::env::var_os("VERA20K_TEST_TRIGGERS_WASM") else {
        return;
    };
    let Some(mut scene) = scenario() else { return };
    let bytes = std::fs::read(&path).expect("module file reads");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &bytes)
        .expect("module installs");
    scene.tick();
    let victim = sturdy_object(&scene);
    hit(&mut scene, victim, 10);
    scene.tick();
    let status = scene.sim().ext_modules().status();
    assert_eq!(status[0].1, None, "the module is still running");
    assert!(scene.sim().trigger_runtime.local_variable_is_set(7));
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert!(
        log.iter().any(|l| l == "panic-triggers: hit for 10"),
        "{log:?}"
    );
}
