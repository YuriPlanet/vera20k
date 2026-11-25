//! Keyboard command dispatch: TypeSelect key edges, one-shot hotkey presses,
//! sidebar tab and ready-building commands, window commands, super weapon and
//! command-bar buttons, the pause and options menus, and the home-camera jump.

#[cfg(feature = "dev-overlays")]
use super::KeyModifiers;
#[cfg(feature = "dev-overlays")]
use super::handle_dev_hotkey_pressed;
use super::selection_navigation;
use super::{
    GroupPress, GroupPressAction, TYPE_SELECT_TAP_ACTION_LINE_POLICY, TYPE_SELECT_TAP_VOICE_POLICY,
    WheelAction, apply_selection_action_line_policy, apply_selection_mutation,
    apply_sidebar_action, handle_control_group_command, push_local_eva,
    queue_control_group_assignment, queue_deploy_undeploy_for_selected, queue_guard_for_selected,
    queue_scatter_for_selected, queue_selection_snapshot_command, queue_stop_for_selected,
    selected_stable_ids_in_order, sidebar_scroll_command,
};
use crate::app::AppState;
use crate::app::input::commands::{
    preferred_command_owner, preferred_local_owner_name, queue_house_cheer,
};
use crate::app::input::entity_pick::{
    compute_type_select_tap_with_playfield, map_entity_creation_order,
};
use crate::app::input::hotkeys::{HotkeyCommand, HotkeyFallback, HotkeyResolution};
use crate::app::input::sidebar_eva;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::types::OrderMode;
use crate::ui::sidebar::{SidebarAction, SidebarTab};
use winit::event::ElementState;

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
        state.match_state.input.type_select.press(
            physical_code,
            crate::util::time::Instant::now(),
            repeat,
        );
        return true;
    }
    if !is_type_select && !state.match_state.input.type_select.owns_key(physical_code) {
        return false;
    }
    let execute_tap = state
        .match_state
        .input
        .type_select
        .release(physical_code, crate::util::time::Instant::now());
    if execute_tap {
        execute_type_select_tap(state);
    }
    true
}

fn execute_type_select_tap(state: &mut AppState) {
    // `TypeSelectCommand::ExecuteTap @ 0x00732950` does nothing at all while
    // A8B538 is set (0x00732961), the defeat guard P, Health and Y share.
    if state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|rt| !selection_navigation::selection_navigation_allowed(&rt.simulation))
    {
        return;
    }
    state.match_state.input.type_select.prepare_tap_scope();
    let result = {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .runtime()
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
                .runtime()
                .map(|rt| &rt.simulation),
            state.rules(),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        let fog = (!state.match_state.sandbox_full_visibility)
            .then_some((sim.fog(), sim.house_alliances().as_map()));
        compute_type_select_tap_with_playfield(
            sim,
            &screen_order,
            &map_order,
            &current,
            fog,
            preferred_local_owner_name(state).as_deref(),
            state.rules(),
            Some(sim.interner()),
            state.match_state.input.type_select.across_map,
            sim.playfield_bounds().is_some(),
        )
    };
    let outcome = result.outcome;
    let across_map = result.across_map;
    apply_selection_mutation(state, result.mutation, TYPE_SELECT_TAP_VOICE_POLICY);
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
    if crate::app::match_runtime::network_failure::pending(state) {
        return;
    }
    handle_hotkey_pressed_inner(state, resolution, physical_code);
    super::commands::reconcile_paid_super_weapon_cursor(state);
}

