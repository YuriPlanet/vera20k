//! Decoded LAN/WOL service results. The LAN transport and controlled captures
//! share these shell boundaries; this owner applies admission and UI effects.

use super::*;
use crate::app::match_runtime::session::PeerChoicesChange;
use crate::app::shell_route::ShellRoute;
use crate::skirmish_launch::{LaunchTeam, SkirmishLaunchSession, SkirmishLocalSlot};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::modal::ShellBox;
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
    /// A guest's `R` to the host: WOL's `0x007A8260`, LAN's Type18
    /// (`0x005E0D1C`).
    PlayerChoices {
        index: usize,
        choices: SkirmishLocalSlot,
        observer: bool,
    },
    /// The VERA lobby's answer to the host's `/balance`: each participant
    /// (by index) that moves to another team, and that team.
    Teams(Vec<(usize, u8)>),
    PlayerReady {
        index: usize,
        accepted: bool,
    },
    MapStatus {
        index: usize,
        status: u8,
    },
    HostSetup(HostOptions),
    CampaignHostSetup {
        options: HostOptions,
        room: campaign_room::CampaignRoom,
    },
    StartPrompt,
    /// Accepted LAN protocol timing, independent of local input-delay settings.
    Timing {
        protocol: i32,
        response_times: [u32; 2],
    },
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
    HostLeft,
    Kicked,
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
}

/// A LAN guest's room closed because its host left or stopped answering.
pub(crate) const HOST_LEFT_LABEL: &str = "VERA:HostLeftRoomClosed";

