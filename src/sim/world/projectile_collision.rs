//! BulletClass AI pre-commit ordinary admission and post-commit shared probe.
//! Native owners: 4666E0 (467494..467B7A), 468BB0, 4CC360. World state is
//! read at each native receiver boundary; collision does not own a second map.

use super::*;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback, get_cellclass_fallback_leptons};
use crate::sim::projectile::{
    ProjectileCollisionMotion, ProjectileCollisionPhase, ProjectileTarget, ProjectileTrajectory,
    ProjectileVelocity, coord_distance, projectile_ground_z,
};

pub(super) struct ProjectileCollisionWorld<'a> {
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    pub dummy: &'a SharedCellDummy,
    pub occupancy: &'a OccupancyGrid,
    pub entities: &'a EntityStore,
    pub interner: &'a crate::sim::intern::StringInterner,
    pub alliances: &'a HouseAllianceMap,
    pub overlays: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    pub overlay_registry: Option<&'a crate::map::overlay_types::OverlayTypeRegistry>,
    pub rules: Option<&'a RuleSet>,
    pub map_size: Option<(i32, i32)>,
}

#[cfg(test)]
mod tests {
    use super::super::lifecycle_tests::{
        gsi_05_02_projectile, insert_entity, install_common_raw_terrain,
    };
    use super::*;
    use crate::sim::occupancy::CellListInsertion;
    use crate::util::fixed_math::SimFixed;
    use serde_json::Value;

