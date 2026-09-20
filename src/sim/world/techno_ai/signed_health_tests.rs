use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::game_entity::GameEntity;

#[test]
fn original_selfheal_tail_and_192_height_rows_retain_art_and_mark_smoke() {
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/building_art_transition.json"
    ))
    .unwrap();
    let height: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/selfheal_smoke_height.json"
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["source"] == "selfheal")
        .chain(height.as_array().unwrap())
    {
        let input = &row["input"];
        let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=SIGNED\n[SIGNED]\nStrength={}\nSelfHealing=yes\n[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\nMaxDC=4\nStartStateAI=0\nEndStateAI=10\nStateAIAdvance=4\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\nParticleCap=10\nSpawnFrames=1\nLifetime=200\n",
            input["strength"]
        ))).unwrap();
        rules.general.repair_rate_minutes = 1.0;
        rules.general.condition_yellow = 0.5;
        let mut sim = Simulation::new();
        assert_eq!(sim.allocate_stable_id(), 1);
        let mut entity = GameEntity::test_default(1, "SIGNED", "A", 2, 2);
        entity.type_ref = sim.interner.intern("SIGNED");
        entity.owner = sim.interner.intern("A");
        entity.category = EntityCategory::Structure;
        entity.health.current = input["current"].as_i64().unwrap() as i32;
        entity.building_damage_state_active = input["old_flag"].as_u64().unwrap_or(1) == 1;
        if let Some(xy) = input["xy"].as_array() {
            use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
            let x = xy[0].as_i64().unwrap() as i32;
            let y = xy[1].as_i64().unwrap() as i32;
            entity.position.rx = (x >> 8) as u16;
            entity.position.ry = (y >> 8) as u16;
            entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(x & 255);
            entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(y & 255);
            entity.position.exact_z_leptons = Some(input["z"].as_i64().unwrap() as i32);
            entity.on_bridge = input["on_bridge"].as_bool().unwrap();
            let mut cell = test_flat_cell(entity.position.rx, entity.position.ry);
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.slope_type = input["slope"].as_u64().unwrap() as u8;
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(8, 8, vec![cell]));
        }
        sim.substrate.entities.insert(entity);
        sim.session.binary_frame = 900;
        let smoke_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                glam::IVec3::ZERO,
                Some(1),
                Some(1),
                glam::IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .damage_smoke_system_id = Some(smoke_id);
        let before_rng = sim.scenario_rng.logical_state();
        self_heal_step(&mut sim, 1, &rules);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            entity.health.current,
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.building_damage_state_active,
            row["output"]["retained_flag"] == 1,
            "{row}"
        );
        assert_eq!(entity.damage_smoke_system_id, Some(smoke_id));
        assert_eq!(
            sim.particle_systems().get(smoke_id).unwrap().done_spawning,
            row["output"]["smoke_done"] == 1,
            "{row}"
        );
        assert_eq!(sim.particle_systems().len(), 1);
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        checked += 1;
    }
    assert_eq!(checked, 228);
}

fn fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let category = if input["kind"] == "building" {
        "BuildingTypes"
    } else {
        "VehicleTypes"
    };
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[{category}]\n0=SIGNED\n[SIGNED]\nStrength={}\nSelfHealing={}\n",
        input["strength"],
        if input["self_healing"] == true {
            "yes"
        } else {
            "no"
        }
    )))
    .unwrap();
    rules.general.repair_rate_minutes = f64::from_bits(
        u64::from_str_radix(input["repair_rate_bits"].as_str().unwrap(), 16).unwrap(),
    );
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "SIGNED", "Americans", 2, 2);
    entity.type_ref = sim.interner.intern("SIGNED");
    entity.owner = sim.interner.intern("Americans");
    entity.health.current = input["current"].as_i64().unwrap() as i32;
    entity.estimated_health = crate::sim::estimated_health::EstimatedHealth::from_raw(-987654321);
    entity.category = if input["kind"] == "building" {
        EntityCategory::Structure
    } else {
        EntityCategory::Unit
    };
    entity.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(entity);
    sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
    sim.set_logic_order_for_test(vec![1]);
    (sim, rules)
}

#[test]
fn production_self_heal_matches_original_signed_cadence_and_increment() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/object_health.json"
    ))
    .unwrap();
    let rows = corpus["self_heal"].as_array().unwrap();
    assert_eq!(rows.len(), 131);
    for row in rows {
        let (mut sim, rules) = fixture(row);
        if let Some(period) = row["output"]["period"].as_i64() {
            assert_eq!(
                crate::sim::combat::veterancy::self_heal_interval_frames(
                    rules.general.repair_rate_minutes
                ),
                period as i32,
                "{row}"
            );
        }
        self_heal_step(&mut sim, 1, &rules);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            entity.health.current,
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            entity.estimated_health.get(),
            -987654321,
            "self-heal must not reset reservations"
        );
    }
    let faults = corpus["self_heal_traps"].as_array().unwrap();
    assert_eq!(faults.len(), 5);
    for row in faults {
        let (mut sim, rules) = fixture(row);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self_heal_step(&mut sim, 1, &rules)
        }));
        assert!(
            result.is_err(),
            "native IDIV fault cannot become no-heal: {row}"
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().health.current,
            row["input"]["current"].as_i64().unwrap() as i32
        );
    }
}

#[test]
fn whole_object_ai_keeps_wrapping_self_heal_and_signed_frame_period() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/object_health.json"
    ))
    .unwrap();
    for row in corpus["self_heal"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["input"]["kind"] == "unit"
                && row["output"]["entered_increment"] == true
                && (row["input"]["current"] == i32::MAX || row["input"]["frame"] == 4294966396_u64)
        })
    {
        let (mut sim, rules) = fixture(row);
        sim.object_ai_stage(Some(&rules));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().health.current,
            row["output"]["actual"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn building_damage_fire_runs_before_yellow_crossing_selfheal() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=B\n[B]\nStrength=100\nSelfHealing=yes\n[General]\nRepairRate=1\n[AudioVisual]\nConditionYellow=50%\n"
    )).unwrap();
    rules.general.repair_rate_minutes = 1.0;
    let mut sim = Simulation::new();
    let id = sim.allocate_stable_id();
    let mut building = GameEntity::test_default(id, "B", "A", 2, 2);
    building.type_ref = sim.interner.intern("B");
    building.owner = sim.interner.intern("A");
    building.category = crate::map::entities::EntityCategory::Structure;
    building.health.current = 50;
    sim.substrate.entities.insert(building);
    sim.reveal(id);
    sim.session.binary_frame = 900;
    sim.object_ai_stage(Some(&rules));
    let building = sim.entities().get(id).unwrap();
    assert_eq!(building.health.current, 51);
    assert!(
        building.damage_fire_state_active,
        "43FC39 precedes6FA756 INC"
    );
    sim.session.binary_frame = 901;
    sim.object_ai_stage(Some(&rules));
    assert!(!sim.entities().get(id).unwrap().damage_fire_state_active);
}
