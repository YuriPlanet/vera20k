//! Decoded LAN/WOL service results. These inputs exercise the original shell
//! boundaries without implementing transport, file transfer or peers.

use super::*;
use crate::skirmish_launch::{LaunchHumanRole, SkirmishLaunchSession, SkirmishLocalSlot};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::skirmish_shell::SkirmishValidationModalState;
use std::time::Instant;

pub(crate) enum NetworkSetupEvent {
    ParticipantJoined(SkirmishHumanSlot),
    ParticipantLeft {
        index: usize,
    },
    #[expect(
        dead_code,
        reason = "the network transport, outside this change, delivers these packets"
    )]
    PlayerChoices {
        index: usize,
        choices: SkirmishLocalSlot,
        role: LaunchHumanRole,
    },
    PlayerReady {
        index: usize,
        accepted: bool,
    },
    MapStatus {
        index: usize,
        status: u8,
    },
    HostSetup {
        launch: SkirmishLaunchSession,
        map_metadata: crate::app::match_runtime::session::NetworkMapMetadata,
        vacancies: [NetworkVacancy; 8],
        observer_row: Option<u8>,
        cooperative_difficulty: i32,
        seed: Option<u32>,
    },
    StartPrompt,
    /// Accepted LAN protocol timing, independent of local input-delay settings.
    Timing {
        protocol: i32,
        response_times: [u32; 2],
    },
    #[expect(
        dead_code,
        reason = "the network transport, outside this change, delivers these packets"
    )]
    CooperativeAssignments(Vec<[[i32; 2]; 8]>),
    HostGo {
        agreed_seed: u32,
        max_ahead_frames: i32,
    },
    GoCompleted,
    MapTransferStarted,
    MapTransferProgress {
        completed: u32,
        total: u32,
    },
    MapTransferFinished {
        transferred_file: Option<String>,
    },
    HostLeft,
    Kicked,
    #[expect(
        dead_code,
        reason = "the network transport, outside this change, delivers these packets"
    )]
    Chat(LanChatLine),
    /// WOL OnGameStart (`CChatEventSink+0x5C`, `0x7A7C80`) with the server's
    /// user list and the ping table `0xB7790C` the timing block reads.
    GameStarted {
        users: Vec<super::WolStartUser>,
        pings: [i32; 8],
    },
    /// A WOL ping result for one user (`007A0EA0`).
    Ping {
        name: String,
        ping: i32,
    },
    /// WOL leave/part of the game channel: C9 Back completes on this.
    ChannelLeft,
}

