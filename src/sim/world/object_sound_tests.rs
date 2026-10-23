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
    sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
    (sim, map)
}

fn poll(sim: &mut Simulation) {
    sim.advance_triggers(
        crate::rules::ruleset::RuleSet::empty_for_test(),
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
    sim.set_next_stable_object_id_for_test(sim.next_stable_object_id().max(id + 1));
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(id);
}

/// The waypoint cell's centre (`trigger_waypoint_location` on flat level-0
/// ground).
const WAYPOINT_COORD: DriveCoord = DriveCoord {
    x: 5 * 256 + 128,
    y: 6 * 256 + 128,
    z: 0,
};

#[test]
fn event_8_springs_on_the_first_logic_poll_and_plays_action_99_at_the_waypoint() {
    let (mut sim, _) = sim_with_trigger("99,7,TestAmbient,0,0,0,0,B");
    poll(&mut sim);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocAtCoord { sound_id, coordinate, flags: 1 }]
            if sound_id == "TestAmbient" && *coordinate == WAYPOINT_COORD
    ));
    // Repeat 0: the Tag is gone, so later polls play nothing.
    sim.sound_events.clear_for_test();
    poll(&mut sim);
    assert!(sim.sound_events.is_empty());
}

#[test]
fn action_99_gives_the_waypoint_building_its_custom_sound_for_object_ai_to_play() {
    let (mut sim, _) = sim_with_trigger("99,7,tEsTaMbIeNt,0,0,0,0,B");
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    poll(&mut sim);
    // FindIndex folds case; the object takes the sound, nothing plays yet.
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.object_custom_sounds.get(&40).map(String::as_str),
        Some("TestAmbient")
    );

    sim.object_ai_sounds(
        40,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        FrameEffects::default(),
    );
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
    sim.sound_events.clear_for_test();
    sim.object_conceal(40);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(40)
    ));
    sim.sound_events.clear_for_test();
    sim.object_ai_sounds(
        40,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        FrameEffects::default(),
    );
    assert!(sim.sound_events.is_empty());
    assert!(sim.object_custom_sounds.contains_key(&40));
    assert_eq!(
        sim.object_sound_owner_alive(custom_sound_owner(40)),
        Some(false)
    );
}

#[test]
fn action_99_with_minus_one_clears_the_custom_sound_and_detaches_its_handle() {
    let (mut sim, _) = sim_with_trigger("99,7,NoSuchSound,0,0,0,0,B");
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    sim.object_custom_sounds
        .insert(40, "TestAmbient".to_owned());
    poll(&mut sim);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(40)
    ));
    assert!(sim.object_custom_sounds.is_empty());
}

fn insert_tree(sim: &mut Simulation, id: u64, cell: (u16, u16)) {
    let type_ref = sim.interner.intern("TREE01");
    sim.terrain_objects.objects_mut_for_test().insert(
        id,
        TerrainObjectState::for_test(id, type_ref, cell.0, cell.1),
    );
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert(cell, id);
}

#[test]
fn action_99_takes_the_cell_terrain_without_a_building_and_its_uninit_detaches() {
    let (mut sim, _) = sim_with_trigger("99,7,TestAmbient,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    poll(&mut sim);
    assert!(sim.sound_events.is_empty());
    assert_eq!(
        sim.object_custom_sounds.get(&41).map(String::as_str),
        Some("TestAmbient")
    );

    // TerrainClass::AI calls ObjectClass::AI first.
    sim.object_ai_sounds(
        41,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        FrameEffects::default(),
    );
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, sound_id, world }]
            if *anim_id == custom_sound_owner(41)
                && sound_id == "TestAmbient"
                && (world.x, world.y, world.z) == (WAYPOINT_COORD.x, WAYPOINT_COORD.y, 0)
    ));

    // The tree's UnInit Limbo detaches its handle.
    sim.sound_events.clear_for_test();
    assert!(sim.retire_non_entity_object(41, crate::sim::world::UninitContext::default()));
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::ObjectSoundDetached { owner }] if *owner == custom_sound_owner(41)
    ));
}

