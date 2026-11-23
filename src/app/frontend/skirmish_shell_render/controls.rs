//! Owner-draw control sprite helpers for the skirmish shell renderer.
//!
//! Contains NewEdit chrome, checkboxes, trackbars, combo faces,
//! and ComboDropWin popup sprite construction.

use crate::app::loading::init::MapMenuEntry;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::{self, ShellTextDraw};
use crate::render::skirmish_shell_chrome::{
    ControlChrome, SkirmishShellChromeAtlas, SkirmishShellChromeEntry,
};
use crate::rules::ruleset::RuleSet;
use crate::ui::shell::geom::{ShellAlign, ShellScale};
use crate::ui::skirmish_shell::{
    DropdownMetrics, DropdownScrollbarPart, RectPx, SkirmishShellLayout, SkirmishShellState,
    checkbox_icon_rect, combo_arrow_rect, combo_face_rect, combo_swatch_rect,
    opponent_scroll_geometry,
};
use vera20k_gui::shell::trackbar::{PLAQUE_RESERVE, THUMB_W};

use super::chrome::{
    push_entry, push_entry_grown, push_entry_top_clipped, push_entry_wrapped_centered,
    push_ownerdraw_two_pixel_bevel_frame, push_ownerdraw_two_pixel_bevel_frame_px, push_solid_rect,
    push_solid_rect_px, push_tinted_entry,
};
use super::text::rect_to_text_rect;
use super::{
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF, SHELL_CONTROL_DEPTH,
    SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE, SHELL_DROPDOWN_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
    SHELL_SCROLLBAR_TRACK_RGB_PENDING_SCROLLBAR_SOURCE_CAPTURE, SHELL_SWATCH_DEPTH,
};

/// Original61E700 group and6208F0 plain frames both use two bevel rings and
/// averaged mixed corners. A group's top edge leaves the native caption gap.
pub(super) fn frame_segments(
    rect: RectPx,
    caption_width: Option<i32>,
    outer_light: bool,
) -> Vec<(RectPx, [u8; 3])> {
    let light = [0xc5, 0xbe, 0xa7];
    let dark = [0x80, 0x7a, 0x68];
    let mixed = [0xa2, 0x9c, 0x87];
    let mut out = Vec::new();
    for ring in 0..2 {
        let l = rect.x + ring;
        let t = rect.y + ring;
        let r = rect.x + rect.w - ring - 1;
        let b = rect.y + rect.h - ring - 1;
        let (tl, br) = if outer_light == (ring == 0) {
            (light, dark)
        } else {
            (dark, light)
        };
        if let Some(width) = caption_width.filter(|width| *width > 0) {
            out.push((RectPx::new(l, t, rect.x + 8 - l + 1, 1), tl));
            let start = rect.x + width + 12;
            out.push((RectPx::new(start, t, (r - start + 1).max(0), 1), tl));
        } else {
            out.push((RectPx::new(l, t, r - l, 1), tl));
        }
        out.push((RectPx::new(l, t + 1, 1, b - t), tl));
        out.push((RectPx::new(l, b, r - l + 1, 1), br));
        out.push((RectPx::new(r, t + 1, 1, (b - t - 1).max(0)), br));
        out.push((RectPx::new(r, t, 1, 1), mixed));
        out.push((RectPx::new(l, b, 1, 1), mixed));
    }
    out
}

/// Original61E700's group window split: the frame starts half a font cell
/// below the window top, and the caption sits 10px in, one cell tall.
/// Derive both rectangles in authored pixels before mapping them to the window,
/// so the caption's clip and the frame grow together. `cell_height` is unscaled.
pub(super) fn group_box_geometry(
    group: RectPx,
    cell_height: i32,
    scale: ShellScale,
) -> (RectPx, RectPx) {
    let top = cell_height / 2;
    (
        scale.derive_rect(group, |group| {
            RectPx::new(group.x, group.y + top, group.w, group.h - top)
        }),
        scale.derive_rect(group, |group| {
            RectPx::new(group.x + 10, group.y, group.w - 10, cell_height)
        }),
    )
}

/// Paint the shared61E700/6208F0 bevel from a window-space frame. Caption
/// width is in authored pixels; map each ring and its gap to the window once.
pub(super) fn push_group_or_plain_frame(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    caption_width: Option<i32>,
    outer_light: bool,
    scale: ShellScale,
) {
    let rect = if scale.is_unscaled() {
        rect
    } else {
        scale.base_rect(rect)
    };
    for (index, (rect, rgb)) in frame_segments(rect, caption_width, outer_light)
        .into_iter()
        .enumerate()
    {
        push_solid_rect_px(
            out,
            white_pixel,
            scale.scale_rect(rect),
            rgb.map(|c| f32::from(c) / 255.0),
            SHELL_CONTROL_DEPTH - index as f32 * 0.0000001,
        );
    }
}

/// A NewEdit's frame, caret and text, as NewEdit__DrawTextAndCaret
/// `0x00623880` paints them in BB, BC/BD, C2/C9 and `0x10F`: the frame two
/// pixels round `rect`, the text inside it from the first visible unit, and
/// an open IME composition spliced in at the caret in the composition colour
/// (`app::input::ime`), the rest in `rgb`.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_new_edit(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    atlas: &SkirmishShellChromeAtlas,
    font: &BitFont,
    edit: &vera20k_gui::shell::new_edit::NewEdit,
    composition: Option<(&[u16], usize)>,
    rect: RectPx,
    caret_visible: bool,
    rgb: [f32; 3],
    text_depth: f32,
    scale: ShellScale,
) {
    let frame = scale.derive_rect(rect, |r| RectPx::new(r.x - 2, r.y - 2, r.w + 5, r.h + 5));
    let field = scale.derive_rect(rect, |r| RectPx::new(r.x + 2, r.y, r.w - 1, r.h + 1));
    let (rects, look) = ((frame, field), (caret_visible, rgb, text_depth));
    push_framed_edit(
        controls,
        texts,
        atlas,
        font,
        edit,
        composition,
        rects,
        look,
        scale,
    );
}

/// [`push_new_edit`] with the frame and the text field given (a framed
/// edit's `TextInset`); `caret` shows the caret, `rgb` and `depth` the text.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_framed_edit(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    atlas: &SkirmishShellChromeAtlas,
    font: &BitFont,
    edit: &vera20k_gui::shell::new_edit::NewEdit,
    composition: Option<(&[u16], usize)>,
    (frame, field): (RectPx, RectPx),
    (caret, rgb, text_depth): (bool, [f32; 3], f32),
    scale: ShellScale,
) {
    let available = (scale.base_rect(field).w - 2).max(0) as u32;
    let composed = composition.map(|(units, cursor)| {
        edit.with_composition(units, cursor, available, |text| font.text_width(text))
    });
    let (view, composed_units) = match &composed {
        Some((view, units)) => (view, units.clone()),
        None => (edit, 0..0),
    };
    push_ownerdraw_two_pixel_bevel_frame(controls, atlas, frame, SHELL_EDIT_FRAME_DEPTH);
    if caret {
        let depth = SHELL_EDIT_CARET_DEPTH;
        push_new_edit_caret(controls, atlas, font, view, field, depth, scale);
    }
    let first = view.first_visible_unit.min(view.units.len());
    let (start, end) = (
        composed_units.start.max(first),
        composed_units.end.max(first),
    );
    let composition_rgb = crate::app::input::ime::COMPOSITION_RGB.map(|c| f32::from(c) / 255.0);
    let mut x = 0;
    for (units, rgb) in [
        (first..start, rgb),
        (start..end, composition_rgb),
        (end..view.units.len(), rgb),
    ] {
        if units.is_empty() {
            continue;
        }
        let text = view.painted_range(units.start, units.end);
        let at = scale.derive_rect(field, |f| RectPx::new(f.x + x, f.y, f.w - x, f.h));
        texts.push(shell_text::draw_in_rect_scaled(
            font,
            &text,
            rect_to_text_rect(at),
            rgb,
            ShellAlign::V_CENTER,
            [0.0; 2],
            text_depth,
            scale.factor(),
        ));
        x += font.text_width(&text) as i32;
    }
}

