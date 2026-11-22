//! BC/BD and C2/C9 commands after the shared window/control input owner
//! admits them.

mod start;
use super::*;
use crate::app::match_runtime::session::{NetworkRowKind, PeerChoicesChange};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};
use crate::ui::multiplayer_setup::*;
use anyhow::Context;
pub(crate) use start::begin_external_lan_go;
pub(super) use start::request_campaign_start;

/// While C2's StartGame request is pending (`00B77E28`), its proc skips
/// checkboxes and sliders (`0x78F823`), and Back, ChooseMap and every
/// row combo test the same flag (`0x790C5B`, `0x791033`, `0x791075`,
/// `0x791097`, `0x790D12`). VERA's online host holds the same edits, and its
/// room's own, from its Start click while the guests accept the final
/// settings. It may still leave then, or `kick` a guest from its Player list:
/// either cancels that start, so a stalled guest need not hold the room to
/// the start's deadline.
fn held_while_starting(phase: StartPhase, action: &SetupAction, kick: bool) -> bool {
    match action {
        SetupAction::Back => phase.submitted(),
        SetupAction::Select { .. } if kick => phase.submitted(),
        SetupAction::ChooseMap
        | SetupAction::Select { .. }
        | SetupAction::Toggle { .. }
        | SetupAction::Trackbar { .. }
        | SetupAction::Join { .. }
        | SetupAction::DropRow { .. }
        | SetupAction::Lobby => phase.in_progress(),
        _ => false,
    }
}

/// The host's Player list puts Open, Closed, Open Observer or an AI on a
/// guest's row, which kicks the guest (`select`).
fn kicks_guest(state: &AppState, action: &SetupAction) -> bool {
    let SetupAction::Select { control, value } = *action else {
        return false;
    };
    // 0 keeps the guest; the other two move it.
    !matches!(value, 0 | MOVE_TO_SPECTATORS | GIVE_SEAT)
        && matches!(SetupColumn::decode(control), Some((SetupColumn::Player, _)))
        && super::view(state)
            .and_then(|view| view.combo(control).map(|combo| combo.row))
            .is_some_and(|row| guest_on_row(state, row).is_some())
}

/// The guest, not the host itself, on a host's row, with its team.
fn guest_on_row(state: &AppState, row: u8) -> Option<(usize, LaunchTeam)> {
    let session = &state.match_state.session;
    let launch = session.network_setup().filter(|_| session.host())?;
    launch
        .humans
        .iter()
        .enumerate()
        .find(|(index, human)| {
            *index != launch.local_player_index && human.raw_slot_index == Some(row)
        })
        .map(|(index, human)| (index, human.slot.team))
}