#[test]
fn action_116_clears_the_first_building_instead_of_terrain_or_coordinate_requests() {
    let (mut sim, _) = sim_with_trigger("116,0,0,0,0,0,0,B");
    insert_tree(&mut sim, 41, (5, 6));
    insert_structure(&mut sim, 40, "CAWASH18", (5, 6));
    sim.set_custom_sound(40, Some("TestAmbient"));
    sim.set_custom_sound(41, Some("CowAmbient"));
    poll(&mut sim);
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
    sim.sound_events.clear_for_test();
    sim.object_ai_sounds(
        40,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        FrameEffects::default(),
    );
    assert!(sim.sound_events.is_empty());
    sim.object_ai_sounds(
        41,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        FrameEffects::default(),
    );
    assert!(matches!(sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, .. }] if *anim_id == custom_sound_owner(41)));
}

#[test]
fn action_116_without_an_object_preserves_full_waypoint_and_request_flags() {
    let (mut sim, _) = sim_with_trigger("116,0,0,0,0,0,0,B");
    let before = sim.rng_state();
    poll(&mut sim);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocStopAtCoord { coordinate, flags: 1 }]
            if *coordinate == WAYPOINT_COORD
    ));
    assert_eq!(sim.rng_state(), before);
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
        sim.sound_events.clear_for_test();
        sim.object_ai_sounds(50, &rules, FrameEffects::default());
        assert!(matches!(
            sim.sound_events.as_slice(),
            [SimSoundEvent::AnimationStarted { anim_id, sound_id, world: at }]
                if *anim_id == ambient_sound_owner(50) && sound_id == "CowAmbient" && *at == world
        ));
    }
    // A type without AmbientSound plays nothing.
    insert_structure(&mut sim, 51, "CAWASH18", (8, 8));
    sim.sound_events.clear_for_test();
    sim.object_ai_sounds(51, &rules, FrameEffects::default());
    assert!(sim.sound_events.is_empty());

    sim.object_sound_conceal(50, &rules);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStopped { anim_id, stop_sound_id: None, .. }]
            if *anim_id == ambient_sound_owner(50)
    ));
}

