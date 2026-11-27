//! LAN's full options and roster, which a VERA host sends in VS31 setup
//! payloads ([`crate::net::lan_messages::extended_setup`]) in place of
//! gamemd's Options and Roster packets (`0x005E3720`, `0x005E8EA0`). The
//! payloads are projections: MatchSession retains participants, choices, map
//! and readiness; the existing HostSetup service performs mode/map admission
//! and its effects.

use std::net::SocketAddrV4;

use anyhow::{Result, anyhow, ensure};

use crate::app::AppState;
use crate::app::match_runtime::session::{
    MatchSession, NetworkAiOptions, NetworkMapMetadata, NetworkRosterEntry, NetworkVacancy,
};
use crate::app::shell_network_setup::agreement::MultiplayerAgreement;
use crate::app::shell_network_setup::campaign_room::{self, CampaignRoom};
use crate::app::shell_network_setup::{HostOptions, NetworkSetupEvent};
use crate::net::lan_messages::game_name_hash;
use crate::net::lan_messages::setup;
use crate::skirmish_launch::{
    AiDifficulty, NETWORK_PLAYER_SLOT_COUNT, SkirmishAiSlot, SkirmishLaunchMode,
    SkirmishLaunchOptions,
};

pub(super) fn publish_options(state: &mut AppState) -> Result<bool> {
    // Generated dependency preparation can exceed a discovery-query interval.
    // A backpressured retry must yield before that work, allowing the existing
    // transport owner to drain. queue_payload retains its final ACK recheck.
    if state
        .match_state
        .lan_service
        .as_ref()
        .ok_or_else(|| anyhow!("no LAN service"))?
        .queued_setup_count()
        != 0
    {
        return Ok(false);
    }
    let campaign = campaign_room::publication(
        state.platform.game_config.as_ref(),
        &state.match_state.session,
    )?;
    if let Some(room) = &campaign {
        campaign_room::verify_current(
            state.platform.game_config.as_ref(),
            &state.process_assets,
            room,
        )?;
    }
    let agreement = crate::app::shell_network_setup::agreement::local(
        state.platform.game_config.as_ref(),
        &state.match_state.session,
        &mut state.process_assets,
    )?;
    // The room carries the applied speed, the world's while one is installed:
    // another player's speed event in the last match changed only the world.
    // Build_Options_String reads that global, [0xA8EB60], at 0x005DBC68.
    crate::app::loading::transitions::sync_in_game_options_speed_from_sim(&mut state.match_state);
    let session = &state.match_state.session;
    ensure!(session.host(), "only the host publishes full LAN options");
    let current_speed = state
        .match_state
        .match_presentation
        .in_game_options
        .game_speed;
    let game_speed = session
        .network_setup()
        .ok_or_else(|| anyhow!("no setup"))?
        .options
        .game_speed;
    let body = match &campaign {
        Some(room) => TaggedOptionsBody::Campaign {
            room: room.clone(),
            seed: session
                .network_start_seed()
                .ok_or_else(|| anyhow!("host seed is not initialized"))?,
        },
        None => TaggedOptionsBody::Stock {
            random_map: crate::app::shell_network_setup::maps::host_generated_manifest(
                state.platform.game_config.as_ref(),
                &state.frontend.multiplayer_shell_runtime,
                &state.match_state.session,
            )?,
            room: projected_room(
                session,
                state
                    .frontend
                    .multiplayer_shell_runtime
                    .cooperative_difficulty(),
            )?,
        },
    };
    let service = state.match_state.lan_service.as_ref().expect("LAN visit");
    let admissions = session
        .network_setup()
        .expect("projected setup")
        .humans
        .iter()
        .enumerate()
        .filter_map(|(source, human)| {
            let endpoint = session.network_endpoint(source)?;
            Some(ExtendedAdmission {
                name: human.name.clone(),
                endpoint,
                identity: service.peer_admissions.get(&endpoint)?.identity,
            })
        })
        .collect();
    let payload = serde_json::to_vec(&ExtendedOptions {
        agreement: agreement.clone(),
        current_speed,
        game_speed,
        admissions,
        body: body.clone(),
    })?;
    let admitted = queue_payload(
        &mut state.match_state,
        crate::net::lan_messages::extended_setup::SetupKind::Options,
        payload,
    )?;
    if admitted {
        crate::app::shell_network_setup::invalidate_network_agreement(
            &mut state.frontend.network_setup_protocol,
            &mut state.match_state.session,
        )?;
        let service = state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?;
        service.deferred_setup.retain(|request| {
            !matches!(
                request,
                crate::app::shell_network_setup::NetworkSetupRequest::Ready(_)
                    | crate::app::shell_network_setup::NetworkSetupRequest::MapStatus(_)
            )
        });
        service.setup_agreement = Some(agreement);
        let scope = service.setup_scope().expect("admitted options scope");
        match body {
            TaggedOptionsBody::Stock { random_map, .. } => {
                super::maps::received_options(state, scope, random_map)?
            }
            TaggedOptionsBody::Campaign { room, .. } => {
                state
                    .match_state
                    .session
                    .accept_campaign_room(Some(room), game_speed)?;
                super::maps::clear_campaign_transfer(&mut state.frontend, &mut state.match_state);
                campaign_room::report_local_files(
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    true,
                )?;
            }
        }
    }
    Ok(admitted)
}

/// The host's full options (`SetupKind::Options`), replaced whole on every
/// revision. Only a game of the same build reads them: the join refuses any
/// other (reason 8), and a guest refuses options that name another build as
/// invalid (`admit_host_room`).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtendedOptions {
    agreement: MultiplayerAgreement,
    /// Live OptionsClass speed, written by the options parser at 0x005E4DE1.
    current_speed: u32,
    /// The setup trackbar value at 0x005E4E01, copied at launch (0x005E74AA).
    game_speed: i32,
    admissions: Vec<ExtendedAdmission>,
    body: TaggedOptionsBody,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TaggedOptionsBody {
    Stock {
        random_map: Option<crate::app::shell_network_setup::maps::GeneratedMapManifest>,
        room: HostRoom,
    },
    Campaign {
        room: CampaignRoom,
        seed: u32,
    },
}

#[derive(Debug, thiserror::Error)]
enum OptionsRefusal {
    #[error("invalid LAN options JSON: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("invalid LAN room speeds: applied {current_speed}, selected {game_speed}")]
    InvalidSpeed { current_speed: u32, game_speed: i32 },
}

impl ExtendedOptions {
    fn decode(bytes: &[u8]) -> std::result::Result<Self, OptionsRefusal> {
        let options: Self = serde_json::from_slice(bytes)?;
        let selectable = crate::sim::game_options::is_selectable_game_speed;
        if !u8::try_from(options.current_speed).is_ok_and(selectable)
            || !u8::try_from(options.game_speed).is_ok_and(selectable)
        {
            return Err(OptionsRefusal::InvalidSpeed {
                current_speed: options.current_speed,
                game_speed: options.game_speed,
            });
        }
        Ok(options)
    }

