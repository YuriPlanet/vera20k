use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::{
    command::Command,
    overlay_grid::OverlayGrid,
    pathfinding::{PathGrid, zone_map::ZoneGrid},
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) fn fixture() -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    fixture_with_rules("")
}

pub(super) fn fixture_with_rules(
    extra: &str,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    let mut text = String::from(
        "[InfantryTypes]\n0=ENGINEER\n1=JUMPJET\n[JUMPJET]\nStrength=125\nSpeed=9\nSpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\nJumpjetHeight=500\nJumpjetClimb=20\nJumpJet=yes\nBalloonHover=yes\nHoverAttack=yes\nLocomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n[AircraftTypes]\n0=HORNET\n[HORNET]\nLandable=yes\nSpeed=12\nSpeedType=Winged\nStrength=75\nLocomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n[BuildingTypes]\n0=CABHUT\n[ENGINEER]\nEngineer=yes\nSpeed=4\nSpeedType=Foot\nStrength=75\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n[CABHUT]\nBridgeRepairHut=yes\nFoundation=1x1\nStrength=200\n[Warheads]\n0=SA\n1=Super\n[Super]\nInfDeath=2\nPenetratesBunker=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[CombatDamage]\nC4Warhead=SA\n[SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n",
    );
    for id in 0..=238 {
        text.push_str(&format!("{id}=O{id}\n"));
    }
    for id in 0..=238 {
        text.push_str(&format!("[O{id}]\nLand=Clear\nNoUseTileLandType=no\n"));
    }
    for land in crate::rules::terrain_rules::LandType::ALL.iter().take(9) {
        text.push_str(&format!(
            "[{}]\nFoot=100%\nTrack=100%\nWheel=100%\nBuildable=yes\n",
            land.section_name()
        ));
    }
    let mut ini = IniFile::from_str(&text);
    ini.merge(&IniFile::from_str(extra));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut terrain = ResolvedTerrainGrid::from_cells(
        33,
        33,
        (0..33)
            .flat_map(|y| {
                (0..33).map(move |x| {
                    crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                })
            })
            .collect(),
    );
    crate::map::resolved_terrain::install_ordinary_repair_test_catalog(&mut terrain);
    terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&ini);
    let costs = terrain_rules
        .semantics_for_land_type(0)
        .unwrap()
        .speed_costs;
    for y in 0..33 {
        for x in 0..33 {
            let c = terrain.cell_mut(x, y).unwrap();
            c.speed_costs = costs.clone();
            c.base_speed_costs = costs.clone();
        }
    }
    let bounds =
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 16, 16);
    let bridges =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (16, 16));
    let path = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_native_map_context(
        &path,
        &BTreeMap::new(),
        &terrain,
        bridges.endpoint_records(),
        Some((16, 16)),
        Some(bounds),
    );
    assert!(zones.hierarchy_for(MovementZone::Normal).is_some());
    let mut sim = Simulation::with_seed(31);
    // Match production initialization: every registered type exists before
    // building the derived table, including types first spawned after load.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.playfield_bounds = Some(bounds);
    sim.session.map_width = 33;
    sim.session.map_height = 33;
    sim.overlay_grid = Some(OverlayGrid::new(33, 33));
    sim.bridge_state = Some(bridges);
    sim.zone_grid = Some(zones);
    sim.path_grid = Some(Arc::new(path));
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.terrain_costs = crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    for p in [(15, 15), (16, 15), (17, 14), (17, 15), (17, 16)] {
        assert!(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (p.0, p.1),
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref()
        ));
    }
    (sim, rules, registry)
}

fn broken_strip(sim: &mut Simulation) {
    for (x, y) in [(17, 14), (17, 15), (17, 16)] {
        let c = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(x, y)
            .unwrap();
        c.bridge_facts.overlay_id = Some(231);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .write_bridge_overlay_identity(x, y, 231);
    }
}

#[test]
fn ordinary_engineer_command_repairs_from_completed_walk_in_object_turn() {
    command_repair_fixture(false, false);
}

#[test]
fn slave_master_admission_reaches_head_selection_in_the_same_object_turn() {
    use crate::map::entities::EntityCategory;
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
    use crate::sim::occupancy::{CellListInsertion, infantry_raw_occupation_mask};
    use crate::util::fixed_math::SimFixed;
    for later_blocker in [false, true] {
        let (mut sim, rules, registry) = fixture();
        let master = sim
            .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        let slave = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        // Supplied manager/deposit leg tests the real object-turn admission
        // continuation, not production slave AI or a stock hut manager.
        assert!(crate::sim::movement::issue_direct_move(
            &mut sim.substrate.entities,
            slave,
            (16, 15),
            SimFixed::from_num(150),
            crate::sim::movement::DestinationTiming::from_rules(
                sim.session.binary_frame,
                Some(&rules)
            ),
        ));
        sim.mission_assign_exact(
            slave,
            crate::sim::mission::MissionId::from_known(
                crate::rules::mission_data::MissionType::Move,
            ),
            sim.session.binary_frame,
        )
        .unwrap();
        let e = sim.substrate.entities.get_mut(slave).unwrap();
        e.slave_harvester = Some(crate::sim::slave_miner::SlaveHarvester::new(master, 4));
        e.navigation.nav_com = Some(NavTargetRef::Cell { rx: 16, ry: 15 });
        e.locomotor
            .as_mut()
            .unwrap()
            .set_walk_destination(Some(DriveCoord::cell(16, 15, 0)));
        // As for the existing building-enter producer, static path blocking
        // is bypassed for the admitted last leg; live objects still decide.
        e.movement_target.as_mut().unwrap().bypass_grid = true;
        let before = ground_pose::position_world_coord(&e.position);
        sim.production.slave_bindings.insert(master, vec![slave]);
        if later_blocker {
            let mut b = crate::sim::game_entity::GameEntity::test_default(
                100,
                "CABHUT",
                "Americans",
                16,
                15,
            );
            b.owner = sim.intern("Americans");
            b.type_ref = sim.intern("CABHUT");
            b.category = EntityCategory::Structure;
            sim.substrate.entities.insert(b);
            sim.substrate.occupancy.add(
                16,
                15,
                100,
                MovementLayer::Ground,
                None,
                CellListInsertion::AppendBuilding,
            );
        }
        let grid = sim.path_grid_snapshot();
        sim.advance_live_object_pass(Some(&rules), grid.as_deref(), Some(&registry))
            .expect("fixture frame must complete");
        let e = sim.substrate.entities.get(slave).unwrap();
        let head = e.locomotor.as_ref().unwrap().step_head();
        if later_blocker {
            // Can_Enter_Cell 7 with a Building in the target cell takes the
            // Find_Path code-7 arm (0x4D3CDD..0x4D3E03): FNPC redirects the
            // destination to a passable cell near the target, nearest to the
            // actor, and the search continues there.
            let head = head.expect("the code-7 redirect still yields a fresh head");
            let redirected = e.locomotor.as_ref().unwrap().walk_destination().unwrap();
            let redirected = (redirected.x / 256, redirected.y / 256);
            assert_ne!(redirected, (16, 15), "the Building cell is not searched");
            assert!(
                (redirected.0 - 16).abs() <= 1 && (redirected.1 - 15).abs() <= 1,
                "FNPC picked {redirected:?} away from the target ring"
            );
            assert_eq!((head.x / 256, head.y / 256), redirected);
            assert_eq!(
                e.navigation.nav_com,
                Some(NavTargetRef::Cell {
                    rx: redirected.0 as u16,
                    ry: redirected.1 as u16,
                })
            );
            assert_eq!(ground_pose::position_world_coord(&e.position), before);
        } else {
            let head = head.expect("Clear CanEnter resumes fresh-head selection in this call");
            assert_eq!((head.x / 256, head.y / 256), (16, 15));
            assert_eq!(
                ground_pose::position_world_coord(&e.position),
                before,
                "original75C240 fresh-head publication returns before paid motion"
            );
            assert_eq!(
                e.locomotor.as_ref().unwrap().walk_animation_moving(),
                Some(true)
            );
            let mask = infantry_raw_occupation_mask(
                SimFixed::from_num(head.x % 256),
                SimFixed::from_num(head.y % 256),
            );
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(16, 15) & mask,
                mask
            );
            assert_eq!(
                sim.substrate
                    .raw_cell_occupation
                    .ground_infantry_owner(16, 15),
                Some(e.owner())
            );
        }
    }
}

