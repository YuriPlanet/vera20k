//! Match resources for selection/status rendering and the drag rectangle.
//!
//! Display-parent health replay shares the centered SHP resources here;
//! placement, conditional status and UI/debug consumers retain their assets.
//! The presentation layer chooses their native draw order and material.
//!
//! ## Dependency rules
//! - Part of render/ — reads from sim/ (Selected, Position) but NEVER mutates sim state.

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::sim::production::{BuildingPlacementCell, BuildingPlacementPreview};
use crate::sim::selection::SelectionState;

/// Drag rectangle color — white.
const DRAG_RECT_COLOR: [u8; 4] = [255, 255, 255, 255];
/// Depth for the drag rectangle — always in front of everything.
const DRAG_RECT_DEPTH: f32 = 0.001;

/// Thickness of the drag rectangle outline in pixels (1px = RA2 original style).
const DRAG_RECT_THICKNESS: f32 = 1.0;

/// Manages textures and rendering for selection overlays.
pub struct SelectionOverlay {
    /// Solid color texture for drag rectangle lines.
    drag_texture: BatchTexture,
    /// Full physical PLACE frame table in its original palette. Active cells
    /// select frame0 or slope+2 through the common centered SHP owner.
    placement_shape: Option<CenteredShapeStrip>,
    /// Solid white texture reused for order markers/cursor feedback.
    white_texture: BatchTexture,
    /// Diamond outline texture for debug cell grid visualization.
    diamond_outline_texture: BatchTexture,
    /// White filled-diamond texture for debug pathgrid/path overlays.
    diamond_filled_texture: BatchTexture,
    /// Full PIPBRD frame table for selected mobile health backgrounds.
    pipbrd: Option<CenteredShapeStrip>,
    /// PIPS2.SHP: all raw frames, shared by cargo and MindControl scales.
    /// Native LoadPipAssets5F76B0 binds AC1480; DrawPipScalePips709AEF
    /// selects this shape for nonbuildings, in PALETTE.PAL/Convert87F6C4.
    scale_pips: Option<CenteredShapeStrip>,
    /// BOMBCURS.SHP, the Crazy Ivan bomb clock.
    bomb_clock: Option<CenteredShapeStrip>,
    /// LoadPipAssets5F77A6..77DD, DrawExtras6F5EC8 (PALETTE.PAL).
    talk_bubble: Option<CenteredShapeStrip>,
    /// WRENCH.SHP, the repairing building's wrench.
    repair_wrench: Option<CenteredShapeStrip>,
    /// Full PIPS: health, rank, raw garrison Pip values and self-heal.
    /// Native LoadPipAssets 5F76B0 binds AC147C; both use Convert 87F6C4.
    pips: Option<CenteredShapeStrip>,
    /// One optional custom SHP per `[DigitalDisplayTypes]` catalog entry, in
    /// catalog order. `None` for an entry without `Shape=` or a missing file;
    /// Phobos then draws that display with the game font.
    #[cfg(feature = "phobos")]
    digital_shapes: Vec<Option<CenteredShapeStrip>>,
    /// Phobos custom insignia SHPs keyed by their normalized file name.
    #[cfg(feature = "phobos")]
    insignia_shapes: std::collections::HashMap<String, CenteredShapeStrip>,
}

/// Every frame of one SHP packed side by side at its stored size, each with
/// the offset `CC_Draw_Shape @ 0x004AED70` gives a centered draw (flag
/// `0x200`): back by the integer canvas half, then on by the stored frame
/// origin.
pub struct CenteredShapeStrip {
    texture: BatchTexture,
    atlas_w: f32,
    atlas_h: f32,
    /// `SHPStruct::Width` — the authored canvas, used by Phobos's digital
    /// display `Shape.PercentageFrame` and its align math.
    shape_w: u32,
    /// `SHPStruct::Height`.
    shape_h: u32,
    frames: Vec<CenteredShapeFrame>,
}

struct CenteredShapeFrame {
    x: u32,
    w: u32,
    h: u32,
    /// Centered draw: stored origin minus the integer canvas half.
    offset: (i32, i32),
    /// `BlitterFlags::None` draw: the stored frame origin is the offset.
    raw_offset: (i32, i32),
}