    fn apply_current_speed(
        &self,
        options: &mut crate::ui::shell::in_game_options_state::InGameOptionsState,
    ) {
        options.game_speed = self.current_speed;
    }
}

/// What a guest applies to its own setup ([`host_options`]): the room's
/// options whole, its mode and map, and each player's and AI row's choices
/// as shell values (gamemd's options string `0x005DBB60` wrote these words).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostRoom {
    seed: u32,
    #[serde(with = "StockRoomSettings")]
    options: SkirmishLaunchOptions,
    mode: i32,
    cooperative_difficulty: i32,
    selected_map_file: String,
    map_metadata: NetworkMapMetadata,
    humans: Vec<HumanChoices>,
    /// Every row's AI words, active and inactive ([`NetworkAiOptions`]).
    ai: [NetworkAiOptions; NETWORK_PLAYER_SLOT_COUNT],
}

// Project the shared stock options schema onto the LAN body. The envelope
// owns game_speed, so reading this body leaves that member for host_options.
// A remote derive checks field names and types against the common options.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "SkirmishLaunchOptions", deny_unknown_fields)]
struct StockRoomSettings {
    starting_credits: i32,
    unit_count: i32,
    tech_level: i32,
    #[serde(skip)]
    game_speed: i32,
    default_ai_difficulty: i32,
    short_game: bool,
    bases: bool,
    bridges_destroyable: bool,
    super_weapons: bool,
    build_off_ally: bool,
    crates: bool,
    mcv_redeploy: bool,
    fog_of_war: bool,
    shroud: bool,
    tiberium_grows: bool,
    multi_engineer: bool,
    harvester_truce: bool,
    ally_change_allowed: bool,
    #[serde(
        default,
        skip_serializing_if = "crate::sim::game_options::RoomBans::is_empty"
    )]
    bans: crate::sim::game_options::RoomBans,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    teams: Option<u8>,
    #[serde(default, skip_serializing)]
    auto_balance: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    queue: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::skirmish_launch::HostRules::is_empty"
    )]
    host_rules: crate::skirmish_launch::HostRules,
}

/// One player's row by name: side (-3 watching, -2 Random), colour (-2
/// Random), start and team.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct HumanChoices {
    name: String,
    country: i32,
    color: i32,
    start: i32,
    team: i32,
}

/// Host-authored admission projection for native Type3 assembly, derived from
/// Session and replaced with each complete snapshot. Only Session admits a
/// participant/address; a stale Type3 cannot replace the host's current proof.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ExtendedAdmission {
    name: String,
    endpoint: SocketAddrV4,
    identity: super::PeerAdmission,
}

/// The host's last admitted room, kept for a player whose Type3 comes after
/// it; replaced on every revision and cleared with the visit.
pub(super) struct ReceivedOptions {
    scope: crate::net::lan_messages::SessionScope,
    envelope: ExtendedOptions,
}

impl ReceivedOptions {
    fn for_scope(&self, scope: crate::net::lan_messages::SessionScope) -> Option<&ExtendedOptions> {
        (self.scope == scope).then_some(&self.envelope)
    }
}

pub(super) fn reapply_received_options(state: &mut AppState) -> Result<()> {
    let Some(envelope) = state.match_state.lan_service.as_ref().and_then(|service| {
        service
            .received_options
            .as_ref()?
            .for_scope(service.setup_scope()?)
            .cloned()
    }) else {
        return Ok(());
    };
    match &envelope.body {
        TaggedOptionsBody::Stock { room, .. } => {
            let setup = host_options(
                &state.match_state.session,
                &state.frontend.skirmish_modes,
                room,
                envelope.game_speed,
            )?;
            apply_host_room(state, &envelope, Some(setup))
        }
        TaggedOptionsBody::Campaign { .. } => apply_host_room(state, &envelope, None),
    }
}

fn queue_payload(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    kind: crate::net::lan_messages::extended_setup::SetupKind,
    payload: Vec<u8>,
) -> Result<bool> {
    let endpoints = super::remote_sources(&match_state.session)
        .into_iter()
        .filter_map(|source| match_state.session.network_endpoint(source))
        .collect::<Vec<_>>();
    let game = super::active_game_name(match_state).ok_or_else(|| anyhow!("no active LAN game"))?;
    let mut scoped = game_name_hash(&game).to_le_bytes().to_vec();
    scoped.extend_from_slice(&payload);
    let service = match_state
        .lan_service
        .as_mut()
        .ok_or_else(|| anyhow!("no LAN service"))?;
    // Packet7 must already be in a new guest's receive queue before any
    // full setup can arrive. The same ACK boundary also serializes complete
    // extended snapshots across native global backpressure and reordering.
    if service.queued_setup_count() != 0 {
        return Ok(false);
    }
    let previous_scope = service.ensure_host_scope()?;
    service.setup_generation = service
        .setup_generation
        .checked_add(1)
        .ok_or_else(|| anyhow!("LAN setup generation exhausted"))?;
    let scope = crate::net::lan_messages::SessionScope {
        epoch: previous_scope.epoch,
        revision: service.setup_generation,
    };
    let parts = crate::net::lan_messages::extended_setup::encode(kind, scope, &scoped)?;
    let packets = endpoints
        .into_iter()
        .flat_map(|endpoint| {
            parts
                .iter()
                .map(move |part| (part.clone(), super::direct(endpoint)))
        })
        .collect::<Vec<_>>();
    let admitted = service.queue_setup(packets.into_iter())?;
    if admitted && kind == crate::net::lan_messages::extended_setup::SetupKind::Options {
        service.setup_scope = Some(scope);
    }
    Ok(admitted)
}

/// The host's room applied through HostSetup, then the AI rows' inactive
/// words and the applied speed, which HostSetup does not own.
fn apply_host_room(
    state: &mut AppState,
    envelope: &ExtendedOptions,
    host_setup: Option<HostOptions>,
) -> Result<()> {
    match (&envelope.body, host_setup) {
        (TaggedOptionsBody::Stock { room, .. }, Some(options)) => {
            state
                .match_state
                .session
                .accept_campaign_room(None, envelope.game_speed)?;
            crate::app::shell_network_setup::service_event(
                state,
                NetworkSetupEvent::HostSetup(options),
            )?;
            state
                .match_state
                .session
                .accept_network_inactive_ai_options(room.ai)?;
        }
        (TaggedOptionsBody::Campaign { room, seed }, None) => {
            state
                .match_state
                .session
                .accept_campaign_room(Some(room.clone()), envelope.game_speed)?;
            state.match_state.session.set_network_start_seed(*seed)?;
            let verified = campaign_room::verify_current(
                state.platform.game_config.as_ref(),
                &state.process_assets,
                room,
            );
            if let Err(error) = &verified {
                log::info!("Campaign room files refused: {error:#}");
            }
            campaign_room::report_local_files(
                &state.renderer,
                &mut state.frontend,
                &mut state.match_state,
                &state.process_assets,
                verified.is_ok(),
            )?;
        }
        _ => anyhow::bail!("LAN host setup projection does not match its options body"),
    }
    envelope.apply_current_speed(&mut state.match_state.match_presentation.in_game_options);
    Ok(())
}

