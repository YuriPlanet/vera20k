//! The test service's game: the stand-in host and guest, the guest's
//! tournament and clan room, and the host's options with the target's
//! departure or kick through the production lobby sinks.

use super::*;

/// C9 in the test service's game: the host's options, then the target's
/// departure or kick through the production lobby sinks.
impl WolCapture {
    /// Test-service C2 with its stand-in guest: the join sink `0x7A6100`,
    /// the first `R` (`0x7A8260`) and `A1` (`0x77E900`).
    pub(super) fn service_host(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition, LaunchTeam};
        let guest = |state: &AppState| {
            let launch = state.match_state.session.network_setup()?;
            let index = launch
                .humans
                .iter()
                .position(|human| human.name == SERVICE_GUEST)?;
            Some((index, launch.humans[index].clone()))
        };
        let (index, human) = guest(state).context("the stand-in guest did not join")?;
        let view = setup_view(state)?;
        let accepted = human
            .raw_slot_index
            .and_then(|row| {
                row_combo(
                    &view,
                    crate::ui::multiplayer_setup::SetupColumn::Player,
                    row,
                )
            })
            .and_then(|(page_row, _)| view.row_icons[page_row].as_ref())
            .is_some_and(|icon| icon.accepted);
        ensure!(
            human.raw_slot_index == Some(1)
                && human.slot.country == LaunchCountry::Korea
                && !human.slot.country_random
                && human.slot.color_index == 3
                && !human.slot.color_random
                && state.match_state.session.network_player_ready(index)
                && state.match_state.session.network_map_status(index) == Some(1)
                && accepted,
            "the stand-in guest's row, choices, K1 or acceptance: {human:?}, icon {accepted}"
        );
        self.event(
            frame,
            json!({"guest joined": {"row": 1, "country": "Korea", "color": 3, "K": 1, "accepted": true}}),
        );
        let choices = |state: &mut AppState, slot, observer| {
            crate::app::shell_wol::service_event(
                state,
                WolServiceEvent::PlayerChoices {
                    from: SERVICE_GUEST.into(),
                    slot,
                    observer,
                },
            );
            setup_requests(state)
        };
        match self.target {
            WolTarget::GuestChoices => {
                // The host's colour 5 first, then the guest asks for it.
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Color.control(0),
                        value: 5,
                    },
                );
                let host = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C2")?
                    .local_player()
                    .slot
                    .clone();
                ensure!(
                    host.color_index == 5 && !host.color_random,
                    "the host's colour 5: {host:?}"
                );
                let requests = choices(
                    state,
                    crate::skirmish_launch::SkirmishLocalSlot {
                        color_index: 5,
                        start_position: LaunchStartPosition::Position(0),
                        team: LaunchTeam::Team(7),
                        ..human.slot.clone()
                    },
                    false,
                );
                let (_, human) = guest(state).context("guest")?;
                ensure!(
                    human.slot.color_random
                        && human.slot.start_position == LaunchStartPosition::Position(0)
                        && human.slot.team == LaunchTeam::None
                        && requests.contains(&NetworkSetupRequest::PublishOptions),
                    "the R checks: {human:?} {requests:?}"
                );
                self.event(
                    frame,
                    json!({"guest R": "colour 5 taken -> Random, start 1, team 7 -> none"}),
                );
            }
            WolTarget::GuestObserver => {
                // The host opens the observer row 7 first (player combo 7).
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Player.control(7),
                        value: 7,
                    },
                );
                let open_observer = state.match_state.session.network_free_row(true);
                ensure!(
                    open_observer == Some(7),
                    "C2 did not open observer row 7: {:?}",
                    setup_view(state)?
                        .combo(crate::ui::multiplayer_setup::SetupColumn::Player.control(7))
                );
                setup_requests(state);
                let requests = choices(state, human.slot.clone(), true);
                let (_, observer) = guest(state).context("guest")?;
                let roster = [
                    NetworkSetupRequest::Advertise,
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                ];
                ensure!(
                    requests == roster
                        && match open_observer {
                            Some(_) => {
                                observer.is_observer()
                                    && observer.raw_slot_index
                                        == state.match_state.session.network_observer_row()
                            }
                            None => !observer.is_observer() && observer.raw_slot_index.is_none(),
                        },
                    "the observer R with observer row {open_observer:?}: {observer:?} {requests:?}"
                );
                self.event(frame, json!({"guest observes": {"open observer row": open_observer, "row": observer.raw_slot_index}}));
                if open_observer.is_some() {
                    let requests = choices(state, human.slot.clone(), false);
                    let (_, player) = guest(state).context("guest")?;
                    // 0x7A8838: country Random, then the R's Korea differs.
                    ensure!(
                        !player.is_observer()
                            && player.raw_slot_index == Some(1)
                            && player.slot.country == LaunchCountry::Korea
                            && !player.slot.country_random
                            && requests == roster,
                        "the player R: {player:?} {requests:?}"
                    );
                    self.event(frame, json!({"guest plays again": {"row": 1}}));
                }
            }
            WolTarget::ModernSpectators => {
                // VERA's room keeps one spectator row open past its players:
                // the guest who watches takes it and the next opens.
                use crate::ui::multiplayer_setup::SetupColumn;
                setup_requests(state);
                let first = state.match_state.session.network_free_row(true);
                let requests = choices(state, human.slot.clone(), true);
                let (_, observer) = guest(state).context("guest")?;
                let second = state.match_state.session.network_free_row(true);
                ensure!(
                    first.is_some()
                        && observer.is_observer()
                        && observer.raw_slot_index == first
                        && second == first.map(|row| row + 1),
                    "the guest takes the open spectator row {first:?}: \
                     {observer:?}, then {second:?} {requests:?}"
                );
                // The room lists the host, the seat the guest left, then the
                // spectator, whom the host's menu seats again or kicks.
                // Nobody adds an AI while anyone watches.
                let listed: Vec<_> = setup_view(state)?
                    .combos
                    .iter()
                    .filter(|combo| {
                        SetupColumn::decode(combo.id)
                            .is_some_and(|(column, _)| column == SetupColumn::Player)
                    })
                    .map(|combo| {
                        let values: Vec<_> =
                            combo.choices.iter().map(|choice| choice.value).collect();
                        (combo.row, values)
                    })
                    .collect();
                let seat = crate::ui::multiplayer_setup::GIVE_SEAT;
                let spectator = first.context("spectator row")?;
                ensure!(
                    listed == [(0, vec![0]), (1, vec![2]), (spectator, vec![0, seat, 2])],
                    "the room lists {listed:?}"
                );
                self.event(
                    frame,
                    json!({"spectator rows": [first, second], "guest observes": first, "listed": [0, 1, first]}),
                );
                // The host's Seat puts the spectator back in the free seat,
                // as the guest's own side pick would.
                let control = row_combo(&setup_view(state)?, SetupColumn::Player, spectator)
                    .map(|(_, combo)| combo.id)
                    .context("the spectator's Player list")?;
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control,
                        value: seat,
                    },
                );
                let requests = setup_requests(state);
                let (_, player) = guest(state).context("guest")?;
                ensure!(
                    !player.is_observer()
                        && player.raw_slot_index == Some(1)
                        && requests.contains(&NetworkSetupRequest::PublishOptions),
                    "the host's Seat: {player:?} {requests:?}"
                );
                self.event(frame, json!({"host seats the spectator": {"row": 1}}));
                // With no seat free, the spectator asking to play waits in
                // the join queue, its place before its name, and the host's
                // game seats it when a seat frees (`service::seat_queue`).
                use crate::app::match_runtime::session::NetworkRowKind;
                choices(state, player.slot.clone(), true);
                let session = &mut state.match_state.session;
                session.choose_network_row_kind(1, NetworkRowKind::Closed)?;
                choices(state, player.slot.clone(), false);
                let (index, waiting) = guest(state).context("guest")?;
                let row = waiting.raw_slot_index.context("the spectator's row")?;
                let name = crate::i18n::tr_fill(
                    "VERA:QueuedName",
                    &[("place", &1), ("name", &SERVICE_GUEST)],
                );
                let labelled = row_combo(&setup_view(state)?, SetupColumn::Player, row)
                    .is_some_and(|(_, combo)| {
                        combo
                            .choices
                            .iter()
                            .any(|choice| choice.value == 0 && choice.text == name)
                    });
                let session = &state.match_state.session;
                ensure!(
                    waiting.is_observer()
                        && session.network_queue() == [SERVICE_GUEST]
                        && session.network_queue_place(index) == Some(1)
                        && labelled,
                    "the spectator did not wait first in the join queue: {waiting:?}"
                );
                self.event(
                    frame,
                    json!({"join queue": {"waiting": row, "label": name}}),
                );
                let session = &mut state.match_state.session;
                session.choose_network_row_kind(1, NetworkRowKind::Open)?;
                setup_requests(state);
                crate::app::shell_network_setup::poll(state);
                let requests = setup_requests(state);
                let (_, seated) = guest(state).context("guest")?;
                ensure!(
                    !seated.is_observer()
                        && seated.raw_slot_index == Some(1)
                        && state.match_state.session.network_queue().is_empty()
                        && requests.contains(&NetworkSetupRequest::PublishOptions),
                    "the join queue did not seat the spectator: {seated:?} {requests:?}"
                );
                self.event(frame, json!({"join queue seats the spectator": {"row": 1}}));
                // The lobby's answer gives the guest a Major's badge: the
                // room draws it in their row, and the help of their name
                // under the pointer names it, their rank and OS, as the
                // lobby's player list does.
                use crate::app::shell_wol::{WolHighScoreRecord, WolLadderRecord};
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![WolLadderRecord {
                        name: SERVICE_GUEST.into(),
                        points: 31,
                        os_hundredths: 3120,
                        rank: 5,
                        ..Default::default()
                    }]),
                );
                service_event(
                    state,
                    WolServiceEvent::HighScores(Ok(vec![WolHighScoreRecord {
                        name: SERVICE_GUEST.into(),
                        points: 151,
                        rank: 5,
                        coop: 0,
                    }])),
                );
                let view = setup_view(state)?;
                let (page_row, combo) =
                    row_combo(&view, SetupColumn::Player, 1).context("the guest's name")?;
                let badge = view
                    .row_icons
                    .get(page_row)
                    .and_then(Option::as_ref)
                    .and_then(|icon| icon.badge);
                let rect = crate::app::input::multiplayer_setup::layout(state)
                    .control_rect(combo.id)
                    .context("the guest's name")?;
                set_window_pointer_to_rect_center(state, rect);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                let help = setup::view::player_help(state, &view, combo.id);
                let label = |key: &str| state.csf_label(key, key);
                let expected = crate::i18n::fill(
                    &label("VERA:PlayerHelp"),
                    &[
                        ("name", &SERVICE_GUEST),
                        ("badge", &label("VERA:LobbyBadgeMajor")),
                        ("rank", &5),
                        ("os", &"31.20"),
                    ],
                );
                ensure!(
                    badge == Some("major.pcx") && help.as_deref() == Some(expected.as_str()),
                    "the guest's badge {badge:?} and help {help:?}"
                );
                self.event(
                    frame,
                    json!({"player badge": {"row": 1, "badge": badge, "help": help}}),
                );
                self.settle();
                return Ok(());
            }
            WolTarget::ClanLockout | WolTarget::ClanNoStart | WolTarget::ClanChooseMap => {
                return self.clan_room(state, frame);
            }
            WolTarget::ScenariosMismatch | WolTarget::MismatchOk => {
                setup_button(state, crate::ui::multiplayer_setup::START)?;
                self.event(frame, "C2 Start with the stand-in guest");
                self.phase = WolPhase::Mismatch;
                return Ok(());
            }
            WolTarget::GuestKick => {
                crate::app::shell_network_setup::apply_action(
                    state,
                    crate::ui::multiplayer_setup::SetupAction::Select {
                        control: crate::ui::multiplayer_setup::SetupColumn::Player.control(1),
                        value: 2,
                    },
                );
                self.event(frame, "C2 kicks the stand-in guest");
                self.phase = WolPhase::GuestKicked;
                return Ok(());
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    /// C9 in Boris's tournament room: two rows (`0x005EC408`, `0x005E6440`).
    /// After Accept, host options this guest cannot preview take the
    /// acceptance back (`0x007A8145..0x007A8196`).
    fn guest_tournament(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let session = &state.match_state.session;
        ensure!(
            session.network_room().tournament
                && session.network_setup_capacity() == Some(2)
                && session.network_row_limit() == Some(2),
            "the tournament room's rows: {:?} capacity {:?} limit {:?}",
            session.network_room(),
            session.network_setup_capacity(),
            session.network_row_limit()
        );
        self.event(frame, json!({"tournament room": {"rows": 2}}));
        setup::apply_action(state, crate::ui::multiplayer_setup::SetupAction::Accept);
        let local = |state: &AppState| {
            let session = &state.match_state.session;
            session
                .network_setup()
                .is_some_and(|launch| session.network_player_ready(launch.local_player_index))
        };
        ensure!(
            local(state) && setup_requests(state) == [NetworkSetupRequest::Ready(true)],
            "Accept did not send A1"
        );
        let options = unverified_host_options(state)?;
        setup::service_event(state, options)?;
        let requests = setup_requests(state);
        let view = setup_view(state)?;
        ensure!(
            !local(state)
                && !view.enabled(crate::ui::multiplayer_setup::ACCEPT)
                && requests.contains(&NetworkSetupRequest::Ready(false)),
            "the tournament room did not take the acceptance back: {requests:?}"
        );
        self.event(frame, "host options without a preview: A0, Accept disabled");
        Ok(())
    }

    /// C2 in a clan room: the lockout, the hidden team column, an even row
    /// count; then the start gate or the chooser's filters.
    fn clan_room(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup::{CHECKBOXES, SetupColumn};
        let session = &state.match_state.session;
        let launch = session.network_setup().context("C2")?;
        let options = launch.options.clone();
        let room = session.network_room();
        let capacity = session.network_setup_capacity();
        let view = setup_view(state)?;
        let team_hidden = view
            .combos
            .iter()
            .filter(|combo| {
                SetupColumn::decode(combo.id).is_some_and(|(column, _)| column == SetupColumn::Team)
            })
            .all(|combo| !combo.visible);
        ensure!(
            room.clan
                && options.short_game
                && options.bases
                && CHECKBOXES[0] == 0x54E
                && view.checks[0]
                && !view.enabled(0x54E)
                && team_hidden
                && !view.team_header
                && capacity.is_some_and(|capacity| capacity % 2 == 0),
            "C2's clan room: {room:?} short game {} enabled {} teams hidden {team_hidden} capacity {capacity:?}",
            options.short_game,
            view.enabled(0x54E)
        );
        self.event(
            frame,
            json!({"clan room": {"short game": "on, disabled", "team column": "hidden", "capacity": capacity}}),
        );
        match self.target {
            WolTarget::ClanNoStart => {
                setup_requests(state);
                setup_button(state, crate::ui::multiplayer_setup::START)?;
                let text = state.csf_label("TXT_CLAN_NOSTART", "TXT_CLAN_NOSTART");
                let requests = setup_requests(state);
                // The line wraps; its first row starts the text.
                let lines: Vec<String> = state
                    .frontend
                    .multiplayer_setup
                    .as_ref()
                    .context("C2")?
                    .chat
                    .lines
                    .iter()
                    .map(|line| String::from_utf16_lossy(&line.text))
                    .collect();
                ensure!(
                    lines
                        .iter()
                        .any(|line| line.len() > 10 && text.starts_with(line.trim_end()))
                        && !requests.iter().any(|request| matches!(
                            request,
                            NetworkSetupRequest::StartGame { .. }
                        )),
                    "Start in the clan room with a guest in no squad: {requests:?} {lines:?}"
                );
                self.event(frame, "Start: TXT_CLAN_NOSTART");
            }
            WolTarget::ClanChooseMap => {
                crate::app::shell_network_setup::open_choose_map(state);
                let modal = state
                    .frontend
                    .skirmish_shell_state
                    .choose_map_modal
                    .as_ref()
                    .context("the chooser")?;
                let modes = &state.frontend.skirmish_modes;
                let listed: Vec<_> = modal
                    .mode_rows()
                    .iter()
                    .filter_map(|row| row.resolve(modes))
                    .map(|mode| mode.class)
                    .collect();
                let records = state.frontend.scenario_catalog.records();
                ensure!(
                    !listed.contains(&crate::skirmish_modes::MpModeClass::FreeForAll)
                        && listed.contains(&crate::skirmish_modes::MpModeClass::Battle)
                        && modal
                            .filtered_record_indices
                            .iter()
                            .all(|index| records[*index].official),
                    "the clan room's chooser: {listed:?}"
                );
                self.event(frame, json!({"chooser modes": format!("{listed:?}")}));
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    pub(super) fn guest_options(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("C9 setup")?;
        let rows: Vec<(String, Option<u8>)> = launch
            .humans
            .iter()
            .map(|human| (human.name.clone(), human.raw_slot_index))
            .collect();
        let map = launch.selected_map_file.clone();
        let view = setup_view(state)?;
        let host_icon = view.row_icons[0].as_ref().is_some_and(|icon| icon.host)
            && !view.row_icons[1].as_ref().is_some_and(|icon| icon.host);
        ensure!(
            map.is_some()
                && rows
                    == [
                        (NICKNAME.to_owned(), Some(1)),
                        (self.target.guest_host().to_owned(), Some(0))
                    ]
                && host_icon,
            "C9 did not take the host's options: {rows:?} {map:?} host icon {host_icon}"
        );
        self.event(frame, json!({"host options": {"map": map, "rows": rows}}));
        let channel = WolChannel {
            kind: GAME_CHANNEL,
            name: "Tanya's_game".into(),
            ..Default::default()
        };
        match self.target {
            WolTarget::GuestHostLeft => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::UserLeft("Tanya".into()),
                );
                self.event(frame, "the host left the game channel");
                self.rejoined = true;
                self.phase = WolPhase::ServiceLobby;
                return Ok(());
            }
            WolTarget::GuestKicked => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel,
                        user: member(NICKNAME, crate::ui::wol_lobby::MEMBER_LOCAL, ""),
                        by: member("Tanya", MEMBER_OPERATOR, ""),
                    },
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the kick did not close C9"
                );
                self.event(frame, "this guest kicked by the host");
                self.rejoined = true;
                self.phase = WolPhase::ServiceLobby;
                return Ok(());
            }
            WolTarget::GuestUserKicked => {
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::UserJoined(member("Boris", 0, "")),
                );
                let joined = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| launch.humans.iter().any(|human| human.name == "Boris"));
                ensure!(joined, "Boris's join added no row");
                crate::app::shell_wol::service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member("Boris", 0, ""),
                        by: member("Tanya", MEMBER_OPERATOR, ""),
                    },
                );
                let kicked = state.csf_label("TXT_USER_KICKED", "TXT_USER_KICKED");
                let expected = crate::assets::csf_file::format_csf(
                    &kicked,
                    &[
                        crate::assets::csf_file::CsfArg::Str("Boris"),
                        crate::assets::csf_file::CsfArg::Str("Tanya's_game"),
                        crate::assets::csf_file::CsfArg::Str("Tanya"),
                    ],
                );
                let gone = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| !launch.humans.iter().any(|human| human.name == "Boris"));
                ensure!(
                    gone && setup_chat_has(state, &expected)?,
                    "Boris's kick: row gone {gone}, line {expected:?}"
                );
                self.event(frame, "Boris joined, then was kicked");
            }
            WolTarget::GuestTournamentRows => self.guest_tournament(state, frame)?,
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }
}
