//! The left-side children of the WOL persona pages: `0x112`'s question and
//! Don't Ask box (`OwnerDraw_CheckBox 0x006163A0`), `0xFE`'s caption and
//! owner-draw locale list (`0x00618D40`). The WOL page compositor draws them
//! over its own backdrop and chrome.

use super::controls::{ControlPaint, paint_control};
use super::text::rect_to_text_rect;
use super::wol_options::WolOptionsPaint;
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::wol_persona::{
    AUTO_LOGIN_TEXT_KEY, DO_NOT_ASK_KEY, LOCALE_LABEL_KEY, WolAutoLoginQuery, WolLocalePage,
    WolPersonaLayout,
};

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

/// `0x112`: the centred question (`SS_CENTER|SS_CENTERIMAGE`) and Don't Ask.
pub(crate) fn paint_wol_auto_login_children(
    state: &AppState,
    query: &WolAutoLoginQuery,
    layout: &WolPersonaLayout,
) -> WolOptionsPaint {
    let mut paint = WolOptionsPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let scale = layout.page.scale;
    paint.texts.push(text(
        state,
        &label(state, AUTO_LOGIN_TEXT_KEY),
        layout.text,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        scale,
    ));
    paint_control(
        &mut paint.controls,
        &atlas.control_chrome(),
        ControlPaint::Checkbox {
            rect: layout.control,
            checked: query.do_not_ask,
        },
        scale,
    );
    paint.texts.push(text(
        state,
        &label(state, DO_NOT_ASK_KEY),
        crate::ui::skirmish_shell::checkbox_text_rect(layout.control, scale),
        ShellAlign::V_CENTER,
        scale,
    ));
    paint
}

/// `0xFE`: the centred caption and the list with its selection.
pub(crate) fn paint_wol_locale_children(
    state: &AppState,
    page: &WolLocalePage,
    layout: &WolPersonaLayout,
) -> WolOptionsPaint {
    let mut paint = WolOptionsPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let scale = layout.page.scale;
    paint.texts.push(text(
        state,
        &label(state, LOCALE_LABEL_KEY),
        layout.text,
        ShellAlign::H_CENTER,
        scale,
    ));
    let geometry = crate::app::shell_wol::persona::list_geometry(layout, page);
    super::list::paint_list(
        &mut paint.controls,
        atlas,
        geometry,
        page.top,
        page.selected,
        page.scroll.pressed_part(),
        false,
        scale,
    );
    for (visible, row) in page
        .rows
        .iter()
        .skip(page.top)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let r = geometry.row(visible);
        paint.texts.push(text(
            state,
            &row.text,
            RectPx::new(r.x + 2, r.y, r.w - 2, r.h),
            ShellAlign::V_CENTER,
            scale,
        ));
    }
    paint
}
