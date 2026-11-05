//! VERA: the enlarged map preview. A press on the preview box of the
//! Skirmish, Choose Map, LAN or online setup screen shows the box's picture
//! large over the whole screen, with its start markers; a click or Escape
//! closes it. gamemd's preview box takes no clicks. While the enlarged
//! preview shows it takes every key and click, so nothing under it acts.
//!
//! It shows the box's own picture (`skirmish_shell_render::
//! SkirmishPreviewTexture`): VERA's picture of the map when one is kept,
//! which the menus draw up to eight times the box's size
//! (`map_picture::canvas::MAX_PICTURE`), else the map's own preview.

use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::app::AppState;
use crate::ui::skirmish_shell::RectPx;

/// The hint the preview box's help ends with while a press would enlarge it.
const HINT: &str = "Click to enlarge.";

/// The enlarged preview, while it shows.
#[derive(Debug, Default)]
pub(crate) struct PreviewZoom {
    /// A press made while it shows, whose release closes it. The press that
    /// opened it is not one.
    closing_press: bool,
}

/// Whether a press on the preview box would enlarge it: the box shows a
/// picture.
fn available(state: &AppState) -> bool {
    state.frontend.skirmish_preview_texture.is_some()
}

/// Enlarge the preview when a press lands on `preview_box` while it shows a
/// picture. True when it did, as the press is then the preview's.
pub(crate) fn press_on_box(state: &mut AppState, preview_box: RectPx) -> bool {
    let (x, y) = state.match_state.input.cursor_px();
    if !preview_box.contains(x, y) || !available(state) {
        return false;
    }
    state.frontend.preview_zoom = Some(PreviewZoom::default());
    state.platform.window.request_redraw();
    true
}

/// The preview box's `help` with the hint that a press enlarges it.
pub(crate) fn box_help(state: &AppState, help: String) -> String {
    if !available(state) {
        return help;
    }
    if help.is_empty() {
        HINT.to_owned()
    } else {
        format!("{help} {HINT}")
    }
}

pub(crate) fn close(state: &mut AppState) {
    if state.frontend.preview_zoom.take().is_some() {
        state.platform.window.request_redraw();
    }
}

impl PreviewZoom {
    /// A mouse button's press or release; true when it closes the preview.
    fn button(&mut self, pressed: bool) -> bool {
        if pressed {
            self.closing_press = true;
        }
        !pressed && self.closing_press
    }
}

/// A mouse button while the enlarged preview shows: a press and then its
/// release close it. True while it shows, as it takes every button.
pub(crate) fn mouse(state: &mut AppState, pressed: bool) -> bool {
    let Some(zoom) = state.frontend.preview_zoom.as_mut() else {
        return false;
    };
    if zoom.button(pressed) {
        close(state);
    }
    true
}

/// A key while the enlarged preview shows: Escape, Enter or Space closes it.
/// True while it shows, as it takes every key.
pub(crate) fn key(state: &mut AppState, event: &KeyEvent) -> bool {
    if state.frontend.preview_zoom.is_none() {
        return false;
    }
    if event.state.is_pressed()
        && !event.repeat
        && matches!(
            event.physical_key,
            PhysicalKey::Code(
                KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space
            )
        )
    {
        close(state);
    }
    true
}

pub(crate) fn is_open(state: &AppState) -> bool {
    state.frontend.preview_zoom.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opening_press_releases_without_closing_and_the_next_click_closes() {
        let mut zoom = PreviewZoom::default();
        assert!(!zoom.button(false), "the release of the press that opened it");
        assert!(!zoom.button(true));
        assert!(zoom.button(false));
    }
}