pub(crate) fn apply_action(state: &mut AppState, action: SetupAction) {
    let dialog = super::dialog(state);
    let phase = super::start_phase(state);
    let kick = phase.in_progress() && kicks_guest(state, &action);
    if held_while_starting(phase, &action, kick) {
        return;
    }
    let result = match action {
        SetupAction::Start => start::request_start(state),
        SetupAction::Accept => accept(state),
        SetupAction::Back => {
            if state
                .frontend
                .network_setup_protocol
                .as_ref()
                .is_some_and(|protocol| !protocol.locked)
            {
                if !dialog.wol() && state.match_state.lan_service.is_some() {
                    //5E09CD..5E0AC6 services the direct Type10 drain before
                    // leaving. The visit owner bounds VERA's larger FIFO;
                    // its completion resumes this ordinary Back transition.
                    if let Err(error) = crate::app::lan_service::request_setup_back(state) {
                        log::error!("LAN setup departure failed: {error:#}");
                    }
                } else {
                    // WOL C2 Back's leave (0x7900B8) goes to the lobby's
                    // service once C2 has closed (commit_back).
                    if !(dialog.wol() && dialog.host) {
                        queue(state, NetworkSetupRequest::Leave);
                    }
                    // C9 Back (0x791E4A..5F) closes only when the leave call
                    // fails; otherwise the channel-left event closes it.
                    if !(dialog.wol() && !dialog.host) {
                        App::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellSlideKind::NetworkSetup(
                                dialog,
                            ),
                            crate::app::shell_network_setup::commit_back,
                        );
                    }
                }
            }
            Ok(())
        }
        SetupAction::ChooseMap => {
            if state.match_state.session.campaign_room().is_none()
                && state.match_state.session.host()
                && state
                    .frontend
                    .network_setup_protocol
                    .as_ref()
                    .is_some_and(|protocol| !protocol.locked)
            {
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::NetworkSetup(dialog),
                    crate::app::shell_network_setup::open_choose_map,
                );
            }
            Ok(())
        }
        SetupAction::Toggle { control } => toggle(state, control),
        SetupAction::Trackbar { control, value } => trackbar(state, control, value),
        SetupAction::Select { control, value } => select(state, control, value),
        SetupAction::Join { control } => join(state, control),
        SetupAction::DropRow { row, group } => drop_row(state, row, group),
        SetupAction::Chat(text) if dialog.wol() => {
            super::wol::chat(state, text);
            Ok(())
        }
        SetupAction::Emote => {
            super::wol::emote(state);
            Ok(())
        }
        // 0x00790C8D (C2) and 0x0079235F (C9): the modal page dialog 0xCF.
        SetupAction::RosterPage { forward } => {
            if let Some(view) = super::view(state)
                && !view.locked
                && let Some(ui) = state.frontend.multiplayer_setup.as_mut()
            {
                ui.page_rows(&view.pages, forward);
                crate::app::input::multiplayer_setup::drop_pointer(state, true);
            }
            Ok(())
        }
        SetupAction::GameOptions(open) => {
            crate::app::input::multiplayer_setup::drop_pointer(state, true);
            if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
                ui.show_game_options(open);
            }
            Ok(())
        }
        SetupAction::Lobby => {
            crate::app::shell_wol::lobby::visit_lobby(state);
            Ok(())
        }
        SetupAction::MapOffer { download } => {
            crate::app::input::multiplayer_setup::drop_pointer(state, false);
            let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
                return;
            };
            if download {
                if let Some(offer) = ui.map_offer.as_mut() {
                    offer.downloading = true;
                }
            } else {
                // Without the map the player cannot play here.
                ui.map_offer = None;
                apply_action(state, SetupAction::Back);
            }
            Ok(())
        }
        SetupAction::Page => {
            crate::app::shell_wol::page::open_page_dialog(state);
            Ok(())
        }
        SetupAction::Chat(text) => {
            // Original BC/BD chat is admitted even during its GO input lock.
            let name = state
                .match_state
                .session
                .local_player_name()
                .unwrap_or_default()
                .to_owned();
            let display = format!("[{name}] {}", String::from_utf16_lossy(&text));
            crate::app::shell_lan::append_room_chat(
                state,
                crate::ui::lan_shell::LanChatLine {
                    text: display.encode_utf16().collect(),
                    color: 0x40ffff,
                },
            );
            queue(state, NetworkSetupRequest::Chat(text));
            Ok(())
        }
    };
    if let Err(error) = result {
        log::warn!("network setup command refused: {error:#}");
    }
}

pub(super) fn queue(state: &mut AppState, request: NetworkSetupRequest) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.requests.push(request);
    }
}

