//! `0x10F`'s left-side children on the shared control owners: the two
//! labels, the nickname combo (face `0x00617250` with its NewEdit), the
//! `ES_PASSWORD` NewEdit, the Remember checkbox and the combo's drop list;
//! on VERA online's page, its info pane in their place and its popups over
//! it. The WOL page compositor draws them over its own backdrop and chrome.

use super::controls::{
    ControlPaint, NewEditPaint, dropdown_scroll_part, paint_control, push_new_edit_caret,
    push_new_edit_instances, push_popup_instances,
};
use super::text::{button_label_color_for_disabled, combo_face_text_color, rect_to_text_rect};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint, label as csf_label, push_edit};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::app::shell_wol::login_info::{Activity, News};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::list_frame_lines;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{FORM_CANCEL, FORM_OK, WolAccountForm};
use crate::ui::wol_login::{WolLoginInfoLayout, WolLoginLayout, WolLoginState};
use crate::util::time::Instant;

/// The info pane's panels sit over the page art, under its popups.
const INFO_FILL_DEPTH: f32 = shell_paint::CHROME_DEPTH - 0.00001;
const INFO_FRAME_DEPTH: f32 = shell_paint::CHROME_DEPTH - 0.00002;
/// The panels darken the art behind them, which still shows through.
const INFO_FILL_ALPHA: f32 = 0.82;
const INFO_TITLE_RGB: [f32; 3] = [1.0, 1.0, 1.0];
const INFO_BODY_RGB: [f32; 3] = [0.80, 0.80, 0.80];
const INFO_NOTE_RGB: [f32; 3] = [0.60, 0.60, 0.60];
/// Inside a panel's frame, in the 800x600 shell's pixels.
const INFO_PAD: i32 = 12;
/// A post's date column, and a rule's number column.
const INFO_DATE_W: i32 = 88;
const INFO_NUMBER_W: i32 = 20;

/// Sprites (skirmish atlas) and texts of `0x10F`'s children; `popup` paints
/// after everything else.
#[derive(Default)]
pub(crate) struct WolLoginPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
    pub(crate) popup: Vec<SpriteInstance>,
    pub(crate) popup_texts: Vec<ShellTextDraw>,
}

/// The disabled color of a disabled owner-draw label (`0x00612F5F`).
pub(crate) fn wol_button_label_rgb(enabled: bool) -> [f32; 3] {
    button_label_color_for_disabled(!enabled)
}

