//! Display-only loading jobs. Native Rules, the simulation and the process
//! asset lease remain with the loading attempt; workers own only its derived
//! physical-source projections. No frame is installed before both atlases finish.

use super::{PreparedManifest, PresentationManifest};
use crate::render::{batch::BatchRenderer, gpu::GpuContext, sprite_atlas, unit_atlas};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::task::Poll;

pub(crate) struct PresentationAtlasJob {
    unit: Option<(
        mpsc::Receiver<unit_atlas::UnitRasterOutput>,
        unit_atlas::UnitAtlasUpload,
    )>,
    sprite: Option<sprite_atlas::SpriteAtlasLoadingJob>,
    cancel: Arc<AtomicBool>,
}

impl PreparedManifest {
    /// Saved-game preparation retains its synchronous transaction. The same
    /// derived inputs and atlas owners also serve the yielding fresh-load path.
    pub(crate) fn finish(self, gpu: &GpuContext, batch: &BatchRenderer) -> PresentationManifest {
        PresentationManifest {
            unit_atlas: self.unit_work.and_then(|(work, upload)| {
                upload.finish(&gpu.device, &gpu.queue, batch, work.render())
            }),
            sprite_atlas: self
                .sprite_work
                .and_then(|work| work.finish(&gpu.device, &gpu.queue, batch)),
            palette_set: self.palette_set,
        }
    }

    pub(crate) fn start_loading(
        self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
    ) -> anyhow::Result<(PresentationManifest, PresentationAtlasJob)> {
        let cancel = Arc::new(AtomicBool::new(false));
        let unit = if let Some((work, upload)) = self.unit_work {
            let (sender, receiver) = mpsc::channel();
            let cancelled = cancel.clone();
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            {
                let _ = sender.send(work.render_cancellable(&cancelled));
            }
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            std::thread::Builder::new()
                .name("voxel-loading-raster".into())
                .spawn(move || {
                    let output = work.render_cancellable(&cancelled);
                    let _ = sender.send(output);
                })?;
            Some((receiver, upload))
        } else {
            None
        };
        let sprite = match self
            .sprite_work
            .map(|work| work.start_loading(&gpu.device, &gpu.queue, batch))
            .transpose()
        {
            Ok(sprite) => sprite,
            Err(error) => {
                cancel.store(true, Ordering::Relaxed);
                return Err(error);
            }
        };
        Ok((
            PresentationManifest {
                unit_atlas: None,
                sprite_atlas: None,
                palette_set: self.palette_set,
            },
            PresentationAtlasJob {
                unit,
                sprite,
                cancel,
            },
        ))
    }
}

impl PresentationAtlasJob {
    pub(crate) fn poll(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        presentation: &mut crate::app::loading::init::PresentationLoadAssets,
    ) -> anyhow::Result<bool> {
        if let Some((receiver, _)) = &self.unit {
            match receiver.try_recv() {
                Ok(output) => {
                    let (_, upload) = self.unit.take().expect("one voxel completion owner");
                    presentation.unit_atlas = upload.finish(&gpu.device, &gpu.queue, batch, output);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("voxel loading worker disconnected")
                }
            }
        }
        if let Some(sprite) = &mut self.sprite {
            if let Poll::Ready(atlas) = sprite.poll(&gpu.device, &gpu.queue)? {
                presentation.sprite_atlas = atlas;
                self.sprite = None;
            }
        }
        Ok(self.unit.is_none() && self.sprite.is_none())
    }
}

impl Drop for PresentationAtlasJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
