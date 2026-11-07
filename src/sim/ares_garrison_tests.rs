//! Ares urban combat and raidable garrisons: the occupier admission
//! (`10017060`, hook `0x457D58`), the raid (`10017240`, `100136C0`) and the
//! pass-through hit (`BulletExt::DamageOccupants @ 10019AC0`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

const RULES: &str = "\
[InfantryTypes]
0=GI
1=E2
[VehicleTypes]
0=TANK
[AircraftTypes]
[BuildingTypes]
0=BNK
[Warheads]
0=AP
[GI]
Strength=100
Speed=4
Sight=4
Occupier=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E2]
Strength=100
Speed=4
Sight=4
Occupier=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[TANK]
Strength=400
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
Primary=Gun
[Gun]
Damage=30
ROF=50
Range=5
Projectile=Invisible
Warhead=AP
[Invisible]
Inviso=yes
SubjectToTrenches=no
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[BNK]
Strength=1000
TechLevel=1
CanBeOccupied=yes
MaxNumberOccupants=5
Bunker.Raidable=yes
CanBeOccupiedBy=GI
UC.PassThrough=100%
UC.DamageMultiplier=50%
";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[BNK]\nFoundation=2x2\n"),
    )
    .expect("garrison rules")
}

/// Americans (human) and Russians (computer), in that house order.
fn arena(rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = Simulation::with_seed(0x457D58);
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

fn owner(sim: &Simulation, id: u64) -> &str {
    sim.interner
        .resolve(sim.substrate.entities.get(id).unwrap().owner())
}

/// `CanBeOccupiedBy=GI` keeps E2 out; `Bunker.Raidable=yes` lets the
/// Americans' GI into the Russians' empty building, which then is theirs
/// until it is empty again.
#[test]
fn an_occupier_raids_an_empty_raidable_building_until_it_empties() {
    let rules = rules();
    let (mut sim, _grid) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "BNK", "Russians", 10, 10);
    let gi = spawn(&mut sim, &rules, "GI", "Americans", 12, 10);
    let e2 = spawn(&mut sim, &rules, "E2", "Americans", 12, 12);
    let admits = |sim: &Simulation, id| {
        crate::sim::passenger::building_can_be_occupied_by(sim, &rules, bunker, id)
    };
    assert!(!admits(&sim, e2), "CanBeOccupiedBy");
    assert!(admits(&sim, gi), "an empty raidable building");

    assert!(crate::sim::passenger::add_garrison_occupant(
        &mut sim,
        &rules,
        gi,
        bunker,
        None,
        FrameEffects::default(),
    ));
    assert_eq!(owner(&sim, bunker), "Americans", "raided");

    crate::sim::passenger::garrison_eject::kill_building_occupants(
        &mut sim,
        bunker,
        UninitContext::new(&rules, None),
    );
    assert_eq!(owner(&sim, bunker), "Russians", "returned when empty");
}

/// With `UC.PassThrough=100%` and a projectile that is not
/// `SubjectToTrenches`, the shot passes through to the occupant with
/// `UC.DamageMultiplier=50%` of its damage, and the building takes none.
#[test]
fn a_shot_passes_through_to_an_occupant() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "BNK", "Russians", 10, 10);
    let gi = spawn(&mut sim, &rules, "GI", "Russians", 12, 12);
    assert!(crate::sim::passenger::add_garrison_occupant(
        &mut sim,
        &rules,
        gi,
        bunker,
        None,
        FrameEffects::default(),
    ));
    let tank = spawn(&mut sim, &rules, "TANK", "Americans", 13, 10);
    let sender = sim.interner.get("Americans").unwrap();
    let tick = sim.session.tick() + 1;
    sim.queue_command(CommandEnvelope::new(
        sender,
        tick,
        Command::Attack {
            attacker_id: tank,
            target_id: bunker,
        },
    ));
    let health = |sim: &Simulation, id| sim.substrate.entities.get(id).unwrap().health.current();
    for _ in 0..200 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        if health(&sim, gi) < 100 {
            break;
        }
    }
    assert_eq!(health(&sim, gi), 85);
    assert_eq!(health(&sim, bunker), 1000);
}
