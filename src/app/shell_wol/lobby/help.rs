//! The status help of the lobby lists: the user and game help (`0x00781090`,
//! `0x00780E30`), player badges and locale names.

use super::WolLobby;
use crate::app::AppState;
use crate::ui::wol_lobby::*;

use super::{cooperative_campaign, lobby};

/// The status help of what the pointer is over: the table's key, or a
/// list's `0x4E9` answer (`0x00780E30` for a game, `0x00781090` for a user).
pub(crate) fn status_help(state: &AppState) -> String {
    let Some(lobby) = lobby(state) else {
        return String::new();
    };
    let ui = &lobby.ui;
    if let Some(popup) = ui.popup.as_ref() {
        return (popup.create.is_some().then(|| ui.hovered()).flatten())
            .and_then(create_tooltip_key)
            .map(|key| state.csf_text(key))
            .unwrap_or_default();
    }
    if let Some(help) = super::player_card::status_help(state) {
        return help;
    }
    // VERA: a list's row under the pointer gives its own help, also as the
    // list refills under a resting pointer.
    let (x, y) = state.match_state.input.cursor_px();
    let row = |list| ui.row_at(list, x, y);
    match ui.hovered() {
        Some(GAMES) => match row(GAMES).and_then(|row| ui.game_rows.get(row)) {
            Some(row) if ui.is_modern() => modern_game_help(state, lobby, row).unwrap_or_default(),
            Some(row) => game_help(state, row),
            _ => state.csf_text("STT:LobbyListGames"),
        },
        Some(VIEW) if ui.is_modern() => state.csf_text(ui.view_help_key()),
        Some(PLAYERS) => match row(PLAYERS).and_then(|row| ui.player_rows.get(row)) {
            None => state.csf_text("STT:LobbyListUsers"),
            Some(row) if ui.is_modern() => modern_user_help(state, row),
            Some(row) => {
                let member = lobby.members.iter().find(|member| member.name == row.name);
                user_help(state, &row.name, member)
            }
        },
        Some(control) => tooltip_key(control, ui.page_spec())
            .map(|key| state.csf_text(key))
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// VERA's help for a modern game row, the room or match its heading names:
/// its sides' seats, spectators, AI and how full it is, on the status line's
/// one line. The team numbers retain their original slots, including gaps.
/// Room spectators have seats; a running match's viewers connect separately.
fn modern_game_help(state: &AppState, lobby: &WolLobby, row: &WolGameRow) -> Option<String> {
    let (summary, capacity, campaign) = if row.watch {
        let watch = lobby.matches.get(row.record)?;
        (
            watch.summary.as_ref(),
            crate::i18n::tr_fill(
                "VERA:LobbyWatchCapacity",
                &[("count", &watch.watchers), ("max", &watch.max_watchers)],
            ),
            None,
        )
    } else {
        let room = lobby.games.get(row.record)?;
        let campaign = cooperative_campaign(state, room)
            .and_then(|campaign| {
                state
                    .frontend
                    .multiplayer_shell_runtime
                    .cooperative_campaign_name(campaign)
            })
            .map(|key| state.csf_text(key));
        (
            room.summary.as_ref(),
            crate::i18n::tr_fill(
                "VERA:LobbyRoomCapacity",
                &[("count", &room.users), ("max", &room.max_users)],
            ),
            campaign,
        )
    };
    let mut details = Vec::new();
    if let Some(summary) = summary {
        if let Some(teams) = &summary.teams {
            let occupied = teams.iter().filter(|&&count| count > 0).count();
            if occupied > 2 {
                // The heading already gives the matchup. With more sides,
                // the line keeps room for spectators and occupancy.
                details.push(format!("{}: {occupied}", crate::i18n::tr("VERA:Teams")));
            } else {
                for (team, count) in teams.iter().enumerate().filter(|(_, count)| **count > 0) {
                    details.push(crate::i18n::tr_count(
                        "VERA:LobbyTeamSeats",
                        *count,
                        &[("team", &(team + 1))],
                    ));
                }
            }
        }
        if let Some(count) = summary.observers {
            details.push(crate::i18n::tr_count("VERA:LobbySpectators", count, &[]));
        }
        if let Some(count) = summary.computers {
            details.push(crate::i18n::tr_fill(
                "VERA:LobbyAiCount",
                &[("count", &count)],
            ));
        }
    }
    details.push(capacity);
    details.extend(campaign);
    Some(details.join(" - "))
}

/// VERA's help for a modern player row ([`badge_help`]); the name alone
/// without a badge.
fn modern_user_help(state: &AppState, row: &WolPlayerRow) -> String {
    row.score_icons
        .iter()
        .find(|(_, x)| *x == PLAYER_BADGE_X)
        .and_then(|(file, _)| badge_help(state, &row.name, file))
        .unwrap_or_else(|| row.name.clone())
}

/// VERA: the badge `name`'s high-score record gives, which the modern player
/// list, the player card and VERA's online room show.
pub(crate) fn player_badge(state: &AppState, name: &str) -> Option<&'static str> {
    state
        .frontend
        .wol_lobby_globals
        .high_scores
        .get(&name.to_ascii_lowercase())
        .map(|record| crate::ui::wol_lobby::badge(record.points))
}

/// VERA's help for a player with the `badge` PCX, in the modern player list
/// and VERA's online room: the name, then the badge's name and the season's
/// rank and OS as the player card shows them, or Unranked.
pub(crate) fn badge_help(state: &AppState, name: &str, badge: &str) -> Option<String> {
    let badge = state.csf_text(badge_name_key(badge)?);
    let ladder = &state.frontend.wol_lobby_globals.ladder;
    Some(match ladder.get(&name.to_ascii_lowercase()) {
        Some(record) => crate::i18n::fill(
            &state.csf_text("VERA:PlayerHelp"),
            &[
                ("name", &name),
                ("badge", &badge),
                ("rank", &record.rank),
                ("os", &record.os_text()),
            ],
        ),
        None => crate::i18n::fill(
            &state.csf_text("VERA:PlayerHelpUnranked"),
            &[("name", &name), ("badge", &badge)],
        ),
    })
}

/// `0x00780A90`: `"<nick> : "`, then `TXT_RANK` and the ladder rank or
/// `TXT_UNRANKED`, then `": "` and the clan name (`+0x2E`, else the tag
/// `+0x57`) when there is one, then `" : "` and `WOL:Locale%02d` for a
/// known nonzero locale. Both tables are looked up by the ASCII-lowered
/// name (`0x007B5B30`). A player row passes its user record; a game row's
/// record (`0x00780E30`) holds only the host's name, so no clan.
/// RESIDUAL: the lobby's locale queue `0xB765F8` (filled at `0x007A73C8`
/// and `0x007A6495`) is not ported; only the user's own locale request and
/// VERA's player card fill the locale table.
fn user_help(state: &AppState, name: &str, member: Option<&WolMember>) -> String {
    if state.frontend.wol.is_none() {
        return String::new();
    }
    let key = name.to_ascii_lowercase();
    let mut help = format!("{name} : ");
    match state.frontend.wol_lobby_globals.ladder.get(&key) {
        Some(record) => {
            help.push_str(&state.csf_text("TXT_RANK"));
            help.push(' ');
            help.push_str(&record.rank.to_string());
        }
        None => help.push_str(&state.csf_text("TXT_UNRANKED")),
    }
    if let Some(member) = member {
        let clan = if member.clan_name.is_empty() {
            &member.clan_tag
        } else {
            &member.clan_name
        };
        if !clan.is_empty() {
            help.push_str(": ");
            help.push_str(clan);
        }
    }
    if let Some(locale) = locale_name(state, name) {
        help.push_str(" : ");
        help.push_str(&locale);
    }
    help
}

/// `WOL:Locale%02d` of the user's known nonzero locale, by the ASCII-lowered
/// name, while the WOLAPI object exists (`0x00780A90`'s locale part).
pub(in crate::app::shell_wol) fn locale_name(state: &AppState, name: &str) -> Option<String> {
    let visit = state.frontend.wol.as_ref().filter(|visit| visit.api)?;
    let locale = *visit
        .locales
        .get(&name.to_ascii_lowercase())
        .filter(|locale| **locale != 0)?;
    Some(state.csf_text(&format!("WOL:Locale{locale:02}")))
}

/// `0x00780D90`: `TXT_RANDOM_MAP_DESCRIPTION` for `RandMap.Sed`, else the
/// description (`+0x00`) of the first scenario record (`[0xA8B8CC]`)
/// whose file name (`+0x58`) equals `scenario` without case; empty when
/// none does.
pub(super) fn scenario_description(state: &AppState, scenario: &str) -> String {
    use crate::util::native_string::crt_strcmpi;
    use std::cmp::Ordering;
    if crt_strcmpi(scenario, crate::map::skirmish_scenarios::RANDMAP_SED) == Ordering::Equal {
        return state.csf_text(crate::app::shell_random_map::RANDOM_MAP_DESCRIPTION_KEY);
    }
    state
        .frontend
        .scenario_catalog
        .records()
        .iter()
        .find(|record| crt_strcmpi(&record.file_name, scenario) == Ordering::Equal)
        .map(|record| record.display_name.clone())
        .unwrap_or_default()
}

/// `0x00780E30`: the host (the name up to `'`, at most 10 characters), the
/// ping, then `", "` and the map when exInfo names a scenario `0x00780D90`
/// knows. Used by the classic lobby.
fn game_help(state: &AppState, row: &WolGameRow) -> String {
    let host: String = row
        .name
        .split('\'')
        .next()
        .unwrap_or_default()
        .chars()
        .take(10)
        .collect();
    let ping = if row.ping == -1 {
        state.csf_text("TXT_UNKNOWN_PING")
    } else {
        crate::assets::csf_file::format_csf(
            &state.csf_text("MSG:PingInfo"),
            &[crate::assets::csf_file::CsfArg::Int(i64::from(row.ping))],
        )
    };
    let mut help = format!("{}, {ping}", user_help(state, &host, None));
    let channel = lobby(state).and_then(|lobby| lobby.games.get(row.record));
    let scenario = channel
        .map(|channel| channel.ex_info().scenario)
        .unwrap_or_default();
    if !scenario.is_empty() {
        let name = scenario_description(state, &scenario);
        if !name.is_empty() {
            help.push_str(", ");
            help.push_str(&name);
        }
    }
    help
}
