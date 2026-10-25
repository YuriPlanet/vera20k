//! Per-frame sprites of posed bodies: tilted (crashing) bodies, pitched
//! rockets, rocking Drive/Ship/Hover unit parts and tumbling voxel debris.
//!
//! `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`) and
//! `JumpjetLocomotionClass`'s `TiltCrashJumpjet=` arm (`0x0054DCC0`) key their
//! draw -1, so the native voxel cache never holds such a pose: the body is
//! rasterized afresh every frame with its current roll and pitch.
//! `RocketLocomotionClass::Draw_Matrix` (`0x00663470`) keys a pitched missile
//! -1 too, except at the stored PitchFinal product, whose cached image is the
//! one a fresh render gives. `VoxelAnimClass::DrawIt` (`0x00749B70`) renders
//! its piece fresh every frame too. This page renders the posed bodies on
//! screen. Equal unit poses share a slot within each presentation frame
//! ([`VxlPoseFrameCache::begin_frame`]); overflow grows retained GPU pages.
//! Each page’s used rows are uploaded after the voxel instances are built
//! ([`VxlPoseFrameCache::upload`]).

use std::collections::{BTreeMap, HashMap};

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
    pages: Vec<PosePage>,
    /// Only frame-local derived sprites; immutable models remain cached below.
    unit_poses: HashMap<UnitSpriteKey, HashMap<PoseTiltKey, UnitSpriteEntry>>,
    /// `VOXELS.VPL`, parsed on the first pose.
    vpl: Option<Option<VplFile>>,
    /// Each posed type's voxel model, parsed on its first pose.
    models: BTreeMap<String, Option<UnitModel>>,
    /// Image-file models shared by voxel debris and bullets. These keys are
    /// retained image-load attempts, unlike the rules type keys in `models`.
    image_models: BTreeMap<String, ImageModel>,
}

#[derive(Default)]
struct PosePage {
    pixels: Vec<u8>,
    cursor_x: u32,
    cursor_y: u32,
    shelf_height: u32,
    dirty: bool,
    texture: Option<BatchTexture>,
}

/// Bit keys preserve every retained input, including signed zero and the
/// locomotor arm. Palette/house state is applied after rasterization.
#[derive(PartialEq, Eq, Hash)]
enum PoseTiltKey {
    Fly([u32; 2]),
    FlyNormal {
        pitch: u32,
        roll: u32,
    },
    Jumpjet([u32; 2]),
    Rocket(u32),
    Ground {
        angles: [u32; 2],
        body_facing: u8,
        slope_blend: Option<crate::map::voxel_pose::VxlSlopeBlend>,
        turret_offset: i32,
    },
    /// Sources are immutable atlas bodies or append-only pose-page regions.
    /// begin_frame clears this identity before either source can be reused.
    Shadow {
        body_offset: [i32; 2],
        parts: Vec<ShadowPartKey>,
    },
}

type ShadowPartKey = (bool, usize, [u32; 2], [u32; 2], [i32; 2]);

