//! Read-only BC/BD control projection. Native5EB060 builds Player entries;
//!5EA180/5E9E70 gate their siblings. MatchSession is the sole setup authority.

use crate::app::AppState;
use crate::app::frontend::skirmish_shell_render::{
    combo_item_label, flag_pcx_for_side_item_data, side_item_data_for_country,
};
use crate::app::match_runtime::session::{MatchSession, NetworkVacancy};
use crate::app::shell_wol::lobby::player_badge;
use crate::skirmish_launch::{AiDifficulty, LaunchStartPosition, LaunchTeam};
use crate::skirmish_modes::{MpModeClass, mode_by_id};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::trackbar::TrackbarRange;
use crate::ui::skirmish_shell::{
    self, SkirmishComboItem, SkirmishCountryChoice, SkirmishTrackbarId,
};

fn label(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(key))
        .unwrap_or(key)
        .to_owned()
}
pub(crate) fn choice(state: &AppState, item: SkirmishComboItem) -> SetupChoice {
    let value = match item {
        SkirmishComboItem::Country(SkirmishCountryChoice::Random) => -2,
        SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
            side_item_data_for_country(country)
        }
        SkirmishComboItem::ColorSentinel(value) => value,
        SkirmishComboItem::Color(value) => value as i32,
        SkirmishComboItem::Start(LaunchStartPosition::Auto) => -2,
        SkirmishComboItem::Start(LaunchStartPosition::Position(index)) => i32::from(index),
        SkirmishComboItem::Team(value) => value,
        SkirmishComboItem::AiType(value) => value.item_data(),
    };
    SetupChoice {
        value,
        text: combo_item_label(state, item),
        swatch: if let SkirmishComboItem::Color(index) = item {
            Some(index)
        } else {
            None
        },
    }
}
fn team_value(team: LaunchTeam) -> i32 {
    match team {
        LaunchTeam::None => -2,
        LaunchTeam::Team(index) => i32::from(index),
    }
}

/// A team's letter as the Team list shows it.
pub(super) fn team_letter(state: &AppState, team: u8) -> String {
    combo_item_label(state, SkirmishComboItem::Team(i32::from(team)))
}

/// A row of VERA's room list: a seat or spectator row, or a header over
/// the rows of its group and how many players they hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    Row(u8),
    Header(SetupGroup, usize),
}

/// VERA's online room: the help under the pointer on the name of a player
/// with a badge, as the lobby's player list gives it
/// ([`crate::app::shell_wol::lobby::badge_help`]).
pub(crate) fn player_help(state: &AppState, view: &SetupView, control: u16) -> Option<String> {
    let (SetupColumn::Player, row) = SetupColumn::decode(control)? else {
        return None;
    };
    let badge = view.row_icons.get(row)?.as_ref()?.badge?;
    let row = view.combo(control)?.row;
    let launch = state.match_state.session.network_setup()?;
    let human = launch
        .humans
        .iter()
        .find(|human| human.raw_slot_index == Some(row))?;
    crate::app::shell_wol::lobby::badge_help(state, &human.name, badge)
}

/// Whether the local player could press Accept (`SetupView::accept_enabled`).
pub(super) fn accept_enabled(state: &AppState) -> bool {
    let session = &state.match_state.session;
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.accept_eligible)
        && session
            .network_setup()
            .is_some_and(|launch| !session.network_player_ready(launch.local_player_index))
        && crate::app::shell_wol::local_choices_agreed(state)
}

/// The room's map in the scenario catalog, when this player shows its own
/// copy of it. A guest shows its own copy only when it verified the map, and
/// on WOL only from a mixfile (0x005E74E0); otherwise its preview is the
/// host's Preview.bin.
pub(super) fn local_map_index(state: &AppState) -> Option<usize> {
    let session = &state.match_state.session;
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    if !session.host() && !protocol.local_preview() {
        return None;
    }
    let file = session.network_setup()?.selected_map_file.as_deref()?;
    state
        .frontend
        .scenario_catalog
        .shell_maps()
        .iter()
        .position(|map| map.file_name.eq_ignore_ascii_case(file))
}

