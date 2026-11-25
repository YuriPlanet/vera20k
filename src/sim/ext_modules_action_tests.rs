//! The atomic actions added beside `set` and the modifier store, a trigger
//! `Log` action's copied text, modifiers in the multiplayer checksum and in
//! their engine owners, modifier events, `AllowType` and a timed `Convert`,
//! on a retail map.

use vera20k_wasm_ext::triggers::{ACTION_MODULE, EVENT_MODIFIER_EXPIRE, EVENT_TIMER};

use super::actions::{
    ALLOW_TYPE, CONVERT, DETONATE_WEAPON, HEAL, LOG, MODIFY, SET_MISSION, SET_PROPERTY,
};
use super::{ModuleSource, cell_ref, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::map::entities::EntityCategory;
use crate::rules::mission_data::MissionType;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;

fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

fn install(scene: &mut HeadlessScenario, module: &str) {
    let module = wat::parse_str(module).expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &module)
        .expect("module installs");
}

fn prop(name: &str) -> i32 {
    vera20k_wasm_ext::property_id(name).expect("known property")
}

fn mem_i32(scene: &HeadlessScenario, at: usize) -> i32 {
    let memory = scene.sim().ext_modules.slots[0].module.memory();
    i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"))
}

/// The first live, placed unit or infantryman with more than 50 health.
fn ground_object(scene: &HeadlessScenario) -> (u64, (u16, u16)) {
    scene
        .sim()
        .entities()
        .iter_sorted()
        .find(|(_, e)| {
            matches!(
                e.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && e.health.current() > 50
                && e.lifecycle.object_alive()
                && e.lifecycle.cell_marked()
                && !e.lifecycle.in_limbo()
        })
        .map(|(id, e)| (id, (e.position.rx(), e.position.ry())))
        .expect("the map has a placed ground object")
}

/// WAT calling action `kind` on `target` with `ActionParams` at 128 whose
/// `id`, `arg`, `value` and `cell` are set; the status goes to `status_at`.
fn action_wat(
    kind: i32,
    target: i64,
    id: i32,
    arg: i32,
    value: i64,
    cell: i64,
    status_at: u32,
) -> String {
    format!(
        "(memory.fill (i32.const 128) (i32.const 0) (i32.const 72)) \
         (i32.store (i32.const 128) (i32.const {id})) (i32.store (i32.const 132) (i32.const {arg})) \
         (i64.store (i32.const 144) (i64.const {value})) (i64.store (i32.const 160) (i64.const {cell})) \
         (i32.store (i32.const {status_at}) (call $action (i32.const {kind}) (i64.const {target}) (i32.const 128) (i32.const 72) (i32.const 256)))"
    )
}

fn actions_module(calls: &[String]) -> String {
    format!(
        r#"(module
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start") {}))"#,
        calls.join(" ")
    )
}

/// `SetProperty` writes Health through `set`; `Heal` adds to it.
#[test]
fn set_property_and_heal_write_health_through_its_owner() {
    let Some(mut scene) = scenario() else { return };
    let (unit, _) = ground_object(&scene);
    install(
        &mut scene,
        &actions_module(&[
            action_wat(SET_PROPERTY, unit as i64, prop("Health"), 0, 10, 0, 0),
            action_wat(HEAL, unit as i64, 0, 0, 5, 0, 4),
        ]),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!([mem_i32(&scene, 0), mem_i32(&scene, 4)], [0, 0]);
    let entity = scene.sim().entities().get(unit).expect("unit lives");
    assert_eq!(entity.health.current(), 15);
}

/// `SetMission` orders Move to a cell as a player's order does.
#[test]
fn set_mission_orders_a_move_to_a_cell() {
    let Some(mut scene) = scenario() else { return };
    let (unit, (x, y)) = ground_object(&scene);
    let to = cell_ref(x + 3, y);
    install(
        &mut scene,
        &actions_module(&[action_wat(
            SET_MISSION,
            unit as i64,
            0,
            MissionType::Move as i32,
            0,
            to,
            0,
        )]),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0), 0, "SetMission answers Ok");
    let mission = &scene
        .sim()
        .entities()
        .get(unit)
        .expect("unit lives")
        .mission;
    let move_ = crate::sim::mission::MissionId::from_known(MissionType::Move);
    assert!(
        mission.current() == move_ || mission.queued() == move_,
        "{mission:?}"
    );
}

/// `DetonateWeapon` at an object's cell hurts it through the area owner.
#[test]
fn detonate_weapon_at_a_cell_damages_what_stands_there() {
    let Some(mut scene) = scenario() else { return };
    let (unit, (x, y)) = ground_object(&scene);
    let health = scene
        .sim()
        .entities()
        .get(unit)
        .expect("unit")
        .health
        .current();
    // The first weapon with real damage over at least a cell.
    let rules = &scene.runtime.resources.rules;
    let weapon = (0..)
        .map_while(|i| rules.weapon_at(i).map(|w| (i, w)))
        .find(|(_, w)| {
            w.damage >= 50
                && rules
                    .weapon_warhead(w)
                    .is_some_and(|warhead| warhead.cell_spread_f64 >= 1.0)
        })
        .map(|(i, _)| i as i32)
        .expect("a spreading weapon");
    install(
        &mut scene,
        &actions_module(&[action_wat(
            DETONATE_WEAPON,
            cell_ref(x, y),
            weapon,
            0,
            0,
            0,
            0,
        )]),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0), 0, "DetonateWeapon answers Ok");
    assert!(
        scene
            .sim()
            .entities()
            .get(unit)
            .is_none_or(|e| e.health.current() < health)
    );
}

