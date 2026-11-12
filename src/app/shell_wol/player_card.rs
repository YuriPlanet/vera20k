//! VERA's player card over the online lobby ([`WolPlayerCard`]): its box,
//! what it shows of the player from the lobby's tables, and its buttons.
//! Message opens the page dialog `0xCF` on the player, Add or Remove Buddy
//! goes through the buddy mirror, Full Profile opens the player's page on
//! vera20k.org, and Close, Escape or a press outside the box closes it.
//! Your own card's Change Country drops `0xFE`'s country list below the
//! Country row; a pick is kept in VERA.INI and sent to the lobby.

use winit::keyboard::KeyCode;

use super::service::WolServiceRequest;
use crate::app::web_link;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::combobox::{ComboPopup, ComboPress};
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::list::ListScrollPart;
use crate::ui::shell::modal::{WolPlayerCardLayout, wol_player_card_layout_on};
use crate::ui::wol_lobby::{
    CARD_BUDDY, CARD_CLOSE, CARD_COUNTRY, CARD_COUNTRY_ROWS, CARD_MESSAGE, CARD_PROFILE,
    WolCardAction, WolPlayerCard, badge_name_key,
};
use crate::ui::wol_persona::{LOCALE_COUNT, locale_rows};
use crate::util::time::Instant;

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
    pub(crate) countries: Option<CountryListView>,
}

/// The open country list, for the painter.
pub(crate) struct CountryListView {
    pub(crate) popup: ComboPopup,
    pub(crate) rows: Vec<String>,
    pub(crate) hot_row: Option<usize>,
    pub(crate) pressed: Option<ListScrollPart>,
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

fn own(state: &AppState, name: &str) -> bool {
    name.eq_ignore_ascii_case(&state.frontend.wol_account.nickname)
}

/// Message, or Change Country on your own card, and Add or Remove Buddy,
/// then Full Profile and Close.
fn buttons(layout: &WolPlayerCardLayout, own: bool) -> [(u16, RectPx); 4] {
    [
        (
            if own { CARD_COUNTRY } else { CARD_MESSAGE },
            layout.message,
        ),
        (CARD_BUDDY, layout.buddy),
        (CARD_PROFILE, layout.profile),
        (CARD_CLOSE, layout.close),
    ]
}

fn control_at(state: &AppState, own: bool) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    buttons(&layout(state), own)
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
        CARD_COUNTRY => state.frontend.wol.as_ref().is_some_and(|visit| visit.api),
        CARD_BUDDY => {
            buddy(state, name).is_some() || state.frontend.wol_buddies.valid_new(name, local)
        }
        CARD_PROFILE => web_link::profile_url(name).is_some(),
        _ => true,
    }
}

/// The player's clan, as the card's Clan row shows it.
fn clan(state: &AppState, name: &str) -> Option<String> {
    let member = super::lobby::members(state)
        .into_iter()
        .find(|member| member.name == name)?;
    match (member.clan_name.is_empty(), member.clan_tag.is_empty()) {
        (true, true) => None,
        (false, true) => Some(member.clan_name),
        (true, false) => Some(format!("[{}]", member.clan_tag)),
        (false, false) => Some(format!("{} [{}]", member.clan_name, member.clan_tag)),
    }
}

/// The open country list, below your Country row's value.
fn country_list(state: &AppState) -> Option<ComboPopup> {
    let card = card(state).filter(|card| card.countries_open())?;
    let row = 1 + usize::from(clan(state, &card.name).is_some());
    let (_, value) = layout(state).rows.get(row).copied()?;
    Some(ComboPopup::new(
        value,
        super::lobby::popup_scale(state),
        card.countries.len(),
        CARD_COUNTRY_ROWS,
        card.country_list.top_index,
    ))
}

/// The country the lobby holds for you, 0 when it has none.
fn own_locale(state: &AppState) -> i32 {
    let nickname = state.frontend.wol_account.nickname.to_ascii_lowercase();
    state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.locales.get(&nickname))
        .copied()
        .unwrap_or(0)
}

