//! Bounded, cancellable transfers of the selected physical map/seed and preview.
//! Existing source, staging, generation and UI owners decide what those bytes mean.
use super::{LiveService, http};
use crate::app::PlatformState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::MatchSession;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_network_setup::maps::{self as shared_maps, GeneratedMapManifest};
use crate::app::shell_network_setup::{accept_network_map_source, set_network_map_progress};
use crate::app::shell_wol::WolVisit;
use crate::map::source::{LoadedTransferAsset, TransferResource};
use crate::util::time::{Duration, Instant};
use anyhow::{Result, anyhow, ensure};
use std::path::PathBuf;
use std::sync::Arc;
use winit::window::Window;
/// The lobby's map transfer chunk (docs/lobby-protocol.md, `map_upload`):
/// each chunk but a transfer's last holds exactly this many bytes, which
/// [`Transfer::append_chunk`] alone checks.
const MAP_CHUNK_BYTES: usize = 32768;
const MAP_PROGRESS_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(PartialEq, Eq)]
struct Key {
    room: String,
    digest: String,
    total: usize,
    effective_digest: String,
    random: Option<GeneratedMapManifest>,
}
impl Key {
    fn asset(&self, resource: TransferResource) -> (&str, usize) {
        if resource == TransferResource::Preview {
            let preview = &self.random.as_ref().expect("random preview").preview;
            (&preview.source_sha256, preview.byte_length)
        } else {
            (&self.digest, self.total)
        }
    }
}
#[derive(Clone, Copy)]
enum Direction {
    Upload,
    Download,
}
struct Transfer {
    /// Dropping the selection cancels map work still waiting for its turn.
    active: Arc<()>,
    resource: TransferResource,
    direction: Direction,
    bytes: Vec<u8>,
    offset: usize,
    in_flight: bool,
    retry_at: Instant,
    deadline: Instant,
    retries: u8,
    directory: PathBuf,
}
impl LiveService {
    fn request_map(&self, transfer: u64, active: &Arc<()>, io: http::MapIo) -> Result<()> {
        self.requests.send(http::Request::Map {
            transfer,
            active: Arc::downgrade(active),
            io,
            completed: self.requests.clone(),
        })?;
        Ok(())
    }
}
impl Transfer {
    fn append_chunk(
        &mut self,
        key: &Key,
        digest: &str,
        total: usize,
        offset: usize,
        bytes: &[u8],
        complete: bool,
    ) -> Result<()> {
        let (expected_digest, expected_total) = key.asset(self.resource);
        let expected_size = MAP_CHUNK_BYTES.min(expected_total.saturating_sub(self.offset));
        ensure!(
            digest == expected_digest
                && total == expected_total
                && offset == self.offset
                && bytes.len() == expected_size
                && !bytes.is_empty()
                && complete == (offset + bytes.len() == total),
            "invalid map chunk"
        );
        self.bytes.extend_from_slice(bytes);
        self.advanced_to(self.offset + bytes.len());
        Ok(())
    }
}
enum FailureNotice<'a> {
    Retry(&'a str),
    Notice(&'static str),
}

#[derive(Default)]
pub(super) struct Transfers {
    active: bool,
    generation: u64,
    key: Option<Key>,
    transfer: Option<Transfer>,
    next_resource: Option<TransferResource>,
    complete: bool,
    failed: bool,
    generated: Option<GeneratedMapManifest>,
    /// The room and map of the lobby's latest `map_available`. The lobby's
    /// map library sends it with the host's settings when it already holds
    /// the map, before the host's upload has begun.
    available: Option<(String, String, usize)>,
    /// VERA: a guest's download of a map it does not have waits for the
    /// player's answer to the room's missing-map offer.
    asking: bool,
    /// VERA: the match whose map a watcher's game downloads, which takes
    /// the room's place (docs/lobby-protocol.md "Watching").
    watching: Option<String>,
}

pub(super) fn received_options(
    frontend: &mut FrontendState,
    session: &mut MatchSession,
    service: &mut LiveService,
    generated: Option<GeneratedMapManifest>,
) -> Result<()> {
    shared_maps::accept_generated_manifest(
        &mut frontend.multiplayer_shell_runtime,
        session,
        service.maps.generated.as_ref(),
        generated.as_ref(),
    )?;
    service.maps.generated = generated;
    service.maps.active = true;
    Ok(())
}

/// A watcher's game fetches the map of the match `match_id`, as a guest
/// fetches its room's: from its own maps, its cache, then the lobby.
pub(super) fn watch(service: &mut LiveService, match_id: String) {
    service.maps.watching = Some(match_id);
    service.maps.generated = None;
    service.maps.active = true;
}

pub(super) fn cancel(
    frontend: &mut FrontendState,
    window: &Window,
    session: &MatchSession,
    service: &mut LiveService,
) {
    service.maps.active = false;
    service.maps.watching = None;
    service.maps.generation = service.maps.generation.wrapping_add(1);
    service.maps.key = None;
    service.maps.transfer = None;
    service.maps.generated = None;
    service.maps.next_resource = None;
    service.maps.available = None;
    service.maps.failed = false;
    service.maps.complete = false;
    service.maps.asking = false;
    crate::app::shell_network_setup::close_missing_map_offer(
        window,
        &mut frontend.multiplayer_setup,
    );
    set_network_map_progress(&mut frontend.network_setup_protocol, session, None);
}
pub(super) fn poll(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    service: &mut LiveService,
) {
    let mut maps = std::mem::take(&mut service.maps);
    if maps
        .poll(
            frontend,
            platform,
            match_state,
            process_assets,
            renderer,
            service,
        )
        .is_err()
    {
        maps.fail(
            frontend,
            &platform.window,
            match_state,
            process_assets,
            renderer,
        );
    }
    service.maps = maps;
}
impl Transfers {
    pub(super) fn current_verified(&self, session: &MatchSession, wol: Option<&WolVisit>) -> bool {
        self.complete && self.matches_selection(session, wol)
    }

    fn matches_selection(&self, session: &MatchSession, wol: Option<&WolVisit>) -> bool {
        let Some(metadata) = session.network_map_metadata() else {
            return false;
        };
        self.key.as_ref().is_some_and(|key| {
            key.digest == metadata.source_sha256
                && key.total == usize::try_from(metadata.byte_length).unwrap_or(0)
                && key.effective_digest == metadata.effective_digest
                && key.random == self.generated
                && self.scope(wol).is_some_and(|scope| scope == key.room)
        })
    }
    /// The current attempt failed and cannot advance.
    pub(super) fn failed(&self) -> bool {
        self.failed
    }
    /// What the transfer belongs to: the room this game is in, or the match
    /// it watches.
    fn scope(&self, wol: Option<&WolVisit>) -> Option<String> {
        self.watching.clone().or_else(|| {
            super::super::lobby::current_channel(wol).map(|channel| channel.name.clone())
        })
    }
    fn fail(
        &mut self,
        frontend: &mut FrontendState,
        window: &Window,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
    ) {
        let can_retry = !match_state.session.host()
            && !crate::app::shell_network_setup::quick_match(
                frontend.network_setup_protocol.as_ref(),
            )
            && !crate::app::shell_network_setup::external(frontend.network_setup_protocol.as_ref())
            && self.matches_selection(&match_state.session, frontend.wol.as_ref());
        let map = match_state
            .session
            .network_map_metadata()
            .filter(|_| can_retry)
            .map(|map| map.display_name.as_str());
        match self.stop_failed(map) {
            Some(FailureNotice::Retry(name)) => {
                let name = name.to_owned();
                let message = crate::i18n::tr_fill("VERA:RoomMapRetryPrompt", &[("map", &name)]);
                crate::app::shell_network_setup::offer_missing_map(
                    window,
                    frontend,
                    match_state,
                    name,
                    message,
                    "VERA:RoomMapRetry",
                );
            }
            Some(FailureNotice::Notice(key)) => {
                crate::app::shell_network_setup::close_missing_map_offer(
                    window,
                    &mut frontend.multiplayer_setup,
                );
                super::notice(
                    frontend,
                    window,
                    match_state,
                    process_assets,
                    renderer,
                    &crate::i18n::tr(key),
                );
            }
            None => {}
        }
        set_network_map_progress(
            &mut frontend.network_setup_protocol,
            &match_state.session,
            None,
        );
    }

    fn stop_failed<'a>(&mut self, retry_map: Option<&'a str>) -> Option<FailureNotice<'a>> {
        if self.failed {
            return None;
        }
        let notice = match retry_map {
            // The guest's source lookup and final verification can fail on either
            // side of the chunk phase, when no transfer buffer exists.
            Some(name) => FailureNotice::Retry(name),
            _ if self
                .transfer
                .as_ref()
                .is_some_and(|t| t.retries > 0 || Instant::now() > t.deadline) =>
            {
                FailureNotice::Notice("VERA:MapTransferInterrupted")
            }
            _ => FailureNotice::Notice("VERA:MapTransferFailed"),
        };
        self.failed = true;
        self.complete = false;
        self.transfer = None;
        self.next_resource = None;
        self.asking = false;
        Some(notice)
    }

    fn poll(
        &mut self,
        frontend: &mut FrontendState,
        platform: &PlatformState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        service: &mut LiveService,
    ) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        if match_state.session.network_setup().is_none() {
            self.key = None;
            self.transfer = None;
            return Ok(());
        }
        let Some(metadata) = match_state.session.network_map_metadata().cloned() else {
            return Ok(());
        };
        let Some(scope) = self.scope(frontend.wol.as_ref()) else {
            return Ok(());
        };
        let seed = shared_maps::generated_selection(&match_state.session);
        if seed && self.generated.is_none() {
            return Ok(());
        }
        let key = Key {
            room: scope,
            digest: metadata.source_sha256.clone(),
            total: shared_maps::transfer_length(&metadata)?,
            effective_digest: metadata.effective_digest.clone(),
            random: self.generated.clone(),
        };
        if self.select_map(
            key,
            crate::app::shell_network_setup::missing_map_download_agreed(
                frontend.multiplayer_setup.as_ref(),
            ),
        ) {
            crate::app::shell_network_setup::close_missing_map_offer(
                &platform.window,
                &mut frontend.multiplayer_setup,
            );
        }
        self.recheck_lost_source(&match_state.session, seed);
        if self.failed || self.complete {
            return Ok(());
        }
        if let Some(resource) = self.next_resource.take() {
            self.begin_resource(
                frontend,
                platform,
                match_state,
                process_assets,
                renderer,
                service,
                resource,
            )?;
        }
        if self.asking {
            if !crate::app::shell_network_setup::missing_map_download_agreed(
                frontend.multiplayer_setup.as_ref(),
            ) {
                return Ok(());
            }
            self.agreed_to_download();
            set_network_map_progress(
                &mut frontend.network_setup_protocol,
                &match_state.session,
                Some((0, metadata.byte_length.max(0) as u32)),
            );
        }
        let Some(transfer) = self.transfer.as_mut() else {
            return Ok(());
        };
        ensure!(
            Instant::now() <= transfer.deadline,
            "map transfer timed out"
        );
        if transfer.in_flight || Instant::now() < transfer.retry_at {
            return Ok(());
        }
        let key = self.key.as_ref().expect("transfer key");
        let (digest, total) = key.asset(transfer.resource);
        let request = match transfer.direction {
            Direction::Upload => {
                let end = (transfer.offset + MAP_CHUNK_BYTES).min(total);
                let id = service.next_request;
                service.next_request = service.next_request.wrapping_add(1);
                http::MapIo::UploadMapChunk {
                    resource: transfer.resource,
                    id,
                    digest: digest.to_owned(),
                    total,
                    offset: transfer.offset,
                    bytes: transfer.bytes[transfer.offset..end].to_vec(),
                }
            }
            Direction::Download => http::MapIo::MapChunk {
                resource: transfer.resource,
                seed_digest: key.digest.clone(),
                digest: digest.to_owned(),
                offset: transfer.offset,
                match_id: self.watching.clone(),
            },
        };
        service.request_map(self.generation, &transfer.active, request)?;
        transfer.in_flight = true;
        Ok(())
    }
    /// A changed selection dismisses any prompt for the previous map.
    fn select_map(&mut self, key: Key, retry: bool) -> bool {
        let changed = self.key.as_ref() != Some(&key);
        if changed || self.failed && retry {
            self.forget_other_available(&key);
            self.asking = false;
            self.generation = self.generation.wrapping_add(1);
            self.transfer = None;
            self.complete = false;
            self.failed = false;
            self.next_resource = Some(if key.random.is_some() {
                TransferResource::Seed
            } else {
                TransferResource::Map
            });
            self.key = Some(key);
        }
        changed
    }
    fn begin_resource(
        &mut self,
        frontend: &mut FrontendState,
        platform: &PlatformState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        service: &mut LiveService,
        resource: TransferResource,
    ) -> Result<()> {
        let key = self.key.as_ref().expect("transfer key");
        let (digest, total) = key.asset(resource);
        let config = platform
            .game_config
            .as_ref()
            .ok_or_else(|| anyhow!("no asset directory"))?;
        let directory = config.paths.settings_dir().join("NetworkMaps");
        let host = match_state.session.host();
        let local = shared_maps::local_transfer_source(
            platform.game_config.as_ref(),
            frontend,
            &match_state.session,
            process_assets,
            resource,
        )?;
        let assets = process_assets
            .manager()
            .ok_or_else(|| anyhow!("assets missing"))?;
        let metadata = match_state
            .session
            .network_map_metadata()
            .expect("transfer metadata");
        if let Some(source) = &local {
            ensure!(
                source.physical_identity() == Some((total, digest)),
                "local resource changed"
            );
        }
        let upload = if host {
            Some(crate::map::source::prepare_transfer_map_source(
                local
                    .as_ref()
                    .ok_or_else(|| anyhow!("host resource missing"))?,
                assets,
            )?)
        } else {
            None
        };
        if resource != TransferResource::Preview
            && let Some(source) = local.clone()
        {
            let effective = metadata.effective_digest.clone();
            match_state
                .session
                .accept_verified_network_map_source(source, effective)?;
            if !host || self.lobby_holds(resource) {
                return self.finish_resource(
                    frontend,
                    &platform.window,
                    match_state,
                    process_assets,
                    renderer,
                    resource,
                );
            }
        }
        let now = Instant::now();
        let active = Arc::new(());
        self.transfer = Some(Transfer {
            active: Arc::clone(&active),
            resource,
            direction: if host {
                Direction::Upload
            } else {
                Direction::Download
            },
            bytes: Vec::new(),
            offset: 0,
            in_flight: true,
            retry_at: now,
            deadline: now + MAP_PROGRESS_TIMEOUT,
            retries: 0,
            directory: directory.clone(),
        });
        set_network_map_progress(
            &mut frontend.network_setup_protocol,
            &match_state.session,
            Some((0, total as u32)),
        );
        let io = if let Some(source) = upload {
            http::MapIo::ReadMapUpload {
                source,
                digest: digest.to_owned(),
                total,
            }
        } else {
            http::MapIo::CachedMap {
                resource,
                directory,
                digest: digest.to_owned(),
                total,
            }
        };
        service.request_map(self.generation, &active, io)?;
        Ok(())
    }
    /// A finished transfer stands only while the session holds the map it
    /// verified. A match's end drops that map
    /// (`MatchSession::return_to_network_lobby`), but a room reopened for a
    /// rematch keeps its map and so this key. Without a fresh check, a game
    /// whose match end lands after the reopen, host or guest, keeps reporting
    /// map status 0, and the host's Start waits on it until it times out. The
    /// lobby still holds the room's map, so nothing is uploaded again
    /// (`lobby_holds`).
    fn recheck_lost_source(
        &mut self,
        session: &crate::app::match_runtime::session::MatchSession,
        seed: bool,
    ) {
        if self.complete && session.network_local_map_source().is_none() {
            self.complete = false;
            self.next_resource = Some(if seed {
                TransferResource::Seed
            } else {
                TransferResource::Map
            });
        }
    }
    /// Whether the lobby already has this room's map, so the host need not
    /// upload it. Generated maps and their previews always come from the host.
    fn lobby_holds(&self, resource: TransferResource) -> bool {
        resource == TransferResource::Map
            && self.key.as_ref().zip(self.available.as_ref()).is_some_and(
                |(key, (room, digest, total))| {
                    key.random.is_none()
                        && &key.room == room
                        && &key.digest == digest
                        && key.total == *total
                },
            )
    }
    /// A `map_available` for an earlier map says nothing about `key`'s: after
    /// L, another map, then L again, the lobby may not copy L this time, and
    /// its guests would wait for an upload. One for `key` that came first stays.
    fn forget_other_available(&mut self, key: &Key) {
        if self
            .available
            .as_ref()
            .is_some_and(|(room, digest, total)| {
                (room, digest, *total) != (&key.room, &key.digest, key.total)
            })
        {
            self.available = None;
        }
    }
    /// A `map_available` for the host's own upload ends it: the lobby has
    /// the map, so the rest need not be sent.
    fn heard_available(
        &mut self,
        frontend: &mut FrontendState,
        window: &Window,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        room: Option<String>,
        digest: &str,
        total: usize,
    ) -> Result<()> {
        self.available = room.map(|room| (room, digest.to_owned(), total));
        let uploading = self
            .transfer
            .as_ref()
            .filter(|transfer| matches!(transfer.direction, Direction::Upload))
            .map(|transfer| transfer.resource);
        match uploading {
            Some(resource) if self.lobby_holds(resource) => {
                set_network_map_progress(
                    &mut frontend.network_setup_protocol,
                    &match_state.session,
                    Some((total as u32, total as u32)),
                );
                self.finish_resource(
                    frontend,
                    window,
                    match_state,
                    process_assets,
                    renderer,
                    resource,
                )
            }
            _ => Ok(()),
        }
    }
    fn finish_resource(
        &mut self,
        frontend: &mut FrontendState,
        window: &Window,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        resource: TransferResource,
    ) -> Result<()> {
        self.transfer = None;
        if resource != TransferResource::Preview
            && self.key.as_ref().is_some_and(|key| key.random.is_some())
        {
            self.next_resource = Some(TransferResource::Preview);
            return Ok(());
        }
        let source = match_state
            .session
            .network_local_map_source()
            .cloned()
            .ok_or_else(|| anyhow!("verified source missing"))?;
        let effective = match_state
            .session
            .network_local_map_effective_digest()
            .ok_or_else(|| anyhow!("effective map missing"))?
            .to_owned();
        accept_network_map_source(
            renderer,
            frontend,
            match_state,
            process_assets,
            source,
            effective,
        )?;
        self.complete = true;
        // The map arrived: the offer showing its download closes.
        crate::app::shell_network_setup::close_missing_map_offer(
            window,
            &mut frontend.multiplayer_setup,
        );
        Ok(())
    }
    fn accept_download(
        &mut self,
        frontend: &mut FrontendState,
        platform: &PlatformState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        loaded: LoadedTransferAsset,
        resource: TransferResource,
    ) -> Result<()> {
        shared_maps::accept_transfer_asset(
            platform,
            renderer,
            frontend,
            &mut match_state.session,
            process_assets,
            loaded,
            resource,
            self.generated.as_ref(),
        )?;
        self.finish_resource(
            frontend,
            &platform.window,
            match_state,
            process_assets,
            renderer,
            resource,
        )
    }
    /// A previous attempt can finish its cache lookup or install after a new
    /// attempt has already started on this connection.
    fn transfer_for_response(&self, generation: u64) -> Option<&Transfer> {
        self.transfer
            .as_ref()
            .filter(|_| generation == self.generation)
    }
    fn response(
        &mut self,
        frontend: &mut FrontendState,
        platform: &PlatformState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        renderer: &RendererState,
        service: &mut LiveService,
        response: http::Response,
    ) -> Result<()> {
        let generation = match &response {
            http::Response::MapChunk { transfer, .. }
            | http::Response::MapUploadPrepared { transfer, .. }
            | http::Response::MapUploaded { transfer, .. }
            | http::Response::CachedMap { transfer, .. }
            | http::Response::MapInstalled { transfer, .. }
            | http::Response::MapPending { transfer }
            | http::Response::MapFailure { transfer, .. } => *transfer,
            _ => return Ok(()),
        };
        let Some(resource) = self.transfer_for_response(generation).map(|t| t.resource) else {
            return Ok(());
        };
        if !self.matches_selection(&match_state.session, frontend.wol.as_ref()) {
            return Ok(());
        }
        let key = self.key.as_ref().expect("current transfer key");
        let (digest, total) = key.asset(resource);
        match response {
            http::Response::MapUploadPrepared { bytes, .. } => {
                ensure!(bytes.len() == total, "prepared resource differs");
                let transfer = self.transfer.as_mut().unwrap();
                transfer.bytes = bytes;
                transfer.in_flight = false;
            }
            http::Response::CachedMap {
                loaded: Some(loaded),
                ..
            }
            | http::Response::MapInstalled { loaded, .. } => self.accept_download(
                frontend,
                platform,
                match_state,
                process_assets,
                renderer,
                loaded,
                resource,
            )?,
            http::Response::CachedMap { loaded: None, .. } => {
                self.transfer.as_mut().unwrap().in_flight = false;
                // Quick Match and launcher-started games show no room to ask in.
                let dialog = !crate::app::shell_network_setup::quick_match(
                    frontend.network_setup_protocol.as_ref(),
                ) && !crate::app::shell_network_setup::external(
                    frontend.network_setup_protocol.as_ref(),
                );
                if dialog
                    && !crate::app::shell_network_setup::missing_map_download_agreed(
                        frontend.multiplayer_setup.as_ref(),
                    )
                    && self.ask_before_download()
                {
                    set_network_map_progress(
                        &mut frontend.network_setup_protocol,
                        &match_state.session,
                        None,
                    );
                    let metadata = match_state
                        .session
                        .network_map_metadata()
                        .expect("transfer metadata");
                    // Only custom maps are kept (`keep_for_choose_map`).
                    let offer = if metadata.official {
                        "VERA:MissingMapOffer"
                    } else {
                        "VERA:MissingMapOfferKept"
                    };
                    let message = crate::i18n::tr_count(
                        offer,
                        metadata.player_capacity,
                        &[
                            ("size", &(metadata.byte_length.max(0) as u32).div_ceil(1024)),
                            ("map", &metadata.display_name),
                        ],
                    );
                    let name = metadata.display_name.clone();
                    crate::app::shell_network_setup::offer_missing_map(
                        &platform.window,
                        frontend,
                        match_state,
                        name,
                        message,
                        "VERA:Download",
                    );
                }
            }
            http::Response::MapUploaded { completed, .. } => {
                let transfer = self.transfer.as_mut().unwrap();
                ensure!(
                    completed == (transfer.offset + MAP_CHUNK_BYTES).min(total),
                    "upload offset differs"
                );
                transfer.advanced_to(completed);
                set_network_map_progress(
                    &mut frontend.network_setup_protocol,
                    &match_state.session,
                    Some((completed as u32, total as u32)),
                );
                if completed == total {
                    if resource == TransferResource::Preview {
                        let config = platform
                            .game_config
                            .as_ref()
                            .ok_or_else(|| anyhow!("no asset directory"))?;
                        let source = crate::map::source::transfer_file_source(
                            &crate::map::source::random_map_dir(&config.paths.ra2_dir)
                                .join(crate::map::skirmish_scenarios::RANDMAP_PREVIEW),
                            resource,
                        )?;
                        ensure!(
                            source.physical_identity() == Some((total, digest)),
                            "preview changed during upload"
                        );
                        match_state
                            .session
                            .accept_network_random_map_preview(&key.digest, source)?;
                    }
                    self.finish_resource(
                        frontend,
                        &platform.window,
                        match_state,
                        process_assets,
                        renderer,
                        resource,
                    )?;
                }
            }
            http::Response::MapChunk {
                digest: received,
                total: received_total,
                offset,
                bytes,
                complete,
                ..
            } => {
                let transfer = self.transfer.as_mut().unwrap();
                transfer.append_chunk(key, &received, received_total, offset, &bytes, complete)?;
                set_network_map_progress(
                    &mut frontend.network_setup_protocol,
                    &match_state.session,
                    Some((transfer.offset as u32, total as u32)),
                );
                if complete {
                    service.request_map(
                        self.generation,
                        &transfer.active,
                        http::MapIo::InstallMap {
                            resource,
                            directory: transfer.directory.clone(),
                            digest: digest.to_owned(),
                            total,
                            bytes: std::mem::take(&mut transfer.bytes),
                        },
                    )?;
                    transfer.in_flight = true;
                }
            }
            http::Response::MapPending { .. } => {
                let transfer = self.transfer.as_mut().unwrap();
                transfer.in_flight = false;
                transfer.retry_at = Instant::now() + Duration::from_secs(2);
            }
            http::Response::MapFailure { retryable, .. } => {
                self.transfer
                    .as_mut()
                    .unwrap()
                    .retry_after_failure(retryable)?;
            }
            _ => {}
        }
        Ok(())
    }
}
pub(super) fn response(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    service: &mut LiveService,
    response: http::Response,
) {
    let mut maps = std::mem::take(&mut service.maps);
    if maps
        .response(
            frontend,
            platform,
            match_state,
            process_assets,
            renderer,
            service,
            response,
        )
        .is_err()
    {
        maps.fail(
            frontend,
            &platform.window,
            match_state,
            process_assets,
            renderer,
        );
    }
    service.maps = maps;
}
pub(super) fn available(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    service: &mut LiveService,
    digest: &str,
    total: usize,
) {
    let room = super::super::lobby::current_channel(frontend.wol.as_ref())
        .map(|channel| channel.name.clone());
    let mut maps = std::mem::take(&mut service.maps);
    if maps
        .heard_available(
            frontend,
            window,
            match_state,
            process_assets,
            renderer,
            room,
            digest,
            total,
        )
        .is_err()
    {
        maps.fail(frontend, window, match_state, process_assets, renderer);
    }
    service.maps = maps;
    if service.maps.key.as_ref().is_some_and(|key| {
        key.digest == digest && key.total == total
            || key.random.as_ref().is_some_and(|r| {
                r.preview.source_sha256 == digest && r.preview.byte_length == total
            })
    }) {
        if service.maps.failed && !match_state.session.host() {
            service.maps.key = None;
        }
        if let Some(transfer) = service.maps.transfer.as_mut() {
            transfer.retry_at = Instant::now();
        }
    }
}

