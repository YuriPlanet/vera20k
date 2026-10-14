//! End-to-end stock cloak/sensor/radar presentation acceptance.

use super::tests::{flat_terrain, visibility_projection};
use super::*;
use crate::map::map_file::MapHeader;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::Simulation;

#[test]
fn radar_visibility_consumes_live_stock_cloak_and_sensor_lifecycle() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nCloakingStages=9\n\
         [AudioVisual]\nCloakSound=NavalUnitEmerge\n\
         [VehicleTypes]\n0=SUB\n1=DEST\n\
         [SUB]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
         [DEST]\nStrength=600\nSpeed=6\nSensorsSight=8\n",
    ))
    .expect("stock submarine/detector fixture");
    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 64,
        height: 64,
        local_left: 2,
        local_top: 2,
        local_width: 56,
        local_height: 52,
    };
    let mut sim = Simulation::with_seed(0xC10A_5E45);
    sim.install_playfield_from_map_header(&header);
    sim.fog.width = 64;
    sim.fog.height = 64;
    sim.resolved_terrain = Some(flat_terrain(64));
    let bounds = sim.playfield_bounds.expect("normalized map authority");
    let cell = (8u16..56)
        .flat_map(|ry| (8u16..56).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
        .expect("interior mode-one cell");
    let far_cell = (8u16..56)
        .flat_map(|ry| (8u16..56).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| {
            bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
                && i32::from(rx).abs_diff(i32::from(cell.0)) >= 10
                && i32::from(ry).abs_diff(i32::from(cell.1)) >= 10
        })
        .expect("separate interior mode-one detector cell");
    let entering_cell = (8u16..56)
        .flat_map(|ry| (8u16..56).map(move |rx| (rx, ry)))
        .find(|&(rx, ry)| {
            (rx, ry) != cell
                && bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
                && rx.abs_diff(cell.0) <= 1
                && ry.abs_diff(cell.1) <= 1
        })
        .expect("adjacent interior mode-one submarine cell");
    let local = sim.interner.intern("Americans");
    let sub = sim
        .spawn_object_at_height("SUB", "Soviet", cell.0, cell.1, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(sub)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .establish_unlimbo_fully_cloaked();
    sim.fog.mark_visible_for_owner(local, cell.0, cell.1);
    let entering = sim
        .spawn_object_at_height(
            "SUB",
            "Soviet",
            entering_cell.0,
            entering_cell.1,
            0,
            0,
            &rules,
        )
        .unwrap();
    let entering_owner = sim.substrate.entities.get(entering).unwrap().owner();
    sim.fog
        .mark_visible_for_owner(local, entering_cell.0, entering_cell.1);
    sim.fog
        .mark_visible_for_owner(entering_owner, entering_cell.0, entering_cell.1);
    assert_eq!(
        sim.substrate
            .entities
            .get(entering)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        0,
        "inside-playfield Unlimbo leaves the sensor-callback fixture uncloaked"
    );

    let build_update = |sim: &Simulation, stable_id| {
        build_radar_object_update(
            sim.substrate.entities.get(stable_id).unwrap(),
            &sim.houses,
            Some(local),
            &sim.fog,
            false,
            true,
            Some(&rules),
            Some(&sim.interner),
            visibility_projection(),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
        )
    };
    let evaluate = |sim: &Simulation| build_update(sim, sub).visibility.evaluate(false);
    let mut tracker = crate::render::radar::tracker::RetainedRadarTracker::default();
    assert_eq!(evaluate(&sim), RadarVisibilityResult::HIDDEN);
    assert_eq!(tracker.update_object(build_update(&sim, sub), false), None);
    assert!(!tracker.is_registered(sub));
    assert_eq!(
        tracker.update_object(build_update(&sim, entering), false),
        None
    );
    assert!(tracker.is_registered(entering));

    let detector = sim
        .spawn_object_at_height("DEST", "Americans", far_cell.0, far_cell.1, 0, 0, &rules)
        .unwrap();
    assert_eq!(evaluate(&sim), RadarVisibilityResult::HIDDEN);
    sim.substrate.occupancy.move_entity(
        far_cell.0,
        far_cell.1,
        cell.0,
        cell.1,
        detector,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
    {
        let detector = sim.substrate.entities.get_mut(detector).unwrap();
        detector.position.rx = cell.0;
        detector.position.ry = cell.1;
    }
    sim.refresh_unit_sensor_at_per_cell(detector, &rules);
    // CORRECTION: this used to require the sensor deposit to force-cloak the
    // state-zero resident. `TechnoClass+0x420 @ 0x006F4EB0` gates that arm on
    // `CellClass::IsVisibleToHouse` — the `CloakedByHouses` bit written only by
    // a `CloakGenerator=yes` building's field, which stock YR does not ship.
    // The deposit changes what the detector's house can SEE, never the
    // resident's cloak state.
    assert_eq!(
        sim.substrate
            .entities
            .get(entering)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        0,
        "a sensor deposit never starts a cloak in stock YR"
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, crate::sim::world::SimSoundEvent::CloakSound { .. })),
        "and therefore emits no entering-cloak cue"
    );
    assert_eq!(
        tracker.update_object(build_update(&sim, entering), false),
        None,
        "an uncloaked resident stays radar-visible across the callback edge"
    );
    assert!(
        tracker.is_registered(entering),
        "the accepted sensor callback and tracker reconciliation agree in the same operation"
    );
    assert_eq!(
        evaluate(&sim),
        RadarVisibilityResult {
            visible: true,
            out_code: 1,
        }
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(sub)
            .unwrap()
            .cloak
            .as_ref()
            .unwrap()
            .state,
        2,
        "+0x420 @ 0x006F4EB0 is sensor/redraw reevaluation, not forced uncloaking"
    );
    assert_eq!(
        tracker.update_object(build_update(&sim, sub), false),
        Some(
            crate::render::radar::tracker::RadarSensedPresentationEvent {
                stable_id: sub,
                out_code: 1,
                cell,
            }
        )
    );
    assert!(tracker.is_registered(sub));

    let second_detector_cell = (cell.0 + 1, cell.1);
    let second_detector = sim
        .spawn_object_at_height(
            "DEST",
            "Americans",
            second_detector_cell.0,
            second_detector_cell.1,
            0,
            0,
            &rules,
        )
        .unwrap();
    let sensor_index = usize::from(cell.1) * usize::from(sim.fog.width) + usize::from(cell.0);
    assert_eq!(sim.fog.sensors_by_house[&local][sensor_index], 2);
    sim.substrate.occupancy.move_entity(
        cell.0,
        cell.1,
        far_cell.0,
        far_cell.1,
        detector,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
    {
        let detector = sim.substrate.entities.get_mut(detector).unwrap();
        detector.position.rx = far_cell.0;
        detector.position.ry = far_cell.1;
    }
    sim.refresh_unit_sensor_at_per_cell(detector, &rules);
    assert_eq!(sim.fog.sensors_by_house[&local][sensor_index], 1);
    assert_eq!(evaluate(&sim).out_code, 1, "overlapping DEST remains");

    // Two equal FUN_006E21E0 writers still advance the global refresh
    // authority. The stock sensor deposit remains the live +0x328 input.
    assert!(sim.change_visible_map_area([2, 2, 56, 52], Some(&rules)));
    assert!(sim.change_visible_map_area([2, 2, 56, 52], Some(&rules)));
    assert_eq!(sim.playfield_revision, 2);
    assert_eq!(evaluate(&sim).out_code, 1);

    sim.substrate.occupancy.move_entity(
        second_detector_cell.0,
        second_detector_cell.1,
        far_cell.0,
        far_cell.1,
        second_detector,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
    {
        let detector = sim.substrate.entities.get_mut(second_detector).unwrap();
        detector.position.rx = far_cell.0;
        detector.position.ry = far_cell.1;
    }
    sim.refresh_unit_sensor_at_per_cell(second_detector, &rules);
    assert_eq!(sim.fog.sensors_by_house[&local][sensor_index], 0);
    assert_eq!(evaluate(&sim), RadarVisibilityResult::HIDDEN);
    assert_eq!(tracker.update_object(build_update(&sim, sub), false), None);
    assert!(
        !tracker.is_registered(sub),
        "same committed sensor move makes the presentation tracker unregister"
    );
}
