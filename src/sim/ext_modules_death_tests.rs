//! The death plan on a retail map (control-point list DMG-S5, RFC 0001
//! appendix 4.3 `FatalDamage`'s decision form): the handler sees the
//! engine's plan, a plan that keeps it changes nothing, and one without
//! survivors spawns none.

use vera20k_wasm_ext::triggers::EVENT_FATAL_DAMAGE;

use super::ModuleSource;
use crate::headless_scenario::HeadlessScenario;
use crate::map::entities::EntityCategory;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

/// Handler keys: answer `previous`; answer `previous` without the
/// survivors bit (3).
const KEEP: i32 = 1;
const NO_SURVIVORS: i32 = 2;

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

/// Creates a `FatalDamage` decider with `key` and binds it to `target`
/// (statuses at 72 and 76); `on_decide` copies its record to 512.
fn module(key: i32, target: i64) -> Vec<u8> {
    let def = decide_def(EVENT_FATAL_DAMAGE, key);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\a0\00\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (i64.store offset=136 (local.get $ptr) (i64.load offset=128 (local.get $ptr)))
    (if (i32.eq (i32.load (local.get $ptr)) (i32.const {NO_SURVIVORS}))
      (then
        (i64.store offset=136 (local.get $ptr)
          (i64.and (i64.load offset=128 (local.get $ptr)) (i64.const -9)))))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#,
        len = def.len(),
    ))
    .expect("test module parses")
}

/// The duel's American power plant.
fn power_plant(scene: &HeadlessScenario) -> u64 {
    let sim = scene.sim();
    sim.entities()
        .iter_sorted()
        .find(|(_, e)| {
            e.category() == EntityCategory::Structure
                && sim
                    .interner
                    .object_type(e.type_ref(), &scene.runtime.resources.rules)
                    .is_some_and(|object| object.id == "GAPOWR")
        })
        .map(|(id, _)| id)
        .expect("the duel's power plant")
}

fn infantry(scene: &HeadlessScenario) -> usize {
    scene
        .sim()
        .entities()
        .iter_sorted()
        .filter(|(_, e)| e.category() == EntityCategory::Infantry && e.lifecycle.object_alive())
        .count()
}

/// Kills `victim` with one `Super` hit that does not ignore defenses.
fn kill(scene: &mut HeadlessScenario, victim: u64) {
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let warhead = runtime.simulation.interner.intern("Super");
    let event = EntityDamageEvent::direct_receiver(
        victim,
        100_000,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
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

/// The power plant's death in a duel scene with a decider of `key`, or
/// none: the infantry count after it and the request the handler saw.
fn plant_death(key: Option<i32>) -> Option<(usize, Vec<u8>)> {
    let (mut scene, _, _) = super::tests::gi_duel()?;
    let plant = power_plant(&scene);
    if let Some(key) = key {
        scene
            .runtime
            .simulation
            .install_ext_module(
                &scene.runtime.resources.rules,
                ModuleSource::Map,
                &module(key, plant as i64),
            )
            .expect("module installs");
    }
    scene.tick();
    kill(&mut scene, plant);
    let record = match key {
        Some(_) => scene.sim().ext_modules.slots[0].module.memory()[512..672].to_vec(),
        None => Vec::new(),
    };
    Some((infantry(&scene), record))
}

#[test]
fn the_death_plan_decides_the_survivors() {
    let Some((native, _)) = plant_death(None) else {
        return;
    };
    let (kept, record) = plant_death(Some(KEEP)).expect("second scene");
    assert_eq!(kept, native, "keeping the plan changes nothing");
    let i32_at = |at: usize| i32::from_le_bytes(record[at..at + 4].try_into().unwrap());
    let i64_at = |at: usize| i64::from_le_bytes(record[at..at + 8].try_into().unwrap());
    assert_eq!(i32_at(4), EVENT_FATAL_DAMAGE);
    let plan = i64_at(120);
    assert_eq!(plan & 1 << 3, 1 << 3, "a hit that does not ignore defenses: survivors");
    assert_eq!(plan & 1 << 5, 1 << 5, "the announcement");
    assert_eq!(plan & 1 << 6, 0, "no Tiberium remains without Ares");
    assert_eq!(plan >> 32, 0, "the type's own death weapon");
    let (none, _) = plant_death(Some(NO_SURVIVORS)).expect("third scene");
    let (before, _) = {
        let (scene, _, _) = super::tests::gi_duel().expect("fourth scene");
        (infantry(&scene), ())
    };
    assert_eq!(none, before, "no survivors left the plant");
}
