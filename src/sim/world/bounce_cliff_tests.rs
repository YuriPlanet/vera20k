//! Whole original Bounce439B00 controls through the production Cell adapter.

use super::{ResolvedBounceTerrain, fixture, fixture_rules};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::bounce::{BounceOutcome, BounceState};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use serde_json::Value;

fn native_body(hex: &str) -> BounceState {
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(bytes.len(), 0x50);
    let double = |offset| {
        NativeF64Bits::from_bits(u64::from_le_bytes(
            bytes[offset..offset + 8].try_into().unwrap(),
        ))
    };
    let float = |offset| {
        NativeF32Bits::from_bits(u32::from_le_bytes(
            bytes[offset..offset + 4].try_into().unwrap(),
        ))
    };
    BounceState::from_native_for_test(
        double(0),
        double(8),
        double(0x10),
        std::array::from_fn(|i| float(0x18 + i * 4)),
        std::array::from_fn(|i| float(0x24 + i * 4)),
        std::array::from_fn(|i| float(0x30 + i * 4)),
        std::array::from_fn(|i| float(0x40 + i * 4)),
    )
}

#[test]
fn native_cliff_collisions_match_full_original_bounce_update() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bounce_cliff.json",
    ))
    .unwrap();
    let cases = corpus["update_cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let rules = fixture_rules();
    let mut issues = Vec::new();
    let mut cliff_calls = 0;
    for case in cases {
        let sim = fixture(&case["input"]);
        let terrain = ResolvedBounceTerrain {
            sim: &sim,
            rules: &rules,
        };
        let mut state = native_body(case["initial_body_hex"].as_str().unwrap());
        let actual = state.update(&terrain).unwrap();
        let expected = match case["outcome"].as_u64().unwrap() {
            0 => BounceOutcome::Falling,
            1 => BounceOutcome::Bounced,
            2 => BounceOutcome::Stopped,
            other => panic!("unexpected native Bounce outcome {other}"),
        };
        let expected_body = native_body(case["final_body_hex"].as_str().unwrap());
        let dummy = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .snapshot()
            .coord;
        assert_eq!(
            serde_json::json!([dummy.0, dummy.1]),
            case["final_dummy_coord"],
            "{} Dummy after whole Update",
            case["input"]["name"]
        );
        if actual != expected || state != expected_body {
            issues.push(format!(
                "{}: outcome {actual:?} vs {expected:?}; body {state:?} vs {expected_body:?}",
                case["input"]["name"]
            ));
        }
        cliff_calls += case["direction_calls"].as_array().unwrap().len();
    }
    assert!(cliff_calls > 0, "controls must reach actual Cell4848B0");
    assert!(issues.is_empty(), "{}", issues.join("\n"));
}

#[test]
fn native_cliff_directions_keep_corner_and_shared_dummy_semantics() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bounce_cliff.json",
    ))
    .unwrap();
    let mut directions = std::collections::BTreeSet::new();
    for case in corpus["direction_cases"].as_array().unwrap() {
        let input = &case["input"];
        let sim = fixture(input);
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cells = NativeCellQuery::canonical(terrain);
        let receiver = cells.lookup((
            input["receiver"][0].as_i64().unwrap() as i16,
            input["receiver"][1].as_i64().unwrap() as i16,
        ));
        let actual = crate::sim::cell_kernel::cliff_face_direction(
            &cells,
            receiver,
            (
                input["world_coord"][0].as_i64().unwrap() as i32,
                input["world_coord"][1].as_i64().unwrap() as i32,
            ),
        )
        .map(i32::from)
        .unwrap_or(-1);
        let expected = case["direction_calls"][0]["result"].as_i64().unwrap() as i32;
        assert_eq!(actual, expected, "{} direction", input["name"]);
        let (x, y) = cells.dummy().snapshot().coord;
        assert_eq!(
            serde_json::json!([x, y]),
            case["final_dummy_coord"],
            "{} final Dummy coordinate",
            input["name"]
        );
        directions.insert(actual);
    }
    assert_eq!(directions, (-1..=8).collect());
}

