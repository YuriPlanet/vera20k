//! Presentation-facing simulation state: water wakes for Drive, Ship, Hover and
//! Jumpjet movers, art-model (SHP or voxel) selection at spawn, move-sound start
//! and its RNG draw, and voxel anims with their attached particle systems.

use super::*;

/// Only Drive (`0x004B07CA`), Ship (`0x0069FE92`) and Hover (`0x00514A65`)
/// read `Rules->Wake` for a moving locomotor. A Jumpjet cruising over water is
/// moving now (`0x0054D0D0`) and leaves no wake.
#[test]
fn a_jumpjet_cruising_over_water_leaves_no_wake() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=JJV\n[AircraftTypes]\n[BuildingTypes]\n\
         [JJV]\nStrength=200\nArmor=light\nSpeed=14\nSpeedType=Hover\nMovementZone=Fly\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n",
    ))
    .expect("Jumpjet rules");
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object("JJV", "Americans", 0, 0, 64, &rules)
        .expect("spawn Jumpjet");
    let mut terrain = water_terrain(2, 1);
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    }
    sim.resolved_terrain = Some(terrain);
    let runtime = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|e| e.locomotor.as_mut())
        .and_then(|loco| loco.jumpjet_runtime_mut())
        .expect("Jumpjet runtime");
    *runtime = runtime
        .clone()
        .with_phase_for_test(
            crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_TRANSLATE,
        )
        .with_moving_for_test(true);
    let speed = crate::sim::movement::SpeedRules::new(&rules, &sim.interner, &sim.houses);
    let jumpjet = sim.substrate.entities.get(id).expect("Jumpjet");
    assert!(crate::sim::movement::motion_query::is_moving_now(
        jumpjet, speed, 10
    ));
    assert_eq!(
        super::wake_anchor_for(jumpjet, speed, false, sim.resolved_terrain.as_ref(), 10),
        None
    );
}

/// Ship's own wake gate (`0x0069FE3C..0x0069FE74`): every 8th frame, not
/// Drive's 10th, and never for an `Underwater=` type.
#[test]
fn a_ship_wakes_every_eighth_frame_unless_it_is_underwater() {
    let ini = IniFile::from_str(
        "[General]\nWake=WAKE1\n\
         [InfantryTypes]\n[VehicleTypes]\n0=DEST\n1=SUB\n[AircraftTypes]\n[BuildingTypes]\n\
         [DEST]\nStrength=600\nArmor=heavy\nSpeed=6\nMovementZone=Water\nSpeedType=Float\n\
         Naval=yes\nLocomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}\n\
         [SUB]\nStrength=600\nArmor=heavy\nSpeed=6\nMovementZone=Water\nSpeedType=Float\n\
         Naval=yes\nUnderwater=yes\nLocomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}\n",
    );
    let art = IniFile::from_str("[WAKE1]\nLayer=ground\nYSortAdjust=-288\nRate=120\n");
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("ship rules should parse");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_anim_frame_count_for_test("WAKE1", 15);
    let mut sim = Simulation::new();
    for (name, rx) in [("DEST", 0), ("SUB", 2)] {
        let id = sim
            .spawn_object(name, "Americans", rx, 0, 64, &rules)
            .expect("spawn ship");
        let ship = sim.substrate.entities.get_mut(id).expect("ship");
        let ahead = crate::sim::components::DriveCoord {
            x: i32::from(rx) * 256 + 256 + 128,
            y: 128,
            z: 0,
        };
        let runtime = ship.locomotor.as_mut().unwrap();
        assert!(runtime.ensure_installed_track_state());
        assert!(runtime.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Ship,
            Some(ahead)
        ));
        assert!(runtime.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Ship,
            Some(ahead)
        ));
        ship.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    }
    // The gate reads the `CellClass+0xEC` mirror; stamp Water on the fixture.
    let mut terrain = water_terrain(4, 1);
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    }
    sim.resolved_terrain = Some(terrain);
    let wakes = |sim: &Simulation| {
        sim.logic_order()
            .iter()
            .filter(|id| {
                sim.anim(**id)
                    .is_some_and(|anim| sim.interner.resolve(anim.type_id()) == "WAKE1")
            })
            .count()
    };

    sim.session.set_binary_frame_for_test(10);
    sim.spawn_wakes_for_frame(&rules, &Default::default());
    assert_eq!(wakes(&sim), 0, "a ship ignores Drive's 10th frame");
    sim.session.set_binary_frame_for_test(8);
    sim.spawn_wakes_for_frame(&rules, &Default::default());
    assert_eq!(wakes(&sim), 1, "only the surface ship wakes on the 8th");
}

