//! BC/BD and C2/C9 commands after the shared window/control input owner
//! admits them.

mod start;
use super::*;
use crate::app::match_runtime::session::NetworkRowKind;
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};
use crate::ui::multiplayer_setup::*;

pub(crate) fn apply_action(state: &mut AppState, action: SetupAction) {
    let dialog = super::dialog(state);
    // While C2's StartGame request is pending (`00B77E28`), its proc skips
    // checkboxes and sliders (`0x78F823`), and Back, ChooseMap and every
    // row combo test the same flag (`0x790C5B`, `0x791033`, `0x791075`,
    // `0x791097`, `0x790D12`).
    if super::wol::start_pending(state)
        && matches!(
            action,
            SetupAction::Back
                | SetupAction::ChooseMap
                | SetupAction::Select { .. }
                | SetupAction::Toggle { .. }
                | SetupAction::Trackbar { .. }
        )
    {
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
                queue(state, NetworkSetupRequest::Leave);
                // C9 Back (0x791E4A..5F) closes only when the leave call
                // fails; otherwise the channel-left event closes it.
                if !(dialog.wol() && !dialog.host) {
                    App::leave_shell_dialog(
                        state,
                        shell_transition::ShellExitThen::NetworkSetupBack(dialog),
                    );
                }
            }
            Ok(())
        }
        SetupAction::ChooseMap => {
            if state.match_state.session.host()
                && state
                    .frontend
                    .network_setup_protocol
                    .as_ref()
                    .is_some_and(|protocol| !protocol.locked)
            {
                App::leave_shell_dialog(
                    state,
                    shell_transition::ShellExitThen::NetworkSetupChooseMap(dialog),
                );
            }
            Ok(())
        }
        SetupAction::Toggle { control } => toggle(state, control),
        SetupAction::Trackbar { control, value } => trackbar(state, control, value),
        SetupAction::Select { control, value } => select(state, control, value),
        SetupAction::Chat(text) if dialog.wol() => {
            super::wol::chat(state, text);
            Ok(())
        }
        SetupAction::Emote => {
            super::wol::emote(state);
            Ok(())
        }
        SetupAction::Page => {
            // 0x5C2 runs the modal page dialog 0xCF (0078D120), a later
            // WOL chain; nothing is invented in its place.
            log::warn!("WOL page dialog 0xCF is not implemented");
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
            crate::app::shell_lan::append_chat(
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
    state.platform.window.request_redraw();
}

fn queue(state: &mut AppState, request: NetworkSetupRequest) {
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
            options.short_game = !options.short_game;
            if options.short_game {
                options.bases = true;
            }
        }
        0x3E8 => options.mcv_redeploy = !options.mcv_redeploy,
        0x54D => options.crates = !options.crates,
        0x69A => options.super_weapons = !options.super_weapons,
        0x69D => options.build_off_ally = !options.build_off_ally,
        _ => anyhow::bail!("unknown setup checkbox"),
    }
    state.match_state.session.set_network_options(options)?;
    host_option_edited(state)?;
    queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// C2 checkbox and slider handlers (`0x78F9C1`, `0x78FA97`) clear every