fn handle_hotkey_pressed_inner(
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
            crate::app::input::diplomacy::open(
                state,
                crate::app::input::diplomacy::DiplomacyRequest::Hotkey,
            );
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
/// `ObjectClass::FindFactory` 5F5C20 (0,0), with Ares (0,1)
/// ([`ready_tab_factory`]), and `HouseClass::Begin_Building_Placement`
/// 4FB840. Native registers exactly one such building per structure tab
/// (`BuildingClass::SetSidebarTabObject` 734250 by the type's BuildCat),
/// which is this owner's ready building for that tab.
fn place_ready_building_for_tab(state: &mut AppState, tab: SidebarTab) {
    let Some(owner_name) = preferred_command_owner(state) else {
        return;
    };
    let ready_target = {
        let Some(runtime) = state.match_state.sim_runtime.runtime() else {
            return;
        };
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let Some(owner_id) = sim.interner().get(&owner_name) else {
            return;
        };
        state
            .match_state
            .match_presentation
            .sidebar_projection
            .ready_building_for_tab(sim, owner_id, tab)
            .and_then(|(type_id, held)| {
                // Manual_Place4FB840 refuses a null factory; placement start
                // centres the foundation on it.
                let builder = ready_tab_factory(sim, rules, held)?;
                Some((
                    owner_id,
                    held,
                    builder,
                    sim.interner().resolve(type_id).to_string(),
                ))
            })
    };
    let Some((owner_id, held, builder, held_type)) = ready_target else {
        return;
    };
    if state
        .match_state
        .input
        .begin_building_placement(&held_type, held, owner_id)
    {
        centre_placement_on_factory(state, builder);
        // ManualPlace4FB881 calls UnselectAll48DC90 only on entry.
        queue_selection_snapshot_command(state, Vec::new(), false);
        let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
        gadgets.disable_cursor_modes();
    }
}

/// The factory a structure tab hotkey hands Manual_Place4FB840 with the
/// tab's ready building `held`: its `ObjectClass::FindFactory(0, 0)`
/// (`0x005F5C20`), or with Ares (0, 1), a powered one
/// (Set{Structure,Defense}TabCommandClass_Execute_Power, Ares.dll
/// `10025150`/`10025130`, hooks `0x00535DB6`/`0x00535E76`).
pub(in crate::app::input) fn ready_tab_factory(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    held: u64,
) -> Option<u64> {
    crate::sim::production::object_find_factory(sim, rules, held, false, rules.ares_form())
}

/// Placement start centres the foundation on the Cell of `factory`, the
/// product's `ObjectClass::FindFactory` (0, 0) (`0x005F5C20`) that
/// `HouseClass::Begin_Building_Placement @ 0x004FB840` was handed, at its
/// Location (`ObjectClass::Get_Cell_Packed @ 0x0041BEA0`, at `0x004FB8FE`).
pub(super) fn centre_placement_on_factory(state: &mut AppState, factory: u64) {
    use crate::app::match_runtime::sim_tick::{
        FoundationCentre, update_building_placement_preview,
    };
    let Some(cell) = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let sim = &runtime.simulation;
        let location =
            crate::sim::movement::ground_pose::object_location(sim.entities().get(factory)?, None);
        Some((
            crate::util::lepton::lepton_to_cell_packed(location.x) as u16,
            crate::util::lepton::lepton_to_cell_packed(location.y) as u16,
        ))
    }) else {
        return;
    };
    update_building_placement_preview(state, FoundationCentre::Cell(cell));
}