pub(super) fn publish_roster(
    match_state: &mut crate::app::match_runtime::state::MatchState,
) -> Result<bool> {
    let session = &match_state.session;
    ensure!(session.host(), "only the host publishes a LAN roster");
    // A VERA room's spectator rows may lie past the native eight.
    let rows = if session.network_several_observers()
        || session
            .network_setup_capacity()
            .is_some_and(|capacity| capacity > 8)
    {
        NETWORK_PLAYER_SLOT_COUNT
    } else {
        8
    };
    let names = projected_roster(session)?;
    let names = &names[..rows];
    queue_payload(
        match_state,
        crate::net::lan_messages::extended_setup::SetupKind::Roster,
        serde_json::to_vec(names)?,
    )
}

pub(super) fn receive_extended(
    state: &mut AppState,
    source: SocketAddrV4,
    bytes: &[u8],
    now: u32,
) -> Result<()> {
    if !accepts_guest_setup(
        &state.match_state.session,
        state.frontend.multiplayer_setup.is_some()
            && state.frontend.network_setup_protocol.is_some(),
    ) {
        return Ok(());
    }
    let host = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(|service| service.admitted_host.as_ref())
        .ok_or_else(|| anyhow!("no admitted LAN host"))?
        .clone();
    validate_host_endpoint(&state.match_state.session, &host, source, &host.0)?;
    let scope = crate::net::lan_messages::extended_setup::fragment_scope(bytes)?;
    ensure!(
        state
            .match_state
            .lan_service
            .as_ref()
            .and_then(|service| service.setup_scope())
            .is_some_and(|expected| expected.epoch == scope.epoch),
        "extended setup belongs to an old LAN admission"
    );
    let complete = state
        .match_state
        .lan_service
        .as_mut()
        .expect("admitted LAN service")
        .setup_reassembly
        .receive(bytes, now)?;
    let Some((kind, bytes)) = complete else {
        return Ok(());
    };
    let game =
        super::active_game_name(&state.match_state).ok_or_else(|| anyhow!("no active LAN game"))?;
    ensure!(
        bytes.len() >= 4 && bytes[..4] == game_name_hash(&game).to_le_bytes(),
        "different LAN game"
    );
    let bytes = &bytes[4..];
    match kind {
        crate::net::lan_messages::extended_setup::SetupKind::Options => {
            if state
                .match_state
                .lan_service
                .as_ref()
                .and_then(super::LanService::setup_scope)
                .is_some_and(|current| scope.revision < current.revision)
            {
                return Ok(());
            }
            let result = ExtendedOptions::decode(bytes)
                .map_err(anyhow::Error::from)
                .and_then(|options| receive_host_options(state, scope, options, host));
            if result.is_err() {
                let step = super::reject_setup_compatibility(
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &crate::i18n::tr("VERA:LanOptionsInvalid"),
                );
                super::App::apply_step(state, step);
            }
            result
        }
        crate::net::lan_messages::extended_setup::SetupKind::Roster => {
            let names: Vec<String> = serde_json::from_slice(bytes)?;
            let rows = parsed_roster(&state.match_state.session, &names)?;
            state.match_state.session.accept_network_roster(rows)?;
            state.platform.window.request_redraw();
            Ok(())
        }
    }
}

fn accepts_guest_setup(session: &MatchSession, setup_dialog: bool) -> bool {
    // VS31 is dispatched before loading and live packet handling. Those
    // phases retain the admission scope and immutable launch, but only an
    // editable setup has capacity and a dialog that can accept a revision.
    setup_dialog && !session.host() && session.network_setup_capacity().is_some()
}

fn receive_host_options(
    state: &mut AppState,
    scope: crate::net::lan_messages::SessionScope,
    options: ExtendedOptions,
    host: (String, SocketAddrV4),
) -> Result<()> {
    let (host_setup, departed) = admit_host_room(
        state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(super::closed_visit)?,
        &state.match_state.session,
        &state.frontend.skirmish_modes,
        scope,
        &options,
        &host,
    )?;
    // A snapshot retires old endpoints through the same owner as Type10.
    // This covers a lost departure followed by a new proof for that endpoint.
    for endpoint in departed {
        if let Some(index) = state.match_state.session.network_endpoint_source(endpoint) {
            crate::app::shell_network_setup::service_event(
                state,
                NetworkSetupEvent::ParticipantLeft { index },
            )?;
        }
    }
    crate::app::shell_network_setup::invalidate_network_agreement(
        &mut state.frontend.network_setup_protocol,
        &mut state.match_state.session,
    )?;
    apply_host_room(state, &options, host_setup)?;
    match &options.body {
        TaggedOptionsBody::Stock { random_map, .. } => {
            super::maps::received_options(state, scope, random_map.clone())?
        }
        TaggedOptionsBody::Campaign { .. } => {
            super::maps::clear_campaign_transfer(&mut state.frontend, &mut state.match_state)
        }
    }
    state
        .match_state
        .lan_service
        .as_mut()
        .ok_or_else(super::closed_visit)?
        .received_options = Some(ReceivedOptions {
        scope,
        envelope: options,
    });
    Ok(())
}

