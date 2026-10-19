//! UnitClass's BalloonHover arms, Approach_Target's (0x00741599) and
//! Assign_Destination's (0x00741983), replayed against the original's rows in
//! tools/spatial_oracle/balloon_hover.json.
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

const ACTOR: u64 = 1;
const TARGET: u64 = 2;
const OTHER: u64 = 3;

fn unit(id: u64, name: &str, cell: (u16, u16), sub: (i32, i32)) -> GameEntity {
    let mut unit = GameEntity::test_default(id, name, "Americans", cell.0, cell.1);
    unit.set_category_for_test(EntityCategory::Unit);
    unit.position.set_sub_x_for_test(SimFixed::from_num(sub.0));
    unit.position.set_sub_y_for_test(SimFixed::from_num(sub.1));
    unit.lifecycle.set_in_limbo_for_test(false);
    unit
}

/// The row's Target: the Target Unit or the Cell `target_cell`.
fn target(input: &Value) -> TargetKind {
    match input["target_cell"].as_array() {
        Some(cell) => TargetKind::Cell(
            cell[0].as_u64().unwrap() as u16,
            cell[1].as_u64().unwrap() as u16,
        ),
        None => TargetKind::Entity(TARGET),
    }
}

/// The oracle's Unit at (2688, 2688), its Target Unit at `target_at` or
/// (3388, 2388) and the other object at (1788, 3088); the NavCom Cell is
/// (11, 10). The oracle answers SelectWeapon with an `approach` row's slot,
/// so the Unit carries that slot's weapon in both of its slots, and VERA's
/// selection answers it whichever slot it picks (its weapons carry a
/// warhead, which VERA's selection needs and GetWeapon does not read).
fn world(input: &Value, facing: u16) -> (Simulation, RuleSet) {
    let balloon = if input["balloon"] == false {
        "no"
    } else {
        "yes"
    };
    let weapon = input["slot"]
        .as_u64()
        .and_then(
            |slot| match input["weapons"][slot as usize].as_str().unwrap() {
                "vertical" => Some("Drop"),
                "level" => Some("Level"),
                "no_projectile" => Some("Bare"),
                _ => None,
            },
        )
        .map_or(String::new(), |weapon| {
            format!("Primary={weapon}\nSecondary={weapon}\n")
        });
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=KIROV\n1=TANK\n[KIROV]\nBalloonHover={balloon}\n{weapon}[TANK]\n\
         [Drop]\nProjectile=Bomb\nWarhead=HE\n[Level]\nProjectile=Shot\nWarhead=HE\n\
         [Bare]\nWarhead=HE\n[Bomb]\nVertical=yes\n[Shot]\nROT=0\n[HE]\nCellSpread=0\n"
    )))
    .unwrap();
    let mut actor = unit(ACTOR, "KIROV", (10, 10), (128, 128));
    let target = target(input);
    if input["target"] != false {
        actor.attack_target.set_for_test(Some(target));
    }
    actor
        .navigation
        .set_nav_com_for_test(match input["nav"].as_str().unwrap_or("none") {
            "none" => None,
            "target" => Some(NavTargetRef::from(target)),
            "other" => Some(NavTargetRef::object(OTHER)),
            _ => Some(NavTargetRef::cell(11, 10)),
        });
    actor.body_facing = FacingClass::new(facing, 0);
    actor.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(input["mission"].as_i64().unwrap_or(1) as i32),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(input["queued"].as_i64().unwrap_or(-1) as i32),
        movement_bypass_latch: 1,
        handler_state: 3,
        mission_start_frame: 7,
        ai_counter: 11,
        dispatch_timer: MissionDispatchTimer::from_raw(13, 19),
    });
    let mut sim = Simulation::new();
    sim.substrate.entities.insert(actor);
    let at = input["target_at"].as_array().map_or([3388, 2388], |at| {
        [0, 1].map(|i| at[i].as_i64().unwrap() as i32)
    });
    sim.substrate.entities.insert(unit(
        TARGET,
        "TANK",
        ((at[0] / 256) as u16, (at[1] / 256) as u16),
        (at[0] % 256, at[1] % 256),
    ));
    sim.substrate
        .entities
        .insert(unit(OTHER, "TANK", (6, 12), (252, 16)));
    sim.interner = crate::sim::intern::test_interner();
    sim.session.set_binary_frame_for_test(100);
    let first = match input["nav"].as_str().unwrap_or("none") {
        "cell_target" => Some(TARGET),
        "cell_other" => Some(OTHER),
        _ => None,
    };
    if let Some(first) = first {
        sim.substrate.occupancy.add(
            11,
            10,
            first,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }
    (sim, rules)
}

