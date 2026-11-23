//! In-game input handling. This file owns mouse dispatch (tactical clicks,
//! band-box release, minimap, cursor motion) and the selection policies its
//! callers share; the rest lives in child modules: `sidebar` (wheel, cameo and
//! sidebar actions), `hotkey_dispatch` (keyboard commands), `selection` (the
//! ordered selection and its mutations), `unit_orders` (Stop, Guard, Scatter,
//! Deploy), `control_groups` (digit and team keys) and `dev_tools`.
//!
//! Context-sensitive order resolution (click → command) lives in
//! `input::context_order`. This file handles raw input dispatch and UI state.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use winit::event::{ElementState, MouseButton};

use crate::app::AppState;
use crate::app::input::commands::{place_ready_building_at_cell, preferred_local_owner_name};
use crate::app::input::context_order::{ContextClick, ContextOrderTarget, try_queue_context_order};
use crate::app::input::entity_pick::{
    SelectionMutation, compute_box_selection_snapshot_with_playfield,
    compute_click_selection_snapshot_with_playfield,
    compute_double_click_type_select_with_playfield,
    compute_type_select_box_mutation_with_playfield,
    compute_type_select_click_mutation_with_playfield, map_entity_creation_order,
    pick_entity_at_point,
};
use crate::app::input::precedence::{ModalPage, modal_page};
use crate::app::types::OrderMode;
#[cfg(test)]
use crate::sim::command::Command;
use crate::sim::selection::SelectAction;

#[cfg(all(test, tests_app_input_d_z))]
#[path = "area_guard_tests.rs"]
mod area_guard_tests;

#[cfg(all(test, tests_app_input_d_z))]
#[path = "player_scatter_tests.rs"]
mod player_scatter_tests;

#[cfg(all(test, tests_app_input_d_z))]
#[path = "mcv_group_tests.rs"]
mod mcv_group_tests;

mod control_groups;
mod dev_tools;
mod hotkey_dispatch;
mod selection;
mod sidebar;
mod unit_orders;

// Sibling input modules the parts reach as `super::<module>`.
use super::{build_repeat, commands, context_order, superweapon_cameo, transport_orders};

pub(crate) use control_groups::GroupPressAction;
use control_groups::{GroupPress, handle_control_group_command, queue_control_group_assignment};
#[cfg(all(test, tests_app_input_d_z))]
use control_groups::{
    TeamSelection, command_bar_team_press_action, control_group_action_for_input,
    control_group_press_action, live_group_members, schedule_control_group_assignment,
    team_selection, team_walk_members,
};
#[cfg(feature = "dev-overlays")]
use dev_tools::handle_dev_hotkey_pressed;
pub(crate) use dev_tools::report_black_cell_causes;
#[cfg(feature = "debug-tools")]
pub(crate) use dev_tools::{
    cycle_terrain_cost_speed_type, request_debug_frame_step, toggle_cell_grid_overlay,
    toggle_debug_pause, toggle_foot_path_overlay, toggle_heightmap_overlay,
    toggle_pathgrid_overlay,
};
use hotkey_dispatch::{centre_placement_on_factory, open_pause_menu, press_super_weapon};
pub(crate) use hotkey_dispatch::{
    dispatch_command_bar, handle_hotkey_pressed, handle_type_select_key_edge, run_window_command,
};
pub(crate) use selection::{
    CommittedSelection, KeyModifiers, is_alt_held, is_ctrl_held, is_shift_held,
    reconcile_selection_order_after_sim, selected_in_order, selected_stable_ids_in_order,
    selection_order_for_world,
};
use selection::{
    apply_selection_action_line_policy, apply_selection_mutation, queue_selection_snapshot_command,
};
#[cfg(all(test, tests_app_input_d_z))]
use selection::{
    apply_selection_action_line_policy_at_frame, insert_selected_id_by_role, logic_selects,
    reconcile_selection_order_for_sim, resolve_selection_mutation, selection_membership_committed,
    selection_voice_recipients,
};
pub(super) use sidebar::local_has_manageable_building;
#[cfg(test)]
pub(super) use {hotkey_dispatch::ready_tab_factory, sidebar::ready_cameo_factory};
#[cfg(test)]
pub(crate) use sidebar::wheel_action;
pub(crate) use sidebar::{
    WheelAction, apply_sidebar_action, local_queue_view, push_local_eva, sidebar_scroll_command,
    wheel_notch,
};
#[cfg(all(test, tests_app_input_d_z))]
use unit_orders::deploy_key_command;
#[cfg(all(test, tests_app_input_d_z))]
use unit_orders::{
    deploy_key_commands, queue_area_guard_orders, queue_scatter_orders, queue_stop_orders,
};
use unit_orders::{
    queue_deploy_undeploy_for_selected, queue_guard_for_selected, queue_scatter_for_selected,
    queue_stop_for_selected,
};

/// Handle mouse button press/release for selection and move commands.
///
/// The in-game gadget walk runs FIRST (study G22/A8): chrome buttons + cameos
/// fire/consume there; the full-tactical catcher and the minimap region decide
/// WHICH body runs (the regions are sticky, so a drag stays bound to its region
/// across the sidebar boundary). A `NotConsumed` click hits no live gadget — only
/// the legacy dev/pause/producer press path runs; empty-sidebar / off-window
/// clicks do nothing (gamemd's sidebar-body gadget swallows them, A6). The middle
/// button has no tactical behavior; with VERA's wheel zoom on, its press over
/// the battlefield resets the view distance. Right-press is owned by the tactical catcher
/// (viewport-only), so right-clicking dead sidebar chrome no longer deselects —
/// matching gamemd.
pub(crate) fn handle_mouse_input(
    state: &mut AppState,
    button: MouseButton,
    btn_state: ElementState,
) {
    // The window path routes these pages in its own handler from the same
    // decision (`input::precedence`); captures enter here directly.
    if modal_page(state) == Some(ModalPage::NetworkFailure) {
        crate::app::match_runtime::network_failure::mouse(state, button, btn_state.is_pressed());
        return;
    }
    handle_mouse_input_inner(state, button, btn_state);
    super::commands::reconcile_paid_super_weapon_cursor(state);
}

