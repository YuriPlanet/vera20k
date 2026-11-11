//! Original enqueue/rebuild and connected caller goldens, not a Rust model.
//! Scope, fixture seams and reproduction: tools/spatial_oracle/ore_queue.md.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
use crate::sim::tiberium::{ReduceTiberiumContext, reduce_tiberium};
use crate::sim::world::Simulation;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn at(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap() as u16,
        value[1].as_u64().unwrap() as u16,
    )
}

fn types(input: &Value) -> (OverlayTypeRegistry, RuleSet) {
    let text = crate::sim::tiberium::test_support::tiberium_rules_text().replace(
        "[Tiberiums]\n0=Riparius\n1=Cruentus\n",
        "[Tiberiums]\n0=Riparius\n1=Cruentus\n2=Vinifera\n3=Aboreus\n",
    );
    let mut overrides = String::new();
    for (i, name) in ["Riparius", "Cruentus", "Vinifera", "Aboreus"]
        .iter()
        .enumerate()
    {
        overrides.push_str(&format!(
            "\n[{name}]\nImage={}\nGrowthPercentage={}\nSpreadPercentage={}\n",
            i + 1,
            input["growth_percentages"][i],
            input["spread_percentages"][i]
        ));
        for (key, field) in [("Growth", "growth_frames"), ("Spread", "spread_frames")] {
            if let Some(value) = input[field][i].as_i64() {
                overrides.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    // Declared native fixture land rows and visible blocker; these are supplied
    // state, not claims about retail rules or full object construction.
    overrides.push_str(&format!(
        "\n[Clear]\nBuildable=yes\n[Tiberium]\nBuildable=yes\n[Road]\nBuildable=no\n[BuildingTypes]\n0=BLOCK\n[BLOCK]\nStrength=900\nInvisible={}\nInvisibleInGame={}\n",
        input["building_invisible"].as_bool().unwrap_or(false),
        input["building_invisible_in_game"].as_bool().unwrap_or(false)
    ));
    let mut ini = IniFile::from_str(&text);
    ini.merge(&IniFile::from_str(&overrides));
    let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
    (
        OverlayTypeRegistry::from_ini(&ini, None),
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str("")).unwrap(),
    )
}

fn seed_queue(
    input: &Value,
    class: usize,
    name: &str,
    capacity: u32,
) -> (NativeTiberiumQueue, BTreeSet<(u16, u16)>) {
    if let Some(prepared) = input["queue_states"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|q| q["type_id"] == class && q["queue"] == name)
    {
        let entries = prepared["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let cell = at(&entry["cell"]);
                NativeTiberiumQueueEntry {
                    rx: cell.0,
                    ry: cell.1,
                    priority_bits: entry["priority_bits"].as_u64().unwrap() as u32,
                }
            })
            .collect();
        let heap = prepared["heap_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as u32)
            .collect();
        let bitmap = prepared["bitmap"]
            .as_array()
            .unwrap()
            .iter()
            .map(at)
            .collect();
        return (
            NativeTiberiumQueue {
                entries,
                heap: NativeHeap::from_native_slots(heap, capacity as usize),
            },
            bitmap,
        );
    }
    let selected = class == input["receiver"].as_u64().unwrap() as usize
        && name == input["queue"].as_str().unwrap();
    let count = if selected {
        input["pre_count"].as_u64().unwrap() as usize
    } else if class == input["receiver"].as_u64().unwrap() as usize {
        input["other_pre_counts"][name].as_u64().unwrap_or(1) as usize
    } else {
        1
    };
    let cell = at(&input["pre_cell"]);
    let entries = vec![
        NativeTiberiumQueueEntry {
            rx: cell.0,
            ry: cell.1,
            priority_bits: input["pre_priority_bits"].as_u64().unwrap() as u32
        };
        count
    ];
    let heap = if selected {
        input["pre_heap_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as u32)
            .collect()
    } else {
        vec![0]
    };
    let bitmap = if selected {
        input["pre_bitmap"]
            .as_array()
            .unwrap()
            .iter()
            .map(at)
            .collect()
    } else {
        BTreeSet::from([(2, 3)])
    };
    (
        NativeTiberiumQueue {
            entries,
            heap: NativeHeap::from_native_slots(heap, capacity as usize),
        },
        bitmap,
    )
}

fn queue_state(queue: &NativeTiberiumQueue, bitmap: &BTreeSet<(u16, u16)>) -> Value {
    json!({"array_count": queue.array_len(),
        "entries": queue.entries.iter().map(|e| json!({"cell":[e.rx,e.ry],"priority_bits":e.priority_bits})).collect::<Vec<_>>(),
        "heap_indices": queue.heap.as_slice(),
        "bitmap_cells": bitmap.iter().map(|&(x,y)| [x,y]).collect::<Vec<_>>()})
}

fn compare_queues(state: &OreGrowthState, expected: &Value, capacity: u32, name: &str) {
    assert_eq!(
        state.native_tiberium.classes.len(),
        expected["classes"].as_array().unwrap().len(),
        "{name}: class count"
    );
    for (i, class) in state.native_tiberium.classes.iter().enumerate() {
        let native = &expected["classes"][i];
        assert_eq!(native["type_id"], i, "{name}: class identity");
        for (kind, queue, bitmap) in [
            ("growth", &class.growth, &class.growth_bitmap),
            ("spread", &class.spread, &class.spread_bitmap),
        ] {
            assert_eq!(queue.capacity(), capacity, "{name}: {i} {kind} capacity");
            let mut expected_queue = native[kind].clone();
            expected_queue
                .as_object_mut()
                .unwrap()
                .remove("bitmap_indices");
            assert_eq!(
                queue_state(queue, bitmap),
                expected_queue,
                "{name}: class {i} {kind}"
            );
        }
        assert_eq!(
            json!([
                class.growth_timer.start_frame(),
                0,
                class.growth_timer.duration()
            ]),
            native["growth_timer"],
            "{name}: {i} growth timer"
        );
        assert_eq!(
            json!([
                class.spread_timer.start_frame(),
                0,
                class.spread_timer.duration()
            ]),
            native["spread_timer"],
            "{name}: {i} spread timer"
        );
    }
}

pub(crate) fn compare_state(
    state: &OreGrowthState,
    rng: &SimRng,
    expected: &Value,
    capacity: u32,
    name: &str,
) {
    assert_eq!(
        state.native_tiberium.classes.len(),
        4,
        "{name}: supplied class count"
    );
    compare_queues(state, expected, capacity, name);
    let view = rng.logical_view();
    assert_eq!(
        json!([view.index_a, view.index_b]),
        expected["rng_indices"],
        "{name}: RNG indices"
    );
    let hex = rng.native_state_hex();
    let bytes: Vec<_> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(
        crate::util::sha256::sha256_hex(&bytes),
        expected["rng_sha256"].as_str().unwrap(),
        "{name}: full RNG state"
    );
}

/// Declared native input state, shared by direct-owner and production-frame tests.
/// No expected output is used to prepare the Rust scene.
pub(crate) struct SuppliedFixture {
    pub(crate) registry: OverlayTypeRegistry,
    pub(crate) rules: RuleSet,
    pub(crate) grid: OverlayGrid,
    pub(crate) terrain: ResolvedTerrainGrid,
    pub(crate) state: OreGrowthState,
    pub(crate) rng: SimRng,
    pub(crate) entities: EntityStore,
    pub(crate) occupancy: OccupancyGrid,
    pub(crate) interner: StringInterner,
    pub(crate) trees: BTreeMap<(u16, u16), u64>,
    pub(crate) sources: BTreeSet<(u16, u16)>,
}

pub(crate) fn supplied_fixture(row: &Value) -> SuppliedFixture {
    let input = &row["input"];
    let name = input["name"].as_str().unwrap();
    let rect = at(&input["size"]);
    let capacity = row["capacity"].as_u64().unwrap() as u32;
    assert_eq!(
        native_tiberium_queue_capacity(rect),
        capacity,
        "{name}: map capacity"
    );
    let (registry, rules) = types(input);
    let types = &rules.tiberium_types;
    for (i, ty) in types.types().iter().enumerate() {
        for (field, actual) in [("growth_frames", ty.growth), ("spread_frames", ty.spread)] {
            if let Some(expected) = input[field][i].as_i64() {
                assert_eq!(i64::from(actual), expected, "{name}: {field} reader");
            }
        }
        assert_eq!(
            ty.growth_percentage_bits,
            input["growth_percentages"][i].as_f64().unwrap().to_bits(),
            "{name}: growth reader"
        );
        assert_eq!(
            ty.spread_percentage_bits,
            input["spread_percentages"][i].as_f64().unwrap().to_bits(),
            "{name}: spread reader"
        );
    }
    let mut grid = OverlayGrid::new(33, 33);
    // The shared native fixture's Ground table (0x89EA40, written by
    // tools/spatial_oracle/unit_source_scatter.py make_source_fixture) gives
    // Clear 1.0 for all nine SpeedTypes; the new Overlay's Mark admission
    // (0x004834A0, called at 0x005FD00C, or 0x005FD04F for TIB01) reads its
    // Track entry.
    let mut terrain = crate::map::resolved_terrain::test_grid(33, 33, |x, y| {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_loader_clear_cell(x, y)
        }
    });
    let natural = matches!(
        input["entry"].as_str().unwrap(),
        "spread_processor" | "spread_driver" | "growth_then_spread_driver"
    );
    if natural {
        let shape = NativeOverlayMapShape::new(i32::from(rect.0), i32::from(rect.1));
        let allocated: Vec<_> = shape
            .recalc_cells()
            .into_iter()
            .map(|(x, y)| (x as u16, y as u16))
            .collect();
        prepare_native_dummy(&terrain);
        for y in 0..33 {
            for x in 0..33 {
                terrain.cell_mut_for_test(x, y).unwrap().outside_playfield =
                    !shape.admits(x as i16, y as i16);
                terrain.cell_mut_for_test(x, y).unwrap().allows_tiberium =
                    input["tile_allows_tiberium"].as_bool().unwrap_or(true);
            }
        }
        terrain.test_set_native_allocated_cells(&allocated);
    }
    let mut occupancy = OccupancyGrid::new();
    for c in input["cells"].as_array().unwrap() {
        let cell = at(&c["cell"]);
        let kind = c["type_id"].as_u64().unwrap();
        let variant = c["variant"].as_u64().unwrap();
        let overlay = registry
            .id_for_name(&format!(
                "{}{:02}",
                if kind == 0 { "TIB" } else { "GEM" },
                variant + 1
            ))
            .unwrap();
        grid.place_overlay(
            cell.0,
            cell.1,
            overlay,
            c["density"].as_u64().unwrap() as u8,
        );
        if let Some(raw) = c["overlay"].as_i64() {
            grid.cell_mut_for_test(cell.0, cell.1).overlay_id = u8::try_from(raw).ok();
        }
        if natural {
            grid.recalculate_runtime_cell(&mut terrain, &registry, cell);
        }
        if let Some(land) = c["land"].as_u64() {
            terrain.cell_mut_for_test(cell.0, cell.1).unwrap().land_type = land as u8;
        }
        terrain
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap()
            .slope_type = c["slope"].as_u64().unwrap() as u8;
        if c["occupied"] == true {
            occupancy.add(
                cell.0,
                cell.1,
                100,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
    }
    let mut state = OreGrowthState::new(33, 33);
    state.reset_native_tiberium_classes_for_rect(rect, 4, 0);
    for i in 0..4 {
        let (growth, growth_bitmap) = seed_queue(input, i, "growth", capacity);
        let (spread, spread_bitmap) = seed_queue(input, i, "spread", capacity);
        let class = &mut state.native_tiberium.classes[i];
        class.growth = growth;
        class.growth_bitmap = growth_bitmap;
        class.spread = spread;
        class.spread_bitmap = spread_bitmap;
        class.growth_timer =
            timer_from_input(input, "growth_timers", i, 21 + i as i32, 300 + i as i32);
        class.spread_timer =
            timer_from_input(input, "spread_timers", i, 11 + i as i32, 200 + i as i32);
    }
    let mut rng = SimRng::new(input["seed"].as_u64().unwrap());
    if let Some(raw) = input["next_raw"].as_u64() {
        let view = rng.logical_view();
        let (a, b) = (view.index_a as usize, view.index_b as usize);
        let word = raw as u32 ^ view.words[b];
        let mut saved = serde_json::to_value(&rng).unwrap();
        saved["state"][a] = json!(word);
        rng = serde_json::from_value(saved).unwrap();
    }
    let mut sources = BTreeSet::new();
    let mut trees = BTreeMap::new();
    let mut entities = EntityStore::new();
    let mut interner = StringInterner::new();
    for (i, blocked) in input["blocked"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let cell = at(blocked);
        match blocked[2].as_str().unwrap() {
            "overlay" => grid.place_overlay(cell.0, cell.1, 7, 0),
            "land" => {
                let c = terrain.cell_mut_for_test(cell.0, cell.1).unwrap();
                c.land_type = 1;
                c.base_build_blocked = true;
            }
            "slope" => {
                terrain
                    .cell_mut_for_test(cell.0, cell.1)
                    .unwrap()
                    .slope_type = 1
            }
            "bridge" => {
                terrain
                    .cell_mut_for_test(cell.0, cell.1)
                    .unwrap()
                    .bridge_facts
                    .raw_flags = 0x100
            }
            "tree" | "ordinary_tree" => {
                trees.insert(cell, 1000 + i as u64);
                if blocked[2] == "tree" {
                    sources.insert(cell);
                }
            }
            "building" => {
                let id = 1000 + i as u64;
                let owner = interner.intern("OWNER");
                let typ = interner.intern("BLOCK");
                entities.insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    cell.0,
                    cell.1,
                    0,
                    0,
                    owner,
                    Health::new(input["building_health"].as_i64().unwrap_or(900) as i32),
                    typ,
                    EntityCategory::Structure,
                    0,
                    0,
                    false,
                ));
                occupancy.add(
                    cell.0,
                    cell.1,
                    id,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::PrependNonBuilding,
                );
            }
            other => panic!("unhandled native blocker {other}"),
        }
    }
    for (i, unit) in input["units"].as_array().into_iter().flatten().enumerate() {
        let cell = at(unit);
        let id = 2000 + i as u64;
        let owner = interner.intern("OWNER");
        let typ = interner.intern("UNIT");
        entities.insert(GameEntity::new_at_frame_zero_for_test(
            id,
            cell.0,
            cell.1,
            0,
            0,
            owner,
            Health::new(900),
            typ,
            EntityCategory::Unit,
            0,
            0,
            false,
        ));
        occupancy.add(
            cell.0,
            cell.1,
            id,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }
    // These stamps describe initial native map state, not pending runtime Mark
    // callbacks. Later mutations keep their real owner-produced dirty list.
    grid.take_dirty_cells();
    SuppliedFixture {
        registry,
        rules,
        grid,
        terrain,
        state,
        rng,
        entities,
        occupancy,
        interner,
        trees,
        sources,
    }
}

fn compare_case(row: &Value) {
    let input = &row["input"];
    let name = input["name"].as_str().unwrap();
    let capacity = row["capacity"].as_u64().unwrap() as u32;
    let SuppliedFixture {
        registry,
        rules,
        grid,
        terrain,
        state,
        rng,
        entities,
        occupancy,
        interner,
        trees,
        sources,
    } = supplied_fixture(row);
    let types = &rules.tiberium_types;
    let mut constructors = Simulation::with_seed(0);
    constructors.native_unique_ids = Some(
        crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(
            input["scenario_serial"].as_u64().unwrap_or(0) as u32,
        ),
    );
    constructors.overlay_grid = Some(grid);
    constructors.resolved_terrain = Some(terrain);
    constructors.ore_growth_state = state;
    constructors.scenario_rng = rng;
    constructors.substrate.entities = entities;
    constructors.substrate.occupancy = occupancy;
    constructors.interner = interner;
    *constructors.terrain_objects.cell_index_mut_for_test() = trees;
    *constructors.terrain_objects.spawning_cells_mut_for_test() = sources;
    constructors.ore_growth_config = OreGrowthConfig::for_test(
        input["grows"] == true,
        input["spreads"] == true,
        input["fast_growth"] == true,
    );
    compare_overlay_state(&constructors, &row["before"], name);
    compare_dummy(
        constructors.resolved_terrain.as_ref().unwrap(),
        &row["before"],
        name,
    );
    compare_state(
        &constructors.ore_growth_state,
        &constructors.scenario_rng,
        &row["before"],
        capacity,
        name,
    );
    let receiver = TiberiumTypeId(input["receiver"].as_u64().unwrap() as u8);
    let cell = at(&input["target"]);
    let frame = input["frame"].as_i64().unwrap() as u32;
    let frames = input["frames"].as_array().map_or_else(
        || vec![frame],
        |frames| {
            frames
                .iter()
                .map(|frame| frame.as_i64().unwrap() as u32)
                .collect()
        },
    );
    let mut total_draws = 0;
    for (step, frame) in frames.into_iter().enumerate() {
        constructors.session.set_binary_frame_for_test(frame);
        let (_, draws) = crate::sim::rng::trace_draws(|| match input["entry"].as_str().unwrap() {
            "enqueue" if input["queue"] == "growth" => {
                constructors
                    .ore_growth_state
                    .add_native_growth_queue_cell(
                        constructors.overlay_grid.as_ref().unwrap(),
                        &registry,
                        types,
                        constructors.resolved_terrain.as_ref(),
                        input["grows"] == true,
                        receiver,
                        cell.0,
                        cell.1,
                        frame,
                        &mut constructors.scenario_rng,
                    )
                    .unwrap();
            }
            "enqueue" => {
                constructors
                    .ore_growth_state
                    .add_native_spread_queue_cell(
                        receiver,
                        constructors.overlay_grid.as_ref().unwrap(),
                        &registry,
                        types,
                        constructors.resolved_terrain.as_ref(),
                        constructors.terrain_objects.spawning_cells(),
                        Some(NativeCellObjectView::new(
                            &constructors.substrate.occupancy,
                            constructors.terrain_objects.cell_index(),
                        )),
                        cell.0,
                        cell.1,
                        frame,
                        input["spreads"] == true,
                        &mut constructors.scenario_rng,
                    )
                    .unwrap();
            }
            "reduce" => {
                let result = reduce_tiberium(
                    &mut ReduceTiberiumContext {
                        overlay_grid: constructors.overlay_grid.as_mut(),
                        ore_growth_state: &mut constructors.ore_growth_state,
                        overlay_registry: Some(&registry),
                        tiberium_types: Some(types),
                        resolved_terrain: constructors.resolved_terrain.as_mut(),
                        source_object_cells: Some(constructors.terrain_objects.spawning_cells()),
                        live_objects: Some(NativeCellObjectView::new(
                            &constructors.substrate.occupancy,
                            constructors.terrain_objects.cell_index(),
                        )),
                        rng: Some(&mut constructors.scenario_rng),
                        binary_frame: frame,
                        spread_enabled: input["spreads"] == true,
                        dirty_cells: None,
                    },
                    cell,
                    input["amount"].as_i64().unwrap_or(1) as i32,
                )
                .unwrap();
                assert_eq!(
                    json!(result.removed_amount),
                    row["returned"],
                    "{name}: reduced amount"
                );
            }
            "growth_processor" => {
                constructors
                    .process_native_growth_for_type_with_placement(
                        receiver,
                        &rules,
                        &registry,
                        FrameEffects::default(),
                    )
                    .unwrap();
            }
            "spread_processor" => {
                constructors
                    .process_native_spread_for_type_with_placement(
                        receiver,
                        &rules,
                        &registry,
                        FrameEffects::default(),
                    )
                    .unwrap();
            }
            "spread_driver" | "growth_then_spread_driver" => {
                if input["entry"] == "growth_then_spread_driver" {
                    constructors
                        .tick_native_growth_driver(&rules, &registry, FrameEffects::default())
                        .unwrap();
                }
                constructors
                    .tick_native_spread_driver(&rules, &registry, FrameEffects::default())
                    .unwrap();
            }
            other => panic!("unhandled native entry {other}"),
        });
        if matches!(
            input["entry"].as_str().unwrap(),
            "spread_processor" | "spread_driver" | "growth_then_spread_driver"
        ) {
            let changed = constructors
                .overlay_grid
                .as_mut()
                .unwrap()
                .take_dirty_cells();
            constructors.publish_tiberium_cells(&rules, &registry, &changed);
        }
        total_draws += draws.len();
        if let Some(expected) = row["steps"].get(step) {
            compare_dummy(
                constructors.resolved_terrain.as_ref().unwrap(),
                &expected["state"],
                name,
            );
            compare_overlay_state(&constructors, &expected["state"], name);
            assert_eq!(
                json!(draws.len()),
                expected["draw_count"],
                "{name}: step {step} draws"
            );
            compare_state(
                &constructors.ore_growth_state,
                &constructors.scenario_rng,
                &expected["state"],
                capacity,
                &format!("{name}: step {step}"),
            );
            compare_cells(
                constructors.overlay_grid.as_ref().unwrap(),
                constructors.resolved_terrain.as_ref().unwrap(),
                &expected["cells"],
                name,
            );
        }
        if let Some(expected) = row["after_drains"].get(step) {
            compare_dummy(
                constructors.resolved_terrain.as_ref().unwrap(),
                &expected["state"],
                name,
            );
            constructors.process_pending_delete_with(
                crate::rules::ruleset::RuleSet::empty_for_test(),
                None,
                crate::sim::world::FrameEffects::default(),
                1,
                false,
            );
            compare_overlay_state(&constructors, &expected["state"], name);
            compare_cells(
                constructors.overlay_grid.as_ref().unwrap(),
                constructors.resolved_terrain.as_ref().unwrap(),
                &expected["cells"],
                name,
            );
        }
    }
    assert_eq!(json!(total_draws), row["draw_count"], "{name}: RNG draws");
    compare_state(
        &constructors.ore_growth_state,
        &constructors.scenario_rng,
        &row["state"],
        capacity,
        name,
    );
    compare_overlay_state(&constructors, &row["state"], name);
    compare_dummy(
        constructors.resolved_terrain.as_ref().unwrap(),
        &row["state"],
        name,
    );
    compare_cells(
        constructors.overlay_grid.as_ref().unwrap(),
        constructors.resolved_terrain.as_ref().unwrap(),
        &row["cells"],
        name,
    );
    let next: Vec<_> = (0..4)
        .map(|_| constructors.scenario_rng.next_u32())
        .collect();
    assert_eq!(json!(next), row["next_random"], "{name}: RNG continuation");
}

/// The original fixture maps zeroed resident dummy bytes without executing
/// Scenario startup. Supply that same declared input, not retail ctor defaults.
pub(crate) fn prepare_native_dummy(terrain: &ResolvedTerrainGrid) {
    let dummy = terrain.shared_cell_dummy();
    dummy.stamp_coord(0, 0);
    dummy.write_overlay_identity_state(0, 0);
}

pub(crate) fn compare_dummy(terrain: &ResolvedTerrainGrid, expected: &Value, name: &str) {
    let Some(expected) = expected.get("dummy") else {
        return;
    };
    let dummy = terrain.shared_cell_dummy();
    let snapshot = dummy.snapshot();
    let (overlay, density) = dummy.overlay_identity_state();
    assert_eq!(
        json!({
            "cell": [snapshot.coord.0, snapshot.coord.1],
            "overlay": overlay,
            "density": density,
            "land": dummy.land_type(),
            "level": snapshot.level,
            "slope": snapshot.slope_type,
            "flags": dummy.raw_flags(),
        }),
        *expected,
        "{name}: retained native dummy"
    );
}

pub(crate) fn compare_overlay_state(sim: &Simulation, expected: &Value, name: &str) {
    if expected["scenario_serial"].is_null() {
        return;
    }
    assert_eq!(
        json!(sim.native_unique_ids.as_ref().unwrap().current_raw()),
        expected["scenario_serial"],
        "{name}: Scenario constructor ID continuation",
    );
    assert_eq!(
        json!(sim.load_objects.registry_counts()[4]),
        expected["overlay_registrations"],
        "{name}: registered Overlay constructors",
    );
    assert_eq!(
        json!(sim.substrate.pending_delete.overlay_count()),
        expected["pending_deletes"],
        "{name}: deferred Overlay retirement",
    );
}

fn timer_from_input(input: &Value, key: &str, class: usize, start: i32, duration: i32) -> CdTimer {
    input[key].get(class).map_or_else(
        || CdTimer::from_raw(start, duration),
        |timer| {
            assert_eq!(timer[1], 0, "native fixture timer padding");
            CdTimer::from_raw(
                timer[0].as_i64().unwrap() as i32,
                timer[2].as_i64().unwrap() as i32,
            )
        },
    )
}

pub(crate) fn compare_cells(
    grid: &OverlayGrid,
    terrain: &ResolvedTerrainGrid,
    cells: &Value,
    name: &str,
) {
    for c in cells.as_array().unwrap() {
        let cell = at(&c["cell"]);
        let overlay = grid.cell(cell.0, cell.1);
        assert_eq!(
            json!(overlay.overlay_id.map_or(-1, i32::from)),
            c["overlay"],
            "{name}: cell {cell:?} overlay"
        );
        assert_eq!(
            json!(overlay.overlay_data),
            c["density"],
            "{name}: cell {cell:?} density"
        );
        if let Some(land) = c["land"].as_i64() {
            assert_eq!(
                i64::from(terrain.cell(cell.0, cell.1).unwrap().land_type),
                land,
                "{name}: cell {cell:?} land"
            );
        }
    }
}

pub(crate) fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/ore_queue.json")
}

#[test]
fn enqueue_rebuilds_and_callers_match_native_queue_arrays_heaps_bitmaps_timers_and_rng() {
    let corpus = corpus();
    let rows: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            matches!(
                row["input"]["entry"].as_str().unwrap(),
                "enqueue" | "reduce" | "growth_processor"
            )
        })
        .collect();
    assert_eq!(rows.len(), 60, "native enqueue/caller coverage");
    for row in rows {
        compare_case(row);
    }
}