#[test]
fn retail_terrain_ambient_reaches_logic_saved_load_and_lethal_cleanup() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    use crate::rules::retail_ini_fixture::retail_ini;

    let (Some(root), Some(art), Some(sound), Some(mode)) = (
        retail_ini("rulesmd.ini"),
        retail_ini("artmd.ini"),
        retail_ini("soundmd.ini"),
        retail_ini("mpbattlemd.ini"),
    ) else {
        return;
    };
    let catalog = std::sync::Arc::new(SoundRegistry::from_ini(&sound));
    assert!(catalog.get("OreMineExtract").is_some());
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art,
        catalog,
        Default::default(),
    )
    .unwrap();
    // Authored activation: retain the real drill/weapon data, permitting damage.
    let rules = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &IniFile::from_str("[TIBTRE01]\nAmbientSound=OreMineExtract\nImmune=no\n"),
        )
        .unwrap()
        .into_parts()
        .0;
    let mut sim = Simulation::new();
    sim.playfield_bounds =
        Some(crate::map::playfield::PlayfieldBounds::from_normalized_local_size(8, 0, 0, 8, 8));
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    crate::sim::terrain_spawn::construct_terrain_objects(
        &mut sim,
        &[crate::map::overlay::TerrainObject {
            name: "TIBTRE01".into(),
            rx: 5,
            ry: 6,
        }],
        &rules,
        false,
    );
    let id = *sim.terrain_objects.objects().keys().next().unwrap();
    crate::sim::terrain_object::mark_terrain_occupation(
        sim.terrain_objects.get(id).unwrap(),
        sim.resolved_terrain.as_mut(),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert_eq!(sim.live_object_order_snapshot(), [id]);
    sim.set_custom_sound(id, Some("CowAmbient"));
    sim.advance_live_object_pass(&rules, None, FrameEffects::default())
        .unwrap();
    let starts = |events: &[SimSoundEvent]| {
        assert!(matches!(events, [
            SimSoundEvent::AnimationStarted { anim_id: ambient, sound_id, world },
            SimSoundEvent::AnimationStarted { anim_id: custom, sound_id: custom_sound, .. },
        ] if *ambient == ambient_sound_owner(id) && sound_id == "OreMineExtract"
            && *custom == custom_sound_owner(id) && custom_sound == "CowAmbient"
            && (world.x, world.y, world.z) == (WAYPOINT_COORD.x, WAYPOINT_COORD.y, 0)));
    };
    starts(sim.sound_events.as_slice());
    sim.sound_events.clear_for_test();

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "terrain-ambient", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    let saved = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let processed = crate::util::save_codec::deserialize(&saved).unwrap();
    let loaded_rules = owner.project_saved_scenario_rules(&processed).unwrap();
    // The app restores the resolved map separately from retained save state.
    restored.resolved_terrain = sim.resolved_terrain.clone();
    assert!(restored.rebuild_dynamic_navigation(&loaded_rules));
    let zone_slots = |world: &Simulation| {
        let graph = world
            .zone_grid()
            .unwrap()
            .hierarchy_for(crate::rules::locomotor_type::MovementZone::Normal)
            .unwrap();
        std::array::from_fn::<_, 3, _>(|level| graph.level(level).unwrap().record_slot_count())
    };
    let initial_slots = zone_slots(&sim);
    assert_eq!(zone_slots(&restored), initial_slots);
    restored
        .advance_live_object_pass(&loaded_rules, None, FrameEffects::default())
        .unwrap();
    starts(restored.sound_events.as_slice());
    restored.sound_events.clear_for_test();

    // Direct UnInit and a lethal receiver must stop +3C before detaching +50.
    let stops = |events: &[SimSoundEvent]| {
        assert!(matches!(events, [
            SimSoundEvent::AnimationStopped { anim_id, stop_sound_id: None, .. },
            SimSoundEvent::ObjectSoundDetached { owner },
        ] if *anim_id == ambient_sound_owner(id) && *owner == custom_sound_owner(id)));
    };
    struct StopBoundary {
        id: u64,
        delivered: std::cell::Cell<usize>,
        stopped: std::cell::Cell<bool>,
    }
    impl crate::sim::world::SoundBoundary for StopBoundary {
        fn consume(&self, sim: &Simulation, _: &RuleSet, events: &[SimSoundEvent]) {
            for event in events.iter().skip(self.delivered.get()) {
                if matches!(event, SimSoundEvent::AnimationStopped { anim_id, .. }
                    if *anim_id == ambient_sound_owner(self.id))
                {
                    // ObjectLimbo5F4D79..5F4DD3: cell/Display are gone;
                    // the synchronous audio consumer still sees Live/Logic.
                    let terrain = sim.terrain_objects.get(self.id).unwrap();
                    assert!(terrain.is_live() && terrain.in_logic_vector());
                    assert_ne!(sim.terrain_objects.id_at((5, 6)), Some(self.id));
                    assert_eq!(sim.substrate.display.layer_of(self.id), None);
                    self.stopped.set(true);
                }
            }
            self.delivered.set(events.len());
        }
        fn delivered(&self) -> usize {
            self.delivered.get()
        }
    }
    let boundary = || StopBoundary {
        id,
        delivered: Default::default(),
        stopped: Default::default(),
    };
    let direct = boundary();
    assert!(
        sim.retire_non_entity_object(
            id,
            crate::sim::world::UninitContext::with_rules(&rules)
                .with_effects(FrameEffects::for_sound(&direct))
        )
    );
    assert!(direct.stopped.get());
    stops(sim.sound_events.as_slice());
    let direct_slots = zone_slots(&sim);
    assert_ne!(
        direct_slots, initial_slots,
        "installed hierarchy must exercise local repair"
    );
    let warhead_ref = restored.intern("AP");
    let lethal = boundary();
    restored.commit_noncombat_aoe_receivers(
        &loaded_rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Terrain(
            crate::sim::combat::TerrainDamageEvent {
                stable_id: id,
                rx: 5,
                ry: 6,
                damage: 1000,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            },
        )],
        FrameEffects::for_sound(&lethal),
    );
    assert!(lethal.stopped.get());
    stops(restored.sound_events.as_slice());
    assert_eq!(
        zone_slots(&restored),
        direct_slots,
        "native TerrainLimbo71CA2A..71CA51 publishes once for direct and lethal UnInit; receiver delivery must not replay it"
    );
    assert!(!restored.terrain_objects.get(id).unwrap().object_alive());
    assert!(restored.live_object_order_snapshot().is_empty());
    restored.sound_events.clear_for_test();
    restored
        .advance_live_object_pass(&loaded_rules, None, FrameEffects::default())
        .unwrap();
    assert!(restored.sound_events.is_empty());
}

