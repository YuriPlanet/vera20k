//! Per-frame sprites of posed bodies: tilted (crashing) bodies, pitched
//! rockets and tumbling voxel debris.
//!
//! `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`) and
//! `JumpjetLocomotionClass`'s `TiltCrashJumpjet=` arm (`0x0054DCC0`) key their
//! draw -1, so the native voxel cache never holds such a pose: the body is
//! rasterized afresh every frame with its current roll and pitch.
//! `RocketLocomotionClass::Draw_Matrix` (`0x00663470`) keys a pitched missile
//! -1 too, except at the stored PitchFinal product, whose cached image is the
//! one a fresh render gives. `VoxelAnimClass::DrawIt` (`0x00749B70`) renders
//! its piece fresh every frame too. This page renders the posed bodies on
//! screen. It is rebuilt each presentation frame
//! ([`VxlPoseFrameCache::begin_frame`]) and its used rows are written into one
//! reused GPU page after the voxel instances are built
//! ([`VxlPoseFrameCache::upload`]).

use std::collections::BTreeMap;

use crate::assets::asset_manager::AssetManager;
use crate::assets::vpl_file::VplFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::unit_atlas::{PoseTilt, UnitModel, UnitSpriteEntry, UnitSpriteKey};
use crate::render::vxl_raster::{VxlLighting, VxlSprite};
use crate::rules::ruleset::RuleSet;

const PAGE_SIZE: u32 = 1024;
const SPRITE_PADDING: u32 = 1;

#[derive(Default)]
pub struct VxlPoseFrameCache {
    pixels: Vec<u8>,
    cursor_x: u32,
    cursor_y: u32,
    shelf_height: u32,
    dirty: bool,
    /// The GPU page, created on the first pose and rewritten in place.
    texture: Option<BatchTexture>,
    /// `VOXELS.VPL`, parsed on the first pose.
    vpl: Option<Option<VplFile>>,
    /// Each posed type's voxel model, parsed on its first pose.
    models: BTreeMap<String, Option<UnitModel>>,
    /// Image-file models shared by voxel debris and bullets. These keys are
    /// retained image-load attempts, unlike the rules type keys in `models`.
    image_models: BTreeMap<String, ImageModel>,
}

struct ImageModel {
    model: Option<UnitModel>,
    /// Equal direct matrices share a page slot only within this frame.
    poses: BTreeMap<MatrixPoseKey, UnitSpriteEntry>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct MatrixPoseKey {
    matrix: [u32; 16],
    frame: u32,
    use_hva: bool,
    lighting: VxlLighting,
}

impl VxlPoseFrameCache {
    /// Forget last frame's poses. The GPU page keeps its old texels until the
    /// next upload; no instance references them.
    pub fn begin_frame(&mut self) {
        for image in self.image_models.values_mut() {
            image.poses.clear();
        }
        if self.pixels.is_empty() {
            self.pixels = vec![0; (PAGE_SIZE * PAGE_SIZE) as usize];
        } else if self.dirty {
            let used = (self.used_rows() * PAGE_SIZE) as usize;
            self.pixels[..used].fill(0);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_height = 0;
        self.dirty = false;
    }

    /// Rasterize one body at its arm's pose and place it on this frame's page.
    /// None when the model does not resolve or the page is full.
    pub fn render(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        key: &UnitSpriteKey,
        tilt: PoseTilt,
    ) -> Option<UnitSpriteEntry> {
        if self.pixels.is_empty() {
            self.begin_frame();
        }
        let vpl = self
            .vpl
            .get_or_insert_with(|| {
                asset_manager
                    .get_ref("VOXELS.VPL")
                    .and_then(|data| VplFile::from_bytes(data).ok())
            })
            .as_ref();
        let model = self
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules))
            .as_ref()?;
        let (sprite, native_draw_bounds) = model.render_pose(key, vpl, tilt);
        self.place(&sprite, native_draw_bounds)
    }

    /// Rasterize one `VoxelAnimClass` piece's body and ground shadow at its
    /// bounce matrix and place both on this frame's page. `image` is the
    /// type's `ImageFile`, which names its model (`%s.VXL`, `0x0074B56C`).
    /// None when the model does not resolve, the piece is not drawable or the
    /// page is full.
    pub fn render_tumble(
        &mut self,
        asset_manager: &AssetManager,
        image: &str,
        section: i32,
        matrix: glam::Mat4,
    ) -> Option<(UnitSpriteEntry, Option<UnitSpriteEntry>)> {
        if self.pixels.is_empty() {
            self.begin_frame();
        }
        let vpl = self
            .vpl
            .get_or_insert_with(|| {
                asset_manager
                    .get_ref("VOXELS.VPL")
                    .and_then(|data| VplFile::from_bytes(data).ok())
            })
            .as_ref();
        let model = self
            .image_models
            .entry(image.to_string())
            .or_insert_with(|| ImageModel {
                model: UnitModel::load(asset_manager, image, None),
                poses: BTreeMap::new(),
            })
            .model
            .as_ref()?;
        let (body, shadow) = model.render_tumble(section, matrix, vpl)?;
        let shadow = shadow.and_then(|sprite| self.place(&sprite, None));
        Some((self.place(&body, None)?, shadow))
    }

