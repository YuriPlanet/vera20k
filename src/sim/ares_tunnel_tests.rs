//! Ares building tunnels through their production owners
//! (`docs/ares-building-hooks.md`): the `[TunnelTypes]` read, entry by
//! CAN_LOAD and the Unit arrival arm, the exit through another entrance's
//! Unload, and the last entrance's death and sale.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse};

const RULES: &str = "[Countries]\n0=Americans\n1=Russians\n\
     [TunnelTypes]\n0=Net\n\
     [Net]\nPassengers=2\n\
     [BuildingTypes]\n0=TUNA\n1=TUNX\n\
     [TUNA]\nStrength=500\nTunnel=NET\nSizeLimit=15\nSight=0\n\
     [TUNX]\nStrength=500\nTunnel=Other\nSizeLimit=15\nSight=0\n\
     [InfantryTypes]\n0=E1\n\
     [E1]\nStrength=125\nSight=0\nSpeed=4\n\
     Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
     [VehicleTypes]\n0=LTNK\n\
     [LTNK]\nStrength=300\nSight=0\nSpeed=6\n\
     Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n";

fn arena() -> (Simulation, RuleSet) {
    let art = IniFile::from_str("[TUNA]\nFoundation=1x1\n[TUNX]\nFoundation=1x1\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &art,
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x100097D0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object(kind, "Americans", rx, ry, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn americans(sim: &Simulation) -> InternedId {
    sim.interner.get("Americans").unwrap()
}

fn in_limbo(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).unwrap().in_limbo()
}

fn enter(sim: &mut Simulation, rules: &RuleSet, foot: u64, tunnel: u64) -> bool {
    let owner = americans(sim);
    sim.ares_can_enter_tunnel(foot, tunnel, owner, 0, rules, None, FrameEffects::default())
}

/// `Tunnel=` resolves case-insensitively against `[TunnelTypes]` (an
/// unlisted name is -1); CAN_LOAD admits a Foot while the House's network
/// has room under `Passengers=` (hook `0x43C326`), EnterTunnel limboes it
/// (`10009000`), and another entrance's Unload puts it out next to that
/// building (hook `0x44D8A7`, UnloadOne `100098A0`).
#[test]
fn a_foot_enters_one_tunnel_and_leaves_from_another() {
    let (mut sim, rules) = arena();
    let owner = americans(&sim);
    assert_eq!(rules.ares_building_globals().tunnel_max_cap(0), Some(2));
    let unlisted = spawn(&mut sim, &rules, "TUNX", 4, 4);
    assert_eq!(sim.ares_tunnel_type(unlisted, &rules), None);
    let west = spawn(&mut sim, &rules, "TUNA", 10, 10);
    let east = spawn(&mut sim, &rules, "TUNA", 20, 10);
    let feet = [(9, 10), (9, 12), (9, 14)].map(|(rx, ry)| spawn(&mut sim, &rules, "E1", rx, ry));

    assert!(enter(&mut sim, &rules, feet[0], west));
    assert!(enter(&mut sim, &rules, feet[1], west));
    assert!(in_limbo(&sim, feet[0]));
    assert_eq!(sim.ares_tunnels.members(owner, 0), [feet[0], feet[1]]);
    // The full network leaves CAN_LOAD to the later arms.
    let third = crate::sim::radio::transmit(
        &mut sim,
        feet[2],
        east,
        RadioMessage::CanEnter,
        RadioPayload::default(),
        &rules,
        FrameEffects::default(),
    );
    assert_ne!(third, RadioResponse::Roger);
    assert!(!in_limbo(&sim, feet[2]));

    assert!(sim.ares_tunnel_offers_unload(east, &rules));
    assert!(
        crate::sim::transport_unload::ares_tunnel_mission_unload(
            &mut sim,
            &rules,
            east,
            None,
            FrameEffects::default(),
        )
        .is_some()
    );
    // Last in, first out.
    assert!(!in_limbo(&sim, feet[1]));
    let out = &sim.substrate.entities.get(feet[1]).unwrap().position;
    let (dx, dy) = (i32::from(out.rx()) - 20, i32::from(out.ry()) - 10);
    assert_eq!(dx.abs().max(dy.abs()), 1, "next to the east entrance");
    assert_eq!(sim.ares_tunnels.members(owner, 0), [feet[0]]);
}

/// A unit ordered into a tunnel parks it as its pending entry like a
/// `UnitAbsorb=` building (hook `0x741CE5`); on arrival the Unit PerCell
/// arm asks it CAN_LOAD and the unit enters its own House's network (hook
/// `0x73A23F`).
#[test]
fn a_unit_ordered_into_a_tunnel_enters_on_arrival() {
    let (mut sim, rules) = arena();
    let owner = americans(&sim);
    let tunnel = spawn(&mut sim, &rules, "TUNA", 16, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", 12, 10);
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::clicked_mission(
            tank,
            MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(tunnel),
        ),
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().pending_entry(),
        Some(tunnel)
    );
    crate::sim::docking::building_dock::try_pending_entry(
        &mut sim,
        &rules,
        tank,
        FrameEffects::default(),
    );
    sim.foot_set_location_marked(
        tank,
        crate::sim::components::DriveCoord {
            x: 16 * 256 + 128,
            y: 10 * 256 + 128,
            z: 0,
        },
        &rules,
        None,
        FrameEffects::default(),
    );
    sim.per_cell_process(
        tank,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();

    assert!(in_limbo(&sim, tank));
    assert_eq!(sim.ares_tunnels.members(owner, 0), [tank]);
}

/// While another entrance stands, neither a sale (hook `0x44A37F`) nor a
/// death (hook `0x442DF2`) of one touches the network; the death of the
/// last entrance kills every member (KillFootsIfLastTunnel `100093C0`).
#[test]
fn only_the_last_entrance_kills_its_network() {
    let (mut sim, rules) = arena();
    let owner = americans(&sim);
    let west = spawn(&mut sim, &rules, "TUNA", 10, 10);
    let east = spawn(&mut sim, &rules, "TUNA", 20, 10);
    let feet = [(9, 10), (9, 12)].map(|(rx, ry)| spawn(&mut sim, &rules, "E1", rx, ry));
    for foot in feet {
        assert!(enter(&mut sim, &rules, foot, west));
    }
    let context = UninitContext::new(&rules, None);
    sim.ares_sell_tunnel(west, &rules, None, FrameEffects::default());
    sim.ares_kill_tunnel_feet_if_last(west, None, context);
    assert_eq!(sim.ares_tunnels.members(owner, 0), feet);

    sim.techno_limbo_with_context(west, context);
    sim.ares_kill_tunnel_feet_if_last(east, None, context);
    assert!(sim.ares_tunnels.members(owner, 0).is_empty());
    for foot in feet {
        assert!(
            sim.substrate
                .entities
                .get(foot)
                .is_none_or(|entity| !entity.is_object_alive())
        );
    }
}

/// The sale of the last entrance puts every member out on its cell
/// (PlaceOne `10009810`).
#[test]
fn a_sold_last_entrance_puts_its_network_out() {
    let (mut sim, rules) = arena();
    let owner = americans(&sim);
    let tunnel = spawn(&mut sim, &rules, "TUNA", 10, 10);
    let foot = spawn(&mut sim, &rules, "E1", 9, 10);
    assert!(enter(&mut sim, &rules, foot, tunnel));

    sim.ares_sell_tunnel(tunnel, &rules, None, FrameEffects::default());

    assert!(!in_limbo(&sim, foot));
    assert!(sim.ares_tunnels.members(owner, 0).is_empty());
}

/// `Passengers.Allowed`/`.Disallowed` (IsPassengerAllowed `1003DD60`) gate
/// every carrier's CAN_LOAD by the sender's type: a tunnel entrance's
/// (`100174E0`, hook `0x43C326`) and a Unit transport's (`1004C270`, hook
/// `0x73769E`), which answer NEGATORY where the stock receiver says ROGER.
#[test]
fn passenger_lists_gate_building_and_transport_can_load() {
    const LISTS: &str = "[Countries]\n0=Americans\n1=Russians\n\
         [TunnelTypes]\n0=Net\n\
         [Net]\nPassengers=4\n\
         [BuildingTypes]\n0=TUNA\n\
         [TUNA]\nStrength=500\nTunnel=Net\nSizeLimit=15\nSight=0\n\
         Passengers.Disallowed=E2\n\
         [InfantryTypes]\n0=E1\n1=E2\n\
         [E1]\nStrength=125\nSight=0\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [E2]\nStrength=125\nSight=0\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [VehicleTypes]\n0=APC\n\
         [APC]\nStrength=300\nSight=0\nSpeed=6\nPassengers=4\nSizeLimit=2\n\
         Passengers.Allowed=E2\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n";
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(LISTS),
        &IniFile::from_str("[TUNA]\nFoundation=1x1\n"),
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x1003DD60);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tunnel = spawn(&mut sim, &rules, "TUNA", 10, 10);
    let apc = spawn(&mut sim, &rules, "APC", 20, 10);
    let e1 = spawn(&mut sim, &rules, "E1", 15, 12);
    let e2 = spawn(&mut sim, &rules, "E2", 15, 14);
    let can_load =
        |carrier, from| crate::sim::radio::receive::can_load_query(&sim, carrier, from, &rules);

    assert_eq!(can_load(tunnel, e1), RadioResponse::Roger);
    assert_eq!(can_load(tunnel, e2), RadioResponse::Negatory);
    assert_eq!(can_load(apc, e2), RadioResponse::Roger);
    assert_eq!(can_load(apc, e1), RadioResponse::Negatory);
}