fn moving_drive_boat() -> (Simulation, RuleSet, u64) {
    // A drive-family boat: the native wake gate is the drive locomotor's
    // `Is_Moving_Now` slot, so the fixture names the Drive locomotor.
    let ini: IniFile = IniFile::from_str(
        "[General]
Wake=WAKE1

         [InfantryTypes]

         [VehicleTypes]
0=BOAT

         [AircraftTypes]

         [BuildingTypes]

         [BOAT]
Strength=300
Armor=heavy
Speed=6
MovementZone=Water
SpeedType=Float
Naval=yes
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
",
    );
    // Stock artmd.ini [WAKE1]: ground layer, sorted under the hull.
    let art = IniFile::from_str(
        "[WAKE1]
Layer=ground
YSortAdjust=-288
Translucent=yes
Rate=120
",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("drive boat rules should parse");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_anim_frame_count_for_test("WAKE1", 15);
    let mut sim = Simulation::new();
    let boat_id = sim
        .spawn_object("BOAT", "Americans", 0, 0, 64, &rules)
        .expect("spawn boat");
    assert!(sim.interner.get("WAKE1").is_none());
    // The native gate reads CellClass+0xEC == 2 (Water); the fixture's water
    // grid carries land type 4 for movement, so stamp the binary's mirror.
    let mut terrain = water_terrain(2, 1);
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    }
    sim.resolved_terrain = Some(terrain);
    {
        let boat = sim
            .substrate
            .entities
            .get_mut(boat_id)
            .expect("boat entity");
        // The native gate is the locomotor's `Is_Moving_Now` slot: a live
        // destination/head-to coordinate and a positive owner speed. BOAT's
        // drive runtime is created lazily by the first movement step, so seed it.
        let ahead = crate::sim::components::DriveCoord {
            x: 256 + 128,
            y: 128,
            z: 0,
        };
        let drive = boat.locomotor.as_mut().unwrap();
        assert!(drive.ensure_installed_track_state());
        assert!(drive.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(ahead)
        ));
        assert!(drive.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(ahead)
        ));
        boat.foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    }

    (sim, rules, boat_id)
}

#[test]
fn a_drive_tube_turn_omits_the_wake_even_when_it_finishes_on_water() {
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

    let (mut sim, rules, id) = moving_drive_boat();
    let cells = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cells()
        .iter()
        .cloned()
        .collect();
    let tube = TubeFact::explicit((0, 0), (1, 0), 2, vec![2]);
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
        2,
        1,
        cells,
        vec![tube],
    ));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim.remove_entity_occupancy(id);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.position.put_location(DriveCoord::cell(1, 0, 0));
    actor.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
        TubeId(0),
        1,
        DriveCoord::cell(1, 0, 0),
    ));
    sim.session.set_binary_frame_for_test(10);
    sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 33);
    assert!(
        sim.entities().get(id).unwrap().active_tube().is_none(),
        "the unit completed its exit"
    );
    assert!(
        sim.anims()
            .all(|(_, a)| sim.interner.resolve(a.type_id()) != "WAKE1"),
        "the Unit tube turn replaced Drive Process on a wake frame"
    );
}

