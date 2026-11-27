//! Bounded LAN map/seed/preview delivery over the existing reliable global FIFO.
//!
//! VERA prefetches while setup is editable. Native file/preview requests
//! (5E7EB0/693B70/DD8A0) consume the same verified result before GO. This is a
//! VERA transport adaptation; no legacy file-transfer grammar is retained.
//! Admission belongs to LanService's current SessionScope and MatchSession.
//! Source decoding, generated W staging and cache installation have one shared
//! owner with WOL in shell_network_setup::maps and map::source.

use crate::util::time::{Duration, Instant};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use anyhow::{Result, anyhow, ensure};

use crate::app::AppState;
use crate::app::shell_main_menu::ClientStep;
use crate::app::shell_network_setup as setup;
use crate::app::shell_network_setup::maps::{self as shared, GeneratedMapManifest};
use crate::map::source::{LoadedTransferAsset, TransferMapSource, TransferResource};
use crate::net::lan_messages::SessionScope;

pub(super) const MAGIC: &[u8; 4] = b"VLMP";
// 4 magic + kind + epoch16 + options revision8 + identity32 + resource + offset4.
const HEADER: usize = 66;
const CHUNK: usize = crate::net::lan_messages::LAN_MESSAGE_LEN - HEADER;
const WINDOW_PARTS: usize = 32;
const WINDOW_BYTES: usize = CHUNK * WINDOW_PARTS;
const MAX_PUMP_PACKETS: usize = 128;
const MAX_QUEUED_ASSET_PACKETS: usize = 256;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Asset {
    resource: TransferResource,
    digest: String,
    total: usize,
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    scope: SessionScope,
    identity: [u8; 32],
    host: bool,
    effective: String,
    generated: Option<GeneratedMapManifest>,
    assets: Vec<Asset>,
}

impl Key {
    fn selected(
        session: &crate::app::match_runtime::session::MatchSession,
        scope: SessionScope,
        generated: Option<GeneratedMapManifest>,
    ) -> Result<Self> {
        ensure!(
            scope.epoch != [0; 16] && scope.revision > 0,
            "map transfer has no admitted options scope"
        );
        let launch = session
            .network_setup()
            .ok_or_else(|| anyhow!("map transfer has no setup"))?;
        let file = launch
            .selected_map_file
            .as_deref()
            .ok_or_else(|| anyhow!("map transfer has no selection"))?;
        crate::map::source::validate_network_map_name(file)?;
        let metadata = session
            .network_map_metadata()
            .ok_or_else(|| anyhow!("map transfer has no metadata"))?;
        // `Transfers::selected` checks the manifest against the selection
        // (`shared::accept_generated_manifest`) before it keeps this key.
        let total = shared::transfer_length(metadata)?;
        let mut assets = vec![Asset {
            resource: if crate::map::rmg::is_seed_selection(file) {
                TransferResource::Seed
            } else {
                TransferResource::Map
            },
            digest: metadata.source_sha256.clone(),
            total,
        }];
        if let Some(manifest) = &generated {
            assets.push(Asset {
                resource: TransferResource::Preview,
                digest: manifest.preview.source_sha256.clone(),
                total: manifest.preview.byte_length,
            });
        }
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(&serde_json::to_vec(&(
            file,
            &metadata.source_sha256,
            total,
            &metadata.effective_digest,
            &metadata.digest,
            metadata.player_capacity,
            &generated,
        ))?);
        let identity = digest.finalize();
        Ok(Self {
            scope,
            identity,
            host: session.host(),
            effective: metadata.effective_digest.clone(),
            generated,
            assets,
        })
    }

    fn asset(&self, resource: TransferResource) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.resource == resource)
    }
}

fn resource_index(resource: TransferResource) -> usize {
    match resource {
        TransferResource::Map => 0,
        TransferResource::Seed => 1,
        TransferResource::Preview => 2,
    }
}

#[derive(Clone, Copy)]
enum PacketKind {
    Request,
    Chunk,
}
struct Packet<'a> {
    kind: PacketKind,
    scope: SessionScope,
    identity: [u8; 32],
    resource: TransferResource,
    offset: usize,
    bytes: &'a [u8],
}

