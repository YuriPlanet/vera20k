//! Paints a dialog's `vera20k-gui` widgets with the shell's owner-draw
//! emitters: one place that turns Checkbox, Trackbar, Combo and Text nodes into the
//! sprites and texts each screen used to assemble by hand. A screen keeps
//! its backdrop and chrome and calls [`paint_gui_children`] for the widgets
//! it built under its dialog root.

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use super::controls::{ControlPaint, dropdown_scroll_part, paint_control, push_popup_instances};
use super::text::{combo_face_text_color, rect_to_text_rect};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F, SHELL_DROPDOWN_DEPTH,
    SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::trackbar::{TrackbarRange, value_text_rect};

/// Sprites (skirmish atlas) and texts of a dialog's widgets, and of an open
/// combo drop-down over them.
#[derive(Default)]
pub(crate) struct GuiPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
    pub(crate) popup: Vec<SpriteInstance>,
    pub(crate) popup_texts: Vec<ShellTextDraw>,
}

/// A node's caption: `Label` is a CSF key (shown as the key when missing),
/// `Text` a literal.
fn caption(state: &AppState, gui: &Gui, node: NodeId) -> Option<String> {
    match gui.get(node, Prop::Label).ok().flatten() {
        Some(Value::Text(key)) => Some(
            state
                .process_assets
                .csf
                .as_ref()
                .map_or_else(|| key.clone(), |csf| csf.text(key).into_owned()),
        ),
        _ => match gui.get(node, Prop::Text).ok().flatten() {
            Some(Value::Text(text)) => Some(text.clone()),
            _ => None,
        },
    }
}

fn int(gui: &Gui, node: NodeId, prop: Prop, default: i32) -> i32 {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Int(v)) => *v,
        _ => default,
    }
}

fn flag(gui: &Gui, node: NodeId, prop: Prop, default: bool) -> bool {
    match gui.get(node, prop).ok().flatten() {
        Some(Value::Bool(v)) => *v,
        _ => default,
    }
}

fn rgb(gui: &Gui, node: NodeId, default: [f32; 3]) -> [f32; 3] {
    match gui.get(node, Prop::Color).ok().flatten() {
        Some(Value::Rgba(c)) => [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0],
        _ => default,
    }
}

fn text(state: &AppState, value: &str, rect: RectPx, align: ShellAlign, scale: ShellScale, rgb: [f32; 3]) -> ShellTextDraw {
    text_at_depth(state, value, rect, align, scale, rgb, SHELL_CONTROL_TEXT_DEPTH)
}

fn text_at_depth(
    state: &AppState,
    value: &str,
    rect: RectPx,
    align: ShellAlign,
    scale: ShellScale,
    rgb: [f32; 3],
    depth: f32,
) -> ShellTextDraw {
    shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        value,
        rect_to_text_rect(rect),
        rgb,
        align,
        [0.0; 2],
        depth,
        scale.factor(),
    )
}

fn items(gui: &Gui, node: NodeId) -> &[String] {
    match gui.get(node, Prop::Items).ok().flatten() {
        Some(Value::Items(items)) => items,
        _ => &[],
    }
}

/// The widgets under `root`, in tree order. Invisible subtrees are skipped.
pub(crate) fn paint_gui_children(state: &AppState, gui: &Gui, root: NodeId, scale: ShellScale) -> GuiPaint {
    paint_gui_children_under(state, gui, root, scale, &[])
}

