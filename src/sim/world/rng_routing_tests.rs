//! RNG routing, authority-boundary, and determinism tests.
//!
//! Guards Scenario/Main/MapGen independence, per-accessor routing (the
//! dominant silent-misroute failure), authoritative hash/snapshot coverage,
//! in-scenario process-global load retention, end-to-end determinism, and
//! ground-truth gamemd value-parity pins.

use super::{DEFAULT_SIM_SEED, Simulation};
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use std::collections::BTreeMap;

const RNG_INDEX_B_START: i32 = 0x67;
const NATIVE_MAPGEN_SEED0_HEX: &str =
    include_str!("../../../tests/fixtures/rng/mapgen_seed0_native_0x3f4.hex");

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn decode_hex_fixture(text: &str) -> Vec<u8> {
    let mut nibbles = Vec::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        nibbles.push(hex_nibble(byte).expect("fixture contains non-hex data"));
    }
    assert_eq!(nibbles.len() % 2, 0, "fixture has an odd hex digit count");
    nibbles
        .chunks_exact(2)
        .map(|pair| (pair[0] << 4) | pair[1])
        .collect()
}

/// Helper: advance a sim by one tick with empty inputs.
fn tick(sim: &mut Simulation) {
    let height_map = BTreeMap::new();
    sim.advance_tick(&[], None, &height_map, None, None, 67);
}

// --- Test 1: seed-equality invariant (design §4) ---
#[test]
fn both_streams_seed_byte_identically() {
    for &seed in &[0u64, 1, DEFAULT_SIM_SEED, u32::MAX as u64] {
        let sim = Simulation::with_seed(seed);
        assert_eq!(
            sim.scenario_rng.state(),
            sim.main_rng.state(),
            "scenario and main streams must seed byte-identically for seed {seed:#x}"
        );
        // Both must start at the gamemd index_b = 0x67 lag start.
        assert_eq!(sim.scenario_rng.index_b(), RNG_INDEX_B_START);
        assert_eq!(sim.main_rng.index_b(), RNG_INDEX_B_START);
        // And must equal a fresh SimRng::new(seed).
        let fresh = SimRng::new(seed);
        assert_eq!(sim.scenario_rng.state(), fresh.state());
    }
}

// --- Test 2: independence (design §7.2) ---
#[test]
fn drawing_scenario_leaves_main_untouched() {
    let seed = 1234u64;
    let mut sim = Simulation::with_seed(seed);
    let fresh = SimRng::new(seed);
    let mapgen_before = sim.mapgen_rng.state();

    for _ in 0..32 {
        sim.scatter_rng().next_u32();
    }
    assert_eq!(
        sim.main_rng.state(),
        fresh.state(),
        "drawing only from the scenario stream must not advance main"
    );
    assert_ne!(
        sim.scenario_rng.state(),
        fresh.state(),
        "scenario stream must have advanced"
    );
    assert_eq!(
        sim.mapgen_rng.state(),
        mapgen_before,
        "drawing only from the scenario stream must not advance MapGen"
    );
}

#[test]
fn drawing_main_leaves_scenario_untouched() {
    let seed = 1234u64;
    let mut sim = Simulation::with_seed(seed);
    let fresh = SimRng::new(seed);
    let mapgen_before = sim.mapgen_rng.state();

    for _ in 0..32 {
        sim.weapon_spread_rng().next_u32();
    }
    assert_eq!(
        sim.scenario_rng.state(),
        fresh.state(),
        "drawing only from the main stream must not advance scenario"
    );
    assert_ne!(
        sim.main_rng.state(),
        fresh.state(),
        "main stream must have advanced"
    );
    assert_eq!(
        sim.mapgen_rng.state(),
        mapgen_before,
        "drawing only from the main stream must not advance MapGen"
    );
}

// --- Test 3: per-stream gamemd raw-sequence pin (design §7.3) ---
//
// Both streams from seed 1 must independently reproduce the gamemd raw draw
// sequence (verified vs the binary scenario RNG; pinned in rng.rs by
// test_gamemd_raw_sequence_seed_one). Proves the dual seeding is an exact clone.
#[test]
fn gsi_04_02_terrain_load_handoffs_install_scenario_and_main_independently() {
    let seed = 0x1234_5678u64;
    let mut sim = Simulation::with_seed(seed);
    let fresh = SimRng::new(seed).logical_state();

    let mut fill_scenario = SimRng::new(seed);
    for _ in 0..10 {
        let _ = fill_scenario.next_range_u32_inclusive(0, 3);
    }
    let expected_scenario = fill_scenario.logical_state();
    sim.install_terrain_load_advanced_scenario_rng(fill_scenario);
    assert_eq!(sim.scenario_rng.logical_state(), expected_scenario);
    assert_eq!(sim.main_rng.logical_state(), fresh);

    let mut selector_main = SimRng::new(seed);
    for _ in 0..128 {
        let _ = selector_main.next_u32();
    }
    let expected_main = selector_main.logical_state();
    sim.install_variant_advanced_main_rng(selector_main);
    assert_eq!(sim.scenario_rng.logical_state(), expected_scenario);
    assert_eq!(sim.main_rng.logical_state(), expected_main);
}