#[test]
fn moving_water_unit_spawns_rules_bound_wake_without_preinterned_effect_name() {
    let (mut sim, rules, boat_id) = moving_drive_boat();
    // The movement step of a full tick would rewrite the drive runtime from
    // the fixture's (non-)motion, so exercise the spawn owner directly at a
    // frame the native cadence accepts.
    assert_eq!(sim.session.binary_frame() % 10, 0);
    sim.spawn_wakes_for_frame(&rules, &Default::default());

    let wake_anims: Vec<u64> = sim
        .logic_order()
        .iter()
        .copied()
        .filter(|id| {
            sim.anim(*id)
                .is_some_and(|anim| sim.interner.resolve(anim.type_id()) == "WAKE1")
        })
        .collect();
    assert_eq!(wake_anims.len(), 1, "one wake AnimClass per moving boat");
    let wake = sim.anim(wake_anims[0]).expect("wake anim");
    // Exact lepton position, not the cell centre (native `PositionCoord`).
    let boat = sim.substrate.entities.get(boat_id).expect("boat");
    let expected = crate::sim::anim_class::AnimWorldCoord::from_cell_sub_z(
        boat.position.rx(),
        boat.position.ry(),
        boat.position.sub_x(),
        boat.position.sub_y(),
        boat.position.z(),
    );
    assert_eq!(
        (wake.world_coord().x, wake.world_coord().y),
        (expected.x, expected.y)
    );
    assert_eq!(wake.draw_flags(), 0x600);

    // Off-cadence frames and a unit that is not moving now spawn nothing.
    sim.session.set_binary_frame_for_test(5);
    sim.spawn_wakes_for_frame(&rules, &Default::default());
    sim.session.set_binary_frame_for_test(10);
    sim.substrate
        .entities
        .get_mut(boat_id)
        .expect("boat")
        .foot_speed
        .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
    sim.spawn_wakes_for_frame(&rules, &Default::default());
    let count_after = sim
        .logic_order()
        .iter()
        .filter(|id| {
            sim.anim(**id)
                .is_some_and(|a| sim.interner.resolve(a.type_id()) == "WAKE1")
        })
        .count();
    assert_eq!(
        count_after, 1,
        "off-cadence or stationary must not add wakes"
    );
}

/// Hover's wake (`HoverLocomotionClass::Process`, `0x00514A85..0x00514AC3`)
/// starts at the Foot's whole Location (`+0x9C`), Z included: the Anim's
/// Unlimbo (`0x005F4EC0`) keeps it through `AnimTypeClass` vt+0x6C
/// (`0x0041CF80`) and `SetLocation` (`0x005F6940`). A hovering hull's wake
/// starts at the hull's height, not on the water below it.
#[test]
fn a_hover_wake_starts_at_the_hulls_height() {
    use crate::sim::movement::hover::HoverRuntime;
    use crate::util::native_x87::NativeF64Bits;
    let ini = IniFile::from_str(
        "[General]\nWake=WAKE1\n\
         [InfantryTypes]\n[VehicleTypes]\n0=HOVR\n[AircraftTypes]\n[BuildingTypes]\n\
         [HOVR]\nStrength=300\nArmor=heavy\nSpeed=6\nMovementZone=Amphibious\n\
         SpeedType=Hover\nLocomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n",
    );
    let art = IniFile::from_str("[WAKE1]\nLayer=ground\nYSortAdjust=-288\nRate=120\n");
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("hover rules should parse");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_anim_frame_count_for_test("WAKE1", 15);
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object("HOVR", "Americans", 0, 0, 64, &rules)
        .expect("spawn hover");
    // The gate reads the `CellClass+0xEC` mirror; stamp Water on the fixture.
    let mut terrain = water_terrain(2, 1);
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    }
    sim.resolved_terrain = Some(terrain);
    let hover = sim.substrate.entities.get_mut(id).expect("hover");
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Hover);
    *locomotor.hover_runtime_mut().expect("Hover payload") =
        HoverRuntime::moving_for_test(Some(DriveCoord::cell(1, 0, 0)), NativeF64Bits::ONE);
    hover.locomotor = Some(locomotor);
    // Afloat about HoverHeight's equilibrium above the level-0 water.
    hover.position.set_exact_z_leptons_for_test(Some(90));
    let hull = crate::sim::movement::ground_pose::object_location(
        sim.substrate.entities.get(id).expect("hover"),
        sim.resolved_terrain.as_ref(),
    );
    assert_eq!(hull.z, 90);

    sim.spawn_wakes_for_frame(&rules, &Default::default());

    let wakes: Vec<_> = sim
        .logic_order()
        .iter()
        .filter_map(|id| sim.anim(*id))
        .filter(|anim| sim.interner.resolve(anim.type_id()) == "WAKE1")
        .map(|anim| anim.world_coord())
        .collect();
    assert_eq!(wakes.len(), 1, "one wake for the moving hover");
    assert_eq!(
        (wakes[0].x, wakes[0].y, wakes[0].z),
        (hull.x, hull.y, hull.z),
        "the wake starts at the hull, not on the water"
    );
}