fn dispatch_retail_hotkey(state: &mut AppState, command: HotkeyCommand) {
    match command {
        // Held camera commands are sampled by queue_keyboard_scroll.
        HotkeyCommand::CameraUp
        | HotkeyCommand::CameraDown
        | HotkeyCommand::CameraLeft
        | HotkeyCommand::CameraRight => {}
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
                .and_then(|minimap| minimap.cycle_radar_event(crate::util::time::Instant::now()));
            if let Some((rx, ry)) = event {
                crate::app::input::camera::center_camera_on_cell(state, rx, ry);
                // With a building armed, its foundation follows the cursor's
                // new Cell (`Center_View_On_Radar_Event`, `0x00734200`).
                crate::app::match_runtime::sim_tick::update_building_placement_preview(
                    state,
                    crate::app::match_runtime::sim_tick::FoundationCentre::Cursor,
                );
            }
        }
        HotkeyCommand::ScreenCapture => {
            state.match_state.input.retail_screenshot_requested = true;
            state.platform.window.request_redraw();
        }
        HotkeyCommand::View(slot) => crate::app::input::camera::recall_view_bookmark(state, slot),
        HotkeyCommand::SetView(slot) => crate::app::input::camera::set_view_bookmark(state, slot),
        HotkeyCommand::TeamSelect(slot) => {
            handle_control_group_command(state, slot, GroupPress::Key)
        }
        HotkeyCommand::TeamAddSelect(slot) => handle_control_group_command(
            state,
            slot,
            GroupPress::Fixed(GroupPressAction::AddToSelection),
        ),
        HotkeyCommand::TeamCreate(slot) => {
            handle_control_group_command(state, slot, GroupPress::Fixed(GroupPressAction::Assign))
        }
        HotkeyCommand::TeamCenter(slot) => {
            handle_control_group_command(state, slot, GroupPress::Fixed(GroupPressAction::Center))
        }
        // Native SidebarUp/Down execute SidebarClass::Scroll 0x6A6A00 on the
        // active strip (up=true/false, tab -1), as the wheel does.
        HotkeyCommand::SidebarUp => sidebar_scroll_command(state, WheelAction::SidebarUp),
        HotkeyCommand::SidebarDown => sidebar_scroll_command(state, WheelAction::SidebarDown),
        // VERA: Esc leaves the layout edit mode without saving, before it
        // would open the menu.
        HotkeyCommand::Options
            if crate::app::input::layout_edit::active(state) && !state.match_state.paused() =>
        {
            crate::app::input::layout_edit::cancel(state)
        }
        HotkeyCommand::Options => handle_options_hotkey(state),
        HotkeyCommand::CenterView => crate::app::input::camera::center_view_on_selection(state),
        HotkeyCommand::Follow => crate::app::input::camera::toggle_follow_target(state),
        HotkeyCommand::PlanningMode => crate::app::input::planning_mode::toggle(state, false),
        // DeleteCommand731A10: the retained planning node, else the selected beacon.
        HotkeyCommand::Delete => {
            if !crate::app::input::planning_mode::delete_highlighted(state) {
                crate::app::input::beacon::delete_selected(state);
            }
        }
        HotkeyCommand::ScatterObject => queue_scatter_for_selected(state),
        HotkeyCommand::AllToCheer => queue_house_cheer(state),
        HotkeyCommand::VeterancyNav => selection_navigation::execute_category_navigation(
            state,
            crate::app::types::CategoryNavigationKind::Veterancy,
        ),
        HotkeyCommand::ToggleAlliance => {
            crate::app::input::diplomacy::toggle_alliance_hotkey(state)
        }
        // `TauntCommandClass::Execute 0x00536350`; the command's +4
        // parameter is the catalog's `n + 1`.
        HotkeyCommand::Taunt(n) => crate::app::input::taunt::execute(
            state,
            n as u8 + 1,
            crate::app::match_runtime::frame_pacer::wall_clock_ms(),
        ),
        HotkeyCommand::PageUser => crate::app::input::page_user::execute(state),
        HotkeyCommand::PlaceBeacon => crate::app::input::beacon::execute_command(state),
        #[cfg(feature = "phobos")]
        HotkeyCommand::FireTacticalSw(index) => {
            if state
                .rules()
                .is_some_and(|rules| rules.sw_sidebar.keys_enabled)
                && let Some(entry) =
                    crate::app::presentation::sw_sidebar::positional_entry(state, index)
            {
                let type_id = entry.type_id.clone();
                press_super_weapon(state, &type_id, false, false);
            }
        }
        #[cfg(feature = "phobos")]
        HotkeyCommand::ToggleSwSidebar => {
            if state
                .rules()
                .is_some_and(|rules| rules.sw_sidebar.toggle_key_enabled)
                && crate::app::presentation::sw_sidebar::toggle(state)
            {
                crate::app::input::sw_sidebar::play_main_button_sound(state);
                crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
            }
        }
        HotkeyCommand::AutoLoad => crate::app::input::smart_command::hotkey_pressed(state),
        // A chord runs in the window owner before this dispatch; a binding on
        // a typing key reaches here, after the chat edit.
        HotkeyCommand::PerformanceStats
        | HotkeyCommand::ThreadStats
        | HotkeyCommand::LockCursor
        | HotkeyCommand::FullScreen
        | HotkeyCommand::StringKeys => run_window_command(state, command),
        // VERA: the mouse drags the interface's parts until Save, or until
        // the key comes again and puts them back.
        HotkeyCommand::LayoutEdit => crate::app::input::layout_edit::toggle(state),
        // VERA: show or hide the countdown above every provider building.
        // Presentation only, and saved under the same `[VERA]` key the old
        // Options box wrote, so a match's choice seeds the next scenario.
        HotkeyCommand::SuperweaponTimers => {
            let timers = &mut state
                .match_state
                .match_presentation
                .superweapon_building_timers;
            *timers = !*timers;
            let on = *timers;
            state
                .persistence
                .options_profile
                .superweapon_building_timers = on;
            crate::app::persistence::options::persist_options_profile(state);
            log::info!(
                "Super weapon building timers: {}",
                if on { "shown" } else { "hidden" }
            );
        }
        // VERA: show or hide the convenience help rows (repair/reload and
        // future lines) above friendly buildings and units. Presentation only,
        // and saved under the same `[VERA]` key the Options box writes, so a
        // match's choice seeds the next scenario.
        HotkeyCommand::ConvenienceHelp => {
            let help = &mut state.match_state.match_presentation.convenience_help;
            *help = !*help;
            let on = *help;
            state.persistence.options_profile.convenience_help = on;
            state.match_state.sim_runtime.show_status_rows(on);
            crate::app::persistence::options::persist_options_profile(state);
            log::info!("Convenience help: {}", if on { "shown" } else { "hidden" });
        }
        // A held key: `input::keyboard::in_game_key_edge` tracks it.
        HotkeyCommand::FormationLine => {}
        HotkeyCommand::FormationLock => {
            crate::app::input::context_order::toggle_formation_lock(state);
        }
        // VERA: the next left click names the drop spot.
        HotkeyCommand::Ferry => crate::app::input::ferry::hotkey_pressed(state),
        // VERA: the selected factories take turns being primary.
        HotkeyCommand::RotatePrimary => {
            crate::app::input::primary_rotation::hotkey_pressed(state);
        }
        // VERA: toggle relation colours — recolour the world's units and
        // buildings, the minimap dots and the super-weapon timers by each
        // house's relation to the local observer (self = blue, ally = green,
        // enemy = red, neutral unchanged), so a player in a large melee can tell
        // who is who. Pure presentation: it never touches the simulation, so it
        // cannot affect determinism, networking, or replays.
        HotkeyCommand::RelationColors => {
            let presentation = &mut state.match_state.match_presentation;
            presentation.relation_colors = !presentation.relation_colors;
            state.match_state.refresh_relation_colors();
        }
    }
}

