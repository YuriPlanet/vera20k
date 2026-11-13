use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::passenger::PassengerRole;

const RULES: &str = "\
[InfantryTypes]
0=INIT
1=E1
2=E2
[VehicleTypes]
0=TRNS
[BuildingTypes]
0=NABNKR
1=ARMORY
[Guard]
Rate=.030
[INIT]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E2]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[TRNS]
Strength=300
Speed=6
Passengers=3
InitialPayload.Types=E1,E2
InitialPayload.Nums=2
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[NABNKR]
Strength=500
TechLevel=1
CanBeOccupied=yes
MaxNumberOccupants=5
InitialPayload.Types=E1,TRNS
InitialPayload.Nums=4
[ARMORY]
Strength=500
Armory=yes
Passengers=5
";

const ART: &str = "[NABNKR]\nFoundation=2x2\n[ARMORY]\nFoundation=2x2\n";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str(ART),
    )
    .expect("transport rules")
}

/// The flat arena with the computer YuriCountry and the human Americans.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0x43C10);
    for (name, side, human) in [("YuriCountry", 2, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    let _ = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn kinds(sim: &Simulation, ids: &[u64]) -> Vec<String> {
    let mut kinds: Vec<String> = ids
        .iter()
        .map(|&id| {
            let entity = sim.substrate.entities.get(id).unwrap();
            sim.interner.resolve(entity.type_ref()).to_owned()
        })
        .collect();
    kinds.sort();
    kinds
}

/// CreateInitialPayload (10043C10) from FootClass_Put (hook 0x4D718C): a
/// `Passengers=3` transport holds two E1 (`InitialPayload.Nums=2`) and one
/// E2, whose count repeats the last one and is capped at the last free
/// slot; the passengers ride inside it, and a later Put makes no second
/// payload (PayloadCreated). From BuildingClass_Place (hook 0x446EE2) a
/// garrison takes four E1 occupants and skips the TRNS, which is no
/// infantry.
#[test]
fn initial_payload_fills_a_transport_once_and_garrisons_infantry_only() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let mut sim = arena(&rules);
    let transport = spawn(&mut sim, &rules, "TRNS", "Americans", 10, 10);
    let cargo = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(transport)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers()
            .to_vec()
    };
    assert_eq!(kinds(&sim, &cargo(&sim)), ["E1", "E1", "E2"]);
    for passenger in cargo(&sim) {
        let passenger = sim.substrate.entities.get(passenger).unwrap();
        assert!(passenger.lifecycle.in_limbo());
        assert_eq!(
            passenger.passenger_role.inside_transport_id(),
            Some(transport)
        );
    }
    sim.create_ares_initial_payload(transport, &rules, None, FrameEffects::default());
    assert_eq!(cargo(&sim).len(), 3);

    let bunker = spawn(&mut sim, &rules, "NABNKR", "Americans", 16, 10);
    let occupants = sim
        .substrate
        .entities
        .get(bunker)
        .unwrap()
        .passenger_role
        .occupants()
        .ids()
        .to_vec();
    assert_eq!(kinds(&sim, &occupants), ["E1"; 4]);
}

/// BuildingExt::KickOutHospitalArmory (10012920): a capture (ChangeOwner,
/// 100163B0, hook 0x448277) and then a sale order (Sell_Back, 10016970,
/// hook 0x447113) each kick one trainee out of an `Armory=` building, the
/// head of its cargo first.
// Runs again once production's sale step moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn armory_capture_and_sale_each_kick_out_one_trainee() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let mut sim = arena(&rules);
    let armory = spawn(&mut sim, &rules, "ARMORY", "Americans", 16, 10);
    let trainees = [(); 2].map(|()| {
        let trainee = sim
            .construct_object_limbo_at_height(
                "INIT",
                "Americans",
                1,
                1,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(trainee)
            .unwrap()
            .passenger_role = PassengerRole::inside_for_test(armory, false);
        trainee
    });
    sim.substrate
        .entities
        .get_mut(armory)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .board_forced_chain(&[(trainees[0], 1.0), (trainees[1], 1.0)]);
    let in_limbo =
        |sim: &Simulation, id: u64| sim.substrate.entities.get(id).unwrap().lifecycle.in_limbo();
    let yuri = sim.interner.get("YuriCountry").unwrap();
    sim.change_owner_with_rules(armory, yuri, &rules, None, FrameEffects::default());
    assert!(!in_limbo(&sim, trainees[0]) && in_limbo(&sim, trainees[1]));
    let _ = crate::sim::production::sell_back(
        &mut sim,
        &rules,
        armory,
        crate::sim::production::SellOrder::Player,
        None,
        FrameEffects::default(),
    );
    assert!(!in_limbo(&sim, trainees[1]));
    assert!(
        sim.substrate
            .entities
            .get(armory)
            .unwrap()
            .passenger_role
            .cargo()
            .is_none_or(|cargo| cargo.is_empty())
    );
}