impl From<PoseTilt> for PoseTiltKey {
    fn from(tilt: PoseTilt) -> Self {
        match tilt {
            PoseTilt::Fly(angles) => Self::Fly(angles.map(f32::to_bits)),
            PoseTilt::FlyNormal { pitch, roll } => Self::FlyNormal {
                pitch: pitch.to_bits(),
                roll: roll.to_bits(),
            },
            PoseTilt::Jumpjet(angles) => Self::Jumpjet(angles.map(f32::to_bits)),
            PoseTilt::Rocket(pitch) => Self::Rocket(pitch.to_bits()),
            PoseTilt::Ground(pose) => Self::Ground {
                angles: pose.angles.map(f32::to_bits),
                body_facing: pose.body_facing,
                slope_blend: pose.slope_blend,
                turret_offset: pose.turret_offset,
            },
        }
    }
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
    /// Read only the visible body's frame-local pixels through their owner.
    /// Stable atlas pieces use the same native composite copy owner.
    pub(crate) fn stamp_shadow_parts(
        &self,
        atlas: &crate::render::unit_atlas::UnitAtlas,
        parts: &[crate::render::unit_atlas::UnitShadowPart],
        body: &mut [u8],
    ) -> bool {
        if parts.is_empty() {
            return false;
        }
        for part in parts {
            if !part.posed {
                if !atlas.stamp_shadow_part(body, &part.entry, part.point) {
                    return false;
                }
                continue;
            }
            let Some(page) = self.pages.get(part.entry.page) else {
                return false;
            };
            let [x, y] = part
                .entry
                .uv_origin
                .map(|v| (v * PAGE_SIZE as f32).round() as u32);
            let [width, height] = part.entry.pixel_size.map(|v| v as u32);
            if x + width > PAGE_SIZE || y + height > PAGE_SIZE {
                return false;
            }
            let pixels: Vec<_> = (y..y + height)
                .flat_map(|row| {
                    let start = (row * PAGE_SIZE + x) as usize;
                    page.pixels[start..start + width as usize].iter().copied()
                })
                .collect();
            crate::render::unit_atlas::composite_mask_part(
                body, &pixels, width, height, part.point,
            );
        }
        true
    }

    /// Unit73C472's uncached low-flight shadow uses this frame's body mask.
    /// Storage and upload follow the same owner as the visible posed body.
    pub(crate) fn render_unit_shadow(
        &mut self,
        atlas: &crate::render::unit_atlas::UnitAtlas,
        key: &UnitSpriteKey,
        body_offset: [i32; 2],
        parts: &[crate::render::unit_atlas::UnitShadowPart],
    ) -> Option<UnitSpriteEntry> {
        let pose_key = PoseTiltKey::Shadow {
            body_offset,
            parts: parts
                .iter()
                .map(|p| {
                    (
                        p.posed,
                        p.entry.page,
                        p.entry.uv_origin.map(f32::to_bits),
                        p.entry.pixel_size.map(f32::to_bits),
                        p.point,
                    )
                })
                .collect(),
        };
        if let Some(entry) = self
            .unit_poses
            .get(key)
            .and_then(|poses| poses.get(&pose_key))
        {
            return Some(*entry);
        }
        let sprite = atlas.masked_shadow(key, body_offset, |body| {
            self.stamp_shadow_parts(atlas, parts, body)
        })?;
        let bounds = Some([
            sprite.offset_x as i32,
            sprite.offset_y as i32,
            sprite.width as i32,
            sprite.height as i32,
        ]);
        let entry = self.place(&sprite, bounds)?;
        self.unit_poses
            .entry(key.clone())
            .or_default()
            .insert(pose_key, entry);
        Some(entry)
    }

    /// Invalidate all frame-local entries while retaining parsed models and
    /// page allocations. No live instance survives this presentation boundary.
    pub fn begin_frame(&mut self) {
        self.unit_poses.clear();
        for image in self.image_models.values_mut() {
            image.poses.clear();
        }
        for page in &mut self.pages {
            page.begin_frame();
        }
    }