pub(crate) fn service_event(state: &mut AppState, event: NetworkSetupEvent) -> anyhow::Result<()> {
    // The route stays NetworkSetup until the match returns, but GO's handoff
    // drops the setup protocol: later setup packets have no receiver.
    anyhow::ensure!(
        state.frontend.shell_route.network_setup()
            && state.frontend.network_setup_protocol.is_some(),
        "no network setup is active"
    );
    let dialog = super::dialog(state);
    // LAN-only packets: WOL negotiates timing and the map after OnGameStart
    // (0x794BA0), and its host-left/kick sinks are not established.
    anyhow::ensure!(
        !dialog.wol()
            || !matches!(
                event,
                NetworkSetupEvent::Timing { .. }
                    | NetworkSetupEvent::HostGo { .. }
                    | NetworkSetupEvent::GoCompleted
                    | NetworkSetupEvent::MapTransferStarted
                    | NetworkSetupEvent::MapTransferProgress { .. }
                    | NetworkSetupEvent::MapTransferFinished { .. }
                    | NetworkSetupEvent::HostLeft
                    | NetworkSetupEvent::Kicked
            ),
        "LAN setup packet in a WOL setup"
    );
    anyhow::ensure!(
        dialog.wol()
            || !matches!(
                event,
                NetworkSetupEvent::GameStarted { .. }
                    | NetworkSetupEvent::Ping { .. }
                    | NetworkSetupEvent::ChannelLeft
            ),
        "WOL setup event in a LAN setup"
    );
    match event {
        NetworkSetupEvent::ParticipantJoined(human) if dialog.wol() => {
            let name = human.name.clone();
            state.match_state.session.add_network_participant(human)?;
            // Join sink 0x7A6625: every acceptance clears, Accept enables.
            super::wol::clear_guest_acceptance(state)?;
            // Sink vt+0x28 plays [AudioVisual] PlayerJoined (0x750920) for a
            // remote game-channel join.
            let sound = state
                .rules()
                .and_then(|rules| rules.general.player_joined_sound())
                .map(str::to_owned);
            App::play_shell_ui_sound_by_id(state, sound.as_deref());
            if dialog.host {
                // 0x7A6E7D..9E: observer row, slot names, options, pings,
                // then the refreshed advertisement.
                let row = state
                    .match_state
                    .session
                    .network_observer_row()
                    .unwrap_or(8);
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.requests.extend([
                    NetworkSetupRequest::ObserverSlot { name, row },
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::RequestPings,
                    NetworkSetupRequest::Advertise,
                ]);
            }
        }
        NetworkSetupEvent::ParticipantJoined(human) => {
            state.match_state.session.add_network_participant(human)?;
            // Type3 admission5E0770 and accepted host type6 at5E1B32.
            // This adapter is the admitted setup (membership>=2); repeated
            // discovery packets never manufacture another participant/cue.
            let sound = state
                .rules()
                .and_then(|rules| rules.general.player_joined_sound())
                .map(str::to_owned);
            App::play_shell_ui_sound_by_id(state, sound.as_deref());
        }
        NetworkSetupEvent::ParticipantLeft { index } => {
            state
                .match_state
                .session
                .remove_network_participant(index)?;
            if dialog.wol() {
                // The WOL part sink calls no 0x750920 PlayerLeft (lead).
                clear_cooperative_partners(state);
                state.platform.window.request_redraw();
                return Ok(());
            }
            // Original5E10F2..5E1114 plays Rules+6BC only after endpoint
            // admission/removal while membershipAC0264>=2. This adapter is
            // admitted setup (state2), not a participant-count predicate.
            // Host disappearance has already cleared native membership and
            // uses the separate silent HostLeft return below.
            let sound = state
                .rules()
                .and_then(|rules| rules.general.player_left_sound())
                .map(str::to_owned);
            App::play_shell_ui_sound_by_id(state, sound.as_deref());
            clear_cooperative_partners(state);
        }
        NetworkSetupEvent::PlayerChoices {
            index,
            choices,
            role,
        } => {
            state
                .match_state
                .session
                .accept_network_player_choices(index, choices, role)?;
        }
        NetworkSetupEvent::PlayerReady { index, accepted } => {
            state
                .match_state
                .session
                .set_network_player_ready(index, accepted)?;
            // Native never automatically restarts when the last A1 arrives.
        }
        NetworkSetupEvent::MapStatus { index, status } => {
            anyhow::ensure!(status <= 2, "unknown native K status");
            state
                .match_state
                .session
                .set_network_map_status(index, status)?;
        }
        NetworkSetupEvent::HostSetup {
            launch,
            map_metadata,
            vacancies,
            observer_row,
            cooperative_difficulty,
            seed,
        } => {
            let before = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
                .clone();
            let map_changed = state
                .match_state
                .session
                .network_map_requires_verification(&launch, &map_metadata)?;
            // ParsePacket5E51C4..5E5409 verifies only map-field changes,
            // independently of the later general-option acceptance flag.
            // A qualified local record supplies its title;
            // otherwise the accepted host metadata remains sufficient for BD.
            let local_map = if map_changed {
                launch.selected_map_file.as_deref().and_then(|file| {
                    let config = state.platform.game_config.as_ref()?;
                    let assets = state.process_assets.manager()?;
                    state
                        .frontend
                        .scenario_catalog
                        .verify_network_map(file, &map_metadata, &config.paths.ra2_dir, assets)
                        .map(|verified| verified.record.display_name.clone())
                })
            } else {
                None
            };
            let map_admission = if map_changed {
                let file = launch
                    .selected_map_file
                    .as_deref()
                    .filter(|file| !file.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("host options have no map"))?;
                Some(map_metadata.admission(file, local_map.is_some()))
            } else {
                None
            };
            state.match_state.session.accept_network_host_setup(
                launch,
                vacancies,
                map_metadata.player_capacity,
                observer_row,
            )?;
            state.match_state.session.accept_network_map_metadata(
                map_metadata,
                local_map.as_deref(),
                map_changed,
            )?;
            if let Some(admission) = map_admission {
                apply_local_map_admission(state, admission)?;
            }
            // Full options5E5AAB updates persistent Color for the local name,
            // independently of raw ColorEx and the two country preferences.
            let local_color = state
                .match_state
                .session
                .network_setup()
                .expect("accepted setup")
                .local_slot()
                .color_index;
            state
                .frontend
                .multiplayer_shell_runtime
                .accept_network_local_color(local_color);
            // The owner validates source identities and normalizes raw AI row
            // order before the native material-change comparison.
            // Original5E5E0A..5E5E1A combines the independent mapsame and
            // general-options flags. Map verification/K is already complete;
            // metadata or mode changes must also retire local acceptance.
            let material = map_changed
                || settings_materially_changed(
                    &before,
                    state
                        .match_state
                        .session
                        .network_setup()
                        .expect("accepted setup"),
                )
                || state
                    .frontend
                    .multiplayer_shell_runtime
                    .cooperative_difficulty()
                    != cooperative_difficulty;
            state
                .frontend
                .multiplayer_shell_runtime
                .set_cooperative_difficulty(cooperative_difficulty);
            if let Some(seed) = seed {
                state.match_state.session.set_network_start_seed(seed)?;
            }
            if material {
                invalidate_local_acceptance(state)?;
            }
            state.frontend.skirmish_preview_texture = None;
        }
        NetworkSetupEvent::StartPrompt => {
            let index = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
                .local_player_index;
            let ready = state.match_state.session.network_player_ready(index);
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            protocol.start_requested = true;
            // WOL 7A2750 also enables Accept before flashing it.
            if dialog.wol() {
                protocol.accept_eligible = true;
            }
            if !ready {
                let text =
                    App::csf_label(state, "GUI:HostGameStartJoiner", "GUI:HostGameStartJoiner");
                crate::app::shell_lan::append_chat(
                    state,
                    LanChatLine {
                        text: text.encode_utf16().collect(),
                        color: u32::MAX,
                    },
                );
            }
        }
        NetworkSetupEvent::Timing {
            protocol,
            response_times,
        } => {
            anyhow::ensure!(
                state.match_state.session.host()
                    || state
                        .frontend
                        .network_setup_protocol
                        .as_ref()
                        .is_some_and(|p| p.locked),
                "guest final timing follows the accepted GO/map preparation"
            );
            let mut reads = 0;
            state
                .match_state
                .session
                .prepare_network_timing(protocol, || {
                    let value = response_times[reads];
                    reads += 1;
                    value
                })?;
        }
        NetworkSetupEvent::CooperativeAssignments(rounds) => {
            anyhow::ensure!(
                !state.match_state.session.host(),
                "host cannot receive its own campaign assignments"
            );
            state
                .frontend
                .multiplayer_shell_runtime
                .accept_cooperative_round_assignments(rounds);
        }
        NetworkSetupEvent::HostGo {
            agreed_seed,
            max_ahead_frames,
        } => {
            anyhow::ensure!(
                !state.match_state.session.host(),
                "host cannot receive guest-authored GO"
            );
            let current = state.match_state.session.network_start_seed();
            anyhow::ensure!(
                current.is_none_or(|seed| seed == agreed_seed),
                "GO changed the admitted seed"
            );
            state
                .match_state
                .session
                .set_network_start_seed(agreed_seed)?;
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            protocol.locked = true;
            protocol
                .requests
                .push(NetworkSetupRequest::PrepareMapForHandoff {
                    received_budget: max_ahead_frames,
                });
            state.frontend.skirmish_preview_texture = None;
        }
        NetworkSetupEvent::GoCompleted => {
            prepare_local_map_for_handoff(state)?;
            finish_go(state)?;
        }
        NetworkSetupEvent::MapTransferStarted => {
            state.frontend.loading_screen_atlas = None;
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            anyhow::ensure!(protocol.locked, "no GO service is pending");
            protocol.map_transfer_pending = true;
            protocol.transfer_progress = Some((0, 0));
        }
        NetworkSetupEvent::MapTransferProgress { completed, total } => {
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            anyhow::ensure!(protocol.map_transfer_pending, "no map transfer is active");
            let previous = protocol.transfer_progress.unwrap_or((0, 0));
            anyhow::ensure!(
                completed >= previous.0
                    && completed <= total
                    && (previous.1 == 0 || previous.1 == total),
                "map transfer must report monotonic unique chunk counts"
            );
            protocol.transfer_progress = Some((completed, total));
        }
        NetworkSetupEvent::MapTransferFinished { transferred_file } => {
            state.frontend.loading_screen_atlas = None;
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            anyhow::ensure!(protocol.map_transfer_pending, "no map transfer is active");
            protocol.transfer_progress = None;
            if let Some(file) = transferred_file {
                // Original693B70 returns the FILE_INFO filename after the
                // actual file receiver completes. DD8A0 then loads that file
                // directly, without registering a synthetic catalog record.
                state
                    .match_state
                    .session
                    .accept_prepared_network_map_filename(file)?;
                finish_go(state)?;
            } else {
                let message =
                    App::csf_label(state, "TXT_UNABLE_PLAY_WAAUGH", "TXT_UNABLE_PLAY_WAAUGH");
                let ok = App::csf_label(state, "TXT_OK", "OK");
                state
                    .frontend
                    .multiplayer_setup
                    .as_mut()
                    .expect("setup interaction")
                    .notice = Some(SkirmishValidationModalState::new(message, ok));
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.return_after_notice = true;
                protocol.go_deadline = None;
                state
                    .frontend
                    .shell_controller
                    .push(crate::ui::shell::descriptor::DialogId(0xCE), true);
            }
        }
        NetworkSetupEvent::HostLeft => {
            clear_cooperative_partners(state);
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::NetworkSetupBack(dialog),
            );
        }
        NetworkSetupEvent::Kicked => {
            crate::app::shell_lan::report_error(state, "TXT_REQUEST_DENIED");
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::NetworkSetupBack(dialog),
            );
        }
        NetworkSetupEvent::Chat(line) => crate::app::shell_lan::append_chat(state, line),
        NetworkSetupEvent::GameStarted { users, pings } => {
            super::wol::game_started(state, users, pings)?;
        }
        NetworkSetupEvent::Ping { name, ping } => super::wol::ping(state, name, ping)?,
        NetworkSetupEvent::ChannelLeft => {
            anyhow::ensure!(!dialog.host, "the C2 host closes on its own Back");
            App::leave_shell_dialog(
                state,
                shell_transition::ShellExitThen::NetworkSetupBack(dialog),
            );
        }
    }
    state.platform.window.request_redraw();
    Ok(())
}

