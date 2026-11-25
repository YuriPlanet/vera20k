//! `0x113`'s children and its two popups on the shared control owners: the
//! lists (`0x00618D40`/`0x00619230` with their column cells `0x4A8`), the
//! chat NewEdit, the image buttons (`0x497` faces), the observer checkbox
//! and image, and the Create Game `0xC3` / channel password `0xE1` boxes.
//! The WOL page compositor draws them over its own background and chrome.
//!
//! Residual: the five text sort buttons (`0x765`, `0x766`, `0x76A..0x76C`)
//! paint their caption only; their owner-draw face is not traced.

use super::gui_paint::{ButtonArt, CellRow, ComboArt, GuiStyle, HostRows, Picture};
use super::{SHELL_CONTROL_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint::{self, ModalDepths};
use crate::render::shell_text::ShellTextDraw;
use crate::ui::shell::geom::{RectPx, ShellAlign, ShellScale};
use crate::ui::wol_lobby::*;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

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

/// VERA: a match's Watch tag, light blue to set it apart from the rooms'
/// yellow.
const WATCH_TAG_RGB: [f32; 3] = [0.45, 0.80, 1.0];

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
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    ui: &WolLobbyState,
    layout: &WolLobbyLayout,
    art: Option<&WolPageArt>,
    base_names: &[String; 8],
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    if frontend.skirmish_shell_chrome.is_none() {
        return paint;
    }
    let font = bit_font;
    let scale = layout.page.scale;
    let modern = ui.is_modern();
    let rect_of = |id| layout.control_rect(id).unwrap_or_default();
    // The image buttons show their faces on the page art; the native
    // lobby's observer image shows its eye.
    let image = |id: i32| {
        IMAGE_BUTTONS
            .into_iter()
            .find(|(image, ..)| i32::from(*image) == id)
    };
    let art_of = |id: i32| image(id).map(|_| ButtonArt::Image);
    let observer = ui.observer();
    let picture_of = |id: i32| match image(id) {
        Some((_, released, pressed)) => Some(Picture::Icon { released, pressed }),
        None if id == i32::from(OBSERVER_IMAGE) => Some(Picture::Observer { on: observer }),
        None => None,
    };

    // The observer box, the chat NewEdit (its caret on the page's blink,
    // under the popups too) and the owner-draw lists from the page's tree:
    // the channel and chat rows are the lists' own, the game and user rows
    // their cells. The transcript and the multi-select user list fill no
    // selected row; the user list fills each of its selected rows.
    let rows_of = |id: i32| -> Option<HostRows> {
        Some(match u16::try_from(id).ok()? {
            LOBBIES => (ui.lobby_rows.iter())
                .map(|row| (row.text(base_names), u32::MAX))
                .collect(),
            CHAT_LOG => (ui.chat.lines.iter())
                .map(|line| (String::from_utf16_lossy(&line.text), line.color))
                .collect(),
            _ => return None,
        })
    };
    // The modern list centres a ping under its Ping header, in the row's
    // base pixels.
    let games = ShellListGeometry::new(rect_of(GAMES), 0, 0, ListMetrics::scaled(scale));
    let row = scale.base_rect(games.row(0));
    let ping_x = layout.control_rect(SORT_PING).map(|header| {
        let header = scale.base_rect(header);
        (header.x - row.x, header.w)
    });
    let cells_of = |id: i32| match u16::try_from(id).ok()? {
        GAMES => Some(game_cells(ui, font, ping_x, row.w)),
        PLAYERS => Some(player_cells(ui)),
        _ => None,
    };
    // The headings, the Observe caption and the text sort buttons'
    // captions; the modern games sort by name.
    let text_of = |id: i32| {
        let name = modern && id == i32::from(SORT_ALPHA);
        name.then(|| crate::i18n::tr("VERA:SortName"))
    };
    // The edit that takes the keys: the chat's, or VERA's room search.
    let caret_of = |node| ui.caret_visible() && ui.node(ui.view_edit()) == Some(node);
    // VERA's room search names itself while empty, as the map search does.
    let hint_of = |id: i32| {
        let empty = ui.edit(SEARCH).units.is_empty();
        (id == i32::from(SEARCH) && empty && ime.open_composition().is_none())
            .then(|| (crate::i18n::tr("VERA:LobbySearchHint"), 5))
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        buttons: ButtonArt::Caption,
        rows_of: Some(&rows_of),
        cells_of: Some(&cells_of),
        art,
        caret_of: Some(&caret_of),
        hint_of: Some(&hint_of),
        control_lists: true,
        art_of: Some(&art_of),
        picture_of: Some(&picture_of),
        ..GuiStyle::default()
    };
    let captions = LABELS
        .map(|(id, _)| id)
        .into_iter()
        .chain([OBSERVER_CAPTION]);
    // The online page's view hides the other view's children: they have no
    // window ([`crate::ui::wol_lobby::LobbyView`]); hidden nodes paint
    // nothing. VERA's filter row follows the games.
    let lists = [
        OBSERVER_TOGGLE,
        CHAT,
        LOBBIES,
        GAMES,
        PLAYERS,
        CHAT_LOG,
        SEARCH,
        HIDE_FULL,
        HIDE_LOCKED,
    ];
    let texts = SORT_BUTTONS.iter().map(|(id, _)| *id);
    let faces = IMAGE_BUTTONS.map(|(id, ..)| id);
    let others = (captions.chain(texts).chain(lists))
        .filter(|id| !IMAGE_BUTTONS.iter().any(|(image, ..)| image == id));
    // The image buttons' faces first, the observer image last.
    let eye = (!modern).then_some(OBSERVER_IMAGE);
    for node in (faces.into_iter().chain(others).chain(eye)).filter_map(|id| ui.node(id)) {
        let widget = super::gui_paint::paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            ui.gui(),
            node,
            scale,
            &[],
            style,
        );
        paint
            .controls
            .extend(widget.lane(Material::Controls).copied());
        paint.icons.extend(widget.lane(Material::Icons).copied());
        paint.texts.extend(widget.texts);
    }
    paint
}