    /// Rasterize a retained unit pose once per frame. Missing models and
    /// oversized sprites are not drawable; page capacity never drops a part.
    pub fn render(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        key: &UnitSpriteKey,
        tilt: PoseTilt,
    ) -> Option<UnitSpriteEntry> {
        let pose_key = PoseTiltKey::from(tilt);
        if let Some(entry) = self
            .unit_poses
            .get(key)
            .and_then(|poses| poses.get(&pose_key))
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
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules))
            .as_ref()?;
        let (sprite, native_draw_bounds) = model.render_pose(key, vpl, tilt)?;
        let entry = self.place(&sprite, native_draw_bounds)?;
        self.unit_poses
            .entry(key.clone())
            .or_default()
            .insert(pose_key, entry);
        Some(entry)
    }

    /// Rasterize one `VoxelAnimClass` piece's body and ground shadow at its
    /// bounce matrix and place both on this frame's page. `image` is the
    /// type's `ImageFile`, which names its model (`%s.VXL`, `0x0074B56C`).
    /// None when the model does not resolve, the piece is not drawable or the
    /// sprite exceeds the page size.
    pub fn render_tumble(
        &mut self,
        asset_manager: &AssetManager,
        image: &str,
        section: i32,
        matrix: glam::Mat4,
    ) -> Option<(UnitSpriteEntry, Option<UnitSpriteEntry>)> {
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
        if sprite.width > PAGE_SIZE || sprite.height > PAGE_SIZE {
            return None;
        }
        // Retain existing page identity: only the last used page can grow.
        // Earlier entries keep their UVs and page through all later requests.
        let mut page_index = self.pages.iter().rposition(|page| page.dirty).unwrap_or(0);
        if self.pages.is_empty() {
            self.pages.push(PosePage::new());
        }
        let (px, py) = loop {
            if let Some(position) = self.pages[page_index].try_place(sprite) {
                break position;
            }
            page_index += 1;
            if page_index == self.pages.len() {
                self.pages.push(PosePage::new());
            }
        };
        let page = &mut self.pages[page_index];
        page.blit(sprite, px, py);
        page.dirty = true;
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
            page: page_index,
        })
    }

    /// Upload every used page. Bindings select the page carried by each entry;
    /// page changes never reorder the retained native Display sequence.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, batch: &BatchRenderer) {
        for page in self.pages.iter_mut().filter(|page| page.dirty) {
            let rows = page.used_rows();
            let texture = page.texture.get_or_insert_with(|| {
                batch.create_blank_unit_atlas_texture(device, PAGE_SIZE, PAGE_SIZE)
            });
            crate::render::atlas_growth::write_texels(
                queue,
                texture.view.texture(),
                [0, 0],
                [PAGE_SIZE, rows],
                1,
                &page.pixels[..(rows * PAGE_SIZE) as usize],
            );
        }
    }

    pub fn page_texture(&self, page: usize) -> Option<&BatchTexture> {
        self.pages.get(page)?.texture.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn from_test_textures(textures: Vec<BatchTexture>) -> Self {
        Self {
            pages: textures
                .into_iter()
                .map(|texture| PosePage {
                    texture: Some(texture),
                    ..PosePage::new()
                })
                .collect(),
            ..Self::default()
        }
    }

    /// Exercise production overflow allocation in GPU replay fixtures.
    #[cfg(test)]
    pub(crate) fn fill_test_page(&mut self) -> UnitSpriteEntry {
        self.place(
            &VxlSprite {
                palette_indices: vec![7; (PAGE_SIZE * PAGE_SIZE) as usize],
                depth: vec![0.; (PAGE_SIZE * PAGE_SIZE) as usize],
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                offset_x: 0.,
                offset_y: 0.,
            },
            None,
        )
        .unwrap()
    }
}

impl PosePage {
    fn new() -> Self {
        Self {
            pixels: vec![0; (PAGE_SIZE * PAGE_SIZE) as usize],
            ..Self::default()
        }
    }

