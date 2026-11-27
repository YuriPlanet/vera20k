//! Poll the local sidebar's ready-building and Super predicates, and drop
//! sell mode once the player's House holds no counted building.
//!
//! Building/Defense tabs pulse while retained Factory links hold completed
//! buildings; Defense also pulses while a Super passes its native tab-flash
//! predicate. Infantry and Vehicle never pulse: StripClass::AI polls only
//! tabs 0 and 1, and ShapeButton StartFlashing 69DFC0 has no other live caller.
//! Phase, catch-up and gadget state belong to `SidebarGadgetState`.

use crate::app::frontend::state::FrontendState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::match_runtime::state::MatchState;
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

/// How many times the Sidebar Action 6A7780 clocks run on this pass, for the
/// per-visit work they drive: StripClass::AI's scroll step and tab poll,
/// FlashAI 69E010 (6A7A8C), PowerClass AnimationTick 63FEA0 (its only call,
/// 6A7B0A) and, through TabClass::Action 6D099D, CreditsClass::AI 4A2600.
///
/// VERA keeps these UI clocks in step with the world: a deactivated window
/// does not stop them (unlike gamemd's `A8ED80`-gated GScreen Input poll at
/// 55D8AB). Only an offline focus
/// freeze, a menu, a scenario pause or a Wait_For_Players dialog stops them,
/// through `RuntimePassDecision::sidebar_action`. In g_GameMode 0 and 5
/// Main_Tick polls Input once per frame (55D8AB) because ThrottleFrame skips
/// its spin (55E1A5..55E1B6); the network modes' spin also polls Input while
/// more than 10 ms of the frame remain (55E23A..55E271). One VERA pass stands
/// for that poll. Offline the single-player world supplies one tick per frame,
/// so `tick - last_tick` visits the clocks once per simulated frame.
pub(crate) fn sidebar_action_visits(
    sidebar_action: bool,
    single_player: bool,
    tick: u64,
    last_tick: u64,
) -> u64 {
    if !sidebar_action {
        0
    } else if single_player {
        tick.saturating_sub(last_tick)
    } else {
        1
    }
}

/// [`sidebar_action_visits`] for the running match's current pass.
pub(crate) fn current_sidebar_action_visits(match_state: &MatchState, sidebar_action: bool) -> u64 {
    let tick = match_state
        .sim_runtime
        .presentation()
        .map_or(0, |rt| rt.simulation.session().tick());
    sidebar_action_visits(
        sidebar_action,
        match_state.session_mode().is_single_player(),
        tick,
        match_state
            .match_presentation
            .sidebar_gadget_state
            .last_sim_tick,
    )
}

/// Roll queued strip scrolls, then poll the retained strip links before the
/// current frame's tab FlashAI.
/// Original StripAI 0x006A8D07..0x006A8D7A admits completed Building objects
/// on tabs0/1, independently of which tab is selected. The Defense predicate
/// also includes ready Supers; the shared gadget owner supplies phase/timing.
/// The scroll roll moves one row per Sidebar Action visit
/// ([`sidebar_action_visits`]); the view is rebuilt from the moved row by the
/// pass's republish that follows.
pub(crate) fn update_sidebar_gadget_state(
    frontend: &FrontendState,
    match_state: &mut MatchState,
    visits: u64,
) {
    // TabClass::ReceiveNotice @ 0x006D1770 (the planning notice sink) turns the
    // PlanningMode button on for Planning__StartMode's 0x11C7 and off for
    // EndMode's 0x11C8, so its look follows the PlanMgr owner's mode every
    // frame, whatever ended or started it (hotkey, committed plan, focus loss).
    let planning = match_state.input.planning.active();
    match_state
        .match_presentation
        .sidebar_gadget_state
        .command_pressed[9] = planning;
    // A ready tab's completed object is the frame's (`presentation::frame`).
    match_state.sim_runtime.ensure_frame();
    let Some(frame) = match_state.sim_runtime.frame() else {
        return;
    };
    let Some(sim) = match_state
        .sim_runtime
        .presentation()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(rules) = crate::app::state::active_rules(frontend, match_state) else {
        return;
    };
    let Some(owner) = preferred_local_owner_name(match_state) else {
        return;
    };
    let owner_iid = sim.interner().get(&owner).unwrap_or_default();
    let superweapon_views =
        crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid);
    let ready = sim.interner().get(&owner).map_or([false; 2], |owner| {
        match_state
            .match_presentation
            .sidebar_projection
            .model
            .ready_building_tabs(sim, frame, owner)
    });
    let (tick, frame) = (sim.session().tick(), sim.session().binary_frame() as i32);
    let presentation = &mut match_state.match_presentation;
    // StripClass::AI scrolls before its tab poll, once per Sidebar Action.
    if let Some(rows) = presentation
        .sidebar_projection
        .view()
        .map(|view| view.layout.side2_tile_count)
    {
        presentation
            .sidebar_projection
            .model
            .step_pending_scroll(rows, visits);
    }
    update_tab_flashes_for_owner(
        &mut presentation.sidebar_gadget_state,
        ready,
        &superweapon_views,
        visits,
        frame,
    );
    presentation.sidebar_gadget_state.last_sim_tick = tick;
}

/// StripClass::AI @ 0x006A8B30 calls SuperClass::ShouldFlashTab @
/// 0x006CE1A0 for each Defense-strip Super and stops only when no entry passes.
/// This is the single Rust update step combining that result with ready
/// buildings before the shared SidebarGadgetState advances its flash phase.
/// The house's active views that keep a cameo
/// ([`crate::sim::superweapon::SuperWeaponView::ares_show_cameo`]) are exactly
/// the Defense strip's Super candidates, so polling them equals polling the
/// strip below its 75-entry cap.
fn update_tab_flashes_for_owner(
    gadgets: &mut SidebarGadgetState,
    ready_building_tabs: [bool; 2],
    superweapon_views: &[crate::sim::superweapon::SuperWeaponView],
    visits: u64,
    frame: i32,
) {
    let has_flash_eligible_super = superweapon_views
        .iter()
        .any(|sw| sw.ares_show_cameo && sw.should_flash_tab);
    gadgets.update_tab_flashes(
        visits,
        frame,
        [
            ready_building_tabs[0],
            ready_building_tabs[1] || has_flash_eligible_super,
            false,
            false,
        ],
    );
}

#[cfg(all(test, tests_app_presentation))]
#[path = "sidebar_gadgets_tests.rs"]
mod tests;
