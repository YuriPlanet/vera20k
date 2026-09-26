//! Paused-overlay mouse routing for the active in-game Options (`0xBBB`) dialog.
//!
//! Part of the app layer. While `state.paused`, `handle_mouse_input` routes here
//! BEFORE the gadget/tactical dispatch and this consumes the click so it never
//! reaches the tactical viewport (no unit orders behind the overlay). Interaction
//! changes only the visual/stored state — slider thumb + stored value, checkbox
//! check, pressed button frame, and the changed-position value-label flag. The downstream
//! EFFECTS (sim cadence, target-line gate, INI persist) apply on close only, in
//! `app::persistence::options::in_game_options_close` (KD-8).
//!
//! Hit-testing uses the `InGameOptionsAnchor` the overlay render pass cached on
//! `AppState` (KD-6) — the sidebar-anchored Back/Sound/Keyboard button Y is only
//! known at render time, so recomputing the anchor here would be wrong; the cached
//! anchor also guarantees the hit rects exactly match what was drawn.

use winit::event::MouseButton;

use crate::app::AppState;
use crate::ui::shell::descriptor::ControlKind;
use crate::ui::shell::in_game_options::{build_in_game_options_descriptor, control};
use crate::ui::shell::in_game_options_state::{
    InGameOptionsState, OPTIONS_SPEED_MAX, OPTIONS_SPEED_MIN, plain_trackbar_thumb_left,
    speed_from_slider_pos, speed_slider_pos, trackbar_pos_from_mouse_x,
};
use crate::ui::shell::layout::{LaidOutControl, layout_pass_in_game_options};
use crate::ui::shell::trackbar::admits_press_y;
use crate::ui::skirmish_shell::RectPx;

/// Which visible `0xBBB` control (if any) is under the cursor. For a trackbar the
/// quantized slider position (0..6) the cursor x maps to is carried alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionsHit {
    Button(u16),
    Slider(u16, i32),
    Checkbox(u16),
    None,
}

/// Pure hit-test: the visible interactive control under `cursor`, if any. `laid`
/// must be the `layout_pass_in_game_options` output for the live descriptor (same
/// descriptor order), so control KINDS are recovered by zipping a fresh descriptor.
/// Hidden controls (the VisualDetails triplet) carry `visible: false` and are
/// skipped; statics are not interactive (the cursor falls through them).
pub(crate) fn in_game_options_hit(laid: &[LaidOutControl], cursor: (i32, i32)) -> OptionsHit {
    let (cx, cy) = cursor;
    let desc = build_in_game_options_descriptor();
    for (c, l) in desc.controls.iter().zip(laid.iter()) {
        if !c.visible || !l.rect.contains(cx, cy) {
            continue;
        }
        match c.kind {
            ControlKind::Button => return OptionsHit::Button(c.id),
            ControlKind::Trackbar => {
                // 61E505..61E512: only y > client bottom - 18 is admitted.
                if !admits_press_y(cy - l.rect.y, l.rect.h) {
                    continue;
                }
                let pos = trackbar_pos_from_mouse_x(
                    cx,
                    OPTIONS_SPEED_MIN as i32,
                    OPTIONS_SPEED_MAX as i32,
                    l.rect,
                );
                return OptionsHit::Slider(c.id, pos);
            }
            ControlKind::Checkbox => {
                // Owner6163A0, 6166EE..616708: caption is not a click target.
                if RectPx::new(l.rect.x, l.rect.y, 18, 18).contains(cx, cy) {
                    return OptionsHit::Checkbox(c.id);
                }
            }
            // Statics are not interactive; keep scanning the remaining controls.
            _ => continue,
        }
    }
    OptionsHit::None
}