/// A trigger's `Log` action logs the text `trigger_create` copied, after
/// the module overwrote its own copy.
#[test]
fn a_trigger_log_action_logs_its_copied_text() {
    let Some(mut scene) = scenario() else { return };
    let i32s = |values: &[i32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    // House 0, repeating, every difficulty, one Timer event every frame,
    // one Log action whose text is "copied" at 1024.
    let mut def = 0i64.to_le_bytes().to_vec();
    def.extend(i32s(&[2, 7, 0, -1, 1, 1]));
    def.extend(i32s(&[EVENT_TIMER, 0, 1, -1]));
    def.extend(0i64.to_le_bytes());
    def.extend(i32s(&[0, 0]));
    def.extend(i32s(&[LOG, 0]));
    def.extend([0; 56]);
    def.extend(i32s(&[1024, 6, 0, 0]));
    let bytes: String = def.iter().map(|b| format!("\\{b:02x}")).collect();
    install(
        &mut scene,
        &format!(
            r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "copied")
  (data (i32.const 2048) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 0) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i64.store (i32.const 1024) (i64.const 0))))"#,
            len = def.len()
        ),
    );
    for _ in 0..3 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0), 0, "trigger_create answers Ok");
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert!(log.iter().any(|line| line == "copied"), "{log:?}");
}

/// A modifier `Modify` added changes the multiplayer checksum; the same
/// module memory without it does not.
#[test]
fn a_modifier_enters_the_multiplayer_checksum() {
    let (Some(mut with), Some(mut without)) = (scenario(), scenario()) else {
        return;
    };
    let (unit, _) = ground_object(&with);
    // Both modules store nothing; one modifies the unit, the other a gone
    // object.
    let module = |target: u64| {
        actions_module(&[format!(
            "(memory.fill (i32.const 128) (i32.const 0) (i32.const 72)) \
             (i32.store (i32.const 128) (i32.const {})) (i32.store (i32.const 132) (i32.const 1)) \
             (i64.store (i32.const 144) (i64.const 131072)) (i64.store (i32.const 152) (i64.const -1)) \
             (drop (call $action (i32.const {MODIFY}) (i64.const {target}) (i32.const 128) (i32.const 72) (i32.const 256))) \
             (memory.fill (i32.const 128) (i32.const 0) (i32.const 136))",
            prop("FirepowerMultiplier")
        )])
    };
    install(&mut with, &module(unit));
    install(&mut without, &module(0));
    let checksum = |scene: &mut HeadlessScenario| {
        scene
            .runtime
            .simulation
            .compute_multiplayer_frame_checksum()
            .expect("checksum")
            .value
    };
    assert_eq!(checksum(&mut with), checksum(&mut without));
    with.tick();
    without.tick();
    assert!(!with.sim().ext_modules().modifiers().is_empty());
    assert!(without.sim().ext_modules().modifiers().is_empty());
    // One digest per module, and one more over the held modifier.
    assert_eq!(with.sim().ext_modules().checksum_digests().count(), 2);
    assert_eq!(without.sim().ext_modules().checksum_digests().count(), 1);
    assert_ne!(checksum(&mut with), checksum(&mut without));
}