/// The game list's cells (`0x00781480`): the name, the ping and the type,
/// password and limit icons; a full game's row greys. The modern list keeps
/// one retail icon, a password game's lock, in the first icon column, and
/// that row's name starts at the next one. A -1 ping is the text "???"; the
/// modern list centres it under its Ping header (`ping_x`, base pixels into
/// a row `row_w` wide) instead of starting it at the retail column, and a
/// match to watch shows its tag there instead, light blue. A room's tag ends
/// the name cell, which stops a gap before the column's centred text; a
/// heading too wide even at the tightest font fit is cut at the font's own
/// width and ends in three dots (GAME.FNT has no "…").
fn game_cells(
    ui: &WolLobbyState,
    font: &crate::render::bit_font::BitFont,
    ping_x: Option<(i32, i32)>,
    row_w: i32,
) -> Vec<CellRow> {
    use crate::render::bit_font::Fit;
    let modern = ui.is_modern();
    let ping_x = ping_x.filter(|_| modern);
    let v = ShellAlign::V_CENTER;
    (ui.game_rows.iter())
        .map(|game| {
            let mut row = CellRow::default();
            let rgb = match game.greyed {
                true => GREYED_RGB,
                false => SHELL_LABEL_TEXT_RGB,
            };
            let start = match game.keyed {
                true => GAME_PASSWORD_X,
                false => GAME_TYPE_X,
            };
            let ping = match game.ping {
                -1 => "???".to_owned(),
                ping => ping.to_string(),
            };
            let tag = game.tag.as_deref().filter(|_| modern);
            let (column, tag) = match game.watch {
                true => (tag.unwrap_or_default(), None),
                false => (ping.as_str(), tag),
            };
            let name_end = match ping_x {
                Some((x, w)) => {
                    let column_w = font.text_width(column) as i32;
                    (x + (w - column_w) / 2 - GAME_TAG_GAP).min(row_w - 2)
                }
                None => GAME_PING_X - 2,
            };
            let tag_w = tag.map_or(0, |tag| (font.text_width(tag) as i32).min(name_end - start));
            let (name_x, name_w) = match (modern, tag) {
                (false, _) => (GAME_NAME_X, GAME_NAME_W),
                (true, None) => (start, name_end - start),
                (true, Some(_)) => (start, name_end - start - tag_w - GAME_TAG_GAP),
            };
            let mut caption = std::borrow::Cow::Borrowed(game.text.as_str());
            if modern {
                let width = name_w.max(1) as u32;
                if !font.fitted(Fit::TIGHTEST, |font| font.text_width(&game.text) <= width) {
                    caption = std::borrow::Cow::Owned(font.fitted(Fit::NONE, |font| {
                        font.truncate_with_ellipsis(&game.text, width, "...")
                    }));
                }
            }
            row.text(name_x, name_w.max(1), &caption, v, rgb);
            if let Some(tag) = tag {
                let (x, right) = (name_end - tag_w, ShellAlign::H_RIGHT | v);
                row.text(x, tag_w.max(1), tag, right, rgb);
            }
            let (x, w, align) = match ping_x {
                Some((x, w)) => (x, w, ShellAlign::H_CENTER | v),
                None => (GAME_PING_X, 0, v),
            };
            let column_rgb = match game.watch && !game.greyed {
                true => WATCH_TAG_RGB,
                false => rgb,
            };
            row.text(x, w, column, align, column_rgb);
            if modern {
                if game.keyed {
                    row.icons.push((GAME_TYPE_X, "wolpriv.pcx"));
                }
                return row;
            }
            row.icons.push((GAME_TYPE_X, game.kind.file()));
            if game.keyed {
                row.icons.push((GAME_PASSWORD_X, "wolpriv.pcx"));
            }
            if let Some(file) = game.limit_icon {
                row.icons.push((GAME_LIMIT_X, file));
            }
            row
        })
        .collect()
}

