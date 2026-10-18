//! Original RadSite/Cell operation and store boundaries, including the
//! overlap threshold that changes a stock DOG's first-pulse survival.

use super::*;
use crate::util::save_codec;

fn integer(value: &serde_json::Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn cell(value: &serde_json::Value) -> (u16, u16) {
    (
        u16::try_from(value[0].as_u64().unwrap()).unwrap(),
        u16::try_from(value[1].as_u64().unwrap()).unwrap(),
    )
}

/// Full native activation/AI/removal, including the independent light clock.
/// The before-AI refresh is an explicit component input, separate from the
/// paid opening's Bullet phase. Neither history imports Rust expected values.
#[test]
fn light_lifecycle_matches_original_site_and_cell_boundaries() {
    assert_light_lifecycle(
        "XMP03T4.MAP",
        "tools/spatial_oracle/radiation_light_lifecycle.json",
    );
}

#[test]
fn snow_light_lifecycle_matches_original_site_and_cell_boundaries() {
    assert_light_lifecycle(
        "XXmas.MAP",
        "tools/spatial_oracle/radiation_snow_light_lifecycle.json",
    );
}

fn assert_light_lifecycle(map: &str, packet: &'static str) {
    use crate::sim::light_sources::LightingEvent;

    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let scene = crate::headless_scenario::load(&root, map, 0).unwrap();
    let rules = &scene.runtime.resources.rules;
    let terrain = scene.sim().resolved_terrain.as_ref();
    let native = crate::test_fixture::json(packet);
    for history in native["histories"].as_array().unwrap() {
        let name = history["name"].as_str().unwrap();
        let input = &history["input"];
        assert_eq!(
            scene.sim().session.theater,
            input["setup"]["theater"]["source_value"].as_str().unwrap()
        );
        let center = cell(&input["center"]);
        let weapon = rules
            .weapon(input["source"]["weapon"].as_str().unwrap())
            .unwrap();
        assert_eq!(weapon.rad_level, integer(&input["source"]["rad_level"]));
        let spread = crate::sim::combat::cell_spread::whole_cells(
            rules
                .warhead(weapon.warhead.as_deref().unwrap())
                .unwrap()
                .cell_spread_f64,
        );
        assert_eq!(spread, integer(&input["source"]["spread"]));
        for supplied in input["setup"]["geometry"]["allocated"].as_array().unwrap() {
            let at = cell(&supplied["cell"]);
            let actual = terrain.unwrap().cell(at.0, at.1).unwrap();
            assert_eq!(i32::from(actual.level), integer(&supplied["level"]));
            assert_eq!(i32::from(actual.slope_type), integer(&supplied["slope"]));
        }
        let detonation = RadDetonation {
            rx: center.0,
            ry: center.1,
            rad_level: weapon.rad_level,
            spread,
        };
        let mut state = RadiationState::default();
        let mut events = Vec::new();
        let mut source = None;
        let mut publish = |events: &mut Vec<LightingEvent>| {
            for event in events.drain(..) {
                match event {
                    LightingEvent::Radiation {
                        center: at,
                        source: next,
                    } => {
                        assert_eq!(at, center);
                        source = next;
                    }
                    other => panic!("unexpected radiation owner publication {other:?}"),
                }
            }
            source.clone()
        };
        state.apply_detonation(
            detonation,
            0,
            &scene.sim().session.theater,
            &rules.radiation,
            terrain,
            &mut events,
        );
        let mut current_source = publish(&mut events);
        let mut frame = 0;
        let refresh = input["refresh_frame"].as_u64().map(|frame| frame as u32);
        for stage in history["stages"].as_array().unwrap() {
            let expected = &stage["state"];
            let end = expected["frame"].as_u64().unwrap() as u32;
            while frame < end {
                frame += 1;
                if refresh == Some(frame) {
                    state.apply_detonation(
                        detonation,
                        frame,
                        &scene.sim().session.theater,
                        &rules.radiation,
                        terrain,
                        &mut events,
                    );
                }
                state.tick_decay(frame, &rules.radiation, terrain, &mut events);
                current_source = publish(&mut events);
            }
            if expected["site"].is_null() {
                assert!(
                    state.site_at(center).is_none(),
                    "{name} frame{frame}: removal"
                );
                assert!(
                    current_source.is_none(),
                    "{name} frame{frame}: source removal"
                );
            } else {
                let actual = state.site_at(center).unwrap();
                let site = &expected["site"];
                for (value, key) in [
                    (actual.radius_leptons, "radius"),
                    (actual.level, "level"),
                    (actual.level_steps, "level_steps"),
                    (actual.duration, "duration"),
                    (actual.remaining, "remaining"),
                ] {
                    assert_eq!(value, integer(&site[key]), "{name} frame{frame} {key}");
                }
                assert_eq!(
                    actual.level_timer.start_frame(),
                    integer(&site["level_timer"][0]),
                    "{name} frame{frame}"
                );
                assert_eq!(
                    actual.level_timer.duration(),
                    integer(&site["level_timer"][1]),
                    "{name} frame{frame}"
                );
                assert_eq!(
                    serde_json::json!([
                        actual.light.timer.start_frame(),
                        actual.light.timer.duration()
                    ]),
                    site["light_timer"],
                    "{name} frame{frame}: independent light clock"
                );
                assert_eq!(serde_json::json!(actual.light.base_tint), site["tint"]);
                assert_eq!(actual.light.decrement, integer(&site["light_decrement"]));
                let light = &expected["light"];
                let actual = current_source.as_ref().expect("retained native source");
                assert_eq!(
                    actual.intensity,
                    integer(&light["intensity"]),
                    "{name} frame{frame}: source intensity"
                );
                assert_eq!(
                    serde_json::json!(actual.tint),
                    light["tint"],
                    "{name} frame{frame}: source tint"
                );
                assert_eq!(actual.radius_leptons, integer(&light["radius"]));
                assert_eq!(actual.active, light["active"].as_u64().unwrap() != 0);
            }
            for sample in expected["samples"].as_array().unwrap() {
                let at = cell(&sample["cell"]);
                assert_eq!(
                    format!("{:016x}", state.cell_level(at).to_bits()),
                    sample["radiation_bits"],
                    "{name} frame{frame} Cell{at:?}"
                );
                let registered = state.site_at(at).is_some();
                assert_eq!(
                    registered,
                    sample["radsite"].as_u64().unwrap() != 0,
                    "{name} frame{frame}: Cell registration{at:?}"
                );
            }
            if name == "same_center_refresh" && frame == 134 {
                // Persist the actual native-bound component immediately
                // before its independent light deadline. This empty World
                // envelope is a snapshot control, not a paid-game bootstrap.
                let mut world = crate::sim::world::Simulation::with_seed(0);
                world.session.binary_frame = frame;
                world.session.theater = scene.sim().session.theater.clone();
                world.radiation = state.clone();
                let before = world.state_hash();
                let bytes = crate::sim::snapshot::GameSnapshot::save(
                    &world,
                    0,
                    0,
                    "native-radiation-clock",
                    0,
                );
                let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                loaded.restore_after_snapshot_load().unwrap();
                assert_eq!(
                    save_codec::serialize(&loaded.radiation).unwrap(),
                    save_codec::serialize(&state).unwrap(),
                    "load must preserve the source and independent clock"
                );
                world
                    .radiation
                    .sites
                    .get_mut(&center)
                    .unwrap()
                    .light
                    .timer
                    .start(46, 90);
                assert_ne!(
                    before,
                    world.state_hash(),
                    "retained source clock is hashed"
                );
                // Continue through the original135 boundary using the loaded
                // owner, rather than reconstructing a projection from age.
                state = loaded.radiation;
            }
        }
        assert!(state.sites().next().is_none());
    }
}

/// Explicit numeric priors execute in the same native runtime after its real
/// lifecycles retire. These controls certify the consumed integer/gate bodies,
/// without claiming native constructors for the supplied operand storage.
#[test]
fn light_parameters_match_original_integer_and_update_controls() {
    let native = crate::test_fixture::json("tools/spatial_oracle/radiation_light_lifecycle.json");
    let tint = |value: &serde_json::Value| std::array::from_fn(|i| integer(&value[i]));
    let rows = native["tint_integer_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let input = &row["input"];
        assert_eq!(
            serde_json::json!(fade_rad_tint(
                tint(&input["base"]),
                integer(&input["remaining"]),
                integer(&input["duration"]),
            )),
            row["output"]["tint"],
            "{}",
            row["name"]
        );
    }
    let rows = native["light_update_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let input = &row["input"];
        let current = &input["current"];
        let mut source = lighting::radiation_point_light(
            1,
            1,
            128,
            integer(&current["intensity"]),
            tint(&current["tint"]),
        );
        source.active = integer(&current["active"]) != 0;
        let changed = source.update_parameters(
            integer(&input["proposed"]["intensity"]),
            tint(&input["proposed"]["tint"]),
        );
        assert_eq!(source.intensity, integer(&row["output"]["intensity"]));
        assert_eq!(serde_json::json!(source.tint), row["output"]["tint"]);
        assert_eq!(source.active, integer(&row["output"]["active"]) != 0);
        assert_eq!(
            changed,
            row["visited"]
                .as_array()
                .unwrap()
                .iter()
                .any(|pc| pc == "00554AC0"),
            "native parameter-store branch {}",
            row["name"]
        );
    }
}

#[test]
fn field_operations_match_native_overlap_controls() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/radiation_overlap.json",
    ))
    .unwrap();
    let scene = crate::sim::arena_fixture::rival_arena_with_size(&fixture.rules, 31, 48);
    let terrain = scene.resolved_terrain.as_ref();
    for row in native["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let mut rules = fixture.rules.radiation.clone();
        if name.starts_with("zero_steps_") {
            // The corpus explicitly supplies this nonstock reader input to
            // exercise the same shared operation's masked zero divisor.
            rules.level_delay = 600;
        }
        assert_eq!(
            format!("{:016x}", rules.level_factor.to_bits()),
            row["factor_bits"]
        );
        let mut state = RadiationState::default();
        let mut lights = Vec::new();
        let mut frame = 0;
        for stage in row["stages"].as_array().unwrap() {
            let action = &stage["action"];
            match action["kind"].as_str().unwrap() {
                "create" | "merge" => {
                    let (rx, ry) = cell(&action["cell"]);
                    state.apply_detonation(
                        RadDetonation {
                            rx,
                            ry,
                            rad_level: integer(&row["source_inputs"]["rad_level"]),
                            spread: integer(&row["source_inputs"]["spread"]),
                        },
                        frame,
                        &scene.session.theater,
                        &rules,
                        terrain,
                        &mut lights,
                    );
                }
                "tick_to" => {
                    let end = u32::try_from(action["frame"].as_u64().unwrap()).unwrap();
                    while frame < end {
                        frame += 1;
                        state.tick_decay(frame, &rules, terrain, &mut lights);
                    }
                }
                "decay" => state.decay_step(cell(&action["cell"]), terrain),
                "decrease" => state.decrease_site_levels(cell(&action["cell"]), terrain, &rules),
                other => panic!("unknown native action {other}"),
            }
            for sample in stage["samples"].as_array().unwrap() {
                let at = cell(&sample["cell"]);
                assert_eq!(
                    format!("{:016x}", state.cell_level(at).to_bits()),
                    sample["level_bits"],
                    "{name} {action} {at:?}: Cell's stored bits"
                );
                assert_eq!(
                    state.damaging_level(at, rules.level_max),
                    integer(&sample["damaging_level"]),
                    "{name} {action} {at:?}: Cell487CB0 conversion"
                );
            }
            for expected in stage["sites"].as_array().unwrap() {
                let actual = state.site_at(cell(&expected["cell"])).unwrap();
                assert_eq!(
                    actual.radius_leptons,
                    integer(&expected["radius"]),
                    "{name}"
                );
                assert_eq!(actual.level, integer(&expected["level"]), "{name}");
                assert_eq!(
                    actual.level_steps,
                    integer(&expected["level_steps"]),
                    "{name}"
                );
                assert_eq!(actual.duration, integer(&expected["duration"]), "{name}");
                assert_eq!(actual.remaining, integer(&expected["remaining"]), "{name}");
                assert_eq!(
                    RadiationState::current_site_level(actual),
                    integer(&expected["current_level"]),
                    "{name}: Guard65B510 retains its distinct integer query"
                );
                assert_eq!(
                    actual.level_timer.start_frame(),
                    integer(&expected["level_timer_start"]),
                    "{name}"
                );
                assert_eq!(
                    actual.level_timer.duration(),
                    integer(&expected["level_timer_duration"]),
                    "{name}"
                );
            }
            let expected_order: Vec<_> = stage["update_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(cell)
                .collect();
            assert_eq!(
                state
                    .sites_in_update_order()
                    .map(|site| site.center)
                    .collect::<Vec<_>>(),
                expected_order,
                "{name}: native reverse creation order survives each operation"
            );
        }
    }
}