/// Change Country: `0xFE`'s rows, every `WOL:Locale%02d`, with yours lit.
fn open_countries(state: &mut AppState) {
    let rows = locale_rows(
        (0..LOCALE_COUNT).map(|locale| (locale, label(state, &format!("WOL:Locale{locale:02}")))),
    );
    let locale = own_locale(state);
    if let Some(card) = card_mut(state) {
        card.open_countries(rows, locale);
    }
    state.frontend.shell_status_line.repaint();
}

/// Your pick: VERA.INI keeps it for later sign-ins, and the lobby has it at
/// once.
fn choose_country(state: &mut AppState, locale: i32) {
    let profile = &mut state.persistence.options_profile;
    if profile.online_locale != locale {
        profile.online_locale = locale;
        if let Some(config) = state.platform.game_config.as_ref()
            && let Err(error) = profile.commit_online_locale(config.paths.settings_dir())
        {
            log::warn!("Could not save the online country: {error}");
        }
    }
    super::service::send_locale(state, locale);
}

/// The open list captures the mouse as ComboDropWin does: its scrollbar
/// takes a press, and a pick or a press anywhere else closes it.
fn countries_mouse(state: &mut AppState, list: ComboPopup, pressed: bool) {
    let (x, y) = state.match_state.input.cursor_px();
    let Some(card) = card_mut(state) else {
        return;
    };
    if !pressed {
        card.country_list.cancel_pointer();
        return;
    }
    let press = card.country_list.press_list(list, x, y, Instant::now());
    let picked = match press {
        ComboPress::Pick(row) => card.countries.get(row).map(|row| row.locale),
        _ => None,
    };
    if !card.countries_open() {
        card.close_countries();
    }
    if let Some(cue) = press.cue() {
        App::play_launcher_options_cue(state, LauncherCue::from(cue));
    }
    if let Some(locale) = picked {
        choose_country(state, locale);
    }
    state.frontend.shell_status_line.repaint();
}

/// The country list's scrollbar repeats.
pub(super) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let list = country_list(state)?;
    let (x, y) = state.match_state.input.cursor_px();
    let card = card_mut(state)?;
    let moved = card.country_list.scroll.poll(
        list.scroll_geometry(),
        &mut card.country_list.top_index,
        x,
        y,
        Instant::now(),
    );
    let repeat = card.country_list.scroll.repeat_at();
    if moved {
        state.platform.window.request_redraw();
    }
    repeat
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
/// The open country list lights its rows and drags its thumb instead.
pub(super) fn cursor_moved(state: &mut AppState) -> bool {
    let Some(own) = card(state).map(|card| own(state, &card.name)) else {
        return false;
    };
    let list = country_list(state);
    let over = control_at(state, own).filter(|_| list.is_none());
    let (x, y) = state.match_state.input.cursor_px();
    let Some(card) = card_mut(state) else {
        return false;
    };
    if let Some(list) = list {
        card.country_list.pointer_moved(list, x, y);
    }
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
    if let Some(list) = country_list(state) {
        countries_mouse(state, list, pressed);
        state.platform.window.request_redraw();
        return true;
    }
    let over = control_at(state, own(state, &name));
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
        WolCardAction::Country => open_countries(state),
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

/// Escape closes the open country list, else the card, which keeps every
/// key from the chat edit.
pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>) -> bool {
    let Some(open) = card(state).map(WolPlayerCard::countries_open) else {
        return false;
    };
    if code == Some(KeyCode::Escape) {
        if open {
            if let Some(card) = card_mut(state) {
                card.close_countries();
            }
            state.platform.window.request_redraw();
        } else {
            close(state);
        }
    }
    true
}

