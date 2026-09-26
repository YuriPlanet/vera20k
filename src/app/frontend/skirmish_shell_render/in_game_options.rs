//! Active Game Controls (BBB) generic controls and clipped GAME.FNT text.
//! Side-specific SIDEBTTN and parent chrome use the sidebar atlas in a separate
//! batch. Layout and values are supplied by their retained UI authorities.

use crate::assets::csf_file::CsfFile;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::render::skirmish_shell_chrome::ControlChrome;
use crate::ui::shell::descriptor::ControlKind;
use crate::ui::shell::geom;
use crate::ui::shell::in_game_options::{
    build_in_game_options_descriptor, control, speed_value_label_key,
};
use crate::ui::shell::in_game_options_state::{
    InGameOptionsState, plain_trackbar_thumb_left, speed_slider_pos,
};
use crate::ui::shell::layout::{InGameOptionsAnchor, layout_pass_in_game_options};

use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};

use super::controls::{ControlPaint, paint_control};

/// Generic controls only: the caller paints buttons from the side-specific atlas.
pub(crate) fn build_in_game_options_instances(
    chrome: &ControlChrome,
    screen_w: i32,
    screen_h: i32,
    anchor: InGameOptionsAnchor,
    state: &InGameOptionsState,
) -> Vec<SpriteInstance> {
    let desc = build_in_game_options_descriptor();
    let laid = layout_pass_in_game_options(&desc, screen_w, screen_h, anchor);
    let mut out = Vec::new();
    for (c, l) in desc.controls.iter().zip(laid.iter()) {
        if !c.visible {
            continue;
        }
        let rect = l.rect;
        match c.kind {
            ControlKind::Trackbar => {
                let pos = match c.id {
                    control::GAME_SPEED => speed_slider_pos(state.game_speed),
                    control::SCROLL_RATE => speed_slider_pos(state.scroll_rate),
                    _ => continue,
                };
                paint_control(
                    &mut out,
                    chrome,
                    ControlPaint::Trackbar {
                        rect,
                        thumb_left: plain_trackbar_thumb_left(pos, rect),
                        plaque: false,
                    },
                );
            }
            ControlKind::Checkbox => {
                let checked = match c.id {
                    control::TARGET_LINES => state.unit_action_lines,
                    control::SHOW_HIDDEN => state.show_hidden,
                    control::TOOLTIPS => state.tooltips,
                    _ => false,
                };
                paint_control(&mut out, chrome, ControlPaint::Checkbox { checked, rect });
            }
            // The active 0xBBB set carries only buttons/trackbars/checkboxes.
            _ => {}
        }
    }
    out
}

/// Preserve each text rectangle's scissor for the caller's font batch.
pub(crate) fn build_in_game_options_text_instances(
    font: &BitFont,
    csf: Option<&CsfFile>,
    screen_w: i32,
    screen_h: i32,
    anchor: InGameOptionsAnchor,
    state: &InGameOptionsState,
    theme: crate::sidebar::SidebarTheme,
) -> Vec<ShellTextDraw> {
    let mut out = Vec::new();
    for draw in in_game_options_static_draws(csf, screen_w, screen_h, anchor, state) {
        let rect = TextRect {
            x: draw.rect.x,
            y: draw.rect.y,
            w: draw.rect.w.max(0) as u32,
            h: draw.rect.h.max(0) as u32,
        };
        let text_draw = shell_text::draw_in_rect(
            font,
            &draw.text,
            rect,
            if draw.id == control::SOUND && !state.sound_enabled {
                super::pause_menu::button_text_rgb(theme, false)
            } else {
                SHELL_LABEL_TEXT_RGB
            },
            draw.align,
            [0.0, 0.0],
            SHELL_CONTROL_TEXT_DEPTH,
        );
        out.push(text_draw);
    }
    out
}

/// Resolved visible control text, kept separate from font emission for testing.
struct OptionsStaticDraw {
    #[cfg_attr(not(test), allow(dead_code))]
    id: u16,
    rect: geom::RectPx,
    align: ShellAlign,
    text: String,
}

