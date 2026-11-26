//! VERA's Game Options box over the skirmish page
//! (`ui::skirmish_shell::game_options`), drawn as the room draws its own
//! (`multiplayer_setup.rs`): the online lobby's box art, its type-3 OK,
//! then its sliders and checkboxes.

use super::SkirmishShellChromeAtlas;
use super::chrome::type3_button_frames;
use super::gui_paint::{ButtonArt, GuiStyle, paint_gui_with};
use super::text::button_label_color_for_disabled;
use super::wol_lobby::POPUP_DEPTHS;
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::Material;
use crate::render::shell_paint;
use crate::render::shell_text::ShellTextDraw;
use crate::ui::skirmish_shell::game_options::{GameOptionsControl, GameOptionsLayout, sync};

/// The open box's art and controls, then its text, over the page: the
/// box's widgets ([`crate::ui::skirmish_shell::game_options::sync`]) through
/// the shared painter, its OK first.
pub(super) fn push_game_options_box(
    sprites: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &SkirmishShellChromeAtlas,
    layout: &GameOptionsLayout,
) {
    let Some(open) = frontend.skirmish_shell_state.game_options else {
        return;
    };
    let Some(root) = sync(&mut frontend.shell_controller, &open, layout) else {
        return;
    };
    let gui = frontend.shell_controller.gui();
    let ok_id = GameOptionsControl::Ok.id();
    let ok = gui.find_id(root, ok_id);
    // OK shows pushed while its press holds it.
    let ok_pressed = ok.is_some_and(|ok| gui.pressed() == Some(ok));
    let scale = layout.scale;
    sprites.extend(shell_paint::paint_modal_sprites(
        atlas
            .ra2_dialog_background
            .or_else(|| atlas.mode_two_dialog_background(None)),
        type3_button_frames(atlas),
        layout.dialog,
        &[],
        scale,
        POPUP_DEPTHS,
    ));
    let text_of = |id: i32| (id == ok_id).then(|| process_assets.csf_label("GUI:OK", "OK"));
    let pressed_of = |id: i32| (id == ok_id).then_some(ok_pressed);
    let ok_style = GuiStyle {
        buttons: ButtonArt::Modal,
        text_of: Some(&text_of),
        pressed_of: Some(&pressed_of),
        rgb: button_label_color_for_disabled(false),
        ..Default::default()
    };
    if let Some(ok) = ok {
        let ok = paint_gui_with(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            atlas,
            gui,
            ok,
            scale,
            &[],
            ok_style,
        );
        sprites.extend(ok.lane(Material::Controls).copied());
        texts.extend(ok.texts);
    }
    let widgets = paint_gui_with(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        gui,
        root,
        scale,
        &[],
        Default::default(),
    );
    sprites.extend(widgets.lane(Material::Controls).copied());
    texts.extend(widgets.texts);
}