impl Packet<'_> {
    fn matches_key(&self, key: &Key) -> bool {
        self.scope == key.scope && self.identity == key.identity
    }

    fn decode(bytes: &[u8]) -> Result<Packet<'_>> {
        ensure!(
            bytes.len() >= HEADER
                && bytes.len() <= crate::net::lan_messages::LAN_MESSAGE_LEN
                && bytes.starts_with(MAGIC),
            "invalid LAN asset packet"
        );
        let kind = match bytes[4] {
            0 => PacketKind::Request,
            1 => PacketKind::Chunk,
            _ => anyhow::bail!("invalid LAN asset packet kind"),
        };
        let resource = match bytes[61] {
            0 => TransferResource::Map,
            1 => TransferResource::Seed,
            2 => TransferResource::Preview,
            _ => anyhow::bail!("invalid LAN asset resource"),
        };
        ensure!(
            match kind {
                PacketKind::Request => bytes.len() == HEADER,
                PacketKind::Chunk => bytes.len() > HEADER,
            },
            "invalid LAN asset packet size"
        );
        Ok(Packet {
            kind,
            scope: SessionScope {
                epoch: bytes[5..21].try_into()?,
                revision: u64::from_le_bytes(bytes[21..29].try_into()?),
            },
            identity: bytes[29..61].try_into()?,
            resource,
            offset: u32::from_le_bytes(bytes[62..66].try_into()?) as usize,
            bytes: &bytes[HEADER..],
        })
    }

    fn encode(
        kind: PacketKind,
        key: &Key,
        resource: TransferResource,
        offset: usize,
        bytes: &[u8],
    ) -> Result<Vec<u8>> {
        ensure!(
            bytes.len() <= CHUNK && offset <= u32::MAX as usize,
            "LAN asset packet exceeds its bound"
        );
        let mut packet = Vec::with_capacity(HEADER + bytes.len());
        packet.extend_from_slice(MAGIC);
        packet.push(match kind {
            PacketKind::Request => 0,
            PacketKind::Chunk => 1,
        });
        packet.extend_from_slice(&key.scope.epoch);
        packet.extend_from_slice(&key.scope.revision.to_le_bytes());
        packet.extend_from_slice(&key.identity);
        packet.push(resource_index(resource) as u8);
        packet.extend_from_slice(&(offset as u32).to_le_bytes());
        packet.extend_from_slice(bytes);
        Ok(packet)
    }
}

struct Download {
    asset: Asset,
    bytes: Vec<u8>,
    parts: Vec<Option<Vec<u8>>>,
    requested: bool,
}

impl Download {
    fn new(asset: Asset) -> Self {
        let count = asset.total.div_ceil(CHUNK).min(WINDOW_PARTS);
        Self {
            bytes: Vec::with_capacity(asset.total),
            asset,
            parts: vec![None; count],
            requested: false,
        }
    }

    fn receive(&mut self, offset: usize, bytes: &[u8]) -> Result<bool> {
        if offset < self.bytes.len() {
            return Ok(false);
        }
        ensure!(self.requested, "unsolicited LAN asset window");
        let relative = offset - self.bytes.len();
        ensure!(
            relative % CHUNK == 0 && relative / CHUNK < self.parts.len(),
            "LAN asset chunk is outside the requested window"
        );
        let count = CHUNK.min(self.asset.total.saturating_sub(offset));
        ensure!(
            count > 0 && bytes.len() == count,
            "LAN asset chunk has a different size"
        );
        let index = relative / CHUNK;
        if let Some(previous) = &self.parts[index] {
            ensure!(previous == bytes, "LAN asset duplicate changed its bytes");
        } else {
            self.parts[index] = Some(bytes.to_vec());
        }
        if self.parts.iter().any(Option::is_none) {
            return Ok(false);
        }
        for part in &mut self.parts {
            self.bytes
                .extend_from_slice(&part.take().expect("complete asset window"));
        }
        self.requested = false;
        let remaining = self.asset.total - self.bytes.len();
        self.parts = vec![None; remaining.div_ceil(CHUNK).min(WINDOW_PARTS)];
        Ok(remaining == 0)
    }

    fn completed_bytes(&self) -> usize {
        self.bytes.len()
            + self
                .parts
                .iter()
                .filter_map(Option::as_ref)
                .map(Vec::len)
                .sum::<usize>()
    }
}

struct SendWindow {
    resource: TransferResource,
    base: usize,
    next: usize,
    end: usize,
}
struct Peer {
    endpoint: SocketAddrV4,
    next_offsets: [usize; 3],
    window: Option<SendWindow>,
}

