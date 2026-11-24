//! INI names a module registers (RFC 0001 section 6h) on a
//! retail map: `vera_init` registers a registry, a key and an event; the
//! first module phase hands over `IniEntry` and `IniKey` before
//! `on_scenario_start`; an INI trigger raises the module event with
//! `RaiseEvent` and another, listening to it through `Event=Wasm:`, queues a
//! module action with its target and parameter text.

use vera20k_wasm_ext::triggers::{EVENT_INI_ENTRY, EVENT_INI_KEY, EVENT_MODULE};
use vera20k_wasm_ext::{EventRecord, NAME_TECHNO_TYPE};

use super::ini_triggers::IniTriggers;
use super::{ModuleSource, type_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::world::FrameEffects;

/// `onBoom`'s dispatch key and the local variable the listener sets.
const KEY: i32 = 7;
const LOCAL: u32 = 9;

/// The merged rules text the start records read.
const RULES: &str = "\
[Test.Types]
1=HeavyShield
2=heavyshield
3=LightShield
[E1]
Test.Shield=HeavyShield
[E2]
Test.Shield=
";

/// The mod's INI triggers: a hit on an E1 raises `Test.Boom` on it, which
/// sets a local and queues `onBoom` for the other object with text.
const TRIGGERS: &str = "\
[E1]
Wasm.Tags=HitTag,BoomTag
[HitTag]
Trigger=Hit
[Hit]
Event=TakeDamage
Action1=RaiseEvent(Subject,Test.Boom,3,4)
Repeat=Repeating
[BoomTag]
Trigger=Boom
[Boom]
Event=Wasm:Test.Boom
Action1=SetLocal(,9)
Action2=Wasm:onBoom(Other,50, extra)
Repeat=Repeating
";

/// `vera_init` registers `Test.Types` (key 21), `Test.Shield` on
/// TechnoTypes (key 22) and the object event `Test.Boom` (its ID at 96).
/// `on_events` counts `IniEntry` at 80, `IniKey` at 84 and other records at
/// 88, copying the first of each to 256, 384 and 512.
fn module() -> Vec<u8> {
    wat::parse_str(
        r#"(module
  (import "vera:sim@1.0" "ini_register" (func $reg (param i32 i32 i32 i32 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\80\03\00\00")
  (@custom "vera.actions" "\01\01\00\00\00\06\00onBoom\07\00\00\00")
  (memory (export "memory") 1)
  (data (i32.const 0) "Test.Types")
  (data (i32.const 16) "Test.Shield")
  (data (i32.const 32) "Test.Boom")
  (func (export "vera_init")
    (drop (call $reg (i32.const 1) (i32.const 0) (i32.const 0) (i32.const 10) (i32.const 21) (i32.const 104)))
    (drop (call $reg (i32.const 0) (i32.const 0) (i32.const 16) (i32.const 11) (i32.const 22) (i32.const 104)))
    (i32.store (i32.const 100)
      (call $reg (i32.const 2) (i32.const 0) (i32.const 32) (i32.const 9) (i32.const -1) (i32.const 96))))
  (func $take (param $ptr i32) (param $at i32) (param $to i32)
    (if (i32.eqz (i32.load (local.get $at)))
      (then (memory.copy (local.get $to) (local.get $ptr) (i32.const 120))))
    (i32.store (local.get $at) (i32.add (i32.load (local.get $at)) (i32.const 1))))
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (loop $l
      (if (i32.eq (i32.load offset=4 (local.get $ptr)) (i32.const 1041))
        (then (call $take (local.get $ptr) (i32.const 80) (i32.const 256)))
        (else
          (if (i32.eq (i32.load offset=4 (local.get $ptr)) (i32.const 1042))
            (then (call $take (local.get $ptr) (i32.const 84) (i32.const 384)))
            (else (call $take (local.get $ptr) (i32.const 88) (i32.const 512))))))
      (local.set $ptr (i32.add (local.get $ptr) (i32.const 120)))
      (br_if $l (local.tee $count (i32.sub (local.get $count) (i32.const 1)))))))"#,
    )
    .expect("test module parses")
}

fn scene() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    let mut scene = crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269)
        .expect("retail scenario loads");
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    sim.install_ext_module(rules, ModuleSource::Mod, &module())
        .expect("module installs");
    sim.ext_modules.rules_ini = Some(IniFile::from_str(RULES));
    let triggers = IniTriggers::read(&IniFile::from_str(TRIGGERS)).expect("INI reads");
    sim.install_ini_triggers(rules, &triggers)
        .expect("INI triggers build");
    Some(scene)
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory()
}