pub(crate) fn view(state: &AppState) -> Option<SetupView> {
    let session = &state.match_state.session;
    let launch = session.network_setup()?;
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    let mode = mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)?;
    let host = session.host();
    let host_name = protocol.wol_host.as_deref();
    let local = launch.humans.get(launch.local_player_index)?;
    let room = session.network_room();
    // 0x4E5C5B..0x4E5C90, 0x4E5FD5..0x4E6004: a WOL clan room hides every
    // team combo and the team header 0x794.
    let clan_room = profile_is_wol(state) && room.clan;
    let map_index = local_map_index(state);
    let capacity = session
        .network_setup_capacity()?
        .clamp(0, crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32)
        as usize;
    let observer_row = session.network_observer_row().map(usize::from);
    let row_limit = session.network_row_limit()?;
    let visible_limit = if observer_row == Some(0) && (capacity as i32) < row_limit {
        capacity + 1
    } else {
        capacity
    };
    let cooperative = mode.class.uses_cooperative_setup();
    let coop = cooperative.then(|| {
        state
            .frontend
            .multiplayer_shell_runtime
            .cooperative_setup_view()
    });
    // VERA's rooms list their seats, the map's players, whether taken or
    // Open, then the spectators. A seat the host removed (Closed) stays
    // hidden until the host's Add slot row, after the seats, opens it.
    let several = session.network_several_observers() && observer_row.is_some();
    let visible_limit = if several {
        usize::from(session.network_player_row_limit())
    } else {
        visible_limit
    };
    // No AI plays in a VERA room while anyone watches (`0x5EC9F0`'s
    // TXT_NO_AI_WITH_OBSERVER).
    let watched = several && launch.humans.iter().any(|human| human.is_observer());
    let seats = 0..visible_limit.min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT) as u8;
    let removed = |row: &u8| session.network_vacancy(*row) == Some(NetworkVacancy::Closed);
    let add_slot = (several && host)
        .then(|| seats.clone().find(removed))
        .flatten();
    // The host keeps at least two seats.
    let may_remove = seats.filter(|row| !removed(row)).count() > 2;
    let ai_allowed = !watched && mode.class.lists_in_skirmish();
    // In a mode with teams, VERA's rooms group their rows under headers:
    // with Teams set, one for each of its teams.
    let grouped = several && session.network_team_room();
    let teams = session.network_teams();
    let team_seats = session.network_team_seats();
    // While the room balances itself, nobody picks a team: the Team lists and
    // the headers' Join close for every seated player, the host too. A
    // spectator's Join still asks to play, and the balance places it.
    let auto_balance = session.network_auto_balance();
    let listed = if several {
        listed_rows(
            session,
            visible_limit,
            add_slot,
            grouped.then(|| skirmish_shell::team_choices(mode.must_ally)),
            teams,
        )
    } else {
        Vec::new()
    };
    let row_count = if several {
        listed.len()
    } else {
        visible_limit
            .max(observer_row.map_or(0, |row| row + 1))
            .min(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
    };
    let row_at = |position: usize| {
        if several {
            listed.get(position).copied()
        } else {
            (position < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
                .then_some(Listed::Row(position as u8))
        }
    };
    // The local player's Join and Watch on the headers: a seated player
    // joins another team, a guest watching takes a free seat on any, or
    // with none free joins the queue for the next.
    let starting = super::wol::start_pending(state);
    let placed = local.raw_slot_index.is_some();
    let seated = placed && !local.is_observer();
    let seat_free = !session.network_no_open_player_row();
    let watch_free = session.network_free_row(true).is_some();
    let queued = session
        .network_queue_place(launch.local_player_index)
        .is_some();
    let queue = !seated && !seat_free && session.network_queue_open();
    let ai_seat = (host && ai_allowed)
        .then(|| session.network_free_row(false))
        .flatten();
    let dialog = super::dialog(state);
    let rows = dialog.rows_per_page();
    let first_row = state
        .frontend
        .multiplayer_setup
        .as_ref()
        .map_or(0, |ui| ui.first_row(row_count, rows));
    let mut combos = Vec::with_capacity(5 * rows);
    let mut row_icons = std::array::from_fn(|_| None);
    let mut headers = std::array::from_fn(|_| None);
    for visible_row in 0..rows {
        let row = match row_at(first_row + visible_row) {
            None => break,
            Some(Listed::Row(row)) => row,
            Some(Listed::Header(group, count)) => {
                let fill = |key: &str| crate::i18n::tr_fill(key, &[("count", &count)]);
                let joins = |team: LaunchTeam| {
                    placed
                        && if seated {
                            local.slot.team != team
                        } else {
                            !host
                        }
                };
                // A full team's Join is greyed; the host still moves anyone.
                // With no seat free, a spectator's Join queues it, once.
                let join = |open: bool| {
                    if queue {
                        (crate::i18n::tr("VERA:JoinQueue"), !queued)
                    } else {
                        (crate::i18n::tr("VERA:Join"), open && (seated || seat_free))
                    }
                };
                let (caption, button) = match group {
                    SetupGroup::Team(team) => (
                        match team_seats {
                            Some(seats) => crate::i18n::tr_fill(
                                "VERA:TeamSeatsGroup",
                                &[
                                    ("letter", &team_letter(state, team)),
                                    ("count", &count),
                                    ("seats", &seats),
                                ],
                            ),
                            None => crate::i18n::tr_fill(
                                "VERA:TeamGroup",
                                &[("letter", &team_letter(state, team)), ("count", &count)],
                            ),
                        },
                        joins(LaunchTeam::Team(team))
                            .then(|| join(!session.network_team_full(team))),
                    ),
                    SetupGroup::NoTeam => (
                        fill("VERA:NoTeamGroup"),
                        (!mode.must_ally && teams.is_none() && joins(LaunchTeam::None))
                            .then(|| join(true)),
                    ),
                    SetupGroup::OpenSeats => (fill("VERA:OpenSeatsGroup"), None),
                    SetupGroup::Spectators => (
                        fill("VERA:SpectatorsGroup"),
                        if queued {
                            Some((crate::i18n::tr("VERA:LeaveQueue"), true))
                        } else {
                            (seated && !host && watch_free)
                                .then(|| (crate::i18n::tr("VERA:Watch"), true))
                        },
                    ),
                };
                // A spectator's Join asks to play: the balance places it.
                let closed = auto_balance && seated && group != SetupGroup::Spectators;
                headers[visible_row] = Some(SetupHeader {
                    group,
                    caption,
                    join_enabled: button.as_ref().is_some_and(|(_, enabled)| *enabled)
                        && !starting
                        && !closed,
                    join: button.map(|(label, _)| label),
                });
                // The host's Add AI fills the first free seat on this team,
                // while the team has one.
                if let (SetupGroup::Team(team), Some(seat)) = (group, ai_seat)
                    && !session.network_team_full(team)
                {
                    let item = |value: i32, text: String| SetupChoice {
                        value,
                        text,
                        swatch: None,
                    };
                    combos.push(SetupCombo {
                        id: team_add_ai(visible_row),
                        row: seat,
                        enabled: !starting,
                        visible: true,
                        selected: Some(0),
                        choices: vec![
                            item(0, crate::i18n::tr("VERA:AddAi")),
                            item(4, label(state, "GUI:AIEasy")),
                            item(5, label(state, "GUI:AINormal")),
                            item(6, label(state, "GUI:AIHard")),
                        ],
                    });
                }
                continue;
            }
        };
        let human = launch
            .humans
            .iter()
            .enumerate()
            .find(|(_, human)| human.raw_slot_index == Some(row));
        let ai = session.network_display_ai(row);
        let vacancy = session.network_vacancy(row);
        let is_local = local.raw_slot_index == Some(row);
        let observer = human.is_some_and(|(_, human)| human.is_observer())
            || vacancy == Some(NetworkVacancy::OpenObserver);
        let visible =
            several || usize::from(row) < visible_limit || observer_row == Some(usize::from(row));
        // A VERA room's Open, Add slot and Waiting rows show only their
        // Player combo.
        let adding = Some(row) == add_slot;
        let bare = several && human.is_none() && ai.is_none();
        // VERA's room accepts for its guest (`service::auto_accept`), so the
        // guest's row stays open: the host's next options take it back.
        let accepted = !dialog.modern && session.network_player_ready(launch.local_player_index);
        let editable = ((is_local && (host || !accepted)) || host && ai.is_some()) && !observer;
        // A VERA room's guest picks Observer in its Side list to watch while
        // a spectator row is open, and a side to play while a player row is.
        let switching = several && is_local && !host && !accepted;
        // VERA's host sets a seated guest's team (`service::move_player`).
        let host_moves = several && host && !is_local && human.is_some() && !observer;
        let choices = session.network_row_choices(row)?;
        let country = if observer {
            -3
        } else if choices.country_random {
            -2
        } else {
            side_item_data_for_country(choices.country)
        };
        let color = if choices.color_random {
            -2
        } else {
            i32::from(choices.color_index)
        };
        let start = choices.start_position;
        let team = team_value(choices.team);
        let slot_value = if human.is_some() {
            0
        } else if let Some(ai) = ai {
            match ai {
                AiDifficulty::Easy => 4,
                AiDifficulty::Normal => 5,
                AiDifficulty::Hard => 6,
            }
        } else {
            match vacancy.unwrap_or(NetworkVacancy::Closed) {
                NetworkVacancy::Open => 2,
                NetworkVacancy::Closed => 3,
                NetworkVacancy::OpenObserver => 7,
                NetworkVacancy::Waiting => 1,
            }
        };
        // A spectator in the join queue shows its place before its name.
        let slot_label = human
            .map(|(index, human)| match session.network_queue_place(index) {
                Some(place) => crate::i18n::tr_fill(
                    "VERA:QueuedName",
                    &[("place", &place), ("name", &human.name)],
                ),
                None => human.name.clone(),
            })
            .unwrap_or_else(|| {
                label(
                    state,
                    match slot_value {
                        2 => "GUI:Open",
                        3 => "GUI:Closed",
                        4 => "GUI:AIEasy",
                        5 => "GUI:AINormal",
                        6 => "GUI:AIHard",
                        7 => "GUI:OpenObserver",
                        _ => "GUI:Waiting",
                    },
                )
            });
        let ai_choices = [(4, "GUI:AIEasy"), (5, "GUI:AINormal"), (6, "GUI:AIHard")];
        let mut players = Vec::new();
        if several {
            // VERA's host kicks a person to an Open seat, puts an AI in an
            // Open seat or retunes one, removes a seat (3, Closed), and
            // opens a removed one from the Add slot row.
            let item = |value: i32, text: String| SetupChoice {
                value,
                text,
                swatch: None,
            };
            let seat = adding || ai.is_some() || vacancy == Some(NetworkVacancy::Open);
            if adding {
                players.push(item(3, crate::i18n::tr("VERA:AddSlot")));
            }
            if !(host && ai.is_some()) {
                players.push(item(
                    if adding { 2 } else { slot_value },
                    if adding {
                        label(state, "GUI:Open")
                    } else {
                        slot_label.clone()
                    },
                ));
            }
            if host && seat && (ai_allowed || ai.is_some()) {
                for (value, key) in ai_choices {
                    players.push(item(value, label(state, key)));
                }
            }
            if host && ai.is_some() {
                players.push(item(2, label(state, "GUI:Open")));
            }
            if host && human.is_some() && !is_local {
                // VERA's host seats a spectator, or moves a player to the
                // spectators, as their own Side list would.
                if observer && seat_free {
                    players.push(item(GIVE_SEAT, crate::i18n::tr("VERA:GiveSeat")));
                }
                if !observer && watch_free {
                    players.push(item(
                        MOVE_TO_SPECTATORS,
                        crate::i18n::tr("VERA:MoveToSpectators"),
                    ));
                }
                players.push(item(2, crate::i18n::tr("VERA:Kick")));
            }
            if host
                && !adding
                && (ai.is_some() || vacancy == Some(NetworkVacancy::Open))
                && may_remove
            {
                players.push(item(3, crate::i18n::tr("VERA:RemoveSlot")));
            }
        } else {
            if human.is_some() || !host {
                players.push(SetupChoice {
                    value: slot_value,
                    text: slot_label.clone(),
                    swatch: None,
                });
            }
            if host && !is_local {
                let observer_only = observer_row == Some(usize::from(row))
                    && usize::from(row) > capacity
                    && ai.is_none()
                    && vacancy != Some(NetworkVacancy::Open);
                if !observer_only {
                    players.push(SetupChoice {
                        value: 2,
                        text: label(state, "GUI:Open"),
                        swatch: None,
                    });
                }
                if observer_row == Some(usize::from(row)) {
                    players.push(SetupChoice {
                        value: 7,
                        text: label(state, "GUI:OpenObserver"),
                        swatch: None,
                    });
                }
                players.push(SetupChoice {
                    value: 3,
                    text: label(state, "GUI:Closed"),
                    swatch: None,
                });
                //5EB060 asks the same vtable+40 used by the Skirmish mode list.
                if mode.class.lists_in_skirmish() && !observer_only {
                    for (value, key) in ai_choices {
                        players.push(SetupChoice {
                            value,
                            text: label(state, key),
                            swatch: None,
                        });
                    }
                }
            }
        }
        let selected = players.iter().position(|item| item.value == slot_value);
        combos.push(SetupCombo {
            id: SetupColumn::Player.control(visible_row),
            row,
            enabled: host || is_local,
            visible,
            selected,
            choices: players,
        });
        // VERA's room bans leave their countries off the list.
        let banned = super::bans::banned_countries(state, launch.options.bans);
        let mut countries: Vec<_> = state
            .rules()
            .map(skirmish_shell::country_choices)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                !matches!(item, SkirmishComboItem::Country(SkirmishCountryChoice::Country(country))
                    if banned.contains(country))
            })
            .map(|item| choice(state, item))
            .collect();
        if let Some(player_countries) = coop.as_ref().and_then(|coop| coop.player_countries) {
            //5C23B0 rebuilds country entries in the campaign's authored order.
            let mut allowed = Vec::new();
            if player_countries.len() > 1 {
                allowed.push(SetupChoice {
                    value: -2,
                    text: label(state, "GUI:Random"),
                    swatch: None,
                });
            }
            for country in state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_player_countries()
                .unwrap_or_default()
            {
                allowed.push(choice(
                    state,
                    SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)),
                ));
            }
            countries = allowed;
        }
        let sides = if switching && observer {
            !session.network_no_open_player_row()
        } else {
            !observer
        };
        if !sides {
            countries.clear();
        }
        if observer || switching && session.network_free_row(true).is_some() {
            countries.push(SetupChoice {
                value: -3,
                text: label(state, "GUI:Observer"),
                swatch: None,
            });
        }
        let color_claims = launch
            .humans
            .iter()
            .filter(|human| {
                //5E9C00 checks visibleA8DA90 pointers, not the source vector.
                // A kicked peer remains admitted until type10 but has no row.
                human.raw_slot_index.is_some()
                    && human.raw_slot_index != Some(row)
                    && !human.slot.color_random
                    && !human.is_observer()
            })
            .map(|human| usize::from(human.slot.color_index))
            .chain(
                launch
                    .opponents
                    .iter()
                    .filter(|ai| ai.raw_slot_index != row && !ai.color_random)
                    .map(|ai| usize::from(ai.color_index)),
            );
        let lobby_colors = state.lobby_colors();
        let color_claims = color_claims.filter(|_| {
            launch.humans.len() + launch.opponents.len() <= lobby_colors.lobby_color_count()
        });
        let colors = skirmish_shell::available_color_choices(
            lobby_colors,
            usize::try_from(color).ok(),
            color_claims,
        )
        .into_iter()
        .map(|item| choice(state, item))
        .collect::<Vec<_>>();
        // VERA shared control: outside Cooperative a player may pick a start
        // another player holds, which joins that player's House. Such starts
        // stay listed, marked "+".
        let shares = human.is_some() && mode.class != MpModeClass::Cooperative;
        let human_starts = launch
            .humans
            .iter()
            .filter(|human| {
                human.raw_slot_index.is_some()
                    && human.raw_slot_index != Some(row)
                    && !human.is_observer()
            })
            .map(|human| human.slot.start_position)
            .collect::<Vec<_>>();
        let start_claims = human_starts.iter().copied().filter(|_| !shares).chain(
            launch
                .opponents
                .iter()
                .filter(|ai| ai.raw_slot_index != row)
                .map(|ai| ai.start_position),
        );
        let starts = skirmish_shell::available_start_choices(Some(start), start_claims, capacity)
            .into_iter()
            .map(|item| {
                let mut choice = choice(state, item);
                if shares
                    && let SkirmishComboItem::Start(held @ LaunchStartPosition::Position(_)) = item
                    && human_starts.contains(&held)
                {
                    choice.text.push('+');
                }
                choice
            })
            .collect::<Vec<_>>();
        // With Teams set, the room's teams only; a guest's own list leaves
        // out the full ones it is not on.
        let team_items = match teams {
            Some(count) => (0..count)
                .filter(|team| {
                    host || choices.team == LaunchTeam::Team(*team)
                        || !session.network_team_full(*team)
                })
                .map(|team| SkirmishComboItem::Team(i32::from(team)))
                .collect(),
            None => skirmish_shell::team_choices(mode.must_ally),
        };
        let team_items = team_items
            .into_iter()
            .map(|item| choice(state, item))
            .collect::<Vec<_>>();
        for (column, value, choices) in [
            (SetupColumn::Country, country, countries),
            (SetupColumn::Color, color, colors),
            (
                SetupColumn::Start,
                match start {
                    LaunchStartPosition::Auto => -2,
                    LaunchStartPosition::Position(index) => i32::from(index),
                },
                starts,
            ),
            (SetupColumn::Team, team, team_items),
        ] {
            let selected = choices.iter().position(|item| item.value == value);
            combos.push(SetupCombo {
                id: column.control(visible_row),
                row,
                enabled: (editable
                    || switching && column == SetupColumn::Country
                    || host_moves && column == SetupColumn::Team)
                    && (column != SetupColumn::Team || mode.allies_allowed && !auto_balance),
                visible: visible
                    && !bare
                    && (column == SetupColumn::Country || !observer)
                    && !(column == SetupColumn::Team && clan_room),
                selected,
                choices,
            });
        }
        if visible && !bare {
            let flag = match u16::try_from(country)
                .ok()
                .and_then(|index| state.process_assets.country_flag_surface(index))
            {
                Some(flag) => Some(flag),
                None => flag_pcx_for_side_item_data(country).map(str::to_owned),
            };
            row_icons[visible_row] = Some(SetupRowIcon {
                country: flag,
                // 0x5EA3E0: wolhost.pcx on the human row named [0xA8D610];
                // a WOL guest learns it from the user list, otherwise the
                // host is row 0.
                host: human.is_some_and(|(_, human)| match host_name {
                    Some(host) => human.name == host,
                    None => row == 0,
                }),
                accepted: ai.is_some()
                    || human.is_some_and(|(index, _)| session.network_player_ready(index)),
                // VERA's online room shows each player's badge, as the
                // lobby's player list does.
                badge: human
                    .filter(|_| dialog.modern && dialog.wol())
                    .and_then(|(_, human)| player_badge(state, &human.name)),
            });
        }
    }
    let bounds = state.frontend.skirmish_shell_state.trackbar_bounds;
    let options = &launch.options;
    let trackbars = vec![
        SetupTrackbar {
            id: SPEED,
            value: crate::ui::shell::game_speed::SpeedContext::Network
                .visual_position(options.game_speed),
            range: bounds.range(SkirmishTrackbarId::GameSpeed0x529),
            enabled: host,
            visible: true,
        },
        SetupTrackbar {
            id: CREDITS,
            value: bounds.credits_visual_position(options.starting_credits),
            range: bounds.visual_range(SkirmishTrackbarId::Credits0x511),
            enabled: host,
            visible: true,
        },
        SetupTrackbar {
            id: UNITS,
            value: options.unit_count,
            range: bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            enabled: host,
            visible: true,
        },
        SetupTrackbar {
            id: DIFFICULTY,
            value: coop.as_ref().map_or(0, |coop| coop.difficulty),
            range: TrackbarRange::new(0, 2, 1),
            enabled: host,
            visible: cooperative,
        },
        // The Teams box's count: 1 is Off, then the teams.
        SetupTrackbar {
            id: TEAM_COUNT,
            value: options.teams.map_or(1, i32::from),
            range: TrackbarRange::new(1, i32::from(*crate::skirmish_launch::ROOM_TEAMS.end()), 1),
            enabled: host && grouped,
            visible: dialog.modern && host,
        },
    ];
    // 7A0E50 from 5EA3E0: rows without a node show no ping image.
    let ping_icons = std::array::from_fn(|position| {
        (dialog.wol() && position < rows).then_some(())?;
        let Listed::Row(row) = row_at(first_row + position)? else {
            return None;
        };
        let human = launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row))?;
        ping_icon(protocol.ping(&human.name)?)
    });
    Some(SetupView {
        first_row,
        row_count,
        dialog,
        ping_icons,
        host,
        locked: protocol.locked,
        // C2's lockout (0x78F85E..0x78F907, 0x79132A..0x7913D1): Short Game
        // checked and disabled in a tournament or clan room.
        short_game_locked: host && profile_is_wol(state) && room.tournament_or_clan(),
        team_header: !clan_room,
        accept_enabled: accept_enabled(state),
        start_requested: protocol.start_requested
            && !session.network_player_ready(launch.local_player_index),
        combos,
        trackbars,
        checks: [
            options.short_game,
            options.mcv_redeploy,
            options.crates,
            options.super_weapons,
            options.build_off_ally,
        ],
        room_checks: [
            options.bridges_destroyable,
            options.ally_change_allowed,
            options.bans.yuri,
            options.bans.france,
            options.bans.spy,
            options.bans.stolen_tech,
        ],
        row_icons,
        headers,
        grouped,
        open_box: state
            .frontend
            .multiplayer_setup
            .as_ref()
            .and_then(|ui| ui.open_box)
            .filter(|_| dialog.modern),
        teams_enabled: host && grouped,
        team_starts_enabled: host && teams.is_some() && local_map_index(state).is_some(),
        team_colors_enabled: host && teams.is_some(),
        balance_enabled: host && grouped && dialog.wol(),
        auto_balance,
        auto_balance_enabled: host && teams.is_some() && dialog.wol(),
        map_offer_open: state
            .frontend
            .multiplayer_setup
            .as_ref()
            .is_some_and(|ui| ui.map_offer.is_some()),
        mode_label: label(
            state,
            if launch.selected_map_file.is_none() {
                "GUI:None"
            } else {
                &mode.ui_name_key
            },
        ),
        map_label: super::map_display_name(state)
            .map_or_else(|| label(state, "GUI:None"), str::to_owned),
        selected_map_index: map_index,
        no_preview: protocol.wol_no_preview().0,
        preview_hidden: protocol.wol_no_preview().1,
        cooperative,
        cooperative_description: coop.as_ref().and_then(|coop| coop.description).map(
            |(campaign, stage)| {
                label(
                    state,
                    &format!("NAME:COOPDESC{}{}", campaign + 1, stage + 1),
                )
            },
        ),
    })
}

