//! Trigger actions 99/116 and `ObjectClass::AI`'s sound block through the
//! production map reader, trigger poll and object visits. Control flow is
//! instruction-level reading (see the module doc); these are Rust
//! regressions, not native executions.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::world::TriggerInputs;

fn sounds() -> SoundRegistry {
    SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=TestAmbient\n1=CowAmbient\n\
         [TestAmbient]\nSounds=amb1\n[CowAmbient]\nSounds=cow1\n",
    ))
}

/// One Neutral trigger on a repeat-0 Tag whose only event is 8 ("any event")
/// and whose only action is `action`, plus waypoint 1 at cell (5, 6).
fn sim_with_trigger(action: &str) -> (Simulation, crate::map::map_file::MapFile) {
    let text = format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Waypoints]\n1=6005\n\
         [Triggers]\nAMB=Neutral,<none>,Ambient,0,1,1,1,0\n\
         [Events]\nAMB=1,8,0,0\n\
         [Actions]\nAMB=1,{action}\n\
         [Tags]\nAMBTAG=0,Ambient tag,AMB\n"
    );
    let map = crate::map::map_file::MapFile::from_bytes(text.as_bytes()).unwrap();
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        Some(&sounds()),
    );
    sim.classify_map_triggers(None);
    (sim, map)
}

fn poll(sim: &mut Simulation, map: &crate::map::map_file::MapFile) {
    sim.advance_triggers(
        TriggerInputs {
            events: &map.events,
            actions: &map.actions,
            rules: None,
        },
        None,
        FrameEffects::default(),
    );
}

fn insert_structure(sim: &mut Simulation, id: u64, type_name: &str, cell: (u16, u16)) {
    let owner = sim.interner.intern("Neutral");
    let type_ref = sim.interner.intern(type_name);
    let entity = GameEntity::new_at_frame_zero_for_test(
        id,
        cell.0,
        cell.1,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(id + 1);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(id);
}

/// The waypoint cell's centre (`trigger_waypoint_coord` on flat level-0
/// ground).
const WAYPOINT_COORD: DriveCoord = DriveCoord {
    x: 5 * 256 + 128,
    y: 6 * 256 + 128,
    z: 0,
};

#[test]
fn event_8_springs_on_the_first_logic_poll_and_plays_action_99_at_the_waypoint() {
    let (mut sim, map) = sim_with_trigger("99,7,TestAmbient,0,0,0,0,B");
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocAtCoord { sound_id, coordinate, flags: 1 }]
            if sound_id == "TestAmbient" && *coordinate == WAYPOINT_COORD
    ));
    // Repeat 0: the Tag is gone, so later polls play nothing.
    sim.sound_events.clear();
    poll(&mut sim, &map);
    assert!(sim.sound_events.is_empty());
}

#[test]
fn action_99_with_an_unregistered_name_plays_nothing_at_an_empty_waypoint() {
    let (mut sim, map) = sim_with_trigger("99,7,NoSuchSound,0,0,0,0,B");
    poll(&mut sim, &map);
    assert!(sim.sound_events.is_empty());
}

#[test]
fn action_99_gives_the_waypoint_building_its_custom_sound_for_object_ai_to_play() {
    let (mut sim, map) = sim_with_trigger("99,7,tEsTaMbIeNt,0,0,0,0,B");
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    poll(&mut sim, &map);
    // FindIndex folds case; the object takes the sound, nothing plays yet.
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.object_custom_sounds.get(&40).map(String::as_str),
        Some("TestAmbient")
    );

    sim.object_ai_sounds(40, None, FrameEffects::default());
    let world = Simulation::movement_sound_world(sim.substrate.entities.get(40).unwrap());
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, sound_id, world: at }]
            if *anim_id == custom_sound_owner(40) && sound_id == "TestAmbient" && *at == world
    ));
    // PlayAt on each visit drives the handle; no 750D40 owner coordinate.
    assert_eq!(
        sim.object_sound_owner_alive(custom_sound_owner(40)),
        Some(true)
    );
    assert_eq!(sim.looping_sound_owner_coord(custom_sound_owner(40)), None);

    // ObjectClass::AI skips both handles while the object is in Limbo, and
    // Limbo detaches the custom handle but keeps +0x64.
    sim.sound_events.clear();
    sim.object_conceal(40);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(40)
    ));
    sim.sound_events.clear();
    sim.object_ai_sounds(40, None, FrameEffects::default());
    assert!(sim.sound_events.is_empty());
    assert!(sim.object_custom_sounds.contains_key(&40));
    assert_eq!(
        sim.object_sound_owner_alive(custom_sound_owner(40)),
        Some(false)
    );
}

#[test]
fn action_99_with_minus_one_clears_the_custom_sound_and_detaches_its_handle() {
    let (mut sim, map) = sim_with_trigger("99,7,NoSuchSound,0,0,0,0,B");
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    sim.object_custom_sounds
        .insert(40, "TestAmbient".to_owned());
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(40)
    ));
    assert!(sim.object_custom_sounds.is_empty());
}

fn insert_tree(sim: &mut Simulation, id: u64, cell: (u16, u16)) {
    let type_ref = sim.interner.intern("TREE01");
    sim.production.terrain_objects.insert(
        id,
        TerrainObjectState::for_test(id, type_ref, cell.0, cell.1),
    );
    sim.production.terrain_object_cells.insert(cell, id);
}

