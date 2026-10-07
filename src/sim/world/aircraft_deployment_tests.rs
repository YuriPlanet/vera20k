//! Retained aircraft deployment history through production construction/reveal.
use super::{PlacementEvidence, Simulation};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::snapshot::GameSnapshot;

fn rules(selectable: bool, landable: bool, weapon: &str, elite: &str) -> RuleSet {
    let yes = |value| if value { "yes" } else { "no" };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=PLANE\n[InfantryTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
         [PLANE]\nStrength=100\nSpeed=10\nSelectable={}\nLandable={}\nPrimary={}\nElitePrimary={}\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         [gun]\nDamage=1\n[camera]\nDamage=1\nCamera=yes\n",
        yes(selectable),
        yes(landable),
        if weapon == "none" { "" } else { weapon },
        if elite == "none" { "" } else { elite },
    )))
    .unwrap()
}

#[test]
fn mission_only_aircraft_reveal_matches_original_flag_histories() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_mission_only.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 96);
    for row in rows {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap();
        let rules = rules(
            flag("selectable"),
            flag("landable"),
            input["weapon"].as_str().unwrap(),
            input["elite_weapon"].as_str().unwrap(),
        );
        let mut sim = Simulation::new();
        let id = sim
            .construct_object_limbo_at_height("PLANE", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        assert!(
            !entity.is_mission_only(),
            "construction is not successful Unlimbo"
        );
        entity.set_veterancy_rank(input["veterancy"].as_u64().unwrap() as u16 * 100);
        if flag("previous") {
            entity.mark_mission_only();
        }
        let placement = if flag("success") {
            PlacementEvidence::MarkSucceeded
        } else {
            PlacementEvidence::MarkFailed
        };
        let rng = sim.scenario_rng.logical_state();
        let result = sim.reveal_constructed_object_at_height(id, 10, 10, 0, 0, placement, &rules);
        assert_eq!(result.is_some(), flag("success"), "{row}");
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().is_mission_only(),
            row["mission_only"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            rng,
            "flag suffix draws no RNG"
        );
    }
}