/// Approach_Target 0x007414E0 against the `approach` rows. For a type
/// without `Crusher=` the owner and the NavCom change nothing; a type without
/// `BalloonHover=` asks nothing here; a balloon takes its Target, a Unit or a
/// Cell, only when the selected weapon's projectile is `Vertical=`.
#[test]
fn balloon_approach_arm_matches_the_original() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/balloon_hover.json",
    ))
    .unwrap();
    let rows = native["approach"].as_array().unwrap();
    assert_eq!(rows.len(), 133);
    let mut returned = 0;
    for row in rows {
        let input = &row["input"];
        // Every Foot mission calls Approach with a Target (Mission_Attack's
        // 0x004D4E66).
        if input["target"] == false {
            continue;
        }
        let (mut sim, rules) = world(input, 0);
        let answer = sim.balloon_hover_approach(ACTOR, &rules);
        assert_eq!(answer.is_some(), row["path"] == "returned", "{input}");
        if let Some(destination) = answer {
            assert_eq!(destination, NavTargetRef::from(target(input)), "{input}");
            returned += 1;
        }
        // The missions pass no-move 0: then, and only then, the Target becomes
        // the destination with 1.
        if input["flag"] == 0 {
            let assigned = row["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| *call == serde_json::json!(["assign_destination", true, 1]));
            assert_eq!(
                sim.approach_unit_head(
                    ACTOR,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default()
                ),
                assigned,
                "{input}"
            );
            if assigned {
                let actor = sim.substrate.entities.get(ACTOR).unwrap();
                assert_eq!(actor.navigation.nav_com(), answer, "{input}");
            }
        }
    }
    assert_eq!(returned, 18);
}

#[test]
fn balloon_null_destination_arm_matches_the_original() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/balloon_hover.json",
    ))
    .unwrap();
    let rows = native["null"].as_array().unwrap();
    assert_eq!(rows.len(), 498);
    let mut kept = 0;
    for row in rows {
        let input = &row["input"];
        let (mut sim, rules) = world(input, row["facing"].as_u64().unwrap() as u16);
        let before = sim
            .substrate
            .entities
            .get(ACTOR)
            .unwrap()
            .navigation
            .nav_com();
        // Direction_To (0x005F3DB0) on the two coordinates (vt+0x48), a
        // Cell's its centre.
        let actor = sim.substrate.entities.get(ACTOR).unwrap();
        assert_eq!(
            super::turret::facing_toward_target(actor, &target(input), &sim),
            Some(row["direction"].as_u64().unwrap() as u16),
            "{input}"
        );
        let keeps = sim.balloon_hover_keeps_nav_com(ACTOR, &rules);
        assert_eq!(keeps, row["path"] == "kept", "{input}");
        kept += usize::from(keeps);
        let actor = sim.substrate.entities.get(ACTOR).unwrap();
        assert_eq!(actor.navigation.nav_com(), before, "{input}");
        let mission = &actor.mission;
        let timer = mission.dispatch_timer();
        let fields = [
            ("0xac", mission.current().raw() as i64),
            ("0xb4", mission.queued().raw() as i64),
            ("0xb8", i64::from(mission.movement_bypass_latch())),
            ("0xbc", i64::from(mission.handler_state())),
            ("0xc0", i64::from(mission.mission_start_frame())),
            ("0xc4", i64::from(mission.ai_counter())),
            ("0xc8", i64::from(timer.start_frame())),
            ("0xd0", i64::from(timer.delay())),
        ];
        for (offset, value) in fields {
            assert_eq!(
                row["mission"][offset].as_i64().unwrap(),
                value,
                "{offset} {input}"
            );
        }
    }
    assert_eq!(kept, 281);
}

/// Event IDLE (Stop) calls the NULL destination and Target twice for a
/// `BalloonHover=` type (`0x004C75FE..0x004C7624`). The first call keeps the
/// NavCom that leads to the Target and puts the Kirov on Attack; the second,
/// once the Target is gone, clears it. A type without BalloonHover clears it
/// on the first and keeps its mission.
#[test]
fn stop_clears_an_attacking_balloons_destination() {
    for balloon in [true, false] {
        let input = serde_json::json!({
            "balloon": balloon, "nav": "target", "mission": 5, "queued": -1,
        });
        let (mut sim, rules) = world(&input, 0);
        let actor = sim.substrate.entities.get(ACTOR).unwrap();
        let direction =
            super::turret::facing_toward_target(actor, &TargetKind::Entity(TARGET), &sim).unwrap();
        sim.substrate.entities.get_mut(ACTOR).unwrap().body_facing = FacingClass::new(direction, 0);

        assert!(sim.apply_command(
            "Americans",
            &crate::sim::command::Command::Stop { entity_id: ACTOR },
            Some(&rules)
        ));
        let actor = sim.substrate.entities.get(ACTOR).unwrap();
        assert_eq!(actor.navigation.nav_com(), None, "balloon {balloon}");
        assert!(actor.attack_target.target().is_none());
        let attack = MissionId::from_known(crate::sim::mission::MissionType::Attack);
        let guard = MissionId::from_known(crate::sim::mission::MissionType::Guard);
        assert_eq!(
            actor.mission.current(),
            if balloon { attack } else { guard }
        );
    }
}