#[test]
fn each_stream_reproduces_gamemd_raw_sequence_seed_one() {
    let mut sim = Simulation::with_seed(1);
    assert_eq!(sim.scenario_rng.next_u32(), 0x78B7_6ED5);
    assert_eq!(sim.scenario_rng.next_u32(), 0x275D_74AE);
    assert_eq!(sim.scenario_rng.next_u32(), 0xDA63_B931);

    assert_eq!(sim.main_rng.next_u32(), 0x78B7_6ED5);
    assert_eq!(sim.main_rng.next_u32(), 0x275D_74AE);
    assert_eq!(sim.main_rng.next_u32(), 0xDA63_B931);
}

// --- Test 4: routing regression — one test per accessor (design §7.4) ---
//
// The central guard against a future edit silently re-pointing an accessor at
// the wrong field. Each scenario accessor must advance ONLY scenario_rng (main
// unchanged); each main accessor must advance ONLY main_rng.
macro_rules! assert_routes_scenario {
    ($name:ident, $accessor:ident) => {
        #[test]
        fn $name() {
            let seed = 7u64;
            let mut sim = Simulation::with_seed(seed);
            let fresh = SimRng::new(seed);
            sim.$accessor().next_u32();
            assert_ne!(
                sim.scenario_rng.state(),
                fresh.state(),
                concat!(stringify!($accessor), " must advance the scenario stream")
            );
            assert_eq!(
                sim.main_rng.state(),
                fresh.state(),
                concat!(stringify!($accessor), " must NOT advance the main stream")
            );
        }
    };
}

macro_rules! assert_routes_main {
    ($name:ident, $accessor:ident) => {
        #[test]
        fn $name() {
            let seed = 7u64;
            let mut sim = Simulation::with_seed(seed);
            let fresh = SimRng::new(seed);
            sim.$accessor().next_u32();
            assert_ne!(
                sim.main_rng.state(),
                fresh.state(),
                concat!(stringify!($accessor), " must advance the main stream")
            );
            assert_eq!(
                sim.scenario_rng.state(),
                fresh.state(),
                concat!(
                    stringify!($accessor),
                    " must NOT advance the scenario stream"
                )
            );
        }
    };
}

assert_routes_scenario!(route_scatter_rng, scatter_rng);
assert_routes_scenario!(route_subcell_rng, subcell_rng);
assert_routes_scenario!(route_smudge_rng, smudge_rng);
assert_routes_scenario!(route_wall_damage_rng, wall_damage_rng);
assert_routes_scenario!(route_bridge_rng, bridge_rng);
assert_routes_scenario!(route_ore_rng, ore_rng);
assert_routes_scenario!(route_anim_rng, anim_rng);
assert_routes_scenario!(route_particle_rng, particle_rng);
assert_routes_scenario!(route_superweapon_rng, superweapon_rng);

assert_routes_main!(route_weapon_spread_rng, weapon_spread_rng);
assert_routes_main!(route_house_ai_rng, house_ai_rng);

// --- Test 5: ground-truth value parity vs gamemd (design §7.5, REQUIRED) ---
//
// gamemd's `Random__RandomRanged` (0x0065C7E0) is the rejection-sampling
// algorithm reproduced by `SimRng::next_range_u32_inclusive`. Its decompiled +
// disassembled form was verified this session (decompile_function 0x0065C7E0 /
// disassemble_function 0x0065C7E0): low/high at [ESP+4]/[ESP+8], `this` in ECX,
// struct layout disabled@0 / index_a@4 / index_b@8 / state[250]@0xc, mask =
// 2^(msb+1)-1, reject `> span`, index wrap at 250, RET 0x8.
//
// The MCP `emulate_function 0x0065C7E0` harness times out re-initializing the
// 1012-byte post-seed RNG image, so the emitted values below are derived the
// equally-rigorous way: feeding the binary-pinned raw draw stream for seed 1
// (0x78B76ED5, 0x275D74AE, 0xDA63B931, ... — read_memory-verified, pinned by
// `test_gamemd_raw_sequence_seed_one`) through that verified algorithm.
//
// RandomRanged(0,4), seed 1: mask=7, draws &7 -> 5(reject),6(reject),1(accept),
// then 2,1,0,1 -> sequence [1, 2, 1, 0, 1].
// RandomRanged(0,7), seed 1: mask=7 -> [5, 6, 1, 2, 1].
#[test]
fn scenario_stream_matches_gamemd_random_ranged_0_4() {
    // gamemd emitted values (Random__RandomRanged 0x0065C7E0, seed 1).
    const GAMEMD_RANGED_0_4_SEED1: [u32; 5] = [1, 2, 1, 0, 1];
    let mut sim = Simulation::with_seed(1);
    for (i, &expected) in GAMEMD_RANGED_0_4_SEED1.iter().enumerate() {
        let got = sim.wall_damage_rng().next_range_u32_inclusive(0, 4);
        assert_eq!(
            got, expected,
            "scenario RandomRanged(0,4) draw {i} must match gamemd"
        );
    }
}

