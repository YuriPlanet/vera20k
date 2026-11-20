//! The left-side children of WOL Options `0x2BC` (nine auto-checkboxes on
//! `OwnerDraw_CheckBox 0x006163A0`, and VERA's chat translation list and
//! Translate in Matches box on the online lobby's page) and of the shell's
//! Sound Options `0xD6` (three captions and trackbars, as B8 paints them):
//! the widgets each page built under its dialog, through the generic painter.
//! The WOL page compositor draws them over its own backdrop and chrome.

use crate::app::AppState;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::ShellScale;

/// Sprites (skirmish atlas) and texts of one page's children, and of an open
/// list over them.
pub(crate) use super::gui_paint::GuiPaint as WolOptionsPaint;

pub(crate) fn paint_wol_options_children(
    state: &AppState,
    dialog: DialogId,
    scale: ShellScale,
) -> WolOptionsPaint {
    let controller = &state.frontend.shell_controller;
    match controller.root_in(dialog) {
        Some(root) => super::gui_paint::paint_gui_children(state, controller.gui(), root, scale),
        None => WolOptionsPaint::default(),
    }
}
