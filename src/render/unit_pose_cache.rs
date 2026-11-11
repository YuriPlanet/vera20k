//! Per-frame sprites of posed bodies: tilted (crashing) bodies, pitched
//! rockets, rocking Drive/Ship/Hover unit parts and tumbling voxel debris.
//!
//! `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`) and
//! `JumpjetLocomotionClass`'s `TiltCrashJumpjet=` arm (`0x0054DCC0`) key their
//! draw -1, so the native voxel cache never holds such a pose: the body is
//! rasterized afresh every frame with its current roll and pitch.
//! `RocketLocomotionClass::Draw_Matrix` (`0x00663470`) keys a pitched missile
//! -1 too, except at the block's PitchInitial or stored PitchFinal product,
//! whose cached image is the one a fresh render gives. `VoxelAnimClass::DrawIt` (`0x00749B70`) renders
//! its piece fresh every frame too. This page renders the posed bodies on
//! screen. Equal unit poses share a slot within each presentation frame
//! ([`VxlPoseFrameCache::begin_frame`]); overflow grows retained GPU pages.
//! Before the frame's draws, each image rasterized on the CPU is written to
//! its page and the GPU voxel raster draws the rest
//! ([`VxlPoseFrameCache::encode`], [`crate::render::voxel_gpu`]).
//!
//! It also keeps, on retained pages, the depth-tested image of each stable
//! key drawn while 0xB43180 is set ([`VxlPoseFrameCache::render_depth_tested`],
//! [`crate::render::voxel_image_cache`]). A key's image depends on nothing
//! else, so one render serves the match.

use std::collections::{BTreeMap, HashMap};

use crate::assets::asset_manager::AssetManager;
use crate::assets::vpl_file::VplFile;
use crate::render::atlas_growth::{SPRITE_PADDING, ShelfCursor};
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::unit_atlas::{PoseTilt, UnitModel, UnitSpriteEntry, UnitSpriteKey};
use crate::render::voxel_gpu::{GpuImage, PagePass, VoxelGpu};
use crate::render::vxl_raster::{VoxelImage, VxlLighting, VxlSprite};
use crate::rules::ruleset::RuleSet;

const PAGE_SIZE: u32 = 1024;

/// Where [`VxlPoseFrameCache::render_live`] keeps its image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePage {
    /// Nowhere: it matches a clean render, and the render only writes depth.
    None,
    /// A retained page, for a cache record.
    Retained,
    /// This frame's page, for an uncached render.
    Frame,
}

#[derive(Default)]
pub struct VxlPoseFrameCache {
    pages: Vec<PosePage>,
    /// Only frame-local derived sprites; immutable models remain cached below.
    unit_poses: HashMap<UnitSpriteKey, HashMap<(PoseTiltKey, bool), UnitSpriteEntry>>,
    /// `VOXELS.VPL`, parsed on the first pose.
    vpl: Option<Option<VplFile>>,
    /// Each posed type's voxel model, parsed on its first pose.
    models: BTreeMap<smol_str::SmolStr, Option<UnitModel>>,
    /// Image-file models shared by voxel debris and bullets. These keys are
    /// retained image-load attempts, unlike the rules type keys in `models`.
    image_models: BTreeMap<String, ImageModel>,
    /// Depth-tested stable images on retained pages, with their pool bytes.
    depth_tested: HashMap<UnitSpriteKey, Option<(UnitSpriteEntry, u32)>>,
    /// The GPU raster that draws the images it admits.
    gpu: VoxelGpu,
}

struct PosePage {
    shelf: ShelfCursor,
    /// Frame-local: used this frame. Retained: placed on since the last encode.
    dirty: bool,
    /// Holds depth-tested stable images for the whole match.
    retained: bool,
    texture: Option<BatchTexture>,
    /// The images on the page: this frame's, or a retained page's.
    images: Vec<PageImage>,
}

struct PageImage {
    corner: [u32; 2],
    size: [u32; 2],
    pixels: PagePixels,
}

