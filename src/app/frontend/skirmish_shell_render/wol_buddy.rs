//! `0x116`'s children on the shared control owners: the column headers,
//! the owner-draw buddy list `0x689` with its cells (`0x4A8` at the
//! `0x0084A1C4` stops), Refresh's picture, the observer checkbox and image,
//! and the selected buddy's heading and statistics. The WOL page
//! compositor draws them, and the group box, over its own backdrop and
//! chrome.

use super::chrome::push_flag_entry_clipped_centered;
use super::controls::{ControlPaint, list_controls, paint_control};
use super::text::rect_to_text_rect;
use super::wol_lobby::{WolLobbyPaint, label, push_icon, row_rgb};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint;
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_buddy::*;

pub(crate) fn paint_wol_buddy_children(
    state: &AppState,
    ui: &WolBuddyPage,
    layout: &WolBuddyLayout,
    art: Option<&WolPageArt>,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        )
    };
    // A cell `x` base columns into a row, `w` wide (to the row's end when 0).
    let cell = |row: RectPx, x: i32, w: i32| {
        scale.derive_rect(row, |row| {
            let w = if w > 0 { w } else { row.w - x - 2 };
            RectPx::new(row.x + x, row.y, w, row.h)
        })
    };
    // The headers are left-aligned statics; the statistics' labels SS_RIGHT.
    for (index, ((key, _), rect)) in LABELS.iter().zip(layout.labels).enumerate() {
        let align = if index < 2 {
            ShellAlign::NONE
        } else {
            ShellAlign::H_RIGHT
        };
        paint
            .texts
            .push(text(&label(state, key), rect, align, SHELL_LABEL_TEXT_RGB));
    }
    let value = |id: u16| match &ui.texts {
        Some(texts) => texts
            .iter()
            .find(|(control, _)| *control == id)
            .map(|(_, value)| value.clone())
            .unwrap_or_default(),
        None if id == HEADING => String::new(),
        None => label(state, UNKNOWN_STATS_KEY),
    };
    let heading = value(HEADING);
    if !heading.is_empty() {
        paint.texts.push(text(
            &heading,
            layout.heading,
            ShellAlign::H_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
    for (id, rect) in STATS.iter().copied().zip(layout.stats) {
        let value = value(id);
        if !value.is_empty() {
            paint
                .texts
                .push(text(&value, rect, ShellAlign::NONE, SHELL_LABEL_TEXT_RGB));
        }
    }

    // Refresh's picture: released, pressed, or disabled (the released
    // face; the owner-draw button greys nothing of a picture).
    if let Some(art) = art {
        let face = if ui.buttons.is_pressed(REFRESH) {
            REFRESH_FACES.1
        } else {
            REFRESH_FACES.0
        };
        if let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == face) {
            let (w, h) = entry.native_size();
            let base = scale.base_rect(layout.refresh);
            let [x, y] = scale.scale_point([base.x + (base.w - w) / 2, base.y + (base.h - h) / 2]);
            shell_paint::push_entry_grown(
                &mut paint.icons,
                *entry,
                x,
                y,
                scale,
                SHELL_CONTROL_DEPTH - 0.00003,
            );
        }
    }
    paint_control(
        &mut paint.controls,
        &atlas.control_chrome(),
        ControlPaint::Checkbox {
            rect: layout.observer_toggle,
            checked: ui.observer,
        },
        scale,
    );
    if let Some(entry) = atlas.lan_observer_toggle(ui.observer) {
        push_flag_entry_clipped_centered(
            &mut paint.controls,
            entry,
            layout.observer_image,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }

    let geometry = ui.list.geometry(layout.list, ui.rows.len(), scale);
    list_controls(&mut paint.controls, atlas, geometry, &ui.list, true, scale);
    for (visible, row) in ui
        .rows
        .iter()
        .skip(ui.list.top_index)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let rect = geometry.row(visible);
        let rgb = row_rgb(row.color);
        paint.texts.push(text(
            &row.name,
            cell(rect, NAME_X, NAME_W),
            ShellAlign::V_CENTER,
            rgb,
        ));
        paint.texts.push(text(
            &row.status,
            cell(rect, STATUS_X, STATUS_W),
            ShellAlign::V_CENTER,
            rgb,
        ));
        let Some(game) = &row.game else {
            continue;
        };
        paint.texts.push(text(
            &game.ping.to_string(),
            cell(rect, PING_X, 0),
            ShellAlign::V_CENTER,
            rgb,
        ));
        if let Some(art) = art {
            if let Some(file) = game.kind.file() {
                push_icon(&mut paint.icons, art, file, rect, TYPE_X, scale);
            }
            if game.keyed {
                push_icon(
                    &mut paint.icons,
                    art,
                    "wolpriv.pcx",
                    rect,
                    PASSWORD_X,
                    scale,
                );
            }
            if let Some(file) = game.limit_icon {
                push_icon(&mut paint.icons, art, file, rect, LIMIT_X, scale);
            }
        }
    }
    paint
}