/// VERA's room list: its seats, the host's Add slot row, then the
/// spectators, those in the join queue first. In a mode with teams
/// (`teams`, the Team list's items) a header for each team in use and for
/// the list's next free letter goes over its players (with Teams set,
/// `fixed`, one for each of its teams, empty or not), then come the players
/// on no team, the free seats with the Add slot row, and the spectators,
/// each group under its header.
fn listed_rows(
    session: &MatchSession,
    visible_limit: usize,
    add_slot: Option<u8>,
    teams: Option<Vec<SkirmishComboItem>>,
    fixed: Option<u8>,
) -> Vec<Listed> {
    let Some(launch) = session.network_setup() else {
        return Vec::new();
    };
    let rows = 0..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8;
    let watching = |row: u8| match launch
        .humans
        .iter()
        .find(|human| human.raw_slot_index == Some(row))
    {
        Some(human) => Some(human.is_observer()),
        None if session.network_display_ai(row).is_some() => Some(false),
        None => match session.network_vacancy(row) {
            Some(NetworkVacancy::Waiting) => Some(usize::from(row) >= visible_limit),
            Some(NetworkVacancy::Open) if usize::from(row) < visible_limit => Some(false),
            _ => None,
        },
    };
    let seated = rows.clone().filter(|row| watching(*row) == Some(false));
    // The spectators in the join queue come first, in its order.
    let place = |row: &u8| {
        launch
            .humans
            .iter()
            .position(|human| human.raw_slot_index == Some(*row))
            .and_then(|index| session.network_queue_place(index))
            .unwrap_or(usize::MAX)
    };
    let mut spectators: Vec<u8> = rows.filter(|row| watching(*row) == Some(true)).collect();
    spectators.sort_by_key(place);
    let Some(choices) = teams else {
        return seated
            .chain(add_slot)
            .chain(spectators)
            .map(Listed::Row)
            .collect();
    };
    let held = |row: &u8| {
        launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index == Some(*row))
            || session.network_display_ai(*row).is_some()
    };
    let team_of = |row: &u8| {
        session
            .network_row_choices(*row)
            .map_or(LaunchTeam::None, |slot| slot.team)
    };
    let (players, free): (Vec<u8>, Vec<u8>) = seated.partition(held);
    let teams: Vec<u8> = match fixed {
        Some(count) => (0..count).collect(),
        None => {
            let mut teams: Vec<u8> = players
                .iter()
                .filter_map(|row| match team_of(row) {
                    LaunchTeam::Team(team) => Some(team),
                    LaunchTeam::None => None,
                })
                .collect();
            let next = choices.into_iter().find_map(|item| match item {
                SkirmishComboItem::Team(team) => {
                    u8::try_from(team).ok().filter(|team| !teams.contains(team))
                }
                _ => None,
            });
            teams.extend(next);
            teams.sort_unstable();
            teams.dedup();
            teams
        }
    };
    let mut listed = Vec::new();
    // A header, how many players it counts, then its rows.
    let mut group = |header, count: usize, rows: Vec<u8>| {
        listed.push(Listed::Header(header, count));
        listed.extend(rows.into_iter().map(Listed::Row));
    };
    for &team in &teams {
        let members: Vec<u8> = players
            .iter()
            .copied()
            .filter(|row| team_of(row) == LaunchTeam::Team(team))
            .collect();
        group(SetupGroup::Team(team), members.len(), members);
    }
    let loners: Vec<u8> = players
        .iter()
        .copied()
        .filter(|row| !matches!(team_of(row), LaunchTeam::Team(team) if teams.contains(&team)))
        .collect();
    if !loners.is_empty() {
        group(SetupGroup::NoTeam, loners.len(), loners);
    }
    if !free.is_empty() || add_slot.is_some() {
        let count = free.len();
        group(
            SetupGroup::OpenSeats,
            count,
            free.into_iter().chain(add_slot).collect(),
        );
    }
    if !spectators.is_empty() || session.network_free_row(true).is_some() {
        group(SetupGroup::Spectators, spectators.len(), spectators);
    }
    listed
}