/// WAT adding a modifier through `Modify`: `prop`, `op`, raw `value`,
/// `frames` (−1 unlimited); the handle goes to 256, the status to
/// `status_at`.
fn modify_wat(target: u64, prop: i32, op: i32, value: i64, frames: i64, status_at: u32) -> String {
    format!(
        "(memory.fill (i32.const 128) (i32.const 0) (i32.const 72)) \
         (i32.store (i32.const 128) (i32.const {prop})) (i32.store (i32.const 132) (i32.const {op})) \
         (i64.store (i32.const 144) (i64.const {value})) (i64.store (i32.const 152) (i64.const {frames})) \
         (i32.store (i32.const {status_at}) (call $action (i32.const {MODIFY}) (i64.const {target}) (i32.const 128) (i32.const 72) (i32.const 256)))"
    )
}

/// `WeaponRangeMultiplier` and `WeaponRangeExtra` reach the range owner and
/// `DisableWeapons` makes GetFireError answer Rearm; the always-in-range
/// sentinel stays.
#[test]
fn range_and_disable_weapons_modifiers_reach_their_owners() {
    use crate::sim::attach_effect::range_with_attach_effects;
    use crate::sim::combat::TargetKind;
    use crate::sim::combat::fire_error::FireError;
    use crate::sim::combat::fire_error_world::FireSubject;
    let Some(mut scene) = scenario() else { return };
    let (unit, (x, y)) = ground_object(&scene);
    let range = |scene: &HeadlessScenario, leptons: i32| {
        let entity = scene.sim().entities().get(unit).expect("unit lives");
        range_with_attach_effects(entity, "", leptons, &scene.runtime.resources.rules)
    };
    assert_eq!(range(&scene, 1024), 1024);
    install(
        &mut scene,
        &actions_module(&[
            // Multiply by 2.0, add 128 leptons, Override 1.
            modify_wat(unit, prop("WeaponRangeMultiplier"), 1, 131_072, -1, 0),
            modify_wat(unit, prop("WeaponRangeExtra"), 0, 128, -1, 4),
            modify_wat(unit, prop("DisableWeapons"), 2, 1, -1, 8),
        ]),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!([0, 4, 8].map(|at| mem_i32(&scene, at)), [0; 3]);
    assert_eq!(range(&scene, 1024), 2048 + 128);
    assert_eq!(range(&scene, -512), -512, "the sentinel stays");
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let firer = sim.entities().get(unit).expect("unit lives");
    let obj = sim.object_type(firer.type_ref(), rules).expect("type");
    let error = FireSubject {
        world: sim,
        rules,
        fog: None,
        firer,
        obj,
        target: Some(TargetKind::Cell(x + 1, y)),
        weapon_index: 0,
    }
    .fire_error(true);
    assert_eq!(error, FireError::Rearm);
}

/// `ModifierExpire` on the bound object after a one-frame modifier
/// expires, carrying the handle and the property.
#[test]
fn an_expiring_modifier_raises_modifier_expire() {
    let Some(mut scene) = scenario() else { return };
    let (unit, _) = ground_object(&scene);
    let i32s = |values: &[i32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    // House 0, repeating, every difficulty, one ModifierExpire event, one
    // Module action (key 7) on the subject.
    let mut def = 0i64.to_le_bytes().to_vec();
    def.extend(i32s(&[2, 7, 0, -1, 1, 1]));
    def.extend(i32s(&[EVENT_MODIFIER_EXPIRE, 0, 0, -1]));
    def.extend(0i64.to_le_bytes());
    def.extend(i32s(&[0, 0]));
    def.extend(i32s(&[ACTION_MODULE, 0, 7, 0, 0, 0]));
    def.extend([0; 72 - 16]);
    let bytes: String = def.iter().map(|b| format!("\\{b:02x}")).collect();
    let firepower = prop("FirepowerMultiplier");
    install(
        &mut scene,
        &format!(
            r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\80\03\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 0) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 4) (call $bind (i64.load (i32.const 64)) (i64.const {unit})))
    {modify})
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (if (i32.eqz (i32.load (i32.const 80)))
      (then (memory.copy (i32.const 512) (local.get $ptr) (i32.const 120))))
    (i32.store (i32.const 80) (i32.add (i32.load (i32.const 80)) (local.get $count)))))"#,
            len = def.len(),
            modify = modify_wat(unit, firepower, 1, 131_072, 1, 8),
        ),
    );
    for _ in 0..3 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!([0, 4, 8].map(|at| mem_i32(&scene, at)), [0; 3]);
    assert!(scene.sim().ext_modules().modifiers().is_empty());
    let memory = scene.sim().ext_modules.slots[0].module.memory();
    let i64_at = |at: usize| i64::from_le_bytes(memory[at..at + 8].try_into().expect("8 bytes"));
    assert_eq!(mem_i32(&scene, 80), 1, "one record");
    assert_eq!(mem_i32(&scene, 512 + 4), EVENT_MODIFIER_EXPIRE);
    // subject, modifier and value (the property).
    assert_eq!(
        [i64_at(512 + 24), i64_at(512 + 56), i64_at(512 + 72)],
        [unit as i64, i64_at(256), i64::from(firepower)]
    );
}

