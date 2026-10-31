//! Read-only BC/BD control projection. Native5EB060 builds Player entries;
//!5EA180/5E9E70 gate their siblings. MatchSession is the sole setup authority.

use crate::app::AppState;
use crate::app::frontend::skirmish_shell_render::{
    combo_item_label, flag_pcx_for_side_item_data, side_item_data_for_country,
};
use crate::app::match_runtime::session::NetworkVacancy;
use crate::skirmish_launch::{AiDifficulty, LaunchStartPosition, LaunchTeam};
use crate::skirmish_modes::mode_by_id;
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
    // A guest shows its own copy of the map only when it verified, and on
    // WOL only from a mixfile (0x005E74E0); otherwise its preview is the
    // host's Preview.bin.
    let local_map = host || protocol.local_preview();
    let map_index = launch
        .selected_map_file
        .as_deref()
        .filter(|_| local_map)
        .and_then(|file| {
            state
                .frontend
                .scenario_catalog
                .shell_maps()
                .iter()
                .position(|map| map.file_name.eq_ignore_ascii_case(file))
        });
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
    // VERA's rooms list only who is in them: the players and AI, then the
    // spectators. Their Open and Closed rows stay hidden; a joiner takes
    // one and shows up.
    let several = session.network_several_observers() && observer_row.is_some();
    let visible_limit = if several {
        usize::from(session.network_player_row_limit())
    } else {
        visible_limit
    };
    // No AI plays in a VERA room while anyone watches (`0x5EC9F0`'s
    // TXT_NO_AI_WITH_OBSERVER).
    let watched = several && launch.humans.iter().any(|human| human.is_observer());
    // The host adds an AI from an Add AI row after the players, which is
    // the first open player row.
    let add_ai = (several && host && !watched && mode.class.lists_in_skirmish())
        .then(|| session.network_free_row(false))
        .flatten();
    let mut listed = Vec::new();
    if several {
        let rows = 0..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8;
        let watching = |row: u8| match launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row))
        {
            Some(human) => Some(human.is_observer()),
            None if session.network_display_ai(row).is_some() => Some(false),
            None if session.network_vacancy(row) == Some(NetworkVacancy::Waiting) => {
                Some(usize::from(row) >= visible_limit)
            }
            None => None,
        };
        listed.extend(rows.clone().filter(|row| watching(*row) == Some(false)));
        listed.extend(add_ai);
        listed.extend(rows.filter(|row| watching(*row) == Some(true)));
    }
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
            (position < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT).then_some(position as u8)
        }
    };
    let dialog = super::dialog(state);
    let rows = dialog.rows_per_page();
    let first_row = state
        .frontend
        .multiplayer_setup
        .as_ref()
        .map_or(0, |ui| ui.first_row(row_count, rows));
    let mut combos = Vec::with_capacity(5 * rows);
    let mut row_icons = std::array::from_fn(|_| None);
    for visible_row in 0..rows {
        let Some(row) = row_at(first_row + visible_row) else {
            break;
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
        // A VERA room's Add AI and Waiting rows show only their Player combo.
        let adding = Some(row) == add_ai;
        let bare = several && human.is_none() && ai.is_none();
        // VERA's room accepts for its guest (`service::auto_accept`), so the
        // guest's row stays open: the host's next options take it back.
        let accepted = !dialog.modern && session.network_player_ready(launch.local_player_index);
        let editable = ((is_local && (host || !accepted)) || host && ai.is_some()) && !observer;
        // A VERA room's guest picks Observer in its Side list to watch while
        // a spectator row is open, and a side to play while a player row is.
        let switching = several && is_local && !host && !accepted;
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
        let slot_label = human
            .map(|(_, human)| human.name.clone())
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
            // VERA's host kicks a person, removes or retunes an AI, and adds
            // one from the Add AI row; each opens its row (2), which leaves
            // the list.
            if adding {
                players.push(SetupChoice {
                    value: 2,
                    text: "Add AI".to_owned(),
                    swatch: None,
                });
            }
            if host && (adding || ai.is_some()) {
                for (value, key) in ai_choices {
                    players.push(SetupChoice {
                        value,
                        text: label(state, key),
                        swatch: None,
                    });
                }
            } else {
                players.push(SetupChoice {
                    value: slot_value,
                    text: slot_label.clone(),
                    swatch: None,
                });
            }
            if host && !is_local && !adding {
                players.push(SetupChoice {
                    value: 2,
                    text: if ai.is_some() { "Remove" } else { "Kick" }.to_owned(),
                    swatch: None,
                });
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
        let mut countries: Vec<_> = state
            .rules()
            .map(skirmish_shell::country_choices)
            .unwrap_or_default()
            .into_iter()
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
        let color_claims =
            color_claims.filter(|_| launch.humans.len() + launch.opponents.len() <= 8);
        let colors =
            skirmish_shell::available_color_choices(usize::try_from(color).ok(), color_claims)
                .into_iter()
                .map(|item| choice(state, item))
                .collect::<Vec<_>>();
        let start_claims = launch
            .humans
            .iter()
            .filter(|human| {
                human.raw_slot_index.is_some()
                    && human.raw_slot_index != Some(row)
                    && !human.is_observer()
            })
            .map(|human| human.slot.start_position)
            .chain(
                launch
                    .opponents
                    .iter()
                    .filter(|ai| ai.raw_slot_index != row)
                    .map(|ai| ai.start_position),
            );
        let starts = skirmish_shell::available_start_choices(Some(start), start_claims, capacity)
            .into_iter()
            .map(|item| choice(state, item))
            .collect::<Vec<_>>();
        let teams = skirmish_shell::team_choices(mode.must_ally)
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
            (SetupColumn::Team, team, teams),
        ] {
            let selected = choices.iter().position(|item| item.value == value);
            combos.push(SetupCombo {
                id: column.control(visible_row),
                row,
                enabled: (editable || switching && column == SetupColumn::Country)
                    && (column != SetupColumn::Team || mode.allies_allowed),
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
            });
        }
    }
    let bounds = state.frontend.skirmish_shell_state.trackbar_bounds;
    let options = &launch.options;
    let trackbars = vec![
        SetupTrackbar {
            id: SPEED,
            value: skirmish_shell::game_speed_visual_position(options.game_speed),
            range: bounds.range(SkirmishTrackbarId::GameSpeed0x529),
            enabled: host,
            visible: true,
        },
        SetupTrackbar {
            id: CREDITS,
            value: options.starting_credits,
            range: bounds.range(SkirmishTrackbarId::Credits0x511),
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
    ];
    // 7A0E50 from 5EA3E0: rows without a node show no ping image.
    let ping_icons = std::array::from_fn(|position| {
        (dialog.wol() && position < rows).then_some(())?;
        let row = row_at(first_row + position)?;
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
        row_icons,
        options_open: dialog.modern
            && state
                .frontend
                .multiplayer_setup
                .as_ref()
                .is_some_and(|ui| ui.options_open),
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

fn profile_is_wol(state: &AppState) -> bool {
    state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol
}