    fn world(sim: &Simulation) -> ProjectileCollisionWorld<'_> {
        ProjectileCollisionWorld {
            terrain: sim.resolved_terrain.as_ref(),
            dummy: &sim.shared_cell_dummy,
            occupancy: &sim.substrate.occupancy,
            entities: &sim.substrate.entities,
            interner: &sim.interner,
            alliances: &sim.house_alliances,
            overlays: sim.overlay_grid.as_ref(),
            overlay_registry: None,
            rules: None,
            map_size: Some((2, 3)),
        }
    }

    fn coord(row: &Value, key: &str, default: [i32; 3]) -> ProjectileCoord {
        let values = row
            .get(key)
            .map(|array| std::array::from_fn(|i| array[i].as_i64().unwrap() as i32))
            .unwrap_or(default);
        ProjectileCoord::new(values[0], values[1], values[2])
    }

    fn place(sim: &mut Simulation, id: u64, position: ProjectileCoord, category: EntityCategory) {
        insert_entity(sim, id, category);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.rx = (position.x / 256) as u16;
        entity.position.ry = (position.y / 256) as u16;
        entity.position.sub_x = SimFixed::from_num(position.x % 256);
        entity.position.sub_y = SimFixed::from_num(position.y % 256);
        entity.position.exact_z_leptons = Some(position.z);
        entity.occupancy_enter_order = id;
    }

    #[test]
    fn ordinary_tail_matches_executed_native_admissions() {
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/ordinary_collision_vectors.json"
        ))
        .unwrap();
        for (index, row) in vectors["admissions"].as_array().unwrap().iter().enumerate() {
            let mut sim = Simulation::new();
            install_common_raw_terrain(&mut sim, 8, 8, 0, None);
            let height = row["old_height"].as_i64().unwrap_or(500) as i32;
            let candidate = coord(row, "candidate", [384, 128, 0]);
            let old = coord(row, "old", [128, 128, height]);
            let target = coord(row, "target", [640, 128, 0]);
            let selected = row["selected"].as_str().unwrap_or("none");
            let source_present = row["source"].as_bool().unwrap_or(false);
            if source_present || selected == "source" {
                place(
                    &mut sim,
                    1,
                    ProjectileCoord::new(0, 0, 0),
                    EntityCategory::Unit,
                );
            }
            if selected == "object" {
                place(
                    &mut sim,
                    2,
                    coord(row, "object_coord", [384, 128, 0]),
                    EntityCategory::Unit,
                );
                let owner = sim.interner.intern("Russians");
                sim.substrate.entities.get_mut(2).unwrap().owner = owner;
            }
            if selected != "none" {
                sim.substrate.occupancy.add(
                    (candidate.x / 256) as u16,
                    (candidate.y / 256) as u16,
                    if selected == "source" { 1 } else { 2 },
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
            }
            if row["same_building"].as_bool().unwrap_or(false) {
                place(
                    &mut sim,
                    3,
                    ProjectileCoord::new(384, 128, 2000),
                    EntityCategory::Structure,
                );
                for position in [candidate, target] {
                    sim.substrate.occupancy.add(
                        (position.x / 256) as u16,
                        (position.y / 256) as u16,
                        3,
                        MovementLayer::Ground,
                        None,
                        CellListInsertion::AppendBuilding,
                    );
                }
            }
            if row["allied"].as_bool().unwrap_or(false) {
                sim.house_alliances
                    .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
            }
            let mut spawn = gsi_05_02_projectile(if source_present { 1 } else { 999 }, None);
            spawn.origin = old;
            spawn.initial_target_position = target;
            spawn.collision.inaccurate = row["inaccurate"].as_bool().unwrap_or(false);
            let velocity = coord(row, "velocity", [20, 0, -6]);
            spawn.velocity = ProjectileVelocity::new(velocity.x, velocity.y, velocity.z);
            if row["vertical"].as_bool().unwrap_or(false) {
                spawn.trajectory = ProjectileTrajectory::Vertical {
                    detonation_altitude: 1000,
                    acceleration: 1,
                    max_speed: 20,
                };
            }
            sim.projectiles.spawn(100, spawn);
            let projectile = sim.projectiles.get(100).unwrap();
            let response = world(&sim).ordinary_tail(
                projectile,
                candidate,
                projectile.velocity,
                projectile.velocity,
                ProjectileCollisionMotion::from_coordinate(candidate, projectile.velocity).velocity,
                false,
            );
            let ProjectileCollisionResponse::Ordinary {
                candidate: result,
                impact,
                near_target,
                left_map,
                ..
            } = response
            else {
                unreachable!()
            };
            assert_eq!(
                (impact, near_target, left_map),
                (
                    row["impact"].as_bool().unwrap(),
                    row["near_target"].as_bool().unwrap(),
                    row["reason"].as_i64().unwrap() == 2
                ),
                "native row {index}: {row}"
            );
            assert_eq!(
                result,
                coord(row, "result", [0; 3]),
                "native row {index}: {row}"
            );
        }
    }

    #[test]
    fn shared_probe_matches_original_bodies_and_receivers() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::movement::rocket_movement::{RocketPhase, attach_rocket_state};
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/shared_collision_vectors.json"
        ))
        .unwrap();
        for (index, row) in vectors["cases"].as_array().unwrap().iter().enumerate() {
            let mut sim = Simulation::new();
            let flag = |key: &str| row[key].as_bool().unwrap_or(false);
            let mut cells = Vec::new();
            let mut allocated = Vec::new();
            let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
            let wall_owner = sim.interner.intern("Russians");
            for y in 0..8 {
                for x in 0..8 {
                    if !row["missing"]
                        .as_array()
                        .is_some_and(|missing| missing.iter().any(|xy| xy[0] == x && xy[1] == y))
                    {
                        allocated.push((x as u16, y as u16));
                    }
                    let input = &row["cells"][format!("{x},{y}")];
                    let mut cell = super::super::lifecycle_tests::common_raw_terrain_cell(
                        x as u16,
                        y as u16,
                        input["level"].as_i64().unwrap_or(0) as u8,
                        false,
                    );
                    cell.final_tile_index = input["tile"].as_i64().unwrap_or(65535) as i32;
                    cell.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
                    cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap_or(0) as u32;
                    if input["wall"].as_bool().unwrap_or(false) {
                        overlays.place_overlay(x as u16, y as u16, 0, 0);
                        overlays.set_wall_owner(x as u16, y as u16, wall_owner);
                    }
                    cells.push(cell);
                }
            }
            let mut terrain = ResolvedTerrainGrid::from_cells(8, 8, cells);
            terrain.test_set_native_allocated_cells(&allocated);
            terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
            terrain.set_projectile_water_set_base(row["water_base"].as_i64().unwrap_or(100) as i32);
            sim.resolved_terrain = Some(terrain);
            sim.overlay_grid = Some(overlays);
            if row["source"].as_bool().unwrap_or(true) {
                place(
                    &mut sim,
                    1,
                    ProjectileCoord::new(128, 128, 0),
                    EntityCategory::Unit,
                );
            }
            if flag("source_allied") {
                sim.house_alliances
                    .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
            }
            if flag("wall_allied") {
                sim.house_alliances
                    .insert("RUSSIANS".into(), ["AMERICANS".into()].into());
            }
            let dimensions = row["foundation_dimensions"]
                .as_array()
                .map(|dims| format!("{}x{}", dims[0], dims[1]))
                .unwrap_or_else(|| "1x1".into());
            let ini = IniFile::from_str(&format!(
                "[WallModel]\nAlliedWallTransparency={}\n[General]\nV3RocketType=V3ROCKET\nDMislType=DMISL\n[VehicleTypes]\n0=TEST\n[AircraftTypes]\n0=V3ROCKET\n1=DMISL\n2=AIR\n[BuildingTypes]\n0=BUILD\n[BUILD]\nFoundation={dimensions}\n[OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\n",
                flag("transparency")
            ));
            let rules = RuleSet::from_ini(&ini).unwrap();
            let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
            let target = &row["target"];
            let mut spawn = gsi_05_02_projectile(1, None);
            if target.is_object() {
                let category = match target["category"].as_str().unwrap_or("unit") {
                    "building" => EntityCategory::Structure,
                    "aircraft" => EntityCategory::Aircraft,
                    _ => EntityCategory::Unit,
                };
                place(
                    &mut sim,
                    2,
                    coord(target, "coord", [640, 640, 500]),
                    category,
                );
                let name = if category == EntityCategory::Structure {
                    "BUILD"
                } else if target["rocket"].as_bool().unwrap_or(false) {
                    if target["dmisl"].as_bool().unwrap_or(false) {
                        "DMISL"
                    } else {
                        "V3ROCKET"
                    }
                } else {
                    "TEST"
                };
                let type_ref = sim.interner.intern(name);
                let entity = sim.substrate.entities.get_mut(2).unwrap();
                entity.type_ref = type_ref;
                entity.lifecycle.cell_marked = target["marked"].as_bool().unwrap_or(true);
                entity.on_bridge = target["on_bridge"].as_bool().unwrap_or(false);
                if target["rocket"].as_bool().unwrap_or(false)
                    && target["phase"].as_u64().unwrap() != 0
                {
                    attach_rocket_state(
                        &mut sim.substrate.entities,
                        2,
                        (2, 2),
                        (3, 2),
                        SimFixed::from_num(1),
                    );
                    sim.substrate
                        .entities
                        .get_mut(2)
                        .unwrap()
                        .rocket_state
                        .as_mut()
                        .unwrap()
                        .phase = match target["phase"].as_u64().unwrap() {
                        1 => RocketPhase::Ignition,
                        2 => RocketPhase::Tilt,
                        3 => RocketPhase::Ascent,
                        4 => RocketPhase::Cruise,
                        5 => RocketPhase::Terminal,
                        _ => RocketPhase::Secondary,
                    };
                }
                spawn.target = ProjectileTarget::Entity(2);
            } else {
                spawn.target = ProjectileTarget::None;
            }
            let candidate = coord(row, "candidate", [640, 640, 500]);
            spawn.origin = coord(row, "origin", [128, 128, 0]);
            spawn.initial_target_position = coord(row, "launch_target", [1408, 640, 0]);
            spawn.collision.subject_to_cliffs = flag("cliffs");
            spawn.collision.subject_to_walls = flag("walls");
            spawn.collision.flak_scatter = flag("flak");
            spawn.collision.anti_air = flag("aa");
            spawn.collision.level_non_water = flag("level");
            sim.projectiles.spawn(100, spawn);
            let mut projectile = sim.projectiles.get(100).unwrap().clone();
            projectile.position = candidate;
            projectile.previous_cell = row["previous"]
                .as_array()
                .map(|xy| {
                    (
                        xy[0].as_i64().unwrap() as i16,
                        xy[1].as_i64().unwrap() as i16,
                    )
                })
                .unwrap_or((1, 2));
            let mut world = world(&sim);
            world.rules = Some(&rules);
            world.overlay_registry = Some(&registry);
            assert_eq!(
                world.shared(&projectile, candidate),
                row["admitted"].as_bool().unwrap(),
                "original shared row {index}: {row}"
            );
            assert_eq!(
                sim.shared_cell_dummy.snapshot().coord,
                (
                    row["dummy_coord"][0].as_i64().unwrap() as i32,
                    row["dummy_coord"][1].as_i64().unwrap() as i32
                ),
                "dummy query order row {index}: {row}"
            );
        }
    }

    #[test]
    fn nearest_selector_matches_original_vtable_getters_and_list_ties() {
        use crate::rules::ini_parser::IniFile;
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/ordinary_collision_vectors.json"
        ))
        .unwrap();
        for row in vectors["nearest"].as_array().unwrap() {
            let mut sim = Simulation::new();
            install_common_raw_terrain(&mut sim, 8, 8, 0, None);
            let mut foundation = "1x1".to_string();
            for (index, object) in row["objects"].as_array().unwrap().iter().enumerate().rev() {
                // GameEntity is Techno (+14 bit0); Terrain has only bit1.
                if object["eligible"] == false {
                    continue;
                }
                let id = index as u64 + 1;
                let position = coord(object, "coord", [0; 3]);
                if object["terrain"] == true {
                    // Native original selector excludes this bit2-only node.
                    continue;
                } else {
                    let category = if object["building"] == true {
                        EntityCategory::Structure
                    } else {
                        EntityCategory::Unit
                    };
                    place(&mut sim, id, position, category);
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .occupancy_enter_order = 100 - id;
                    sim.substrate.occupancy.add(
                        1,
                        0,
                        id,
                        MovementLayer::Ground,
                        None,
                        CellListInsertion::from_category(category),
                    );
                    if let Some(dims) = object["dimensions"].as_array() {
                        foundation = format!("{}x{}", dims[0], dims[1]);
                    }
                }
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[BuildingTypes]\n0=TEST\n[TEST]\nFoundation={foundation}\n"
            )))
            .unwrap();
            let mut world = world(&sim);
            world.rules = Some(&rules);
            let selected = world
                .nearest(&world.cell(ProjectileCoord::new(384, 128, 0)))
                .map(|(id, _)| id - 1);
            // New Walk priority shares the original ordered scalar, with
            // +48 coordinates. Reuse these executed47C3D0 vectors; no second
            // hand-authored nearest golden is needed for the repair query.
            let shared = crate::sim::cell_kernel::nearest_eligible_in_order(
                crate::sim::cell_kernel::CellQueryPoint { x: 0, y: 0 },
                row["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, _)| {
                        let e = sim.substrate.entities.get(index as u64 + 1)?;
                        let c = crate::sim::movement::ground_pose::object_center_coord(
                            e,
                            rules.object("TEST").unwrap(),
                        );
                        Some((
                            index as u64,
                            true,
                            crate::sim::cell_kernel::CellQueryPoint { x: c.x, y: c.y },
                        ))
                    }),
            );
            assert_eq!(
                shared,
                row["selected"].as_u64(),
                "shared original selector: {row}"
            );
            assert_eq!(
                selected,
                row["selected"].as_u64(),
                "original selector: {row}"
            );
        }
    }

    fn install_native_size_terrain(sim: &mut Simulation, width: u16, height: u16) {
        let header = crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: u32::from(width),
            height: u32::from(height),
            local_left: 0,
            local_top: 0,
            local_width: u32::from(width),
            local_height: u32::from(height),
        };
        sim.install_playfield_from_map_header(&header);
        // Match the native Size allocation diamond, not an allocated
        // rectangular fixture: 565C10 and 568350 share these predicates.
        let extent = width.saturating_add(height).max(24);
        let cells = (0u16..extent)
            .flat_map(|y| {
                (0u16..extent).map(move |x| {
                    super::super::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                })
            })
            .collect();
        let allocated = (0i32..i32::from(extent))
            .flat_map(|y| {
                (0i32..i32::from(extent)).filter_map(move |x| {
                    (i32::from(width) < x + y
                        && x - y < i32::from(width)
                        && y - x < i32::from(width)
                        && x + y <= i32::from(width) + 2 * i32::from(height))
                    .then_some((x as u16, y as u16))
                })
            })
            .collect::<Vec<_>>();
        let mut terrain = ResolvedTerrainGrid::from_cells(extent, extent, cells);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
        sim.resolved_terrain = Some(terrain);
    }

    #[test]
    fn ordinary_and_shared_admission_deliver_damage_in_runtime_logic_slot() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::runtime::SimRuntime;
        let ini = IniFile::from_str(
            "[General]\nVeteranRatio=3.0\n[AudioVisual]\nGravity=6\n[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n[Warheads]\n0=WALLWH\n[WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=1\n",
        );
        let art = IniFile::from_str("[WALL]\nDamageLevels=4\n");
        for (source_present, shared_wall) in [(false, false), (true, false), (true, true)] {
            let mut sim = Simulation::new();
            install_native_size_terrain(&mut sim, 8, 8);
            let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(24, 24);
            overlays.place_overlay(5, 5, 0, 0);
            sim.overlay_grid = Some(overlays);
            if source_present {
                place(
                    &mut sim,
                    1,
                    ProjectileCoord::new(5 * 256 + 128, 4 * 256 + 128, 0),
                    EntityCategory::Unit,
                );
            }
            let mut shot = gsi_05_02_projectile(if source_present { 1 } else { 999 }, None);
            shot.target = ProjectileTarget::None;
            shot.origin = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 1);
            shot.initial_target_position = ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0);
            shot.velocity = ProjectileVelocity::new(16, 0, 0);
            shot.trajectory = ProjectileTrajectory::Ballistic;
            shot.collision.subject_to_walls = shared_wall;
            shot.arm_frames = 5;
            shot.payload = crate::sim::projectile::ProjectilePayload {
                base_damage: 1,
                warhead: sim.interner.intern("WALLWH"),
                weapon: sim.interner.intern("MISSING"),
                owner: sim.interner.intern("Americans"),
            };
            sim.admit_projectile(100, shot);
            let mut runtime = SimRuntime::from_simulation(sim);
            runtime.resources.rules = RuleSet::from_ini(&ini).unwrap();
            runtime.resources.overlay_registry =
                crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
            let _ = runtime
                .advance_frame(&[], 16, crate::sim::world::TickLane::Ordinary)
                .expect("fixture frame must complete");
            let detonate = !source_present || shared_wall;
            assert_eq!(
                runtime.simulation.projectiles.get(100).is_none(),
                detonate,
                "source={source_present}, shared={shared_wall}"
            );
            assert_eq!(
                runtime
                    .simulation
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(5, 5)
                    .overlay_data,
                if detonate { 0x10 } else { 0 }
            );
            assert!(
                runtime.simulation.pending_projectile_detonations.is_empty(),
                "damage commits in the normal runtime Logic slot"
            );
            if !detonate {
                assert_eq!(
                    runtime.simulation.projectiles.get(100).unwrap().position.z,
                    -5,
                    "467BEE skips the common clamp when no impact was admitted"
                );
            }
        }
    }

    #[test]
    fn projectile_load_timer_opens_proximity_in_real_runtime_logic() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::snapshot::GameSnapshot;
        let ini = IniFile::from_str(
            "[General]\nVeteranRatio=3.0\n[AudioVisual]\nGravity=6\n[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n[Warheads]\n0=WALLWH\n[WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=1\n",
        );
        let art = IniFile::from_str("[WALL]\nDamageLevels=4\n");
        let make_rules = || RuleSet::from_ini(&ini).unwrap();
        for dropping in [false, true] {
            let mut sim = Simulation::new();
            install_native_size_terrain(&mut sim, 16, 16);
            let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
            sim.install_resolved_terrain_for_new_map(terrain.clone());
            let mut overlays = crate::sim::overlay_grid::OverlayGrid::new_with_retained_wall_plane(
                terrain.width(),
                terrain.height(),
            );
            overlays.place_overlay(12, 12, 0, 0);
            sim.overlay_grid = Some(overlays);
            let source = sim.allocate_stable_id();
            place(
                &mut sim,
                source,
                ProjectileCoord::new(2944, 3200, 0),
                EntityCategory::Unit,
            );
            assert!(matches!(sim.reveal(source), RevealOutcome::Revealed { .. }));
            sim.session.binary_frame = 100;
            sim.session.map_name = "TIMER.MAP".into();
            let mut shot = gsi_05_02_projectile(source, None);
            shot.target = ProjectileTarget::Cell { rx: 12, ry: 12 };
            let origin_z = if dropping { 100 } else { 20 };
            shot.origin = ProjectileCoord::new(3200, 3200, origin_z);
            shot.collision.dropping = dropping;
            shot.initial_target_position = ProjectileCoord::new(3200, 3200, 0);
            // Native Vertical skips the ordinary same-cell/old-height impact,
            // isolating the proximity timer without disabling world admission.
            shot.velocity = ProjectileVelocity::new(0, 0, -3);
            shot.trajectory = ProjectileTrajectory::Vertical {
                detonation_altitude: 2000,
                acceleration: 0,
                max_speed: 3,
            };
            shot.arm_frames = 10;
            shot.ranged_fuse = true;
            shot.payload.warhead = sim.interner.intern("WALLWH");
            shot.payload.base_damage = 1;
            let id = sim.allocate_stable_id();
            sim.admit_projectile(id, shot);
            let mut runtime = SimRuntime::from_simulation(sim);
            runtime.resources.rules = make_rules();
            runtime.resources.overlay_registry =
                crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
            let _ = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            let saved = runtime.simulation.projectiles.get(id).unwrap().clone();
            assert_eq!(saved.arm_timer.start_frame(), 100);
            assert_eq!(
                saved
                    .arm_timer
                    .remaining(runtime.simulation.session.binary_frame as i32),
                9
            );
            assert_eq!(
                saved.position,
                ProjectileCoord::new(3200, 3200, origin_z - 3)
            );
            let bytes = GameSnapshot::save(&runtime.simulation, 17, 18, "TIMER.MAP", 0);
            let mut restored = GameSnapshot::load_validated(&bytes, 17, 18, "TIMER.MAP")
                .unwrap()
                .sim;
            let mut serialized_expected = saved.clone();
            serialized_expected.in_logic_vector = false;
            assert_eq!(restored.projectiles.get(id), Some(&serialized_expected));
            restored.restore_after_snapshot_load().unwrap();
            let mut expected = saved.clone();
            expected
                .arm_timer
                .start(restored.session.binary_frame as i32, 0);
            assert_eq!(restored.projectiles.get(id), Some(&expected));
            let hash = restored.state_hash();
            restored
                .projectiles
                .get_mut(id)
                .unwrap()
                .arm_timer
                .start(restored.session.binary_frame as i32, 1);
            assert_ne!(
                restored.state_hash(),
                hash,
                "proximity duration is hash authority"
            );
            restored.projectiles.get_mut(id).unwrap().arm_timer = expected.arm_timer;
            restored.projectiles.get_mut(id).unwrap().collision.dropping = !dropping;
            assert_ne!(
                restored.state_hash(),
                hash,
                "Dropping admission policy is hash authority"
            );
            restored.projectiles.get_mut(id).unwrap().collision.dropping = dropping;
            assert_eq!(restored.state_hash(), hash);
            assert!(restored.projectiles.get(id).unwrap().in_logic_vector);
            restored.rebuild_caches_after_load(
                terrain,
                crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
                Vec::new(),
                Vec::new(),
            );
            restored
                .restore_map_authority_after_snapshot_load(
                    &make_rules(),
                    &runtime.resources.overlay_registry,
                )
                .unwrap();
            let mut resumed = SimRuntime::from_simulation(restored);
            resumed.resources.rules = make_rules();
            resumed.resources.overlay_registry =
                crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
            let _ = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            let _ = resumed
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            assert!(
                runtime.simulation.projectiles.get(id).is_some(),
                "unloaded positive Arm still suppresses the near fuse"
            );
            if dropping {
                let shot = resumed.simulation.projectiles.get(id).unwrap();
                assert_eq!(
                    shot.last_distance_half, 47,
                    "Dropping commits Check's native 94/2 watermark after load"
                );
                assert_eq!(
                    runtime
                        .simulation
                        .projectiles
                        .get(id)
                        .unwrap()
                        .last_distance_half,
                    100
                );
                assert_eq!(
                    resumed
                        .simulation
                        .overlay_grid
                        .as_ref()
                        .unwrap()
                        .cell(12, 12)
                        .overlay_data,
                    0
                );
            } else {
                assert!(
                    resumed.simulation.projectiles.get(id).is_none(),
                    "load-opened fuse retires the projectile in its actual Logic slot"
                );
                assert_eq!(
                    runtime
                        .simulation
                        .overlay_grid
                        .as_ref()
                        .unwrap()
                        .cell(12, 12)
                        .overlay_data,
                    0
                );
                assert_eq!(
                    resumed
                        .simulation
                        .overlay_grid
                        .as_ref()
                        .unwrap()
                        .cell(12, 12)
                        .overlay_data,
                    0x10,
                    "native final cell coordinate reaches synchronous wall damage"
                );
            }
            assert!(resumed.simulation.pending_projectile_detonations.is_empty());
        }
    }

    #[test]
    fn projectile_arm_producer_uses_aircraft_identity_in_runtime_fireat() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::combat::AttackTarget;
        use crate::sim::runtime::SimRuntime;
        for arm in [-1, i32::MIN, i32::MAX, 9_999_999, 10] {
            for aircraft in [false, true] {
                let ini = IniFile::from_str(&
                "[General]\nVeteranRatio=3.0\n[VehicleTypes]\n0=TEST\n1=VICTIM\n[AircraftTypes]\n0=AIR\n[TEST]\nStrength=300\nPrimary=GUN\n[VICTIM]\nStrength=300\n[AIR]\nStrength=300\n[GUN]\nDamage=10\nROF=100\nRange=10\nSpeed=100\nProjectile=SHOT\nWarhead=WH\n[SHOT]\nArm=10\nRanged=yes\nAA=yes\nAG=yes\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n".replace("Arm=10", &format!("Arm={arm}")));
                let mut sim = Simulation::new();
                install_native_size_terrain(&mut sim, 16, 16);
                let source = sim.allocate_stable_id();
                let target = sim.allocate_stable_id();
                place(
                    &mut sim,
                    source,
                    ProjectileCoord::new(3200, 3200, 100),
                    EntityCategory::Unit,
                );
                place(
                    &mut sim,
                    target,
                    ProjectileCoord::new(3700, 3200, 100),
                    if aircraft {
                        EntityCategory::Aircraft
                    } else {
                        EntityCategory::Unit
                    },
                );
                let type_ref = sim.interner.intern(if aircraft { "AIR" } else { "VICTIM" });
                let owner = sim.interner.intern("Russians");
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim.type_ref = type_ref;
                victim.owner = owner;
                if aircraft {
                    // Aircraft object-list membership requires its real Fly
                    // host; a category-only fixture has no valid receiver.
                    let mut locomotor =
                        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                            crate::rules::locomotor_type::LocomotorKind::Fly,
                        );
                    locomotor.altitude = SimFixed::from_num(100);

                    victim.locomotor = Some(locomotor);
                }
                assert!(matches!(sim.reveal(source), RevealOutcome::Revealed { .. }));
                assert!(matches!(sim.reveal(target), RevealOutcome::Revealed { .. }));
                for id in [source, target] {
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .position
                        .exact_z_leptons = Some(100);
                }
                let firer = sim.substrate.entities.get_mut(source).unwrap();
                firer.facing = 64;
                firer.attack_target = Some(AttackTarget::new(target));
                sim.session.binary_frame = 100;
                let mut runtime = SimRuntime::from_simulation(sim);
                runtime.resources.rules = RuleSet::from_ini(&ini).unwrap();
                let output = runtime
                    .advance_frame(&[], 16, TickLane::Ordinary)
                    .expect("fixture frame must complete");
                assert_eq!(
                    runtime.simulation.projectiles.len(),
                    1,
                    "aircraft={aircraft}, {output:?}"
                );
                let (_, shot) = runtime.simulation.projectiles.iter().next().unwrap();
                assert_eq!(
                    shot.arm_timer.start_frame(),
                    100, // Logic ran at frame100; advance_frame increments the clock afterward.
                );
                assert_eq!(shot.arm_timer.duration(), if aircraft { 0 } else { arm });
            }
        }
    }

    #[test]
    #[ignore = "requires RA2_DIR with verified gamemd.exe math tables"]
    fn runtime_fireat_fractional_velocity_survives_live_gravity_and_snapshot() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::combat::AttackTarget;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::snapshot::GameSnapshot;
        let tables = crate::map::retail_trig::required_math_tables();
        assert!(tables.0.matches_retail() && tables.1.matches_retail());
        for voxel in [false, true] {
            let ini = IniFile::from_str(&format!(
                "[General]\nVeteranRatio=3.0\n[AudioVisual]\nGravity=6\n[VehicleTypes]\n0=TEST\n1=VICTIM\n[TEST]\nStrength=300\nArmor=heavy\nPrimary=GUN\n[VICTIM]\nStrength=300\nArmor=heavy\n[GUN]\nDamage=10\nROF=100\nRange=10\nSpeed=100\nProjectile=SHOT\nWarhead=WH\n[SHOT]\nImage=BULLET\nArcing={}\nVertical={}\nAA=yes\nDetonationAltitude=2000\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
                if voxel { "no" } else { "yes" },
                if voxel { "yes" } else { "no" },
            ));
            let make_rules = || {
                let mut rules = RuleSet::from_ini(&ini).unwrap();
                rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(&format!(
                    "[BULLET]\nVoxel={}\n",
                    if voxel { "yes" } else { "no" }
                ))));
                rules
            };
            let mut sim = Simulation::new();
            install_native_size_terrain(&mut sim, 16, 16);
            let terrain_template = sim.resolved_terrain.as_ref().unwrap().clone();
            sim.install_resolved_terrain_for_new_map(terrain_template.clone());
            sim.overlay_grid = Some(
                crate::sim::overlay_grid::OverlayGrid::new_with_retained_wall_plane(
                    terrain_template.width(),
                    terrain_template.height(),
                ),
            );
            let source = sim.allocate_stable_id();
            let target = sim.allocate_stable_id();
            let z = if voxel { 1000 } else { 100 };
            place(
                &mut sim,
                source,
                ProjectileCoord::new(3200, 3200, z),
                EntityCategory::Unit,
            );
            place(
                &mut sim,
                target,
                ProjectileCoord::new(3700, 3200, z - if voxel { 300 } else { 0 }),
                EntityCategory::Unit,
            );
            let target_type = sim.interner.intern("VICTIM");
            let enemy = sim.interner.intern("Russians");
            let victim = sim.substrate.entities.get_mut(target).unwrap();
            victim.type_ref = target_type;
            victim.owner = enemy;
            assert!(matches!(sim.reveal(source), RevealOutcome::Revealed { .. }));
            assert!(matches!(sim.reveal(target), RevealOutcome::Revealed { .. }));
            // Reveal commits the coarse map coordinate; install the supplied
            // admitted raw XYZ afterward, matching the native oracle receiver.
            sim.substrate
                .entities
                .get_mut(source)
                .unwrap()
                .position
                .exact_z_leptons = Some(z);
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .position
                .exact_z_leptons = Some(z - if voxel { 300 } else { 0 });
            assert!(sim.substrate.entities.get(source).unwrap().in_playfield);
            assert!(sim.substrate.entities.get(target).unwrap().in_playfield);
            let firer = sim.substrate.entities.get_mut(source).unwrap();
            firer.facing = 64;
            firer.attack_target = Some(AttackTarget::new(target));
            let mut runtime = SimRuntime::from_simulation(sim);
            runtime.resources.rules = make_rules();
            let output = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            assert_eq!(
                runtime.simulation.projectiles.len(),
                1,
                "normal runtime FireAt must create one projectile, voxel={voxel}, fire_events={:?}, attack={:?}, logic={:?}",
                output.fire_events,
                runtime.simulation.substrate.entities.get(source).map(|e| (
                    &e.attack_target,
                    e.lifecycle,
                    e.in_playfield,
                    e.mission.current()
                )),
                runtime.simulation.live_object_order_snapshot()
            );
            let (&id, shot) = runtime.simulation.projectiles.iter().next().unwrap();
            let data = if voxel {
                include_str!("../../../tools/projectile_oracle/voxel_launch.json")
            } else {
                include_str!("../../../tools/projectile_oracle/fireat_launch.json")
            };
            let rows: Vec<Value> = serde_json::from_str(data).unwrap();
            let expected = rows
                .iter()
                .find(|row| {
                    row["delta"] == serde_json::json!([500, 0, if voxel { -300 } else { 0 }])
                        && row["speed"] == 100
                        && if voxel {
                            row["voxel"] == true && row["vertical"] == true
                        } else {
                            row["arcing"] == true
                                && row["lobber"] == false
                                && row["floater"] == false
                        }
                })
                .unwrap();
            let expected_bits: Vec<_> = expected["bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                shot.velocity
                    .native()
                    .map(|v| format!("{:016x}", v.bits()))
                    .as_slice(),
                expected_bits.as_slice()
            );
            assert_eq!(
                shot.position,
                ProjectileCoord::new(3200, 3200, z),
                "new projectile waits until the next Logic visit"
            );
            if voxel {
                continue;
            }
            runtime
                .simulation
                .substrate
                .entities
                .get_mut(source)
                .unwrap()
                .attack_target = None;
            let _ = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            let snapshot = GameSnapshot::save(&runtime.simulation, 0, 0, "native motion", 0);
            let mut restored = GameSnapshot::load(&snapshot).unwrap().sim;
            let mut serialized_expected = runtime.simulation.projectiles.get(id).unwrap().clone();
            serialized_expected.in_logic_vector = false;
            assert_eq!(restored.projectiles.get(id), Some(&serialized_expected));
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(
                restored.projectiles.get(id).unwrap().velocity,
                runtime.simulation.projectiles.get(id).unwrap().velocity
            );
            let mut expected_loaded = runtime.simulation.projectiles.get(id).unwrap().clone();
            expected_loaded
                .arm_timer
                .start(restored.session.binary_frame as i32, 0);
            assert_eq!(restored.projectiles.get(id), Some(&expected_loaded));
            let hash = restored.state_hash();
            let velocity = restored.projectiles.get(id).unwrap().velocity;
            restored.projectiles.get_mut(id).unwrap().velocity.x =
                crate::util::native_x87::NativeF64Bits::from_bits(velocity.x.bits() ^ 1);
            assert_ne!(
                restored.state_hash(),
                hash,
                "one binary64 ULP remains hash-visible"
            );
            restored.projectiles.get_mut(id).unwrap().velocity = velocity;
            assert_eq!(restored.state_hash(), hash);
            // The real load contract rebuilds skipped map caches and native
            // overlay/Tiberium authority. Scenario RNG and Tiberium timers have
            // their own load resets, so compare the delivered Bullet state rather
            // than claiming whole-world identity across those resets.
            restored.rebuild_caches_after_load(
                terrain_template,
                crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
                Vec::new(),
                Vec::new(),
            );
            restored
                .restore_map_authority_after_snapshot_load(
                    &make_rules(),
                    &runtime.resources.overlay_registry,
                )
                .unwrap();
            let mut resumed = SimRuntime::from_simulation(restored);
            resumed.resources.rules = make_rules();
            let rows: Vec<Value> = serde_json::from_str(include_str!(
                "../../../tools/projectile_oracle/ordinary_motion.json"
            ))
            .unwrap();
            let row = rows
                .iter()
                .find(|row| {
                    row["origin"] == serde_json::json!([3200, 3200, 100])
                        && row["input_bits"][0] == "4058b4d922000000"
                        && row["floater"] == false
                        && row["gravity_sequence"] == serde_json::json!([6, 3, 1, 0, -1, 2, 5, 6])
                })
                .unwrap();
            for frame in 0..3 {
                if frame != 0 {
                    for runtime in [&mut runtime, &mut resumed] {
                        runtime.resources.rules.general.gravity =
                            row["gravity_sequence"][frame].as_i64().unwrap() as i32;
                        let _ = runtime
                            .advance_frame(&[], 16, TickLane::Ordinary)
                            .expect("fixture frame must complete");
                    }
                }
                let shot = runtime
                    .simulation
                    .projectiles
                    .get(id)
                    .expect("ordinary source shot survives these visits");
                let expected = &row["frames"][frame];
                let expected_bits: Vec<_> = expected["bits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(
                    shot.velocity
                        .native()
                        .map(|v| format!("{:016x}", v.bits()))
                        .as_slice(),
                    expected_bits.as_slice(),
                    "runtime native motion frame {frame}"
                );
                assert_eq!(
                    [shot.position.x, shot.position.y, shot.position.z],
                    std::array::from_fn::<_, 3, _>(
                        |i| expected["candidate"][i].as_i64().unwrap() as i32
                    )
                );
                assert_eq!(
                    shot.position,
                    resumed.simulation.projectiles.get(id).unwrap().position,
                    "snapshot continuation frame {frame}"
                );
                assert_eq!(
                    shot.velocity,
                    resumed.simulation.projectiles.get(id).unwrap().velocity
                );
            }
        }
    }

    #[test]
    fn vertical_projectile_keeps_post_ramp_velocity_across_runtime_visits() {
        use crate::sim::runtime::SimRuntime;
        let mut sim = Simulation::new();
        install_native_size_terrain(&mut sim, 2, 3);
        let mut shot = gsi_05_02_projectile(999, None);
        shot.origin = ProjectileCoord::new(640, 640, 5);
        shot.target = ProjectileTarget::Cell { rx: 5, ry: 2 };
        shot.initial_target_position = ProjectileCoord::new(1408, 640, 0);
        shot.velocity = ProjectileVelocity::new(1, 0, 0);
        shot.speed_leptons_per_frame = 1;
        shot.trajectory = ProjectileTrajectory::Vertical {
            detonation_altitude: 1000,
            acceleration: 10,
            max_speed: 100,
        };
        sim.admit_projectile(100, shot);
        let mut runtime = SimRuntime::from_simulation(sim);
        // Original successive visits preserve the binary64 ramp, including
        // the approximate normalization's fractional bits after the first visit.
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/vertical_motion.json"
        ))
        .unwrap();
        let row = rows
            .iter()
            .find(|row| {
                row["origin"] == serde_json::json!([640, 640, 5]) && row["acceleration"] == 10
            })
            .unwrap();
        for expected in row["frames"].as_array().unwrap().iter().take(3) {
            let _ = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            let projectile = runtime
                .simulation
                .projectiles
                .get(100)
                .expect("post-ramp speed survives the ordinary tail");
            let candidate = &expected["candidate"];
            assert_eq!(
                projectile.position,
                ProjectileCoord::new(
                    candidate[0].as_i64().unwrap() as i32,
                    candidate[1].as_i64().unwrap() as i32,
                    candidate[2].as_i64().unwrap() as i32
                )
            );
            let expected_bits: Vec<_> = expected["bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                projectile
                    .velocity
                    .native()
                    .map(|v| format!("{:016x}", v.bits()))
                    .as_slice(),
                expected_bits.as_slice()
            );
            assert!(runtime.simulation.pending_projectile_detonations.is_empty());
        }
    }

    #[test]
    fn ordinary_projectile_near_target_final_coordinate_reaches_runtime_damage() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::runtime::SimRuntime;
        let ini = IniFile::from_str(
            "[General]\nVeteranRatio=3.0\n[AudioVisual]\nGravity=0\n[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n[Warheads]\n0=WALLWH\n[WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=1\n",
        );
        let art = IniFile::from_str("[WALL]\nDamageLevels=4\n");
        let mut sim = Simulation::new();
        install_native_size_terrain(&mut sim, 8, 8);
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(24, 24);
        overlays.place_overlay(5, 5, 0, 0);
        overlays.place_overlay(6, 5, 0, 0);
        sim.overlay_grid = Some(overlays);
        // The frozen target cell admits near-target at candidate (1424,1408,5).
        // The live target has moved to the next cell: final +48 must redirect
        // the actual damage to (1664,1408,0), not leave it at that candidate.
        place(
            &mut sim,
            2,
            ProjectileCoord::new(1664, 1408, 0),
            EntityCategory::Unit,
        );
        let mut shot = gsi_05_02_projectile(999, None);
        shot.target = ProjectileTarget::Entity(2);
        shot.origin = ProjectileCoord::new(1408, 1408, 5);
        shot.initial_target_position = ProjectileCoord::new(1408, 1408, 0);
        shot.velocity = ProjectileVelocity::new(16, 0, 0);
        shot.trajectory = ProjectileTrajectory::Ballistic;
        shot.payload = crate::sim::projectile::ProjectilePayload {
            base_damage: 1,
            warhead: sim.interner.intern("WALLWH"),
            weapon: sim.interner.intern("MISSING"),
            owner: sim.interner.intern("Americans"),
        };
        sim.admit_projectile(100, shot);
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources.rules = RuleSet::from_ini(&ini).unwrap();
        runtime.resources.overlay_registry =
            crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
        let _ = runtime
            .advance_frame(&[], 16, TickLane::Ordinary)
            .expect("fixture frame must complete");
        assert!(runtime.simulation.projectiles.get(100).is_none());
        let overlays = runtime.simulation.overlay_grid.as_ref().unwrap();
        assert_eq!(
            overlays.cell(5, 5).overlay_data,
            0,
            "candidate cell receives no wall damage"
        );
        assert_eq!(
            overlays.cell(6, 5).overlay_data,
            0x10,
            "final target coordinate receives damage immediately"
        );
        assert!(runtime.simulation.pending_projectile_detonations.is_empty());
    }

    #[test]
    fn reflected_fractional_motion_reaches_runtime_impact_and_removal() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::runtime::SimRuntime;
        use crate::util::native_x87::NativeF64Bits;
        let ini = IniFile::from_str(
            "[General]\nVeteranRatio=3.0\n[AudioVisual]\nGravity=0\n[Warheads]\n0=WALLWH\n[WALLWH]\nWall=yes\nCellSpread=0\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[OverlayTypes]\n0=WALL\n[WALL]\nWall=yes\nStrength=1\n",
        );
        let art = IniFile::from_str("[WALL]\nDamageLevels=4\n");
        let mut sim = Simulation::new();
        install_native_size_terrain(&mut sim, 8, 8);
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(24, 24);
        overlays.place_overlay(5, 5, 0, 0);
        sim.overlay_grid = Some(overlays);
        let mut shot = gsi_05_02_projectile(999, None);
        shot.target = ProjectileTarget::None;
        shot.origin = ProjectileCoord::new(1388, 1405, 5);
        shot.initial_target_position = ProjectileCoord::new(2000, 1408, 0);
        shot.velocity = ProjectileVelocity::from_native(
            [20.0f64, 3.0, -5.5].map(|v| NativeF64Bits::from_bits(v.to_bits())),
        );
        shot.trajectory = ProjectileTrajectory::Ballistic;
        shot.payload = crate::sim::projectile::ProjectilePayload {
            base_damage: 1,
            warhead: sim.interner.intern("WALLWH"),
            weapon: sim.interner.intern("MISSING"),
            owner: sim.interner.intern("Americans"),
        };
        sim.admit_projectile(100, shot);
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources.rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(runtime.resources.rules.general.gravity, 0);
        runtime.resources.overlay_registry =
            crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
        // Native geometry fixtures establish the source-less fractional-below-floor
        // admission and Z=0 selection. Exact reflected stores have their own
        // original-byte comparisons; this checks the real AI-to-damage delivery.
        let _ = runtime
            .advance_frame(&[], 16, TickLane::Ordinary)
            .expect("fixture frame must complete");
        assert!(runtime.simulation.projectiles.get(100).is_none());
        assert_eq!(
            runtime
                .simulation
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(5, 5)
                .overlay_data,
            0x10
        );
        assert!(runtime.simulation.pending_projectile_detonations.is_empty());
    }

    #[test]
    fn ordinary_geometry_uses_native_double_and_integer_boundaries() {
        use crate::util::native_x87::NativeF64Bits;
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/ordinary_collision_vectors.json"
        ))
        .unwrap();
        for (index, row) in vectors["geometry"].as_array().unwrap().iter().enumerate() {
            let mut sim = Simulation::new();
            install_common_raw_terrain(&mut sim, 8, 8, 0, None);
            let cell = &row["cells"]["2,2"];
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(2, 2)
                .unwrap()
                .bridge_facts
                .raw_flags = cell["flags"].as_u64().unwrap_or(0) as u32;
            let overlay = cell["overlay"].as_i64().unwrap_or(-1);
            if overlay >= 0 {
                let mut grid = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
                grid.place_overlay(2, 2, overlay as u8, 0);
                sim.overlay_grid = Some(grid);
            }
            if row["source"].as_bool().unwrap_or(true) {
                place(
                    &mut sim,
                    1,
                    ProjectileCoord::new(384, 640, 0),
                    EntityCategory::Unit,
                );
            }
            let building = &row["building"];
            let dimensions = row["foundation_dimensions"]
                .as_array()
                .map(|v| format!("{}x{}", v[0], v[1]))
                .unwrap_or_else(|| "1x1".into());
            let undeploy = if building["undeploy"] == true {
                "UndeploysInto=UNIT\n"
            } else {
                ""
            };
            let rules=RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!("[VehicleTypes]\n0=UNIT\n[UNIT]\nStrength=100\n[BuildingTypes]\n0=TEST\n[TEST]\nFoundation={dimensions}\n{undeploy}"))).unwrap();
            if building.is_object() {
                place(
                    &mut sim,
                    2,
                    ProjectileCoord::new(640, 640, 0),
                    EntityCategory::Structure,
                );
                let owner = sim.interner.intern("Russians");
                sim.substrate.entities.get_mut(2).unwrap().owner = owner;
                sim.substrate.occupancy.add(
                    2,
                    2,
                    2,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
            if row["source_allied"] == true {
                sim.house_alliances
                    .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
            }
            let mut shot = gsi_05_02_projectile(
                if building["source_identity"] == true {
                    2
                } else {
                    1
                },
                None,
            );
            shot.origin = coord(row, "old", [640, 640, 500]);
            sim.projectiles.spawn(100, shot);
            let candidate: [NativeF64Bits; 3] = std::array::from_fn(|i| {
                NativeF64Bits::from_bits(row["candidate"][i].as_f64().unwrap().to_bits())
            });
            let motion = ProjectileCollisionMotion {
                candidate,
                velocity: [20.0f64, 3.0, -6.0].map(|v| NativeF64Bits::from_bits(v.to_bits())),
            };
            let mut world = world(&sim);
            world.rules = Some(&rules);
            let (actual, impact) =
                world.ordinary_geometry(sim.projectiles.get(100).unwrap(), motion);
            assert_eq!(
                impact,
                row["impact"].as_bool().unwrap(),
                "native geometry{index}: {row}"
            );
            assert_eq!(
                actual.candidate_coord(),
                coord(row, "result", [0; 3]),
                "native geometry{index}: {row}"
            );
            assert_eq!(
                serde_json::json!(actual.candidate.map(|v| v.bits())),
                row["result_candidate_bits"],
                "native candidate{index}: {row}"
            );
            assert_eq!(
                serde_json::json!(actual.velocity.map(|v| v.bits())),
                row["result_velocity_bits"],
                "native reflection{index}: {row}"
            );
        }
    }
}

