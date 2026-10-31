//! Decoded LAN/WOL service results. The LAN transport and controlled captures
//! share these shell boundaries; this owner applies admission and UI effects.

use super::*;
use crate::skirmish_launch::{SkirmishLaunchSession, SkirmishLocalSlot};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::skirmish_shell::SkirmishValidationModalState;
use crate::util::time::Instant;

/// A host's full options as `0x005E3D10`'s default (digit-led) arm decodes
/// them: from the host, or in Quick Match from the matchbot's `Start`
/// (`[0xB73714]`), which the host applies too (`0x007A6DE2..0x007A6DF3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostOptions {
    pub(crate) launch: SkirmishLaunchSession,
    pub(crate) map_metadata: crate::app::match_runtime::session::NetworkMapMetadata,
    pub(crate) vacancies: [NetworkVacancy; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT],
    pub(crate) observer_row: Option<u8>,
    pub(crate) cooperative_difficulty: i32,
    pub(crate) seed: Option<u32>,
}

pub(crate) enum NetworkSetupEvent {
    ParticipantJoined(SkirmishHumanSlot),
    ParticipantLeft {
        index: usize,
    },
    /// A WOL guest's `R` to the host (`0x007A8260`).
    PlayerChoices {
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
    },
    PlayerReady {
        index: usize,
        accepted: bool,
    },
    MapStatus {
        index: usize,
        status: u8,
    },
    HostSetup(HostOptions),
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
    /// The host's `C0` campaign text (packet `0x17` at `+0x2F`).
    CooperativeCampaign(String),
    HostGo {
        agreed_seed: u32,
        max_ahead_frames: i32,
    },
    GoCompleted,
    /// Explicit capture input, never emitted by the physical transport.
    ControlledGoCompleted,
    /// Type13 from a participant in the host's current GO reply wait.
    GoAcknowledged {
        source_index: usize,
    },
    GoMapRequested {
        source_index: usize,
    },
    MapTransferStarted,
    MapTransferProgress {
        completed: u32,
        total: u32,
    },
    MapTransferFinished {
        transferred_file: Option<String>,
    },
    /// A guest's preview request `0x1B` reached the host (`0x005E014B`).
    PreviewRequested,
    /// The host's `'W'` start staging string (packet `0x17` at `+0x2F`).
    StartStaging(String),
    /// A guest's `0x1A` answer to the host's preview offer.
    PreviewAcknowledged {
        index: usize,
    },
    /// The host's preview offer `0x19` reached this guest (`0x005E0133`;
    /// WOL `0x00796710`).
    PreviewOffered,
    /// The WOL host's public `Z3`: some player cannot receive a preview
    /// (`0x005E3D10` arm `Z`).
    PreviewUnavailable,
    /// The offered file's receive ended; `None` when it failed.
    PreviewFileReceived {
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
        pings: Vec<i32>,
    },
    /// WOL Start Game Now's verify `0x7947B0` began; with connection pairs
    /// it checks node 0 (`0x00794809`).
    VerifyStarted {
        pairs: bool,
    },
    /// A peer's verify reply (`0x00793D20`): `node` in the start order.
    PeerConnected {
        node: usize,
    },
    /// The scenario negotiation `0x795CD0`'s result: 0 goes on to the
    /// match, 1 aborts silently, 2 aborts behind "Scenarios don't match".
    Negotiated {
        result: u8,
    },
    /// A WOL ping result for one user (`007A0EA0`).
    Ping {
        name: String,
        ping: i32,
    },
}