#[test]
fn engineer_repair_damages_landed_fly_occupant_before_its_later_turn() {
    //Supplied just-landed Fly state; native carrier/Hornet reachability is
    //proved separately. This exercises repair and the actual damage receiver.
    command_repair_fixture(true, false);
}

#[test]
fn engineer_repair_damages_landed_fly_with_known_non_waypoint_team_script() {
    command_repair_fixture(true, true);
}

fn command_repair_fixture(with_aircraft: bool, with_team: bool) {
    let (mut sim, rules, registry) = fixture();
    let hut = sim
        .spawn_object("CABHUT", "Soviets", 16, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let engineer = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    assert!(sim.substrate.entities.get(engineer).is_some());
    assert_eq!(
        sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .kind,
        crate::rules::locomotor_type::LocomotorKind::Walk
    );
    broken_strip(&mut sim);
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command(
        "Americans",
        &Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: hut
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new()
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .navigation
            .nav_com,
        Some(crate::sim::components::NavTargetRef::Building { id: hut }),
        "the real order publishes Foot NavCom before head selection"
    );
    let mut saw_boundary_before_completion = false;
    let mut changed = false;
    let mut trace = Vec::new();
    let mut aircraft = None;
    for _ in 0..100 {
        if with_aircraft
            && aircraft.is_none()
            && sim.substrate.entities.get(engineer).is_some_and(|e| {
                let Some(head) = e.locomotor.as_ref().and_then(|l| l.step_head()) else {
                    return false;
                };
                let xy = crate::sim::movement::ground_pose::position_world_xy(&e.position);
                let dx = head.x - xy[0];
                let dy = head.y - xy[1];
                dx * dx + dy * dy < 17 * 17
            })
        {
            let id = sim
                .spawn_object_at_height("HORNET", "Americans", 17, 15, 0, 0, &rules)
                .unwrap();
            assert!(id > engineer, "repair precedes this aircraft's Logic visit");
            assert!(sim.substrate.occupancy.get(17, 15).is_some_and(|list| {
                list.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
                    .any(|e| e.entity_id == id)
            }));
            if with_team {
                let script_id = sim.intern("REPAIR_TEAM_SCRIPT");
                let owner = sim.substrate.entities.get(id).unwrap().owner();
                sim.team_script_vm.register_script(
                    crate::sim::team_script_vm::TeamScriptDefinition {
                        id: script_id,
                        actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                            action_id: 0,
                            argument: 0,
                        }],
                        source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                    },
                );
                sim.team_script_vm.create_team(
                    owner,
                    script_id,
                    vec![id],
                    None,
                    sim.session.binary_frame as i32,
                );
                assert!(sim.team_script_vm.team_for_member(id).is_some());
            }
            aircraft = Some(id);
        }
        let grid = sim.path_grid_snapshot();
        let result = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            grid.as_deref(),
            Some(&registry),
            67,
        );
        changed |= result.bridge_state_changed;
        if let Some(e) = sim.substrate.entities.get(engineer) {
            trace.push(format!(
                "frame{} cell{},{} sub{},{} head{:?} mt{:?} mission{:?} alive{}",
                sim.session.binary_frame,
                e.position.rx,
                e.position.ry,
                e.position.sub_x,
                e.position.sub_y,
                e.locomotor.as_ref().unwrap().step_head(),
                e.movement_target,
                e.mission.current(),
                e.lifecycle.object_alive
            ));
            if e.position.rx == 16 && e.locomotor.as_ref().unwrap().step_head().is_some() {
                assert!(!changed, "crossing alone must not emit PerCell repair");
                if !saw_boundary_before_completion {
                    let mask = crate::sim::occupancy::infantry_raw_occupation_mask(
                        e.position.sub_x,
                        e.position.sub_y,
                    );
                    assert_ne!(
                        (if e.on_bridge {
                            sim.substrate
                                .raw_cell_occupation
                                .deck_bits(e.position.rx, e.position.ry)
                        } else {
                            sim.substrate
                                .raw_cell_occupation
                                .ground_bits(e.position.rx, e.position.ry)
                        }) & mask,
                        0,
                        "current XYZ is marked before a later object's repair query; marked={} enabled={} bridge={} raw={:x}/{:x} mask={mask:x} trace={trace:#?}",
                        e.lifecycle.cell_marked,
                        e.foot_occupation_enabled,
                        e.on_bridge,
                        sim.substrate
                            .raw_cell_occupation
                            .ground_bits(e.position.rx, e.position.ry),
                        sim.substrate
                            .raw_cell_occupation
                            .deck_bits(e.position.rx, e.position.ry)
                    );
                }
                saw_boundary_before_completion = true;
            }
        } else {
            break;
        }
    }
    assert!(saw_boundary_before_completion, "{trace:#?}");
    assert!(
        changed,
        "real object-turn repair must publish its outcome: {trace:#?}"
    );
    assert!(
        sim.substrate.entities.get(engineer).is_none(),
        "same-call Uninit followed by normal frame-tail deletion"
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(16, 15) & 0x1c,
        0
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(16, 15),
        None
    );
    for (x, y) in [(17, 14), (17, 15), (17, 16)] {
        assert!(
            (205..=208).contains(
                &sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .bridge_facts
                    .overlay_id
                    .unwrap()
            )
        );
        assert_eq!(
            sim.path_grid().unwrap().cell(x, y).unwrap().ground_level,
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(x, y)
                .unwrap()
                .level
        );
    }
    assert!(
        sim.zone_grid
            .as_mut()
            .unwrap()
            .base_topology_mut()
            .is_some()
    );
    assert!(sim.terrain_costs.contains_key(&SpeedType::Foot));
    if with_aircraft {
        let id = aircraft.expect("fixture must install the landed Fly before PerCell");
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|e| !e.lifecycle.object_alive)
        );
        assert!(!sim.substrate.occupancy.contains_entity(17, 15, id));
    }
}

