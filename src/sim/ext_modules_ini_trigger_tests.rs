//! Module triggers from the mod's INI on a retail map: an E1 bound through
//! `Wasm.Tags` delivers a filtered hit to its `Wasm:` action and runs a stock
//! action at once, a mistake refuses the start, and the triggers survive a
//! save and a stopped module.

use vera20k_wasm_ext::triggers::{ACTION_MODULE, EVENT_TAKE_DAMAGE};
use vera20k_wasm_ext::{EventRecord, Export, Fault};

use super::ini_triggers::IniTriggers;
use super::{ModuleSource, PreparedExtModules};
use crate::headless_scenario::HeadlessScenario;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

/// The module action's dispatch key, and the local variable `SetLocal` sets.
const KEY: i32 = 7;
const LOCAL: u32 = 9;

/// The RFC's binding example with a stock action.
const GI_PANIC: &str = "\
[E1]
Wasm.Tags=GIPanicTag
[GIPanicTag]
Trigger=GIPanic
[GIPanic]
Event=TakeDamage
Where1=Subject.HealthPercent<30
Action1=SetLocal(,9)
Action2=Wasm:giPanic
Repeat=Repeating
";

/// Names `giPanic` (key [`KEY`]) in `vera.actions`; `on_events` copies the
/// first record it gets to 512 and counts records at 80.
fn module() -> Vec<u8> {
    wat::parse_str(
        r#"(module
  (@custom "vera.delivery" "\01\00\04\00\00\80\03\00\00")
  (@custom "vera.actions" "\01\01\00\00\00\07\00giPanic\07\00\00\00")
  (memory (export "memory") 1)
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (if (i32.eqz (i32.load (i32.const 80)))
      (then (memory.copy (i32.const 512) (local.get $ptr) (i32.const 120))))
    (i32.store (i32.const 80) (i32.add (i32.load (i32.const 80)) (local.get $count)))))"#,
    )
    .expect("test module parses")
}

/// XMP03T4 with the mod module installed and `ini`'s triggers built.
fn scene_with(ini: &str) -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    let mut scene = crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269)
        .expect("retail scenario loads");
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    sim.install_ext_module(rules, ModuleSource::Mod, &module())
        .expect("module installs");
    let triggers = IniTriggers::read(&IniFile::from_str(ini)).expect("INI reads");
    sim.install_ini_triggers(rules, &triggers)
        .expect("INI triggers build");
    Some(scene)
}

