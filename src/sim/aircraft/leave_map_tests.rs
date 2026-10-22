//! `aircraft::leave_map` against `tools/superweapon_oracle.json`
//! `aircraft_leave_map` (`--check` regenerates it): AircraftClass::AI's
//! removal block and its predicate, run natively over cells inside the
//! playfield, outside it but in the map's `Size=` diamond, and past each side
//! of the diamond, for plain, `FlyBy=` and `FlyBack=` types and each exit of
//! the predicate.

use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

const FLY: &str = "{4A582746-9839-11D1-B709-00A024DDAFD1}";

fn rows() -> Vec<Value> {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    oracle["aircraft_leave_map"].as_array().unwrap().clone()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(row: &Value, key: &str) -> bool {
    row[key].as_bool().unwrap()
}

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=PLANE\n1=FLYBY\n2=FLYBACK\n[InfantryTypes]\n[VehicleTypes]\n\
         [BuildingTypes]\n[PLANE]\nLocomotor={FLY}\n[FLYBY]\nFlyBy=yes\nLocomotor={FLY}\n\
         [FLYBACK]\nFlyBack=yes\nLocomotor={FLY}\n"
    )))
    .unwrap()
}

/// The row's plane alone at its cell's centre, 1500 leptons up, on the
/// row's playfield and Map Size.
fn world(row: &Value) -> Simulation {
    let kind = match (flag(row, "fly_by"), flag(row, "fly_back")) {
        (true, _) => "FLYBY",
        (_, true) => "FLYBACK",
        _ => "PLANE",
    };
    let (rx, ry) = (int(&row["cell"][0]) as u16, int(&row["cell"][1]) as u16);
    let mut plane = GameEntity::test_default_of_category(
        1,
        kind,
        "Americans",
        rx,
        ry,
        EntityCategory::Aircraft,
    );
    plane.position.set_sub_x_for_test(SimFixed::from_num(128));
    plane.position.set_sub_y_for_test(SimFixed::from_num(128));
    plane.position.set_exact_z_leptons_for_test(Some(1500));
    if flag(row, "target") {
        plane
            .attack_target
            .set_for_test(Some(TargetKind::Cell(40, 40)));
    }
    plane.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(int(&row["current"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(int(&row["queued"])),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    plane.set_in_playfield(flag(row, "in_playfield"));
    if flag(row, "mission_only") {
        plane.mission_only.mark();
    }
    let mut sim = Simulation::with_seed(0);
    sim.substrate.entities.insert(plane);
    sim.interner = crate::sim::intern::test_interner();
    let playfield = &row["playfield"];
    sim.set_playfield_bounds_for_test(Some(PlayfieldBounds {
        base: int(&playfield["base"]),
        off_fc: int(&playfield["off_fc"]),
        off_100: int(&playfield["off_100"]),
        off_104: int(&playfield["off_104"]),
        off_108: int(&playfield["off_108"]),
    }));
    sim.set_playfield_size_height_for_test(Some(int(&row["size_height"])));
    if !row["team"].is_null() {
        // A team with no script action: `0x006EC300` answers false.
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[1], false, false);
    }
    sim
}

#[test]
fn removal_past_the_map_matches_native() {
    let rules = rules();
    let rows = rows();
    assert!(rows.len() > 40);
    for row in rows {
        // The team rows answering true need `0x006EC300`'s waypoint read
        // (the RESIDUAL on `aircraft_may_leave_map`).
        if row["team"].as_bool() == Some(true) {
            continue;
        }
        let mut sim = world(&row);
        let removed = sim.remove_aircraft_off_map(
            1,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(removed, flag(&row, "removed"), "{row}");
        // UnInit: dead, and deleted at the frame's end.
        let alive = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive();
        assert_eq!(alive, !removed, "{row}");
        assert_eq!(
            sim.substrate.pending_delete.contains_object(&1),
            removed,
            "{row}"
        );
    }
}

/// Removal enters shared FootLimbo405FD0; deferred destruction releases the
/// handle at 0x4D3677. The MoveSound latch is not rewritten by either operation.
#[test]
fn removal_releases_the_planes_sounds() {
    let rules = rules();
    let row = rows()
        .into_iter()
        .find(|row| flag(row, "removed") && row["team"].is_null())
        .unwrap();
    let mut sim = world(&row);
    let position = sim.substrate.entities.get(1).unwrap().position;
    // The predicate fixtures start in constructor Limbo. This cleanup
    // control enters AI as an already-revealed Foot, so its first Limbo
    // actually reaches the shared 405FD0 detach boundary.
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let plane = sim.substrate.entities.get_mut(1).unwrap();
    // Reveal recomputes +3D5 from this already-outside location. Reapply
    // the original row's AI-entry pose and retained visited-playfield byte;
    // the excluded flight that reached this boundary is not being replayed.
    plane.position = position;
    plane.set_in_playfield(flag(&row, "in_playfield"));
    plane.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(true, 3);
    assert!(sim.aircraft_may_leave_map(1));
    assert!(sim.remove_aircraft_off_map(
        1,
        &rules,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(
        sim.sound_events
            .iter()
            .any(|event| { matches!(event, SimSoundEvent::ObjectSoundDetached { owner: 1 }) })
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| { matches!(event, SimSoundEvent::AnimationStopped { anim_id: 1, .. }) })
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| { matches!(event, SimSoundEvent::ObjectSoundReleased { owner: 1 }) })
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .move_sound
            .is_active()
    );
    sim.process_pending_delete();
    assert!(
        sim.sound_events
            .iter()
            .any(|event| { matches!(event, SimSoundEvent::ObjectSoundReleased { owner: 1 }) })
    );
}

fn cleanup_world() -> (Simulation, RuleSet, u64) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules("");
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let plane = sim
        .spawn_object_with_overlay_registry("HORNET", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::SpyplaneOverfly), 0);
    (sim, rules, plane)
}

#[test]
fn dummy_cleanup_uses_allocation_identity_and_does_not_stamp_the_dummy() {
    let (mut sim, rules, plane) = cleanup_world();
    // The packed fixed-stride alias is real (1,1), despite its negative X.
    let alias = NavTargetRef::cell((-511_i16) as u16, 2);
    assert!(matches!(
        TargetKind::from(alias).cell_identity(sim.resolved_terrain.as_ref().unwrap()),
        Some(crate::map::cell_index::NativeCellIdentity::Real(_))
    ));
    for pointer in [None, Some(alias), Some(NavTargetRef::object(plane))] {
        let entity = sim.substrate.entities.get_mut(plane).unwrap();
        entity.navigation.set_nav_com_for_test(pointer);
        entity
            .attack_target
            .set_for_test(pointer.map(TargetKind::from));
        let before = entity.navigation.path_runtime().clone();
        let rng = sim.scenario_rng.logical_state();
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .stamp_dummy_cell_requested_coord(700, 701);
        sim.cleanup_aircraft_dummy_cells(plane, &rules, None, Default::default());
        let entity = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(entity.navigation.nav_com(), pointer);
        assert_eq!(entity.attack_target.target(), pointer.map(TargetKind::from));
        assert_eq!(entity.navigation.path_runtime(), &before);
        assert_eq!(sim.scenario_rng.logical_state(), rng);
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (700, 701)
        );
    }

    // A hole inside the storage rectangle is Dummy too; playfield/Size
    // checks alone cannot establish CellClass allocation identity.
    let cells: Vec<_> = (0..33)
        .flat_map(|y| (0..33).map(move |x| (x, y)))
        .filter(|&at| at != (20, 20))
        .collect();
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .test_set_native_allocated_cells(&cells);
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(20, 20)));
    sim.cleanup_aircraft_dummy_cells(plane, &rules, None, Default::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.navigation.nav_com(), None);
    assert_eq!(entity.attack_target.target(), None);
}

/// The first Idle restores a suspended Target. AI415016 must read that new
/// pointer, then execute its second complete setter/Idle sequence.
#[test]
fn dummy_cleanup_rereads_the_target_after_idle_restores_a_mission() {
    let (mut sim, rules, plane) = cleanup_world();
    let entity = sim.substrate.entities.get_mut(plane).unwrap();
    let mission = MissionId::from_known(MissionType::SpyplaneOverfly);
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: mission,
        suspended: mission,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(700, 700)));
    entity
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(16, 15)));
    entity
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Cell(701, 701)));
    sim.cleanup_aircraft_dummy_cells(plane, &rules, None, Default::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.mission.current(), mission);
    assert_eq!(entity.mission.suspended(), MissionId::NONE);
    assert_eq!(
        entity.navigation.nav_com(),
        None,
        "second clear drops restored NavCom"
    );
    assert_eq!(
        entity.attack_target.target(),
        None,
        "second clear drops restored Target"
    );
}
