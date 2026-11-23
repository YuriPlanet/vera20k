//! VERA's player card over the online lobby ([`WolPlayerCard`]): its box,
//! what it shows of the player from the lobby's tables, and its buttons.
//! Message opens the page dialog `0xCF` on the player, Add or Remove Buddy
//! goes through the buddy mirror, Full Profile opens the player's page on
//! vera20k.org, and Close, Escape or a press outside the box closes it.
//! Your own card's Change Country drops `0xFE`'s country list below the
//! Country row; a pick is kept in VERA.INI and sent to the lobby.

use winit::keyboard::KeyCode;

use super::service::WolServiceRequest;
use crate::app::AppState;
use crate::app::web_link;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{WolPlayerCardLayout, wol_player_card_layout_on};
use crate::ui::wol_lobby::{
    CARD_BUDDY, CARD_CLOSE, CARD_COUNTRY, CARD_MESSAGE, CARD_PROFILE, CARD_ROWS, WolCardAction,
    WolLobbyState, WolPlayerCard, badge_name_key,
};
use crate::ui::wol_persona::{LOCALE_COUNT, locale_rows};

/// What the card shows, for the painter.
pub(crate) struct PlayerCardView {
    pub(crate) layout: WolPlayerCardLayout,
    /// The player list's badge for the player, beside the name.
    pub(crate) badge: Option<&'static str>,
    /// Each shown row's caption and value; the first holds the name.
    pub(crate) rows: Vec<(String, String)>,
    /// Each shown button's id and caption; the lobby's tree holds the
    /// rest ([`sync`]).
    pub(crate) buttons: Vec<(u16, String)>,
    /// The hovered button's help, in the box's status line.
    pub(crate) status: String,
    /// The open country list.
    pub(crate) countries: Option<vera20k_gui::ComboDropDown>,
}

/// The lobby's widgets while its card is open; a lobby popup over it takes
/// the input first.
fn card_ui(state: &AppState) -> Option<&WolLobbyState> {
    let ui = &state.frontend.wol.as_ref()?.lobby()?.ui;
    ui.card.as_ref().filter(|_| ui.popup.is_none())?;
    Some(ui)
}

fn card(state: &AppState) -> Option<&WolPlayerCard> {
    card_ui(state)?.card.as_ref()
}

fn card_ui_mut(state: &mut AppState) -> Option<&mut WolLobbyState> {
    let ui = &mut state.frontend.wol.as_mut()?.lobby_mut()?.ui;
    ui.card.as_ref().filter(|_| ui.popup.is_none())?;
    Some(ui)
}