/// The NewEdit caret alone, for an editor painted inside another control's
/// face (the WOL login's editable combo).
pub(super) fn push_new_edit_caret(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    font: &BitFont,
    editor: &vera20k_gui::shell::new_edit::NewEdit,
    text: RectPx,
    depth: f32,
    scale: ShellScale,
) {
    if !editor.focused {
        return;
    }
    let field = scale.base_rect(text);
    let caret = editor.caret.min(editor.units.len());
    let start = editor.first_visible_unit.min(caret);
    let prefix = editor.painted_range(start, caret);
    let x = field.x + font.text_width(&prefix) as i32;
    if x < field.x + field.w {
        push_solid_rect(
            out,
            atlas,
            scale.scale_rect(RectPx::new(x, field.y + 2, 2, (field.h - 4).max(1))),
            SHELL_LABEL_TEXT_RGB,
            depth,
        );
    }
}

pub(super) fn combo_face_entry(
    chrome: &ControlChrome,
    rect: RectPx,
    scale: ShellScale,
) -> Option<SkirmishShellChromeEntry> {
    match scale.base_rect(rect).w {
        249 => chrome.combo_face_249,
        207 => chrome.combo_face_207,
        180 => chrome.combo_face_180,
        150 => chrome.combo_face_150,
        151 => chrome.combo_face_151,
        118 => chrome.combo_face_118,
        42 => chrome.combo_face_42,
        39 => chrome.combo_face_39,
        213 => chrome.combo_face_213,
        117 => chrome.combo_face_117,
        44 => chrome.combo_face_44,
        38 => chrome.combo_face_38,
        _ => None,
    }
}

pub(super) fn push_scrollbar_thumb(
    out: &mut Vec<SpriteInstance>,
    chrome: &ControlChrome,
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    let top_h = chrome
        .scrollbar_thumb_top
        .map(|entry| scale.art_size(entry.pixel_size[0], entry.pixel_size[1])[1].round() as i32)
        .unwrap_or(0);
    let bottom_h = chrome
        .scrollbar_thumb_bottom
        .map(|entry| scale.art_size(entry.pixel_size[0], entry.pixel_size[1])[1].round() as i32)
        .unwrap_or(0);
    if let Some(top) = chrome.scrollbar_thumb_top {
        push_entry_grown(out, top, rect.x, rect.y, scale, depth);
    }
    if let Some(bottom) = chrome.scrollbar_thumb_bottom {
        push_entry_grown(
            out,
            bottom,
            rect.x,
            rect.y + rect.h - bottom_h,
            scale,
            depth,
        );
    }
    if let Some(mid) = chrome.scrollbar_thumb_mid {
        let mid_y = rect.y + top_h;
        let mid_h = rect.h - top_h - bottom_h;
        if mid_h > 0 {
            push_entry(out, mid, RectPx::new(rect.x, mid_y, rect.w, mid_h), depth);
        }
    }
}

pub(super) fn scrollbar_arrow_entry(
    released: Option<SkirmishShellChromeEntry>,
    pressed: Option<SkirmishShellChromeEntry>,
    is_pressed: bool,
) -> Option<SkirmishShellChromeEntry> {
    if is_pressed {
        pressed.or(released)
    } else {
        released
    }
}

/// YR's eight lobby colours' fixed owner-draw swatches.
///
/// The active native initializer copies these eight packed RGB values from
/// `0x008316A8`; gameplay `[Colors]` schemes do not feed this UI table.
const SKIRMISH_SWATCH_RGB: [[u8; 3]; 8] = [
    [221, 226, 13],
    [255, 25, 25],
    [42, 116, 226],
    [62, 209, 46],
    [255, 160, 25],
    [50, 215, 230],
    [149, 40, 189],
    [255, 154, 235],
];

/// Resolve a selectable lobby colour priority to its owner-draw swatch.
///
/// YR's eight read the fixed table whatever `rules` hold. gamemd has no
/// swatch for VERA's extra colours, so theirs is the colour gamemd prints
/// their House's text in: the scheme's own H,S,V (`house_text_rgb`). Anything
/// else has no player swatch.
fn lobby_swatch_rgb(rules: Option<&RuleSet>, index: usize) -> Option<[u8; 3]> {
    SKIRMISH_SWATCH_RGB.get(index).copied().or_else(|| {
        let rules = rules?;
        let priority = i32::try_from(index).ok()?;
        let colors = rules.skirmish_colors();
        colors.is_lobby_priority(priority).then(|| {
            crate::rules::house_colors::house_text_rgb(
                &rules.color_schemes,
                crate::rules::house_colors::HouseColorIndex(
                    colors.entry_for_priority(priority) as u8
                ),
            )
        })
    })
}

pub(super) fn house_color_tint(rules: Option<&RuleSet>, index: usize) -> [f32; 3] {
    lobby_swatch_rgb(rules, index)
        .unwrap_or([96, 96, 96])
        .map(|v| f32::from(v) / 255.0)
}

/// Hue in degrees round the RGB colour wheel. Very low saturation has no
/// useful hue; near-grey swatches share a separate family.
fn swatch_hue(rgb: [u8; 3]) -> Option<i32> {
    let [r, g, b] = rgb.map(i32::from);
    let high = r.max(g).max(b);
    let chroma = high - r.min(g).min(b);
    if 8 * chroma <= high {
        return None;
    }
    let hue = if r == high {
        60 * (g - b) / chroma
    } else if g == high {
        120 + 60 * (b - r) / chroma
    } else {
        240 + 60 * (r - g) / chroma
    };
    Some(hue.rem_euclid(360))
}

/// Each seat votes for the closest of the eight lobby swatch hues or grey.
/// The caption takes the mean colour of the strict majority's swatches or,
/// with no majority, of every seat's.
pub(super) fn team_caption_tint(rules: Option<&RuleSet>, colors: &[u8]) -> Option<[f32; 3]> {
    const GREY: usize = SKIRMISH_SWATCH_RGB.len();
    let hues = SKIRMISH_SWATCH_RGB.map(|rgb| swatch_hue(rgb).unwrap_or_default());
    let mut families = [(0usize, [0u32; 3]); GREY + 1];
    let mut every = (0usize, [0u32; 3]);
    for &priority in colors {
        let Some(rgb) = lobby_swatch_rgb(rules, usize::from(priority)) else {
            continue;
        };
        let family = swatch_hue(rgb)
            .and_then(|hue| {
                (0..hues.len()).min_by_key(|&i| {
                    let gap = (hue - hues[i]).abs();
                    gap.min(360 - gap)
                })
            })
            .unwrap_or(GREY);
        for (count, sum) in [&mut families[family], &mut every] {
            *count += 1;
            for (channel, value) in sum.iter_mut().zip(rgb) {
                *channel += u32::from(value);
            }
        }
    }
    // Team Colors gives each player on a team a hue of its own from the
    // team's range, so a team of two or more holds no majority: its caption
    // mixes every seat's swatch, warm or cool as the range is.
    let votes = every.0;
    let (count, sum) = families
        .into_iter()
        .find(|(count, _)| *count > votes / 2)
        .unwrap_or(every);
    if count == 0 {
        return None;
    }
    let mut rgb = sum.map(|v| v as f32 / (255.0 * count as f32));
    // First raise the value of dark shades (such as navy) keeping their saturation.
    // Mix in white only when the hue itself is too dark for the small text.
    let mut luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    const MIN_LUMA: f32 = 0.55;
    if luma > 0.0 && luma < MIN_LUMA {
        let peak = rgb[0].max(rgb[1]).max(rgb[2]);
        let gain = (MIN_LUMA / luma).min(1.0 / peak);
        rgb = rgb.map(|v| v * gain);
        luma *= gain;
    }
    if luma < MIN_LUMA {
        let white = (MIN_LUMA - luma) / (1.0 - luma);
        rgb = rgb.map(|v| v + (1.0 - v) * white);
    }
    Some(rgb)
}