fn toggle(state: &mut AppState, control: u16) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    anyhow::ensure!(current.enabled(control), "disabled checkbox");
    let mut options = state
        .match_state
        .session
        .network_setup()
        .expect("view has setup")
        .options
        .clone();
    match control {
        0x54E => {
            crate::skirmish_launch::click_short_game(&mut options.short_game, &mut options.bases)
        }
        BASES => {
            crate::skirmish_launch::click_bases(&mut options.bases, &mut options.short_game)
        }
        // C2's other hidden session checkboxes: Multi-Engineer (0x78FB2B),
        // Harvester Truce (0x78FBBB) and Fog of War (0x78FC07).
        MULTI_ENGINEER => options.multi_engineer = !options.multi_engineer,
        HARVESTER_TRUCE => options.harvester_truce = !options.harvester_truce,
        FOG_OF_WAR => options.fog_of_war = !options.fog_of_war,
        0x3E8 => options.mcv_redeploy = !options.mcv_redeploy,
        0x54D => options.crates = !options.crates,
        0x69A => options.super_weapons = !options.super_weapons,
        0x69D => options.build_off_ally = !options.build_off_ally,
        // VERA's room adds these in its Game Options.
        BRIDGES => options.bridges_destroyable = !options.bridges_destroyable,
        ALLYING => options.ally_change_allowed = !options.ally_change_allowed,
        NO_YURI => options.bans.yuri = !options.bans.yuri,
        NO_FRANCE => options.bans.france = !options.bans.france,
        NO_SPY => options.bans.spy = !options.bans.spy,
        NO_STOLEN_TECH => options.bans.stolen_tech = !options.bans.stolen_tech,
        TERRAIN_DEFORMATION => {
            let rules = &mut options.host_rules;
            rules.terrain_deformation = !rules.terrain_deformation;
        }
        BUILDING_QUEUE => {
            let rules = &mut options.host_rules;
            rules.building_production_queue = !rules.building_production_queue;
        }
        _ => anyhow::bail!("unknown setup checkbox"),
    }
    state.match_state.session.set_network_options(options)?;
    super::bans::clear_banned_rows(state)?;
    host_option_edited(state)?;
    queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// C2 checkbox and slider handlers (`0x78F9C1`, `0x78FA97`) clear every
/// remote acceptance at once (`005E9850`); BC leaves that to peers' A0.
pub(super) fn host_option_edited(state: &mut AppState) -> anyhow::Result<()> {
    if super::dialog(state).wol() {
        super::wol::clear_guest_acceptance(state)?;
    }
    Ok(())
}

fn trackbar(state: &mut AppState, control: u16, value: i32) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    anyhow::ensure!(current.enabled(control), "disabled slider");
    let range = current
        .trackbars
        .iter()
        .find(|bar| bar.id == control)
        .ok_or_else(|| anyhow::anyhow!("unknown setup slider"))?
        .range;
    let value = range.stepped(range.set_up(value));
    if control == DIFFICULTY && state.match_state.session.campaign_room().is_some() {
        return super::campaign_room::set_difficulty(state, value);
    }
    if control == DIFFICULTY {
        state
            .frontend
            .multiplayer_shell_runtime
            .set_cooperative_difficulty(value);
    } else if control == TEAM_COUNT {
        // 1 is Off.
        let teams = u8::try_from(value).ok().filter(|teams| *teams >= 2);
        if state
            .match_state
            .session
            .network_setup()
            .is_some_and(|launch| launch.options.teams == teams)
        {
            return Ok(());
        }
        return super::teams::set_teams(state, teams).and_then(|()| host_option_edited(state));
    } else if control == ROOM_SIZE {
        // The room's Max players, which the online lobby lists; the next
        // Create Game starts from it.
        let people = u8::try_from(value).unwrap_or(u8::MAX);
        let session = &mut state.match_state.session;
        if session.network_people_limit() == Some(people) {
            return Ok(());
        }
        session.set_network_people_limit(people)?;
        state.frontend.room_limits.players = session.network_people_limit().unwrap_or(people);
        if super::dialog(state).wol() {
            queue(state, NetworkSetupRequest::Advertise);
        }
        return Ok(());
    } else {
        let mut options = state
            .match_state
            .session
            .network_setup()
            .expect("view has setup")
            .options
            .clone();
        let before = options.clone();
        match control {
            SPEED => {
                options.game_speed =
                    crate::ui::shell::game_speed::SpeedContext::Network.stored_speed(value)
            }
            CREDITS => {
                let credits = state
                    .frontend
                    .skirmish_shell_state
                    .trackbar_bounds
                    .credits_from_visual_position(value);
                if options.starting_credits == credits {
                    return Ok(());
                }
                options.starting_credits = credits;
            }
            UNITS => options.unit_count = value,
            // 1 is Off.
            SUPERWEAPON_LIMIT => {
                let limit = crate::rules::personal_rules::admitted_superweapon_limit(value);
                if options.host_rules.superweapon_limit == limit {
                    return Ok(());
                }
                options.host_rules.superweapon_limit = limit;
            }
            TECH_LEVEL => options.tech_level = value,
            _ => anyhow::bail!("unknown setup slider"),
        }
        // A drag sends only when its value moves.
        if options == before {
            return Ok(());
        }
        state.match_state.session.set_network_options(options)?;
    }
    host_option_edited(state)?;
    queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