fn i32_at(scene: &HeadlessScenario, at: usize) -> i32 {
    i32::from_le_bytes(memory(scene)[at..at + 4].try_into().expect("4 bytes"))
}

fn i64_at(bytes: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

fn string(scene: &HeadlessScenario, id: i32) -> Option<String> {
    scene.sim().ext_modules.ini.string(id).map(str::to_owned)
}

/// An E1 beside the map's first object, for that object's owner.
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
fn registered_names_reach_their_handlers_and_a_raised_event_fires_its_listener() {
    let Some(mut scene) = scene() else {
        eprintln!("SKIPPED: retail install not found");
        return;
    };
    assert_eq!(i32_at(&scene, 100), 0, "Test.Boom registers (Ok)");
    let event = i32_at(&scene, 96);
    assert_eq!(
        scene.sim().ext_modules.ini.event_id("test.boom"),
        Some(event)
    );

    // The first module phase: the registry's two distinct entries, then the
    // one TechnoType whose key has a value.
    scene.tick();
    assert_eq!(i32_at(&scene, 80), 2, "IniEntry per distinct entry");
    assert_eq!(i32_at(&scene, 84), 1, "IniKey for E1 only");
    assert_eq!(i32_at(&scene, 88), 0);
    let entry = memory(&scene)[256..376].to_vec();
    assert_eq!(&entry[..4], &21i32.to_le_bytes());
    assert_eq!(&entry[4..8], &EVENT_INI_ENTRY.to_le_bytes());
    assert_eq!(i64_at(&entry, 72), 0, "first position");
    let args = i32::from_le_bytes(entry[108..112].try_into().expect("args"));
    assert_eq!(string(&scene, args).as_deref(), Some("HeavyShield"));
    let key = memory(&scene)[384..504].to_vec();
    assert_eq!(&key[..4], &22i32.to_le_bytes());
    assert_eq!(&key[4..8], &EVENT_INI_KEY.to_le_bytes());
    let e1_type = scene.runtime.resources.rules.type_handle("E1").expect("E1");
    assert_eq!(i64_at(&key, 24), type_ref(NAME_TECHNO_TYPE, e1_type.0));
    let args = i32::from_le_bytes(key[108..112].try_into().expect("args"));
    assert_eq!(string(&scene, args).as_deref(), Some("HeavyShield"));

    // A hit raises Test.Boom on the E1: the listener's stock action runs at
    // once, its module action reaches the next module phase.
    let e1 = spawn_e1(&mut scene);
    hit(&mut scene, e1, 1);
    assert!(
        scene.sim().trigger_runtime.local_variable_is_set(LOCAL),
        "SetLocal runs where the event is raised"
    );
    scene.tick();
    assert_eq!(i32_at(&scene, 88), 1, "one module action record");
    let record = memory(&scene)[512..632].to_vec();
    assert_eq!(record.len(), EventRecord::SIZE);
    assert_eq!(&record[..4], &KEY.to_le_bytes());
    assert_eq!(&record[4..8], &EVENT_MODULE.to_le_bytes());
    assert_eq!(i64_at(&record, 24), e1 as i64, "subject");
    assert_eq!((i64_at(&record, 72), i64_at(&record, 80)), (3, 4), "values");
    assert_eq!(&record[88..92], &event.to_le_bytes(), "event ID");
    let args = i32::from_le_bytes(record[108..112].try_into().expect("args"));
    assert_eq!(string(&scene, args).as_deref(), Some("50, extra"));
    assert_eq!(&record[112..116], &1i32.to_le_bytes(), "target Other");

    // The start records are not delivered again.
    scene.tick();
    assert_eq!((i32_at(&scene, 80), i32_at(&scene, 84)), (2, 1));
}

#[test]
fn an_unregistered_module_event_refuses_the_ini_trigger() {
    let Some(mut scene) = scene() else {
        eprintln!("SKIPPED: retail install not found");
        return;
    };
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    let triggers = IniTriggers::read(&IniFile::from_str(
        "[E1]\nWasm.Tags=T\n[T]\nTrigger=X\n[X]\nEvent=Wasm:Test.Nothing\nAction1=SetLocal(,9)\n",
    ))
    .expect("INI reads");
    let error = sim
        .install_ini_triggers(rules, &triggers)
        .expect_err("an unknown event refuses the start");
    assert!(error.contains("Test.Nothing"), "{error}");
}