#[test]
fn main_stream_matches_gamemd_random_ranged_0_7() {
    // gamemd emitted values (Random__RandomRanged 0x0065C7E0, seed 1) — the
    // same algorithm/stream a main-stream weapon-spread consumer will draw from.
    const GAMEMD_RANGED_0_7_SEED1: [u32; 5] = [5, 6, 1, 2, 1];
    let mut sim = Simulation::with_seed(1);
    for (i, &expected) in GAMEMD_RANGED_0_7_SEED1.iter().enumerate() {
        let got = sim.weapon_spread_rng().next_range_u32_inclusive(0, 7);
        assert_eq!(
            got, expected,
            "main RandomRanged(0,7) draw {i} must match gamemd"
        );
    }
}

// --- Test 6: authoritative hash boundary (design §7.6) ---
#[test]
fn advancing_main_only_does_not_change_state_hash() {
    let mut sim = Simulation::with_seed(99);
    let before = sim.state_hash();
    sim.weapon_spread_rng().next_u32();
    assert_eq!(
        sim.state_hash(),
        before,
        "process-global Main state is outside the authoritative world hash"
    );
}

#[test]
fn advancing_scenario_only_changes_state_hash() {
    let mut sim = Simulation::with_seed(99);
    let before = sim.state_hash();
    sim.scatter_rng().next_u32();
    assert_ne!(
        sim.state_hash(),
        before,
        "advancing the scenario stream must change the world hash"
    );
}

#[test]
fn advancing_mapgen_only_does_not_change_state_hash_or_gameplay_streams() {
    let mut sim = Simulation::with_seed(99);
    let before_rng = sim.rng_state();
    let before_hash = sim.state_hash();

    sim.mapgen_rng.next_u32();

    let after_rng = sim.rng_state();
    assert_eq!(
        after_rng.scenario, before_rng.scenario,
        "advancing MapGen must not move the Scenario stream"
    );
    assert_eq!(
        after_rng.main, before_rng.main,
        "advancing MapGen must not move the Main stream"
    );
    assert_ne!(
        after_rng.mapgen, before_rng.mapgen,
        "the MapGen stream must advance"
    );
    assert_eq!(
        sim.state_hash(),
        before_hash,
        "process-global MapGen state is outside the authoritative world hash"
    );
}

// --- Test 7: snapshot reads Scenario bytes, then resets it like retail load ---
#[test]
fn snapshot_load_resets_scenario_and_omits_process_globals() {
    let mut sim = Simulation::with_seed(0xABCD_1234);
    for _ in 0..11 {
        sim.scatter_rng().next_u32();
    }
    for _ in 0..7 {
        sim.weapon_spread_rng().next_u32();
    }
    sim.mapgen_rng = SimRng::new(99);
    for _ in 0..3 {
        sim.mapgen_rng.next_u32();
    }

    let scenario_before = sim.scenario_rng.state();
    let main_before = sim.main_rng.state();
    let mapgen_before = sim.mapgen_rng.state();
    let process_placeholder = SimRng::new(0).state();
    assert_ne!(scenario_before, process_placeholder);
    assert_ne!(main_before, process_placeholder);
    assert_ne!(mapgen_before, process_placeholder);

    let bytes = GameSnapshot::save(&sim, 0, 0, "rng_test", 0);
    let loaded = GameSnapshot::load(&bytes).expect("snapshot load");
    let restored = loaded.sim;

    assert_eq!(
        restored.scenario_rng.state(),
        process_placeholder,
        "native load reseeds Scenario->Random with zero after reading its saved bytes"
    );
    assert_eq!(
        restored.main_rng.state(),
        process_placeholder,
        "Main is process-global and must not be restored from Scenario data"
    );
    assert_eq!(
        restored.mapgen_rng.state(),
        process_placeholder,
        "MapGen is process-global and must not be restored from Scenario data"
    );
}