fn gsi_13_10_art_model_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\nFixtureOnly=1\n\
         [InfantryTypes]\n0=FALLBACKINF\n\
         [VehicleTypes]\n0=DLPH\n1=DRON\n2=SQD\n3=VXLTEST\n4=ALIASED\n5=OMITTED\n6=FALLBACKVEH\n\
         [AircraftTypes]\n0=FALLBACKAIR\n\
         [BuildingTypes]\n0=FALLBACKBLD\n\
         [DLPH]\nStrength=100\n\
         [DRON]\nStrength=100\n\
         [SQD]\nStrength=100\n\
         [VXLTEST]\nStrength=100\n\
         [ALIASED]\nStrength=100\nImage=ALT\n\
         [OMITTED]\nStrength=100\n\
         [FALLBACKVEH]\nStrength=100\n\
         [FALLBACKAIR]\nStrength=100\n\
         [FALLBACKINF]\nStrength=100\n\
         [FALLBACKBLD]\nStrength=100\n",
    );
    let art_ini = IniFile::from_str(
        "[DLPH]\nVoxel=no\n\
         [DRON]\nVoxel=no\n\
         [SQD]\nVoxel=no\n\
         [VXLTEST]\nVoxel=yes\n\
         [ALT]\nVoxel=no\n\
         [OMITTED]\nCameo=OMITTEDICON\n",
    );
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).expect("art model rules");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

fn assert_gsi_13_10_shp_unit(entity: &GameEntity) {
    assert_eq!(entity.category(), EntityCategory::Unit);
    assert!(
        !entity.is_voxel(),
        "SHP Unit must retain its SHP drawing representation"
    );
    assert!(entity.animation.is_some());
}

fn assert_gsi_13_10_vxl_unit(entity: &GameEntity) {
    assert_eq!(entity.category(), EntityCategory::Unit);
    assert!(entity.is_voxel());
    assert!(entity.animation.is_none());
}

/// Original SQD Foot4DA806..4DAB3C: idle frame4 advances +538 and starts
/// SquidMove even though Ship IsMovingNow69F330 returns false.
#[test]
fn idle_squid_body_counter_starts_move_sound() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let object = rules.object("SQD").expect("retail SQD");
    let mut sim = Simulation::with_seed(0x1020_3040);
    let mut entity = GameEntity::test_default(1, "SQD", "Americans", 4, 4);
    entity.set_type_ref_for_test(sim.interner.intern("SQD"));
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    sim.substrate.entities.insert(entity);
    let before = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .body_frame_counter
        .get();
    crate::sim::animation::tick_foot_body_frame_counter(
        sim.substrate.entities.get_mut(1).unwrap(),
        crate::sim::movement::SpeedRules::new(
            crate::rules::ruleset::RuleSet::empty_for_test(),
            &sim.interner,
            &sim.houses,
        ),
        crate::sim::animation::FootBodyCadence {
            walk_rate: object.walk_rate,
            idle_rate: object.idle_rate,
        },
        object.hover_attack,
        object.deploy_to_land,
        0,
        None,
        4,
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .body_frame_counter
            .get(),
        1
    );
    Simulation::tick_move_sound_after_process(
        &sim.interner,
        &sim.session,
        &mut sim.main_rng,
        &mut sim.substrate,
        &mut sim.sound_events,
        &sim.houses,
        1,
        before,
        &rules,
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .is_active()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .countdown(),
        3
    );
}