pub(crate) fn service_event(state: &mut AppState, event: NetworkSetupEvent) -> anyhow::Result<()> {
    // The route stays NetworkSetup until the match returns, but GO's handoff
    // drops the setup protocol: later setup packets have no receiver.
    anyhow::ensure!(
        (state.frontend.shell_route.network_setup()
            || super::quick_match(state)
            || super::external(state))
            && state.frontend.network_setup_protocol.is_some(),
        "no network setup is active"
    );
    let dialog = super::dialog(state);
    if super::external(state)
        && !dialog.wol()
        && matches!(
            event,
            NetworkSetupEvent::HostLeft
                | NetworkSetupEvent::ParticipantLeft { .. }
                | NetworkSetupEvent::Kicked
        )
    {
        // A direct spawn has no room to return to: a participant whose setup
        // lease expired before GO ends the launch.
        App::fail_external_launch(
            state,
            "a direct participant did not respond (unreachable, or its spawn.ini differs)",
        );
        return Ok(());
    }
    // LAN-only packets: WOL negotiates timing and the map after OnGameStart
    // (0x794BA0), and its host-left/kick sinks are not established.
    anyhow::ensure!(
        !dialog.wol()
            || !matches!(
                event,
                NetworkSetupEvent::Timing { .. }
                    | NetworkSetupEvent::HostGo { .. }
                    | NetworkSetupEvent::GoCompleted
                    | NetworkSetupEvent::ControlledGoCompleted
                    | NetworkSetupEvent::GoAcknowledged { .. }
                    | NetworkSetupEvent::GoMapRequested { .. }
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
                    | NetworkSetupEvent::PlayerChoices { .. }
                    | NetworkSetupEvent::VerifyStarted { .. }
                    | NetworkSetupEvent::PeerConnected { .. }
                    | NetworkSetupEvent::Negotiated { .. }
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
            let quick_match = super::quick_match(state);
            if dialog.host && quick_match {
                // 0x007A6DC2..0x007A6E50: modes 1/2 parse the matchbot's
                // Start payload [0xB73714] (0x5E3D10) and select its
                // scenario.
                match crate::app::shell_wol::quick_match::start_options(state) {
                    Some(options) => service_event(state, NetworkSetupEvent::HostSetup(options))?,
                    None => log::warn!("WOL Quick Match host has no matchbot options"),
                }
            }
            if dialog.host {
                // 0x7A6E5A: 0x5E2FA0, the mode's +94 (Cooperative's C0)
                // before the observer row, names and options.
                super::cooperative::guest_joined(state)?;
            }
            if dialog.host {
                // 0x7A6E7D..9E: observer row, slot names, options, pings,
                // then the refreshed advertisement (0x78F000, skipped in
                // modes 1-3 at 0x7A6B0D).
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
                ]);
                if !quick_match {
                    protocol.requests.push(NetworkSetupRequest::Advertise);
                }
            }
        }
        NetworkSetupEvent::ParticipantJoined(mut human) => {
            if dialog.host {
                // 0x5E1945..0x5E1973: the mode's +1C/+28 country check.
                super::cooperative::admit_join_country(state, &mut human.slot);
            }
            state.match_state.session.add_network_participant(human)?;
            // Type3 admission5E0770 and accepted host type6 at5E1B32.
            // This adapter is the admitted setup (membership>=2); repeated
            // discovery packets never manufacture another participant/cue.
            let sound = state
                .rules()
                .and_then(|rules| rules.general.player_joined_sound())
                .map(str::to_owned);
            App::play_shell_ui_sound_by_id(state, sound.as_deref());
            if dialog.host {
                // 0x5E1B5A..0x5E1BF2: the options, the 0x16 roster to every
                // guest, then 0x5E2FA0's mode +94.
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.republish_setup();
                super::cooperative::guest_joined(state)?;
            }
        }
        NetworkSetupEvent::ParticipantLeft { index } => {
            state
                .match_state
                .session
                .remove_network_participant(index)?;
            if dialog.wol() {
                // The part sink 0x785740 played PlayerLeft and clears the
                // acceptances itself (super::wol::user_left).
                super::cooperative::guest_left(state)?;
                state.platform.window.request_redraw();
                return Ok(());
            }
            // A VERA room's host moves its open spectator row after a leave;
            // online the part sink's options carry it.
            if dialog.host && state.match_state.session.network_several_observers() {
                state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol")
                    .republish_setup();
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
            super::cooperative::guest_left(state)?;
        }
        NetworkSetupEvent::PlayerChoices {
            index,
            choices,
            observer,
        } => {
            // Cooperative's mode +0x1C (0x5C17B0) admits its campaign's
            // countries; the other modes' (0x5D67A0) admit any.
            let countries = super::cooperative::current(state)
                .then(|| {
                    state
                        .frontend
                        .multiplayer_shell_runtime
                        .cooperative_player_countries()
                })
                .flatten();
            let change = state.match_state.session.accept_network_peer_choices(
                index,
                choices,
                observer,
                countries.as_deref(),
            )?;
            // 0x5EC9F0's advertisement 0x78F000, 0x77DB90's slot names,
            // then 0x5E32D0's options and the repaint 0x783C80.
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            if change.roster {
                protocol.requests.extend([
                    NetworkSetupRequest::Advertise,
                    NetworkSetupRequest::PublishRoster,
                ]);
            }
            if change.options {
                protocol.requests.push(NetworkSetupRequest::PublishOptions);
            }
            state.platform.window.request_redraw();
        }
        NetworkSetupEvent::PlayerReady { index, accepted } => {
            state
                .match_state
                .session
                .set_network_player_ready(index, accepted)?;
            // Native never automatically restarts when the last A1 arrives.
        }
        NetworkSetupEvent::MapStatus { index, status } => {
            // 0x5E39C0 (the K arm, 0x5E4C39): only the host screens C2/BC
            // take it (0x5E4C3D..0x5E4C57); the local node always holds its
            // map; K0 and K2 print the sender and map name.
            anyhow::ensure!(status <= 2, "unknown native K status");
            if !state.match_state.session.host() {
                return Ok(());
            }
            let launch = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?;
            let status = if index == launch.local_player_index {
                1
            } else {
                status
            };
            let name = launch
                .humans
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("no participant {index}"))?
                .name
                .clone();
            state
                .match_state
                .session
                .set_network_map_status(index, status)?;
            let key = match status {
                0 => Some("GUI:HostNoMap"),
                2 => Some("GUI:HostMapTransfer"),
                _ => None,
            };
            if let Some(key) = key {
                let map = super::map_display_name(state)
                    .unwrap_or_default()
                    .to_owned();
                super::chat_notice(
                    state,
                    key,
                    &[
                        crate::assets::csf_file::CsfArg::Str(&name),
                        crate::assets::csf_file::CsfArg::Str(&map),
                    ],
                );
            }
        }
        NetworkSetupEvent::HostSetup(HostOptions {
            launch,
            map_metadata,
            vacancies,
            observer_row,
            cooperative_difficulty,
            seed,
        }) => {
            // 0x5E3E73..0x5E3E90: a WOL parse drops every option text while
            // the game runs ([0xB77DC1]) or a StartGame is pending ([0xB77E28]).
            if dialog.wol()
                && (crate::app::shell_wol::in_match(state) || super::wol::start_pending(state))
            {
                return Ok(());
            }
            // Only a Quick Match host parses options it did not author.
            let quick_match = super::quick_match(state);
            anyhow::ensure!(
                !dialog.host || quick_match,
                "a host cannot receive guest-authored host setup"
            );
            if let Some(file) = launch.selected_map_file.as_deref()
                && let Err(error) = crate::map::source::validate_network_map_name(file)
            {
                fail_go_handoff(state);
                return Err(error);
            }
            let before = state
                .match_state
                .session
                .network_setup()
                .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
                .clone();
            // 0x5E5411..0x5E541D: the current mode's +24 on every parse.
            if super::cooperative::current(state) {
                state
                    .frontend
                    .multiplayer_shell_runtime
                    .cooperative_parts()
                    .progress
                    .request_refresh();
            }
            let mode_changed = before.mode.id != launch.mode.id;
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
                        .map(|verified| {
                            let loose = matches!(
                                verified.source,
                                crate::map::source::LoadedMapSource::Loose { .. }
                            );
                            (verified.record.display_name.clone(), loose)
                        })
                })
            } else {
                None
            };
            // 0x005E7604..0x005E76E8: a WOL guest skips the loose-file test
            // only for an unofficial name holding both ".MPR" and ".YRM".
            let local_map_loose = local_map.as_ref().is_some_and(|(_, loose)| {
                let upper = launch
                    .selected_map_file
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                *loose
                    && dialog.wol()
                    && (map_metadata.official
                        || !(upper.contains(".MPR") && upper.contains(".YRM")))
            });
            let local_map = local_map.map(|(name, _)| name);
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
                quick_match,
            )?;
            state.match_state.session.accept_network_map_metadata(
                map_metadata,
                local_map.as_deref(),
                map_changed,
            )?;
            // 0x5E542B..0x5E5466: a changed mode's +9C/+20, then +7C.
            if mode_changed {
                super::cooperative::mode_entered(state)?;
            }
            if let Some(admission) = map_admission {
                apply_local_map_admission(state, admission)?;
            }
            // 0x005E57DA → 0x005E74E0: a material change frees the preview
            // (0x005E74E9..0x005E7513); a LAN guest that cannot verify the
            // map (always so for RandMap.Sed) then asks the host for one,
            // once.
            if map_changed {
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.invalidate_received_preview(map_changed);
                // Native's receive blocks until it ends, so a file still in
                // flight belongs to the map before this change.
                protocol.awaiting_preview_file = false;
                protocol.local_map_verified = local_map.is_some();
                protocol.local_map_loose = local_map_loose;
                if local_map.is_none() && !dialog.host && !dialog.wol() {
                    protocol.requests.push(NetworkSetupRequest::RequestPreview);
                }
            }
            // 0x007A7F72: modes 1-3 skip the guest's preview handling.
            if dialog.wol() && !dialog.host && !quick_match {
                super::wol::options_preview(state, map_changed);
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
            if dialog.wol() && !dialog.host && quick_match {
                super::wol::quick_match_accept(state);
            }
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
            protocol.lan_go = Some(LanGoWait::GuestPreparing);
            protocol
                .requests
                .push(NetworkSetupRequest::PrepareMapForHandoff {
                    received_budget: max_ahead_frames,
                });
            state.frontend.skirmish_preview_texture = None;
        }
        NetworkSetupEvent::GoAcknowledged { source_index } => {
            anyhow::ensure!(
                dialog.host && !dialog.wol(),
                "GO reply has no LAN host receiver"
            );
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup");
            if let Some(phase) = protocol.lan_go.as_mut() {
                phase.acknowledge(source_index);
            }
        }
        NetworkSetupEvent::GoMapRequested { source_index } => {
            let required = state
                .frontend
                .network_setup_protocol
                .as_ref()
                .and_then(|protocol| protocol.lan_go.as_ref())
                .is_some_and(|phase| {
                    matches!(phase, LanGoWait::HostReplies { pending, .. }
                    if pending.contains(&source_index))
                });
            if required {
                // Native5DD6F8 records a file requester. This bounded local-map
                // path cannot serve that file; retain an explicit failure.
                fail_go_handoff(state);
            }
        }
        NetworkSetupEvent::GoCompleted => {
            anyhow::ensure!(
                state
                    .frontend
                    .network_setup_protocol
                    .as_ref()
                    .is_some_and(|protocol| matches!(protocol.lan_go, Some(LanGoWait::Complete))),
                "physical GO completion preceded its native wait"
            );
            finish_go(state)?;
        }
        NetworkSetupEvent::ControlledGoCompleted => {
            prepare_local_map_for_handoff(state)?;
            finish_go(state)?;
        }
        NetworkSetupEvent::PreviewRequested => {
            // 0x005E00B0 handles 0x1B only on the host's own dialog.
            anyhow::ensure!(
                dialog.host && !dialog.wol(),
                "only the LAN host answers 0x1B"
            );
            preview_requested(state)?;
        }
        NetworkSetupEvent::PreviewAcknowledged { index } => {
            anyhow::ensure!(dialog.host, "only the host waits for 0x1A");
            preview_acknowledged(state, index)?;
        }
        NetworkSetupEvent::PreviewOffered => {
            // The receive loop answers 0x19 only on BD (0x005E0133), whether
            // or not this guest asked; 0x005E7920 acks, then receives.
            // A receive in progress blocks native's loop; a second offer is
            // not answered on top of it. WOL's pump takes it only while no
            // start is pending and shows 0x468 first, leaving 0x71F as it is
            // (0x00796710).
            let wol_pending = dialog.wol() && super::wol::start_pending(state);
            let protocol = state
                .frontend
                .network_setup_protocol
                .as_mut()
                .expect("setup owns protocol");
            if !dialog.host && !wol_pending && !protocol.awaiting_preview_file {
                protocol.wol_preview_hidden = false;
                protocol.awaiting_preview_file = true;
                protocol.requests.extend([
                    NetworkSetupRequest::AcknowledgePreview,
                    NetworkSetupRequest::RequestPreviewFile,
                ]);
            }
        }
        NetworkSetupEvent::PreviewUnavailable => {
            anyhow::ensure!(dialog.wol(), "Z3 is a WOL options text");
            // The arm runs before the parse's start gate and tests only that
            // the local user is not the operator, then C9's 0x71F; it does
            // not test the guest's own preview.
            if !dialog.host {
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.wol_no_preview = true;
                protocol.wol_preview_hidden = true;
            }
        }
        NetworkSetupEvent::PreviewFileReceived { transferred_file } => {
            preview_file_received(state, transferred_file)?;
        }
        NetworkSetupEvent::CooperativeCampaign(text) => {
            super::cooperative::campaign_received(state, &text)?;
        }
        NetworkSetupEvent::StartStaging(text) => {
            // The 'W' arm is not gated on role or map type; a string short
            // of 37 tokens commits nothing. A WOL parse drops it while the
            // game runs or a start is pending (0x005E3E73..0x005E3E90).
            if dialog.wol()
                && (crate::app::shell_wol::in_match(state) || super::wol::start_pending(state))
            {
                return Ok(());
            }
            if state
                .frontend
                .multiplayer_shell_runtime
                .accept_start_staging_text(&text)
            {
                state.platform.window.request_redraw();
            }
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
            let awaiting_image = protocol.awaiting_random_map_image;
            if let Some(file) = transferred_file {
                if awaiting_image {
                    // The second receive's name is not kept; both files are
                    // now on disk (0x005DDA68 → 0x005DD97F).
                    finish_go(state)?;
                } else {
                    // Original693B70 returns the FILE_INFO filename after the
                    // actual file receiver completes. DD8A0 then loads that file
                    // directly, without registering a synthetic catalog record.
                    let random_map =
                        file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED);
                    state
                        .match_state
                        .session
                        .accept_prepared_network_map_filename(file)?;
                    if random_map {
                        // 0x005DDA5D: a random map's RandMap.img follows on
                        // its own receive and progress bar.
                        let protocol = state
                            .frontend
                            .network_setup_protocol
                            .as_mut()
                            .expect("setup owns protocol");
                        protocol.awaiting_random_map_image = true;
                        protocol.map_transfer_pending = false;
                        protocol
                            .requests
                            .push(NetworkSetupRequest::RequestRandomMapImage);
                    } else {
                        finish_go(state)?;
                    }
                }
            } else {
                fail_go_handoff(state);
            }
        }
        NetworkSetupEvent::HostLeft => {
            // The setup closes: only Cooperative +0x90's last step, the
            // names, matters.
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
        NetworkSetupEvent::VerifyStarted { pairs } => {
            if pairs {
                super::wol::verify_connected(state, 0)?;
            }
        }
        NetworkSetupEvent::PeerConnected { node } => super::wol::verify_connected(state, node)?,
        NetworkSetupEvent::Negotiated { result } => super::wol::negotiated(state, result)?,
    }
    state.platform.window.request_redraw();
    Ok(())
}