#[test]
fn terrain_ambient_retains_only_its_reached_rules_reads() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(
            "[TerrainTypes]\n0=DRILL\n[DRILL]\nSpawnsTiberium=yes\nAmbientSound=testambient\n\
             [VehicleTypes]\n0=COLLIDE\n[COLLIDE]\nAmbientSound=CowAmbient\n",
        ),
        Some(IniFile::from_str("[DRILL]\nAmbientSound=Unknown\n")),
        IniFile::from_str("[DRILL]\nAmbientSound=CowAmbient\n"),
        std::sync::Arc::new(sounds()),
        Default::default(),
    )
    .unwrap();
    let mut rules = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&IniFile::from_str(
                "[DRILL]\nAmbientSound= \n",
            ))),
            &IniFile::from_str("[DRILL]\nAmbientSound=none\n"),
        )
        .unwrap()
        .into_parts()
        .0;
    // ObjectRead5F93FB retains the registered ID after empty/unknown/none;
    // TerrainRead71DEBB reads Rules, independently of fixed ART sound text.
    assert_eq!(
        rules
            .terrain_object_type_case_insensitive("DRILL")
            .unwrap()
            .ambient_sound(),
        Some("TestAmbient")
    );
    owner
        .find_or_allocate_map_terrain_type("COLLIDE", &mut rules)
        .unwrap()
        .unwrap();
    assert_eq!(
        rules
            .terrain_object_type_case_insensitive("COLLIDE")
            .unwrap()
            .ambient_sound(),
        None
    );
    let bytes = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let processed = crate::util::save_codec::deserialize(&bytes).unwrap();
    let restored = owner.project_saved_scenario_rules(&processed).unwrap();
    assert_eq!(
        restored
            .terrain_object_type_case_insensitive("DRILL")
            .unwrap()
            .ambient_sound(),
        Some("TestAmbient")
    );
    assert_eq!(
        restored
            .terrain_object_type_case_insensitive("COLLIDE")
            .unwrap()
            .ambient_sound(),
        None
    );
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

#[test]
fn sound_actions_resolve_the_selected_cell_coordinate_for_packed_waypoint_aliases() {
    for action in ["99,7,TestAmbient,0,0,0,0,B", "116,0,0,0,0,0,0,B"] {
        let (mut sim, mut map) = sim_with_trigger(action);
        let waypoint = map.waypoints.get_mut(&1).unwrap();
        waypoint.rx = 512;
        waypoint.ry = 0;
        sim.session.install_waypoints(&map.waypoints);
        // Map5657A0(512,0) aliases the allocated Cell(0,1). Native68BD00
        // reads that receiver's own centre, not the raw requested centre.
        poll(&mut sim);
        let expected = DriveCoord {
            x: 128,
            y: 384,
            z: 0,
        };
        assert!(matches!(sim.sound_events.as_slice(),
            [SimSoundEvent::VocAtCoord{coordinate,..} | SimSoundEvent::VocStopAtCoord{coordinate,..}]
                if *coordinate==expected));
    }
}