#[test]
fn walk_boundary_marks_current_xyz_without_replacing_head_or_consuming_path() {
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::{ground_pose, locomotor::MovementLayer, walk_head};
    use crate::util::fixed_math::SimFixed;

    let (mut sim, rules, registry) = fixture();
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: 17,
            target_ry: 15,
            queue: false,
            group_id: None,
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new(),
    ));
    drop(grid);
    let head = DriveCoord {
        x: 16 * 256 + 192,
        y: 15 * 256 + 192,
        z: 0,
    };
    let (owner, current, queue, path_index) = {
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.position.sub_x = SimFixed::from_num(192);
        e.position.sub_y = SimFixed::from_num(64);
        e.locomotor.as_mut().unwrap().set_step_head(Some(head));
        (
            e.owner(),
            ground_pose::position_world_coord(&e.position),
            e.navigation.path_replay.clone(),
            e.movement_target.as_ref().unwrap().next_index,
        )
    };
    sim.substrate
        .raw_cell_occupation
        .clear_ground_infantry(15, 15, 0x1f);
    for coord in [current, head] {
        walk_head::raw_at(
            &mut sim.substrate.raw_cell_occupation,
            owner,
            coord,
            true,
            sim.resolved_terrain.as_ref(),
            None,
        );
    }
    let crossing = DriveCoord {
        x: 16 * 256 + 8,
        y: 15 * 256 + 64,
        z: 0,
    };
    sim.run_walk_boundary(id, crossing, Some(&rules), None, Some(&registry));
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!(ground_pose::position_world_coord(&e.position), crossing);
    assert!(e.lifecycle.cell_marked && e.foot_occupation_enabled);
    assert_eq!(e.sub_cell, Some(0));
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
    assert_eq!(e.navigation.path_replay, queue);
    assert_eq!(e.movement_target.as_ref().unwrap().next_index, path_index);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(15, 15) & 0x1f,
        0
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(16, 15) & 0x1f,
        0x11
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(16, 15),
        Some(owner)
    );
    assert!(sim.substrate.occupancy.get(15, 15).is_none_or(|list| {
        !list
            .iter_layer(MovementLayer::Ground)
            .any(|entry| entry.entity_id == id)
    }));
    assert!(sim.substrate.occupancy.get(16, 15).is_some_and(|list| {
        list.iter_layer(MovementLayer::Ground)
            .any(|entry| entry.entity_id == id && entry.sub_cell == Some(0))
    }));
    assert!(repair_sounds(&sim).is_empty());
}

#[test]
fn diagonal_walk_relinks_the_first_actual_side_cell_before_reaching_its_head() {
    use crate::sim::movement::ground_pose;
    use crate::util::fixed_math::SimFixed;
    let (mut sim, rules, registry) = fixture();
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    {
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.position.sub_x = SimFixed::from_num(192);
        e.position.sub_y = SimFixed::from_num(64);
    }
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: 16,
            target_ry: 16,
            queue: false,
            group_id: None,
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new()
    ));
    drop(grid);
    for _ in 0..100 {
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            grid.as_deref(),
            Some(&registry),
            67,
        );
        let e = sim.substrate.entities.get(id).unwrap();
        let xy = ground_pose::position_world_xy(&e.position);
        if xy[0] >= 16 * 256 {
            assert!(xy[1] < 16 * 256, "unequal offsets must cross X first");
            assert_eq!((e.position.rx, e.position.ry), (16, 15));
            let head = e.locomotor.as_ref().unwrap().step_head().unwrap();
            assert_eq!((head.x / 256, head.y / 256), (16, 16));
            assert!(sim.substrate.occupancy.contains_entity(16, 15, id));
            assert!(!sim.substrate.occupancy.contains_entity(15, 15, id));
            let mask = crate::sim::occupancy::infantry_raw_occupation_mask(
                e.position.sub_x,
                e.position.sub_y,
            );
            assert_ne!(
                sim.substrate.raw_cell_occupation.ground_bits(16, 15) & mask,
                0
            );
            return;
        }
    }
    panic!("diagonal walker never reached first side cell");
}

#[test]
fn refused_fresh_walk_head_restores_the_current_raw_occupation() {
    use crate::util::fixed_math::SimFixed;
    let (mut sim, rules, registry) = fixture();
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let owner = {
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.position.sub_x = SimFixed::from_num(192);
        e.position.sub_y = SimFixed::from_num(64);
        e.owner()
    };
    sim.substrate
        .raw_cell_occupation
        .clear_ground_infantry(15, 15, 0x1f);
    sim.substrate
        .raw_cell_occupation
        .mark_ground_infantry(15, 15, 4, owner);
    // These are retained heads, with no listed infantry at the destination.
    sim.substrate
        .raw_cell_occupation
        .mark_ground_infantry(16, 15, 0x1c, owner);
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: id,
            target_rx: 16,
            target_ry: 15,
            queue: false,
            group_id: None,
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new()
    ));
    drop(grid);
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(
        &[],
        Some(&rules),
        &BTreeMap::new(),
        grid.as_deref(),
        Some(&registry),
        67,
    );
    let e = sim.substrate.entities.get(id).unwrap();
    assert_eq!((e.position.rx, e.position.ry), (15, 15));
    assert_eq!(e.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(15, 15) & 0x1c,
        4
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_infantry_owner(15, 15),
        Some(owner)
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(16, 15) & 0x1c,
        0x1c
    );
}

#[test]
fn production_fresh_head_and_raw_history_match_original_walk_producer() {
    use crate::sim::components::DriveCoord;
    use crate::util::fixed_math::SimFixed;
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_head_occupation.json"
    ))
    .unwrap();
    let cases = data["producer"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let (mut sim, rules, registry) = fixture();
        let id = sim
            .spawn_object("ENGINEER", "Americans", 9, 10, 0, &rules, &BTreeMap::new())
            .unwrap();
        for x in [9, 10] {
            let c = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, 10)
                .unwrap();
            c.level = 2;
            c.slope_type = 1;
        }
        sim.path_grid = Some(Arc::new(PathGrid::from_resolved_terrain(
            sim.resolved_terrain.as_ref().unwrap(),
        )));
        let owner = {
            let e = sim.substrate.entities.get_mut(id).unwrap();
            e.position.sub_x = SimFixed::from_num(192);
            e.position.sub_y = SimFixed::from_num(64);
            e.position.z = 2;
            e.position.exact_z_leptons = Some(260);
            e.owner()
        };
        sim.substrate
            .raw_cell_occupation
            .clear_ground_infantry(9, 10, 0x1f);
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(9, 10, 4, owner);
        let ground = case["input"]["ground"].as_u64().unwrap() as u8;
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(10, 10, ground, owner);
        let grid = sim.path_grid_snapshot();
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: id,
                target_rx: 10,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            grid.as_deref(),
            &BTreeMap::new()
        ));
        drop(grid);
        sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            None,
            Some(&registry),
            67,
        );
        let output = &case["output"];
        let expected = DriveCoord {
            x: output["head"][0].as_i64().unwrap() as i32,
            y: output["head"][1].as_i64().unwrap() as i32,
            z: output["head"][2].as_i64().unwrap() as i32,
        };
        let accepted = output["accepted"].as_bool().unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .step_head(),
            accepted.then_some(expected),
            "{case}"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(9, 10),
            output["current_ground"].as_u64().unwrap() as u8,
            "{case}"
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(10, 10),
            output["ground"].as_u64().unwrap() as u8,
            "{case}"
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_infantry_owner(9, 10),
            (!accepted).then_some(owner),
            "{case}"
        );
    }
}

// The legacy global-repair tests below now start at an admitted completed
// Walk head. Actual command traversal is covered above; these cases isolate
// PerCell's publication and live Logic-vector continuation.
fn ready_repair_fixture(
    overlay: Option<u8>,
) -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
    u64,
) {
    use crate::sim::bridge_state::{
        Axis, BridgeCellRole, BridgeRuntimeCell, BridgeheadAnchorClass, DamageState,
    };
    let (mut sim, rules, registry) = fixture();
    sim.mapgen_rng = Simulation::with_seed(0).mapgen_rng;
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    house.player_control = true;
    sim.houses.insert(owner, house);
    let hut = sim
        .spawn_object("CABHUT", "Soviets", 16, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    if let Some(overlay) = overlay {
        for (x, y) in [(17, 14), (17, 15), (17, 16)] {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(x, y)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(overlay);
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .write_bridge_overlay_identity(x, y, overlay);
            sim.bridge_state.as_mut().unwrap().test_seed_cell(
                x,
                y,
                BridgeRuntimeCell {
                    deck_present: true,
                    destroyable: true,
                    deck_level: 0,
                    bridge_group_id: None,
                    damage_state: if overlay == 231 {
                        DamageState::Destroyed
                    } else {
                        DamageState::Healthy { variant: 0 }
                    },
                    axis: Some(Axis::NS),
                    role: BridgeCellRole::Body,
                    anchor_span_id: None,
                    overlay_byte: overlay,
                    bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
                },
            );
        }
    }
    (sim, rules, registry, hut)
}

fn ready_engineer(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
    hut: u64,
) -> u64 {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, rules, &BTreeMap::new())
        .unwrap();
    assert!(crate::sim::movement::issue_direct_move(
        &mut sim.substrate.entities,
        id,
        (16, 15),
        crate::util::fixed_math::SimFixed::from_num(61),
        crate::sim::movement::DestinationTiming::from_rules(sim.session.binary_frame, Some(&rules)),
    ));
    sim.mission_assign_exact(
        id,
        crate::sim::mission::MissionId::from_known(
            crate::rules::mission_data::MissionType::Capture,
        ),
        sim.session.binary_frame,
    )
    .unwrap();
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.navigation.nav_com = Some(NavTargetRef::Building { id: hut });
    e.capture_target = Some(hut);
    let head = DriveCoord::cell(16, 15, 0);
    e.locomotor
        .as_mut()
        .unwrap()
        .set_walk_destination(Some(head));
    sim.run_walk_boundary(id, head, Some(rules), None, Some(registry));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_step_head(Some(head));
    id
}