/// Verify through the existing map owner before the transport queues Type13.
/// A missing or mismatched map is not a successful local handoff.
pub(crate) fn prepare_lan_guest_handoff(state: &mut AppState) -> anyhow::Result<()> {
    anyhow::ensure!(
        state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_some_and(|protocol| matches!(protocol.lan_go, Some(LanGoWait::GuestPreparing))),
        "no guest GO map preparation is pending"
    );
    if let Err(error) = prepare_local_map_for_handoff(state) {
        fail_go_handoff(state);
        return Err(error);
    }
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("checked setup")
        .lan_go = Some(LanGoWait::GuestSending);
    Ok(())
}

pub(crate) fn fail_go_handoff(state: &mut AppState) {
    if state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| {
            matches!(
                protocol.lan_go,
                Some(LanGoWait::Failed | LanGoWait::HandingOff)
            )
        })
    {
        return;
    }
    if super::external(state) {
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.lan_go = Some(LanGoWait::Failed);
        }
        App::fail_external_launch(state, "the direct start handshake failed");
        return;
    }
    let message = App::csf_label(state, "TXT_UNABLE_PLAY_WAAUGH", "TXT_UNABLE_PLAY_WAAUGH");
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
        .expect("setup owner");
    protocol.return_after_notice = true;
    protocol.lan_go = Some(LanGoWait::Failed);
    state
        .frontend
        .shell_controller
        .push(crate::ui::shell::descriptor::DialogId(0xCE), true);
}

