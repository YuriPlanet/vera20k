//! The left-side children of WOL Options `0x2BC` (nine auto-checkboxes on
//! `OwnerDraw_CheckBox 0x006163A0`, and VERA's chat translation list and
//! Translate in Matches box on the online lobby's page) and of the shell's Sound Options `0xD6` (three
//! captions and trackbars, as B8 paints them). The WOL page compositor draws
//! them over its own backdrop and chrome.

use super::controls::{ControlPaint, paint_control, push_popup_instances};
use super::text::{combo_face_text_color, rect_to_text_rect};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F, SHELL_DROPDOWN_DEPTH,
    SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::sound::{SHELL_SOUND_LABEL_KEYS, ShellSoundLayout, SoundSlider, SoundState};
use crate::ui::wol_options::{
    CHECKBOXES, TRANSLATE_CAPTION_KEY, TRANSLATE_MATCHES_KEY, TRANSLATE_OFF_KEY, WolOptionsLayout,
    WolOptionsState,
};

/// Sprites (skirmish atlas) and texts of one page's children, and of an open
/// list over them.
#[derive(Default)]
pub(crate) struct WolOptionsPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
    pub(crate) popup: Vec<SpriteInstance>,
    pub(crate) popup_texts: Vec<ShellTextDraw>,
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
    colored_text(state, value, rect, align, scale, SHELL_LABEL_TEXT_RGB)
}

fn colored_text(
    state: &AppState,
    value: &str,
    rect: RectPx,
    align: ShellAlign,
    scale: ShellScale,
    rgb: [f32; 3],
) -> ShellTextDraw {
    text_at_depth(
        state,
        value,
        rect,
        scale,
        rgb,
        align,
        SHELL_CONTROL_TEXT_DEPTH,
    )
}

fn text_at_depth(
    state: &AppState,
    value: &str,
    rect: RectPx,
    scale: ShellScale,
    rgb: [f32; 3],
    align: ShellAlign,
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
    let checkbox = |paint: &mut WolOptionsPaint, rect: RectPx, checked: bool, key: &str| {
        paint_control(
            &mut paint.controls,
            &chrome,
            ControlPaint::Checkbox { rect, checked },
            scale,
        );
        paint.texts.push(text(
            state,
            &label(state, key),
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            scale,
        ));
    };
    for (index, (spec, rect)) in CHECKBOXES.iter().zip(layout.checkboxes).enumerate() {
        if let Some(rect) = rect {
            checkbox(&mut paint, rect, options.checked[index], spec.csf_key);
        }
    }
    let Some(translate) = layout.translate else {
        return paint;
    };
    checkbox(
        &mut paint,
        translate.in_matches,
        options.translate.in_matches,
        TRANSLATE_MATCHES_KEY,
    );
    let (caption, list) = (translate.caption, translate.list);
    paint.texts.push(text(
        state,
        &label(state, TRANSLATE_CAPTION_KEY),
        caption,
        ShellAlign::V_CENTER,
        scale,
    ));
    let popup = options.translate_popup(layout);
    paint_control(
        &mut paint.controls,
        &chrome,
        ControlPaint::Combo {
            rect: list,
            swatch: None,
            open: popup.is_some(),
            disabled: false,
        },
        scale,
    );
    let off = label(state, TRANSLATE_OFF_KEY);
    paint.texts.push(colored_text(
        state,
        &WolOptionsState::translate_row_text(options.translate_row(), &off),
        crate::ui::skirmish_shell::combo_text_rect(list, scale),
        ShellAlign::V_CENTER,
        scale,
        combo_face_text_color(false),
    ));
    let Some(popup) = popup else {
        return paint;
    };
    push_popup_instances(
        &mut paint.popup,
        atlas,
        popup.rect,
        options
            .translate_list
            .hot_row
            .and_then(|row| popup.row_rect(row)),
        [],
        popup.scrollbar().zip(popup.thumb()),
        None,
        scale,
        SHELL_DROPDOWN_DEPTH - 0.00002,
    );
    for row in popup.first..popup.first + popup.visible_rows {
        let Some(rect) = popup.row_rect(row) else {
            continue;
        };
        paint.popup_texts.push(text_at_depth(
            state,
            &WolOptionsState::translate_row_text(row, &off),
            scale.derive_rect(rect, |rect| {
                RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
            }),
            scale,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH,
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
                disabled: !dialog.sliders_enabled,
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
        // A disabled trackbar's value text takes [0xAC1CB4] (`0x0061E2B1`).
        paint.texts.push(colored_text(
            state,
            &dialog.positions[index].to_string(),
            crate::ui::shell::trackbar::value_text_rect(rect, scale),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            scale,
            if dialog.sliders_enabled {
                SHELL_LABEL_TEXT_RGB
            } else {
                SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
            },
        ));
    }
    paint
}
