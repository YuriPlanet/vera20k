//! Bounded, cancellable transfers of the selected physical map/seed and preview.
//! Existing source, staging, generation and UI owners decide what those bytes mean.
use super::{LiveService, http};
use crate::app::AppState;
use crate::app::shell_network_setup::maps::{self as shared_maps, GeneratedMapManifest};
use crate::app::shell_network_setup::{accept_network_map_source, set_network_map_progress};
use crate::map::source::{LoadedTransferAsset, TransferResource};
use anyhow::{Result, anyhow, ensure};
use std::path::PathBuf;
use std::time::{Duration, Instant};
const CHUNK: usize = 32768;

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
        let expected_size = CHUNK.min(expected_total.saturating_sub(self.offset));
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
        self.offset += bytes.len();
        self.in_flight = false;
        self.retries = 0;
        Ok(())
    }
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
}

pub(super) fn received_options(
    state: &mut AppState,
    service: &mut LiveService,
    generated: Option<GeneratedMapManifest>,
) -> Result<()> {
    shared_maps::accept_generated_manifest(
        state,
        service.maps.generated.as_ref(),
        generated.as_ref(),
    )?;
    service.maps.generated = generated;
    service.maps.active = true;
    Ok(())
}

pub(super) fn cancel(state: &mut AppState, service: &mut LiveService) {
    service.maps.active = false;
    service.maps.generation = service.maps.generation.wrapping_add(1);
    service.maps.key = None;
    service.maps.transfer = None;
    service.maps.generated = None;
    service.maps.next_resource = None;
    service.maps.failed = false;
    service.maps.complete = false;
    set_network_map_progress(state, None);
}
pub(super) fn poll(state: &mut AppState, service: &mut LiveService) {
    let mut maps = std::mem::take(&mut service.maps);
    if maps.poll(state, service).is_err() {
        maps.fail(state);
    }
    service.maps = maps;
}
impl Transfers {
    pub(super) fn current_verified(&self, state: &AppState) -> bool {
        let Some(metadata) = state.match_state.session.network_map_metadata() else {
            return false;
        };
        self.complete
            && self.key.as_ref().is_some_and(|key| {
                key.digest == metadata.source_sha256
                    && key.total == usize::try_from(metadata.byte_length).unwrap_or(0)
                    && key.effective_digest == metadata.effective_digest
                    && key.random == self.generated
                    && super::super::lobby::current_channel(state)
                        .is_some_and(|channel| channel.name == key.room)
            })
    }
    fn fail(&mut self, state: &mut AppState) {
        if !self.failed {
            super::notice(
                state,
                "Map transfer or include verification failed. Rejoin after checking the map and its required include files.",
            );
        }
        self.failed = true;
        self.complete = false;
        self.transfer = None;
        self.next_resource = None;
        set_network_map_progress(state, None);
    }
    fn poll(&mut self, state: &mut AppState, service: &mut LiveService) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let Some(launch) = state.match_state.session.network_setup() else {
            self.key = None;
            self.transfer = None;
            return Ok(());
        };
        let Some(metadata) = state.match_state.session.network_map_metadata().cloned() else {
            return Ok(());
        };
        let Some(channel) = super::super::lobby::current_channel(state) else {
            return Ok(());
        };
        let seed = launch
            .selected_map_file
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("RandMap.Sed"));
        if seed && self.generated.is_none() {
            return Ok(());
        }
        let key = Key {
            room: channel.name.clone(),
            digest: metadata.source_sha256.clone(),
            total: usize::try_from(metadata.byte_length)?,
            effective_digest: metadata.effective_digest.clone(),
            random: self.generated.clone(),
        };
        ensure!(
            crate::map::source::valid_source_sha256(&key.digest)
                && crate::map::source::valid_source_sha256(&key.effective_digest)
                && key.total > 0
                && key.total <= TransferResource::Map.limit(),
            "invalid map transfer identity"
        );
        if self.key.as_ref() != Some(&key) {
            self.generation = self.generation.wrapping_add(1);
            self.transfer = None;
            self.complete = false;
            self.failed = false;
            self.next_resource = Some(if seed {
                TransferResource::Seed
            } else {
                TransferResource::Map
            });
            self.key = Some(key);
        }
        if self.failed || self.complete {
            return Ok(());
        }
        if let Some(resource) = self.next_resource.take() {
            self.begin_resource(state, service, resource)?;
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
                let end = (transfer.offset + CHUNK).min(total);
                let id = service.next_request;
                service.next_request = service.next_request.wrapping_add(1);
                http::Request::UploadMapChunk {
                    transfer: self.generation,
                    resource: transfer.resource,
                    id,
                    digest: digest.to_owned(),
                    total,
                    offset: transfer.offset,
                    bytes: transfer.bytes[transfer.offset..end].to_vec(),
                }
            }
            Direction::Download => http::Request::MapChunk {
                transfer: self.generation,
                resource: transfer.resource,
                seed_digest: key.digest.clone(),
                digest: digest.to_owned(),
                offset: transfer.offset,
            },
        };
        service.requests.send(request)?;
        transfer.in_flight = true;
        Ok(())
    }
    fn begin_resource(
        &mut self,
        state: &mut AppState,
        service: &mut LiveService,
        resource: TransferResource,
    ) -> Result<()> {
        let key = self.key.as_ref().expect("transfer key");
        let (digest, total) = key.asset(resource);
        let config = state
            .platform
            .game_config
            .as_ref()
            .ok_or_else(|| anyhow!("no asset directory"))?;
        let directory = config.paths.settings_dir().join("NetworkMaps");
        let host = state.match_state.session.host();
        let local = shared_maps::local_transfer_source(state, resource)?;
        let assets = state
            .process_assets
            .manager()
            .ok_or_else(|| anyhow!("assets missing"))?;
        let metadata = state
            .match_state
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
            state
                .match_state
                .session
                .accept_verified_network_map_source(source, effective)?;
            if !host {
                return self.finish_resource(state, resource);
            }
        }
        let now = Instant::now();
        self.transfer = Some(Transfer {
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
            deadline: now + Duration::from_secs(300),
            retries: 0,
            directory: directory.clone(),
        });
        set_network_map_progress(state, Some((0, total as u32)));
        if let Some(source) = upload {
            service.requests.send(http::Request::ReadMapUpload {
                transfer: self.generation,
                source,
                digest: digest.to_owned(),
                total,
            })?;
        } else {
            service.requests.send(http::Request::CachedMap {
                transfer: self.generation,
                resource,
                directory,
                digest: digest.to_owned(),
                total,
            })?;
        }
        Ok(())
    }
    fn finish_resource(&mut self, state: &mut AppState, resource: TransferResource) -> Result<()> {
        self.transfer = None;
        if resource != TransferResource::Preview
            && self.key.as_ref().is_some_and(|key| key.random.is_some())
        {
            self.next_resource = Some(TransferResource::Preview);
            return Ok(());
        }
        let source = state
            .match_state
            .session
            .network_local_map_source()
            .cloned()
            .ok_or_else(|| anyhow!("verified source missing"))?;
        let effective = state
            .match_state
            .session
            .network_local_map_effective_digest()
            .ok_or_else(|| anyhow!("effective map missing"))?
            .to_owned();
        accept_network_map_source(state, source, effective)?;
        self.complete = true;
        Ok(())
    }
    fn accept_download(
        &mut self,
        state: &mut AppState,
        loaded: LoadedTransferAsset,
        resource: TransferResource,
    ) -> Result<()> {
        shared_maps::accept_transfer_asset(state, loaded, resource, self.generated.as_ref())?;
        self.finish_resource(state, resource)
    }
    fn response(
        &mut self,
        state: &mut AppState,
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
        if generation != self.generation || self.transfer.is_none() {
            return Ok(());
        }
        let Some(expected) = state.match_state.session.network_map_metadata() else {
            return Ok(());
        };
        let key = self.key.as_ref().expect("transfer key");
        if key.digest != expected.source_sha256
            || key.total != expected.byte_length as usize
            || key.effective_digest != expected.effective_digest
            || key.random != self.generated
        {
            return Ok(());
        }
        let resource = self.transfer.as_ref().expect("active").resource;
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
            | http::Response::MapInstalled { loaded, .. } => {
                self.accept_download(state, loaded, resource)?
            }
            http::Response::CachedMap { loaded: None, .. } => {
                self.transfer.as_mut().unwrap().in_flight = false
            }
            http::Response::MapUploaded { completed, .. } => {
                let transfer = self.transfer.as_mut().unwrap();
                ensure!(
                    completed == (transfer.offset + CHUNK).min(total),
                    "upload offset differs"
                );
                transfer.offset = completed;
                transfer.in_flight = false;
                transfer.retries = 0;
                set_network_map_progress(state, Some((completed as u32, total as u32)));
                if completed == total {
                    if resource == TransferResource::Preview {
                        let config = state
                            .platform
                            .game_config
                            .as_ref()
                            .ok_or_else(|| anyhow!("no asset directory"))?;
                        let source = crate::map::source::transfer_file_source(
                            &config.paths.ra2_dir.join("RandMap.img"),
                            resource,
                        )?;
                        ensure!(
                            source.physical_identity() == Some((total, digest)),
                            "preview changed during upload"
                        );
                        state
                            .match_state
                            .session
                            .accept_network_random_map_preview(&key.digest, source)?;
                    }
                    self.finish_resource(state, resource)?;
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
                set_network_map_progress(state, Some((transfer.offset as u32, total as u32)));
                if complete {
                    service.requests.send(http::Request::InstallMap {
                        transfer: self.generation,
                        resource,
                        directory: transfer.directory.clone(),
                        digest: digest.to_owned(),
                        total,
                        bytes: std::mem::take(&mut transfer.bytes),
                    })?;
                    transfer.in_flight = true;
                }
            }
            http::Response::MapPending { .. } => {
                let transfer = self.transfer.as_mut().unwrap();
                transfer.in_flight = false;
                transfer.retry_at = Instant::now() + Duration::from_secs(2);
            }
            http::Response::MapFailure { retryable, .. } => {
                let transfer = self.transfer.as_mut().unwrap();
                transfer.retries += 1;
                ensure!(retryable && transfer.retries <= 3, "map transfer failed");
                transfer.in_flight = false;
                transfer.retry_at = Instant::now() + Duration::from_secs(2);
            }
            _ => {}
        }
        Ok(())
    }
}
pub(super) fn response(state: &mut AppState, service: &mut LiveService, response: http::Response) {
    let mut maps = std::mem::take(&mut service.maps);
    if maps.response(state, service, response).is_err() {
        maps.fail(state);
    }
    service.maps = maps;
}
pub(super) fn available(
    state: &mut AppState,
    service: &mut LiveService,
    digest: &str,
    total: usize,
) {
    if service.maps.key.as_ref().is_some_and(|key| {
        key.digest == digest && key.total == total
            || key.random.as_ref().is_some_and(|r| {
                r.preview.source_sha256 == digest && r.preview.byte_length == total
            })
    }) {
        if service.maps.failed && !state.match_state.session.host() {
            service.maps.key = None;
        }
        if let Some(transfer) = service.maps.transfer.as_mut() {
            transfer.retry_at = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_rejects_duplicate_offsets_and_changed_identity_without_appending() {
        let key = Key {
            room: "room".into(),
            digest: "a".repeat(64),
            total: CHUNK + 2,
            effective_digest: "b".repeat(64),
            random: None,
        };
        let now = Instant::now();
        let mut transfer = Transfer {
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
        transfer
            .append_chunk(&key, &key.digest, key.total, 0, &vec![1; CHUNK], false)
            .unwrap();
        assert!(
            transfer
                .append_chunk(&key, &key.digest, key.total, 0, &vec![1; CHUNK], false)
                .is_err()
        );
        assert!(
            transfer
                .append_chunk(&key, &"b".repeat(64), key.total, CHUNK, &[2, 3], true)
                .is_err()
        );
        assert_eq!(transfer.offset, CHUNK);
        transfer
            .append_chunk(&key, &key.digest, key.total, CHUNK, &[2, 3], true)
            .unwrap();
        assert_eq!(&transfer.bytes[CHUNK..], &[2, 3]);
    }
}