fn repair_frame(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::map::overlay_types::OverlayTypeRegistry,
) -> crate::sim::world::TickResult {
    sim.advance_tick(&[], Some(rules), &BTreeMap::new(), None, Some(registry), 67)
}

/// One entry per repair announcement: whether it published the type-14 radar
/// request whose client-side accept gates `EVA_BridgeRepaired`.
fn repair_sounds(sim: &Simulation) -> Vec<bool> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::SimSoundEvent::BridgeRepaired { radar, .. } => {
                Some(radar.is_some_and(|request| {
                    request.event_type == crate::sim::radar::RadarEventType::BridgeRepaired
                }))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn engineer_enters_cabhut_repairs_bridge() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    assert!(repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(engineer).is_none());
    assert_eq!(repair_sounds(&sim), [true]);
    for (x, y) in [(17, 14), (17, 15), (17, 16)] {
        let c = sim.bridge_state.as_ref().unwrap().cell(x, y).unwrap();
        assert_eq!(c.overlay_byte, 0xce, "Seed0 native MapGen variant");
        assert_eq!(
            c.damage_state,
            crate::sim::bridge_state::DamageState::Destroyed,
            "native stale damage byte"
        );
        assert!(sim.bridge_state.as_ref().unwrap().is_bridge_walkable(x, y));
    }
}

#[test]
fn engineer_at_intact_cabhut_emits_sound_no_mutation() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(205));
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    assert!(!repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(engineer).is_none());
    assert_eq!(repair_sounds(&sim), [true]);
}

#[test]
fn engineer_far_from_bridge_at_cabhut_no_mutation() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(None);
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    assert!(!repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(engineer).is_none());
    assert_eq!(repair_sounds(&sim), [true]);
}

#[test]
fn bridge_repair_preserves_unrelated_foundation_before_next_reader() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let unrelated = sim
        .spawn_object("CABHUT", "Soviets", 13, 13, 0, &rules, &BTreeMap::new())
        .unwrap();
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let pinned = sim.path_grid_snapshot().unwrap();
    assert!(!pinned.is_walkable(13, 13));
    let gameplay = (sim.scenario_rng.state(), sim.main_rng.state());
    let mut mapgen = sim.mapgen_rng.clone();
    mapgen.next_range_u32_inclusive_scaled(0, 3);
    assert!(
        sim.run_completed_walk_step(
            engineer,
            crate::sim::components::DriveCoord::cell(16, 15, 0),
            Some(&rules),
            None,
            Some(&registry)
        )
        .expect("fixture frame must complete")
    );
    assert!(
        !sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .lifecycle
            .object_alive,
        "Uninit before frame-tail drain"
    );
    assert!(sim.substrate.occupancy.contains_entity(13, 13, unrelated));
    assert!(!sim.path_grid().unwrap().is_walkable(13, 13));
    assert!(!pinned.is_walkable(13, 13));
    assert_eq!(gameplay, (sim.scenario_rng.state(), sim.main_rng.state()));
    assert_eq!(sim.mapgen_rng.state(), mapgen.state());
    for (x, y) in [(17, 14), (17, 15), (17, 16)] {
        assert!(sim.bridge_state.as_ref().unwrap().is_bridge_walkable(x, y));
    }
}

#[test]
fn consecutive_engineers_cancel_the_successors_hut_target_before_its_next_turn() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let a = ready_engineer(&mut sim, &rules, &registry, hut);
    let b = ready_engineer(&mut sim, &rules, &registry, hut);
    repair_frame(&mut sim, &rules, &registry);
    assert!(sim.substrate.entities.get(a).is_none());
    assert!(
        sim.substrate.entities.get(b).is_some(),
        "live vector removal skips its immediate successor"
    );
    let successor = sim.substrate.entities.get(b).unwrap();
    assert_eq!(successor.navigation.nav_com, None);
    assert!(successor.locomotor.as_ref().unwrap().step_head().is_some());
    assert_eq!(repair_sounds(&sim), [true]);
    assert!(sim.radar_terrain_dirty_generation > 0);
    for p in [(17, 14), (17, 15), (17, 16)] {
        assert!(sim.radar_terrain_dirty_cells.contains(&p));
    }
    repair_frame(&mut sim, &rules, &registry);
    let successor = sim.substrate.entities.get(b).unwrap();
    assert!(successor.lifecycle.object_alive);
    assert_eq!(successor.navigation.nav_com, None);
    assert_eq!(successor.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(repair_sounds(&sim), [true]);
}