fn in_game_options_static_draws(
    csf: Option<&CsfFile>,
    screen_w: i32,
    screen_h: i32,
    anchor: InGameOptionsAnchor,
    state: &InGameOptionsState,
) -> Vec<OptionsStaticDraw> {
    let desc = build_in_game_options_descriptor();
    let laid = layout_pass_in_game_options(&desc, screen_w, screen_h, anchor);
    let mut out = Vec::new();
    for (c, l) in desc.controls.iter().zip(laid.iter()) {
        if !c.visible
            || !matches!(
                c.kind,
                ControlKind::Static | ControlKind::Checkbox | ControlKind::Button
            )
        {
            continue;
        }
        // The two value labels (0x671/0x672) swap to the slider-position CSF key
        // once their slider has been dragged this open; before that (and for every
        // other static) they keep their template caption key (the gamemd quirk).
        let key = match c.id {
            control::GAME_SPEED_VALUE => speed_value_label_key(
                speed_slider_pos(state.game_speed),
                state.game_speed_label_dragged,
            ),
            control::SCROLL_RATE_VALUE => speed_value_label_key(
                speed_slider_pos(state.scroll_rate),
                state.scroll_rate_label_dragged,
            ),
            _ => match c.csf_key {
                Some(key) => key,
                None => continue,
            },
        };
        let text = resolve_static_text(csf, key);
        if text.is_empty() {
            continue;
        }
        let (rect, align) = match c.kind {
            // 61663A..616674 shifts the RECT's left edge by26; right stays put.
            ControlKind::Checkbox => (
                geom::RectPx::new(l.rect.x + 26, l.rect.y, l.rect.w - 26, l.rect.h),
                ShellAlign::V_CENTER,
            ),
            // 61358D..6135EE uses a slightly inset RECT and adds (2,4) to
            // its left/top when pressed. Its right/bottom edges do not move.
            ControlKind::Button => {
                let pressed = state.buttons.is_pressed(c.id);
                let dx = if pressed { 2 } else { 0 };
                let dy = if pressed { 4 } else { 0 };
                (
                    geom::RectPx::new(
                        l.rect.x + dx,
                        l.rect.y + 1 + dy,
                        l.rect.w - 2 - dx,
                        l.rect.h - 1 - dy,
                    ),
                    ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                )
            }
            _ => (l.rect, options_static_align(c.id)),
        };
        out.push(OptionsStaticDraw {
            id: c.id,
            rect,
            align,
            text,
        });
    }
    out
}

/// Resolve a static's CSF caption key. Once the table is initialized, a missing
/// label stays visible as retail's `MISSING:'<key>'`; English literals exist
/// only for assetless operation where no CSF table is available.
fn resolve_static_text(csf: Option<&CsfFile>, key: &str) -> String {
    match csf {
        Some(table) => table.text(key).into_owned(),
        None => options_static_fallback(key).to_string(),
    }
}

fn options_static_fallback(key: &str) -> &'static str {
    match key {
        "GUI:GameOptions" => "Game Options",
        "GUI:GameSpeed" => "Game Speed",
        "GUI:ScrollRate" => "Scroll Rate",
        "GUI:Back" => "Back",
        "GUI:Keyboard" => "Keyboard",
        "GUI:Sound" => "Sound",
        "GUI:TargetLines" => "Target Lines",
        "GUI:ShowHidden" => "Show Hidden Objects",
        "GUI:Tooltips" => "Tooltips",
        "GUI:Faster" => "Faster",
        // Slider value labels (after first drag) when the CSF table is absent.
        "TXT_SLOWEST" => "Slowest",
        "TXT_SLOWER" => "Slower",
        "TXT_SLOW" => "Slow",
        "TXT_MEDIUM" => "Medium",
        "TXT_FAST" => "Fast",
        "TXT_FASTER" => "Faster",
        "TXT_FASTEST" => "Fastest",
        // GUI:Blank (footer) + anything else -> no text.
        _ => "",
    }
}

