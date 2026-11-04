//! Live entity-atlas growth: the voxel and SHP sprites a match draws after
//! map load, rasterized on worker threads so the frame that first shows a
//! new model does not wait for its raster.
//!
//! [`EntityAtlasGrowth::start`] checks coverage per object model, prepares the
//! missing keys on the calling thread, where Rules, ART and the asset manager
//! stay, and hands the raster to a worker. [`EntityAtlasGrowth::publish`]
//! completes a finished raster into the atlas it was prepared against. One
//! growth per atlas runs at a time; an atlas whose growth is running is
//! checked again once it publishes.
//!
//! Until a growth lands, the sprites it carries are missing from the picture,
//! so a new model or object type appears a few frames late. This is a
//! presentation-only difference, since gamemd has no atlas and draws from its
//! files on first use; the simulation never reads these atlases.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crate::assets::asset_manager::AssetManager;
use crate::map::houses::HouseColorMap;
use crate::render::batch::BatchRenderer;
use crate::render::sprite_atlas::{
    self, PreparedSpriteAtlas, ShpAtlasUpload, ShpRasterOutput, ShpRasterWork, SpriteAtlas,
};
use crate::render::unit_atlas::{self, UnitAtlas, UnitAtlasUpload, UnitRasterOutput};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::util::time::Instant;

/// What a growth is prepared from: the world, its rules and files, and the
/// match's colours and theater.
pub(crate) struct GrowthSource<'a> {
    pub(crate) sim: &'a Simulation,
    pub(crate) rules: Option<&'a RuleSet>,
    pub(crate) asset_manager: &'a AssetManager,
    pub(crate) house_colors: &'a HouseColorMap,
    pub(crate) theater_name: &'a str,
    pub(crate) theater_ext: &'a str,
    /// The device's 2D texture limit, which bounds the SHP palette rows.
    pub(crate) max_texture_dimension_2d: u32,
}

/// The growths running for a match's entity atlases.
#[derive(Default)]
pub(crate) struct EntityAtlasGrowth {
    unit: Option<Growth<UnitRasterOutput, UnitAtlasUpload>>,
    sprite: Option<Growth<ShpRasterOutput, ShpAtlasUpload>>,
    /// Growths published so far. Each one draws sprites the previous picture
    /// lacked, so the redraw gate keys on it.
    published: u64,
}

/// One atlas's raster running on a worker, and the state it publishes with.
struct Growth<Output, Upload> {
    raster: mpsc::Receiver<Output>,
    upload: Upload,
    began: Instant,
    prepare_ms: f64,
    /// Stops a voxel raster whose growth is dropped unpublished.
    _cancel: Option<CancelOnDrop>,
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

impl EntityAtlasGrowth {
    /// Start a growth for each atlas without one running that lacks a voxel
    /// model, an object (type, house colour), an AnimClass colour remap, the
    /// harvest overlay or a FoggedObject frame the world now draws. The checks
    /// are per object model, never per sprite key, so this is cheap enough to
    /// run on every spawn, death, ownership change and Limbo.
    pub(crate) fn start(
        &mut self,
        source: &GrowthSource,
        unit_atlas: Option<&UnitAtlas>,
        sprite_atlas: Option<&SpriteAtlas>,
    ) {
        let sim = source.sim;
        if self.unit.is_none()
            && !unit_atlas::atlas_covers_world(
                unit_atlas,
                sim.entities(),
                source.rules,
                Some(sim.interner()),
            )
        {
            log::info!("Refreshing the unit atlas: new voxel models");
            let began = Instant::now();
            let (work, upload) = unit_atlas::prepare_unit_atlas(
                sim.entities(),
                source.asset_manager,
                source.rules,
                unit_atlas,
                Some(sim.interner()),
            );
            let cancel = Arc::new(AtomicBool::new(false));
            match work.spawn(cancel.clone()) {
                Ok(raster) => {
                    self.unit = Some(Growth {
                        raster,
                        upload,
                        began,
                        prepare_ms: elapsed_ms(began),
                        _cancel: Some(CancelOnDrop(cancel)),
                    });
                }
                Err(err) => log::error!("Unit atlas growth not started: {err}"),
            }
        }

        if self.sprite.is_some() {
            return;
        }
        // Map load collects the same coverage (`prepare_presentation_manifest`).
        let coverage =
            sprite_atlas::EntityAtlasCoverage::collect(sim, source.rules, source.house_colors);
        if sprite_atlas::atlas_covers_world(
            sprite_atlas,
            sim.entities(),
            source.house_colors,
            &coverage.extra_buildings,
            &coverage.anim_remap_keys,
            Some(sim.interner()),
        ) && sprite_atlas::atlas_covers_sprite_keys(
            sprite_atlas,
            &coverage.frozen_keys,
            source.rules,
        ) {
            return;
        }
        // Growth pages must key on the palettes the load-time pages used.
        let Some((palette, cell_palette)) = crate::map::theater::load_theater_unit_and_iso_palettes(
            source.asset_manager,
            source.theater_name,
        ) else {
            log::warn!("Sprite atlas refresh skipped: theater palettes not found");
            return;
        };
        log::info!("Refreshing the sprite atlas: new SHP object types or colour remaps");
        let began = Instant::now();
        let prepared = sprite_atlas::prepare_sprite_atlas(
            source.max_texture_dimension_2d,
            sim.entities(),
            source.asset_manager,
            &palette,
            source.theater_ext,
            source.theater_name,
            source.rules,
            source.rules.map(|rules| rules.art()),
            source.house_colors,
            &coverage.extra_buildings,
            &coverage.anim_remap_keys,
            &coverage.cell_drawer_type_ids,
            &coverage.frozen_keys,
            Some(&cell_palette),
            sprite_atlas,
            Some(sim.interner()),
        );
        let PreparedSpriteAtlas::Raster(work, upload) = prepared else {
            return;
        };
        self.grow_sprites(work, upload, began);
    }