    /// Rasterize an image-file body at the supplied native matrix. Bullet
    /// Draw46B0C0 requires the retained VXL/HVA pair and frame0; debris's
    /// no-HVA/shadow policy stays with `render_tumble`. Failed image attempts
    /// are cached too, and the match/asset owner replaces this entire cache.
    pub fn render_matrix(
        &mut self,
        asset_manager: &AssetManager,
        image: &str,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: VxlLighting,
    ) -> Option<UnitSpriteEntry> {
        if self.pixels.is_empty() {
            self.begin_frame();
        }
        let key = MatrixPoseKey {
            matrix: matrix.to_cols_array().map(f32::to_bits),
            frame,
            use_hva,
            lighting,
        };
        if let Some(entry) = self
            .image_models
            .get(image)
            .and_then(|model| model.poses.get(&key))
        {
            return Some(*entry);
        }
        let vpl = self
            .vpl
            .get_or_insert_with(|| {
                asset_manager
                    .get_ref("VOXELS.VPL")
                    .and_then(|data| VplFile::from_bytes(data).ok())
            })
            .as_ref();
        let model = self
            .image_models
            .entry(image.to_owned())
            .or_insert_with(|| ImageModel {
                model: UnitModel::load(asset_manager, image, None),
                poses: BTreeMap::new(),
            });
        let (sprite, bounds) = model
            .model
            .as_ref()?
            .render_matrix(matrix, frame, use_hva, lighting, vpl)?;
        let entry = self.place(&sprite, bounds)?;
        self.image_models.get_mut(image)?.poses.insert(key, entry);
        Some(entry)
    }

    /// Put one rasterized sprite on the page.
    fn place(
        &mut self,
        sprite: &VxlSprite,
        native_draw_bounds: Option<[i32; 4]>,
    ) -> Option<UnitSpriteEntry> {
        let (px, py) = self.try_place(sprite)?;
        self.blit(sprite, px, py);
        self.dirty = true;
        Some(UnitSpriteEntry {
            uv_origin: [px as f32 / PAGE_SIZE as f32, py as f32 / PAGE_SIZE as f32],
            uv_size: [
                sprite.width as f32 / PAGE_SIZE as f32,
                sprite.height as f32 / PAGE_SIZE as f32,
            ],
            pixel_size: [sprite.width as f32, sprite.height as f32],
            offset_x: sprite.offset_x,
            offset_y: sprite.offset_y,
            native_draw_bounds,
            page: 0,
        })
    }

