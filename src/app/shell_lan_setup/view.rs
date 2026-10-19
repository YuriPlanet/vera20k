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
fn choice(state: &AppState, item: SkirmishComboItem) -> SetupChoice {
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

pub(crate) fn view(state: &AppState) -> Option<SetupView> {
    let session = &state.match_state.session;
    let launch = session.network_setup()?;
    let protocol = state.frontend.lan_setup_protocol.as_ref()?;
    let mode = mode_by_id(&state.frontend.skirmish_modes, launch.mode.id)?;
    let host = session.host();
    let local = launch.humans.get(launch.local_player_index)?;
    let map_index = launch.selected_map_file.as_deref().and_then(|file| {
        state
            .frontend
            .scenario_catalog
            .shell_maps()
            .iter()
            .position(|map| map.file_name.eq_ignore_ascii_case(file))
    });
    let capacity = session.network_setup_capacity()?.clamp(0, 8) as usize;
    let observer_row = session.network_observer_row().map(usize::from);
    let visible_limit = if observer_row == Some(0) && capacity < 8 {
        capacity + 1
    } else {
        capacity
    };
    let cooperative = mode.class.uses_cooperative_setup();
    let coop = cooperative
        .then(|| {
            state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_setup_view()
        })
        .flatten();
    let mut combos = Vec::with_capacity(40);
    let mut row_icons = std::array::from_fn(|_| None);
    for row in 0..8u8 {
        let human = launch
            .humans
            .iter()
            .enumerate()
            .find(|(_, human)| human.raw_slot_index == Some(row));
        let ai = launch.opponents.iter().find(|ai| ai.raw_slot_index == row);
        let vacancy = session.network_vacancy(row);
        let is_local = local.raw_slot_index == Some(row);
        let observer = human.is_some_and(|(_, human)| human.is_observer())
            || vacancy == Some(NetworkVacancy::OpenObserver);
        let visible = usize::from(row) < visible_limit || observer_row == Some(usize::from(row));
        let editable = ((is_local
            && (host || !session.network_player_ready(launch.local_player_index)))
            || host && ai.is_some())
            && !observer;
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
            match ai.difficulty {
                AiDifficulty::Easy => 4,
                AiDifficulty::Normal => 5,
                AiDifficulty::Hard => 6,
            }
        } else {
            match vacancy.unwrap_or(NetworkVacancy::Closed) {
                NetworkVacancy::Open => 2,
                NetworkVacancy::Closed => 3,
                NetworkVacancy::OpenObserver => 7,
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
        let mut players = Vec::new();
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
                for (value, key) in [(4, "GUI:AIEasy"), (5, "GUI:AINormal"), (6, "GUI:AIHard")] {
                    players.push(SetupChoice {
                        value,
                        text: label(state, key),
                        swatch: None,
                    });
                }
            }
        }
        let selected = players.iter().position(|item| item.value == slot_value);
        combos.push(SetupCombo {
            id: SetupColumn::Player.control(usize::from(row)),
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
        if let Some(coop) = &coop {
            //5C23B0 rebuilds country entries in the campaign's authored order.
            let mut allowed = Vec::new();
            if coop.player_countries.len() > 1 {
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
        if observer {
            countries = vec![SetupChoice {
                value: -3,
                text: label(state, "GUI:Observer"),
                swatch: None,
            }];
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
                id: column.control(usize::from(row)),
                enabled: editable && (column != SetupColumn::Team || mode.allies_allowed),
                visible: visible && (column == SetupColumn::Country || !observer),
                selected,
                choices,
            });
        }
        if visible {
            row_icons[usize::from(row)] = Some(SetupRowIcon {
                country: flag_pcx_for_side_item_data(country).map(str::to_owned),
                host: human.is_some() && row == 0,
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
    Some(SetupView {
        host,
        locked: protocol.locked,
        accept_enabled: protocol.accept_eligible
            && !session.network_player_ready(launch.local_player_index),
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
        cooperative,
        cooperative_description: coop.as_ref().map(|coop| {
            label(
                state,
                &format!(
                    "NAME:COOPDESC{}{}",
                    coop.campaign_index + 1,
                    coop.stage_index + 1
                ),
            )
        }),
    })
}
