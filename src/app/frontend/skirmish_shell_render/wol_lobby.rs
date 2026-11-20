//! `0x113`'s children and its two popups on the shared control owners: the
//! lists (`0x00618D40`/`0x00619230` with their column cells `0x4A8`), the
//! chat NewEdit, the image buttons (`0x497` faces), the observer checkbox
//! and image, and the Create Game `0xC3` / channel password `0xE1` boxes.
//! The WOL page compositor draws them over its own background and chrome.
//!
//! Residual: the five text sort buttons (`0x765`, `0x766`, `0x76A..0x76C`)
//! paint their caption only; their owner-draw face is not traced.

use super::chrome::*;
use super::controls::{
    ControlPaint, dropdown_scroll_part, list_view_controls, paint_control, push_new_edit,
    push_popup_instances,
};
use super::gui_paint::GuiStyle;
use super::text::rect_to_text_rect;
use super::{
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF, SHELL_CONTROL_DEPTH,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint::{self, ModalButton, ModalDepths, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;
use crate::ui::wol_lobby::*;

/// The popups paint over the covered dialog, before their own controls.
pub(super) const POPUP_DEPTHS: ModalDepths = ModalDepths {
    background: 0.00060,
    button: 0.00057,
    text: 0.00038,
};

/// VERA's online boxes' art at `dialog`, under their widgets: Red Alert
/// 2's box, as VERA's updater boxes have, else gamemd's mode-2 box.
pub(super) fn online_box(
    atlas: &crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas,
    dialog: RectPx,
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    shell_paint::paint_modal_sprites(
        background,
        super::type3_button_frames(atlas),
        dialog,
        &[],
        scale,
        POPUP_DEPTHS,
    )
}

/// `0x808080`: a full game's row (`0x00781480`).
const GREYED_RGB: [f32; 3] = [128.0 / 255.0; 3];

/// VERA: the modern game list's gap between a room's name and its tag.
const GAME_TAG_GAP: i32 = 8;

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
    if let Some(rect) = layout.observer_caption {
        paint.texts.push(text(
            &label(state, "GUI:Observe"),
            rect,
            ShellAlign::H_RIGHT,
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
        let pressed = ui.pressed(id);
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
                        crate::i18n::tr("VERA:SortName")
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
            checked: ui.observer(),
        },
        scale,
    );
    if !modern && let Some(entry) = atlas.lan_observer_toggle(ui.observer()) {
        push_flag_entry_clipped_centered(
            &mut paint.controls,
            entry,
            layout.observer_image,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    // The chat NewEdit keeps the focus under the popups too.
    let chat_edit = SavedSeedDescriptionEdit {
        focused: true,
        ..ui.edit(CHAT)
    };
    push_new_edit(
        &mut paint.controls,
        &mut paint.texts,
        atlas,
        font,
        &chat_edit,
        state
            .platform
            .ime
            .composition(crate::app::input::ime::ImeTarget::WolLobbyChat),
        layout.chat,
        ui.caret_visible(),
        SHELL_LABEL_TEXT_RGB,
        SHELL_CONTROL_TEXT_DEPTH,
        scale,
    );

    // The owner-draw lists on the rows and top the tree keeps (the
    // transcript's top is its own, which new lines move); the transcript and
    // the multi-select user list fill no selected row.
    let mut list = |id: u16, rect: RectPx, count: usize, selectable: bool, paint_it: bool| {
        let view = ui.list_view(id);
        let top = match id {
            CHAT_LOG => ui.chat.list.top_index,
            _ => view.map_or(0, |view| view.top),
        };
        let geometry = ShellListGeometry::new(rect, count, top, ListMetrics::scaled(scale));
        let selected = view.and_then(|view| view.selected).filter(|_| selectable);
        if paint_it {
            let pressed = view.and_then(|view| view.pressed);
            list_view_controls(
                &mut paint.controls,
                atlas,
                geometry,
                (selected, top, pressed),
                scale,
            );
        }
        (geometry, top)
    };
    let (lobbies, lobby_top) = list(LOBBIES, layout.lobbies, ui.lobby_rows.len(), true, !modern);
    let (games, game_top) = list(GAMES, layout.games, ui.game_rows.len(), true, true);
    let (players, player_top) = list(PLAYERS, layout.players, ui.player_rows.len(), false, true);
    let (chat, chat_top) = list(CHAT_LOG, layout.chat_log, ui.chat.lines.len(), false, true);
    // LBS_MULTIPLESEL: every selected user row is filled.
    for row in &ui.selected_players {
        if let Some(visible) = row.checked_sub(player_top)
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
        .skip(lobby_top)
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
    let ping_header = layout.control_rect(SORT_PING);
    for (visible, row) in ui
        .game_rows
        .iter()
        .skip(game_top)
        .take(games.visible_rows)
        .enumerate()
    {
        let rect = games.row(visible);
        let rgb = if row.greyed {
            GREYED_RGB
        } else {
            SHELL_LABEL_TEXT_RGB
        };
        // The modern list keeps one retail icon, a password game's lock, in
        // the first icon column; that row's name starts at the next one. A
        // tag ends the name cell against the Ping column, and the name stops
        // a gap before it.
        let name_x = if row.keyed {
            GAME_PASSWORD_X
        } else {
            GAME_TYPE_X
        };
        let name_end = GAME_PING_X - 2;
        let tag = row.tag.as_deref().filter(|_| modern);
        let tag_w = tag.map_or(0, |tag| {
            (font.text_width(tag) as i32).min(name_end - name_x)
        });
        let name_w = match tag {
            Some(_) => name_end - name_x - tag_w - GAME_TAG_GAP,
            None => name_end - name_x,
        };
        paint.texts.push(text(
            &row.text,
            if modern {
                cell(rect, name_x, name_w.max(1))
            } else {
                cell(rect, GAME_NAME_X, GAME_NAME_W)
            },
            ShellAlign::V_CENTER,
            rgb,
        ));
        if let Some(tag) = tag {
            paint.texts.push(text(
                tag,
                cell(rect, name_end - tag_w, tag_w.max(1)),
                ShellAlign::H_RIGHT | ShellAlign::V_CENTER,
                rgb,
            ));
        }
        // A -1 ping is the text "???"; any other is an integer cell. The
        // modern list centres it under its Ping header instead of starting
        // it at the retail column, left of the header's caption. A match to
        // watch has no ping.
        let ping = if row.ping == -1 {
            "???".to_owned()
        } else {
            row.ping.to_string()
        };
        let (ping_rect, ping_align) = match ping_header.filter(|_| modern) {
            Some(header) => (
                RectPx::new(header.x, rect.y, header.w, rect.h),
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            ),
            None => (cell(rect, GAME_PING_X, 0), ShellAlign::V_CENTER),
        };
        if !row.watch {
            paint.texts.push(text(&ping, ping_rect, ping_align, rgb));
        }
        if modern {
            if row.keyed
                && let Some(art) = art
            {
                push_icon(
                    &mut paint.icons,
                    art,
                    "wolpriv.pcx",
                    rect,
                    GAME_TYPE_X,
                    scale,
                );
            }
        } else if let Some(art) = art {
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
        .skip(player_top)
        .take(players.visible_rows)
        .enumerate()
    {
        let rect = players.row(visible);
        // VERA: the modern list shows only the badge of a row's high-score
        // cells, at its left, and every name as far right of it as the retail
        // list's, so names line up whether or not a row has a badge.
        if modern {
            paint.texts.push(text(
                &row.name,
                cell(rect, 2 + PLAYER_NAME_X - PLAYER_BADGE_X, 0),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
            if let Some(art) = art
                && let Some((file, _)) = row.score_icons.iter().find(|(_, x)| *x == PLAYER_BADGE_X)
            {
                push_icon(&mut paint.icons, art, file, rect, 2, scale);
            }
            continue;
        }
        paint.texts.push(text(
            &row.name,
            cell(rect, PLAYER_NAME_X, PLAYER_NAME_W),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
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
        .skip(chat_top)
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
/// status line text. The `modern` lobby draws them on Red Alert 2's box art,
/// as VERA's updater boxes do; the native lobby keeps gamemd's mode-2 art.
/// `scale` is the lobby page's, so the box grows with the page. The box
/// first, then its widgets: OK and Cancel, the room's limits, the boxes and
/// the edits, and their captions, titles and values in the order they read.
pub(crate) fn paint_wol_lobby_popup(
    state: &AppState,
    ui: &WolLobbyState,
    width: i32,
    height: i32,
    scale: ShellScale,
    status: &str,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let (Some(atlas), Some(popup)) = (
        state.frontend.skirmish_shell_chrome.as_ref(),
        ui.popup.as_ref(),
    ) else {
        return paint;
    };
    let modern = ui.is_modern();
    let dialog = match popup.create {
        Some(_) => {
            crate::ui::shell::modal::wol_create_game_layout_on(width, height, modern, scale).dialog
        }
        None => {
            crate::ui::shell::modal::wol_channel_password_layout_on(width, height, scale).dialog
        }
    };
    let background = atlas
        .ra2_dialog_background
        .filter(|_| modern)
        .or_else(|| atlas.mode_two_dialog_background(None));
    let frames = super::type3_button_frames(atlas);
    paint.controls =
        shell_paint::paint_modal_sprites(background, frames, dialog, &[], scale, POPUP_DEPTHS);
    let titled = |id: i32| {
        BOXES
            .iter()
            .zip(TITLE..)
            .find(|((control, _), title)| i32::from(*control) == id || i32::from(*title) == id)
    };
    // Online, a box shows its value beside it: Yes or No, or Watching's.
    let text_of = |id: i32| match u16::try_from(id).ok()? {
        STATUS => Some(status.to_owned()),
        CREATE_WATCHING if modern => {
            let key = popup.watching()?.label_key();
            Some(super::text::localized_label(state, key, key))
        }
        id if modern && titled(id.into()).is_some_and(|((control, _), _)| *control == id) => {
            let key = if ui.popup_checked(id) {
                "GUI:Yes"
            } else {
                "GUI:No"
            };
            Some(super::text::localized_label(state, key, key))
        }
        _ => None,
    };
    // A title greys with its box.
    let enabled_of = |id: i32| {
        let ((control, _), _) = titled(id).filter(|(_, title)| i32::from(*title) == id)?;
        Some(ui.popup_enabled(*control))
    };
    let caret_of = |node| {
        ui.gui()
            .id_of(node)
            .and_then(|id| u16::try_from(id).ok())
            .is_some_and(|id| ui.popup_caret_visible(id))
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        enabled_of: Some(&enabled_of),
        caret_of: Some(&caret_of),
        ..GuiStyle::popup(atlas)
    };
    let boxes = BOXES.map(|(id, _)| id);
    let sprites = [POPUP_OK, POPUP_CANCEL, CREATE_PLAYERS]
        .into_iter()
        .chain(boxes)
        .chain([POPUP_PASSWORD, CREATE_NAME]);
    let texts = [PASSWORD_CAPTION, PLAYERS_CAPTION, CREATE_PLAYERS]
        .into_iter()
        .chain(
            boxes
                .into_iter()
                .zip(TITLE..)
                .flat_map(|(id, title)| [title, id]),
        )
        .chain([
            NAME_CAPTION,
            POPUP_OK,
            POPUP_CANCEL,
            STATUS,
            POPUP_PASSWORD,
            CREATE_NAME,
        ]);
    let paint_ids =
        |ids| super::gui_paint::paint_gui_ids(state, ui.gui(), popup.root, ids, scale, style);
    paint
        .controls
        .extend(paint_ids(sprites.collect::<Vec<_>>()).controls);
    paint.texts = paint_ids(texts.collect::<Vec<_>>()).texts;
    paint
}

/// VERA's player card over the modern lobby, on the online Create Game box:
/// a caption and value per row, the player list's badge beside the name, the
/// four buttons and the status line.
pub(crate) fn paint_wol_player_card(
    state: &AppState,
    view: &crate::app::shell_wol::player_card::PlayerCardView,
    art: Option<&WolPageArt>,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let plain = |text: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| PaintLabel {
        text: text.to_owned().into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    };
    let layout = &view.layout;
    let mut labels = Vec::new();
    for (row, ((caption, value), &(caption_rect, value_rect))) in
        view.rows.iter().zip(&layout.rows).enumerate()
    {
        labels.push(plain(
            caption,
            caption_rect,
            ShellAlign(ShellAlign::H_RIGHT.0 | ShellAlign::V_CENTER.0),
            SHELL_LABEL_TEXT_RGB,
        ));
        // The name sits after its badge as in the modern player list.
        let value_rect = match (row, view.badge, art) {
            (0, Some(file), Some(art)) => {
                push_icon(&mut paint.icons, art, file, value_rect, 2, scale);
                scale.derive_rect(value_rect, |rect| {
                    let x = 2 + PLAYER_NAME_X - PLAYER_BADGE_X;
                    RectPx::new(rect.x + x, rect.y, rect.w - x, rect.h)
                })
            }
            _ => value_rect,
        };
        labels.push(plain(
            value,
            value_rect,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
    let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    for button in &view.buttons {
        labels.push(plain(
            &button.caption,
            crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                button.rect,
                button.pressed,
            ),
            button_align,
            if button.enabled {
                SHELL_LABEL_TEXT_RGB
            } else {
                shell_paint::SHELL_TEXT_RGB_DISABLED
            },
        ));
    }
    labels.push(plain(
        &view.status,
        layout.status,
        ShellAlign::NONE,
        SHELL_LABEL_TEXT_RGB,
    ));
    let buttons: Vec<ModalButton> = view
        .buttons
        .iter()
        .map(|button| ModalButton {
            rect: button.rect,
            pressed: button.pressed,
            enabled: button.enabled,
        })
        .collect();
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
    paint.controls = draw.sprites;
    paint.texts = draw.text;
    paint
}

/// Your own card's open country list in the shared drop list, over the card.
pub(crate) fn paint_wol_card_countries(
    state: &AppState,
    list: &crate::app::shell_wol::player_card::CountryListView,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let popup = &list.popup;
    push_popup_instances(
        &mut paint.controls,
        atlas,
        popup.rect,
        list.hot_row.and_then(|row| popup.row_rect(row)),
        std::iter::empty(),
        popup.scrollbar().zip(popup.thumb()),
        list.pressed.map(dropdown_scroll_part),
        scale,
        SHELL_DROPDOWN_DEPTH - 0.00002,
    );
    for index in popup.first..popup.first + popup.visible_rows {
        let (Some(row), Some(text)) = (popup.row_rect(index), list.rows.get(index)) else {
            continue;
        };
        paint.texts.push(shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            text,
            rect_to_text_rect(scale.derive_rect(row, |rect| {
                RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
            })),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            [0.0; 2],
            SHELL_DROPDOWN_TEXT_DEPTH,
            scale.factor(),
        ));
    }
    paint
}
