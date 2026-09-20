use super::*;
use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::bounce::{BounceOutcome, BounceState};
use crate::sim::overlay_grid::OverlayGrid;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use serde_json::Value;

fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: None,
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: TerrainClass::Clear,
        base_speed_costs: SpeedCostProfile::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0; 3],
        radar_right: [0; 3],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

fn body(case: &Value) -> BounceState {
    let floats = |name: &str| {
        std::array::from_fn(|i| {
            NativeF32Bits::from_bits((case[name][i].as_f64().unwrap() as f32).to_bits())
        })
    };
    BounceState {
        elasticity: NativeF64Bits::from_bits(case["elasticity"].as_f64().unwrap().to_bits()),
        gravity: NativeF64Bits::from_bits(case["gravity"].as_f64().unwrap().to_bits()),
        angular_velocity_magnitude: NativeF64Bits::POSITIVE_ZERO,
        position: floats("position"),
        velocity: floats("velocity"),
        spin_axis: [NativeF32Bits::POSITIVE_ZERO; 3],
        spin_angle: NativeF64Bits::POSITIVE_ZERO,
    }
}
fn fixture(case: &Value) -> Simulation {
    let rows = case["cells"].as_array().unwrap();
    let width = rows
        .iter()
        .map(|c| c[0].as_u64().unwrap() as u16 + 1)
        .max()
        .unwrap_or(1);
    let height = rows
        .iter()
        .map(|c| c[1].as_u64().unwrap() as u16 + 1)
        .max()
        .unwrap_or(1);
    let mut cells: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| terrain_cell(x, y)))
        .collect();
    let mut allocated = Vec::new();
    let mut overlays = OverlayGrid::new(width, height);
    for row in rows {
        let x = row[0].as_u64().unwrap() as u16;
        let y = row[1].as_u64().unwrap() as u16;
        let mut cell = terrain_cell(x, y);
        cell.level = row[2].as_i64().unwrap() as u8;
        cell.slope_type = row[3].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = row[4].as_u64().unwrap() as u32;
        // Deliberately keep the broader metadata true for lowwood237: raw100
        // alone must select the deck plane.
        cell.has_bridge_deck =
            cell.bridge_facts.raw_flags & 256 != 0 || row[5].as_i64() == Some(237);
        let overlay = row[5].as_i64().unwrap();
        overlays.cell_mut(x, y).overlay_id = (overlay >= 0).then_some(overlay as u8);
        cells[usize::from(y) * usize::from(width) + usize::from(x)] = cell;
        allocated.push((x, y));
    }
    let mut grid = ResolvedTerrainGrid::from_cells(width, height, cells);
    grid.test_set_native_allocated_cells(&allocated);
    grid.test_set_dummy_cell_level_slope(
        case["dummy_level"].as_i64().unwrap() as i8,
        case["dummy_slope"].as_u64().unwrap() as u8,
    );
    grid.shared_cell_dummy()
        .set_bridge_flags_0x1180(case["dummy_flags"].as_u64().unwrap() as u32);
    grid.shared_cell_dummy().stamp_coord(1234, -2345);
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(grid);
    sim.overlay_grid = Some(overlays);
    if let Some(objects) = case["objects"].as_array() {
        for kind in objects {
            let name = kind.as_str().unwrap();
            let id = sim.allocate_stable_id();
            let owner = sim.interner.intern("Neutral");
            let type_ref = sim.interner.intern(name);
            let category = if name == "nonbuilding" {
                EntityCategory::Unit
            } else {
                EntityCategory::Structure
            };
            let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                id,
                0,
                0,
                0,
                0,
                owner,
                crate::sim::components::Health { current: 100 },
                type_ref,
                category,
                0,
                0,
                false,
            );
            sim.entities_mut().insert(entity);
            sim.occupancy_mut().add(
                0,
                0,
                id,
                MovementLayer::Ground,
                None,
                crate::sim::occupancy::CellListInsertion::AppendBuilding,
            );
        }
    }
    sim
}
#[test]
fn native_bounce_ground_deck_contacts_match_original_update() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bounce_height.json"
    ))
    .unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 48);
    let rules = fixture_rules();
    let mut issues = Vec::new();
    macro_rules! check_eq {
        ($actual:expr,$expected:expr,$($message:tt)*) => {{
            let(actual,expected)=(&$actual,&$expected);
            if actual != expected {issues.push(format!("{}: {:?} != {:?}",format!($($message)*),actual,expected));}
        }};
    }
    for case in cases.as_array().unwrap() {
        let sim = fixture(case);
        let terrain = ResolvedBounceTerrain {
            sim: &sim,
            rules: Some(&rules),
        };
        let mut state = body(case);
        // The ground observation is compared on a separate fixture so it cannot
        // pre-stamp the live transaction's dummy or mask the ordering check.
        let ground_sim = fixture(case);
        let ground_terrain = ResolvedBounceTerrain {
            sim: &ground_sim,
            rules: Some(&rules),
        };
        let query = &case["events"][0];
        check_eq!(
            ground_terrain.ground_height_leptons(IVec3::new(
                query[1].as_i64().unwrap() as i32,
                query[2].as_i64().unwrap() as i32,
                query[3].as_i64().unwrap() as i32
            )),
            case["ground_z"].as_i64().unwrap() as i32,
            "{} ground",
            case["name"]
        );
        let terrain = RecordingTerrain {
            inner: terrain,
            events: Default::default(),
        };
        let outcome = state.update(&terrain).unwrap();
        check_eq!(
            serde_json::Value::Array(terrain.events.into_inner()),
            case["events"],
            "{} lookup order",
            case["name"]
        );
        let native_outcome = match outcome {
            BounceOutcome::Falling => 0,
            BounceOutcome::Bounced => 1,
            BounceOutcome::Stopped => 2,
        };
        check_eq!(
            native_outcome,
            case["outcome"].as_u64().unwrap(),
            "{} outcome",
            case["name"]
        );
        for (key, values) in [
            ("position_bits", state.position),
            ("velocity_bits", state.velocity),
        ] {
            for i in 0..3 {
                let actual = values[i].bits();
                let expected = case[key][i].as_u64().unwrap() as u32;
                if key == "velocity_bits" {
                    // Existing flat reflection collapses identity matrices and
                    // loses native signed zero. Physics-bit parity is excluded;
                    // require exact finite numeric equality, no tolerance.
                    check_eq!(
                        f32::from_bits(actual),
                        f32::from_bits(expected),
                        "{} {key}[{i}] (signed zero excluded)",
                        case["name"]
                    );
                } else {
                    check_eq!(actual, expected, "{} {key}[{i}]", case["name"]);
                }
            }
        }
        let actual = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .dummy_cell_requested_coord();
        check_eq!(
            actual,
            (
                case["dummy_coord"][0].as_i64().unwrap() as i32,
                case["dummy_coord"][1].as_i64().unwrap() as i32
            ),
            "{} dummy",
            case["name"]
        );
    }
    assert!(issues.is_empty(), "{}", issues.join("\n"));
}

