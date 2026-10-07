//! Original enqueue/rebuild and connected caller goldens, not a Rust model.
//! Scope, fixture seams and reproduction: tools/spatial_oracle/ore_queue.md.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Health;
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
        let mut heap = vec![0];
        heap.extend(
            prepared["heap_indices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap() as u32),
        );
        let bitmap = prepared["bitmap"]
            .as_array()
            .unwrap()
            .iter()
            .map(at)
            .collect();
        return (
            NativeTiberiumQueue {
                entries,
                heap,
                capacity,
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
    let mut heap = vec![0];
    if selected {
        heap.extend(
            input["pre_heap_indices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_u64().unwrap() as u32),
        );
    } else {
        heap.push(0);
    }
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
            heap,
            capacity,
        },
        bitmap,
    )
}

fn queue_state(queue: &NativeTiberiumQueue, bitmap: &BTreeSet<(u16, u16)>) -> Value {
    json!({"array_count": queue.array_len(),
        "entries": queue.entries.iter().map(|e| json!({"cell":[e.rx,e.ry],"priority_bits":e.priority_bits})).collect::<Vec<_>>(),
        "heap_indices": &queue.heap[1..],
        "bitmap_cells": bitmap.iter().map(|&(x,y)| [x,y]).collect::<Vec<_>>()})
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
    let view = rng.logical_view();
    assert_eq!(
        json!([view.index_a, view.index_b]),
        expected["rng_indices"],
        "{name}: RNG indices"
    );
    let hex = rng.native_state_hex();
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
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
    let mut terrain = crate::map::resolved_terrain::test_grid(33, 33, |x, y| {
        crate::map::resolved_terrain::test_loader_clear_cell(x, y)
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
                terrain.cell_mut(x, y).unwrap().outside_playfield =
                    !shape.admits(x as i16, y as i16);
                terrain.cell_mut(x, y).unwrap().allows_tiberium =
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
            grid.cell_mut(cell.0, cell.1).overlay_id = u8::try_from(raw).ok();
        }
        if natural {
            grid.recalculate_runtime_cell(&mut terrain, &registry, cell);
        }
        if let Some(land) = c["land"].as_u64() {
            terrain.cell_mut(cell.0, cell.1).unwrap().land_type = land as u8;
        }
        terrain.cell_mut(cell.0, cell.1).unwrap().slope_type = c["slope"].as_u64().unwrap() as u8;
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
                let c = terrain.cell_mut(cell.0, cell.1).unwrap();
                c.land_type = 1;
                c.base_terrain_class = crate::rules::terrain_rules::TerrainClass::Road;
                c.base_build_blocked = true;
            }
            "slope" => terrain.cell_mut(cell.0, cell.1).unwrap().slope_type = 1,
            "bridge" => {
                terrain
                    .cell_mut(cell.0, cell.1)
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
                    Health {
                        current: input["building_health"].as_i64().unwrap_or(900) as i32,
                    },
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
            Health { current: 900 },
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
        mut grid,
        mut terrain,
        mut state,
        mut rng,
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
    compare_overlay_state(&constructors, &row["before"], name);
    compare_dummy(&terrain, &row["before"], name);
    compare_state(&state, &rng, &row["before"], capacity, name);
    let receiver = TiberiumTypeId(input["receiver"].as_u64().unwrap() as u8);
    let cell = at(&input["target"]);
    let frame = input["frame"].as_i64().unwrap() as u32;
    let view = Some(NativeCellObjectView::new(&occupancy, &trees));
    let objects =
        TiberiumPlacementObjectContext::new(&entities, &occupancy, &rules, &interner, &trees);
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
        let mut constructed = Vec::new();
        let (_, draws) = crate::sim::rng::trace_draws(|| match input["entry"].as_str().unwrap() {
            "enqueue" if input["queue"] == "growth" => {
                state.add_native_growth_queue_cell(
                    &grid,
                    &registry,
                    types,
                    Some(&terrain),
                    input["grows"] == true,
                    receiver,
                    cell.0,
                    cell.1,
                    frame,
                    &mut rng,
                );
            }
            "enqueue" => {
                state.add_native_spread_queue_cell(
                    receiver,
                    &grid,
                    &registry,
                    types,
                    Some(&terrain),
                    &sources,
                    view,
                    cell.0,
                    cell.1,
                    frame,
                    input["spreads"] == true,
                    &mut rng,
                );
            }
            "reduce" => {
                let result = reduce_tiberium(
                    &mut ReduceTiberiumContext {
                        overlay_grid: Some(&mut grid),
                        ore_growth_state: &mut state,
                        overlay_registry: Some(&registry),
                        tiberium_types: Some(types),
                        resolved_terrain: Some(&mut terrain),
                        source_object_cells: Some(&sources),
                        live_objects: view,
                        rng: Some(&mut rng),
                        binary_frame: frame,
                        spread_enabled: input["spreads"] == true,
                        radar_dirty_cells: None,
                        radar_dirty_generation: None,
                        tactical_dirty_cells: None,
                    },
                    cell,
                    input["amount"].as_i64().unwrap_or(1) as i32,
                );
                assert_eq!(
                    json!(result.removed_amount),
                    row["returned"],
                    "{name}: reduced amount"
                );
            }
            "growth_processor" => {
                state.process_native_growth_for_type_with_placement(
                    receiver,
                    &mut grid,
                    &registry,
                    types,
                    Some(&terrain),
                    &sources,
                    Some(objects),
                    &mut rng,
                    frame,
                    input["grows"] == true,
                    input["spreads"] == true,
                    None,
                    None,
                    None,
                );
            }
            "spread_processor" => {
                let result = state.process_native_spread_for_type_with_placement(
                    receiver,
                    &mut grid,
                    &registry,
                    types,
                    Some(&terrain),
                    &sources,
                    Some(NewTiberiumAdmission::runtime(&terrain, objects)),
                    &mut rng,
                    frame,
                    input["spreads"] == true,
                    None,
                    None,
                    None,
                );
                constructed.extend(result.into_overlay_constructions());
            }
            "spread_driver" | "growth_then_spread_driver" => {
                if input["entry"] == "growth_then_spread_driver" {
                    state.tick_native_growth_driver(
                        &mut grid,
                        &registry,
                        types,
                        Some(&terrain),
                        &sources,
                        Some(objects),
                        &mut rng,
                        frame,
                        input["grows"] == true,
                        input["spreads"] == true,
                        input["fast_growth"] == true,
                        None,
                        None,
                        None,
                    );
                }
                let result = state.tick_native_spread_driver(
                    &mut grid,
                    &registry,
                    types,
                    Some(&terrain),
                    &sources,
                    Some(objects),
                    &mut rng,
                    frame,
                    input["grows"] == true,
                    input["spreads"] == true,
                    None,
                    None,
                    None,
                );
                constructed.extend(result.into_overlay_constructions());
            }
            other => panic!("unhandled native entry {other}"),
        });
        constructors.publish_overlay_constructions(constructed);
        if matches!(
            input["entry"].as_str().unwrap(),
            "spread_processor" | "spread_driver" | "growth_then_spread_driver"
        ) {
            // Production's ore rung publishes these Mark attributes before
            // live-object AI. The direct-owner fixture uses the same cell
            // projection; expected native cell fields never seed this update.
            for cell in grid.take_dirty_cells() {
                grid.recalculate_runtime_cell(&mut terrain, &registry, cell);
            }
        }
        total_draws += draws.len();
        if let Some(expected) = row["steps"].get(step) {
            compare_dummy(&terrain, &expected["state"], name);
            compare_overlay_state(&constructors, &expected["state"], name);
            assert_eq!(
                json!(draws.len()),
                expected["draw_count"],
                "{name}: step {step} draws"
            );
            compare_state(
                &state,
                &rng,
                &expected["state"],
                capacity,
                &format!("{name}: step {step}"),
            );
            compare_cells(&grid, &terrain, &expected["cells"], name);
        }
        if let Some(expected) = row["after_drains"].get(step) {
            compare_dummy(&terrain, &expected["state"], name);
            constructors.load_objects.drain_deferred().unwrap();
            compare_overlay_state(&constructors, &expected["state"], name);
            compare_cells(&grid, &terrain, &expected["cells"], name);
        }
    }
    assert_eq!(json!(total_draws), row["draw_count"], "{name}: RNG draws");
    compare_state(&state, &rng, &row["state"], capacity, name);
    compare_overlay_state(&constructors, &row["state"], name);
    compare_dummy(&terrain, &row["state"], name);
    compare_cells(&grid, &terrain, &row["cells"], name);
    let next: Vec<_> = (0..4).map(|_| rng.next_u32()).collect();
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
        json!(sim.load_objects.queue_count()),
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

pub(crate) fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ore_queue.json",
    ))
    .unwrap()
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
