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

#[test]
fn defense_tab_flash_expires_by_ready_frame_and_keeps_other_eligible_supers() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n\
         [Americans]\nSide=GDI\n[SuperWeaponTypes]\n\
         0=SpyPlaneSpecial\n1=PsychicRevealSpecial\n2=ForceShieldSpecial\n3=ZeroSpecial\n4=NukeSpecial\n\
         [SpyPlaneSpecial]\nType=SpyPlane\nFlashSidebarTabFrames=120\n\
         [PsychicRevealSpecial]\nType=PsychicReveal\nFlashSidebarTabFrames=120\n\
         [ForceShieldSpecial]\nType=ForceShield\nUseChargeDrain=yes\nFlashSidebarTabFrames=120\n\
         [ZeroSpecial]\nType=SpyPlane\nFlashSidebarTabFrames=0\n\
         [NukeSpecial]\nType=MultiMissile\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();

    let mut sim = Simulation::with_seed(0x006C_E1A0);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.intern("Americans");
    add_flash_super(&mut sim, owner, "SpyPlaneSpecial", 100, true);

    let mut gadgets = SidebarGadgetState::default();
    // 100 + 120 is still greater than frame 219, but not frame 220.
    sim.session_mut_for_test().set_binary_frame_for_test(219);
    update_flash(&mut gadgets, &sim, &rules, 219);
    assert!(gadgets.tab_flashes[1].is_active());

    sim.session_mut_for_test().set_binary_frame_for_test(220);
    update_flash(&mut gadgets, &sim, &rules, 220);
    assert!(!gadgets.tab_flashes[1].is_active());

    let force_shield = add_flash_super(&mut sim, owner, "ForceShieldSpecial", 210, false);
    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&force_shield)
        .unwrap()
        .set_charge_drain_state_for_test(2);
    update_flash(&mut gadgets, &sim, &rules, 221);
    assert!(
        gadgets.tab_flashes[1].is_active(),
        "drain state 2 passes +7C!=0"
    );
    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&force_shield)
        .unwrap()
        .set_charge_drain_state_for_test(0);
    update_flash(&mut gadgets, &sim, &rules, 222);
    assert!(!gadgets.tab_flashes[1].is_active());

    add_flash_super(&mut sim, owner, "ZeroSpecial", 210, true);
    update_flash(&mut gadgets, &sim, &rules, 223);
    assert!(
        !gadgets.tab_flashes[1].is_active(),
        "zero disables this type"
    );

    add_flash_super(&mut sim, owner, "PsychicRevealSpecial", 210, true);
    update_flash(&mut gadgets, &sim, &rules, 224);
    assert!(
        gadgets.tab_flashes[1].is_active(),
        "another eligible type keeps the shared flash alive"
    );

    add_flash_super(&mut sim, owner, "NukeSpecial", -100, true);
    sim.session_mut_for_test().set_binary_frame_for_test(10_000);
    update_flash(&mut gadgets, &sim, &rules, 10_000);
    assert!(gadgets.tab_flashes[1].is_active());
}

fn update_flash(
    gadgets: &mut SidebarGadgetState,
    sim: &Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    tick: u64,
) {
    let owner = sim.interner().get("Americans").unwrap_or_default();
    let views = crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner);
    super::update_tab_flashes_for_owner(
        gadgets,
        [false; 2],
        &views,
        tick,
        sim.session().binary_frame() as i32,
    );
}

fn add_flash_super(
    sim: &mut Simulation,
    owner: InternedId,
    name: &str,
    ready_tick: i32,
    ready: bool,
) -> InternedId {
    let type_id = sim.interner().get(name).expect("retail SuperWeaponType ID");
    let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(type_id, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_is_ready_for_test(ready);
    instance.set_ready_tick_for_test(ready_tick);
    sim.super_weapons_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(type_id, instance);
    type_id
}