fn move_sound_test_rules(configured: bool) -> RuleSet {
    let move_sound = if configured {
        "MoveSound=TestMove\n"
    } else {
        ""
    };
    RuleSet::from_ini_with_sounds_for_test(
        &IniFile::from_str(&format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=TESTUNIT\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TESTUNIT]\nStrength=100\nArmor=light\nSpeed=6\n{move_sound}"
        )),
        &["TestMove"],
    )
    .expect("MoveSound rules")
}

fn move_sound_test_sim() -> Simulation {
    let mut sim = Simulation::with_seed(0x1020_3040);
    let mut entity = GameEntity::test_default(1, "TESTUNIT", "Americans", 4, 4);
    entity.set_type_ref_for_test(sim.interner.intern("TESTUNIT"));
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    sim.substrate.entities.insert(entity);
    sim
}

fn trigger_move_sound_tail(sim: &mut Simulation, rules: &RuleSet) {
    let entity = sim.substrate.entities.get_mut(1).expect("test Foot exists");
    let before = entity.body_frame_counter.get();
    entity
        .body_frame_counter
        .set_for_test(before.wrapping_add(1));
    Simulation::tick_move_sound_after_process(
        &sim.interner,
        &sim.session,
        &mut sim.main_rng,
        &mut sim.substrate,
        &mut sim.sound_events,
        &sim.houses,
        1,
        before,
        rules,
    );
}

#[test]
fn move_sound_start_consumes_exactly_one_main_draw() {
    let configured_rules = move_sound_test_rules(true);
    let mut sim = move_sound_test_sim();
    let scenario_before = sim.scenario_rng.state();
    let mapgen_before = sim.mapgen_rng.state();
    let mut expected_main = sim.main_rng.clone();

    // Retail calls Random::Next even before modulo-by-one. Full native
    // vector, timer and paid-visit controls live in foot_move_sound.json.
    expected_main.next_u32();
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());
    assert_eq!(sim.scenario_rng.state(), scenario_before);
    assert_eq!(sim.mapgen_rng.state(), mapgen_before);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .is_active()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .countdown(),
        3
    );
    assert!(matches!(
        sim.sound_events.last(),
        Some(SimSoundEvent::AnimationStarted { anim_id: 1, .. })
    ));

    // Qualifying again while the handle is active only reloads the grace
    // counter; it does not choose another sample.
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());

    // A fresh start after stop chooses again and therefore draws once again.
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.set_falling_down_for_test(true);
    let before = entity.body_frame_counter.get();
    Simulation::tick_move_sound_after_process(
        &sim.interner,
        &sim.session,
        &mut sim.main_rng,
        &mut sim.substrate,
        &mut sim.sound_events,
        &sim.houses,
        1,
        before,
        &configured_rules,
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_falling_down_for_test(false);
    expected_main.next_u32();
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());
    assert_eq!(sim.scenario_rng.state(), scenario_before);
    assert_eq!(sim.mapgen_rng.state(), mapgen_before);

    // No configured MoveSound never enters the native vector-pick branch.
    let no_sound_rules = move_sound_test_rules(false);
    let mut silent = move_sound_test_sim();
    let silent_scenario = silent.scenario_rng.state();
    let silent_main = silent.main_rng.state();
    let silent_mapgen = silent.mapgen_rng.state();
    trigger_move_sound_tail(&mut silent, &no_sound_rules);
    assert_eq!(silent.scenario_rng.state(), silent_scenario);
    assert_eq!(silent.main_rng.state(), silent_main);
    assert_eq!(silent.mapgen_rng.state(), silent_mapgen);
    assert!(
        !silent
            .substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .is_active()
    );
}