fn select(state: &mut AppState, control: u16, value: i32) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    let combo = current
        .combo(control)
        .filter(|combo| combo.visible && combo.enabled && !current.locked)
        .ok_or_else(|| anyhow::anyhow!("disabled setup combo"))?;
    anyhow::ensure!(
        combo.choices.iter().any(|choice| choice.value == value),
        "selection no longer belongs to this popup"
    );
    let room_role = state.match_state.session.campaign_room().is_some()
        && matches!(
            SetupColumn::decode(control),
            Some((SetupColumn::Country, _))
        );
    if matches!(control, CAMPAIGN_SERIES | CAMPAIGN_STAGE) || room_role {
        return super::campaign_room::select(state, control, combo.row, value);
    }
    // The team bar's lists: the caption and a grey item do nothing.
    if matches!(control, TEAM_MENU | SEAT_MENU) {
        if combo
            .choices
            .iter()
            .any(|choice| choice.value == value && !choice.greyed)
        {
            return super::team_menus::select(state, control, value);
        }
        return Ok(());
    }
    if let Some(page_row) = team_add_ai_row(control) {
        let group = current.headers[page_row]
            .as_ref()
            .map(|header| header.group);
        return add_ai(state, group, combo.row, value);
    }
    let (column, _) =
        SetupColumn::decode(control).ok_or_else(|| anyhow::anyhow!("unknown setup combo"))?;
    let row = usize::from(combo.row);
    // VERA's host moves a guest as the guest's own choices would: into a
    // seat or to the spectators from its Player list; its side, colour,
    // start and team from the rest of its row, below.
    let guest = guest_on_row(state, combo.row);
    if let Some((index, team)) = guest {
        let observer = match value {
            MOVE_TO_SPECTATORS => Some(true),
            GIVE_SEAT => Some(false),
            _ => None,
        };
        if let Some(observer) = observer.filter(|_| column == SetupColumn::Player) {
            let change = super::service::move_player(state, index, team, observer)?;
            super::service::publish_choices(state, change);
            return Ok(());
        }
    }
    if column == SetupColumn::Player {
        // 0x5EC9F0 returns at once when the row keeps its item.
        if value == 0 || combo.selected.map(|index| combo.choices[index].value) == Some(value) {
            return Ok(());
        }
        let kind = match value {
            2 => NetworkRowKind::Open,
            3 => NetworkRowKind::Closed,
            4 => NetworkRowKind::Ai(AiDifficulty::Easy),
            5 => NetworkRowKind::Ai(AiDifficulty::Normal),
            6 => NetworkRowKind::Ai(AiDifficulty::Hard),
            7 => NetworkRowKind::OpenObserver,
            _ => anyhow::bail!("unknown setup row type"),
        };
        if let Some(peer) = state
            .match_state
            .session
            .choose_network_row_kind(row as u8, kind)?
        {
            let keep_out = state.match_state.session.network_several_observers();
            queue(
                state,
                NetworkSetupRequest::Kick {
                    name: peer.name,
                    keep_out,
                },
            );
        }
        // Then online ([0xAC116C]) the advertisement 0x78F000: the game
        // record and user limit count Open Observer rows.
        if super::dialog(state).wol() {
            queue(state, NetworkSetupRequest::Advertise);
        }
        queue(state, NetworkSetupRequest::PublishOptions);
        //5EC9F0 ->5DAF30 publishes the changed row bindings independently
        //of the options' participant array, including kicked/unbound nodes.
        queue(state, NetworkSetupRequest::PublishRoster);
        return Ok(());
    }
    let mut slot = state
        .match_state
        .session
        .network_row_choices(row as u8)
        .ok_or_else(|| anyhow::anyhow!("setup row has no choices"))?;
    match column {
        // Observer (-3) asks to watch and keeps the side.
        SetupColumn::Country if value == -3 => {}
        SetupColumn::Country => {
            slot.country_random = value == -2;
            if value >= 0 {
                slot.country = LaunchCountry::from_country_index(value as u32);
            }
        }
        SetupColumn::Color => {
            slot.color_random = value == -2;
            if value >= 0 {
                slot.color_index = value as u8;
            }
        }
        SetupColumn::Start => {
            slot.start_position = if value < 0 {
                LaunchStartPosition::Auto
            } else {
                LaunchStartPosition::Position(value as u8)
            }
        }
        SetupColumn::Team => slot.team = LaunchTeam::from_shell_value(value),
        SetupColumn::Player => unreachable!(),
    }
    // A VERA room's guest watches with Observer and plays with a side; the
    // host edits only a seated guest's row.
    let observer = if column == SetupColumn::Country {
        value == -3
    } else if guest.is_some() {
        false
    } else {
        state
            .match_state
            .session
            .network_setup()
            .expect("view has setup")
            .local_player()
            .is_observer()
    };
    if let Some((index, _)) = guest {
        let change = super::service::place_player(state, index, slot, observer)?;
        super::service::publish_choices(state, change);
        return Ok(());
    }
    commit_row(state, row as u8, slot, observer)
}

