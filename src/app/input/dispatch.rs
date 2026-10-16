//! In-game input handling — mouse clicks, hotkeys, sidebar interactions,
//! control groups, and selection commands.
//!
//! Context-sensitive order resolution (click → command) lives in
//! `input::context_order`. This file handles raw input dispatch and UI state.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::collections::{HashSet, VecDeque};

use winit::event::{ElementState, MouseButton};
// Only the dev chord matches on bare `KeyCode`; the retail path below keeps
// fully-qualified `winit::keyboard::KeyCode`.
#[cfg(feature = "dev-overlays")]
use winit::keyboard::KeyCode;

use crate::app::AppState;
use crate::app::input::commands::{
    cancel_build_by_type, cycle_active_producer, cycle_local_owner, place_ready_building_at_cursor,
    place_starter_base_for_local_owner, preferred_local_owner, preferred_local_owner_name,
    queue_build_by_type, schedule_command, spawn_test_units_for_local_owner, suspend_build,
};
use crate::app::input::context_order::try_queue_context_order_at_screen_point;
use crate::app::input::entity_pick::{
    SelectionMutation, compute_box_selection_snapshot_with_playfield,
    compute_click_selection_snapshot_with_playfield,
    compute_type_select_box_mutation_with_playfield,
    compute_type_select_click_mutation_with_playfield, compute_type_select_tap_with_playfield,
    map_entity_creation_order, pick_entity_at_point,
};
use crate::app::input::hotkeys::{HotkeyCommand, HotkeyFallback, HotkeyResolution};
use crate::app::input::sidebar_eva;
use crate::app::presentation::sidebar_render::current_sidebar_view;
use crate::app::types::OrderMode;
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::sim::selection::SelectAction;
use crate::ui::sidebar::{SidebarAction, SidebarTab};

#[cfg(test)]
#[path = "area_guard_tests.rs"]
mod area_guard_tests;

#[cfg(test)]
#[path = "player_scatter_tests.rs"]
mod player_scatter_tests;

/// Click radius for single-click selection (pixels in world space).
pub(crate) const CLICK_SELECT_RADIUS: f32 = 30.0;

/// Handle mouse button press/release for selection and move commands.
///
/// The in-game gadget walk runs FIRST (study G22/A8): chrome buttons + cameos
/// fire/consume there; the full-tactical catcher and the minimap region decide
/// WHICH body runs (the regions are sticky, so a drag stays bound to its region
/// across the sidebar boundary). A `NotConsumed` click hits no live gadget — only
/// the legacy dev/pause/producer press path runs; empty-sidebar / off-window
/// clicks do nothing (gamemd's sidebar-body gadget swallows them, A6). The middle
/// button has no tactical behavior. Right-press is owned by the tactical catcher
/// (viewport-only), so right-clicking dead sidebar chrome no longer deselects —
/// matching gamemd.
pub(crate) fn handle_mouse_input(
    state: &mut AppState,
    button: MouseButton,
    btn_state: ElementState,
) {
    use crate::app::input::gadget_input::GadgetConsume;
    let pressed = btn_state.is_pressed();
    state
        .match_state
        .input
        .tactical_mouse
        .record_physical_button(button, pressed);
    if state.frontend.keyboard_dialog.is_some() {
        crate::app::input::keyboard::mouse(state, button, pressed);
        return;
    }
    // Paused in-game Options overlay owns the mouse: route press/release/checkbox/
    // Back here and CONSUME the click so it never reaches the tactical viewport or
    // a gadget (no unit orders behind the overlay). KD-6.
    if state.match_state.paused() {
        // VERA-internal (gamemd has no pause overlay; gamemd equivalent
        // UNCHECKED): a release that arrives while the overlay owns the mouse
        // never reaches the tactical body, so the whole gesture is dropped
        // here — not just the capture byte. Leaving `minimap_dragging` or the
        // band drag set would freeze edge auto-scroll, swallow every cursor
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
        if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::Sound
        {
            crate::app::input::sound::mouse(state, button, pressed);
            return;
        }
        if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::AbortConfirm
        {
            crate::app::input::abort::mouse(state, button, pressed);
            return;
        }
        if matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
        ) {
            if button == MouseButton::Left {
                crate::app::App::saved_game_mouse(state, pressed);
            }
        } else if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::Menu
        {
            crate::app::input::pause_menu::mouse(state, button, pressed);
        } else {
            crate::app::input::in_game_options::in_game_options_mouse(state, button, pressed);
        }
        return;
    }
    // ScrollHandleMouse6930A0 refuses ordinary pointer messages while the
    // Scenario input lock is active; capture cancellation already ran in46.
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    // The stock tactical handler has no middle-button case.
    if button == MouseButton::Middle {
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
const HELD_TYPE_SELECT_VOICE_POLICY: SelectionVoicePolicy = SelectionVoicePolicy::Suppressed;
const ORDINARY_SELECTION_ACTION_LINE_POLICY: SelectionActionLinePolicy =
    SelectionActionLinePolicy::Start;
const TYPE_SELECT_TAP_ACTION_LINE_POLICY: SelectionActionLinePolicy =
    SelectionActionLinePolicy::Preserve;

/// Resolve the ordinary tactical action before TypeSelect is allowed to modify
/// a selection/toggle click. Non-held Shift keeps its established toggle path;
/// while TypeSelect is held, Shift attack/move actions still resolve first.
fn route_click_action_before_type_select<T>(
    action: SelectAction,
    shift: bool,
    type_select_held: bool,
    resolve_context_order: impl FnOnce(f32, f32) -> Option<T>,
) -> ClickActionRoute<T> {
    let SelectAction::Click(sx, sy) = action else {
        return ClickActionRoute::Selection;
    };
    if shift && !type_select_held {
        return ClickActionRoute::Selection;
    }
    resolve_context_order(sx, sy)
        .map(ClickActionRoute::ContextOrder)
        .unwrap_or(ClickActionRoute::Selection)
}

/// No selected object can send a click order (`click_order_receivers`), so
/// DetermineAction's best object is berserk: `SelectBestObjectForAction`
/// ranks a berserk object below every other (`0x00535435`).
fn every_selected_object_is_berserk(state: &AppState) -> bool {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return false;
    };
    let selection = selected_stable_ids_in_order(
        Some(sim),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    !selection.is_empty() && sim.click_order_receivers(&selection).is_empty()
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
                if state.match_state.input.planning.active()
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
                {
                    return; // suppress selection drag while a targeting / repair / sell mode is active
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
                if !state.match_state.input.tactical_mouse.end_left_press() {
                    return;
                }
                // Repair / Sell cursor modes consume the click — toggle repair or
                // sell the own building under the cursor. The mode stays active
                // (sticky) so the player can act on several buildings in a row.
                if crate::app::input::commands::try_repair_sell_mode_click(state) {
                    return;
                }
                if let Some(section) = state.armed_super_weapon_type().map(str::to_owned) {
                    crate::app::input::commands::launch_super_weapon_at_cursor(state, &section);
                    return;
                }
                if let Some(type_id) = state.armed_building_type().map(str::to_owned) {
                    place_ready_building_at_cursor(state, &type_id);
                    return;
                }
                if crate::app::input::planning_mode::connect_at_cursor(state) {
                    return;
                }
                // Only an active band box owns a clamped tactical endpoint.
                // A pending press is still an ordinary click at the actual
                // release point, including after sticky capture routing.
                let release_point = if state.match_state.input.selection_state.is_band_box_active()
                {
                    let (tactical_width, tactical_height) =
                        crate::app::input::camera::tactical_viewport_size(state);
                    clamp_tactical_drag_endpoint(
                        state.match_state.input.cursor_x,
                        state.match_state.input.cursor_y,
                        tactical_width,
                        tactical_height,
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
                    apply_selection_action_line_policy(
                        state,
                        ORDINARY_SELECTION_ACTION_LINE_POLICY,
                    );
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
                        crate::app::presentation::instances::tactical_band_preflight_entity_encounter_order(state);
                    if band_caught_drawn_object(state, &order, min_x, min_y, max_x, max_y) {
                        band_preflight_order = Some(order);
                    } else {
                        action = SelectAction::Click(release_point.0, release_point.1);
                    }
                }
                // TypeSelect modifies only actions that already resolved as
                // selection/toggle. Ground move, attack, and every other
                // context action keep their ordinary priority while T is held.
                let type_select_held = state.match_state.input.type_select.held();
                // Shift toggles selection through What_Action's Shift arm
                // (`0x00700082`). A berserk best object answers 0 before it
                // (`0x006FFEC9`), so when every selected object is berserk a
                // Shift click resolves like any other click.
                let shift_selects = shift && !every_selected_object_is_berserk(state);
                if matches!(
                    route_click_action_before_type_select(
                        action,
                        shift_selects,
                        type_select_held,
                        |sx, sy| {
                            // A resolved context dispatch consumes the click
                            // even when its receiver admits no command.
                            try_queue_context_order_at_screen_point(state, sx, sy, true)
                                .then_some(())
                        },
                    ),
                    ClickActionRoute::ContextOrder(())
                ) {
                    return;
                }
                let mut queued_selection: Option<SelectionMutation> = None;
                let mut held_type_select_batch = false;
                if let Some(sim) = state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation)
                {
                    let screen_order =
                        crate::app::presentation::instances::tactical_screen_entity_encounter_order(
                            state,
                        );
                    let current_selection = selected_stable_ids_in_order(
                        state
                            .match_state
                            .sim_runtime
                            .as_ref()
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
                            // Native TacticalPick6DA380 reads the retained
                            // Display registrations after the projected click
                            // gate. ScreenSelection's raw-anchor filter belongs
                            // to band/type scope, not this pointer pick.
                            let click_order = crate::app::presentation::instances::
                                tactical_entity_encounter_order(sim);
                            let world_x: f32 = sx / state.match_state.input.zoom_level
                                + state.match_state.input.camera_x;
                            let world_y: f32 = sy / state.match_state.input.zoom_level
                                + state.match_state.input.camera_y;
                            let fog_ref = if state.match_state.sandbox_full_visibility {
                                None
                            } else {
                                Some(&sim.fog)
                            };
                            if held_type_select {
                                let picked = pick_entity_at_point(
                                    sim.entities(),
                                    &click_order,
                                    fog_ref,
                                    preferred_local_owner_name(state).as_deref(),
                                    world_x,
                                    world_y,
                                    CLICK_SELECT_RADIUS,
                                    state.rules(),
                                    Some(&sim.houses),
                                    state.height_map(),
                                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                                    Some(&sim.interner),
                                    sim.resolved_terrain.as_ref(),
                                );
                                queued_selection = if let Some(clicked_id) = picked {
                                    Some(compute_type_select_click_mutation_with_playfield(
                                        sim.entities(),
                                        scope_order,
                                        &current_selection,
                                        clicked_id,
                                        shift,
                                        preferred_local_owner_name(state).as_deref(),
                                        state.rules(),
                                        Some(&sim.interner),
                                        sim.playfield_bounds.is_some(),
                                    ))
                                } else {
                                    compute_click_selection_snapshot_with_playfield(
                                        sim.entities(),
                                        &click_order,
                                        &current_selection,
                                        fog_ref,
                                        preferred_local_owner_name(state).as_deref(),
                                        world_x,
                                        world_y,
                                        CLICK_SELECT_RADIUS,
                                        shift,
                                        state.rules(),
                                        Some(&sim.houses),
                                        state.height_map(),
                                        crate::app::match_runtime::sim_tick::tactical_bridge_cells(
                                            sim,
                                        ),
                                        Some(&sim.interner),
                                        sim.playfield_bounds.is_some(),
                                        sim.resolved_terrain.as_ref(),
                                    )
                                };
                                held_type_select_batch = true;
                            } else {
                                queued_selection = compute_click_selection_snapshot_with_playfield(
                                    sim.entities(),
                                    &click_order,
                                    &current_selection,
                                    fog_ref,
                                    preferred_local_owner_name(state).as_deref(),
                                    world_x,
                                    world_y,
                                    CLICK_SELECT_RADIUS,
                                    shift,
                                    state.rules(),
                                    Some(&sim.houses),
                                    state.height_map(),
                                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                                    Some(&sim.interner),
                                    sim.playfield_bounds.is_some(),
                                    sim.resolved_terrain.as_ref(),
                                );
                            }
                        }
                        SelectAction::BoxSelect(min_x, min_y, max_x, max_y) => {
                            let fog_ref = if state.match_state.sandbox_full_visibility {
                                None
                            } else {
                                Some(&sim.fog)
                            };
                            let z = state.match_state.input.zoom_level;
                            let (min_x, min_y, max_x, max_y) = (
                                min_x / z + state.match_state.input.camera_x,
                                min_y / z + state.match_state.input.camera_y,
                                max_x / z + state.match_state.input.camera_x,
                                max_y / z + state.match_state.input.camera_y,
                            );
                            if held_type_select {
                                queued_selection =
                                    Some(compute_type_select_box_mutation_with_playfield(
                                        sim.entities(),
                                        &screen_order,
                                        scope_order,
                                        &current_selection,
                                        fog_ref,
                                        preferred_local_owner_name(state).as_deref(),
                                        min_x,
                                        min_y,
                                        max_x,
                                        max_y,
                                        shift,
                                        state.rules(),
                                        Some(&sim.interner),
                                        sim.playfield_bounds.is_some(),
                                    ));
                                held_type_select_batch = true;
                            } else {
                                let preflight_order = band_preflight_order
                                    .as_deref()
                                    .unwrap_or(screen_order.as_slice());
                                queued_selection = compute_box_selection_snapshot_with_playfield(
                                    sim.entities(),
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
                                    Some(&sim.houses),
                                    Some(&sim.interner),
                                    sim.playfield_bounds.is_some(),
                                );
                            }
                        }
                        SelectAction::None => {}
                    }
                }
                if let Some(mutation) = queued_selection {
                    if held_type_select_batch {
                        if mutation.select.is_empty() {
                            apply_selection_mutation(
                                state,
                                mutation,
                                false,
                                ORDINARY_SELECTION_VOICE_POLICY,
                            );
                        } else {
                            let prior = state.match_state.input.selection_voice_enabled;
                            state.match_state.input.selection_voice_enabled = false;
                            apply_selection_mutation(
                                state,
                                mutation,
                                false,
                                HELD_TYPE_SELECT_VOICE_POLICY,
                            );
                            state.match_state.input.selection_voice_enabled = prior;
                        }
                    } else {
                        apply_selection_mutation(
                            state,
                            mutation,
                            true,
                            ORDINARY_SELECTION_VOICE_POLICY,
                        );
                    }
                    // Click-select4ABE83 starts separately. A band release
                    // already started before its empty-catch decision above.
                    if !band_release {
                        apply_selection_action_line_policy(
                            state,
                            ORDINARY_SELECTION_ACTION_LINE_POLICY,
                        );
                    }
                }
            }
        }
        MouseButton::Right => {
            if btn_state.is_pressed() {
                // The native right press has no game effect at all: it records
                // the pan anchor and takes the capture, and only does that when
                // no other button already holds it. Everything the player sees
                // happens on the release edge.
                if state.match_state.input.tactical_mouse.press_may_arm() {
                    state.match_state.input.tactical_mouse.begin_right_drag((
                        state.match_state.input.cursor_x,
                        state.match_state.input.cursor_y,
                    ));
                }
            } else if let Some(run_cancel_ladder) =
                state.match_state.input.tactical_mouse.end_right_press()
            {
                // The cancel ladder runs only when the drag threshold was
                // never crossed. A right drag that panned the map ends
                // silently — the selection survives it.
                if run_cancel_ladder {
                    right_click_cancel_ladder(state);
                }
                // The native release tears the band rectangle down too.
                state.match_state.input.selection_state.cancel_drag();
            }
        }
        _ => {}
    }
}

/// The right-release cancel ladder: cancel exactly one armed cursor mode, and
/// clear the selection only when nothing was armed.
///
/// gamemd walks seven mode flags in a fixed order and returns after the first
/// one it cancels; VERA models the two it has (a targeting/placement cursor and
/// the repair/sell cursor). The final rung — clear the selection — is retail
/// behaviour, not a VERA deviation.
fn right_click_cancel_ladder(state: &mut AppState) {
    if state.match_state.input.cancel_building_placement() {
        return;
    }
    if state
        .match_state
        .input
        .targeting_mode
        .as_ref()
        .and_then(crate::app::types::TargetingMode::as_super_weapon)
        .is_some()
    {
        state.match_state.input.targeting_mode = None;
        state.match_state.input.building_placement_preview = None;
        return;
    }
    if state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .repair_mode_on
        || state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .sell_mode_on
    {
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .repair_mode_on = false;
        state
            .match_state
            .match_presentation
            .sidebar_gadget_state
            .sell_mode_on = false;
        return;
    }
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
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return false;
    };
    let z = state.match_state.input.zoom_level;
    let fog_ref = if state.match_state.sandbox_full_visibility {
        None
    } else {
        Some(&sim.fog)
    };
    crate::app::input::entity_pick::band_rect_contains_drawn_object(
        sim.entities(),
        encounter_order,
        fog_ref,
        preferred_local_owner_name(state).as_deref(),
        min_x / z + state.match_state.input.camera_x,
        min_y / z + state.match_state.input.camera_y,
        max_x / z + state.match_state.input.camera_x,
        max_y / z + state.match_state.input.camera_y,
        Some(&sim.interner),
    )
}

