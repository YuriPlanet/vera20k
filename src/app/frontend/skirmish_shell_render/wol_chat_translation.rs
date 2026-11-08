//! The Chat Translation page's left side on the shared control owners: the
//! captions, the language list, the checkboxes and the Preview panel. The WOL
//! page compositor draws them over its own backdrop and chrome.

use super::controls::{ControlPaint, paint_control, push_popup_instances};
use super::text::{combo_face_text_color, rect_to_text_rect};
use super::wol_login::{WolLoginPaint, push_info_panel, wol_button_label_rgb};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::chat_translation::{ChatLanguage, ChatPlace};
use crate::ui::shell::button::CheckboxFace;
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_chat_translation::*;

/// A preview row's place tag.
const PREVIEW_TAG_RGB: [f32; 3] = [0.498, 0.643, 0.722];

pub(crate) fn paint_wol_chat_translation_children(
    state: &AppState,
    page: &WolChatTranslationPage,
    layout: &WolChatTranslationLayout,
) -> WolLoginPaint {
    let mut paint = WolLoginPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let on = page.settings.language.is_some();
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            depth,
            scale.factor(),
        )
    };
    let label = |key: &str| {
        state
            .process_assets
            .csf
            .as_ref()
            .map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned())
    };
    for (key, rect) in [
        (INTO_KEY, layout.into_caption),
        (PLACES_KEY, layout.places_caption),
        (LINES_KEY, layout.lines_caption),
        (READS_KEY, layout.reads_caption),
        (PREVIEW_KEY, layout.preview_caption),
        (PRIVACY_KEY, layout.privacy),
    ] {
        paint.texts.push(text(
            &label(key),
            rect,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    let open = page.popup.control.is_some();
    paint_control(
        &mut paint.controls,
        &atlas.control_chrome(),
        ControlPaint::Combo {
            rect: layout.language,
            swatch: None,
            open,
            disabled: false,
        },
        scale,
    );
    let off = label(OFF_KEY);
    paint.texts.push(text(
        &WolChatTranslationPage::row_text(page.selected_row(), &off),
        crate::ui::skirmish_shell::combo_text_rect(layout.language, scale),
        ShellAlign::V_CENTER,
        combo_face_text_color(false),
        SHELL_CONTROL_TEXT_DEPTH,
    ));

    let names = PLACE_KEYS
        .iter()
        .zip(layout.places)
        .map(|((key, _), rect)| (label(key), rect))
        .chain([
            (label(SHOW_SOURCE_KEY), layout.show_source),
            (label(SAME_ALPHABET_KEY), layout.same_alphabet),
        ])
        .chain(
            ChatLanguage::all()
                .zip(layout.reads)
                .map(|(language, rect)| (language.name().to_owned(), rect)),
        );
    let controls = (0..ChatPlace::ALL.len())
        .map(place_control)
        .chain([SHOW_SOURCE, SAME_ALPHABET])
        .chain((0..layout.reads.len()).map(reads_control));
    for (control, (name, rect)) in controls.zip(names) {
        let enabled = page.enabled(control);
        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::StatefulCheckbox {
                face: if page.is_checked(control) {
                    CheckboxFace::Checked
                } else {
                    CheckboxFace::Unchecked
                },
                disabled: !enabled,
                rect,
            },
            scale,
        );
        paint.texts.push(text(
            &name,
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            wol_button_label_rgb(enabled),
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    push_info_panel(&mut paint.controls, atlas, layout.preview);
    if !on {
        paint.texts.push(text(
            &label(PREVIEW_OFF_KEY),
            layout.preview_off(),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    for row in preview_rows(&page.settings) {
        let (tag, line) = layout.preview_row(row.top);
        if let Some(key) = row.place_key {
            paint.texts.push(text(
                &label(key),
                tag,
                ShellAlign::V_CENTER,
                PREVIEW_TAG_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        paint.texts.push(text(
            &row.text,
            line,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    if let Some(popup) = page.popup_geometry(layout) {
        push_popup_instances(
            &mut paint.popup,
            atlas,
            popup.rect,
            page.popup.hot_row.and_then(|row| popup.row_rect(row)),
            [],
            popup.scrollbar().zip(popup.thumb()),
            None,
            scale,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
        for index in popup.first..popup.first + popup.visible_rows {
            let Some(row) = popup.row_rect(index) else {
                continue;
            };
            paint.popup_texts.push(text(
                &WolChatTranslationPage::row_text(index, &off),
                scale.derive_rect(row, |rect| {
                    RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
                }),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        }
    }
    paint
}
