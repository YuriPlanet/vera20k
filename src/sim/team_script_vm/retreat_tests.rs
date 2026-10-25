use super::super::*;
use super::*;
use crate::rules::retail_ini_fixture::{retail_battle_rules_for_map, retail_ini};
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::FrameEffects;
use serde_json::Value;

/// The stock six-member Allied base guard loses four members, then reaches
/// the real Team AI under-strength branch with a friendly unarmed base.
#[test]
fn retail_base_guard_survivors_retreat_toward_a_friendly_building() {
    retail_guard_retreat(true);
}

#[test]
fn retail_base_guard_keeps_centre_when_native_entry_refuses_flat_ground() {
    retail_guard_retreat(false);
}

fn retail_guard_retreat(ramp: bool) {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let Some(aimd) = retail_ini("aimd.ini") else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let rules = retail.rules;
    let registry = TeamAiIniRegistry::from_sources(
        &aimd,
        &crate::rules::ini_parser::IniFile::from_str(""),
        true,
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1302);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert_for_test(crate::sim::house_state::HouseState::new(
            owner, 0, None, false, 0, 10,
        ));
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    sim.session.set_binary_frame_for_test(100);
    sim.spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
        .unwrap();
    // A nearer armed building must not become the retreat anchor.
    sim.spawn_object("GAPILL", "Americans", 18, 18, 0, &rules)
        .unwrap();
    for (index, kind) in ["E1", "E1", "E1", "GGI", "GGI", "GGI"].iter().enumerate() {
        sim.spawn_object(
            kind,
            "Americans",
            23 + index as u16 % 3,
            23 + index as u16 / 3,
            0,
            &rules,
        )
        .unwrap();
    }
    let definition = sim.interner.get("08DA30EC-G").unwrap();
    let team = sim.create_team(definition, Some(owner), &rules).unwrap();
    for _ in 0..6 {
        sim.run_team_ai_pass(&rules, None, FrameEffects::default());
        let state = &sim.team_script_vm.teams[&team];
        if state.members.len() == 6 && state.formed {
            break;
        }
    }
    let state = &sim.team_script_vm.teams[&team];
    assert_eq!(state.members.len(), 6, "stock TaskForce must be recruited");
    assert!(state.formed && state.has_been_full);
    let casualties: Vec<_> = state
        .members
        .iter()
        .take(4)
        .map(|member| member.id)
        .collect();
    for member in casualties {
        sim.uninit_with_rules(member, &rules);
    }
    // Original 4CBC40 passes height=-1 plus the edge direction. On
    // level-zero flat cells, Foot4D9C60 refuses these asks. A source ramp
    // admits them (whole original Infantry/Unit controls in team_retreat.json).
    if ramp {
        set_slopes(&mut sim, 1);
    }
    // Exercise the production restore seam on its documented seed-zero
    // Scenario cursor and freshly published map hierarchy (native load rebuilds it).
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.restore_map_authority_after_snapshot_load(&rules, &overlays, None)
        .unwrap();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    // Player load discards active planning (ClearScene6370B0 / TechnoLoad70BF79).
    sim.reset_planning_after_player_load();
    let before_rng = rng(&sim);
    let timers = {
        let state = &sim.team_script_vm.teams[&team];
        (state.guard_timer, state.suspend_timer)
    };
    let mut restored = snapshot_copy(&sim, &rules, &overlays);
    assert_same_hash(&sim, &restored);
    for world in [&mut sim, &mut restored] {
        world.run_team_ai_pass(&rules, None, FrameEffects::default());
        assert_eq!(rng(world), before_rng, "retreat must not draw RNG");
        let state = &world.team_script_vm.teams[&team];
        assert_eq!((state.guard_timer, state.suspend_timer), timers);
    }
    assert_same_hash(&sim, &restored);
    let state = &sim.team_script_vm.teams[&team];
    assert_eq!(state.members.len(), 2);
    assert!(state.under_strength && !state.formed);
    let Some(TeamTarget::Cell { x, y }) = state.focus else {
        panic!("retreat must choose a cell");
    };
    assert_eq!(state.cursor, -1);
    if ramp {
        assert!(
            x < 12 && y < 12,
            "survivors must fall back to their unarmed base: ({x},{y})"
        );
        for member in &state.members {
            let entity = sim.substrate.entities.get(member.id).unwrap();
            assert_eq!(
                entity.mission.effective(),
                MissionId::from_known(MissionType::Move)
            );
            assert_eq!(
                entity.navigation.nav_com(),
                Some(NavTargetRef::cell(x as u16, y as u16))
            );
            assert_eq!(
                sim.team_script_vm.member_team.get(&member.id).copied(),
                Some(team)
            );
        }
    } else {
        assert!(x >= 20 && y >= 20, "original refuses flat nearby cells");
    }
    // Persist the chosen destination/orders and continue the real pass.
    let mut restored = snapshot_copy(&sim, &rules, &overlays);
    assert_same_hash(&sim, &restored);
    for world in [&mut sim, &mut restored] {
        world
            .session
            .set_binary_frame_for_test(world.session.binary_frame() + 1);
        world.run_team_ai_pass(&rules, None, FrameEffects::default());
    }
    assert_same_hash(&sim, &restored);
}