fn prepare_local_map_for_handoff(state: &mut AppState) -> anyhow::Result<()> {
    // DD8A0 calls6951F0 again after GO, independently of the setup's earlier
    // title/K probe. Only this successful local-file path canonicalizes the
    // accepted filename at695451..695479. Transferred files take their actual
    // FILE_INFO filename through the separate completion arm above.
    let session = &state.match_state.session;
    // A direct spawn's map is the launcher's file beside spawn.ini, loaded
    // by the same owner as a relay spawn's; no host options describe it.
    if session.host() || super::external(state) {
        return Ok(());
    }
    let file = session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .ok_or_else(|| anyhow::anyhow!("map preparation requires a selected map"))?;
    // VERA prefetches through the same verified source/preview owners while
    // setup is editable. DD8A0's post-GO local-file probe must not reject bytes
    // already received and admitted before GO; keep the logical scenario name
    // and retained physical source used by freeze_network_start/the loader.
    if let Some(source) = session.network_local_map_source() {
        let metadata = session
            .network_map_metadata()
            .ok_or_else(|| anyhow::anyhow!("map preparation requires host metadata"))?;
        anyhow::ensure!(
            source.physical_identity()
                == Some((
                    usize::try_from(metadata.byte_length)?,
                    metadata.source_sha256.as_str()
                ))
                && session.network_local_map_effective_digest()
                    == Some(metadata.effective_digest.as_str()),
            "retained map source differs from accepted options"
        );
        if crate::map::rmg::is_seed_selection(file) {
            anyhow::ensure!(
                session.network_random_map_preview().is_some(),
                "generated map preview has not been verified"
            );
            super::maps::checked_staging(
                &state
                    .frontend
                    .multiplayer_shell_runtime
                    .start_staging()
                    .to_w_text(),
                metadata.player_capacity,
            )?;
        }
        return Ok(());
    }
    // 0x006951F0 always fails for RandMap.Sed (0x0069520A), so a random map
    // is always transferred and never completes GO from a local file.
    anyhow::ensure!(
        !file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED),
        "a random map is always transferred at GO"
    );
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