fn admit_host_room(
    service: &mut super::LanService,
    session: &MatchSession,
    modes: &[crate::skirmish_modes::SkirmishGameMode],
    scope: crate::net::lan_messages::SessionScope,
    options: &ExtendedOptions,
    host: &(String, SocketAddrV4),
) -> Result<(Option<HostOptions>, Vec<SocketAddrV4>)> {
    // The join admits only this build (reason 8, and the guest's own check
    // of the host's proof), so options naming another are not this room's.
    ensure!(
        options.agreement.version == crate::app::shell_network_setup::agreement::version(),
        "LAN options name another build"
    );
    ensure!(
        crate::map::source::valid_source_sha256(&options.agreement.rules_digest),
        "invalid LAN rules agreement"
    );
    if let TaggedOptionsBody::Stock { room, .. } = &options.body {
        crate::app::shell_network_setup::maps::validate_room_map_name(&room.selected_map_file)?;
        crate::app::shell_network_setup::maps::transfer_length(&room.map_metadata)?;
    }
    ensure!(
        options.admissions.len() <= NETWORK_PLAYER_SLOT_COUNT,
        "too many LAN admission proofs"
    );
    let mut endpoints = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for admission in &options.admissions {
        ensure!(
            admission.endpoint != host.1
                && endpoints.insert(admission.endpoint)
                && names.insert(admission.name.to_ascii_lowercase())
                && !admission.name.eq_ignore_ascii_case(&host.0)
                && admission.identity.join_nonce != [0; 16]
                && admission.identity.admission_nonce != [0; 16],
            "invalid LAN admission proof projection"
        );
    }
    let launch = session.network_setup().ok_or_else(|| anyhow!("no setup"))?;
    let departed = launch
        .humans
        .iter()
        .enumerate()
        .filter_map(|(source, human)| {
            if source == launch.local_player_index {
                return None;
            }
            let endpoint = session.network_endpoint(source)?;
            let retained = (endpoint == host.1 && host.0.eq_ignore_ascii_case(&human.name))
                || options.admissions.iter().any(|admission| {
                    admission.endpoint == endpoint
                        && admission.name.eq_ignore_ascii_case(&human.name)
                });
            (!retained).then_some(endpoint)
        })
        .collect::<Vec<_>>();
    // Check the same HostSetup admission used by the setup dialog against
    // the prospective roster before changing the scope, proofs or requests.
    let mut projected = session.clone();
    for &endpoint in &departed {
        if let Some(index) = projected.network_endpoint_source(endpoint) {
            projected.remove_network_participant(index)?;
        }
    }
    let host_setup = match &options.body {
        TaggedOptionsBody::Stock { room, .. } => {
            let host_setup = host_options(&projected, modes, room, options.game_speed)?;
            projected.accept_network_host_setup(
                host_setup.launch.clone(),
                host_setup.vacancies,
                host_setup.map_metadata.player_capacity,
                host_setup.observer_row,
                false,
            )?;
            Some(host_setup)
        }
        TaggedOptionsBody::Campaign { room, .. } => {
            ensure!(
                room.source(&host.0).is_some()
                    && room
                        .player_names()
                        .all(|name| name.eq_ignore_ascii_case(&host.0)
                            || names.contains(&name.to_ascii_lowercase())),
                "campaign playing roster is outside LAN admissions"
            );
            projected.accept_campaign_room(Some(room.clone()), options.game_speed)?;
            None
        }
    };

    service.setup_scope = Some(scope);
    service.deferred_setup.retain(|request| {
        !matches!(
            request,
            crate::app::shell_network_setup::NetworkSetupRequest::Ready(_)
                | crate::app::shell_network_setup::NetworkSetupRequest::MapStatus(_)
        )
    });
    service.setup_agreement = Some(options.agreement.clone());
    service.replace_peer_admissions(
        options
            .admissions
            .iter()
            .map(|admission| {
                (
                    admission.endpoint,
                    admission.name.clone(),
                    admission.identity,
                )
            })
            .chain(std::iter::once((
                host.1,
                host.0.clone(),
                super::PeerAdmission {
                    join_nonce: [0; 16],
                    admission_nonce: [0; 16],
                },
            ))),
    );
    Ok((host_setup, departed))
}

fn validate_host_endpoint(
    session: &MatchSession,
    host: &(String, SocketAddrV4),
    source: SocketAddrV4,
    name: &str,
) -> Result<()> {
    ensure!(
        !session.host(),
        "host does not receive guest-authored setup"
    );
    ensure!(
        source == host.1 && name.eq_ignore_ascii_case(&host.0),
        "setup is not from the admitted LAN host"
    );
    if let Some(index) = session.network_endpoint_source(source) {
        ensure!(
            session.network_setup().expect("bound participant").humans[index]
                .name
                .eq_ignore_ascii_case(&host.0),
            "host endpoint belongs to another participant"
        );
    }
    Ok(())
}

fn projected_room(session: &MatchSession, cooperative_difficulty: i32) -> Result<HostRoom> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    Ok(HostRoom {
        seed: session
            .network_start_seed()
            .ok_or_else(|| anyhow!("host seed is not initialized"))?,
        options: launch.options.clone(),
        mode: launch.mode.id,
        cooperative_difficulty,
        selected_map_file: launch
            .selected_map_file
            .clone()
            .ok_or_else(|| anyhow!("no selected map filename"))?,
        map_metadata: session
            .network_map_metadata()
            .ok_or_else(|| anyhow!("no selected map metadata"))?
            .clone(),
        humans: launch
            .humans
            .iter()
            .map(|human| HumanChoices {
                name: human.name.clone(),
                country: human.slot.country_shell_value(human.is_observer()),
                color: human.slot.color_shell_value(),
                start: human.slot.start_position.shell_value(),
                team: human.slot.team.shell_value(),
            })
            .collect(),
        ai: std::array::from_fn(|row| {
            session
                .network_ai_options(row as u8)
                .expect("thirty setup rows")
        }),
    })
}

/// The host's room on this guest's own setup, as the options arm of
/// gamemd's parse (`0x005E4DA6` in `0x005E3D10`) applies the host's text:
/// the options, mode and map; each player's row by name, while this guest
/// has admitted them (one whose Type3 comes later takes it on
/// [`reapply_received_options`]); and an AI on each row whose country word is
/// not -1.
fn host_options(
    session: &MatchSession,
    modes: &[crate::skirmish_modes::SkirmishGameMode],
    room: &HostRoom,
    game_speed: i32,
) -> Result<HostOptions> {
    let metadata = &room.map_metadata;
    ensure!(
        metadata.source_sha256.is_empty()
            || crate::map::source::valid_source_sha256(&metadata.source_sha256),
        "invalid map source identity"
    );
    crate::map::source::validate_network_map_name(&room.selected_map_file)?;
    let mut launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?
        .clone();
    launch.options = room.options.clone();
    launch.options.game_speed = game_speed;
    let mode = crate::skirmish_modes::mode_by_id(modes, room.mode)
        .ok_or_else(|| anyhow!("unknown LAN game mode {}", room.mode))?;
    launch.mode = SkirmishLaunchMode::from_game_mode(mode);
    launch.selected_map_file = Some(room.selected_map_file.clone());
    for choices in &room.humans {
        if let Some(human) = launch
            .humans
            .iter_mut()
            .find(|human| human.name.eq_ignore_ascii_case(&choices.name))
        {
            human.slot = super::picks::decoded_pick(
                &human.slot,
                choices.country,
                choices.color,
                choices.start,
                choices.team,
            )?;
            human.set_observer(choices.country == -3);
        }
    }
    let mut opponents = Vec::new();
    for (row, ai) in (0..).zip(&room.ai) {
        if ai.country == -1 {
            continue;
        }
        let old = session
            .network_row_choices(row)
            .ok_or_else(|| anyhow!("no setup row {row}"))?;
        let choices = super::picks::decoded_pick(&old, ai.country, ai.color, ai.start, ai.team)?;
        opponents.push(SkirmishAiSlot {
            driver_policy: None,
            raw_slot_index: row,
            country: choices.country,
            country_random: choices.country_random,
            color_index: choices.color_index,
            color_random: choices.color_random,
            start_position: choices.start_position,
            team: choices.team,
            difficulty: decoded_difficulty(ai.difficulty)?,
        });
    }
    launch.ai_player_count = opponents.len() as i32;
    launch.opponents = opponents;
    let vacancies = std::array::from_fn(|row| {
        session
            .network_vacancy(row as u8)
            .unwrap_or(NetworkVacancy::Open)
    });
    Ok(HostOptions {
        launch,
        map_metadata: metadata.clone(),
        vacancies,
        observer_row: session.network_observer_row(),
        cooperative_difficulty: room.cooperative_difficulty,
        seed: Some(room.seed),
    })
}