/// `AllowType` 0 forbids a type to one house through CanBuild; another
/// house still builds it.
#[test]
fn allow_type_forbids_a_type_to_one_house() {
    use crate::rules::object_type::ObjectType;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::intern::InternedId;
    use crate::sim::production::{CanBuild, can_build};
    use crate::sim::world::Simulation;
    fn builds(sim: &Simulation, rules: &RuleSet, house: InternedId, obj: &ObjectType) -> bool {
        can_build(sim, rules, house, obj, true, false) != CanBuild::No
    }
    let Some(mut scene) = scenario() else { return };
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let [first, second] = [0, 1].map(|index| sim.houses.order()[index]);
    let (type_id, name) = rules
        .all_objects()
        .enumerate()
        .find(|(_, obj)| builds(sim, rules, first, obj) && builds(sim, rules, second, obj))
        .map(|(id, obj)| (id as i32, obj.id.clone()))
        .expect("a type both houses may build");
    install(
        &mut scene,
        &actions_module(&[action_wat(ALLOW_TYPE, house_ref(0), type_id, 0, 0, 0, 0)]),
    );
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0), 0, "AllowType answers Ok");
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let obj = rules.object(&name).expect("the type");
    assert!(!builds(sim, rules, first, obj));
    assert!(builds(sim, rules, second, obj));
}

/// A timed `Convert` turns a vehicle into another of its kind and back
/// once the frames are up.
#[test]
fn a_timed_convert_reverts() {
    let Some(mut scene) = scenario() else { return };
    let rules = &scene.runtime.resources.rules;
    let sim = scene.sim();
    let (unit, from) = sim
        .entities()
        .iter_sorted()
        .filter(|(_, e)| {
            e.category() == EntityCategory::Unit
                && e.lifecycle.object_alive()
                && !e.lifecycle.in_limbo()
        })
        .find_map(|(id, e)| Some((id, sim.entity_object_type(e, rules)?)))
        .expect("the map has a vehicle");
    let (to, to_name) = rules
        .all_objects()
        .enumerate()
        .find(|(_, obj)| {
            obj.category == from.category
                && obj.id != from.id
                && obj.locomotor == from.locomotor
                && obj.speed_type == from.speed_type
        })
        .map(|(id, obj)| (id as i32, obj.id.clone()))
        .expect("another vehicle of the same kind");
    let from_name = from.id.clone();
    install(
        &mut scene,
        &actions_module(&[format!(
            "(memory.fill (i32.const 128) (i32.const 0) (i32.const 72)) \
             (i32.store (i32.const 128) (i32.const {to})) (i64.store (i32.const 152) (i64.const 2)) \
             (i32.store (i32.const 0) (call $action (i32.const {CONVERT}) (i64.const {unit}) (i32.const 128) (i32.const 72) (i32.const 256)))"
        )]),
    );
    let type_name = |scene: &HeadlessScenario| {
        let sim = scene.sim();
        let entity = sim.entities().get(unit).expect("unit lives");
        sim.entity_object_type(entity, &scene.runtime.resources.rules)
            .map(|obj| obj.id.clone())
    };
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0), 0, "Convert answers Ok");
    assert_eq!(type_name(&scene).as_deref(), Some(to_name.as_str()));
    for _ in 0..3 {
        scene.tick();
    }
    assert_eq!(type_name(&scene).as_deref(), Some(from_name.as_str()));
}
