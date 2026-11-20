//! Paints a dialog's `vera20k-gui` widgets with the shell's owner-draw
//! emitters: one place that turns Checkbox, Trackbar and Text nodes into the
//! sprites and texts each screen used to assemble by hand. A screen keeps
//! its backdrop and chrome and calls [`paint_gui_children`] for the widgets
//! it built under its dialog root.

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use super::controls::{ControlPaint, paint_control};
use super::text::rect_to_text_rect;
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::trackbar::{TrackbarRange, value_text_rect};

/// Sprites (skirmish atlas) and texts of a dialog's widgets.
#[derive(Default)]
pub(crate) struct GuiPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
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
    shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        value,
        rect_to_text_rect(rect),
        rgb,
        align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor(),
    )
}

/// The widgets under `root`, in tree order. Invisible subtrees are skipped.
pub(crate) fn paint_gui_children(state: &AppState, gui: &Gui, root: NodeId, scale: ShellScale) -> GuiPaint {
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
        let enabled = flag(gui, node, Prop::Enabled, true);
        let align = ShellAlign(int(gui, node, Prop::Align, ShellAlign::V_CENTER.0 as i32) as u8);
        match kind {
            NodeKind::Checkbox => {
                let checked = flag(gui, node, Prop::Checked, false);
                paint_control(&mut paint.controls, &chrome, ControlPaint::Checkbox { rect, checked }, scale);
                if let Some(label) = caption(state, gui, node) {
                    let at = crate::ui::skirmish_shell::checkbox_text_rect(rect, scale);
                    paint.texts.push(text(state, &label, at, align, scale, rgb(gui, node, SHELL_LABEL_TEXT_RGB)));
                }
            }
            NodeKind::Trackbar => {
                let range = TrackbarRange::new(int(gui, node, Prop::Min, 0), int(gui, node, Prop::Max, 100), int(gui, node, Prop::Step, 1));
                let value = int(gui, node, Prop::Pos, range.min);
                // The plaque and its value text unless the owner turned them
                // off (`0x4AC` with 0).
                let plaque = flag(gui, node, Prop::ShowValue, true);
                paint_control(
                    &mut paint.controls,
                    &chrome,
                    ControlPaint::Trackbar { rect, thumb_left: range.thumb_left_in(value, rect, scale), plaque, disabled: !enabled },
                    scale,
                );
                if plaque {
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
            NodeKind::Text => {
                if let Some(label) = caption(state, gui, node) {
                    paint.texts.push(text(state, &label, rect, align, scale, rgb(gui, node, SHELL_LABEL_TEXT_RGB)));
                }
            }
            _ => {}
        }
    }
    paint
}