/// The Performance Stats boxes follow the panel the keys showed or hid.
fn follow_performance_stats(state: &mut AppState, shown: bool) {
    if let Some(dialog) = state.frontend.options_dialog.as_mut() {
        dialog.sync_performance_stats(shown);
    }
    if let Some(page) = state.match_state.match_presentation.options_page.as_mut() {
        page.dialog.sync_performance_stats(shown);
    }
    crate::app::App::shell_menu_follow_performance_stats(state, shown);
    log::info!(
        "Performance stats: {}",
        if shown { "shown" } else { "hidden" }
    );
}

/// VERA's every-screen commands ([`HotkeyBindings::window_command`]): from the
/// window owner's key handling on every screen, or from a battle's hotkey
/// dispatch for a binding on a typing key.
///
/// [`HotkeyBindings::window_command`]: crate::app::input::hotkeys::HotkeyBindings::window_command
pub(crate) fn run_window_command(state: &mut AppState, command: HotkeyCommand) {
    match command {
        HotkeyCommand::LockCursor => crate::app::input::cursor::toggle_cursor_confinement(state),
        // A capture keeps its fixed surface.
        HotkeyCommand::FullScreen if state.platform.capture_client_size.is_none() => {
            let full_screen = state.platform.window.fullscreen().is_none();
            crate::app::App::set_full_screen(state, full_screen);
        }
        HotkeyCommand::PerformanceStats => {
            let shown = state.renderer.performance_overlay.toggle();
            follow_performance_stats(state, shown);
        }
        // The rows are the panel's: asking for them while it is hidden
        // shows it.
        HotkeyCommand::ThreadStats => {
            let rows = state.renderer.performance_overlay.toggle_thread_rows();
            if rows && !state.renderer.performance_overlay.is_visible() {
                state.renderer.performance_overlay.set_visible(true);
                follow_performance_stats(state, true);
            }
            log::info!("Thread stats: {}", if rows { "shown" } else { "hidden" });
        }
        HotkeyCommand::StringKeys => state.toggle_string_keys_view(),
        _ => return,
    }
    state.platform.window.request_redraw();
}