fn snapshot_copy(
    sim: &Simulation,
    rules: &RuleSet,
    overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> Simulation {
    let bytes = GameSnapshot::save_validated(sim, 0, 0, "retail-guard-retreat", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(sim));
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        sim.terrain_speed_config().clone(),
        rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(rules, overlays, None)
        .unwrap();
    restored.resolve_type_handles(rules);
    restored
}

fn assert_same_hash(first: &Simulation, second: &Simulation) {
    assert_eq!(
        first.state_hash(),
        second.state_hash(),
        "source owners {:?}; restored owners {:?}",
        first.hash_owner_diagnostics_for_test(),
        second.hash_owner_diagnostics_for_test()
    );
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn set_slopes(sim: &mut Simulation, slope: u8) {
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 0..32 {
        for x in 0..32 {
            terrain.cell_mut_for_test(x, y).unwrap().slope_type = slope;
        }
    }
}

fn synthetic_rules() -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=SOLDIER\n[VehicleTypes]\n0=VEHICLE\n\
         [BuildingTypes]\n0=BASE\n1=ARMED\n2=DEPOT\n\
         [SOLDIER]\nStrength=100\nSpeed=4\nLeadershipRating=5\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [VEHICLE]\nStrength=100\nSpeed=4\nLeadershipRating=5\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [BASE]\nStrength=500\nFoundation=1x1\n\
         [ARMED]\nStrength=500\nFoundation=1x1\nPrimary=GUN\n\
         [DEPOT]\nStrength=500\nFoundation=1x1\nUnitRepair=yes\n\
         [GUN]\nDamage=10\nROF=20\nRange=6\nWarhead=SA\n\
         [SA]\nVerses=100%\n",
    ))
    .unwrap()
}

fn pair(value: &Value) -> (i16, i16) {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

#[test]
fn foot_nearby_matches_original_whole_infantry_and_unit_admission() {
    let rules = synthetic_rules();
    let corpus = crate::test_fixture::json("tools/team_retreat.json");
    for row in corpus["live_nearby"].as_array().unwrap() {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1302);
        let kind = if row["kind"] == "infantry" {
            "SOLDIER"
        } else {
            "VEHICLE"
        };
        let member = sim
            .spawn_object(kind, "Russians", 20, 20, 0, &rules)
            .unwrap();
        set_slopes(&mut sim, row["slope"].as_u64().unwrap() as u8);
        let before_rng = rng(&sim);
        let result = sim.foot_nearby_location(
            member,
            pair(&row["source"]),
            pair(&row["target"]),
            2..4,
            &rules,
            None,
        );
        let expected = pair(&row["result"]);
        assert_eq!(result, (expected != (0, 0)).then_some(expected), "{row}");
        assert_eq!(rng(&sim), before_rng);
    }
}

/// Replays whole original retreat selections through the actual ordered House
/// projection, GetWeapon, threat owner and class admission. The native selector
/// controls supply entry replies; separate whole-class controls above establish
/// the admitted ramp input used here. Coordinates are declared by native rows.
#[test]
fn retreat_building_selection_matches_original_controls() {
    let rules = synthetic_rules();
    let corpus = crate::test_fixture::json("tools/team_retreat.json");
    for row in corpus["selections"].as_array().unwrap() {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1302);
        sim.session.set_binary_frame_for_test(100);
        let owner = sim.interner.intern("Russians");
        let mut members = Vec::new();
        for i in 0..2 {
            let id = sim
                .spawn_object("SOLDIER", "Russians", 20 + i, 20, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .position
                .put_location(DriveCoord {
                    x: 5248 + i32::from(i) * 256,
                    y: 5248,
                    z: 0,
                });
            members.push(id);
        }
        let script = sim.interner.intern("RETREAT");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 58,
                argument: 0,
            }],
        });
        let team = sim.team_script_vm.create_team(owner, script, members, 100);
        for building in row["buildings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| !b.is_null())
        {
            let kind = if building["armed"] == true {
                "ARMED"
            } else if building["repair"] == true {
                "DEPOT"
            } else {
                "BASE"
            };
            let coords: Vec<i32> = building["xyz"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let id = sim
                .spawn_object(
                    kind,
                    "Russians",
                    (coords[0] / 256) as u16,
                    (coords[1] / 256) as u16,
                    0,
                    &rules,
                )
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.position.put_location(DriveCoord {
                x: coords[0],
                y: coords[1],
                z: coords[2],
            });
            if building["limbo"] == true {
                entity.lifecycle.set_in_limbo_for_test(true);
            }
            let actual = sim.team_target_coord(TeamTarget::Object(id)).unwrap();
            assert_eq!(
                [actual.x, actual.y, actual.z],
                coords.as_slice(),
                "native coordinate input"
            );
            let threat = building["threat"].as_i64().unwrap_or(0) as i32;
            if threat != 0 {
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .adjust_spatial_threat(
                        ((coords[0] / 256) as i16, (coords[1] / 256) as i16),
                        threat,
                    )
                    .unwrap();
            }
        }
        set_slopes(&mut sim, 1);
        for blocked in row["blocked"].as_array().unwrap() {
            let (x, y) = pair(blocked);
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(x as u16, y as u16)
                .unwrap();
            cell.speed_costs.foot = None;
            cell.base_speed_costs.foot = None;
        }
        let before_rng = rng(&sim);
        sim.team_under_strength_retreat(team, &rules, None, FrameEffects::default());
        let (x, y) = pair(&row["focus"]);
        let state = &sim.team_script_vm.teams[&team];
        assert_eq!(
            state.focus,
            Some(TeamTarget::Cell { x, y }),
            "{}",
            row["name"]
        );
        assert_eq!(state.formed, row["formed"].as_u64().unwrap() != 0);
        assert_eq!(rng(&sim), before_rng);
    }
}