/// The host's 0x1B arm (`0x005E014B..0x005E0172`): `0x005E9850` clears the
/// acceptance of every node but the local one (the packet names no sender),
/// then [`send_preview`].
fn preview_requested(state: &mut AppState) -> anyhow::Result<()> {
    if preview_offer_pending(state) {
        // The ack wait's message loop discards this packet.
        return Ok(());
    }
    state.match_state.session.clear_guest_readiness()?;
    send_preview(state)
}

pub(super) fn preview_offer_pending(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| protocol.preview_offer.is_some())
}

/// `0x005E7EB0`, shared by LAN's `0x1B` and WOL's `P`: given the host's
/// preview and more than one node, it sends the `'W'` start staging and
/// offers the preview (`0x19`) to every guest.
/// RESIDUAL: WOL sends `Z3` instead of the offer when a player reported
/// a NAT port or address in its `R` (`0x005E7EB0` mode 4); those `R` fields
/// are not stored, so the offer always goes out.
/// RESIDUAL: the `'W'` for an authored map takes its header from Scenario
/// (`[Header]`, `0x00689D30`) but its pairs from the random-map globals
/// (`0x0058BB00`: `0xABE328` below `0xABED0C`), which this staging owner does
/// not hold apart, so only a random map's `'W'` and offer are sent. The host
/// dialog also blocks while it waits and drops every other packet
/// (`0x005E8269..0x005E8318`); here other events still arrive.
pub(super) fn send_preview(state: &mut AppState) -> anyhow::Result<()> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?;
    let has_guests = launch.humans.len() > 1;
    let random_map = launch
        .selected_map_file
        .as_deref()
        .is_some_and(|file| file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED));
    if !(random_map && has_guests) {
        return Ok(());
    }
    let Some(preview) = host_preview(state) else {
        return Ok(());
    };
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?;
    let guests = launch
        .humans
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != launch.local_player_index)
        .map(|(_, human)| human.name.clone())
        .collect();
    let text = state
        .frontend
        .multiplayer_shell_runtime
        .start_staging()
        .to_w_text();
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.requests.extend([
        NetworkSetupRequest::PublishStartStaging(text),
        NetworkSetupRequest::OfferPreview,
    ]);
    protocol.preview_offer = Some(super::PreviewOffer {
        guests,
        acknowledged: Vec::new(),
        preview,
        deadline: Instant::now() + PREVIEW_ACK_WAIT,
    });
    Ok(())
}

