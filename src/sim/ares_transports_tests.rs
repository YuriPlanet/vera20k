use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::passenger::PassengerRole;

const RULES: &str = "\
[InfantryTypes]
0=INIT
1=E1
2=E2
3=E3
[VehicleTypes]
0=TRNS
1=OTRK
2=TGT
[BuildingTypes]
0=NABNKR
1=ARMORY
[Warheads]
0=RiderWH
1=NoDamageWH
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
[RiderGun]
Damage=10
ROF=20
Range=8
Projectile=InvisibleAll
Warhead=RiderWH
[RiderWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[HullGun]
Damage=20
ROF=20
Range=8
Projectile=InvisibleAll
Warhead=NoDamageWH
[NoDamageWH]
Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%
[InvisibleAll]
Inviso=yes
AA=yes
AG=yes
[E3]
Strength=125
Speed=4
Primary=RiderGun
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[OTRK]
Strength=300
Speed=6
Passengers=2
OpenTopped=yes
Primary=HullGun
InitialPayload.Types=E3
InitialPayload.Nums=2
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[TGT]
Strength=500
Speed=6
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
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
    arena_with_grid(rules).0
}

/// [`arena`] plus the path grid its frames need.
fn arena_with_grid(rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = Simulation::with_seed(0x43C10);
    for (name, side, human) in [("YuriCountry", 2, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    (sim, grid)
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

/// An `OpenTopped=yes` transport's `InitialPayload` riders are its guns.
/// CargoClass::AddPassenger (vt+0x394) limboes the rider, and Limbo's Conceal
/// takes it out of the logic order; native's Unit arm then sets Transporter
/// and InOpenToppedTransport and EnteredOpenTopped (0x00710470) puts the
/// rider back (0x0073A740..0x0073A768), so the rider keeps its AI and fires
/// out. Marking the rider before the hold instead leaves it concealed with no
/// AI: the hull's own warhead is left doing nothing to show the difference.
#[test]
fn initial_payload_riders_of_an_open_topped_transport_fire_for_it() {
    let rules = rules();
    let (mut sim, grid) = arena_with_grid(&rules);
    let hull = spawn(&mut sim, &rules, "OTRK", "Americans", 10, 10);
    let riders = sim
        .substrate
        .entities
        .get(hull)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()
        .to_vec();
    assert_eq!(kinds(&sim, &riders), ["E3", "E3"]);
    let logic = sim.logic_order().to_vec();
    for rider in &riders {
        assert!(
            logic.contains(rider),
            "an open-topped rider keeps its AI in the logic order"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(*rider)
                .unwrap()
                .passenger_role
                .open_transport_id(),
            Some(hull)
        );
    }
    let target = spawn(&mut sim, &rules, "TGT", "YuriCountry", 14, 10);
    let health = |sim: &Simulation| sim.substrate.entities.get(target).unwrap().health.current();
    let before = health(&sim);
    let sender = sim.interner.get("Americans").unwrap();
    let tick = sim.session.tick() + 1;
    sim.queue_command(CommandEnvelope::new(
        sender,
        tick,
        Command::Attack {
            attacker_id: hull,
            target_id: target,
        },
    ));
    for frame in 0..300 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if frame % 50 == 0 {
            for rider in &riders {
                let entity = sim.substrate.entities.get(*rider).unwrap();
                println!(
                    "f{frame} rider {rider} mission={:?} target={:?} in_open={} limbo={}",
                    entity.mission.current().known(),
                    entity.attack_target.target(),
                    entity.passenger_role.in_open_transport(),
                    entity.lifecycle.in_limbo()
                );
            }
            let hull_entity = sim.substrate.entities.get(hull).unwrap();
            println!(
                "f{frame} hull mission={:?} target={:?} health={}",
                hull_entity.mission.current().known(),
                hull_entity.attack_target.target(),
                health(&sim)
            );
        }
    }
    assert!(health(&sim) < before, "the riders did the damage");
}

/// [`initial_payload_riders_of_an_open_topped_transport_fire_for_it`] without
/// the riders: the hull's `NoDamageWH` warhead leaves the target untouched,
/// so the damage above is the riders' own.
#[test]
fn an_open_topped_hull_alone_deals_no_damage() {
    let rules = rules();
    let (mut sim, grid) = arena_with_grid(&rules);
    let hull = spawn(&mut sim, &rules, "OTRK", "Americans", 10, 10);
    sim.substrate
        .entities
        .get_mut(hull)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .set_passengers_for_test(Vec::new());
    let target = spawn(&mut sim, &rules, "TGT", "YuriCountry", 14, 10);
    let health = |sim: &Simulation| sim.substrate.entities.get(target).unwrap().health.current();
    let before = health(&sim);
    let sender = sim.interner.get("Americans").unwrap();
    let tick = sim.session.tick() + 1;
    sim.queue_command(CommandEnvelope::new(
        sender,
        tick,
        Command::Attack {
            attacker_id: hull,
            target_id: target,
        },
    ));
    for _ in 0..300 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    }
    assert_eq!(health(&sim), before, "the hull's own warhead does nothing");
}

/// A transport delivered by a factory is constructed in limbo
/// (`construct_object_limbo_at_height`) and only then Unlimbos, so its
/// `InitialPayload` comes from the constructor rather than from FootClass_Put's
/// `CreateInitialPayload`. Those riders must still be admitted open-topped:
/// an `OpenTopped=` hull carries its guns in the hold, and a rider admitted
/// with `open_topped: false` stays concealed with no AI slot, so it never
/// fires and the hull is left unarmed.
#[test]
fn a_limbo_constructed_open_topped_hull_admits_its_riders_open_topped() {
    let rules = rules();
    let mut sim = arena(&rules);
    let hull = sim
        .construct_object_limbo_at_height(
            "OTRK",
            "Americans",
            10,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .expect("open-topped transport in limbo");
    let riders = sim
        .substrate
        .entities
        .get(hull)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()
        .to_vec();
    assert_eq!(kinds(&sim, &riders), ["E3", "E3"]);
    for rider in riders {
        let entity = sim.substrate.entities.get(rider).unwrap();
        assert!(
            entity.passenger_role.in_open_transport(),
            "the constructor admitted the rider open-topped"
        );
        assert_eq!(entity.passenger_role.inside_transport_id(), Some(hull));
        assert!(entity.lifecycle.in_limbo());
        assert!(
            sim.substrate.logic.as_slice().contains(&rider),
            "and EnteredOpenTopped710470 kept its AI slot"
        );
    }
}

/// BuildingExt::KickOutHospitalArmory (10012920): a capture (ChangeOwner,
/// 100163B0, hook 0x448277) and then a sale order (Sell_Back, 10016970,
/// hook 0x447113) each kick one trainee out of an `Armory=` building, the
/// head of its cargo first.
#[test]
fn armory_capture_and_sale_each_kick_out_one_trainee() {
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