enum PagePixels {
    /// Rasterized on the CPU, and written to the texture once.
    Cpu { indices: Vec<u8>, written: bool },
    /// Drawn by the GPU raster. A unit pose keeps its key and tilt, to be
    /// rasterized again on the CPU for the rare reads of its pixels
    /// ([`VxlPoseFrameCache::stamp_shadow_parts`]); nothing reads a bullet's
    /// or a debris piece's.
    Gpu {
        image: GpuImage,
        pose: Option<(UnitSpriteKey, PoseTilt)>,
    },
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
    use_buffer: bool,
}

/// `VOXELS.VPL`, parsed on the first draw that needs it.
fn load_vpl<'a>(
    vpl: &'a mut Option<Option<VplFile>>,
    asset_manager: &AssetManager,
) -> Option<&'a VplFile> {
    vpl.get_or_insert_with(|| {
        asset_manager
            .get_ref("VOXELS.VPL")
            .and_then(|data| VplFile::from_bytes(data).ok())
    })
    .as_ref()
}

fn image_model<'a>(
    image_models: &'a mut BTreeMap<String, ImageModel>,
    asset_manager: &AssetManager,
    image: &str,
) -> &'a mut ImageModel {
    image_models
        .entry(image.to_owned())
        .or_insert_with(|| ImageModel {
            model: UnitModel::load(asset_manager, image, None),
            poses: BTreeMap::new(),
        })
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
            let [width, height] = part.entry.pixel_size.map(|v| v as u32);
            let Some(pixels) = self.entry_pixels(&part.entry) else {
                return false;
            };
            crate::render::unit_atlas::composite_mask_part(
                body, &pixels, width, height, part.point,
            );
        }
        true
    }

    /// The indices `entry` names on its page, as the page's texture holds
    /// them: each image's pixels, a GPU-drawn pose's rasterized again on the
    /// CPU on a clear depth map, as the GPU drew it, and zero elsewhere.
    /// None off the page, where a raster fails, or over a GPU-drawn bullet
    /// or debris piece.
    fn entry_pixels(&self, entry: &UnitSpriteEntry) -> Option<Vec<u8>> {
        let page = self.pages.get(entry.page)?;
        let [x, y] = entry
            .uv_origin
            .map(|v| (v * PAGE_SIZE as f32).round() as u32);
        let [width, height] = entry.pixel_size.map(|v| v as u32);
        if x + width > PAGE_SIZE || y + height > PAGE_SIZE {
            return None;
        }
        let mut pixels = vec![0; (width * height) as usize];
        for image in &page.images {
            let [ix, iy] = image.corner;
            let [iw, ih] = image.size;
            let (left, right) = (x.max(ix), (x + width).min(ix + iw));
            let (top, bottom) = (y.max(iy), (y + height).min(iy + ih));
            if left >= right || top >= bottom {
                continue;
            }
            let rendered;
            let source = match &image.pixels {
                PagePixels::Cpu { indices, .. } => indices,
                PagePixels::Gpu { pose, .. } => {
                    let (key, tilt) = pose.as_ref()?;
                    let vpl = self.vpl.as_ref().and_then(Option::as_ref);
                    let (sprite, _) = self
                        .models
                        .get(&key.type_id)?
                        .as_ref()?
                        .render_pose(key, vpl, *tilt, false, None)?;
                    if [sprite.width, sprite.height] != image.size {
                        return None;
                    }
                    rendered = sprite.palette_indices;
                    &rendered
                }
            };
            let span = (right - left) as usize;
            for row in top..bottom {
                let from = ((row - iy) * iw + left - ix) as usize;
                let to = ((row - y) * width + left - x) as usize;
                pixels[to..to + span].copy_from_slice(&source[from..from + span]);
            }
        }
        Some(pixels)
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
        let pose_key = (pose_key, false);
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
        let entry = place_sprite(&mut self.pages, sprite, bounds, false)?;
        self.unit_poses
            .entry(key.clone())
            .or_default()
            .insert(pose_key, entry);
        Some(entry)
    }

    /// Invalidate all frame-local entries while retaining parsed models,
    /// depth-tested images and page allocations. No live instance survives
    /// this presentation boundary.
    pub fn begin_frame(&mut self) {
        self.unit_poses.clear();
        for image in self.image_models.values_mut() {
            image.poses.clear();
        }
        for page in self.pages.iter_mut().filter(|page| !page.retained) {
            page.images.clear();
            page.shelf = ShelfCursor::new(PAGE_SIZE, PAGE_SIZE);
            page.dirty = false;
        }
    }

    /// Rasterize a retained unit pose once per frame, depth-tested when
    /// `depth_tested` (0xB43180 at the draw). Missing models and oversized
    /// sprites are not drawable; page capacity never drops a part.
    pub fn render(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        key: &UnitSpriteKey,
        tilt: PoseTilt,
        depth_tested: bool,
    ) -> Option<UnitSpriteEntry> {
        let pose_key = (PoseTiltKey::from(tilt), depth_tested);
        if let Some(entry) = self
            .unit_poses
            .get(key)
            .and_then(|poses| poses.get(&pose_key))
        {
            return Some(*entry);
        }
        let vpl = load_vpl(&mut self.vpl, asset_manager);
        let model = self
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules))
            .as_ref()?;
        let (image, bounds) = model.pose_image(key, vpl, tilt, depth_tested)?;
        let pose = Some((key, tilt));
        let entry = place_image(&mut self.pages, &mut self.gpu, vpl, image, bounds, pose)?;
        self.unit_poses
            .entry(key.clone())
            .or_default()
            .insert(pose_key, entry);
        Some(entry)
    }

    /// The depth-tested image of a stable atlas key and its pool bytes,
    /// rendered on its first request and kept on a retained page. None when
    /// the model or key does not draw.
    pub fn render_depth_tested(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        key: &UnitSpriteKey,
    ) -> Option<(UnitSpriteEntry, u32)> {
        if let Some(image) = self.depth_tested.get(key) {
            return *image;
        }
        let vpl = load_vpl(&mut self.vpl, asset_manager);
        let image = self
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules))
            .as_ref()
            .and_then(|model| model.render_depth_tested(key, vpl, None))
            .and_then(|(sprite, bounds)| {
                let bytes = crate::render::voxel_image_cache::sprite_pool_bytes(
                    &sprite.palette_indices,
                    [sprite.width, sprite.height],
                    [sprite.offset_x, sprite.offset_y],
                    bounds,
                );
                Some((place_sprite(&mut self.pages, sprite, bounds, true)?, bytes))
            });
        self.depth_tested.insert(key.clone(), image);
        image
    }

    /// A depth-tested render of a stable key (`tilt` None) or a posed body
    /// on the shared `depth` map ([`crate::render::voxel_image_cache`]): its
    /// image on `page`, with its pool bytes. None when `page` keeps none, or
    /// the model or key does not draw. The image cache counts its pixels, so
    /// it stays on the CPU.
    pub fn render_live(
        &mut self,
        asset_manager: &AssetManager,
        rules: Option<&RuleSet>,
        key: &UnitSpriteKey,
        tilt: Option<PoseTilt>,
        depth: &mut crate::render::vxl_raster::DepthMap,
        page: LivePage,
    ) -> Option<(UnitSpriteEntry, u32)> {
        let vpl = load_vpl(&mut self.vpl, asset_manager);
        let model = self
            .models
            .entry(key.type_id.clone())
            .or_insert_with(|| UnitModel::load(asset_manager, &key.type_id, rules))
            .as_ref()?;
        let (sprite, bounds) = match tilt {
            Some(tilt) => model.render_pose(key, vpl, tilt, true, Some(depth))?,
            None => model.render_depth_tested(key, vpl, Some(depth))?,
        };
        let retained = match page {
            LivePage::None => return None,
            LivePage::Retained => true,
            LivePage::Frame => false,
        };
        let bytes = crate::render::voxel_image_cache::sprite_pool_bytes(
            &sprite.palette_indices,
            [sprite.width, sprite.height],
            [sprite.offset_x, sprite.offset_y],
            bounds,
        );
        Some((
            place_sprite(&mut self.pages, sprite, bounds, retained)?,
            bytes,
        ))
    }

    /// Rasterize one `VoxelAnimClass` piece's body and ground shadow at its
    /// bounce matrix and place both on this frame's page. The rules asset
    /// owner supplies the retained physical part (VoxelAnim74B4D0..74B5DB),
    /// including a copied source model; this draw never reloads it by name.
    /// None when the model does not resolve, the piece is not drawable or the
    /// sprite exceeds the page size. `depth` is the shared depth map the body
    /// reads and writes; None draws it on a clear one.
    pub fn render_tumble(
        &mut self,
        asset_manager: &AssetManager,
        model: &crate::rules::voxel_asset_catalog::VoxelPart,
        section: i32,
        matrix: glam::Mat4,
        use_buffer: bool,
        depth: Option<&mut crate::render::vxl_raster::DepthMap>,
    ) -> Option<(UnitSpriteEntry, Option<UnitSpriteEntry>)> {
        let vpl = load_vpl(&mut self.vpl, asset_manager);
        if let Some(depth) = depth {
            let (body, shadow) =
                UnitModel::render_tumble(model, section, matrix, vpl, use_buffer, Some(depth))?;
            let shadow =
                shadow.and_then(|sprite| place_sprite(&mut self.pages, sprite, None, false));
            return Some((place_sprite(&mut self.pages, body, None, false)?, shadow));
        }
        let (body, shadow) = UnitModel::tumble_images(model, section, matrix, vpl, use_buffer)?;
        let shadow = shadow.and_then(|shadow| {
            place_image(&mut self.pages, &mut self.gpu, vpl, shadow, None, None)
        });
        let body = place_image(&mut self.pages, &mut self.gpu, vpl, body, None, None)?;
        Some((body, shadow))
    }

    /// Rasterize an image-file body at the supplied native matrix. Bullet
    /// Draw46B0C0 requires the retained VXL/HVA pair and frame0; debris's
    /// no-HVA/shadow policy stays with `render_tumble`. Failed image attempts
    /// are cached too, and the match/asset owner replaces this entire cache.
    /// `depth` is the shared depth map the draw reads and writes, whose image
    /// no other draw shares; None draws on a clear one.
    #[allow(clippy::too_many_arguments)]
    pub fn render_matrix(
        &mut self,
        asset_manager: &AssetManager,
        image: &str,
        matrix: glam::Mat4,
        frame: u32,
        use_hva: bool,
        lighting: VxlLighting,
        use_buffer: bool,
        depth: Option<&mut crate::render::vxl_raster::DepthMap>,
    ) -> Option<UnitSpriteEntry> {
        let live = depth.is_some();
        let key = MatrixPoseKey {
            matrix: matrix.to_cols_array().map(f32::to_bits),
            frame,
            use_hva,
            lighting,
            use_buffer,
        };
        if let Some(entry) = self
            .image_models
            .get(image)
            .and_then(|model| model.poses.get(&key))
            .filter(|_| !live)
        {
            return Some(*entry);
        }
        let vpl = load_vpl(&mut self.vpl, asset_manager);
        let model = image_model(&mut self.image_models, asset_manager, image)
            .model
            .as_ref()?;
        let entry = match depth {
            Some(depth) => {
                let (sprite, bounds) = model.render_matrix(
                    matrix,
                    frame,
                    use_hva,
                    lighting,
                    vpl,
                    use_buffer,
                    Some(depth),
                )?;
                place_sprite(&mut self.pages, sprite, bounds, false)?
            }
            None => {
                let (picture, bounds) =
                    model.matrix_image(matrix, frame, use_hva, lighting, vpl, use_buffer)?;
                place_image(&mut self.pages, &mut self.gpu, vpl, picture, bounds, None)?
            }
        };
        if !live {
            self.image_models.get_mut(image)?.poses.insert(key, entry);
        }
        Some(entry)
    }

    /// Draw this frame's pages, once per submission: write each CPU image
    /// not yet on its texture, then draw the GPU images of the pages used
    /// this frame. Bindings select the page carried by each entry; page
    /// changes never reorder the retained native Display sequence.
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        for page in self.pages.iter_mut().filter(|page| page.dirty) {
            let texture = page.texture.get_or_insert_with(|| {
                batch.create_unit_page_texture(device, PAGE_SIZE, PAGE_SIZE)
            });
            for image in &mut page.images {
                if let PagePixels::Cpu { indices, written } = &mut image.pixels
                    && !*written
                {
                    *written = true;
                    write_padded(
                        queue,
                        texture.view.texture(),
                        image.corner,
                        image.size,
                        indices,
                    );
                }
            }
        }
        let passes: Vec<_> = self
            .pages
            .iter()
            .filter(|page| page.dirty && !page.retained)
            .filter_map(|page| {
                Some(PagePass {
                    target: &page.texture.as_ref()?.view,
                    images: page
                        .images
                        .iter()
                        .filter_map(|image| match &image.pixels {
                            PagePixels::Gpu { image: gpu, .. } => Some((image.corner, gpu)),
                            PagePixels::Cpu { .. } => None,
                        })
                        .collect(),
                })
            })
            .collect();
        let vpl = self.vpl.as_ref().and_then(Option::as_ref);
        self.gpu
            .encode(device, queue, encoder, [PAGE_SIZE; 2], &passes, vpl);
        for page in self.pages.iter_mut().filter(|page| page.retained) {
            page.dirty = false;
        }
    }

    pub fn page_texture(&self, page: usize) -> Option<&BatchTexture> {
        self.pages.get(page)?.texture.as_ref()
    }

    /// [`Self::encode`] alone in a submission, for a test that reads the
    /// pages.
    #[cfg(test)]
    pub(crate) fn submit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        self.encode(device, queue, batch, &mut encoder);
        queue.submit([encoder.finish()]);
    }

    /// The pixels `entry` names on its page.
    #[cfg(test)]
    pub(crate) fn texels(&self, entry: UnitSpriteEntry) -> Vec<u8> {
        self.entry_pixels(&entry).expect("an entry on a page")
    }

    #[cfg(test)]
    pub(crate) fn from_test_textures(textures: Vec<BatchTexture>) -> Self {
        Self {
            pages: textures
                .into_iter()
                .map(|texture| PosePage {
                    texture: Some(texture),
                    ..PosePage::new(false)
                })
                .collect(),
            ..Self::default()
        }
    }

    /// Exercise production overflow allocation in GPU replay fixtures.
    #[cfg(test)]
    pub(crate) fn fill_test_page(&mut self) -> UnitSpriteEntry {
        place_sprite(
            &mut self.pages,
            VxlSprite {
                palette_indices: vec![7; (PAGE_SIZE * PAGE_SIZE) as usize],
                depth: vec![0.; (PAGE_SIZE * PAGE_SIZE) as usize],
                width: PAGE_SIZE,
                height: PAGE_SIZE,
                offset_x: 0.,
                offset_y: 0.,
            },
            None,
            false,
        )
        .unwrap()
    }
}

