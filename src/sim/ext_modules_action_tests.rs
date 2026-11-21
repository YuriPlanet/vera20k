//! The atomic actions added beside `set` and the modifier store, a trigger
//! `Log` action's copied text, and modifiers in the multiplayer checksum,
//! on a retail map.

use vera20k_wasm_ext::triggers::EVENT_TIMER;

use super::actions::{DETONATE_WEAPON, HEAL, LOG, MODIFY, SET_MISSION, SET_PROPERTY};
use super::{ModuleSource, cell_ref};
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
