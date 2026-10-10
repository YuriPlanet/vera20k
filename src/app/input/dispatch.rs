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
        // never reaches the tactical body, so the capture is dropped here.
        // Leaving it set would freeze edge auto-scroll for the rest of the match.
        if !pressed {
            state.match_state.input.tactical_mouse.left_held = false;
            state.match_state.input.tactical_mouse.right_held = false;
            state.match_state.input.tactical_mouse.release();
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

/// Tactical-viewport mouse body (routed here when the full-tactical ClickRegion
/// consumes the edge — i.e. a click in the play area, or a captured drag/release
/// that started there). Logic is the legacy handler's tactical path, unchanged;
/// the minimap-drag-end and minimap-begin checks moved to `minimap_mouse`. The
/// stock tactical path has no middle-button case, so the Rust dispatcher ignores it.
pub(crate) fn tactical_mouse(state: &mut AppState, button: MouseButton, btn_state: ElementState) {
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
                if state.match_state.input.targeting_mode.is_some()
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
                // Only an active band box owns a clamped tactical endpoint.
                // A pending press is still an ordinary click at the actual
                // release point, including after sticky capture routing.
                let release_point = if state.match_state.input.selection_state.is_band_box_active()
                {
                    let (tactical_width, tactical_height) =
                        crate::app::input::camera::tactical_viewport_size_px(
                            state.render_width(),
                            state.render_height(),
                        );
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
                if matches!(
                    route_click_action_before_type_select(
                        action,
                        shift,
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
                                    &screen_order,
                                    fog_ref,
                                    preferred_local_owner_name(state).as_deref(),
                                    world_x,
                                    world_y,
                                    CLICK_SELECT_RADIUS,
                                    state.rules(),
                                    Some(&sim.houses),
                                    &state.height_map(),
                                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                                    Some(&sim.interner),
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
                                        &screen_order,
                                        &current_selection,
                                        fog_ref,
                                        preferred_local_owner_name(state).as_deref(),
                                        world_x,
                                        world_y,
                                        CLICK_SELECT_RADIUS,
                                        shift,
                                        state.rules(),
                                        Some(&sim.houses),
                                        &state.height_map(),
                                        crate::app::match_runtime::sim_tick::tactical_bridge_cells(
                                            sim,
                                        ),
                                        Some(&sim.interner),
                                        sim.playfield_bounds.is_some(),
                                    )
                                };
                                held_type_select_batch = true;
                            } else {
                                queued_selection = compute_click_selection_snapshot_with_playfield(
                                    sim.entities(),
                                    &screen_order,
                                    &current_selection,
                                    fog_ref,
                                    preferred_local_owner_name(state).as_deref(),
                                    world_x,
                                    world_y,
                                    CLICK_SELECT_RADIUS,
                                    shift,
                                    state.rules(),
                                    Some(&sim.houses),
                                    &state.height_map(),
                                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                                    Some(&sim.interner),
                                    sim.playfield_bounds.is_some(),
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
                state.match_state.input.tactical_mouse.right_held = true;
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
    if state.match_state.input.targeting_mode.is_some() {
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
    match button {
        MouseButton::Left => {
            if btn_state.is_pressed() {
                crate::app::presentation::sidebar_render::try_begin_minimap_drag(state);
            } else if state.match_state.input.minimap_dragging {
                state.match_state.input.minimap_dragging = false;
            }
        }
        MouseButton::Right => {
            // A right-press centers the view on the clicked cell (no command);
            // right-release just releases the gadget's sticky capture.
            if btn_state.is_pressed()
                && crate::app::presentation::sidebar_render::is_cursor_over_minimap(state)
            {
                crate::app::presentation::sidebar_render::update_camera_from_minimap_cursor(state);
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
    let (tactical_width, tactical_height) = crate::app::input::camera::tactical_viewport_size_px(
        state.render_width(),
        state.render_height(),
    );
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
    use crate::sim::command::Command;
    use crate::sim::components::Health;
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
                    20,
                    20,
                    true,
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
                    queue: false,
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
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.lifecycle.object_alive = true;
            entity.lifecycle.in_limbo = false;
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
    let click = sidebar_eva::cameo_click(&queue, category, type_iid, press, at_build_limit);
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
        // Residual (building path): `HouseClass::Manual_Place @ 0x004FB840`
        // ignores the click while any placement is pending (`0x004FB852..
        // 0x004FB859`); VERA's click leaves placement mode for the same
        // building and switches it to another finished one.
        Some(sidebar_eva::CameoOrder::Place) => {
            let armed = state
                .armed_building_type()
                .is_some_and(|armed| armed.eq_ignore_ascii_case(type_id));
            if armed {
                state.match_state.input.targeting_mode = None;
                state.match_state.input.building_placement_preview = None;
            } else {
                state.match_state.input.targeting_mode = Some(
                    crate::app::types::TargetingMode::BuildingPlacement(type_id.to_string()),
                );
                let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
                gadgets.repair_mode_on = false;
                gadgets.sell_mode_on = false;
            }
        }
    }
}

pub(crate) fn apply_sidebar_action(state: &mut AppState, action: SidebarAction) {
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
        if let Some((id, fog_state)) = fog {
            if !fog_state.is_cell_revealed(id, cell.rx, cell.ry) {
                unrevealed += 1;
                if unrevealed_samples.len() < 12 {
                    unrevealed_samples.push((cell.rx, cell.ry));
                }
                // An unrevealed cell is never drawn, so its tile is irrelevant.
                continue;
            }
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

/// Toggle debug pause (J hotkey / dev overlay).
///
/// On resume, local frame admission is re-anchored so elapsed modal time
/// cannot cause a catch-up frame.
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
            if KeyModifiers::from_modifiers_state(state.match_state.input.hotkey_modifiers)
                .dev_chord()
            {
                handle_dev_hotkey_pressed(state, physical_code);
            }
        }
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
}

#[path = "selection_navigation.rs"]
pub(crate) mod selection_navigation;

fn dispatch_retail_hotkey(state: &mut AppState, command: HotkeyCommand) {
    match command {
        HotkeyCommand::HealthNav => selection_navigation::execute_health_navigation(state),
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
        HotkeyCommand::StructureTab => {
            apply_sidebar_action(state, SidebarAction::SelectTab(SidebarTab::Building))
        }
        HotkeyCommand::DefenseTab => {
            apply_sidebar_action(state, SidebarAction::SelectTab(SidebarTab::Defense))
        }
        HotkeyCommand::InfantryTab => {
            apply_sidebar_action(state, SidebarAction::SelectTab(SidebarTab::Infantry))
        }
        HotkeyCommand::UnitTab => {
            apply_sidebar_action(state, SidebarAction::SelectTab(SidebarTab::Vehicle))
        }
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
        // DEFERRED, not overlooked: waypoint planning mode is a whole input
        // mode, and VERA has none of it. Prior research already handed this off:
        // docs/research/DRIVE_QUEUED_CLICK_EVENT_PLANNING_MODE_OUTCOME_RESWARM_20260528.md,
        // whose conclusion — implement it as a separate path-command surface,
        // NOT as FootClass NavQueue entries — still stands.
        //
        // Mode edge. `PlanningModeCommandClass::Execute` 0x00536750 reads the
        // key-up bit itself (`TEST AH,0x8`), so the mode is HELD, not toggled.
        // It is not alone in that: `TypeSelectCommandClass::Execute` 0x005368B0
        // has the identical prologue, and VERA already models that one as a
        // both-edges held command in `handle_type_select_key_edge`. Press routes
        // 0x00731A50 -> 0x006379C0, which sets the mode byte `DAT_00AC4CF4`,
        // plays a sound and posts UI message 0x11C7. Release routes 0x00731A70
        // -> 0x00637A10, which clears the mode byte and posts 0x11C8. That same
        // mode byte is what `FUN_0063AB60` hands
        // `ScrollClass__UpdateMouseScrolling` 0x00692F30 ahead of its capture
        // routing.
        //
        // Command surface — THREE event types, not one, and the plan is streamed
        // as it is drawn rather than shipped at the end:
        //   * PLANCONNECT 0x2A — emitted by `FUN_0063AD50` once PER SELECTED
        //     OBJECT PER CLICK, through the payload ctor 0x004C6780 and
        //     `Try_Append_Event_To_OutList`.
        //   * PLANCOMMIT 0x2B — the release event. Built with the 2-arg
        //     `EventClass(house, type)` ctor 0x004C66C0, so it carries only the
        //     type, the local house `g_PlayerPtr+0x30` and the frame. It carries
        //     NO path data.
        //   * PLANNODEDELETE 0x2C — `DeleteCommandClass::Execute` 0x00537F90 ->
        //     0x00731A10 -> `FUN_00637D00`, i.e. the Delete key edits the plan
        //     before it commits. `HotkeyCommand::Delete` below belongs to this
        //     same deferred mode.
        // All three land in one executor, `FUN_00637E00`. Module string
        // `D:\ra2mdpost\PlanMgr.cpp` at 0x00836BD4.
        //
        // Storage is the row's named model: TWELVE `WaypointPathClass*` slots on
        // HouseClass at +0x210..+0x23C with the active slot index at +0x20C
        // (walked by `FUN_006DAD60`), the point count at path+0x38, and a
        // per-object back-pointer at `TechnoClass+0x514`. Paths can loop
        // (`WaypointPathClass+0x24`). The overlay is a dashed all-segment line
        // with a per-point shroud test and MOUSE.SHA action 0x3C.
        //
        // INI: `MaxWaypointPathLength=15` caps the path (Rules+0x90), and
        // `AddPlanningModeCommandSound=PlanningModeAdd` is a third sound, played
        // per added node — start and end are not the whole set.
        //
        // Trigger: holding Z. Player effect: the key does nothing, so no
        // multi-leg route can be planned. Frequency: occasional — a deliberate
        // habit rather than a reflex. Downstream risk: bounded but wider than a
        // single command — three new event types on the deterministic stream
        // (row 89 owns the envelope), house-side path storage, and a new overlay
        // layer. Nothing here needs rework to accommodate it later.
        //
        // Note the honest limit of the frequency call: VERA's Shift-queue is NOT
        // an equivalent, and is itself VERA-internal with the gamemd equivalent
        // UNIMPLEMENTED. It reproduces single-unit route geometry only. It
        // cannot commit every selected unit's path at one instant, cannot edit
        // nodes before committing, and cannot loop a path.
        HotkeyCommand::PlanningMode => {}
        // DEFERRED (row 86, GSI-14.05's selection-cycling half). Control groups
        // themselves are implemented; these two verbs are not, and each needs
        // its own mechanism rather than a shared one.
        //
        // `CombatantSelect` (P, Execute 0x005367F0) and `VeterancyNav`
        // (Y, Execute 0x005369F0) share a prologue that hands
        // `!(key >> 8) & 1` — the INVERTED Shift bit — to 0x00732280 and
        // 0x007336C0 respectively. `FUN_00732280` is the closest of the four to
        // landable: it is the TypeSelect escalation machine VERA already owns
        // (`g_bTypeSelectAcrossMap`, `g_SelectionMode`, and the same
        // screen/map/empty CSF feedback triple, string ids 0x3F5 / 0x3F3 /
        // 0x3F1) driven by a different member predicate — a drawn-list test
        // `FUN_007342C0` (`entry->+0x14 & 1`) plus a type flag at
        // `TechnoTypeClass+0xDBC`, whose INI key is UNCHECKED. So closing it is
        // "reuse `compute_type_select_tap`'s scope machinery with a combatant
        // predicate", not new infrastructure.
        //
        // Trigger: pressing P or Y. Player effect: the key does nothing.
        // Frequency: occasional — control groups carry this load in ordinary
        // play. Downstream risk: none for Veterancy's app-side selection;
        // CombatantSelect touches the shared TypeSelect scope latch, so it
        // should land beside that machinery rather than duplicating it.
        HotkeyCommand::CombatantSelect | HotkeyCommand::VeterancyNav => {}
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
        | HotkeyCommand::ScatterObject
        | HotkeyCommand::Delete
        | HotkeyCommand::Taunt(_) => {}
    }
}

/// Native command-bar IDs use the same command endpoints as keyboard input.
/// Team right-click clears its group (6D0660); the other source actions reuse
/// the existing input owners rather than introducing a second gameplay path.
pub(crate) fn dispatch_command_bar(state: &mut AppState, command: usize, right: bool) {
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
        9 => HotkeyCommand::PlanningMode,
        10 => HotkeyCommand::AllToCheer,
        _ => return,
    };
    dispatch_retail_hotkey(state, action);
}

// Native 653952 -> GameState1 -> 48C9BA/4F10E0 opens the pause menu.
// The button calls this directly; Escape's cancel/resume priority is separate.
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
        if state
            .match_state
            .match_presentation
            .software_cursor
            .is_some()
        {
            state.platform.window.set_cursor_visible(false);
        }
        log::info!("Game resumed");
    } else if state.match_state.input.targeting_mode.is_some() {
        state.match_state.input.targeting_mode = None;
        state.match_state.input.building_placement_preview = None;
    } else if state
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
    } else {
        open_pause_menu(state);
    }
}

/// Dev/debug hotkeys require Ctrl+Shift, a chord stock binds to no command.
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
        KeyCode::Period => {
            if state.match_state.paused() {
                state.diag.debug_frame_step_requested = true;
            }
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
        if let Some(entity) = sim.entities().get(id) {
            if selection_order_pending || entity.selected {
                ordered.push(id);
            }
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
        if entity.selected && members.insert(entity.stable_id()) {
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
        .filter(|entity| entity.selected)
        .all(|entity| {
            committed_count += 1;
            members.contains(&entity.stable_id())
        })
        && committed_count == ordered.len()
}

/// Synchronize the app ledger after the due selection commands and lifecycle
/// removals have committed for this frame.
pub(crate) fn reconcile_selection_order_after_sim(state: &mut AppState) {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        state.match_state.input.selection_order.clear();
        state.match_state.input.selection_order_pending = false;
        state.match_state.input.health_navigation = Default::default();
        state.match_state.input.type_select.reset_scope();
        return;
    };
    // 733160 removes expired objects from the retained navigation snapshot.
    state.match_state.input.health_navigation.retain(|id| {
        sim.entities()
            .get(*id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
    });
    if state.match_state.input.selection_order_pending {
        let before_retain = state.match_state.input.selection_order.len();
        state.match_state.input.selection_order.retain(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| entity.lifecycle.object_alive)
        });
        if state.match_state.input.selection_order.len() != before_retain {
            state.match_state.input.type_select.reset_scope();
        }
        if !selection_membership_committed(sim, &state.match_state.input.selection_order) {
            return;
        }
        state.match_state.input.selection_order_pending = false;
        return;
    }
    let prior_len = state.match_state.input.selection_order.len();
    let reconciled: Vec<u64> = state
        .match_state
        .input
        .selection_order
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| entity.selected)
        })
        .collect();
    let lifecycle_removed = reconciled.len() < prior_len;
    let reconciled = admit_missing_selected_ids(reconciled, sim, state.rules());
    if lifecycle_removed {
        state.match_state.input.type_select.reset_scope();
    }
    state.match_state.input.selection_order = reconciled;
    state.match_state.input.selection_order_pending = false;
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
                native_selection_mode_reset = true;
                // Deselect's Follow clear precedes Select, even for a singleton
                // that will immediately be reselected. A final-set diff loses it.
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
    use crate::sim::components::Health;
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
            entity.type_ref = sim.interner.intern(type_name);
            entity.owner = sim.interner.intern("Americans");
            entity.selected = selected;
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
                    Health { current: 100 },
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