/// Writes the choices of a row this player edits, then sends them: a
/// guest's own to the host, which answers with its options; the host's in
/// its options.
fn commit_row(
    state: &mut AppState,
    row: u8,
    slot: crate::skirmish_launch::SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<()> {
    state
        .match_state
        .session
        .set_network_row_choices(row, slot.clone())?;
    let launch = state
        .match_state
        .session
        .network_setup()
        .expect("editable setup");
    if launch.local_player().raw_slot_index == Some(row) {
        state
            .frontend
            .multiplayer_shell_runtime
            .update_local_preferences(launch.local_player(), launch.mode.id);
    }
    if state.match_state.session.host() {
        // The host's own Watch, or Join from the spectators.
        if launch.local_player().is_observer() != observer {
            let index = launch.local_player_index;
            let change = super::service::place_player(state, index, slot, observer)?;
            super::service::publish_choices(state, change);
            return Ok(());
        }
        queue(state, NetworkSetupRequest::PublishOptions);
        return Ok(());
    }
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        // The host's answer may put the guest here (`service::tell_moved`).
        protocol.asked_place = Some((slot.team, observer));
        protocol.requests.push(NetworkSetupRequest::PlayerChoices {
            row,
            slot,
            observer,
        });
    }
    Ok(())
}

/// VERA's header Join: the player moves to the header's team as its Team
/// list does, a spectator into a free seat on it as a side in its Side list
/// does; the spectators' Watch asks to watch as Observer there does.
fn join(state: &mut AppState, control: u16) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    anyhow::ensure!(current.enabled(control), "Join is not offered");
    let group = current
        .header_button(control)
        .ok_or_else(|| anyhow::anyhow!("no header Join"))?
        .group;
    let local = state
        .match_state
        .session
        .network_setup()
        .expect("view has setup")
        .local_player();
    let row = local
        .raw_slot_index
        .ok_or_else(|| anyhow::anyhow!("the local player has no row"))?;
    let mut slot = local.slot.clone();
    let observer = match group {
        SetupGroup::Team(team) => {
            slot.team = LaunchTeam::Team(team);
            false
        }
        SetupGroup::Players => {
            slot.team = state.match_state.session.network_default_team();
            false
        }
        SetupGroup::NoTeam => anyhow::bail!("no team has no Join"),
        SetupGroup::ClosedSeats => anyhow::bail!("closed seats have no Join"),
        SetupGroup::Spectators => true,
    };
    commit_row(state, row, slot, observer)
}

/// The host drops a dragged roster row on another box ([`put_on_team`]).
fn drop_row(state: &mut AppState, row: u8, group: SetupGroup) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    let page_row = current
        .headers
        .iter()
        .position(|header| header.as_ref().is_some_and(|header| header.group == group));
    anyhow::ensure!(
        page_row.is_some_and(|header| current.drop_group(row, header) == Some(group)),
        "the row does not drop there"
    );
    let session = &state.match_state.session;
    let team = match group {
        SetupGroup::Team(team) => LaunchTeam::Team(team),
        SetupGroup::Players => session.network_default_team(),
        _ => LaunchTeam::None,
    };
    let observer = group == SetupGroup::Spectators;
    let change = put_on_team(state, row, team, observer)?;
    super::service::publish_choices(state, change);
    Ok(())
}

