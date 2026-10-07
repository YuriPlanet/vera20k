use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::locomotor::LocomotorState;

fn infantry() -> GameEntity {
    let mut e = GameEntity::test_default(1, "E1", "Allies", 5, 5);
    e.category = EntityCategory::Infantry;
    e.mission_leaf =
        crate::sim::mission::leaf::MissionLeafState::for_entity_category(EntityCategory::Infantry);
    e
}

fn set_mission(entity: &mut GameEntity, mission: MissionType) {
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_known(mission),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
}

fn sim_with(entity: GameEntity) -> Simulation {
    let mut sim = Simulation::new();
    sim.substrate.entities.insert(entity);
    sim.interner = crate::sim::intern::test_interner();
    sim
}

/// Original51AA40 asks active718080, then the physical Cell4834A0 before
/// its current-Attack/same-reference exception. An armed CLEG transaction
/// cannot approve that prefix without map inputs. No admission is executed
/// by this preview and the complete owned state/RNG must remain unchanged.
#[test]
fn armed_teleport_destination_requires_the_current_cell_inputs() {
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=CLEG\n[CLEG]\nSpeed=5\nMovementZone=Infantry\n\
         Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    let id = sim
        .construct_object_limbo_at_height("CLEG", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.lifecycle.in_limbo = false;
    set_mission(actor, MissionType::Guard);
    assert!(
        actor.infantry.is_some(),
        "fixture must reach the Infantry setter"
    );
    actor.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::Relocate,
        12,
        10,
        16,
    )));
    assert_eq!(super::super::motion_query::is_moving(actor), Some(true));
    let before = serde_json::to_value(&*actor).unwrap();
    let rng = sim.rng_state();
    assert!(sim.resolved_terrain.is_none());
    assert!(
        !sim.infantry_destination_inputs_available(id, NavTargetRef::cell(13, 10), &rules, None,),
        "armed Teleport must require its physical current Cell"
    );
    assert_eq!(
        serde_json::to_value(sim.substrate.entities.get(id).unwrap()).unwrap(),
        before
    );
    assert_eq!(sim.rng_state(), rng);
}

/// The hut caller's `Scatter(NULL, 1, 1)` rows (tools/spatial_oracle/hut_scatter)
/// through the production receiver: its gates on the man each row describes,
/// then its one `RandomRanged(0, 4)` from the corpus seed. This fixture has
/// no map, so the receiver ends at the search; the native Scenario indices
/// pin the gate answer and the draw together.
#[test]
fn forced_gates_and_draw_match_original_hut_caller() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/hut_scatter.json",
    ))
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in &rows[..8] {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap();
        // The corpus runs a computer man on Guard with `PlayerScatter=` set.
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\n[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nFraidycat={}\n[Guard]\n\
             Scatter={}\n[CombatDamage]\nPlayerScatter=yes\n",
            flag("fraidycat"),
            flag("mission_scatter"),
        )))
        .unwrap();
        let mut man = infantry();
        set_mission(&mut man, MissionType::Guard);
        man.mission_leaf
            .set_infantry_doing_verified(input["doing"].as_i64().unwrap() as i32)
            .unwrap();
        if flag("attack") {
            man.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
        }
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
        loco.set_walk_destination(flag("moving").then(|| DriveCoord::cell(6, 5, 0)));
        man.locomotor = Some(loco);
        let mut sim = sim_with(man);
        sim.scenario_rng = crate::sim::rng::SimRng::new(31);
        assert!(
            !sim.infantry_scatter_null(1, ScatterFlags::new(true, true), &rules, None)
                .unwrap()
        );
        let state = sim.scenario_rng.logical_state();
        assert_eq!(
            serde_json::json!([state.index_a, state.index_b]),
            row["output"]["random_indices"],
            "{row}"
        );
    }
}