fn profile_is_wol(state: &AppState) -> bool {
    state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A VERA room on FOUR.MAP, with spectators: the host in row 0, a guest
    /// in row 1, rows 2 and 3 open.
    fn vera_room() -> MatchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        let mut guest = launch.local_player().clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        let mode = launch.mode.clone();
        let mut session = MatchSession::default();
        session.enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch, true, Some(7), true)
            .unwrap();
        session
            .select_network_map(mode, "FOUR.MAP".into(), 4)
            .unwrap();
        session.set_network_spectators(true).unwrap();
        session
    }

    /// In a mode with teams each team in use and the next free letter head
    /// their players, then come the players on no team, the free seats and
    /// the spectators. A guest who starts watching moves under Spectators.
    #[test]
    fn a_room_with_teams_lists_its_players_under_team_headers() {
        use Listed::{Header, Row};
        use SetupGroup::{NoTeam, OpenSeats, Spectators, Team};
        let mut session = vera_room();
        let mut host = session.network_row_choices(0).unwrap();
        host.team = LaunchTeam::Team(0);
        session.set_network_row_choices(0, host).unwrap();
        let limit = usize::from(session.network_player_row_limit());
        let teams = || Some(skirmish_shell::team_choices(false));
        assert_eq!(
            listed_rows(&session, limit, None, teams(), None),
            [
                Header(Team(0), 1),
                Row(0),
                Header(Team(1), 0),
                Header(NoTeam, 1),
                Row(1),
                Header(OpenSeats, 2),
                Row(2),
                Row(3),
                Header(Spectators, 0),
            ]
        );
        let guest = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, guest, true, None)
            .unwrap();
        assert_eq!(
            listed_rows(&session, limit, None, teams(), None),
            [
                Header(Team(0), 1),
                Row(0),
                Header(Team(1), 0),
                Header(OpenSeats, 3),
                Row(1),
                Row(2),
                Row(3),
                Header(Spectators, 1),
                Row(4),
            ]
        );
        // A mode without teams lists its rows as before.
        assert_eq!(
            listed_rows(&session, limit, None, None, None),
            [Row(0), Row(1), Row(2), Row(3), Row(4)]
        );
    }

    /// With Teams set, each of its teams has a header, an empty one too.
    #[test]
    fn a_room_with_teams_set_lists_every_team() {
        use Listed::{Header, Row};
        use SetupGroup::{OpenSeats, Spectators, Team};
        let mut session = vera_room();
        session.set_network_teams(Some(3)).unwrap();
        let limit = usize::from(session.network_player_row_limit());
        let teams = Some(skirmish_shell::team_choices(false));
        assert_eq!(
            listed_rows(&session, limit, None, teams, session.network_teams()),
            [
                Header(Team(0), 1),
                Row(0),
                Header(Team(1), 1),
                Row(1),
                Header(Team(2), 0),
                Header(OpenSeats, 2),
                Row(2),
                Row(3),
                Header(Spectators, 0),
            ]
        );
    }

    /// The spectators in the join queue list first, in its order.
    #[test]
    fn a_room_lists_its_queued_spectators_first() {
        use crate::app::match_runtime::session::NetworkRowKind;
        let mut session = vera_room();
        let slot = session.network_setup().unwrap().humans[1].slot.clone();
        session
            .accept_network_peer_choices(1, slot.clone(), true, None)
            .unwrap();
        let mut third = session.network_setup().unwrap().humans[1].clone();
        third.name = "Third".into();
        third.raw_slot_index = Some(5);
        session.add_network_participant(third).unwrap();
        for row in 1..4 {
            session
                .choose_network_row_kind(row, NetworkRowKind::Closed)
                .unwrap();
        }
        session
            .accept_network_peer_choices(2, slot, false, None)
            .unwrap();
        let limit = usize::from(session.network_player_row_limit());
        assert_eq!(
            listed_rows(&session, limit, None, None, None),
            [Listed::Row(0), Listed::Row(5), Listed::Row(4)]
        );
    }
}
