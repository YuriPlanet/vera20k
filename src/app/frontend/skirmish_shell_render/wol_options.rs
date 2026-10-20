//! The left-side children of WOL Options `0x2BC` (nine auto-checkboxes on
//! `OwnerDraw_CheckBox 0x006163A0`) and of the shell's Sound Options `0xD6`
//! (three captions and trackbars, as B8 paints them). The WOL page
//! compositor draws them over its own backdrop and chrome.

use super::controls::{ControlPaint, paint_control};
use super::text::rect_to_text_rect;
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::sound::{SHELL_SOUND_LABEL_KEYS, ShellSoundLayout, SoundSlider, SoundState};
use crate::ui::wol_options::{CHECKBOXES, WolOptionsLayout, WolOptionsState};

/// Sprites (skirmish atlas) and texts of one page's children.
#[derive(Default)]
pub(crate) struct WolOptionsPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
}

fn label(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned())
}

fn text(
    state: &AppState,
    value: &str,
    rect: RectPx,
    align: ShellAlign,
    scale: ShellScale,
) -> ShellTextDraw {
    shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        value,
        rect_to_text_rect(rect),
        SHELL_LABEL_TEXT_RGB,
        align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor(),
    )
}

pub(crate) fn paint_wol_options_children(
    state: &AppState,
    options: &WolOptionsState,
    layout: &WolOptionsLayout,
) -> WolOptionsPaint {
    let mut paint = WolOptionsPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let scale = layout.page.scale;
    let chrome = atlas.control_chrome();
    for (index, (checkbox, rect)) in CHECKBOXES.iter().zip(layout.checkboxes).enumerate() {
        paint_control(
            &mut paint.controls,
            &chrome,
            ControlPaint::Checkbox {
                rect,
                checked: options.checked[index],
            },
            scale,
        );
        paint.texts.push(text(
            state,
            &label(state, checkbox.csf_key),
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            scale,
        ));
    }
    paint
}

pub(crate) fn paint_shell_sound_children(
    state: &AppState,
    dialog: &SoundState,
    layout: &ShellSoundLayout,
) -> WolOptionsPaint {
    let mut paint = WolOptionsPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let scale = layout.page.scale;
    let chrome = atlas.control_chrome();
    for id in SoundSlider::ALL {
        let index = id as usize;
        let rect = layout.sliders[index];
        paint_control(
            &mut paint.controls,
            &chrome,
            ControlPaint::Trackbar {
                rect,
                thumb_left: crate::ui::shell::trackbar::thumb_left_at(
                    i32::from(dialog.positions[index]),
                    rect,
                    50,
                    10,
                    scale,
                ),
                plaque: true,
            },
            scale,
        );
        paint.texts.push(text(
            state,
            &label(state, SHELL_SOUND_LABEL_KEYS[index]),
            layout.labels[index],
            ShellAlign::H_RIGHT,
            scale,
        ));
        paint.texts.push(text(
            state,
            &dialog.positions[index].to_string(),
            crate::ui::shell::trackbar::value_text_rect(rect, scale),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            scale,
        ));
    }
    paint
}