/// Left-button press/release routing for the paused Options overlay. Non-left
/// buttons are swallowed (no overlay action). No-ops until the overlay has rendered
/// at least once (the anchor cache is `None`).
pub(crate) fn in_game_options_mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return; // consume non-left while paused; nothing to do
    }
    let Some(anchor) = state.match_state.match_presentation.in_game_options_anchor else {
        return; // overlay not rendered yet -> nothing to hit-test
    };
    let screen_w = state.renderer.gpu.config.width as i32;
    let screen_h = state.renderer.gpu.config.height as i32;
    let desc = build_in_game_options_descriptor();
    let laid = layout_pass_in_game_options(&desc, screen_w, screen_h, anchor);
    let (cx, cy) = state.window_cursor_position();
    let (cx, cy) = (cx.round() as i32, cy.round() as i32);

    if pressed {
        if state
            .match_state
            .match_presentation
            .in_game_options
            .dragging_slider
            .is_some()
        {
            return;
        }
        match in_game_options_hit(&laid, (cx, cy)) {
            OptionsHit::Button(id) => {
                if id == control::SOUND && !state.audio.launcher_audio_available {
                    return;
                }
                // Buttons (Back/Keyboard/Sound) paint pressed on hold; the action
                // fires on release over the same rectangle (Back or Sound).
                state
                    .match_state
                    .match_presentation
                    .in_game_options
                    .buttons
                    .press(Some(id));
                // Shared type-2 procedure612B70,61374B..613771: ordinary
                // button-down plays Rules+188's generic click before dispatch.
                crate::app::App::play_skirmish_shell_generic_click_sound(state);
            }
            OptionsHit::Slider(id, _) => {
                let rect = laid.iter().find(|l| l.id == id).expect("hit control").rect;
                if begin_slider_press(
                    &mut state.match_state.match_presentation.in_game_options,
                    id,
                    rect,
                    cx,
                ) {
                    crate::app::App::play_skirmish_shell_generic_click_sound(state);
                }
            }
            // Checkbox toggles on press (BS_AUTOCHECKBOX) — visual/stored only (KD-8).
            OptionsHit::Checkbox(id) => {
                toggle_checkbox(
                    &mut state.match_state.match_presentation.in_game_options,
                    id,
                );
                // Native 616736..61674E emits GUICheckboxSound after toggle.
                let sound = state
                    .rules()
                    .and_then(|rules| rules.general.gui_checkbox_sound.clone());
                crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
            }
            OptionsHit::None => {}
        }
    } else {
        let over = match in_game_options_hit(&laid, (cx, cy)) {
            OptionsHit::Button(control::SOUND) if !state.audio.launcher_audio_available => None,
            OptionsHit::Button(id) => Some(id),
            _ => None,
        };
        let options = &mut state.match_state.match_presentation.in_game_options;
        options.dragging_slider = None;
        match options.buttons.release(over) {
            Some(control::SOUND) => crate::app::input::sound::open(state),
            Some(control::KEYBOARD) => crate::app::input::keyboard::open(
                state,
                crate::ui::shell::keyboard::KeyboardParent::GameControls,
            ),
            Some(control::BACK) => crate::app::persistence::options::in_game_options_close(state),
            _ => {}
        }
    }
}

/// Live slider drag while paused: re-quantize the dragged slider's value from the
/// current cursor x against the cached anchor's laid rect. Visual/stored only — the
/// cadence and other effects are deferred to close (KD-8). No-op when no slider is
/// being dragged; button hover still updates to release its held visual outside.
pub(crate) fn in_game_options_drag(state: &mut AppState) {
    let Some(anchor) = state.match_state.match_presentation.in_game_options_anchor else {
        return;
    };
    let screen_w = state.renderer.gpu.config.width as i32;
    let screen_h = state.renderer.gpu.config.height as i32;
    let desc = build_in_game_options_descriptor();
    let laid = layout_pass_in_game_options(&desc, screen_w, screen_h, anchor);
    let (x, y) = state.window_cursor_position();
    let over = match in_game_options_hit(&laid, (x.round() as i32, y.round() as i32)) {
        OptionsHit::Button(control::SOUND) if !state.audio.launcher_audio_available => None,
        OptionsHit::Button(id) => Some(id),
        _ => None,
    };
    let options = &mut state.match_state.match_presentation.in_game_options;
    options.buttons.hovered = over;
    let Some(id) = options.dragging_slider else {
        return;
    };
    let Some(rect) = laid.iter().find(|l| l.id == id).map(|l| l.rect) else {
        return;
    };
    let cx = state.window_cursor_position().0.round() as i32;
    let pos =
        trackbar_pos_from_mouse_x(cx, OPTIONS_SPEED_MIN as i32, OPTIONS_SPEED_MAX as i32, rect);
    if store_slider_value(
        &mut state.match_state.match_presentation.in_game_options,
        id,
        pos,
    ) {
        // 61E609..61E6DD: changed position notifies the parent, then plays the cue.
        crate::app::App::play_skirmish_shell_generic_click_sound(state);
    }
}