enum JobAction {
    Read(TransferMapSource),
    Cached(PathBuf),
    Install(PathBuf, Vec<u8>),
}
struct Job {
    generation: u64,
    asset: Asset,
    action: JobAction,
}
enum IoValue {
    Bytes(Vec<u8>),
    Cached(Option<LoadedTransferAsset>),
    Installed(LoadedTransferAsset),
}
struct Completion {
    generation: u64,
    resource: TransferResource,
    result: Result<IoValue, ()>,
}
struct Worker {
    requests: SyncSender<Job>,
    responses: Receiver<Completion>,
}
impl Worker {
    fn new() -> Result<Self> {
        let (input, incoming) = mpsc::sync_channel::<Job>(1);
        let (output, responses) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("lan-map-assets".into())
            .spawn(move || {
                while let Ok(job) = incoming.recv() {
                    let asset = &job.asset;
                    let result = match job.action {
                        JobAction::Read(source) => source
                            .read(&asset.digest, asset.total)
                            .map(IoValue::Bytes)
                            .map_err(|_| ()),
                        JobAction::Cached(directory) => Ok(IoValue::Cached(
                            crate::map::source::load_received_asset(
                                &directory,
                                &asset.digest,
                                asset.total,
                                asset.resource,
                            )
                            .ok(),
                        )),
                        JobAction::Install(directory, bytes) => {
                            crate::map::source::install_received_asset(
                                &directory,
                                bytes,
                                &asset.digest,
                                asset.total,
                                asset.resource,
                            )
                            .map(IoValue::Installed)
                            .map_err(|_| ())
                        }
                    };
                    if output
                        .send(Completion {
                            generation: job.generation,
                            resource: asset.resource,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests: input,
            responses,
        })
    }
}

#[derive(Default)]
enum Flow {
    #[default]
    Idle,
    Inspect,
    AwaitRead(TransferResource),
    AwaitCache(TransferResource),
    Download(Download),
    AwaitInstall(TransferResource),
    Serving,
    Complete,
    Failed,
}

#[derive(Default)]
pub(super) struct Transfers {
    generation: u64,
    key: Option<Key>,
    resource: usize,
    flow: Flow,
    worker: Option<Worker>,
    pending_job: Option<Job>,
    deadline: Option<Instant>,
    host_bytes: Vec<(TransferResource, Vec<u8>)>,
    peers: Vec<Peer>,
    next_peer: usize,
}

impl Transfers {
    pub(super) fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.key = None;
        self.resource = 0;
        self.flow = Flow::Idle;
        self.pending_job = None;
        self.deadline = None;
        self.worker = None;
        self.host_bytes.clear();
        self.peers.clear();
    }

    fn selected(
        &mut self,
        frontend: &mut crate::app::frontend::state::FrontendState,
        session: &mut crate::app::match_runtime::session::MatchSession,
        scope: SessionScope,
        generated: Option<GeneratedMapManifest>,
    ) -> Result<()> {
        let key = Key::selected(session, scope, generated)?;
        if self.key.as_ref() == Some(&key) {
            return Ok(());
        }
        shared::accept_generated_manifest(
            &mut frontend.multiplayer_shell_runtime,
            session,
            self.key.as_ref().and_then(|key| key.generated.as_ref()),
            key.generated.as_ref(),
        )?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("LAN asset generation exhausted"))?;
        self.resource = 0;
        self.flow = Flow::Inspect;
        self.pending_job = None;
        self.deadline = Some(Instant::now() + TRANSFER_TIMEOUT);
        self.host_bytes.clear();
        self.peers.clear();
        if !key.host {
            setup::set_network_map_progress(
                &mut frontend.network_setup_protocol,
                session,
                Some((0, key.assets[0].total as u32)),
            );
        }
        self.key = Some(key);
        Ok(())
    }

    pub(super) fn current_verified(&self, scope: SessionScope) -> bool {
        self.key.as_ref().is_some_and(|key| key.scope == scope)
            && matches!(self.flow, Flow::Serving | Flow::Complete)
    }

    fn queue_job(&mut self, asset: Asset, action: JobAction) -> Result<()> {
        if self.worker.is_none() {
            self.worker = Some(Worker::new()?);
        }
        self.pending_job = Some(Job {
            generation: self.generation,
            asset,
            action,
        });
        Ok(())
    }

    fn begin_resource(&mut self, state: &mut AppState) -> Result<()> {
        let key = self.key.as_ref().expect("selected LAN asset");
        let asset = key.assets[self.resource].clone();
        let host = key.host;
        let local = shared::local_transfer_source(
            state.platform.game_config.as_ref(),
            &mut state.frontend,
            &state.match_state.session,
            &state.process_assets,
            asset.resource,
        )?;
        if let Some(source) = &local {
            ensure!(
                source.physical_identity() == Some((asset.total, asset.digest.as_str())),
                "local LAN asset changed"
            );
        }
        if asset.resource != TransferResource::Preview
            && let Some(source) = local.clone()
        {
            state
                .match_state
                .session
                .accept_verified_network_map_source(source, key.effective.clone())?;
            if !host {
                return self.finish_resource(state);
            }
        }
        if host {
            let assets = state
                .process_assets
                .manager()
                .ok_or_else(|| anyhow!("assets missing"))?;
            let source = local.ok_or_else(|| anyhow!("host LAN asset is unavailable"))?;
            let source = crate::map::source::prepare_transfer_map_source(&source, assets)?;
            self.flow = Flow::AwaitRead(asset.resource);
            self.queue_job(asset, JobAction::Read(source))?;
        } else {
            let directory = state
                .platform
                .game_config
                .as_ref()
                .ok_or_else(|| anyhow!("settings directory missing"))?
                .paths
                .settings_dir()
                .join("NetworkMaps");
            self.flow = Flow::AwaitCache(asset.resource);
            self.queue_job(asset, JobAction::Cached(directory))?;
        }
        Ok(())
    }

    fn finish_resource(&mut self, state: &mut AppState) -> Result<()> {
        self.resource += 1;
        let key = self.key.as_ref().expect("selected LAN asset");
        if self.resource < key.assets.len() {
            self.flow = Flow::Inspect;
            self.deadline = Some(Instant::now() + TRANSFER_TIMEOUT);
            if !key.host {
                setup::set_network_map_progress(
                    &mut state.frontend.network_setup_protocol,
                    &state.match_state.session,
                    Some((0, key.assets[self.resource].total as u32)),
                );
            }
            return Ok(());
        }
        let source = state
            .match_state
            .session
            .network_local_map_source()
            .cloned()
            .ok_or_else(|| anyhow!("verified LAN map source missing"))?;
        // A downloaded seed can change selected-mode/RMG rules. Recompute
        // through ProcessAssets before K1/Ready acknowledges the host identity.
        super::verify_setup_agreement(state)?;
        setup::accept_network_map_source(
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            source,
            key.effective.clone(),
        )?;
        self.deadline = None;
        self.flow = if key.host {
            Flow::Serving
        } else {
            Flow::Complete
        };
        Ok(())
    }

    fn completion(&mut self, state: &mut AppState, completion: Completion) -> Result<()> {
        if completion.generation != self.generation || self.key.is_none() {
            return Ok(());
        }
        let resource = completion.resource;
        let value = completion
            .result
            .map_err(|()| anyhow!("LAN asset read or validation failed"))?;
        match (&self.flow, value) {
            (Flow::AwaitRead(expected), IoValue::Bytes(bytes)) if *expected == resource => {
                let key = self.key.as_ref().expect("selected LAN asset");
                if resource == TransferResource::Preview {
                    let source = shared::local_transfer_source(
                        state.platform.game_config.as_ref(),
                        &mut state.frontend,
                        &state.match_state.session,
                        &state.process_assets,
                        resource,
                    )?
                    .ok_or_else(|| anyhow!("generated preview disappeared"))?;
                    let asset = key.asset(resource).expect("preview manifest");
                    ensure!(
                        source.physical_identity() == Some((asset.total, asset.digest.as_str())),
                        "generated preview changed"
                    );
                    state
                        .match_state
                        .session
                        .accept_network_random_map_preview(&key.assets[0].digest, source)?;
                }
                self.host_bytes.push((resource, bytes));
                self.finish_resource(state)?;
            }
            (Flow::AwaitCache(expected), IoValue::Cached(Some(loaded)))
                if *expected == resource =>
            {
                shared::accept_transfer_asset(
                    &state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state.session,
                    &state.process_assets,
                    loaded,
                    resource,
                    self.key.as_ref().and_then(|key| key.generated.as_ref()),
                )?;
                self.finish_resource(state)?;
            }
            (Flow::AwaitCache(expected), IoValue::Cached(None)) if *expected == resource => {
                let asset = self
                    .key
                    .as_ref()
                    .expect("selected LAN asset")
                    .asset(resource)
                    .expect("selected resource")
                    .clone();
                self.flow = Flow::Download(Download::new(asset));
            }
            (Flow::AwaitInstall(expected), IoValue::Installed(loaded)) if *expected == resource => {
                shared::accept_transfer_asset(
                    &state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state.session,
                    &state.process_assets,
                    loaded,
                    resource,
                    self.key.as_ref().and_then(|key| key.generated.as_ref()),
                )?;
                self.finish_resource(state)?;
            }
            _ => anyhow::bail!("LAN asset completion differs from current work"),
        }
        Ok(())
    }

    fn pump(&mut self, state: &mut AppState) -> Result<()> {
        ensure!(
            self.deadline
                .is_none_or(|deadline| Instant::now() <= deadline),
            "LAN map transfer timed out"
        );
        loop {
            let Some(worker) = self.worker.as_ref() else {
                break;
            };
            match worker.responses.try_recv() {
                Ok(completion) => self.completion(state, completion)?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("LAN asset worker stopped"),
            }
        }
        if matches!(self.flow, Flow::Inspect) {
            self.begin_resource(state)?;
        }
        if let Some(job) = self.pending_job.take() {
            match self
                .worker
                .as_ref()
                .expect("pending job worker")
                .requests
                .try_send(job)
            {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => self.pending_job = Some(job),
                Err(TrySendError::Disconnected(_)) => anyhow::bail!("LAN asset worker stopped"),
            }
        }
        let Some(key) = self.key.as_ref() else {
            return Ok(());
        };
        if let Flow::Download(download) = &mut self.flow {
            if !download.requested {
                let endpoint = state
                    .match_state
                    .lan_service
                    .as_ref()
                    .and_then(|service| service.admitted_host.as_ref())
                    .map(|(_, endpoint)| *endpoint)
                    .ok_or_else(|| anyhow!("no admitted LAN map host"))?;
                let packet = Packet::encode(
                    PacketKind::Request,
                    key,
                    download.asset.resource,
                    download.bytes.len(),
                    &[],
                )?;
                if queue(&mut state.match_state, endpoint, packet)? {
                    download.requested = true;
                }
            }
        }
        if key.host {
            self.pump_host(&mut state.match_state)?;
        }
        Ok(())
    }

    fn pump_host(
        &mut self,
        match_state: &mut crate::app::match_runtime::state::MatchState,
    ) -> Result<()> {
        self.peers.retain(|peer| {
            match_state
                .session
                .network_endpoint_source(peer.endpoint)
                .is_some()
        });
        if self.peers.is_empty() {
            return Ok(());
        }
        for _ in 0..MAX_PUMP_PACKETS {
            if match_state
                .lan_service
                .as_ref()
                .is_none_or(|service| service.pending_setup.len() >= MAX_QUEUED_ASSET_PACKETS)
            {
                break;
            }
            let index = self.next_peer % self.peers.len();
            self.next_peer = (index + 1) % self.peers.len();
            let peer = &mut self.peers[index];
            let Some(window) = &mut peer.window else {
                continue;
            };
            let Some((_, bytes)) = self
                .host_bytes
                .iter()
                .find(|(resource, _)| *resource == window.resource)
            else {
                continue;
            };
            let end = (window.next + CHUNK).min(window.end);
            let packet = Packet::encode(
                PacketKind::Chunk,
                self.key.as_ref().expect("selected LAN map"),
                window.resource,
                window.next,
                &bytes[window.next..end],
            )?;
            if !queue(match_state, peer.endpoint, packet)? {
                break;
            }
            window.next = end;
            if end == window.end {
                peer.next_offsets[resource_index(window.resource)] = end;
                peer.window = None;
            }
        }
        Ok(())
    }

    fn receive(
        &mut self,
        game_config: Option<&crate::util::config::GameConfig>,
        frontend: &mut crate::app::frontend::state::FrontendState,
        match_state: &crate::app::match_runtime::state::MatchState,
        source: SocketAddrV4,
        packet: Packet<'_>,
    ) -> Result<()> {
        let Some(key) = &self.key else {
            return Ok(());
        };
        if !packet.matches_key(key) {
            return Ok(());
        }
        let Some(asset) = key.asset(packet.resource) else {
            return Ok(());
        };
        if key.host {
            ensure!(
                matches!(packet.kind, PacketKind::Request),
                "guest supplied LAN map bytes"
            );
            let admitted = match_state
                .session
                .network_endpoint_source(source)
                .ok_or_else(|| anyhow!("LAN asset request is not admitted"))?;
            ensure!(
                match_state
                    .session
                    .network_setup()
                    .is_some_and(|setup| admitted != setup.local_player_index),
                "local source requested LAN bytes"
            );
            ensure!(
                packet.offset < asset.total && packet.offset % WINDOW_BYTES == 0,
                "LAN asset request offset is not canonical"
            );
            let peer =
                if let Some(index) = self.peers.iter().position(|peer| peer.endpoint == source) {
                    &mut self.peers[index]
                } else {
                    ensure!(
                        self.peers.len() < crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT - 1,
                        "too many LAN asset peers"
                    );
                    self.peers.push(Peer {
                        endpoint: source,
                        next_offsets: [0; 3],
                        window: None,
                    });
                    self.peers.last_mut().expect("new LAN asset peer")
                };
            if let Some(window) = &peer.window {
                ensure!(
                    window.resource == packet.resource && window.base == packet.offset,
                    "LAN asset peer changed its outstanding window"
                );
                return Ok(());
            }
            if packet.offset < peer.next_offsets[resource_index(packet.resource)] {
                return Ok(());
            }
            ensure!(
                packet.offset == peer.next_offsets[resource_index(packet.resource)],
                "LAN asset peer skipped a window"
            );
            peer.window = Some(SendWindow {
                resource: packet.resource,
                base: packet.offset,
                next: packet.offset,
                end: (packet.offset + WINDOW_BYTES).min(asset.total),
            });
            return Ok(());
        }
        ensure!(
            match_state
                .lan_service
                .as_ref()
                .and_then(|service| service.admitted_host.as_ref())
                .is_some_and(|(_, host)| *host == source),
            "LAN asset chunk is not from the admitted host"
        );
        ensure!(
            matches!(packet.kind, PacketKind::Chunk),
            "host requested guest map bytes"
        );
        let Flow::Download(download) = &mut self.flow else {
            return Ok(());
        };
        if download.asset.resource != packet.resource {
            return Ok(());
        }
        let complete = download.receive(packet.offset, packet.bytes)?;
        setup::set_network_map_progress(
            &mut frontend.network_setup_protocol,
            &match_state.session,
            Some((
                download.completed_bytes() as u32,
                download.asset.total as u32,
            )),
        );
        if complete {
            let directory = game_config
                .ok_or_else(|| anyhow!("settings directory missing"))?
                .paths
                .settings_dir()
                .join("NetworkMaps");
            let Flow::Download(download) =
                std::mem::replace(&mut self.flow, Flow::AwaitInstall(packet.resource))
            else {
                unreachable!("download just completed");
            };
            self.queue_job(
                download.asset,
                JobAction::Install(directory, download.bytes),
            )?;
        }
        Ok(())
    }

    /// Stop the transfer; the returned step fails the GO handoff.
    fn fail(
        &mut self,
        network_setup_protocol: &mut Option<setup::NetworkSetupProtocol>,
        session: &crate::app::match_runtime::session::MatchSession,
    ) -> ClientStep {
        self.flow = Flow::Failed;
        self.pending_job = None;
        self.deadline = None;
        self.worker = None;
        self.host_bytes.clear();
        self.peers.clear();
        setup::set_network_map_progress(network_setup_protocol, session, None);
        ClientStep::Now(Box::new(setup::fail_go_handoff))
    }
}

fn queue(
    match_state: &mut crate::app::match_runtime::state::MatchState,
    endpoint: SocketAddrV4,
    bytes: Vec<u8>,
) -> Result<bool> {
    match_state
        .lan_service
        .as_mut()
        .ok_or_else(|| anyhow!("no LAN service"))?
        .queue_setup(std::iter::once((bytes, super::direct(endpoint))))
}

pub(super) fn received_options(
    state: &mut AppState,
    scope: SessionScope,
    generated: Option<GeneratedMapManifest>,
) -> Result<()> {
    let mut maps = std::mem::take(
        &mut state
            .match_state
            .lan_service
            .as_mut()
            .ok_or_else(|| anyhow!("no LAN service"))?
            .maps,
    );
    let result = maps.selected(
        &mut state.frontend,
        &mut state.match_state.session,
        scope,
        generated,
    );
    if result.is_err() {
        let step = maps.fail(
            &mut state.frontend.network_setup_protocol,
            &state.match_state.session,
        );
        crate::app::App::apply_step(state, step);
    }
    // The step above runs with the whole app state and can close the visit;
    // the transfer state then ends with the service it belonged to.
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.maps = maps;
    }
    result
}