#[test]
fn native_physical_hills_tyre_constructor_flight_and_host_retirement() {
    use crate::sim::world::Simulation;
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bounce_cliff.json",
    ))
    .unwrap();
    let native = &corpus["physical_hills"];
    let mut loaded = crate::headless_scenario::load(&root, "Hills.mmx", 17).unwrap();
    let terrain = loaded.runtime.simulation.resolved_terrain.take().unwrap();
    for cell in native["input"]["physical_cells"].as_array().unwrap() {
        let resolved = terrain
            .cell(
                cell["x"].as_u64().unwrap() as u16,
                cell["y"].as_u64().unwrap() as u16,
            )
            .unwrap();
        assert_eq!(
            i64::from(resolved.source_tile_index),
            cell["tile"].as_i64().unwrap()
        );
        assert_eq!(
            u64::from(resolved.source_sub_tile),
            cell["subtile"].as_u64().unwrap()
        );
        assert_eq!(u64::from(resolved.level), cell["level"].as_u64().unwrap());
        assert_eq!(
            u64::from(resolved.slope_type),
            cell["slope"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(resolved.land_type),
            cell["land"].as_u64().unwrap()
        );
    }
    let rules = &loaded.runtime.resources.rules;
    // Seed5 is at CONSTRUCTOR ENTRY, independently established by original
    // 7493B0. It is not a seed for the preceding paid/death/world prefix.
    let mut sim =
        Simulation::with_seed(native["input"]["constructor_entry_seed"].as_u64().unwrap());
    sim.install_resolved_terrain_for_new_map(terrain);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        native["constructor_rng_before"]["scenario"]
            .as_str()
            .unwrap()
    );
    let kind = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
    let origin = &native["input"]["source_coord"];
    let (_, draws) = crate::sim::rng::trace_draws(|| {
        sim.emit_debris_voxel(
            kind,
            rules.voxel_anim_type(kind),
            None,
            glam::IVec3::new(
                origin[0].as_i64().unwrap() as i32,
                origin[1].as_i64().unwrap() as i32,
                origin[2].as_i64().unwrap() as i32,
            ),
            true,
            &mut Vec::new(),
        )
        .unwrap();
    });
    let raw: Vec<_> = draws.iter().map(|draw| draw["value"].clone()).collect();
    let expected_raw: Vec<_> = native["constructor_requests"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|request| request["raw_draws"].as_array().unwrap().iter().cloned())
        .collect();
    assert_eq!(raw, expected_raw);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        native["constructor_rng_after"]["scenario"]
            .as_str()
            .unwrap()
    );
    let rng_after_constructor = sim.scenario_rng.native_state_hex();
    let ids = sim.voxel_anims().ids();
    assert_eq!(ids.len(), 1);
    let id = ids[0];
    assert_eq!(
        sim.voxel_anims().get(id).unwrap().bounce(),
        native_body(native["constructor_body_hex"].as_str().unwrap())
    );
    assert_eq!(
        i64::from(sim.voxel_anims().get(id).unwrap().duration()),
        native["constructor_state"]["duration"].as_i64().unwrap()
    );
    let air = crate::sim::world::display_layers::DisplayLayer::AIR;
    for visit in 1..=native["delete_visit"].as_u64().unwrap() {
        let checkpoint = native["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["visit"].as_u64() == Some(visit));
        if let Some(row) = checkpoint {
            let before = sim.voxel_anims().get(id).unwrap();
            assert_eq!(
                before.bounce(),
                native_body(row["before"]["body_hex"].as_str().unwrap()),
                "before visit{visit}"
            );
            assert_eq!(
                i64::from(before.duration()),
                row["before"]["duration"].as_i64().unwrap(),
                "before visit{visit}"
            );
        }
        sim.visit_voxel_anim(id, rules);
        if let Some(row) = checkpoint {
            let registered = row["after"]["logic_registered"].as_bool().unwrap();
            assert_eq!(
                sim.live_object_order_snapshot().contains(&id),
                registered,
                "Logic visit{visit}"
            );
            assert_eq!(
                sim.display_layers().members(air).contains(&id),
                registered,
                "Display visit{visit}"
            );
            if registered {
                let after = sim.voxel_anims().get(id).unwrap();
                assert_eq!(
                    after.bounce(),
                    native_body(row["after"]["body_hex"].as_str().unwrap()),
                    "after visit{visit}"
                );
                assert_eq!(
                    i64::from(after.duration()),
                    row["after"]["duration"].as_i64().unwrap(),
                    "after visit{visit}"
                );
                let coord = after.world_coord();
                assert_eq!(
                    serde_json::json!([coord.x, coord.y, coord.z]),
                    row["after"]["location"],
                    "Location visit{visit}"
                );
            } else {
                // Existing visitor removes immediately after UnInit; original
                // retains a deferred identity, then725C70 drains it. Both have
                // already retired from the gameplay Logic/Display traversal.
                assert!(sim.voxel_anims().get(id).is_none());
            }
        }
    }
    assert_eq!(sim.scenario_rng.native_state_hex(), rng_after_constructor);
}