pub(super) fn decoded_difficulty(value: i32) -> Result<AiDifficulty> {
    match value {
        0 => Ok(AiDifficulty::Hard),
        1 => Ok(AiDifficulty::Normal),
        2 => Ok(AiDifficulty::Easy),
        _ => anyhow::bail!("invalid native AI difficulty"),
    }
}

fn projected_roster(session: &MatchSession) -> Result<[String; NETWORK_PLAYER_SLOT_COUNT]> {
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    Ok(std::array::from_fn(|row| {
        if let Some(human) = launch
            .humans
            .iter()
            .find(|human| human.raw_slot_index == Some(row as u8))
        {
            human.name.clone()
        } else if let Some(ai) = session.network_display_ai(row as u8) {
            match ai {
                AiDifficulty::Easy => setup::EASY_AI,
                AiDifficulty::Normal => setup::MEDIUM_AI,
                AiDifficulty::Hard => setup::HARD_AI,
            }
            .to_owned()
        } else {
            match session
                .network_vacancy(row as u8)
                .unwrap_or(NetworkVacancy::Closed)
            {
                NetworkVacancy::Open => setup::OPEN,
                NetworkVacancy::Closed => setup::CLOSED,
                NetworkVacancy::OpenObserver => setup::OPEN_OBSERVER,
                NetworkVacancy::Waiting => setup::WAITING,
            }
            .to_owned()
        }
    }))
}