impl CenteredShapeStrip {
    pub fn texture(&self) -> &BatchTexture {
        &self.texture
    }

    /// Number of frames the source SHP carries.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// `frame` drawn centered on `point`; `None` for a frame the file lacks
    /// or one without pixels.
    pub fn instance(&self, frame: usize, point: (f32, f32), depth: f32) -> Option<SpriteInstance> {
        let f = self.frames.get(frame).filter(|f| f.w > 0 && f.h > 0)?;
        Some(SpriteInstance {
            position: [point.0 + f.offset.0 as f32, point.1 + f.offset.1 as f32],
            size: [f.w as f32, f.h as f32],
            uv_origin: [f.x as f32 / self.atlas_w, 0.0],
            uv_size: [f.w as f32 / self.atlas_w, f.h as f32 / self.atlas_h],
            depth,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        })
    }

    /// `frame` drawn without `BlitterFlags::Centered`, the way
    /// `ShapeTextPrinter::PrintShape` places each digital-display glyph:
    /// the stored frame origin is added to the point, with no canvas-half
    /// subtraction.
    pub fn raw_instance(
        &self,
        frame: usize,
        point: (f32, f32),
        depth: f32,
    ) -> Option<SpriteInstance> {
        let f = self.frames.get(frame).filter(|f| f.w > 0 && f.h > 0)?;
        Some(SpriteInstance {
            position: [
                point.0 + f.raw_offset.0 as f32,
                point.1 + f.raw_offset.1 as f32,
            ],
            size: [f.w as f32, f.h as f32],
            uv_origin: [f.x as f32 / self.atlas_w, 0.0],
            uv_size: [f.w as f32 / self.atlas_w, f.h as f32 / self.atlas_h],
            depth,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        })
    }

    /// `SHPStruct::Width`.
    pub fn width(&self) -> u32 {
        self.shape_w
    }

    /// `SHPStruct::Height`.
    pub fn height(&self) -> u32 {
        self.shape_h
    }
    /// Cell47EC90 active ordinary draw -> CC_Draw_Shape4AED70. Its native
    /// flags20600 pick the plain53-row Convert's RLE row4960A0: opaque stores
    /// without A/Z interaction. Source: placement_render native execution.
    pub(crate) fn building_placement_instance(
        &self,
        cell: BuildingPlacementCell,
        alpha: f32,
    ) -> Option<SpriteInstance> {
        let frame = if cell.valid {
            0
        } else {
            usize::from(cell.slope) + 2
        };
        // Caller supplies the unraised tile point;47EC90 adds30 to X and
        // subtracts15*signed_level+1 from Y before the centered shape draw.
        // iso_to_screen includes that signed level and VERA's shared row bias.
        let (x, y) = terrain::iso_to_screen(cell.coord.0, cell.coord.1, cell.level as u8);
        let mut instance = self.instance(frame, (x + 30.0, y - 1.0), 0.0)?;
        instance.palette_light = crate::render::palette_light::PaletteLight::plain(53, 1000);
        instance.alpha = alpha;
        Some(instance)
    }

