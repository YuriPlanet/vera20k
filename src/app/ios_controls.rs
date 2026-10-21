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
    /// The UI contact owns its button until release, even if a held shortcut
    /// finger lifts first. `None` leaves tactical drags on the left button.
    native_ui_pointer: Option<MouseButton>,
    pub(super) show_tags: bool,
    pub(super) save_failed: bool,
    last_service: Option<Instant>,
}

impl IosControls {
    pub(super) fn order_modifiers(&self) -> (bool, bool) {
        // Target-mode fingers only modify battlefield orders. In particular,
        // AttackMove's virtual Shift must not batch-produce sidebar cameos.
        if self.native_ui_pointer.is_some() {
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

    fn begin_drag(&mut self, native_ui: bool) -> MouseButton {
        self.native_ui_pointer = native_ui.then(|| {
            if self.touch.active_mode() == Some(Shortcut::Cancel) {
                MouseButton::Right
            } else {
                MouseButton::Left
            }
        });
        self.drag_button()
    }

    fn drag_button(&self) -> MouseButton {
        self.native_ui_pointer.unwrap_or(MouseButton::Left)
    }

    fn clear_pointer(&mut self) {
        self.native_ui_pointer = None;
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
    controls.clear_pointer();
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
                pointer_button(state, MouseButton::Left, ElementState::Pressed);
                pointer_button(state, MouseButton::Left, ElementState::Released);
                state.match_state.input.ios_controls.force_click = false;
                state.match_state.input.ios_controls.attack_move_click = false;
                state.match_state.input.ios_controls.click_in_progress = false;
            }
            TouchAction::BeginDrag { position } => {
                let button = state
                    .match_state
                    .input
                    .ios_controls
                    .begin_drag(!geometry.touch.world.contains(position));
                pointer_move(state, position);
                pointer_button(state, button, ElementState::Pressed);
            }
            TouchAction::Drag { position } => pointer_move(state, position),
            TouchAction::EndDrag { position } => {
                let button = state.match_state.input.ios_controls.drag_button();
                pointer_move(state, position);
                pointer_button(state, button, ElementState::Released);
                state.match_state.input.ios_controls.clear_pointer();
            }
            TouchAction::CancelPointer => {
                state.match_state.input.ios_controls.clear_pointer();
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

fn pointer_button(state: &mut AppState, button: MouseButton, edge: ElementState) {
    if button == MouseButton::Left
        && edge == ElementState::Released
        && state.armed_building_type().is_some()
    {
        // A real finger may move and release without any intervening render.
        // Re-evaluate the shared foundation owner at this touch's actual point:
        // PLACE still goes through ordinary release admission and command intake.
        super::match_runtime::sim_tick::update_building_placement_preview(state);
    }
    super::input::tooltips::on_button_event(state);
    // Sidebar RMB_DOWN owns production suspension/abandon through its ordinary
    // gadget path; a tactical cancel ladder alone cannot operate a HUD cameo.
    dispatch::handle_mouse_input(state, button, edge);
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

    fn pointer_layout(scale: f32) -> TouchLayout {
        TouchLayout {
            scale_factor: scale,
            world: TouchRect {
                x: 0.0,
                y: 0.0,
                width: 700.0 * scale,
                height: 400.0 * scale,
            },
            joystick: None,
            controls: vec![ControlRegion {
                rect: TouchRect {
                    x: 50.0 * scale,
                    y: 300.0 * scale,
                    width: 50.0 * scale,
                    height: 50.0 * scale,
                },
                command: Shortcut::Cancel,
            }],
            excluded: vec![],
        }
    }

    fn send(
        controls: &mut IosControls,
        layout: &TouchLayout,
        id: u64,
        phase: TouchPhase,
        position: [f32; 2],
        now: Instant,
    ) -> Vec<TouchAction> {
        controls.touch.handle(
            Touch {
                device_id: DeviceId::dummy(),
                id,
                phase,
                location: PhysicalPosition::new(position[0] as f64, position[1] as f64),
                force: None,
            },
            now,
            layout,
        )
    }

    #[test]
    fn held_cancel_hud_keeps_right_button_until_release_and_never_cancels_world() {
        for scale in [1.0, 3.0] {
            for cancel_phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
                let layout = pointer_layout(scale);
                let mut controls = IosControls::default();
                let now = Instant::now();
                let cancel = [75.0 * scale, 325.0 * scale];
                let hud = [750.0 * scale, 100.0 * scale];
                assert!(
                    send(&mut controls, &layout, 1, TouchPhase::Started, cancel, now).is_empty()
                );
                // Repeat while X is held: each HUD contact must be its own
                // right press/release, so pause then cancel stays sidebar-owned.
                for id in [2, 3] {
                    assert_eq!(
                        send(&mut controls, &layout, id, TouchPhase::Started, hud, now),
                        vec![TouchAction::BeginDrag { position: hud }]
                    );
                    assert_eq!(
                        controls.begin_drag(!layout.world.contains(hud)),
                        MouseButton::Right
                    );
                    assert_eq!(controls.order_modifiers(), (false, false));
                    if id == 3 {
                        // Dropping the mode finger first must not change the
                        // HUD release to Left or emit a battlefield cancel.
                        assert!(
                            send(&mut controls, &layout, 1, cancel_phase, cancel, now).is_empty()
                        );
                        assert_eq!(controls.touch.active_mode(), None);
                    }
                    assert_eq!(
                        send(&mut controls, &layout, id, TouchPhase::Ended, hud, now),
                        vec![TouchAction::EndDrag { position: hud }]
                    );
                    assert_eq!(controls.drag_button(), MouseButton::Right);
                    controls.clear_pointer();
                    assert_eq!(controls.drag_button(), MouseButton::Left);
                }
                assert!(!controls.touch.is_interacting());
            }
        }
    }

    #[test]
    fn hud_left_drag_does_not_change_button_when_cancel_is_pressed_later() {
        let layout = pointer_layout(1.0);
        let mut controls = IosControls::default();
        let now = Instant::now();
        let hud = [750.0, 100.0];
        let cancel = [75.0, 325.0];
        assert_eq!(
            send(&mut controls, &layout, 1, TouchPhase::Started, hud, now),
            vec![TouchAction::BeginDrag { position: hud }]
        );
        assert_eq!(controls.begin_drag(true), MouseButton::Left);
        assert!(send(&mut controls, &layout, 2, TouchPhase::Started, cancel, now).is_empty());
        let moved = [750.0, 130.0];
        assert_eq!(
            send(&mut controls, &layout, 1, TouchPhase::Moved, moved, now),
            vec![TouchAction::Drag { position: moved }]
        );
        assert_eq!(controls.drag_button(), MouseButton::Left);
        assert_eq!(
            send(&mut controls, &layout, 1, TouchPhase::Ended, moved, now),
            vec![TouchAction::EndDrag { position: moved }]
        );
        assert_eq!(controls.drag_button(), MouseButton::Left);
        controls.clear_pointer();
        assert!(send(&mut controls, &layout, 2, TouchPhase::Ended, cancel, now).is_empty());
        assert!(!controls.touch.is_interacting());
    }

    #[test]
    fn cancelled_hud_right_pointer_drops_capture_and_next_contact_uses_left() {
        let layout = pointer_layout(1.0);
        let mut controls = IosControls::default();
        let now = Instant::now();
        let cancel = [75.0, 325.0];
        let hud = [750.0, 100.0];
        send(&mut controls, &layout, 1, TouchPhase::Started, cancel, now);
        send(&mut controls, &layout, 2, TouchPhase::Started, hud, now);
        assert_eq!(controls.begin_drag(true), MouseButton::Right);
        assert_eq!(
            send(&mut controls, &layout, 2, TouchPhase::Cancelled, hud, now),
            vec![TouchAction::CancelPointer]
        );
        controls.clear_pointer();
        assert_eq!(controls.drag_button(), MouseButton::Left);
        assert!(send(&mut controls, &layout, 2, TouchPhase::Ended, hud, now).is_empty());
        assert!(send(&mut controls, &layout, 1, TouchPhase::Ended, cancel, now).is_empty());
        assert_eq!(
            send(&mut controls, &layout, 3, TouchPhase::Started, hud, now),
            vec![TouchAction::BeginDrag { position: hud }]
        );
        assert_eq!(controls.begin_drag(true), MouseButton::Left);
    }

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
        controls.native_ui_pointer = Some(MouseButton::Left);
        assert_eq!(controls.order_modifiers(), (false, false));
        controls.native_ui_pointer = None;
        controls.click_in_progress = true;
        controls.force_click = true;
        assert_eq!(controls.order_modifiers(), (true, false));
        controls.force_click = false;
        assert_eq!(controls.order_modifiers(), (false, false));
    }
}
