//! VERA's Game Options box over the skirmish page
//! (`ui::skirmish_shell::game_options`), drawn as the room draws its own
//! (`multiplayer_setup.rs`): the online lobby's box art with its type-3 OK,
//! then its sliders and checkboxes.

use super::SkirmishShellChromeAtlas;
use super::chrome::type3_button_frames;
use super::text::button_label_color_for_disabled;
use super::wol_lobby::POPUP_DEPTHS;
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::ShellTextDraw;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::skirmish_shell::game_options::{GameOptionsControl, GameOptionsLayout, sync};

/// The open box's art and controls, then its text, over the page: the
/// box's widgets ([`crate::ui::skirmish_shell::game_options::sync`]) through
/// the shared painter.
pub(super) fn push_game_options_box(
    sprites: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    state: &mut AppState,
    atlas: &SkirmishShellChromeAtlas,
    layout: &GameOptionsLayout,
) {
    let Some(open) = state.frontend.skirmish_shell_state.game_options else {
        return;
    };
    let Some(root) = sync(&mut state.frontend.shell_controller, &open, layout) else {
        return;
    };
    let gui = state.frontend.shell_controller.gui();
    let ok_pressed = gui
        .find_id(root, GameOptionsControl::Ok.id())
        .is_some_and(|ok| gui.pressed() == Some(ok));
    let scale = layout.scale;
    let ok = PaintLabel {
        text: state.csf_label("GUI:OK", "OK").into(),
        rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(layout.ok, ok_pressed),
        align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        rgb: button_label_color_for_disabled(false),
        path_a_reveal: None,
    };
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas
            .ra2_dialog_background
            .or_else(|| atlas.mode_two_dialog_background(None)),
        type3_button_frames(atlas),
        layout.dialog,
        &[ModalButton {
            rect: layout.ok,
            pressed: ok_pressed,
            enabled: true,
        }],
        &[ok],
        scale,
        POPUP_DEPTHS,
    );
    sprites.extend(draw.sprites);
    texts.extend(draw.text);
    let widgets =
        super::gui_paint::paint_gui_with(state, atlas, gui, root, scale, &[], Default::default());
    sprites.extend(widgets.controls);
    texts.extend(widgets.texts);
}