/// The user list's cells (`0x00783C80`): the name, the clan tag, the ladder
/// rank and the op/voice and high-score icons, each selected row filled
/// (`LBS_MULTIPLESEL`). VERA: the modern list shows only the badge of a
/// row's high-score cells, at its left, and every name as far right of it
/// as the retail list's, so names line up whether or not a row has a badge.
fn player_cells(ui: &WolLobbyState) -> Vec<CellRow> {
    let (v, label) = (ShellAlign::V_CENTER, SHELL_LABEL_TEXT_RGB);
    let selected = ui.selected_players();
    (ui.player_rows.iter().enumerate())
        .map(|(index, player)| {
            let mut row = CellRow {
                filled: selected.contains(&index),
                ..CellRow::default()
            };
            if ui.is_modern() {
                let x = 2 + PLAYER_NAME_X - PLAYER_BADGE_X;
                row.text(x, 0, &player.name, v, label);
                let badge = player
                    .score_icons
                    .iter()
                    .find(|(_, x)| *x == PLAYER_BADGE_X);
                if let Some((file, _)) = badge {
                    row.icons.push((2, *file));
                }
                return row;
            }
            row.text(PLAYER_NAME_X, PLAYER_NAME_W, &player.name, v, label);
            if let Some((tag, _)) = &player.clan {
                row.text(PLAYER_CLAN_X, 0, tag, v, label);
            }
            if let Some(rank) = &player.rank {
                row.text(PLAYER_RANK_X, 0, rank, v, label);
            }
            if let Some((file, _)) = player.icon {
                row.icons.push((PLAYER_ICON_X, file));
            }
            for (file, x) in &player.score_icons {
                row.icons.push((*x, *file));
            }
            row
        })
        .collect()
}