pub(super) fn clear_campaign_transfer(
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
) {
    if let Some(service) = &mut match_state.lan_service {
        service.maps.cancel();
    }
    setup::set_network_map_progress(
        &mut frontend.network_setup_protocol,
        &match_state.session,
        None,
    );
}

pub(super) fn poll(state: &mut AppState) {
    if state.match_state.session.campaign_room().is_some() {
        return;
    }
    // A direct spawn's map is the launcher's file; nothing is transferred.
    if state
        .match_state
        .lan_service
        .as_ref()
        .is_some_and(|service| service.preadmitted)
    {
        return;
    }
    let scope = state
        .match_state
        .lan_service
        .as_ref()
        .and_then(super::LanService::setup_scope);
    let mut maps = match state.match_state.lan_service.as_mut() {
        Some(service) => std::mem::take(&mut service.maps),
        None => return,
    };
    let result = (|| -> Result<()> {
        let Some(scope) = scope.filter(|scope| scope.revision > 0) else {
            if maps.key.is_some() || maps.worker.is_some() {
                maps.cancel();
                setup::set_network_map_progress(
                    &mut state.frontend.network_setup_protocol,
                    &state.match_state.session,
                    None,
                );
            }
            return Ok(());
        };
        if state.frontend.network_setup_protocol.is_none()
            || state.match_state.session.network_setup().is_none()
        {
            if maps.key.is_some() || maps.worker.is_some() {
                maps.cancel();
                setup::set_network_map_progress(
                    &mut state.frontend.network_setup_protocol,
                    &state.match_state.session,
                    None,
                );
            }
            return Ok(());
        }
        if state.match_state.session.host()
            && maps.key.as_ref().is_none_or(|key| key.scope != scope)
        {
            let generated = shared::host_generated_manifest(
                state.platform.game_config.as_ref(),
                &state.frontend.multiplayer_shell_runtime,
                &state.match_state.session,
            )?;
            maps.selected(
                &mut state.frontend,
                &mut state.match_state.session,
                scope,
                generated,
            )?;
        }
        if maps.key.as_ref().is_some_and(|key| key.scope != scope) {
            maps.cancel();
            setup::set_network_map_progress(
                &mut state.frontend.network_setup_protocol,
                &state.match_state.session,
                None,
            );
        }
        maps.pump(state)
    })();
    if let Err(error) = result {
        log::warn!("LAN map transfer failed: {error:#}");
        let step = maps.fail(
            &mut state.frontend.network_setup_protocol,
            &state.match_state.session,
        );
        crate::app::App::apply_step(state, step);
    }
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.maps = maps;
    }
}