#[test]
fn retail_hills_harv_death_admits_a_cliff_tyre_and_retires_its_display() {
    use crate::sim::world::Simulation;
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    // Shared production map load: no flat-grid replacement, supplied velocity
    // or edited retail rules. The HARV placement and pre-death seed are supplied component inputs.
    let mut loaded = crate::headless_scenario::load(&root, "Hills.mmx", 17).unwrap();
    let terrain = loaded.runtime.simulation.resolved_terrain.take().unwrap();
    for (x, tile, level) in [(102, 505, 2), (103, 49, 6)] {
        let cell = terrain.cell(x, 63).unwrap();
        assert_eq!(
            (
                cell.source_tile_index,
                cell.source_sub_tile,
                cell.level,
                cell.slope_type
            ),
            (tile, 0, level, 0)
        );
    }
    let rules = &loaded.runtime.resources.rules;
    // Finite admission search1..128 first reached this east cliff at46.
    // This is a pre-death component seed, independent of native ctor Seed5.
    let mut sim = Simulation::with_seed(46);
    sim.install_resolved_terrain_for_new_map(terrain);
    let miner = sim.substrate.allocate_stable_id();
    sim.substrate
        .entities
        .insert(crate::sim::game_entity::GameEntity::test_default(
            miner,
            "HARV",
            "Americans",
            102,
            63,
        ));
    sim.interner = crate::sim::intern::test_interner();
    assert!(matches!(
        sim.reveal(miner),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let source = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(miner).unwrap().position,
    );
    assert_eq!((source.x, source.y, source.z), (26240, 16256, 208));
    sim.receive_own_health_c4(
        miner,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let ids = sim.voxel_anims().ids();
    assert!(!ids.is_empty(), "stock HARV death must admit its TIREs");
    let air = crate::sim::world::display_layers::DisplayLayer::AIR;
    for &id in &ids {
        assert!(sim.live_object_order_snapshot().contains(&id));
        assert!(sim.display_layers().members(air).contains(&id));
    }
    let mut reflected_at_selected_cliff = false;
    for _ in 1..=151 {
        for &id in &ids {
            let before = sim
                .voxel_anims()
                .get(id)
                .map(|piece| piece.bounce().clone());
            sim.visit_voxel_anim(id, rules);
            if let (Some(before), Some(after)) = (before, sim.voxel_anims().get(id)) {
                let coord = before.position_leptons();
                reflected_at_selected_cliff |= coord.x / 256 == 102
                    && coord.y / 256 == 63
                    && before.position() == after.bounce().position()
                    && f32::from_bits(before.velocity()[0].bits()) > 0.0
                    && f32::from_bits(after.bounce().velocity()[0].bits()) < 0.0;
            }
        }
    }
    assert!(
        reflected_at_selected_cliff,
        "no naturally launched HARV tyre reflected at the selected Hills cliff"
    );
    assert!(
        sim.voxel_anims().is_empty(),
        "debris outlived its host duration"
    );
    for id in ids {
        assert!(!sim.live_object_order_snapshot().contains(&id));
        assert!(!sim.display_layers().members(air).contains(&id));
    }
}