/// The host puts roster row `row` on `team`, or to watch keeping its team:
/// a person as the host's Team list or Seat and Move to spectators would
/// move them (`service::move_player`), an AI as its Team list would. The
/// caller publishes the change.
pub(super) fn put_on_team(
    state: &mut AppState,
    row: u8,
    team: LaunchTeam,
    observer: bool,
) -> anyhow::Result<PeerChoicesChange> {
    let session = &state.match_state.session;
    let launch = session.network_setup().context("no setup")?;
    if let Some(index) = launch
        .humans
        .iter()
        .position(|human| human.raw_slot_index == Some(row))
    {
        let team = if observer {
            launch.humans[index].slot.team
        } else {
            team
        };
        return super::service::move_player(state, index, team, observer);
    }
    let mut slot = session
        .network_row_choices(row)
        .ok_or_else(|| anyhow::anyhow!("the AI's row has no choices"))?;
    slot.team = team;
    state
        .match_state
        .session
        .set_network_row_choices(row, slot)?;
    Ok(PeerChoicesChange {
        roster: false,
        options: true,
    })
}

/// The host's Add AI on a box header: an AI of the picked difficulty in
/// the list's row, the room's AI seat
/// (`MatchSession::network_ai_seat`), on the
/// header's team (with Teams Off, the mode's default). It publishes as the
/// Player list's AI does.
fn add_ai(
    state: &mut AppState,
    group: Option<SetupGroup>,
    row: u8,
    value: i32,
) -> anyhow::Result<()> {
    let team = match group {
        Some(SetupGroup::Team(team)) => LaunchTeam::Team(team),
        Some(SetupGroup::Players) => state.match_state.session.network_default_team(),
        _ => anyhow::bail!("Add AI belongs to a box header"),
    };
    let difficulty = match value {
        0 => return Ok(()),
        4 => AiDifficulty::Easy,
        5 => AiDifficulty::Normal,
        6 => AiDifficulty::Hard,
        _ => anyhow::bail!("unknown AI difficulty"),
    };
    anyhow::ensure!(
        state.match_state.session.network_ai_seat() == Some(row),
        "Add AI's seat is no longer free"
    );
    state
        .match_state
        .session
        .choose_network_row_kind(row, NetworkRowKind::Ai(difficulty))?;
    let mut slot = state
        .match_state
        .session
        .network_row_choices(row)
        .ok_or_else(|| anyhow::anyhow!("the AI's row has no choices"))?;
    slot.team = team;
    state
        .match_state
        .session
        .set_network_row_choices(row, slot)?;
    if super::dialog(state).wol() {
        queue(state, NetworkSetupRequest::Advertise);
    }
    queue(state, NetworkSetupRequest::PublishOptions);
    queue(state, NetworkSetupRequest::PublishRoster);
    Ok(())
}

pub(super) fn accept(state: &mut AppState) -> anyhow::Result<()> {
    let current = super::view(state).ok_or_else(|| anyhow::anyhow!("no setup"))?;
    anyhow::ensure!(current.enabled(ACCEPT), "Accept is not eligible");
    let index = state
        .match_state
        .session
        .network_setup()
        .expect("view has setup")
        .local_player_index;
    state
        .match_state
        .session
        .set_network_player_ready(index, true)?;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.start_requested = false;
    }
    queue(state, NetworkSetupRequest::Ready(true));
    Ok(())
}

pub(crate) fn commit_start(state: &mut AppState, agreed_seed: u32) {
    if let Err(error) = try_commit_start(state, agreed_seed) {
        log::error!("Could not start the network match: {error:#}");
        if state.frontend.external_launch {
            App::fail_external_launch(state, &crate::i18n::tr("VERA:StartRefusedSetup"));
        } else if super::quick_match(state) {
            crate::app::shell_wol::start_aborted(state);
        } else {
            super::commit_back(state);
        }
    }
}

