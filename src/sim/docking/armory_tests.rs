use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture;
use crate::sim::combat::veterancy::RANK_ELITE_U16;
use crate::sim::passenger::{BoardingArm, board_passenger};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};

fn armory_rules(extra: &str) -> RuleSet {
    let ini = format!(
        "[General]\nIRepairRate=0\nIRepairStep=1\n\
         [InfantryTypes]\n0=E1\n[BuildingTypes]\n0=ARMR\n\
         [E1]\nStrength=100\nSpeed=4\n\
         [ARMR]\nStrength=800\nArmory=yes\nPassengers=1\n{extra}"
    );
    RuleSet::from_ini(&arena_fixture::clear_ground_ini(&ini)).unwrap()
}

fn armory_arena(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = arena_fixture::rival_arena(rules, 60);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let armory = sim
        .spawn_object("ARMR", "Americans", 20, 10, 0, rules)
        .unwrap();
    (sim, armory)
}

fn board_trainee(sim: &mut Simulation, rules: &RuleSet, armory: u64, trainee: u64) {
    for message in [
        RadioMessage::CanEnter,
        RadioMessage::Hello,
        RadioMessage::DockNow,
    ] {
        assert_eq!(
            radio::transmit(
                sim,
                trainee,
                armory,
                message,
                RadioPayload::default(),
                rules,
                &mut FrameEffects::empty()
            ),
            RadioResponse::Roger,
            "{message:?} must admit the trainee"
        );
    }
    assert!(board_passenger(
        sim,
        rules,
        trainee,
        armory,
        BoardingArm::InfantryBuilding,
        None,
        &mut FrameEffects::empty()
    ));
}

fn run_trainee(sim: &mut Simulation, rules: &RuleSet, armory: u64) {
    let now = sim.session.binary_frame();
    assert!(sim.substrate.mission_commence_exact(armory, now).unwrap());
    for _ in 0..32 {
        let _ = crate::sim::docking::building_dock::mission_repair(
            sim,
            rules,
            armory,
            None,
            &mut FrameEffects::empty(),
        );
        if sim
            .substrate
            .entities
            .get(armory)
            .unwrap()
            .passenger_role()
            .passenger_count()
            == 0
        {
            return;
        }
    }
    panic!("armory never released its trainee");
}

/// `Armory=yes` with `Ammo=6`: entering infantry leaves as Elite and exactly
/// one round of the building's Ammo is spent.
#[test]
fn armory_promotes_trainee_to_elite_and_spends_one_ammo() {
    let rules = armory_rules("Ammo=6\n");
    let (mut sim, armory) = armory_arena(&rules);
    let trainee = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();

    assert_eq!(
        sim.entities().get(armory).unwrap().ammo().unwrap().current(),
        6
    );
    assert_eq!(
        radio::receive::can_load_query(&sim, armory, trainee, &rules),
        RadioResponse::Roger
    );
    board_trainee(&mut sim, &rules, armory, trainee);
    run_trainee(&mut sim, &rules, armory);

    assert_eq!(
        sim.entities().get(trainee).unwrap().veterancy.rank(),
        RANK_ELITE_U16,
        "Armory trainee must leave at Elite (three stars)"
    );
    assert_eq!(
        sim.entities().get(armory).unwrap().ammo().unwrap().current(),
        5,
        "one promotion spends one Ammo"
    );
}

/// Without `Ammo=`, native takes the absolute value of the constructor's -1,
/// so the Armory promotes exactly one soldier and then stops.
#[test]
fn armory_without_ammo_key_promotes_exactly_one_trainee() {
    let rules = armory_rules("");
    let (mut sim, armory) = armory_arena(&rules);
    let first = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.entities().get(armory).unwrap().ammo().unwrap().current(),
        -1
    );
    assert_eq!(
        radio::receive::can_load_query(&sim, armory, first, &rules),
        RadioResponse::Roger
    );
    board_trainee(&mut sim, &rules, armory, first);
    run_trainee(&mut sim, &rules, armory);
    assert_eq!(
        sim.entities().get(first).unwrap().veterancy.rank(),
        RANK_ELITE_U16
    );
    assert_eq!(
        sim.entities().get(armory).unwrap().ammo().unwrap().current(),
        0
    );

    let second = sim
        .spawn_object("E1", "Americans", 14, 10, 0, &rules)
        .unwrap();
    assert_eq!(
        radio::receive::can_load_query(&sim, armory, second, &rules),
        RadioResponse::Negatory,
        "the implicit single promotion is spent"
    );
}

/// `Ammo=0` is an Armory with no promotions left.
#[test]
fn armory_with_zero_ammo_refuses_trainees() {
    let rules = armory_rules("Ammo=0\n");
    let (mut sim, armory) = armory_arena(&rules);
    let trainee = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .unwrap();
    assert_eq!(
        radio::receive::can_load_query(&sim, armory, trainee, &rules),
        RadioResponse::Negatory
    );
}