/// remote acceptance at once (`005E9850`); BC leaves that to peers' A0.
fn host_option_edited(state: &mut AppState) -> anyhow::Result<()> {
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
    if control == DIFFICULTY {
        state
            .frontend
            .multiplayer_shell_runtime
            .set_cooperative_difficulty(value);
    } else {
        let mut options = state
            .match_state
            .session
            .network_setup()
            .expect("view has setup")
            .options
            .clone();
        match control {
            SPEED => {
                options.game_speed =
                    crate::ui::skirmish_shell::game_speed_from_visual_position(value)
            }
            CREDITS => options.starting_credits = value,
            UNITS => options.unit_count = value,
            _ => anyhow::bail!("unknown setup slider"),
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
    let (column, row) =
        SetupColumn::decode(control).ok_or_else(|| anyhow::anyhow!("unknown setup combo"))?;
    if column == SetupColumn::Player {
        if value == 0 {
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
            .set_network_row_kind(row as u8, kind)?
        {
            queue(state, NetworkSetupRequest::Kick { name: peer.name });
        }
    } else {
        let mut slot = state
            .match_state
            .session
            .network_row_choices(row as u8)
            .ok_or_else(|| anyhow::anyhow!("setup row has no choices"))?;
        match column {
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
        state
            .match_state
            .session
            .set_network_row_choices(row as u8, slot.clone())?;
        let launch = state
            .match_state
            .session
            .network_setup()
            .expect("editable setup");
        if launch.local_player().raw_slot_index == Some(row as u8) {
            state
                .frontend
                .multiplayer_shell_runtime
                .update_local_preferences(launch.local_player(), launch.mode.id);
        }
        if !state.match_state.session.host() {
            let launch = state
                .match_state
                .session
                .network_setup()
                .expect("choices retain setup");
            queue(
                state,
                NetworkSetupRequest::PlayerChoices {
                    row: row as u8,
                    slot,
                    observer: launch.local_player().is_observer(),
                },
            );
            return Ok(());
        }
    }
    queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

fn accept(state: &mut AppState) -> anyhow::Result<()> {
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
    super::persist_setup_preferences(state);
    if let Some(launch) = state.match_state.session.network_setup()
        && let Err(error) = state
            .frontend
            .multiplayer_shell_runtime
            .validate_network_countries(launch)
    {
        // The dialog has already slid out; a refused start returns to a
        // fresh BB like the failed network StartScenario retry (52E75F).
        log::error!("Cannot admit network country identity: {error}");
        super::commit_back(state);
        return;
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
    let (accepted_random_map, guest_staging) = if state.match_state.session.host() {
        (
            state
                .frontend
                .random_map_retention
                .take_acceptance_for_loading(selected.as_deref()),
            None,
        )
    } else {
        let random_map = selected.as_deref().is_some_and(|file| {
            file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED)
        });
        let staging = state.frontend.multiplayer_shell_runtime.start_staging();
        (
            None,
            (random_map && staging.defined_cells().next().is_some()).then(|| {
                crate::app::shell_random_map::AcceptedRmgStartStaging::from_scenario_staging(
                    &staging,
                )
            }),
        )
    };
    match state.match_state.session.freeze_network_start(agreed_seed) {
        Ok(startup) => {
            state.frontend.multiplayer_setup = None;
            state.frontend.network_setup_protocol = None;
            state.frontend.skirmish_preview_texture = None;
            crate::app::loading::pump::begin_loading(
                state,
                crate::app::loading::pump::LoadingRequest::network(startup)
                    .with_accepted_random_map(accepted_random_map)
                    .with_start_staging(guest_staging),
            );
        }
        Err(error) => {
            log::error!("Could not freeze accepted LAN start: {error:#}");
            super::commit_back(state);
        }
    }
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
    let removed = state.match_state.session.select_network_map(
        crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(mode),
        file.clone(),
        map.player_capacity,
    )?;
    state
        .match_state
        .session
        .accept_network_map_metadata(metadata, None, true)?;
    for human in removed {
        queue(state, NetworkSetupRequest::Kick { name: human.name });
    }
    state.frontend.random_map_retention.select_map(&file);
    state
        .frontend
        .multiplayer_shell_runtime
        .set_preferred_game_mode(mode_id);
    state.frontend.skirmish_preview_texture = None;
    if super::dialog(state).wol() {
        // C2 ChooseMap 0x790F68: acceptance clears only for a new
        // filename or title; the advertisement always follows (78F000).
        if changed {
            super::wol::clear_guest_acceptance(state)?;
        }
        queue(state, NetworkSetupRequest::Advertise);
    }
    queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}