/// The lobby's watch admission and playable starts freeze the same world.
/// Each caller owns the return from a refusal.
pub(super) fn try_commit_start(state: &mut AppState, agreed_seed: u32) -> anyhow::Result<()> {
    if !super::quick_match(state) && !super::external(state) {
        super::persist_setup_preferences(state);
    }
    let campaign = super::campaign_room::prepared_start(state)?;
    if campaign.is_none()
        && let Some(launch) = state.match_state.session.network_setup()
    {
        state
            .frontend
            .multiplayer_shell_runtime
            .validate_network_countries(launch)
            .context("network country identity")?;
    }
    // The host's random map keeps the start staging its generator wrote
    // (Scenario +0x1140, 0x0059647B..0x00596565), as skirmish's does. A
    // guest's comes from the host's last 'W' (Scenario +0x11C0); without
    // one it has none, and the load is refused.
    // RESIDUAL: a native guest without a 'W' assigns starts from whatever
    // Scenario +0x11C0 last held.
    let selected = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.clone());
    let (accepted_random_map, mut guest_staging) = if campaign.is_some() {
        (None, None)
    } else if state.match_state.session.host() {
        (
            state
                .frontend
                .random_map_retention
                .take_acceptance_for_loading(selected.as_deref()),
            None,
        )
    } else {
        (
            None,
            crate::app::shell_random_map::AcceptedRmgStartStaging::from_retained_staging(
                &state.frontend.multiplayer_shell_runtime.start_staging(),
                selected.as_deref(),
            ),
        )
    };
    if campaign.is_none()
        && accepted_random_map.is_none()
        && guest_staging.is_none()
        && matches!(
            state.match_state.session_mode(),
            SessionMode::Lan | SessionMode::Wol
        )
        && selected.as_deref().is_some_and(|file| {
            file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED)
        })
    {
        // A network rematch reuses the accepted W table after the first match
        // consumed its presentation preview. Full_Init6873E6 copies these
        // same Scenario+11C0 cells; it does not require regenerating a preview.
        let staging = state.frontend.multiplayer_shell_runtime.start_staging();
        guest_staging = Some(
            crate::app::shell_random_map::AcceptedRmgStartStaging::from_scenario_staging(&staging),
        );
    }
    // VERA's LAN room comes back after the match; the original one does not.
    use crate::app::match_runtime::session::NetworkRoomReturn;
    let dialog = super::dialog(state);
    let room_return = if !dialog.modern || super::external(state) {
        NetworkRoomReturn::Lobby
    } else if dialog.wol() {
        if super::quick_match(state) {
            NetworkRoomReturn::Lobby
        } else {
            NetworkRoomReturn::Wol {
                cooperative: None,
                campaign: None,
            }
        }
    } else if dialog.host {
        NetworkRoomReturn::LanHost
    } else {
        crate::app::lan_service::admitted_host_room(state)
            .map(|(host, game_id)| NetworkRoomReturn::LanGuest {
                host,
                game_id,
                until: None,
            })
            .unwrap_or_default()
    };
    let startup = state
        .match_state
        .session
        .freeze_network_start(agreed_seed, campaign)
        .context("network start")?;
    state
        .match_state
        .session
        .set_network_room_return(room_return);
    if state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol {
        crate::app::shell_wol::match_started(state);
    }
    state.frontend.multiplayer_setup = None;
    state.frontend.network_setup_protocol = None;
    state.frontend.skirmish_preview_texture = None;
    crate::app::loading::pump::begin_loading(
        state,
        crate::app::loading::pump::LoadingRequest::network(startup)
            .with_accepted_random_map(accepted_random_map)
            .with_start_staging(guest_staging),
    );
    Ok(())
}

pub(crate) fn open_choose_map(state: &mut AppState) {
    // The shared chooser remains a transaction over the selected map. The
    // admitted setup is not edited until Use Map commits its result, and the
    // offline Skirmish dialog's retained selection is never touched.
    let Some(launch) = state.match_state.session.network_setup() else {
        return;
    };
    let mode_id = launch.mode.id;
    let record_index = launch.selected_map_file.as_deref().and_then(|file| {
        state
            .frontend
            .scenario_catalog
            .records()
            .iter()
            .position(|record| record.file_name.eq_ignore_ascii_case(file))
    });
    App::open_choose_map_modal(state, mode_id, record_index);
}