/// The value plaque of `ShellTrackbar__WndProc`'s paint
/// (`0x0061DE92..0x0061E008`): TROFM through `0x006BA3E0` into the 50 columns
/// `{x + W - 49, y - 1}` at TROFM's own height, which shows TROFM's middle
/// columns and none of its red edge rows past the frames; TROFL at the
/// plaque's left, TROFR at `x + W + 1 - its width`, both at `y - 1`.
pub(super) fn paint_trackbar_plaque(
    out: &mut Vec<SpriteInstance>,
    chrome: &ControlChrome,
    rect: RectPx,
    depth: f32,
    scale: ShellScale,
) {
    // The plaque's offsets are authored numbers, so they are computed on the
    // window's base rect and each piece is stretched into device space. That
    // keeps the art on the same rect as [`value_text_rect`].
    let base = scale.base_rect(rect);
    let x = base.x + base.w - (PLAQUE_RESERVE - 1);
    let y = base.y - 1;
    if let Some(mid) = chrome.trackbar_plaque_mid_trofm {
        let h = mid.pixel_size[1].round() as i32;
        push_entry_wrapped_centered(
            out,
            mid,
            scale.scale_rect(RectPx::new(x, y, PLAQUE_RESERVE, h)),
            scale,
            depth,
        );
    }
    if let Some(left) = chrome.trackbar_plaque_left_trofl {
        push_entry(
            out,
            left,
            scale.scale_rect(RectPx::new(
                x,
                y,
                left.pixel_size[0].round() as i32,
                left.pixel_size[1].round() as i32,
            )),
            depth - 0.00001,
        );
    }
    if let Some(right) = chrome.trackbar_plaque_right_trofr {
        let w = right.pixel_size[0].round() as i32;
        push_entry(
            out,
            right,
            scale.scale_rect(RectPx::new(
                base.x + base.w + 1 - w,
                y,
                w,
                right.pixel_size[1].round() as i32,
            )),
            depth - 0.00001,
        );
    }
}

/// Render-side per-control paint state for the Slice 4 dispatch seam (O1: a
/// data-enum match like `ButtonPolicy`, NOT a trait). One variant per painted
/// control kind (mirrors `ui::shell::ControlKind`); each carries only the RESOLVED
/// per-control STATE (a checkbox's checked flag, a trackbar's thumb pixel offset) +
/// its rect — `paint_control` looks the glyphs up itself from the `ControlChrome`
/// subset (the atlas-taking seam shape). App-layer because the emitters + skirmish
/// layout live here (`render/` must not depend on the app layer, so the seam can't
/// sit in `render/shell_paint.rs` as the original draft assumed). 4A lands
/// `Checkbox`; 4B adds `Trackbar`; 4C adds the collapsed `Combo` face; 4D adds the
/// shared `ScrollBar` (combo dropdown + choose-map listbox); later steps add listbox arms.
pub(super) enum ControlPaint {
    Checkbox {
        checked: bool,
        rect: RectPx,
    },
    /// A checkbox in dual-state mode (message 0x4E5) or disabled: Diplomacy
    /// 0x73's Allies and Chat boxes.
    StatefulCheckbox {
        face: crate::ui::shell::checkbox::CheckboxFace,
        disabled: bool,
        rect: RectPx,
    },
    /// A trackbar window with its thumb's window-relative left edge
    /// ([`vera20k_gui::shell::trackbar::thumb_left`]). `plaque` is off where
    /// the owner sends `0x4AC` with 0 (Options Detail/Difficulty/Scroll, BBB).
    /// `disabled` is the window's `WS_DISABLED` style (`0x0061E0B0`).
    Trackbar {
        rect: RectPx,
        thumb_left: i32,
        plaque: bool,
        disabled: bool,
    },
    Combo {
        rect: RectPx,
        swatch: Option<[f32; 3]>,
        open: bool,
        disabled: bool,
    },
    ScrollBar {
        scrollbar: RectPx,
        thumb: RectPx,
        pressed_part: Option<DropdownScrollbarPart>,
    },
}

/// Slice 4 paint seam: emit one control, selecting the emitter by `ControlPaint`
/// variant (the per-control-kind dispatch) and resolving its glyphs from the
/// `ControlChrome` subset. Re-homes what were direct per-family emitter calls into
/// one match; emission ORDER (entry / rect / depth) is byte-identical to the
/// pre-seam code.
pub(super) fn paint_control(
    out: &mut Vec<SpriteInstance>,
    chrome: &ControlChrome,
    paint: ControlPaint,
    scale: ShellScale,
) {
    match paint {
        ControlPaint::Checkbox { checked, rect } => {
            let face = if checked {
                crate::ui::shell::checkbox::CheckboxFace::Checked
            } else {
                crate::ui::shell::checkbox::CheckboxFace::Unchecked
            };
            paint_checkbox_face(out, chrome, face, false, rect, scale);
        }
        ControlPaint::StatefulCheckbox {
            face,
            disabled,
            rect,
        } => paint_checkbox_face(out, chrome, face, disabled, rect, scale),
        ControlPaint::Trackbar {
            rect,
            thumb_left,
            plaque,
            disabled,
        } => {
            // The paint blits the plaque and the thumb, then draws two
            // adjacent border-2 frames; the frame entry includes their
            // two-pixel outside expansion. TRAKGRIP goes through a plain
            // blit into `{x + thumb_left, y, 12, H}`, so a shorter window
            // clips it rather than scaling it.
            if plaque {
                paint_trackbar_plaque(out, chrome, rect, SHELL_CONTROL_DEPTH, scale);
            }
            if let Some(thumb) = chrome.trackbar_thumb_trakgrip {
                push_entry_top_clipped(
                    out,
                    thumb,
                    RectPx::new(
                        rect.x + thumb_left,
                        rect.y,
                        (THUMB_W as f32 * scale.factor()).round() as i32,
                        rect.h,
                    ),
                    scale.factor(),
                    SHELL_CONTROL_DEPTH - 0.00002,
                );
                // A disabled window darkens the thumb rect with the overlay
                // alpha (`0x0061E0B0..0x0061E0D4`). RESIDUAL: native also
                // darkens the backdrop under TRAKGRIP's transparent texels.
                if disabled && let Some(sprite) = out.last_mut() {
                    sprite.tint = [DISABLED_OVERLAY_KEEP; 3];
                }
            }
            let base = scale.base_rect(rect);
            if let Some(frame) = crate::render::skirmish_shell_chrome::drawn_trackbar_frame_slot(
                plaque, base.w, base.h, scale,
            )
            .and_then(|slot| chrome.trackbar_frames[slot])
            {
                push_entry(
                    out,
                    frame,
                    scale.scale_rect(RectPx::new(
                        base.x - 2,
                        base.y - 2,
                        frame.pixel_size[0].round() as i32,
                        frame.pixel_size[1].round() as i32,
                    )),
                    SHELL_CONTROL_DEPTH - 0.00003,
                );
            }
        }
        ControlPaint::Combo {
            rect,
            swatch,
            open,
            disabled,
        } => {
            // Emission order preserved byte-for-byte from the pre-seam
            // push_combo_face: face glyph → swatch (color faces only) → arrow, at
            // the same depths. The caller resolves the swatch RGB so the arm stays
            // chrome-only.
            if let Some(face) = combo_face_entry(chrome, rect, scale) {
                push_entry(out, face, combo_face_rect(rect, scale), SHELL_CONTROL_DEPTH);
            }
            if let (Some(tint), Some(white)) = (swatch, chrome.white_pixel) {
                push_tinted_entry(
                    out,
                    white,
                    combo_swatch_rect(rect, scale),
                    tint,
                    SHELL_SWATCH_DEPTH,
                );
            }
            let arrow = match (disabled, open) {
                (true, true) => chrome.combo_arrow_down_gray_pressed,
                (true, false) => chrome.combo_arrow_down_gray_released,
                (false, true) => chrome.combo_arrow_down_pressed,
                (false, false) => chrome.combo_arrow_down_released,
            };
            if let Some(arrow) = arrow {
                // The arrow grows with its face: the anchor and the glyph's own
                // authored box both stretch, so the gaps to the face's right and
                // top edges keep their authored size in stretched pixels too.
                let arrow_rect = combo_arrow_rect(rect, scale);
                push_entry_grown(
                    out,
                    arrow,
                    arrow_rect.x,
                    arrow_rect.y,
                    scale,
                    SHELL_CONTROL_DEPTH - 0.00001,
                );
            }
        }
        ControlPaint::ScrollBar {
            scrollbar,
            thumb,
            pressed_part,
        } => {
            // Byte-for-byte the pre-seam push_dropdown_scrollbar_instances: track fill
            // → up arrow → down arrow → thumb(top/mid/bottom) → bevel frame, at the
            // hardcoded SHELL_DROPDOWN_DEPTH offsets the shared emitter used for BOTH
            // the combo popup and the choose-map listbox.
            push_solid_rect_px(
                out,
                chrome.white_pixel,
                scrollbar,
                SHELL_SCROLLBAR_TRACK_RGB_PENDING_SCROLLBAR_SOURCE_CAPTURE,
                SHELL_DROPDOWN_DEPTH - 0.00004,
            );
            let up_entry = scrollbar_arrow_entry(
                chrome.scrollbar_arrow_up_released,
                chrome.scrollbar_arrow_up_pressed,
                pressed_part == Some(DropdownScrollbarPart::UpArrow),
            );
            if let Some(up) = up_entry {
                push_entry_grown(
                    out,
                    up,
                    scrollbar.x,
                    scrollbar.y,
                    scale,
                    SHELL_DROPDOWN_DEPTH - 0.00005,
                );
            }
            let down_entry = scrollbar_arrow_entry(
                chrome.scrollbar_arrow_down_released,
                chrome.scrollbar_arrow_down_pressed,
                pressed_part == Some(DropdownScrollbarPart::DownArrow),
            );
            if let Some(down) = down_entry {
                push_entry_grown(
                    out,
                    down,
                    scrollbar.x,
                    scrollbar.y + scrollbar.h - DropdownMetrics::scaled(scale).button_h,
                    scale,
                    SHELL_DROPDOWN_DEPTH - 0.00005,
                );
            }
            push_scrollbar_thumb(out, chrome, thumb, scale, SHELL_DROPDOWN_DEPTH - 0.00006);
            push_ownerdraw_two_pixel_bevel_frame_px(
                out,
                chrome.white_pixel,
                scrollbar,
                SHELL_DROPDOWN_DEPTH - 0.00007,
            );
        }
    }
}

