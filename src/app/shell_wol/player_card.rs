//! VERA's player card over the online lobby ([`WolPlayerCard`]): its box,
//! what it shows of the player from the lobby's tables, and its buttons.
//! Message opens the page dialog `0xCF` on the player, Add or Remove Buddy
//! goes through the buddy mirror, Full Profile opens the player's page on
//! vera20k.org, and Close, Escape or a press outside the box closes it.

use winit::keyboard::KeyCode;

use super::service::WolServiceRequest;
use crate::app::web_link;
use crate::app::{App, AppState};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{WolPlayerCardLayout, wol_player_card_layout_on};
use crate::ui::wol_lobby::{
    CARD_BUDDY, CARD_CLOSE, CARD_MESSAGE, CARD_PROFILE, PLAYER_BADGE_X, WolCardAction,
    WolPlayerCard,
};

/// What the card shows, for the painter.
pub(crate) struct PlayerCardView {
    pub(crate) layout: WolPlayerCardLayout,
    /// The player list's badge for the player, beside the name.
    pub(crate) badge: Option<&'static str>,
    /// Each shown row's caption and value; the first holds the name.
    pub(crate) rows: Vec<(String, String)>,
    pub(crate) buttons: Vec<CardButton>,
    /// The hovered button's help, in the box's status line.
    pub(crate) status: String,
}

pub(crate) struct CardButton {
    pub(crate) rect: RectPx,
    pub(crate) caption: String,
    pub(crate) enabled: bool,
    pub(crate) pressed: bool,
}

/// The open card; a lobby popup over it takes the input first.
fn card(state: &AppState) -> Option<&WolPlayerCard> {
    let lobby = state.frontend.wol.as_ref()?.lobby()?;
    lobby.ui.card.as_ref().filter(|_| lobby.ui.popup.is_none())
}

fn card_mut(state: &mut AppState) -> Option<&mut WolPlayerCard> {
    let lobby = state.frontend.wol.as_mut()?.lobby_mut()?;
    if lobby.ui.popup.is_some() {
        return None;
    }
    lobby.ui.card.as_mut()
}

fn close(state: &mut AppState) {
    if let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby_mut())
    {
        lobby.ui.card = None;
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

fn label(state: &AppState, key: &str) -> String {
    App::csf_label(state, key, key)
}

pub(crate) fn layout(state: &AppState) -> WolPlayerCardLayout {
    wol_player_card_layout_on(
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
        super::lobby::popup_scale(state),
    )
}

/// Message and Add or Remove Buddy, then Full Profile and Close.
fn buttons(layout: &WolPlayerCardLayout) -> [(u16, RectPx); 4] {
    [
        (CARD_MESSAGE, layout.message),
        (CARD_BUDDY, layout.buddy),
        (CARD_PROFILE, layout.profile),
        (CARD_CLOSE, layout.close),
    ]
}

fn control_at(state: &AppState) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    buttons(&layout(state))
        .into_iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(id, _)| id)
}

/// The buddy mirror's spelling of the player, when they are a buddy.
fn buddy(state: &AppState, name: &str) -> Option<String> {
    state
        .frontend
        .wol_buddies
        .names()
        .iter()
        .find(|known| known.eq_ignore_ascii_case(name))
        .cloned()
}

fn enabled(state: &AppState, name: &str, control: u16) -> bool {
    let local = &state.frontend.wol_account.nickname;
    match control {
        CARD_MESSAGE => !name.eq_ignore_ascii_case(local),
        CARD_BUDDY => {
            buddy(state, name).is_some() || state.frontend.wol_buddies.valid_new(name, local)
        }
        CARD_PROFILE => web_link::profile_url(name).is_some(),
        _ => true,
    }
}

/// A press on a modern player row opened `name`'s card. The lobby asks for
/// a locale it has not heard; gamemd's locale queue `0xB765F8` is not ported.
pub(super) fn opened(state: &mut AppState, name: &str) {
    let unheard =
        state.frontend.wol.as_ref().is_some_and(|visit| {
            visit.api && !visit.locales.contains_key(&name.to_ascii_lowercase())
        });
    if unheard {
        super::service::request(
            state,
            WolServiceRequest::RequestLocales {
                names: vec![name.to_owned()],
            },
        );
    }
    state.platform.window.request_redraw();
}

/// The pointer over the open card: its buttons light, the lobby's do not.
pub(super) fn cursor_moved(state: &mut AppState) -> bool {
    let over = control_at(state);
    let Some(card) = card_mut(state) else {
        return false;
    };
    card.buttons.hovered = over;
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
    true
}

