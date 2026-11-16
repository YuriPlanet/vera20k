//! VERA's Game Options box over the skirmish page
//! (`ui::skirmish_shell::game_options`), drawn as the room draws its own
//! (`multiplayer_setup.rs`): the online lobby's box art with its type-3 OK,
//! then the Superweapon Limit slider with its plaque and the Terrain
//! Deformation box.

use super::chrome::type3_button_frames;
use super::controls::{ControlPaint, paint_control};
use super::text::{button_label_color_for_disabled, localized_label, rect_to_text_rect};
use super::wol_lobby::POPUP_DEPTHS;
use super::{SHELL_LABEL_TEXT_RGB, SkirmishShellChromeAtlas};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::RectPx;
use crate::ui::skirmish_shell::checkbox_text_rect;
use crate::ui::skirmish_shell::game_options::{
    GameOptionsBox, GameOptionsLayout, SUPERWEAPON_LIMIT_LABEL, SUPERWEAPON_LIMIT_RANGE,
    TERRAIN_DEFORMATION_LABEL, superweapon_limit_text,
};

/// The open box's art and controls, then its text, over the page.
pub(super) fn push_game_options_box(
    sprites: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    layout: &GameOptionsLayout,
    open: &GameOptionsBox,
) {
    let scale = layout.scale;
    let font = &state.renderer.bit_font;
    let ok = PaintLabel {
        text: localized_label(state, "GUI:OK", "OK").into(),
        rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            layout.ok,
            open.ok_pressed,
        ),
        align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        rgb: button_label_color_for_disabled(false),
        path_a_reveal: None,
    };
    let draw = shell_paint::paint_modal_shp(
        font,
        atlas
            .ra2_dialog_background
            .or_else(|| atlas.mode_two_dialog_background(None)),
        type3_button_frames(atlas),
        layout.dialog,
        &[ModalButton {
            rect: layout.ok,
            pressed: open.ok_pressed,
            enabled: true,
        }],
        &[ok],
        scale,
        POPUP_DEPTHS,
    );
    sprites.extend(draw.sprites);
    texts.extend(draw.text);

    let text = |value: &str, rect: RectPx, align: ShellAlign| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            [0.0; 2],
            POPUP_DEPTHS.text,
            scale.factor(),
        )
    };
    let chrome = atlas.control_chrome();
    let bar = layout.superweapon_limit;
    paint_control(
        sprites,
        &chrome,
        ControlPaint::Trackbar {
            rect: bar,
            thumb_left: SUPERWEAPON_LIMIT_RANGE.thumb_left_in(
                open.superweapon_limit_value(),
                bar,
                scale,
            ),
            plaque: true,
            disabled: false,
        },
        scale,
    );
    paint_control(
        sprites,
        &chrome,
        ControlPaint::Checkbox {
            rect: layout.terrain_deformation,
            checked: open.rules.terrain_deformation,
        },
        scale,
    );
    texts.push(text(
        &crate::i18n::tr(SUPERWEAPON_LIMIT_LABEL),
        layout.superweapon_limit_label,
        ShellAlign::NONE,
    ));
    texts.push(text(
        &superweapon_limit_text(open.rules.superweapon_limit),
        crate::ui::shell::trackbar::value_text_rect(bar, scale),
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    ));
    texts.push(text(
        &crate::i18n::tr(TERRAIN_DEFORMATION_LABEL),
        checkbox_text_rect(layout.terrain_deformation, scale),
        ShellAlign::V_CENTER,
    ));
}