/// Write a CPU image and the padding after it, zeroed, so no earlier image
/// shows through either.
fn write_padded(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    [x, y]: [u32; 2],
    [width, height]: [u32; 2],
    indices: &[u8],
) {
    let padded = [
        (width + SPRITE_PADDING).min(PAGE_SIZE - x),
        (height + SPRITE_PADDING).min(PAGE_SIZE - y),
    ];
    let mut texels = vec![0; (padded[0] * padded[1]) as usize];
    for (row, pixels) in indices.chunks_exact(width.max(1) as usize).enumerate() {
        let at = row * padded[0] as usize;
        texels[at..at + width as usize].copy_from_slice(pixels);
    }
    crate::render::atlas_growth::write_texels(queue, texture, [x, y], padded, 1, &texels);
}

/// Put one voxel image on this frame's pages: for the GPU raster where it
/// admits the image, else rasterized here. `pose` is a unit pose's key and
/// tilt.
fn place_image(
    pages: &mut Vec<PosePage>,
    gpu: &mut VoxelGpu,
    vpl: Option<&VplFile>,
    image: VoxelImage<'_>,
    bounds: Option<[i32; 4]>,
    pose: Option<(&UnitSpriteKey, PoseTilt)>,
) -> Option<UnitSpriteEntry> {
    if let Some(admitted) = gpu.admit(&image, vpl) {
        let (size, offset) = image.frame();
        let pixels = PagePixels::Gpu {
            image: admitted,
            pose: pose.map(|(key, tilt)| (key.clone(), tilt)),
        };
        return place_on(pages, pixels, size, offset, bounds, false);
    }
    place_sprite(pages, image.render_cpu(vpl)?, bounds, false)
}

