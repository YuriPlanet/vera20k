//! VERA: the enlarged map preview. A press on the preview box of the
//! Skirmish, Choose Map, LAN or online setup screen shows the box's picture
//! large over the whole screen, with its start markers and the map's
//! briefing over its middle when it has one (`map::basic::briefing_text`); a
//! click or Escape closes it. gamemd's preview box takes no clicks. While the
//! enlarged preview shows it takes every key and click, so nothing under it
//! acts: it is a modal overlay on the shell controller ([`DIALOG`]) whose one
//! button covers the screen, the dialog's default and cancel button, so a
//! press there and its release, Enter or Escape close it. The briefing hides while the pointer is over it, so the map under it
//! shows.
//!
//! It shows the box's own picture (`skirmish_shell_render::
//! SkirmishPreviewTexture`): VERA's picture of the map when one is kept,
//! which the menus draw up to eight times the box's size
//! (`map_picture::canvas::MAX_PICTURE`), else the map's own preview.

use vera20k_gui::{InputEvent, Key, MouseButton, NodeKind, NodeSpec, Prop, Value};
use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::app::frontend::skirmish_shell_render::SkirmishPreviewTexture;
use crate::app::frontend::state::FrontendState;
use crate::app::input::state::MatchInputState;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::skirmish_shell::RectPx;

/// The enlarged preview's dialog: VERA's own id.
const DIALOG: DialogResourceId = DialogResourceId(0x7600);
/// Its button, the whole screen and beyond, which the pointer never leaves.
const CLOSE: u16 = 1;
/// The briefing's box, which hovers (the briefing then hides) but lets a
/// press through to the button. Its rect is the paint's.
const BRIEFING: u16 = 2;

/// How far, in window pixels, the pointer may drift from the press that
/// enlarged the preview and still count as where it was.
const STILL: u32 = 3;

/// The enlarged preview, while it shows.
#[derive(Debug, Default)]
pub(crate) struct PreviewZoom {
    /// Where the press that opened it was, until the pointer moves away.
    opened_at: Option<(i32, i32)>,
}

/// Whether a press on the preview box would enlarge it: the box shows a
/// picture.
fn available(skirmish_preview_texture: &Option<SkirmishPreviewTexture>) -> bool {
    skirmish_preview_texture.is_some()
}

/// Whether the pointer is on `dialog`'s preview static `0x468`, as its
/// widgets hit-test it.
pub(crate) fn over_box(
    shell_controller: &DialogController,
    input: &MatchInputState,
    dialog: crate::ui::shell::descriptor::DialogResourceId,
) -> bool {
    let (x, y) = input.cursor_px();
    let controller = shell_controller;
    let gui = controller.gui();
    let node = controller
        .root_in(dialog)
        .and_then(|root| gui.hit_at(root, x, y, true));
    node.and_then(|node| gui.id_of(node)) == Some(0x468)
}

/// Enlarge the preview when a press lands on the box (`over`) while it
/// shows a picture. True when it did, as the press is then the preview's.
pub(crate) fn press_on_box(
    frontend: &mut FrontendState,
    input: &MatchInputState,
    over: bool,
) -> bool {
    let (x, y) = input.cursor_px();
    if !over || !available(&frontend.skirmish_preview_texture) {
        return false;
    }
    frontend.preview_zoom = Some(PreviewZoom {
        opened_at: Some((x, y)),
    });
    // The press that opened it was the page's: its release finds nothing
    // held here and closes nothing.
    let controller = &mut frontend.shell_controller;
    controller.close(DIALOG);
    controller.push_modal_overlay(DIALOG);
    if let Some(root) = controller.root_in(DIALOG) {
        let everywhere =
            vera20k_gui::Rect::new(i32::MIN / 4, i32::MIN / 4, i32::MAX / 2, i32::MAX / 2);
        let briefing = NodeSpec::new(NodeKind::Text)
            .id(BRIEFING.into())
            .tooltip("")
            .with(Prop::PressThrough, Value::Bool(true));
        let spec = NodeSpec::new(NodeKind::Dialog).child(briefing).child(
            NodeSpec::new(NodeKind::Button)
                .id(CLOSE.into())
                .with(Prop::Rect, Value::Rect(everywhere))
                .with(Prop::Default, Value::Bool(true))
                .with(Prop::Cancel, Value::Bool(true)),
        );
        crate::ui::shell::page::ensure_built(controller.gui_mut(), root, || spec);
    }
    true
}

/// The preview box's `help`, ending with the hint that a press enlarges it.
pub(crate) fn box_help(
    skirmish_preview_texture: &Option<SkirmishPreviewTexture>,
    help: String,
) -> String {
    if !available(skirmish_preview_texture) {
        return help;
    }
    if help.is_empty() {
        crate::i18n::tr("VERA:ClickToEnlarge")
    } else {
        crate::i18n::tr_fill("VERA:HelpClickToEnlarge", &[("help", &help)])
    }
}