#[test]
fn gsi_13_10_art_voxel_no_selects_shp_unit_in_all_three_spawn_constructors() {
    let rules = gsi_13_10_art_model_rules();
    let mut sim = Simulation::new();

    assert_eq!(
        sim.spawn_from_map(
            &[make_test_entity("DLPH", EntityCategory::Unit)],
            Some(&rules),
        ),
        1
    );
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(1).expect("map DLPH"));

    let dron = sim
        .spawn_object_at_height("DRON", "Americans", 31, 40, 64, 0, &rules)
        .expect("placed DRON");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(dron).expect("DRON entity"));

    let squid = sim
        .spawn_object_limbo_at_height("SQD", "Americans", 32, 40, 64, 0, &rules)
        .expect("limbo SQD");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(squid).expect("SQD entity"));
}

#[test]
fn gsi_13_10_effective_art_metadata_precedes_complete_category_fallback() {
    let rules = gsi_13_10_art_model_rules();
    let mut sim = Simulation::new();

    let vxl = sim
        .spawn_object_limbo_at_height("VXLTEST", "Americans", 1, 1, 0, 0, &rules)
        .expect("explicit VXL vehicle");
    assert_gsi_13_10_vxl_unit(sim.substrate.entities.get(vxl).expect("VXL entity"));

    let aliased = sim
        .spawn_object_limbo_at_height("ALIASED", "Americans", 2, 1, 0, 0, &rules)
        .expect("Image=ALT vehicle");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(aliased).expect("aliased entity"));

    let omitted = sim
        .spawn_object_limbo_at_height("OMITTED", "Americans", 3, 1, 0, 0, &rules)
        .expect("art entry with omitted Voxel");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(omitted).expect("omitted entity"));

    for (type_id, expected_voxel) in [
        ("FALLBACKVEH", true),
        ("FALLBACKAIR", true),
        ("FALLBACKINF", false),
        ("FALLBACKBLD", false),
    ] {
        let id = sim
            .spawn_object_limbo_at_height(type_id, "Americans", 4, 1, 0, 0, &rules)
            .unwrap_or_else(|| panic!("missing-metadata fallback spawn {type_id}"));
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .expect("fallback entity")
                .is_voxel(),
            expected_voxel,
            "category fallback for {type_id}"
        );
    }
}

/// VoxelAnim ctor `0x00749864..0x007498BD` builds the type's AttachedSystem
/// at the input location, owned by the anim; Smoke `0x0062ED49..0x0062EDB5`
/// then follows that non-Building Object owner plus the constructor's offset.
#[test]
fn voxel_anim_builds_its_attached_system_and_smoke_follows_the_anim() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\n\
         [Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(17);
    let mut kind = crate::rules::voxel_anim_type::VoxelAnimType::with_defaults("DEBRIS");
    kind.attached_system = Some("Sys".to_owned());
    let origin = IVec3::new(1280, 2560, 0);
    sim.emit_debris_voxel(
        crate::rules::voxel_anim_type::VoxelAnimTypeId(0),
        &kind,
        None,
        origin,
        true,
        &mut Vec::new(),
        &rules,
    )
    .unwrap();
    let id = sim.voxel_anims().ids()[0];
    let system = sim.voxel_anims().get(id).unwrap().attached_system();
    let system = system.expect("the constructor's tail builds AttachedSystem");
    let sys = sim.particle_systems().get(system).unwrap();
    assert_eq!(sys.owner_entity(), Some(id));
    assert_eq!(sys.coords(), origin);
    let offset = sys.offset();

    let moved = origin + IVec3::new(512, -256, 300);
    sim.substrate
        .voxel_anims
        .get_mut(id)
        .unwrap()
        .set_location_for_test(moved);
    crate::sim::particles::system_ai::tick_particle_system(
        &mut sim,
        &rules,
        system,
        crate::sim::world::UninitContext::new(&rules),
    );
    assert_eq!(
        sim.particle_systems().get(system).unwrap().coords(),
        moved + offset
    );
}

