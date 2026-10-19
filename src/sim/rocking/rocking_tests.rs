//! Consumers of unmodified original70B280/70B570 execution. Expected
//! state words are native output, not calculations from the Rust formula.

use super::rocking_system::{RockingUpdateInputs, advance_state};
use super::{RockerImpulse, RockingState, apply_rocker_impulse};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF32Bits;
use serde_json::Value;

fn word(value: &Value) -> NativeF32Bits {
    NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}

fn state(bits: [u32; 4]) -> RockingState {
    RockingState {
        angle_sideways: NativeF32Bits::from_bits(bits[0]),
        angle_forwards: NativeF32Bits::from_bits(bits[1]),
        vel_sideways: NativeF32Bits::from_bits(bits[2]),
        vel_forwards: NativeF32Bits::from_bits(bits[3]),
    }
}

fn bits(value: &Value) -> [u32; 4] {
    std::array::from_fn(|i| value[i].as_u64().unwrap() as u32)
}

fn normal_inputs() -> RockingUpdateInputs {
    RockingUpdateInputs {
        direct_rocker_linked: false,
        is_foot: true,
        crush_slowdown: false,
        sinking: false,
        crashing: false,
        balloon_hover: false,
        body_facing: 0,
        fallback: NativeF32Bits::ONE,
    }
}

#[test]
fn complete_rocking_update_matches_native_normal_sinking_and_crash_controls() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_rocking_update.json",
    ))
    .unwrap();
    let mut counts = [0; 3];
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let expected = &row["output"];
        let mut state = state([
            word(&input["side_angle_bits"]).bits(),
            word(&input["forward_angle_bits"]).bits(),
            word(&input["side_velocity_bits"]).bits(),
            word(&input["forward_velocity_bits"]).bits(),
        ]);
        let inputs = RockingUpdateInputs {
            direct_rocker_linked: input["direct_rocker_link"].as_bool().unwrap(),
            is_foot: input["abstract_flags"].as_u64().unwrap() & 4 != 0,
            crush_slowdown: input["crush_flag"].as_i64().unwrap() != 0,
            sinking: input["sinking"].as_i64().unwrap() != 0,
            crashing: input["crash"].as_i64().unwrap() != 0,
            balloon_hover: input["balloon_hover"].as_i64().unwrap() != 0,
            body_facing: input["facing_raw"].as_u64().unwrap() as u16,
            fallback: word(&input["fallback_bits"]),
        };
        let damage = advance_state(&mut state, inputs);
        assert_eq!(
            state.native_bits(),
            [
                word(&expected["side_angle_bits"]).bits(),
                word(&expected["forward_angle_bits"]).bits(),
                word(&expected["side_velocity_bits"]).bits(),
                word(&expected["forward_velocity_bits"]).bits()
            ],
            "{}",
            input["name"]
        );
        assert_eq!(
            damage,
            expected["damage_called"].as_bool().unwrap(),
            "{}",
            input["name"]
        );
        counts[match expected["mode"].as_str().unwrap() {
            "normal" => 0,
            "sinking" => 1,
            "crash" => 2,
            _ => unreachable!(),
        }] += 1;
    }
    assert_eq!(counts, [723, 170, 135]);
}

#[test]
fn rocker_impulses_and_monotonic_update_history_match_native() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_rocker_impulse.json",
    ))
    .unwrap();
    assert_eq!(corpus["model"]["is_voxel"], true);
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        let initial = [
            input["angle_bits"][0].as_u64().unwrap() as u32,
            input["angle_bits"][1].as_u64().unwrap() as u32,
            input["velocity_bits"][0].as_u64().unwrap() as u32,
            input["velocity_bits"][1].as_u64().unwrap() as u32,
        ];
        let mut owned = Some(state(initial));
        for impulse in row["impulses"].as_array().unwrap() {
            assert_eq!(
                owned.unwrap().native_bits(),
                bits(&impulse["before_bits"]),
                "{} before",
                input["name"]
            );
            let request = &impulse["input"];
            let weight = f64::from_bits(
                u64::from_str_radix(input["weight_bits"].as_str().unwrap(), 16).unwrap(),
            );
            let assigned = apply_rocker_impulse(
                &mut owned,
                RockerImpulse {
                    position: std::array::from_fn(|i| {
                        input["position"][i].as_i64().unwrap() as i32
                    }),
                    origin: std::array::from_fn(|i| request["origin"][i].as_i64().unwrap() as i32),
                    body_facing: input["facing"].as_u64().unwrap() as u16,
                    force: NativeF32Bits::from_bits(request["force_bits"].as_u64().unwrap() as u32),
                    weight: SimFixed::from_num(weight),
                    no_dampen: request["no_dampen"].as_u64().unwrap() != 0,
                    has_loaded_voxel: true,
                },
            );
            assert_eq!(assigned, !impulse["stores"].as_array().unwrap().is_empty());
            assert_eq!(
                owned.unwrap().native_bits(),
                bits(&impulse["after_bits"]),
                "{} impulse",
                input["name"]
            );
        }
        // Preserve every intermediate native-float state, including visits
        // between the compact retained checkpoints. No reseeding or rewinding.
        for visit in 1..=input["update_steps"].as_u64().unwrap() {
            let expected = row["updates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|sample| sample["visit"].as_u64() == Some(visit));
            if let Some(expected) = expected {
                assert_eq!(
                    owned.unwrap().native_bits(),
                    bits(&expected["before_bits"]),
                    "{} visit{visit} before",
                    input["name"]
                );
            }
            assert!(!advance_state(owned.as_mut().unwrap(), normal_inputs()));
            if let Some(expected) = expected {
                assert_eq!(
                    owned.unwrap().native_bits(),
                    bits(&expected["after_bits"]),
                    "{} visit{visit} after",
                    input["name"]
                );
            }
        }
    }
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 8);
}