#[test]
fn action_99_takes_the_cell_terrain_without_a_building_and_its_uninit_detaches() {
    let (mut sim, map) = sim_with_trigger("99,7,TestAmbient,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    poll(&mut sim, &map);
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.object_custom_sounds.get(&41).map(String::as_str),
        Some("TestAmbient")
    );

    // TerrainClass::AI calls ObjectClass::AI first.
    sim.object_ai_sounds(41, None, FrameEffects::default());
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, sound_id, world }]
            if *anim_id == custom_sound_owner(41)
                && sound_id == "TestAmbient"
                && (world.x, world.y, world.z) == (WAYPOINT_COORD.x, WAYPOINT_COORD.y, 0)
    ));

    // The tree's UnInit Limbo detaches its handle.
    sim.sound_events.clear();
    assert!(sim.retire_non_entity_object(41));
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(41)
    ));
}

#[test]
fn action_99_prefers_the_cell_building_over_its_terrain() {
    let (mut sim, map) = sim_with_trigger("99,7,TestAmbient,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    poll(&mut sim, &map);
    assert_eq!(
        sim.object_custom_sounds.keys().copied().collect::<Vec<_>>(),
        [40]
    );
}

#[test]
fn action_116_clears_the_first_building_instead_of_terrain_or_coordinate_requests() {
    let (mut sim, map) = sim_with_trigger("116,0,0,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    sim.set_custom_sound(40, Some("TestAmbient"));
    sim.set_custom_sound(41, Some("CowAmbient"));
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(40)
    ));
    assert!(!sim.object_custom_sounds.contains_key(&40));
    assert_eq!(
        sim.object_custom_sounds.get(&41).map(String::as_str),
        Some("CowAmbient")
    );
    // Object AI can no longer recreate +50; Terrain's independent handle survives.
    sim.sound_events.clear();
    sim.object_ai_sounds(40, None, FrameEffects::default());
    assert!(sim.sound_events.is_empty());
    sim.object_ai_sounds(41, None, FrameEffects::default());
    assert!(matches!(sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, .. }] if *anim_id == custom_sound_owner(41)));
}

#[test]
fn action_116_clears_the_cell_terrain_without_a_building() {
    let (mut sim, map) = sim_with_trigger("116,0,0,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    sim.set_custom_sound(41, Some("TestAmbient"));
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(41)
    ));
    assert!(sim.object_custom_sounds.is_empty());
}

#[test]
fn action_116_without_an_object_preserves_full_waypoint_and_request_flags() {
    let (mut sim, map) = sim_with_trigger("116,0,0,0,0,0,0,B");
    let before = sim.rng_state();
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocStopAtCoord { coordinate, flags: 1 }]
            if *coordinate == WAYPOINT_COORD
    ));
    assert_eq!(sim.rng_state(), before);
}

#[test]
fn action_116_zero_cell_still_stops_coordinate_requests() {
    let (mut sim, mut map) = sim_with_trigger("116,0,0,0,0,0,0,B");
    map.waypoints.get_mut(&1).unwrap().rx = 0;
    map.waypoints.get_mut(&1).unwrap().ry = 0;
    sim.session.install_waypoints(&map.waypoints);
    insert_structure(&mut sim, 40, "CAWASH18", (0, 0));
    sim.set_custom_sound(40, Some("TestAmbient"));
    let expected = sim.trigger_waypoint_coord(1).unwrap();
    poll(&mut sim, &map);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocStopAtCoord { coordinate, flags: 1 }]
            if [coordinate.x, coordinate.y, coordinate.z] == expected
    ));
    assert!(sim.object_custom_sounds.contains_key(&40));
}

#[test]
fn a_type_ambient_sound_plays_every_visit_and_limbo_hard_stops_it() {
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=COW\n[COW]\nAmbientSound=CowAmbient\n\
         [BuildingTypes]\n0=CAWASH18\n[CAWASH18]\n",
    );
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds());
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Neutral");
    let type_ref = sim.interner.intern("COW");
    let cow = GameEntity::new_at_frame_zero_for_test(
        50,
        3,
        4,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(cow);
    sim.add_entity_occupancy(50);
    let world = Simulation::movement_sound_world(sim.substrate.entities.get(50).unwrap());
    for _ in 0..2 {
        sim.sound_events.clear();
        sim.object_ai_sounds(50, Some(&rules), FrameEffects::default());
        assert!(matches!(
            sim.sound_events.as_slice(),
            [SimSoundEvent::AnimationStarted { anim_id, sound_id, world: at }]
                if *anim_id == ambient_sound_owner(50) && sound_id == "CowAmbient" && *at == world
        ));
    }
    // A type without AmbientSound plays nothing.
    insert_structure(&mut sim, 51, "CAWASH18", (8, 8));
    sim.sound_events.clear();
    sim.object_ai_sounds(51, Some(&rules), FrameEffects::default());
    assert!(sim.sound_events.is_empty());

    sim.object_sound_conceal(50, Some(&rules));
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStopped { anim_id, stop_sound_id: None, .. }]
            if *anim_id == ambient_sound_owner(50)
    ));
}

#[test]
fn a_custom_sound_reaches_the_hash_and_a_snapshot_restore_rejects_its_missing_object() {
    let mut sim = Simulation::new();
    let empty = sim.state_hash();
    sim.object_custom_sounds
        .insert(77, "TestAmbient".to_owned());
    assert_ne!(sim.state_hash(), empty, "+0x64 reaches the state hash");

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "custom_sound", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(
        restored.object_custom_sounds.get(&77).map(String::as_str),
        Some("TestAmbient")
    );
    // No Building or Terrain 77 exists, so the sound has no owner.
    let error = restored.restore_after_snapshot_load().unwrap_err();
    assert!(
        matches!(
            error,
            crate::sim::snapshot::SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "ObjectCustomSounds",
                source_id: 77,
                ..
            }
        ),
        "{error:?}"
    );
}