/// An E1 spawned after the triggers were built, beside the map's first
/// object, for that object's owner.
fn spawn_e1(scene: &mut HeadlessScenario) -> u64 {
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    let (bx, by, owner) = sim
        .entities()
        .iter_sorted()
        .find(|(_, e)| !e.lifecycle.in_limbo())
        .map(|(_, e)| {
            let owner = sim.interner.resolve(e.owner()).to_string();
            (e.position.rx(), e.position.ry(), owner)
        })
        .expect("the map has an object");
    for n in 0..400 {
        let (Ok(rx), Ok(ry)) = (
            u16::try_from(i32::from(bx) + n % 20 - 10),
            u16::try_from(i32::from(by) + n / 20 - 10),
        ) else {
            continue;
        };
        let walkable = sim
            .path_grid()
            .is_some_and(|grid| grid.is_walkable_for_infantry(rx, ry));
        let empty = sim.occupancy().get(rx, ry).is_none_or(|cell| {
            cell.is_empty_on(crate::sim::movement::locomotor::MovementLayer::Ground)
        });
        if !walkable || !empty {
            continue;
        }
        let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
        if let Some(id) = sim.spawn_object_at_height("E1", owner.as_str(), rx, ry, 0, z, rules) {
            return id;
        }
    }
    panic!("no free cell for an E1");
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

/// A hit that leaves the E1 at 20% health.
fn hit_to_20_percent(scene: &mut HeadlessScenario, e1: u64) {
    let health = scene.sim().entities().get(e1).expect("E1").health.current();
    let strength = scene
        .runtime
        .resources
        .rules
        .object("E1")
        .expect("E1")
        .strength;
    hit(scene, e1, health - strength / 5);
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn received(scene: &HeadlessScenario) -> i32 {
    i32::from_le_bytes(memory(scene)[80..84].try_into().expect("4 bytes"))
}

/// The handle of the trigger bound to `e1`'s type.
fn e1_trigger(scene: &HeadlessScenario, e1: u64) -> u64 {
    let e1_type = scene
        .runtime
        .resources
        .rules
        .type_handle("E1")
        .expect("E1")
        .0;
    let bound = scene.sim().trigger_runtime.module_triggers().bound(
        EVENT_TAKE_DAMAGE,
        e1,
        Some(e1_type),
        &[],
    );
    assert_eq!(bound.len(), 1, "one trigger reaches the E1");
    bound[0]
}

#[test]
fn an_e1_tag_trigger_runs_its_stock_action_and_delivers_a_filtered_hit() {
    let Some(mut scene) = scene_with(GI_PANIC) else {
        return;
    };
    let e1 = spawn_e1(&mut scene);
    hit(&mut scene, e1, 10);
    scene.tick();
    assert_eq!(received(&scene), 0, "the E1 is above 30%");
    assert!(!scene.sim().trigger_runtime.local_variable_is_set(LOCAL));
    hit_to_20_percent(&mut scene, e1);
    assert!(
        scene.sim().trigger_runtime.local_variable_is_set(LOCAL),
        "SetLocal runs at the hit"
    );
    scene.tick();
    assert_eq!(received(&scene), 1);
    let record = &memory(&scene)[512..632];
    let trigger = e1_trigger(&scene, e1);
    assert_eq!(&record[..4], &KEY.to_le_bytes());
    assert_eq!(&record[4..8], &EVENT_TAKE_DAMAGE.to_le_bytes());
    assert_eq!(&record[8..16], &(trigger as i64).to_le_bytes());
    assert_eq!(&record[24..32], &(e1 as i64).to_le_bytes());
    assert_eq!(record.len(), EventRecord::SIZE);

    // A stopped module takes only the trigger's module action along.
    scene
        .runtime
        .simulation
        .disable_ext_module(0, Export::Frame, Fault::Budget);
    let triggers = scene.sim().trigger_runtime.module_triggers();
    let actions = &triggers
        .get(trigger)
        .expect("the INI trigger stays")
        .actions;
    assert!(actions.iter().all(|a| a.kind != ACTION_MODULE));
    hit(&mut scene, e1, 1);
    scene.tick();
    let triggers = scene.sim().trigger_runtime.module_triggers();
    assert_eq!(triggers.queued(ModuleSource::Mod), 0);
}

#[test]
fn a_mistake_in_the_ini_refuses_the_start_naming_section_and_key() {
    let misspelt = GI_PANIC.replace("Event=", "Evnt=");
    let refused = PreparedExtModules::load(&IniFile::from_str(&misspelt), None, &IniFile::empty(), 0, 0)
        .err()
        .expect("a misspelt key refuses");
    assert!(
        refused.starts_with("the mod's rulesmd.ini: [GIPanic] Evnt"),
        "{refused}"
    );
    let refusal = |ini: &str| {
        let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
        let mut scene = crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269)
            .expect("retail scenario loads");
        let triggers = IniTriggers::read(&IniFile::from_str(ini)).expect("the syntax is right");
        let rules = &scene.runtime.resources.rules;
        scene
            .runtime
            .simulation
            .install_ini_triggers(rules, &triggers)
            .err()
    };
    let Some(no_module) = refusal(GI_PANIC) else {
        return;
    };
    assert!(
        no_module.contains("[GIPanic] Action2=Wasm:giPanic: the mod has no rules.wasm"),
        "{no_module}"
    );
    let unknown = refusal(&GI_PANIC.replace("HealthPercent", "HealthPercnt"))
        .expect("an unknown property refuses");
    assert!(
        unknown.contains("[GIPanic] Where1: no property HealthPercnt"),
        "{unknown}"
    );
}

#[test]
fn ini_triggers_survive_a_save() {
    use crate::sim::snapshot::GameSnapshot;
    let Some(mut live) = scene_with(GI_PANIC) else {
        return;
    };
    let e1 = spawn_e1(&mut live);
    hit_to_20_percent(&mut live, e1);
    let rules = &live.runtime.resources.rules;
    let bytes = GameSnapshot::save_validated(
        live.sim(),
        0,
        rules.simulation_config_hash(),
        "INI triggers",
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
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets().expect("retail");
    let mut resumed =
        crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269).expect("second load");
    resumed.runtime.simulation = loaded;
    let trigger = e1_trigger(&resumed, e1);
    assert!(
        resumed
            .sim()
            .trigger_runtime
            .module_triggers()
            .get(trigger)
            .expect("kept")
            .ini
    );
    resumed.tick();
    assert_eq!(received(&resumed), 1, "the queued record");
    // The trigger itself came back: a later hit fires it again (the E1
    // has healed above 30% meanwhile).
    hit_to_20_percent(&mut resumed, e1);
    resumed.tick();
    assert_eq!(received(&resumed), 2);
}
