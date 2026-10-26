//! `0x113`'s children and its two popups on the shared control owners: the
//! lists (`0x00618D40`/`0x00619230` with their column cells `0x4A8`), the
//! chat NewEdit, the image buttons (`0x497` faces), the observer checkbox
//! and image, and the Create Game `0xC3` / channel password `0xE1` boxes.
//! The WOL page compositor draws them over its own background and chrome.
//!
//! Residual: the five text sort buttons (`0x765`, `0x766`, `0x76A..0x76C`)
//! paint their caption only; their owner-draw face is not traced.

use super::chrome::*;
use super::controls::{ControlPaint, list_controls, paint_control, push_new_edit};
use super::text::rect_to_text_rect;
use super::{
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF, SHELL_CONTROL_DEPTH,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint::{self, ModalButton, ModalDepths, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;
use crate::ui::wol_lobby::*;
use crate::util::time::Instant;

/// The popups paint over the covered dialog, before their own controls.
pub(super) const POPUP_DEPTHS: ModalDepths = ModalDepths {
    background: 0.00060,
    button: 0.00057,
    text: 0.00038,
};

/// `0x808080`: a full game's row (`0x00781480`).
const GREYED_RGB: [f32; 3] = [128.0 / 255.0; 3];

/// Sprites on the skirmish atlas, sprites on the lobby art and texts.
#[derive(Default)]
pub(crate) struct WolLobbyPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) icons: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
}

/// LB row colors are packed `00BBGGRR`; -1 is the ordinary yellow.
pub(super) fn row_rgb(color: u32) -> [f32; 3] {
    if color == u32::MAX {
        return SHELL_LABEL_TEXT_RGB;
    }
    [color as u8, (color >> 8) as u8, (color >> 16) as u8].map(|c| f32::from(c) / 255.0)
}

pub(super) fn label(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned())
}

/// A PCX of the lobby art centred vertically in `rect`, `x` columns in.
pub(super) fn push_icon(
    out: &mut Vec<SpriteInstance>,
    art: &WolPageArt,
    file: &str,
    rect: RectPx,
    x: i32,
    scale: ShellScale,
) {
    let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == file) else {
        return;
    };
    let (_, h) = entry.native_size();
    let base = scale.base_rect(rect);
    let [px, py] = scale.scale_point([base.x + x, base.y + (base.h - h) / 2]);
    shell_paint::push_entry_grown(out, *entry, px, py, scale, SHELL_CONTROL_DEPTH - 0.00003);
}

/// A NewEdit as `0x10F` and the WOL popups paint theirs.
pub(super) fn push_edit(
    paint: &mut WolLobbyPaint,
    state: &AppState,
    edit: &SavedSeedDescriptionEdit,
    rect: RectPx,
    caret_visible: bool,
    scale: ShellScale,
    text_depth: f32,
) {
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return;
    };
    push_new_edit(
        &mut paint.controls,
        &mut paint.texts,
        atlas,
        &state.renderer.bit_font,
        edit,
        None,
        rect,
        caret_visible,
        text_depth,
        scale,
    );
}

