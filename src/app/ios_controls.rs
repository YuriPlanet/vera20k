//! Landscape touch host. Gesture recognition and layout are pure modules;
//! gameplay remains owned by the existing pointer, hotkey and camera dispatch.

use std::path::Path;
use std::time::Instant;
use winit::event::{ElementState, MouseButton, Touch};

use super::input::{
    camera, dispatch,
    hotkeys::{HotkeyCommand, HotkeyResolution},
};
use super::ios_battle_touch::{BattleTouch, TouchAction};
use super::ios_shortcuts::{EditorOutcome, LandscapeLayout, Preferences, Shortcut, ShortcutEditor};
use super::{App, AppState, GameScreen};

const PREFERENCES_FILE: &str = "touch-shortcuts.json";

#[derive(Default)]
pub(crate) struct IosControls {
    pub(super) touch: BattleTouch,
    pub(super) prefs: Preferences,
    pub(super) editor: Option<ShortcutEditor>,
    pub(super) force_click: bool,
    pub(super) attack_move_click: bool,
    pub(super) click_in_progress: bool,
    native_ui_pointer: bool,
    pub(super) show_tags: bool,
    pub(super) save_failed: bool,
    last_service: Option<Instant>,
}

impl IosControls {
    pub(super) fn order_modifiers(&self) -> (bool, bool) {
        // Target-mode fingers only modify battlefield orders. In particular,
        // AttackMove's virtual Shift must not batch-produce sidebar cameos.
        if self.native_ui_pointer {
            return (false, false);
        }
        if self.click_in_progress {
            return (
                self.force_click || self.attack_move_click,
                self.attack_move_click,
            );
        }
        match self.touch.active_mode() {
            Some(Shortcut::ForceAttack) => (true, false),
            Some(Shortcut::AttackMove) => (true, true),
            _ => (false, false),
        }
    }
}

pub(crate) fn initialize(state: &mut AppState) {
    match Preferences::load(Path::new(PREFERENCES_FILE)) {
        Ok(prefs) => state.match_state.input.ios_controls.prefs = prefs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Ignoring invalid touch shortcuts: {error}"),
    }
}

pub(crate) fn enabled(state: &AppState) -> bool {
    state.frontend.screen == GameScreen::InGame
        && !state.match_state.paused()
        && !state.match_state.match_presentation.in_game_menu.is_open()
        && state.frontend.keyboard_dialog.is_none()
        && state.match_state.sim_runtime.is_some()
        && state.match_state.scenario_outcome.is_none()
        && state.match_state.scenario_exit.is_none()
        && !state.platform.focus_freeze_active()
}

pub(crate) fn layout(state: &AppState) -> LandscapeLayout {
    let width = state.renderer.gpu.config.width as f32;
    let height = state.renderer.gpu.config.height as f32;
    let column_left = state.sidebar_frame().frame_left();
    let window_left = state.tactical_zoom().map_or(column_left, |zoom| {
        zoom.source_to_window(column_left, 0.0).0
    });
    let controls = &state.match_state.input.ios_controls;
    LandscapeLayout::new(
        width,
        height,
        state.platform.window.scale_factor() as f32,
        (width - window_left).max(0.0),
        &controls.prefs,
        controls.editor.as_ref(),
    )
}

/// A lost gesture is aborted, never released onto the current world target.
pub(crate) fn reset(state: &mut AppState) {
    let controls = &mut state.match_state.input.ios_controls;
    let captured = controls.touch.is_interacting();
    controls.touch.reset();
    controls.force_click = false;
    controls.attack_move_click = false;
    controls.click_in_progress = false;
    controls.native_ui_pointer = false;
    controls.last_service = None;
    if captured {
        App::release_in_game_pointer_state(state);
    }
}