#[test]
fn nonconsecutive_engineer_finishes_its_head_without_repeating_cancelled_repair() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let a = ready_engineer(&mut sim, &rules, &registry, hut);
    let separator = sim
        .spawn_object("ENGINEER", "Americans", 19, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let b = ready_engineer(&mut sim, &rules, &registry, hut);
    repair_frame(&mut sim, &rules, &registry);
    assert!(sim.substrate.entities.get(a).is_none());
    assert!(sim.substrate.entities.get(separator).is_some());
    let successor = sim.substrate.entities.get(b).unwrap();
    assert!(successor.lifecycle.object_alive);
    assert_eq!(successor.navigation.nav_com, None);
    assert_eq!(successor.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(repair_sounds(&sim), [true]);
}

#[test]
fn later_repair_scatters_stationary_hut_occupant_and_processes_it_synchronously() {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::movement::ground_pose;
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let first = ready_engineer(&mut sim, &rules, &registry, hut);
    let waiting = ready_engineer(&mut sim, &rules, &registry, hut);
    repair_frame(&mut sim, &rules, &registry);
    assert!(sim.substrate.entities.get(first).is_none());
    repair_frame(&mut sim, &rules, &registry);
    let e = sim.substrate.entities.get(waiting).unwrap();
    assert_eq!(e.locomotor.as_ref().unwrap().walk_is_moving(), Some(false));
    assert_eq!(e.navigation.nav_com, None);
    let before = ground_pose::position_world_coord(&e.position);
    let mission = e.mission.current();
    let ai_counter = e.mission.ai_counter();
    let frames = (sim.session.tick, sim.session.binary_frame);
    let rng = sim.scenario_rng.state();
    let last = ready_engineer(&mut sim, &rules, &registry, hut);
    sim.run_completed_walk_step(
        last,
        DriveCoord::cell(16, 15, 0),
        Some(&rules),
        None,
        Some(&registry),
    )
    .expect("hut evacuation finishes within the completion callback");
    let e = sim.substrate.entities.get(waiting).unwrap();
    assert!(matches!(
        e.navigation.nav_com,
        Some(NavTargetRef::Cell { .. })
    ));
    assert!(e.locomotor.as_ref().unwrap().step_head().is_some());
    assert_eq!(
        ground_pose::position_world_coord(&e.position),
        before,
        "the nested Process publishes its fresh head and returns before XYZ motion"
    );
    assert_eq!(
        e.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(true)
    );
    assert_eq!(
        e.mission.current(),
        mission,
        "FNPC success does not queue Move"
    );
    assert_eq!(e.mission.ai_counter(), ai_counter, "no second Object AI");
    assert_eq!((sim.session.tick, sim.session.binary_frame), frames);
    assert_ne!(
        sim.scenario_rng.state(),
        rng,
        "Scatter's direction draw precedes FNPC"
    );
    assert!(
        !sim.substrate
            .entities
            .get(last)
            .unwrap()
            .lifecycle
            .object_alive
    );
}

#[test]
fn hut_repair_scatters_a_jumpjet_occupant_through_its_air_destination_owner() {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    // A rocketeer whose Foot coordinate resolves to the hut cell is a hut
    // occupant for Building 0x4576F0 (Map 0x565730, first Building 0x47C520),
    // exactly like a Walk infantryman standing there.
    let rocketeer = sim
        .spawn_object("JUMPJET", "Americans", 16, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(rocketeer)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .jumpjet_runtime()
            .is_some()
    );
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    let rng = sim.scenario_rng.state();
    sim.run_completed_walk_step(
        engineer,
        DriveCoord::cell(16, 15, 0),
        Some(&rules),
        None,
        Some(&registry),
    )
    .expect("a Jumpjet hut occupant scatters without stopping the frame");
    assert!(
        sim.substrate
            .entities
            .get(engineer)
            .is_none_or(|e| !e.lifecycle.object_alive)
    );
    let e = sim.substrate.entities.get(rocketeer).unwrap();
    // SetDestination(cell,1) reached Jumpjet MoveTo 0x54B1C0: the request is
    // installed as NavCom plus the cached instance destination/moving byte.
    assert!(matches!(
        e.navigation.nav_com,
        Some(NavTargetRef::Cell { .. })
    ));
    let state = e.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
    assert!(state.moving);
    assert_ne!(
        state.destination,
        crate::sim::movement::jumpjet_movement::JumpjetRuntime::NULL
    );
    assert_ne!(
        sim.scenario_rng.state(),
        rng,
        "Scatter's direction draw and the placement sub-cell draw precede the destination"
    );
}

#[test]
fn repair_pointer_expiry_uses_descending_infantry_registry_and_preserves_paid_heads() {
    use crate::sim::components::NavTargetRef;
    let (mut sim, rules, registry, hut) = ready_repair_fixture(None);
    let a = ready_engineer(&mut sim, &rules, &registry, hut);
    let b = ready_engineer(&mut sim, &rules, &registry, hut);
    let c = ready_engineer(&mut sim, &rules, &registry, hut);
    let mut expected = sim.scenario_rng.clone();
    let mut delays = BTreeMap::new();
    let mut locomotors = BTreeMap::new();
    for id in [c, b, a] {
        delays.insert(id, expected.next_range_u32_inclusive(4, 8));
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.attack_target = Some(crate::sim::combat::AttackTarget::new(hut));
        e.passive_scan_timer.arm(sim.session.binary_frame, 30);
        e.navigation.nav_com_aux = Some(NavTargetRef::Cell { rx: 19, ry: 19 });
        e.navigation
            .nav_queue
            .push(NavTargetRef::Building { id: hut });
        e.navigation
            .nav_queue
            .push(NavTargetRef::Cell { rx: 16, ry: 15 });
        e.mark_live_contact_with(hut);
        locomotors.insert(id, serde_json::to_value(&e.locomotor).unwrap());
    }
    // Registry membership survives Limbo and is independent of Logic's list.
    sim.substrate
        .entities
        .get_mut(c)
        .unwrap()
        .lifecycle
        .in_limbo = true;
    // The hut itself is not an Infantry receiver even if it holds the pointer.
    sim.substrate
        .entities
        .get_mut(hut)
        .unwrap()
        .navigation
        .nav_com = Some(NavTargetRef::Building { id: hut });
    sim.expire_infantry_bridge_hut_targets(hut);
    assert_eq!(sim.scenario_rng.state(), expected.state());
    for id in [a, b, c] {
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!(e.passive_scan_timer.duration, delays[&id]);
        assert!(e.attack_target.is_none());
        assert_eq!(e.navigation.nav_com, None);
        assert_eq!(e.navigation.nav_com_aux, None, "native clears the pair");
        assert_eq!(
            e.navigation.nav_queue,
            [NavTargetRef::Cell { rx: 16, ry: 15 }]
        );
        assert_eq!(serde_json::to_value(&e.locomotor).unwrap(), locomotors[&id]);
        assert!(
            e.has_live_contact_with(hut),
            "control0 keeps radio contacts"
        );
    }
    assert_eq!(
        sim.substrate.entities.get(hut).unwrap().navigation.nav_com,
        Some(NavTargetRef::Building { id: hut })
    );
}

#[test]
fn repair_pointer_expiry_keeps_sensor_and_occupier_exceptions_and_current_nav_gate() {
    use crate::sim::components::NavTargetRef;
    for exception in 0..3 {
        let (mut sim, rules, registry, hut) = ready_repair_fixture(None);
        let id = ready_engineer(&mut sim, &rules, &registry, hut);
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        if exception == 0 {
            sim.fog.width = 33;
            sim.fog.height = 33;
            sim.fog.increment_sensor_at(owner, 16, 15);
        }
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.occupier = exception == 1;
        if exception == 2 {
            e.navigation.nav_com = Some(NavTargetRef::Cell { rx: 16, ry: 15 });
        }
        e.navigation.nav_com_aux = Some(NavTargetRef::Building { id: hut });
        let before = e.navigation.clone();
        sim.expire_infantry_bridge_hut_targets(hut);
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            e.navigation.nav_com, before.nav_com,
            "exception {exception}"
        );
        assert_eq!(
            e.navigation.nav_com_aux, before.nav_com_aux,
            "exception {exception}"
        );
    }
}

#[test]
fn engineer_adjacent_to_cabhut_enters_before_repairing_and_dirtying_minimap() {
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command(
        "Americans",
        &Command::CaptureBuilding {
            engineer_id: id,
            target_building_id: hut
        },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new()
    ));
    drop(grid);
    assert!(!repair_frame(&mut sim, &rules, &registry).bridge_state_changed);
    assert!(sim.substrate.entities.get(id).is_some());
    assert!(repair_sounds(&sim).is_empty());
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    let mut changed = false;
    for _ in 0..100 {
        changed |= repair_frame(&mut sim, &rules, &registry).bridge_state_changed;
        if sim.substrate.entities.get(id).is_none() {
            break;
        }
    }
    assert!(changed);
    assert!(sim.substrate.entities.get(id).is_none());
    assert_eq!(repair_sounds(&sim), [true]);
    assert!(sim.radar_terrain_dirty_generation > 0);
    for p in [(17, 14), (17, 15), (17, 16)] {
        assert!(sim.radar_terrain_dirty_cells.contains(&p));
    }
}