/// Minimap mouse body (routed here when the minimap ClickRegion consumes the
/// edge). gamemd (decompile 0x006539D0) centers the tactical view on press
/// edges (left OR right) and IGNORES held motion — there is no continuous
/// camera-follow (the held branch is dropped in `handle_cursor_moved_in_game`).
pub(crate) fn minimap_mouse(state: &mut AppState, button: MouseButton, btn_state: ElementState) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    match button {
        MouseButton::Left => {
            if btn_state.is_pressed() {
                crate::app::presentation::sidebar_render::try_begin_minimap_drag(state);
            } else if state.match_state.input.minimap_dragging {
                state.match_state.input.minimap_dragging = false;
            }
        }
        MouseButton::Right
            // A right-press centers the view on the clicked cell (no command);
            // right-release just releases the gadget's sticky capture.
            if btn_state.is_pressed()
                && crate::app::presentation::sidebar_render::is_cursor_over_minimap(state)
            => {
                crate::app::presentation::sidebar_render::update_camera_from_minimap_cursor(state);
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
    tactical_width: u32,
    tactical_height: u32,
) -> (f32, f32) {
    (
        cursor_x.clamp(0.0, tactical_width.saturating_sub(1) as f32),
        cursor_y.clamp(0.0, tactical_height.saturating_sub(1) as f32),
    )
}

pub(crate) fn handle_cursor_moved_in_game(state: &mut AppState) {
    if state.frontend.keyboard_dialog.is_some() {
        crate::app::input::keyboard::cursor_moved(state);
        return;
    }
    // Paused in-game Options overlay: drive a live slider drag (visual/stored only —
    // cadence applies on close, KD-8) and swallow the move so it can't begin a
    // selection drag or camera pan behind the overlay.
    if state.match_state.paused() {
        if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::Sound
        {
            crate::app::input::sound::cursor_moved(state);
            return;
        }
        if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::AbortConfirm
        {
            crate::app::input::abort::cursor_moved(state);
            return;
        }
        if matches!(
            state.match_state.match_presentation.in_game_menu,
            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
        ) {
            crate::app::App::update_saved_game_browser(state, true);
        } else if state.match_state.match_presentation.in_game_menu
            == crate::ui::pause_menu::InGameMenuState::Menu
        {
            crate::app::input::pause_menu::cursor_moved(state);
        } else {
            crate::app::input::in_game_options::in_game_options_drag(state);
        }
        return;
    }
    // Minimap: gamemd re-centers only on press edges and ignores held motion
    // (decompile 0x006539D0: `param_1 & 0x22` early-out). While a minimap press
    // is held we do NOT follow the cursor — but still swallow the move so it
    // doesn't begin a unit selection-drag.
    if state.match_state.input.minimap_dragging {
        return;
    }
    // Clamp drag position to the tactical viewport (exclude sidebar area).
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let (tactical_width, tactical_height) =
        crate::app::input::camera::tactical_viewport_size(state);
    let clamped_endpoint = clamp_tactical_drag_endpoint(
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
        tactical_width,
        tactical_height,
    );

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

#[cfg(test)]
mod drag_tests {
    use super::clamp_tactical_drag_endpoint;
    use crate::sim::selection::{SelectAction, SelectionState};

    #[test]
    fn item82_captured_drag_live_and_release_endpoints_stop_at_tactical_rect() {
        let endpoint = clamp_tactical_drag_endpoint(950.0, 650.0, 632, 568);
        assert_eq!(endpoint, (631.0, 567.0));

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

#[cfg(test)]
mod item83_click_route_tests {
    use super::{ClickActionRoute, route_click_action_before_type_select};
    use crate::app::input::context_order::{OrderModifier, object_click_payload};
    use crate::app::input::entity_pick::compute_type_select_click_mutation;
    use crate::app::types::OrderMode;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::command::Command;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;
    use crate::sim::selection::SelectAction;

    #[test]
    fn item83_held_type_select_keeps_attack_and_ground_move_ahead_of_selection() {
        let attack = route_click_action_before_type_select(
            SelectAction::Click(40.0, 40.0),
            true,
            true,
            |_, _| {
                Some(object_click_payload(
                    OrderMode::Move,
                    OrderModifier::Normal,
                    false,
                    1,
                    9,
                ))
            },
        );
        assert_eq!(
            attack,
            ClickActionRoute::ContextOrder(Command::Attack {
                attacker_id: 1,
                target_id: 9,
            })
        );

        let mut selection = vec![1];
        let ground = route_click_action_before_type_select(
            SelectAction::Click(80.0, 70.0),
            false,
            true,
            |_, _| {
                Some(Command::Move {
                    entity_id: 1,
                    target_rx: 14,
                    target_ry: 15,
                })
            },
        );
        match ground {
            ClickActionRoute::ContextOrder(Command::Move { .. }) => {}
            ClickActionRoute::Selection => selection.clear(),
            other => panic!("unexpected held-ground route: {other:?}"),
        }
        assert_eq!(
            selection,
            [1],
            "an ordered ground click never reaches selection clear"
        );
    }

    #[test]
    fn consumed_empty_context_dispatch_does_not_become_a_selection_click() {
        // Display4ABFA9 ->4ABFAE ->4AC294 consumes Move1/NoMove2
        // independently of the receiver's event output. Select7 exits earlier
        // at4ABEF3. This tests the existing typed route, not a reconstructed
        // AppState or a fabricated production input receiver.
        for (shift, type_select_held) in [(false, false), (false, true), (true, true)] {
            let route = route_click_action_before_type_select(
                SelectAction::Click(80.0, 70.0),
                shift,
                type_select_held,
                |_, _| Some(Vec::<Command>::new()),
            );
            assert_eq!(route, ClickActionRoute::ContextOrder(Vec::new()));
        }
        // A genuine selection result still falls through. Event count alone
        // cannot distinguish these two native input outcomes.
        let route = route_click_action_before_type_select(
            SelectAction::Click(80.0, 70.0),
            false,
            false,
            |_, _| None::<Vec<Command>>,
        );
        assert_eq!(route, ClickActionRoute::Selection);
    }

    #[test]
    fn item83_held_friendly_selection_falls_through_to_exact_type_batch() {
        let route = route_click_action_before_type_select(
            SelectAction::Click(40.0, 40.0),
            false,
            true,
            |_, _| None::<Command>,
        );

        let mut interner = StringInterner::new();
        let owner = interner.intern("Americans");
        let type_ref = interner.intern("AMCV");
        let mut entities = EntityStore::new();
        for (id, rx) in [(1, 10), (2, 12)] {
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                rx,
                10,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.lifecycle.set_object_alive_for_test(true);
            entity.lifecycle.set_in_limbo_for_test(false);
            entities.insert(entity);
        }

        let mutation = match route {
            ClickActionRoute::Selection => compute_type_select_click_mutation(
                &entities,
                &[1, 2],
                &[1],
                1,
                false,
                Some("Americans"),
                None,
                Some(&interner),
            ),
            ClickActionRoute::ContextOrder(command) => {
                panic!("friendly selection unexpectedly dispatched {command:?}")
            }
        };
        assert!(mutation.clear);
        assert_eq!(mutation.select, [1, 2]);
    }
}

/// The sidebar command one wheel notch resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelAction {
    /// Scroll the active build strip up one row.
    SidebarUp,
    /// Scroll the active build strip down one row.
    SidebarDown,
}

/// Resolve a wheel notch to its sidebar command.
///
/// gamemd's window procedure intercepts every wheel message and executes the
/// command named `SidebarDown` when the delta is negative and `SidebarUp`
/// otherwise — a zero delta goes up, because the test is a signed less-than.
/// Magnitude is not a multiplier: one message is one command is one row.
pub(crate) fn wheel_action(delta_lines: f32) -> WheelAction {
    if delta_lines < 0.0 {
        WheelAction::SidebarDown
    } else {
        WheelAction::SidebarUp
    }
}

/// Apply one wheel notch to a strip's scroll row.
///
/// The native scroll refuses to move above row 0 or past the strip's computed
/// visible capacity, so both ends saturate rather than wrap.
pub(crate) fn wheel_scrolled_row(current: usize, max_rows: usize, action: WheelAction) -> usize {
    match action {
        WheelAction::SidebarUp => current.saturating_sub(1),
        WheelAction::SidebarDown => (current + 1).min(max_rows),
    }
}

/// Scroll the active build strip by one row.
///
/// The cursor position is deliberately not consulted. The retail binding is a
/// window message routed straight to a command, not a hit-tested gadget, so the
/// wheel scrolls the sidebar from anywhere on the screen — and there is no world
/// zoom in gamemd for the wheel to reach instead.
pub(crate) fn sidebar_wheel_scroll(state: &mut AppState, delta_lines: f32) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let Some(view) = current_sidebar_view(state).cloned() else {
        return;
    };
    let row = wheel_scrolled_row(
        view.scroll_rows,
        view.max_scroll_rows,
        wheel_action(delta_lines),
    );
    let presentation = &mut state.match_state.match_presentation;
    presentation
        .sidebar_projection
        .set_scroll_row(presentation.active_sidebar_tab, row);
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

/// The local player's queue of record, in queue order (empty without a sim).
fn local_queue_view(state: &AppState) -> Vec<crate::sim::production::QueueItemView> {
    let Some(owner) = preferred_local_owner_name(state) else {
        return Vec::new();
    };
    match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
    ) {
        (Some(sim), Some(rules)) => {
            crate::sim::production::queue_view_for_owner(sim, rules, &owner)
        }
        _ => Vec::new(),
    }
}

/// The interned id a type name already has in the sim (never interns).
fn local_interned(state: &AppState, type_id: &str) -> Option<crate::sim::intern::InternedId> {
    state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|rt| rt.simulation.interner.get(type_id))
}

/// The `EVA_SelectTarget` decision for the local player's superweapon
/// `section`: its live view (ready, online) and the type's `Action=`.
fn local_super_weapon_select_target_line(state: &AppState, section: &str) -> Option<&'static str> {
    let owner = preferred_local_owner_name(state)?;
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)?;
    let rules = state.rules()?;
    let type_id = sim.interner.get(section)?;
    let owner_iid = sim.interner.get(&owner)?;
    let view = crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner_iid)
        .into_iter()
        .find(|view| view.type_id == type_id)?;
    let action = rules.super_weapon(section)?.action.as_deref();
    sidebar_eva::select_target_line(view.is_ready, view.is_online, action)
}

/// Speak an app-local EVA line (`VoxClass::PlayEVA(name, -1)` inside a click
/// handler): the entry's own `Type=`/`Priority=` route it.
pub(crate) fn push_local_eva(state: &mut AppState, event: &str) {
    state
        .match_state
        .match_audio
        .sound_events
        .push(crate::audio::events::GameSoundEvent::Eva {
            event: event.to_string(),
            type_override: None,
        });
}

/// A press on a build cameo: `SelectClass::Action @ 0x006AAD00`'s decision
/// (`sidebar_eva::cameo_click`) against the local player's queue, then its
/// `GUIBuildSound`, EVA line and event.
fn sidebar_cameo_press(
    state: &mut AppState,
    type_id: &str,
    press: sidebar_eva::CameoPress,
    at_build_limit: bool,
) {
    let Some(category) = state
        .rules()
        .and_then(|rules| rules.object(type_id))
        .map(crate::sim::production::category_for_object)
    else {
        return;
    };
    let queue = local_queue_view(state);
    let type_iid = local_interned(state, type_id);
    let linked_factory = preferred_local_owner_name(state).and_then(|owner| {
        let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .cameo_factory(sim, sim.interner.get(&owner)?, type_iid?)
            .map(|factory| {
                (
                    factory.cameo_state(),
                    factory
                        .complete_object()
                        .and_then(|object| object.entity_id.map(|held| (object.type_id, held))),
                )
            })
    });
    let click = sidebar_eva::cameo_click(
        &queue,
        category,
        type_iid,
        linked_factory.map(|(status, _)| status),
        press,
        at_build_limit,
    );
    if click.clear_building_placement {
        state.match_state.input.cancel_building_placement();
    }
    if click.sound {
        let sound = state
            .rules()
            .and_then(|rules| rules.general.gui_build_sound.clone());
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
    if let Some(line) = click.eva {
        push_local_eva(state, line);
    }
    match click.order {
        None => {}
        Some(sidebar_eva::CameoOrder::Produce) => queue_build_by_type(state, type_id),
        Some(sidebar_eva::CameoOrder::Suspend) => suspend_build(state, category),
        Some(sidebar_eva::CameoOrder::Abandon { all }) => {
            cancel_build_by_type(state, type_id, all);
        }
        Some(sidebar_eva::CameoOrder::Place) => {
            // Select6AB30D plays GUIBuildSound before ObjectFindFactory5F5C20
            // (0,0) at6AB31A, including repeated pending clicks. A null builder
            // instead queues ABANDON16 before EVA_UnableToComply6AB3A9.
            let ready_target = preferred_local_owner_name(state).and_then(|owner| {
                let runtime = state.match_state.sim_runtime.as_ref()?;
                let sim = &runtime.simulation;
                let rules = &runtime.resources.rules;
                let owner_id = sim.interner.get(&owner)?;
                // Select6AB2CC reads this linked Factory's actual product;
                // an older cameo can still refer to a rearmed Factory.
                let (held_type, held) = linked_factory?.1?;
                Some((
                    owner,
                    owner_id,
                    held,
                    sim.interner.resolve(held_type).to_string(),
                    sim.entities().get(held)?.category() == EntityCategory::Structure,
                    crate::sim::production::object_find_factory(sim, rules, held, false, false),
                ))
            });
            let Some((owner, owner_id, held, held_type, is_building, builder)) = ready_target
            else {
                return;
            };
            let Some(_builder) = builder else {
                cancel_build_by_type(state, type_id, false);
                push_local_eva(state, sidebar_eva::EVA_UNABLE_TO_COMPLY);
                return;
            };
            if !is_building {
                // Select6AB3D9..6AB459 emits PLACE11 using the requested
                // cameo's kind/Naval slot and index -1, without HasChanged.
                // The existing receiver resolves the current complete head.
                // Source: tools/sidebar_oracle/factory_sale_sidebar.md.
                schedule_command(state, &owner, Command::PlaceProducedMobile { category });
                return;
            }
            // ManualPlace4FB852 refuses while any building placement is pending.
            if state
                .match_state
                .input
                .begin_building_placement(&held_type, held, owner_id)
            {
                // ManualPlace4FB881 calls UnselectAll48DC90 only on entry.
                // This existing owner clears immediate selection/follow and
                // schedules the authoritative membership commit. A clear has
                // no additions, so its effects do not depend on cursor mode.
                queue_selection_snapshot_command(state, Vec::new(), false);
                let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
                gadgets.disable_selection_modes(false);
            }
        }
    }
}

pub(crate) fn apply_sidebar_action(state: &mut AppState, action: SidebarAction) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    match action {
        SidebarAction::None => {}
        SidebarAction::OpenPauseMenu => open_pause_menu(state),
        SidebarAction::OpenDiplomacy => open_diplomacy_menu(state),
        SidebarAction::SelectTab(tab) => {
            // Retained strips own their rows; changing tabs only selects a strip.
            state.match_state.match_presentation.active_sidebar_tab = tab;
        }
        SidebarAction::CameoPress {
            type_id,
            right,
            shift,
            at_build_limit,
        } => {
            let press = if right {
                sidebar_eva::CameoPress::Right { shift }
            } else {
                sidebar_eva::CameoPress::Left
            };
            sidebar_cameo_press(state, &type_id, press, at_build_limit);
        }
        SidebarAction::ArmSuperWeapon(section) => {
            // `SelectClass::Action 0x006AAFA7`: a ready, targeted superweapon
            // cameo speaks `EVA_SelectTarget` on the clicking machine
            // (`sidebar_eva::select_target_line`). The cameo only arms when
            // its view is ready, so the not-ready silence is the hit test's.
            if let Some(line) = local_super_weapon_select_target_line(state, &section) {
                push_local_eva(state, line);
            }
            state.match_state.input.targeting_mode =
                Some(crate::app::types::TargetingMode::SuperWeapon(section));
            // Mutual exclusion: clear building-placement preview AND repair/sell modes.
            state.match_state.input.building_placement_preview = None;
            state
                .match_state
                .match_presentation
                .sidebar_gadget_state
                .repair_mode_on = false;
            state
                .match_state
                .match_presentation
                .sidebar_gadget_state
                .sell_mode_on = false;
            log::info!(
                "SuperWeapon armed: type={}",
                state.armed_super_weapon_type().unwrap_or("")
            );
        }
        SidebarAction::ClearSuperWeaponMode => {
            state.match_state.input.targeting_mode = None;
            log::info!("SuperWeapon targeting cleared");
        }
        SidebarAction::CycleProducer(category) => {
            cycle_active_producer(state, category);
        }
        SidebarAction::CycleOwner => {
            cycle_local_owner(state);
        }
        SidebarAction::PlaceStarterBase => {
            place_starter_base_for_local_owner(state);
        }
        SidebarAction::SpawnTestUnits => {
            spawn_test_units_for_local_owner(state);
        }
        SidebarAction::ToggleRepairMode => {
            let g = &mut state.match_state.match_presentation.sidebar_gadget_state;
            g.repair_mode_on = !g.repair_mode_on;
            if g.repair_mode_on {
                g.sell_mode_on = false;
                state.match_state.input.targeting_mode = None;
                state.match_state.input.building_placement_preview = None;
            }
        }
        SidebarAction::ToggleSellMode => {
            let g = &mut state.match_state.match_presentation.sidebar_gadget_state;
            g.sell_mode_on = !g.sell_mode_on;
            if g.sell_mode_on {
                g.repair_mode_on = false;
                state.match_state.input.targeting_mode = None;
                state.match_state.input.building_placement_preview = None;
            }
        }
        SidebarAction::Deploy => {
            queue_deploy_undeploy_for_selected(state);
        }
    }
}

/// Toggle the unit-inspector debug overlay.
///
/// Beyond flipping `state.diag.debug_unit_inspector`, this allocates per-entity
/// debug logs on enable and frees them on disable, and sets the sim flag
/// `debug_event_logging`. Called by both the X hotkey and the dev overlay
/// checkbox so the two paths cannot drift.
/// Explain every terrain cell that draws as black, and say which cause is responsible.
///
/// A cell renders black for exactly two reasons, and they need completely different
/// fixes: the vision system never revealed it, or it was revealed but its tile key is
/// missing from the atlas. Both look identical on screen, so this counts them separately
/// instead of leaving the diagnosis to guesswork.
pub(crate) fn report_black_cell_causes(state: &mut AppState) {
    let Some(grid) = state.match_state.match_presentation.terrain_grid.as_ref() else {
        log::info!("Black-cell report: no terrain grid loaded");
        return;
    };

    let owner = crate::app::input::commands::preferred_local_owner_name(state).and_then(|name| {
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)?
            .interner
            .get(&name)
    });
    let fog = match (
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        owner,
    ) {
        _ if state.match_state.sandbox_full_visibility => None,
        (Some(sim), Some(id)) => Some((id, &sim.fog)),
        _ => None,
    };

    let mut unrevealed: u32 = 0;
    let mut missing_tile: u32 = 0;
    // Checked for every cell regardless of shroud. The fog-gated count above only sees
    // cells the player has explored, so on an unexplored map it can report zero while the
    // rest of the map is full of unresolvable tiles. This one cannot be fooled that way.
    let mut missing_tile_anywhere: u32 = 0;
    let mut missing_samples: Vec<(u16, u16, u16, u8)> = Vec::new();
    let mut unrevealed_samples: Vec<(u16, u16)> = Vec::new();

    for cell in &grid.cells {
        if let Some(atlas) = state.match_state.match_presentation.tile_atlas.as_ref() {
            let key = crate::map::theater::TileKey {
                tile_id: cell.tile_id,
                sub_tile: cell.sub_tile,
                variant: 0,
            };
            if atlas.get_uv(key).is_none() {
                missing_tile_anywhere += 1;
            }
        }
        if let Some((id, fog_state)) = fog
            && !fog_state.is_cell_revealed(id, cell.rx, cell.ry)
        {
            unrevealed += 1;
            if unrevealed_samples.len() < 12 {
                unrevealed_samples.push((cell.rx, cell.ry));
            }
            // An unrevealed cell is never drawn, so its tile is irrelevant.
            continue;
        }
        if let Some(atlas) = state.match_state.match_presentation.tile_atlas.as_ref() {
            let key = crate::map::theater::TileKey {
                tile_id: cell.tile_id,
                sub_tile: cell.sub_tile,
                variant: 0,
            };
            if atlas.get_uv(key).is_none() {
                missing_tile += 1;
                if missing_samples.len() < 12 {
                    missing_samples.push((cell.rx, cell.ry, cell.tile_id, cell.sub_tile));
                }
            }
        }
    }

    log::info!(
        "Black-cell report: {} cells total | fog {} | unrevealed={} | revealed-but-no-tile={}          | no-tile-anywhere={}",
        grid.cells.len(),
        if fog.is_some() { "ON" } else { "OFF" },
        unrevealed,
        missing_tile,
        missing_tile_anywhere,
    );
    if !unrevealed_samples.is_empty() {
        log::info!("  unrevealed sample (rx,ry): {unrevealed_samples:?}");
    }
    if !missing_samples.is_empty() {
        log::info!("  no-tile sample (rx,ry,tile_id,sub_tile): {missing_samples:?}");
    }
    if unrevealed == 0 && missing_tile == 0 {
        log::info!("  no cell is black for either reason — the black must come from elsewhere");
    }
}