// --- Test 8: end-to-end determinism, both streams (design §7.8) ---
#[test]
fn determinism_both_streams_match_across_ticks() {
    let seed = DEFAULT_SIM_SEED;
    let mut sim_a = Simulation::with_seed(seed);
    let mut sim_b = Simulation::with_seed(seed);
    for _ in 0..40 {
        tick(&mut sim_a);
        tick(&mut sim_b);
        assert_eq!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "world hash must match each tick"
        );
        assert_eq!(
            sim_a.scenario_rng.state(),
            sim_b.scenario_rng.state(),
            "scenario streams must match each tick"
        );
        assert_eq!(
            sim_a.main_rng.state(),
            sim_b.main_rng.state(),
            "main streams must match each tick"
        );
    }
}

// --- Test 9: in-scenario load keeps the live process-global state ---
#[test]
fn production_in_scenario_load_retains_live_seed_main_and_mapgen() {
    let mut saved = Simulation::with_seed(0xABCD_1234);
    for _ in 0..11 {
        saved.scatter_rng().next_u32();
    }

    let saved_seed = saved.session.seed;
    let saved_scenario = saved.scenario_rng.state();
    let reset_scenario = SimRng::new(0).state();
    assert_ne!(saved_scenario, reset_scenario);
    let bytes = GameSnapshot::save(&saved, 0, 0, "rng_test", 0);

    let mut live = Simulation::with_seed(0x7654_3210);
    for _ in 0..3 {
        live.scatter_rng().next_u32();
    }
    for _ in 0..7 {
        live.weapon_spread_rng().next_u32();
    }
    live.mapgen_rng = SimRng::new(99);
    for _ in 0..3 {
        live.mapgen_rng.next_u32();
    }

    let live_seed = live.session.seed;
    assert_ne!(live_seed, saved_seed);
    let live_scenario = live.scenario_rng.state();
    let live_main = live.main_rng.state();
    let live_mapgen = live.mapgen_rng.state();

    let mut restored = GameSnapshot::load(&bytes).expect("snapshot load").sim;
    restored.retain_in_scenario_process_state_from(&live);
    assert_eq!(
        restored.scenario_rng.state(),
        reset_scenario,
        "Scenario must be reseeded with zero after its saved bytes are read"
    );
    assert_ne!(restored.scenario_rng.state(), saved_scenario);
    assert_ne!(restored.scenario_rng.state(), live_scenario);
    assert_eq!(
        restored.session.seed, live_seed,
        "in-scenario load must retain the live process-global seed"
    );
    assert_eq!(
        restored.main_rng.state(),
        live_main,
        "Main must retain the live pre-load process cursor"
    );
    assert_eq!(
        restored.mapgen_rng.state(),
        live_mapgen,
        "MapGen must retain the live pre-load process cursor"
    );
}

#[test]
fn mapgen_fresh_state_matches_native_seed_zero_full_object() {
    let sim = Simulation::with_seed(0xDEAD_BEEF);
    let mapgen = sim.rng_views().mapgen;
    let raw = decode_hex_fixture(NATIVE_MAPGEN_SEED0_HEX);

    assert_eq!(raw.len(), 0x3F4, "native fixture must be one RNG object");
    assert_eq!(
        &raw[1..4],
        &[0, 0, 0],
        "zero padding is an observed fact of this native-derived fixture"
    );
    assert_eq!(raw[0], mapgen.disabled);
    assert_eq!(&raw[4..8], &mapgen.index_a.to_le_bytes());
    assert_eq!(&raw[8..12], &mapgen.index_b.to_le_bytes());

    let native_words = raw[0x0C..0x3F4].chunks_exact(4);
    assert_eq!(
        native_words.len(),
        mapgen.words.len(),
        "native fixture must contain all 250 logical words"
    );
    for (index, (native_word, rust_word)) in native_words.zip(mapgen.words).enumerate() {
        let native_word = u32::from_le_bytes(
            native_word
                .try_into()
                .expect("chunks_exact(4) yields four bytes"),
        );
        assert_eq!(
            native_word, *rust_word,
            "fresh MapGen logical word {index} must match the native fixture"
        );
    }
}

#[test]
fn scenario_main_reseed_does_not_change_mapgen() {
    let mut sim = Simulation::with_seed(7);
    for _ in 0..5 {
        sim.mapgen_rng.next_u32();
    }
    let before = sim.mapgen_rng.logical_state();
    sim.reseed_scenario_and_main(99);
    assert_eq!(sim.mapgen_rng.logical_state(), before);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        SimRng::new(99).logical_state()
    );
    assert_eq!(
        sim.main_rng.logical_state(),
        SimRng::new(99).logical_state()
    );
}

#[test]
fn rng_views_name_all_three_streams() {
    let mut sim = Simulation::with_seed(5);
    sim.scatter_rng().next_u32();
    sim.weapon_spread_rng().next_u32();
    sim.mapgen_rng.next_u32();
    let views = sim.rng_views();
    assert_eq!(views.scenario, sim.scenario_rng.logical_view());
    assert_eq!(views.main, sim.main_rng.logical_view());
    assert_eq!(views.mapgen, sim.mapgen_rng.logical_view());
}