impl Transfers {
    /// VERA asks first: a guest whose cache misses the room's map holds its
    /// download until the player agrees. Seeds and generated previews are
    /// not maps the player keeps, so they never ask.
    fn ask_before_download(&mut self) -> bool {
        let ask = self.key.as_ref().is_some_and(|key| key.random.is_none())
            && self.transfer.as_ref().is_some_and(|transfer| {
                transfer.resource == TransferResource::Map
                    && matches!(transfer.direction, Direction::Download)
            });
        self.asking |= ask;
        ask
    }
    /// The player agreed: the download starts now, with its full deadline.
    fn agreed_to_download(&mut self) {
        self.asking = false;
        if let Some(transfer) = self.transfer.as_mut() {
            let now = Instant::now();
            transfer.retry_at = now;
            transfer.deadline = now + MAP_PROGRESS_TIMEOUT;
        }
    }
}

impl Transfer {
    /// Only an accepted chunk gives the next chunk another full idle interval.
    fn advanced_to(&mut self, completed: usize) {
        self.offset = completed;
        self.in_flight = false;
        self.retries = 0;
        self.deadline = Instant::now() + MAP_PROGRESS_TIMEOUT;
    }

    /// A dropped connection retries, less often each time, until the
    /// transfer's deadline rather than failing on a short drop.
    fn retry_after_failure(&mut self, retryable: bool) -> Result<()> {
        ensure!(retryable, "map transfer failed");
        self.retries = self.retries.saturating_add(1);
        self.in_flight = false;
        self.retry_at = Instant::now() + Duration::from_secs((1 << self.retries.min(4)).min(15));
        Ok(())
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn a_map_the_lobby_already_holds_spares_only_that_rooms_upload() {
        let digest = "a".repeat(64);
        let mut maps = Transfers {
            key: Some(Key {
                room: "room".into(),
                digest: digest.clone(),
                total: 9,
                effective_digest: "b".repeat(64),
                random: None,
            }),
            ..Default::default()
        };
        assert!(!maps.lobby_holds(TransferResource::Map));
        // Heard with the host's settings, before its upload began.
        maps.available = Some(("room".into(), digest.clone(), 9));
        assert!(maps.lobby_holds(TransferResource::Map));
        assert!(!maps.lobby_holds(TransferResource::Preview));
        maps.available = Some(("another room".into(), digest.clone(), 9));
        assert!(!maps.lobby_holds(TransferResource::Map));
        maps.available = Some(("room".into(), digest.clone(), 10));
        assert!(!maps.lobby_holds(TransferResource::Map));
        // Another map's settings drop it, so choosing this map again uploads
        // unless the lobby says again that it holds it.
        let key = maps.key.take().unwrap();
        maps.available = Some(("room".into(), digest, 9));
        maps.forget_other_available(&key);
        maps.key = Some(key);
        assert!(maps.lobby_holds(TransferResource::Map));
        let other = Key {
            room: "room".into(),
            digest: "c".repeat(64),
            total: 9,
            effective_digest: "b".repeat(64),
            random: None,
        };
        maps.forget_other_available(&other);
        assert!(maps.available.is_none());
    }
    #[test]
    fn a_rematch_checks_the_map_a_match_end_dropped_without_uploading_it() {
        use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(fixture["launch"].clone()).unwrap();
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch.clone(), true, None, false)
            .unwrap();
        let directory = crate::map::source::test_support::TestDirectory::new("rematch-map");
        directory.write(
            "room.map",
            b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n",
        );
        let source = crate::map::source::transfer_file_source(
            &directory.path().join("room.map"),
            TransferResource::Map,
        )
        .unwrap();
        let (total, digest) = source.physical_identity().unwrap();
        let digest = digest.to_owned();
        let metadata = crate::app::match_runtime::session::NetworkMapMetadata {
            display_name: "Room map".into(),
            player_capacity: 8,
            official: false,
            byte_length: total as i32,
            digest: "Room".into(),
            source_sha256: digest.clone(),
            effective_digest: "b".repeat(64),
        };
        session
            .accept_network_map_metadata(metadata.clone(), None, true)
            .unwrap();
        session
            .accept_external_network_map_source(source, "b".repeat(64))
            .unwrap();
        let mut maps = Transfers {
            key: Some(Key {
                room: "room".into(),
                digest: digest.clone(),
                total,
                effective_digest: "b".repeat(64),
                random: None,
            }),
            available: Some(("room".into(), digest, total)),
            complete: true,
            ..Default::default()
        };
        maps.recheck_lost_source(&session, false);
        assert!(maps.complete && maps.next_resource.is_none());
        // The match ends, then the reopened room brings back its setup and
        // the same map: none of that restores the dropped source.
        session.return_to_network_lobby();
        session
            .accept_network_setup(launch, true, None, false)
            .unwrap();
        session
            .accept_network_map_metadata(metadata, None, true)
            .unwrap();
        assert!(session.network_local_map_source().is_none());
        maps.recheck_lost_source(&session, false);
        assert!(!maps.complete);
        assert_eq!(maps.next_resource, Some(TransferResource::Map));
        assert!(maps.lobby_holds(TransferResource::Map));
    }
    fn download(resource: TransferResource) -> Transfer {
        let now = Instant::now();
        Transfer {
            resource,
            direction: Direction::Download,
            bytes: Vec::new(),
            offset: 0,
            in_flight: false,
            retry_at: now,
            deadline: now,
            retries: 0,
            active: Arc::new(()),
            directory: PathBuf::new(),
        }
    }
    #[test]
    fn a_guest_missing_the_map_waits_for_its_answer_before_downloading() {
        let now = Instant::now();
        let mut maps = Transfers {
            key: Some(Key {
                room: "room".into(),
                digest: "a".repeat(64),
                total: 9,
                effective_digest: "b".repeat(64),
                random: None,
            }),
            transfer: Some(download(TransferResource::Map)),
            ..Default::default()
        };
        assert!(maps.ask_before_download());
        assert!(maps.asking);
        maps.agreed_to_download();
        assert!(!maps.asking);
        assert!(maps.transfer.as_ref().unwrap().deadline > now);
        // A host's upload never asks.
        maps.transfer.as_mut().unwrap().direction = Direction::Upload;
        assert!(!maps.ask_before_download());
        // Nor does a generated map's seed.
        maps.transfer = Some(download(TransferResource::Seed));
        assert!(!maps.ask_before_download());
    }
    #[test]
    fn retry_in_the_same_room_starts_a_fresh_attempt_for_the_selected_map() {
        let map = |room: &str| Key {
            room: room.into(),
            digest: "a".repeat(64),
            total: 9,
            effective_digest: "b".repeat(64),
            random: None,
        };
        let mut maps = Transfers::default();
        assert!(maps.select_map(map("room"), false));
        let first = maps.generation;
        // Source checking and final verification can fail outside the chunk phase.
        assert!(maps.transfer.is_none());
        assert!(matches!(
            maps.stop_failed(Some("Room map")),
            Some(FailureNotice::Retry("Room map"))
        ));
        assert!(!maps.select_map(map("room"), false));
        assert!(maps.failed && maps.next_resource.is_none());
        assert!(!maps.select_map(map("room"), true));
        assert!(!maps.failed && maps.next_resource == Some(TransferResource::Map));
        let retried = maps.generation;
        maps.transfer = Some(download(TransferResource::Map));
        assert!(maps.transfer_for_response(first).is_none());
        assert!(maps.transfer_for_response(retried).is_some());
        let mut replacement = map("room");
        replacement.digest = "c".repeat(64);
        assert!(
            maps.select_map(replacement, true),
            "dismiss the old map's offer"
        );
        maps.transfer = Some(download(TransferResource::Map));
        assert!(maps.transfer_for_response(retried).is_none());
        assert!(maps.select_map(map("other room"), true));
    }
    #[test]
    fn a_dropped_connection_keeps_a_map_transfer_retrying() {
        let now = Instant::now();
        let mut transfer = Transfer {
            active: Arc::new(()),
            resource: TransferResource::Map,
            direction: Direction::Download,
            bytes: Vec::new(),
            offset: 0,
            in_flight: true,
            retry_at: now,
            deadline: now + MAP_PROGRESS_TIMEOUT,
            retries: 0,
            directory: PathBuf::new(),
        };
        for _ in 0..6 {
            transfer.retry_after_failure(true).unwrap();
        }
        assert!(!transfer.in_flight);
        assert!(transfer.retry_at <= Instant::now() + Duration::from_secs(15));
        assert!(transfer.retry_after_failure(false).is_err());
    }
    #[test]
    fn download_rejects_oversized_or_duplicate_chunks_and_changed_identity_without_appending() {
        let key = Key {
            room: "room".into(),
            digest: "a".repeat(64),
            total: MAP_CHUNK_BYTES + 2,
            effective_digest: "b".repeat(64),
            random: None,
        };
        let now = Instant::now();
        let mut transfer = Transfer {
            active: Arc::new(()),
            resource: TransferResource::Map,
            direction: Direction::Download,
            bytes: Vec::new(),
            offset: 0,
            in_flight: true,
            retry_at: now,
            deadline: now,
            retries: 0,
            directory: PathBuf::new(),
        };
        let (full, oversized) = (vec![1; MAP_CHUNK_BYTES], vec![1; MAP_CHUNK_BYTES + 1]);
        let (digest, total) = (&key.digest, key.total);
        assert!(
            transfer
                .append_chunk(&key, digest, total, 0, &oversized, false)
                .is_err()
        );
        assert_eq!(
            transfer.deadline, now,
            "invalid data cannot extend the transfer"
        );
        transfer
            .append_chunk(&key, digest, total, 0, &full, false)
            .unwrap();
        assert!(
            transfer.deadline >= now + MAP_PROGRESS_TIMEOUT,
            "verified progress renews the full wait"
        );
        assert!(
            transfer
                .append_chunk(&key, digest, total, 0, &full, false)
                .is_err()
        );
        let changed = "b".repeat(64);
        assert!(
            transfer
                .append_chunk(&key, &changed, total, MAP_CHUNK_BYTES, &[2, 3], true)
                .is_err()
        );
        assert_eq!(transfer.offset, MAP_CHUNK_BYTES);
        transfer
            .append_chunk(&key, digest, total, MAP_CHUNK_BYTES, &[2, 3], true)
            .unwrap();
        assert_eq!(&transfer.bytes[MAP_CHUNK_BYTES..], &[2, 3]);
    }

