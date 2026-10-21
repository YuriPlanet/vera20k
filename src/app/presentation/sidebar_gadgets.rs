//! Poll the local sidebar's ready-building and Super predicates, and drop
//! sell mode once the player's House holds no counted building.
//!
//! Building/Defense tabs pulse while retained Factory links hold completed
//! buildings; Defense also pulses for a ready Super. Infantry never pulses.
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

/// Poll the retained strip links before the current frame's tab FlashAI.
/// Original StripAI 0x006A8D07..0x006A8D7A admits completed Building objects
/// on tabs0/1, independently of which tab is selected. The Defense predicate
/// also includes ready Supers; the shared gadget owner supplies phase/timing.
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
    let ready = sim.interner().get(&owner).map_or([false; 2], |owner| {
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .ready_building_tabs(sim, owner)
    });
    let flags = [
        ready[0],
        ready[1] || has_charged_sw_for_owner(sim, rules, &owner),
        false,
        false,
    ];
    let (tick, frame) = (sim.session().tick(), sim.session().binary_frame() as i32);
    state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .update_tab_flashes(tick, frame, flags);
}

fn has_charged_sw_for_owner(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: &str,
) -> bool {
    let owner_iid = sim.interner().get(owner).unwrap_or_default();
    crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid)
        .iter()
        .any(|sw| sw.is_ready)
}

#[cfg(test)]
#[path = "sidebar_gadgets_tests.rs"]
mod tests;
