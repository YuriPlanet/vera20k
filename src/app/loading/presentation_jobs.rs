//! Display-only loading jobs. Native Rules, the simulation and the process
//! asset lease remain with the loading attempt; workers own only its derived
//! physical-source projections. No frame is installed before the sprite atlas
//! finishes; the unit atlas draws its pictures on first use.

use super::{PreparedManifest, PresentationManifest};
use crate::render::{batch::BatchRenderer, gpu::GpuContext, sprite_atlas};
use std::task::Poll;

pub(crate) struct PresentationAtlasJob {
    sprite: Option<sprite_atlas::SpriteAtlasLoadingJob>,
}

impl PreparedManifest {
    /// Saved-game preparation retains its synchronous transaction. The same
    /// derived inputs and atlas owners also serve the yielding fresh-load path.
    pub(crate) fn finish(self, gpu: &GpuContext, batch: &BatchRenderer) -> PresentationManifest {
        PresentationManifest {
            unit_atlas: self.unit_atlas,
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
        let sprite = self
            .sprite_work
            .map(|work| work.start_loading(&gpu.device, &gpu.queue, batch))
            .transpose()?;
        Ok((
            PresentationManifest {
                unit_atlas: self.unit_atlas,
                sprite_atlas: None,
                palette_set: self.palette_set,
            },
            PresentationAtlasJob { sprite },
        ))
    }
}

impl PresentationAtlasJob {
    pub(crate) fn poll(
        &mut self,
        gpu: &GpuContext,
        presentation: &mut crate::app::loading::init::PresentationLoadAssets,
    ) -> anyhow::Result<bool> {
        if let Some(sprite) = &mut self.sprite
            && let Poll::Ready(atlas) = sprite.poll(&gpu.device, &gpu.queue)?
        {
            presentation.sprite_atlas = atlas;
            self.sprite = None;
        }
        Ok(self.sprite.is_none())
    }
}