    /// Hand a prepared SHP raster to a worker as the sprite atlas's growth.
    fn grow_sprites(&mut self, work: ShpRasterWork, upload: ShpAtlasUpload, began: Instant) {
        match work.spawn() {
            Ok(raster) => {
                self.sprite = Some(Growth {
                    raster,
                    upload,
                    began,
                    prepare_ms: elapsed_ms(began),
                    _cancel: None,
                });
            }
            Err(err) => log::error!("Sprite atlas growth not started: {err}"),
        }
    }

    /// Test hook: a sprite growth from a raster the test prepared.
    #[cfg(test)]
    pub(crate) fn grow_sprites_for_test(&mut self, work: ShpRasterWork, upload: ShpAtlasUpload) {
        self.grow_sprites(work, upload, Instant::now());
    }

    /// Complete the finished growths into the atlases they were prepared
    /// against; `wait` blocks for every running one. Whether any published:
    /// the caller then starts the growths the world still needs. A growth
    /// that leaves its atlas as it was, such as an abandoned SHP refresh, is
    /// not published; the sim tick's next refresh retries it, so a waiting
    /// publish never restarts it for ever.
    pub(crate) fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        unit_atlas: &mut Option<UnitAtlas>,
        sprite_atlas: &mut Option<SpriteAtlas>,
        wait: bool,
    ) -> bool {
        let published = self.published;
        if let Some((growth, output)) = finished(&mut self.unit, wait)
            && growth.publish("Unit", output, |upload, output| {
                upload.finish(device, queue, batch, output, unit_atlas)
            })
        {
            self.published += 1;
        }
        if let Some((growth, output)) = finished(&mut self.sprite, wait)
            && growth.publish("Sprite", output, |upload, output| {
                upload.finish(device, queue, batch, output, sprite_atlas)
            })
        {
            self.published += 1;
        }
        self.published != published
    }

    /// Drop the running growths unpublished. Called wherever the atlases they
    /// were prepared against are freed or replaced.
    pub(crate) fn cancel(&mut self) {
        self.unit = None;
        self.sprite = None;
    }

    pub(crate) fn published(&self) -> u64 {
        self.published
    }

    #[cfg(test)]
    pub(crate) fn running(&self) -> bool {
        self.unit.is_some() || self.sprite.is_some()
    }
}

/// Take `growth` once its raster has finished; `wait` blocks for it.
fn finished<Output, Upload>(
    growth: &mut Option<Growth<Output, Upload>>,
    wait: bool,
) -> Option<(Growth<Output, Upload>, Output)> {
    let raster = &growth.as_ref()?.raster;
    let output = if wait {
        raster.recv().map_err(|_| mpsc::TryRecvError::Disconnected)
    } else {
        raster.try_recv()
    };
    match output {
        Ok(output) => Some((growth.take()?, output)),
        Err(mpsc::TryRecvError::Empty) => None,
        // A raster that panics on its worker ends the match, as the
        // synchronous refresh did.
        Err(mpsc::TryRecvError::Disconnected) => {
            panic!("an entity atlas raster worker ended without its output")
        }
    }
}

impl<Output, Upload> Growth<Output, Upload> {
    /// Complete `output` through `finish`, which says whether it changed the
    /// atlas, and log the calling thread's share.
    fn publish(
        self,
        atlas: &str,
        output: Output,
        finish: impl FnOnce(Upload, Output) -> bool,
    ) -> bool {
        let publishing = Instant::now();
        let published = finish(self.upload, output);
        log::info!(
            "{atlas} atlas growth {} {:.1} ms after it began; main thread: \
             {:.1} ms preparing, {:.1} ms publishing",
            if published {
                "published"
            } else {
                "changed nothing"
            },
            elapsed_ms(self.began),
            self.prepare_ms,
            elapsed_ms(publishing),
        );
        published
    }
}