/// The host's offer waits 1800 buckets for `0x1A` (`0x005E81D0`).
const PREVIEW_ACK_WAIT: std::time::Duration = std::time::Duration::from_millis(1800 * 16);
const PREVIEW_FILE: &str = "Preview.bin";

/// The host's `[0xAC1154]`: the preview of the map BC shows.
fn host_preview(state: &AppState) -> Option<crate::map::preview::DecodedPreview> {
    let file = state
        .match_state
        .session
        .network_setup()?
        .selected_map_file
        .clone()?;
    crate::app::frontend::skirmish_shell_render::decode_network_map_preview(state, &file)
}

/// A `0x1A` from a guest the offer went to and the host has not counted
/// yet; when every one has answered, the host writes `Preview.bin` from the
/// offered preview and sends it (`0x005E8336..0x005E847E`). Joins and leaves
/// during the wait do not change who must answer: native's wait discards
/// their packets.
fn preview_acknowledged(state: &mut AppState, index: usize) -> anyhow::Result<()> {
    let Some(name) = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
        .humans
        .get(index)
        .map(|human| human.name.clone())
    else {
        return Ok(());
    };
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    let Some(offer) = protocol.preview_offer.as_mut() else {
        return Ok(());
    };
    if !offer.guests.contains(&name) || offer.acknowledged.contains(&name) {
        return Ok(());
    }
    offer.acknowledged.push(name);
    if offer.acknowledged.len() < offer.guests.len() {
        return Ok(());
    }
    let offer = protocol.preview_offer.take().expect("offer completed");
    let blob = crate::map::preview::create_paletted_preview(
        &offer.preview,
        crate::map::preview::PALETTED_PREVIEW_COLORS,
    );
    let path = state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no install directory for Preview.bin"))?
        .paths
        .ra2_dir
        .join(PREVIEW_FILE);
    // RawFileClass deletes any old file, then writes the length and the
    // compressed blob.
    if path.exists() {
        crate::util::file_system::remove_file(&path)?;
    }
    crate::util::file_system::write(&path, crate::map::preview::preview_bin_bytes(&blob))?;
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol")
        .requests
        .push(NetworkSetupRequest::SendPreviewFile);
    Ok(())
}