#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_unit_inspector(state: &mut AppState) {
    state.diag.debug_unit_inspector = !state.diag.debug_unit_inspector;
    if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation)
    {
        // F10: sim owns the write; the app only requests the toggle.
        sim.set_debug_event_logging(state.diag.debug_unit_inspector);
        log::info!(
            "Debug unit inspector: {}",
            if state.diag.debug_unit_inspector {
                "ON"
            } else {
                "OFF"
            }
        );
    }
}

/// Toggle the PathGrid / terrain-cost debug overlay.
///
/// Beyond flipping `state.diag.debug_show_pathgrid`, this resets the per-overlay
/// SpeedType override to None when the overlay turns off, so reopening
/// the overlay defaults back to "auto from selected unit". Called by both
/// the F9/P hotkey and the dev overlay checkbox.
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_pathgrid_overlay(state: &mut AppState) {
    state.diag.debug_show_pathgrid = !state.diag.debug_show_pathgrid;
    if !state.diag.debug_show_pathgrid {
        state.diag.debug_terrain_cost_speed_type = None;
    }
    log::info!(
        "Debug terrain cost overlay: {}",
        if state.diag.debug_show_pathgrid {
            "ON"
        } else {
            "OFF"
        }
    );
}

/// Toggle the foot-path step overlay (Ctrl+Shift+O).
///
/// Independent of `toggle_pathgrid_overlay`: the terrain-cost wash and the
/// numbered route answer different questions, and the route is useful with the
/// cost grid off.
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_foot_path_overlay(state: &mut AppState) {
    state.diag.debug_show_foot_path = !state.diag.debug_show_foot_path;
    log::info!(
        "Debug foot-path step overlay: {}",
        if state.diag.debug_show_foot_path {
            "ON (blue=route, magenta=next step, yellow=arrival)"
        } else {
            "OFF"
        }
    );
}

/// Toggle debug pause (J hotkey / dev overlay).
///
/// On resume, local frame admission is re-anchored so elapsed modal time
/// cannot cause a catch-up frame.
#[cfg(feature = "dev-overlays")]
pub(crate) fn toggle_debug_pause(state: &mut AppState) {
    state.match_state.debug_pause = !state.match_state.paused();
    if !state.match_state.paused() {
        state.platform.frame_pacer.reset_for_immediate_frame();
    }
    log::info!(
        "Debug pause: {}",
        if state.match_state.paused() {
            "ON"
        } else {
            "OFF"
        }
    );
}

/// Handle one-shot gameplay hotkeys (called on key press, not held).
///
/// **Modifier matching is exact for all but five commands.** Every stock binding
/// names a precise modifier set, and a bare-key command is normally rejected
/// while Shift, Ctrl or Alt is held — so holding Ctrl to force-fire or Alt to
/// force-move and tapping a letter does nothing instead of firing Stop or
/// Deploy. The exceptions are the five classes that override
/// `CommandClass::AcceptsModifiers`; see `hotkeys::HotkeyCommand`. TypeSelect is
/// one of them, so Shift+T still reaches this tap/hold machinery.
///
/// Dev/debug functions live behind the Ctrl+Shift chord, which stock binds
/// nothing to, so bare keys stay free for stock game hotkeys.
pub(crate) fn handle_type_select_key_edge(
    state: &mut AppState,
    resolution: HotkeyResolution,
    physical_code: winit::keyboard::KeyCode,
    element_state: ElementState,
    repeat: bool,
) -> bool {
    if crate::app::input::campaign_input::is_locked(state) {
        return false;
    }
    let is_type_select = resolution == HotkeyResolution::Command(HotkeyCommand::TypeSelect);
    if element_state.is_pressed() {
        if !is_type_select {
            return false;
        }
        state
            .match_state
            .input
            .type_select
            .press(physical_code, std::time::Instant::now(), repeat);
        return true;
    }
    if !is_type_select && !state.match_state.input.type_select.owns_key(physical_code) {
        return false;
    }
    let execute_tap = state
        .match_state
        .input
        .type_select
        .release(physical_code, std::time::Instant::now());
    if execute_tap {
        execute_type_select_tap(state);
    }
    true
}

fn execute_type_select_tap(state: &mut AppState) {
    state.match_state.input.type_select.prepare_tap_scope();
    let result = {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        else {
            return;
        };
        let screen_order =
            crate::app::presentation::instances::tactical_screen_entity_encounter_order(state);
        let map_order = map_entity_creation_order(sim.entities());
        let current = selected_stable_ids_in_order(
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.rules(),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        let fog = (!state.match_state.sandbox_full_visibility).then_some(&sim.fog);
        compute_type_select_tap_with_playfield(
            sim.entities(),
            &screen_order,
            &map_order,
            &current,
            fog,
            preferred_local_owner_name(state).as_deref(),
            state.rules(),
            Some(&sim.interner),
            state.match_state.input.type_select.across_map,
            sim.playfield_bounds.is_some(),
        )
    };
    let outcome = result.outcome;
    let across_map = result.across_map;
    apply_selection_mutation(state, result.mutation, false, TYPE_SELECT_TAP_VOICE_POLICY);
    state
        .match_state
        .input
        .type_select
        .finish_tap(outcome, across_map);
    crate::app::input::messages::post_type_select_feedback(state, outcome.csf_key());
    // Native marks the tactical display dirty here but does not start action
    // lines. The visible-window event loop already requests a redraw from
    // `about_to_wait`, so the tap needs no duplicate redraw request.
    apply_selection_action_line_policy(state, TYPE_SELECT_TAP_ACTION_LINE_POLICY);
}

pub(crate) fn handle_hotkey_pressed(
    state: &mut AppState,
    resolution: HotkeyResolution,
    physical_code: winit::keyboard::KeyCode,
) {
    if crate::app::input::campaign_input::is_locked(state)
        && resolution != HotkeyResolution::Command(HotkeyCommand::Options)
    {
        return;
    }
    match resolution {
        HotkeyResolution::Command(command) => dispatch_retail_hotkey(state, command),
        HotkeyResolution::Fallback(HotkeyFallback::DiplomacyDialog) => {
            open_diplomacy_menu(state);
        }
        HotkeyResolution::Fallback(
            HotkeyFallback::ArrowLeft
            | HotkeyFallback::ArrowUp
            | HotkeyFallback::ArrowRight
            | HotkeyFallback::ArrowDown,
        ) => {}
        HotkeyResolution::Unhandled => {
            #[cfg(feature = "dev-overlays")]
            if KeyModifiers::from_modifiers_state(state.match_state.input.hotkey_modifiers)
                .dev_chord()
            {
                handle_dev_hotkey_pressed(state, physical_code);
            }
            // Builds without `dev-overlays` (release) carry no dev chord.
            #[cfg(not(feature = "dev-overlays"))]
            let _ = physical_code;
        }
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

#[path = "selection_navigation.rs"]
pub(crate) mod selection_navigation;

/// One `Set{Structure,Defense,Infantry,Unit}TabCommandClass::Execute`
/// (535D90/535E50/535F10/535FA0).
///
/// `StripClass::HasEntries` 6ABAC0 gates the switch: a strip with no retained
/// entry refuses the command outright, so the active tab does not move — VERA
/// reaches the same gate through the retained strip that also drives
/// `SidebarProjectionState::selectable_tab`. Structures and Defense then run
/// their completed-building tail; a tab that is already active still runs it,
/// because native only skips the switch, not the tail.
fn select_sidebar_tab_command(state: &mut AppState, index: i32) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let Some(tab) = state
        .match_state
        .match_presentation
        .sidebar_projection
        .selectable_tab(index)
    else {
        return;
    };
    state.match_state.match_presentation.active_sidebar_tab = tab;
    if matches!(tab, SidebarTab::Building | SidebarTab::Defense) {
        place_ready_building_for_tab(state, tab);
    }
}

/// The tail of the Structures and Defense tab commands: Building7342A0 and
/// Building7342B0 hand the tab's completed building to
/// `ObjectClass::FindFactory` 5F5C20 (0,0) and `HouseClass::Begin_Building_
/// Placement` 4FB840. Native registers exactly one such building per structure
/// tab (`BuildingClass::SetSidebarTabObject` 734250 by the type's BuildCat),
/// which is this owner's ready building for that tab.
fn place_ready_building_for_tab(state: &mut AppState, tab: SidebarTab) {
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let ready_target = {
        let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
            return;
        };
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let Some(owner_id) = sim.interner.get(&owner_name) else {
            return;
        };
        crate::sim::production::ready_buildings_for_owner(sim, rules, &owner_name)
            .into_iter()
            .find(|ready| SidebarTab::for_category(ready.queue_category) == tab)
            .and_then(|ready| {
                let held = crate::sim::production::ready_building_entity_for_owner(
                    sim,
                    rules,
                    owner_id,
                    ready.type_id,
                )?;
                // Native hands Manual_Place4FB840 the object's Factory
                // 5F5C20(0,0), and 4FB840 refuses a null one. VERA's placement
                // owner takes no factory, so this lookup is admission only.
                crate::sim::production::object_find_factory(sim, rules, held, false, false)?;
                Some((
                    owner_id,
                    held,
                    sim.interner.resolve(ready.type_id).to_string(),
                ))
            })
    };
    let Some((owner_id, held, held_type)) = ready_target else {
        return;
    };
    if state
        .match_state
        .input
        .begin_building_placement(&held_type, held, owner_id)
    {
        // ManualPlace4FB881 calls UnselectAll48DC90 only on entry.
        queue_selection_snapshot_command(state, Vec::new(), false);
        let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
        gadgets.disable_selection_modes(false);
    }
}

fn dispatch_retail_hotkey(state: &mut AppState, command: HotkeyCommand) {
    match command {
        HotkeyCommand::HealthNav => selection_navigation::execute_category_navigation(
            state,
            crate::app::types::CategoryNavigationKind::Health,
        ),
        HotkeyCommand::CombatantSelect => selection_navigation::execute_combatant_selection(state),
        HotkeyCommand::NextObject => selection_navigation::execute_object_navigation(
            state,
            selection_navigation::ObjectDirection::Next,
        ),
        HotkeyCommand::PreviousObject => selection_navigation::execute_object_navigation(
            state,
            selection_navigation::ObjectDirection::Previous,
        ),
        HotkeyCommand::CursorCheat => {
            // 537EF0: flag only; the next mouse move refreshes the tooltip.
            state.match_state.input.cursor_coordinates =
                !state.match_state.input.cursor_coordinates;
        }
        HotkeyCommand::StopObject => queue_stop_for_selected(state),
        HotkeyCommand::DeployObject => queue_deploy_undeploy_for_selected(state),
        HotkeyCommand::GuardObject => queue_guard_for_selected(state),
        // Set{Structure,Defense,Infantry,Unit}TabCommandClass::Execute
        // 535D90/535E50/535F10/535FA0: the target strip must hold an entry, and
        // the two structure tabs then place their completed building.
        HotkeyCommand::StructureTab => select_sidebar_tab_command(state, 0),
        HotkeyCommand::DefenseTab => select_sidebar_tab_command(state, 1),
        HotkeyCommand::InfantryTab => select_sidebar_tab_command(state, 2),
        HotkeyCommand::UnitTab => select_sidebar_tab_command(state, 3),
        // TypeSelect owns both edges in `handle_type_select_key_edge`.
        HotkeyCommand::TypeSelect => {}
        HotkeyCommand::ToggleRepair => apply_sidebar_action(state, SidebarAction::ToggleRepairMode),
        HotkeyCommand::ToggleSell => apply_sidebar_action(state, SidebarAction::ToggleSellMode),
        HotkeyCommand::CenterBase => jump_camera_to_base(state),
        HotkeyCommand::CenterOnRadarEvent => {
            // Native's eight-cell review ring lives in RadarClass and holds
            // the cell of every accepted event, whatever its type.
            let event = state
                .match_state
                .match_presentation
                .minimap
                .as_mut()
                .and_then(|minimap| minimap.cycle_radar_event(std::time::Instant::now()));
            if let Some((rx, ry)) = event {
                crate::app::input::camera::center_camera_on_cell(state, rx, ry);
            }
        }
        HotkeyCommand::ScreenCapture => {
            state.match_state.input.retail_screenshot_requested = true;
            state.platform.window.request_redraw();
        }
        HotkeyCommand::View(slot) => crate::app::input::camera::recall_view_bookmark(state, slot),
        HotkeyCommand::SetView(slot) => crate::app::input::camera::set_view_bookmark(state, slot),
        HotkeyCommand::TeamSelect(slot) => handle_control_group_command(state, slot, None),
        HotkeyCommand::TeamAddSelect(slot) => {
            handle_control_group_command(state, slot, Some(GroupPressAction::AddToSelection))
        }
        HotkeyCommand::TeamCreate(slot) => {
            handle_control_group_command(state, slot, Some(GroupPressAction::Assign))
        }
        HotkeyCommand::TeamCenter(slot) => {
            handle_control_group_command(state, slot, Some(GroupPressAction::Center))
        }
        // Native SidebarUp/Down execute the same one-row saturated sidebar
        // owner as wheel input (up=1, down=0).
        HotkeyCommand::SidebarUp => sidebar_wheel_scroll(state, 1.0),
        HotkeyCommand::SidebarDown => sidebar_wheel_scroll(state, -1.0),
        HotkeyCommand::Options => handle_options_hotkey(state),
        HotkeyCommand::CenterView => crate::app::input::camera::center_view_on_selection(state),
        HotkeyCommand::Follow => crate::app::input::camera::toggle_follow_target(state),
        HotkeyCommand::PlanningMode => crate::app::input::planning_mode::toggle(state),
        HotkeyCommand::Delete => crate::app::input::planning_mode::delete_at_cursor(state),
        HotkeyCommand::ScatterObject => queue_scatter_for_selected(state),
        HotkeyCommand::VeterancyNav => selection_navigation::execute_category_navigation(
            state,
            crate::app::types::CategoryNavigationKind::Veterancy,
        ),
        // UNCHECKED residual, all still no-ops. Three of them are audio
        // triggers whose rules keys VERA already parses or could:
        // `PlaceBeacon` should place the beacon and play `[AudioVisual]
        // PlaceBeaconSound` (`Rules+0x1CC`, stock `BeaconPlaced`),
        // `AllToCheer` should run the cheer animation and play `CheerSound`
        // (`Rules+0x1C8`, stock `Cheer`), and `Taunt(n)` should broadcast the
        // multiplayer taunt sample. None of their native command handlers was
        // read this session, so no mapping is asserted and nothing is wired.
        // Trigger: pressing the bound key. Player effect: the key does
        // nothing. Frequency: rare in skirmish — all three are multiplayer
        // social commands. Downstream risk: beacons and taunts are network
        // messages, so they belong with `net/`, not with this row.
        HotkeyCommand::ToggleAlliance
        | HotkeyCommand::PlaceBeacon
        | HotkeyCommand::AllToCheer
        | HotkeyCommand::PageUser
        | HotkeyCommand::Taunt(_) => {}
    }
}

/// Native command-bar IDs use the same command endpoints as keyboard input.
/// Team right-click clears its group (6D0660); the other source actions reuse
/// the existing input owners rather than introducing a second gameplay path.
pub(crate) fn dispatch_command_bar(state: &mut AppState, command: usize, right: bool) {
    if crate::app::input::campaign_input::is_locked(state) {
        return;
    }
    let action = match command {
        0..=2 => {
            let group = command + 1;
            if right {
                if let Some(members) = state.match_state.input.control_groups.get_mut(group) {
                    members.clear();
                }
                return;
            }
            HotkeyCommand::TeamSelect(group)
        }
        3 => {
            execute_type_select_tap(state);
            return;
        }
        4 => HotkeyCommand::DeployObject,
        5 => {
            state.match_state.input.queued_order_mode = OrderMode::AttackMove;
            return;
        }
        6 => HotkeyCommand::GuardObject,
        7 => HotkeyCommand::PlaceBeacon,
        8 => HotkeyCommand::StopObject,
        9 => {
            crate::app::input::planning_mode::toggle(state);
            return;
        }
        10 => HotkeyCommand::AllToCheer,
        _ => return,
    };
    dispatch_retail_hotkey(state, action);
}

// Native 653952 -> GameState1 -> 48C9BA/4F10E0 opens the pause menu.
// The button and ordinary Options hotkey share this modal entry.
fn open_pause_menu(state: &mut AppState) {
    crate::app::App::enter_in_game_menu_state(state, crate::ui::pause_menu::InGameMenuState::Menu);
    log::info!("Game paused");
}

fn open_diplomacy_menu(_state: &mut AppState) {
    // 6538FE -> GameState8 (diplomacy), or9 (mission briefing in mode0).
    // Existing diplomacy hotkey boundary: the actual modal is not implemented.
    // Keep this residual explicit; this increment establishes its gadget only.
}

fn handle_options_hotkey(state: &mut AppState) {
    // Only Escape reaches a hotkey while paused, and the in-game menu takes
    // Escape whenever it is open, so the only pause seen here is the debug one.
    debug_assert!(!state.match_state.match_presentation.in_game_menu.is_open());
    if state.match_state.debug_pause {
        state.match_state.debug_pause = false;
        state.platform.frame_pacer.reset_for_immediate_frame();
        log::info!("Game resumed");
    } else {
        // Options535BE0/5372D0 and unconsumed Escape55E079 both request
        // GameState1. Deactivate683EB0/4AEAD0 tears down drag/capture without
        // dropping current building placement or repair/sell. Source:
        // tools/sidebar_oracle/cameo_order.md, ready-building histories.
        open_pause_menu(state);
    }
}

/// Dev/debug hotkeys require Ctrl+Shift, a chord stock binds to no command.
///
/// Gated on `dev-overlays`: every arm here is a diagnostic, a sandbox cheat or
/// a developer shortcut, and none of them is reachable from the retail UI path.
#[cfg(feature = "dev-overlays")]
fn handle_dev_hotkey_pressed(state: &mut AppState, code: winit::keyboard::KeyCode) {
    match code {
        // The hotkey-help overlay is VERA-only and used to sit on bare F1, which
        // stock YR binds to the first camera bookmark. Moved onto the dev chord,
        // which stock binds nothing to.
        #[cfg(feature = "dev-ui")]
        KeyCode::F1 => {
            state.match_state.match_presentation.show_hotkey_help =
                !state.match_state.match_presentation.show_hotkey_help;
        }
        KeyCode::KeyM => {
            crate::app::persistence::commands::quicksave(state);
        }
        KeyCode::KeyN => {
            crate::app::persistence::commands::quickload(state);
        }
        KeyCode::F5 => {
            // Diagnostic shortcut shares the retail browser and modal owner.
            crate::app::App::open_saved_game_browser(
                state,
                crate::ui::skirmish_shell::SavedSeedMode::Load,
            );
        }
        // Interim order-mode arms until the stock click modifiers
        // (Ctrl+Shift+click attack move, beacon key) are implemented.
        KeyCode::KeyA => {
            state.match_state.input.queued_order_mode = OrderMode::AttackMove;
            log::info!("Order mode armed: AttackMove");
        }
        KeyCode::KeyB => {
            state.match_state.input.queued_order_mode = OrderMode::Move;
        }
        KeyCode::KeyL => {
            state.diag.debug_show_cell_grid = !state.diag.debug_show_cell_grid;
            log::info!(
                "Debug cell grid overlay: {}",
                if state.diag.debug_show_cell_grid {
                    "ON (blue=terrain, yellow=overlay)"
                } else {
                    "OFF"
                }
            );
        }
        KeyCode::KeyK => {
            state.diag.debug_show_heightmap = !state.diag.debug_show_heightmap;
            log::info!(
                "Debug height map overlay: {}",
                if state.diag.debug_show_heightmap {
                    "ON (brighter = higher elevation, blue = bridge deck)"
                } else {
                    "OFF"
                }
            );
        }
        KeyCode::F9 | KeyCode::KeyP => {
            toggle_pathgrid_overlay(state);
        }
        KeyCode::KeyO => {
            toggle_foot_path_overlay(state);
        }
        KeyCode::BracketRight => {
            if state.diag.debug_show_pathgrid {
                let current =
                    crate::app::diagnostics::debug_overlays::resolve_debug_speed_type(state);
                let next = current.cycle_next();
                state.diag.debug_terrain_cost_speed_type = Some(next);
                log::info!("Terrain cost overlay: {}", next.name());
            }
        }
        KeyCode::BracketLeft => {
            if state.diag.debug_show_pathgrid {
                let current =
                    crate::app::diagnostics::debug_overlays::resolve_debug_speed_type(state);
                let prev = current.cycle_prev();
                state.diag.debug_terrain_cost_speed_type = Some(prev);
                log::info!("Terrain cost overlay: {}", prev.name());
            }
        }
        KeyCode::F10 | KeyCode::KeyV => {
            state.match_state.sandbox_full_visibility = !state.match_state.sandbox_full_visibility;
            log::info!(
                "Fog of war: {}",
                if state.match_state.sandbox_full_visibility {
                    "OFF (full visibility)"
                } else {
                    "ON"
                }
            );
        }
        KeyCode::F8 => {
            report_black_cell_causes(state);
        }
        KeyCode::KeyX => {
            toggle_unit_inspector(state);
        }
        KeyCode::KeyJ => {
            toggle_debug_pause(state);
        }
        KeyCode::Period if state.match_state.paused() => {
            state.diag.debug_frame_step_requested = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::{WheelAction, wheel_action, wheel_scrolled_row};

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

    /// The native test is a signed less-than against zero, so a zero delta goes
    /// up rather than doing nothing.
    #[test]
    fn zero_delta_scrolls_up() {
        assert_eq!(wheel_action(0.0), WheelAction::SidebarUp);
    }

    #[test]
    fn rows_move_one_at_a_time_and_saturate_at_both_ends() {
        assert_eq!(wheel_scrolled_row(0, 4, WheelAction::SidebarDown), 1);
        assert_eq!(wheel_scrolled_row(3, 4, WheelAction::SidebarDown), 4);
        // Refuses to move past the strip's computed capacity.
        assert_eq!(wheel_scrolled_row(4, 4, WheelAction::SidebarDown), 4);
        assert_eq!(wheel_scrolled_row(2, 4, WheelAction::SidebarUp), 1);
        // Refuses to move above row 0.
        assert_eq!(wheel_scrolled_row(0, 4, WheelAction::SidebarUp), 0);
        // A strip that fits entirely on screen cannot scroll at all.
        assert_eq!(wheel_scrolled_row(0, 0, WheelAction::SidebarDown), 0);
    }
}

/// The modifier bits a hotkey press carries.
///
/// The engine packs Shift, Ctrl and Alt into the key value as separate bits and
/// the command dispatcher matches the whole value, so every binding names an
/// exact modifier set. A command bound to a bare key is *rejected* while any
/// modifier is held — the base "does this command accept a modified form?"
/// predicate returns false for every stock command — and the dispatcher then
/// looks for a binding of the full chord instead. That is why holding Ctrl to
/// force-fire and tapping a letter does nothing in retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct KeyModifiers {
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
}

impl KeyModifiers {
    #[cfg(feature = "dev-overlays")]
    fn from_modifiers_state(modifiers: winit::keyboard::ModifiersState) -> Self {
        Self {
            shift: modifiers.shift_key(),
            ctrl: modifiers.control_key(),
            alt: modifiers.alt_key(),
        }
    }

    /// No modifier held — the only state in which a bare-key binding fires.
    pub(crate) fn none(self) -> bool {
        !self.shift && !self.ctrl && !self.alt
    }

    pub(crate) fn any(self) -> bool {
        !self.none()
    }

    pub(crate) fn only_shift(self) -> bool {
        self.shift && !self.ctrl && !self.alt
    }

    pub(crate) fn only_ctrl(self) -> bool {
        self.ctrl && !self.shift && !self.alt
    }

    pub(crate) fn only_alt(self) -> bool {
        self.alt && !self.shift && !self.ctrl
    }

    /// The VERA-internal dev chord. Stock binds nothing to Ctrl+Shift, so it
    /// collides with no retail hotkey.
    #[cfg(any(feature = "dev-overlays", test))]
    pub(crate) fn dev_chord(self) -> bool {
        self.ctrl && self.shift && !self.alt
    }
}

pub(crate) fn is_shift_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.shift_key()
}

pub(crate) fn is_ctrl_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.control_key()
}

