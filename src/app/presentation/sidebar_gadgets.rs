//! Poll the local sidebar's ready-building and Super predicates, and drop
//! sell mode once the player's House holds no counted building.
//!
//! Building/Defense tabs pulse while retained Factory links hold completed
//! buildings; Defense also pulses while a Super passes its native tab-flash
//! predicate. Infantry never pulses.
//! Vehicle's aircraft-waiting predicate remains a separate existing residual.
//! Phase, catch-up and gadget state belong to `SidebarGadgetState`.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;

/// `HouseClass::Update @ 0x004F8440`, `0x004F8B08..0x004F8B2B`: the player's
/// House (`PlayerPtr`) whose tracked buildings (`+0x2F0`,
/// [`crate::sim::house_tracking::HouseTracking::buildings`]) are exactly 0
/// calls `DisplayClass::SetSellMode(0) @ 0x004AC660` while sell mode
/// (`[0x880999]`, Display `+0x11B1`) is on. That call is
/// [`SidebarGadgetState::disable_selection_modes`]: refused while a building
/// placement is armed (`+0x11A8`), otherwise it clears the modes, Repair
/// included, and reverts the cursor to the default arrow (vt+0x48
/// SetDefaultCursor(0, 0), vt+0x50 RevertCursor), which the cursor resolver
/// derives from the cleared mode. The test reads sell mode only, so a Repair
/// mode stays on.
///
/// The caller runs it after each simulation frame, whose House rung ran this
/// test. RESIDUAL: native reads the count at the player's House turn, this
/// the count the frame ends with. Losing the last building after that turn
/// drops sell mode a frame early (invisible); gaining one after that turn, in
/// the frame the last one went, keeps it on. Both need the count to change
/// after the House rung; the second needs a loss and a gain in one frame.
pub(crate) fn exit_sell_mode_without_buildings(
    gadgets: &mut SidebarGadgetState,
    sim: &Simulation,
    local_house: Option<InternedId>,
    placement_armed: bool,
) {
    if !gadgets.sell_mode_on {
        return;
    }
    let holds_no_building = local_house
        .and_then(|house| sim.houses().get(&house))
        .is_some_and(|house| house.tracking().buildings() == 0);
    if holds_no_building {
        gadgets.disable_selection_modes(placement_armed);
    }
}

/// Roll queued strip scrolls, then poll the retained strip links before the
/// current frame's tab FlashAI.
/// Original StripAI 0x006A8D07..0x006A8D7A admits completed Building objects
/// on tabs0/1, independently of which tab is selected. The Defense predicate
/// also includes ready Supers; the shared gadget owner supplies phase/timing.
/// The scroll roll moves one row per simulation frame; the view is rebuilt
/// from the moved row by the idle tick that follows.
pub(crate) fn update_sidebar_gadget_state(state: &mut AppState) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(rules) = state.rules() else {
        return;
    };
    let owner = preferred_local_owner_name(state).unwrap_or_else(|| "Americans".to_string());
    let owner_iid = sim.interner().get(&owner).unwrap_or_default();
    let superweapon_views =
        crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid);
    let ready = sim.interner().get(&owner).map_or([false; 2], |owner| {
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .ready_building_tabs(sim, owner)
    });
    let (tick, frame) = (sim.session().tick(), sim.session().binary_frame() as i32);
    let presentation = &mut state.match_state.match_presentation;
    // StripClass::AI scrolls before its tab poll, once per Sidebar Action.
    if let Some(rows) = presentation
        .sidebar_projection
        .view()
        .map(|view| view.layout.side2_tile_count)
    {
        let visits = tick.saturating_sub(presentation.sidebar_gadget_state.last_sim_tick);
        presentation
            .sidebar_projection
            .step_pending_scroll(rows, visits);
    }
    update_tab_flashes_for_owner(
        &mut presentation.sidebar_gadget_state,
        ready,
        &superweapon_views,
        tick,
        frame,
    );
}

/// StripClass::AI @ 0x006A8B30 calls SuperClass::ShouldFlashTab @
/// 0x006CE1A0 for each Defense-strip Super and stops only when no entry passes.
/// This is the single Rust update step combining that result with ready
/// buildings before the shared SidebarGadgetState advances its flash phase.
fn update_tab_flashes_for_owner(
    gadgets: &mut SidebarGadgetState,
    ready_building_tabs: [bool; 2],
    superweapon_views: &[crate::sim::superweapon::SuperWeaponView],
    tick: u64,
    frame: i32,
) {
    let has_flash_eligible_super = superweapon_views.iter().any(|sw| sw.should_flash_tab);
    gadgets.update_tab_flashes(
        tick,
        frame,
        [
            ready_building_tabs[0],
            ready_building_tabs[1] || has_flash_eligible_super,
            false,
            false,
        ],
    );
}

#[cfg(test)]
#[path = "sidebar_gadgets_tests.rs"]
mod tests;