/// Native75ADA0/75ACB0 retain the head even after its cell was entered;
///75AEC0 consumes that head before consulting the replacement destination.
#[test]
fn walk_stop_and_retarget_finish_a_same_cell_committed_head() {
    use crate::sim::movement::ground_pose;
    use crate::sim::snapshot::GameSnapshot;
    for stop in [true, false] {
        let (mut sim, rules, registry) = fixture();
        let id = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        let grid = sim.path_grid_snapshot();
        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id: id,
                target_rx: 18,
                target_ry: 15,
                queue: false,
                group_id: None
            },
            Some(&rules),
            grid.as_deref(),
            &BTreeMap::new()
        ));
        drop(grid);
        let mut retained = None;
        for _ in 0..120 {
            repair_frame(&mut sim, &rules, &registry);
            let e = sim.substrate.entities.get(id).unwrap();
            if let Some(head) = e.locomotor.as_ref().unwrap().step_head()
                && (head.x / 256, head.y / 256)
                    == (i32::from(e.position.rx), i32::from(e.position.ry))
                && crate::util::native_x87::distance_3d_leptons(
                    {
                        let c = ground_pose::position_world_coord(&e.position);
                        [c.x, c.y, 0]
                    },
                    [head.x, head.y, 0],
                ) >= 17
            {
                retained = Some(head);
                break;
            }
        }
        let head = retained.expect("enter head cell before subcell completion");
        let before =
            ground_pose::position_world_coord(&sim.substrate.entities.get(id).unwrap().position);
        let grid = sim.path_grid_snapshot();
        let order = if stop {
            Command::Stop { entity_id: id }
        } else {
            Command::Move {
                entity_id: id,
                target_rx: 18,
                target_ry: 16,
                queue: false,
                group_id: None,
            }
        };
        assert!(sim.apply_command(
            "Americans",
            &order,
            Some(&rules),
            grid.as_deref(),
            &BTreeMap::new()
        ));
        drop(grid);
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!(ground_pose::position_world_coord(&e.position), before);
        assert_eq!(e.locomotor.as_ref().unwrap().step_head(), Some(head));
        assert_eq!(e.movement_target.as_ref().unwrap().next_index, 0);
        assert_eq!(e.navigation.nav_com.is_none(), stop);
        // Both instance-owned XYZ values survive the actual snapshot envelope.
        // This synthetic map has no walls; retain its known zero contribution
        // plane before exercising the production map-load restoration owners.
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .retain_zero_wall_plane_for_tests();
        let map_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = GameSnapshot::save(&sim, 0, 0, "walk retained order", 0);
        let mut replay = GameSnapshot::load(&bytes).unwrap().sim;
        replay.restore_after_snapshot_load().unwrap();
        replay.resolve_type_handles(&rules);
        assert!(replay.path_grid.is_none());
        assert!(replay.zone_grid.is_none());
        assert!(replay.terrain_costs.is_empty());
        replay.rebuild_caches_after_load(
            map_terrain,
            sim.terrain_speed_config.clone(),
            sim.bridge_explosions.clone(),
            sim.metallic_debris.clone(),
        );
        replay
            .restore_map_authority_after_snapshot_load(&rules, &registry)
            .unwrap();
        assert!(replay.path_grid.is_some());
        assert!(replay.zone_grid.is_some());
        assert!(!replay.terrain_costs.is_empty());
        // Scenario reload applies native Seed(0); compare both continuations
        // under that established load rule, not uninterrupted RNG preservation.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let mut retired = false;
        for _ in 0..240 {
            repair_frame(&mut sim, &rules, &registry);
            repair_frame(&mut replay, &rules, &registry);
            assert_eq!(
                sim.state_hash(),
                replay.state_hash(),
                "stop={stop}, tick={}, original={:?}, restored={:?}",
                sim.session.tick,
                sim.substrate.entities.get(id),
                replay.substrate.entities.get(id)
            );
            let e = sim.substrate.entities.get(id).unwrap();
            if e.locomotor.as_ref().unwrap().step_head() != Some(head) {
                retired = true;
            }
            if retired && e.movement_target.is_none() {
                break;
            }
        }
        let e = sim.substrate.entities.get(id).unwrap();
        assert!(retired && e.movement_target.is_none());
        assert!(e.locomotor.as_ref().unwrap().step_head().is_none());
        assert!(e.navigation.nav_com.is_none());
        if stop {
            assert_eq!(ground_pose::position_world_coord(&e.position), head);
        } else {
            assert_eq!((e.position.rx, e.position.ry), (18, 16));
        }
    }
}

#[test]
fn walk_completion_uses_retained_destination_and_exact_height_tolerance() {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::timer::CdTimer;
    //75BE6F reads live class fields after PerCell. A changed destination remains
    //authoritative even when the old A* adapter reports its last node complete.
    for (dest, survives) in [
        (
            DriveCoord {
                x: 17 * 256 + 128,
                y: 15 * 256 + 128,
                z: 0,
            },
            true,
        ),
        (
            DriveCoord {
                x: 16 * 256 + 128,
                y: 15 * 256 + 128,
                z: 208,
            },
            true,
        ),
        (
            DriveCoord {
                x: 16 * 256 + 128,
                y: 15 * 256 + 128,
                z: 207,
            },
            false,
        ),
    ] {
        let (mut sim, mut rules, registry) = fixture();
        rules.general.blockage_path_delay_ticks = 65536;
        sim.session.binary_frame = 123;
        let id = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        let head = DriveCoord {
            x: 16 * 256 + 192,
            y: 15 * 256 + 64,
            z: 0,
        };
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.navigation.nav_com = Some(NavTargetRef::cell(
            (dest.x / 256) as u16,
            (dest.y / 256) as u16,
        ));
        e.locomotor
            .as_mut()
            .unwrap()
            .set_walk_destination(Some(dest));
        e.locomotor.as_mut().unwrap().set_step_head(Some(head));
        e.navigation.path_runtime.start_movement(50, 5);
        e.navigation.path_runtime.start_blocked(40, 6);
        e.navigation.path_runtime.path_blocked = true;
        e.navigation.path_runtime.retries_left = 0x8000_0001;
        // Supplied exhausted paid-head adapter, as the object-turn suspension
        // exposes it to the real PerCell completion owner.
        e.movement_target = Some(crate::sim::components::MovementTarget {
            path: vec![(16, 15)],
            path_layers: vec![crate::sim::movement::locomotor::MovementLayer::Ground],
            next_index: 1,
            final_goal: Some(((dest.x / 256) as u16, (dest.y / 256) as u16)),
            ..Default::default()
        });
        sim.run_completed_walk_step(id, head, Some(&rules), None, Some(&registry))
            .expect("fixture frame must complete");
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!(e.navigation.nav_com.is_some(), survives);
        assert!(!e.navigation.path_runtime.path_blocked);
        assert_eq!(e.navigation.path_runtime.retries_left, 0x8000_0001);
        assert_eq!(
            e.navigation.path_runtime.movement_timer,
            if survives {
                CdTimer::from_raw(50, 5)
            } else {
                CdTimer::from_raw(123, 0)
            },
        );
        assert_eq!(
            e.navigation.path_runtime.blocked_timer,
            if survives {
                CdTimer::from_raw(40, 6)
            } else {
                CdTimer::from_raw(123, 65536)
            },
        );
        assert_eq!(
            e.locomotor.as_ref().unwrap().walk_destination().is_some(),
            survives
        );
        if survives {
            let request = e.movement_target.as_ref().unwrap();
            assert!(
                request.path.is_empty(),
                "surviving destination must regain a first-search request"
            );
            assert!(request.path_layers.is_empty());
            assert_eq!(request.next_index, 0);
            assert_eq!(
                request.final_goal,
                Some(((dest.x / 256) as u16, (dest.y / 256) as u16))
            );
            assert_eq!(e.navigation.path_replay.reference_cell, Some((16, 15)));
            assert_eq!(e.locomotor.as_ref().unwrap().walk_destination(), Some(dest));
        }
    }
}