#[test]
fn mission_only_survives_snapshot_limbo_and_ordinary_type_reveal() {
    let special = rules(true, true, "camera", "none");
    let normal = rules(true, true, "gun", "none");
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object_at_height("PLANE", "Americans", 10, 10, 0, 0, &special)
        .unwrap();
    assert!(sim.substrate.entities.get(id).unwrap().is_mission_only());
    sim.conceal(id);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let saved = GameSnapshot::save(&sim, 0, 0, "deployment", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    for world in [&mut sim, &mut restored] {
        assert!(world.substrate.entities.get(id).unwrap().is_mission_only());
        assert!(
            world
                .reveal_constructed_object_at_height(
                    id,
                    11,
                    10,
                    0,
                    0,
                    PlacementEvidence::MarkSucceeded,
                    &normal
                )
                .is_some()
        );
        assert!(
            world.substrate.entities.get(id).unwrap().is_mission_only(),
            "ordinary type cannot clear deployment history"
        );
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}

#[test]
fn mission_only_has_its_own_hash_fold() {
    let rules = rules(true, true, "gun", "none");
    let mut sim = Simulation::new();
    let id = sim
        .construct_object_limbo_at_height("PLANE", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let before = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mark_mission_only();
    assert_ne!(sim.state_hash(), before);
}

/// AircraftClass::Unlimbo through production construction and reveal against
/// the original's runs (tools/spatial_oracle/aircraft_unlimbo_height.json):
/// the Z it hands Foot Unlimbo, then its tail's +3D4, Stage restart and speed
/// fraction. The run's input Z is staged on the limbo Location, where the
/// spawn launch puts its coordinate; a `MissileSpawn=` type keeps it.
#[test]
fn aircraft_unlimbo_height_and_tail_match_original_runs() {
    use crate::map::playfield::PlayfieldBounds;
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};

    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_unlimbo_height.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 146);
    let bounds = PlayfieldBounds::from_normalized_local_size(32, 2, 2, 24, 20);
    for row in rows {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap();
        let int = |key: &str| input[key].as_i64().unwrap() as i32;
        let type_level = int("type_flight_level");
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[AircraftTypes]\n0=PLANE\n[InfantryTypes]\n[VehicleTypes]\n[BuildingTypes]\n\
             [General]\nFlightLevel={}\n\
             [PLANE]\nStrength=100\nSpeed=10\nSelectable=yes\nLandable=yes\nMissileSpawn={}\n{}\
             Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n",
            int("rules_flight_level"),
            if flag("missile_spawn") { "yes" } else { "no" },
            if type_level == -1 {
                String::new()
            } else {
                format!("FlightLevel={type_level}\n")
            },
        )))
        .unwrap();
        // The supplied floor: flat level 0, the slope-1 ramp at the cell
        // centre (ramp_1_sub_128_128 in tools/ramp_height_vectors.json), or
        // flat level 4.
        let (level, slope) = match int("ground") {
            0 => (0, 0),
            52 => (0, 1),
            416 => (4, 0),
            ground => panic!("no fixture floor {ground}"),
        };
        let in_playfield = flag("in_playfield");
        let cell = (0u16..32)
            .flat_map(|ry| (0u16..32).map(move |rx| (rx, ry)))
            .find(|&(rx, ry)| {
                bounds.contains_height_aware_packed(rx.into(), ry.into(), level as i8, slope)
                    == in_playfield
            })
            .expect("a cell on each side of the playfield");
        let mut sim = Simulation::new();
        super::lifecycle_tests::install_common_raw_terrain(&mut sim, 32, 32, level, None);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = slope;
        sim.playfield_bounds = Some(bounds);
        sim.session.binary_frame = 1000;
        let id = sim
            .construct_object_limbo_at_height(
                "PLANE",
                "Americans",
                cell.0,
                cell.1,
                0,
                level,
                &rules,
            )
            .unwrap();
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        if flag("mission_only") {
            entity.mark_mission_only();
        }
        crate::sim::movement::ground_pose::put_location(
            &mut entity.position,
            crate::sim::components::DriveCoord {
                x: i32::from(cell.0) * 256 + 128,
                y: i32::from(cell.1) * 256 + 128,
                z: input["input"][2].as_i64().unwrap() as i32,
            },
        );
        let before = (*entity.native_stage(), entity.foot_speed.applied_fraction());
        let placement = if flag("success") {
            PlacementEvidence::MarkSucceeded
        } else {
            PlacementEvidence::MarkFailed
        };
        let revealed = sim
            .reveal_constructed_object_at_height(id, cell.0, cell.1, 0, level, placement, &rules);
        assert_eq!(revealed.is_some(), flag("success"), "{row}");
        let entity = sim.substrate.entities.get(id).unwrap();
        if !flag("success") {
            assert_eq!(
                (*entity.native_stage(), entity.foot_speed.applied_fraction()),
                before,
                "{row}"
            );
            continue;
        }
        let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        assert_eq!(
            crate::sim::movement::ground_pose::ground_surface_z_at(
                xy,
                false,
                sim.resolved_terrain.as_ref(),
                None
            ),
            Some(int("ground")),
            "fixture floor"
        );
        assert_eq!(
            entity.is_mission_only(),
            row["mission_only"].as_bool().unwrap(),
            "{row}"
        );
        let stage = &row["stage"];
        let stage_int = |key: &str| stage[key].as_i64().unwrap() as i32;
        assert_eq!(entity.native_stage().value(), stage_int("value"), "{row}");
        assert_eq!(entity.native_stage().rate(), stage_int("rate"), "{row}");
        assert_eq!(
            entity.native_stage().timer(),
            crate::sim::timer::CdTimer::started(stage_int("start"), stage_int("left")),
            "{row}"
        );
        let z = row["unlimbo_coord"][2].as_i64().unwrap() as i32;
        assert_eq!(entity.position.exact_z_leptons, Some(z), "{row}");
        // The altitude cache mirrors the committed GetHeight.
        assert_eq!(
            entity.locomotor.as_ref().unwrap().altitude,
            crate::util::fixed_math::SimFixed::from_num(z - int("ground")),
            "{row}"
        );
        let fraction = match row["speed_fraction_bits"].as_str().unwrap() {
            "0x3FF0000000000000" => SIM_ONE,
            "0x0000000000000000" => SIM_ZERO,
            bits => panic!("unexpected speed fraction {bits}"),
        };
        assert_eq!(entity.foot_speed.applied_fraction(), fraction, "{row}");
    }
}
