//! House Update's sell-mode exit (`0x004F8B08..0x004F8B2B`) against the
//! player House's tracked building count.

use super::exit_sell_mode_without_buildings;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

fn player(buildings: i32) -> (Simulation, InternedId) {
    let mut sim = Simulation::with_seed(0x004F_8B08);
    let house = sim.intern("Americans");
    let mut state = HouseState::new(house, 0, None, true, 0, 10);
    state
        .tracking_mut_for_test()
        .set_buildings_for_test(buildings);
    sim.houses_mut_for_test().register(state);
    (sim, house)
}

/// (sell, repair) after one House Update for a player with `buildings`.
fn after_update(buildings: i32, sell: bool, repair: bool, placement_armed: bool) -> (bool, bool) {
    let (sim, house) = player(buildings);
    let mut gadgets = SidebarGadgetState {
        sell_mode_on: sell,
        repair_mode_on: repair,
        ..SidebarGadgetState::default()
    };
    exit_sell_mode_without_buildings(&mut gadgets, &sim, Some(house), placement_armed);
    (gadgets.sell_mode_on, gadgets.repair_mode_on)
}

#[test]
fn sell_mode_drops_once_the_player_tracks_no_building() {
    // 0x004F8B14..0x004F8B2B: +0x2F0 == 0 with [0x880999] set -> SetSellMode(0).
    assert_eq!(after_update(0, true, false, false), (false, false));
    // 0x004F8B1A: any other count keeps it.
    assert_eq!(after_update(1, true, false, false), (true, false));
    // SetSellMode(0) refuses while a building placement is armed (0x004AC68F).
    assert_eq!(after_update(0, true, false, true), (true, false));
    // 0x004F8B1C reads sell mode only: Repair mode outlives the last building.
    assert_eq!(after_update(0, false, true, false), (false, true));
}