#[test]
fn natural_spread_processors_and_drivers_match_native_cells_queues_timers_and_rng() {
    let corpus = corpus();
    let rows: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            matches!(
                row["input"]["entry"].as_str().unwrap(),
                "spread_processor" | "spread_driver" | "growth_then_spread_driver"
            )
        })
        .collect();
    assert_eq!(rows.len(), 68, "native natural-spread coverage");
    for row in rows {
        compare_case(row);
    }
}

#[test]
fn same_frame_ore_edge_probes_update_retained_dummy_when_source_refuses() {
    let corpus = corpus();
    let rows: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            matches!(
                row["input"]["name"].as_str().unwrap(),
                "spread_processor_source_on_diamond_edge_occupied"
                    | "spread_processor_source_on_diamond_edge_spreads_disabled"
            )
        })
        .collect();
    assert_eq!(rows.len(), 2, "native no-placement edge controls");
    for row in rows {
        compare_case(row);
    }
}

#[test]
fn natural_spread_dense_hole_germination_controls_growth_admission_and_rng() {
    let corpus = corpus();
    let rows: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["input"]["name"]
                .as_str()
                .unwrap()
                .starts_with("spread_processor_interior_hole_")
        })
        .collect();
    assert_eq!(rows.len(), 3, "native eight-versus-seven-neighbor controls");
    for row in rows {
        compare_case(row);
    }
}

