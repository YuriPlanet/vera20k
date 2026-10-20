//! App-owned runtime for transient combat-light presentation.
//!
//! The simulation emits receiver facts; this module materializes the native
//! 24-byte light-vector payload, ages it on committed logic frames, and hands
//! reverse insertion order to the dedicated tactical renderer. It deliberately
//! owns no gameplay state and never enters snapshots or world hashes.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::CombatLightRequest;
use crate::sim::intern::StringInterner;
use crate::sim::projectile::ProjectileCoord;

use super::detail::DetailState;

const STAGE_STEP: u8 = 8;
const EXPIRE_STAGE: u8 = 0x50;

/// Native stage-to-surface scale table. Persistent lights only observe the
/// entries at stages 0, 8, ..., 72, but retaining the complete table makes the
/// integer indexing contract explicit.
#[rustfmt::skip]
const STAGE_SCALE: [u8; 90] = [
     5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 61, 62, 63, 63,
    63, 62, 61, 60, 59, 58, 57, 56, 55, 54, 53, 52, 51, 50, 49, 48,
    47, 46, 45, 44, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34, 33, 32,
    31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16,
    15, 14, 13, 12, 11, 10,  9,  8,  7,  6,  5,  4,  3,  2,  1,  0,
    64,65,66,67,68,69,70,71,72,73,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CombatLight {
    pub coord: ProjectileCoord,
    pub stage: u8,
    pub base_size: i32,
    pub flags: u32,
}

impl CombatLight {
    /// Spotlight5FF850's shared admission and stage selection. Both temporary
    /// System62E280 lights and the later5FFFA0 vector call this owner.
    pub(crate) fn draw_record(self, detail: &mut DetailState) -> Option<CombatLightDrawRecord> {
        if self.flags & 0xE == 0 && detail.frame_rate() < detail.minimum_frame_rate() {
            return None;
        }
        Some(CombatLightDrawRecord {
            coord: self.coord,
            surface_index: scaled_surface_index(self.base_size, self.stage),
            flags: self.flags,
        })
    }
}

/// ParticleSystemDraw62E280 constructs/draws/destroys one temporary Spotlight.
/// F4 is the current stage, independent of persistent light ageing. The body
/// continues drawing after F0 ends while its children remain registered.
pub(crate) fn particle_system_light(
    system: &crate::sim::particles::ParticleSystem,
    kind: &crate::rules::particle_system_type::ParticleSystemType,
    detail: &mut DetailState,
) -> Option<CombatLightDrawRecord> {
    if !kind.one_frame_light || kind.light_size <= 0 || system.particles().is_empty() {
        return None;
    }
    let count =
        i32::try_from(system.particles().len()).expect("native particle vector count fits i32");
    let base_size =
        system_light_base_size(count, kind.particle_cap as i32, kind.light_size).ok()?;
    CombatLight {
        coord: ProjectileCoord::new(system.coords().x, system.coords().y, system.coords().z),
        stage: system.facing(),
        base_size,
        flags: 0,
    }
    .draw_record(detail)
}

fn system_light_base_size(
    count: i32,
    cap: i32,
    light_size: i32,
) -> Result<i32, crate::util::native_x87::NativeX87Error> {
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Ordering};
    // Original62E280: division, binary64 lower clamp7E3550,
    // binary32 upper clamp7E2AC8, multiplication then Math::ftol.
    // Consumed original controls: procedural_drawing_oracle/welding.json.
    let mut ratio = X::div(X::load_i32(count), X::load_i32(cap))?;
    let minimum = X::load_f64(NativeF64Bits::from_bits(0x3fd9_9999_9999_999a))?;
    let maximum = X::load_f32(NativeF32Bits::from_bits(0x3f80_0000))?;
    if X::compare(ratio, minimum) == X87Ordering::Less {
        ratio = minimum;
    }
    if X::compare(ratio, maximum) == X87Ordering::Greater {
        ratio = maximum;
    }
    Ok(X::ftol_i64(X::mul(ratio, X::load_i32(light_size)))? as i32)
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct CombatLightObservation {
    coord: [i32; 3],
    stage: u8,
    base_size: i32,
    flags: u32,
    surface_index: i32,
}

pub(crate) use crate::render::combat_light::CombatLightDrawRecord;

#[derive(Debug, Default)]
pub(crate) struct CombatLightRuntime {
    entries: Vec<CombatLight>,
}

impl CombatLightRuntime {
    pub(crate) fn observations(&self) -> impl Iterator<Item = CombatLightObservation> + '_ {
        self.entries.iter().map(|entry| CombatLightObservation {
            coord: [entry.coord.x, entry.coord.y, entry.coord.z],
            stage: entry.stage,
            base_size: entry.base_size,
            flags: entry.flags,
            surface_index: scaled_surface_index(entry.base_size, entry.stage),
        })
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Age the pre-existing vector before this logic frame's producers append.
    pub(crate) fn commit_frame(&mut self, new_entries: impl IntoIterator<Item = CombatLight>) {
        for entry in &mut self.entries {
            entry.stage = entry.stage.saturating_add(STAGE_STEP);
        }
        self.entries.retain(|entry| entry.stage < EXPIRE_STAGE);
        self.entries.extend(new_entries);
    }

    /// DrawAll5FFFA0 walks tail to head. Draw5FF860..5FF879 queries the
    /// shared mutative FPS threshold for every light without flags bits1..3,
    /// before projection. Options Detail gates Spark creation, not this draw.
    /// Original executable controls: electric_bolt.py `light_admission`.
    pub(crate) fn draw_records(&self, detail: &mut DetailState) -> Vec<CombatLightDrawRecord> {
        self.entries
            .iter()
            .rev()
            .filter_map(|entry| entry.draw_record(detail))
            .collect()
    }
}

/// Materialize current-frame receiver records into the final native light-vector
/// fields. Helper-only provenance (target, warhead, damage) intentionally stops
/// at this boundary.
pub(crate) fn materialize_combat_lights(
    impacts: Vec<CombatLightRequest>,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
    detail_level: u32,
    detail: &mut DetailState,
) -> Vec<CombatLight> {
    impacts
        .into_iter()
        .filter_map(|effect| match effect {
            CombatLightRequest::Spark { coord, base_size } => {
                (detail_level == 2).then_some(CombatLight {
                    coord,
                    base_size,
                    stage: 0,
                    flags: 0,
                })
            }
            CombatLightRequest::Impact {
                damage,
                warhead_ref,
                coord,
                force_create,
                flags,
                ..
            } => {
                // Flash48A620..48A65A queries mutable55AF60 before Bright,
                // including calls that produce no light. Force bypasses only
                // Bright; creation's low-nibble bypass differs from Draw's E.
                // Native controls: trigger_map_lighting flash_controls.
                let fps_admitted = detail_level != 0 && {
                    let minimum = detail.minimum_frame_rate();
                    detail.frame_rate() >= minimum
                };
                if !fps_admitted && flags & 0xF == 0 {
                    return None;
                }
                let warhead = rules.and_then(|rules| rules.warhead(interner.resolve(warhead_ref)));
                if !force_create && !warhead.is_some_and(|warhead| warhead.bright) {
                    return None;
                }
                let override_size = warhead.map_or(0.0, |warhead| warhead.combat_light_size_f64);
                Some(CombatLight {
                    coord,
                    stage: 0,
                    base_size: materialize_base_size(damage, override_size),
                    flags,
                })
            }
        })
        .collect()
}

fn materialize_base_size(damage: i32, override_size: f64) -> i32 {
    if override_size > 0.0 {
        // ReadDouble is f32-first; positive values are capped only at the high
        // end and Math::ftol chops the product toward zero.
        return (override_size.min(1.0) * 63.0) as i32;
    }
    // Preserve the actual signed 32-bit shifts. `/ 4` differs for negative
    // values and cannot reproduce the wrapping left shift.
    (damage.wrapping_shl(6) >> 8).clamp(0x15, 0x3f)
}

fn scaled_surface_index(base_size: i32, stage: u8) -> i32 {
    // Native indexes beyond the known90-byte table are undefined; preserve a
    // missing logical record instead of inventing another type16 light.
    let Some(&scale) = STAGE_SCALE.get(usize::from(stage)) else {
        return -1;
    };
    if scale >= 64 {
        i32::from(scale)
    } else {
        base_size.wrapping_mul(i32::from(scale)) / 64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::DetailRules;
    use crate::sim::world::Simulation;

    #[test]
    fn welding_immediate_light_size_stage_and_admission_match_original_system_draw() {
        let native = crate::render::tactical_compat::welding_tests::native();
        let stage_bytes =
            crate::render::tactical_compat::welding_tests::decode_hex(&native["stage_table_hex"]);
        assert_eq!(&STAGE_SCALE[..80], stage_bytes);
        let rows = native["system_draw_cases"].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let input = &row["input"];
            let light = &row["light"];
            if light.is_null() {
                continue;
            }
            let int = crate::render::tactical_compat::welding_tests::int;
            let size = system_light_base_size(
                int(&input["particle_count"]),
                int(&input["particle_cap"]),
                int(&input["light_size"]),
            )
            .unwrap();
            assert_eq!(
                serde_json::json!(size),
                light["base_size"],
                "{}",
                input["name"]
            );
            let mut detail = creation_detail(input);
            let position =
                crate::render::tactical_compat::welding_tests::array::<3>(&input["position"]);
            let actual = CombatLight {
                coord: ProjectileCoord::new(position[0], position[1], position[2]),
                stage: u8::try_from(int(&input["stage"])).unwrap(),
                base_size: size,
                flags: 0,
            }
            .draw_record(&mut detail);
            if let Some(performance_passed) = light["performance_passed"].as_bool() {
                assert_eq!(
                    actual.is_some(),
                    performance_passed,
                    "{} FPS admission",
                    input["name"]
                );
            }
            if let Some(actual) = actual {
                if !light["surface_index"].is_null() {
                    assert_eq!(
                        serde_json::json!(actual.surface_index),
                        light["surface_index"],
                        "{}",
                        input["name"]
                    );
                }
                assert_eq!(serde_json::json!(actual.flags), light["flags"]);
            }
        }
    }

    #[test]
    fn welding_points_keep_original_shared_unsigned_detail_query_history() {
        let native = crate::render::tactical_compat::welding_tests::native();
        for row in native["spark_draw_cases"].as_array().unwrap() {
            let mut detail = creation_detail(&row["input"]);
            for (index, draw) in row["draws"].as_array().unwrap().iter().enumerate() {
                let threshold = detail.minimum_frame_rate();
                assert_eq!(
                    serde_json::json!(threshold),
                    draw["threshold"],
                    "{} point{index}",
                    row["input"]["name"]
                );
                assert_eq!(
                    serde_json::json!(detail.frame_rate() >= threshold),
                    draw["performance_passed"],
                    "{} point{index}",
                    row["input"]["name"]
                );
                assert_eq!(
                    serde_json::to_value(detail.observation()).unwrap()["reduced"],
                    draw["reduced_after"],
                    "{} point{index}",
                    row["input"]["name"]
                );
            }
        }
    }

    fn original_light_corpus() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/electric_bolt.json",
        ))
        .unwrap()
    }

    fn native_light_fields(light: &CombatLight) -> serde_json::Value {
        serde_json::json!({
            "position": [light.coord.x, light.coord.y, light.coord.z],
            "size": light.base_size,
            "flags": light.flags,
            "ramp": light.stage,
        })
    }

    fn detail_with_frame_rate(frame_rate: u32) -> DetailState {
        let mut detail = DetailState::new();
        for _ in 0..frame_rate {
            detail.record_logic_visit();
        }
        detail.throttle_tail(0);
        detail
    }

    fn creation_detail(input: &serde_json::Value) -> DetailState {
        let mut detail = DetailState::new();
        if input["reduced"] == 1 || input["reduced"] == true {
            detail.minimum_frame_rate();
        }
        detail.configure_normal(&DetailRules {
            min_frame_rate_normal: input["minimum"].as_i64().unwrap() as i32,
            buffer_zone_width: input["buffer"].as_i64().unwrap() as i32,
            ..DetailRules::default()
        });
        for _ in 0..input["fps"].as_u64().unwrap() {
            detail.record_logic_visit();
        }
        detail.throttle_tail(0);
        detail
    }

    #[test]
    fn original_light_fps_gate_queries_each_eligible_record_in_reverse_order() {
        let corpus = original_light_corpus();
        let cases = corpus["light_admission"].as_array().unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let input = &case["input"];
            let name = input["name"].as_str().unwrap();
            let mut detail = DetailState::new();
            if input["reduced"].as_bool().unwrap() {
                // Establish the retained latch through its existing owner,
                // before selecting this case's possibly signed thresholds.
                detail.minimum_frame_rate();
            }
            detail.configure_normal(&DetailRules {
                min_frame_rate_normal: input["minimum"].as_i64().unwrap() as i32,
                buffer_zone_width: input["buffer"].as_i64().unwrap() as i32,
                ..DetailRules::default()
            });
            for _ in 0..input["fps"].as_u64().unwrap() {
                detail.record_logic_visit();
            }
            detail.throttle_tail(0);
            let coordinate = |value: &serde_json::Value| -> [i32; 3] {
                std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
            };
            let mut runtime = CombatLightRuntime::default();
            runtime.commit_frame(input["lights"].as_array().unwrap().iter().map(|entry| {
                let [x, y, z] = coordinate(&entry["position"]);
                CombatLight {
                    coord: ProjectileCoord { x, y, z },
                    stage: 0,
                    base_size: 15,
                    flags: entry["flags"].as_u64().unwrap() as u32,
                }
            }));
            for (visit, expected) in case["visits"].as_array().unwrap().iter().enumerate() {
                let actual: Vec<_> = runtime
                    .draw_records(&mut detail)
                    .iter()
                    .map(|entry| ([entry.coord.x, entry.coord.y, entry.coord.z], entry.flags))
                    .collect();
                // A recorded projection result means the original FPS gate
                // admitted the light. Projection/rectangle clipping happens
                // after this app-owned gate; offscreen controls still query.
                let expected_records: Vec<_> = expected["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|entry| !entry["projected"].is_null())
                    .map(|entry| {
                        (
                            coordinate(&entry["position"]),
                            entry["flags"].as_u64().unwrap() as u32,
                        )
                    })
                    .collect();
                assert_eq!(actual, expected_records, "{name}, visit {visit}");
                let observed = serde_json::to_value(detail.observation()).unwrap();
                assert_eq!(
                    observed["reduced"], expected["reduced"],
                    "{name}, visit {visit}"
                );
            }
        }
    }

    #[test]
    fn original_spark_light_materialization_uses_current_detail_only_at_creation() {
        let corpus = original_light_corpus();
        let interner = StringInterner::new();
        let mut details = std::collections::BTreeSet::new();
        for case in corpus["spark_cases"].as_array().unwrap() {
            let input = &case["input"];
            if !input["system_type_overrides"].is_null() || input["fail_system_allocation"] == true
            {
                // The particle owner compares those upstream admission gates.
                // This boundary receives an already-admitted stock Spark fact.
                continue;
            }
            let detail = input["detail"].as_u64().unwrap_or(2) as u32;
            details.insert(detail);
            let position: [i32; 3] =
                serde_json::from_value(case["endpoint"]["target"].clone()).unwrap();
            let [x, y, z] = position;
            let size = case["effective_types"]["system"]["light_size"]
                .as_i64()
                .unwrap() as i32;
            let lights = materialize_combat_lights(
                vec![CombatLightRequest::Spark {
                    coord: ProjectileCoord { x, y, z },
                    base_size: size,
                }],
                None,
                &interner,
                detail,
                &mut detail_with_frame_rate(60),
            );
            let expected = &case["first_logic_visit"]["state"]["lights"];
            assert_eq!(
                serde_json::json!(lights.iter().map(native_light_fields).collect::<Vec<_>>()),
                *expected,
                "{}",
                input["name"]
            );
            let mut runtime = CombatLightRuntime::default();
            runtime.commit_frame(lights);
            // Later drawing has no Options Detail input: changing it cannot
            // remove an already-created native light or replace the FPS gate.
            assert_eq!(
                runtime.draw_records(&mut detail_with_frame_rate(60)).len(),
                expected.as_array().unwrap().len()
            );
        }
        assert!(details.contains(&0) && details.contains(&1) && details.contains(&2));
    }

    #[test]
    fn original_light_retains_signed_size_and_ages_through_shared_runtime() {
        let corpus = original_light_corpus();
        let mut sizes = std::collections::BTreeMap::<i32, Vec<&serde_json::Value>>::new();
        for row in corpus["light_indices"]["rows"].as_array().unwrap() {
            sizes
                .entry(row["size"].as_i64().unwrap() as i32)
                .or_default()
                .push(row);
        }
        assert!(!sizes.is_empty());
        for (size, rows) in sizes {
            let mut runtime = CombatLightRuntime::default();
            let mut detail = detail_with_frame_rate(60);
            runtime.commit_frame([CombatLight {
                coord: ProjectileCoord { x: 0, y: 0, z: 0 },
                stage: 0,
                base_size: size,
                flags: 0,
            }]);
            for (visit, row) in rows.into_iter().enumerate() {
                if visit != 0 {
                    runtime.commit_frame([]);
                }
                let draw = runtime.draw_records(&mut detail);
                assert_eq!(
                    runtime.entries[0].stage,
                    row["stage"].as_u64().unwrap() as u8
                );
                assert_eq!(draw[0].surface_index, row["index"].as_i64().unwrap() as i32);
            }
        }
        for case in corpus["spark_cases"].as_array().unwrap() {
            let lights = case["first_logic_visit"]["state"]["lights"]
                .as_array()
                .unwrap();
            let mut runtime = CombatLightRuntime::default();
            runtime.commit_frame(lights.iter().map(|light| {
                let [x, y, z]: [i32; 3] =
                    serde_json::from_value(light["position"].clone()).unwrap();
                CombatLight {
                    coord: ProjectileCoord { x, y, z },
                    stage: light["ramp"].as_u64().unwrap() as u8,
                    base_size: light["size"].as_i64().unwrap() as i32,
                    flags: light["flags"].as_u64().unwrap() as u32,
                }
            }));
            for expected in case["light_updates"].as_array().unwrap() {
                runtime.commit_frame([]);
                let native = expected.as_array().unwrap();
                let count = native.last().unwrap()["registry_count"].as_u64().unwrap() as usize;
                assert_eq!(runtime.len(), count, "{}", case["input"]["name"]);
                assert_eq!(
                    runtime
                        .entries
                        .iter()
                        .map(native_light_fields)
                        .collect::<Vec<_>>(),
                    native[..count],
                    "{}",
                    case["input"]["name"]
                );
            }
        }
    }

    #[test]
    fn map_lighting_transylv_large_flash_keeps_original_non_bright_c4_no_light_result() {
        let Some(fixture) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Transylv.yro")
        else {
            return;
        };
        let rules = fixture.rules;
        let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_map_lighting.json");
        let producer = &corpus["flash_controls"];
        let wh = rules.warhead(&rules.bridge_warheads.c4_name).unwrap();
        assert_eq!(wh.id, producer["warhead_inputs"]["c4"].as_str().unwrap());
        assert_eq!(
            wh.bright,
            producer["warhead_inputs"]["after"]["bright"] == 1
        );
        let size_hex = (wh.combat_light_size_f64 as f32)
            .to_le_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            size_hex,
            producer["warhead_inputs"]["after"]["size_f32_le_hex"]
                .as_str()
                .unwrap()
        );
        for row in producer["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["definition"]["kind"] == 66)
        {
            let call = &row["calls"][0];
            let mut sim = Simulation::with_seed(0);
            let impact = CombatLightRequest::Impact {
                target_id: None,
                damage: call["damage"].as_i64().unwrap() as i32,
                warhead_ref: sim.intern(call["warhead"].as_str().unwrap()),
                coord: ProjectileCoord::new(
                    call["arguments"][0].as_i64().unwrap() as i32,
                    call["arguments"][1].as_i64().unwrap() as i32,
                    call["arguments"][2].as_i64().unwrap() as i32,
                ),
                force_create: call["arguments"][3] != 0,
                flags: call["arguments"][4].as_u64().unwrap() as u32,
            };
            assert_eq!(
                materialize_combat_lights(
                    vec![impact],
                    Some(&rules),
                    sim.interner(),
                    2,
                    &mut detail_with_frame_rate(60),
                )
                .len(),
                row["light_constructor_calls"].as_u64().unwrap() as usize
            );
        }
    }

    #[test]
    fn map_lighting_flash_creation_preserves_original_admission_and_shared_hysteresis() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_map_lighting.json");
        let controls = &corpus["flash_controls"];
        for row in controls["creation_admission"].as_array().unwrap() {
            let input = &row["input"];
            let bright = if input["bright"] == 1 { "yes" } else { "no" };
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Warheads]\n0=WH\n[WH]\nBright={bright}\n",
            )))
            .unwrap();
            let mut sim = Simulation::new();
            let mut detail = creation_detail(input);
            let request = CombatLightRequest::Impact {
                target_id: None,
                damage: input["damage"].as_i64().unwrap() as i32,
                warhead_ref: sim.intern("WH"),
                coord: ProjectileCoord::new(0, 0, 0),
                force_create: input["force"] != 0,
                flags: input["flags"].as_u64().unwrap() as u32,
            };
            let lights = materialize_combat_lights(
                vec![request],
                Some(&rules),
                sim.interner(),
                input["detail"].as_u64().unwrap() as u32,
                &mut detail,
            );
            assert_eq!(
                serde_json::json!(
                    lights
                        .iter()
                        .map(|light| light.base_size)
                        .collect::<Vec<_>>()
                ),
                row["constructor_sizes"],
                "{}",
                input["name"]
            );
            assert_eq!(
                serde_json::to_value(detail.observation()).unwrap()["reduced"],
                row["reduced_after"] == 1,
                "{}",
                input["name"]
            );
        }

        let history = &controls["hysteresis_history"];
        let input = &history["input"]["first_flash"];
        let rules = RuleSet::from_ini(&IniFile::from_str("[Warheads]\n0=WH\n[WH]\n")).unwrap();
        let mut sim = Simulation::new();
        let mut detail = creation_detail(input);
        let request = CombatLightRequest::Impact {
            target_id: None,
            damage: input["damage"].as_i64().unwrap() as i32,
            warhead_ref: sim.intern("WH"),
            coord: ProjectileCoord::new(0, 0, 0),
            force_create: input["force"] != 0,
            flags: input["flags"].as_u64().unwrap() as u32,
        };
        assert!(
            materialize_combat_lights(
                vec![request],
                Some(&rules),
                sim.interner(),
                input["detail"].as_u64().unwrap() as u32,
                &mut detail,
            )
            .is_empty()
        );
        assert_eq!(
            serde_json::to_value(detail.observation()).unwrap()["reduced"],
            history["after_flash_reduced"] == 1
        );
        let mut omitted_flash = creation_detail(input);
        for state in [&mut detail, &mut omitted_flash] {
            for _ in 0..history["input"]["next_fps"].as_u64().unwrap() {
                state.record_logic_visit();
            }
            state.throttle_tail(60 * 16);
        }
        assert_eq!(
            serde_json::json!(detail.minimum_frame_rate()),
            history["with_flash_next_threshold"]
        );
        assert_eq!(
            serde_json::json!(omitted_flash.minimum_frame_rate()),
            history["without_flash_next_threshold"]
        );
    }
}

#[cfg(test)]
mod spotlight_tests {
    use super::*;
    #[test]
    fn spotlight_stages_use_original_surface_bank_without_scale_multiplication() {
        let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
        let hex = STAGE_SCALE
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(hex, native["stage_table_hex"].as_str().unwrap());
        assert_eq!(scaled_surface_index(16, 90), -1);
        for size in [16, 0, -1, i32::MAX] {
            for stage in 80..=89 {
                assert_eq!(scaled_surface_index(size, stage), i32::from(stage) - 16);
            }
        }
    }
}