#[test]
fn repair_receiver_failure_stops_runtime_before_consumption_or_frame_commit() {
    use crate::sim::runtime::{SimResources, SimRuntime};
    let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
    let engineer = ready_engineer(&mut sim, &rules, &registry, hut);
    let victim = sim
        .spawn_object("ENGINEER", "Americans", 17, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let later = sim
        .spawn_object("ENGINEER", "Americans", 14, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    // An intentionally unavailable receiver input after the ordinary walker
    // has written its three overlays. This exercises failure delivery, not a
    // claim that missing retail type data is a native admitted state.
    let missing = sim.intern("MISSING_REPAIR_RECEIVER_TYPE");
    sim.substrate.entities.get_mut(victim).unwrap().type_ref = missing;
    let frame = (sim.session.tick, sim.session.binary_frame);
    let later_counter = sim
        .substrate
        .entities
        .get(later)
        .unwrap()
        .mission
        .ai_counter();
    let original_nav = sim
        .substrate
        .entities
        .get(engineer)
        .unwrap()
        .navigation
        .nav_com;
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            ..SimResources::empty()
        },
    };
    let error = runtime
        .advance_frame(&[], 67, crate::sim::world::TickLane::Ordinary)
        .err()
        .expect("missing receiver must fail the actual runtime frame");
    assert_eq!(error.entity_id, engineer);
    assert_eq!((error.tick, error.binary_frame), frame);
    assert!(error.cause.contains("missing ObjectType"), "{error}");
    assert!(error.to_string().contains("prior world mutations remain"));
    let sim = &runtime.simulation;
    assert_eq!((sim.session.tick, sim.session.binary_frame), frame);
    let e = sim.substrate.entities.get(engineer).unwrap();
    assert!(e.lifecycle.object_alive && !e.lifecycle.in_limbo && !e.dying);
    assert_eq!(
        e.navigation.nav_com, original_nav,
        "post-repair expiry was not reached"
    );
    assert_eq!(
        e.locomotor.as_ref().unwrap().step_head(),
        None,
        "pre-PerCell head retirement remains visible"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(later)
            .unwrap()
            .mission
            .ai_counter(),
        later_counter,
        "the live cursor must stop before the following object"
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(17, 15)
            .unwrap()
            .bridge_facts
            .overlay_id,
        Some(206),
        "already-published repair writes are not rolled back"
    );
    assert_eq!(
        repair_sounds(sim),
        [true],
        "pre-repair announcement is not rolled back or drained as a completed output"
    );
}

#[test]
fn hut_queries_pending_uninit_and_active_tube_exit_before_other_gates() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
    use std::collections::BTreeMap;
    let (mut sim, rules, registry) = fixture();
    let hut = sim
        .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let id = sim
        .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/hut_scatter.json"
    ))
    .unwrap();
    let row = &rows[8];
    let head = DriveCoord {
        x: 10368,
        y: 10624,
        z: 0,
    };
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.lifecycle.object_alive = false;
    e.lifecycle.in_limbo = true;
    e.locomotor.as_mut().unwrap().set_step_head(Some(head));
    assert!(
        !sim.scatter_bridge_hut(hut, &rules, Some(&registry))
            .unwrap()
    );
    let dummy = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .shared_cell_dummy()
        .snapshot();
    assert_eq!(
        serde_json::json!([dummy.coord.0, dummy.coord.1]),
        row["output"]["dummy"]
    );
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
        33,
        33,
        terrain.cells().to_vec(),
        vec![TubeFact::explicit((15, 15), (u16::MAX, 2), 0, vec![0])],
    ));
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
        tube_id: TubeId(0),
        cursor: 0,
        target: DriveCoord::cell(7, 8, 999),
    });
    e.locomotor = None;
    let coord = sim.foot_navigation_coordinate(id).unwrap();
    assert_eq!(
        serde_json::json!([coord.x, coord.y, coord.z]),
        rows[11]["output"]["coordinates"][0]
    );
}

/// An unrelated stock-kind Rocketeer still receives +4C before the hut gate.
/// Covers production construction/order and native phase0 activation; full
/// Jumpjet flight/landing and a Rocketeer actually occupying the hut are separate.
#[test]
fn repair_queries_unrelated_rocketeer_after_move_and_snapshot_restore() {
    use crate::sim::{components::DriveCoord, snapshot::GameSnapshot};
    for ordered in [false, true] {
        let (mut sim, rules, registry, hut) = ready_repair_fixture(Some(231));
        let rocketeer = sim
            .spawn_object("JUMPJET", "Americans", 19, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        let current = crate::sim::movement::ground_pose::position_world_coord(
            &sim.substrate.entities.get(rocketeer).unwrap().position,
        );
        assert_eq!(sim.foot_navigation_coordinate(rocketeer).unwrap(), current);
        if ordered {
            // Infantry's accepted Foot setter4D96C2..9707 runs after Jumpjet
            // MoveTo: reset timers/latch while preserving the retry dword.
            sim.substrate
                .entities
                .get_mut(rocketeer)
                .unwrap()
                .navigation
                .path_runtime = crate::sim::components::FootPathRuntime {
                movement_timer: crate::sim::timer::CdTimer::from_raw(-1, 7),
                blocked_timer: crate::sim::timer::CdTimer::from_raw(-1, 31),
                path_blocked: true,
                retries_left: 256,
            };
            let grid = sim.path_grid_snapshot();
            assert!(sim.apply_command_with_overlays(
                "Americans",
                &Command::Move {
                    entity_id: rocketeer,
                    target_rx: 20,
                    target_ry: 15,
                    queue: false,
                    group_id: None
                },
                Some(&rules),
                grid.as_deref(),
                &BTreeMap::new(),
                Some(&registry)
            ));
            drop(grid);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(rocketeer)
                    .unwrap()
                    .navigation
                    .path_runtime,
                crate::sim::components::FootPathRuntime {
                    movement_timer: crate::sim::timer::CdTimer::started(
                        sim.session.binary_frame as i32,
                        0
                    ),
                    blocked_timer: crate::sim::timer::CdTimer::started(
                        sim.session.binary_frame as i32,
                        60
                    ),
                    path_blocked: false,
                    retries_left: 256,
                }
            );
            let state = sim
                .substrate
                .entities
                .get(rocketeer)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .jumpjet_runtime()
                .unwrap();
            assert!(state.moving);
            assert_eq!(state.phase, 0, "MoveTo does not activate phase0");
            assert_ne!(
                state.destination,
                DriveCoord::cell(20, 15, 0),
                "Infantry keeps the selected subcell"
            );
            // A Rocketeer is a Jumpjet, so its locomotor owns the tick now:
            // `Process 0x0054AEC0` dispatches State 0 (`0x0054B980`), which is
            // what promotes a moving owner out of the ground state. The air
            // adapter no longer touches Jumpjets at all.
            sim.tick_air_movement_with_cell_lists_one(rocketeer, None);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(rocketeer)
                    .unwrap()
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .jumpjet_runtime()
                    .unwrap()
                    .phase,
                1
            );
        }
        let state = sim
            .substrate
            .entities
            .get(rocketeer)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .jumpjet_runtime()
            .unwrap()
            .clone();
        let coordinate = sim.foot_navigation_coordinate(rocketeer).unwrap();
        // Map assets are deliberately skipped by the snapshot envelope.
        // Supply the same map-load grid and run the production restore owners
        // before resuming repair; this fixture has no wall contributions.
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .retain_zero_wall_plane_for_tests();
        let map_terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let saved = GameSnapshot::save(&sim, 0, 0, "jumpjet", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        restored.resolve_type_handles(&rules);
        restored.rebuild_caches_after_load(
            map_terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );
        restored
            .restore_map_authority_after_snapshot_load(&rules, &registry)
            .unwrap();
        assert_eq!(
            restored.foot_navigation_coordinate(rocketeer).unwrap(),
            coordinate
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(rocketeer)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .jumpjet_runtime(),
            Some(&state)
        );
        let engineer = ready_engineer(&mut restored, &rules, &registry, hut);
        assert!(
            restored
                .object_type(
                    restored
                        .substrate
                        .entities
                        .get(engineer)
                        .unwrap()
                        .type_ref(),
                    &rules
                )
                .is_some_and(|t| t.engineer)
        );
        assert!(
            restored
                .object_type(
                    restored.substrate.entities.get(hut).unwrap().type_ref(),
                    &rules
                )
                .is_some_and(|t| t.bridge_repair_hut)
        );
        let describe = |world: &Simulation| {
            let e = world.substrate.entities.get(engineer);
            format!(
                "actor={:?}; first_hut={:?}; overlay={:?}; wood={}; sounds={:?}",
                e.map(|e| (
                    crate::sim::movement::ground_pose::position_world_coord(&e.position),
                    e.mission.current(),
                    e.navigation.nav_com,
                    e.locomotor.as_ref().and_then(|l| l.step_head()),
                    e.lifecycle.clone(),
                    e.movement_target.clone()
                )),
                world.substrate.occupancy.first_building_on_layer(
                    16,
                    15,
                    crate::sim::movement::locomotor::MovementLayer::Ground
                ),
                [(17, 14), (17, 15), (17, 16)].map(|(x, y)| world
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .bridge_facts
                    .overlay_id),
                world
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .wood_bridge_set_base(),
                repair_sounds(world)
            )
        };
        let before = describe(&restored);
        let result = repair_frame(&mut restored, &rules, &registry);
        assert!(
            result.bridge_state_changed,
            "ordered={ordered}; before={before}; after={}",
            describe(&restored)
        );
        assert!(restored.substrate.entities.get(engineer).is_none());
        assert!(
            restored
                .substrate
                .entities
                .get(rocketeer)
                .unwrap()
                .is_alive()
        );
        assert_eq!(repair_sounds(&restored), [true]);
    }
}