fn handle_mouse_input_inner(state: &mut AppState, button: MouseButton, btn_state: ElementState) {
    use crate::app::input::gadget_input::GadgetConsume;
    let pressed = btn_state.is_pressed();
    state
        .match_state
        .input
        .tactical_mouse
        .record_physical_button(button, pressed);
    match modal_page(state) {
        // 0xEA is pushed over the game and any open page.
        Some(ModalPage::NetworkWait) => {
            if !pressed {
                state.match_state.input.cancel_tactical_drag();
                state
                    .match_state
                    .match_presentation
                    .in_game_gadgets
                    .cancel_pointer_capture();
            }
            crate::app::input::connection::mouse(state, button, pressed);
            return;
        }
        Some(ModalPage::Keyboard) => {
            crate::app::input::keyboard::mouse(state, button, pressed);
            return;
        }
        // The shell error card is main-menu only.
        Some(ModalPage::ShellError | ModalPage::NetworkFailure | ModalPage::ShellMessageBox)
        | None => {}
    }
    // VERA: in the layout edit mode the mouse moves the interface's parts and
    // reaches nothing else in the match.
    if crate::app::input::layout_edit::active(state) && !state.match_state.paused() {
        crate::app::input::layout_edit::mouse(state, button, pressed);
        return;
    }
    // Paused in-game Options overlay owns the mouse: route press/release/checkbox/
    // Back here and CONSUME the click so it never reaches the tactical viewport or
    // a gadget (no unit orders behind the overlay). KD-6.
    if state.match_state.paused() {
        // VERA-internal (gamemd has no pause overlay; gamemd equivalent
        // UNCHECKED): a release that arrives while the overlay owns the mouse
        // never reaches the tactical body, so the whole gesture is dropped
        // here — not just the capture byte. Leaving a minimap press or the
        // band drag active would freeze edge auto-scroll, swallow every cursor
        // move, and keep a stale gadget sticky capture eating clicks for the
        // rest of the match. `enter_in_game_menu_state` already tore the
        // gesture down on the way into the modal; this catches releases that
        // arrive afterwards (debug pause and any path that skips that owner).
        if !pressed {
            state.match_state.input.cancel_tactical_drag();
            state
                .match_state
                .match_presentation
                .in_game_gadgets
                .cancel_pointer_capture();
        }
        match state.match_state.match_presentation.in_game_menu {
            crate::ui::pause_menu::InGameMenuState::Page => {
                if button == MouseButton::Left {
                    crate::app::shell_wol::page::mouse(state, pressed);
                }
            }
            crate::ui::pause_menu::InGameMenuState::SavedGame(_) => {
                if button == MouseButton::Left {
                    crate::app::App::saved_game_mouse(state, pressed);
                }
            }
            menu if button == MouseButton::Left => {
                crate::app::input::dialog_page::in_game_pointer(state, menu, Some(pressed));
            }
            _ => {}
        }
        return;
    }
    // ScrollHandleMouse6930A0 refuses ordinary pointer messages while the
    // Scenario input lock is active; capture cancellation already ran in46.
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    // The stock tactical handler has no middle-button case. VERA's optional
    // wheel zoom takes a press over the battlefield as its reset.
    if button == MouseButton::Middle {
        if pressed {
            crate::app::input::camera::middle_press_resets_view_distance(state);
        }
        return;
    }
    // MessageList::Input 0x005D4510 ends an open chat edit on the right
    // button, press or release, and leaves the click to the game.
    if button == MouseButton::Right {
        crate::app::input::chat::right_button(state);
    }
    if super::chat::skirmish_spawn_mouse(state, button, pressed) {
        return;
    }
    #[cfg(feature = "phobos")]
    if crate::app::input::sw_sidebar::mouse(state, button, pressed) {
        return;
    }
    match crate::app::input::gadget_input::handle_mouse_button_event(state, button, pressed) {
        GadgetConsume::Tactical => tactical_mouse(state, button, btn_state),
        GadgetConsume::Minimap => minimap_mouse(state, button, btn_state),
        // Consumed (chrome/cameo/control button) → handled by the gadget.
        // NotConsumed → the click hit no live gadget; nothing to do (every
        // in-game surface is now on the retained list — R7 complete).
        GadgetConsume::Consumed | GadgetConsume::NotConsumed => {}
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClickActionRoute<T> {
    ContextOrder(T),
    Selection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionVoicePolicy {
    FirstAdded,
    /// The team walks clear the latch after the first Select they *call*,
    /// whatever it returns (Recall `0x00731344`/`0x00731369`, AddTeam
    /// `0x0073156C`/`0x00731572`, CenterCamera `0x00731481`/`0x00731487`),
    /// so a refused first member leaves the walk silent.
    FirstAttempted,
    EveryAdded,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionActionLinePolicy {
    Start,
    Preserve,
}

const ORDINARY_SELECTION_VOICE_POLICY: SelectionVoicePolicy = SelectionVoicePolicy::FirstAdded;
const TYPE_SELECT_TAP_VOICE_POLICY: SelectionVoicePolicy = SelectionVoicePolicy::EveryAdded;
const TEAM_SELECTION_VOICE_POLICY: SelectionVoicePolicy = SelectionVoicePolicy::FirstAttempted;
const HELD_TYPE_SELECT_VOICE_POLICY: SelectionVoicePolicy = SelectionVoicePolicy::Suppressed;
const ORDINARY_SELECTION_ACTION_LINE_POLICY: SelectionActionLinePolicy =
    SelectionActionLinePolicy::Start;
const TYPE_SELECT_TAP_ACTION_LINE_POLICY: SelectionActionLinePolicy =
    SelectionActionLinePolicy::Preserve;

/// Ask the context action before applying selection/TypeSelect. Raw Shift
/// cannot bypass WhatAction: Ctrl+Shift cancels the selection modifier, and
/// Shift over bare ground is still Move (Techno700706..70071B/700B03).
/// The context owner returns a selection click for Shift over an object.
fn route_click_action_before_type_select<T>(
    action: SelectAction,
    context_orders: bool,
    resolve_context_order: impl FnOnce(f32, f32) -> Option<T>,
) -> ClickActionRoute<T> {
    if !context_orders {
        return ClickActionRoute::Selection;
    }
    let SelectAction::Click(sx, sy) = action else {
        return ClickActionRoute::Selection;
    };
    resolve_context_order(sx, sy)
        .map(ClickActionRoute::ContextOrder)
        .unwrap_or(ClickActionRoute::Selection)
}

/// VERA-only input mapping. Explicit attack-move and force-command chords
/// still accept a left-click target; an ordinary left click only selects.
fn left_click_can_issue_orders(state: &AppState) -> bool {
    !state.persistence.options_profile.right_click_orders
        || state.match_state.input.queued_order_mode == OrderMode::AttackMove
        || is_ctrl_held(state)
        || is_alt_held(state)
}

/// Tactical-viewport mouse body (routed here when the full-tactical ClickRegion
/// consumes the edge — i.e. a click in the play area, or a captured drag/release
/// that started there). Logic is the legacy handler's tactical path, unchanged;
/// the minimap-drag-end and minimap-begin checks moved to `minimap_mouse`. The
/// stock tactical path has no middle-button case, so the Rust dispatcher ignores it.
pub(crate) fn tactical_mouse(state: &mut AppState, button: MouseButton, btn_state: ElementState) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    match button {
        MouseButton::Left => {
            if btn_state.is_pressed() {
                if crate::app::input::planning_mode::refuse_press_at_cursor(state) {
                    return;
                }
                // PlanMgr's press 0x0063A5A0 also takes a press inside the
                // hover node's rectangle (AC4CCC), arming AC4C38 and returning
                // 1 (0x0063A736), so LeftDown never arms a band. RESIDUAL:
                // natively it skips the capture too; here the capture is kept
                // so the release still reaches `connect_at_cursor`.
                let press_on_hover_node =
                    crate::app::input::planning_mode::hover_node(state).is_some();
                // gamemd takes the mouse capture on the press edge whether or
                // not the band drag arms: the modal gates live inside the
                // drag-arm helper 0x004AC310, not around the capture. The
                // capture is what freezes edge auto-scroll for the gesture.
                //
                // It refuses to arm at all when the shared capture byte is
                // already held, so a left press landing mid right-drag records
                // only the physical button. See `TacticalMouseState`.
                if !state.match_state.input.tactical_mouse.begin_left_press() {
                    return;
                }
                // LeftDown 0x004AC310 has no planning test, so a drag off the
                // hover node in Planning Mode makes a band box.
                if press_on_hover_node
                    || state.match_state.input.targeting_mode.is_some()
                    || state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .repair_mode_on
                    || state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .sell_mode_on
                    // LeftDown 0x004AC310 also refuses the band in power mode
                    // (+0x11B2, 0x004AC31A).
                    || state
                        .match_state
                        .match_presentation
                        .sidebar_gadget_state
                        .power_mode_on
                    // The held-drag 4AC380 never starts a band in beacon
                    // mode (4AC3A3). RESIDUAL (kept by the maintainer): past
                    // 4 px it raises the band flag (0x004AC496..0x004AC4B7), so
                    // the release takes the band path over the last band's
                    // rectangle and places no beacon when that stale rectangle
                    // catches an object.
                    || crate::app::input::beacon::mode_active(state)
                    // VERA ferry: the armed click names the drop spot.
                    || state.match_state.input.ferry.armed()
                {
                    return; // suppress selection drag while a targeting / repair / sell / power / beacon mode is active
                }
                // VERA formation line: with its key held and a group
                // selected, the drag draws a line and no band box.
                if begin_formation_line(state) {
                    return;
                }
                state.match_state.input.selection_state.begin_drag(
                    state.match_state.input.cursor_x,
                    state.match_state.input.cursor_y,
                );
            } else {
                // Case 0x202 gates the whole release on the same shared capture
                // byte and does NOT test which button set it. With the byte
                // clear it exits having done nothing; with it set it runs
                // BandBox_LeftUp and drops the capture, whichever button was
                // holding it. See `TacticalMouseState::end_left_press`.
                // VERA formation line: the drag ends with the button,
                // whether or not the capture below is still held.
                let formation = state.match_state.input.formation_line.finish();
                if !state.match_state.input.tactical_mouse.end_left_press() {
                    return;
                }
                // A drawn line or block orders the group; a line too short
                // to be one is the ordinary click below.
                if let Some(path) = formation
                    && path.is_order()
                    && crate::app::input::context_order::queue_formation_line(state, &path)
                {
                    // As BandBox_LeftUp ends the attack-move mode (`0x00731CF0`).
                    state.match_state.input.queued_order_mode = OrderMode::Move;
                    return;
                }
                band_box_left_up(state, LeftUp::Tactical);
            }
        }
        MouseButton::Right => {
            if btn_state.is_pressed() {
                // PlanMgr's right press 0x0063AAC0 runs first (0x006932D0).
                crate::app::input::planning_mode::right_press(state);
                // Otherwise the native right press has no game effect: it
                // records the pan anchor and takes the capture, and only does
                // that when no other button already holds it. Everything the
                // player sees happens on the release edge.
                if state.match_state.input.tactical_mouse.press_may_arm() {
                    state.match_state.input.tactical_mouse.begin_right_drag((
                        state.match_state.input.cursor_x,
                        state.match_state.input.cursor_y,
                    ));
                }
            } else {
                // 0x0069338A: PlanMgr's release 0x0063AB00 precedes the
                // capture test.
                crate::app::input::planning_mode::right_release(state);
                if let Some(is_click) = state.match_state.input.tactical_mouse.end_right_press() {
                    // Dispatch a click only when the drag threshold was never
                    // crossed. A right drag that panned the map ends silently;
                    // the selection survives it.
                    if is_click {
                        handle_tactical_right_click(state);
                    }
                    // The native release tears the band rectangle down too.
                    state.match_state.input.selection_state.cancel_drag();
                    // VERA formation line: and a line being drawn.
                    state.match_state.input.formation_line.cancel();
                }
            }
        }
        _ => {}
    }
}

/// VERA formation line (`input::formation_line`): start one at the cursor
/// when its key is held and at least two units that take a point are
/// selected ([`crate::app::input::context_order::formation_unit_count`]). Fewer leave
/// the press to the band box, so the key alone never blocks selecting.
fn begin_formation_line(state: &mut AppState) -> bool {
    if !state.match_state.input.formation_line.key_held() {
        return false;
    }
    let selected = crate::app::input::context_order::formation_unit_count(state);
    if selected < 2 {
        return false;
    }
    let world = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    // Shift held as well: the drag is a block's front row.
    let block = state.match_state.input.hotkey_modifiers.shift_key();
    state
        .match_state
        .input
        .formation_line
        .begin(world, selected, block);
    true
}

/// What reaches `DisplayClass::BandBox_LeftUp @ 0x004AB9B0`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LeftUp {
    /// A tactical left release that held the capture, at the cursor. The
    /// release resolves its own pick and action (`0x00693271..0x0069328B`).
    Tactical,
    /// The radar's release (`0x00653D58`): `GetObjectAtRadarPixel`'s pick
    /// and, when the action the radar kept is a superweapon's, that action
    /// ([`crate::app::input::radar`]).
    Radar {
        target: crate::render::radar::NativeRadarInputTarget,
        super_weapon: Option<crate::app::input::commands::PlaceAction<'static>>,
        /// VERA minimap beacons: the radar answered the beacon action
        /// ([`crate::app::input::radar::with_minimap_beacon`]).
        beacon: bool,
    },
}

/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0`. `0x00731CF0` ends the
/// attack-move mode (`0x00B0FE58`) before each of its returns
/// (`0x004ABC67`, `0x004ABC85`, `0x004AC294`), whatever the release did.
pub(crate) fn band_box_left_up(state: &mut AppState, click: LeftUp) {
    left_up_branches(state, click);
    state.match_state.input.queued_order_mode = OrderMode::Move;
}

fn left_up_branches(state: &mut AppState, click: LeftUp) {
    // A pending placement takes the clicked Cell, whatever the action
    // (`0x004ABA16..0x004ABC67`).
    if let Some(type_id) = state.armed_building_type().map(str::to_owned) {
        let cell = match click {
            LeftUp::Tactical => crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
                state,
                state.match_state.input.cursor_x,
                state.match_state.input.cursor_y,
            ),
            LeftUp::Radar { target, .. } => target.cell,
        };
        place_ready_building_at_cell(state, &type_id, cell);
        return;
    }
    let LeftUp::Radar {
        target,
        super_weapon,
        beacon,
    } = click
    else {
        tactical_left_up(state);
        return;
    };
    // VERA minimap beacons: the beacon action places at the pick, as the
    // tactical release's 0x3C arm does at its Cell (0x004AC033).
    if beacon {
        crate::app::input::beacon::place_at_cell(state, target.cell);
        return;
    }
    // The selection's dispatch, each object asking its own What_Action
    // (`Selection__DispatchMultiUnitOrder @ 0x004AE750`, at `0x004ABF94`), a
    // superweapon action included; for one SPECIAL_PLACE follows
    // (`0x004AC20C`). The radar never hands over a Select action, so a click
    // the dispatch leaves to selection selects nothing.
    //
    // RESIDUAL: BandBox_LeftUp drops a picked object whose VisualCharacter
    // is Hidden (`0x004AB9DF..0x004AB9EF`); the radar's pick is not tested.
    // Trigger: a radar dot of an object hidden from the player. Effect: the
    // order may target it. Frequency: rare.
    // RESIDUAL: the dispatch hands Cell actions other than 1, 2 and 0x33
    // the second Cell (-1,-1) (`0x004AE8FC`), which ClickedMission reads
    // only into a Planning Mode record (`0x006FFC30`); VERA has no second
    // Cell. Trigger: a kept superweapon, or a Cell Harvest, Guard or Attack,
    // in Planning Mode. Effect: that record's third target. Frequency: rare.
    try_queue_context_order(state, ContextOrderTarget::Radar(target), true);
    if let Some(action) = super_weapon
        && let Some(armed) = state.armed_super_weapon_type().map(str::to_owned)
    {
        crate::app::input::commands::launch_super_weapon_at_cell(
            state,
            &armed,
            action,
            target.cell,
        );
    }
}

/// The tactical release past the placement branch: the superweapon's
/// SPECIAL_PLACE, a planning node's connection, the band box and the click.
fn tactical_left_up(state: &mut AppState) {
    // DetermineAction answers the armed superweapon's action only with
    // nothing selected (`0x00692610..0x00692673`); with a selection it is the
    // best object's What_Action, which the click below dispatches. With
    // nothing selected the dispatch has no receiver, so SPECIAL_PLACE alone
    // remains.
    let selection_empty = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    )
    .is_empty();
    match state.match_state.input.tactical_pointer_mode(
        &state.match_state.match_presentation.sidebar_gadget_state,
        selection_empty,
    ) {
        super::state::TacticalPointerMode::SuperWeapon(section) => {
            let section = section.to_owned();
            crate::app::input::commands::launch_super_weapon_at_cursor(state, &section);
            return;
        }
        super::state::TacticalPointerMode::RepairSell => {
            crate::app::input::commands::try_repair_sell_mode_click(state);
            return;
        }
        super::state::TacticalPointerMode::Power => {
            crate::app::input::power_mode::click(state);
            return;
        }
        super::state::TacticalPointerMode::Ordinary => {}
    }
    // VERA ferry: the armed click sets the selected transports' route, a
    // left-button tool in both control schemes like the ones around it.
    if crate::app::input::ferry::drop_click(state) {
        return;
    }
    // Planning-node editing stays a left-button tool in both control schemes,
    // like explicit placement and targeting above; ordinary planned missions
    // use the configured order button through the context owner below.
    if crate::app::input::planning_mode::connect_at_cursor(state) {
        return;
    }
    // Past PlanMgr's release (0x00693225), DetermineAction answers 0x3C in
    // beacon mode with nothing selected (0x006929AF); the release places
    // the beacon (0x004AC033).
    if selection_empty && crate::app::input::beacon::mode_active(state) {
        let cell = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        );
        crate::app::input::beacon::place_at_cell(state, cell);
        return;
    }
    // Only an active band box owns a clamped tactical endpoint.
    // A pending press is still an ordinary click at the actual
    // release point, including after sticky capture routing.
    let release_point = if state.match_state.input.selection_state.is_band_box_active() {
        clamp_tactical_drag_endpoint(
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
            crate::app::input::camera::tactical_viewport_px(state),
        )
    } else {
        (
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        )
    };
    let mut action: SelectAction = state
        .match_state
        .input
        .selection_state
        .end_drag(release_point.0, release_point.1);
    let band_release = matches!(action, SelectAction::BoxSelect(..));
    if band_release {
        // BandBox_LeftUp4ABCF0 -> StartTimer70D150 precedes the
        // empty-catch bail and click fallthrough. The old
        // selection therefore flashes even when the box is empty.
        // Evidence: tools/procedural_drawing_oracle/action_lines.
        apply_selection_action_line_policy(state, ORDINARY_SELECTION_ACTION_LINE_POLICY);
    }
    // BandBox_LeftUp 0x004AB9B0 does not early-return when nothing
    // was armed: with the band flag clear it falls straight through
    // to the action dispatch. So a release whose press never armed
    // a drag -- because the shared capture byte was already held, or
    // because a cursor mode swallowed the press -- is still an
    // ordinary click at its own release point, not a no-op.
    if matches!(action, SelectAction::None) {
        action = SelectAction::Click(release_point.0, release_point.1);
    }
    let shift = is_shift_held(state);
    // A band box that caught no drawn object leaves the selection
    // exactly as it was, and the release is handled as an ordinary
    // click at the release point — the native release only clears
    // when something was inside the rectangle.
    //
    // The whole empty/clear/fall-through block sits inside the
    // native "shift is not held" arm, and the fall-through flag is
    // the only thing that lets control reach the click/action path.
    // So a shift drag that catches nothing does nothing at all: it
    // must not walk the army to the release point.
    let mut band_preflight_order = None;
    if let SelectAction::BoxSelect(min_x, min_y, max_x, max_y) = action
        && !shift
    {
        let order =
            crate::app::presentation::instances::tactical_band_preflight_entity_encounter_order(
                state,
            );
        if band_caught_drawn_object(state, &order, min_x, min_y, max_x, max_y) {
            band_preflight_order = Some(order);
        } else {
            action = SelectAction::Click(release_point.0, release_point.1);
        }
    }
    // A click over a beacon with nothing selected answers 0x3D
    // (0x006927C5); the release selects the beacon (0x004ABE88).
    if let SelectAction::Click(sx, sy) = action
        && selection_empty
    {
        let cell = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, sx, sy);
        if crate::app::input::beacon::beacon_at_cell(state, cell) {
            crate::app::input::beacon::select_at_cell(state, cell);
            return;
        }
    }
    // TypeSelect modifies only actions that already resolved as
    // selection/toggle. Ground move, attack, and every other
    // context action keep their ordinary priority while T is held.
    let type_select_held = state.match_state.input.type_select.held();
    // A resolved context dispatch consumes the click even when its receiver
    // admits no command. A release that answered Select or 0 takes the pick
    // without Shift's toggle.
    let pick_toggles = match route_click_action_before_type_select(
        action,
        left_click_can_issue_orders(state),
        |sx, sy| {
            Some(try_queue_context_order(
                state,
                ContextOrderTarget::ScreenPoint(sx, sy),
                true,
            ))
        },
    ) {
        ClickActionRoute::ContextOrder(ContextClick::Consumed) => return,
        ClickActionRoute::ContextOrder(ContextClick::Select) => false,
        ClickActionRoute::ContextOrder(ContextClick::Pick) | ClickActionRoute::Selection => shift,
    };
    // VERA right-click orders: a second quick click on the same battlefield
    // point selects every on-screen object of the clicked object's type, the
    // modern-RTS shortcut for the T key's screen pass. It rides on the option
    // and on an ordinary selection click only: a held T, a modifier chord or
    // an armed attack-move keeps the click it already has and ends the chain,
    // so it never pairs with the next plain click. The chain records every
    // ordinary selection click, so the next one can pair with it.
    if let SelectAction::Click(sx, sy) = action {
        let ordinary = !type_select_held
            && !shift
            && !is_ctrl_held(state)
            && !is_alt_held(state)
            && state.match_state.input.queued_order_mode == OrderMode::Move;
        let window_point = state
            .tactical_zoom()
            .map_or((sx, sy), |zoom| zoom.source_to_window(sx, sy));
        let admitted = ordinary && state.persistence.options_profile.right_click_orders;
        let double_click = state.match_state.input.note_tactical_selection_click(
            admitted,
            crate::util::time::Instant::now(),
            (window_point.0.round() as i32, window_point.1.round() as i32),
        );
        if double_click && let Some(mutation) = double_click_type_select(state, sx, sy) {
            apply_selection_mutation(state, mutation, ORDINARY_SELECTION_VOICE_POLICY);
            apply_selection_action_line_policy(state, TYPE_SELECT_TAP_ACTION_LINE_POLICY);
            return;
        }
    }
    let mut queued_selection: Option<SelectionMutation> = None;
    let mut held_type_select_batch = false;
    if let Some(sim) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
    {
        let screen_order =
            crate::app::presentation::instances::tactical_screen_entity_encounter_order(state);
        let current_selection = selected_stable_ids_in_order(
            state
                .match_state
                .sim_runtime
                .runtime()
                .map(|rt| &rt.simulation),
            state.rules(),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        let map_order = map_entity_creation_order(sim.entities());
        let held_type_select = type_select_held;
        let scope_order = if state.match_state.input.type_select.across_map {
            map_order.as_slice()
        } else {
            screen_order.as_slice()
        };
        match action {
            SelectAction::Click(sx, sy) => {
                // Native TacticalPick6DA380 reads the Display
                // registrations in draw order after the projected click
                // gate. The ScreenSelection list belongs to band/type
                // scope, not this pointer pick.
                let click_order =
                    crate::app::presentation::instances::tactical_entity_encounter_order(
                        sim,
                        state.rules(),
                    );
                let world_x: f32 =
                    sx / state.match_state.input.zoom_level + state.match_state.input.camera_x;
                let world_y: f32 =
                    sy / state.match_state.input.zoom_level + state.match_state.input.camera_y;
                let fog_ref = if state.match_state.sandbox_full_visibility {
                    None
                } else {
                    Some(sim.fog())
                };
                if held_type_select {
                    let picked = pick_entity_at_point(
                        crate::app::input::entity_pick::presented(state, sim),
                        &click_order,
                        fog_ref,
                        preferred_local_owner_name(state).as_deref(),
                        world_x,
                        world_y,
                        Some(sim.interner()),
                        sim.resolved_terrain(),
                    );
                    queued_selection = if let Some(clicked_id) = picked {
                        Some(compute_type_select_click_mutation_with_playfield(
                            sim,
                            scope_order,
                            &current_selection,
                            clicked_id,
                            pick_toggles,
                            preferred_local_owner_name(state).as_deref(),
                            state.rules(),
                            Some(sim.interner()),
                            sim.playfield_bounds().is_some(),
                        ))
                    } else {
                        compute_click_selection_snapshot_with_playfield(
                            crate::app::input::entity_pick::presented(state, sim),
                            &click_order,
                            &current_selection,
                            fog_ref,
                            preferred_local_owner_name(state).as_deref(),
                            world_x,
                            world_y,
                            pick_toggles,
                            state.rules(),
                            Some(sim.interner()),
                            sim.playfield_bounds().is_some(),
                            sim.resolved_terrain(),
                        )
                    };
                    held_type_select_batch = true;
                } else {
                    queued_selection = compute_click_selection_snapshot_with_playfield(
                        crate::app::input::entity_pick::presented(state, sim),
                        &click_order,
                        &current_selection,
                        fog_ref,
                        preferred_local_owner_name(state).as_deref(),
                        world_x,
                        world_y,
                        pick_toggles,
                        state.rules(),
                        Some(sim.interner()),
                        sim.playfield_bounds().is_some(),
                        sim.resolved_terrain(),
                    );
                }
            }
            SelectAction::BoxSelect(min_x, min_y, max_x, max_y) => {
                let fog_ref = if state.match_state.sandbox_full_visibility {
                    None
                } else {
                    Some((sim.fog(), sim.house_alliances().as_map()))
                };
                let z = state.match_state.input.zoom_level;
                let (min_x, min_y, max_x, max_y) = (
                    min_x / z + state.match_state.input.camera_x,
                    min_y / z + state.match_state.input.camera_y,
                    max_x / z + state.match_state.input.camera_x,
                    max_y / z + state.match_state.input.camera_y,
                );
                if held_type_select {
                    queued_selection = Some(compute_type_select_box_mutation_with_playfield(
                        crate::app::input::entity_pick::presented(state, sim),
                        &screen_order,
                        scope_order,
                        &current_selection,
                        fog_ref.map(|(fog, _)| fog),
                        preferred_local_owner_name(state).as_deref(),
                        min_x,
                        min_y,
                        max_x,
                        max_y,
                        shift,
                        state.rules(),
                        Some(sim.interner()),
                        sim.playfield_bounds().is_some(),
                    ));
                    held_type_select_batch = true;
                } else {
                    let preflight_order = band_preflight_order
                        .as_deref()
                        .unwrap_or(screen_order.as_slice());
                    queued_selection = compute_box_selection_snapshot_with_playfield(
                        crate::app::input::entity_pick::presented(state, sim),
                        preflight_order,
                        &screen_order,
                        &current_selection,
                        fog_ref,
                        preferred_local_owner_name(state).as_deref(),
                        min_x,
                        min_y,
                        max_x,
                        max_y,
                        shift,
                        state.rules(),
                        Some(sim.houses()),
                        Some(sim.interner()),
                        sim.playfield_bounds().is_some(),
                    );
                }
            }
            SelectAction::None => {}
        }
    }
    if let Some(mutation) = queued_selection {
        if held_type_select_batch {
            if mutation.select.is_empty() {
                apply_selection_mutation(state, mutation, ORDINARY_SELECTION_VOICE_POLICY);
            } else {
                let prior = state.match_state.input.selection_voice_enabled;
                state.match_state.input.selection_voice_enabled = false;
                apply_selection_mutation(state, mutation, HELD_TYPE_SELECT_VOICE_POLICY);
                state.match_state.input.selection_voice_enabled = prior;
            }
        } else {
            apply_selection_mutation(state, mutation, ORDINARY_SELECTION_VOICE_POLICY);
        }
        // Click-select4ABE83 starts separately. A band release
        // already started before its empty-catch decision above.
        if !band_release {
            apply_selection_action_line_policy(state, ORDINARY_SELECTION_ACTION_LINE_POLICY);
        }
    }
}

/// The selection a right-click-orders double-click commits, or `None` when the
/// click found no locally controlled object (ground, an enemy, a refused
/// object), which leaves the ordinary click to decide. The screen list is the
/// T key's screen pass; there is no map-wide fallback.
fn double_click_type_select(state: &AppState, sx: f32, sy: f32) -> Option<SelectionMutation> {
    let sim = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)?;
    // TypeSelect's defeat guard `0x00732961` (`A8B538`): a defeated local
    // player's shortcut leaves the plain click in charge.
    if !selection_navigation::selection_navigation_allowed(sim) {
        return None;
    }
    let click_order =
        crate::app::presentation::instances::tactical_entity_encounter_order(sim, state.rules());
    let screen_order =
        crate::app::presentation::instances::tactical_screen_entity_encounter_order(state);
    let world_x = sx / state.match_state.input.zoom_level + state.match_state.input.camera_x;
    let world_y = sy / state.match_state.input.zoom_level + state.match_state.input.camera_y;
    let fog = (!state.match_state.sandbox_full_visibility)
        .then_some((sim.fog(), sim.house_alliances().as_map()));
    compute_double_click_type_select_with_playfield(
        crate::app::input::entity_pick::presented(state, sim),
        &click_order,
        &screen_order,
        fog,
        preferred_local_owner_name(state).as_deref(),
        world_x,
        world_y,
        state.rules(),
        Some(sim.interner()),
        sim.playfield_bounds().is_some(),
        sim.resolved_terrain(),
    )
}

/// Cancel exactly one armed cursor mode before right-click orders or deselection.
///
/// Display4AAD30's represented cursor rungs belong to MatchInputState.
/// This existing selection owner handles its final UnselectAll48DC90 rung.
fn cancel_armed_pointer_mode(state: &mut AppState) -> bool {
    if state
        .match_state
        .input
        .cancel_pointer_mode(&mut state.match_state.match_presentation.sidebar_gadget_state)
    {
        return true;
    }
    // RightUp4AAE47: beacon mode is the last rung before Unselect_All.
    if crate::app::input::beacon::mode_active(state) {
        crate::app::input::beacon::set_place_beacon_mode(
            state,
            crate::app::input::beacon::BeaconModeRequest::Off,
        );
        return true;
    }
    // VERA ferry: the armed drop click is one more mode a right click ends.
    state.match_state.input.ferry.disarm()
}

fn handle_tactical_right_click(state: &mut AppState) {
    if cancel_armed_pointer_mode(state) {
        return;
    }
    // VERA-only alternative to Display4AAD30's final Unselect_All rung.
    // Reuse the same context owner as retail left-click orders, including
    // admission, modifiers, audio, feedback and network scheduling. Never
    // fall through to selection on a refused right-click command.
    if state.persistence.options_profile.right_click_orders {
        try_queue_context_order(
            state,
            ContextOrderTarget::ScreenPoint(
                state.match_state.input.cursor_x,
                state.match_state.input.cursor_y,
            ),
            true,
        );
        state.match_state.input.queued_order_mode = OrderMode::Move;
    } else {
        unselect_all(state);
    }
}

/// `Unselect_All_With_Mode_Reset 0x0048DC90`: the selection's clear, whose
/// mutation also clears beacon selection.
pub(crate) fn unselect_all(state: &mut AppState) {
    queue_selection_snapshot_command(state, Vec::new(), false);
}

/// Did the band rectangle cover any drawn object? Screen-space rectangle in, the
/// native "is the box empty" answer out.
fn band_caught_drawn_object(
    state: &AppState,
    encounter_order: &[u64],
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
) -> bool {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
    else {
        return false;
    };
    let z = state.match_state.input.zoom_level;
    crate::app::input::entity_pick::band_rect_contains_drawn_object(
        crate::app::input::entity_pick::presented(state, sim),
        encounter_order,
        min_x / z + state.match_state.input.camera_x,
        min_y / z + state.match_state.input.camera_y,
        max_x / z + state.match_state.input.camera_x,
        max_y / z + state.match_state.input.camera_y,
    )
}

/// Route minimap mouse edges after gadget capture has selected this region.
/// Retail input is `RadarClass::Action @ 0x006539D0`: left begins a context
/// action (or centres the camera without one), right centres the camera.
/// VERA's alternative scheme uses right for orders and ordinary left for the
/// camera. Explicit left-button targeting still uses the context action.
pub(crate) fn minimap_mouse(state: &mut AppState, button: MouseButton, btn_state: ElementState) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let right_click_orders = state.persistence.options_profile.right_click_orders;
    match (button, btn_state.is_pressed()) {
        (MouseButton::Left, true)
            if right_click_orders && state.match_state.input.minimap_press_active() => {}
        (MouseButton::Left, true)
            if left_click_can_issue_orders(state)
                || state.match_state.input.targeting_mode.is_some()
                // VERA minimap beacons: the beacon click stays on the left
                // button, as on the battlefield, in both schemes. RESIDUAL:
                // the release asks the radar again, so a team key pressed
                // while the left button is held can turn it into the team's
                // order, as a targeting press cancelled mid-press already can.
                || crate::app::input::radar::minimap_beacon_armed(state) =>
        {
            crate::app::input::radar::begin_context_press(state);
        }
        (MouseButton::Right, true) if right_click_orders => {
            crate::app::input::radar::begin_order_press(state, MouseButton::Right);
        }
        (MouseButton::Left | MouseButton::Right, true) => {
            crate::app::input::radar::centre_view_at_cursor(state);
        }
        (MouseButton::Left, false) => {
            crate::app::input::radar::finish_order_press(state, MouseButton::Left);
        }
        (MouseButton::Right, false)
            if right_click_orders
                && state.match_state.input.minimap_press_button() == Some(MouseButton::Right) =>
        {
            if cancel_armed_pointer_mode(state) {
                state.match_state.input.cancel_minimap_press();
            } else {
                crate::app::input::radar::finish_order_press(state, MouseButton::Right);
            }
        }
        _ => {}
    }
}

/// Handle cursor-move behavior while in-game.
///
/// While a minimap press is held the camera does NOT follow (gamemd ignores
/// held minimap motion); the move is swallowed so it can't start a selection
/// drag. Otherwise this updates the unit-selection drag rectangle.
fn clamp_tactical_drag_endpoint(
    cursor_x: f32,
    cursor_y: f32,
    (left, top, width, height): (u32, u32, u32, u32),
) -> (f32, f32) {
    let (left, top) = (left as f32, top as f32);
    (
        cursor_x.clamp(left, left + width.saturating_sub(1) as f32),
        cursor_y.clamp(top, top + height.saturating_sub(1) as f32),
    )
}

pub(crate) fn handle_cursor_moved_in_game(state: &mut AppState) {
    match modal_page(state) {
        Some(ModalPage::NetworkFailure) => {
            crate::app::match_runtime::network_failure::cursor_moved(state);
            return;
        }
        Some(ModalPage::NetworkWait) => {
            crate::app::input::connection::cursor_moved(state);
            return;
        }
        Some(ModalPage::Keyboard) => {
            crate::app::input::keyboard::cursor_moved(state);
            return;
        }
        Some(ModalPage::ShellError | ModalPage::ShellMessageBox) | None => {}
    }
    // VERA: a part grabbed in the layout edit mode follows the cursor.
    if crate::app::input::layout_edit::active(state) && !state.match_state.paused() {
        crate::app::input::layout_edit::cursor_moved(state);
        return;
    }
    // Paused in-game Options overlay: drive a live slider drag (visual/stored only —
    // cadence applies on close, KD-8) and swallow the move so it can't begin a
    // selection drag or camera pan behind the overlay.
    if state.match_state.paused() {
        match state.match_state.match_presentation.in_game_menu {
            crate::ui::pause_menu::InGameMenuState::Page => {
                crate::app::shell_wol::page::cursor_moved(state);
            }
            crate::ui::pause_menu::InGameMenuState::SavedGame(_) => {
                crate::app::App::update_saved_game_browser(state, true);
            }
            menu => crate::app::input::dialog_page::in_game_pointer(state, menu, None),
        }
        return;
    }
    // Minimap: gamemd re-centers only on press edges and ignores held motion
    // (decompile 0x006539D0: `param_1 & 0x22` early-out). While a minimap press
    // is held we do NOT follow the cursor — but still swallow the move so it
    // doesn't begin a unit selection-drag.
    if state.match_state.input.minimap_press_active() {
        return;
    }
    // Clamp drag position to the tactical viewport (exclude sidebar area).
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let clamped_endpoint = clamp_tactical_drag_endpoint(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
        crate::app::input::camera::tactical_viewport_px(state),
    );

    // VERA formation line: the drag adds to the line, clamped to the
    // battlefield like the band box's corner.
    if state.match_state.input.formation_line.drawing() {
        let world = crate::app::match_runtime::sim_tick::screen_point_to_world(
            state,
            clamped_endpoint.0,
            clamped_endpoint.1,
        );
        state.match_state.input.formation_line.extend(world);
        return;
    }

    // Activation arms the rectangle and nothing else. The call gamemd makes at
    // that moment is a cursor-shape setter, not an unselect — the selection is
    // only replaced on the release, and only when the box caught something.
    // The threshold is measured from the live mouse point. Once active, the
    // rendered/stored endpoint is restricted to the tactical surface.
    state.match_state.input.selection_state.update_drag(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    if state.match_state.input.selection_state.is_band_box_active() {
        state.match_state.input.selection_state.drag_current = Some(clamped_endpoint);
    }
}

#[cfg(all(test, tests_app_input_d_z))]
mod drag_tests {
    use super::clamp_tactical_drag_endpoint;
    use crate::sim::selection::{SelectAction, SelectionState};

    #[test]
    fn item82_captured_drag_live_and_release_endpoints_stop_at_tactical_rect() {
        let endpoint = clamp_tactical_drag_endpoint(950.0, 650.0, (0, 0, 632, 568));
        assert_eq!(endpoint, (631.0, 567.0));
        // A battlefield a layout sets in from the frame's corner clamps to
        // its own rectangle.
        let moved = (168, 32, 632, 568);
        assert_eq!(
            clamp_tactical_drag_endpoint(10.0, 5.0, moved),
            (168.0, 32.0)
        );
        assert_eq!(
            clamp_tactical_drag_endpoint(950.0, 650.0, moved),
            (799.0, 599.0)
        );

        let mut selection = SelectionState::new();
        selection.begin_drag(100.0, 100.0);
        selection.update_drag(950.0, 650.0);
        selection.drag_current = Some(endpoint);
        assert_eq!(selection.drag_rect(), Some((100.0, 100.0, 631.0, 567.0)));
        let SelectAction::BoxSelect(min_x, min_y, max_x, max_y) =
            selection.end_drag(endpoint.0, endpoint.1)
        else {
            panic!("active drag must end as a box selection");
        };
        assert_eq!((min_x, min_y, max_x, max_y), (100.0, 100.0, 631.0, 567.0));
    }
}

#[path = "selection_navigation.rs"]
pub(crate) mod selection_navigation;

#[cfg(all(test, tests_app_input_d_z))]
mod wheel_tests {
    use super::{WheelAction, wheel_action, wheel_notch};

    /// One notch is one row, whichever way it turns and however large the OS
    /// reports the delta. gamemd tests only the sign of the wheel delta and then
    /// executes a command that moves the strip by exactly one.
    #[test]
    fn magnitude_never_scales_the_step() {
        for up in [0.5_f32, 1.0, 3.0, 120.0] {
            assert_eq!(wheel_action(up), WheelAction::SidebarUp, "delta {up}");
        }
        for down in [-0.5_f32, -1.0, -3.0, -120.0] {
            assert_eq!(wheel_action(down), WheelAction::SidebarDown, "delta {down}");
        }
    }

    /// The native sign test maps zero to up; the Winit adapter filters
    /// zero-motion events before that mapping.
    #[test]
    fn native_sign_test_maps_zero_to_up() {
        assert_eq!(wheel_action(0.0), WheelAction::SidebarUp);
    }

    #[test]
    fn zero_motion_events_issue_no_command() {
        assert_eq!(wheel_notch(0.0), None);
        assert_eq!(wheel_notch(-0.0), None);
        assert_eq!(wheel_notch(-1.0), Some(WheelAction::SidebarDown));
    }
}

#[cfg(all(test, tests_app_input_d_z))]
mod item83_selection_order_tests;

#[cfg(all(test, tests_app_input_d_z))]
mod deploy_key_tests;

#[cfg(all(test, tests_app_input_d_z))]
mod control_group_tests;

#[cfg(all(test, tests_app_input_d_z))]
mod stop_command_tests {
    use crate::audio::events::GameSoundEvent;
    use crate::sim::command::Command;

    /// `Selection__IssueStopCommand @ 0x00730EA0` stops an own unit and then
    /// plays the retail StopSound (read at `0x0066ADE3`). An enemy alone
    /// does not qualify but still gets the cue; an empty selection gets
    /// nothing (`0x00730EAB`).
    #[test]
    fn stop_issues_to_qualifying_units_then_plays_its_retail_cue() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        let stop_sound = rules.general.stop_sound.clone();
        assert!(
            stop_sound
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("CommandBar"))
        );
        let mut sim = crate::sim::world::Simulation::new();
        for (index, (name, human)) in [("Americans", true), ("Russians", false)]
            .into_iter()
            .enumerate()
        {
            let id = sim.intern(name);
            let mut house =
                crate::sim::house_state::HouseState::new(id, index as u8, None, human, 0, 10);
            house.set_player_control_for_test(human);
            sim.houses_mut_for_test().register(house);
        }
        let owner = sim.interner().get("Americans").unwrap();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let own = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, rules)
            .unwrap();
        let enemy = sim
            .spawn_object("HTNK", "Russians", 12, 10, 0, rules)
            .unwrap();
        let cue = |sounds: &[GameSoundEvent]| matches!(sounds, [GameSoundEvent::UiSound { sound_id }] if Some(sound_id.name_in(rules)) == stop_sound.as_deref());

        assert!(cue(&super::queue_stop_orders(&mut sim, rules, vec![own])));
        let stops: Vec<_> = sim
            .pending_command_snapshot()
            .into_iter()
            .map(|envelope| envelope.payload)
            .collect();
        assert!(matches!(stops.as_slice(), [Command::Stop { entity_id }] if *entity_id == own));

        assert!(cue(&super::queue_stop_orders(&mut sim, rules, vec![enemy])));
        assert_eq!(sim.pending_command_snapshot().len(), 1);
        assert!(super::queue_stop_orders(&mut sim, rules, Vec::new()).is_empty());
    }
}

#[cfg(all(test, tests_app_input_d_z))]
mod modifier_tests {
    use super::KeyModifiers;

    /// A bare-key command fires only with no modifier held; the dispatcher
    /// rejects it under Ctrl, Alt or Shift and then looks for a chord binding
    /// that stock does not have.
    #[test]
    fn bare_key_bindings_require_no_modifier() {
        let bare = KeyModifiers::default();
        assert!(bare.none());
        assert!(!bare.any());
        for m in [
            KeyModifiers {
                shift: true,
                ..Default::default()
            },
            KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
            KeyModifiers {
                alt: true,
                ..Default::default()
            },
        ] {
            assert!(m.any(), "{m:?} should block a bare-key binding");
            assert!(!m.none());
        }
    }

    #[test]
    fn exact_modifier_sets_do_not_overlap() {
        let ctrl_shift = KeyModifiers {
            shift: true,
            ctrl: true,
            alt: false,
        };
        assert!(!ctrl_shift.only_ctrl());
        assert!(!ctrl_shift.only_shift());
        assert!(!ctrl_shift.only_alt());
        assert!(ctrl_shift.dev_chord());
    }
}

#[cfg(all(test, tests_app_input_d_z))]
mod context_click_route_tests {
    use super::*;

    #[test]
    fn click_modifiers_are_resolved_before_selection() {
        // This boundary previously skipped the resolver for every raw Shift
        // click, including Ctrl+Shift attack-move and Shift ground Move.
        let mut resolved = 0;
        let route =
            route_click_action_before_type_select(SelectAction::Click(12.0, 34.0), true, |x, y| {
                resolved += 1;
                assert_eq!((x, y), (12.0, 34.0));
                Some("admitted context action")
            });
        assert_eq!(resolved, 1);
        assert_eq!(
            route,
            ClickActionRoute::ContextOrder("admitted context action")
        );
        let selection = route_click_action_before_type_select::<()>(
            SelectAction::Click(12.0, 34.0),
            true,
            |_, _| None,
        );
        assert_eq!(selection, ClickActionRoute::Selection);
        assert_eq!(
            route_click_action_before_type_select::<()>(SelectAction::None, true, |_, _| panic!(
                "a box/empty selection action is not a context click"
            )),
            ClickActionRoute::Selection
        );
    }

    #[test]
    fn right_click_orders_leave_an_ordinary_left_click_to_selection() {
        assert_eq!(
            route_click_action_before_type_select::<()>(
                SelectAction::Click(12.0, 34.0),
                false,
                |_, _| panic!("with Right-Click Orders a plain left click only selects"),
            ),
            ClickActionRoute::Selection
        );
    }
}

#[cfg(all(test, tests_app_input_d_z))]
#[path = "building_occupant_deploy_tests.rs"]
mod building_occupant_deploy_tests;
