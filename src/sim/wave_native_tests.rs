//! Original executable state continuations, separate from the ordinary shot test.

use super::*;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/magnetic_wave.json")
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(integer(&value[0]), integer(&value[1]), integer(&value[2]))
}

fn bytes<const N: usize>(value: &Value) -> [u8; N] {
    let text = value.as_str().unwrap();
    assert_eq!(text.len(), N * 2);
    std::array::from_fn(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
}

fn bits64(value: &Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(u64::from_le_bytes(bytes(value)))
}

fn context(input: &Value) -> WaveUpdateContext {
    WaveUpdateContext {
        owner_position: Some(coord(&input["source"])),
        owner_fire_coordinate: Some(coord(&input["source_flh"])),
        owner_current_target: Some(TargetKind::Entity(2)),
        target_position: Some(coord(&input["target_center"])),
        target_is_unit: true,
    }
}

fn edges(native: &Value) -> WaveEdgeGeometry {
    WaveEdgeGeometry {
        firer_a: coord(&native["corners"][0]),
        firer_b: coord(&native["corners"][1]),
        target_a: coord(&native["corners"][2]),
        target_b: coord(&native["corners"][3]),
    }
}

/// A supplied native continuation state, never a second expected algorithm.
fn from_native(native: &Value) -> Wave {
    let mut wave = Wave::new_owned(
        integer(&native["wave_type"]) as u8,
        1,
        TargetKind::Entity(2),
        coord(&native["source"]),
        coord(&native["target"]),
    );
    wave.edge_geometry = edges(native);
    wave.direction_octant = integer(&native["direction"]);
    wave.lifetime = integer(&native["phase"]);
    wave.type3_cycle_count = integer(&native["restart"]);
    wave.fade_in = bits64(&native["front_f64_le_hex"]);
    wave.fade_out = bits64(&native["back_f64_le_hex"]);
    wave.active_geometry = native["active"].as_bool().unwrap();
    wave.decaying = native["decay"].as_bool().unwrap();
    wave.recorded_cells = native["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| WaveRecordedCell::real(integer(&cell[0]) as u16, integer(&cell[1]) as u16))
        .collect();
    wave
}

fn assert_state(wave: &Wave, native: &Value, alive: bool) {
    assert_eq!(wave.source, coord(&native["source"]));
    assert_eq!(wave.target, coord(&native["target"]));
    assert_eq!(wave.edge_geometry, edges(native));
    assert_eq!(wave.direction_octant, integer(&native["direction"]));
    assert_eq!(wave.lifetime, integer(&native["phase"]));
    assert_eq!(wave.type3_cycle_count, integer(&native["restart"]));
    assert_eq!(wave.fade_in, bits64(&native["front_f64_le_hex"]));
    assert_eq!(wave.fade_out, bits64(&native["back_f64_le_hex"]));
    assert_eq!(wave.active_geometry, native["active"].as_bool().unwrap());
    assert_eq!(wave.decaying, native["decay"].as_bool().unwrap());
    assert_eq!(alive, native["alive"].as_bool().unwrap());
    let cells = wave
        .recorded_cells
        .iter()
        .map(|cell| {
            let cell = cell.current_cell(None);
            [cell.rx, cell.ry]
        })
        .collect::<Vec<_>>();
    let expected_cells = native["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| [integer(&cell[0]), integer(&cell[1])])
        .collect::<Vec<_>>();
    assert_eq!(
        cells, expected_cells,
        "original UpdateCells pointer encounter order"
    );
    if alive {
        // Remove the common presentation-owner bias BEFORE interpolation.
        // The original component camera is zero; this constant is not fitted.
        let camera = [0, crate::util::lepton::WORLD_ROW_BIAS_PX];
        let polygon = if wave.wave_type == 3 {
            wave.magnetic_draw_geometry(camera)
                .unwrap()
                .polygon
                .to_vec()
        } else {
            wave.sonic_draw_polygon(camera).unwrap().to_vec()
        };
        let expected = native["polygon"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| [integer(&point[0]), integer(&point[1])])
            .collect::<Vec<_>>();
        assert_eq!(
            polygon, expected,
            "original projected fade precedes drawing"
        );
    }
}

#[test]
fn retail_tele_and_mtnk_read_the_native_magnetic_and_lift_inputs() {
    let map = crate::test_fixture::checkout_root()
        .join("tools/procedural_drawing_oracle/magnetic_wave/ordinary-tele-magnetic.map");
    let Some(retail) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map(map.to_str().unwrap())
    else {
        return;
    };
    let rules = &retail.rules;
    let expected = &corpus()["inputs"]["final"];
    for name in ["TELE", "MTNK"] {
        let native = &expected["types"][name];
        let object = rules.object(name).unwrap();
        assert_eq!(object.strength, integer(&native["strength"]));
        assert_eq!(
            native["locomotor_clsid_hex"],
            "4127584a3998d111b70900a024ddafd1"
        );
        assert_eq!(
            object.locomotor,
            crate::rules::locomotor_type::LocomotorKind::Drive
        );
        if name == "TELE" {
            // The native selected cache admits weapon/FLH reads for the
            // firer. MTNK's weapons are outside this target-only component.
            assert_eq!(object.primary(), native["primary"].as_str());
            assert_eq!(object.elite_primary(), native["elite_primary"].as_str());
            let art = crate::sim::combat::fire_coord::firer_art(rules, object).unwrap();
            assert_eq!(
                <[i32; 3]>::from(art.primary_fire_flh),
                [
                    integer(&native["primary_flh"][0]),
                    integer(&native["primary_flh"][1]),
                    integer(&native["primary_flh"][2])
                ]
            );
            assert_eq!(art.turret_offset, integer(&native["turret_offset"]));
        }
        // Selected native D70..D94 block. Padding has no gameplay authority.
        let block = bytes::<40>(&native["jumpjet_block_hex"]);
        let raw = |offset| u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap());
        let params = &object.jumpjet_params;
        assert_eq!(params.turn_rate, raw(0) as i32);
        assert_eq!(
            params.speed,
            crate::util::fixed_math::SimFixed::from_num(raw(4) as i32)
        );
        assert_eq!(params.climb.to_bits(), raw(8));
        assert_eq!(params.crash.to_bits(), raw(12));
        assert_eq!(params.height, raw(16) as i32);
        assert_eq!(params.accel.to_bits(), raw(20));
        assert_eq!(params.wobbles.to_bits(), raw(24));
        assert_eq!(params.no_wobbles, block[28] != 0);
        assert_eq!(params.deviation, raw(32) as i32);
        assert_eq!(object.jumpjet, block[36] != 0);
    }
    for (name, native) in expected["weapons"].as_object().unwrap() {
        let weapon = rules.weapon(name).unwrap();
        assert_eq!(weapon.damage, integer(&native["damage"]));
        assert_eq!(weapon.range_leptons, integer(&native["range_leptons"]));
        assert_eq!(weapon.projectile.as_deref(), native["projectile"].as_str());
        assert_eq!(weapon.warhead.as_deref(), native["warhead"].as_str());
        for (actual, key) in [
            (weapon.is_sonic, "is_sonic"),
            (weapon.is_laser, "is_laser"),
            (weapon.is_electric_bolt, "is_electric_bolt"),
            (weapon.is_rad_beam, "is_rad_beam"),
            (weapon.is_rad_eruption, "is_rad_eruption"),
            (weapon.is_mag_beam, "is_mag_beam"),
        ] {
            assert_eq!(actual, native[key].as_bool().unwrap(), "{name}/{key}");
        }
    }
    let native_wh = &expected["warheads"]["LocomotorBeam"];
    let warhead = rules.warhead("LocomotorBeam").unwrap();
    assert_eq!(
        warhead.is_locomotor,
        native_wh["is_locomotor"].as_bool().unwrap()
    );
    assert_eq!(
        native_wh["locomotor_clsid_hex"],
        "462c61921ff7d111ac9f006008055bb5"
    );
    assert_eq!(
        warhead.locomotor,
        crate::rules::locomotor_type::LocomotorKind::Jumpjet
    );
    let native = &expected["projectiles"]["InvisibleHigh"];
    let projectile = rules.projectile("InvisibleHigh").unwrap();
    assert_eq!(projectile.inviso, native["inviso"].as_bool().unwrap());
    assert_eq!(
        projectile.subject_to_elevation,
        native["subject_to_elevation"].as_bool().unwrap()
    );
}

#[test]
fn original_shared_wave_geometry_in_all_octants_elevation_and_converged_xy() {
    let mut directions = [[false; 8]; 2];
    for key in ["geometry_rows", "direct_geometry_rows"] {
        for row in corpus()[key].as_array().unwrap() {
            let input = &row["input"];
            let wave_type = integer(&input["wave_type"]) as u8;
            let expected = if key == "geometry_rows" {
                &row["initial_geometry"]
            } else {
                &row["after"]
            };
            if expected.is_null() {
                let rejected =
                    Wave::new(wave_type, coord(&input["source"]), coord(&input["target"]));
                assert!(
                    !rejected.constructor_distance_is_live(),
                    "original ctor rejects zero XY"
                );
                continue;
            }
            let geometry =
                wave_geometry(wave_type, coord(&input["source"]), coord(&input["target"]));
            assert_eq!(geometry.source, coord(&expected["source"]), "{input}");
            assert_eq!(geometry.target, coord(&expected["target"]), "{input}");
            assert_eq!(geometry.edges, edges(expected), "{input}");
            if key == "geometry_rows" {
                assert_eq!(
                    geometry.direction_octant,
                    integer(&expected["direction"]),
                    "{input}"
                );
                directions[usize::from(wave_type == 3)][geometry.direction_octant as usize] = true;
            }
        }
    }
    assert!(directions.into_iter().flatten().all(|covered| covered));
}

#[test]
fn original_wave_phase_threshold_and_double_fades_reach_native_cells_and_polygons() {
    let constants = &corpus()["numeric_constants"];
    assert_eq!(
        STEP_F32.bits().to_le_bytes(),
        bytes::<4>(&constants["step_f32_le_hex"])
    );
    assert_eq!(SNAP_FADE_F64, bits64(&constants["snap_f64_le_hex"]));
    assert_eq!(NativeF64Bits::HALF, bits64(&constants["half_f64_le_hex"]));
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(80);
    for row in corpus()["phase_rows"].as_array().unwrap() {
        let mut wave = from_native(&row["before"]);
        let terminal = wave.update_geometry_and_cells(context(&row["input"]), Some(&terrain));
        assert_state(&wave, &row["after"], !terminal);
        for call in row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["name"] == "update")
        {
            for threshold in call["phase_thresholds"].as_array().unwrap() {
                let actual = x87_ftol_i32(
                    X87Chop53::div(load_f64(NativeF64Bits::ONE), load_f32(STEP_F32)).unwrap(),
                );
                assert_eq!(actual as u32, threshold["low32"].as_u64().unwrap() as u32);
            }
        }
    }
    for row in corpus()["lifecycle_rows"].as_array().unwrap() {
        let history = row["history"].as_array().unwrap();
        let mut wave = from_native(&history[0]["after"]);
        let mut ctx = context(&row["input"]);
        let ai = wave.wave_type == 3;
        let mut previous_visit = 0;
        for state in history.iter().skip(1) {
            let visit = integer(&state["visit"]);
            for _ in previous_visit..visit {
                if ai {
                    assert!(wave.advance(ctx, Some(&terrain)).alive);
                } else {
                    assert!(!wave.update_geometry_and_cells(ctx, Some(&terrain)));
                }
            }
            assert_state(&wave, &state["after"], true);
            previous_visit = visit;
        }
        ctx.owner_current_target = None;
        previous_visit = 0;
        for state in row["fade"].as_array().unwrap() {
            let visit = integer(&state["visit"]);
            let mut alive = true;
            for _ in previous_visit..visit {
                alive = if ai {
                    wave.advance(ctx, Some(&terrain)).alive
                } else {
                    !wave.update_geometry_and_cells(ctx, Some(&terrain))
                };
            }
            assert_state(&wave, &state["after"], alive);
            previous_visit = visit;
        }
    }
}

#[test]
fn sonic_damage_request_uses_the_native_b4_bridge_plane() {
    // DamageArea75F3C6 loads Wave+BC (B4.Z), then75F3D3 compares it
    // with (cell level+4)*levelheight. Reuse whole native Update inputs and
    // outputs to bind the existing receiver request to that actual endpoint.
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(80);
    for row in corpus()["phase_rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["wave_type"] == 0)
    {
        let mut store = WaveStore::new();
        store.spawn(7, from_native(&row["before"]));
        let (request, _) = store
            .advance_one(7, context(&row["input"]), Some(&terrain))
            .unwrap();
        assert_eq!(
            request.unwrap().wave_z,
            coord(&row["after"]["source"]).z,
            "native bridge-plane input comes from B4, independently of C0+50"
        );
    }
}

#[test]
fn original_fire_at_births_match_the_production_magnetic_commit() {
    // Each row is one original FireAt tail (0x006FF5F5..0x006FF656) for the
    // retained TELE/MTNK pose: the constructor (target coordinate first,
    // launch source second) and its immediate Update_Wave.
    let map = crate::test_fixture::checkout_root()
        .join("tools/procedural_drawing_oracle/magnetic_wave/ordinary-tele-magnetic.map");
    let Some(retail) =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map(map.to_str().unwrap())
    else {
        return;
    };
    let rules = &retail.rules;
    for row in corpus()["birth_rows"].as_array().unwrap() {
        let input = &row["input"];
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 0x1269, 64);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        let source = sim
            .spawn_object("TELE", "Americans", 44, 48, 0, rules)
            .unwrap();
        let target = sim
            .spawn_object("MTNK", "Russians", 50, 48, 0, rules)
            .unwrap();
        for (id, key) in [(source, "source"), (target, "target")] {
            let location = crate::sim::movement::ground_pose::object_location(
                sim.substrate.entities.get(id).unwrap(),
                sim.resolved_terrain.as_ref(),
            );
            assert_eq!(
                ProjectileCoord::new(location.x, location.y, location.z),
                coord(&input[key])
            );
        }
        let heading = integer(&input["heading"]) as u16;
        let firer = sim.substrate.entities.get_mut(source).unwrap();
        // The harness sets both of the firer's facings (+0x388, +0x3A0).
        firer.body_facing =
            crate::sim::movement::FacingClass::with_rate_of(heading, &firer.body_facing);
        if let Some(barrel) = firer.barrel_facing.as_mut() {
            *barrel = crate::sim::movement::FacingClass::with_rate_of(heading, barrel);
        }
        firer
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target)));
        let firer = sim.substrate.entities.get(source).unwrap();
        let object = sim.interner.object_type(firer.type_ref(), rules).unwrap();
        let fire = crate::sim::combat::fire_coord::fire_coordinate(
            &sim,
            rules,
            &crate::sim::combat::fire_coord::FireSource::of_entity(firer),
            object,
            0,
            0,
            crate::rules::flh::Flh::default(),
        );
        assert_eq!(fire.coord, coord(&input["source_flh"]), "GetFLH");
        let mut event = crate::sim::world::SimFireEvent::for_test(source);
        event.target = TargetKind::Entity(target);
        event.fire_coord = fire.coord;
        sim.commit_fired_wave(rules, &event, 3);
        let wave = sim.waves.get(sim.active_wave_id(source).unwrap()).unwrap();
        assert_state(wave, &row["after"], true);
    }
}

#[test]
fn original_update_reverses_only_a_unit_target() {
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(80);
    // The first birth's Wave, updated as the held MTNK is lifted.
    let mut wave = from_native(&corpus()["birth_rows"][0]["after"]);
    for row in corpus()["tracking_rows"].as_array().unwrap() {
        let terminal = wave.update_geometry_and_cells(context(&row["input"]), Some(&terrain));
        assert_state(&wave, &row["after"], !terminal);
    }
    // A Cell target (What_Am_I != Unit) keeps the owner's GetFLH first.
    let row = &corpus()["nonunit_rows"][0];
    let input = &row["input"];
    let cell = TargetKind::Cell(50, 48);
    let mut wave = Wave::new_owned(
        3,
        1,
        cell,
        coord(&input["constructor_source"]),
        coord(&input["constructor_target"]),
    );
    let context = WaveUpdateContext {
        owner_position: Some(coord(&input["source"])),
        owner_fire_coordinate: Some(coord(&input["source_flh"])),
        owner_current_target: Some(cell),
        target_position: Some(coord(&input["target_center"])),
        target_is_unit: false,
    };
    let terminal = wave.initialize(context, Some(&terrain));
    assert_state(&wave, &row["after"], !terminal);
}
