//! Ares unit sale on a depot: FootClass::CanBeSold (0x004D9E70) with
//! FootClass_CanBeSold_SellUnit (10048FD0, hook 0x4D9EBD) and
//! FootClass_CanBeSold_Dock (1005CA30, hook 0x4D9EE1), and the SELL event's
//! unit arm (0x004C6F5A..0x004C6F96) into On_Sold (0x004D9F70).

use super::foot_can_be_sold;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;

/// A 1x1 depot at (14, 10) and a tank in radio contact with it, standing
/// `dx` leptons east of the depot's dock coordinate for it.
fn scene(depot: &str, tank: &str, general: &str, dx: i32) -> (Simulation, RuleSet, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nRefundPercent=50%\n{general}\
         [VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=DEPOT\n\
         [TANK]\nStrength=300\nCost=600\nSpeed=6\nSpeedType=Track\n\
         Locomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n{tank}\n\
         [DEPOT]\nStrength=800\n{depot}\n"
    ));
    let art = IniFile::from_str("[DEPOT]\nFoundation=1x1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    let depot = sim
        .spawn_object_at_height("DEPOT", "Americans", 14, 10, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .mark_live_contact_with(depot);
    let mut at = crate::sim::movement::building_dock_coordinate(
        &sim.substrate.entities,
        depot,
        Some(tank),
        sim.resolved_terrain.as_ref(),
        &rules,
        &sim.interner,
    )
    .unwrap();
    at.x += dx;
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .position
        .put_location(at);
    (sim, rules, tank)
}

/// The distance is measured to the dock coordinate and must stay below 0x80.
#[test]
fn a_unit_on_a_depot_can_be_sold_within_0x80_leptons_of_its_dock() {
    for (dx, sellable) in [(0x7F, true), (0x80, false)] {
        let (sim, rules, tank) = scene("UnitRepair=yes", "", "", dx);
        assert_eq!(foot_can_be_sold(&sim, &rules, tank), sellable, "dx {dx}");
    }
}

/// `UnitSell` defaults to `UnitRepair`; `Unsellable` defaults to
/// `[General] UnitsUnsellable`.
#[test]
fn unit_sell_and_unsellable_decide_whether_the_unit_sells() {
    for (depot, tank, general, sellable) in [
        ("UnitRepair=yes\nUnitSell=no", "", "", false),
        ("UnitSell=yes", "", "", true),
        ("UnitRepair=yes", "", "UnitsUnsellable=yes\n", false),
        (
            "UnitRepair=yes",
            "Unsellable=no",
            "UnitsUnsellable=yes\n",
            true,
        ),
    ] {
        let (sim, rules, tank) = scene(depot, tank, general, 0);
        assert_eq!(
            foot_can_be_sold(&sim, &rules, tank),
            sellable,
            "{depot:?} {tank:?} {general:?}"
        );
    }
}

/// The SELL event sells a unit standing in a building's cell: the owner's
/// refund (Cost 600 at RefundPercent 50% for a human house), then UnInit.
/// Off the building it does nothing.
#[test]
fn the_sell_event_sells_a_unit_standing_on_a_building() {
    for (dx, sold) in [(0, true), (0x200, false)] {
        let (mut sim, rules, tank) = scene("UnitRepair=yes", "", "", dx);
        let owner = sim.interner.get("Americans").unwrap();
        let before = sim.houses[&owner].economy().credits();
        assert_eq!(
            sim.apply_command(
                "Americans",
                &Command::SellBuilding { entity_id: tank },
                Some(&rules)
            ),
            sold
        );
        assert_eq!(
            sim.houses[&owner].economy().credits() - before,
            if sold { 300 } else { 0 }
        );
        let alive = sim
            .substrate
            .entities
            .get(tank)
            .is_some_and(|entity| entity.lifecycle.object_alive());
        assert_eq!(alive, !sold);
    }
}