/// Return `true` if either Alt key is currently held.
///
/// Used in order resolution to detect Alt+Ctrl = attack-move (NOT force-fire),
/// matching gamemd's `What_Action_OnCell` Alt-overrides-Ctrl rule.
pub(crate) fn is_alt_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.alt_key()
}

/// Read the player-side selection vector in native order. While a selection
/// command is queued the ledger is already the newest local state; after the
/// sim tick, reconciliation trusts the committed selected bits and admits any
/// lifecycle-transferred selection that was not issued by input.
pub(crate) fn selected_stable_ids_in_order(
    sim: Option<&crate::sim::world::Simulation>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selection_order: &[u64],
    selection_order_pending: bool,
) -> Vec<u64> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    let mut ordered = Vec::new();
    for &id in selection_order {
        if let Some(entity) = sim.entities().get(id)
            && (selection_order_pending || entity.selection.is_selected())
        {
            ordered.push(id);
        }
    }
    if selection_order_pending {
        ordered
    } else {
        admit_missing_selected_ids(ordered, sim, rules)
    }
}

/// Selection membership is a transient index, never an iteration authority.
/// The ledger keeps prior order; lifecycle transfers still enter in entity-store
/// order, with the same native front/back insertion as input selection. A deque
/// avoids shifting the whole list for each missing armed object.
fn admit_missing_selected_ids(
    ordered: Vec<u64>,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<u64> {
    let mut members: HashSet<_> = ordered.iter().copied().collect();
    let mut ordered = VecDeque::from(ordered);
    for entity in sim.entities().values() {
        if entity.selection.is_selected() && members.insert(entity.stable_id()) {
            insert_selected_id(&mut ordered, entity.stable_id(), sim, rules);
        }
    }
    ordered.into()
}

fn selection_membership_committed(sim: &crate::sim::world::Simulation, ordered: &[u64]) -> bool {
    let members: HashSet<_> = ordered.iter().copied().collect();
    let mut committed_count = 0;
    sim.entities()
        .values()
        .filter(|entity| entity.selection.is_selected())
        .all(|entity| {
            committed_count += 1;
            members.contains(&entity.stable_id())
        })
        && committed_count == ordered.len()
}

/// Synchronize the app ledger after the due selection commands and lifecycle
/// removals have committed for this frame.
pub(crate) fn reconcile_selection_order_after_sim(state: &mut AppState) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        state.match_state.input.selection_order.clear();
        state.match_state.input.selection_order_pending = false;
        state.match_state.input.category_navigation = Default::default();
        state.match_state.input.type_select.reset_scope();
        return;
    };
    reconcile_selection_order_for_sim(
        &mut state.match_state.input,
        &runtime.simulation,
        Some(&runtime.resources.rules),
    );
}

/// Reconcile the existing input owner against its bound simulation. Kept
/// independent of the window so lifecycle boundaries use this same path.
fn reconcile_selection_order_for_sim(
    input: &mut crate::app::input::state::MatchInputState,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    // Navigation733160 consumes actual pointer expiry through LifecycleOutput,
    // independently of this membership ledger. Alive/Health/Deselect alone
    // do not remove an entry from its retained vector.
    let (order, pending) = selection_order_for_world(
        sim,
        rules,
        &input.selection_order,
        input.selection_order_pending,
    );
    input.selection_order = order;
    input.selection_order_pending = pending;
}

/// Shared reconciliation decision for the live input ledger and a saved
/// local-view record. The simulation's command queue remains authoritative;
/// observing a save boundary neither drains nor applies queued commands.
pub(crate) fn selection_order_for_world(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    order: &[u64],
    pending: bool,
) -> (Vec<u64>, bool) {
    if pending && sim.has_pending_selection_commands() {
        return (
            order
                .iter()
                .copied()
                .filter(|id| {
                    sim.entities()
                        .get(*id)
                        .is_some_and(|entity| entity.lifecycle.object_alive())
                })
                .collect(),
            true,
        );
    }
    // MainTick55D8AB/55D8B4 commits local Select before Logic55DC9E.
    // A consumed snapshot may have been refused or then deselected by
    // ObjectDestroy/Conceal. Trust committed bits once the queue is drained,
    // rather than waiting forever for those bits to equal the provisional
    // ledger. Deselect5F44A0/expiry733160 do not reset mode or map scope.
    let reconciled: Vec<u64> = order
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| entity.selection.is_selected())
        })
        .collect();
    let reconciled = admit_missing_selected_ids(reconciled, sim, rules);
    debug_assert!(selection_membership_committed(sim, &reconciled));
    (reconciled, false)
}

fn apply_selection_mutation(
    state: &mut AppState,
    mutation: SelectionMutation,
    reset_type_select_scope: bool,
    voice_policy: SelectionVoicePolicy,
) -> bool {
    if !mutation.clear && mutation.deselect.is_empty() && mutation.select.is_empty() {
        return false;
    }
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return false;
    };
    let current = selected_stable_ids_in_order(
        Some(sim),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let SelectionMutationEffects {
        ordered,
        successful_adds,
        follow_target,
        native_selection_mode_reset,
    } = resolve_selection_mutation(
        sim,
        state.rules(),
        current,
        state.match_state.input.follow_target,
        state.armed_building_type().is_some(),
        &mutation,
    );
    state.match_state.input.follow_target = follow_target;

    if reset_type_select_scope {
        state.match_state.input.type_select.reset_scope();
    } else if native_selection_mode_reset {
        state
            .match_state
            .input
            .type_select
            .note_successful_selection_mutation(false);
    }
    for id in selection_voice_recipients(
        voice_policy,
        state.match_state.input.selection_voice_enabled,
        &successful_adds,
    ) {
        emit_selection_voice(state, *id);
    }
    state.match_state.input.selection_order = ordered.clone();
    state.match_state.input.selection_order_pending = true;
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    schedule_command(
        state,
        &owner,
        Command::Select {
            entity_ids: ordered,
            additive: !mutation.clear,
        },
    );
    true
}

/// Object5F44A0/5F4520 effects on the immediate local selection ledger. This
/// is also the read-only native-comparison seam; the command receiver commits
/// the final membership through the same Simulation admission owner.
struct SelectionMutationEffects {
    ordered: Vec<u64>,
    successful_adds: Vec<u64>,
    follow_target: Option<u64>,
    native_selection_mode_reset: bool,
}

fn resolve_selection_mutation(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    current: Vec<u64>,
    mut follow_target: Option<u64>,
    placement_armed: bool,
    mutation: &SelectionMutation,
) -> SelectionMutationEffects {
    let deselected: HashSet<_> = mutation.deselect.iter().copied().collect();
    let mut native_selection_mode_reset = mutation.clear;
    let mut ordered: VecDeque<_> = current
        .into_iter()
        .filter(|id| {
            let remove = mutation.clear || deselected.contains(id);
            if remove {
                // Deselect's Follow clear precedes Select, even for a singleton
                // that will immediately be reselected. A final-set diff loses it.
                //5F44A0 itself retains SelectionMode; UnselectAll and a
                // successful new Select are the reset callers.
                if follow_target == Some(*id) {
                    follow_target = None;
                }
            }
            !remove
        })
        .collect();
    let mut members: HashSet<_> = ordered.iter().copied().collect();
    let mut successful_adds = Vec::new();
    for &id in &mutation.select {
        if placement_armed || members.contains(&id) || !sim.can_select_object(id, rules) {
            continue;
        }
        members.insert(id);
        successful_adds.push(id);
        native_selection_mode_reset = true;
        insert_selected_id(&mut ordered, id, sim, rules);
    }
    SelectionMutationEffects {
        ordered: ordered.into(),
        successful_adds,
        follow_target,
        native_selection_mode_reset,
    }
}

fn insert_selected_id(
    ordered: &mut VecDeque<u64>,
    id: u64,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    let positive_damage_primary = sim.entities().get(id).is_some_and(|entity| {
        let type_id = sim.interner.resolve(entity.type_ref());
        rules
            .and_then(|rules| rules.object(type_id))
            .and_then(|object| object.primary())
            .and_then(|weapon_id| rules.and_then(|rules| rules.weapon(weapon_id)))
            .is_some_and(|weapon| weapon.damage > 0)
    });
    insert_selected_id_by_role(ordered, id, positive_damage_primary);
}

// Active-retail ObjectClass::Select5F4520 reads type+C9C at5F461F,
// prepends at5F468C/5F469A, otherwise appends at5F470E. ReadINI715793..7157AE
// derives that byte from Primary(+898)->Damage(+A4)>0. No RNG/timer here;
// successful-add voices remain in candidate order outside this ordering helper.
fn insert_selected_id_by_role(ordered: &mut VecDeque<u64>, id: u64, positive_damage_primary: bool) {
    if positive_damage_primary {
        ordered.push_front(id);
    } else {
        ordered.push_back(id);
    }
}

fn selection_voice_recipients(
    policy: SelectionVoicePolicy,
    voice_enabled: bool,
    successful_adds: &[u64],
) -> &[u64] {
    if !voice_enabled || policy == SelectionVoicePolicy::Suppressed {
        return &[];
    }
    match policy {
        SelectionVoicePolicy::FirstAdded => &successful_adds[..successful_adds.len().min(1)],
        SelectionVoicePolicy::EveryAdded => successful_adds,
        SelectionVoicePolicy::Suppressed => &[],
    }
}

#[cfg(test)]
mod item83_selection_order_tests {
    use super::{
        HELD_TYPE_SELECT_VOICE_POLICY, ORDINARY_SELECTION_ACTION_LINE_POLICY,
        ORDINARY_SELECTION_VOICE_POLICY, TYPE_SELECT_TAP_ACTION_LINE_POLICY,
        TYPE_SELECT_TAP_VOICE_POLICY, apply_selection_action_line_policy_at_frame,
        insert_selected_id_by_role, selected_stable_ids_in_order, selection_membership_committed,
        selection_voice_recipients,
    };
    use crate::app::presentation::target_lines::TargetLineState;
    use crate::audio::events::GameSoundEvent;
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;
    use std::collections::VecDeque;

    #[test]
    fn item83_positive_damage_technos_prepend_and_noncombat_technos_append() {
        let mut order = VecDeque::from([10]);
        insert_selected_id_by_role(&mut order, 20, true);
        insert_selected_id_by_role(&mut order, 30, true);
        insert_selected_id_by_role(&mut order, 40, false);
        assert_eq!(order, [30, 20, 10, 40]);
    }