    /// The lobby keeps its own copies of a room's speeds, the rate it signs
    /// for each and the map chunk size; this file is what its tests compare.
    #[test]
    fn the_lobby_reads_the_games_speeds_rates_and_map_chunk() {
        let game_speeds: Vec<_> = (0..=u8::MAX)
            .filter(|&speed| crate::sim::game_options::is_selectable_game_speed(speed))
            .map(|speed| {
                // The lobby signs, as `tick_hz`, the PreCalcFrameRate the host
                // (the channel owner) sets for its speed.
                let timing = crate::net::lockstep::wol_start_timing(i32::from(speed), [0], true);
                serde_json::json!({"game_speed": speed, "tick_hz": timing.precalc_frame_rate})
            })
            .collect();
        crate::test_fixture::assert_sent_json(
            "tests/fixtures/lobby/game_contract.json",
            &serde_json::json!({
                "about": "Each Game Speed a room can select (`launch.options.game_speed`) \
                    with the rate a match at that speed runs at, which the lobby signs \
                    as the ordering certificate's `tick_hz`, and the map transfer's \
                    chunk size (docs/lobby-protocol.md). The game's test in \
                    src/app/shell_wol/live_service/maps.rs writes this file.",
                "game_speeds": game_speeds,
                "map_chunk_bytes": MAP_CHUNK_BYTES,
            }),
        );
    }
}