pub(crate) fn handle_touch(state: &mut AppState, touch: Touch) {
    let geometry = layout(state);
    let actions =
        state
            .match_state
            .input
            .ios_controls
            .touch
            .handle(touch, Instant::now(), &geometry.touch);
    for action in actions {
        match action {
            TouchAction::Click {
                position,
                force,
                attack_move,
            } => {
                state.match_state.input.ios_controls.click_in_progress = true;
                state.match_state.input.ios_controls.force_click = force;
                state.match_state.input.ios_controls.attack_move_click = attack_move;
                pointer_move(state, position);
                pointer_button(state, ElementState::Pressed);
                pointer_button(state, ElementState::Released);
                state.match_state.input.ios_controls.force_click = false;
                state.match_state.input.ios_controls.attack_move_click = false;
                state.match_state.input.ios_controls.click_in_progress = false;
            }
            TouchAction::BeginDrag { position } => {
                state.match_state.input.ios_controls.native_ui_pointer =
                    !geometry.touch.world.contains(position);
                pointer_move(state, position);
                pointer_button(state, ElementState::Pressed);
            }
            TouchAction::Drag { position } => pointer_move(state, position),
            TouchAction::EndDrag { position } => {
                pointer_move(state, position);
                pointer_button(state, ElementState::Released);
                state.match_state.input.ios_controls.native_ui_pointer = false;
            }
            TouchAction::CancelPointer => {
                state.match_state.input.ios_controls.native_ui_pointer = false;
                App::release_in_game_pointer_state(state);
            }
            TouchAction::Cancel => dispatch::right_click_cancel_ladder(state),
            TouchAction::CameraPan { delta } => request_window_pan(state, delta),
            TouchAction::Shortcut { command, held_long } => shortcut(state, command, held_long),
        }
    }
    state.platform.window.request_redraw();
}

fn pointer_move(state: &mut AppState, point: [f32; 2]) {
    state.set_cursor_from_window(point[0], point[1]);
    super::input::tooltips::on_mouse_move(state);
    dispatch::handle_cursor_moved_in_game(state);
}

fn pointer_button(state: &mut AppState, edge: ElementState) {
    if edge == ElementState::Released && state.armed_building_type().is_some() {
        // A real finger may move and release without any intervening render.
        // Re-evaluate the shared foundation owner at this touch's actual point:
        // PLACE still goes through ordinary release admission and command intake.
        super::match_runtime::sim_tick::update_building_placement_preview(state);
    }
    super::input::tooltips::on_button_event(state);
    dispatch::handle_mouse_input(state, MouseButton::Left, edge);
}

fn request_window_pan(state: &mut AppState, delta: [f32; 2]) {
    let movement = pan_delta_to_world(
        delta,
        state.tactical_zoom(),
        state.match_state.input.zoom_level,
    );
    camera::request_touch_scroll(state, movement);
}

fn pan_delta_to_world(
    delta: [f32; 2],
    geometry: Option<crate::render::upscale_pass::ZoomGeometry>,
    world_zoom: f32,
) -> (f32, f32) {
    // This is a vector, not a point: window_to_source clamps points at the
    // letterbox, which would discard negative movement or movement in a bar.
    let scale = geometry.map_or(1.0, |geometry| geometry.scale()) * world_zoom;
    (-delta[0] / scale, -delta[1] / scale)
}

/// WERHD's full-stick rate is 30 canvas pixels per 60 Hz frame. Wall-clock
/// integration here keeps that speed independent of display refresh rate.
pub(crate) fn service(state: &mut AppState) {
    if !enabled(state) {
        reset(state);
        return;
    }
    let controls = &mut state.match_state.input.ios_controls;
    let now = Instant::now();
    let dt = controls
        .last_service
        .replace(now)
        .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.05));
    let velocity = controls.touch.camera_velocity();
    // Negate the stick vector for the shared drag-map vector conversion:
    // the stick scrolls toward its displacement, unlike dragging the map.
    request_window_pan(
        state,
        [-velocity[0] * 1800.0 * dt, -velocity[1] * 1800.0 * dt],
    );
}