/// Original 615A81..615AE8 inspects only the two horizontal style bits.
/// SS_CENTERIMAGE does not introduce vertical centering in this owner-draw path.
fn options_static_align(id: u16) -> ShellAlign {
    match id {
        control::TITLE => ShellAlign::H_CENTER,
        control::GAME_SPEED_CAPTION | control::SCROLL_RATE_CAPTION => ShellAlign::H_RIGHT,
        _ => ShellAlign::NONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::skirmish_shell_chrome::{SkirmishShellChromeEntry, trackbar_frame_slot};

    fn initialized_empty_csf() -> CsfFile {
        let mut bytes = Vec::new();
        for value in [0x4353_4620_u32, 3, 1, 1, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        CsfFile::from_bytes(&bytes).expect("valid initialized CSF header")
    }

    fn entry(w: f32, h: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [w, h],
        }
    }

    fn test_anchor() -> InGameOptionsAnchor {
        InGameOptionsAnchor {
            button_canvas_w: 125,
            button_canvas_h: 25,
            button_stack_top_y: 200,
            back_button_y: 540,
        }
    }

    #[test]
    fn ingame_options_emitter_emits_visible_controls_skips_visualdetails() {
        // Checkbox-icon-only chrome: the 3 visible checkboxes emit one icon each.
        let checks_only = ControlChrome {
            checkbox_checked_cce_i: Some(entry(18.0, 18.0)),
            checkbox_unchecked_cue_i: Some(entry(18.0, 18.0)),
            ..Default::default()
        };
        let out = build_in_game_options_instances(
            &checks_only,
            800,
            600,
            test_anchor(),
            &InGameOptionsState::default(),
        );
        assert_eq!(out.len(), 3, "3 checkboxes");

        // Frame-only chrome: only the 2 VISIBLE trackbars
        // (GameSpeed/ScrollRate) emit the shared two-frame composition; the
        // hidden VisualDetails trackbar is skipped.
        let mut frame_only = ControlChrome::default();
        frame_only.trackbar_frames[trackbar_frame_slot(false, 192, 21).unwrap()] =
            Some(entry(196.0, 25.0));
        let out = build_in_game_options_instances(
            &frame_only,
            800,
            600,
            test_anchor(),
            &InGameOptionsState::default(),
        );
        assert_eq!(out.len(), 2, "2 visible trackbars (VisualDetails hidden)");

        // Empty chrome: no glyphs loaded -> nothing emitted.
        let out = build_in_game_options_instances(
            &ControlChrome::default(),
            800,
            600,
            test_anchor(),
            &InGameOptionsState::default(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn static_draws_select_visible_statics_resolve_text_and_align() {
        // Empty/hidden text drops out; every visible button and checkbox caption
        // joins the five nonempty statics.
        let draws = in_game_options_static_draws(
            None,
            800,
            600,
            test_anchor(),
            &InGameOptionsState::default(),
        );
        let ids: Vec<u16> = draws.iter().map(|d| d.id).collect();
        assert_eq!(
            draws.len(),
            11,
            "five statics plus six interactive captions"
        );
        for id in [
            control::TITLE,
            control::GAME_SPEED_CAPTION,
            control::SCROLL_RATE_CAPTION,
            control::GAME_SPEED_VALUE,
            control::SCROLL_RATE_VALUE,
            control::BACK,
            control::KEYBOARD,
            control::SOUND,
            control::TARGET_LINES,
            control::SHOW_HIDDEN,
            control::TOOLTIPS,
        ] {
            assert!(ids.contains(&id), "missing static {id:#06x}");
        }
        // Hidden VisualDetails caption/label + the blank footer are NOT drawn.
        assert!(!ids.contains(&control::VISUAL_DETAILS_CAPTION));
        assert!(!ids.contains(&control::VISUAL_DETAILS_VALUE));
        assert!(!ids.contains(&control::FOOTER));

        let find = |id: u16| draws.iter().find(|d| d.id == id).unwrap();
        // Alignment from the template SS_* bits.
        assert_eq!(find(control::TITLE).align, ShellAlign::H_CENTER);
        assert_eq!(find(control::GAME_SPEED_CAPTION).align, ShellAlign::H_RIGHT);
        assert_eq!(find(control::GAME_SPEED_VALUE).align, ShellAlign::NONE);
        // Laid rect == projected DLU rect (centered offset is 0 at the 800x600 base).
        assert_eq!(
            find(control::TITLE).rect,
            geom::RectPx::new(635, 2, 162, 16)
        );
        assert_eq!(
            find(control::GAME_SPEED_CAPTION).rect,
            geom::dlu_rect(61, 99, 78, 15)
        );
        // The value labels paint the template default ("Faster") at open until the
        // slider is dragged (the gamemd quirk; see the dragged-swap test below).
        assert_eq!(find(control::GAME_SPEED_VALUE).text, "Faster");
    }

    #[test]
    fn initialized_csf_exposes_missing_static_label() {
        let csf = initialized_empty_csf();
        assert_eq!(
            resolve_static_text(Some(&csf), "GUI:GameOptions"),
            "MISSING:'GUI:GameOptions'"
        );
        assert_eq!(resolve_static_text(None, "GUI:GameOptions"), "Game Options");
    }

    #[test]
    fn interactive_captions_keep_native_insets_and_pressed_clip_edges() {
        let mut state = InGameOptionsState::default();
        let released = in_game_options_static_draws(None, 800, 600, test_anchor(), &state);
        let check = released
            .iter()
            .find(|d| d.id == control::TARGET_LINES)
            .unwrap();
        let raw = geom::dlu_rect(89, 206, 119, 10);
        assert_eq!(
            check.rect,
            geom::RectPx::new(raw.x + 26, raw.y, raw.w - 26, raw.h)
        );
        assert_eq!(check.align, ShellAlign::V_CENTER);
        let button = released.iter().find(|d| d.id == control::BACK).unwrap();
        assert_eq!(button.align, ShellAlign::H_CENTER | ShellAlign::V_CENTER);
        state.buttons.press(Some(control::BACK));
        let pressed = in_game_options_static_draws(None, 800, 600, test_anchor(), &state);
        let held = pressed.iter().find(|d| d.id == control::BACK).unwrap();
        assert_eq!(held.rect.x, button.rect.x + 2);
        assert_eq!(held.rect.y, button.rect.y + 4);
        assert_eq!(held.rect.x + held.rect.w, button.rect.x + button.rect.w);
        assert_eq!(held.rect.y + held.rect.h, button.rect.y + button.rect.h);
    }

    #[test]
    fn dragged_gamespeed_slider_swaps_value_label_to_position_word() {
        // Once the GameSpeed slider is dragged this open, its value label swaps
        // from the template "Faster" to the slider-position CSF word. game_speed=3
        // -> slider pos 3 -> TXT_MEDIUM ("Medium" via the no-CSF fallback). The
        // ScrollRate label is independent and stays "Faster" (not dragged).
        let state = InGameOptionsState {
            game_speed: 3,
            game_speed_label_dragged: true,
            ..Default::default()
        };
        let draws = in_game_options_static_draws(None, 800, 600, test_anchor(), &state);
        let find = |id: u16| draws.iter().find(|d| d.id == id).unwrap();
        assert_ne!(find(control::GAME_SPEED_VALUE).text, "Faster");
        assert_eq!(find(control::GAME_SPEED_VALUE).text, "Medium");
        assert_eq!(find(control::SCROLL_RATE_VALUE).text, "Faster");
    }

    #[test]
    fn text_instances_emit_glyphs_from_bit_font_atlas_path() {
        // Smoke test the glyph delegation: a font carrying the letters of "Faster"
        // (the value-label fallback) yields glyph instances. The detailed layout
        // math lives in shell_text/bit_font unit tests.
        use crate::render::bit_font::tests::make_test_font;
        let font = make_test_font(
            &[
                (b'F' as u16, 6),
                (b'a' as u16, 6),
                (b's' as u16, 6),
                (b't' as u16, 6),
                (b'e' as u16, 6),
                (b'r' as u16, 6),
            ],
            8,
        );
        let out = build_in_game_options_text_instances(
            &font,
            None,
            800,
            600,
            test_anchor(),
            &InGameOptionsState::default(),
            crate::sidebar::SidebarTheme::Allied,
        );
        assert!(out.iter().any(|draw| !draw.instances.is_empty()));
        assert!(
            out.iter()
                .all(|draw| draw.scissor.w > 0 && draw.scissor.h > 0)
        );
    }
}