/// Native 61E518..61E594: thumb press starts drag without changing the position;
/// a rail press jumps once without enabling drag. Returns a changed notification.
fn begin_slider_press(opts: &mut InGameOptionsState, id: u16, rect: RectPx, cx: i32) -> bool {
    let internal = match id {
        control::GAME_SPEED => opts.game_speed,
        control::SCROLL_RATE => opts.scroll_rate,
        _ => return false,
    };
    let left = rect.x + plain_trackbar_thumb_left(speed_slider_pos(internal), rect);
    if (left..left + 12).contains(&cx) {
        opts.dragging_slider = Some(id);
        return false;
    }
    let pos = trackbar_pos_from_mouse_x(cx, 0, OPTIONS_SPEED_MAX as i32, rect);
    store_slider_value(opts, id, pos)
}

/// Store a slider's new internal value from a slider position (`6 - pos`). Render
/// reads the stored value to draw the thumb + value label; no effect applies here.
fn store_slider_value(opts: &mut InGameOptionsState, id: u16, pos: i32) -> bool {
    let value = speed_from_slider_pos(pos.max(0) as u32);
    let (stored, notified) = match id {
        control::GAME_SPEED => (&mut opts.game_speed, &mut opts.game_speed_label_dragged),
        control::SCROLL_RATE => (&mut opts.scroll_rate, &mut opts.scroll_rate_label_dragged),
        _ => return false,
    };
    if *stored == value {
        return false;
    }
    *stored = value;
    // 4E2278..4E232B updates the caption only after HSCROLL code5.
    *notified = true;
    true
}