/// The open card takes every press: a button acts when released over
/// itself, and a press outside the box closes the card.
pub(super) fn mouse(state: &mut AppState, pressed: bool) -> bool {
    let Some(name) = card(state).map(|card| card.name.clone()) else {
        return false;
    };
    let over = control_at(state);
    if pressed {
        let (x, y) = state.match_state.input.cursor_px();
        if !layout(state).dialog.contains(x, y) {
            close(state);
            return true;
        }
        let control = over.filter(|&control| enabled(state, &name, control));
        if let Some(card) = card_mut(state) {
            card.buttons.press(control);
        }
        if control.is_some() {
            App::play_main_menu_button_sound(state);
        }
    } else if let Some(action) = card_mut(state)
        .and_then(|card| card.buttons.release(over))
        .and_then(WolCardAction::of)
    {
        act(state, name, action);
    }
    state.platform.window.request_redraw();
    true
}

fn act(state: &mut AppState, name: String, action: WolCardAction) {
    match action {
        WolCardAction::Message => {
            close(state);
            super::page::open_page_dialog_to(state, &name);
        }
        WolCardAction::Buddy => match buddy(state, &name) {
            Some(known) => super::buddies::remove(state, &known),
            None => super::buddies::add(state, name),
        },
        WolCardAction::Profile => {
            if let Some(url) = web_link::profile_url(&name) {
                web_link::open_in_browser(&url);
            }
        }
        WolCardAction::Close => close(state),
    }
}

/// Escape closes the open card, which keeps every key from the chat edit.
pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>) -> bool {
    if card(state).is_none() {
        return false;
    }
    if code == Some(KeyCode::Escape) {
        close(state);
    }
    true
}

/// The hovered button's help while the card is open.
pub(super) fn status_help(state: &AppState) -> Option<String> {
    let card = card(state)?;
    let key = match card.buttons.hovered {
        Some(CARD_MESSAGE) => "VERA:MessageHelp",
        Some(CARD_BUDDY) if buddy(state, &card.name).is_some() => "VERA:RemoveBuddyHelp",
        Some(CARD_BUDDY) => "VERA:AddBuddyHelp",
        Some(CARD_PROFILE) => "VERA:FullProfileHelp",
        _ => return Some(String::new()),
    };
    Some(crate::i18n::fill(
        &label(state, key),
        &[("name", &card.name)],
    ))
}

/// The open card's rows and buttons: the name with its badge, the clan, the
/// country, the season's rank and OS or Unranked, and the season's wins and
/// losses.
pub(crate) fn view(state: &AppState) -> Option<PlayerCardView> {
    let card = card(state)?;
    let lobby = state.frontend.wol.as_ref()?.lobby()?;
    let name = card.name.as_str();
    let label = |key: &str| label(state, key);
    let badge = lobby
        .ui
        .player_rows
        .iter()
        .find(|row| row.name == name)
        .and_then(|row| row.score_icons.iter().find(|(_, x)| *x == PLAYER_BADGE_X))
        .map(|(file, _)| *file);
    let mut rows = vec![(label("VERA:Username"), name.to_owned())];
    let member = super::lobby::members(state)
        .into_iter()
        .find(|member| member.name == name);
    let clan =
        member.and_then(
            |member| match (member.clan_name.is_empty(), member.clan_tag.is_empty()) {
                (true, true) => None,
                (false, true) => Some(member.clan_name),
                (true, false) => Some(format!("[{}]", member.clan_tag)),
                (false, false) => Some(format!("{} [{}]", member.clan_name, member.clan_tag)),
            },
        );
    rows.extend(clan.map(|clan| (label("GUI:Clan"), clan)));
    rows.extend(
        super::lobby::locale_name(state, name).map(|country| (label("VERA:Country"), country)),
    );
    let ladder = state
        .frontend
        .wol_lobby_globals
        .ladder
        .get(&name.to_ascii_lowercase());
    let season = match ladder {
        Some(record) => crate::i18n::fill(
            &label("VERA:SeasonRank"),
            &[("rank", &record.rank), ("os", &record.points)],
        ),
        None => label("TXT_UNRANKED"),
    };
    rows.push((label("VERA:Season"), season));
    if let Some(record) = ladder {
        rows.push((
            label("VERA:Record"),
            crate::i18n::fill(
                &label("VERA:WonLost"),
                &[("wins", &record.wins), ("losses", &record.losses)],
            ),
        ));
    }
    let layout = layout(state);
    let buddy = buddy(state, name).is_some();
    let buttons = buttons(&layout)
        .into_iter()
        .map(|(id, rect)| CardButton {
            rect,
            caption: label(match id {
                CARD_MESSAGE => "VERA:Message",
                CARD_BUDDY if buddy => "GUI:RemoveBuddy",
                CARD_BUDDY => "GUI:AddBuddy",
                CARD_PROFILE => "VERA:FullProfile",
                _ => "GUI:Close",
            }),
            enabled: enabled(state, name, id),
            pressed: card.buttons.is_pressed(id),
        })
        .collect();
    Some(PlayerCardView {
        layout,
        badge,
        rows,
        buttons,
        status: status_help(state).unwrap_or_default(),
    })
}