    /// Write this frame's used rows into the GPU page when anything was
    /// placed on it.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, batch: &BatchRenderer) {
        if !self.dirty {
            return;
        }
        let rows = self.used_rows();
        let texture = self.texture.get_or_insert_with(|| {
            batch.create_blank_unit_atlas_texture(device, PAGE_SIZE, PAGE_SIZE)
        });
        crate::render::atlas_growth::write_texels(
            queue,
            texture.view.texture(),
            [0, 0],
            [PAGE_SIZE, rows],
            1,
            &self.pixels[..(rows * PAGE_SIZE) as usize],
        );
    }

    pub fn texture(&self) -> Option<&BatchTexture> {
        self.texture.as_ref()
    }

    /// Rows holding this frame's sprites: every closed shelf and the open one.
    fn used_rows(&self) -> u32 {
        (self.cursor_y + self.shelf_height).min(PAGE_SIZE)
    }

    fn try_place(&mut self, sprite: &VxlSprite) -> Option<(u32, u32)> {
        let (w, h) = (sprite.width, sprite.height);
        if w > PAGE_SIZE || h > PAGE_SIZE {
            return None;
        }
        if self.cursor_x + w > PAGE_SIZE {
            self.cursor_y = self
                .cursor_y
                .saturating_add(self.shelf_height + SPRITE_PADDING);
            self.cursor_x = 0;
            self.shelf_height = 0;
        }
        if self.cursor_y + h > PAGE_SIZE {
            return None;
        }
        let position = (self.cursor_x, self.cursor_y);
        self.cursor_x = self.cursor_x.saturating_add(w + SPRITE_PADDING);
        self.shelf_height = self.shelf_height.max(h);
        Some(position)
    }

    fn blit(&mut self, sprite: &VxlSprite, px: u32, py: u32) {
        for y in 0..sprite.height {
            let src = (y * sprite.width) as usize;
            let dst = ((py + y) * PAGE_SIZE + px) as usize;
            self.pixels[dst..dst + sprite.width as usize]
                .copy_from_slice(&sprite.palette_indices[src..src + sprite.width as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kirov_image_cache_reuses_poses_resets_frames_and_keeps_failed_attempts() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let down = crate::render::vxl_raster::projectile_voxel_matrix(
            crate::sim::projectile::ProjectileVelocity::new(0, 0, -1),
        )
        .unwrap();
        let east = crate::render::vxl_raster::projectile_voxel_matrix(
            crate::sim::projectile::ProjectileVelocity::new(1, 0, 0),
        )
        .unwrap();
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        let first = cache
            .render_matrix(&assets, "ZBOMB", down, 0, true, VxlLighting::Diffuse)
            .unwrap();
        let cursor = (cache.cursor_x, cache.cursor_y, cache.shelf_height);
        let repeated = cache
            .render_matrix(&assets, "ZBOMB", down, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert_eq!(first.uv_origin, repeated.uv_origin);
        assert_eq!(
            (cache.cursor_x, cache.cursor_y, cache.shelf_height),
            cursor,
            "equal simultaneous bombs must not consume extra page space"
        );
        let changed = cache
            .render_matrix(&assets, "ZBOMB", east, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert_ne!(first.uv_origin, changed.uv_origin);
        assert_eq!(cache.image_models["ZBOMB"].poses.len(), 2);
        for missing in ["VERA_MISSING_BULLET_IMAGE", ""] {
            assert!(
                cache
                    .render_matrix(&assets, missing, down, 0, true, VxlLighting::Diffuse)
                    .is_none()
            );
            assert!(cache.image_models[missing].model.is_none());
        }
        let parsed_model = cache.image_models["ZBOMB"].model.as_ref().unwrap() as *const UnitModel;
        cache.begin_frame();
        assert!(
            cache
                .image_models
                .values()
                .all(|model| model.poses.is_empty())
        );
        assert_eq!(
            cache.image_models["ZBOMB"].model.as_ref().unwrap() as *const UnitModel,
            parsed_model,
            "a frame reset retains the parsed immutable asset"
        );
        assert!(cache.pixels.iter().all(|&pixel| pixel == 0));
        assert_eq!(cache.used_rows(), 0);
        let next = cache
            .render_matrix(&assets, "ZBOMB", east, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert_eq!(
            next.uv_origin, first.uv_origin,
            "new frame starts at the first page slot, not a prior pose's slot"
        );
        assert!(
            cache.image_models["VERA_MISSING_BULLET_IMAGE"]
                .model
                .is_none()
        );
        assert!(
            cache.image_models[""].model.is_none(),
            "an empty attempted Image differs from no attempt"
        );
        let replacement = VxlPoseFrameCache::default();
        assert!(
            replacement.image_models.is_empty(),
            "match/asset replacement drops model and failure caches"
        );
    }

    #[test]
    fn a_new_frame_starts_an_empty_page() {
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        assert_eq!(cache.pixels.len(), (PAGE_SIZE * PAGE_SIZE) as usize);
        assert!(!cache.dirty && cache.texture().is_none());
    }

    /// Sprites pack left to right on shelves; the upload covers exactly the
    /// rows they use, and the next frame clears those rows.
    #[test]
    fn placed_sprites_bound_the_uploaded_rows() {
        let sprite = |width: u32, height: u32| VxlSprite {
            palette_indices: vec![7; (width * height) as usize],
            depth: vec![0.0; (width * height) as usize],
            width,
            height,
            offset_x: 0.0,
            offset_y: 0.0,
        };
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        let a = sprite(600, 40);
        let b = sprite(600, 30);
        let first = cache.try_place(&a).unwrap();
        cache.blit(&a, first.0, first.1);
        let second = cache.try_place(&b).unwrap();
        cache.blit(&b, second.0, second.1);
        cache.dirty = true;
        assert_eq!((first, second), ((0, 0), (0, 40 + SPRITE_PADDING)));
        assert_eq!(cache.used_rows(), 40 + SPRITE_PADDING + 30);
        cache.begin_frame();
        assert!(cache.pixels.iter().all(|&pixel| pixel == 0));
        assert_eq!(cache.used_rows(), 0);
    }
}