/// VERA's scrollbar beside the computer rows of a map with more than eight
/// starts: the shared list's frame and scrollbar.
pub(super) fn push_opponent_scrollbar_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    shell: &SkirmishShellState,
    maps: &[MapMenuEntry],
    // The arrow its gui List draws pushed.
    pressed: Option<vera20k_gui::shell::list::ListScrollPart>,
) {
    if let Some(geometry) = opponent_scroll_geometry(layout, shell, maps) {
        super::list::paint_list(
            out,
            atlas,
            geometry,
            shell.first_opponent,
            None,
            pressed,
            false,
            layout.scale,
        );
    }
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::render::skirmish_shell_chrome::{TRACKBAR_FRAMES, trackbar_frame_slot};
    use crate::ui::skirmish_shell::COMBO_DROPDOWN_SCROLLBAR_BUTTON_H;

    #[test]
    fn skirmish_dropdown_highlight_follows_pointer_without_committing_selection() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::ui::shell::controller::DialogController;
        use crate::ui::skirmish_shell::page::{self, Pointer};
        use crate::ui::skirmish_shell::{
            SkirmishComboId, combo_arrow_hit_rect, combo_dropdown_row_rect, combo_rect,
            compute_layout, selected_combo_item, selected_combo_item_index,
        };

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n[Americans]\nMultiplay=yes\nUIName=NAME:Americans\n[Russians]\nMultiplay=yes\nUIName=NAME:Russians\n",
        ))
        .unwrap();
        for layout in [
            compute_layout(800, 600),
            crate::ui::shell::geom::fit_window(1600, 1200, compute_layout),
        ] {
            for id in [
                SkirmishComboId::AiType(0),
                SkirmishComboId::Side(0),
                SkirmishComboId::Color(0),
                SkirmishComboId::Team(0),
            ] {
                let mut shell = SkirmishShellState::default();
                shell.refresh_country_catalog(&rules);
                let mut ui = DialogController::default();
                let labels = Default::default();
                let at = |shell: &mut SkirmishShellState, ui: &mut DialogController, p, (x, y)| {
                    page::pointer(ui, shell, &layout, &[], &labels, p, x, y);
                };
                // The painted highlight: the hot row on the list's grid.
                let hot = |ui: &DialogController| {
                    let list = ui.gui().combo_drop_down(ui.gui().popup()?)?;
                    list.popup.row_rect(list.hot_row?)
                };
                let selected = selected_combo_item(&shell, id);
                let arrow =
                    combo_arrow_hit_rect(combo_rect(&layout, &shell, id).unwrap(), layout.scale);
                at(&mut shell, &mut ui, Pointer::Down, (arrow.x, arrow.y));
                let open = page::open_combo(ui.gui(), &shell).unwrap();
                assert_eq!(open.id, id);
                let content = ui
                    .gui()
                    .combo_drop_down(ui.gui().popup().unwrap())
                    .unwrap()
                    .popup
                    .content();

                // Move through two rows, including a row other than the saved
                // choice. The renderer must follow each move immediately.
                for index in [0, 1] {
                    let row = combo_dropdown_row_rect(content, layout.scale, index);
                    at(&mut shell, &mut ui, Pointer::Move, (row.x + 1, row.y + 1));
                    assert_eq!(
                        hot(&ui),
                        Some(row),
                        "{id:?}, row {index}, scale {:?}",
                        layout.scale
                    );
                    assert_eq!(selected_combo_item(&shell, id), selected);
                }

                // Leaving the popup retains its last hot row. Dismissing it
                // must discard the preview when the control opens again.
                at(&mut shell, &mut ui, Pointer::Move, (-1, -1));
                assert_eq!(
                    hot(&ui),
                    Some(combo_dropdown_row_rect(content, layout.scale, 1))
                );
                at(&mut shell, &mut ui, Pointer::Down, (-1, -1));
                at(&mut shell, &mut ui, Pointer::Down, (arrow.x, arrow.y));
                let saved_index = selected_combo_item_index(&shell, &[], id).unwrap();
                assert_eq!(
                    hot(&ui),
                    Some(combo_dropdown_row_rect(content, layout.scale, saved_index))
                );
            }
        }
    }

    #[test]
    fn skirmish_swatches_match_the_native_fixed_table() {
        for (slot, rgb) in SKIRMISH_SWATCH_RGB.into_iter().enumerate() {
            let expected = [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ];
            assert_eq!(house_color_tint(None, slot), expected, "slot {slot}");
        }
    }

    #[test]
    fn team_caption_follows_the_majority_of_picked_swatch_hues() {
        use crate::rules::ini_parser::IniFile;

        // Two different blue shades outvote red, including a mod's dark navy.
        let ini = IniFile::from_str("[Colors]\nNavy=166,220,100\n[SkirmishColors]\n1=Navy\n");
        let rules = RuleSet::from_ini(&ini).unwrap();
        let team = team_caption_tint(Some(&rules), &[2, 9, 1]).expect("blue majority");
        assert!(team[2] > team[1] && team[2] > team[0]);
        assert_eq!(Some(team), team_caption_tint(Some(&rules), &[2, 9]));
        let navy = team_caption_tint(Some(&rules), &[9]).expect("navy");
        assert!(navy[2] > 0.8 && navy[2] - navy[0] > 0.2);
        // Navy against red is a tie: the caption mixes both.
        let mixed = team_caption_tint(Some(&rules), &[9, 1]).expect("a mix");
        assert!(mixed[0] > navy[0] && mixed[0] > mixed[1], "{mixed:?}");
        assert_eq!(team_caption_tint(None, &[]), None);
    }

    /// Team Colors gives each player on a team a lobby colour of its own
    /// from the team's range, so no colour family holds a majority: both
    /// captions still take a colour, Team A's warm and Team B's cool.
    #[test]
    fn team_colors_tint_both_team_captions() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let listed = crate::ui::team_tools::listed_hues(&rules);
        let colors = crate::ui::team_tools::colors_by_team(&listed, &[0, 1, 0, 1], 2);
        let team = |team: usize| -> Vec<u8> {
            colors
                .iter()
                .skip(team)
                .step_by(2)
                .flatten()
                .copied()
                .collect()
        };
        let warm = team_caption_tint(Some(&rules), &team(0)).expect("Team A's caption");
        let cool = team_caption_tint(Some(&rules), &team(1)).expect("Team B's caption");
        assert!(warm[0] > warm[2] && cool[2] > cool[0], "{warm:?} {cool:?}");
    }

    #[test]
    fn extra_colour_swatches_come_from_the_rules_and_the_eight_do_not() {
        use crate::rules::ini_parser::IniFile;

        // Entry 10 is priority 2's scheme, here black; Mine is the one extra.
        let mut text = String::from("[Colors]\n");
        for index in 0..15 {
            text.push_str(&format!("Stock{index}=0,0,0\n"));
        }
        text.push_str("Mine=0,255,255\n[SkirmishColors]\n1=Mine\n");
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        let grey = [96.0 / 255.0; 3];

        assert_eq!(house_color_tint(Some(&rules), 2), house_color_tint(None, 2));
        // Priority 9 is the first extra colour: pure red here.
        assert_eq!(house_color_tint(Some(&rules), 9), [1.0, 0.0, 0.0]);
        // 8 is the observer's, 10 is past this list, and no rules list none.
        assert_eq!(house_color_tint(Some(&rules), 8), grey);
        assert_eq!(house_color_tint(Some(&rules), 10), grey);
        assert_eq!(house_color_tint(None, 9), grey);
    }

    #[test]
    fn checkbox_paint_seam_emits_icon_at_icon_rect_with_control_depth() {
        // Draw-list assertion (Slice 4 §1.4): the per-control-kind seam emits
        // exactly one instance, at the 18x18 icon rect, at SHELL_CONTROL_DEPTH,
        // carrying the resolved checked-icon entry's uv — byte-identical to the
        // pre-seam push_checkbox_instances emission. The seam resolves the icon
        // from the ControlChrome subset itself (the atlas-taking seam shape).
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.1, 0.2],
            uv_size: [0.3, 0.4],
            pixel_size: [18.0, 18.0],
        };
        let chrome = ControlChrome {
            checkbox_checked_cce_i: Some(entry),
            ..Default::default()
        };
        let rect = RectPx::new(71, 286, 150, 16);
        let icon = checkbox_icon_rect(rect, crate::ui::shell::geom::RETAIL_SCALE);

        let mut out = Vec::new();
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Checkbox {
                checked: true,
                rect,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [icon.x as f32, icon.y as f32]);
        assert_eq!(out[0].size, [icon.w as f32, icon.h as f32]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_eq!(out[0].uv_size, entry.uv_size);
        assert_eq!(out[0].depth, SHELL_CONTROL_DEPTH);

        // Missing icon → no instance (matches the pre-seam `else continue`).
        let mut empty = Vec::new();
        paint_control(
            &mut empty,
            &ControlChrome::default(),
            ControlPaint::Checkbox {
                checked: true,
                rect,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert!(empty.is_empty());
    }

    fn entry(origin: f32, width: f32, height: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [origin, origin + 0.01],
            uv_size: [0.2, 0.1],
            pixel_size: [width, height],
        }
    }

    #[test]
    fn trackbar_paint_places_the_native_plaque_thumb_and_frames() {
        // Retail art sizes: TROFM 114x24, TROFL 8x24, TROFR 10x24,
        // TRAKGRIP 12x22; the Skirmish Credits window 129x22.
        let mid = entry(0.1, 114.0, 24.0);
        let left = entry(0.2, 8.0, 24.0);
        let right = entry(0.3, 10.0, 24.0);
        let thumb = entry(0.4, 12.0, 22.0);
        let frame = entry(0.5, 133.0, 26.0);
        let mut chrome = ControlChrome {
            trackbar_plaque_mid_trofm: Some(mid),
            trackbar_plaque_left_trofl: Some(left),
            trackbar_plaque_right_trofr: Some(right),
            trackbar_thumb_trakgrip: Some(thumb),
            ..Default::default()
        };
        chrome.trackbar_frames[trackbar_frame_slot(true, 129, 22).unwrap()] = Some(frame);
        let rect = RectPx::new(404, 314, 129, 22);
        for thumb_left in [1, 34, 67] {
            let mut out = Vec::new();
            paint_control(
                &mut out,
                &chrome,
                ControlPaint::Trackbar {
                    rect,
                    thumb_left,
                    plaque: true,
                    disabled: false,
                },
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            assert_eq!(out.len(), 5, "plaque (mid, left, right), thumb, frame");
            // TROFM's middle 50 columns (32..82) at its own height: its red
            // first and last rows land under the frames, not inside them.
            assert_eq!(out[0].position, [484.0, 313.0]);
            assert_eq!(out[0].size, [50.0, 24.0]);
            assert_eq!(out[0].uv_origin, [0.1 + 0.2 * 32.0 / 114.0, 0.11]);
            assert_eq!(out[0].uv_size, [0.2 * (50.0 / 114.0), 0.1]);
            assert_eq!(out[0].depth, SHELL_CONTROL_DEPTH);
            assert_eq!(out[1].position, [484.0, 313.0]);
            assert_eq!(out[1].size, left.pixel_size);
            assert_eq!(out[2].position, [524.0, 313.0]);
            assert_eq!(out[2].size, right.pixel_size);
            assert_eq!(out[3].position, [(404 + thumb_left) as f32, 314.0]);
            assert_eq!(out[3].size, [12.0, 22.0]);
            assert_eq!(out[3].uv_size, thumb.uv_size);
            assert_eq!(out[3].depth, SHELL_CONTROL_DEPTH - 0.00002);
            assert_eq!(out[4].position, [402.0, 312.0]);
            assert_eq!(out[4].size, frame.pixel_size);
            assert_eq!(out[4].uv_origin, frame.uv_origin);
            assert_eq!(out[4].depth, SHELL_CONTROL_DEPTH - 0.00003);
        }

        // A 21-pixel window (in-game B8) clips the 22-pixel thumb.
        let mut out = Vec::new();
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Trackbar {
                rect: RectPx::new(236, 98, 263, 21),
                thumb_left: 1,
                plaque: false,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(out.len(), 1, "no plaque, no frame of this size");
        assert_eq!(out[0].size, [12.0, 21.0]);
        assert_eq!(out[0].uv_size, [0.2, 0.1 * (21.0 / 22.0)]);

        let mut empty = Vec::new();
        paint_control(
            &mut empty,
            &ControlChrome::default(),
            ControlPaint::Trackbar {
                rect,
                thumb_left: 1,
                plaque: true,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn a_disabled_trackbar_darkens_only_its_thumb() {
        let chrome = ControlChrome {
            trackbar_plaque_mid_trofm: Some(entry(0.1, 114.0, 24.0)),
            trackbar_thumb_trakgrip: Some(entry(0.4, 12.0, 22.0)),
            ..Default::default()
        };
        let paint = |disabled| {
            let mut out = Vec::new();
            paint_control(
                &mut out,
                &chrome,
                ControlPaint::Trackbar {
                    rect: RectPx::new(404, 314, 129, 22),
                    thumb_left: 34,
                    plaque: true,
                    disabled,
                },
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            out.iter().map(|sprite| sprite.tint).collect::<Vec<_>>()
        };
        assert_eq!(paint(false), [[1.0; 3]; 2]);
        // 0x60 overlay: (255 - 0x60) / 256 of each channel.
        assert_eq!(paint(true), [[1.0; 3], [159.0 / 256.0; 3]]);
    }

    #[test]
    fn each_trackbar_window_takes_its_own_frame() {
        let mut chrome = ControlChrome::default();
        for (slot, &(width, height, _)) in TRACKBAR_FRAMES.iter().enumerate() {
            chrome.trackbar_frames[slot] = Some(entry(
                slot as f32 / 10.0,
                (width + 4) as f32,
                (height + 4) as f32,
            ));
        }
        for &(width, height, reserve) in &TRACKBAR_FRAMES {
            let mut out = Vec::new();
            paint_control(
                &mut out,
                &chrome,
                ControlPaint::Trackbar {
                    rect: RectPx::new(236, 98, width, height),
                    thumb_left: 1,
                    plaque: reserve > 0,
                    disabled: false,
                },
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            assert_eq!(out.len(), 1, "{width}x{height}");
            assert_eq!(out[0].position, [234.0, 96.0]);
            assert_eq!(out[0].size, [(width + 4) as f32, (height + 4) as f32]);
        }
        // The resource sizes before the runtime growth have no frame.
        assert!(chrome.trackbar_frame(true, 128, 21).is_none());
        assert!(chrome.trackbar_frame(false, 180, 21).is_none());
    }

    /// Every trackbar window the shell pages paint has a frame, in a window
    /// large enough that the room's team bar draws above the shell's size
    /// too.
    #[test]
    fn every_shell_trackbar_window_has_a_frame() {
        use crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout;
        use crate::ui::skirmish_shell::{compute_layout, compute_random_map_setup_layout};
        for (width, height) in [
            (640u32, 480u32),
            (800, 600),
            (1024, 768),
            (1600, 900),
            (3440, 1440),
        ] {
            let skirmish = compute_layout(width, height);
            let players = compute_random_map_setup_layout(width, height).control_rects[5];
            let mut windows = vec![
                (true, skirmish.static_rect(0x529)),
                (true, skirmish.static_rect(0x511)),
                (true, skirmish.static_rect(0x50C)),
                (true, players),
            ];
            // 640x480 fits Options to the window, so its rects map back first
            // and take the drawn frame lookup, as the paint does.
            let mut drawn = Vec::new();
            for category in crate::ui::main_menu_dialogs::options::Category::ALL {
                let layout = LauncherOptionsLayout::new(width as i32, height as i32, category);
                drawn.extend(
                    layout
                        .settings()
                        .filter_map(|(setting, rect)| match setting {
                            crate::ui::main_menu_dialogs::options::Setting::Slider(id) => Some((
                                id.plaque_reserve() > 0,
                                layout.scale.base_rect(rect),
                                layout.scale,
                            )),
                            _ => None,
                        }),
                );
            }
            windows.extend(
                crate::ui::shell::sound::ShellSoundLayout::new(
                    width as i32,
                    height as i32,
                    crate::ui::shell::geom::RETAIL_SCALE,
                )
                .sliders
                .map(|rect| (true, rect)),
            );
            // The online pause menu's Internet Game Controls.
            let shell =
                crate::ui::shell::in_game_shell::allied_fixture_layout(width as i32, height as i32);
            let internet = crate::ui::shell::pause_menu::pause_menu_layout(
                width as i32,
                height as i32,
                shell,
                [125, 25],
                crate::ui::shell::pause_menu::PauseMenuTemplate::Internet,
            )
            .internet
            .expect("FF has its sliders");
            windows.extend(internet.sliders.map(|rect| (true, rect)));
            use crate::ui::multiplayer_setup::{
                DIFFICULTY, MultiplayerSetupLayout, ROOM_SIZE, SUPERWEAPON_LIMIT, SetupDialog,
                SetupFlavor, TEAM_COUNT, TECH_LEVEL,
            };
            // gamemd's setup dialogs and VERA's room, in a centred shell and
            // one filling the window; only the room paints its Teams count,
            // Superweapon Limit and Tech Level, and only its host Room size.
            for (dialog, fills) in [SetupFlavor::Lan, SetupFlavor::Wol]
                .into_iter()
                .flat_map(|flavor| {
                    [(false, false), (true, false), (false, true), (true, true)].map(
                        |(host, modern)| SetupDialog {
                            host,
                            flavor,
                            modern,
                        },
                    )
                })
                .flat_map(|dialog| [(dialog, false), (dialog, true)])
            {
                let layout =
                    MultiplayerSetupLayout::new(dialog, width as i32, height as i32, fills);
                // The room's team bar draws its Teams count at 78% size.
                drawn.extend(
                    layout
                        .trackbars
                        .into_iter()
                        .filter(|(id, _)| match *id {
                            TEAM_COUNT | SUPERWEAPON_LIMIT | TECH_LEVEL => dialog.modern,
                            ROOM_SIZE => dialog.modern && dialog.host,
                            _ => true,
                        })
                        .map(|(id, rect)| {
                            let scale = layout.control_scale(id);
                            (id != DIFFICULTY, scale.base_rect(rect), scale)
                        }),
                );
            }
            for (plaque, rect) in windows {
                assert!(
                    trackbar_frame_slot(plaque, rect.w, rect.h).is_some(),
                    "{rect:?} at {width}x{height}"
                );
            }
            for (plaque, rect, scale) in drawn {
                assert!(
                    crate::render::skirmish_shell_chrome::drawn_trackbar_frame_slot(
                        plaque, rect.w, rect.h, scale
                    )
                    .is_some(),
                    "{rect:?} at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn lan_setup_layouts_resolve_and_paint_every_combo_face() {
        use crate::ui::multiplayer_setup::MultiplayerSetupLayout;

        let face = |width| SkirmishShellChromeEntry {
            uv_origin: [width as f32 / 1000.0, 0.2],
            uv_size: [0.1, 0.1],
            pixel_size: [width as f32, 24.0],
        };
        let chrome = ControlChrome {
            combo_face_151: Some(face(151)),
            combo_face_118: Some(face(118)),
            combo_face_42: Some(face(42)),
            combo_face_39: Some(face(39)),
            ..Default::default()
        };
        use crate::ui::multiplayer_setup::{SetupDialog, SetupFlavor};
        for dialog in [SetupFlavor::Lan, SetupFlavor::Wol]
            .into_iter()
            .flat_map(|flavor| {
                [false, true].map(|host| SetupDialog {
                    host,
                    flavor,
                    modern: false,
                })
            })
        {
            for (width, height, responsive) in [
                (640, 480, false),
                (800, 600, false),
                (1024, 768, false),
                (1280, 960, true),
            ] {
                let layout = MultiplayerSetupLayout::new(dialog, width, height, responsive);
                for rect in layout.combos.into_iter().flatten() {
                    let entry = combo_face_entry(&chrome, rect, layout.page.scale)
                        .expect("every BC/BD combo window must resolve an atlas face");
                    assert_eq!(
                        entry.pixel_size[0] as i32,
                        layout.page.scale.base_rect(rect).w
                    );
                    for disabled in [false, true] {
                        let mut out = Vec::new();
                        paint_control(
                            &mut out,
                            &chrome,
                            ControlPaint::Combo {
                                rect,
                                swatch: None,
                                open: false,
                                disabled,
                            },
                            layout.page.scale,
                        );
                        assert_eq!(out.len(), 1, "missing BC/BD face at {rect:?}");
                        assert_eq!(out[0].uv_origin, entry.uv_origin);
                        let target = combo_face_rect(rect, layout.page.scale);
                        assert_eq!(out[0].position, [target.x as f32, target.y as f32]);
                        assert_eq!(out[0].size, [target.w as f32, target.h as f32]);
                    }
                }
            }
        }
    }

    #[test]
    fn keyboard_category_uses_its_wide_native_face() {
        let face = SkirmishShellChromeEntry {
            uv_origin: [0.4, 0.2],
            uv_size: [0.3, 0.1],
            pixel_size: [207.0, 24.0],
        };
        let chrome = ControlChrome {
            combo_face_207: Some(face),
            ..Default::default()
        };
        let mut out = Vec::new();
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::Combo {
                rect: RectPx::new(95, 135, 207, 24),
                swatch: None,
                open: false,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [95.0, 135.0]);
        assert_eq!(out[0].size, [207.0, 24.0]);
        assert_eq!(out[0].uv_origin, face.uv_origin);
    }

    #[test]
    fn combo_face_paint_seam_emits_face_swatch_arrow() {
        // Draw-list assertion (Slice 4 §1.4): the collapsed combo seam emits face
        // glyph → swatch (color faces only) → arrow variant, in the exact
        // order/positions/depths of the pre-seam push_combo_face emitter, across the
        // four face shapes — (a) color open, (b) color closed, (c) plain, (d)
        // disabled. Geometry is pinned via the same layout helpers the emitter uses.
        let face = SkirmishShellChromeEntry {
            uv_origin: [0.01, 0.02],
            uv_size: [0.03, 0.04],
            pixel_size: [150.0, 24.0],
        };
        let white = SkirmishShellChromeEntry {
            uv_origin: [0.11, 0.12],
            uv_size: [0.13, 0.14],
            pixel_size: [1.0, 1.0],
        };
        let arrow_released = SkirmishShellChromeEntry {
            uv_origin: [0.21, 0.22],
            uv_size: [0.23, 0.24],
            pixel_size: [16.0, 16.0],
        };
        let arrow_pressed = SkirmishShellChromeEntry {
            uv_origin: [0.31, 0.32],
            uv_size: [0.33, 0.34],
            pixel_size: [16.0, 16.0],
        };
        let arrow_gray_released = SkirmishShellChromeEntry {
            uv_origin: [0.41, 0.42],
            uv_size: [0.43, 0.44],
            pixel_size: [16.0, 16.0],
        };
        let arrow_gray_pressed = SkirmishShellChromeEntry {
            uv_origin: [0.51, 0.52],
            uv_size: [0.53, 0.54],
            pixel_size: [16.0, 16.0],
        };
        let chrome = ControlChrome {
            combo_face_150: Some(face),
            white_pixel: Some(white),
            combo_arrow_down_released: Some(arrow_released),
            combo_arrow_down_pressed: Some(arrow_pressed),
            combo_arrow_down_gray_released: Some(arrow_gray_released),
            combo_arrow_down_gray_pressed: Some(arrow_gray_pressed),
            ..Default::default()
        };
        // w=150 selects combo_face_150 via combo_face_entry.
        let rect = RectPx::new(176, 100, 150, 24);
        let face_rect = combo_face_rect(rect, crate::ui::shell::geom::RETAIL_SCALE);
        let swatch_rect = combo_swatch_rect(rect, crate::ui::shell::geom::RETAIL_SCALE);
        let arrow_rect = combo_arrow_rect(rect, crate::ui::shell::geom::RETAIL_SCALE);
        let tint = [0.25, 0.5, 0.75];

        let assert_face = |inst: &SpriteInstance| {
            assert_eq!(inst.position, [face_rect.x as f32, face_rect.y as f32]);
            assert_eq!(inst.size, [face_rect.w as f32, face_rect.h as f32]);
            assert_eq!(inst.uv_origin, face.uv_origin);
            assert_eq!(inst.depth, SHELL_CONTROL_DEPTH);
        };
        let assert_arrow = |inst: &SpriteInstance, entry: SkirmishShellChromeEntry| {
            assert_eq!(inst.position, [arrow_rect.x as f32, arrow_rect.y as f32]);
            assert_eq!(inst.size, entry.pixel_size);
            assert_eq!(inst.uv_origin, entry.uv_origin);
            assert_eq!(inst.depth, SHELL_CONTROL_DEPTH - 0.00001);
        };

        // (a) color face open: face → swatch → pressed arrow.
        let mut a = Vec::new();
        paint_control(
            &mut a,
            &chrome,
            ControlPaint::Combo {
                rect,
                swatch: Some(tint),
                open: true,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(a.len(), 3, "face + swatch + arrow");
        assert_face(&a[0]);
        assert_eq!(a[1].position, [swatch_rect.x as f32, swatch_rect.y as f32]);
        assert_eq!(a[1].size, [swatch_rect.w as f32, swatch_rect.h as f32]);
        assert_eq!(a[1].uv_origin, white.uv_origin);
        assert_eq!(a[1].tint, tint);
        assert_eq!(a[1].depth, SHELL_SWATCH_DEPTH);
        assert_arrow(&a[2], arrow_pressed);

        // (b) color face closed: face → swatch → released arrow.
        let mut b = Vec::new();
        paint_control(
            &mut b,
            &chrome,
            ControlPaint::Combo {
                rect,
                swatch: Some(tint),
                open: false,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(b.len(), 3);
        assert_face(&b[0]);
        assert_eq!(b[1].tint, tint);
        assert_eq!(b[1].depth, SHELL_SWATCH_DEPTH);
        assert_arrow(&b[2], arrow_released);

        // (c) plain face (no swatch): face → released arrow.
        let mut c = Vec::new();
        paint_control(
            &mut c,
            &chrome,
            ControlPaint::Combo {
                rect,
                swatch: None,
                open: false,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(c.len(), 2, "face + arrow, no swatch");
        assert_face(&c[0]);
        assert_arrow(&c[1], arrow_released);

        // (d) disabled face (no swatch): face → gray released arrow.
        let mut d = Vec::new();
        paint_control(
            &mut d,
            &chrome,
            ControlPaint::Combo {
                rect,
                swatch: None,
                open: false,
                disabled: true,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(d.len(), 2);
        assert_face(&d[0]);
        assert_arrow(&d[1], arrow_gray_released);

        // (e) the same face stretched 2x: the arrow grows with the face, so its
        // authored 3x1 gap to the face's right/top edges keeps its proportion.
        let double = crate::ui::shell::geom::shell_scale(1600, 1200);
        let wide = double.scale_rect(rect);
        let wide_arrow = combo_arrow_rect(wide, double);
        let mut e = Vec::new();
        paint_control(
            &mut e,
            &chrome,
            ControlPaint::Combo {
                rect: wide,
                swatch: None,
                open: false,
                disabled: false,
            },
            double,
        );
        assert_eq!(e.len(), 2, "the authored face width still selects 150");
        assert_eq!(e[0].size, [wide.w as f32, wide.h as f32]);
        assert_eq!(e[1].position, [wide_arrow.x as f32, wide_arrow.y as f32]);
        assert_eq!(e[1].size, [32.0, 32.0]);
        assert_eq!((wide.x + wide.w - (wide_arrow.x + 32)) / 2, 3);
        assert_eq!((wide_arrow.y - wide.y) / 2, 1);

        // Empty chrome (no glyphs) → nothing emitted.
        let mut empty = Vec::new();
        paint_control(
            &mut empty,
            &ControlChrome::default(),
            ControlPaint::Combo {
                rect,
                swatch: Some(tint),
                open: true,
                disabled: false,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn scrollbar_paint_seam_emits_track_arrows_thumb_bevel() {
        // Draw-list assertion (Slice 4 §1.4): the ScrollBar arm reproduces the
        // pre-seam push_dropdown_scrollbar_instances sequence — track fill →
        // up arrow → down arrow → thumb(top/bottom/mid) → 2-ring bevel frame — at
        // the hardcoded SHELL_DROPDOWN_DEPTH offsets, for both pressed states.
        let white = SkirmishShellChromeEntry {
            uv_origin: [0.01, 0.02],
            uv_size: [0.03, 0.04],
            pixel_size: [1.0, 1.0],
        };
        let up_r = SkirmishShellChromeEntry {
            uv_origin: [0.11, 0.12],
            uv_size: [0.13, 0.14],
            pixel_size: [20.0, 22.0],
        };
        let up_p = SkirmishShellChromeEntry {
            uv_origin: [0.21, 0.22],
            uv_size: [0.23, 0.24],
            pixel_size: [20.0, 22.0],
        };
        let dn_r = SkirmishShellChromeEntry {
            uv_origin: [0.31, 0.32],
            uv_size: [0.33, 0.34],
            pixel_size: [20.0, 22.0],
        };
        let dn_p = SkirmishShellChromeEntry {
            uv_origin: [0.41, 0.42],
            uv_size: [0.43, 0.44],
            pixel_size: [20.0, 22.0],
        };
        let th_t = SkirmishShellChromeEntry {
            uv_origin: [0.51, 0.52],
            uv_size: [0.53, 0.54],
            pixel_size: [20.0, 6.0],
        };
        let th_m = SkirmishShellChromeEntry {
            uv_origin: [0.61, 0.62],
            uv_size: [0.63, 0.64],
            pixel_size: [20.0, 4.0],
        };
        let th_b = SkirmishShellChromeEntry {
            uv_origin: [0.71, 0.72],
            uv_size: [0.73, 0.74],
            pixel_size: [20.0, 6.0],
        };
        let chrome = ControlChrome {
            white_pixel: Some(white),
            scrollbar_arrow_up_released: Some(up_r),
            scrollbar_arrow_up_pressed: Some(up_p),
            scrollbar_arrow_down_released: Some(dn_r),
            scrollbar_arrow_down_pressed: Some(dn_p),
            scrollbar_thumb_top: Some(th_t),
            scrollbar_thumb_mid: Some(th_m),
            scrollbar_thumb_bottom: Some(th_b),
            ..Default::default()
        };
        // scrollbar tall enough for top+bottom thumb caps + a positive mid span.
        let scrollbar = RectPx::new(300, 100, 20, 120);
        let thumb = RectPx::new(300, 144, 20, 30);

        // Default (no pressed part): both arrows show the RELEASED glyph.
        let mut out = Vec::new();
        paint_control(
            &mut out,
            &chrome,
            ControlPaint::ScrollBar {
                scrollbar,
                thumb,
                pressed_part: None,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        // track(1) + up(1) + down(1) + thumb top/bottom/mid(3) + 2 bevel rings × 4
        // edges each (each ring: top/left/bottom/right solid rects) = 8.
        assert_eq!(out.len(), 14, "track + 2 arrows + 3 thumb + 8 bevel edges");

        // 0: track fill — white pixel tinted, scrollbar rect, DEPTH-0.00004.
        assert_eq!(out[0].position, [scrollbar.x as f32, scrollbar.y as f32]);
        assert_eq!(out[0].size, [scrollbar.w as f32, scrollbar.h as f32]);
        assert_eq!(out[0].uv_origin, white.uv_origin);
        assert_eq!(out[0].depth, SHELL_DROPDOWN_DEPTH - 0.00004);

        // 1: up arrow — released, native at scrollbar origin, DEPTH-0.00005.
        assert_eq!(out[1].position, [scrollbar.x as f32, scrollbar.y as f32]);
        assert_eq!(out[1].uv_origin, up_r.uv_origin);
        assert_eq!(out[1].depth, SHELL_DROPDOWN_DEPTH - 0.00005);

        // 2: down arrow — released, native at the bottom button slot, DEPTH-0.00005.
        assert_eq!(
            out[2].position,
            [
                scrollbar.x as f32,
                (scrollbar.y + scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H) as f32
            ]
        );
        assert_eq!(out[2].uv_origin, dn_r.uv_origin);
        assert_eq!(out[2].depth, SHELL_DROPDOWN_DEPTH - 0.00005);

        // 3: thumb top — native at thumb origin, DEPTH-0.00006.
        assert_eq!(out[3].position, [thumb.x as f32, thumb.y as f32]);
        assert_eq!(out[3].uv_origin, th_t.uv_origin);
        assert_eq!(out[3].depth, SHELL_DROPDOWN_DEPTH - 0.00006);

        // 4: thumb bottom — native, bottom-aligned in the thumb rect.
        let bottom_h = th_b.pixel_size[1].round() as i32;
        assert_eq!(
            out[4].position,
            [thumb.x as f32, (thumb.y + thumb.h - bottom_h) as f32]
        );
        assert_eq!(out[4].uv_origin, th_b.uv_origin);

        // 5: thumb mid — stretched between caps.
        let top_h = th_t.pixel_size[1].round() as i32;
        assert_eq!(out[5].position, [thumb.x as f32, (thumb.y + top_h) as f32]);
        assert_eq!(
            out[5].size,
            [thumb.w as f32, (thumb.h - top_h - bottom_h) as f32]
        );
        assert_eq!(out[5].uv_origin, th_m.uv_origin);

        // 6..10: outer bevel ring (4 edges) at DEPTH-0.00007; 10..14: inner ring at -0.00008.
        for inst in &out[6..10] {
            assert_eq!(
                inst.depth,
                SHELL_DROPDOWN_DEPTH - 0.00007,
                "outer bevel ring depth"
            );
        }
        for inst in &out[10..14] {
            assert_eq!(
                inst.depth,
                SHELL_DROPDOWN_DEPTH - 0.00007 - 0.00001,
                "inner bevel ring depth"
            );
        }

        // Pressed up-arrow swaps ONLY the up glyph to the pressed uv.
        let mut pressed = Vec::new();
        paint_control(
            &mut pressed,
            &chrome,
            ControlPaint::ScrollBar {
                scrollbar,
                thumb,
                pressed_part: Some(DropdownScrollbarPart::UpArrow),
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert_eq!(pressed[1].uv_origin, up_p.uv_origin);
        assert_eq!(pressed[2].uv_origin, dn_r.uv_origin);

        // Empty chrome → nothing emitted.
        let mut empty = Vec::new();
        paint_control(
            &mut empty,
            &ControlChrome::default(),
            ControlPaint::ScrollBar {
                scrollbar,
                thumb,
                pressed_part: None,
            },
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn checkbox_icon_rect_right_edge_is_half_open() {
        // The toggle keys off the 18x18 icon rect via RectPx::contains
        // (half-open): the last interior px (icon.x+17) HITS, the right edge
        // (icon.x+18) MISSES — the boundary the input toggle branch relies on.
        let rect = RectPx::new(71, 286, 150, 16);
        let icon = checkbox_icon_rect(rect, crate::ui::shell::geom::RETAIL_SCALE);
        assert_eq!(icon.w, 18, "C-Checkbox icon width is 18px");
        assert!(
            icon.contains(icon.x + icon.w - 1, icon.y),
            "last interior px hits"
        );
        assert!(
            !icon.contains(icon.x + icon.w, icon.y),
            "right edge (x+18) misses"
        );
    }
}

/// A disabled owner-draw control's `AlphaBlendRect` colour 0 at the overlay
/// alpha `[0xAC4898]` = 0x60 (`0x0060FAB9`) keeps `(255 - 0x60) / 256` of each
/// channel. As a tint it skips the RGB565 truncation (at most one unit per
/// channel).
const DISABLED_OVERLAY_KEEP: f32 = (255 - 0x60) as f32 / 256.0;

/// Owner-draw checkbox 0x006163A0's WM_PAINT: the face PCX for its check
/// and dual-state flags at the control's origin, then for a disabled window
/// the overlay ([`DISABLED_OVERLAY_KEEP`]).
fn paint_checkbox_face(
    out: &mut Vec<SpriteInstance>,
    chrome: &ControlChrome,
    face: crate::ui::shell::checkbox::CheckboxFace,
    disabled: bool,
    rect: RectPx,
    scale: ShellScale,
) {
    use crate::ui::shell::checkbox::CheckboxFace;
    let entry = match face {
        CheckboxFace::Unchecked => chrome.checkbox_unchecked_cue_i,
        CheckboxFace::Checked => chrome.checkbox_checked_cce_i,
        CheckboxFace::CheckedAlone => chrome.checkbox_dual_left_cce_il,
        CheckboxFace::AnsweredAlone => chrome.checkbox_dual_right_cce_ir,
    };
    let Some(entry) = entry else {
        return;
    };
    let icon = checkbox_icon_rect(rect, scale);
    if disabled {
        push_tinted_entry(
            out,
            entry,
            icon,
            [DISABLED_OVERLAY_KEEP; 3],
            SHELL_CONTROL_DEPTH,
        );
    } else {
        push_entry(out, entry, icon, SHELL_CONTROL_DEPTH);
    }
}

/// A shell dialog's list (`0x00618D40`) of its `(selected, top, pushed
/// arrow)`: the frame, the selected row's fill and the scrollbar.
pub(super) fn list_view_controls(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    geometry: vera20k_gui::shell::list::ShellListGeometry,
    (selected, top, pressed): (
        Option<usize>,
        usize,
        Option<vera20k_gui::shell::list::ListScrollPart>,
    ),
    scale: ShellScale,
) {
    // Frame lines and scrollbar geometry are shared with all native lists.
    // Their backing remains the composed parent. Its one-RGB565-unit darkening
    // is a small presentation residual until the common atlas carries it.
    for (line, tone) in vera20k_gui::shell::list::list_frame_lines(scale.base_rect(geometry.outer))
    {
        push_solid_rect(
            out,
            atlas,
            scale.scale_rect(line),
            tone.rgb().map(|c| f32::from(c) / 255.0),
            SHELL_CONTROL_DEPTH,
        );
    }
    if let Some(index) = selected
        && let Some(visible) = index.checked_sub(top)
        && visible < geometry.visible_rows
    {
        push_solid_rect(
            out,
            atlas,
            geometry.row(visible),
            OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
            SHELL_CONTROL_DEPTH - 0.00001,
        );
    }
    if let (Some(scrollbar), Some(thumb)) = (geometry.scrollbar, geometry.thumb) {
        paint_control(
            out,
            &atlas.control_chrome(),
            ControlPaint::ScrollBar {
                scrollbar,
                thumb,
                pressed_part: pressed.map(dropdown_scroll_part),
            },
            scale,
        );
    }
}

/// Shared ComboDropWin paint60E0C0: background, hot row, optional swatches,
/// scrollbar and two bevel rings. Callers supply their control's row model.
pub(super) fn push_popup_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    selected: Option<RectPx>,
    swatches: impl IntoIterator<Item = (RectPx, [f32; 3])>,
    scrollbar: Option<(RectPx, RectPx)>,
    pressed: Option<DropdownScrollbarPart>,
    scale: ShellScale,
    frame_depth: f32,
) {
    push_solid_rect(
        out,
        atlas,
        rect,
        SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
        SHELL_DROPDOWN_DEPTH,
    );
    if let Some(row) = selected {
        push_solid_rect(
            out,
            atlas,
            row,
            OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
            SHELL_DROPDOWN_DEPTH - 0.00001,
        );
    }
    for (rect, rgb) in swatches {
        push_solid_rect(out, atlas, rect, rgb, SHELL_DROPDOWN_DEPTH - 0.00002);
    }
    if let Some((scrollbar, thumb)) = scrollbar {
        paint_control(
            out,
            &atlas.control_chrome(),
            ControlPaint::ScrollBar {
                scrollbar,
                thumb,
                pressed_part: pressed,
            },
            scale,
        );
    }
    push_ownerdraw_two_pixel_bevel_frame(out, atlas, rect, frame_depth);
}
pub(super) fn dropdown_scroll_part(
    part: vera20k_gui::shell::list::ListScrollPart,
) -> DropdownScrollbarPart {
    use vera20k_gui::shell::list::ListScrollPart;
    match part {
        ListScrollPart::Up => DropdownScrollbarPart::UpArrow,
        ListScrollPart::Down => DropdownScrollbarPart::DownArrow,
        ListScrollPart::Thumb => DropdownScrollbarPart::Thumb,
        ListScrollPart::Track => DropdownScrollbarPart::Track,
    }
}
