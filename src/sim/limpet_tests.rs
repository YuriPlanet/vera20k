//! Tiberian Sun limpets through production fire, heal and depot paths, on
//! Firestorm's own `[LIMP]`, `[LimpetBullet]`, `[LIMPY]` and `[DLIMPET]`
//! values (FIRESTRM.INI).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[VehicleTypes]
0=HTNK
[BuildingTypes]
0=DLIMPET
[Warheads]
0=LIMPY
[HTNK]
Strength=1000
Speed=8
Sight=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
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

/// An Americans (human, the player) mine limpets a Russian tank next to it:
/// the tank and the Americans' house.
fn limpeted() -> (Simulation, RuleSet, u64, InternedId) {
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
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    let mine = sim
        .spawn_object_at_height("DLIMPET", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("HTNK", "Russians", 11, 10, 0, 0, &rules)
        .unwrap();
    assert!(!sim.fog.sight_is_latched(tank, americans));
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: mine,
            target_id: tank,
        },
    ));
    let alive = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(mine)
            .is_some_and(|entity| entity.lifecycle.object_alive())
    };
    for _ in 0..60 {
        if !alive(&sim) {
            break;
        }
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    }
    assert!(!alive(&sim), "the mine is deleted with its shot");
    (sim, rules, tank, americans)
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
#[test]
fn a_limpet_mine_limpets_a_vehicle_instead_of_shooting_it() {
    let (sim, rules, tank, americans) = limpeted();
    let entity = sim.substrate.entities.get(tank).unwrap();
    assert_eq!(entity.health.current(), 1000, "no damage");
    assert!(entity.seldom.limpets.attached_by(americans));
    assert_eq!(type_speed(&sim, &rules, tank), 12);
    assert!(sim.fog.sight_is_latched(tank, americans));
    assert!(sim.fog.is_cell_visible(americans, 14, 10));
}

/// A heal (`TechnoClass::Take_Damage` `0x006326E1`) and a depot repair at
/// full health (RADIO_REPAIR `0x0062ABE1`) each take the limpets off, and
/// the limpeting player's sight through the tank with them.
#[test]
fn a_heal_or_a_depot_repair_takes_the_limpets_off() {
    for heal in [true, false] {
        let (mut sim, rules, tank, americans) = limpeted();
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