/// Use Map commits the shared chooser transaction into the admitted session.
/// Cancel never calls this path and leaves every accepted choice untouched.
pub(crate) fn commit_map_selection(
    state: &mut AppState,
    mode_id: i32,
    map_index: usize,
    row: Option<usize>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|p| !p.locked),
        "setup is locked"
    );
    let mode = crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, mode_id)
        .ok_or_else(|| anyhow::anyhow!("unknown chosen game mode"))?;
    let map = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .get(map_index)
        .ok_or_else(|| anyhow::anyhow!("unknown chosen map"))?;
    let file = map.file_name.clone();
    let metadata = super::local_map_metadata(state, &file)?;
    let mode_changed = state
        .match_state
        .session
        .network_setup()
        .is_none_or(|launch| launch.mode.id != mode_id);
    let changed = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .is_none_or(|current| !current.eq_ignore_ascii_case(&file))
        || state
            .match_state
            .session
            .network_map_metadata()
            .is_none_or(|current| current.display_name != metadata.display_name);
    // Use Map can reselect an already-uploaded source. Ask the same map
    // identity owner as received HostSetup before replacing its selection;
    // unchanged physical/effective bytes retain their image and marker bounds.
    let mut selected = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
        .clone();
    selected.mode = crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(mode);
    selected.selected_map_file = Some(file.clone());
    let map_changed = state
        .match_state
        .session
        .network_map_requires_verification(&selected, &metadata)?;
    let removed = state.match_state.session.select_network_map(
        selected.mode,
        file.clone(),
        map.player_capacity,
    )?;
    state
        .match_state
        .session
        .accept_network_map_metadata(metadata, None, true)?;
    for human in removed {
        queue(
            state,
            NetworkSetupRequest::Kick {
                name: human.name,
                keep_out: false,
            },
        );
    }
    state.frontend.random_map_retention.select_map(&file);
    state
        .frontend
        .multiplayer_shell_runtime
        .set_preferred_game_mode(mode_id);
    state.frontend.skirmish_preview_texture = None;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.invalidate_received_preview(map_changed);
    }
    if super::dialog(state).wol() {
        // C2 ChooseMap 0x790F68: acceptance clears only for a new
        // filename or title; the advertisement always follows (78F000).
        if changed {
            super::wol::clear_guest_acceptance(state)?;
        }
        queue(state, NetworkSetupRequest::Advertise);
    }
    // Use Map (`0x005E7160`) writes `[0xA8B254]` (0x005E7370), then runs
    // the mode's slots on LAN and WOL alike.
    super::note_scenario(state);
    super::cooperative::map_used(state, mode_changed, row.unwrap_or(0))?;
    queue(state, NetworkSetupRequest::PublishOptions);
    // A VERA room's spectators may move past the new players.
    if state.match_state.session.network_several_observers() {
        queue(state, NetworkSetupRequest::PublishRoster);
    }
    Ok(())
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// VERA's online host keeps its room still from its Start click, as C2
    /// does once StartGame is out, but may leave or kick a stalled guest
    /// while the guests accept.
    #[test]
    fn a_preparing_start_holds_the_hosts_edits_but_not_its_back_or_kick() {
        let edit = SetupAction::Select {
            control: 0,
            value: 0,
        };
        let drag = SetupAction::DropRow {
            row: 1,
            group: SetupGroup::Spectators,
        };
        let back = SetupAction::Back;
        assert!(held_while_starting(StartPhase::Preparing, &edit, false));
        assert!(held_while_starting(StartPhase::Preparing, &drag, false));
        assert!(!held_while_starting(StartPhase::Preparing, &back, false));
        assert!(!held_while_starting(StartPhase::Preparing, &edit, true));
        assert!(held_while_starting(StartPhase::Submitted, &back, false));
        assert!(held_while_starting(StartPhase::Submitted, &edit, true));
        assert!(!held_while_starting(StartPhase::Idle, &edit, false));
    }
}
