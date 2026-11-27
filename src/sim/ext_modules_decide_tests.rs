//! Decisions on a retail map: a `TakeDamage` handler changes the damage a
//! real hit deals, one that does not handle it or writes keeps the native
//! damage, its `random` leaves the main stream alone, and two runs hash
//! alike; a `ProductionAllowed` refusal reaches CanBuild; a
//! `CommandAllowed` refusal stops a command; a decision caused inside the
//! other module's call asks this module's link.

use vera20k_wasm_ext::NAME_TECHNO_TYPE;
use vera20k_wasm_ext::decide::{EVENT_COMMAND_ALLOWED, EVENT_PRODUCTION_ALLOWED};
use vera20k_wasm_ext::triggers::EVENT_TAKE_DAMAGE;

use super::ModuleSource;
use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;

/// Handler keys: halve `native`; leave the record alone; call `var_set`;
/// draw `random(0, 999_999)` into memory and halve; answer 0; answer 0 to
/// the first request whose `value` is 2 (`CommandAllowed`'s Stop) and leave
/// the others alone, counting those requests at 696.
pub(super) const HALVE: i32 = 1;
const IGNORE: i32 = 2;
const WRITE: i32 = 3;
const DRAW: i32 = 4;
pub(super) const REFUSE: i32 = 5;
const REFUSE_FIRST_STOP: i32 = 6;

fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

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
/// (statuses at 72 and 76). `on_decide` copies its record to 512, then
/// answers by key; `DRAW` appends each draw to 700 and counts them at 696.
/// Its delivery area is one `DecideRecord` at 1024.
pub(super) fn module(kind: i32, key: i32, target: i64) -> Vec<u8> {
    let def = decide_def(kind, key);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "random" (func $random (param i32 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\a0\00\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func $halve (param $ptr i32)
    (i64.store offset=136 (local.get $ptr)
      (i64.div_s (i64.load offset=120 (local.get $ptr)) (i64.const 2)))
    (i32.store offset=144 (local.get $ptr) (i32.const 1)))
  (func $refuse (param $ptr i32)
    (i64.store offset=136 (local.get $ptr) (i64.const 0))
    (i32.store offset=144 (local.get $ptr) (i32.const 1)))
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (local $key i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (local.set $key (i32.load (local.get $ptr)))
    (if (i32.eq (local.get $key) (i32.const {HALVE})) (then (call $halve (local.get $ptr))))
    (if (i32.eq (local.get $key) (i32.const {WRITE}))
      (then (drop (call $var_set (i64.const 0) (i32.const 3) (i64.const 1)))))
    (if (i32.eq (local.get $key) (i32.const {DRAW}))
      (then
        (drop (call $random (i32.const 0) (i32.const 999999)
          (i32.add (i32.const 700) (i32.shl (i32.load (i32.const 696)) (i32.const 2)))))
        (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
        (call $halve (local.get $ptr))))
    (if (i32.eq (local.get $key) (i32.const {REFUSE})) (then (call $refuse (local.get $ptr))))
    (if (i32.and
          (i32.eq (local.get $key) (i32.const {REFUSE_FIRST_STOP}))
          (i64.eq (i64.load offset=72 (local.get $ptr)) (i64.const 2)))
      (then
        (if (i32.eqz (i32.load (i32.const 696))) (then (call $refuse (local.get $ptr))))
        (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))))))"#,
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

/// The first live, placed unit or infantryman on the map, with its owner.
pub(super) fn ground_unit(scene: &HeadlessScenario) -> (u64, String) {
    use crate::map::entities::EntityCategory;
    let sim = scene.sim();
    sim.entities()
        .iter_sorted()
        .find(|(_, e)| {
            matches!(
                e.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && e.lifecycle.object_alive()
                && e.lifecycle.cell_marked()
                && !e.lifecycle.in_limbo()
        })
        .map(|(id, e)| (id, sim.interner.resolve(e.owner()).to_string()))
        .expect("the map has a placed ground unit")
}

/// Installs a map module whose decider for `kind` with `key` binds to
/// `target`, and runs the frame of its `on_scenario_start`.
pub(super) fn install_decider(scene: &mut HeadlessScenario, kind: i32, key: i32, target: i64) {
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

/// A scene whose map module bound a `TakeDamage` decider with `key` to
/// [`sturdy_object`], after the frame that ran `on_scenario_start`.
fn decided_scene(key: i32) -> Option<(HeadlessScenario, u64)> {
    let mut scene = scenario()?;
    let victim = sturdy_object(&scene);
    install_decider(&mut scene, EVENT_TAKE_DAMAGE, key, victim as i64);
    Some((scene, victim))
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    memory_of(scene, 0)
}

fn memory_of(scene: &HeadlessScenario, slot: usize) -> Vec<u8> {
    scene.sim().ext_modules.slots[slot].module.memory().to_vec()
}

fn i64_at(bytes: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
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
    let record = memory(&scene)[512..672].to_vec();
    assert_eq!(
        &record[..8],
        &[HALVE.to_le_bytes(), EVENT_TAKE_DAMAGE.to_le_bytes()].concat()
    );
    assert_eq!(i64_at(&record, 24), victim as i64, "subject");
    assert_eq!(
        (
            i64_at(&record, 72),
            i64_at(&record, 120),
            i64_at(&record, 128)
        ),
        (10, 10, 10)
    );
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

#[test]
fn decisions_have_their_own_pool_and_skip_once_it_is_used_up() {
    let Some((mut scene, victim)) = decided_scene(HALVE) else {
        return;
    };
    let frame = scene.sim().session.binary_frame();
    let slot = &mut scene.runtime.simulation.ext_modules.slots[0];
    slot.frame_budget = 2;
    // The module phase spending the whole budget takes nothing from it.
    slot.charge_frame_ops(frame, 2);
    let before = health(&scene, victim);
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 5, "a whole call");
    hit(&mut scene, victim, 10);
    assert_eq!(health(&scene, victim), before - 15, "the pool is used up");
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

/// `ProductionAllowed`: a handler answering 0 for its bound type makes
/// CanBuild refuse the type to a house that could build it, from the
/// frame's decision step on.
#[test]
fn a_production_allowed_refusal_makes_can_build_refuse_the_type() {
    use crate::sim::production::{CanBuild, can_build};
    let Some(mut scene) = scenario() else { return };
    let (owner, type_id) = {
        let (sim, rules) = (scene.sim(), &scene.runtime.resources.rules);
        sim.houses
            .order()
            .iter()
            .filter(|owner| sim.houses.get(owner).is_some_and(|h| !h.is_defeated()))
            .find_map(|&owner| {
                let object = rules.all_objects().find(|object| {
                    can_build(sim, rules, owner, object, false, true) == CanBuild::Yes
                })?;
                Some((owner, rules.type_handle(&object.id)?.0))
            })
            .expect("a house can build a type")
    };
    let bound = super::type_ref(NAME_TECHNO_TYPE, type_id);
    install_decider(&mut scene, EVENT_PRODUCTION_ALLOWED, REFUSE, bound);
    scene.tick();
    let rules = &scene.runtime.resources.rules;
    let object = super::techno_type(rules, type_id as i32).expect("the bound type");
    assert_eq!(
        can_build(scene.sim(), rules, owner, object, false, true),
        CanBuild::No
    );
    let record = memory(&scene)[512..672].to_vec();
    assert_eq!(
        &record[..8],
        &[REFUSE.to_le_bytes(), EVENT_PRODUCTION_ALLOWED.to_le_bytes()].concat()
    );
    assert_eq!(&record[88..92], &(type_id as i32).to_le_bytes(), "id");
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);

    // A step with no budget left asks nobody and keeps the refusal.
    let frame = scene.sim().session.binary_frame();
    let sim = &mut scene.runtime.simulation;
    sim.ext_modules.slots[0].frame_budget = 1;
    sim.ext_modules.slots[0].charge_frame_ops(frame, 1);
    sim.decide_production_allowed(rules);
    assert_eq!(
        can_build(sim, rules, owner, object, false, true),
        CanBuild::No
    );
}

#[test]
fn an_unhandled_production_decision_preserves_factory_admission() {
    use crate::sim::production::{CanBuild, can_build};
    let Some(mut scene) = scenario() else { return };
    let (owner, type_id) = {
        let (sim, rules) = (scene.sim(), &scene.runtime.resources.rules);
        sim.houses
            .order()
            .iter()
            .filter(|owner| sim.houses.get(owner).is_some_and(|h| !h.is_defeated()))
            .find_map(|&owner| {
                let object = rules.all_objects().find(|object| {
                    can_build(sim, rules, owner, object, false, true) == CanBuild::No
                        && can_build(sim, rules, owner, object, true, true) == CanBuild::Yes
                })?;
                Some((owner, rules.type_handle(&object.id)?.0))
            })
            .expect("a type whose sidebar and FindFactory checks differ")
    };
    let bound = super::type_ref(NAME_TECHNO_TYPE, type_id);
    install_decider(&mut scene, EVENT_PRODUCTION_ALLOWED, IGNORE, bound);
    scene.tick();

    let record = memory(&scene);
    assert_eq!(
        &record[512..516],
        &IGNORE.to_le_bytes(),
        "handler was called"
    );
    let rules = &scene.runtime.resources.rules;
    let object = super::techno_type(rules, type_id as i32).expect("the bound type");
    assert_eq!(
        can_build(scene.sim(), rules, owner, object, false, true),
        CanBuild::No,
    );
    assert_eq!(
        can_build(scene.sim(), rules, owner, object, true, true),
        CanBuild::Yes,
    );
}

/// `CommandAllowed`: the receiver refuses a Stop its bound unit's handler
/// answers 0 for, and runs the next one the handler leaves alone.
#[test]
fn a_command_allowed_refusal_stops_the_command() {
    use crate::sim::command::Command;
    let Some(mut scene) = scenario() else { return };
    let (unit, owner) = ground_unit(&scene);
    install_decider(
        &mut scene,
        EVENT_COMMAND_ALLOWED,
        REFUSE_FIRST_STOP,
        unit as i64,
    );
    let stop = Command::Stop { entity_id: unit };
    assert!(!scene.runtime.apply_command_for_test(&owner, &stop));
    let record = memory(&scene)[512..672].to_vec();
    assert_eq!(
        &record[..8],
        &[
            REFUSE_FIRST_STOP.to_le_bytes(),
            EVENT_COMMAND_ALLOWED.to_le_bytes()
        ]
        .concat()
    );
    // Subject the unit, value Stop (appendix 8.3 number 2), native 1.
    assert_eq!(
        (
            i64_at(&record, 24),
            i64_at(&record, 72),
            i64_at(&record, 120)
        ),
        (unit as i64, 2, 1)
    );
    assert!(scene.runtime.apply_command_for_test(&owner, &stop));
    assert_eq!(
        &memory(&scene)[696..700],
        &2i32.to_le_bytes(),
        "asked twice"
    );
}

/// RFC section 5, "No reentry": the mod module's own `Damage` action
/// reaches the map module's `TakeDamage` decider, which halves the hit.
#[test]
fn a_decision_inside_the_other_modules_call_asks_its_link() {
    let Some(mut scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    install_decider(&mut scene, EVENT_TAKE_DAMAGE, HALVE, victim as i64);
    let rules = &scene.runtime.resources.rules;
    let warhead = rules.warhead_index("SA").expect("the SA warhead");
    // `Damage` 100 with SA on the victim, once, in the mod's `on_frame`;
    // the status goes to 8.
    let damager = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_frame")
    (if (i32.eqz (i32.load (i32.const 0)))
      (then
        (i32.store (i32.const 0) (i32.const 1))
        (i32.store (i32.const 128) (i32.const {warhead}))
        (i64.store (i32.const 144) (i64.const 100))
        (i32.store (i32.const 8)
          (call $action (i32.const {damage}) (i64.const {victim})
            (i32.const 128) (i32.const 72) (i32.const 256)))))))"#,
        damage = super::actions::DAMAGE,
    ))
    .expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(rules, ModuleSource::Mod, &damager)
        .expect("module installs");
    let before = health(&scene, victim);
    scene.tick();
    let status = scene.sim().ext_modules().status();
    assert_eq!(
        (status[0].1.as_deref(), status[1].1.as_deref()),
        (None, None)
    );
    assert_eq!(&memory_of(&scene, 0)[8..12], &[0; 4], "Damage answers Ok");
    // The map module (slot 1, after the mod's) decided the hit.
    let record = memory_of(&scene, 1)[512..672].to_vec();
    assert_eq!(i64_at(&record, 24), victim as i64, "subject");
    let native = i64_at(&record, 120);
    assert!(native >= 2, "the hit deals damage: {native}");
    assert_eq!(i64::from(before - health(&scene, victim)), native / 2);
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
