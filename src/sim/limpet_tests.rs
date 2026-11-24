//! Tiberian Sun limpets through production fire, heal and depot paths, on
//! Firestorm's own `[LIMP]`, `[LimpetBullet]`, `[LIMPY]`, `[DLIMPET]` and
//! `[LIMPET]` values (FIRESTRM.INI).

use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[VehicleTypes]
0=HTNK
1=LIMPET
[BuildingTypes]
0=DLIMPET
1=DEPOT
2=HTNKD
[Warheads]
0=LIMPY
[HTNK]
Strength=1000
Speed=8
Sight=4
ROT=5
DeploysInto=HTNKD
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[HTNKD]
Strength=1000
Sight=4
DeployFacing=0
[LIMPET]
Strength=100
Speed=8
IsLimpetDrone=yes
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[DEPOT]
Strength=800
UnitRepair=yes
[DLIMPET]
Strength=100
Sight=5
Primary=LIMP
IsLimpetMine=true
[LIMP]
Damage=1
ROF=80
Range=2
Projectile=LimpetBullet
Speed=100
Warhead=LIMPY
[LimpetBullet]
Inviso=yes
AV=true
[LIMPY]
LimpetFactor=35
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

type Grid = crate::sim::pathfinding::PathGrid;

fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &Grid) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

/// A flat arena with the Americans (human, the player) and the Russians.
fn arena() -> (Simulation, RuleSet, Grid) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(RULES))
        .expect("limpet rules");
    let mut sim = Simulation::with_seed(7);
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, human, 0, 10));
    }
    let americans = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(americans));
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    *sim.fog = crate::sim::vision::FogState::new(32, 32);
    (sim, rules, grid)
}

/// An Americans mine finds a Russian tank next to it through its own guard
/// scan, whose `AV=` projectile looks for vehicles only (class bits 0x10,
/// `sim::combat::combat_weapon`), and limpets it: the tank and the
/// Americans' house.
fn limpeted() -> (Simulation, RuleSet, Grid, u64, InternedId) {
    let (mut sim, rules, grid) = arena();
    let americans = sim.interner.intern("Americans");
    let mine = sim
        .spawn_object_at_height("DLIMPET", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("HTNK", "Russians", 11, 10, 0, 0, &rules)
        .unwrap();
    assert!(!sim.fog.sight_is_latched(tank, americans));
    // The player has seen the enemy: a human player's scan takes nothing
    // else (`sim::arena_fixture::discover`).
    crate::sim::arena_fixture::discover(&mut sim, tank);
    let alive = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(mine)
            .is_some_and(|entity| entity.lifecycle.object_alive())
    };
    for _ in 0..200 {
        if !alive(&sim) {
            break;
        }
        tick(&mut sim, &rules, &grid);
    }
    assert!(!alive(&sim), "the mine is deleted with its shot");
    (sim, rules, grid, tank, americans)
}

/// The tank's type speed after its max speed (`FootClass::GetCurrentSpeed`
/// stages 1 and 2), in leptons per frame.
fn type_speed(sim: &Simulation, rules: &RuleSet, tank: u64) -> i32 {
    let entity = sim.substrate.entities.get(tank).unwrap();
    let per_second = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        rules.object("HTNK"),
        8,
        rules.general.veteran_speed,
        crate::util::native_x87::NativeF32Bits::ONE,
    );
    (per_second / crate::util::fixed_math::SimFixed::from_num(15)).to_num()
}

/// Firestorm 2.0 `TechnoClass::Fire_At` (`0x006301EB..0x0063032A`): the
/// shot launches nothing and does no damage, the mine is deleted, and the
/// tank's `Speed=8` (20 leptons a frame) times the float factor 0.64999998
/// truncates to 12, not 13 (`UnitClass::Get_Max_Speed @ 0x00659710`). The
/// limpeting player now sees what the enemy tank sees (`TechnoClass::Look`
/// `0x0063829A`).
///
/// Deployed (`UnitClass::Try_To_Deploy` `0x00650D9E..0x00650DB6`), the
/// building it makes carries the limpets and that sight.
#[test]
fn a_limpet_mine_limpets_a_vehicle_instead_of_shooting_it() {
    let (mut sim, rules, _, tank, americans) = limpeted();
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(entity.health.current(), 1000, "no damage");
    assert!(entity.seldom.limpets.attached_by(americans));
    assert_eq!(type_speed(&sim, &rules, tank), 12);
    assert!(sim.fog.sight_is_latched(tank, americans));
    assert!(sim.fog.is_cell_visible(americans, 14, 10));

    assert!(sim.deploy_mcv(tank, &rules, None, &mut FrameEffects::default()));
    let deployed = sim.interner.get("HTNKD").unwrap();
    let building = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.type_ref() == deployed)
        .expect("the tank deployed");
    assert!(building.seldom.limpets.attached_by(americans));
    assert!(sim.fog.sight_is_latched(building.stable_id(), americans));
}