/// The hovered button's help while the card is open, else the help of the
/// row under the pointer.
pub(super) fn status_help(state: &AppState) -> Option<String> {
    let card = card(state)?;
    let key = match card.buttons.hovered {
        Some(CARD_MESSAGE) => "VERA:MessageHelp",
        Some(CARD_COUNTRY) => "VERA:ChangeCountryHelp",
        Some(CARD_BUDDY) if buddy(state, &card.name).is_some() => "VERA:RemoveBuddyHelp",
        Some(CARD_BUDDY) => "VERA:AddBuddyHelp",
        Some(CARD_PROFILE) => "VERA:FullProfileHelp",
        _ if card.countries_open() => return Some(String::new()),
        _ => {
            let (x, y) = state.match_state.input.cursor_px();
            let help = layout(state)
                .rows
                .iter()
                .zip(rows(state, &card.name))
                .find(|((caption, value), _)| {
                    (caption.x..value.x + value.w).contains(&x)
                        && (value.y..value.y + value.h).contains(&y)
                })
                .and_then(|(_, row)| row.help);
            return Some(help.unwrap_or_default());
        }
    };
    Some(crate::i18n::fill(
        &label(state, key),
        &[("name", &card.name)],
    ))
}

/// One row of the card: its caption, its value and the help it shows under
/// the pointer.
struct CardRow {
    caption: String,
    value: String,
    help: Option<String>,
}

/// The card's rows: the name with its badge, the clan, the country (on your
/// own card always, Unknown without one), the season's rank and OS or
/// Unranked, and the season's wins and losses. The badge and the season
/// explain themselves under the pointer.
fn rows(state: &AppState, name: &str) -> Vec<CardRow> {
    let label = |key: &str| label(state, key);
    let row = |caption: &str, value: String| CardRow {
        caption: label(caption),
        value,
        help: None,
    };
    let badge_help = super::lobby::player_badge(state, name)
        .and_then(badge_name_key)
        .map(|key| crate::i18n::fill(&label("VERA:BadgeHelp"), &[("badge", &label(key))]));
    let mut rows = vec![CardRow {
        help: badge_help,
        ..row("VERA:Username", name.to_owned())
    }];
    rows.extend(clan(state, name).map(|clan| row("GUI:Clan", clan)));
    let country = super::lobby::locale_name(state, name)
        .or_else(|| own(state, name).then(|| label("WOL:Locale00")));
    rows.extend(country.map(|country| row("VERA:Country", country)));
    let ladder = state
        .frontend
        .wol_lobby_globals
        .ladder
        .get(&name.to_ascii_lowercase());
    let season = match ladder {
        Some(record) => crate::i18n::fill(
            &label("VERA:SeasonRank"),
            &[("rank", &record.rank), ("os", &record.os_text())],
        ),
        None => label("TXT_UNRANKED"),
    };
    rows.push(CardRow {
        help: Some(label("VERA:SeasonHelp")),
        ..row("VERA:Season", season)
    });
    if let Some(record) = ladder {
        rows.push(row(
            "VERA:Record",
            crate::i18n::fill(
                &label("VERA:WonLost"),
                &[("wins", &record.wins), ("losses", &record.losses)],
            ),
        ));
    }
    rows
}

/// The open card's rows and buttons.
pub(crate) fn view(state: &AppState) -> Option<PlayerCardView> {
    let card = card(state)?;
    let name = card.name.as_str();
    let own = own(state, name);
    let label = |key: &str| label(state, key);
    let badge = super::lobby::player_badge(state, name);
    let rows = rows(state, name)
        .into_iter()
        .map(|row| (row.caption, row.value))
        .collect();
    let layout = layout(state);
    let buddy = buddy(state, name).is_some();
    let buttons = buttons(&layout, own)
        .into_iter()
        .map(|(id, rect)| CardButton {
            rect,
            caption: label(match id {
                CARD_MESSAGE => "VERA:Message",
                CARD_COUNTRY => "VERA:ChangeCountry",
                CARD_BUDDY if buddy => "GUI:RemoveBuddy",
                CARD_BUDDY => "GUI:AddBuddy",
                CARD_PROFILE => "VERA:FullProfile",
                _ => "GUI:Close",
            }),
            enabled: enabled(state, name, id),
            pressed: card.buttons.is_pressed(id),
        })
        .collect();
    let countries = country_list(state).map(|popup| CountryListView {
        popup,
        rows: card.countries.iter().map(|row| row.text.clone()).collect(),
        hot_row: card.country_list.hot_row,
        pressed: card.country_list.scroll.pressed_part(),
    });
    Some(PlayerCardView {
        layout,
        badge,
        rows,
        buttons,
        status: status_help(state).unwrap_or_default(),
        countries,
    })
}