fn prepare_local_map_for_handoff(state: &mut AppState) -> anyhow::Result<()> {
    // DD8A0 calls6951F0 again after GO, independently of the setup's earlier
    // title/K probe. Only this successful local-file path canonicalizes the
    // accepted filename at695451..695479. Transferred files take their actual
    // FILE_INFO filename through the separate completion arm above.
    let session = &state.match_state.session;
    if session.host() {
        return Ok(());
    }
    let file = session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .ok_or_else(|| anyhow::anyhow!("map preparation requires a selected map"))?;
    if file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED) {
        return Ok(());
    }
    let metadata = session
        .network_map_metadata()
        .ok_or_else(|| anyhow::anyhow!("map preparation requires host metadata"))?;
    let config = state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("map preparation requires the retail root"))?;
    let assets = state
        .process_assets
        .manager()
        .ok_or_else(|| anyhow::anyhow!("map preparation requires active retail sources"))?;
    let file = state
        .frontend
        .scenario_catalog
        .verify_network_map(file, metadata, &config.paths.ra2_dir, assets)
        .ok_or_else(|| {
            anyhow::anyhow!("map preparation completed without the advertised local map")
        })?
        .record
        .file_name
        .clone();
    state
        .match_state
        .session
        .accept_prepared_network_map_filename(file)
}