#[test]
fn restore_rejects_missing_or_duplicate_radiation_update_slots() {
    use crate::sim::snapshot::SnapshotRestoreError;
    let mut sim = crate::sim::world::Simulation::new();
    sim.radiation.apply_detonation(
        RadDetonation {
            rx: 10,
            ry: 10,
            rad_level: 500,
            spread: 2,
        },
        0,
        &sim.session.theater,
        &RadiationRules::default(),
        None,
        &mut Vec::new(),
    );
    sim.radiation.creation_order.push((10, 10));
    assert_eq!(
        sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InvalidRadiationSiteOrder(
            "duplicate radiation site in update order"
        ))
    );
    sim.radiation.creation_order.clear();
    assert_eq!(
        sim.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InvalidRadiationSiteOrder(
            "radiation site is missing from update order"
        ))
    );
}

#[test]
fn equal_current_fields_with_different_update_order_hash_differently() {
    let mut forward = crate::sim::world::Simulation::new();
    let mut reverse = crate::sim::world::Simulation::new();
    let centers = [(17, 24), (31, 24)];
    for (world, order) in [
        (&mut forward, centers),
        (&mut reverse, [centers[1], centers[0]]),
    ] {
        for (rx, ry) in order {
            world.radiation.apply_detonation(
                RadDetonation {
                    rx,
                    ry,
                    rad_level: 500,
                    spread: 10,
                },
                0,
                &world.session.theater,
                &RadiationRules::default(),
                None,
                &mut Vec::new(),
            );
        }
    }
    let bits = |state: &RadiationState| {
        state
            .iter_cells()
            .map(|(cell, level)| (*cell, level.to_bits()))
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&forward.radiation), bits(&reverse.radiation));
    assert_ne!(
        forward.state_hash(),
        reverse.state_hash(),
        "equal current levels must not hide a different future subtraction order"
    );
}

#[test]
fn serialized_radius_that_changes_future_decay_is_hash_relevant() {
    let mut original = crate::sim::world::Simulation::new();
    original.radiation.apply_detonation(
        RadDetonation {
            rx: 17,
            ry: 24,
            rad_level: 500,
            spread: 10,
        },
        0,
        &original.session.theater,
        &RadiationRules::default(),
        None,
        &mut Vec::new(),
    );
    let mut restored = crate::sim::world::Simulation::new();
    restored.radiation =
        save_codec::deserialize(&save_codec::serialize(&original.radiation).unwrap()).unwrap();
    restored
        .radiation
        .sites
        .get_mut(&(17, 24))
        .unwrap()
        .radius_leptons += 1;
    assert_eq!(original.restore_after_snapshot_load(), Ok(()));
    assert_eq!(restored.restore_after_snapshot_load(), Ok(()));
    assert_eq!(
        original.radiation.cell_level((24, 24)).to_bits(),
        restored.radiation.cell_level((24, 24)).to_bits()
    );
    assert_ne!(
        original.state_hash(),
        restored.state_hash(),
        "admitted saved radii control future decay even with equal current cells"
    );
}