/// A live DEBRIS VoxelAnim whose type names a Smoke `AttachedSystem=`, built
/// through the constructor: the world, its rules, the anim and its system.
fn voxel_anim_with_attached_smoke() -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n[VoxelAnims]\n0=DEBRIS\n\
         [Smk]\nBehavesLike=Smoke\nMaxEC=10\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\n\
         [DEBRIS]\nAttachedSystem=Sys\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(17);
    let kind = rules.voxel_anim_type_id_by_name("DEBRIS").unwrap();
    sim.emit_debris_voxel(
        kind,
        rules.voxel_anim_type(kind),
        None,
        IVec3::new(1280, 2560, 0),
        true,
        &mut Vec::new(),
        &rules,
    )
    .unwrap();
    let id = sim.voxel_anims().ids()[0];
    let system = sim
        .voxel_anims()
        .get(id)
        .unwrap()
        .attached_system()
        .unwrap();
    (sim, rules, id, system)
}

/// The anim's expiry (`0x00749A11`) takes the generic arm of
/// `DispatchPointerExpiredCleanup @ 0x007258D0` (`0x00725925`), so its
/// system's PointerExpired (`0x0062FE90`) drops the owner and stops spawning.
#[test]
fn a_dying_voxel_anim_ends_its_attached_system() {
    let (mut sim, rules, id, system) = voxel_anim_with_attached_smoke();
    sim.uninit_with_rules(id, &rules);
    let sys = sim.particle_systems().get(system).unwrap();
    assert_eq!(sys.owner_entity(), None);
    assert!(sys.done_spawning());
}

/// A save holding a live anim's AttachedSystem loads: the system's owner is
/// in the VoxelAnim store, which Smoke's owner lookup resolves.
#[test]
fn a_voxel_anims_attached_system_survives_a_save() {
    let (sim, _rules, id, system) = voxel_anim_with_attached_smoke();
    let bytes =
        crate::sim::snapshot::GameSnapshot::save_validated(&sim, 17, 23, "attached system", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load_validated(&bytes, 17, 23, "")
        .expect("the anim owner resolves")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore the anim and its system");
    assert_eq!(
        restored
            .particle_systems()
            .get(system)
            .unwrap()
            .owner_entity(),
        Some(id)
    );
}

/// Callback-disabled damage fixtures may deliver after another constructor;
/// they retain the original constructor IDs and spend no second RNG/ID draws.
#[test]
fn deferred_voxel_delivery_keeps_constructor_identity_without_spending_again() {
    let mut sim = Simulation::with_seed(17);
    let kind = crate::rules::voxel_anim_type::VoxelAnimType::with_defaults("DEBRIS");
    let type_id = crate::rules::voxel_anim_type::VoxelAnimTypeId(0);
    let mut packets = Vec::new();
    sim.emit_debris_voxel(
        type_id,
        &kind,
        None,
        glam::IVec3::ZERO,
        false,
        &mut packets,
        &RuleSet::empty_for_test(),
    )
    .unwrap();
    assert_eq!(packets.len(), 1);
    let id = packets[0].object.stable_id();
    let native_id = packets[0].object.native_unique_id();
    assert_eq!(sim.native_identity_cursor(), Some(native_id as u32));
    assert!(sim.voxel_anims().is_empty());
    assert!(sim.live_object_order_snapshot().is_empty());
    let adjacent = sim.next_native_runtime_id();
    assert_ne!(adjacent, native_id);
    let cursor = sim.native_identity_cursor();
    let rng = sim.scenario_rng.logical_state();
    let stable_cursor = sim.next_stable_object_id();
    sim.admit_death_debris(packets, &RuleSet::empty_for_test());
    assert_eq!(
        sim.voxel_anims().get(id).unwrap().native_unique_id(),
        native_id
    );
    assert_eq!(sim.native_identity_cursor(), cursor);
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    assert_eq!(sim.next_stable_object_id(), stable_cursor);
    assert_eq!(sim.live_object_order_snapshot(), [id]);
    sim.debug_assert_logic_membership_consistent();
}