/// The guest's receive of the offered file (`0x005E7920`): on success it
/// decodes `Preview.bin` into its preview and redraws; on failure it keeps
/// none and shows nothing.
fn preview_file_received(
    state: &mut AppState,
    transferred_file: Option<String>,
) -> anyhow::Result<()> {
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    if !protocol.awaiting_preview_file {
        // The receive was for the map before a change; nothing installs it.
        return Ok(());
    }
    protocol.awaiting_preview_file = false;
    let Some(file) = transferred_file else {
        log::info!("LAN preview receive failed; BD keeps no preview");
        return Ok(());
    };
    let path = state
        .platform
        .game_config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no install directory for {file}"))?
        .paths
        .ra2_dir
        .join(&file);
    let image = crate::util::file_system::read(&path)
        .ok()
        .and_then(|bytes| crate::map::preview::preview_bin_blob(&bytes))
        .and_then(|blob| crate::map::preview::paletted_preview_image(&blob));
    let Some(image) = image else {
        log::warn!("LAN preview {} did not decode", path.display());
        return Ok(());
    };
    install_received_preview_image(state, image)
}

pub(super) fn install_received_preview_image(
    state: &mut AppState,
    mut image: crate::map::preview::DecodedPreview,
) -> anyhow::Result<()> {
    // CreatePreviewSurface writes the colours into the 16-bit surface.
    let format = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    for pixel in image.rgba.as_chunks_mut::<4>().0 {
        let [r, g, b] = format.storage_roundtrip_rgb8([pixel[0], pixel[1], pixel[2]]);
        *pixel = [r, g, b, pixel[3]];
    }
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    protocol.accept_preview_bounds(None);
    protocol.received_preview_count += 1;
    protocol.received_preview = Some((protocol.received_preview_count, image));
    state.platform.window.request_redraw();
    Ok(())
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

/// Verified map prefetch uses the editable lobby's existing progress owner
/// before either LAN GO or WOL negotiation starts.
pub(crate) fn set_network_map_progress(state: &mut AppState, progress: Option<(u32, u32)>) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.transfer_progress = progress;
        if progress.is_some() && !state.match_state.session.host() {
            protocol.accept_eligible = false;
        }
    }
    state.platform.window.request_redraw();
}

pub(crate) fn accept_network_map_source(
    state: &mut AppState,
    source: crate::map::source::LoadedMapSource,
    effective_digest: String,
) -> anyhow::Result<()> {
    state
        .match_state
        .session
        .accept_verified_network_map_source(source, effective_digest)?;
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("no editable network setup"))?;
    protocol.accept_verified_map_source();
    apply_local_map_admission(
        state,
        crate::app::match_runtime::session::NetworkMapAdmission::Present,
    )
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
    // Keep dequeue deferred until the transition removes this protocol.
    protocol.lan_go = Some(LanGoWait::HandingOff);
    protocol.map_transfer_pending = false;
    protocol.transfer_progress = None;
    let seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("GO has no admitted seed"))?;
    if super::external(state) {
        // A direct spawn has no dialog to slide out.
        super::commit_start(state, seed);
        return Ok(());
    }
    App::leave_shell_dialog(
        state,
        shell_transition::ShellExitThen::NetworkStart {
            dialog: super::dialog(state),
            agreed_seed: seed,
        },
    );
    Ok(())
}