fn close(state: &mut AppState) {
    if let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby_mut())
    {
        lobby.ui.close_card();
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
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

/// Puts the open card's buttons on its box, the first one by whose card it
/// is, each taking a press while it can act.
pub(super) fn sync(state: &mut AppState) {
    let Some(name) = card(state).map(|card| card.name.clone()) else {
        return;
    };
    let placed: Vec<_> = buttons(&layout(state), own(state, &name))
        .into_iter()
        .map(|(id, rect)| (id, rect, enabled(state, &name, id)))
        .collect();
    let value = country_value(state);
    let layout = layout(state);
    // Each shown row hovers from its caption to its value's end.
    let rows: Vec<RectPx> = (layout.rows.iter().take(rows(state, &name).len()))
        .map(|(caption, value)| {
            RectPx::new(caption.x, value.y, value.x + value.w - caption.x, value.h)
        })
        .collect();
    if let Some(ui) = card_ui_mut(state) {
        ui.sync_card(layout.dialog, &rows, |id| {
            placed
                .iter()
                .find(|(known, ..)| *known == id)
                .map(|&(_, rect, on)| (rect, on))
        });
        if let Some(value) = value.filter(|_| ui.countries_open()) {
            ui.place_countries(value);
        }
    }
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
        .iter()
        .find(|member| member.name == name)?;
    match (member.clan_name.is_empty(), member.clan_tag.is_empty()) {
        (true, true) => None,
        (false, true) => Some(member.clan_name.clone()),
        (true, false) => Some(format!("[{}]", member.clan_tag)),
        (false, false) => Some(format!("{} [{}]", member.clan_name, member.clan_tag)),
    }
}

/// Your Country row's value, which the country list hangs below.
fn country_value(state: &AppState) -> Option<RectPx> {
    let card = card(state)?;
    let row = 1 + usize::from(clan(state, &card.name).is_some());
    let (_, value) = layout(state).rows.get(row).copied()?;
    Some(value)
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
        (0..LOCALE_COUNT).map(|locale| (locale, state.csf_text(&format!("WOL:Locale{locale:02}")))),
    );
    let locale = own_locale(state);
    if let Some(value) = country_value(state)
        && let Some(ui) = card_ui_mut(state)
    {
        ui.open_countries(rows, locale, value);
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

/// Row `row` of your country list picked.
pub(super) fn country_picked(state: &mut AppState, row: usize) {
    if let Some(locale) = card_ui(state).and_then(|ui| ui.country_locale(row)) {
        choose_country(state, locale);
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

/// A press outside the box closes the card (the card's widgets report it;
/// the open country list takes every press first, ComboDropWin's capture).
pub(super) fn dismissed(state: &mut AppState) {
    close(state);
}

/// Card button `id` released.
pub(super) fn button(state: &mut AppState, id: u16) {
    if let (Some(name), Some(action)) = (
        card(state).map(|card| card.name.clone()),
        WolCardAction::of(id),
    ) {
        act(state, name, action);
    }
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

/// The card keeps every key from the chat edit but Escape, which its
/// widgets take: it closes the open country list, else Close (its Cancel).
pub(super) fn key_press(state: &AppState, code: Option<KeyCode>) -> bool {
    card_ui(state).is_some() && code != Some(KeyCode::Escape)
}

/// The hovered button's help while the card is open, else the help of the
/// row under the pointer.
pub(super) fn status_help(state: &AppState) -> Option<String> {
    let ui = card_ui(state)?;
    let card = ui.card.as_ref()?;
    let open = ui.countries_open();
    let key = match ui.hovered().filter(|_| !open) {
        Some(CARD_MESSAGE) => "VERA:MessageHelp",
        Some(CARD_COUNTRY) => "VERA:ChangeCountryHelp",
        Some(CARD_BUDDY) if buddy(state, &card.name).is_some() => "VERA:RemoveBuddyHelp",
        Some(CARD_BUDDY) => "VERA:AddBuddyHelp",
        Some(CARD_PROFILE) => "VERA:FullProfileHelp",
        _ if open => return Some(String::new()),
        // The row under the pointer gives its own help.
        hovered => {
            let row = hovered.and_then(|id| id.checked_sub(CARD_ROWS));
            let help = row
                .and_then(|row| rows(state, &card.name).into_iter().nth(usize::from(row)))
                .and_then(|row| row.help);
            return Some(help.unwrap_or_default());
        }
    };
    Some(crate::i18n::fill(
        &state.csf_text(key),
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
    let label = |key: &str| state.csf_text(key);
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
    let label = |key: &str| state.csf_text(key);
    let badge = super::lobby::player_badge(state, name);
    let rows = rows(state, name)
        .into_iter()
        .map(|row| (row.caption, row.value))
        .collect();
    let layout = layout(state);
    let buddy = buddy(state, name).is_some();
    let buttons = buttons(&layout, own)
        .into_iter()
        .map(|(id, _)| {
            let key = match id {
                CARD_MESSAGE => "VERA:Message",
                CARD_COUNTRY => "VERA:ChangeCountry",
                CARD_BUDDY if buddy => "GUI:RemoveBuddy",
                CARD_BUDDY => "GUI:AddBuddy",
                CARD_PROFILE => "VERA:FullProfile",
                _ => "GUI:Close",
            };
            (id, label(key))
        })
        .collect();
    let countries = card_ui(state)?.countries_drop_down();
    Some(PlayerCardView {
        layout,
        badge,
        rows,
        buttons,
        status: status_help(state).unwrap_or_default(),
        countries,
    })
}
