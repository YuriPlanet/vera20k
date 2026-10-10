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
use crate::sim::combat::AttackTarget;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
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
    plane.position.sub_x = SimFixed::from_num(128);
    plane.position.sub_y = SimFixed::from_num(128);
    plane.position.exact_z_leptons = Some(1500);
    if flag(row, "target") {
        plane.attack_target = Some(AttackTarget::for_cell(40, 40));
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
    plane.in_playfield = flag(row, "in_playfield");
    if flag(row, "mission_only") {
        plane.mark_mission_only();
    }
    let mut sim = Simulation::with_seed(0);
    sim.substrate.entities.insert(plane);
    sim.interner = crate::sim::intern::test_interner();
    let playfield = &row["playfield"];
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: int(&playfield["base"]),
        off_fc: int(&playfield["off_fc"]),
        off_100: int(&playfield["off_100"]),
        off_104: int(&playfield["off_104"]),
        off_108: int(&playfield["off_108"]),
    });
    sim.playfield_size_height = Some(int(&row["size_height"]));
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
        let removed = sim.remove_aircraft_off_map(1, &rules, None);
        assert_eq!(removed, flag(&row, "removed"), "{row}");
        // UnInit: dead, and deleted at the frame's end.
        let alive = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive;
        assert_eq!(alive, !removed, "{row}");
        assert_eq!(sim.substrate.pending_delete.contains(&1), removed, "{row}");
    }
}

/// Without MapClass authority (headless fixtures) no aircraft leaves.
#[test]
fn no_map_removes_nothing() {
    let rules = rules();
    let row = rows().into_iter().find(|row| flag(row, "removed")).unwrap();
    let mut sim = world(&row);
    sim.playfield_bounds = None;
    sim.playfield_size_height = None;
    assert!(!sim.remove_aircraft_off_map(1, &rules, None));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .lifecycle
            .object_alive
    );
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
    plane.in_playfield = flag(&row, "in_playfield");
    plane.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(true, 3);
    assert!(sim.aircraft_may_leave_map(1));
    assert!(sim.remove_aircraft_off_map(1, &rules, None));
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
