//! `TakeDamage` decisions on a retail map: a handler changes the damage a
//! real hit deals, one that does not handle it or writes keeps the native
//! damage, its `random` leaves the main stream alone, and two runs hash
//! alike.

use vera20k_wasm_ext::triggers::EVENT_TAKE_DAMAGE;

use super::ModuleSource;
use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;

/// Handler keys: halve `native`; leave the record alone; call `var_set`;
/// draw `random(0, 999_999)` into memory and halve.
const HALVE: i32 = 1;
const IGNORE: i32 = 2;
const WRITE: i32 = 3;
const DRAW: i32 = 4;

fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

/// A `TriggerDef`: house 0, repeating, every difficulty, decision handler
/// `key`, one `TakeDamage` event, no actions.
fn decide_def(key: i32) -> Vec<u8> {
    let mut out = 0i64.to_le_bytes().to_vec();
    for value in [2, 7, 0, key, 1, 0, EVENT_TAKE_DAMAGE, 0, 0, -1] {
        out.extend(value.to_le_bytes());
    }
    out.extend(0i64.to_le_bytes());
    out.extend([0; 8]);
    out
}

/// Creates the trigger in `on_scenario_start` and binds it to `target`
/// (statuses at 72 and 76). `on_decide` copies its record to 512, then
/// answers by key; `DRAW` appends each draw to 700 and counts them at 696.
/// Its delivery area is one `DecideRecord` at 1024.
fn module(key: i32, target: u64) -> Vec<u8> {
    let def = decide_def(key);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "random" (func $random (param i32 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\98\00\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func $halve (param $ptr i32)
    (i64.store offset=128 (local.get $ptr)
      (i64.div_s (i64.load offset=112 (local.get $ptr)) (i64.const 2)))
    (i32.store offset=136 (local.get $ptr) (i32.const 1)))
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (local $key i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 152))
    (local.set $key (i32.load (local.get $ptr)))
    (if (i32.eq (local.get $key) (i32.const {HALVE})) (then (call $halve (local.get $ptr))))
    (if (i32.eq (local.get $key) (i32.const {WRITE}))
      (then (drop (call $var_set (i64.const 0) (i32.const 3) (i64.const 1)))))
    (if (i32.eq (local.get $key) (i32.const {DRAW}))
      (then
        (drop (call $random (i32.const 0) (i32.const 999999)
          (i32.add (i32.const 700) (i32.shl (i32.load (i32.const 696)) (i32.const 2)))))
        (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
        (call $halve (local.get $ptr))))))"#,
        len = def.len(),
    ))
    .expect("test module parses")
}

/// The map's first object on the map with more than 100 Strength left.
fn sturdy_object(scene: &HeadlessScenario) -> u64 {
    scene
        .sim()
        .entities()
        .iter_sorted()
        .find(|(_, e)| e.health.current() > 100 && !e.lifecycle.in_limbo())
        .map(|(id, _)| id)
        .expect("the map has a sturdy object")
}

/// A scene whose map module bound a decider with `key` to
/// [`sturdy_object`], after the frame that ran `on_scenario_start`.
fn decided_scene(key: i32) -> Option<(HeadlessScenario, u64)> {
    let mut scene = scenario()?;
    let victim = sturdy_object(&scene);
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(key, victim),
        )
        .expect("module installs");
    scene.tick();
    assert_eq!(
        &memory(&scene)[72..80],
        &[0; 8],
        "create and bind answer Ok"
    );
    Some((scene, victim))
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn health(scene: &HeadlessScenario, id: u64) -> i32 {
    scene
        .sim()
        .entities()
        .get(id)
        .expect("victim lives")
        .health
        .current()
}

/// One direct hit of `damage` with the `SA` warhead and no attacker.
fn hit(scene: &mut HeadlessScenario, victim: u64, damage: i32) {
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
fn a_decision_halves_the_damage_a_hit_deals() {
    let Some((mut scene, victim)) = decided_scene(HALVE) else {
        return;
    };
    let before = health(&scene, victim);
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 5);
    // The handler saw native and previous 10, with the hit as its event.
    let record = memory(&scene)[512..664].to_vec();
    let i64_at = |at: usize| i64::from_le_bytes(record[at..at + 8].try_into().expect("8 bytes"));
    assert_eq!(
        &record[..8],
        &[HALVE.to_le_bytes(), EVENT_TAKE_DAMAGE.to_le_bytes()].concat()
    );
    assert_eq!(i64_at(24), victim as i64, "subject");
    assert_eq!((i64_at(72), i64_at(112), i64_at(120)), (10, 10, 10));
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

#[test]
fn a_handler_that_leaves_handled_unset_keeps_the_native_damage() {
    let Some((mut scene, victim)) = decided_scene(IGNORE) else {
        return;
    };
    let before = health(&scene, victim);
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 10);
    assert_eq!(&memory(&scene)[512..516], &IGNORE.to_le_bytes(), "asked");
}

#[test]
fn a_write_in_on_decide_stops_the_module_and_keeps_the_native_damage() {
    let Some((mut scene, victim)) = decided_scene(WRITE) else {
        return;
    };
    let before = health(&scene, victim);
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 10);
    let status = scene.sim().ext_modules().status();
    assert!(
        status[0]
            .1
            .as_deref()
            .is_some_and(|why| why.contains("on_decide")),
        "{status:?}"
    );
    assert!(!scene.sim().trigger_runtime.local_variable_is_set(3));
    assert!(scene.sim().trigger_runtime.module_triggers().is_empty());
}

/// Decision `random` draws from the (frame, subject, kind) substream: the
/// module's main stream does not move, a second request in the frame
/// continues the substream, and two runs agree on draws and module state.
#[test]
fn decision_random_leaves_the_main_stream_and_runs_hash_alike() {
    let run = || {
        let (mut scene, victim) = decided_scene(DRAW)?;
        let main = scene.sim().ext_modules.slots[0].rng.state();
        hit(&mut scene, victim, 10);
        hit(&mut scene, victim, 10);
        assert_eq!(scene.sim().ext_modules.slots[0].rng.state(), main);
        scene.tick();
        let memory = memory(&scene);
        let draws: Vec<u8> = memory[696..708].to_vec();
        // The whole simulation hashes alike: the TakeDamage record carries
        // the warhead index, which follows the [Warheads] registry order.
        Some((draws, scene.sim().state_hash()))
    };
    let Some((draws, hash)) = run() else {
        return;
    };
    assert_eq!(&draws[..4], &2i32.to_le_bytes());
    assert_ne!(draws[4..8], draws[8..12], "the second request continues");
    assert_eq!(run(), Some((draws, hash)));
}

/// Runs the SDK's decision example when `VERA20K_TEST_DECIDE_WASM` names
/// its build (`sdk/examples/decide-damage`): it binds a `TakeDamage`
/// decision trigger to every object and halves each hit.
#[test]
fn a_rust_built_decide_module_halves_damage() {
    let Some(path) = std::env::var_os("VERA20K_TEST_DECIDE_WASM") else {
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
    let before = health(&scene, victim);
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 5);
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}