    fn begin_frame(&mut self) {
        let used = (self.used_rows() * PAGE_SIZE) as usize;
        self.pixels[..used].fill(0);
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_height = 0;
        self.dirty = false;
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

    fn ground_key(layer: crate::rules::voxel_frame_catalog::VxlLayer) -> UnitSpriteKey {
        UnitSpriteKey {
            type_id: "MTNK".into(),
            turret_index: 0,
            facing: 64,
            layer,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        }
    }

    fn ground_pose(roll: f32) -> PoseTilt {
        PoseTilt::Ground(crate::render::vxl_raster::GroundPose {
            angles: [roll, 0.],
            body_facing: 64,
            slope_blend: None,
            turret_offset: 0,
        })
    }

    fn assert_same_entry(actual: UnitSpriteEntry, expected: UnitSpriteEntry) {
        assert_eq!(actual.page, expected.page);
        assert_eq!(actual.uv_origin, expected.uv_origin);
        assert_eq!(actual.uv_size, expected.uv_size);
        assert_eq!(actual.pixel_size, expected.pixel_size);
        assert_eq!(
            [actual.offset_x, actual.offset_y],
            [expected.offset_x, expected.offset_y]
        );
        assert_eq!(actual.native_draw_bounds, expected.native_draw_bounds);
    }

    #[test]
    fn repeated_real_ground_parts_reuse_one_frame_allocation() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        for layer in [
            crate::rules::voxel_frame_catalog::VxlLayer::Body,
            crate::rules::voxel_frame_catalog::VxlLayer::Turret,
            crate::rules::voxel_frame_catalog::VxlLayer::Barrel,
        ] {
            let key = ground_key(layer);
            let first = cache
                .render(&assets, Some(&battle.rules), &key, ground_pose(0.25))
                .unwrap();
            let page = &cache.pages[first.page];
            let allocation = (page.cursor_x, page.cursor_y, page.shelf_height);
            let pixels = cache.pages[first.page].pixels.clone();
            let repeated = cache
                .render(&assets, Some(&battle.rules), &key, ground_pose(0.25))
                .unwrap();
            assert_same_entry(repeated, first);
            assert_eq!(
                (
                    cache.pages[first.page].cursor_x,
                    cache.pages[first.page].cursor_y,
                    cache.pages[first.page].shelf_height
                ),
                allocation,
                "equal visible MTNK parts must not allocate a second rectangle"
            );
            assert_eq!(
                cache.pages[first.page].pixels, pixels,
                "a repeated request leaves resident texels unchanged"
            );
        }
    }