    /// Decode every SHP frame in its named palette with
    /// `ShpFile::frame_to_rgba`, keeping its indices. This is the same
    /// resource path for windowed loading and offscreen GPU checks.
    pub(crate) fn from_assets_on_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        shp_name: &str,
        pal_name: &str,
    ) -> Option<Self> {
        let shp = ShpFile::from_bytes(&assets.get(shp_name)?).ok()?;
        let palette = Palette::from_bytes(&assets.get(pal_name)?).ok()?;
        let strip = Self::from_shp_on_device(device, queue, batch, &shp, &palette)?;
        log::info!(
            "{shp_name} strip in {pal_name}: {}x{}, {} frames",
            strip.atlas_w,
            strip.atlas_h,
            strip.frames.len()
        );
        Some(strip)
    }

    /// [`Self::from_assets_on_device`] for a parsed SHP and an already
    /// built palette, such as a House's ColorScheme Convert.
    pub(crate) fn from_shp_on_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        shp: &ShpFile,
        palette: &Palette,
    ) -> Option<Self> {
        let atlas_w: u32 = shp
            .frames
            .iter()
            .map(|f| u32::from(f.frame_width))
            .sum::<u32>()
            .max(1);
        let atlas_h: u32 = shp
            .frames
            .iter()
            .map(|f| u32::from(f.frame_height))
            .max()
            .unwrap_or(0)
            .max(1);
        let mut rgba = vec![0u8; (atlas_w * atlas_h * 4) as usize];
        // Preserve the original indices for the shared Convert palette
        // resolver. Raw-color icon consumers keep their default PaletteLight;
        // placement supplies its native plain Convert instead.
        let mut indices = vec![0u8; (atlas_w * atlas_h) as usize];
        let mut frames = Vec::with_capacity(shp.frames.len());
        let mut x = 0;
        for (frame_index, frame) in shp.frames.iter().enumerate() {
            let (w, h) = (u32::from(frame.frame_width), u32::from(frame.frame_height));
            let frame_rgba = shp.frame_to_rgba(frame_index, palette).ok()?;
            let row = w as usize;
            for py in 0..h {
                let src = (py * w) as usize;
                let dst = (py * atlas_w + x) as usize;
                indices[dst..dst + row].copy_from_slice(&frame.pixels[src..src + row]);
                rgba[dst * 4..(dst + row) * 4]
                    .copy_from_slice(&frame_rgba[src * 4..(src + row) * 4]);
            }
            frames.push(CenteredShapeFrame {
                x,
                w,
                h,
                offset: (
                    i32::from(frame.frame_x) - i32::from(shp.width / 2),
                    i32::from(frame.frame_y) - i32::from(shp.height / 2),
                ),
                raw_offset: (i32::from(frame.frame_x), i32::from(frame.frame_y)),
            });
            x += w;
        }
        Some(Self {
            texture: batch.create_texture_on_device(
                device,
                queue,
                &rgba,
                atlas_w,
                atlas_h,
                Some(&indices),
            ),
            atlas_w: atlas_w as f32,
            atlas_h: atlas_h as f32,
            shape_w: u32::from(shp.width),
            shape_h: u32::from(shp.height),
            frames,
        })
    }
    /// Active foundation cells in Tactical6D5030's rectangular draw order.
    /// Admission and current Cell ground fields are recomputed with the preview;
    /// this consumer never decides buildability or uses the render height cache.
    pub fn build_building_preview(
        &self,
        preview: &BuildingPlacementPreview,
        alpha: f32,
    ) -> Vec<SpriteInstance> {
        let mut instances = Vec::with_capacity(preview.cells.len());
        //6D5030 / Ares100155C0 walks actual-list bounds X outer/Y inner;
        //The ordinary preview contains its bit2 active cells, possibly sparse
        //or outside declared X/Y. Pending-bit4 drawing is a separate residual.
        //Preserve draw order independently of stock row-major vs Custom
        //X-first gameplay list order; never regroup by admitted color.
        let mut active: Vec<_> = preview.cells.iter().collect();
        active.sort_by_key(|cell| (cell.coord.0 as i16, cell.coord.1 as i16));
        for cell in active {
            if let Some(instance) = self.building_placement_instance(*cell, alpha) {
                instances.push(instance);
            }
        }
        // Preserve existing wall auto-fill admission and order; pending marks
        // and blocked-wall suppression remain explicit separate residuals.
        instances.extend(
            preview
                .wall_autofill_cells
                .iter()
                .filter_map(|cell| self.building_placement_instance(*cell, alpha)),
        );
        instances
    }
}

impl SelectionOverlay {
    /// Create a new SelectionOverlay with procedurally-generated textures.
    /// If `assets` is provided, loads pips.shp for authentic RA2 health bar rendering.
    pub fn new(gpu: &GpuContext, batch: &BatchRenderer, assets: Option<&AssetManager>) -> Self {
        Self::new_on_device(&gpu.device, &gpu.queue, batch, assets)
    }

