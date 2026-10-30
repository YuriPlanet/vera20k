//! Loading-only band decoding and nonblocking GPU retirement. Existing live
//! growth keeps the synchronous entry; both consume the parent atlas owners.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::task::Poll;

enum RasterMessage {
    Band {
        page: usize,
        origin: [u32; 2],
        size: [u32; 2],
        bytes: Vec<u8>,
    },
    Finished,
}

struct InFlightBand {
    started: Instant,
    complete: Arc<AtomicBool>,
}

/// Private candidate: it is never visible to a scene until every band retires.
struct LoadingUpload {
    completion: Option<ShpAtlasUpload>,
    atlas: Option<SpriteAtlas>,
    indices: Vec<wgpu::Texture>,
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    messages: mpsc::Receiver<RasterMessage>,
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    retired: mpsc::Sender<()>,
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    messages: Box<dyn Iterator<Item = RasterMessage>>,
    in_flight: Option<InFlightBand>,
    added: usize,
    texture_limit: u32,
}

pub(crate) struct SpriteAtlasLoadingJob {
    ready: Option<Option<SpriteAtlas>>,
    upload: Option<LoadingUpload>,
}

impl PreparedSpriteAtlas {
    pub(crate) fn start_loading(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
    ) -> anyhow::Result<SpriteAtlasLoadingJob> {
        let mut completion = match self {
            Self::Ready(atlas) => return Ok(SpriteAtlasLoadingJob::ready(atlas)),
            Self::Upload(upload) => upload,
        };
        anyhow::ensure!(
            completion.previous_atlas.is_none(),
            "loading must not replace a live atlas"
        );
        let rendered = std::mem::take(&mut completion.rendered);
        let added = rendered.len();
        if rendered.is_empty() {
            log::warn!("No SHP sprites rendered");
            return Ok(SpriteAtlasLoadingJob::ready(None));
        }
        let texture_limit = device.limits().max_texture_dimension_2d;
        let mut plan = plan_shp_pages(rendered, texture_limit);
        let palettes = std::mem::take(&mut completion.palettes);
        let (atlas, indices) = create_packed_shp_atlas(device, queue, batch, &mut plan, palettes);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let (send, messages) = mpsc::sync_channel(1);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let (retired, wait_retired) = mpsc::channel();
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        std::thread::Builder::new()
            .name("shp-loading-bands".into())
            .spawn(move || {
                for (page, placement) in plan.pages.into_iter().enumerate() {
                    let bands = ShpUploadBands::new(
                        plan.sprites.clone(),
                        placement.placed,
                        placement.size[0],
                        PACK_BAND_BYTES,
                    );
                    for (origin, size, bytes) in bands {
                        if send
                            .send(RasterMessage::Band {
                                page,
                                origin,
                                size,
                                bytes,
                            })
                            .is_err()
                        {
                            return;
                        }
                        // Keep the existing one-band memory bound. The next
                        // band is decoded only after this staging copy retires.
                        // Dropping a cancelled job closes both channels; it
                        // never joins a worker or lends it process/GPU owners.
                        if wait_retired.recv().is_err() {
                            return;
                        }
                    }
                }
                let _ = send.send(RasterMessage::Finished);
            })?;
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let messages: Box<dyn Iterator<Item = RasterMessage>> = {
            let sprites = plan.sprites;
            Box::new(
                plan.pages
                    .into_iter()
                    .enumerate()
                    .flat_map(move |(page, placement)| {
                        ShpUploadBands::new(
                            sprites.clone(),
                            placement.placed,
                            placement.size[0],
                            PACK_BAND_BYTES,
                        )
                        .map(move |(origin, size, bytes)| {
                            RasterMessage::Band {
                                page,
                                origin,
                                size,
                                bytes,
                            }
                        })
                    })
                    .chain(std::iter::once(RasterMessage::Finished)),
            )
        };
        Ok(SpriteAtlasLoadingJob {
            ready: None,
            upload: Some(LoadingUpload {
                completion: Some(completion),
                atlas: Some(atlas),
                indices,
                messages,
                #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
                retired,
                in_flight: None,
                added,
                texture_limit,
            }),
        })
    }
}

impl SpriteAtlasLoadingJob {
    fn ready(atlas: Option<SpriteAtlas>) -> Self {
        Self {
            ready: Some(atlas),
            upload: None,
        }
    }

    /// At most one small queue write per event-loop pass, with no GPU wait.
    /// wgpu's completion callback is dispatched by Device::poll(Poll);
    /// unlike PollType::Wait this returns control to the window immediately.
    pub(crate) fn poll(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<Poll<Option<SpriteAtlas>>> {
        if let Some(atlas) = self.ready.take() {
            return Ok(Poll::Ready(atlas));
        }
        let upload = self
            .upload
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("SHP loading job already consumed"))?;
        device.poll(wgpu::PollType::Poll)?;
        if let Some(band) = &upload.in_flight {
            if !band.complete.load(Ordering::Acquire) {
                anyhow::ensure!(
                    band.started.elapsed() < PACK_BAND_TIMEOUT,
                    "SHP loading upload timed out"
                );
                return Ok(Poll::Pending);
            }
            upload.in_flight = None;
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            upload.retired.send(())?;
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let message = upload.messages.try_recv();
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let message = upload
            .messages
            .next()
            .ok_or(mpsc::TryRecvError::Disconnected);
        match message {
            Ok(RasterMessage::Band {
                page,
                origin,
                size,
                bytes,
            }) => {
                atlas_growth::write_texels(queue, &upload.indices[page], origin, size, 1, &bytes);
                queue.submit(std::iter::empty());
                let complete = Arc::new(AtomicBool::new(false));
                let callback = complete.clone();
                queue.on_submitted_work_done(move || callback.store(true, Ordering::Release));
                upload.in_flight = Some(InFlightBand {
                    started: Instant::now(),
                    complete,
                });
                Ok(Poll::Pending)
            }
            Ok(RasterMessage::Finished) => {
                let atlas = upload.atlas.take().expect("unpublished loading candidate");
                log_shp_atlas_pack(&atlas, upload.texture_limit);
                let atlas = upload
                    .completion
                    .take()
                    .expect("one completion owner")
                    .publish(atlas, upload.added, true);
                self.upload = None;
                Ok(Poll::Ready(Some(atlas)))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(Poll::Pending),
            Err(mpsc::TryRecvError::Disconnected) => {
                anyhow::bail!("SHP loading worker disconnected")
            }
        }
    }
}