pub(crate) fn close(window: &winit::window::Window, frontend: &mut FrontendState) {
    if frontend.preview_zoom.take().is_some() {
        frontend.shell_controller.close(DIALOG);
        window.request_redraw();
    }
}

impl PreviewZoom {
    /// Whether the briefing shows with the pointer at (`x`, `y`): not while
    /// the pointer is `over` its box, unless it has not moved since the
    /// press that opened the preview, which may land on it.
    fn briefing_shown(&mut self, over: bool, (x, y): (i32, i32)) -> bool {
        if self
            .opened_at
            .is_some_and(|(at_x, at_y)| at_x.abs_diff(x).max(at_y.abs_diff(y)) > STILL)
        {
            self.opened_at = None;
        }
        self.opened_at.is_some() || !over
    }
}

/// Whether the map's briefing shows in its box `frame` (in window pixels)
/// over the enlarged preview, where the pointer is now.
pub(crate) fn shows_briefing(
    frontend: &mut FrontendState,
    input: &MatchInputState,
    frame: RectPx,
) -> bool {
    let pointer = input.cursor_px();
    let controller = &mut frontend.shell_controller;
    controller.set_prop(DIALOG, BRIEFING, Prop::Rect, Value::Rect(frame));
    let over = controller.hovered_node_in(DIALOG).is_some()
        && controller.hovered_node_in(DIALOG) == controller.control(DIALOG, BRIEFING);
    frontend
        .preview_zoom
        .as_mut()
        .is_some_and(|zoom| zoom.briefing_shown(over, pointer))
}

/// One input for the enlarged preview's dialog; true while it shows, as it
/// takes every input. Its button closes it on activation.
fn input(
    window: &winit::window::Window,
    frontend: &mut FrontendState,
    event: Option<InputEvent>,
) -> bool {
    if frontend.preview_zoom.is_none() {
        return false;
    }
    let controller = &mut frontend.shell_controller;
    let Some(event) = event.filter(|_| controller.root_in(DIALOG).is_some()) else {
        return true;
    };
    controller.on_input(event);
    let closes = controller
        .drain_events()
        .iter()
        .any(|event| matches!(event, vera20k_gui::UiEvent::Activated { .. }));
    if closes {
        close(window, frontend);
    }
    true
}

/// The pointer while the enlarged preview shows: a move (`None`) or any
/// button's press or release, every button its one button's press. A press
/// and then its release close it. True while it shows.
pub(crate) fn pointer(
    window: &winit::window::Window,
    frontend: &mut FrontendState,
    match_input: &MatchInputState,
    pressed: Option<bool>,
) -> bool {
    let (x, y) = match_input.cursor_px();
    let button = MouseButton::Left;
    let event = match pressed {
        None => InputEvent::PointerMove { x, y },
        Some(true) => InputEvent::PointerDown { x, y, button },
        Some(false) => InputEvent::PointerUp { x, y, button },
    };
    input(window, frontend, Some(event))
}

/// A key while the enlarged preview shows: Enter (and Space, which clicks
/// its button) its default, Escape its cancel; both close it. True while it
/// shows, as it takes every key.
pub(crate) fn key(
    window: &winit::window::Window,
    frontend: &mut FrontendState,
    event: &KeyEvent,
) -> bool {
    let key = match event.physical_key {
        _ if !event.state.is_pressed() || event.repeat => None,
        PhysicalKey::Code(KeyCode::Escape) => Some(Key::Escape),
        PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space) => {
            Some(Key::Enter)
        }
        _ => None,
    };
    input(window, frontend, key.map(InputEvent::Key))
}

/// The wheel while the enlarged preview shows, which nothing takes. True
/// while it shows.
pub(crate) fn wheel(window: &winit::window::Window, frontend: &mut FrontendState) -> bool {
    input(window, frontend, None)
}

pub(crate) fn is_open(
    preview_zoom: &Option<crate::app::frontend::preview_zoom::PreviewZoom>,
) -> bool {
    preview_zoom.is_some()
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;

    #[test]
    fn the_briefing_hides_while_the_pointer_is_over_its_box_once_it_has_moved() {
        let mut zoom = PreviewZoom {
            opened_at: Some((50, 50)),
        };
        assert!(zoom.briefing_shown(true, (52, 50)), "still at the press");
        assert!(!zoom.briefing_shown(true, (60, 50)), "moved over the box");
        assert!(!zoom.briefing_shown(true, (50, 50)), "back at the press");
        assert!(zoom.briefing_shown(false, (150, 50)), "off the box");
    }
}