/// A heal (`TechnoClass::Take_Damage` `0x006326E1`) and a depot repair at
/// full health (RADIO_REPAIR `0x0062ABE1`) each take the limpets off, and
/// the limpeting player's sight through the tank with them.
#[test]
fn a_heal_or_a_depot_repair_takes_the_limpets_off() {
    for heal in [true, false] {
        let (mut sim, rules, _, tank, americans) = limpeted();
        let mut effects = FrameEffects::default();
        if heal {
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                tank,
                -1,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                sim.interner.intern("LIMPY"),
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            );
            sim.commit_direct_damage_receiver(&rules, None, event, &mut effects);
        } else {
            let reply = crate::sim::radio::receive_radio(
                &mut sim,
                tank,
                None,
                RadioMessage::RepairTick,
                RadioPayload::default(),
                &rules,
                &mut effects,
            );
            assert_eq!(reply, RadioResponse::Negatory, "full health");
        }
        let entity = sim.substrate.entities.get(tank).unwrap();
        assert!(!entity.seldom.limpets.is_attached(), "heal {heal}");
        assert!(!entity.foot_speed.limpeted(), "heal {heal}");
        assert_eq!(type_speed(&sim, &rules, tank), 20, "heal {heal}");
        assert!(!sim.fog.sight_is_latched(tank, americans), "heal {heal}");
        assert!(!sim.fog.is_cell_visible(americans, 14, 10), "heal {heal}");
    }
}

/// A Service Depot takes a full-health limpeted tank as it takes a damaged
/// one, since its RepairTick takes the limpets off: Firestorm 2.0 asks no
/// health (`BuildingClass::Receive_Message` `0x00426B4A..0x00426B79`,
/// `UnitClass::Assign_Destination` `0x00657EA5..0x00657F45`). Sent to a
/// busy depot it waits its turn, and a depot it is already in contact with
/// does not refuse it.
#[test]
fn a_service_depot_takes_a_limpeted_tank_at_full_health() {
    let (mut sim, rules, grid, tank, _) = limpeted();
    let russians = sim.interner.intern("Russians");
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_player_control_for_test(true);
    let busy = sim
        .spawn_object_at_height("DEPOT", "Russians", 16, 10, 0, 0, &rules)
        .unwrap();
    let free = sim
        .spawn_object_at_height("DEPOT", "Russians", 16, 16, 0, 0, &rules)
        .unwrap();
    let other = sim
        .spawn_object_at_height("HTNK", "Russians", 20, 10, 0, 0, &rules)
        .unwrap();
    let mut effects = FrameEffects::default();
    let mut dock = |sim: &mut Simulation, from, depot| {
        crate::sim::radio::transmit(
            sim,
            from,
            depot,
            RadioMessage::CanDock,
            RadioPayload::default(),
            &rules,
            &mut effects,
        )
    };
    assert_eq!(dock(&mut sim, other, busy), RadioResponse::Roger);
    assert!(sim.apply_command(
        "Russians",
        &crate::sim::command::Command::clicked_mission(
            tank,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(busy),
        ),
        Some(&rules),
    ));
    tick(&mut sim, &rules, &grid);
    let waits = sim.substrate.entities.get(tank).unwrap().pending_entry();
    assert_eq!(dock(&mut sim, tank, free), RadioResponse::Roger);
    let linked = dock(&mut sim, tank, free);
    assert_eq!((waits, linked), (Some(busy), RadioResponse::Roger));
}

/// Firestorm 2.0 `UnitClass::Unlimbo` starts a Limpet Drone's Stage at
/// rate 1 (`0x0064F83D..0x0064F85E`), the Techno AI adds one a frame and
/// `UnitClass::AI` wraps it at 10 (`0x0064E205`): its body's ten frames.
#[test]
fn a_limpet_drone_loops_its_stage_through_ten_frames() {
    let (mut sim, rules, grid) = arena();
    let drone = sim
        .spawn_object_at_height("LIMPET", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    let stages: Vec<i32> = (0..25)
        .map(|_| {
            tick(&mut sim, &rules, &grid);
            let entity = sim.substrate.entities.get(drone).unwrap();
            entity.native_stage().value()
        })
        .collect();
    assert!(
        stages.windows(2).all(|pair| pair[1] == (pair[0] + 1) % 10),
        "{stages:?}"
    );
}