    fn mixed_selection_fixture() -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=ARMED\n1=PLAIN\n[ARMED]\nPrimary=GUN\n[GUN]\nDamage=10\n[PLAIN]\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        for (id, type_name, selected) in [
            (10, "ARMED", true),
            (20, "PLAIN", true),
            (30, "ARMED", true),
            (40, "ARMED", false),
            (50, "PLAIN", true),
        ] {
            let mut entity = GameEntity::test_default(id, type_name, "Americans", 10, 20);
            entity.set_type_ref_for_test(sim.interner.intern(type_name));
            entity.set_owner_for_test(sim.interner.intern("Americans"));
            entity.selection.set_selected_for_test(selected);
            sim.entities_mut().insert(entity);
        }
        (sim, rules)
    }

    #[test]
    fn selection_read_keeps_pending_authority_and_native_recovery_order() {
        let (sim, rules) = mixed_selection_fixture();
        let read = |prior: &[u64], pending| {
            selected_stable_ids_in_order(Some(&sim), Some(&rules), prior, pending)
        };
        // Missing armed entries prepend in original scan order; missing plain
        // entries append. Removed IDs disappear, as do committed deselections.
        assert_eq!(read(&[20, 999, 40], false), [30, 10, 20, 50]);
        assert_eq!(read(&[], false), [30, 10, 20, 50]);
        // A complete ledger keeps its existing order regardless of role.
        assert_eq!(read(&[50, 10, 30, 20], false), [50, 10, 30, 20]);
        // Before the command commits, old selected bits cannot revive actors
        // omitted by input, or remove the newly selected actor40.
        assert_eq!(read(&[20, 999, 40], true), [20, 40]);
        assert!(read(&[], true).is_empty());
        assert!(selected_stable_ids_in_order(None, Some(&rules), &[10], false).is_empty());
    }

    #[test]
    fn selection_pending_clears_only_when_committed_membership_matches() {
        let (sim, _) = mixed_selection_fixture();
        assert!(selection_membership_committed(&sim, &[50, 10, 30, 20]));
        assert!(!selection_membership_committed(&sim, &[50, 10, 40, 20]));
        assert!(!selection_membership_committed(&sim, &[50, 10, 20]));
        assert!(!selection_membership_committed(&sim, &[50, 10, 30, 20, 40]));
        assert!(!selection_membership_committed(&sim, &[50, 10, 30, 30]));
    }

    #[test]
    fn item83_voice_policy_emits_every_tap_voice_in_candidate_order_only() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=E2\n\n\
             [VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [E1]\nStrength=100\nArmor=none\nSpeed=4\nVoiceSelect=VoiceOne\n\n\
             [E2]\nStrength=100\nArmor=none\nSpeed=4\nVoiceSelect=VoiceTwo\n",
        ))
        .expect("item83 voice rules");
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(owner);
        for (id, type_name) in [(1, "E1"), (2, "E2")] {
            let type_ref = sim.interner.intern(type_name);
            sim.entities_mut()
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    10 + id as u16,
                    10,
                    0,
                    0,
                    owner,
                    Health::new(100),
                    type_ref,
                    EntityCategory::Infantry,
                    0,
                    5,
                    false,
                ));
        }
        let candidate_order = [2, 1];
        let mut emitted = |policy| {
            selection_voice_recipients(policy, true, &candidate_order)
                .iter()
                .filter_map(|id| {
                    crate::app::match_runtime::sound_dispatch::selection_voice_event(
                        &mut sim, &rules, *id, true,
                    )
                })
                .map(|event| match event {
                    GameSoundEvent::UnitSelected { sound_id, .. } => sound_id,
                    other => panic!("unexpected selection event: {other:?}"),
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(
            emitted(TYPE_SELECT_TAP_VOICE_POLICY),
            vec!["VoiceTwo".to_string(), "VoiceOne".to_string()],
            "tap voices retain successful candidate/Select call order"
        );
        assert!(
            emitted(HELD_TYPE_SELECT_VOICE_POLICY).is_empty(),
            "held exact-type group-select suppresses the whole batch"
        );
        assert_eq!(
            emitted(ORDINARY_SELECTION_VOICE_POLICY),
            vec!["VoiceTwo".to_string()],
            "ordinary band/click policy retains only the first success"
        );
    }

    #[test]
    fn item83_type_select_tap_preserves_action_line_timer_while_mouse_selection_starts_it() {
        let mut target_lines = TargetLineState::default();

        apply_selection_action_line_policy_at_frame(
            &mut target_lines,
            10,
            TYPE_SELECT_TAP_ACTION_LINE_POLICY,
        );
        assert!(
            !target_lines.is_selected_action_active(10),
            "a short TypeSelect tap leaves a zero timer untouched"
        );

        apply_selection_action_line_policy_at_frame(
            &mut target_lines,
            10,
            ORDINARY_SELECTION_ACTION_LINE_POLICY,
        );
        assert!(
            target_lines.is_selected_action_active(10),
            "ordinary click/bandbox selection still opens the action-line window"
        );

        apply_selection_action_line_policy_at_frame(
            &mut target_lines,
            20,
            TYPE_SELECT_TAP_ACTION_LINE_POLICY,
        );
        assert!(
            !target_lines.is_selected_action_active(35),
            "a later TypeSelect tap preserves rather than restarts the existing timer"
        );
    }
}

fn queue_selection_snapshot_command(state: &mut AppState, selected_ids: Vec<u64>, additive: bool) {
    apply_selection_mutation(
        state,
        SelectionMutation {
            clear: !additive,
            select: selected_ids,
            ..Default::default()
        },
        true,
        ORDINARY_SELECTION_VOICE_POLICY,
    );
}

/// `StopCommandClass::Execute @ 0x00730EA0` issues Stop to each selected
/// object it may control (vt+0xA0) or that is active (vt+0x9C,
/// `0x00730ECD..0x00730EE3`), so an offline Robot Tank a click selected stays
/// put.
///
/// Planning refusal at730EF5 ->6FFE00 ->639FD0 ends this walk before
/// scheduling ordinary Stop. The closing Rules+720 sound is a residual.
fn queue_stop_for_selected(state: &mut AppState) {
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation);
    let mut selected_ids = selected_stable_ids_in_order(
        sim,
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    if let (Some(sim), Some(rules)) = (sim, state.rules()) {
        selected_ids.retain(|&id| {
            sim.techno_player_controllable(id, rules) || sim.techno_is_active(id, rules)
        });
    }
    if selected_ids.is_empty() {
        return;
    }
    if state.match_state.input.planning.active() {
        crate::app::input::planning_mode::refuse(
            state,
            crate::app::input::planning_mode::PlanningRefusal::Stop,
        );
        return;
    }
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    for entity_id in selected_ids {
        schedule_command(state, &owner, Command::Stop { entity_id });
    }
}

/// GuardObject536D00 and command-bar slot6 both execute730D60 immediately.
/// The input ledger already contains pending selection order; never sort it
/// or require that the selected bits have reached the simulation yet.
fn queue_guard_for_selected(state: &mut AppState) {
    if state.match_state.input.planning.active() {
        crate::app::input::planning_mode::refuse(
            state,
            crate::app::input::planning_mode::PlanningRefusal::Guard,
        );
        return;
    }
    let selected = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    for event in queue_area_guard_orders(
        &mut runtime.simulation,
        &runtime.resources.rules,
        &selected,
        state.match_state.input.selection_voice_enabled,
    ) {
        state.match_state.match_audio.sound_events.push(event);
    }
}

/// ScatterCommand536D80 ->730FE0 uses the same pending selection-order
/// owner as Guard. No per-object voice or selection-voice latch is involved.
fn queue_scatter_for_selected(state: &mut AppState) {
    let selected = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    //730FE0 only reaches6FFE00 for a controllable Techno. Use the
    // existing Scatter producer admission; an empty/refused selection
    // produces neither a HUD row nor an ordinary event.
    if state.match_state.input.planning.active() {
        let eligible = state
            .match_state
            .sim_runtime
            .as_ref()
            .is_some_and(|runtime| {
                selected.iter().any(|&id| {
                    runtime
                        .simulation
                        .scatter_key_command(id, &runtime.resources.rules)
                        .is_some()
                })
            });
        if eligible {
            crate::app::input::planning_mode::refuse(
                state,
                crate::app::input::planning_mode::PlanningRefusal::Scatter,
            );
        }
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    for sound in queue_scatter_orders(&mut runtime.simulation, &runtime.resources.rules, &selected)
    {
        state.match_state.match_audio.sound_events.push(sound);
    }
}

fn queue_scatter_orders(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
) -> Vec<crate::audio::events::GameSoundEvent> {
    use crate::audio::events::GameSoundEvent;

    if selected.is_empty() {
        return Vec::new();
    }
    let Some(owner) = sim.session.current_house else {
        return Vec::new();
    };
    let owner = sim.interner.resolve(owner).to_owned();
    for &entity_id in selected {
        let Some(payload) = sim.scatter_key_command(entity_id, rules) else {
            continue;
        };
        if crate::app::input::commands::schedule_command_in_sim(sim, &owner, payload).is_none() {
            return Vec::new();
        }
    }
    // 731032..73104A: one PlayAtPos750920, gain1, center2000, arg0.
    // A nonempty selection that admits no actor still produces this cue.
    // PlanningMode637AA0/639FD0 refusal is admitted by handle_scatter_key.
    // The existing offline ingress bypasses bounded native OutList admission;
    // its saturation and network-pump integration remain outside this path.
    rules
        .general
        .scatter_sound
        .as_ref()
        .map_or_else(Vec::new, |sound_id| {
            vec![GameSoundEvent::UiSound {
                sound_id: sound_id.clone(),
            }]
        })
}

/// The ordinary Guard selection loop730D79..730E8A. Simulation owns each
/// actor's eligibility/current Cell and the shared MegaMission codec. This
/// caller preserves list order, leaves the voice latch enabled for every
/// actor, and emits GuardSound even if a nonempty selection was all skipped.
/// Planning-mode637AA0/63A0D0 is handled before this ordinary producer.
fn queue_area_guard_orders(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    voices_enabled: bool,
) -> Vec<crate::audio::events::GameSoundEvent> {
    use crate::audio::events::GameSoundEvent;

    let mut sounds = Vec::new();
    if selected.is_empty() {
        return sounds;
    }
    let Some(owner) = sim.session.current_house else {
        return sounds;
    };
    let owner = sim.interner.resolve(owner).to_owned();
    for &entity_id in selected {
        let Some(payload) = sim.area_guard_key_command(entity_id, rules) else {
            continue;
        };
        // 6FFCBD..6FFDA5 precedes the event constructor/post. Native normal
        // 6FFBE0 still returns true after a full OutList rejects insertion.
        if let Some(event) = crate::app::match_runtime::sound_dispatch::default_order_voice_event(
            sim,
            rules,
            entity_id,
            voices_enabled,
        ) {
            sounds.push(event);
        }
        if crate::app::input::commands::schedule_command_in_sim(sim, &owner, payload).is_none() {
            // A missing Rust registration or unrepresentable record fails
            // closed. This invariant boundary is not native issuer false;
            // Planning Mode is the separate native false-return route.
            return sounds;
        }
    }
    if let Some(sound_id) = &rules.general.guard_sound {
        // 730E73..730E8A: PlayAtPos750920, centre2000, volume1, no handle.
        sounds.push(GameSoundEvent::UiSound {
            sound_id: sound_id.clone(),
        });
    }
    sounds
}

/// Deploy or undeploy selected entities. KeyD toggles:
/// - Selected unit with `DeploysInto` or `IsSimpleDeployer` → `Command::DeployMcv`
/// - Selected structure with `UndeploysInto` → `Command::UndeployBuilding` (ConYard → MCV)
fn queue_deploy_undeploy_for_selected(state: &mut AppState) {
    let selected_ids = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    if selected_ids.is_empty() {
        return;
    }
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    // Collect commands first to avoid borrow conflict with schedule_command.
    let local_owner = sim.interner.get(&owner);
    let commands = deploy_key_commands(sim, state.rules(), &selected_ids, local_owner);
    for cmd in commands {
        schedule_command(state, &owner, cmd);
    }
}

/// `Selection::IssueDeployCommand @ 0x00730AF0`, over the local player's
/// selection in order.
/// - A first selected building with occupants (`vt+0x408`, `0x004581F0`)
///   unloads them alone.
/// - Otherwise a first walk (`0x00730B8C..0x00730C0E`) notes whether any
///   deployer is deployed and any is not, berserk ones included.
/// - The issue walk then admits only an object the player may control
///   (vt+0xA0) or that is active (vt+0x9C, `0x00730C38`, `0x00730C46`), as
///   the Stop key does, skips a berserk object (`0x00730C82`) and, when the
///   first walk found both, every deployed deployer, so only the undeployed
///   ones deploy.
///
/// Its closing sound reads `Rules+0x72C`, which no INI key sets: the
/// constructor's -1 (`0x0066606A`) plays nothing.
///
/// RESIDUAL: the issue walk's other admission tests are not ported: bit 0 of
/// `+0x14`, `vt+0x310` (`0x00700D10`) and `vt+0x314` (`0x00700D50`).
/// `deploy_key_command`'s capability checks stand in for them, and their
/// equivalence is unchecked. Trigger: an object one of them refuses. Effect:
/// the deploy key orders it anyway. Frequency unknown.
fn deploy_key_commands(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selection: &[u64],
    local_owner: Option<crate::sim::intern::InternedId>,
) -> Vec<Command> {
    let first = selection.first().and_then(|&id| sim.entities().get(id));
    if let Some(building) = first.filter(|entity| {
        entity.category() == EntityCategory::Structure
            && entity
                .passenger_role
                .cargo()
                .is_some_and(|cargo| !cargo.is_empty())
    }) {
        return deploy_key_command(sim, rules, building.stable_id(), local_owner)
            .into_iter()
            .collect();
    }
    let deployed = |id: u64| deployer_is_deployed(sim, rules, id);
    let mut found_deployed = false;
    let mut found_undeployed = false;
    for &id in selection {
        match deployed(id) {
            Some(true) => found_deployed = true,
            Some(false) => found_undeployed = true,
            None => continue,
        }
        if found_deployed && found_undeployed {
            break;
        }
    }
    let mixed = found_deployed && found_undeployed;
    selection
        .iter()
        .copied()
        .filter(|&id| {
            rules.is_none_or(|rules| {
                sim.techno_player_controllable(id, rules) || sim.techno_is_active(id, rules)
            }) && sim
                .entities()
                .get(id)
                .is_some_and(|entity| !entity.berserk.is_active())
                && !(mixed && deployed(id) == Some(true))
        })
        .filter_map(|id| deploy_key_command(sim, rules, id, local_owner))
        .collect()
}

/// The deploy walk's deployer state: a `Deployer=` infantryman
/// (`0x005224E0`) is deployed in Doing 27..30 (`0x00522510`); an
/// `IsSimpleDeployer=` Unit is deployed or still deploying (`0x00746D90`:
/// `+0x6E0` or `+0x6E1`). Anything else is no deployer.
fn deployer_is_deployed(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    id: u64,
) -> Option<bool> {
    let entity = sim.entities().get(id)?;
    let object = sim.interner.object_type(entity.type_ref(), rules?)?;
    match entity.category() {
        EntityCategory::Infantry if object.deployer => Some(entity.infantry_deploy_doing()),
        EntityCategory::Unit if object.is_simple_deployer => Some(
            entity
                .mission_leaf
                .as_unit()
                .is_some_and(|leaf| leaf.deployed() != 0 || leaf.deploy_begin_active() != 0),
        ),
        _ => None,
    }
}

/// The deploy key's per-object decision. The command's historical
/// `DeployMcv` wire name also carries a SimpleDeployer toggle.
fn deploy_key_command(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    entity_id: u64,
    local_owner: Option<crate::sim::intern::InternedId>,
) -> Option<Command> {
    let entity = sim.entities().get(entity_id)?;
    let obj = rules.and_then(|rules| sim.interner.object_type(entity.type_ref(), rules));
    match entity.category() {
        EntityCategory::Structure => {
            // Absorbers and garrisons share unload admission; otherwise a
            // construction yard may take its undeploy action.
            super::transport_orders::unload_command(entity, obj, local_owner).or_else(|| {
                rules
                    .is_some_and(|rules| {
                        sim.should_show_undeploy_building_command(entity_id, rules)
                    })
                    .then_some(Command::UndeployBuilding { entity_id })
            })
        }
        EntityCategory::Infantry => obj
            .is_some_and(|object| object.deploy_fire)
            .then_some(Command::ToggleInfantryDeploy { entity_id }),
        _ => {
            // A loaded transport (including aircraft) keeps the unload action.
            super::transport_orders::unload_command(entity, obj, local_owner).or_else(|| {
                let object = obj?;
                let deployable = if super::context_order::is_simple_deploy_unit(entity, object) {
                    rules.is_some_and(|rules| sim.can_simple_deploy(entity_id, rules))
                } else {
                    object.deploys_into.is_some()
                };
                deployable.then_some(Command::DeployMcv { entity_id })
            })
        }
    }
}

/// Double-tap window for the centre-on-group shortcut, in milliseconds.
/// The engine compares `timeGetTime()` against the last recall stamp, so this
/// is wall clock and never sim state.
const GROUP_DOUBLE_TAP_MS: u128 = 800;

/// What a digit press resolves to before any state is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPressAction {
    /// Ctrl+digit — replace the slot's membership with the current selection.
    Assign,
    /// Shift+digit — add the slot's members to the current selection.
    AddToSelection,
    /// Alt+digit, or a bare double-tap — put the camera on the group.
    Center,
    /// Bare digit — clear the selection and select the slot's members.
    Recall,
}

/// Resolve a control-group digit press.
///
/// The four team commands are separate bindings — bare digit, Shift+digit,
/// Ctrl+digit, Alt+digit — and the dispatcher matches the modifier bits
/// exactly, so a two-modifier chord matches no binding and does nothing.
///
/// The double-tap arm is the subtle one: the recall routine only centres when
/// the current selection is *exactly* the group. It bails out on the first
/// group member that is unselected and on the first selected object outside the
/// group — two distinct tests, not "at least one member selected". After a
/// plain recall that condition holds, which is why the familiar double-tap
/// works; shift-clicking one extra unit between the taps makes the second tap
/// recall instead of centre. Only a plain recall stamps the timer.
fn control_group_press_action(
    modifiers: KeyModifiers,
    slot: usize,
    group: &[u64],
    selected: &[u64],
    last_press: Option<(usize, std::time::Duration)>,
) -> Option<GroupPressAction> {
    if modifiers.only_ctrl() {
        return Some(GroupPressAction::Assign);
    }
    if modifiers.only_shift() {
        return Some(GroupPressAction::AddToSelection);
    }
    if modifiers.only_alt() {
        return Some(GroupPressAction::Center);
    }
    if modifiers.any() {
        // Two modifiers at once: no binding carries that key value.
        return None;
    }
    let within_window = last_press.is_some_and(|(last_slot, elapsed)| {
        last_slot == slot && elapsed.as_millis() < GROUP_DOUBLE_TAP_MS
    });
    let selection_is_exactly_the_group = !group.is_empty()
        && group.iter().all(|id| selected.contains(id))
        && selected.iter().all(|id| group.contains(id));
    if within_window && selection_is_exactly_the_group {
        return Some(GroupPressAction::Center);
    }
    Some(GroupPressAction::Recall)
}

fn control_group_action_for_input(
    modifiers: KeyModifiers,
    slot: usize,
    group: &[u64],
    selected: &[u64],
    last_press: Option<(usize, std::time::Duration)>,
    sim: Option<&crate::sim::world::Simulation>,
) -> Option<GroupPressAction> {
    let eligible_selected = live_group_members(sim, selected);
    control_group_press_action(modifiers, slot, group, &eligible_selected, last_press)
}

/// Assign `ids` to `slot`, evicting them from every other slot.
///
/// Membership is a single group index stored on the object, so a unit belongs
/// to at most one group and re-grouping it silently drops it from the old one.
/// VERA keeps ten id lists, so the eviction is explicit here.
fn assign_control_group(groups: &mut [Vec<u64>], slot: usize, ids: Vec<u64>) {
    for (index, group) in groups.iter_mut().enumerate() {
        if index != slot {
            group.retain(|id| !ids.contains(id));
        }
    }
    groups[slot] = ids;
}

/// Apply TeamAssign's clear-then-set effects to the Rust group index.
/// TeamAssign731060 skips both effects for an ineligible owner, so those live
/// members keep their slot latent until their owner becomes eligible again.
fn reassign_control_group(
    groups: &mut [Vec<u64>],
    slot: usize,
    selected: &[u64],
    sim: &crate::sim::world::Simulation,
) {
    let retained_ineligible = groups[slot]
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| !sim.house_is_control_group_eligible(entity.owner()))
        })
        .collect::<Vec<_>>();
    let eligible_selected = live_group_members(Some(sim), selected);
    let mut target_members = retained_ineligible;
    target_members.extend(eligible_selected);
    assign_control_group(groups, slot, target_members);
}

/// Keep the current legal selection intact when Shift+digit adds eligible
/// control-group members. Ordinary click selection may include enemy objects.
fn append_group_members_to_selection(mut selected: Vec<u64>, group: &[u64]) -> Vec<u64> {
    selected.extend_from_slice(group);
    selected
}

