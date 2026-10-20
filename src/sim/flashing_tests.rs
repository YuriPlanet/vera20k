//! Rust regression tests of the vt+0x148 Flash writers: the MegaMission
//! order-target flash (`0x004C739F`), the heal flash (`0x005F5486`) and the
//! Techno body's disguise gate (`0x006F9DD0`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::DisguiseRuntime;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
0=HTNK
[AircraftTypes]
[BuildingTypes]
0=GAPOWR
[Warheads]
0=SA
[E1]
Strength=125
Speed=4
Sight=6
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=M60
[HTNK]
Strength=1000
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
Foundation=2x2
[M60]
Damage=15
ROF=20
Range=5
Projectile=Invisible
Warhead=SA
[Invisible]
Inviso=yes
[SA]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).expect("flash rules")
}

/// Americans and Russians, enemies; the current player is `player`.
fn arena(rules: &RuleSet, player: &str) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = Simulation::with_seed(7);
    for (name, side, human) in [
        ("Americans", 0, true),
        ("Russians", 1, false),
        ("British", 0, false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    sim.session
        .set_current_house_for_test(Some(sim.interner.intern(player)));
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn flash(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().flashing.frames()
}

fn order(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    command: Command,
    owner: &str,
) {
    let owner = sim.interner.intern(owner);
    sim.queue_command(CommandEnvelope::new(owner, sim.session.tick() + 1, command));
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

#[test]
fn an_order_flashes_its_object_only_for_the_ordering_objects_allies() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules, "Americans");
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 10, 10);
    let conscript = spawn(&mut sim, &rules, "E1", "Russians", 20, 20);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    let own_tank = spawn(&mut sim, &rules, "HTNK", "Americans", 20, 10);

    order(
        &mut sim,
        &rules,
        &grid,
        Command::Attack {
            attacker_id: gi,
            target_id: tank,
        },
        "Americans",
    );
    // Flash(7) at the event, which runs after this frame's Logic; the
    // target's own late Techno countdown steps it from the next frame.
    assert_eq!(flash(&sim, tank), 7);
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    assert_eq!(flash(&sim, tank), 6);

    // The Russians' order is executed too, but the current player is not
    // allied with its actor.
    order(
        &mut sim,
        &rules,
        &grid,
        Command::Attack {
            attacker_id: conscript,
            target_id: own_tank,
        },
        "Russians",
    );
    assert_eq!(flash(&sim, own_tank), 0);
}

#[test]
fn an_attack_move_to_a_cell_flashes_nothing_even_with_an_object_target() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules, "Americans");
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 10, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    order(
        &mut sim,
        &rules,
        &grid,
        Command::AttackMove {
            entity_id: gi,
            target: Some(crate::sim::combat::TargetKind::Entity(tank)),
            destination: Some(crate::sim::combat::TargetKind::Cell(15, 10)),
        },
        "Americans",
    );
    assert_eq!(flash(&sim, tank), 0);
}

fn receive(sim: &mut Simulation, rules: &RuleSet, id: u64, damage: i32) {
    let warhead = sim.interner.intern("SA");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        id,
        damage,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_direct_damage_receiver(rules, None, hit, FrameEffects::default());
}

#[test]
fn a_heal_flashes_only_when_it_changes_strength() {
    let rules = rules();
    let (mut sim, _grid) = arena(&rules, "Americans");
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    receive(&mut sim, &rules, tank, -50);
    assert_eq!(flash(&sim, tank), 0, "already at full Strength");
    receive(&mut sim, &rules, tank, 100);
    assert_eq!(flash(&sim, tank), 0, "damage does not flash");
    receive(&mut sim, &rules, tank, -50);
    assert_eq!(flash(&sim, tank), 7);
}

#[test]
fn a_unit_disguised_without_a_house_stays_dark_to_the_enemy_player() {
    let rules = rules();
    let (mut sim, _grid) = arena(&rules, "Americans");
    let mirage = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    let spy = spawn(&mut sim, &rules, "E1", "Russians", 16, 10);
    let worn = Some(sim.interner.intern("E1"));
    for id in [mirage, spy] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .disguise
            .get_or_insert_with(|| DisguiseRuntime::new(0))
            .acquire(0, worn, None);
    }
    sim.flash_object(mirage, 7);
    sim.flash_object(spy, 7);
    assert_eq!(flash(&sim, mirage), 0);
    // Infantry's GetDisguiseHouse(1) falls back on the player's house.
    assert_eq!(flash(&sim, spy), 7);

    sim.session
        .set_current_house_for_test(Some(sim.interner.intern("Russians")));
    sim.flash_object(mirage, 5);
    assert_eq!(flash(&sim, mirage), 5, "its owner sees through it");
}

/// The player's own ally mask decides (`0x004F9A90` reads the asker's +5788):
/// a house that names the player an ally, without the player naming it back,
/// does not flash for the player.
#[test]
fn the_order_flash_asks_the_current_players_one_way_alliance() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules, "Americans");
    let tommy = spawn(&mut sim, &rules, "E1", "British", 10, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    let attack = Command::Attack {
        attacker_id: tommy,
        target_id: tank,
    };

    sim.house_alliances_mut_for_test()
        .insert("BRITISH".into(), ["AMERICANS".to_string()].into());
    order(&mut sim, &rules, &grid, attack.clone(), "British");
    assert_eq!(flash(&sim, tank), 0);

    sim.house_alliances_mut_for_test()
        .insert("AMERICANS".into(), ["BRITISH".to_string()].into());
    order(&mut sim, &rules, &grid, attack, "British");
    assert_eq!(flash(&sim, tank), 7);
}

/// A building keeps the base gate (`0x0041C020`/`0x0041BE70`): its raw
/// disguise flag hides it from every player, its owner included.
#[test]
fn a_building_flashes_unless_its_raw_disguise_flag_is_up() {
    let rules = rules();
    let mut sim = Simulation::with_seed(7);
    let americans = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(americans, 0, None, true, 0, 10));
    sim.session.set_current_house_for_test(Some(americans));
    let power = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
    sim.flash_object(power, 7);
    assert_eq!(flash(&sim, power), 7);
    sim.substrate
        .entities
        .get_mut(power)
        .unwrap()
        .disguise
        .get_or_insert_with(|| DisguiseRuntime::new(0))
        .acquire(0, None, Some(americans));
    sim.flash_object(power, 3);
    assert_eq!(flash(&sim, power), 7);
}

/// A present destination object wins over the target object.
#[test]
fn the_order_flash_prefers_the_destination_object() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules, "Americans");
    let gi = spawn(&mut sim, &rules, "E1", "Americans", 10, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 13, 10);
    let other = spawn(&mut sim, &rules, "HTNK", "Russians", 16, 10);
    order(
        &mut sim,
        &rules,
        &grid,
        Command::AttackMove {
            entity_id: gi,
            target: Some(crate::sim::combat::TargetKind::Entity(tank)),
            destination: Some(crate::sim::combat::TargetKind::Entity(other)),
        },
        "Americans",
    );
    assert_eq!((flash(&sim, tank), flash(&sim, other)), (0, 7));
}