/// [`paint_gui_children`] under `overlays` that paint with the widgets'
/// sprites but before their text (an open list in a box): the captions of
/// the nodes an overlay meets are left out, so they don't show through it.
pub(crate) fn paint_gui_children_under(
    state: &AppState,
    gui: &Gui,
    root: NodeId,
    scale: ShellScale,
    overlays: &[RectPx],
) -> GuiPaint {
    let mut paint = GuiPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let chrome = atlas.control_chrome();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !flag(gui, node, Prop::Visible, true) {
            continue;
        }
        if let Ok(children) = gui.tree().children(node) {
            stack.extend(children.iter().rev());
        }
        let Ok(kind) = gui.tree().kind(node) else { continue };
        let Ok(rect) = gui.tree().screen_rect(node) else { continue };
        let covered = super::text_covered_by_overlay(rect, overlays);
        let enabled = flag(gui, node, Prop::Enabled, true);
        let align = ShellAlign(int(gui, node, Prop::Align, ShellAlign::V_CENTER.0 as i32) as u8);
        match kind {
            NodeKind::Checkbox => {
                let checked = flag(gui, node, Prop::Checked, false);
                paint_control(&mut paint.controls, &chrome, ControlPaint::Checkbox { rect, checked }, scale);
                if let Some(label) = caption(state, gui, node).filter(|_| !covered) {
                    let at = crate::ui::skirmish_shell::checkbox_text_rect(rect, scale);
                    paint.texts.push(text(state, &label, at, align, scale, rgb(gui, node, SHELL_LABEL_TEXT_RGB)));
                }
            }
            NodeKind::Trackbar => {
                // The plaque and its value text unless the owner turned them
                // off (`0x4AC` with 0), which gives the track the whole window.
                let plaque = flag(gui, node, Prop::ShowValue, true);
                let range = TrackbarRange::new(int(gui, node, Prop::Min, 0), int(gui, node, Prop::Max, 100), int(gui, node, Prop::Step, 1))
                    .with_plaque(plaque);
                let value = int(gui, node, Prop::Pos, range.min);
                paint_control(
                    &mut paint.controls,
                    &chrome,
                    ControlPaint::Trackbar { rect, thumb_left: range.thumb_left_in(value, rect, scale), plaque, disabled: !enabled },
                    scale,
                );
                if plaque && !covered {
                    // A disabled trackbar's value text takes [0xAC1CB4] (`0x0061E2B1`).
                    let color = if enabled { SHELL_LABEL_TEXT_RGB } else { SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F };
                    paint.texts.push(text(
                        state,
                        &range.stepped(value).to_string(),
                        value_text_rect(rect, scale),
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        scale,
                        color,
                    ));
                }
            }
            NodeKind::Combo => {
                let drop_down = gui.combo_drop_down(node);
                paint_control(
                    &mut paint.controls,
                    &chrome,
                    ControlPaint::Combo { rect, swatch: None, open: drop_down.is_some(), disabled: !enabled },
                    scale,
                );
                let items = items(gui, node);
                let selected = usize::try_from(int(gui, node, Prop::Selected, -1)).ok();
                if let Some(face) = selected.and_then(|row| items.get(row)) {
                    paint.texts.push(text(
                        state,
                        face,
                        crate::ui::skirmish_shell::combo_text_rect(rect, scale),
                        ShellAlign::V_CENTER,
                        scale,
                        combo_face_text_color(!enabled),
                    ));
                }
                let Some(drop_down) = drop_down else { continue };
                let popup = drop_down.popup;
                push_popup_instances(
                    &mut paint.popup,
                    atlas,
                    popup.rect,
                    drop_down.hot_row.and_then(|row| popup.row_rect(row)),
                    [],
                    popup.scrollbar().zip(popup.thumb()),
                    drop_down.pressed_part.map(dropdown_scroll_part),
                    scale,
                    SHELL_DROPDOWN_DEPTH - 0.00002,
                );
                for row in popup.first..popup.first + popup.visible_rows {
                    let (Some(at), Some(item)) = (popup.row_rect(row), items.get(row)) else { continue };
                    paint.popup_texts.push(text_at_depth(
                        state,
                        item,
                        scale.derive_rect(at, |r| RectPx::new(r.x + 3, r.y, r.w - 6, r.h)),
                        ShellAlign::V_CENTER,
                        scale,
                        SHELL_LABEL_TEXT_RGB,
                        SHELL_DROPDOWN_TEXT_DEPTH,
                    ));
                }
            }
            NodeKind::Text => {
                if let Some(label) = caption(state, gui, node).filter(|_| !covered) {
                    paint.texts.push(text(state, &label, rect, align, scale, rgb(gui, node, SHELL_LABEL_TEXT_RGB)));
                }
            }
            _ => {}
        }
    }
    paint
}