/// Toggle a checkbox's stored bool (the rendered check state only — the downstream
/// effect applies on close, KD-8).
fn toggle_checkbox(opts: &mut InGameOptionsState, id: u16) {
    match id {
        control::TARGET_LINES => opts.unit_action_lines = !opts.unit_action_lines,
        control::SHOW_HIDDEN => opts.show_hidden = !opts.show_hidden,
        control::TOOLTIPS => opts.tooltips = !opts.tooltips,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::shell::layout::InGameOptionsAnchor;

    fn test_laid() -> Vec<LaidOutControl> {
        let desc = build_in_game_options_descriptor();
        let anchor = InGameOptionsAnchor {
            button_canvas_w: 125,
            button_canvas_h: 25,
            button_stack_top_y: 200,
            back_button_y: 540,
        };
        layout_pass_in_game_options(&desc, 800, 600, anchor)
    }

    #[test]
    fn hit_test_routes_visible_controls() {
        let laid = test_laid();
        let rect_of = |id: u16| laid.iter().find(|l| l.id == id).unwrap().rect;

        // Back button center -> Button(BACK).
        let back = rect_of(control::BACK);
        assert_eq!(
            in_game_options_hit(&laid, (back.x + back.w / 2, back.y + back.h / 2)),
            OptionsHit::Button(control::BACK)
        );

        // GameSpeed rail far-right edge (inside the rect) -> Slider at the max stop.
        let gs = rect_of(control::GAME_SPEED);
        assert_eq!(
            in_game_options_hit(&laid, (gs.x + gs.w - 1, gs.y + gs.h / 2)),
            OptionsHit::Slider(control::GAME_SPEED, 6)
        );

        // TargetLines icon center -> Checkbox(TARGET_LINES).
        let tl = rect_of(control::TARGET_LINES);
        assert_eq!(
            in_game_options_hit(&laid, (tl.x + 9, tl.y + 9)),
            OptionsHit::Checkbox(control::TARGET_LINES)
        );
        assert_eq!(
            in_game_options_hit(&laid, (tl.x + 26, tl.y + 9)),
            OptionsHit::None
        );

        // Empty corner -> None.
        assert_eq!(in_game_options_hit(&laid, (2, 2)), OptionsHit::None);
    }

    #[test]
    fn hidden_visualdetails_trackbar_is_not_hittable() {
        // The VisualDetails trackbar carries visible:false; a cursor over its laid
        // rect must NOT register as a slider hit.
        let laid = test_laid();
        let vd = laid
            .iter()
            .find(|l| l.id == control::VISUAL_DETAILS)
            .unwrap()
            .rect;
        assert_eq!(
            in_game_options_hit(&laid, (vd.x + vd.w / 2, vd.y + vd.h / 2)),
            OptionsHit::None
        );
    }

    #[test]
    fn store_slider_value_inverts_position() {
        let mut opts = InGameOptionsState::default();
        // Far-right slider position 6 -> fastest internal 0.
        store_slider_value(&mut opts, control::GAME_SPEED, 6);
        assert_eq!(opts.game_speed, 0);
        // Far-left position 0 -> slowest internal 6.
        store_slider_value(&mut opts, control::SCROLL_RATE, 0);
        assert_eq!(opts.scroll_rate, 6);
    }

    #[test]
    fn thumb_press_preserves_value_and_caption_until_a_changed_notification() {
        let mut opts = InGameOptionsState::default();
        let rect = test_laid()
            .into_iter()
            .find(|l| l.id == control::GAME_SPEED)
            .unwrap()
            .rect;
        let thumb = rect.x + plain_trackbar_thumb_left(3, rect);
        assert!(!begin_slider_press(
            &mut opts,
            control::GAME_SPEED,
            rect,
            thumb + 5
        ));
        assert_eq!(opts.dragging_slider, Some(control::GAME_SPEED));
        assert_eq!(opts.game_speed, 3);
        assert!(!opts.game_speed_label_dragged);
        assert!(!store_slider_value(&mut opts, control::GAME_SPEED, 3));
        assert!(!opts.game_speed_label_dragged);
        assert!(store_slider_value(&mut opts, control::GAME_SPEED, 6));
        assert_eq!(opts.game_speed, 0);
        assert!(opts.game_speed_label_dragged);
        assert!(!opts.scroll_rate_label_dragged);
    }

    #[test]
    fn rail_press_changes_once_without_drag_and_top_rows_are_not_admitted() {
        let laid = test_laid();
        let rect = laid
            .iter()
            .find(|l| l.id == control::SCROLL_RATE)
            .unwrap()
            .rect;
        let mut opts = InGameOptionsState::default();
        assert!(begin_slider_press(
            &mut opts,
            control::SCROLL_RATE,
            rect,
            rect.x
        ));
        assert_eq!(opts.scroll_rate, 6);
        assert!(opts.scroll_rate_label_dragged);
        assert_eq!(opts.dragging_slider, None);
        assert_eq!(
            in_game_options_hit(&laid, (rect.x, rect.y + rect.h - 18)),
            OptionsHit::None
        );
        assert_eq!(
            in_game_options_hit(&laid, (rect.x, rect.y + rect.h - 17)),
            OptionsHit::Slider(control::SCROLL_RATE, 0)
        );
    }

    #[test]
    fn toggle_checkbox_flips_matching_bool_only() {
        let mut opts = InGameOptionsState::default(); // lines on, hidden off, tips on
        toggle_checkbox(&mut opts, control::TARGET_LINES);
        assert!(!opts.unit_action_lines);
        assert!(!opts.show_hidden && opts.tooltips, "others untouched");
        toggle_checkbox(&mut opts, control::SHOW_HIDDEN);
        assert!(opts.show_hidden);
    }
}