#[test]
fn native_due_spread_driver_runs_when_special_spreads_is_off() {
    let corpus = corpus();
    let rows: Vec<_> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["input"]["entry"] == "spread_driver"
                && row["input"]["grows"] == true
                && row["input"]["spreads"] == false
        })
        .collect();
    assert!(!rows.is_empty(), "native spread-off driver controls");
    for row in rows {
        compare_case(row);
    }
}

/// Run the production ore owners from the already loaded physical queue state
/// with the original control's independent seed. This excludes other World AI;
/// the ordinary frame below separately checks publication and real retirement.
fn compare_authored_natural_phases(
    scene: &crate::headless_scenario::HeadlessScenario,
    capacity: u32,
) {
    use crate::sim::rng::MainRng;

    let native = crate::test_fixture::json("tools/spatial_oracle/ore_authored_natural.json");
    let sim = scene.sim();
    let hash = sim.state_hash();
    let rules = &scene.runtime.resources.rules;
    let registry = &scene.runtime.resources.overlay_registry;
    let mut actual = Simulation::with_seed(0);
    actual.ore_growth_state = sim.ore_growth_state.clone();
    actual.overlay_grid = sim.overlay_grid.clone();
    let mut terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    terrain.bind_shared_cell_dummy(terrain.shared_cell_dummy().fork_query_identity());
    actual.resolved_terrain = Some(terrain);
    actual.substrate.entities = sim.substrate.entities.clone();
    actual.substrate.occupancy = sim.substrate.occupancy.clone();
    actual.substrate.raw_cell_occupation = sim.substrate.raw_cell_occupation.clone();
    actual.interner = sim.interner.clone();
    *actual.terrain_objects.cell_index_mut_for_test() = sim.terrain_objects.cell_index().clone();
    *actual.terrain_objects.spawning_cells_mut_for_test() =
        sim.terrain_objects.spawning_cells().clone();
    let seed = native["inputs"]["seed"].as_u64().unwrap();
    actual.scenario_rng = SimRng::new(seed);
    let main = MainRng::new(seed);
    let mapgen = SimRng::new(seed);
    actual.ore_growth_config = sim.ore_growth_config.clone();
    let config = &sim.ore_growth_config;
    assert_eq!(
        config.grows,
        native["inputs"]["session"]["basic_growth_enabled"]
            .as_bool()
            .unwrap()
    );
    assert!(config.spreads && config.tiberium_grows_flag);
    compare_queues(
        &actual.ore_growth_state,
        &native["initial"],
        capacity,
        "physical natural initial",
    );
    let streams = |rng: &SimRng| {
        json!({
            "scenario": rng.native_state_hex(), "main": main.native_state_hex(),
            "mapgen": mapgen.native_state_hex(),
        })
    };
    assert_eq!(
        streams(&actual.scenario_rng),
        native["initial"]["rng_states"]
    );
    let steps = native["steps"].as_array().unwrap();
    let last = steps.last().unwrap()["frame"].as_u64().unwrap() as u32;
    let mut previous_cells = actual.overlay_grid.as_ref().unwrap().cells().to_vec();
    let mut construction_count = 0;
    let mut native_cursor = native["initial"]["runtime_id_cursor"].as_u64().unwrap();
    actual.native_unique_ids = Some(
        crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(
            native_cursor as u32,
        ),
    );
    for frame in 0..=last {
        let expected_step = steps
            .iter()
            .find(|step| step["frame"].as_u64() == Some(u64::from(frame)));
        if let Some(step) = expected_step {
            for change in step["changed"].as_array().unwrap() {
                let before = &change["before"];
                let cell = at(&before["cell"]);
                let overlay = actual.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
                let resolved = actual
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(cell.0, cell.1)
                    .unwrap();
                assert_eq!(
                    json!(overlay.overlay_id.map(i32::from).unwrap_or(-1)),
                    before["overlay"]
                );
                assert_eq!(json!(overlay.overlay_data), before["density"]);
                assert_eq!(json!(resolved.level), before["level"]);
                assert_eq!(json!(resolved.slope_type), before["slope"]);
                assert_eq!(
                    json!(
                        NativeCellObjectView::new(
                            &actual.substrate.occupancy,
                            actual.terrain_objects.cell_index()
                        )
                        .ground_object_present(cell)
                    ),
                    before["has_ground_object"],
                    "native source/target object prior at {cell:?}"
                );
            }
        }
        actual.session.set_binary_frame_for_test(frame);
        let ((growth, spread), draws) = crate::sim::rng::trace_draws(|| {
            let growth = actual
                .tick_native_growth_driver(rules, registry, FrameEffects::default())
                .unwrap();
            let spread = actual
                .tick_native_spread_driver(rules, registry, FrameEffects::default())
                .unwrap();
            (growth, spread)
        });
        let changed = actual.overlay_grid.as_mut().unwrap().take_dirty_cells();
        actual.publish_tiberium_cells(rules, registry, &changed);
        let Some(step) = expected_step else {
            assert!(draws.is_empty(), "native no-op frame {frame}");
            assert_eq!(growth.processor_calls, 0);
            assert_eq!(spread.processor_calls, 0);
            continue;
        };
        let expected = step.get("after").unwrap_or_else(|| {
            &steps
                .iter()
                .find(|prior| prior["frame"] == step["same_after_as_frame"])
                .unwrap()["after"]
        });
        let name = format!("physical natural frame {frame}");
        compare_queues(&actual.ore_growth_state, expected, capacity, &name);
        assert_eq!(
            streams(&actual.scenario_rng),
            expected["rng_states"],
            "{name}: all streams"
        );
        let native_draws: Vec<_> = step["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "random")
            .map(|event| &event["raw"])
            .collect();
        let rust_draws: Vec<_> = draws.iter().map(|draw| &draw["value"]).collect();
        assert_eq!(rust_draws, native_draws, "{name}: raw draw order");
        assert_eq!(json!(draws.len()), step["draw_count"]);
        let actual_cursor = actual.native_unique_ids.as_ref().unwrap().current_raw() as u64;
        let constructed = actual_cursor.wrapping_sub(native_cursor) as usize;
        construction_count += constructed;
        let cursor = expected["runtime_id_cursor"].as_u64().unwrap();
        assert_eq!(actual_cursor, cursor, "{name}: constructor cursor");
        let pending = actual
            .load_objects
            .retained_overlay_ids()
            .filter(|id| actual.substrate.pending_delete.contains_overlay(*id))
            .count();
        assert_eq!(json!(pending), step["pending_before_drain"]);
        native_cursor = cursor;
        let actual_changes: BTreeMap<_, _> = actual
            .overlay_grid
            .as_ref()
            .unwrap()
            .cells()
            .iter()
            .zip(&previous_cells)
            .enumerate()
            .filter(|(_, (after, before))| after != before)
            .map(|(index, (after, _))| {
                let cell = (
                    (index % usize::from(actual.overlay_grid.as_ref().unwrap().width())) as u16,
                    (index / usize::from(actual.overlay_grid.as_ref().unwrap().width())) as u16,
                );
                let value = crate::sim::tiberium::tiberium_cell_view(
                    actual.overlay_grid.as_ref().unwrap(),
                    registry,
                    &rules.tiberium_types,
                    cell,
                )
                .unwrap()
                .map_or(0, |resource| resource.nominal_value);
                (
                    cell,
                    json!({"overlay": after.overlay_id.map(i32::from).unwrap_or(-1),
                    "density": after.overlay_data, "value": value}),
                )
            })
            .collect();
        let expected_changes: BTreeMap<_, _> = step["changed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| {
                let after = &change["after"];
                (
                    at(&after["cell"]),
                    json!({"overlay": after["overlay"],
                    "density": after["density"], "value": after["value"]}),
                )
            })
            .collect();
        assert_eq!(
            actual_changes, expected_changes,
            "{name}: every changed cell"
        );
        previous_cells.clone_from_slice(&actual.overlay_grid.as_ref().unwrap().cells().to_vec());
        actual.process_pending_delete_with(
            rules,
            Some(registry),
            FrameEffects::default(),
            1,
            false,
        );
    }
    assert_eq!(json!(construction_count), native["constructed_overlays"]);
    assert_eq!(
        sim.state_hash(),
        hash,
        "component fork leaves ordinary World input untouched"
    );
}