    /// The same resource construction for windowed rendering and GPU checks.
    pub(crate) fn new_on_device(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: Option<&AssetManager>,
    ) -> Self {
        let drag_texture: BatchTexture = batch.create_texture_on_device(
            device,
            queue,
            &generate_solid_color(DRAG_RECT_COLOR),
            2,
            2,
            None,
        );
        // Display4A885B..4A8873 binds PLACE; InitGame52BF26..52BFC1
        // binds PALETTE.PAL's plain Convert87F6C4. Missing retail art has no
        // invented procedural substitute.
        let placement_shape = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "place.shp",
                "palette.pal",
            )
        });
        let white_texture: BatchTexture = batch.create_texture_on_device(
            device,
            queue,
            &generate_solid_color([255, 255, 255, 255]),
            2,
            2,
            None,
        );
        let diamond_outline_texture: BatchTexture = batch.create_texture_on_device(
            device,
            queue,
            &generate_diamond_outline(),
            TILE_WIDTH as u32,
            TILE_HEIGHT as u32,
            None,
        );
        let diamond_filled_texture: BatchTexture = batch.create_texture_on_device(
            device,
            queue,
            &generate_diamond_cell([255, 255, 255, 255]),
            TILE_WIDTH as u32,
            TILE_HEIGHT as u32,
            None,
        );

        // LoadPipAssets 5F76B0 binds PIPBRD at AC1478. DrawHealthBar 6F68AE/
        // 6F6947 selects raw frame 1/0; 4AED70 owns the stored centering.
        let pipbrd = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "pipbrd.shp",
                "palette.pal",
            )
        });

        // Native Init_Game52BF26/52BFC1 builds Convert87F6C4 from the
        // literal PALETTE.PAL; keep the SHP's full raw frame table. The
        // common centered owner applies each stored frame's own origin.
        let scale_pips = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "pips2.shp",
                "palette.pal",
            )
        });

        // `Rules+0xFE0`, BOMBCURS.SHP by name (0x0066C5F4), drawn through the
        // convert `[0x0087F6C8]` that Init_Game builds from MOUSEPAL.PAL
        // (0x0052C13D..0x0052C1DD).
        let talk_bubble = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "talkbubl.shp",
                "palette.pal",
            )
        });
        let bomb_clock = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "bombcurs.shp",
                "mousepal.pal",
            )
        });
        // WRENCH.SHP by name (`0x0045E99E` into `[0x0089DDC8]`), drawn
        // through the same Init52C1D8 MOUSEPAL N1 Convert. Per-parent Extras
        // selects its packed E00 material; keep raw frame origins/indices here.
        let repair_wrench = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "wrench.shp",
                "mousepal.pal",
            )
        });
        // Original 5F76B0 /52BF26..52BFC1: one PIPS.SHP/PALETTE.PAL
        // resource serves health6F66BA/6F6A4A, rank70AA4F and self-heal70A6BB.
        // Keep every frame's native origin and original index plane.
        let pips = assets.and_then(|assets| {
            CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                "pips.shp",
                "palette.pal",
            )
        });

        Self {
            drag_texture,
            placement_shape,
            white_texture,
            diamond_outline_texture,
            diamond_filled_texture,
            pipbrd,
            scale_pips,
            bomb_clock,
            talk_bubble,
            repair_wrench,
            pips,
            #[cfg(feature = "phobos")]
            digital_shapes: Vec::new(),
            #[cfg(feature = "phobos")]
            insignia_shapes: std::collections::HashMap::new(),
        }
    }

    /// Load each `[DigitalDisplayTypes]` entry's `Shape=`/`Palette=` once per
    /// match, matching `DigitalDisplayTypeClass`'s retained `SHPStruct*`.
    #[cfg(feature = "phobos")]
    pub(crate) fn load_digital_display_shapes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        types: &[crate::rules::digital_display::DigitalDisplayType],
    ) {
        self.digital_shapes = types
            .iter()
            .map(|entry| {
                let shape = entry.shape.as_deref()?;
                let strip = CenteredShapeStrip::from_assets_on_device(
                    device,
                    queue,
                    batch,
                    assets,
                    shape,
                    entry.palette_name(),
                );
                if strip.is_none() {
                    log::warn!(
                        "Phobos DigitalDisplay {}: cannot load {shape} / {}; using text",
                        entry.name,
                        entry.palette_name()
                    );
                }
                strip
            })
            .collect();
    }

    /// One catalog entry's custom SHP, when loaded.
    #[cfg(feature = "phobos")]
    pub(crate) fn digital_shape(&self, index: usize) -> Option<&CenteredShapeStrip> {
        self.digital_shapes.get(index)?.as_ref()
    }

    /// Load every TechnoType/`[InsigniaTypes]` insignia SHP once per match.
    /// Phobos loads each `Promotable<SHPStruct*>` through the SHP reader with
    /// `PALETTE.PAL` and retains the pointer for the match.
    #[cfg(feature = "phobos")]
    pub(crate) fn load_insignia_shapes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let mut names = std::collections::BTreeSet::new();
        for object in rules.all_objects() {
            let insignia = &object.phobos_insignia;
            for variant in std::iter::once(&insignia.base)
                .chain(insignia.weapons.iter())
                .chain(insignia.passengers.iter())
            {
                names.extend(variant.images.iter().flatten().cloned());
            }
        }
        for entry in rules.insignia_types().entries() {
            names.extend(entry.images.iter().flatten().cloned());
        }
        for name in names {
            let strip = CenteredShapeStrip::from_assets_on_device(
                device,
                queue,
                batch,
                assets,
                &name,
                "palette.pal",
            );
            if strip.is_none() {
                log::warn!("Phobos insignia: cannot load {name}");
            }
            if let Some(strip) = strip {
                self.insignia_shapes.insert(name, strip);
            }
        }
    }

    /// One custom insignia SHP by normalized file name, when loaded.
    #[cfg(feature = "phobos")]
    pub(crate) fn insignia_shape(&self, name: &str) -> Option<&CenteredShapeStrip> {
        self.insignia_shapes.get(name)
    }

    /// The Crazy Ivan bomb clock art (BOMBCURS.SHP), when the assets have it.
    pub(crate) fn talk_bubble(&self) -> Option<&CenteredShapeStrip> {
        self.talk_bubble.as_ref()
    }

    pub fn bomb_clock(&self) -> Option<&CenteredShapeStrip> {
        self.bomb_clock.as_ref()
    }

    /// The repair wrench art (WRENCH.SHP), when the assets have it.
    pub fn repair_wrench(&self) -> Option<&CenteredShapeStrip> {
        self.repair_wrench.as_ref()
    }

    /// Physical PIPS.SHP shared by health, rank14/15/19 and self-heal13/20.
    pub fn pips(&self) -> Option<&CenteredShapeStrip> {
        self.pips.as_ref()
    }

    /// Physical PIPBRD.SHP selected-background frames 0/1.
    pub fn pipbrd(&self) -> Option<&CenteredShapeStrip> {
        self.pipbrd.as_ref()
    }

    /// Build sprite instances for the selection drag rectangle outline.
    ///
    /// Returns 4 thin line instances forming the rectangle. Positions are in
    /// screen-space with camera offset added (the shader subtracts camera_pos).
    pub fn build_drag_rect(
        &self,
        selection: &SelectionState,
        camera_x: f32,
        camera_y: f32,
    ) -> Vec<SpriteInstance> {
        let (min_x, min_y, max_x, max_y) = match selection.drag_rect() {
            Some(r) => r,
            None => return Vec::new(),
        };

        let rect_w: f32 = max_x - min_x;
        let rect_h: f32 = max_y - min_y;
        let t: f32 = DRAG_RECT_THICKNESS;

        // Offset by camera so screen-space coords survive the shader's
        // camera_pos subtraction.
        let ox: f32 = camera_x;
        let oy: f32 = camera_y;

        vec![
            // Top edge.
            SpriteInstance {
                position: [ox + min_x, oy + min_y],
                size: [rect_w, t],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DRAG_RECT_DEPTH,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            },
            // Bottom edge.
            SpriteInstance {
                position: [ox + min_x, oy + max_y - t],
                size: [rect_w, t],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DRAG_RECT_DEPTH,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            },
            // Left edge.
            SpriteInstance {
                position: [ox + min_x, oy + min_y],
                size: [t, rect_h],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DRAG_RECT_DEPTH,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            },
            // Right edge.
            SpriteInstance {
                position: [ox + max_x - t, oy + min_y],
                size: [t, rect_h],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DRAG_RECT_DEPTH,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            },
        ]
    }

    /// Get the drag rect texture for drawing the selection box.
    pub fn drag_texture(&self) -> &BatchTexture {
        &self.drag_texture
    }

    pub fn placement_shape(&self) -> Option<&CenteredShapeStrip> {
        self.placement_shape.as_ref()
    }

    pub fn white_texture(&self) -> &BatchTexture {
        &self.white_texture
    }

    /// Get the diamond outline texture for debug cell grid visualization.
    pub fn diamond_outline_texture(&self) -> &BatchTexture {
        &self.diamond_outline_texture
    }

    /// Get the white filled-diamond texture for debug pathgrid/path overlays.
    pub fn diamond_filled_texture(&self) -> &BatchTexture {
        &self.diamond_filled_texture
    }

    /// The one PIPS2.SHP strip: raw frame0 empty,1 occupied,3/4 overload,
    /// and2/5 ore/gem. Callers supply native frame IDs, not atlas variants.
    pub fn scale_pips(&self) -> Option<&CenteredShapeStrip> {
        self.scale_pips.as_ref()
    }
}