pub(crate) fn paint_wol_login_children(
    state: &AppState,
    login: &WolLoginState,
    layout: &WolLoginLayout,
) -> WolLoginPaint {
    let mut paint = WolLoginPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let now = Instant::now();
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
    if let Some(info) = layout.info {
        paint_info_pane(&mut paint, state, atlas, info, scale);
        return paint;
    }
    let label = |key: &str| csf_label(state, key);
    for (key, rect) in &layout.labels {
        paint.texts.push(text(
            &label(key),
            *rect,
            ShellAlign::H_RIGHT,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    // The nickname combo: its face and arrow, then its NewEdit's text and
    // caret inside the face's text area. ComboDrop's edit-child geometry is
    // untraced; the face's text rect stands in for it.
    paint_control(
        &mut paint.controls,
        &atlas.control_chrome(),
        ControlPaint::Combo {
            rect: layout.nickname_face,
            swatch: None,
            open: login.popup.control.is_some(),
            disabled: false,
        },
        scale,
    );
    let combo_text = layout.nickname_text_rect();
    let caret_visible = login.caret_visible(now);
    if caret_visible {
        // The combo's NewEdit has no frame of its own: only its caret paints
        // over the face.
        push_new_edit_caret(
            &mut paint.controls,
            atlas,
            font,
            &login.nickname,
            combo_text,
            SHELL_EDIT_CARET_DEPTH,
            scale,
        );
    }
    paint.texts.push(text(
        &login.nickname.painted_text(),
        combo_text,
        ShellAlign::V_CENTER,
        combo_face_text_color(false),
        SHELL_CONTROL_TEXT_DEPTH,
    ));

    {
        // 614FAB: the text starts two columns into the inset window.
        let password_text = scale.derive_rect(layout.password, |rect| {
            RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
        });
        push_new_edit_instances(
            &mut paint.controls,
            atlas,
            font,
            &login.password,
            NewEditPaint {
                frame: scale.derive_rect(layout.password, |rect| {
                    RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                }),
                text: password_text,
                background: None,
                frame_depth: SHELL_EDIT_FRAME_DEPTH,
                caret_depth: SHELL_EDIT_CARET_DEPTH,
                caret_visible,
            },
            scale,
        );
        paint.texts.push(text(
            &login.password.painted_text(),
            password_text,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));

        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Checkbox {
                rect: layout.remember,
                checked: login.remember_checked,
            },
            scale,
        );
        paint.texts.push(text(
            &label("GUI:RememberAccount"),
            crate::ui::skirmish_shell::checkbox_text_rect(layout.remember, scale),
            ShellAlign::V_CENTER,
            wol_button_label_rgb(login.remember_enabled),
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    if let Some(popup) = crate::app::shell_wol::combo_popup(login, layout) {
        push_popup_instances(
            &mut paint.popup,
            atlas,
            popup.rect,
            login.popup.hot_row.and_then(|row| popup.row_rect(row)),
            [],
            popup.scrollbar().zip(popup.thumb()),
            login.popup.scroll.pressed_part().map(dropdown_scroll_part),
            scale,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
        for index in popup.first..popup.first + popup.visible_rows {
            let (Some(row), Some(entry)) = (popup.row_rect(index), login.rows.get(index)) else {
                continue;
            };
            paint.popup_texts.push(text(
                &entry.nickname,
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

/// VERA online's info pane: the news panel, its heading row naming the
/// lobby's activity on the right, over the online rules panel. Each panel is
/// a list's frame over the darkened art; posts and rules wrap to at most two
/// lines, and what does not fit is left out.
fn paint_info_pane(
    paint: &mut WolLoginPaint,
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    info: WolLoginInfoLayout,
    scale: ShellScale,
) {
    let font = &state.renderer.bit_font;
    let line = font.cell_height().round() as i32;
    let lines = |value: &str, width: i32| {
        (font.wrap_layout(value, width.max(0) as u32).lines.len() as i32).clamp(1, 2)
    };
    // Rects are the 800x600 shell's; the text lays out in them.
    let mut text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| {
        paint.texts.push(shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(scale.scale_rect(rect)),
            rgb,
            align,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        ));
    };
    let label = |key: &str| csf_label(state, key);
    let inner = |panel: RectPx| {
        let panel = scale.base_rect(panel);
        RectPx::new(
            panel.x + INFO_PAD,
            panel.y + INFO_PAD - 2,
            panel.w - 2 * INFO_PAD,
            panel.h - 2 * INFO_PAD + 2,
        )
    };

    let news = inner(info.news);
    let heading = RectPx::new(news.x, news.y, news.w, line);
    text(
        &label("VERA:LatestNews"),
        heading,
        ShellAlign::NONE,
        SHELL_LABEL_TEXT_RGB,
    );
    let login_info = &state.frontend.login_info;
    if let Some(activity) = login_info.activity() {
        let (key, count) = match activity {
            Activity::Online(count) => ("VERA:PlayersOnline", count),
            Activity::RecentGames(count) => ("VERA:RecentGames", count),
        };
        let value = crate::i18n::fill(&label(key), &[("count", &count)]);
        text(&value, heading, ShellAlign::H_RIGHT, INFO_NOTE_RGB);
    }
    let bottom = news.y + news.h;
    let mut y = heading.y + line + 8;
    let note = match login_info.news() {
        News::Loading => Some("VERA:NewsLoading"),
        News::Posts(posts) if !posts.is_empty() => None,
        News::Unavailable | News::Posts(_) => Some("VERA:NewsUnavailable"),
    };
    if let Some(key) = note {
        text(
            &label(key),
            RectPx::new(news.x, y, news.w, line),
            ShellAlign::NONE,
            INFO_NOTE_RGB,
        );
    }
    if let News::Posts(posts) = login_info.news() {
        let (x, w) = (news.x + INFO_DATE_W, news.w - INFO_DATE_W);
        for post in posts {
            let (title, summary) = (lines(&post.title, w), lines(&post.summary, w));
            if y + line * (title + summary) > bottom {
                break;
            }
            text(
                &post.date,
                RectPx::new(news.x, y, INFO_DATE_W, line),
                ShellAlign::NONE,
                INFO_NOTE_RGB,
            );
            text(
                &post.title,
                RectPx::new(x, y, w, line * title),
                ShellAlign::NONE,
                INFO_TITLE_RGB,
            );
            text(
                &post.summary,
                RectPx::new(x, y + line * title, w, line * summary),
                ShellAlign::NONE,
                INFO_BODY_RGB,
            );
            y += line * (title + summary) + 10;
        }
    }

    let rules = inner(info.rules);
    text(
        &label("VERA:OnlineRules"),
        RectPx::new(rules.x, rules.y, rules.w, line),
        ShellAlign::NONE,
        SHELL_LABEL_TEXT_RGB,
    );
    let (bottom, x, w) = (
        rules.y + rules.h,
        rules.x + INFO_NUMBER_W,
        rules.w - INFO_NUMBER_W,
    );
    let mut y = rules.y + line + 8;
    for (index, rule) in label("VERA:OnlineRulesText").split('\n').enumerate() {
        let height = line * lines(rule, w);
        if y + height > bottom {
            break;
        }
        text(
            &format!("{}.", index + 1),
            RectPx::new(rules.x, y, INFO_NUMBER_W, line),
            ShellAlign::NONE,
            INFO_NOTE_RGB,
        );
        text(
            rule,
            RectPx::new(x, y, w, height),
            ShellAlign::NONE,
            INFO_BODY_RGB,
        );
        y += height + 2;
    }

    for panel in [info.news, info.rules] {
        push_info_panel(&mut paint.controls, atlas, panel);
    }
}

/// A panel's darkened backing and a list's two frame rings around it.
fn push_info_panel(out: &mut Vec<SpriteInstance>, atlas: &SkirmishShellChromeAtlas, panel: RectPx) {
    let Some(pixel) = atlas.white_pixel else {
        return;
    };
    let mut push = |rect: RectPx, tint: [f32; 3], alpha: f32, depth: f32| {
        out.push(SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: pixel.uv_origin,
            uv_size: pixel.uv_size,
            depth,
            tint,
            alpha,
            ..Default::default()
        });
    };
    push(
        RectPx::new(panel.x + 1, panel.y + 1, panel.w - 2, panel.h - 2),
        super::SHELL_MODAL_PANEL_RGB,
        INFO_FILL_ALPHA,
        INFO_FILL_DEPTH,
    );
    for (rect, tone) in list_frame_lines(panel) {
        push(
            rect,
            tone.rgb().map(|channel| f32::from(channel) / 255.0),
            1.0,
            INFO_FRAME_DEPTH,
        );
    }
}

/// VERA online's popups on Red Alert 2's box art, as the online Create Game
/// box: a caption and edit per field, Login's Remember password box, OK and
/// Cancel, and the status line.
pub(crate) fn paint_wol_account_form(
    state: &AppState,
    form: &WolAccountForm,
    layout: &WolAccountFormLayout,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let plain = |text: String, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| PaintLabel {
        text: text.into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    };
    let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    let button_label = |id: u16, rect: RectPx| {
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            rect,
            form.buttons.is_pressed(id),
        )
    };
    let mut labels: Vec<PaintLabel> = form
        .fields
        .iter()
        .zip(&layout.fields)
        .map(|(field, (caption, _))| {
            plain(
                csf_label(state, field.label),
                *caption,
                ShellAlign::H_RIGHT,
                SHELL_LABEL_TEXT_RGB,
            )
        })
        .collect();
    labels.push(plain(
        csf_label(state, "GUI:OK"),
        button_label(FORM_OK, layout.ok),
        button_align,
        wol_button_label_rgb(form.ok_enabled()),
    ));
    labels.push(plain(
        csf_label(state, "GUI:Cancel"),
        button_label(FORM_CANCEL, layout.cancel),
        button_align,
        SHELL_LABEL_TEXT_RGB,
    ));
    labels.push(plain(
        form.status.clone(),
        layout.status,
        ShellAlign::NONE,
        SHELL_LABEL_TEXT_RGB,
    ));
    let mut checkboxes = Vec::new();
    if let (Some(checked), Some(rect)) = (form.remember, layout.remember) {
        paint_control(
            &mut checkboxes,
            &atlas.control_chrome(),
            ControlPaint::Checkbox { rect, checked },
            scale,
        );
        labels.push(plain(
            csf_label(state, "VERA:RememberPassword"),
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
    let buttons = [
        ModalButton {
            rect: layout.ok,
            pressed: form.buttons.is_pressed(FORM_OK),
            enabled: form.ok_enabled(),
        },
        ModalButton {
            rect: layout.cancel,
            pressed: form.buttons.is_pressed(FORM_CANCEL),
            enabled: true,
        },
    ];
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        background,
        super::type3_button_frames(atlas),
        layout.dialog,
        &buttons,
        &labels,
        scale,
        POPUP_DEPTHS,
    );
    // Paint order is draw order: the box first, then its checkbox.
    paint.controls = draw.sprites;
    paint.controls.extend(checkboxes);
    paint.texts = draw.text;
    let caret_visible = form.caret_visible(Instant::now());
    for (field, (_, rect)) in form.fields.iter().zip(&layout.fields) {
        push_edit(
            &mut paint,
            state,
            &field.edit,
            *rect,
            caret_visible && field.edit.focused,
            scale,
            POPUP_DEPTHS.text,
        );
    }
    paint
}