fn fixture_rules() -> RuleSet {
    RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[BuildingTypes]\n0=building\n1=undeploy\n2=large_undeploy\n[VehicleTypes]\n0=nonbuilding\n[building]\nFoundation=1x1\n[undeploy]\nFoundation=1x1\nUndeploysInto=nonbuilding\n[large_undeploy]\nFoundation=2x2\nUndeploysInto=nonbuilding\n[nonbuilding]\nSpeed=5\n")).unwrap()
}
struct RecordingTerrain<'a> {
    inner: ResolvedBounceTerrain<'a>,
    events: std::cell::RefCell<Vec<Value>>,
}
impl RecordingTerrain<'_> {
    fn query(&self, address: &str, c: IVec3) {
        self.events
            .borrow_mut()
            .push(serde_json::json!([address, c.x, c.y, c.z]));
    }
}
impl<'a> BounceTerrain for RecordingTerrain<'a> {
    type Cell = CellRef<'a>;
    fn select_cell(&self, c: IVec3) -> Self::Cell {
        self.query("0x565730", c);
        self.inner.select_cell(c)
    }
    fn ground_height_leptons(&self, c: IVec3) -> i32 {
        self.query("0x578080", c);
        self.inner.ground_height_leptons(c)
    }
    fn selected_is_bridge(&self, c: &Self::Cell) -> bool {
        self.inner.selected_is_bridge(c)
    }
    fn cell_height_level(&self, c: IVec3) -> i32 {
        self.query("0x565730", c);
        self.inner.cell_height_level(c)
    }
    fn ramp(&self, c: IVec3) -> u8 {
        self.query("0x565730", c);
        self.inner.ramp(c)
    }
    fn is_water(&self, c: IVec3) -> bool {
        self.query("0x565730", c);
        self.inner.is_water(c)
    }
    fn has_bounce_surface(&self, c: &Self::Cell) -> bool {
        let (x, y, dummy, has_building) = match c {
            CellRef::Dummy { cell } => {
                let (x, y) = cell.snapshot().coord;
                (x, y, true, false)
            }
            CellRef::Real(cell) => {
                let has = self
                    .inner
                    .sim
                    .occupancy()
                    .get(cell.rx, cell.ry)
                    .is_some_and(|o| {
                        o.iter_layer(MovementLayer::Ground).any(|v| {
                            self.inner
                                .sim
                                .entities()
                                .get(v.entity_id)
                                .is_some_and(|e| e.category == EntityCategory::Structure)
                        })
                    });
                (i32::from(cell.rx), i32::from(cell.ry), false, has)
            }
        };
        self.events
            .borrow_mut()
            .push(serde_json::json!(["0x47c520", x, y, dummy]));
        if !has_building {
            self.events
                .borrow_mut()
                .push(serde_json::json!(["0x480510", x, y, dummy]));
        }
        self.inner.has_bounce_surface(c)
    }
}

