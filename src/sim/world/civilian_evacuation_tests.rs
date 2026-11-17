//! Full65DD30 -> Script11 -> Aircraft Retreat415A50 -> Fly4CD510 ->
//! House polling -> event18. The flat arena isolates this authored mission
//! route; original gamemd.exe instructions establish the expected predicates.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::snapshot::GameSnapshot;

fn frame(sim: &mut Simulation, rules: &RuleSet, overlays: &OverlayTypeRegistry) {
    sim.advance_app_frame(
        &[],
        rules,
        Some(overlays),
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
}

fn reload(sim: &Simulation, rules: &RuleSet, overlays: &OverlayTypeRegistry) -> Simulation {
    let bytes = GameSnapshot::save_validated(sim, 0, 0, "Civilian evacuation", 0);
    let mut loaded = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    loaded.retain_process_state_for_load(SnapshotProcessState::Live(sim));
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        sim.terrain_speed_config().clone(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, overlays, None)
        .unwrap();
    loaded.resolve_type_handles(rules);
    loaded
}

#[test]
fn full_aircraft_evacuates_the_civilian_passenger_house_after_save_and_polling() {
    let patch = IniFile::from_str(
        "[ORCA]\nPassengers=2\nPrimary=none\nSecondary=none\nAirportBound=no\nLandable=yes\n",
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    ) else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.object("CIV1").unwrap().civilian);
    assert!(!rules.object("E1").unwrap().civilian);
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=2,2,28,24\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n\
          [Waypoints]\n0=20020\n[TeamTypes]\n0=EVAC\n\
          [EVAC]\nHouse=Americans\nScript=LEAVE\nTaskForce=PAYLOAD\nWaypoint=A\nFull=yes\n\
          [ScriptTypes]\n0=LEAVE\n[LEAVE]\n0=11,4\n\
          [TaskForces]\n0=PAYLOAD\n[PAYLOAD]\n0=1,ORCA\n1=1,CIV1\n2=1,E1\n\
          [Triggers]\nARRIVE=Americans,<none>,Arrival,0,1,1,1,0\n\
          SAVED=Russians,<none>,Evacuated,0,1,1,1,0\n\
          [Events]\nARRIVE=1,13,0,0\nSAVED=1,18,0,0\n\
          [Actions]\nARRIVE=1,80,1,EVAC,0,0,0,0,A\nSAVED=1,28,0,7,0,0,0,0,A\n\
          [Tags]\nARRIVE_TAG=0,Arrival,ARRIVE\nSAVED_TAG=0,Evacuated,SAVED\n",
    )
    .unwrap();
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let teams = TeamAiIniRegistry::load_retail(&assets, &map.ini, true).unwrap();
    for survivor in [false, true] {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4CD510);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.set_playfield_bounds_for_test(Some(
            crate::map::playfield::PlayfieldBounds::from_map_header(&map.header),
        ));
        sim.set_playfield_size_height_for_test(Some(32));
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        let carrier_owner = sim.interner.get("Americans").unwrap();
        let passenger_owner = sim.interner.get("Russians").unwrap();
        for house in sim.houses.values_mut() {
            house.set_country_for_test(Some(house.name()));
            assert!(!house.civilians_evacuated());
        }
        sim.session.set_current_house_for_test(Some(carrier_owner));
        sim.session.game_options_mut_for_test().short_game = false;
        sim.install_team_ai_registry(&teams, rules);
        sim.session.install_waypoints(&map.waypoints);
        sim.initialize_trigger_definitions(&map, &teams, None, None, None);
        sim.classify_map_triggers(rules);
        sim.session.publish_started();
        for _ in 0..32 {
            frame(&mut sim, rules, &overlays);
            if sim
                .entities()
                .values()
                .any(|e| e.category() == EntityCategory::Aircraft)
            {
                break;
            }
        }
        let plane = sim
            .entities()
            .values()
            .find(|e| e.category() == EntityCategory::Aircraft)
            .unwrap()
            .stable_id();
        let passenger = sim
            .entities()
            .values()
            .find(|e| sim.interner.resolve(e.type_ref()) == "CIV1")
            .unwrap()
            .stable_id();
        let cargo = sim
            .entities()
            .get(plane)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers()
            .to_vec();
        assert_eq!(cargo.len(), 2, "Full attaches both members before flight");
        assert!(cargo.contains(&passenger));
        // Controlled lifecycle boundary: transfer a held passenger through
        // the normal owner, so the carrier's House cannot stand in for it.
        sim.change_owner_with_rules(
            passenger,
            passenger_owner,
            rules,
            Some(&overlays),
            FrameEffects::default(),
        );
        if survivor {
            // Existing crew-survival owner supplies this byte independently
            // of Civilian; this control changes only that input predicate.
            sim.substrate
                .entities
                .get_mut(passenger)
                .unwrap()
                .mark_nominal_survivor();
        }
        assert_eq!(sim.is_evacuation_civilian(passenger, rules), !survivor);
        assert!(sim.entities().get(passenger).unwrap().lifecycle.in_limbo());
        assert!(!sim.global_variables()[7]);
        let loaded = reload(&sim, rules, &overlays);
        for mut world in [sim, loaded] {
            for _ in 0..600 {
                frame(&mut world, rules, &overlays);
                if !world.entities().contains(plane) {
                    break;
                }
            }
            assert!(
                !world.entities().contains(plane),
                "Script11 Retreat must leave the playfield"
            );
            assert!(
                cargo.iter().all(|id| !world.entities().contains(*id)),
                "Fly teardown removes its cargo"
            );
            assert_eq!(
                world.houses[&passenger_owner].civilians_evacuated(),
                !survivor
            );
            assert!(
                !world.houses[&carrier_owner].civilians_evacuated(),
                "E1 and the aircraft do not qualify"
            );
            for _ in 0..9 {
                frame(&mut world, rules, &overlays);
            }
            assert_eq!(
                world.global_variables()[7],
                !survivor,
                "ordinary House polling evaluates event18"
            );
            let restored = reload(&world, rules, &overlays);
            assert_eq!(
                restored.houses[&passenger_owner].civilians_evacuated(),
                !survivor
            );
            assert_eq!(restored.global_variables()[7], !survivor);
            if survivor {
                let mut changed = restored;
                let hash = changed.state_hash();
                changed
                    .houses
                    .get_mut(&passenger_owner)
                    .unwrap()
                    .mark_civilians_evacuated();
                assert_ne!(
                    changed.state_hash(),
                    hash,
                    "the event input belongs to deterministic state"
                );
            }
        }
    }
}