/// Native rows from `0x0051D162` with the second argument 1, both first
/// arguments, through the production fact reads: Walk
/// (tools/infantry_scatter_oracle.py) and Jumpjet
/// (tools/spatial_oracle/jumpjet_scatter_gates.py) Is_Moving. Neither gate
/// draws RNG or writes the man; a path-execution adapter changes nothing.
#[test]
fn forced_no_kidding_gates_match_native_walk_and_jumpjet_rows() {
    let walk: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/infantry_scatter_oracle.json",
    ))
    .unwrap();
    let jumpjet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/jumpjet_scatter_gates.json",
    ))
    .unwrap();
    let mut checked = 0;
    for (rows, kind) in [
        (
            walk["scatter_gates"].as_array().unwrap(),
            LocomotorKind::Walk,
        ),
        (jumpjet.as_array().unwrap(), LocomotorKind::Jumpjet),
    ] {
        for row in rows {
            let flag = |key: &str| row[key].as_bool().unwrap();
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nFraidycat={}\n[Move]\nRate=.016\n\
                 [Sleep]\nScatter=no\n[CombatDamage]\nPlayerScatter={}\n",
                flag("fraidycat"),
                flag("global_scatter"),
            )))
            .unwrap();
            let mut man = infantry();
            set_mission(
                &mut man,
                if flag("mission_scatter") {
                    MissionType::Move
                } else {
                    MissionType::Sleep
                },
            );
            if flag("has_target") {
                man.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
            }
            let mut loco = LocomotorState::for_test_kind(kind);
            if let Some(state) = loco.jumpjet_runtime_mut() {
                *state = state
                    .clone()
                    .with_moving_for_test(flag("moving"))
                    .with_phase_for_test(2);
            } else {
                loco.set_walk_destination(flag("moving").then(|| DriveCoord::cell(6, 5, 0)));
            }
            man.locomotor = Some(loco);
            for adapter in [false, true] {
                let mut man = man.clone();
                man.movement_target = adapter.then(MovementTarget::default);
                let before = serde_json::to_value(&man).unwrap();
                let mut sim = sim_with(man);
                let rng = sim.scenario_rng.state();
                let flags = ScatterFlags::new(flag("first_bool"), flag("second_bool"));
                let admitted = sim.infantry_scatter_admitted(1, flags, &rules).unwrap();
                assert_eq!(admitted, flag("gate_admitted"), "{row} adapter={adapter}");
                assert_eq!(sim.scenario_rng.state(), rng, "{row}");
                assert_eq!(
                    serde_json::to_value(sim.substrate.entities.get(1).unwrap()).unwrap(),
                    before,
                    "{row}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 256);
}

/// The damage receiver's `Scatter(attacker, 0, 0)` gates against their
/// original execution (tools/spatial_oracle/infantry_damage_scatter.py):
/// the human deploy-Doing refusal, mission, Doing table, Fraidycat and the
/// owner/Team gate, through the production fact reads.
#[test]
fn damage_gates_match_original_execution() {
    use crate::sim::animation::{Animation, SequenceKind};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_damage_scatter.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus.as_array().unwrap() {
        let input = &row["input"];
        let flag = |name: &str, default: bool| input[name].as_bool().unwrap_or(default);
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nFixture=1\n[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nFraidycat={}\n\
             VeteranAbilities={}\nEliteAbilities={}\n[Guard]\nScatter={}\n\
             [CombatDamage]\nPlayerScatter={}\n",
            flag("fraidycat", true),
            if flag("veteran_scatter", false) {
                "SCATTER"
            } else {
                ""
            },
            if flag("elite_scatter", false) {
                "SCATTER"
            } else {
                ""
            },
            flag("mission_scatter", true),
            flag("player_scatter", false),
        )))
        .unwrap();
        let mut victim = infantry();
        victim
            .mission_leaf
            .set_infantry_doing_verified(input["doing"].as_i64().unwrap_or(-1) as i32)
            .unwrap();
        // Presentation cannot admit or refuse simulation work.
        victim.animation = Some(Animation::new(SequenceKind::Die1));
        victim.set_veterancy_rank(input["rank"].as_u64().unwrap_or(0) as u16 * 100);
        victim.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        set_mission(&mut victim, MissionType::Guard);
        if flag("target", false) {
            victim.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
        }
        if flag("nav", false) {
            victim.navigation.nav_com = Some(NavTargetRef::cell(9, 9));
        }
        if flag("moving", false) {
            victim.movement_target = Some(MovementTarget::default());
        }
        let owner = victim.owner();
        let type_ref = victim.type_ref();
        let mut sim = sim_with(victim);
        let mut house = HouseState::new(owner, 0, None, flag("human", false), 0, 10);
        house.player_control = flag("player_control", false);
        sim.houses.insert(owner, house);
        sim.session.game_mode_nonzero = flag("game_mode_nonzero", true);
        if flag("team", false) {
            sim.team_script_vm.create_team(owner, type_ref, vec![1], 0);
        }
        let admitted = sim
            .infantry_scatter_admitted(1, ScatterFlags::new(false, false), &rules)
            .unwrap();
        assert_eq!(admitted, row["admitted"].as_bool().unwrap(), "{input}");
        checked += 1;
    }
    assert_eq!(checked, 278);
}

/// Without both flags, a deploy-family Doing refuses a human owner's man at
/// once (`0x0051D115..0x0051D148`); a computer's man reaches the Doing table,
/// which admits Doing 28. Forced and no-kidding would take
/// `Do_Action(Undeploy)` (`0x0051D103`) instead.
#[test]
fn deploy_doing_head_refuses_a_human_owner_without_both_flags() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nFraidycat=yes\n",
    ))
    .unwrap();
    let doing = *DEPLOY_DOINGS.start() + 1;
    assert_eq!(
        crate::rules::infantry_sequence::scatter_allowed_by_doing(doing),
        Some(true)
    );
    for human in [false, true] {
        for flags in [
            ScatterFlags::new(false, false),
            ScatterFlags::new(true, false),
        ] {
            let mut man = infantry();
            man.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
            man.mission_leaf.set_infantry_doing_verified(doing).unwrap();
            let owner = man.owner();
            let mut sim = sim_with(man);
            sim.houses
                .insert(owner, HouseState::new(owner, 0, None, human, 0, 10));
            sim.session.game_mode_nonzero = true;
            let admitted = sim.infantry_scatter_admitted(1, flags, &rules).unwrap();
            assert_eq!(admitted, !human, "human={human} flags={flags:?}");
        }
    }
}