/// Apply an authoritative revision through the same admission/protocol owners as
/// native unacceptance. Existing map-transfer eligibility remains decisive.
pub(crate) fn invalidate_network_agreement(state: &mut AppState) -> anyhow::Result<()> {
    state.match_state.session.invalidate_network_agreement()?;
    let local_map = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| {
            state
                .match_state
                .session
                .network_map_status(launch.local_player_index)
        });
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.start_requested = false;
        protocol.accept_eligible = local_map == Some(1) && protocol.transfer_progress.is_none();
        protocol
            .requests
            .retain(|request| !matches!(request, NetworkSetupRequest::Ready(_)));
    }
    state.platform.window.request_redraw();
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
    auto_accept(state);
    let pings = super::wol::poll(state);
    let go = poll_go(state);
    let offer = poll_preview_offer(state);
    // A direct spawn's LAN service renews setup leases and waits for its
    // peers before GO; keep the frame loop servicing it.
    let direct = (super::external(state) && !super::dialog(state).wol())
        .then(|| Instant::now() + std::time::Duration::from_millis(16));
    pings.into_iter().chain(go).chain(offer).chain(direct).min()
}

/// VERA's game room (`SetupDialog::modern`) presses Accept for a guest
/// whenever the player could, so nobody readies up by hand: after joining,
/// after every host change, and for the final settings Start publishes.
/// Quick Match and external launches have no setup dialog.
fn auto_accept(state: &mut AppState) {
    let dialog = super::dialog(state);
    if !dialog.modern
        || dialog.host
        || super::quick_match(state)
        || super::external(state)
        || state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_none_or(|protocol| protocol.locked)
        || !super::view::accept_enabled(state)
    {
        return;
    }
    if let Err(error) = super::actions::accept(state) {
        log::warn!("automatic Accept refused: {error:#}");
    }
    state.platform.window.request_redraw();
}

/// The service could not take the local acceptance. It stays withdrawn
/// until the room changes (a new revision or a verified map makes Accept
/// eligible again), so the automatic Accept never repeats a refused request.
pub(crate) fn refuse_local_acceptance(state: &mut AppState) -> anyhow::Result<()> {
    let index = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no admitted setup"))?
        .local_player_index;
    state
        .match_state
        .session
        .set_network_player_ready(index, false)?;
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.accept_eligible = false;
    }
    state.platform.window.request_redraw();
    Ok(())
}

/// Fewer than every guest answered within 1800 buckets: the host gives up
/// with no `Preview.bin` and no message (`0x005E8326`).
fn poll_preview_offer(state: &mut AppState) -> Option<Instant> {
    let protocol = state.frontend.network_setup_protocol.as_mut()?;
    let deadline = protocol.preview_offer.as_ref()?.deadline;
    if Instant::now() >= deadline {
        protocol.preview_offer = None;
        None
    } else {
        Some(deadline)
    }
}

fn poll_go(state: &mut AppState) -> Option<Instant> {
    let phase = state
        .frontend
        .network_setup_protocol
        .as_ref()?
        .lan_go
        .as_ref()?;
    let now = super::super::match_runtime::frame_pacer::frame_bucket(
        super::super::match_runtime::frame_pacer::wall_clock_ms(),
    );
    let buckets = match phase {
        LanGoWait::HostReplies { timer, .. } => timer.remaining(now),
        LanGoWait::HostSending { .. }
        | LanGoWait::HostDrain { .. }
        | LanGoWait::GuestPreparing
        | LanGoWait::GuestSending
        | LanGoWait::GuestDrain => 1,
        LanGoWait::Complete | LanGoWait::HandingOff | LanGoWait::Failed => return None,
    };
    // Scheduling is a derived view; only transport observations advance GO.
    Some(Instant::now() + std::time::Duration::from_millis(u64::from(buckets) * 16))
}

/// Verified RandMap.img keeps the existing preview surface owner. Its local
/// path is retained separately from the shared logical scenario name.
pub(crate) fn accept_network_random_map_preview(
    state: &mut AppState,
    seed_digest: &str,
    source: crate::map::source::LoadedMapSource,
    image: crate::map::preview::DecodedPreview,
) -> anyhow::Result<()> {
    state
        .match_state
        .session
        .accept_network_random_map_preview(seed_digest, source)?;
    install_received_preview_image(state, image)
}