/// Put one rasterized sprite on a frame-local or retained page.
fn place_sprite(
    pages: &mut Vec<PosePage>,
    sprite: VxlSprite,
    native_draw_bounds: Option<[i32; 4]>,
    retained: bool,
) -> Option<UnitSpriteEntry> {
    let size = [sprite.width, sprite.height];
    let offset = [sprite.offset_x, sprite.offset_y];
    let pixels = PagePixels::Cpu {
        indices: sprite.palette_indices,
        written: false,
    };
    place_on(pages, pixels, size, offset, native_draw_bounds, retained)
}

/// Put one image on a frame-local or retained page.
fn place_on(
    pages: &mut Vec<PosePage>,
    pixels: PagePixels,
    [width, height]: [u32; 2],
    [offset_x, offset_y]: [f32; 2],
    native_draw_bounds: Option<[i32; 4]>,
    retained: bool,
) -> Option<UnitSpriteEntry> {
    if width > PAGE_SIZE || height > PAGE_SIZE {
        return None;
    }
    // Retain existing page identity: only the last used page of its kind
    // can grow. Earlier entries keep their UVs and page through all later
    // requests; every retained page holds images, the last one grows.
    let kind: Vec<usize> = (0..pages.len())
        .filter(|&index| pages[index].retained == retained)
        .collect();
    let mut slot = kind
        .iter()
        .rposition(|&index| retained || pages[index].dirty)
        .unwrap_or(0);
    let (page_index, [px, py]) = loop {
        let index = match kind.get(slot) {
            Some(&index) => index,
            None => {
                pages.push(PosePage::new(retained));
                pages.len() - 1
            }
        };
        if let Some(position) = pages[index].shelf.place(width, height) {
            break (index, position);
        }
        slot += 1;
    };
    let page = &mut pages[page_index];
    page.images.push(PageImage {
        corner: [px, py],
        size: [width, height],
        pixels,
    });
    page.dirty = true;
    Some(UnitSpriteEntry {
        uv_origin: [px as f32 / PAGE_SIZE as f32, py as f32 / PAGE_SIZE as f32],
        uv_size: [
            width as f32 / PAGE_SIZE as f32,
            height as f32 / PAGE_SIZE as f32,
        ],
        pixel_size: [width as f32, height as f32],
        offset_x,
        offset_y,
        native_draw_bounds,
        page: page_index,
    })
}

