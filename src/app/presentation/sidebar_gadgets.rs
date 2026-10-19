//! Poll the local sidebar's ready-building and Super predicates.
//!
//! Building/Defense tabs pulse while retained Factory links hold completed
//! buildings; Defense also pulses for a ready Super. Infantry never pulses.
//! Vehicle's aircraft-waiting predicate remains a separate existing residual.
//! Phase, catch-up and gadget state belong to `SidebarGadgetState`.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;

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
    let ready = sim.interner.get(&owner).map_or([false; 2], |owner| {
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
    let (tick, frame) = (sim.session.tick(), sim.session.binary_frame() as i32);
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
    let owner_iid = sim.interner.get(owner).unwrap_or_default();
    crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid)
        .iter()
        .any(|sw| sw.is_ready)
}