fn parsed_roster(
    session: &MatchSession,
    names: &[String],
) -> Result<[NetworkRosterEntry; NETWORK_PLAYER_SLOT_COUNT]> {
    ensure!(
        names.len() <= NETWORK_PLAYER_SLOT_COUNT,
        "LAN roster exceeds thirty rows"
    );
    let launch = session
        .network_setup()
        .ok_or_else(|| anyhow!("no admitted setup"))?;
    let mut rows = [NetworkRosterEntry::Vacancy(NetworkVacancy::Open); NETWORK_PLAYER_SLOT_COUNT];
    for (row, name) in names.iter().enumerate() {
        rows[row] = match name.as_str() {
            setup::OPEN => NetworkRosterEntry::Vacancy(NetworkVacancy::Open),
            setup::CLOSED => NetworkRosterEntry::Vacancy(NetworkVacancy::Closed),
            setup::OPEN_OBSERVER => NetworkRosterEntry::Vacancy(NetworkVacancy::OpenObserver),
            setup::EASY_AI => NetworkRosterEntry::Ai(AiDifficulty::Easy),
            setup::MEDIUM_AI => NetworkRosterEntry::Ai(AiDifficulty::Normal),
            setup::HARD_AI => NetworkRosterEntry::Ai(AiDifficulty::Hard),
            _ => launch
                .humans
                .iter()
                .position(|human| human.name == *name)
                .map_or(
                    NetworkRosterEntry::Vacancy(NetworkVacancy::Waiting),
                    NetworkRosterEntry::Human,
                ),
        };
    }
    Ok(rows)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::app::match_runtime::session::{NetworkReturnContext, NetworkRowKind};
    use crate::skirmish_launch::{
        LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishLaunchSession,
    };
    use std::net::Ipv4Addr;

    fn fixture(host: bool) -> MatchSession {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        launch.opponents.clear();
        launch.ai_player_count = 0;
        launch.mode =
            SkirmishLaunchMode::from_game_mode(&crate::skirmish_modes::stock_skirmish_modes()[0]);
        launch.selected_map_file = Some("test.map".into());
        launch.humans.truncate(1);
        launch.humans[0].name = "Host".into();
        launch.humans[0].raw_slot_index = Some(0);
        launch.humans[0].slot.country_random = false;
        launch.humans[0].slot.country = LaunchCountry::America;
        launch.humans[0].slot.color_random = false;
        launch.humans[0].slot.color_index = 0;
        let mut guest = launch.humans[0].clone();
        guest.name = "Guest".into();
        guest.raw_slot_index = Some(1);
        guest.slot.color_index = 2;
        launch.humans.push(guest);
        launch.local_player_index = usize::from(!host);
        launch.options.game_speed = 4;
        launch.options.starting_credits = 9000;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::LanLobby);
        session
            .accept_network_setup(launch, host, Some(7), false)
            .unwrap();
        session.set_network_start_seed(0xf1234567).unwrap();
        session
            .accept_network_map_metadata(
                NetworkMapMetadata {
                    display_name: "Høyde地".into(),
                    player_capacity: 8,
                    official: true,
                    byte_length: 123456,
                    digest: "0123456789abcdef0123456789abcdef".into(),
                    source_sha256: String::new(),
                    effective_digest: String::new(),
                },
                None,
                true,
            )
            .unwrap();
        session
    }

    /// The host's room as its guests read it off the wire.
    fn projected_envelope(host: &MatchSession) -> ExtendedOptions {
        let room = projected_room(host, 1).unwrap();
        ExtendedOptions {
            agreement: MultiplayerAgreement {
                version: crate::app::shell_network_setup::agreement::version(),
                rules_digest: "0".repeat(64),
            },
            current_speed: 3,
            game_speed: room.options.game_speed,
            admissions: Vec::new(),
            body: TaggedOptionsBody::Stock {
                random_map: None,
                room,
            },
        }
    }

    fn sent(host: &MatchSession) -> ExtendedOptions {
        let envelope = projected_envelope(host);
        ExtendedOptions::decode(&serde_json::to_vec(&envelope).unwrap()).unwrap()
    }

    #[test]
    fn campaign_envelope_keeps_two_speeds_and_refuses_an_unknown_body() {
        let mut guest = fixture(false);
        let mut envelope = projected_envelope(&fixture(true));
        let campaign: CampaignRoom = serde_json::from_value(serde_json::json!({
            "series": "mission",
            "stage": "first",
            "manifest_sha256": "e".repeat(64),
            "map_sha256": "f".repeat(64),
            "effective_map_digest": "c".repeat(64),
            "attempt": 1,
            "player_limit": 2,
            "difficulty": 2,
            "playing_sources": [
                {"player":"Guest", "role":"Support"},
                {"player":"Host", "role":"Lead"}
            ],
            "carry_over": crate::sim::campaign_carry_over::StageCarryOver::default(),
        }))
        .unwrap();
        envelope.body = TaggedOptionsBody::Campaign {
            room: campaign.clone(),
            seed: 47,
        };
        envelope.current_speed = 2;
        envelope.game_speed = 6;
        let mut bytes = serde_json::to_value(&envelope).unwrap();
        let decoded = ExtendedOptions::decode(&serde_json::to_vec(&bytes).unwrap()).unwrap();
        let TaggedOptionsBody::Campaign { room, seed } = decoded.body.clone() else {
            panic!("lost campaign tag")
        };
        assert_eq!(room, campaign);
        assert_eq!(seed, 47);
        assert!(bytes["body"]["room"].get("options").is_none());
        guest
            .accept_campaign_room(Some(room), decoded.game_speed)
            .unwrap();
        let mut live = crate::ui::shell::in_game_options_state::InGameOptionsState::default();
        decoded.apply_current_speed(&mut live);
        assert_eq!(live.game_speed, 2);
        assert_eq!(guest.network_setup().unwrap().options.game_speed, 6);

        bytes["body"] = serde_json::json!({"kind": "future"});
        let incompatible = serde_json::to_vec(&bytes).unwrap();
        assert!(matches!(
            ExtendedOptions::decode(&incompatible),
            Err(OptionsRefusal::Malformed(_)),
        ));
    }

    #[test]
    fn a_refused_room_keeps_the_previous_scope_and_admission() {
        use crate::app::shell_network_setup::NetworkSetupRequest;

        let mut service = super::super::tests::service();
        let prior = service.setup_scope().unwrap();
        let scope = crate::net::lan_messages::SessionScope {
            revision: prior.revision + 1,
            ..prior
        };
        let mut envelope = projected_envelope(&fixture(true));
        let TaggedOptionsBody::Stock { room, .. } = &mut envelope.body else {
            panic!("stock test fixture")
        };
        let mode = room.mode;
        let capacity = room.map_metadata.player_capacity;
        room.map_metadata.source_sha256 = "2".repeat(64);
        room.map_metadata.effective_digest = "3".repeat(64);
        service.setup_agreement = Some(MultiplayerAgreement {
            version: envelope.agreement.version.clone(),
            rules_digest: "1".repeat(64),
        });
        service
            .deferred_setup
            .push_back(NetworkSetupRequest::Ready(true));
        let host = (
            "Host".into(),
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30_013),
        );
        let guest = fixture(false);
        let modes = crate::skirmish_modes::stock_skirmish_modes();
        let own = envelope.agreement.version.clone();
        for (build, incoming_mode, incoming_capacity, expected) in [
            (own.as_str(), i32::MAX, capacity, "unknown LAN game mode"),
            (own.as_str(), mode, 0, "host map capacity must be positive"),
            ("another", mode, capacity, "name another build"),
        ] {
            envelope.agreement.version = build.to_owned();
            let TaggedOptionsBody::Stock { room, .. } = &mut envelope.body else {
                panic!("stock test fixture")
            };
            room.mode = incoming_mode;
            room.map_metadata.player_capacity = incoming_capacity;
            let error =
                admit_host_room(&mut service, &guest, &modes, scope, &envelope, &host).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(service.setup_scope(), Some(prior));
            assert_eq!(
                service.setup_agreement.as_ref().unwrap().rules_digest,
                "1".repeat(64)
            );
            assert!(service.peer_admissions.is_empty());
            assert_eq!(service.deferred_setup.len(), 1);
        }
    }

    fn apply(guest: &mut MatchSession, envelope: &ExtendedOptions) {
        let TaggedOptionsBody::Stock { room, .. } = &envelope.body else {
            panic!("stock test fixture")
        };
        let options = host_options(
            guest,
            &crate::skirmish_modes::stock_skirmish_modes(),
            room,
            envelope.game_speed,
        )
        .unwrap();
        guest
            .accept_network_host_setup(
                options.launch,
                options.vacancies,
                options.map_metadata.player_capacity,
                options.observer_row,
                false,
            )
            .unwrap();
        guest.accept_network_inactive_ai_options(room.ai).unwrap();
    }

    #[test]
    fn tagged_room_speeds_keep_receive_setup_and_launch_independent() {
        let host = fixture(true);
        let envelope = projected_envelope(&host);
        let payload = serde_json::to_vec(&envelope).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(json["body"]["kind"], "stock");
        assert_eq!(json["current_speed"], 3);
        assert_eq!(json["game_speed"], 4);
        assert!(json["body"]["room"]["options"].get("game_speed").is_none());

        let mut guest = fixture(false);
        let mut effective = crate::ui::shell::in_game_options_state::InGameOptionsState {
            game_speed: 1,
            ..Default::default()
        };
        let received = ExtendedOptions::decode(&payload).unwrap();
        apply(&mut guest, &received);
        received.apply_current_speed(&mut effective);
        assert_eq!(effective.game_speed, 3);
        let configured = guest.network_setup().unwrap().options.game_speed;
        let setup_display =
            crate::ui::shell::game_speed::SpeedContext::Network.visual_position(configured);
        assert_eq!(setup_display, 2);
        guest.prepare_network_timing(2, || 0).unwrap();
        let frozen = guest.freeze_network_start(0xf1234567, None).unwrap();
        let descriptor = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
            frozen.launch_session().unwrap().clone(),
        )
        .unwrap();
        // Without retail data (`RA2_DIR`) the fixture prints SKIPPED.
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut sim = crate::sim::world::Simulation::new();
        crate::sim::scenario_bootstrap::initialize_skirmish_launch_houses(
            &mut sim,
            &crate::map::houses::HouseRoster::default(),
            &retail.rules,
            &descriptor,
        );
        assert_eq!(sim.session().game_options().game_speed, 4);
    }

    #[test]
    fn invalid_options_are_refused_and_stale_options_cannot_reapply() {
        let host = fixture(true);
        let envelope = projected_envelope(&host);
        // An earlier build's layout does not read as this one's.
        let legacy = serde_json::json!({
            "agreement": {"version": "older-build", "rules_digest": "0".repeat(64)},
            "room": {"current_speed": 3, "options": {"game_speed": 4}}
        });
        assert!(matches!(
            ExtendedOptions::decode(&serde_json::to_vec(&legacy).unwrap()),
            Err(OptionsRefusal::Malformed(_))
        ));

        let mut invalid = serde_json::to_value(&envelope).unwrap();
        invalid["current_speed"] = 7.into();
        assert!(matches!(
            ExtendedOptions::decode(&serde_json::to_vec(&invalid).unwrap()),
            Err(OptionsRefusal::InvalidSpeed { .. })
        ));

        let scope = crate::net::lan_messages::SessionScope {
            epoch: [1; 16],
            revision: 2,
        };
        let saved = ReceivedOptions { scope, envelope };
        assert!(saved.for_scope(scope).is_some());
        let other_visit = crate::net::lan_messages::SessionScope {
            epoch: [2; 16],
            ..scope
        };
        assert!(saved.for_scope(other_visit).is_none());
        assert!(
            saved
                .for_scope(crate::net::lan_messages::SessionScope {
                    revision: 1,
                    ..scope
                })
                .is_none()
        );
    }

    #[test]
    fn setup_fragment_admission_ends_at_the_frozen_launch() {
        let mut guest = fixture(false);
        assert!(accepts_guest_setup(&guest, true));
        assert!(!accepts_guest_setup(&guest, false));
        assert!(!accepts_guest_setup(&fixture(true), true));

        guest.prepare_network_timing(2, || 0).unwrap();
        guest.freeze_network_start(0xf1234567, None).unwrap();
        assert!(
            guest.network_setup().is_some(),
            "loading retains the launch"
        );
        assert!(!accepts_guest_setup(&guest, true));
        assert!(!accepts_guest_setup(&guest, false));
    }

    #[test]
    fn roster_reassembly_keeps_its_own_revision_floor() {
        use crate::net::lan_messages::SessionScope;
        use crate::net::lan_messages::extended_setup::{Reassembly, SetupKind, encode};

        let scope = SessionScope {
            epoch: [2; 16],
            revision: 3,
        };
        let mut receiver = Reassembly::default();
        for (kind, revision, result) in [
            (SetupKind::Options, 4, Some((SetupKind::Options, vec![4]))),
            (SetupKind::Roster, 3, Some((SetupKind::Roster, vec![3]))),
            (SetupKind::Roster, 2, None),
        ] {
            let packet = encode(kind, SessionScope { revision, ..scope }, &[revision as u8])
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(receiver.receive(&packet, 0).unwrap(), result);
        }
    }

    /// The room's options reach a guest whole, VERA's beside gamemd's, with
    /// an AI on any row.
    #[test]
    fn host_options_reach_a_guest_whole() {
        let mut host = fixture(true);
        let mut launch = host.network_setup().unwrap().clone();
        launch.humans[1].set_observer(true);
        host.accept_network_setup(launch, true, Some(7), false)
            .unwrap();
        let mut options = host.network_setup().unwrap().options.clone();
        options.tech_level += 1;
        options.bridges_destroyable = !options.bridges_destroyable;
        options.bans.yuri = true;
        options.teams = Some(3);
        options.queue = vec!["Guest".into()];
        options.host_rules.superweapon_limit = 5;
        host.set_network_options(options).unwrap();
        host.set_network_row_kind(9, NetworkRowKind::Ai(AiDifficulty::Easy))
            .unwrap();
        let mut guest = fixture(false);
        apply(&mut guest, &sent(&host));
        let (host, guest) = (
            host.network_setup().unwrap(),
            guest.network_setup().unwrap(),
        );
        assert_eq!(guest.options, host.options);
        assert!(guest.humans[1].is_observer());
        assert_eq!(
            guest
                .opponents
                .iter()
                .map(|ai| (ai.raw_slot_index, ai.difficulty))
                .collect::<Vec<_>>(),
            [(9, AiDifficulty::Easy)]
        );
    }

    #[test]
    fn extended_options_and_roster_retain_thirtieth_participant() {
        let mut host = fixture(true);
        let mut launch = host.network_setup().unwrap().clone();
        let template = launch.humans[0].clone();
        launch.humans = (0..30)
            .map(|row| {
                let mut human = template.clone();
                human.name = format!("Player{row}");
                human.raw_slot_index = Some(row);
                human.slot.start_position = LaunchStartPosition::Position(row);
                human
            })
            .collect();
        host.accept_network_setup(launch.clone(), true, None, false)
            .unwrap();
        host.select_network_map(launch.mode.clone(), "thirty.map".into(), 30)
            .unwrap();
        host.accept_network_map_metadata(
            NetworkMapMetadata {
                player_capacity: 30,
                ..host.network_map_metadata().unwrap().clone()
            },
            None,
            true,
        )
        .unwrap();
        let mut guest = fixture(false);
        launch.local_player_index = 29;
        guest
            .accept_network_setup(launch, false, None, false)
            .unwrap();
        let payload = serde_json::to_vec(&projected_envelope(&host)).unwrap();
        let parts = crate::net::lan_messages::extended_setup::encode(
            crate::net::lan_messages::extended_setup::SetupKind::Options,
            crate::net::lan_messages::SessionScope {
                epoch: [7; 16],
                revision: 1,
            },
            &payload,
        )
        .unwrap();
        assert!(parts.len() > 1);
        let mut receive = crate::net::lan_messages::extended_setup::Reassembly::default();
        let mut complete = None;
        for part in parts.iter().rev() {
            complete = receive.receive(part, 1).unwrap().or(complete);
        }
        let (_, bytes) = complete.unwrap();
        apply(&mut guest, &ExtendedOptions::decode(&bytes).unwrap());
        let names = projected_roster(&host).unwrap();
        guest
            .accept_network_roster(parsed_roster(&guest, &names).unwrap())
            .unwrap();
        assert_eq!(
            guest.network_setup().unwrap().local_player().name,
            "Player29"
        );
        assert_eq!(
            guest.network_setup().unwrap().local_player().raw_slot_index,
            Some(29)
        );
        assert_eq!(guest.network_setup_capacity(), Some(30));
    }

    #[test]
    fn full_options_apply_to_existing_participants() {
        let mut host = fixture(true);
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Easy))
            .unwrap();
        let mut choices = host.network_row_choices(3).unwrap();
        choices.country = LaunchCountry::Yuri;
        choices.country_random = false;
        choices.color_index = 5;
        choices.color_random = false;
        choices.start_position = LaunchStartPosition::Position(6);
        choices.team = LaunchTeam::Team(2);
        host.set_network_row_choices(3, choices).unwrap();
        let envelope = sent(&host);
        let TaggedOptionsBody::Stock { room, .. } = &envelope.body else {
            panic!("stock test fixture")
        };
        let mut guest = fixture(false);
        guest.set_network_player_ready(0, true).unwrap();
        let old_names: Vec<_> = guest
            .network_setup()
            .unwrap()
            .humans
            .iter()
            .map(|h| h.name.clone())
            .collect();
        assert_eq!(
            room.ai[0].start, -2,
            "human row choices do not replace AI arrays"
        );
        let parsed = host_options(
            &guest,
            &crate::skirmish_modes::stock_skirmish_modes(),
            room,
            envelope.game_speed,
        )
        .unwrap();
        assert_eq!(parsed.launch.options.game_speed, 4);
        assert_eq!(parsed.seed, Some(0xf1234567));
        assert_eq!(parsed.launch.opponents[0].raw_slot_index, 3);
        assert_eq!(parsed.launch.opponents[0].country, LaunchCountry::Yuri);
        assert_eq!(parsed.launch.opponents[0].difficulty, AiDifficulty::Easy);
        assert_eq!(
            parsed.launch.opponents[0].start_position,
            LaunchStartPosition::Position(6)
        );
        apply(&mut guest, &envelope);
        assert_eq!(
            guest
                .network_setup()
                .unwrap()
                .humans
                .iter()
                .map(|h| h.name.clone())
                .collect::<Vec<_>>(),
            old_names
        );
        assert!(
            guest.network_player_ready(0),
            "full setup does not replace source acknowledgements"
        );
    }

    #[test]
    fn roster_rebinds_rows_without_creating_nodes_and_late_type3_fills_waiting() {
        let mut guest = fixture(false);
        let host_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
        guest.bind_network_endpoint("Host", host_address).unwrap();
        guest.set_network_player_ready(0, true).unwrap();
        let names = [
            "Guest",
            setup::OPEN,
            "Late",
            setup::HARD_AI,
            "Host",
            setup::CLOSED,
            setup::OPEN,
            setup::OPEN_OBSERVER,
        ]
        .map(str::to_owned);
        guest
            .accept_network_roster(parsed_roster(&guest, &names).unwrap())
            .unwrap();
        let launch = guest.network_setup().unwrap();
        assert_eq!(launch.humans.len(), 2);
        assert_eq!(launch.humans[0].name, "Host");
        assert_eq!(launch.humans[0].raw_slot_index, Some(4));
        assert_eq!(launch.local_player_index, 1);
        assert_eq!(launch.local_player().raw_slot_index, Some(0));
        assert!(guest.network_player_ready(0));
        assert_eq!(guest.network_endpoint(0), Some(host_address));
        assert_eq!(guest.network_vacancy(2), Some(NetworkVacancy::Waiting));
        assert_eq!(projected_roster(&guest).unwrap()[2], setup::WAITING);
        let mut late = launch.humans[0].clone();
        late.name = "Late".into();
        late.raw_slot_index = Some(2);
        guest.add_network_participant(late).unwrap();
        assert_eq!(guest.network_setup().unwrap().humans.len(), 3);
        assert_eq!(guest.network_vacancy(2), None);
        assert_eq!(projected_roster(&guest).unwrap()[2], "Late");
    }

    #[test]
    fn duplicate_roster_is_atomic_and_host_endpoint_must_match() {
        let mut guest = fixture(false);
        let before = guest.clone();
        let names = ["Guest", "Host", "Host"].map(str::to_owned);
        assert!(
            guest
                .accept_network_roster(parsed_roster(&guest, &names).unwrap())
                .is_err()
        );
        assert_eq!(guest, before);
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
        let host = ("Host".into(), address);
        assert!(validate_host_endpoint(&guest, &host, address, "Host").is_ok());
        assert!(
            validate_host_endpoint(
                &guest,
                &host,
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1236),
                "Host"
            )
            .is_err()
        );
        assert!(validate_host_endpoint(&guest, &host, address, "Guest").is_err());
        assert!(validate_host_endpoint(&fixture(true), &host, address, "Host").is_err());
    }

    #[test]
    fn join_and_peer_changes_share_human_and_ai_color_occupancy() {
        let mut host = fixture(true);
        assert!(host.network_color_in_use(2));
        assert!(!host.network_color_in_use(5));
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Hard))
            .unwrap();
        let mut choices = host.network_row_choices(3).unwrap();
        choices.color_index = 5;
        choices.color_random = false;
        host.set_network_row_choices(3, choices).unwrap();
        assert!(host.network_color_in_use(5));
    }

    #[test]
    fn inactive_array_words_survive_options_separately_from_human_choices() {
        let mut host = fixture(true);
        host.set_network_row_kind(3, NetworkRowKind::Ai(AiDifficulty::Easy))
            .unwrap();
        host.set_network_row_kind(3, NetworkRowKind::Open).unwrap();
        let mut human_choices = host.network_setup().unwrap().local_slot().clone();
        human_choices.start_position = LaunchStartPosition::Position(5);
        human_choices.team = LaunchTeam::Team(2);
        host.set_network_row_choices(0, human_choices).unwrap();
        let wire = sent(&host);
        let TaggedOptionsBody::Stock { room, .. } = &wire.body else {
            panic!("stock test fixture")
        };
        assert_eq!(room.humans[0].start, 5);
        assert_eq!(room.ai[0].start, -2);
        assert_eq!(
            room.ai[3],
            NetworkAiOptions {
                difficulty: -1,
                country: -1,
                color: -1,
                start: -1,
                team: -2
            }
        );
        let mut guest = fixture(false);
        apply(&mut guest, &wire);
        assert_eq!(
            guest.network_ai_options(3).unwrap(),
            NetworkAiOptions {
                difficulty: -1,
                country: -1,
                color: -1,
                start: -1,
                team: -2
            }
        );
        assert_eq!(
            guest.network_setup().unwrap().humans[0].slot.start_position,
            LaunchStartPosition::Position(5)
        );
    }

    #[test]
    fn reordered_roster_and_options_keep_display_and_ai_arrays_independent() {
        let mut host = fixture(true);
        let mut launch = host.network_setup().unwrap().clone();
        launch.humans[1].raw_slot_index = Some(2);
        host.accept_network_setup(launch, true, Some(7), false)
            .unwrap();
        host.set_network_row_kind(1, NetworkRowKind::Ai(AiDifficulty::Hard))
            .unwrap();
        let room = sent(&host);
        let roster = projected_roster(&host).unwrap();
        let mut final_states = Vec::new();
        for roster_first in [false, true] {
            let mut guest = fixture(false);
            let apply_roster = |guest: &mut MatchSession| {
                guest
                    .accept_network_roster(parsed_roster(guest, &roster).unwrap())
                    .unwrap();
            };
            if roster_first {
                apply_roster(&mut guest);
                assert_eq!(guest.network_display_ai(1), Some(AiDifficulty::Hard));
                assert!(
                    guest.network_setup().unwrap().opponents.is_empty(),
                    "roster does not write gameplay AI arrays"
                );
            }
            apply(&mut guest, &room);
            if !roster_first {
                assert_eq!(
                    guest.network_setup().unwrap().humans[1].raw_slot_index,
                    Some(1)
                );
                assert_eq!(
                    guest.network_setup().unwrap().opponents[0].raw_slot_index,
                    1,
                    "options may precede the new display bindings"
                );
                assert_eq!(guest.network_display_ai(1), None);
                apply_roster(&mut guest);
            }
            assert_eq!(
                guest.network_setup().unwrap().humans[1].raw_slot_index,
                Some(2)
            );
            let ai = &guest.network_setup().unwrap().opponents[0];
            assert!(ai.country_random && ai.color_random);
            assert_eq!(ai.country.raw_index(), 0);
            assert_eq!(ai.color_index, 0);
            final_states.push(guest);
        }
        assert_eq!(final_states[0], final_states[1]);
    }

    #[test]
    fn lan_peer_policy_keeps_direct_team_and_shared_collision_checks() {
        let mut lan = fixture(true);
        let mut own = lan.network_setup().unwrap().local_slot().clone();
        own.start_position = LaunchStartPosition::Position(3);
        lan.set_network_row_choices(0, own.clone()).unwrap();
        let mut request = lan.network_setup().unwrap().humans[1].slot.clone();
        request.color_index = own.color_index;
        request.color_random = false;
        request.start_position = own.start_position;
        request.team = LaunchTeam::Team(7);
        request.country = LaunchCountry::Yuri;
        request.country_random = false;
        let mut wol = lan.clone();
        let result = lan
            .accept_lan_peer_choices(1, request.clone(), false)
            .unwrap();
        assert!(result.options);
        assert!(!result.roster);
        let slot = &lan.network_setup().unwrap().humans[1].slot;
        assert!(slot.color_random);
        // VERA shared control: the guest shares the host's start and House.
        assert_eq!(slot.start_position, LaunchStartPosition::Position(3));
        assert_eq!(slot.team, LaunchTeam::Team(7));
        assert_eq!(slot.country, LaunchCountry::Yuri);
        wol.accept_network_peer_choices(1, request, false, Some(&[LaunchCountry::America]))
            .unwrap();
        let slot = &wol.network_setup().unwrap().humans[1].slot;
        assert_eq!(slot.team, LaunchTeam::None);
        assert_eq!(slot.country, LaunchCountry::America);
    }
}