impl PosePage {
    fn new(retained: bool) -> Self {
        Self {
            shelf: ShelfCursor::new(PAGE_SIZE, PAGE_SIZE),
            dirty: false,
            retained,
            texture: None,
            images: Vec::new(),
        }
    }

    /// Rows holding this frame's images: every closed shelf and the open one.
    #[cfg(test)]
    fn used_rows(&self) -> u32 {
        self.shelf.used_height().min(PAGE_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::atlas_growth::SPRITE_PADDING;

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
                .render(&assets, Some(&battle.rules), &key, ground_pose(0.25), false)
                .unwrap();
            let page = &cache.pages[first.page];
            let allocation = page.shelf;
            let images = page.images.len();
            let repeated = cache
                .render(&assets, Some(&battle.rules), &key, ground_pose(0.25), false)
                .unwrap();
            assert_same_entry(repeated, first);
            assert_eq!(
                cache.pages[first.page].shelf, allocation,
                "equal visible MTNK parts must not allocate a second rectangle"
            );
            assert_eq!(
                cache.pages[first.page].images.len(),
                images,
                "a repeated request draws no second image"
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
                false,
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
                false,
            )
            .expect("GTNK turret must remain drawable after a full overflow page");
        let barrel = cache
            .render(
                &assets,
                Some(&battle.rules),
                &ground_key(VxlLayer::Barrel),
                ground_pose(0.25),
                false,
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
                false,
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
                false,
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
                .all(|page| page.used_rows() == 0 && page.images.is_empty())
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
                false,
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
                false,
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
            .render_matrix(
                &assets,
                "ZBOMB",
                down,
                0,
                true,
                VxlLighting::Diffuse,
                false,
                None,
            )
            .unwrap();
        assert!(
            first.page > 0,
            "direct image poses share the overflow owner"
        );
        let cursor = cache.pages[first.page].shelf;
        let repeated = cache
            .render_matrix(
                &assets,
                "ZBOMB",
                down,
                0,
                true,
                VxlLighting::Diffuse,
                false,
                None,
            )
            .unwrap();
        assert_same_entry(repeated, first);
        assert_eq!(
            cache.pages[first.page].shelf, cursor,
            "equal simultaneous bombs must not consume extra page space"
        );
        let changed = cache
            .render_matrix(
                &assets,
                "ZBOMB",
                east,
                0,
                true,
                VxlLighting::Diffuse,
                false,
                None,
            )
            .unwrap();
        assert_ne!(first.uv_origin, changed.uv_origin);
        assert_eq!(cache.image_models["ZBOMB"].poses.len(), 2);
        for missing in ["VERA_MISSING_BULLET_IMAGE", ""] {
            assert!(
                cache
                    .render_matrix(
                        &assets,
                        missing,
                        down,
                        0,
                        true,
                        VxlLighting::Diffuse,
                        false,
                        None
                    )
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
        assert!(cache.pages.iter().all(|page| page.images.is_empty()));
        assert_eq!(cache.pages.iter().map(PosePage::used_rows).sum::<u32>(), 0);
        let next = cache
            .render_matrix(
                &assets,
                "ZBOMB",
                east,
                0,
                true,
                VxlLighting::Diffuse,
                false,
                None,
            )
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

    /// Sprites pack left to right on shelves, padded apart, and the next
    /// frame starts the page afresh.
    #[test]
    fn placed_sprites_pack_on_shelves_until_the_next_frame() {
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
        let first = place_sprite(&mut cache.pages, sprite(600, 40), None, false).unwrap();
        let second = place_sprite(&mut cache.pages, sprite(600, 30), None, false).unwrap();
        let row = |entry: UnitSpriteEntry| (entry.uv_origin[1] * PAGE_SIZE as f32) as u32;
        assert_eq!((row(first), row(second)), (0, 40 + SPRITE_PADDING));
        assert_eq!(cache.pages[0].used_rows(), 40 + SPRITE_PADDING + 30);
        assert_eq!(cache.texels(second), vec![7; 600 * 30]);
        cache.begin_frame();
        assert!(cache.pages.iter().all(|page| page.images.is_empty()));
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
                false,
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
                false,
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
                false,
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
                false,
            )
            .expect("stock SPYP VXL accepts a pitched normal-flight matrix");
        let pitched_crash = cache
            .render(
                &assets,
                Some(&battle.rules),
                &key,
                PoseTilt::Fly([roll, pitch]),
                false,
            )
            .expect("stock SPYP VXL accepts the existing pitched crash matrix");
        assert_ne!(
            pose_texels(&cache, pitched_normal),
            pose_texels(&cache, pitched_crash),
            "normal yaw→pitch→roll and crash yaw→roll→pitch keep their native matrix order"
        );
    }

    fn pose_texels(cache: &VxlPoseFrameCache, entry: UnitSpriteEntry) -> Vec<u8> {
        cache.texels(entry)
    }
}