/// Full original Execute98/IsWaypointValid/642740/Scenario RNG controls;
///Voc750E20 is the declared output sink. Hashes cover complete native RNG state.
#[test]
fn waypoint_cue_random_sound_matches_original_selection_and_rng_without_terrain() {
    let cases: &[(u64, &[(u32, u16, u16)], [i32; 3], &str, &str)] = &[
        (
            0,
            &[(5, 12, 17)],
            [3200, 4480, 0],
            "d8df8547ba3a982ec559cb0e0094f4d307a0486faa263540841140debd754268",
            "d8df8547ba3a982ec559cb0e0094f4d307a0486faa263540841140debd754268",
        ),
        (
            0,
            &[(0, 1, 2), (7, 20, 25), (701, 65534, 8)],
            [-384, 2176, 0],
            "d8df8547ba3a982ec559cb0e0094f4d307a0486faa263540841140debd754268",
            "260edfa65df62897368d003460c3a41ea6185c13e6feacc6026dba11cdc687d4",
        ),
        (
            0,
            &[(0, 0, 0), (1, 0, 3), (701, 4, 0)],
            [1152, 128, 0],
            "d8df8547ba3a982ec559cb0e0094f4d307a0486faa263540841140debd754268",
            "4cbcb8d3b563344b5bc504f509a97b4e7ccb869d2d5c5fa7b69261dd83c848c9",
        ),
        (
            0,
            &[],
            [128, 128, 0],
            "d8df8547ba3a982ec559cb0e0094f4d307a0486faa263540841140debd754268",
            "4cbcb8d3b563344b5bc504f509a97b4e7ccb869d2d5c5fa7b69261dd83c848c9",
        ),
        (
            1,
            &[(5, 12, 17)],
            [3200, 4480, 0],
            "fe9ca09811e3e0e7e3fe06267ea3ed652674934d68f0fa7cf48e9d868bdad0db",
            "fe9ca09811e3e0e7e3fe06267ea3ed652674934d68f0fa7cf48e9d868bdad0db",
        ),
        (
            1,
            &[(0, 1, 2), (7, 20, 25), (701, 65534, 8)],
            [5248, 6528, 0],
            "fe9ca09811e3e0e7e3fe06267ea3ed652674934d68f0fa7cf48e9d868bdad0db",
            "dcb64bdf5580022dfdc2ba97609754a45b909f1fe2dc658eec4dc0ff92fef644",
        ),
        (
            1,
            &[(0, 0, 0), (1, 0, 3), (701, 4, 0)],
            [1152, 128, 0],
            "fe9ca09811e3e0e7e3fe06267ea3ed652674934d68f0fa7cf48e9d868bdad0db",
            "dcb64bdf5580022dfdc2ba97609754a45b909f1fe2dc658eec4dc0ff92fef644",
        ),
        (
            1,
            &[],
            [128, 128, 0],
            "fe9ca09811e3e0e7e3fe06267ea3ed652674934d68f0fa7cf48e9d868bdad0db",
            "dcb64bdf5580022dfdc2ba97609754a45b909f1fe2dc658eec4dc0ff92fef644",
        ),
        (
            31,
            &[(5, 12, 17)],
            [3200, 4480, 0],
            "a86f2a8a91a1e270e382e5bd7149570b357d6db028fd06e6445060352114669d",
            "a86f2a8a91a1e270e382e5bd7149570b357d6db028fd06e6445060352114669d",
        ),
        (
            31,
            &[(0, 1, 2), (7, 20, 25), (701, 65534, 8)],
            [5248, 6528, 0],
            "a86f2a8a91a1e270e382e5bd7149570b357d6db028fd06e6445060352114669d",
            "f04f3ddbbbc963c9ab693cef92909cb4f81582ce4f0d8ecd1f8bc663e4dcbe51",
        ),
        (
            31,
            &[(0, 0, 0), (1, 0, 3), (701, 4, 0)],
            [1152, 128, 0],
            "a86f2a8a91a1e270e382e5bd7149570b357d6db028fd06e6445060352114669d",
            "455190bf327892273204c631e560be21409c1922f7166b2942534865fb3c29af",
        ),
        (
            31,
            &[],
            [128, 128, 0],
            "a86f2a8a91a1e270e382e5bd7149570b357d6db028fd06e6445060352114669d",
            "455190bf327892273204c631e560be21409c1922f7166b2942534865fb3c29af",
        ),
    ];
    for &(seed, points, expected, before, after) in cases {
        for sound in [Some("TestAmbient"), None] {
            let mut sim = Simulation::with_seed(seed);
            let waypoints = points
                .iter()
                .map(|&(index, rx, ry)| (index, crate::map::waypoints::Waypoint { index, rx, ry }))
                .collect();
            sim.session.install_waypoints(&waypoints);
            let main = sim.main_rng.native_state_hex();
            let mapgen = sim.mapgen_rng.native_state_hex();
            assert_eq!(
                crate::util::sha256::sha256_hex(sim.scenario_rng.native_state_hex().as_bytes()),
                before
            );
            assert!(sim.trigger_play_sound_at_random_waypoint(
                sound,
                crate::rules::ruleset::RuleSet::empty_for_test(),
                FrameEffects::default()
            ));
            assert_eq!(
                crate::util::sha256::sha256_hex(sim.scenario_rng.native_state_hex().as_bytes()),
                after
            );
            assert_eq!(sim.main_rng.native_state_hex(), main);
            assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen);
            if sound.is_some() {
                assert!(
                    matches!(sim.sound_events.as_slice(), [SimSoundEvent::VocAtCoord { sound_id, coordinate, flags:0 }]
                    if sound_id=="TestAmbient" && [coordinate.x,coordinate.y,coordinate.z]==expected)
                );
            } else {
                assert!(
                    sim.sound_events.is_empty(),
                    "an unresolved sound still takes the original RNG request"
                );
            }
        }
    }
}