/// Generate a 2x2 solid color texture (4 identical pixels).
fn generate_solid_color(color: [u8; 4]) -> Vec<u8> {
    let mut rgba: Vec<u8> = Vec::with_capacity(16);
    for _ in 0..4 {
        rgba.extend_from_slice(&color);
    }
    rgba
}

/// Generate a 1px-thick diamond outline texture (TILE_WIDTH x TILE_HEIGHT).
/// White outline on transparent background — tinted per-instance for debug coloring.
fn generate_diamond_outline() -> Vec<u8> {
    let w: u32 = TILE_WIDTH as u32;
    let h: u32 = TILE_HEIGHT as u32;
    let cx: f32 = w as f32 / 2.0;
    let cy: f32 = h as f32 / 2.0;
    let mut rgba: Vec<u8> = vec![0u8; (w * h * 4) as usize];
    for py in 0..h {
        for px in 0..w {
            let dist: f32 = (px as f32 - cx).abs() / cx + (py as f32 - cy).abs() / cy;
            // Draw pixels near the diamond edge (within ~1.5px of dist=1.0).
            let edge_dist: f32 = (dist - 1.0).abs() * cx;
            if edge_dist < 1.5 {
                let alpha: f32 = (1.5 - edge_dist).min(1.0);
                let idx: usize = ((py * w + px) * 4) as usize;
                rgba[idx] = 255;
                rgba[idx + 1] = 255;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = (alpha * 255.0) as u8;
            }
        }
    }
    rgba
}