fn shortcut(state: &mut AppState, command: Shortcut, held_long: bool) {
    if command == Shortcut::Edit && state.match_state.input.ios_controls.editor.is_none() {
        let controls = &mut state.match_state.input.ios_controls;
        controls.editor = Some(ShortcutEditor::new(&controls.prefs));
        controls.save_failed = false;
        reset(state);
        return;
    }
    if let Some(editor) = state.match_state.input.ios_controls.editor.as_mut() {
        match editor.handle(command) {
            EditorOutcome::Save(prefs) => match prefs.save(Path::new(PREFERENCES_FILE)) {
                Ok(()) => {
                    let controls = &mut state.match_state.input.ios_controls;
                    controls.prefs = prefs;
                    controls.editor = None;
                    controls.save_failed = false;
                }
                Err(error) => {
                    state.match_state.input.ios_controls.save_failed = true;
                    log::error!("Could not save touch shortcuts: {error}");
                }
            },
            EditorOutcome::Discard => {
                state.match_state.input.ios_controls.editor = None;
                state.match_state.input.ios_controls.save_failed = false;
            }
            EditorOutcome::Changed => state.match_state.input.ios_controls.save_failed = false,
            EditorOutcome::None => {}
        }
        reset(state);
        return;
    }
    if matches!(command, Shortcut::PagePrevious | Shortcut::PageNext) {
        let count = layout(state).page_count.max(1);
        let page = &mut state.match_state.input.ios_controls.prefs.page;
        *page = if command == Shortcut::PageNext {
            (*page + 1) % count
        } else {
            (*page + count - 1) % count
        };
        return;
    }
    let hotkey = match command {
        Shortcut::Home => HotkeyCommand::CenterBase,
        Shortcut::Type => {
            dispatch::dispatch_command_bar(state, 3, false);
            return;
        }
        Shortcut::Combat => HotkeyCommand::CombatantSelect,
        Shortcut::Event => HotkeyCommand::CenterOnRadarEvent,
        Shortcut::Scatter => HotkeyCommand::ScatterObject,
        Shortcut::Deploy => HotkeyCommand::DeployObject,
        Shortcut::Next => HotkeyCommand::NextObject,
        Shortcut::Stop => HotkeyCommand::StopObject,
        Shortcut::Guard => HotkeyCommand::GuardObject,
        Shortcut::Planning => HotkeyCommand::PlanningMode,
        Shortcut::Repair => HotkeyCommand::ToggleRepair,
        Shortcut::Sell => HotkeyCommand::ToggleSell,
        Shortcut::Pause => HotkeyCommand::Options,
        Shortcut::Team1 | Shortcut::Team2 | Shortcut::Team3 => {
            let group = match command {
                Shortcut::Team1 => 1,
                Shortcut::Team2 => 2,
                _ => 3,
            };
            if held_long {
                HotkeyCommand::TeamCreate(group)
            } else {
                HotkeyCommand::TeamSelect(group)
            }
        }
        Shortcut::View => {
            state.match_state.input.ios_controls.show_tags ^= true;
            return;
        }
        Shortcut::Cancel => {
            dispatch::right_click_cancel_ladder(state);
            return;
        }
        // Held target modes are interpreted by the gesture owner on the
        // other finger's map click. Their release never issues a stray order.
        Shortcut::AttackMove | Shortcut::ForceAttack => return,
        _ => return,
    };
    dispatch::handle_hotkey_pressed(
        state,
        HotkeyResolution::Command(hotkey),
        winit::keyboard::KeyCode::F24,
    );
}

#[cfg(test)]
mod tests {
    use super::super::ios_battle_touch::{ControlRegion, TouchLayout, TouchRect};
    use super::*;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, TouchPhase};

    #[test]
    fn map_drag_moves_camera_in_opposite_direction_at_world_and_options_zoom() {
        assert_eq!(pan_delta_to_world([40.0, 20.0], None, 2.0), (-20.0, -10.0));
        use crate::render::upscale_pass::{FrameShape, ZoomGeometry};
        let geometry = ZoomGeometry::new(1600, 800, 200, FrameShape::Window);
        assert_eq!(
            pan_delta_to_world([40.0, 20.0], Some(geometry), 1.0),
            (-20.0, -10.0)
        );
        let bars = ZoomGeometry::new(1600, 800, 200, FrameShape::Original);
        assert_eq!(
            pan_delta_to_world([-40.0, -20.0], Some(bars), 1.0),
            (20.0, 10.0)
        );
    }

    #[test]
    fn click_snapshot_overrides_a_later_held_mode_even_when_snapshot_is_plain() {
        let mut controls = IosControls::default();
        let rect = TouchRect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let layout = TouchLayout {
            scale_factor: 1.0,
            world: rect,
            joystick: None,
            controls: vec![ControlRegion {
                rect,
                command: Shortcut::AttackMove,
            }],
            excluded: vec![],
        };
        controls.touch.handle(
            Touch {
                device_id: DeviceId::dummy(),
                id: 1,
                phase: TouchPhase::Started,
                location: PhysicalPosition::new(25.0, 25.0),
                force: None,
            },
            Instant::now(),
            &layout,
        );
        assert_eq!(controls.order_modifiers(), (true, true));
        controls.native_ui_pointer = true;
        assert_eq!(controls.order_modifiers(), (false, false));
        controls.native_ui_pointer = false;
        controls.click_in_progress = true;
        controls.force_click = true;
        assert_eq!(controls.order_modifiers(), (true, false));
        controls.force_click = false;
        assert_eq!(controls.order_modifiers(), (false, false));
    }
}