/// `0xC3` or `0xE1` over the covered dialog; `status` is `0xC3`'s own
/// status line text. The `modern` lobby draws them on Red Alert 2's box art,
/// as VERA's updater boxes do; the native lobby keeps gamemd's mode-2 art.
/// `scale` is the lobby page's, so the box grows with the page. The box
/// first, then its widgets: OK and Cancel, the room's limits, the boxes and
/// the edits, and their captions, titles and values in the order they read.
pub(crate) fn paint_wol_lobby_popup(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    ui: &WolLobbyState,
    width: i32,
    height: i32,
    scale: ShellScale,
    status: &str,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let (Some(atlas), Some(popup)) = (frontend.skirmish_shell_chrome.as_ref(), ui.popup.as_ref())
    else {
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
            Some(process_assets.csf_label(key, key))
        }
        id if modern && titled(id.into()).is_some_and(|((control, _), _)| *control == id) => {
            let key = if ui.popup_checked(id) {
                "GUI:Yes"
            } else {
                "GUI:No"
            };
            Some(process_assets.csf_label(key, key))
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
        ..GuiStyle::popup()
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
    let paint_ids = |ids| {
        super::gui_paint::paint_gui_ids(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            ui.gui(),
            popup.root,
            ids,
            scale,
            style,
        )
    };
    let widgets = paint_ids(sprites.collect::<Vec<_>>());
    paint
        .controls
        .extend(widgets.lane(Material::Controls).copied());
    paint.texts = paint_ids(texts.collect::<Vec<_>>()).texts;
    paint
}

/// VERA's player card over the modern lobby, on the online Create Game box:
/// a caption and value per row, the player list's badge beside the name, the
/// four buttons and the status line.
pub(crate) fn paint_wol_player_card(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    view: &crate::app::shell_wol::player_card::PlayerCardView,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let layout = &view.layout;
    paint.controls = online_box(atlas, layout.dialog, scale);
    let Some(lobby) = frontend.wol.as_ref().and_then(|visit| visit.lobby()) else {
        return paint;
    };
    // The name's badge, the rows' captions and values and the status line
    // from their nodes (`app::shell_wol::player_card::sync`).
    if let Some(root) = lobby.ui.card_root() {
        let gui = lobby.ui.gui();
        let badge = i32::from(CARD_BADGE);
        let picture_of = |id: i32| view.badge.filter(|_| id == badge).map(Picture::Badge);
        let style = GuiStyle {
            picture_of: Some(&picture_of),
            ..GuiStyle::default()
        };
        let mut statics = HudPaint::default();
        for &node in gui.tree().children(root).unwrap_or_default() {
            let id = gui.id_of(node).and_then(|id| u16::try_from(id).ok());
            if id.is_some_and(|id| id == CARD_BADGE || (CARD_CAPTIONS..=CARD_STATUS).contains(&id))
            {
                super::gui_paint::paint_gui_into(
                    &mut statics,
                    ime,
                    bit_font,
                    frontend,
                    match_state,
                    process_assets,
                    Some(atlas),
                    gui,
                    node,
                    scale,
                    &[],
                    style,
                );
            }
        }
        paint
            .controls
            .extend(statics.lane(Material::Controls).copied());
        paint.texts = statics.texts;
    }
    // The buttons from the lobby's tree, with the card's captions.
    let text_of = |id: i32| {
        let button = view
            .buttons
            .iter()
            .find(|(button, _)| i32::from(*button) == id);
        button.map(|(_, caption)| caption.clone())
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        ..GuiStyle::popup()
    };
    for node in view.buttons.iter().filter_map(|(id, _)| lobby.ui.node(*id)) {
        let button = super::gui_paint::paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            lobby.ui.gui(),
            node,
            scale,
            &[],
            style,
        );
        paint
            .controls
            .extend(button.lane(Material::Controls).copied());
        paint.texts.extend(button.texts);
    }
    paint
}

/// Your own card's open country list, over the card: its combo's drop
/// list from the lobby's tree.
pub(crate) fn paint_wol_card_countries(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    scale: ShellScale,
) -> WolLobbyPaint {
    let lobby = frontend.wol.as_ref().and_then(|visit| visit.lobby());
    let Some((ui, node)) =
        lobby.and_then(|lobby| Some((&lobby.ui, lobby.ui.node(CARD_COUNTRIES)?)))
    else {
        return WolLobbyPaint::default();
    };
    let style = GuiStyle {
        combos: ComboArt::Host,
        ..GuiStyle::default()
    };
    let mut list = super::gui_paint::paint_gui_styled(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        ui.gui(),
        node,
        scale,
        &[],
        style,
    );
    WolLobbyPaint {
        controls: list.take_lane(Material::Popup),
        icons: Vec::new(),
        texts: list.popup_texts,
    }
}