/// Generate an isometric diamond-shaped cell texture (TILE_WIDTH x TILE_HEIGHT).
/// The diamond inscribes the tile rectangle with 1px anti-aliased edges.
fn generate_diamond_cell(color: [u8; 4]) -> Vec<u8> {
    let w: u32 = TILE_WIDTH as u32;
    let h: u32 = TILE_HEIGHT as u32;
    let cx: f32 = (w as f32) / 2.0;
    let cy: f32 = (h as f32) / 2.0;
    let mut rgba: Vec<u8> = vec![0u8; (w * h * 4) as usize];
    for py in 0..h {
        for px in 0..w {
            // Normalized diamond distance: 0 at center, 1 at edge.
            let dist: f32 = (px as f32 - cx).abs() / cx + (py as f32 - cy).abs() / cy;
            if dist <= 1.0 {
                // Anti-alias: fade out over the last ~1px before the edge.
                let edge_fade: f32 = ((1.0 - dist) * cx).min(1.0);
                let alpha: u8 = (color[3] as f32 * edge_fade) as u8;
                let idx: usize = ((py * w + px) * 4) as usize;
                rgba[idx] = color[0];
                rgba[idx + 1] = color[1];
                rgba[idx + 2] = color[2];
                rgba[idx + 3] = alpha;
            }
        }
    }
    rgba
}