    #[test]
    fn real_ground_parts_keep_page_identity_through_overflow() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        use crate::rules::voxel_frame_catalog::VxlLayer;
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        let first = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Body),
                ground_pose(0.25),
            )
            .unwrap();
        let filled = cache.fill_test_page();
        assert_ne!(
            filled.page, first.page,
            "earlier body retains its original page"
        );
        let turret = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Turret),
                ground_pose(0.25),
            )
            .expect("GTNK turret must remain drawable after a full overflow page");
        let barrel = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Barrel),
                ground_pose(0.25),
            )
            .expect("GTNK barrel must remain drawable after a full overflow page");
        assert_ne!(turret.page, filled.page);
        assert_ne!(barrel.page, filled.page);
        let different = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Body),
                ground_pose(-0.25),
            )
            .expect("a distinct ground pose remains drawable after earlier requests");
        assert_ne!(
            (different.page, different.uv_origin),
            (first.page, first.uv_origin)
        );
        let repeated = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Body),
                ground_pose(0.25),
            )
            .unwrap();
        assert_same_entry(repeated, first);
        let page_count = cache.pages.len();
        let model = cache.models["MTNK"].as_ref().unwrap() as *const UnitModel;
        cache.begin_frame();
        assert!(cache.unit_poses.is_empty());
        assert_eq!(
            cache.pages.len(),
            page_count,
            "reuse high-water page allocations"
        );
        assert!(
            cache
                .pages
                .iter()
                .all(|page| page.used_rows() == 0 && page.pixels.iter().all(|&pixel| pixel == 0))
        );
        assert_eq!(
            cache.models["MTNK"].as_ref().unwrap() as *const UnitModel,
            model
        );
        let next = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Body),
                ground_pose(-0.25),
            )
            .unwrap();
        assert_eq!(next.page, 0);
        assert_eq!(next.uv_origin, [0.; 2]);
        let after = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Body),
                ground_pose(0.25),
            )
            .unwrap();
        assert_ne!(
            after.uv_origin, first.uv_origin,
            "a prior frame's positive pose cannot return stale UVs over the new negative pose"
        );
    }

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
        assert_eq!(cache.fill_test_page().page, 0);
        let first = cache
            .render_matrix(&assets, "ZBOMB", down, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert!(
            first.page > 0,
            "direct image poses share the overflow owner"
        );
        let cursor = (
            cache.pages[first.page].cursor_x,
            cache.pages[first.page].cursor_y,
            cache.pages[first.page].shelf_height,
        );
        let repeated = cache
            .render_matrix(&assets, "ZBOMB", down, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert_same_entry(repeated, first);
        assert_eq!(
            (
                cache.pages[first.page].cursor_x,
                cache.pages[first.page].cursor_y,
                cache.pages[first.page].shelf_height
            ),
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
        assert!(
            cache
                .pages
                .iter()
                .all(|page| page.pixels.iter().all(|&pixel| pixel == 0))
        );
        assert_eq!(cache.pages.iter().map(PosePage::used_rows).sum::<u32>(), 0);
        let next = cache
            .render_matrix(&assets, "ZBOMB", east, 0, true, VxlLighting::Diffuse)
            .unwrap();
        assert_eq!(next.page, 0, "the next frame reuses the first page");
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
        let mut page = PosePage::new();
        let first = page.try_place(&a).unwrap();
        page.blit(&a, first.0, first.1);
        let second = page.try_place(&b).unwrap();
        page.blit(&b, second.0, second.1);
        page.dirty = true;
        cache.pages.push(page);
        assert_eq!((first, second), ((0, 0), (0, 40 + SPRITE_PADDING)));
        assert_eq!(cache.pages[0].used_rows(), 40 + SPRITE_PADDING + 30);
        cache.begin_frame();
        assert!(
            cache
                .pages
                .iter()
                .all(|page| page.pixels.iter().all(|&pixel| pixel == 0))
        );
        assert_eq!(cache.pages.iter().map(PosePage::used_rows).sum::<u32>(), 0);
    }

    #[test]
    fn retail_spy_plane_normal_bank_rasterizes_both_signs_and_keeps_crash_identity() {
        use crate::render::unit_atlas::VxlLayer;

        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let key = UnitSpriteKey {
            type_id: "SPYP".into(),
            turret_index: 0,
            facing: 64,
            layer: VxlLayer::Body,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        };
        let roll = battle
            .rules
            .object("SPYP")
            .expect("stock Spy Plane type")
            .fly_roll_angle_radians() as f32;
        let mut cache = VxlPoseFrameCache::default();
        cache.begin_frame();
        let positive = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::FlyNormal { pitch: 0.0, roll },
            )
            .expect("stock SPYP VXL accepts a normal positive bank");
        let positive_texels = pose_texels(&cache, positive);
        let negative = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::FlyNormal {
                    pitch: 0.0,
                    roll: -roll,
                },
            )
            .expect("stock SPYP VXL accepts a normal negative bank");
        let negative_texels = pose_texels(&cache, negative);
        assert_ne!(positive.uv_origin, negative.uv_origin);
        assert_ne!(
            positive_texels, negative_texels,
            "bank sign changes VXL pixels"
        );

        // With pitch zero, the crash arm's X(roll)-then-Y(0) matrix equals
        // normal Y(0)-then-X(roll), but they remain distinct pose identities.
        let crash = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::Fly([roll, 0.0]),
            )
            .expect("stock SPYP VXL accepts the existing crash pose");
        assert_ne!(positive.uv_origin, crash.uv_origin);
        assert_eq!(positive_texels, pose_texels(&cache, crash));

        let pitch = 20.0_f32.to_radians();
        let pitched_normal = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::FlyNormal { pitch, roll },
            )
            .expect("stock SPYP VXL accepts a pitched normal-flight matrix");
        let pitched_crash = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::Fly([roll, pitch]),
            )
            .expect("stock SPYP VXL accepts the existing pitched crash matrix");
        assert_ne!(
            pose_texels(&cache, pitched_normal),
            pose_texels(&cache, pitched_crash),
            "normal yaw→pitch→roll and crash yaw→roll→pitch keep their native matrix order"
        );
    }

    fn pose_texels(cache: &VxlPoseFrameCache, entry: UnitSpriteEntry) -> Vec<u8> {
        let page = &cache.pages[entry.page];
        let x = (entry.uv_origin[0] * PAGE_SIZE as f32).round() as u32;
        let y = (entry.uv_origin[1] * PAGE_SIZE as f32).round() as u32;
        let width = entry.pixel_size[0] as u32;
        let height = entry.pixel_size[1] as u32;
        (0..height)
            .flat_map(|row| {
                let start = ((y + row) * PAGE_SIZE + x) as usize;
                page.pixels[start..start + width as usize].iter().copied()
            })
            .collect()
    }
}