/// Centre of a set of world points in leptons, with the single worst outlier
/// dropped once there are more than two of them — the engine's centre-on-
/// selection maths, which is a trimmed mean rather than a plain centroid so one
/// straggler cannot drag the view off the main body.
///
/// Ties on "farthest" keep the first candidate (comparison is strictly greater);
/// the original's tie-break is UNVERIFIED.
fn trimmed_centroid_leptons(points: &[(i64, i64)]) -> Option<(i64, i64)> {
    if points.is_empty() {
        return None;
    }
    let count = points.len() as i64;
    let sum = points
        .iter()
        .fold((0i64, 0i64), |acc, p| (acc.0 + p.0, acc.1 + p.1));
    let mean = (sum.0 / count, sum.1 / count);
    if points.len() <= 2 {
        return Some(mean);
    }
    let mut farthest = points[0];
    let mut farthest_dist = i64::MIN;
    for p in points {
        let (dx, dy) = (p.0 - mean.0, p.1 - mean.1);
        let dist = dx * dx + dy * dy;
        if dist > farthest_dist {
            farthest_dist = dist;
            farthest = *p;
        }
    }
    Some((
        (sum.0 - farthest.0) / (count - 1),
        (sum.1 - farthest.1) / (count - 1),
    ))
}

/// Leptons per cell.
const LEPTONS_PER_CELL: i64 = crate::util::lepton::LEPTONS_PER_CELL_I32 as i64;

/// Live members of a control group, in the sim's iteration order.
fn live_group_members(sim: Option<&crate::sim::world::Simulation>, group: &[u64]) -> Vec<u64> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    group
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| sim.house_is_control_group_eligible(entity.owner()))
        })
        .collect()
}

/// Put the camera on a group's trimmed centroid. Emits no command — the centre
/// arm must stay out of the lockstep stream.
fn center_camera_on_group(state: &mut AppState, group: &[u64]) {
    let points: Vec<(i64, i64)> = {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        else {
            return;
        };
        group
            .iter()
            .filter_map(|id| sim.entities().get(*id))
            .map(|e| {
                (
                    e.position.rx() as i64 * LEPTONS_PER_CELL + e.position.sub_x().to_num::<i64>(),
                    e.position.ry() as i64 * LEPTONS_PER_CELL + e.position.sub_y().to_num::<i64>(),
                )
            })
            .collect()
    };
    let Some((cx, cy)) = trimmed_centroid_leptons(&points) else {
        return;
    };
    let rx = (cx / LEPTONS_PER_CELL).clamp(0, u16::MAX as i64) as u16;
    let ry = (cy / LEPTONS_PER_CELL).clamp(0, u16::MAX as i64) as u16;
    crate::app::input::camera::center_camera_on_cell(state, rx, ry);
}

fn handle_control_group_command(
    state: &mut AppState,
    group_idx: usize,
    action_override: Option<GroupPressAction>,
) {
    if group_idx >= state.match_state.input.control_groups.len() {
        return;
    }
    let sim = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation);
    let group = state.match_state.input.control_groups[group_idx].clone();
    let selected = selected_stable_ids_in_order(
        sim,
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    // Only live members count towards "the selection is exactly the group" —
    // membership is derived by scanning live objects, so a dead unit has
    // already left its group.
    let live_group = live_group_members(sim, &group);
    let last_press = state
        .match_state
        .input
        .last_control_group_press
        .map(|(slot, at)| (slot, at.elapsed()));
    let action = action_override.unwrap_or_else(|| {
        control_group_action_for_input(
            KeyModifiers::default(),
            group_idx,
            &live_group,
            &selected,
            last_press,
            sim,
        )
        .expect("a bare team-select binding always resolves")
    });

    match action {
        GroupPressAction::Assign => {
            let Some(sim) = sim else {
                return;
            };
            reassign_control_group(
                &mut state.match_state.input.control_groups,
                group_idx,
                &selected,
                sim,
            );
        }
        GroupPressAction::Center => {
            // Alt+digit selects the group as well as centring; a bare
            // double-tap centres on a selection that already is the group, so
            // the same centring call covers both.
            if action_override == Some(GroupPressAction::Center) && !live_group.is_empty() {
                queue_selection_snapshot_command(state, live_group.clone(), false);
            }
            center_camera_on_group(state, &live_group);
        }
        GroupPressAction::AddToSelection => {
            if live_group.is_empty() {
                return;
            }
            let final_ids = append_group_members_to_selection(selected, &live_group);
            queue_selection_snapshot_command(state, final_ids, true);
        }
        GroupPressAction::Recall => {
            // A recall on an empty group still clears the selection: the
            // deselect-all runs before the select loop, unconditionally.
            state.match_state.input.last_control_group_press =
                Some((group_idx, std::time::Instant::now()));
            queue_selection_snapshot_command(state, live_group, false);
            apply_selection_action_line_policy(state, ORDINARY_SELECTION_ACTION_LINE_POLICY);
        }
    }
}

/// Apply the selection caller's action-line policy.
///
/// Band-box release, click-select and control-group recall all call the
/// engine's start-timer helper unconditionally, so the freshly selected units
/// flash their current orders for the 25-frame window. TypeSelect tap preserves
/// the prior timer instead.
fn apply_selection_action_line_policy(state: &mut AppState, policy: SelectionActionLinePolicy) {
    let Some(frame) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .map(|sim| sim.session.binary_frame)
    else {
        return;
    };
    apply_selection_action_line_policy_at_frame(
        &mut state.match_state.match_presentation.target_lines,
        frame,
        policy,
    );
}

fn apply_selection_action_line_policy_at_frame(
    target_lines: &mut crate::app::presentation::target_lines::TargetLineState,
    frame: u32,
    policy: SelectionActionLinePolicy,
) {
    if policy == SelectionActionLinePolicy::Start {
        target_lines.start_timer(frame);
    }
}

/// Emit the modeled VoiceSelect side effect for one successful Select call.
fn emit_selection_voice(state: &mut AppState, entity_id: u64) {
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    if let Some(event) = crate::app::match_runtime::sound_dispatch::selection_voice_event(
        &mut runtime.simulation,
        &runtime.resources.rules,
        entity_id,
        state.match_state.input.selection_voice_enabled,
    ) {
        state.match_state.match_audio.sound_events.push(event);
    }
}

/// Jump camera to the local player's base.
///
/// Priority: ConYard (structure with `UndeploysInto=`) → MCV (unit with `DeploysInto=`)
/// → multiplayer start waypoint 0 as fallback.
fn jump_camera_to_base(state: &mut AppState) {
    let owner = preferred_local_owner_name(state);
    let owner_name = owner.as_deref();

    // Collect the target cell from simulation entities before mutating state.
    let target: Option<(u16, u16)> = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(|sim| {
            let rules = state.rules();
            // First pass: look for a ConYard (structure with UndeploysInto=).
            let conyard = sim.entities().values().find(|e| {
                e.category() == EntityCategory::Structure
                    && owner_name
                        .is_none_or(|o| sim.interner.resolve(e.owner()).eq_ignore_ascii_case(o))
                    && rules
                        .and_then(|r| sim.interner.object_type(e.type_ref(), r))
                        .is_some_and(|o| o.undeploys_into.is_some())
            });
            if let Some(entity) = conyard {
                log::info!(
                    "H: jumping to ConYard {} at ({}, {})",
                    sim.interner.resolve(entity.type_ref()),
                    entity.position.rx(),
                    entity.position.ry()
                );
                return Some((entity.position.rx(), entity.position.ry()));
            }
            // Second pass: look for an MCV (unit with DeploysInto=).
            let mcv = sim.entities().values().find(|e| {
                e.category() != EntityCategory::Structure
                    && owner_name
                        .is_none_or(|o| sim.interner.resolve(e.owner()).eq_ignore_ascii_case(o))
                    && rules
                        .and_then(|r| sim.interner.object_type(e.type_ref(), r))
                        .is_some_and(|o| o.deploys_into.is_some())
            });
            if let Some(entity) = mcv {
                log::info!(
                    "H: jumping to MCV {} at ({}, {})",
                    sim.interner.resolve(entity.type_ref()),
                    entity.position.rx(),
                    entity.position.ry()
                );
                return Some((entity.position.rx(), entity.position.ry()));
            }
            log::info!(
                "H: no ConYard/MCV found (owner={:?}, entities={}, rules={})",
                owner_name,
                sim.entities().len(),
                rules.is_some()
            );
            None
        });

    if let Some((rx, ry)) = target {
        crate::app::input::camera::center_camera_on_cell(state, rx, ry);
        return;
    }

    // Fallback: jump to the first multiplayer start waypoint.
    if let Some(wp) = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|rt| rt.simulation.session.first_multiplayer_start())
    {
        log::info!(
            "H: falling back to start waypoint at ({}, {})",
            wp.rx,
            wp.ry
        );
        crate::app::input::camera::center_camera_on_cell(state, wp.rx, wp.ry);
    } else {
        log::info!("H: no base or start waypoint found");
    }
}

#[cfg(test)]
mod deploy_key_tests {
    use super::{deploy_key_command, deploy_key_commands};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::command::Command;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    use crate::sim::world::Simulation;

    /// A battle whose player is `Americans`, so the issue walk's control
    /// and activity tests can admit the fixture's objects.
    fn player_sim() -> Simulation {
        let mut sim = Simulation::new();
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(sim.interner.intern("Americans"));
        sim
    }

    #[test]
    fn simple_deployer_key_preserves_infantry_and_loaded_transport_actions() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=HELI\n1=TRANSPORT\n2=PLAIN\n\
             [AircraftTypes]\n0=AIRCRAFT\n[InfantryTypes]\n0=GI\n\
             [HELI]\nIsSimpleDeployer=yes\n\
             [TRANSPORT]\nIsSimpleDeployer=yes\nPassengers=5\n\
             [PLAIN]\nDeployer=yes\n\
             [AIRCRAFT]\nIsSimpleDeployer=yes\n\
             [GI]\nIsSimpleDeployer=yes\nDeployFire=yes\nDeployer=yes\n",
        ))
        .expect("deploy key rules");
        let mut sim = Simulation::new();
        let mut spawn = |type_id| {
            sim.spawn_object_limbo_at_height(type_id, "Americans", 10, 10, 0, 0, &rules)
                .expect("the deploy key fixture builds")
        };
        let heli = spawn("HELI");
        let transport = spawn("TRANSPORT");
        let plain = spawn("PLAIN");
        let aircraft = spawn("AIRCRAFT");
        let infantry = spawn("GI");
        let mut cargo = PassengerCargo::new(5, 0);
        cargo.board(999, 1);
        sim.entities_mut()
            .get_mut(transport)
            .unwrap()
            .passenger_role = PassengerRole::transport(cargo);

        assert_eq!(
            deploy_key_command(&sim, Some(&rules), heli, sim.session.current_house),
            Some(Command::DeployMcv { entity_id: heli })
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), transport, sim.session.current_house),
            Some(Command::UnloadPassengers {
                transport_id: transport
            })
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), infantry, sim.session.current_house),
            Some(Command::ToggleInfantryDeploy {
                entity_id: infantry
            })
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), aircraft, sim.session.current_house),
            None
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), plain, sim.session.current_house),
            None
        );
        sim.entities_mut()
            .get_mut(heli)
            .unwrap()
            .low_bridge_tube_state = Some(
            crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
                crate::map::tube_facts::TubeId(0),
                0,
                crate::sim::components::DriveCoord::cell(10, 10, 0),
            ),
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), heli, sim.session.current_house),
            None
        );
    }

    #[test]
    fn the_deploy_key_issues_unload_for_a_loaded_bio_reactor() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=YAPOWR\n[YAPOWR]\nStrength=1000\n\
             InfantryAbsorb=yes\nPassengers=5\n",
        ))
        .expect("Bio Reactor deploy-key rules");
        let mut sim = player_sim();
        let reactor = sim
            .spawn_object("YAPOWR", "Americans", 10, 10, 0, &rules)
            .expect("Bio Reactor builds");
        let mut cargo = PassengerCargo::new(5, 0);
        cargo.board(99, 1);
        sim.entities_mut().get_mut(reactor).unwrap().passenger_role =
            PassengerRole::transport(cargo);

        assert_eq!(
            deploy_key_commands(&sim, Some(&rules), &[reactor], sim.session.current_house),
            [Command::UnloadPassengers {
                transport_id: reactor
            }]
        );
    }

    /// `Selection::IssueDeployCommand`'s issue walk skips a berserk object
    /// (`0x00730C82`).
    #[test]
    fn the_deploy_key_skips_a_berserk_object() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=HELI\n[HELI]\nIsSimpleDeployer=yes\n",
        ))
        .expect("deploy key rules");
        let mut sim = player_sim();
        let heli = sim
            .spawn_object_limbo_at_height("HELI", "Americans", 10, 10, 0, 0, &rules)
            .expect("the deploy key fixture builds");
        assert_eq!(
            deploy_key_commands(&sim, Some(&rules), &[heli], sim.session.current_house),
            [Command::DeployMcv { entity_id: heli }]
        );
        sim.entities_mut()
            .get_mut(heli)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        assert!(
            deploy_key_commands(&sim, Some(&rules), &[heli], sim.session.current_house).is_empty()
        );
    }

    /// The issue walk admits an object the player may control (vt+0xA0) or
    /// that is active (vt+0x9C, `0x00730C38`, `0x00730C46`): an offline one
    /// is neither, and nor is another house's.
    #[test]
    fn the_deploy_key_skips_an_object_neither_controllable_nor_active() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=HELI\n[HELI]\nIsSimpleDeployer=yes\n",
        ))
        .expect("deploy key rules");
        let mut sim = player_sim();
        let mut spawn = |owner| {
            sim.spawn_object_limbo_at_height("HELI", owner, 10, 10, 0, 0, &rules)
                .expect("the deploy key fixture builds")
        };
        let (heli, theirs) = (spawn("Americans"), spawn("Russians"));
        assert_eq!(
            deploy_key_commands(
                &sim,
                Some(&rules),
                &[heli, theirs],
                sim.session.current_house
            ),
            [Command::DeployMcv { entity_id: heli }]
        );
        sim.entities_mut()
            .get_mut(heli)
            .unwrap()
            .deactivated
            .set_for_test(true);
        assert!(
            deploy_key_commands(&sim, Some(&rules), &[heli], sim.session.current_house).is_empty()
        );
    }

    /// The first walk (`0x00730B8C..0x00730C0E`) counts deployed and
    /// undeployed `Deployer=` infantry, berserk ones included; a mixed
    /// selection deploys only its undeployed members.
    #[test]
    fn a_mixed_selection_deploys_only_its_undeployed_deployers() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=GI\n[GI]\nDeployer=yes\nDeployFire=yes\n",
        ))
        .expect("deploy key rules");
        let mut sim = player_sim();
        let mut spawn = |rx: u16| {
            sim.spawn_object("GI", "Americans", rx, 5, 0, &rules)
                .expect("the deploy key fixture builds")
        };
        let (standing, deployed, gassed) = (spawn(3), spawn(4), spawn(5));
        sim.entities_mut()
            .get_mut(deployed)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(28)
            .unwrap();
        sim.entities_mut()
            .get_mut(gassed)
            .unwrap()
            .berserk
            .set_active_for_test(true);
        let toggle = |entity_id| Command::ToggleInfantryDeploy { entity_id };
        let commands = |selection: &[u64]| {
            deploy_key_commands(&sim, Some(&rules), selection, sim.session.current_house)
        };
        assert_eq!(commands(&[deployed]), [toggle(deployed)]);
        assert_eq!(commands(&[deployed, standing]), [toggle(standing)]);
        // The gassed man counts as undeployed but is not ordered.
        assert!(commands(&[gassed, deployed]).is_empty());
    }

    #[test]
    fn retail_siege_chopper_deploy_key_uses_the_existing_command_in_both_directions() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &battle.rules;
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
            .expect("the retail Siege Chopper builds");
        for (deployed, begin, reverse) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, false, true),
        ] {
            sim.entities_mut()
                .get_mut(id)
                .unwrap()
                .set_unit_simple_deploy_for_test(deployed, begin, reverse);
            assert_eq!(
                deploy_key_command(&sim, Some(rules), id, sim.session.current_house),
                Some(Command::DeployMcv { entity_id: id }),
            );
        }
    }
}

#[cfg(test)]
mod control_group_tests {
    use super::{
        GroupPressAction, KeyModifiers, append_group_members_to_selection, assign_control_group,
        control_group_action_for_input, control_group_press_action, live_group_members,
        reassign_control_group, trimmed_centroid_leptons,
    };
    use std::time::Duration;

    fn mods(shift: bool, ctrl: bool, alt: bool) -> KeyModifiers {
        KeyModifiers { shift, ctrl, alt }
    }