fn queue_stop_for_selected(state: &mut AppState) {
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
    if selected_ids.is_empty() {
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

/// The ordinary Guard selection loop730D79..730E8A. Simulation owns each
/// actor's eligibility/current Cell and the shared MegaMission codec. This
/// caller preserves list order, leaves the voice latch enabled for every
/// actor, and emits GuardSound even if a nonempty selection was all skipped.
/// Planning-mode637AA0/63A0D0 remains its separate unimplemented input route.
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
    let commands: Vec<Command> = selected_ids
        .iter()
        .filter_map(|&entity_id| deploy_key_command(sim, state.rules(), entity_id))
        .collect();
    for cmd in commands {
        schedule_command(state, &owner, cmd);
    }
}

/// The deploy key's per-object decision, in selection order. The command's
/// historical `DeployMcv` wire name also carries a SimpleDeployer toggle.
fn deploy_key_command(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    entity_id: u64,
) -> Option<Command> {
    let entity = sim.entities().get(entity_id)?;
    let obj = rules.and_then(|rules| rules.object(sim.interner.resolve(entity.type_ref())));
    match entity.category {
        EntityCategory::Structure => {
            // Garrisoned building → evacuate occupants.
            if obj.is_some_and(|object| object.can_be_occupied)
                && entity
                    .passenger_role
                    .cargo()
                    .is_some_and(|cargo| !cargo.is_empty())
            {
                Some(Command::UnloadPassengers {
                    transport_id: entity_id,
                })
            } else {
                rules
                    .is_some_and(|rules| {
                        sim.should_show_undeploy_building_command(entity_id, rules)
                    })
                    .then_some(Command::UndeployBuilding { entity_id })
            }
        }
        EntityCategory::Infantry => obj
            .is_some_and(|object| object.deploy_fire)
            .then_some(Command::ToggleInfantryDeploy { entity_id }),
        _ => {
            // A loaded transport (including aircraft) keeps the unload action.
            super::transport_orders::transport_unload_command(entity, obj).or_else(|| {
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
fn live_group_members(state: &AppState, group: &[u64]) -> Vec<u64> {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return Vec::new();
    };
    group
        .iter()
        .copied()
        .filter(|id| sim.entities().get(*id).is_some())
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
                    e.position.rx as i64 * LEPTONS_PER_CELL + e.position.sub_x.to_num::<i64>(),
                    e.position.ry as i64 * LEPTONS_PER_CELL + e.position.sub_y.to_num::<i64>(),
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
    let group = state.match_state.input.control_groups[group_idx].clone();
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
    // Only live members count towards "the selection is exactly the group" —
    // membership is derived by scanning live objects, so a dead unit has
    // already left its group.
    let live_group = live_group_members(state, &group);
    let last_press = state
        .match_state
        .input
        .last_control_group_press
        .map(|(slot, at)| (slot, at.elapsed()));
    let action = action_override.unwrap_or_else(|| {
        control_group_press_action(
            KeyModifiers::default(),
            group_idx,
            &live_group,
            &selected,
            last_press,
        )
        .expect("a bare team-select binding always resolves")
    });

    match action {
        GroupPressAction::Assign => {
            assign_control_group(
                &mut state.match_state.input.control_groups,
                group_idx,
                selected,
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
            let mut final_ids = selected;
            final_ids.extend(live_group);
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
                e.category == EntityCategory::Structure
                    && owner_name.map_or(true, |o| {
                        sim.interner.resolve(e.owner()).eq_ignore_ascii_case(o)
                    })
                    && rules
                        .and_then(|r| r.object(sim.interner.resolve(e.type_ref())))
                        .map_or(false, |o| o.undeploys_into.is_some())
            });
            if let Some(entity) = conyard {
                log::info!(
                    "H: jumping to ConYard {} at ({}, {})",
                    sim.interner.resolve(entity.type_ref()),
                    entity.position.rx,
                    entity.position.ry
                );
                return Some((entity.position.rx, entity.position.ry));
            }
            // Second pass: look for an MCV (unit with DeploysInto=).
            let mcv = sim.entities().values().find(|e| {
                e.category != EntityCategory::Structure
                    && owner_name.map_or(true, |o| {
                        sim.interner.resolve(e.owner()).eq_ignore_ascii_case(o)
                    })
                    && rules
                        .and_then(|r| r.object(sim.interner.resolve(e.type_ref())))
                        .map_or(false, |o| o.deploys_into.is_some())
            });
            if let Some(entity) = mcv {
                log::info!(
                    "H: jumping to MCV {} at ({}, {})",
                    sim.interner.resolve(entity.type_ref()),
                    entity.position.rx,
                    entity.position.ry
                );
                return Some((entity.position.rx, entity.position.ry));
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
        .and_then(|rt| crate::map::waypoints::first_multiplayer_start(&rt.resources.waypoints))
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
    use super::deploy_key_command;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::command::Command;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    use crate::sim::world::Simulation;

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
            .passenger_role = PassengerRole::Transport { cargo };

        assert_eq!(
            deploy_key_command(&sim, Some(&rules), heli),
            Some(Command::DeployMcv { entity_id: heli })
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), transport),
            Some(Command::UnloadPassengers {
                transport_id: transport
            })
        );
        assert_eq!(
            deploy_key_command(&sim, Some(&rules), infantry),
            Some(Command::ToggleInfantryDeploy {
                entity_id: infantry
            })
        );
        assert_eq!(deploy_key_command(&sim, Some(&rules), aircraft), None);
        assert_eq!(deploy_key_command(&sim, Some(&rules), plain), None);
        sim.entities_mut()
            .get_mut(heli)
            .unwrap()
            .low_bridge_tube_state = Some(
            crate::sim::movement::tube_movement::LowBridgeTubeMovementState {
                tube_id: crate::map::tube_facts::TubeId(0),
                cursor: 0,
                target: crate::sim::components::DriveCoord::cell(10, 10, 0),
            },
        );
        assert_eq!(deploy_key_command(&sim, Some(&rules), heli), None);
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
                deploy_key_command(&sim, Some(rules), id),
                Some(Command::DeployMcv { entity_id: id }),
            );
        }
    }
}

#[cfg(test)]
mod control_group_tests {
    use super::{
        GroupPressAction, KeyModifiers, assign_control_group, control_group_press_action,
        trimmed_centroid_leptons,
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