pub(crate) fn service_event(state: &mut AppState, event: NetworkSetupEvent) -> anyhow::Result<()> {
    // The route stays NetworkSetup until the match returns, but GO's handoff
    // drops the setup protocol: later setup packets have no receiver. VERA:
    // a lobby visitor's room goes on hidden behind the lobby page.
    anyhow::ensure!(
        (state.frontend.shell_route == ShellRoute::NetworkSetup
            || crate::app::shell_wol::lobby::visiting(state)
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
        // lease expired before GO ends the launch. One whose launch differs
        // derives another scope (`DirectAdmission::derive`) and never answers.
        App::fail_external_launch(
            state,
            "a direct participant did not respond (unreachable, or its spawn.ini, \
             Options rules or VERA20k build differ)",
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
                    | NetworkSetupEvent::Teams(_)
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
            tell_joined(state, dialog, &name);
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
                // VERA: the host may wait in another window, which hears no
                // sound (the activation edge stops the primary output).
                state.platform.request_attention(false);
            }
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
                // 0x7A6E7D..9E: observer row, slot names, options, then
                // the refreshed advertisement (0x78F000, skipped in modes
                // 1-3 at 0x7A6B0D).
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
                super::bans::admit_slot(state, &mut human.slot);
            }
            let name = human.name.clone();
            state.match_state.session.add_network_participant(human)?;
            tell_joined(state, dialog, &name);
            // Type3 admission5E0770 and accepted host type6 at5E1B32.
            // This adapter is the admitted setup (membership>=2); repeated
            // discovery packets never manufacture another participant/cue.
            let sound = state
                .rules()
                .and_then(|rules| rules.general.player_joined_sound())
                .map(str::to_owned);
            App::play_shell_ui_sound_by_id(state, sound.as_deref());
            if dialog.host {
                // VERA: as in a WOL room, the host may be in another window.
                state.platform.request_attention(false);
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
            let name = state
                .match_state
                .session
                .network_setup()
                .and_then(|launch| launch.humans.get(index))
                .map(|human| human.name.clone());
            state
                .match_state
                .session
                .remove_network_participant(index)?;
            // Online the part sink tells it, as it knows a kick
            // (super::wol::user_left).
            if !dialog.wol()
                && let Some(name) = name
            {
                tell_room(state, dialog, "VERA:PlayerLeftRoom", &name);
            }
            if dialog.wol() {
                // The part sink 0x785740 played PlayerLeft and clears the
                // acceptances itself (super::wol::user_left).
                super::cooperative::guest_left(state)?;
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
            mut choices,
            mut observer,
        } => {
            // A guest's own seat and team pick keep to the room's teams.
            let refused =
                state
                    .match_state
                    .session
                    .admit_network_team(index, &mut choices, &mut observer);
            let mut change = accept_peer_choices(state, index, choices, observer)?;
            change.options |= refused;
            publish_choices(state, change);
        }
        NetworkSetupEvent::Teams(moves) => {
            // Moves made before one fails still publish.
            let mut change = PeerChoicesChange::default();
            let mut result = Ok(());
            for (index, team) in moves {
                match move_team(state, index, team) {
                    Ok(moved) => {
                        change.roster |= moved.roster;
                        change.options |= moved.options;
                    }
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
            publish_choices(state, change);
            result?;
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
            // map; K0 and K2 print the sender and map name. VERA: only when
            // the K is news, since online guests repeat it every revision.
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
            let news = state
                .match_state
                .session
                .report_network_map_status(index, status)?;
            let key = match status {
                0 => Some("GUI:HostNoMap"),
                2 => Some("GUI:HostMapTransfer"),
                _ => None,
            };
            if let Some(key) = key.filter(|_| news) {
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
        NetworkSetupEvent::CampaignHostSetup { options, room } => {
            if super::start_phase(state).submitted() || crate::app::shell_wol::in_match(state) {
                return Ok(());
            }
            anyhow::ensure!(
                dialog.wol() && !dialog.host && !super::quick_match(state),
                "invalid online campaign options receiver"
            );
            room.validate()?;
            let speed = options.launch.options.game_speed;
            let verified = campaign_room::verify_current(state, &room);
            state.match_state.session.accept_network_host_setup(
                options.launch,
                options.vacancies,
                options.map_metadata.player_capacity,
                options.observer_row,
                false,
            )?;
            if let Some(seed) = options.seed {
                state.match_state.session.set_network_start_seed(seed)?;
            }
            state
                .match_state
                .session
                .accept_campaign_room(Some(room), speed)?;
            if let Err(error) = &verified {
                log::info!("Online campaign files refused: {error:#}");
            }
            campaign_room::report_local_files(state, verified.is_ok())?;
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
                && (crate::app::shell_wol::in_match(state) || super::start_phase(state).submitted())
            {
                return Ok(());
            }
            if state.match_state.session.campaign_room().is_some() {
                state
                    .match_state
                    .session
                    .accept_campaign_room(None, launch.options.game_speed)?;
            }
            // Only a Quick Match host parses options it did not author.
            let quick_match = super::quick_match(state);
            anyhow::ensure!(
                !dialog.host || quick_match,
                "a host cannot receive guest-authored host setup"
            );
            if let Some(file) = launch.selected_map_file.as_deref()
                && let Err(error) = super::maps::validate_room_map_name(file)
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
                            (verified.title(), loose)
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
            if !dialog.host {
                tell_moved(state, mode_changed);
            }
            // 0x5E542B..0x5E5466: a changed mode's +9C/+20, then +7C.
            if mode_changed {
                super::cooperative::mode_entered(state)?;
            }
            if let Some(admission) = map_admission {
                apply_local_map_admission(state, admission)?;
            }
            // 0x005E57DA → 0x005E74E0: a material change frees the preview
            // (0x005E74E9..0x005E7513). RESIDUAL: a guest that cannot verify
            // the map does not ask the host for one (LAN 0x1B, 0x005E7586;
            // WOL P, 0x007A81B8): the map prefetch delivers its preview.
            if map_changed {
                let protocol = state
                    .frontend
                    .network_setup_protocol
                    .as_mut()
                    .expect("setup owns protocol");
                protocol.invalidate_received_preview(map_changed);
                protocol.local_map_verified = local_map.is_some();
                protocol.local_map_loose = local_map_loose;
            }
            // 0x007A7F72: modes 1-3 skip the guest's preview handling.
            if dialog.wol() && !dialog.host && !quick_match {
                super::wol::options_preview(state);
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
                let text = state.csf_label("GUI:HostGameStartJoiner", "GUI:HostGameStartJoiner");
                crate::app::shell_lan::append_room_chat(
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
        NetworkSetupEvent::CooperativeCampaign(text) => {
            // The 'C' arm (0x005E43CE) follows the parse's WOL drop of every
            // option text (0x005E3E73..0x005E3E90), as HostSetup does.
            if dialog.wol()
                && (crate::app::shell_wol::in_match(state) || super::start_phase(state).submitted())
            {
                return Ok(());
            }
            super::cooperative::campaign_received(state, &text)?;
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
            // LAN Type 10 (0x005E10F2..0x005E1114) keeps its silent native exit.
            let notice = !state.frontend.native_shell_ui;
            App::leave_shell_dialog(
                state,
                shell_transition::ShellSlideKind::NetworkSetup(dialog),
                move |state| {
                    super::commit_back(state);
                    // VERA's notice belongs to the lobby opened after setup is retired.
                    if notice {
                        crate::app::shell_lan::report_error(state, HOST_LEFT_LABEL);
                    }
                },
            );
        }
        NetworkSetupEvent::Kicked => {
            crate::app::shell_lan::report_error(state, "TXT_REQUEST_DENIED");
            App::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellSlideKind::NetworkSetup(dialog),
                crate::app::shell_network_setup::commit_back,
            );
        }
        NetworkSetupEvent::GameStarted { users, pings } => {
            super::wol::game_started(state, users, pings)?;
        }
        NetworkSetupEvent::VerifyStarted { pairs } => {
            if pairs {
                super::wol::verify_connected(state, 0)?;
            }
        }
        NetworkSetupEvent::PeerConnected { node } => super::wol::verify_connected(state, node)?,
        NetworkSetupEvent::Negotiated { result } => super::wol::negotiated(state, result)?,
    }
    Ok(())
}

/// The host checks a guest's `R`: in either room its slot passes the bans.
/// WOL's receiver (`0x007A8260`) then checks the country by the mode's
/// +0x1C: Cooperative's (0x5C17B0) admits its campaign's countries, the
/// other modes' (0x5D67A0) any. LAN's Type18 receiver (`0x005E0D1C`) has no
/// such check.
fn accept_peer_choices(
    state: &mut AppState,
    index: usize,
    mut choices: SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<PeerChoicesChange> {
    super::bans::admit_slot(state, &mut choices);
    if !super::dialog(state).wol() {
        return state
            .match_state
            .session
            .accept_lan_peer_choices(index, choices, observer);
    }
    let countries = super::cooperative::current(state)
        .then(|| {
            state
                .frontend
                .multiplayer_shell_runtime
                .cooperative_player_countries()
        })
        .flatten();
    state.match_state.session.accept_network_peer_choices(
        index,
        choices,
        observer,
        countries.as_deref(),
    )
}

/// `/balance`'s move of participant `index` to `team`.
fn move_team(state: &mut AppState, index: usize, team: u8) -> anyhow::Result<PeerChoicesChange> {
    move_player(state, index, LaunchTeam::Team(team), false)
}

/// The host moves participant `index` to `team`, and to the spectators or
/// into a seat as `observer` says: a guest's row as the guest's own `R`
/// would (LAN's Type18 receiver in a LAN room), the host's own as its own
/// Team list does (`actions::select`). `/balance` and VERA's room headers
/// and row lists move players through it.
pub(super) fn move_player(
    state: &mut AppState,
    index: usize,
    team: LaunchTeam,
    observer: bool,
) -> anyhow::Result<PeerChoicesChange> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no network setup"))?;
    let human = launch
        .humans
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
    let mut choices = human.slot.clone();
    choices.team = team;
    place_player(state, index, choices, observer)
}

/// VERA's host sets a player's whole row: a guest's as the guest's own
/// choices would (`accept_peer_choices`), checked as those are, its own as
/// its own, watching or playing again as a guest would.
pub(super) fn place_player(
    state: &mut AppState,
    index: usize,
    choices: SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<PeerChoicesChange> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no network setup"))?;
    let human = launch
        .humans
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("unknown participant"))?;
    if index != launch.local_player_index {
        return accept_peer_choices(state, index, choices, observer);
    }
    if observer != human.is_observer() {
        return state
            .match_state
            .session
            .move_network_host(choices, observer);
    }
    let row = human
        .raw_slot_index
        .ok_or_else(|| anyhow::anyhow!("moves name seated rows"))?;
    state
        .match_state
        .session
        .set_network_row_choices(row, choices)?;
    Ok(PeerChoicesChange {
        roster: false,
        options: true,
    })
}

/// 0x5EC9F0's advertisement 0x78F000, 0x77DB90's slot names, then 0x5E32D0's
/// options and the repaint 0x783C80, for what accepted choices changed. A
/// LAN room has no advertisement.
pub(super) fn publish_choices(state: &mut AppState, change: PeerChoicesChange) {
    let wol = super::dialog(state).wol();
    let protocol = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .expect("setup owns protocol");
    if change.roster {
        if wol {
            protocol.requests.push(NetworkSetupRequest::Advertise);
        }
        protocol.requests.push(NetworkSetupRequest::PublishRoster);
    }
    if change.options {
        protocol.requests.push(NetworkSetupRequest::PublishOptions);
    }
}

/// VERA's room tells a guest in its own chat when the host's options move
/// it to another team, to the spectators or into a seat: the host's Team
/// list, row list or `/balance`. Where the guest asked to be is its own
/// move (`actions::commit_row`), and a new mode may reset every team, so
/// neither is told. A guest in the join queue hears its place when it joins
/// or moves up, and when a seat comes free for it.
fn tell_moved(state: &mut AppState, mode_changed: bool) {
    let several = state.match_state.session.network_several_observers();
    let Some(launch) = state.match_state.session.network_setup() else {
        return;
    };
    let local = launch.local_player();
    let place = (local.slot.team, local.is_observer());
    let queue_place = state
        .match_state
        .session
        .network_queue_place(launch.local_player_index);
    // Online the user list names the host; on LAN its admission does, and
    // VERA's rooms seat it first.
    let host_name = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .and_then(|protocol| protocol.wol_host.clone())
        .or_else(|| crate::app::lan_service::admitted_host_room(state).map(|(name, _)| name))
        .or_else(|| {
            launch
                .humans
                .iter()
                .find(|human| human.raw_slot_index == Some(0))
                .map(|human| human.name.clone())
        })
        .unwrap_or_default();
    let Some(protocol) = state.frontend.network_setup_protocol.as_mut() else {
        return;
    };
    let moved = protocol
        .host_placed(place)
        .filter(|_| several && !mode_changed);
    let queued = std::mem::replace(&mut protocol.queue_place, queue_place);
    let name: &dyn std::fmt::Display = &host_name;
    let text = match (moved, queue_place) {
        // A seat from the queue is the queue's, not a move of the host's.
        _ if queued.is_some() && !place.1 => crate::i18n::tr("VERA:QueueSeated"),
        (_, Some(now)) if queued != Some(now) => {
            crate::i18n::tr_fill("VERA:QueuePlace", &[("place", &now)])
        }
        (Some(HostMove::Team(team)), _) => crate::i18n::tr_fill(
            "VERA:HostMovedYouToTeam",
            &[
                ("name", name),
                ("letter", &super::view::team_letter(state, team)),
            ],
        ),
        (Some(HostMove::OffTeam), _) => {
            crate::i18n::tr_fill("VERA:HostMovedYouOffTeam", &[("name", name)])
        }
        (Some(HostMove::Spectators), _) => {
            crate::i18n::tr_fill("VERA:HostMovedYouToSpectators", &[("name", name)])
        }
        (Some(HostMove::Seat), _) => {
            crate::i18n::tr_fill("VERA:HostGaveYouSeat", &[("name", name)])
        }
        (None, _) => return,
    };
    crate::app::shell_lan::append_room_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

/// VERA's room tells everyone in it who came in (`VERA:PlayerJoinedRoom`)
/// and who left (`VERA:PlayerLeftRoom`); gamemd's rooms only play
/// [AudioVisual] PlayerJoined and PlayerLeft.
pub(super) fn tell_room(state: &mut AppState, dialog: SetupDialog, key: &str, name: &str) {
    if !dialog.modern {
        return;
    }
    let text = crate::i18n::tr_fill(key, &[("name", &name)]);
    crate::app::shell_lan::append_room_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

/// VERA's room tells a join once it has settled (`quiet_joins_until`).
fn tell_joined(state: &mut AppState, dialog: SetupDialog, name: &str) {
    let quiet = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .and_then(|protocol| protocol.quiet_joins_until)
        .is_some_and(|until| Instant::now() < until);
    if !quiet {
        tell_room(state, dialog, "VERA:PlayerJoinedRoom", name);
    }
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
    fail_go_handoff_with_notice(state, "TXT_UNABLE_PLAY_WAAUGH", "TXT_UNABLE_PLAY_WAAUGH");
}

pub(crate) fn fail_go_delivery(state: &mut AppState) {
    fail_go_handoff_with_notice(
        state,
        "TXT_SYSTEM_NOT_RESPONDING",
        "The other system is not responding.",
    );
}

fn fail_go_handoff_with_notice(state: &mut AppState, label: &str, fallback: &str) {
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
    let message = state.csf_label(label, fallback);
    let ok = state.csf_label("TXT_OK", "OK");
    state
        .frontend
        .multiplayer_setup
        .as_mut()
        .expect("setup interaction")
        .notice = Some(ShellBox::notice(message, ok));
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
        .push(crate::ui::shell::descriptor::DialogResourceId(0xCE));
}

fn prepare_local_map_for_handoff(state: &mut AppState) -> anyhow::Result<()> {
    // DD8A0 calls6951F0 again after GO, independently of the setup's earlier
    // title/K probe. Only this successful local-file path canonicalizes the
    // accepted filename at695451..695479.
    let session = &state.match_state.session;
    if let Some(room) = session.campaign_room() {
        return super::campaign_room::verify_current(state, room);
    }
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
    // completes GO only from the prefetched source above.
    anyhow::ensure!(
        !file.eq_ignore_ascii_case(crate::map::skirmish_scenarios::RANDMAP_SED),
        "a random map is always transferred"
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
            &state.csf_label(key, key),
            &[crate::assets::csf_file::CsfArg::Str(map_name)],
        );
        crate::app::shell_lan::append_room_chat(
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
    )?;
    // VERA: a Quick Match guest accepts with the host's options
    // (`0x007A7F72`), but the online lobby verifies the map after them and
    // VERA's live service refuses an acceptance before then; the verified
    // map accepts again.
    if super::quick_match(state) && !state.match_state.session.host() {
        super::wol::quick_match_accept(state);
    }
    Ok(())
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
    protocol.transfer_progress = None;
    let seed = state
        .match_state
        .session
        .network_start_seed()
        .ok_or_else(|| anyhow::anyhow!("GO has no admitted seed"))?;
    // VERA: a guest's match starts without a click. Call back a player in
    // another window now: a hidden window's slide waits for its frames.
    state.platform.request_attention(true);
    if super::external(state) {
        // A direct spawn has no dialog to slide out.
        super::commit_start(state, seed);
        return Ok(());
    }
    App::leave_shell_dialog_for_network_start(state, super::dialog(state), seed);
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
/// VERA's room adds its own options: bridges, allying and the bans.
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
        || a.bridges_destroyable != b.bridges_destroyable
        || a.ally_change_allowed != b.ally_change_allowed
        || a.bans != b.bans
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
    seat_queue(state);
    point_out_briefing(state);
    let go = poll_go(state);
    // A direct spawn's LAN service renews setup leases and waits for its
    // peers before GO; keep the frame loop servicing it.
    let direct = (super::external(state) && !super::dialog(state).wol())
        .then(|| Instant::now() + std::time::Duration::from_millis(16));
    go.into_iter().chain(direct).min()
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
}

/// VERA's join queue in the host's game: each free seat goes to the queue's
/// first spectator, as the host's Seat in its Player list does
/// ([`move_player`]): on the team with the fewest players while Teams is
/// set. A seat frees when a
/// player leaves or starts watching, or the host adds a seat or takes an AI
/// out.
fn seat_queue(state: &mut AppState) {
    if super::start_phase(state).in_progress()
        || state
            .frontend
            .network_setup_protocol
            .as_ref()
            .is_none_or(|protocol| protocol.locked)
    {
        return;
    }
    let mut change = PeerChoicesChange::default();
    let mut seated = Vec::new();
    while let Some(index) = state.match_state.session.network_queue_next() {
        // Each spectator once: one this could not seat stays in the queue.
        if seated.contains(&index) {
            break;
        }
        seated.push(index);
        match move_player(state, index, LaunchTeam::None, false) {
            Ok(moved) => {
                change.roster |= moved.roster;
                change.options |= moved.options;
            }
            Err(error) => {
                log::warn!("the join queue could not seat a spectator: {error:#}");
                break;
            }
        }
    }
    if change != PeerChoicesChange::default() {
        publish_choices(state, change);
    }
}

/// VERA's line pointing a player in its game room at the map's briefing
/// (`map::basic::briefing_text`), which a press on the map preview shows
/// (`frontend::preview_zoom`).
const BRIEFING_HINT: &str = "VERA:MapHasBriefing";

/// VERA's game room tells the player in their own chat, once per map, that
/// the room's map has a briefing: when they enter the room on such a map and
/// when the room changes to one. Nothing is sent to the others.
fn point_out_briefing(state: &mut AppState) {
    if state.frontend.shell_route != ShellRoute::NetworkSetup
        || !super::dialog(state).modern
        || super::quick_match(state)
        || super::external(state)
        || state.frontend.network_setup_protocol.is_none()
    {
        return;
    }
    let Some(index) = super::view::local_map_index(state) else {
        return;
    };
    let Some(record) = state.frontend.scenario_catalog.records().get(index) else {
        return;
    };
    if !state
        .frontend
        .multiplayer_setup
        .as_mut()
        .is_some_and(|ui| ui.first_briefing_check(&record.file_name))
        || crate::app::map_library::briefing(state, index).is_none()
    {
        return;
    }
    crate::app::shell_lan::append_room_chat(
        state,
        LanChatLine {
            text: crate::i18n::tr(BRIEFING_HINT).encode_utf16().collect(),
            color: u32::MAX,
        },
    );
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
    Ok(())
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