fn apply_local_map_admission(
    state: &mut AppState,
    admission: crate::app::match_runtime::session::NetworkMapAdmission,
) -> anyhow::Result<()> {
    let status = admission.status();
    let index = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
        .local_player_index;
    state
        .match_state
        .session
        .set_network_map_status(index, status)?;
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.accept_eligible = admission.accept_eligible();
    protocol
        .requests
        .push(NetworkSetupRequest::MapStatus(status));
    // Original map possession result prints the localized map name;
    // K2 is eligible because the service can transfer the missing map.
    if let Some(key) = match status {
        0 => Some("GUI:JoinerNoMap"),
        2 => Some("GUI:JoinerMapTransfer"),
        _ => None,
    } {
        let map_name = super::map_display_name(state).unwrap_or_default();
        let text = crate::assets::csf_file::format_csf(
            &App::csf_label(state, key, key),
            &[crate::assets::csf_file::CsfArg::Str(map_name)],
        );
        crate::app::shell_lan::append_chat(
            state,
            LanChatLine {
                text: text.encode_utf16().collect(),
                color: u32::MAX,
            },
        );
    }
    Ok(())
}

fn clear_cooperative_partners(state: &mut AppState) {
    // Accepted type10 departure5E1235 invokes mode+90. Ordinary modes are
    // no-ops; Cooperative5C3F80 clears49B760's retained partner buffers.
    if state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.mode.class.uses_cooperative_setup())
    {
        state
            .frontend
            .multiplayer_shell_runtime
            .clear_network_cooperative_partners();
    }
}