pub(crate) fn paint_wol_lobby_children(
    state: &AppState,
    ui: &WolLobbyState,
    layout: &WolLobbyLayout,
    art: Option<&WolPageArt>,
    base_names: &[String; 8],
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let modern = ui.is_modern();
    let now = Instant::now();
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
    for (key, rect) in layout.labels {
        paint.texts.push(text(
            &label(state, key),
            rect,
            ShellAlign::NONE,
            SHELL_LABEL_TEXT_RGB,
        ));
    }

    // The image buttons' faces and the text sort buttons' captions.
    for (id, rect) in layout
        .sort_buttons
        .iter()
        .copied()
        .chain([(REFRESH, layout.refresh), (EMOTE, layout.emote)])
    {
        let pressed = ui.buttons.is_pressed(id);
        match IMAGE_BUTTONS.iter().find(|(known, ..)| *known == id) {
            Some((_, released_face, pressed_face)) => {
                if let Some(art) = art {
                    let face = if pressed { pressed_face } else { released_face };
                    if let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == face) {
                        let (w, h) = entry.native_size();
                        let base = scale.base_rect(rect);
                        let [x, y] = scale
                            .scale_point([base.x + (base.w - w) / 2, base.y + (base.h - h) / 2]);
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
            }
            None => {
                if let Some((_, key)) = SORT_BUTTONS.iter().find(|(known, _)| *known == id) {
                    let caption = if modern && matches!(id, SORT_ALPHA | PLAYER_SORT_ALPHA) {
                        "Name".to_owned()
                    } else {
                        label(state, key)
                    };
                    paint.texts.push(text(
                        &caption,
                        rect,
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        SHELL_LABEL_TEXT_RGB,
                    ));
                }
            }
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
    push_new_edit(
        &mut paint.controls,
        &mut paint.texts,
        atlas,
        font,
        &ui.chat.edit,
        state
            .platform
            .ime
            .composition(crate::app::input::ime::ImeTarget::WolLobbyChat),
        layout.chat,
        ui.popup.is_none() && ui.caret_visible(now),
        SHELL_CONTROL_TEXT_DEPTH,
        scale,
    );

    let lobbies = ui
        .lobby_list
        .geometry(layout.lobbies, ui.lobby_rows.len(), scale);
    let games = ui
        .game_list
        .geometry(layout.games, ui.game_rows.len(), scale);
    let players = ui
        .player_list
        .geometry(layout.players, ui.player_rows.len(), scale);
    let chat = ui
        .chat
        .list
        .geometry(layout.chat_log, ui.chat.lines.len(), scale);
    list_controls(
        &mut paint.controls,
        atlas,
        lobbies,
        &ui.lobby_list,
        true,
        scale,
    );
    list_controls(
        &mut paint.controls,
        atlas,
        games,
        &ui.game_list,
        true,
        scale,
    );
    list_controls(
        &mut paint.controls,
        atlas,
        players,
        &ui.player_list,
        false,
        scale,
    );
    list_controls(
        &mut paint.controls,
        atlas,
        chat,
        &ui.chat.list,
        false,
        scale,
    );
    // LBS_MULTIPLESEL: every selected user row is filled.
    for row in &ui.selected_players {
        if let Some(visible) = row.checked_sub(ui.player_list.top_index)
            && visible < players.visible_rows
        {
            push_solid_rect(
                &mut paint.controls,
                atlas,
                players.row(visible),
                OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                SHELL_CONTROL_DEPTH - 0.00001,
            );
        }
    }

    for (visible, row) in ui
        .lobby_rows
        .iter()
        .skip(ui.lobby_list.top_index)
        .take(lobbies.visible_rows)
        .enumerate()
    {
        paint.texts.push(text(
            &row.text(base_names),
            cell(lobbies.row(visible), LOBBY_TEXT_X, 0),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
    for (visible, row) in ui
        .game_rows
        .iter()
        .skip(ui.game_list.top_index)
        .take(games.visible_rows)
        .enumerate()
    {
        let rect = games.row(visible);
        let rgb = if row.greyed {
            GREYED_RGB
        } else {
            SHELL_LABEL_TEXT_RGB
        };
        paint.texts.push(text(
            &row.text,
            if modern {
                cell(rect, 2, GAME_PING_X - 4)
            } else {
                cell(rect, GAME_NAME_X, GAME_NAME_W)
            },
            ShellAlign::V_CENTER,
            rgb,
        ));
        // A -1 ping is the text "???"; any other is an integer cell.
        let ping = if row.ping == -1 {
            "???".to_owned()
        } else {
            row.ping.to_string()
        };
        paint.texts.push(text(
            &ping,
            cell(rect, GAME_PING_X, 0),
            ShellAlign::V_CENTER,
            rgb,
        ));
        if !modern && let Some(art) = art {
            push_icon(
                &mut paint.icons,
                art,
                row.kind.file(),
                rect,
                GAME_TYPE_X,
                scale,
            );
            if row.keyed {
                push_icon(
                    &mut paint.icons,
                    art,
                    "wolpriv.pcx",
                    rect,
                    GAME_PASSWORD_X,
                    scale,
                );
            }
            if let Some(file) = row.limit_icon {
                push_icon(&mut paint.icons, art, file, rect, GAME_LIMIT_X, scale);
            }
        }
    }
    for (visible, row) in ui
        .player_rows
        .iter()
        .skip(ui.player_list.top_index)
        .take(players.visible_rows)
        .enumerate()
    {
        let rect = players.row(visible);
        paint.texts.push(text(
            &row.name,
            if modern {
                cell(rect, 2, 0)
            } else {
                cell(rect, PLAYER_NAME_X, PLAYER_NAME_W)
            },
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
        if modern {
            continue;
        }
        if let Some((tag, _)) = &row.clan {
            paint.texts.push(text(
                tag,
                cell(rect, PLAYER_CLAN_X, 0),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        if let Some(rank) = &row.rank {
            paint.texts.push(text(
                rank,
                cell(rect, PLAYER_RANK_X, 0),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        if let Some(art) = art {
            if let Some((file, _)) = row.icon {
                push_icon(&mut paint.icons, art, file, rect, PLAYER_ICON_X, scale);
            }
            for (file, x) in &row.score_icons {
                push_icon(&mut paint.icons, art, file, rect, *x, scale);
            }
        }
    }
    for (visible, line) in ui
        .chat
        .lines
        .iter()
        .skip(ui.chat.list.top_index)
        .take(chat.visible_rows)
        .enumerate()
    {
        paint.texts.push(text(
            &String::from_utf16_lossy(&line.text),
            cell(chat.row(visible), 2, 0),
            ShellAlign::V_CENTER,
            row_rgb(line.color),
        ));
    }
    paint
}

/// `0xC3` or `0xE1` over the covered dialog; `status` is `0xC3`'s own
/// status line text.
pub(crate) fn paint_wol_lobby_popup(
    state: &AppState,
    popup: &WolLobbyPopup,
    width: i32,
    height: i32,
    status: &str,
    caret_visible: bool,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let scale = ShellScale::RETAIL;
    let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    let caption = |key: &str, rect: RectPx, align: ShellAlign, enabled: bool| PaintLabel {
        text: label(state, key).into(),
        rect,
        align,
        rgb: if enabled {
            SHELL_LABEL_TEXT_RGB
        } else {
            shell_paint::SHELL_TEXT_RGB_DISABLED
        },
        path_a_reveal: None,
    };
    let button = |id: u16, rect: RectPx| ModalButton {
        rect,
        pressed: popup.buttons.is_pressed(id),
        enabled: popup.enabled(id),
    };
    let ok_label = |rect: RectPx| {
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            rect,
            popup.buttons.is_pressed(POPUP_OK),
        )
    };
    let cancel_label = |rect: RectPx| {
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            rect,
            popup.buttons.is_pressed(POPUP_CANCEL),
        )
    };
    let (dialog, buttons, mut labels, password) = match &popup.kind {
        WolPopupKind::Create(create) => {
            let layout = crate::ui::shell::modal::wol_create_game_layout(width, height);
            for (rect, checked) in [
                (layout.clan, create.clan),
                (layout.limit, create.limit),
                (layout.observe, create.observe),
            ] {
                paint_control(
                    &mut paint.controls,
                    &atlas.control_chrome(),
                    ControlPaint::Checkbox { rect, checked },
                    scale,
                );
            }
            let check_text = |rect| crate::ui::skirmish_shell::checkbox_text_rect(rect, scale);
            let mut labels = vec![
                // Style SS_RIGHT.
                caption(
                    "GUI:Password",
                    layout.password_label,
                    ShellAlign::H_RIGHT,
                    true,
                ),
                caption(
                    "GUI:BattleClanTournament",
                    check_text(layout.clan),
                    ShellAlign::V_CENTER,
                    create.clan_enabled,
                ),
                caption(
                    "WOL:LimitScreenResolution",
                    check_text(layout.limit),
                    ShellAlign::V_CENTER,
                    create.limit_enabled,
                ),
                caption(
                    "GUI:Observe",
                    check_text(layout.observe),
                    ShellAlign::V_CENTER,
                    true,
                ),
                caption(
                    "GUI:OK",
                    ok_label(layout.ok),
                    button_align,
                    popup.ok_enabled(),
                ),
                caption(
                    "GUI:Cancel",
                    cancel_label(layout.cancel),
                    button_align,
                    true,
                ),
            ];
            labels.push(PaintLabel {
                text: status.to_owned().into(),
                rect: layout.status,
                align: ShellAlign::NONE,
                rgb: SHELL_LABEL_TEXT_RGB,
                path_a_reveal: None,
            });
            (
                layout.dialog,
                vec![
                    button(POPUP_OK, layout.ok),
                    button(POPUP_CANCEL, layout.cancel),
                ],
                labels,
                layout.password,
            )
        }
        WolPopupKind::Password(_) => {
            let layout = crate::ui::shell::modal::wol_channel_password_layout(width, height);
            (
                layout.dialog,
                vec![
                    button(POPUP_OK, layout.ok),
                    button(POPUP_CANCEL, layout.cancel),
                ],
                vec![
                    // Style SS_CENTER.
                    caption(
                        "GUI:ChannelPassword",
                        layout.label,
                        ShellAlign::H_CENTER,
                        true,
                    ),
                    caption(
                        "GUI:OK",
                        ok_label(layout.ok),
                        button_align,
                        popup.ok_enabled(),
                    ),
                    caption(
                        "GUI:Cancel",
                        cancel_label(layout.cancel),
                        button_align,
                        true,
                    ),
                ],
                layout.password,
            )
        }
    };
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(None),
        super::type3_button_frames(atlas),
        dialog,
        &buttons,
        &labels,
        scale,
        POPUP_DEPTHS,
    );
    labels.clear();
    // Paint order is draw order: the box first, then its checkboxes.
    let checkboxes = std::mem::replace(&mut paint.controls, draw.sprites);
    paint.controls.extend(checkboxes);
    paint.texts.extend(draw.text);
    push_edit(
        &mut paint,
        state,
        popup.edit(),
        password,
        caret_visible,
        scale,
        POPUP_DEPTHS.text,
    );
    paint
}