pub(super) fn receive(
    game_config: Option<&crate::util::config::GameConfig>,
    frontend: &mut crate::app::frontend::state::FrontendState,
    match_state: &mut crate::app::match_runtime::state::MatchState,
    source: SocketAddrV4,
    bytes: &[u8],
) -> Result<()> {
    let packet = Packet::decode(bytes)?;
    let scope = match_state
        .lan_service
        .as_ref()
        .and_then(super::LanService::setup_scope);
    if scope != Some(packet.scope) {
        return Ok(());
    }
    let mut maps = std::mem::take(
        &mut match_state
            .lan_service
            .as_mut()
            .ok_or_else(|| anyhow!("no LAN service"))?
            .maps,
    );
    let result = maps.receive(game_config, frontend, match_state, source, packet);
    // As in `received_options`: a closed visit takes its transfer state along.
    if let Some(service) = match_state.lan_service.as_mut() {
        service.maps = maps;
    }
    result
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::net::global::{GlobalDestination, GlobalMessage};
    use crate::net::reliable::{Delivery, LanConnectionConfig, LanConnections, RetryTiming};
    use crate::net::udp::{LanUdpTransport, UdpBindConfig};
    use std::net::Ipv4Addr;

    fn key(resource: TransferResource, bytes: &[u8]) -> Key {
        let mut identity = crate::util::sha256::Sha256::new();
        identity.update(bytes);
        Key {
            scope: SessionScope {
                epoch: [3; 16],
                revision: 7,
            },
            identity: identity.finalize(),
            host: false,
            effective: String::new(),
            generated: None,
            assets: vec![Asset {
                resource,
                digest: crate::util::sha256::sha256_hex(bytes),
                total: bytes.len(),
            }],
        }
    }
    fn transport() -> LanUdpTransport {
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        LanUdpTransport::bind(
            UdpBindConfig {
                primary: endpoint,
                alternate: endpoint,
            },
            LanConnections::new(
                LanConnectionConfig::default(),
                RetryTiming {
                    retry_delta: 10,
                    max_retries: 100,
                    timeout: 1000,
                },
            ),
        )
        .unwrap()
    }
    fn service_udp(sender: &mut LanUdpTransport, receiver: &mut LanUdpTransport, now: &mut u32) {
        *now = now.wrapping_add(1);
        sender.service(*now);
        receiver.service(*now);
    }
    fn receive(
        sender: &mut LanUdpTransport,
        receiver: &mut LanUdpTransport,
        now: &mut u32,
    ) -> GlobalMessage {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            service_udp(sender, receiver, now);
            if let Some(message) = receiver.pop_global_received() {
                return message;
            }
            assert!(
                Instant::now() < deadline,
                "LAN asset datagram did not arrive"
            );
        }
    }

    #[test]
    fn udp_reordered_asset_windows_install_the_exact_generated_seed() {
        let mut options = crate::map::rmg::RmgOptions::default();
        options.seed = 1234;
        options.normalize();
        let mut bytes = options.to_sed_bytes(false);
        bytes.extend_from_slice(b"\n;");
        bytes.extend(std::iter::repeat_n(b'x', WINDOW_BYTES));
        let key = key(TransferResource::Seed, &bytes);
        let mut download = Download::new(key.assets[0].clone());
        let (mut host, mut guest) = (transport(), transport());
        let mut now = 0;
        let (host_address, guest_address) =
            (host.primary_addr().unwrap(), guest.primary_addr().unwrap());
        while download.bytes.len() < bytes.len() {
            let offset = download.bytes.len();
            let request = Packet::encode(
                PacketKind::Request,
                &key,
                TransferResource::Seed,
                offset,
                &[],
            )
            .unwrap();
            assert!(guest.global_mut().send(
                &request,
                GlobalDestination::direct(host_address),
                Delivery::Reliable,
                0
            ));
            download.requested = true;
            let received = receive(&mut guest, &mut host, &mut now);
            let request = Packet::decode(&received.payload).unwrap();
            assert!(request.matches_key(&key));
            assert_eq!(request.offset, offset);
            let end = (offset + WINDOW_BYTES).min(bytes.len());
            let offsets: Vec<_> = (offset..end).step_by(CHUNK).collect();
            for &start in offsets.iter().rev() {
                let finish = (start + CHUNK).min(end);
                let chunk = Packet::encode(
                    PacketKind::Chunk,
                    &key,
                    TransferResource::Seed,
                    start,
                    &bytes[start..finish],
                )
                .unwrap();
                assert!(host.global_mut().send(
                    &chunk,
                    GlobalDestination::direct(guest_address),
                    Delivery::Reliable,
                    0
                ));
            }
            for _ in offsets {
                let received = receive(&mut host, &mut guest, &mut now);
                let chunk = Packet::decode(&received.payload).unwrap();
                assert!(chunk.matches_key(&key));
                download.receive(chunk.offset, chunk.bytes).unwrap();
            }
        }
        assert_eq!(download.bytes, bytes);
        let deadline = Instant::now() + Duration::from_secs(2);
        while host.global().queued_send_count() != 0 || guest.global().queued_send_count() != 0 {
            service_udp(&mut host, &mut guest, &mut now);
            assert!(Instant::now() < deadline, "LAN asset ACKs did not drain");
        }
        let directory = crate::map::source::test_support::TestDirectory::new("lan-udp-seed");
        let loaded = crate::map::source::install_received_asset(
            directory.path(),
            download.bytes,
            &key.assets[0].digest,
            bytes.len(),
            TransferResource::Seed,
        )
        .unwrap();
        assert_eq!(
            loaded.source.physical_identity(),
            Some((bytes.len(), key.assets[0].digest.as_str()))
        );
        let crate::map::source::TransferContent::Scenario(ini) = loaded.content else {
            panic!("received seed INI");
        };
        let mut decoded = crate::map::rmg::RmgOptions::default();
        decoded.apply_sed(&ini, false);
        decoded.normalize();
        assert_eq!(decoded, options);
    }

    #[test]
    fn asset_window_rejects_changed_duplicate_size_and_previous_options_scope() {
        let bytes = vec![5; CHUNK + 2];
        let key = key(TransferResource::Map, &bytes);
        let encoded = Packet::encode(
            PacketKind::Chunk,
            &key,
            TransferResource::Map,
            0,
            &bytes[..CHUNK],
        )
        .unwrap();
        let mut old_scope = encoded.clone();
        old_scope[21..29].copy_from_slice(&(key.scope.revision - 1).to_le_bytes());
        assert!(!Packet::decode(&old_scope).unwrap().matches_key(&key));
        let mut download = Download::new(key.assets[0].clone());
        download.requested = true;
        assert!(!download.receive(0, &bytes[..CHUNK]).unwrap());
        assert!(!download.receive(0, &bytes[..CHUNK]).unwrap());
        assert!(download.receive(0, &vec![6; CHUNK]).is_err());
        assert!(download.receive(CHUNK, &[5]).is_err());
        assert!(download.receive(WINDOW_BYTES, &[5]).is_err());
        assert!(
            download.bytes.is_empty(),
            "incomplete window published partial bytes"
        );
        assert!(download.receive(CHUNK, &[5, 5]).unwrap());
        assert_eq!(download.bytes, bytes);
        let mut malformed = encoded;
        malformed[61] = 3;
        assert!(Packet::decode(&malformed).is_err());
    }
}