fn finish_go(state: &mut AppState) -> anyhow::Result<()> {
    anyhow::ensure!(
        state
            .match_state
            .session
            .network_max_ahead_frames()
            .is_some(),
        "GO completion requires final response-time timing, independently of the received budget"
    );
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no setup protocol"))?;
    anyhow::ensure!(protocol.locked, "no accepted GO is pending");
    protocol.go_deadline = None;
    protocol.map_transfer_pending = false;
    protocol.transfer_progress = None;
    let seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("GO has no admitted seed"))?;
    App::leave_shell_dialog(
        state,
        shell_transition::ShellExitThen::NetworkStart {
            dialog: super::dialog(state),
            agreed_seed: seed,
        },
    );
    Ok(())
}

fn invalidate_local_acceptance(state: &mut AppState) -> anyhow::Result<()> {
    let index = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
        .local_player_index;
    let ready = state.match_state.session.network_player_ready(index);
    if ready {
        state
            .match_state
            .session
            .set_network_player_ready(index, false)?;
    }
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.start_requested = false;
    if ready {
        protocol.requests.push(NetworkSetupRequest::Ready(false));
    }
    Ok(())
}

/// MPGameOptions5E3D10's material-change report consumed by5E3AB0.
/// Human color and AI color/difficulty are deliberately outside this test.
fn settings_materially_changed(old: &SkirmishLaunchSession, new: &SkirmishLaunchSession) -> bool {
    let a = &old.options;
    let b = &new.options;
    a.game_speed != b.game_speed
        || a.starting_credits != b.starting_credits
        || a.unit_count != b.unit_count
        || a.short_game != b.short_game
        || a.super_weapons != b.super_weapons
        || a.build_off_ally != b.build_off_ally
        || a.mcv_redeploy != b.mcv_redeploy
        || a.crates != b.crates
        || old.humans.len() != new.humans.len()
        || old
            .humans
            .iter()
            .zip(&new.humans)
            .any(|(a, b)| choices_material(&a.slot, &b.slot) || a.role != b.role)
        || old.opponents.len() != new.opponents.len()
        || old.opponents.iter().zip(&new.opponents).any(|(a, b)| {
            a.raw_slot_index != b.raw_slot_index
                || (!a.country_random && a.country != b.country)
                || a.country_random != b.country_random
                || a.start_position != b.start_position
                || a.team != b.team
        })
}
fn choices_material(a: &SkirmishLocalSlot, b: &SkirmishLocalSlot) -> bool {
    (!a.country_random && a.country != b.country)
        || a.country_random != b.country_random
        || a.start_position != b.start_position
        || a.team != b.team
}

pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let pings = super::wol::poll(state);
    let go = poll_go(state);
    pings.into_iter().chain(go).min()
}

fn poll_go(state: &mut AppState) -> Option<Instant> {
    let protocol = state.frontend.network_setup_protocol.as_ref()?;
    let deadline = protocol.go_deadline?;
    if protocol.map_transfer_pending {
        return None;
    }
    if Instant::now() >= deadline {
        if let Err(error) = finish_go(state) {
            // The start cannot proceed; leave like a failed network start.
            log::error!("LAN GO completion refused: {error:#}");
            super::commit_back(state);
        }
        None
    } else {
        Some(deadline)
    }
}