    const BARE: KeyModifiers = KeyModifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };

    #[test]
    fn ctrl_assigns_shift_adds_alt_centers_bare_recalls() {
        let group = [1u64, 2];
        let selected = [1u64, 2];
        assert_eq!(
            control_group_press_action(mods(false, true, false), 0, &group, &selected, None),
            Some(GroupPressAction::Assign)
        );
        assert_eq!(
            control_group_press_action(mods(true, false, false), 0, &group, &selected, None),
            Some(GroupPressAction::AddToSelection)
        );
        assert_eq!(
            control_group_press_action(mods(false, false, true), 0, &group, &selected, None),
            Some(GroupPressAction::Center)
        );
        assert_eq!(
            control_group_press_action(BARE, 0, &group, &selected, None),
            Some(GroupPressAction::Recall)
        );
    }

    /// Two modifiers at once produce a key value no binding carries, so the
    /// press does nothing at all — Ctrl+Shift+digit must not assign.
    #[test]
    fn two_modifiers_match_no_binding() {
        let group = [1u64];
        assert_eq!(
            control_group_press_action(mods(true, true, false), 0, &group, &group, None),
            None
        );
        assert_eq!(
            control_group_press_action(mods(false, true, true), 0, &group, &group, None),
            None
        );
    }

    #[test]
    fn double_tap_inside_the_window_centers() {
        let group = [1u64, 2, 3];
        assert_eq!(
            control_group_press_action(
                BARE,
                2,
                &group,
                &group,
                Some((2, Duration::from_millis(799)))
            ),
            Some(GroupPressAction::Center)
        );
    }

    #[test]
    fn double_tap_outside_the_window_recalls() {
        let group = [1u64, 2, 3];
        assert_eq!(
            control_group_press_action(
                BARE,
                2,
                &group,
                &group,
                Some((2, Duration::from_millis(800)))
            ),
            Some(GroupPressAction::Recall)
        );
    }

    #[test]
    fn a_different_slot_inside_the_window_recalls() {
        let group = [1u64];
        assert_eq!(
            control_group_press_action(
                BARE,
                3,
                &group,
                &group,
                Some((2, Duration::from_millis(10)))
            ),
            Some(GroupPressAction::Recall)
        );
    }

    #[test]
    fn double_tap_group_action_ignores_ineligible_selected_enemy() {
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let local = house(&mut sim, "Local", false, true);
        let enemy = house(&mut sim, "Enemy", false, false);
        group_entity(&mut sim, 10, local);
        group_entity(&mut sim, 11, enemy);
        let group = live_group_members(Some(&sim), &[10]);
        let raw_selected = [10, 11];

        assert_eq!(
            control_group_action_for_input(
                BARE,
                1,
                &group,
                &raw_selected,
                Some((1, Duration::from_millis(10))),
                Some(&sim),
            ),
            Some(GroupPressAction::Center),
            "an ineligible enemy selected by ordinary click is ignored by native exact-group comparison"
        );
    }

    /// The centre arm needs the selection to be *exactly* the group. One extra
    /// selected unit outside the group is one of the two bail-outs.
    #[test]
    fn an_extra_selected_unit_outside_the_group_recalls() {
        let group = [1u64, 2];
        let selected = [1u64, 2, 9];
        assert_eq!(
            control_group_press_action(
                BARE,
                1,
                &group,
                &selected,
                Some((1, Duration::from_millis(100)))
            ),
            Some(GroupPressAction::Recall)
        );
    }

    /// And the other bail-out: a group member that is not selected.
    #[test]
    fn a_group_member_left_unselected_recalls() {
        let group = [1u64, 2, 3];
        let selected = [1u64, 2];
        assert_eq!(
            control_group_press_action(
                BARE,
                1,
                &group,
                &selected,
                Some((1, Duration::from_millis(100)))
            ),
            Some(GroupPressAction::Recall)
        );
    }

    /// An empty group never centres, however fast the taps come.
    #[test]
    fn an_empty_group_recalls_rather_than_centering() {
        assert_eq!(
            control_group_press_action(BARE, 4, &[], &[], Some((4, Duration::from_millis(10)))),
            Some(GroupPressAction::Recall)
        );
    }

    /// Membership is one group index per object, so assigning evicts.
    #[test]
    fn assignment_evicts_the_units_from_their_previous_group() {
        let mut groups = vec![vec![1u64, 2, 3], vec![4u64], Vec::new()];
        assign_control_group(&mut groups, 1, vec![2, 4]);
        assert_eq!(groups[0], vec![1, 3]);
        assert_eq!(groups[1], vec![2, 4]);
    }

    fn house(
        sim: &mut crate::sim::world::Simulation,
        name: &str,
        is_human: bool,
        player_control: bool,
    ) -> crate::sim::intern::InternedId {
        let id = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(id, 0, None, is_human, 0, 10);
        house.player_control = player_control;
        sim.houses.insert(id, house);
        id
    }

    fn group_entity(
        sim: &mut crate::sim::world::Simulation,
        stable_id: u64,
        owner: crate::sim::intern::InternedId,
    ) {
        let type_ref = sim.interner.intern("MTNK");
        let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            stable_id,
            10,
            10,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            type_ref,
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            false,
        );
        sim.entities_mut().insert(entity);
    }

    #[test]
    fn team_assignment_and_recall_apply_owner_eligibility_without_erasing_latent_members() {
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let local = house(&mut sim, "Local", false, true);
        let ai = house(&mut sim, "AI", false, false);
        let human_only = house(&mut sim, "HumanOnly", true, false);
        let missing_house = sim.interner.intern("MissingHouse");
        for (stable_id, owner) in [
            (1, local),
            (2, local),
            (3, local),
            (4, human_only),
            (5, ai),
            (6, missing_house),
            (7, ai),
        ] {
            group_entity(&mut sim, stable_id, owner);
        }
        assert!(
            sim.house_is_human_player(human_only),
            "the campaign IsHumanPlayer predicate admits this owner"
        );
        assert!(
            !sim.house_is_control_group_eligible(human_only),
            "TeamAssign's mode-zero PlayerControl gate excludes a human-flag-only house"
        );

        let mut groups = vec![Vec::new(); 10];
        groups[1] = vec![1, 2];
        groups[3] = vec![5];
        sim.change_owner(2, ai);

        reassign_control_group(&mut groups, 1, &[3, 4, 5, 6], &sim);
        assert_eq!(
            groups[1],
            vec![2, 3],
            "eligible old membership clears, the transferred AI member stays latent, and only the local selection is added"
        );
        assert_eq!(
            groups[3],
            vec![5],
            "an ineligible selected member remains in its prior group"
        );
        assert_eq!(
            live_group_members(Some(&sim), &groups[1]),
            vec![3],
            "recall filters the latent member while its owner is an AI house"
        );

        groups[2] = vec![1, 7];
        reassign_control_group(&mut groups, 2, &[], &sim);
        assert_eq!(
            groups[2],
            vec![7],
            "an empty assignment clears eligible old members but leaves an ineligible member's native slot intact"
        );

        sim.change_owner(2, local);
        assert_eq!(
            live_group_members(Some(&sim), &groups[1]),
            vec![2, 3],
            "the native group slot becomes usable again when its member returns to an eligible owner"
        );
    }

    #[test]
    fn shift_group_add_preserves_a_legitimately_selected_enemy() {
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let local = house(&mut sim, "Local", false, true);
        let enemy = house(&mut sim, "Enemy", false, false);
        group_entity(&mut sim, 10, local);
        group_entity(&mut sim, 11, enemy);

        let live_group = live_group_members(Some(&sim), &[10, 11]);
        assert_eq!(live_group, [10], "only eligible group members are recalled");
        assert_eq!(
            append_group_members_to_selection(vec![11], &live_group),
            [11, 10],
            "Shift+digit keeps the ordinary click-selected enemy and adds the eligible group"
        );
    }

    #[test]
    fn multiplayer_group_eligibility_is_the_current_house_not_house_flags() {
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let local = house(&mut sim, "Local", false, false);
        let other_human = house(&mut sim, "OtherHuman", true, true);
        group_entity(&mut sim, 10, local);
        group_entity(&mut sim, 11, other_human);
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(local);

        assert_eq!(
            live_group_members(Some(&sim), &[10, 11]),
            vec![10],
            "multiplayer recall follows IsHumanPlayer's current-house comparison"
        );
    }

    #[test]
    fn registered_group_assignment_projects_a_selected_building_status_badge() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let building_type = sim.interner.intern("YAPOWR");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            27,
            12,
            10,
            0,
            0,
            owner,
            Health::new(1000),
            building_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.selection.set_selected_for_test(true);
        sim.entities_mut().insert(building);
        assert!(matches!(
            sim.reveal(27),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        assert!(
            crate::app::presentation::instances::tactical_entity_encounter_order(&sim)
                .contains(&27)
        );

        let create_one_slot = crate::app::input::hotkeys::catalog::registered_commands()
            .iter()
            .find(|metadata| metadata.ini_name == "TeamCreate_1")
            .map(|metadata| metadata.command)
            .expect("registered TeamCreate_1 command");
        let crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) = create_one_slot else {
            panic!("TeamCreate_1 must remain the existing group assignment command");
        };
        let mut groups = vec![Vec::new(); 10];
        assign_control_group(&mut groups, slot, vec![27]);

        assert_eq!(
            crate::app::presentation::ui_overlays::control_group_status_labels(
                &sim,
                &groups,
                None,
                Some(owner),
                false,
                None,
            ),
            vec![(27, '1')],
            "a retained, selected YAPOWR follows the same live group projection"
        );
    }

    #[test]
    fn registered_aircraft_group_badge_uses_shared_screen_height_and_visibility() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut aircraft = GameEntity::new_at_frame_zero_for_test(
            27,
            12,
            10,
            0,
            0,
            owner,
            Health::new(100),
            sim.interner.intern("ORCA"),
            EntityCategory::Aircraft,
            0,
            5,
            true,
        );
        aircraft.position.set_exact_z_leptons_for_test(Some(0));
        aircraft.selection.set_selected_for_test(true);
        sim.entities_mut().insert(aircraft);
        assert!(matches!(
            sim.reveal(27),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        assert!(
            crate::app::presentation::instances::tactical_entity_encounter_order(&sim)
                .contains(&27)
        );

        let registered_slot = |name: &str| {
            let command = crate::app::input::hotkeys::catalog::registered_commands()
                .iter()
                .find(|metadata| metadata.ini_name == name)
                .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
                .command;
            match command {
                crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
                command => panic!("unexpected command for {name}: {command:?}"),
            }
        };
        let mut groups = vec![Vec::new(); 10];
        assign_control_group(&mut groups, registered_slot("TeamCreate_1"), vec![27]);

        let font = crate::render::bit_font::tests::make_test_font(
            &[(b'0' as u16, 6), (b'1' as u16, 5)],
            4,
        );
        let house_colors = crate::map::houses::HouseColorMap::new();
        let house_color_ramps = crate::rules::house_colors::HouseColorRamps::default();
        let render = |sim: &Simulation, groups: &[Vec<u64>], hovered| {
            crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                sim,
                groups,
                hovered,
                Some(owner),
                false,
                &house_colors,
                &house_color_ramps,
                &font,
                [0.0, 0.0],
                1600.0,
                1200.0,
                [0, 0, 1600, 1200],
                None,
            )
        };

        let ground_anchor =
            crate::app::presentation::instances::interpolated_screen_position_entity(
                sim.entities().get(27).expect("retained ORCA"),
            );
        let selected_ground = render(&sim, &groups, None);
        assert_eq!(selected_ground.text.len(), 1);
        assert_eq!(selected_ground.background.len(), 1);
        assert_eq!(selected_ground.outline.len(), 4);
        let ground_text_position = selected_ground.text[0].position;

        {
            let aircraft = sim.entities_mut().get_mut(27).expect("retained ORCA");
            aircraft.selection.set_selected_for_test(false);
            aircraft.position.set_exact_z_leptons_for_test(Some(1500));
        }
        let airborne_anchor =
            crate::app::presentation::instances::interpolated_screen_position_entity(
                sim.entities().get(27).expect("retained ORCA"),
            );
        let hovered_airborne = render(&sim, &groups, Some(27));
        assert_eq!(hovered_airborne.text.len(), 1);
        let airborne_text_position = hovered_airborne.text[0].position;
        assert_eq!(airborne_text_position[0], ground_text_position[0]);
        assert!(airborne_text_position[1] < ground_text_position[1]);
        assert!(
            ((airborne_text_position[1] - ground_text_position[1])
                - (airborne_anchor.1 - ground_anchor.1))
                .abs()
                < 0.001,
            "Aircraft badge follows the shared exact-Z screen-position lift"
        );

        assign_control_group(&mut groups, registered_slot("TeamCreate_10"), vec![27]);
        assert_eq!(
            crate::app::presentation::ui_overlays::control_group_status_labels(
                &sim,
                &groups,
                Some(27),
                Some(owner),
                false,
                None,
            ),
            vec![(27, '0')]
        );
        assert_eq!(render(&sim, &groups, Some(27)).text.len(), 1);

        sim.entities_mut()
            .get_mut(27)
            .expect("retained ORCA")
            .selection
            .set_selected_for_test(false);
        assert!(render(&sim, &groups, None).text.is_empty());
        assign_control_group(&mut groups, registered_slot("TeamCreate_10"), Vec::new());
        sim.entities_mut()
            .get_mut(27)
            .expect("retained ORCA")
            .selection
            .set_selected_for_test(true);
        assert!(render(&sim, &groups, None).text.is_empty());
    }

    #[test]
    fn building_group_badges_follow_registered_slots_visibility_and_removal() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        let current_house = sim.interner.intern("Americans");
        let allied_house = sim.interner.intern("French");
        let gap_house = sim.interner.intern("Russians");
        let alliances = crate::map::houses::HouseAllianceMap::from([
            (
                "AMERICANS".to_owned(),
                std::collections::BTreeSet::from(["FRENCH".to_owned()]),
            ),
            (
                "FRENCH".to_owned(),
                std::collections::BTreeSet::from(["AMERICANS".to_owned()]),
            ),
        ]);
        sim.house_alliances = alliances.clone();
        sim.fog.alliances = alliances;

        // Body/status admission is the fixture subject. Supply no sustained
        // sight source: mark_visible below intentionally tests transient
        // visibility that a hostile Gap can conceal while Display is retained.
        let mut building = GameEntity::new_at_frame_zero_for_test(
            27,
            12,
            10,
            0,
            0,
            allied_house,
            Health::new(1000),
            sim.interner.intern("YAPOWR"),
            EntityCategory::Structure,
            0,
            0,
            false,
        );
        building
            .type_facts
            .set_foundation_for_test("2x2".to_owned());
        building.selection.set_selected_for_test(true);
        sim.entities_mut().insert(building);
        assert!(matches!(
            sim.reveal(27),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));

        let registered_slot = |name: &str| {
            let command = crate::app::input::hotkeys::catalog::registered_commands()
                .iter()
                .find(|metadata| metadata.ini_name == name)
                .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
                .command;
            match command {
                crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
                command => panic!("unexpected command for {name}: {command:?}"),
            }
        };
        let mut groups = vec![Vec::new(); 10];
        assign_control_group(&mut groups, registered_slot("TeamCreate_1"), vec![27]);

        fn labels(
            sim: &Simulation,
            groups: &[Vec<u64>],
            current_house: crate::sim::intern::InternedId,
            hovered: Option<u64>,
            ignore_visibility: bool,
        ) -> Vec<(u64, char)> {
            crate::app::presentation::ui_overlays::control_group_status_labels(
                sim,
                groups,
                hovered,
                Some(current_house),
                ignore_visibility,
                None,
            )
        }
        assert!(labels(&sim, &groups, current_house, None, false).is_empty());
        sim.fog.mark_visible_for_owner(current_house, 12, 10);
        assert_eq!(
            labels(&sim, &groups, current_house, None, false),
            vec![(27, '1')]
        );

        sim.entities_mut()
            .get_mut(27)
            .expect("retained building")
            .selection
            .set_selected_for_test(false);
        assert!(labels(&sim, &groups, current_house, None, false).is_empty());
        assert_eq!(
            labels(&sim, &groups, current_house, Some(27), false),
            vec![(27, '1')]
        );

        crate::sim::vision::apply_gap_generators(
            &mut sim.fog,
            &[(gap_house, 12, 10, 5)],
            &sim.interner,
        );
        assert!(labels(&sim, &groups, current_house, Some(27), false).is_empty());
        assert_eq!(
            labels(&sim, &groups, current_house, Some(27), true),
            vec![(27, '1')]
        );

        sim.entities_mut()
            .get_mut(27)
            .expect("retained building")
            .selection
            .set_selected_for_test(true);
        assign_control_group(&mut groups, registered_slot("TeamCreate_10"), vec![27]);
        assert_eq!(
            labels(&sim, &groups, current_house, None, true),
            vec![(27, '0')]
        );
        assign_control_group(&mut groups, 0, Vec::new());
        assert!(labels(&sim, &groups, current_house, None, true).is_empty());

        assign_control_group(&mut groups, 1, vec![27]);
        sim.entities_mut().remove(27);
        assert!(labels(&sim, &groups, current_house, None, true).is_empty());
    }

    #[test]
    fn assignment_reassignment_and_ungrouping_drive_status_group_projection() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let enemy_owner = sim.interner.intern("Russians");
        let unit_type = sim.interner.intern("MTNK");
        let mut unit = GameEntity::new_at_frame_zero_for_test(
            17,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            unit_type,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        unit.selection.set_selected_for_test(true);
        sim.entities_mut().insert(unit);
        assert!(matches!(
            sim.reveal(17),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        let unselected_unit = GameEntity::new_at_frame_zero_for_test(
            18,
            11,
            10,
            0,
            0,
            owner,
            Health::new(100),
            unit_type,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        sim.entities_mut().insert(unselected_unit);
        assert!(matches!(
            sim.reveal(18),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        let mut enemy = GameEntity::new_at_frame_zero_for_test(
            19,
            12,
            10,
            0,
            0,
            enemy_owner,
            Health::new(100),
            unit_type,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        enemy.selection.set_selected_for_test(true);
        sim.entities_mut().insert(enemy);
        assert!(matches!(
            sim.reveal(19),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        // The native pip tail admits non-allies only through Techno+0x210.
        // That bit is not modeled; a shroud-revealed enemy still does not take
        // the common CurrentPlayer-allied path implemented here.
        sim.fog.mark_visible_for_owner(owner, 12, 10);
        let display_order =
            crate::app::presentation::instances::tactical_entity_encounter_order(&sim);
        for id in [17, 18, 19] {
            assert!(
                display_order.contains(&id),
                "fixture {id} must use the retained production Display list"
            );
        }

        // MatchInputState holds ten control-group slots. Keep this test on the
        // public command/projection path rather than reaching through the
        // private input-state owner for its storage constant.
        let mut groups = vec![Vec::new(); 10];
        let projected_with_visibility =
            |groups: &[Vec<u64>], hovered_unit_id, ignore_visibility| {
                crate::app::presentation::ui_overlays::control_group_status_labels(
                    &sim,
                    groups,
                    hovered_unit_id,
                    Some(owner),
                    ignore_visibility,
                    None,
                )
            };
        let projected = |groups: &[Vec<u64>], hovered_unit_id| {
            projected_with_visibility(groups, hovered_unit_id, false)
        };
        let font = crate::render::bit_font::tests::make_test_font(
            &[(b'0' as u16, 6), (b'1' as u16, 5), (b'2' as u16, 7)],
            4,
        );
        let house_colors = crate::map::houses::HouseColorMap::new();
        let house_color_ramps = crate::rules::house_colors::HouseColorRamps::default();
        let group_member_anchor =
            crate::app::presentation::instances::interpolated_screen_position_entity(
                sim.entities().get(17).expect("assigned test unit"),
            );
        let fixture_clip = [
            group_member_anchor.0.floor() as i32 - 100,
            group_member_anchor.1.floor() as i32 - 100,
            200,
            200,
        ];
        let status_text_left = group_member_anchor.0 - 14.0 - 3.0;
        let status_text_top = group_member_anchor.1 - 0x1A as f32 + 2.0;
        assert!(
            status_text_left >= fixture_clip[0] as f32
                && status_text_left + 12.0 <= (fixture_clip[0] + fixture_clip[2]) as f32
                && status_text_top >= fixture_clip[1] as f32
                && status_text_top + 13.0 <= (fixture_clip[1] + fixture_clip[3]) as f32,
            "the synthetic render clip contains the projected unit's whole status frame"
        );
        let group_overlay = |groups: &[Vec<u64>], hovered_unit_id| {
            crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                &sim,
                groups,
                hovered_unit_id,
                Some(owner),
                false,
                &house_colors,
                &house_color_ramps,
                &font,
                [0.0, 0.0],
                800.0,
                600.0,
                fixture_clip,
                None,
            )
        };
        let group_glyphs =
            |groups: &[Vec<u64>], hovered_unit_id| group_overlay(groups, hovered_unit_id).text;
        let registered_create_slot = |name: &str| {
            let command = crate::app::input::hotkeys::catalog::registered_commands()
                .iter()
                .find(|metadata| metadata.ini_name == name)
                .unwrap_or_else(|| panic!("missing registered hotkey {name}"))
                .command;
            match command {
                crate::app::input::hotkeys::HotkeyCommand::TeamCreate(slot) => slot,
                command => panic!("unexpected command for {name}: {command:?}"),
            }
        };

        let create_one_slot = registered_create_slot("TeamCreate_1");
        assert_eq!(create_one_slot, 1);
        assign_control_group(&mut groups, create_one_slot, vec![17, 18, 19]);
        assert_eq!(projected(&groups, None), vec![(17, '1')]);
        let group_one = group_overlay(&groups, None);
        assert_eq!(group_one.text.len(), 1);
        assert_eq!(group_one.background.len(), 1);
        assert_eq!(group_one.outline.len(), 4);
        assert_eq!(projected(&groups, Some(17)), vec![(17, '1')]);
        let first_glyph = group_glyphs(&groups, Some(17));
        assert_eq!(first_glyph.len(), 1);
        let hovered_labels = projected(&groups, Some(18));
        assert!(hovered_labels.contains(&(17, '1')));
        assert!(hovered_labels.contains(&(18, '1')));
        assert_eq!(group_glyphs(&groups, Some(18)).len(), 2);
        assert_eq!(
            projected(&groups, Some(19)),
            vec![(17, '1')],
            "the pip-only allied gate excludes a revealed non-allied unit"
        );

        assign_control_group(&mut groups, 2, vec![17, 18, 19]);
        assert_eq!(projected(&groups, None), vec![(17, '2')]);
        let group_two = group_overlay(&groups, None);
        assert_eq!(group_two.text.len(), 1);
        assert_eq!(group_two.background.len(), 1);
        assert_eq!(group_two.outline.len(), 4);

        let create_zero_slot = registered_create_slot("TeamCreate_10");
        assert_eq!(create_zero_slot, 0);
        assign_control_group(&mut groups, create_zero_slot, vec![17]);
        assert_eq!(projected(&groups, None), vec![(17, '0')]);
        let group_zero = group_overlay(&groups, None);
        assert_eq!(group_zero.text.len(), 1);
        assert_eq!(group_zero.background.len(), 1);
        assert_eq!(group_zero.outline.len(), 4);

        assign_control_group(&mut groups, 0, Vec::new());
        assert!(projected(&groups, None).is_empty());
        let no_group_overlay = group_overlay(&groups, None);
        assert!(no_group_overlay.text.is_empty());
        assert!(no_group_overlay.background.is_empty());
        assert!(no_group_overlay.outline.is_empty());

        assign_control_group(&mut groups, 0, vec![17]);
        sim.entities_mut().remove(17);
        assert!(
            crate::app::presentation::ui_overlays::control_group_status_labels(
                &sim,
                &groups,
                None,
                Some(owner),
                false,
                None,
            )
            .is_empty()
        );
        assert!(
            crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                &sim,
                &groups,
                None,
                Some(owner),
                false,
                &house_colors,
                &house_color_ramps,
                &font,
                [0.0, 0.0],
                800.0,
                600.0,
                [0, 0, 800, 600],
                None,
            )
            .text
            .is_empty()
        );
    }

    #[test]
    fn allied_control_group_labels_follow_body_admission_without_raw_anchor_gate() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        fn labels(
            sim: &Simulation,
            groups: &[Vec<u64>],
            current_house: crate::sim::intern::InternedId,
            ignore_visibility: bool,
        ) -> Vec<(u64, char)> {
            crate::app::presentation::ui_overlays::control_group_status_labels(
                sim,
                groups,
                None,
                Some(current_house),
                ignore_visibility,
                None,
            )
        }

        let mut sim = Simulation::new();
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        let current_house = sim.interner.intern("Americans");
        let ally = sim.interner.intern("French");
        let gap_owner = sim.interner.intern("Russians");
        let allies = crate::map::houses::HouseAllianceMap::from([
            (
                "AMERICANS".to_owned(),
                std::collections::BTreeSet::from(["FRENCH".to_owned()]),
            ),
            (
                "FRENCH".to_owned(),
                std::collections::BTreeSet::from(["AMERICANS".to_owned()]),
            ),
        ]);
        sim.house_alliances = allies.clone();
        sim.fog.alliances = allies;

        // Body/status admission is the fixture subject. Supply no sustained
        // sight source: mark_visible below intentionally tests transient
        // visibility that a hostile Gap can conceal while Display is retained.
        let mut unit = GameEntity::new_at_frame_zero_for_test(
            80,
            10,
            10,
            0,
            0,
            ally,
            Health::new(100),
            sim.interner.intern("MTNK"),
            EntityCategory::Unit,
            0,
            0,
            false,
        );
        unit.selection.set_selected_for_test(true);
        sim.entities_mut().insert(unit);
        assert!(matches!(
            sim.reveal(80),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        let mut groups = vec![Vec::new(); 10];
        assign_control_group(&mut groups, 1, vec![80]);
        assert!(
            crate::app::presentation::instances::tactical_entity_encounter_order(&sim)
                .contains(&80),
            "the allied unit remains in the retained Display projection"
        );

        // Native Extras6D907E/6D9111 ->6D95D2..9789 ->6F5E27..87
        // reaches the shared pip tail from selected retained body admission;
        // the group tail70A6FD adds no independent raw-anchor fog query.
        assert_eq!(labels(&sim, &groups, current_house, false), vec![(80, '1')]);
        assert_eq!(
            labels(&sim, &groups, current_house, true),
            vec![(80, '1')],
            "sandbox does not change the represented body/pip relationship policy"
        );

        sim.fog.mark_visible_for_owner(current_house, 10, 10);
        assert!(sim.fog.is_cell_revealed(current_house, 10, 10));
        assert_eq!(labels(&sim, &groups, current_house, false), vec![(80, '1')]);

        crate::sim::vision::apply_gap_generators(
            &mut sim.fog,
            &[(gap_owner, 10, 10, 5)],
            &sim.interner,
        );
        assert!(sim.fog.is_cell_gap_covered(current_house, 10, 10));
        assert!(!sim.fog.is_cell_revealed(current_house, 10, 10));
        assert_eq!(
            labels(&sim, &groups, current_house, false),
            vec![(80, '1')],
            "a retained admitted body keeps its label across a raw-anchor Gap frontier"
        );
        assert_eq!(labels(&sim, &groups, current_house, true), vec![(80, '1')]);

        assert!(matches!(
            sim.conceal(80),
            crate::sim::world::ConcealOutcome::Concealed
        ));
        assert!(labels(&sim, &groups, current_house, false).is_empty());
        assert!(
            labels(&sim, &groups, current_house, true).is_empty(),
            "sandbox cannot resurrect a concealed body status tail"
        );
    }

    #[test]
    fn native_tail_corpus_matches_control_group_production_text_builder() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/control_group_text_native.json",
        ))
        .expect("native control-group text corpus is valid JSON");
        assert_eq!(
            corpus["schema"], "control-group-tail-native-v1",
            "the Rust consumer is tied to the original bounded draw-tail corpus"
        );

        let metrics = corpus["native_font_initialization"]["digit_metrics"]
            .as_array()
            .expect("native digit metrics");
        let glyph_widths: Vec<(u16, u32)> = metrics
            .iter()
            .map(|metric| {
                (
                    b'0' as u16 + metric["digit"].as_u64().expect("digit") as u16,
                    metric["glyph_width"].as_u64().expect("glyph width") as u32,
                )
            })
            .collect();
        let font = crate::render::bit_font::tests::make_test_font(&glyph_widths, 4);
        let bitmap_rows = corpus["native_font_initialization"]["runtime_fields"]["bitmap_rows"]
            .as_u64()
            .expect("native bitmap rows") as f32;
        let house_rgb: [u8; 3] = corpus["provenance"]["house_rgb_input"]
            .as_array()
            .expect("native House+0x56F9 RGB input")
            .iter()
            .map(|channel| channel.as_u64().expect("house RGB byte") as u8)
            .collect::<Vec<_>>()
            .try_into()
            .expect("three House RGB channels");
        let house_color_native: serde_json::Value = serde_json::from_str(
            crate::test_fixture::text("tools/procedural_drawing_oracle/house_color.json"),
        )
        .expect("native house-color corpus is valid JSON");
        let house_entries = house_color_native["colors"]
            .as_array()
            .expect("native house colors");
        let schemes: Vec<crate::rules::color_scheme::ColorSchemeEntry> = house_entries
            .iter()
            .map(|entry| crate::rules::color_scheme::ColorSchemeEntry {
                name: entry["name"].as_str().expect("house color name").into(),
                hsv: entry["hsv"]
                    .as_array()
                    .expect("house color HSV")
                    .iter()
                    .map(|value| value.as_u64().expect("HSV byte") as u8)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("three HSV components"),
            })
            .collect();
        let gold_index = house_entries
            .iter()
            .position(|entry| entry["name"] == "Gold")
            .expect("stock Gold scheme");
        let native_gold_rgb: [u8; 3] = house_entries[gold_index]["modes"]["scalar"]["rgb"]
            .as_array()
            .expect("native Gold House RGB")
            .iter()
            .map(|value| value.as_u64().expect("RGB byte") as u8)
            .collect::<Vec<_>>()
            .try_into()
            .expect("three native RGB channels");
        assert_eq!(house_rgb, native_gold_rgb);
        let house_color_index = crate::rules::house_colors::HouseColorIndex(gold_index as u8);
        let house_color_ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&schemes);
        let house_colors =
            crate::map::houses::HouseColorMap::from([("Americans".to_owned(), house_color_index)]);
        assert_eq!(
            crate::render::palette_light::house_color_rgb(&house_color_ramps, house_color_index),
            house_rgb,
            "the shared house-color owner resolves native Gold House+0x56F9"
        );

        for row in corpus["cases"].as_array().expect("native cases") {
            let case = &row["case"];
            let label = case["label"].as_str().expect("case label");
            let native_slot = case["slot"].as_u64().expect("native Techno slot") as usize;
            let kind = case["kind"].as_u64().expect("native WhatAmI");
            let digit = row["text"].as_str().expect("native digit");
            let digit_index = digit
                .as_bytes()
                .first()
                .and_then(|value| value.checked_sub(b'0'))
                .expect("native digit index") as usize;
            assert_eq!(
                digit_index,
                if native_slot == 9 { 0 } else { native_slot + 1 },
                "native Techno index maps to the registered Rust digit slot"
            );
            let packed_rgb = row["text_submission"]["packed_owner_rgb"]
                .as_str()
                .expect("native packed house RGB")
                .strip_prefix("0x")
                .and_then(|value| u16::from_str_radix(value, 16).ok())
                .expect("packed RGB565 word");
            assert_eq!(
                crate::render::native_surface_format::RGB565.pack_rgb8(house_rgb),
                packed_rgb,
                "native case {label} packs the direct House+0x56F9 RGB"
            );
            let native_caller_point = case["caller_point"].as_array().expect("caller point");
            let native_x = native_caller_point[0].as_i64().expect("caller x") as f32;
            let native_y = native_caller_point[1].as_i64().expect("caller y") as f32;

            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let category = match kind {
                0xF => EntityCategory::Infantry,
                2 => EntityCategory::Aircraft,
                6 => EntityCategory::Structure,
                _ => EntityCategory::Unit,
            };
            let type_name = case["type_name"].as_str().unwrap_or(match category {
                EntityCategory::Aircraft => "ORCA",
                EntityCategory::Infantry => "E1",
                EntityCategory::Structure => "YAPOWR",
                _ => "MTNK",
            });
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                1,
                10,
                10,
                0,
                0,
                owner,
                Health::new(100),
                sim.interner.intern(type_name),
                category,
                0,
                5,
                category == EntityCategory::Aircraft,
            );
            if let Some(foundation) = case["foundation"].as_str() {
                entity
                    .type_facts
                    .set_foundation_for_test(foundation.to_owned());
            }
            entity.selection.set_selected_for_test(true);
            sim.entities_mut().insert(entity);
            assert!(matches!(
                sim.reveal(1),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
            let mut groups = vec![Vec::new(); 10];
            assign_control_group(&mut groups, digit_index, vec![1]);
            assert_eq!(
                crate::app::presentation::ui_overlays::control_group_status_labels(
                    &sim,
                    &groups,
                    None,
                    Some(owner),
                    false,
                    None,
                ),
                vec![(1, digit.chars().next().expect("one native digit"))],
                "the assignment reaches the same live production projection"
            );

            let anchor = crate::app::presentation::instances::interpolated_screen_position_entity(
                sim.entities().get(1).expect("retained live entity"),
            );
            // Align the production entity's Rust screen anchor to the native
            // caller fixture. The comparison then exercises the real text
            // builder's anchor, font measurement, glyph and viewport-cull path.
            let camera = [anchor.0 - native_x, anchor.1 - native_y];
            let (sw, sh) = if label == "outside" {
                (100.0, 100.0)
            } else {
                (640.0, 480.0)
            };
            let overlay_instances =
                crate::app::presentation::ui_overlays::build_control_group_status_render_instances(
                    &sim,
                    &groups,
                    None,
                    Some(owner),
                    false,
                    &house_colors,
                    &house_color_ramps,
                    &font,
                    camera,
                    sw,
                    sh,
                    case["clip"]
                        .as_array()
                        .expect("native clip xywh")
                        .iter()
                        .map(|value| value.as_i64().expect("clip coordinate") as i32)
                        .collect::<Vec<_>>()
                        .try_into()
                        .expect("native clip has four xywh values"),
                    None,
                );
            let glyphs = &overlay_instances.text;
            if label == "outside" {
                assert!(glyphs.is_empty(), "the off-viewport native case is culled");
                assert!(
                    overlay_instances.background.is_empty(),
                    "the off-viewport native case does not draw its black fill"
                );
                assert!(
                    overlay_instances.outline.is_empty(),
                    "the off-viewport native case does not draw its colored outline"
                );
                continue;
            }

            assert_eq!(glyphs.len(), 1, "native case {label} emits one digit quad");
            assert_eq!(
                overlay_instances.background.len(),
                1,
                "native case {label} emits the black fill before its outline and glyph"
            );
            let native_fill = &overlay_instances.background[0];
            assert_eq!(
                native_fill.tint, [0.0; 3],
                "native background_fill color_arg 0 is rendered as black"
            );
            assert_eq!(native_fill.alpha, 1.0);
            let native_fill_record = &row["background_fill"];
            let expected_fill = native_fill_record["rect"]
                .as_array()
                .expect("native clipped fill rectangle")
                .iter()
                .map(|value| value.as_i64().expect("fill coordinate") as f32)
                .collect::<Vec<_>>();
            assert_eq!(
                native_fill_record["color_arg"].as_u64(),
                Some(0),
                "the original fill call uses color_arg 0"
            );
            let native_outline_record = &row["surface_draw"];
            assert_eq!(
                native_outline_record["color_arg"].as_u64(),
                Some(u64::from(packed_rgb)),
                "the original four-line outline uses the packed House RGB"
            );
            for (axis, value) in [
                (0, native_fill.position[0] - camera[0]),
                (1, native_fill.position[1] - camera[1]),
                (2, native_fill.size[0]),
                (3, native_fill.size[1]),
            ] {
                assert_eq!(
                    value, expected_fill[axis],
                    "native case {label} background fill axis {axis}"
                );
            }
            assert_eq!(overlay_instances.outline.len(), 4);
            let expected_outline = [
                (
                    [expected_fill[0], expected_fill[1]],
                    [expected_fill[2], 1.0],
                ),
                (
                    [expected_fill[0], expected_fill[1] + expected_fill[3] - 1.0],
                    [expected_fill[2], 1.0],
                ),
                (
                    [expected_fill[0], expected_fill[1] + 1.0],
                    [1.0, expected_fill[3] - 2.0],
                ),
                (
                    [
                        expected_fill[0] + expected_fill[2] - 1.0,
                        expected_fill[1] + 1.0,
                    ],
                    [1.0, expected_fill[3] - 2.0],
                ),
            ];
            for (line, (expected_position, expected_size)) in
                overlay_instances.outline.iter().zip(expected_outline)
            {
                assert_eq!(
                    [line.position[0] - camera[0], line.position[1] - camera[1],],
                    expected_position,
                    "native case {label} outline line position"
                );
                assert_eq!(line.size, expected_size, "native case {label} outline size");
                assert_eq!(
                    line.tint,
                    house_rgb.map(|channel| f32::from(channel) / 255.0),
                    "native case {label} outline uses direct House+0x56F9 RGB"
                );
            }
            let expected_width = metrics[digit_index]["glyph_width"]
                .as_u64()
                .expect("native glyph width") as f32;
            assert_eq!(
                glyphs[0].size,
                [expected_width, bitmap_rows],
                "native case {label} uses the pinned GAME.FNT glyph metrics"
            );
            assert_eq!(
                glyphs[0].tint,
                house_rgb.map(|channel| f32::from(channel) / 255.0),
                "native case {label} uses House+0x56F9 through the existing RGB owner"
            );
            assert_eq!(
                font.text_width(digit),
                row["font_measure"].as_u64().expect("native text measure") as u32,
                "native case {label} measurement follows the shared BitFont owner"
            );
            let expected = row["text_submission"]["position"]
                .as_array()
                .expect("native text submission position");
            for (axis, value) in [
                (0, glyphs[0].position[0] - camera[0]),
                (1, glyphs[0].position[1] - camera[1]),
            ] {
                let expected = expected[axis].as_i64().expect("native text coordinate") as f32;
                assert!(
                    (value - expected).abs() < 0.01,
                    "native case {label} axis {axis}: Rust {value} != native {expected}"
                );
            }
        }
    }

    #[test]
    fn assigning_an_empty_selection_empties_the_slot() {
        let mut groups = vec![vec![1u64, 2], Vec::new()];
        assign_control_group(&mut groups, 0, Vec::new());
        assert!(groups[0].is_empty());
    }

    /// One and two points are a plain mean; the outlier trim only kicks in
    /// above two.
    #[test]
    fn centroid_of_one_or_two_points_is_the_plain_mean() {
        assert_eq!(trimmed_centroid_leptons(&[(100, 200)]), Some((100, 200)));
        assert_eq!(
            trimmed_centroid_leptons(&[(0, 0), (100, 200)]),
            Some((50, 100))
        );
    }

    /// With more than two points the single farthest one is dropped, so a lone
    /// straggler cannot drag the view off the main body.
    #[test]
    fn centroid_of_three_or_more_drops_the_farthest_point() {
        let points = [(0i64, 0i64), (10, 0), (20, 0), (3000, 0)];
        // Plain mean would be 757; dropping (3000,0) leaves 30/3 = 10.
        assert_eq!(trimmed_centroid_leptons(&points), Some((10, 0)));
    }

    #[test]
    fn centroid_of_nothing_is_nothing() {
        assert_eq!(trimmed_centroid_leptons(&[]), None);
    }
}

#[cfg(test)]
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