#[test]
fn voxel_anim_logic_visit_delivers_native_deck_height_and_low_bridge_selection() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bounce_height.json"
    ))
    .unwrap();
    let rules = fixture_rules();
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[TIRE]\nElasticity=0.8\nMinZVel=28\nMaxZVel=32\nMaxXYVel=10\nDuration=150\n",
    );
    let kind = crate::rules::voxel_anim_type::VoxelAnimType::from_ini_section(
        "TIRE",
        ini.section("TIRE").unwrap(),
    );
    for name in [
        "deck_fall",
        "deck_rest",
        "low_wood_no_deck",
        "objects_undeploy",
        "objects_building",
    ] {
        let case = cases
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        let mut sim = fixture(case);
        let mut rng = crate::sim::rng::SimRng::new(17);
        let mut object = crate::sim::voxel_anim::spawn_debris_piece(
            0,
            crate::rules::voxel_anim_type::VoxelAnimTypeId(0),
            &kind,
            None,
            IVec3::ZERO,
            &mut rng,
        )
        .unwrap();
        object.bounce = body(case);
        sim.admit_death_debris(vec![crate::sim::voxel_anim::VoxelDebrisSpawn {
            type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId(0),
            object,
        }]);
        let id = sim.substrate.voxel_anims.ids()[0];
        assert!(sim.live_object_order_snapshot().contains(&id));
        let before = sim.state_hash();
        sim.visit_voxel_anim(id, Some(&rules));
        let actual = sim.substrate.voxel_anims.get(id).unwrap();
        assert_eq!(
            actual.duration,
            if case["outcome"].as_u64() == Some(2) {
                0
            } else {
                149
            },
            "{name} host stop duration"
        );
        for i in 0..3 {
            assert_eq!(
                actual.bounce.position[i].bits(),
                case["position_bits"][i].as_u64().unwrap() as u32,
                "{name} position"
            );
        }
        assert_ne!(before, sim.state_hash(), "{name} persistent result");
    }
}