#[test]
fn jumpjet_query_fields_hash_and_restore_as_one_suspended_instance() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotion::piggyback;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::{components::DriveCoord, snapshot::GameSnapshot};
    let (mut sim, rules, _) = fixture();
    let id = sim
        .spawn_object("JUMPJET", "Americans", 19, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    let initial = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap()
        .destination = DriveCoord {
        x: 5312,
        y: 3904,
        z: 208,
    };
    let coordinate_hash = sim.state_hash();
    assert_ne!(initial, coordinate_hash);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap()
        .moving = true;
    let moving_hash = sim.state_hash();
    assert_ne!(coordinate_hash, moving_hash);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .jumpjet_runtime_mut()
        .unwrap()
        .phase = 3;
    assert_ne!(moving_hash, sim.state_hash());
    let retained = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap()
        .clone();
    let loco = sim
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap();
    assert_eq!(
        piggyback::begin(loco, LocomotorKind::Walk, MovementLayer::Ground, 0),
        piggyback::BeginOutcome::Installed
    );
    let saved = GameSnapshot::save(&sim, 0, 0, "jumpjet stash", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    let before = restored.state_hash();
    // The suspended payload is hashed too; active Walk is unchanged.
    let loco = restored
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap();
    let original = serde_json::to_string(&loco.piggyback).unwrap();
    let mut changed = original.clone();
    assert!(changed.contains("5312"));
    changed = changed.replacen("5312", "5313", 1);
    loco.piggyback = serde_json::from_str(&changed).unwrap();
    assert_ne!(before, restored.state_hash());
    let loco = restored
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap();
    loco.piggyback = serde_json::from_str(&original).unwrap();
    assert!(piggyback::end(loco).is_some());
    assert_eq!(loco.jumpjet_runtime(), Some(&retained));
}

#[test]
fn jumpjet_stop_command_keeps_native_moving_and_selected_coordinate() {
    use crate::util::fixed_math::SimFixed;
    let (mut sim, rules, registry, _) = ready_repair_fixture(None);
    let id = sim
        .spawn_object("JUMPJET", "Americans", 19, 15, 0, &rules, &BTreeMap::new())
        .unwrap();
    assert!(sim.issue_air_cell_destination(id, (20, 15), SimFixed::from_num(9), Some(&rules)));
    crate::sim::movement::air_movement::tick_air_movement(
        &mut sim.substrate.entities,
        &[id],
        sim.session.tick,
        sim.session.binary_frame,
        sim.resolved_terrain.as_ref(),
        Some((&rules, &sim.interner)),
    );
    let before = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap()
        .clone();
    let grid = sim.path_grid_snapshot();
    assert!(sim.apply_command_with_overlays(
        "Americans",
        &Command::Stop { entity_id: id },
        Some(&rules),
        grid.as_deref(),
        &BTreeMap::new(),
        Some(&registry)
    ));
    let state = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap();
    assert!(
        state.moving,
        "Stop is a new selected destination, not a null MoveTo"
    );
    assert_eq!(state.phase, before.phase);
    assert_ne!(state.destination, before.destination);
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .movement_target
            .is_some()
    );
}

#[test]
fn failed_jumpjet_stop_stock_fatal_receiver_precedes_cache_retirement() {
    use crate::sim::{components::DriveCoord, movement::jumpjet_movement::JumpjetRuntime};
    use serde_json::json;
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/jumpjet_stop_damage.json"
    ))
    .unwrap();
    assert_eq!(
        rows[0]["output"], rows[1]["output"],
        "original alias/copied fatal core controls"
    );
    for row in [&rows[0], &rows[2], &rows[3], &rows[4]] {
        let (mut sim, mut rules, registry) = fixture();
        rules.bridge_warheads.c4_name = "Super".into();
        let id = sim
            .spawn_object("JUMPJET", "Americans", 19, 15, 0, &rules, &BTreeMap::new())
            .unwrap();
        // Supplied failed-search terrain exercises the real FNPC receiver;
        // it is not a claim that every stock map can strand a Rocketeer.
        for y in 0..33 {
            for x in 0..33 {
                sim.resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(x, y)
                    .unwrap()
                    .speed_costs
                    .hover = Some(0);
            }
        }
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.health.current = i32::try_from(row["input"]["health"].as_i64().unwrap()).unwrap();
        *e.locomotor.as_mut().unwrap().jumpjet_runtime_mut().unwrap() = JumpjetRuntime {
            destination: DriveCoord {
                x: 2752,
                y: 2752,
                z: 208,
            },
            moving: true,
            phase: 1,
            ..Default::default()
        };
        assert!(sim.stop_jumpjet_infantry_destination(id, Some(&rules), Some(&registry)));
        let e = sim.substrate.entities.get(id).unwrap();
        let state = e.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
        assert_eq!(
            i64::from(e.health.current),
            row["output"]["health"].as_i64().unwrap()
        );
        assert_eq!(
            json!([
                state.destination.x,
                state.destination.y,
                state.destination.z
            ]),
            row["output"]["state"]["destination"]
        );
        assert_eq!(json!(state.moving), row["output"]["state"]["moving"]);
        assert_eq!(json!(state.phase), row["output"]["state"]["phase"]);
        if row["input"]["health"] == 100 {
            assert!(
                e.dying || !e.is_alive(),
                "fatal receiver/lifecycle completed before Stop returns"
            );
            assert_eq!(row["output"]["damage_trace"][1]["health"], 0);
            assert_ne!(
                row["output"]["damage_trace"][1]["destination"],
                json!([0, 0, 0]),
                "native HP0 precedes callback and cache clear"
            );
        } else {
            assert!(!e.dying, "nonpositive-health Stop never dispatches damage");
            assert!(row["output"]["damage_trace"].as_array().unwrap().is_empty());
        }
    }
}