#[test]
fn waypoint_cue_sound_actions_share_literal_and_named_catalog_through_live_tags() {
    for kind in [19, 98, 99] {
        for (parameter, token) in [(0, "1"), (11, "1"), (7, "cowambient")] {
            let (mut sim, _) = sim_with_trigger(&format!("{kind},{parameter},{token},0,0,0,0,B"));
            let rng = sim.rng_state();
            poll(&mut sim);
            match kind {
                19 => assert!(
                    matches!(sim.sound_events.as_slice(),[SimSoundEvent::VocCentered {sound_id}] if sound_id=="CowAmbient")
                ),
                98 => assert!(
                    matches!(sim.sound_events.as_slice(),[SimSoundEvent::VocAtCoord {sound_id,coordinate,flags:0}] if sound_id=="CowAmbient" && *coordinate==WAYPOINT_COORD)
                ),
                99 => {
                    // Parameter11 leaves +44 at its constructor slot0;
                    // only the other readers materialize the tail B slot1.
                    let expected = if parameter == 11 {
                        DriveCoord {
                            x: 128,
                            y: 128,
                            z: 0,
                        }
                    } else {
                        WAYPOINT_COORD
                    };
                    assert!(
                        matches!(sim.sound_events.as_slice(),[SimSoundEvent::VocAtCoord {sound_id,coordinate,flags:1}] if sound_id=="CowAmbient" && *coordinate==expected)
                    );
                }
                _ => unreachable!(),
            }
            assert_eq!(
                sim.rng_state(),
                rng,
                "single-waypoint selection takes no RNG draw"
            );
        }
    }
    let (mut sim, _) = sim_with_trigger("98,7,TestAmbient,0,0,0,0,B");
    insert_structure(&mut sim, 100, "MARKER", (5, 6));
    poll(&mut sim);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::VocAtCoord { flags: 0, .. }]
    ));
    assert_eq!(
        sim.object_custom_sounds.get(&100),
        None,
        "98 does not attach a custom sound to the waypoint building"
    );
}