/// Normal authored load, not the supplied-state oracle fixture above. Native
/// Logic55B4D7/55B4DC runs growth then spread; Cell483780 places amount3 through
/// 487190 and Mark5FC570. Numeric reload controls remain ore_queue.json's original
/// execution. This does not claim native whole-map startup/RNG equivalence.
#[test]
fn retail_authored_first_ore_frame_publishes_cells_and_retires_transient_overlays() {
    use crate::sim::world::{DeferredDeleteTicket, FrameEffects, LifecycleTestEvent, TickLane};

    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let native = crate::test_fixture::json("tools/spatial_oracle/ore_authored_init.json");
    for input in native["inputs"]["manifest"].as_array().unwrap() {
        let name = input["name"].as_str().unwrap();
        let selected = assets.resolve_ref(name);
        if input["absent"].as_bool().unwrap_or(false) {
            assert!(selected.is_none(), "actual optional asset {name}");
        } else {
            let selected = selected.unwrap_or_else(|| panic!("physical asset {name}"));
            assert_eq!(json!(selected.bytes.len()), input["bytes"], "{name} length");
            assert_eq!(
                json!(crate::util::sha256::sha256_hex(selected.bytes)),
                input["sha256"],
                "actual selected input {name} matches native reader bytes"
            );
        }
    }
    // This checked-in ordinary profile has the same launch as the command-free
    // XMP L0/L1 exploration. Its selected cells are
    // integration witnesses; observed Rust variant bytes are not native goldens.
    let profile = crate::test_fixture::json("tools/map_observation.ore-spread.example.json");
    assert_eq!(profile["ticks"], 1);
    assert!(profile["commands"].as_array().unwrap().is_empty());
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map_name = launch.selected_map_file.as_deref().unwrap().to_owned();
    let selected_map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
            .unwrap();
    assert_eq!(
        serde_json::to_value(selected_map.source).unwrap()["source_sha256"],
        native["inputs"]["gates"]["map_sha256"],
        "actual map source, including loose-file precedence"
    );
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map_name,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    let witnesses = [(71, 24), (72, 24), (73, 24), (71, 25), (71, 26)];
    let initial_frame = scene.sim().session.binary_frame();
    assert_eq!(initial_frame, 0);
    let rules = &scene.runtime.resources.rules;
    assert_eq!(
        rules.tiberium_types.len(),
        native["inputs"]["effective_types"]
            .as_array()
            .unwrap()
            .len()
    );
    for (ty, expected) in rules
        .tiberium_types
        .types()
        .iter()
        .zip(native["inputs"]["effective_types"].as_array().unwrap())
    {
        assert_eq!(
            json!({
                "name": ty.section, "type_id": ty.id.0, "growth": ty.growth,
                "spread": ty.spread, "value": ty.value, "max_density": ty.image().density_levels(),
                "growth_percentage_bits": format!("{:016x}", ty.growth_percentage_bits),
                "spread_percentage_bits": format!("{:016x}", ty.spread_percentage_bits),
            }),
            json!({
                "name": expected["name"], "type_id": expected["type_id"],
                "growth": expected["growth"], "spread": expected["spread"],
                "value": expected["value"], "max_density": expected["max_density"],
                "growth_percentage_bits": expected["growth_percentage_bits"],
                "spread_percentage_bits": expected["spread_percentage_bits"],
            }),
            "actual layered Tiberium readers"
        );
        let variants = scene
            .runtime
            .resources
            .overlay_registry
            .flat_tiberium_variant_ids(ty)
            .unwrap();
        assert_eq!(json!(variants[0]), expected["base_overlay"]);
        assert_eq!(json!(variants.len()), expected["images"]);
    }
    for expected in native["inputs"]["flat_overlay_types"].as_array().unwrap() {
        let id = expected["index"].as_u64().unwrap() as u8;
        let registry = &scene.runtime.resources.overlay_registry;
        assert_eq!(json!(registry.name(id).unwrap()), expected["name"]);
        assert_eq!(
            json!(registry.flags(id).unwrap().cell_anim.is_none()),
            expected["cell_anim_null"]
        );
    }
    let riparius = rules.tiberium_types.id_by_name("Riparius").unwrap();
    assert!(scene.sim().ore_growth_config.grows);
    assert!(scene.sim().ore_growth_config.spreads);
    assert!(scene.sim().ore_growth_config.tiberium_grows_flag);

    let state = &scene.sim().ore_growth_state;
    assert_eq!(
        json!([state.native_rect().0, state.native_rect().1]),
        native["native_rect"]
    );
    compare_queues(
        state,
        &native["state"],
        native["capacity"].as_u64().unwrap() as u32,
        "normal authored initialization",
    );
    let objects = TiberiumPlacementObjectContext::new(
        scene.sim().entities(),
        scene.sim().occupancy(),
        &scene.sim().substrate.raw_cell_occupation,
        rules,
        &scene.sim().interner,
        scene.sim().terrain_objects.cell_index(),
    );
    let occupied = objects.object_view().occupied_ground_cells().collect();
    let admission =
        NewTiberiumAdmission::runtime(scene.sim().resolved_terrain.as_ref().unwrap(), objects);
    for expected in native["patch"].as_array().unwrap() {
        let (rx, ry) = at(&expected["cell"]);
        let cell = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(rx, ry)
            .unwrap();
        let overlay = scene.sim().overlay_grid.as_ref().unwrap().cell(rx, ry);
        assert_eq!(json!(!cell.outside_playfield), expected["playfield"]);
        assert_eq!(
            json!(
                rules
                    .terrain_rules
                    .semantics_for_land_type(cell.land_type)
                    .unwrap()
                    .buildable
            ),
            expected["land_buildable"]
        );
        assert_eq!(
            json!(objects.object_view().ground_object_present((rx, ry))),
            expected["has_ground_object"]
        );
        assert_eq!(
            json!(
                admit_new_tiberium_target(
                    scene.sim().overlay_grid.as_ref().unwrap(),
                    &occupied,
                    admission,
                    (rx, ry)
                )
                .is_some()
            ),
            expected["can_place_riparius"],
            "original CanPlace4838E0 at {rx},{ry}"
        );
        assert_eq!(
            json!({
                "tile": cell.final_tile_index, "subtile": cell.final_sub_tile,
                "level": cell.level, "slope": cell.slope_type, "land": cell.land_type,
                "overlay": overlay.overlay_id.map(i32::from).unwrap_or(-1),
                "density": overlay.overlay_data, "tile_allow_tiberium": cell.allows_tiberium,
            }),
            json!({
                "tile": expected["tile"], "subtile": expected["subtile"],
                "level": expected["level"], "slope": expected["slope"], "land": expected["land"],
                "overlay": expected["overlay"], "density": expected["density"],
                "tile_allow_tiberium": expected["tile_allow_tiberium"],
            }),
            "native selected source/target prerequisites at {rx},{ry}"
        );
        let value = crate::sim::tiberium::tiberium_cell_view(
            scene.sim().overlay_grid.as_ref().unwrap(),
            &scene.runtime.resources.overlay_registry,
            &rules.tiberium_types,
            (rx, ry),
        )
        .unwrap()
        .map_or(0, |resource| resource.nominal_value);
        assert_eq!(
            json!(value),
            expected["value"],
            "original GetValue485020 at {rx},{ry}"
        );
    }
    assert_eq!(
        state.native_rect(),
        (
            scene.map.header.width as u16,
            scene.map.header.height as u16
        )
    );
    compare_authored_natural_phases(&scene, native["capacity"].as_u64().unwrap() as u32);
    let class = &state.native_tiberium_state().classes[riparius.0 as usize];
    // Queue initialization precedes the final InitCellAttributes(0) and
    // Technos; never reconstruct its expected membership from final L0 cells.
    let initial_growth = queue_state(&class.growth, &class.growth_bitmap);
    let initial_spread = queue_state(&class.spread, &class.spread_bitmap);
    let initial_pending: BTreeSet<_> = scene
        .sim()
        .substrate
        .pending_delete
        .retained_tickets()
        .collect();
    let initial_overlays: BTreeSet<_> = scene.sim().load_objects.retained_overlay_ids().collect();
    let event_start = scene.sim().lifecycle_test_events_for_test().len();
    let serial_before = scene
        .sim()
        .native_unique_ids
        .as_ref()
        .unwrap()
        .current_raw();
    let rng_before = scene.sim().scenario_rng.native_state_hex();
    for &(rx, ry) in &witnesses {
        assert!(
            scene
                .sim()
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(rx, ry)
                .overlay_id
                .is_none()
        );
    }

    // The capture's game-speed1 clock is22ms. The generic HeadlessScenario
    // helper defaults to66ms; use the actual profile clock explicitly here.
    let (output, draws) = crate::sim::rng::trace_draws(|| {
        scene
            .runtime
            .advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap()
    });
    assert!(output.tick.frame_committed);
    assert_eq!(scene.sim().session.binary_frame(), initial_frame + 1);
    assert_ne!(scene.sim().scenario_rng.native_state_hex(), rng_before);
    assert!(!draws.is_empty());

    let rules = &scene.runtime.resources.rules;
    let registry = &scene.runtime.resources.overlay_registry;
    let grid = scene.sim().overlay_grid.as_ref().unwrap();
    let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
    // The native initialization packet's first source is the first growth
    // root. Observe its ordinary Grow483710 effect as well as new-cell spread.
    let source = &native["patch"][0];
    let source_cell = at(&source["cell"]);
    let grown_source = crate::sim::tiberium::tiberium_cell_view(
        grid,
        registry,
        &rules.tiberium_types,
        source_cell,
    )
    .unwrap()
    .unwrap();
    assert!(u64::from(grown_source.overlay_data) > source["density"].as_u64().unwrap());
    assert!(i64::from(grown_source.nominal_value) > source["value"].as_i64().unwrap());
    let radar_cells = scene.runtime.simulation.radar_terrain_dirty_cells();
    for &(rx, ry) in &witnesses {
        let resource = crate::sim::tiberium::tiberium_cell_view(
            grid,
            registry,
            &rules.tiberium_types,
            (rx, ry),
        )
        .unwrap()
        .expect("ordinary natural spread produces an actual resource cell");
        assert_eq!(resource.tiberium_type, riparius);
        assert_eq!(resource.overlay_data, 3, "native483780's placement amount");
        assert_eq!(resource.resource_type, crate::sim::miner::ResourceType::Ore);
        assert!(
            resource.nominal_value > 0,
            "new field is valuable to the production miner consumer"
        );
        let cell = terrain.cell(rx, ry).unwrap();
        assert_eq!(
            cell.land_type,
            crate::rules::terrain_rules::LandType::Tiberium.as_index()
        );
        assert!(scene.sim().path_grid().unwrap().resolved_cell_is_current(
            cell,
            scene.sim().overlay_grid.as_ref(),
            false
        ));
        assert!(
            scene
                .sim()
                .dirty_cells
                .tactical_for_test()
                .contains(&(rx, ry))
        );
        assert!(radar_cells.contains(&(rx, ry)));
        assert!(
            output.overlay_updates.iter().any(|entry| {
                (entry.rx, entry.ry, entry.overlay_id, entry.frame)
                    == (rx, ry, resource.overlay_id, resource.overlay_data)
            }),
            "actual occupied-cell output reaches the render index"
        );
        assert!(!output.overlay_removals.contains(&(rx, ry)));
    }
    assert!(
        grid.pending_dirty_cells().is_empty(),
        "frame finalizer transferred the render facts"
    );
    let class = &scene.sim().ore_growth_state.native_tiberium_state().classes[riparius.0 as usize];
    assert!(
        initial_growth != queue_state(&class.growth, &class.growth_bitmap)
            || initial_spread != queue_state(&class.spread, &class.spread_bitmap)
    );
    let native_reload = corpus()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "growth_then_spread_driver_stock_fast_growth_reload")
        .unwrap();
    assert_eq!(class.growth_timer.start_frame(), initial_frame as i32);
    assert_eq!(class.spread_timer.start_frame(), initial_frame as i32);
    assert_eq!(
        json!(class.growth_timer.duration()),
        native_reload["steps"][0]["state"]["classes"][0]["growth_timer"][2]
    );
    assert_eq!(
        json!(class.spread_timer.duration()),
        native_reload["steps"][0]["state"]["classes"][0]["spread_timer"][2]
    );

    let events = &scene.sim().lifecycle_test_events_for_test()[event_start..];
    let commit = events
        .iter()
        .position(|event| matches!(event, LifecycleTestEvent::BinaryFrameCommitted))
        .unwrap();
    let drain = events
        .iter()
        .position(|event| matches!(event, LifecycleTestEvent::PendingDeleteDrainStarted))
        .unwrap();
    assert!(commit < drain);
    let newly_drained: BTreeSet<_> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let LifecycleTestEvent::DeferredDeleteSelected {
                ticket: DeferredDeleteTicket::Overlay(id),
            } = event
            else {
                return None;
            };
            assert!(drain < index);
            (!initial_pending.contains(&DeferredDeleteTicket::Overlay(*id))
                && !initial_overlays.contains(id))
            .then_some(*id)
        })
        .collect();
    assert!(
        newly_drained.len() >= witnesses.len(),
        "new ordinary Overlay constructions reach the actual frame tail, apart from authored pending objects"
    );
    for id in &newly_drained {
        assert!(!scene.sim().load_objects.has_overlay(*id));
    }
    let serial_after = scene
        .sim()
        .native_unique_ids
        .as_ref()
        .unwrap()
        .current_raw();
    assert!(serial_after.wrapping_sub(serial_before) as usize >= newly_drained.len());
    eprintln!(
        "normal XMP L0/L1: {} RNG draws across all frame systems, {} newly drained Overlays, native serial {} -> {}; native startup RNG and authored-init queue comparison remain separate",
        draws.len(),
        newly_drained.len(),
        serial_before,
        serial_after
    );
}
