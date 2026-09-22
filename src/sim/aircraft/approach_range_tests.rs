use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::foundation::FOUNDATION_TABLE;
use crate::rules::ini_parser::IniFile;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn native_rows() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_approach_range.json"
    ))
    .unwrap()
}

fn fixture(row: &Value) -> (Simulation, RuleSet) {
    let case = &row["input"];
    let foundation = &FOUNDATION_TABLE[case["foundation"].as_u64().unwrap_or(0) as usize];
    let primary_range = case["range"].as_i64().unwrap_or(1536) as f64 / 256.0;
    let elite_range = case["elite_range"].as_i64().unwrap_or(2304) as f64 / 256.0;
    let elite = if case["elite_weapon"].as_bool().unwrap_or(true) {
        "ElitePrimary=ELITE\n"
    } else {
        ""
    };
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AircraftTypes]\n0=ORCA\n[VehicleTypes]\n0=TARGET\n[BuildingTypes]\n0=BUILDING\n\
         [ORCA]\nStrength=150\nSpeed=14\nAmmo=2\nPrimary=PRIMARY\n{elite}\
         Locomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n\
         [TARGET]\nStrength=300\n[BUILDING]\nStrength=500\nFoundation={}\nBib={}\n\
         [PRIMARY]\nRange={primary_range}\nDamage=10\nROF=20\n\
         [ELITE]\nRange={elite_range}\nDamage=10\nROF=20\n",
        foundation.name,
        case["bib"].as_bool().unwrap_or(false)
    )))
    .unwrap();
    let set_xyz = |entity: &mut GameEntity, xyz: &Value| {
        let x = xyz[0].as_i64().unwrap() as i32;
        let y = xyz[1].as_i64().unwrap() as i32;
        entity.position.rx = (x / 256) as u16;
        entity.position.ry = (y / 256) as u16;
        entity.position.sub_x = SimFixed::from_num(x % 256);
        entity.position.sub_y = SimFixed::from_num(y % 256);
        entity.position.exact_z_leptons = Some(xyz[2].as_i64().unwrap() as i32);
    };
    let mut source = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
    source.category = EntityCategory::Aircraft;
    source.veterancy = (case["veterancy"].as_u64().unwrap_or(0) * 100) as u16;
    source.aircraft_ammo = Some(AircraftAmmo::new(2));
    source.aircraft_mission = Some(AircraftMission::Attack { sub_state: 3 });
    source.locomotor = Some(LocomotorState::from_object_type(
        rules.object("ORCA").unwrap(),
        0,
    ));
    set_xyz(&mut source, &case["source"]);
    let mut sim = Simulation::with_seed(0);
    let target = match case["kind"].as_str().unwrap_or("unit") {
        "cell" => TargetKind::Cell(
            case["cell"][0].as_u64().unwrap() as u16,
            case["cell"][1].as_u64().unwrap() as u16,
        ),
        kind => {
            let building = kind == "building";
            let mut target = GameEntity::test_default(
                2,
                if building { "BUILDING" } else { "TARGET" },
                "Soviet",
                10,
                10,
            );
            if building {
                target.category = EntityCategory::Structure;
                target.foundation = foundation.name.to_owned();
                assert_eq!(
                    serde_json::json!([foundation.width, foundation.height]),
                    row["foundation_dimensions"]
                );
            }
            set_xyz(&mut target, &case["target"]);
            sim.substrate.entities.insert(target);
            TargetKind::Entity(2)
        }
    };
    source.attack_target = Some(match target {
        TargetKind::Entity(id) => AttackTarget::new(id),
        TargetKind::Cell(x, y) => AttackTarget::for_cell(x, y),
    });
    sim.substrate.entities.insert(source);
    sim.interner = test_interner();
    sim.substrate.next_stable_object_id = 3;
    sim.set_logic_order_for_test(vec![1]);
    (sim, rules)
}

fn assert_native_mission_result(sim: &Simulation, row: &Value) {
    let aircraft = sim.substrate.entities.get(1).unwrap();
    let in_range = row["in_range"].as_bool().unwrap();
    let Some(AircraftMission::Attack { sub_state, .. }) = aircraft.aircraft_mission else {
        panic!("unexpected mission for {row}");
    };
    assert_eq!(sub_state, if in_range { 4 } else { 3 }, "{row}");
    assert!(
        !aircraft.aircraft_ammo.as_ref().unwrap().release_pending(),
        "the range branch only authorizes the next mission state"
    );
    assert_eq!(aircraft.aircraft_ammo.as_ref().unwrap().current, 2);
    assert_eq!(aircraft.movement_target.is_some(), !in_range, "{row}");
    assert!(sim.projectiles.is_empty());
}

#[test]
fn native_range_branches_reach_production_mission_and_move_dispatch() {
    let rows = native_rows();
    assert_eq!(rows.len(), 235);
    for row in rows {
        let (mut sim, rules) = fixture(&row);
        let aircraft = sim.substrate.entities.get(1).unwrap();
        let target = aircraft.attack_target.as_ref().unwrap().target;
        assert_eq!(
            crate::sim::combat::object_distance_to(
                aircraft,
                &target,
                &sim.substrate.entities,
                &rules,
                &sim.interner
            ),
            Some(row["distance"].as_i64().unwrap() as i32),
            "{row}"
        );
        let weapon = crate::sim::combat::combat_weapon::primary_for_tier(
            rules.object("ORCA").unwrap(),
            aircraft.veterancy,
        )
        .unwrap();
        assert_eq!(
            weapon.to_lowercase(),
            row["weapon"].as_str().unwrap(),
            "{row}"
        );
        crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
        assert_native_mission_result(&sim, &row);
    }
}

#[test]
fn pending_approach_range_decision_survives_save_restore() {
    for row in native_rows().into_iter().step_by(23) {
        let (mut sim, rules) = fixture(&row);
        // Native range inputs are already object/rules authority; no new saved
        // distance or target cache should be needed to resume this decision.
        let saved = GameSnapshot::save(&sim, 0, 0, "aircraft approach range", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.state_hash(), restored.state_hash());
        crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
        crate::sim::aircraft::tick_aircraft_missions(&mut restored, &rules, None);
        assert_native_mission_result(&restored, &row);
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
}