/// A superweapon cameo press. Phobos's `FireTacticalSW` keys call
/// `LaunchSuper` without the `GUIBuildSound` its button's `Action` plays, so
/// they pass `gui_sound: false`.
pub(super) fn press_super_weapon(
    state: &mut AppState,
    section: &str,
    right: bool,
    gui_sound: bool,
) {
    let ares = state
        .rules()
        .is_some_and(crate::rules::ruleset::RuleSet::ares_form);
    let selection = (|| {
        let runtime = state.match_state.sim_runtime.runtime()?;
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let owner = sim.control_projection().local_command_house()?;
        let type_id = sim.interner().get(section)?;
        let view = crate::sim::superweapon::superweapon_views_for_owner(sim, rules, &owner)
            .into_iter()
            .find(|view| view.type_id == type_id)?;
        Some((view, rules.super_weapon(section)?.action.clone()))
    })();
    let click = super::superweapon_cameo::press(
        &mut state.match_state.input,
        &mut state.match_state.match_presentation.sidebar_gadget_state,
        selection.as_ref().map(|(view, _)| view),
        selection.as_ref().and_then(|(_, action)| action.as_deref()),
        right,
    );
    if gui_sound && click.gui_sound() {
        let sound = state
            .rules()
            .and_then(|rules| rules.general.gui_build_sound.clone());
        crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    }
    match click {
        super::superweapon_cameo::SuperWeaponClick::Refused
            if selection.as_ref().is_some_and(|(view, _)| {
                if ares {
                    view.ares_refusal
                        == Some(crate::sim::superweapon::AresCameoRefusal::InsufficientFunds)
                } else {
                    view.can_fire && !view.can_afford
                }
            }) =>
        {
            let cue = state
                .rules()
                .and_then(|rules| {
                    rules
                        .super_weapon(section)
                        .map(|sw| crate::sim::superweapon::financial_type(rules, sw))
                })
                .and_then(|sw| sw.insufficient_funds_eva.clone());
            if let Some(cue) = cue {
                push_local_eva(state, &cue);
            }
        }
        super::superweapon_cameo::SuperWeaponClick::Target => {
            queue_selection_snapshot_command(state, Vec::new(), false);
            // When the Rules were read the Ares way (`RuleSet::ares_form`),
            // Ares 10034D60 (hook 6AAF9D) plays the type's EVA.SelectTarget,
            // whose constructor default is the stock literal.
            let cue = if ares {
                state
                    .rules()
                    .and_then(|rules| rules.super_weapon(section))
                    .and_then(|sw| sw.select_target_eva.clone())
            } else {
                Some(sidebar_eva::EVA_SELECT_TARGET.to_owned())
            };
            if let Some(cue) = cue {
                push_local_eva(state, &cue);
            }
        }
        super::superweapon_cameo::SuperWeaponClick::Launch(sw_type_id) => {
            if let Some(owner) = preferred_command_owner(state) {
                crate::app::input::commands::queue_super_weapon_click(
                    state,
                    &owner,
                    sw_type_id,
                    (0, 0),
                );
            }
        }
        _ => {}
    }
    if ares
        && click == super::superweapon_cameo::SuperWeaponClick::Refused
        && let Some(refusal) = selection.as_ref().and_then(|(view, _)| view.ares_refusal)
    {
        // 10034DA0: CanFire's refusal plays EVA.Impatient (default -1).
        if refusal == crate::sim::superweapon::AresCameoRefusal::NotReady
            && let Some(cue) = state
                .rules()
                .and_then(|rules| rules.super_weapon(section))
                .and_then(|sw| sw.impatient_eva.clone())
        {
            push_local_eva(state, &cue);
        }
        crate::app::input::messages::post_ares_superweapon_refusal(state, section, refusal);
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
                queue_control_group_assignment(state, group, Vec::new());
                return;
            }
            handle_control_group_command(state, group, GroupPress::CommandBar);
            return;
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
        // TabClass::Action6D0742 calls Enter_Beacon_Placement_Mode731A30
        // directly, without the key's GameMode gate.
        7 => {
            crate::app::input::beacon::enter_placement_mode(state);
            return;
        }
        8 => HotkeyCommand::StopObject,
        9 => {
            crate::app::input::planning_mode::toggle(state, true);
            return;
        }
        10 => HotkeyCommand::AllToCheer,
        _ => return,
    };
    dispatch_retail_hotkey(state, action);
}

// Native 653952 -> GameState1 -> 48C9BA/4F10E0 opens the pause menu.
// The button and ordinary Options hotkey share this modal entry.
pub(super) fn open_pause_menu(state: &mut AppState) {
    // Two states have no tick left to consume the event below: a network
    // match that is waiting for the relay to resume, and a result that is
    // already published and waiting for the losing commander's clip (no frame
    // follows a published result). Both open the same dialog directly, so the
    // button and the Options hotkey keep working there.
    if state.match_state.multiplayer_paused()
        || crate::app::commander_feedback::verdict_is_waiting_for_clip(state)
    {
        // No future EventClass tail exists until the relay resumes. Open the
        // same local dialog directly so its shared Resume remains reachable.
        crate::app::App::enter_in_game_menu_state(
            state,
            crate::ui::pause_menu::InGameMenuState::Menu,
        );
        return;
    }
    if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command) {
        // Queue_Options647040 does not open the dialog. Its accepted local
        // OPTIONS event requests GameState1 at the due EventClass tail.
        let _ = runtime.simulation.queue_options_request();
    }
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

/// Home7315A0 is shared with TAction145. The client's map query forks
/// Dummy writes so a local H press cannot change authoritative simulation.
fn jump_camera_to_base(state: &mut AppState) {
    let coord = state.match_state.sim_runtime.runtime().and_then(|runtime| {
        let cells = runtime
            .simulation
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        state.rules().and_then(|rules| {
            runtime
                .simulation
                .home_camera_coordinate(rules, cells.as_ref())
        })
    });
    crate::app::input::camera::apply_home_camera(state, coord);
}