#[test]
fn rejected_loaded_model_gate_preserves_sparse_state_and_existing_rates() {
    //70B29E is a plain early return; neither allocation nor state writes.
    let request = RockerImpulse {
        position: [2432, 2176, 0],
        origin: [2432, 2178, 0],
        body_facing: 0,
        force: NativeF32Bits::from_bits(0x3fc0_0000),
        weight: SimFixed::ONE,
        no_dampen: false,
        has_loaded_voxel: false,
    };
    let mut absent = None;
    assert!(!apply_rocker_impulse(&mut absent, request));
    assert!(absent.is_none());
    let mut present = Some(state([0, 0, 0x3ca3_d70a, 0xbca3_d70a]));
    let before = present.unwrap().native_bits();
    assert!(!apply_rocker_impulse(&mut present, request));
    assert_eq!(present.unwrap().native_bits(), before);
}

#[test]
fn native_small_rate_survives_compatibility_reads_and_serialization() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_rocker_impulse.json",
    ))
    .unwrap();
    let original = state(bits(&corpus["cases"][0]["impulses"][0]["after_bits"]));
    assert_eq!(original.vel_sideways(), SimFixed::ZERO);
    assert_ne!(original.native_bits()[2] & 0x7fff_ffff, 0);
    let decoded: RockingState =
        serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
    assert_eq!(decoded.native_bits(), original.native_bits());
    let mut next = decoded;
    assert!(!advance_state(&mut next, normal_inputs()));
    assert_eq!(
        next.native_bits(),
        bits(&corpus["cases"][0]["updates"][0]["after_bits"])
    );
}

#[test]
fn production_update_materializes_zero_state_for_a_newly_sinking_ship_only() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    let mut sim = crate::sim::arena_fixture::rival_water_arena(&rules, 7, 32);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let ship = sim
        .spawn_object("DEST", "Americans", 10, 10, 0, &rules)
        .expect("ordinary stock ship admission");
    let object = rules.object("DEST").unwrap();
    let frame = sim.session.binary_frame();
    let entity = sim.substrate.entities.get_mut(ship).unwrap();
    assert!(entity.rocking.is_none());
    assert!(!super::update(
        entity,
        object,
        rules.general.fallback_coefficient,
        frame
    ));
    assert!(
        entity.rocking.is_none(),
        "ordinary zero state remains sparse"
    );

    // Explicit shared sinking-owner boundary, not a fatal-receiver claim.
    sim.begin_ship_sinking(ship, &rules, crate::sim::world::FrameEffects::default());
    let entity = sim.substrate.entities.get_mut(ship).unwrap();
    assert!(entity.sinking.is_active());
    assert!(entity.rocking.is_none());
    assert_eq!(entity.body_facing_current(frame), 0);
    assert!(!super::update(
        entity,
        object,
        rules.general.fallback_coefficient,
        frame
    ));
    let actual = entity
        .rocking
        .as_ref()
        .expect("sinking is a native angle producer")
        .native_bits();
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_rocking_update.json",
    ))
    .unwrap();
    let native = corpus
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["output"]["mode"] == "sinking"
                && row["input"]["facing_raw"] == 0
                && row["input"]["forward_angle_bits"] == "00000000"
        })
        .unwrap();
    //70B5AE only reads/writes the forward angle in this branch. Side angle
    // and rates are untouched; the native control's other supplied words
    // therefore do not become priors for this newly admitted Rust actor.
    assert_eq!(
        actual[1],
        word(&native["output"]["forward_angle_bits"]).bits()
    );
    assert_eq!([actual[0], actual[2], actual[3]], [0; 3]);
}

/// The retained rate writer itself consumes original Unit73B05B..73B06E
/// binary32 outputs, including subnormal/rounding-sensitive rates.
#[test]
fn overlay_recoil_retains_original_unit_rate_words() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_per_cell_overlay.json",
    ))
    .unwrap();
    let mut admitted = 0;
    for row in rows.as_array().unwrap() {
        if !row["output"]["applied"].as_bool().unwrap() {
            continue;
        }
        let mut value = state([
            0x3d00_0000,
            0xbd00_0000,
            0x3d80_0000,
            word(&row["input"]["velocity_bits"]).bits(),
        ]);
        value.recoil_after_overlay_crush();
        assert_eq!(
            value.native_bits(),
            [
                0x3d00_0000,
                0xbd00_0000,
                0x3d80_0000,
                word(&row["output"]["velocity_bits"]).bits()
            ],
            "{}",
            row["input"]["name"]
        );
        admitted += 1;
    }
    assert_eq!(admitted, 144);
}