impl ProjectileCollisionWorld<'_> {
    fn cell(&self, coord: ProjectileCoord) -> CellRef<'_> {
        if self.terrain.is_some() {
            get_cellclass_fallback_leptons(self.terrain, coord.x, coord.y)
        } else {
            self.dummy.stamp_coord(coord.x / 256, coord.y / 256);
            CellRef::Dummy {
                cell: self.dummy.clone(),
            }
        }
    }

    fn packed_cell(&self, coord: ProjectileCoord) -> CellRef<'_> {
        if self.terrain.is_some() {
            get_cellclass_fallback(self.terrain, coord.x / 256, coord.y / 256)
        } else {
            self.cell(coord)
        }
    }

    fn ground(&self, coord: ProjectileCoord) -> i32 {
        projectile_ground_z(self.terrain, self.dummy, coord)
    }

    fn building(&self, cell: &CellRef<'_>) -> Option<u64> {
        let CellRef::Real(cell) = cell else {
            return None;
        };
        self.occupancy
            .first_building_on_layer(cell.rx, cell.ry, MovementLayer::Ground)
    }

    fn raw_location(&self, object: &crate::sim::game_entity::GameEntity) -> ProjectileCoord {
        ProjectileCoord::new(
            i32::from(object.position.rx) * 256 + object.position.sub_x.to_num::<i32>(),
            i32::from(object.position.ry) * 256 + object.position.sub_y.to_num::<i32>(),
            crate::sim::combat::object_world_z_leptons(object, self.terrain),
        )
    }

    fn location(&self, object: &crate::sim::game_entity::GameEntity) -> ProjectileCoord {
        let mut coord = self.raw_location(object);
        if object.category == EntityCategory::Structure
            && let Some(kind) = self
                .rules
                .and_then(|rules| rules.object(self.interner.resolve(object.type_ref())))
        {
            let (width, height) = crate::rules::foundation::foundation_dimensions(&kind.foundation);
            coord.x = coord.x.wrapping_add(i32::from(width) * 128 - 128);
            coord.y = coord.y.wrapping_add(i32::from(height) * 128 - 128);
        }
        coord
    }

    fn allied(&self, source: Option<u64>, other: u64) -> bool {
        let Some(source) = source.and_then(|id| self.entities.get(id)) else {
            return false;
        };
        let Some(other) = self.entities.get(other) else {
            return false;
        };
        crate::map::houses::is_allied_with(
            self.alliances,
            self.interner.resolve(source.owner()),
            self.interner.resolve(other.owner()),
        )
    }

    fn overlay(&self, cell: &CellRef<'_>) -> (Option<u8>, Option<crate::sim::intern::InternedId>) {
        match cell {
            CellRef::Real(cell) => self.overlays.map_or((None, None), |grid| {
                let overlay = grid.cell(cell.rx, cell.ry);
                (overlay.overlay_id, overlay.wall_owner)
            }),
            CellRef::Dummy { cell } => (cell.overlay_fields().0, None),
        }
    }

    fn level(cell: &CellRef<'_>) -> i32 {
        match cell {
            CellRef::Real(c) => i32::from(c.level as i8),
            CellRef::Dummy { cell } => i32::from(cell.snapshot().level),
        }
    }

    fn effective_level(cell: &CellRef<'_>) -> i32 {
        Self::level(cell)
            + if cell.bridge_flags_0x1180() & 0x80 != 0 {
                4
            } else {
                0
            }
    }

    /// 47C3D0(0,0,ground,null) chooses minimum truncated distance of low-byte
    /// XY, with strict ties retaining E4 list order. Nonbuilding Techno
    /// insertions prepend; Building insertions append (47E8A0).
    fn nearest(&self, cell: &CellRef<'_>) -> Option<(u64, ProjectileCoord)> {
        let CellRef::Real(cell) = cell else {
            return None;
        };
        // 47C427 tests Techno identity, set only by6F322F. Terrain's
        // Object base sets bit1 (5F3B37), so Terrain is not eligible.
        let objects = self
            .occupancy
            .get(cell.rx, cell.ry)
            .into_iter()
            .flat_map(|occupants| occupants.iter_layer(MovementLayer::Ground))
            .filter_map(|occupant| self.entities.get(occupant.entity_id))
            .map(|object| (object.stable_id(), self.raw_location(object)));
        let mut best = None;
        let mut best_distance = 0;
        for object in objects {
            let selected_location = self
                .entities
                .get(object.0)
                .map_or(object.1, |entity| self.location(entity));
            let distance = coord_distance(
                ProjectileCoord::new(selected_location.x & 255, selected_location.y & 255, 0),
                ProjectileCoord::new(0, 0, 0),
            );
            if best.is_none() || distance < best_distance {
                best = Some(object);
                best_distance = distance;
            }
        }
        best
    }

    pub fn collide(
        &self,
        projectile: &Projectile,
        candidate: ProjectileCoord,
        phase: ProjectileCollisionPhase,
    ) -> Option<ProjectileCollisionResponse> {
        match phase {
            ProjectileCollisionPhase::Ordinary {
                persistent_velocity,
                motion,
            } => Some(self.ordinary(projectile, motion, persistent_velocity)),
            ProjectileCollisionPhase::Shared => self
                .shared(projectile, candidate)
                .then_some(ProjectileCollisionResponse::TargetZClamp(candidate)),
            ProjectileCollisionPhase::TargetLocation => match projectile.target {
                ProjectileTarget::Entity(id) => self
                    .entities
                    .get(id)
                    .map(|target| ProjectileCollisionResponse::TargetZClamp(self.location(target))),
                _ => None,
            },
        }
    }

    fn ordinary(
        &self,
        projectile: &Projectile,
        motion: ProjectileCollisionMotion,
        persistent_velocity: ProjectileVelocity,
    ) -> ProjectileCollisionResponse {
        let (motion, impact) = self.ordinary_geometry(projectile, motion);
        self.ordinary_tail(
            projectile,
            motion.candidate_coord(),
            persistent_velocity,
            ProjectileVelocity::from_native(motion.velocity),
            motion.velocity,
            impact,
        )
    }

    fn ordinary_geometry(
        &self,
        projectile: &Projectile,
        mut motion: ProjectileCollisionMotion,
    ) -> (ProjectileCollisionMotion, bool) {
        use crate::util::native_x87::NativeF64Bits;
        let candidate = motion.candidate_coord();
        let mut candidate_z = f64::from_bits(motion.candidate[2].bits());
        let vertical = matches!(projectile.trajectory, ProjectileTrajectory::Vertical { .. });
        let source = self
            .entities
            .get(projectile.source_id)
            .map(|_| projectile.source_id);
        let mut impact = false;
        if !vertical {
            let floor = self.ground(candidate);
            let deck = floor.wrapping_add(416);
            let cell = self.cell(candidate);
            let structural = cell.bridge_flags_0x1180() & 0x100 != 0
                || self.cell(projectile.position).bridge_flags_0x1180() & 0x100 != 0;
            let crossing = if structural {
                projectile_bridge_crossing(projectile.position.z, candidate.z, deck)
            } else {
                ProjectileBridgeCrossing::None
            };
            let mut obstacle = false;
            if crossing == ProjectileBridgeCrossing::None
                && candidate_z >= f64::from(floor)
                && candidate_z < f64::from(floor) + 150.0
            {
                let building = self.building(&cell);
                obstacle = building.is_some()
                    || self
                        .overlay(&cell)
                        .0
                        .is_some_and(|id| matches!(id, 2 | 26 | 243));
                if let Some(id) = building {
                    let exempt = self
                        .entities
                        .get(id)
                        .and_then(|object| {
                            self.rules.and_then(|rules| {
                                rules.object(self.interner.resolve(object.type_ref()))
                            })
                        })
                        .is_some_and(|kind| {
                            kind.is_1x1_with_undeploy()
                                && kind.undeploys_into.as_deref().is_some_and(|name| {
                                    self.rules.is_some_and(|rules| rules.object(name).is_some())
                                })
                        });
                    // LaserFence+stage>=8 has no active retail type/data writer.
                    obstacle &= Some(id) != source && !exempt && !self.allied(source, id);
                }
            }
            if candidate_z < f64::from(floor)
                || crossing != ProjectileBridgeCrossing::None
                || obstacle
            {
                if source.is_none() {
                    match crossing {
                        ProjectileBridgeCrossing::Down => candidate_z = f64::from(deck),
                        ProjectileBridgeCrossing::Up => {
                            candidate_z = f64::from(deck.wrapping_sub(20))
                        }
                        ProjectileBridgeCrossing::None
                            if candidate_z > f64::from(floor.wrapping_sub(100)) =>
                        {
                            candidate_z = f64::from(floor)
                        }
                        ProjectileBridgeCrossing::None => {}
                    }
                    let slope = match self.cell(candidate) {
                        CellRef::Real(c) => c.slope_type,
                        CellRef::Dummy { cell } => cell.snapshot().slope_type,
                    };
                    motion.velocity = crate::sim::projectile::projectile_slope_reflect_double(
                        motion.velocity,
                        slope,
                        projectile.collision.elasticity_bits,
                    )
                    .expect("supported native Bullet slope matrix");
                    impact = true;
                } else if crossing != ProjectileBridgeCrossing::None {
                    candidate_z = f64::from(if crossing == ProjectileBridgeCrossing::Down {
                        deck
                    } else {
                        deck.wrapping_sub(20)
                    });
                    impact = true;
                }
            }
        }
        motion.candidate[2] = NativeF64Bits::from_bits(candidate_z.to_bits());
        (motion, impact)
    }

    fn ordinary_tail(
        &self,
        projectile: &Projectile,
        mut candidate: ProjectileCoord,
        persistent_velocity: ProjectileVelocity,
        mut velocity: ProjectileVelocity,
        native_velocity: [crate::util::native_x87::NativeF64Bits; 3],
        mut impact: bool,
    ) -> ProjectileCollisionResponse {
        let vertical = matches!(projectile.trajectory, ProjectileTrajectory::Vertical { .. });
        let source = self
            .entities
            .get(projectile.source_id)
            .map(|_| projectile.source_id);
        let mut near_target = false;
        let mut left_map = false;
        let target_cell_coord = (
            (projectile.launch_target.x / 256) as i16,
            (projectile.launch_target.y / 256) as i16,
        );
        let same_cell =
            ((candidate.x / 256) as i16, (candidate.y / 256) as i16) == target_cell_coord;
        let old_height = || {
            projectile
                .position
                .z
                .wrapping_sub(self.ground(projectile.position))
        };
        if !vertical && same_cell && old_height() < 208 {
            impact = true;
            near_target = true;
            velocity = persistent_velocity;
        } else {
            let candidate_cell = self.packed_cell(candidate);
            let target_cell = self.packed_cell(projectile.launch_target);
            let candidate_building = self.building(&candidate_cell);
            let target_building = self.building(&target_cell);
            if !vertical
                && candidate_building.is_some()
                && candidate_building == target_building
                && old_height() < 208
            {
                impact = true;
                near_target = true;
                velocity = persistent_velocity;
            } else {
                let nearest = self.nearest(&self.cell(candidate));
                if let Some((id, position)) = nearest.filter(|(id, position)| {
                    Some(*id) != source
                        && !self.allied(source, *id)
                        && coord_distance(candidate, *position) < 128
                }) {
                    let _ = id;
                    impact = true;
                    if !projectile.collision.inaccurate {
                        candidate = position;
                    }
                    velocity = persistent_velocity;
                } else if !self.inside_map(candidate) {
                    impact = true;
                    left_map = true;
                    candidate = projectile.position;
                    velocity = persistent_velocity;
                } else {
                    if vertical {
                        velocity = persistent_velocity;
                    }
                    if crate::sim::projectile::projectile_velocity_magnitude_double(if vertical {
                        ProjectileCollisionMotion::from_coordinate(candidate, persistent_velocity)
                            .velocity
                    } else {
                        native_velocity
                    }) < 10.0
                        && old_height() < 10
                    {
                        impact = true;
                    }
                }
            }
        }
        ProjectileCollisionResponse::Ordinary {
            candidate,
            velocity,
            impact,
            near_target,
            left_map,
        }
    }

    fn inside_map(&self, candidate: ProjectileCoord) -> bool {
        if let Some((width, height)) = self.map_size {
            let x = i32::from((candidate.x / 256) as i16);
            let y = i32::from((candidate.y / 256) as i16);
            let sum = x.wrapping_add(y);
            width < sum
                && x.wrapping_sub(y) < width
                && y.wrapping_sub(x) < width
                && sum <= width.wrapping_add(height.wrapping_mul(2))
        } else {
            self.terrain.is_none() || matches!(self.cell(candidate), CellRef::Real(_))
        }
    }

    fn shared(&self, projectile: &Projectile, candidate: ProjectileCoord) -> bool {
        let cell = self.cell(candidate);
        if (projectile.collision.subject_to_cliffs || projectile.collision.subject_to_walls)
            && self.cliff_wall(projectile, candidate)
        {
            return true;
        }
        let height = candidate.z.wrapping_sub(self.ground(candidate));
        if height <= -416 {
            return true;
        }
        if projectile.collision.flak_scatter
            && self
                .target_aim(projectile.target)
                .is_some_and(|target| candidate.z < target.z)
            && candidate.z.wrapping_sub(self.ground(candidate)) < 0
        {
            return true;
        }
        if projectile.collision.level_non_water {
            // Cell ctor47BC11 stores DWORD 0xFFFF; ordinary IsoMap loading
            // rejects/reconstructs the dummy, and Recalc skips it entirely.
            let tile = match &cell {
                CellRef::Real(c) => c.final_tile_index,
                CellRef::Dummy { .. } => 0xFFFF,
            };
            let base = self
                .terrain
                .map_or(-1, |terrain| terrain.projectile_water_set_base());
            if tile < base || tile >= base.wrapping_add(14) {
                return true;
            }
        }
        if projectile.collision.anti_air
            && let ProjectileTarget::Entity(id) = projectile.target
            && let Some(target) = self.entities.get(id)
            && self.high_flying(target)
            && self.target_distance(candidate, target) < 128
        {
            return true;
        }
        false
    }

    fn target_distance(
        &self,
        candidate: ProjectileCoord,
        target: &crate::sim::game_entity::GameEntity,
    ) -> i32 {
        // ObjectClass 5F6360: virtual +48 positions, then RTTI-6 foundation
        // radius subtraction (45ECA0(false), 45EC90), clamped at zero.
        let mut distance = coord_distance(candidate, self.location(target));
        if target.category == EntityCategory::Structure
            && let Some(kind) = self
                .rules
                .and_then(|rules| rules.object(self.interner.resolve(target.type_ref())))
        {
            let (width, height) = crate::rules::foundation::foundation_dimensions(&kind.foundation);
            distance = distance
                .wrapping_sub((i32::from(width) + i32::from(height)) * 64)
                .max(0);
        }
        distance
    }

    fn cell_location(cell: &CellRef<'_>) -> ProjectileCoord {
        let (x, y, level, slope) = match cell {
            CellRef::Real(c) => (
                i32::from(c.rx as i16),
                i32::from(c.ry as i16),
                c.level,
                c.slope_type,
            ),
            CellRef::Dummy { cell } => {
                let c = cell.snapshot();
                (c.coord.0, c.coord.1, c.level as u8, c.slope_type)
            }
        };
        let x = x * 256 + 128;
        let y = y * 256 + 128;
        let z = crate::util::lepton::ground_height_leptons(level, slope, x, y)
            .expect("native Cell +48 slope");
        ProjectileCoord::new(x, y, z)
    }

    fn cliff_wall(&self, projectile: &Projectile, candidate: ProjectileCoord) -> bool {
        let source = self.cell(projectile.launch_origin);
        let target = self.cell(projectile.launch_target);
        let previous = if self.terrain.is_some() {
            get_cellclass_fallback(
                self.terrain,
                i32::from(projectile.previous_cell.0),
                i32::from(projectile.previous_cell.1),
            )
        } else {
            self.dummy.stamp_coord(
                i32::from(projectile.previous_cell.0),
                i32::from(projectile.previous_cell.1),
            );
            CellRef::Dummy {
                cell: self.dummy.clone(),
            }
        };
        let cell = self.packed_cell(candidate);
        if !self.cell_blocks(projectile, &source, &target, &previous, &cell) {
            return false;
        }
        // 4CC489..4CC671 retains each Cell pointer, observes +48 in this
        // order, and may requery the SAME packed candidate. Dummy identities
        // remain live through all queries; never snapshot them at fetch time.
        let center = Self::cell_location(&cell);
        if coord_distance(candidate, center) <= 85 {
            return true;
        }
        let launch = Self::cell_location(&source);
        let destination = Self::cell_location(&target);
        let major_x = launch.x.wrapping_sub(destination.x).wrapping_abs();
        let major_y = launch.y.wrapping_sub(destination.y).wrapping_abs();
        let off_x = candidate.x.wrapping_sub(center.x).wrapping_abs();
        let off_y = candidate.y.wrapping_sub(center.y).wrapping_abs();
        if (major_x > major_y && off_y > off_x) || (major_y > major_x && off_x > off_y) {
            let second = self.packed_cell(candidate);
            self.cell_blocks(projectile, &source, &target, &previous, &second)
        } else {
            true
        }
    }

    fn cell_blocks(
        &self,
        projectile: &Projectile,
        source: &CellRef<'_>,
        target: &CellRef<'_>,
        previous: &CellRef<'_>,
        cell: &CellRef<'_>,
    ) -> bool {
        if projectile.collision.subject_to_cliffs
            && Self::effective_level(cell) - Self::effective_level(previous) >= 4
            && Self::effective_level(cell) > Self::effective_level(source)
        {
            return true;
        }
        if !projectile.collision.subject_to_walls
            || cell == target
            || Self::level(source) > Self::level(target)
        {
            return false;
        }
        let (overlay, owner) = self.overlay(cell);
        if !overlay.is_some_and(|id| {
            self.overlay_registry
                .and_then(|registry| registry.flags(id))
                .is_some_and(|flags| flags.wall)
        }) {
            return false;
        }
        if self
            .rules
            .is_some_and(|rules| rules.allied_wall_transparency)
            && let (Some(owner), Some(source)) = (owner, self.entities.get(projectile.source_id))
            && crate::map::houses::is_allied_with(
                self.alliances,
                self.interner.resolve(owner),
                self.interner.resolve(source.owner()),
            )
        {
            return false;
        }
        true
    }

    pub(super) fn target_aim(&self, target: ProjectileTarget) -> Option<ProjectileCoord> {
        match target {
            ProjectileTarget::Entity(id) => {
                self.entities.get(id).map(|target| self.location(target))
            }
            ProjectileTarget::Cell { rx, ry } => Some(crate::sim::projectile::cell_target_coord(
                self.terrain,
                rx,
                ry,
            )),
            ProjectileTarget::DummyCell => {
                Some(crate::sim::projectile::dummy_cell_target_coord(self.dummy))
            }
            ProjectileTarget::None => None,
        }
    }

    fn high_flying(&self, target: &crate::sim::game_entity::GameEntity) -> bool {
        if target.category == EntityCategory::Aircraft
            && self.rules.is_some_and(|rules| {
                let name = self.interner.resolve(target.type_ref());
                name.eq_ignore_ascii_case(&rules.missile_spawn.v3.type_name)
                    || name.eq_ignore_ascii_case(&rules.missile_spawn.dmisl.type_name)
            })
        {
            return target
                .rocket_state
                .as_ref()
                .is_some_and(|rocket| rocket.phase.is_moving_now());
        }
        let raw = self.raw_location(target);
        target.lifecycle.cell_marked
            && raw
                .z
                .wrapping_sub(self.ground(raw))
                .wrapping_sub(if target.on_bridge { 416 } else { 0 })
                >= 208
    }
}
