//! Regression tests for the Ares abduction (`conductAbduction` 10054A80).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[InfantryTypes]
0=E1
1=E2
[VehicleTypes]
0=CATCHER
[AircraftTypes]
[BuildingTypes]
[Warheads]
0=GrabWH
[E1]
Strength=100
Speed=4
Sight=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E2]
Strength=100
Speed=4
Sight=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
ImmuneToAbduction=yes
[CATCHER]
Strength=400
Speed=4
ROT=5
Passengers=2
SizeLimit=2
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
Primary=Grab
[Grab]
Damage=30
ROF=50
Range=3
Projectile=Invisible
Warhead=GrabWH
Abductor=yes
Abductor.AbductBelowPercent=0.5
[Invisible]
Inviso=yes
[GrabWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).expect("abduction rules")
}

/// Americans (human) and Russians (computer), in that house order.
fn arena(rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = Simulation::with_seed(51);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
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

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().health.current()
}

fn carried(sim: &Simulation, carrier: u64) -> Vec<u64> {
    sim.substrate
        .entities
        .get(carrier)
        .and_then(|entity| entity.passenger_role.cargo())
        .map_or_else(Vec::new, |cargo| cargo.passengers().to_vec())
}

/// The snapped Inviso shot of an `Abductor` weapon (hook 0x46920B) puts a
/// target at or below `AbductBelowPercent` into the firer's hold and deals
/// it nothing; one above it takes the hit.
// Runs again once combat's warhead effects move to the switch.
#[cfg(feature = "ares")]
#[test]
fn an_abductor_shot_takes_a_weakened_target_into_its_hold() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let catcher = spawn(&mut sim, &rules, "CATCHER", "Russians", 10, 10);
    let target = spawn(&mut sim, &rules, "E1", "Americans", 12, 10);
    let sender = sim.interner.get("Americans").unwrap();
    let fire_until = |sim: &mut Simulation, done: &dyn Fn(&Simulation) -> bool| {
        let tick = sim.session.tick() + 1;
        sim.queue_command(CommandEnvelope::new(
            sender,
            tick,
            Command::Attack {
                attacker_id: catcher,
                target_id: target,
            },
        ));
        for _ in 0..200 {
            let commands = sim.take_due_commands();
            sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
            if done(sim) {
                return true;
            }
        }
        false
    };
    assert!(fire_until(&mut sim, &|sim| health(sim, target) < 100));
    assert_eq!(
        health(&sim, target),
        70,
        "above half health: an ordinary hit"
    );
    assert!(carried(&sim, catcher).is_empty());

    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .health
        .set_for_test(50);
    assert!(fire_until(&mut sim, &|sim| !carried(sim, catcher).is_empty()));
    assert_eq!(carried(&sim, catcher), [target]);
    let abductee = sim.substrate.entities.get(target).unwrap();
    assert!(abductee.lifecycle.in_limbo());
    assert_eq!(
        abductee.health.current(),
        50,
        "the neutered bullet deals nothing"
    );
    assert_eq!(abductee.passenger_role.transporter(), Some(catcher));
}

/// conductAbduction's refusals: `ImmuneToAbduction` (TechnoTypeExt +0x304)
/// and a full hold (CanHoldPassenger by size, `Passengers.BySize`).
#[test]
fn immune_types_and_full_holds_refuse_an_abduction() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let (mut sim, _grid) = arena(&rules);
    let catcher = spawn(&mut sim, &rules, "CATCHER", "Russians", 10, 10);
    let immune = spawn(&mut sim, &rules, "E2", "Americans", 12, 10);
    let first = spawn(&mut sim, &rules, "E1", "Americans", 12, 12);
    let second = spawn(&mut sim, &rules, "E1", "Americans", 13, 12);
    let third = spawn(&mut sim, &rules, "E1", "Americans", 14, 12);
    for id in [immune, first, second, third] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(10);
    }
    let grab = rules.weapon("Grab").unwrap();
    let abduct = |sim: &mut Simulation, id| {
        sim.ares_conduct_abduction(catcher, id, grab, &rules, None, FrameEffects::default())
    };
    assert!(!abduct(&mut sim, immune));
    assert!(abduct(&mut sim, first));
    assert!(abduct(&mut sim, second));
    assert!(!abduct(&mut sim, third), "Passengers=2 by size");
    assert_eq!(carried(&sim, catcher), [second, first]);
}
