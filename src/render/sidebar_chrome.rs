//! Sidebar chrome atlas: loads original RA2 sidebar art, keeping
//! theme-specific radar/background authority separate from generic chrome
//! resolved through the active side MIX stack, then packs the decoded frames
//! into GPU textures for batched rendering.
//!
//! Generic SHP dimensions are side-specific retail data, so callers consume
//! the dimensions decoded from each selected archive rather than assuming the
//! Allied layout applies to Soviet or Yuri side-2 art.

use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_archive::MixArchive;
use crate::assets::mix_hash::mix_hash;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;

#[path = "sidebar_chrome_command_bar.rs"]
mod command_bar;
pub use command_bar::CommandBarArt;

#[path = "sidebar_chrome_in_game_shell.rs"]
mod in_game_shell;
pub use in_game_shell::InGameShellArt;

#[cfg(test)]
pub(crate) fn packed_command_bar_fixture(
    assets: &AssetManager,
    theme: SidebarTheme,
) -> (CommandBarArt<SidebarChromeEntry>, Vec<u8>, [u32; 2]) {
    let route = SidebarSideRoute::for_theme(assets, theme);
    let palette = decode_sidebar_palette(route.resolve("SIDEBAR.PAL").unwrap().bytes).unwrap();
    let rendered = command_bar::load(route, &palette);
    let width = rendered.entries().map(|entry| entry.width).max().unwrap();
    let height = rendered
        .entries()
        .map(|entry| entry.height + CHROME_PADDING)
        .sum();
    let mut rgba = vec![0; (width * height * 4) as usize];
    let mut y = 0;
    let packed = rendered.map(|entry| {
        let packed = blit_entry(&mut rgba, width, height, y, entry);
        y += entry.height + CHROME_PADDING;
        packed
    });
    (packed, rgba, [width, height])
}

const CHROME_PADDING: u32 = 2;
/// R-UP.SHP / R-DN.SHP frame count. The strip-scroll art uses a 3-frame
/// convention (0 = idle, 1 = pressed, 2 = disabled) — NOT the 5-frame tab /
/// repair / sell convention.
const SCROLL_FRAME_COUNT: usize = 3;
// RadarClass::Draw 653100: frame 0 is offline, frame 32 is online.
const RADAR_DEFAULT_FRAME: usize = 0;
const TOP_STRIP_LEFT_ID: i32 = 0xD508C1A4u32 as i32;
const TOP_STRIP_SIDEBAR_ID: i32 = 0xF0F1CE8Du32 as i32;
const TOP_STRIP_THIN_ID: i32 = 0x7637D6E1u32 as i32;
const UNKNOWN_TOP_HOUSING_ID: i32 = 0x7AEBAE6Bu32 as i32;
const UNKNOWN_MID_PANEL_ID: i32 = 0xB0259C24u32 as i32;
const ALLIED_SIDE_ARCHIVE_ORDER: &[&str] = &["sidec01md.mix", "sidec01.mix", "sidenc01.mix"];
const SIDE_TWO_ARCHIVE_ORDER: &[&str] = &["sidec02md.mix", "sidec02.mix", "sidenc02.mix"];
const GENERIC_SIDEBAR_SHP_NAMES: &[&str] = &[
    "side1.shp",
    "side2.shp",
    "side2b.shp",
    "sidebttn.shp",
    "side3.shp",
    "addon.shp",
    "top.shp",
    "credits.shp",
    "tab00.shp",
    "tab01.shp",
    "tab02.shp",
    "tab03.shp",
    "repair.shp",
    "sell.shp",
    "r-up.shp",
    "r-dn.shp",
    "powerp.shp",
    "diplobtn.shp",
    "optbtn.shp",
    "gclock2.shp",
];

pub use crate::ui::sidebar::SidebarTheme;

/// One logical sidebar input and the archive selected by the existing resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarChromeAssetIdentity {
    pub logical_name: String,
    pub source_archive: Option<String>,
}

/// Immutable source identity captured when one theme atlas is constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarChromeAtlasIdentity {
    pub atlas_theme: SidebarTheme,
    pub parent_archive: String,
    pub radar: SidebarChromeAssetIdentity,
    pub theme_palette: SidebarChromeAssetIdentity,
    pub generic_palette: SidebarChromeAssetIdentity,
    pub background_palette: Option<SidebarChromeAssetIdentity>,
    pub backgrounds: Option<[SidebarChromeAssetIdentity; 3]>,
}

/// Requested-versus-actual identity for one atlas lookup, including fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSidebarChromeIdentity {
    pub requested_theme: SidebarTheme,
    pub actual_theme: SidebarTheme,
    pub atlas: SidebarChromeAtlasIdentity,
}

/// Borrowed atlas resolution used to keep selection and provenance atomic.
pub struct ResolvedSidebarChrome<'a> {
    pub requested_theme: SidebarTheme,
    pub actual_theme: SidebarTheme,
    pub atlas: &'a SidebarChromeAtlas,
}

impl ResolvedSidebarChrome<'_> {
    pub fn identity(&self) -> ResolvedSidebarChromeIdentity {
        ResolvedSidebarChromeIdentity {
            requested_theme: self.requested_theme,
            actual_theme: self.actual_theme,
            atlas: self.atlas.source_identity().clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct SidebarSideRoute<'a> {
    asset_manager: &'a AssetManager,
    archive_names: &'static [&'static str],
    #[cfg(feature = "ares")]
    registered_group: bool,
}

impl<'a> SidebarSideRoute<'a> {
    fn for_theme(asset_manager: &'a AssetManager, theme: SidebarTheme) -> Self {
        let archive_names = match theme {
            SidebarTheme::Allied => ALLIED_SIDE_ARCHIVE_ORDER,
            SidebarTheme::Soviet | SidebarTheme::Yuri => SIDE_TWO_ARCHIVE_ORDER,
        };
        Self {
            asset_manager,
            archive_names,
            #[cfg(feature = "ares")]
            registered_group: false,
        }
    }

    #[cfg(feature = "ares")]
    fn for_registered_group(asset_manager: &'a AssetManager) -> Self {
        Self {
            asset_manager,
            archive_names: &[],
            registered_group: true,
        }
    }

    fn resolve(&self, asset_name: &str) -> Option<ResolvedSidebarAsset<'a>> {
        #[cfg(feature = "ares")]
        if self.registered_group {
            // The selected Side MIX group is registered by AssetManager.
            // Loose/earlier expansion overrides keep CCFile first-match order.
            let selected = self.asset_manager.resolve_ref(asset_name)?;
            return Some(ResolvedSidebarAsset {
                archive_name: selected.source_archive,
                bytes: selected.bytes,
            });
        }
        resolve_sidebar_asset_in_order(
            self.archive_names.iter().filter_map(|&archive_name| {
                self.asset_manager
                    .archive(archive_name)
                    .map(|archive| (archive_name, archive))
            }),
            asset_name,
        )
    }

    fn resolve_generic_shp(&self, shp_name: &str) -> Option<ResolvedSidebarAsset<'a>> {
        if !is_generic_sidebar_shp_name(shp_name) {
            return None;
        }
        self.resolve(shp_name)
    }
}

#[derive(Clone, Copy)]
struct ResolvedSidebarAsset<'a> {
    archive_name: &'a str,
    bytes: &'a [u8],
}

fn resolve_sidebar_asset_in_order<'a>(
    archives: impl IntoIterator<Item = (&'static str, &'a MixArchive)>,
    asset_name: &str,
) -> Option<ResolvedSidebarAsset<'a>> {
    archives.into_iter().find_map(|(archive_name, archive)| {
        archive
            .get_by_name(asset_name)
            .map(|bytes| ResolvedSidebarAsset {
                archive_name,
                bytes,
            })
    })
}

fn is_generic_sidebar_shp_name(name: &str) -> bool {
    GENERIC_SIDEBAR_SHP_NAMES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// UV coordinates and pixel dimensions for one chrome piece in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct SidebarChromeEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct SidebarChromeExtraEntry {
    pub id: i32,
    pub label: String,
    pub frame_count: usize,
    pub entry: SidebarChromeEntry,
}

pub(crate) struct SidebarDarkenMask {
    pub texture: BatchTexture,
    pub frame_origin: [f32; 2],
}

impl SidebarDarkenMask {
    pub(crate) fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        bytes: &[u8],
        palette: &Palette,
    ) -> Option<Self> {
        let shp = ShpFile::from_bytes(bytes).ok()?;
        let frame = shp.frames.first()?;
        let width = u32::from(frame.frame_width);
        let height = u32::from(frame.frame_height);
        if width == 0 || height == 0 {
            return None;
        }
        // Unavailable496250 uses indices alone; flash4963F0 uses this same
        // frame with the sidebar's ordinary N=1 palette conversion.
        let rgba = shp.frame_to_rgba(0, palette).ok()?;
        Some(Self {
            texture: batch.create_texture_on_device(
                device,
                queue,
                &rgba,
                width,
                height,
                Some(&frame.pixels),
            ),
            frame_origin: [f32::from(frame.frame_x), f32::from(frame.frame_y)],
        })
    }

    pub(crate) fn load(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        palette: &Palette,
    ) -> Option<Self> {
        // Original6A500B..6A5018: literal DARKEN.SHP through LoadFileFromMIX,
        // not the side-only generic chrome resolver.
        let source = assets.load_file_from_mix("DARKEN.SHP")?;
        log::info!("Sidebar DARKEN.SHP resolved from {}", source.source_archive);
        Self::from_bytes(&gpu.device, &gpu.queue, batch, &source.bytes, palette)
    }
}

/// All sidebar chrome art for one faction theme, packed into a single texture.
pub struct SidebarChromeAtlas {
    source_identity: SidebarChromeAtlasIdentity,
    pub texture: BatchTexture,
    pub command_bar: CommandBarArt<SidebarChromeEntry>,
    pub in_game_shell: InGameShellArt<SidebarChromeEntry>,
    pub top_strip_left: Option<SidebarChromeEntry>,
    pub top_strip_sidebar: Option<SidebarChromeEntry>,
    pub top_strip_thin: Option<SidebarChromeEntry>,
    pub unknown_top_housing: Option<SidebarChromeEntry>,
    pub unknown_mid_panel: Option<SidebarChromeEntry>,
    pub background_large: Option<SidebarChromeEntry>,
    pub background_medium: Option<SidebarChromeEntry>,
    pub background_small: Option<SidebarChromeEntry>,
    /// All pre-rendered RGBA frames of radar.shp for the opening/closing animation.
    /// Frame 0 = closed/offline housing; frame 32 is the opening endpoint.
    pub radar_frames: Vec<Vec<u8>>,
    /// Pixel dimensions of each radar frame (width, height).
    pub radar_frame_size: [u32; 2],
    /// Fixed native aperture from RadarClass One_Time (`652CF0`).
    /// [left, top, right, bottom] in unscaled pixels. The minimap fills the area
    /// inside these insets. Derived automatically — no manual tuning needed.
    pub radar_content_insets: [u32; 4],
    pub radar: SidebarChromeEntry,
    pub side1: SidebarChromeEntry,
    pub tabs: Option<SidebarChromeEntry>,
    /// Tab buttons: per-tab × 5-frame SHP state table.
    /// Outer index: tab number (0..=3). Inner index: SHP frame (0..=4).
    /// Frame meanings (`ShapeButtonClass::Draw @ 0x0069DEB0`):
    ///   0 = idle, 1 = active-tab, 2 = disabled, 3 = flash-idle, 4 = flash-active.
    pub tab_frames: [[Option<SidebarChromeEntry>; 5]; 4],
    pub side2: SidebarChromeEntry,
    pub side3: SidebarChromeEntry,
    /// Repair button — same frame convention as `tab_frames`; it never
    /// flashes, and retail art has only frames 0 (off) and 1 (on).
    pub repair_frames: [Option<SidebarChromeEntry>; 5],
    /// Sell button — same as `repair_frames`.
    pub sell_frames: [Option<SidebarChromeEntry>; 5],
    /// Strip scroll-up (R-UP.SHP, −page) — 3-frame convention:
    /// 0 = idle, 1 = pressed, 2 = disabled. Missing frames fall back to
    /// frame 0 at draw exactly like repair/sell.
    /// DIPLOBTN / OPTBTN, ordinary plain-button idle/pressed frames.
    pub top_button_frames: [[Option<SidebarChromeEntry>; 2]; 2],
    pub scroll_up_frames: [Option<SidebarChromeEntry>; SCROLL_FRAME_COUNT],
    /// Strip scroll-down (R-DN.SHP, +page) — same 3-frame convention.
    pub scroll_down_frames: [Option<SidebarChromeEntry>; SCROLL_FRAME_COUNT],
    pub power: Option<SidebarChromeEntry>,
    /// powerp.shp frames: 5 colored strip segments for the power bar meter.
    /// [0]=dark/bg, [1]=green, [2]=yellow, [3]=red, [4]=dark/off.
    pub powerp_frames: [Option<SidebarChromeEntry>; 5],
    pub extra_entries: Vec<SidebarChromeExtraEntry>,
    /// GCLOCK2.SHP progress clock overlay — all frames packed into a grid texture.
    /// Each entry maps a frame index to UV coordinates in gclock_texture.
    pub gclock_texture: Option<BatchTexture>,
    pub gclock_frames: Vec<SidebarChromeEntry>,
    /// Global DARKEN.SHP source, converted with this side's ordinary N=1
    /// palette (SidebarLoadSHPs6A588C). Its resolver is LoadFileFromMIX.
    pub(crate) darken: Option<SidebarDarkenMask>,
}

impl SidebarChromeAtlas {
    pub fn source_identity(&self) -> &SidebarChromeAtlasIdentity {
        &self.source_identity
    }
}

/// The selected side is a runtime index, not another hardcoded faction enum.
/// Retail keeps its three prebuilt themes; Ares prepares the current scenario's
/// selected group through the same atlas builder and resolver.
pub struct SidebarChromeSet {
    atlases: Vec<(i32, SidebarChromeAtlas)>,
}

impl SidebarChromeSet {
    pub fn for_theme(&self, theme: SidebarTheme) -> Option<&SidebarChromeAtlas> {
        self.resolve_theme(theme).map(|resolved| resolved.atlas)
    }

    pub fn resolve_theme(&self, theme: SidebarTheme) -> Option<ResolvedSidebarChrome<'_>> {
        self.resolve_side(match theme {
            SidebarTheme::Allied => 0,
            SidebarTheme::Soviet => 1,
            SidebarTheme::Yuri => 2,
        })
    }

    pub fn resolve_side(&self, side_index: i32) -> Option<ResolvedSidebarChrome<'_>> {
        let requested_theme = sidebar_theme_for_side(side_index);
        #[cfg(feature = "ares")]
        let (actual_theme, atlas) = {
            let (_, atlas) = self.atlases.iter().find(|(side, _)| *side == side_index)?;
            (atlas.source_identity.atlas_theme, atlas)
        };
        #[cfg(not(feature = "ares"))]
        let (actual_theme, atlas) = {
            let lookup = |index| {
                self.atlases
                    .iter()
                    .find(|(side, _)| *side == index)
                    .map(|(_, a)| a)
            };
            select_sidebar_theme(requested_theme, lookup(0), lookup(1), lookup(2))?
        };
        Some(ResolvedSidebarChrome {
            requested_theme,
            actual_theme,
            atlas,
        })
    }
}

/// Sidebar geometry6A5090 is side-zero versus nonzero. The Yuri label keeps
/// existing stock provenance; an extra side uses the same nonzero geometry,
/// independently of its MIX index and Yuri filename switch.
pub(crate) fn sidebar_theme_for_side(side_index: i32) -> SidebarTheme {
    match side_index {
        0 => SidebarTheme::Allied,
        1 => SidebarTheme::Soviet,
        2 => SidebarTheme::Yuri,
        #[cfg(feature = "ares")]
        _ => SidebarTheme::Soviet,
        #[cfg(not(feature = "ares"))]
        _ => SidebarTheme::Allied,
    }
}

pub(crate) fn select_sidebar_theme<'a, T>(
    requested_theme: SidebarTheme,
    allied: Option<&'a T>,
    soviet: Option<&'a T>,
    yuri: Option<&'a T>,
) -> Option<(SidebarTheme, &'a T)> {
    match requested_theme {
        SidebarTheme::Allied => allied.map(|atlas| (SidebarTheme::Allied, atlas)),
        SidebarTheme::Soviet => soviet
            .map(|atlas| (SidebarTheme::Soviet, atlas))
            .or_else(|| allied.map(|atlas| (SidebarTheme::Allied, atlas))),
        SidebarTheme::Yuri => yuri
            .map(|atlas| (SidebarTheme::Yuri, atlas))
            .or_else(|| soviet.map(|atlas| (SidebarTheme::Soviet, atlas)))
            .or_else(|| allied.map(|atlas| (SidebarTheme::Allied, atlas))),
    }
}

/// Ares1002EAA0/1002FA60/1002FA70 consume one independent filename
/// switch, not the actual side index or the selected MIX number.
#[derive(Clone, Copy)]
struct SidebarArtNames {
    radar_palette: &'static str,
    radar: &'static str,
    background_palette: &'static str,
    backgrounds: (&'static str, &'static str, &'static str),
}

impl SidebarArtNames {
    const fn for_yuri(yuri: bool) -> Self {
        if yuri {
            Self {
                radar_palette: "radaryuri.pal",
                radar: "radary.shp",
                background_palette: "UIBKGDY.PAL",
                backgrounds: ("bkgdlgy.shp", "bkgdmdy.shp", "bkgdsmy.shp"),
            }
        } else {
            Self {
                radar_palette: "sidebar.pal",
                radar: "radar.shp",
                background_palette: "UIBKGD.PAL",
                backgrounds: ("bkgdlg.shp", "bkgdmd.shp", "bkgdsm.shp"),
            }
        }
    }
}

pub fn build_sidebar_chrome_set(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &mut AssetManager,
    _rules: &crate::rules::ruleset::RuleSet,
    _side_index: i32,
) -> Result<Option<SidebarChromeSet>, crate::assets::error::AssetError> {
    #[cfg(feature = "ares")]
    {
        let side = u8::try_from(_side_index)
            .ok()
            .map(crate::rules::ruleset::SideIdx);
        let config = side.and_then(|side| {
            Some((
                _rules.side_sidebar_mix_file_index(side)?,
                _rules.side_sidebar_yuri_file_names(side)?,
            ))
        });
        let Some((mix_index, yuri_names)) = config else {
            return Err(crate::assets::error::AssetError::ParseError {
                format: "sidebar".into(),
                detail: format!("side {_side_index} has no retained sidebar configuration"),
            });
        };
        if !asset_manager.activate_sidebar_archives(mix_index)? {
            // Full_Init687824 and saved Load67E766 abort when534FA0 fails.
            return Err(crate::assets::error::AssetError::ParseError {
                format: "sidebar".into(),
                detail: format!(
                    "side {_side_index}: sidebar MIX initialization failed for index {mix_index}"
                ),
            });
        }
        let base = format!("sidec{mix_index:02}.mix");
        let md = format!("sidec{mix_index:02}md.mix");
        let primary = if asset_manager.archive(&md).is_some() {
            &md
        } else {
            &base
        };
        let atlas = build_theme_atlas(
            gpu,
            batch,
            asset_manager,
            sidebar_theme_for_side(_side_index),
            primary,
            SidebarArtNames::for_yuri(yuri_names),
        )
        .ok_or_else(|| crate::assets::error::AssetError::AssetNotFound {
            name: format!(
                "required sidebar shapes or palettes for side {_side_index} in {primary}"
            ),
        })?;
        Ok(Some(SidebarChromeSet {
            atlases: vec![(_side_index, atlas)],
        }))
    }
    #[cfg(not(feature = "ares"))]
    {
        let mut atlases = Vec::new();
        for (index, theme, archive, yuri) in [
            (0, SidebarTheme::Allied, "sidec01.mix", false),
            (1, SidebarTheme::Soviet, "sidec02.mix", false),
            (2, SidebarTheme::Yuri, "sidec02md.mix", true),
        ] {
            if let Some(atlas) = build_theme_atlas(
                gpu,
                batch,
                asset_manager,
                theme,
                archive,
                SidebarArtNames::for_yuri(yuri),
            ) {
                atlases.push((index, atlas));
            }
        }
        Ok((!atlases.is_empty()).then_some(SidebarChromeSet { atlases }))
    }
}

struct CpuGclockAtlas {
    rgba: Vec<u8>,
    indices: Vec<u8>,
    width: u32,
    height: u32,
    frames: Vec<SidebarChromeEntry>,
    source_archive: String,
}

/// Resolve all GCLOCK2 frames from the active side stack and pack them into a
/// CPU atlas. The GPU wrapper below consumes this exact output.
fn build_gclock_cpu_atlas(
    route: SidebarSideRoute<'_>,
    palette: &Palette,
) -> Option<CpuGclockAtlas> {
    let resolved = route.resolve_generic_shp("GCLOCK2.SHP")?;
    build_gclock_cpu_atlas_from_bytes(resolved.bytes, palette, resolved.archive_name)
}

fn build_gclock_cpu_atlas_from_bytes(
    shp_bytes: &[u8],
    palette: &Palette,
    source_archive: &str,
) -> Option<CpuGclockAtlas> {
    let shp = ShpFile::from_bytes(shp_bytes).ok()?;
    let frame_count = shp.frames.len();
    if frame_count == 0 {
        return None;
    }

    let frame_w = shp.width as u32;
    let frame_h = shp.height as u32;
    let mut rendered_frames: Vec<RenderedChromeEntry> = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        // Use render_shp() to composite onto the full canvas, handling frame_x/frame_y
        // offsets and sub-canvas frame sizes correctly.
        // Retain original indices: compressed 4963F0 skips source zero,
        // including when its palette colour would otherwise be opaque black.
        rendered_frames.push(render_shp(&shp, palette, i)?);
    }

    // Pack into a grid texture. 8 columns keeps it roughly square for typical 54-frame SHPs.
    let cols = 8u32.min(frame_count as u32);
    let rows = (frame_count as u32).div_ceil(cols);
    let tex_w = cols * frame_w;
    let tex_h = rows * frame_h;
    let mut atlas_rgba = vec![0u8; (tex_w * tex_h * 4) as usize];
    let mut atlas_indices = vec![0u8; (tex_w * tex_h) as usize];

    let mut entries = Vec::with_capacity(frame_count);
    for (i, frame) in rendered_frames.iter().enumerate() {
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;
        let dst_x = col * frame_w;
        let dst_y = row * frame_h;

        for y in 0..frame_h {
            let src_start = (y * frame_w * 4) as usize;
            let dst_start = (((dst_y + y) * tex_w + dst_x) * 4) as usize;
            let len = (frame_w * 4) as usize;
            if src_start + len <= frame.rgba.len() && dst_start + len <= atlas_rgba.len() {
                atlas_rgba[dst_start..dst_start + len]
                    .copy_from_slice(&frame.rgba[src_start..src_start + len]);
                atlas_indices[dst_start / 4..(dst_start + len) / 4]
                    .copy_from_slice(&frame.indices[src_start / 4..(src_start + len) / 4]);
            }
        }

        entries.push(SidebarChromeEntry {
            uv_origin: [dst_x as f32 / tex_w as f32, dst_y as f32 / tex_h as f32],
            uv_size: [frame_w as f32 / tex_w as f32, frame_h as f32 / tex_h as f32],
            pixel_size: [frame_w as f32, frame_h as f32],
        });
    }

    log::info!(
        "GCLOCK2 atlas for {}: {}x{} px, {} frames ({}x{} grid)",
        source_archive,
        tex_w,
        tex_h,
        frame_count,
        cols,
        rows,
    );

    Some(CpuGclockAtlas {
        rgba: atlas_rgba,
        indices: atlas_indices,
        width: tex_w,
        height: tex_h,
        frames: entries,
        source_archive: source_archive.to_owned(),
    })
}

/// Upload the production CPU atlas without duplicating resolver or decode logic.
fn build_gclock_texture(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    route: SidebarSideRoute<'_>,
    palette: &Palette,
) -> Option<(BatchTexture, Vec<SidebarChromeEntry>)> {
    let atlas = build_gclock_cpu_atlas(route, palette)?;
    log::debug!(
        "Uploading GCLOCK2 from {} ({}x{})",
        atlas.source_archive,
        atlas.width,
        atlas.height
    );
    let texture = batch.create_texture_with_indices(
        gpu,
        &atlas.rgba,
        atlas.width,
        atlas.height,
        Some(&atlas.indices),
    );
    Some((texture, atlas.frames))
}

// Original72ADE0 expands six-bit PAL (the one palette conversion); ordinary
// Convert48E740->4BBB00 N=1 stores exact R5G6B5. Precompose once through the
// enrolled retail output profile; the shared RGBA shader must not apply
// LightConvert row lighting.
fn decode_sidebar_palette(bytes: &[u8]) -> Result<Palette, crate::assets::error::AssetError> {
    let palette = Palette::from_bytes(bytes)?;
    Ok(Palette::from_rgb(|index| {
        let color = palette.color(index);
        let [r, g, b, _] = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
            .quantize_rgba8([color.r, color.g, color.b, color.a]);
        [r, g, b]
    }))
}

fn resolve_theme_palette_with_source(
    asset_manager: &AssetManager,
    mix: &MixArchive,
    mix_name: &str,
    palette_name: &str,
) -> Option<(Palette, String)> {
    #[cfg(feature = "ares")]
    {
        // Palette72ADE0 reads through a fresh CCFile4739F0/4A3890 and
        // current MIX lookup5B4430, not SHP4A38D0 -> CRC cache5B40B0.
        // Yuri palette names therefore follow the newly registered group
        // even though730400 does not retire those differently named keys.
        let _ = (mix, mix_name);
        let selected = asset_manager.resolve_ref(palette_name)?;
        return decode_sidebar_palette(selected.bytes)
            .ok()
            .map(|p| (p, selected.source_archive.to_owned()));
    }
    #[cfg(not(feature = "ares"))]
    {
        if let Some(bytes) = mix.get_by_name(palette_name)
            && let Ok(palette) = decode_sidebar_palette(bytes)
        {
            return Some((palette, mix_name.to_string()));
        }

        let (bytes, source_archive) = asset_manager.get_with_source_ref(palette_name)?;
        decode_sidebar_palette(bytes)
            .ok()
            .map(|palette| (palette, source_archive.to_string()))
    }
}

fn theme_asset_identity(
    asset_manager: &AssetManager,
    mix: &MixArchive,
    mix_name: &str,
    logical_name: &str,
) -> SidebarChromeAssetIdentity {
    #[cfg(feature = "ares")]
    let source_archive = {
        let _ = (mix, mix_name);
        asset_manager
            .resolve_ref(logical_name)
            .map(|s| s.source_archive.to_owned())
    };
    #[cfg(not(feature = "ares"))]
    let source_archive = if mix.get_by_name(logical_name).is_some() {
        Some(mix_name.to_string())
    } else {
        asset_manager
            .get_with_source_ref(logical_name)
            .map(|(_, archive_name)| archive_name.to_string())
    };
    SidebarChromeAssetIdentity {
        logical_name: logical_name.to_string(),
        source_archive,
    }
}

fn build_theme_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    theme: SidebarTheme,
    mix_name: &str,
    names: SidebarArtNames,
) -> Option<SidebarChromeAtlas> {
    let palette_name = names.radar_palette;
    let radar_name = names.radar;
    let background_names = Some(names.backgrounds);
    let mix = asset_manager.archive(mix_name)?;
    let (theme_palette, theme_palette_source_archive) =
        resolve_theme_palette_with_source(asset_manager, mix, mix_name, palette_name)?;
    #[cfg(feature = "ares")]
    let side_route = SidebarSideRoute::for_registered_group(asset_manager);
    #[cfg(not(feature = "ares"))]
    let side_route = SidebarSideRoute::for_theme(asset_manager, theme);
    let generic_palette_name = "SIDEBAR.PAL";
    let generic_palette_source = side_route.resolve(generic_palette_name)?;
    let generic_palette = decode_sidebar_palette(generic_palette_source.bytes).ok()?;
    let command_bar_art = command_bar::load(side_route, &generic_palette);
    let in_game_shell_art = in_game_shell::load(side_route, &generic_palette);
    log::debug!(
        "{theme:?} generic sidebar palette resolved from {}",
        generic_palette_source.archive_name
    );
    // For sidebar inspection/fidelity work, decode every sidebar-side MIX SHP
    // with the theme's main sidebar palette so unknown pieces are comparable
    // under one consistent color treatment.
    let tabs_palette = theme_palette.clone();

    // Required pieces — without these, skip the theme entirely.
    let radar = render_entry(
        asset_manager,
        mix,
        radar_name,
        &theme_palette,
        RADAR_DEFAULT_FRAME,
    )?;
    let radar_identity = theme_asset_identity(asset_manager, mix, mix_name, radar_name);

    // Pre-render all radar.shp frames for the opening/closing animation.
    let (radar_frames, radar_frame_size, radar_content_insets) =
        render_all_radar_frames(asset_manager, mix, radar_name, &theme_palette);
    let side1 = render_side_entry(side_route, "side1.shp", &generic_palette, 0)?;
    let side2 = render_side_entry(side_route, "side2.shp", &generic_palette, 0)?;
    let side3 = render_side_entry(side_route, "side3.shp", &generic_palette, 0)?;

    // Optional pieces — gracefully degrade if missing.
    let tabs = render_entry(asset_manager, mix, "tabs.shp", &tabs_palette, 0);

    // Tab buttons: 4 tabs × 5 frames each. Missing frames in retail fall
    // back to None; the renderer skips that frame's draw if not present.
    // Frame 0 = idle, 1 = active-tab, 2 = disabled, 3 = flash-idle,
    // 4 = flash-active (`ui::sidebar::gadget_flash::shape_button_frame`).
    let mut tab_frame_entries: [[Option<RenderedChromeEntry>; 5]; 4] = Default::default();
    for (tab, tab_entries) in tab_frame_entries.iter_mut().enumerate() {
        for (frame, slot) in tab_entries.iter_mut().enumerate() {
            let entry = render_side_entry(
                side_route,
                &format!("tab0{tab}.shp"),
                &generic_palette,
                frame,
            );
            if entry.is_none() && frame > 0 {
                log::warn!(
                    "tab0{tab}.shp frame {frame} missing in MIX — gadget state {frame} will fall back to idle"
                );
            }
            *slot = entry;
        }
    }

    let mut repair_frame_entries: [Option<RenderedChromeEntry>; 5] = Default::default();
    for (frame, slot) in repair_frame_entries.iter_mut().enumerate() {
        let entry = render_side_entry(side_route, "repair.shp", &generic_palette, frame);
        if entry.is_none() && frame > 0 {
            log::warn!("repair.shp frame {frame} missing in MIX");
        }
        *slot = entry;
    }

    let mut sell_frame_entries: [Option<RenderedChromeEntry>; 5] = Default::default();
    for (frame, slot) in sell_frame_entries.iter_mut().enumerate() {
        let entry = render_side_entry(side_route, "sell.shp", &generic_palette, frame);
        if entry.is_none() && frame > 0 {
            log::warn!("sell.shp frame {frame} missing in MIX");
        }
        *slot = entry;
    }

    // 652E90 + 653010: both top controls use the generic SIDEBAR.PAL
    // converter (87F6CC), including Yuri. Native mask5, plain toggle kind0.
    let top_button_entries: [[Option<RenderedChromeEntry>; 2]; 2] = ["diplobtn.shp", "optbtn.shp"]
        .map(|name| {
            std::array::from_fn(|frame| {
                render_side_entry(side_route, name, &generic_palette, frame)
            })
        });

    // Strip-scroll pair — 3-frame art (0 idle, 1 pressed, 2 disabled).
    // render_entry returns None for absent frames and the draw falls back to
    // frame 0, so a short retail SHP still degrades gracefully.
    let mut scroll_up_entries: [Option<RenderedChromeEntry>; SCROLL_FRAME_COUNT] =
        Default::default();
    let mut scroll_down_entries: [Option<RenderedChromeEntry>; SCROLL_FRAME_COUNT] =
        Default::default();
    for frame in 0..SCROLL_FRAME_COUNT {
        scroll_up_entries[frame] =
            render_side_entry(side_route, "r-up.shp", &generic_palette, frame);
        scroll_down_entries[frame] =
            render_side_entry(side_route, "r-dn.shp", &generic_palette, frame);
    }
    if scroll_up_entries[0].is_none() {
        log::warn!("r-up.shp missing in MIX — strip scroll-up button will not render");
    }
    if scroll_down_entries[0].is_none() {
        log::warn!("r-dn.shp missing in MIX — strip scroll-down button will not render");
    }
    let power = render_entry(asset_manager, mix, "power.shp", &theme_palette, 0);
    // 63FB20 uses the actual SHP frame/canvas; transparent samples keep
    // their skipped-store meaning and the renderer advances by 3 pixels.
    let powerp_rendered: Vec<RenderedChromeEntry> = (0..5)
        .filter_map(|i| render_side_entry(side_route, "powerp.shp", &generic_palette, i))
        .collect();
    let top_strip_left = render_entry_by_id(mix, TOP_STRIP_LEFT_ID, &tabs_palette, 0);
    let top_strip_sidebar = render_side_entry(side_route, "top.shp", &generic_palette, 0);
    let top_strip_thin = render_side_entry(side_route, "credits.shp", &generic_palette, 0);
    let unknown_top_housing = render_side_entry(side_route, "addon.shp", &generic_palette, 0);
    let unknown_mid_panel = render_entry_by_id(mix, UNKNOWN_MID_PANEL_ID, &theme_palette, 0);
    let ([background_large, background_medium, background_small], background_palette) =
        background_names
            .and_then(|backgrounds| {
                in_game_shell::load_backgrounds(
                    asset_manager,
                    mix,
                    mix_name,
                    backgrounds,
                    names.background_palette,
                )
            })
            .map(|(entries, identity)| (entries, Some(identity)))
            .unwrap_or(([None, None, None], None));
    let background_identities = background_names.map(|(large, medium, small)| {
        [
            theme_asset_identity(asset_manager, mix, mix_name, large),
            theme_asset_identity(asset_manager, mix, mix_name, medium),
            theme_asset_identity(asset_manager, mix, mix_name, small),
        ]
    });
    let source_identity = SidebarChromeAtlasIdentity {
        atlas_theme: theme,
        parent_archive: mix_name.to_string(),
        radar: radar_identity,
        theme_palette: SidebarChromeAssetIdentity {
            logical_name: palette_name.to_string(),
            source_archive: Some(theme_palette_source_archive),
        },
        generic_palette: SidebarChromeAssetIdentity {
            logical_name: generic_palette_name.to_string(),
            source_archive: Some(generic_palette_source.archive_name.to_string()),
        },
        backgrounds: background_identities,
        background_palette,
    };
    let excluded_extra_ids = known_loose_piece_ids(radar_name);
    let extra_rendered = collect_extra_entries(
        mix,
        &theme_palette,
        &tabs_palette,
        background_names,
        &excluded_extra_ids,
    );

    // GCLOCK2.SHP — production progress clock overlay (separate texture).
    let (gclock_texture, gclock_frames) =
        match build_gclock_texture(gpu, batch, side_route, &generic_palette) {
            Some((tex, entries)) => (Some(tex), entries),
            None => {
                log::warn!(
                    "GCLOCK2.SHP not found in {:?} side route, progress overlay disabled",
                    side_route.archive_names
                );
                (None, Vec::new())
            }
        };

    // Collect all pieces to pack into the atlas.
    let mut all_entries: Vec<&RenderedChromeEntry> = vec![&radar, &side1, &side2, &side3];
    all_entries.extend(command_bar_art.entries());
    all_entries.extend(in_game_shell_art.entries());
    if let Some(ref top) = top_strip_left {
        all_entries.push(top);
    }
    if let Some(ref top) = top_strip_sidebar {
        all_entries.push(top);
    }
    if let Some(ref top) = top_strip_thin {
        all_entries.push(top);
    }
    if let Some(ref unknown) = unknown_top_housing {
        all_entries.push(unknown);
    }
    if let Some(ref unknown) = unknown_mid_panel {
        all_entries.push(unknown);
    }
    if let Some(ref bg) = background_large {
        all_entries.push(bg);
    }
    if let Some(ref bg) = background_medium {
        all_entries.push(bg);
    }
    if let Some(ref bg) = background_small {
        all_entries.push(bg);
    }
    if let Some(ref t) = tabs {
        all_entries.push(t);
    }
    for entry in tab_frame_entries.iter().flatten().flatten() {
        all_entries.push(entry);
    }
    for entry in repair_frame_entries.iter().flatten() {
        all_entries.push(entry);
    }
    for entry in sell_frame_entries.iter().flatten() {
        all_entries.push(entry);
    }
    for entry in top_button_entries.iter().flatten().flatten() {
        all_entries.push(entry);
    }
    for entry in scroll_up_entries.iter().flatten() {
        all_entries.push(entry);
    }
    for entry in scroll_down_entries.iter().flatten() {
        all_entries.push(entry);
    }
    if let Some(ref p) = power {
        all_entries.push(p);
    }
    for pf in &powerp_rendered {
        all_entries.push(pf);
    }
    for extra in &extra_rendered {
        all_entries.push(&extra.rendered);
    }

    let atlas_width = all_entries.iter().map(|e| e.width).max().unwrap_or(1);
    let atlas_height: u32 = all_entries
        .iter()
        .map(|e| e.height + CHROME_PADDING)
        .sum::<u32>();
    let mut rgba = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut y = 0u32;
    let command_bar = command_bar_art.map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let in_game_shell = in_game_shell_art.map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });

    let top_strip_left_uv = top_strip_left.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let top_strip_sidebar_uv = top_strip_sidebar.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let top_strip_thin_uv = top_strip_thin.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let unknown_top_housing_uv = unknown_top_housing.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let unknown_mid_panel_uv = unknown_mid_panel.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let background_large_uv = background_large.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let background_medium_uv = background_medium.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let background_small_uv = background_small.as_ref().map(|entry| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    });
    let radar_uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, &radar);
    y += radar.height + CHROME_PADDING;
    let side1_uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, &side1);
    y += side1.height + CHROME_PADDING;

    let tabs_uv = tabs.as_ref().map(|t| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, t);
        y += t.height + CHROME_PADDING;
        uv
    });

    let mut tab_frames_packed: [[Option<SidebarChromeEntry>; 5]; 4] = Default::default();
    for tab in 0..4 {
        for frame in 0..5 {
            if let Some(ref entry) = tab_frame_entries[tab][frame] {
                let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
                y += entry.height + CHROME_PADDING;
                tab_frames_packed[tab][frame] = Some(uv);
            }
        }
    }

    let side2_uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, &side2);
    y += side2.height + CHROME_PADDING;
    let side3_uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, &side3);
    y += side3.height + CHROME_PADDING;

    let mut repair_frames_packed: [Option<SidebarChromeEntry>; 5] = Default::default();
    for frame in 0..5 {
        if let Some(ref entry) = repair_frame_entries[frame] {
            let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
            y += entry.height + CHROME_PADDING;
            repair_frames_packed[frame] = Some(uv);
        }
    }
    let mut sell_frames_packed: [Option<SidebarChromeEntry>; 5] = Default::default();
    for frame in 0..5 {
        if let Some(ref entry) = sell_frame_entries[frame] {
            let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
            y += entry.height + CHROME_PADDING;
            sell_frames_packed[frame] = Some(uv);
        }
    }
    let top_button_frames = top_button_entries.each_ref().map(|frames| {
        frames.each_ref().map(|entry| {
            entry.as_ref().map(|entry| {
                let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
                y += entry.height + CHROME_PADDING;
                uv
            })
        })
    });
    let mut scroll_up_frames_packed: [Option<SidebarChromeEntry>; SCROLL_FRAME_COUNT] =
        Default::default();
    for frame in 0..SCROLL_FRAME_COUNT {
        if let Some(ref entry) = scroll_up_entries[frame] {
            let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
            y += entry.height + CHROME_PADDING;
            scroll_up_frames_packed[frame] = Some(uv);
        }
    }
    let mut scroll_down_frames_packed: [Option<SidebarChromeEntry>; SCROLL_FRAME_COUNT] =
        Default::default();
    for frame in 0..SCROLL_FRAME_COUNT {
        if let Some(ref entry) = scroll_down_entries[frame] {
            let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
            y += entry.height + CHROME_PADDING;
            scroll_down_frames_packed[frame] = Some(uv);
        }
    }
    let power_uv = power.as_ref().map(|p| {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, p);
        y += p.height + CHROME_PADDING;
        uv
    });
    let mut powerp_uvs: [Option<SidebarChromeEntry>; 5] = [None; 5];
    for (i, pf) in powerp_rendered.iter().enumerate() {
        if i < 5 {
            let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, pf);
            y += pf.height + CHROME_PADDING;
            powerp_uvs[i] = Some(uv);
        }
    }
    let mut extra_entries = Vec::with_capacity(extra_rendered.len());
    for extra in &extra_rendered {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, &extra.rendered);
        y += extra.rendered.height + CHROME_PADDING;
        extra_entries.push(SidebarChromeExtraEntry {
            id: extra.id,
            label: extra.label.clone(),
            frame_count: extra.frame_count,
            entry: uv,
        });
    }

    log::info!(
        "Sidebar chrome atlas for {}: {}x{} px, {} pieces",
        mix_name,
        atlas_width,
        atlas_height,
        all_entries.len()
    );
    log::info!(
        "  radar={}x{} side1={}x{} tabs={} side2={}x{} side3={}x{}",
        radar.width,
        radar.height,
        side1.width,
        side1.height,
        tabs.as_ref()
            .map(|t| format!("{}x{}", t.width, t.height))
            .unwrap_or("none".into()),
        side2.width,
        side2.height,
        side3.width,
        side3.height,
    );
    for (tab, tab_entries) in tab_frame_entries.iter().enumerate() {
        if let Some(ref entry) = tab_entries[0] {
            log::info!("  tab0{} (5 frames): {}x{}", tab, entry.width, entry.height);
        }
    }

    let texture = batch.create_texture(gpu, &rgba, atlas_width, atlas_height);
    Some(SidebarChromeAtlas {
        source_identity,
        texture,
        command_bar,
        in_game_shell,
        top_strip_left: top_strip_left_uv,
        top_strip_sidebar: top_strip_sidebar_uv,
        top_strip_thin: top_strip_thin_uv,
        unknown_top_housing: unknown_top_housing_uv,
        unknown_mid_panel: unknown_mid_panel_uv,
        background_large: background_large_uv,
        background_medium: background_medium_uv,
        background_small: background_small_uv,
        radar_frames,
        radar_frame_size,
        radar_content_insets,
        radar: radar_uv,
        side1: side1_uv,
        tabs: tabs_uv,
        tab_frames: tab_frames_packed,
        side2: side2_uv,
        side3: side3_uv,
        repair_frames: repair_frames_packed,
        sell_frames: sell_frames_packed,
        top_button_frames,
        scroll_up_frames: scroll_up_frames_packed,
        scroll_down_frames: scroll_down_frames_packed,
        power: power_uv,
        powerp_frames: powerp_uvs,
        extra_entries,
        gclock_texture,
        gclock_frames,
        darken: SidebarDarkenMask::load(gpu, batch, asset_manager, &generic_palette),
    })
}

struct RenderedChromeEntry {
    rgba: Vec<u8>,
    indices: Vec<u8>,
    width: u32,
    height: u32,
}

struct ExtraRenderedEntry {
    id: i32,
    label: String,
    frame_count: usize,
    rendered: RenderedChromeEntry,
}

fn known_loose_piece_ids(radar_name: &str) -> std::collections::HashSet<i32> {
    let mut ids = std::collections::HashSet::new();
    ids.insert(mix_hash(radar_name));
    ids.insert(mix_hash("side1.shp"));
    ids.insert(mix_hash("tabs.shp"));
    ids.insert(mix_hash("tab00.shp"));
    ids.insert(mix_hash("tab01.shp"));
    ids.insert(mix_hash("tab02.shp"));
    ids.insert(mix_hash("tab03.shp"));
    ids.insert(mix_hash("side2.shp"));
    ids.insert(mix_hash("side3.shp"));
    ids.insert(mix_hash("repair.shp"));
    ids.insert(mix_hash("sell.shp"));
    ids.insert(mix_hash("powerp.shp"));
    ids.insert(TOP_STRIP_SIDEBAR_ID);
    ids.insert(TOP_STRIP_THIN_ID);
    ids.insert(UNKNOWN_TOP_HOUSING_ID);
    ids.insert(UNKNOWN_MID_PANEL_ID);
    ids
}

fn collect_extra_entries(
    mix: &MixArchive,
    palette: &Palette,
    _tabs_palette: &Palette,
    _background_names: Option<(&str, &str, &str)>,
    excluded_ids: &std::collections::HashSet<i32>,
) -> Vec<ExtraRenderedEntry> {
    let mut entries = Vec::new();
    for mix_entry in mix.entries() {
        if excluded_ids.contains(&mix_entry.id) {
            continue;
        }
        let Some(shp_bytes) = mix.get_by_id(mix_entry.id) else {
            continue;
        };
        let Ok(shp) = ShpFile::from_bytes(shp_bytes) else {
            continue;
        };
        let Some(rendered) = render_shp(&shp, palette, 0) else {
            continue;
        };
        entries.push(ExtraRenderedEntry {
            id: mix_entry.id,
            label: format!(
                "id {:#010X} ({}x{}, {}f)",
                mix_entry.id as u32,
                shp.width,
                shp.height,
                shp.frames.len()
            ),
            frame_count: shp.frames.len(),
            rendered,
        });
    }
    entries
}

/// Pre-render all frames of radar.shp to RGBA buffers for animation.
/// Returns (Vec of RGBA buffers, [width, height], content insets).
/// Content insets come from the native fixed radar aperture.
fn render_all_radar_frames(
    asset_manager: &AssetManager,
    mix: &MixArchive,
    radar_name: &str,
    palette: &Palette,
) -> (Vec<Vec<u8>>, [u32; 2], [u32; 4]) {
    #[cfg(feature = "ares")]
    let bytes = {
        let _ = mix;
        asset_manager.get_ref(radar_name)
    };
    #[cfg(not(feature = "ares"))]
    let bytes = mix
        .get_by_name(radar_name)
        .or_else(|| asset_manager.get_ref(radar_name));
    let shp_bytes = match bytes {
        Some(b) => b,
        None => return (Vec::new(), [0, 0], [0; 4]),
    };
    let shp = match ShpFile::from_bytes(shp_bytes) {
        Ok(s) => s,
        Err(_) => return (Vec::new(), [0, 0], [0; 4]),
    };
    let frame_count: usize = shp.frames.len();
    let canvas_w: u32 = shp.width as u32;
    let canvas_h: u32 = shp.height as u32;

    // Preserve each frame's skipped pixels. The animation surface owns backing
    // history; aperture geometry comes from Radar One_Time/Init_For_House.
    let content_insets = [16, 1, 12, 1];
    let frames: Vec<Vec<u8>> = (0..frame_count)
        .map(|i| {
            render_shp(&shp, palette, i)
                .map(|entry| entry.rgba)
                .unwrap_or_else(|| vec![0u8; (canvas_w * canvas_h * 4) as usize])
        })
        .collect();
    log::info!(
        "Pre-rendered {} radar animation frames ({}x{}, content insets: l={} t={} r={} b={})",
        frames.len(),
        canvas_w,
        canvas_h,
        content_insets[0],
        content_insets[1],
        content_insets[2],
        content_insets[3],
    );
    (frames, [canvas_w, canvas_h], content_insets)
}

fn render_shp(shp: &ShpFile, palette: &Palette, frame_index: usize) -> Option<RenderedChromeEntry> {
    if frame_index >= shp.frames.len() {
        return None;
    }
    let frame = &shp.frames[frame_index];
    let frame_rgba = shp.frame_to_rgba(frame_index, palette).ok()?;
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let fw = frame.frame_width as u32;
    let fh = frame.frame_height as u32;
    let fx = frame.frame_x as u32;
    let fy = frame.frame_y as u32;
    if canvas_w == fw && canvas_h == fh && fx == 0 && fy == 0 {
        return Some(RenderedChromeEntry {
            rgba: frame_rgba,
            indices: frame.pixels.clone(),
            width: canvas_w,
            height: canvas_h,
        });
    }
    let mut canvas = vec![0u8; (canvas_w * canvas_h * 4) as usize];
    let mut indices = vec![0u8; (canvas_w * canvas_h) as usize];
    for row in 0..fh {
        let src_off = (row * fw * 4) as usize;
        let dst_off = (((fy + row) * canvas_w + fx) * 4) as usize;
        let len = (fw * 4) as usize;
        if src_off + len <= frame_rgba.len() && dst_off + len <= canvas.len() {
            canvas[dst_off..dst_off + len].copy_from_slice(&frame_rgba[src_off..src_off + len]);
            indices[dst_off / 4..(dst_off + len) / 4]
                .copy_from_slice(&frame.pixels[src_off / 4..(src_off + len) / 4]);
        }
    }
    Some(RenderedChromeEntry {
        rgba: canvas,
        indices,
        width: canvas_w,
        height: canvas_h,
    })
}

/// Resolve a binary-proven generic sidebar role only through the active side
/// archive chain. There is deliberately no global cross-side fallback.
fn render_side_entry(
    route: SidebarSideRoute<'_>,
    shp_name: &str,
    palette: &Palette,
    frame_index: usize,
) -> Option<RenderedChromeEntry> {
    let resolved = route.resolve_generic_shp(shp_name)?;
    let shp = ShpFile::from_bytes(resolved.bytes).ok()?;
    render_shp(&shp, palette, frame_index)
}

/// Preserve the existing theme-specific direct lookup domain for radar,
/// backgrounds, unverified pieces, and extras.
fn render_entry(
    asset_manager: &AssetManager,
    mix: &MixArchive,
    shp_name: &str,
    palette: &Palette,
    frame_index: usize,
) -> Option<RenderedChromeEntry> {
    #[cfg(feature = "ares")]
    let shp_bytes = {
        let _ = mix;
        asset_manager.get_ref(shp_name)?
    };
    #[cfg(not(feature = "ares"))]
    let shp_bytes = mix
        .get_by_name(shp_name)
        .or_else(|| asset_manager.get_ref(shp_name))?;
    let shp = ShpFile::from_bytes(shp_bytes).ok()?;
    render_shp(&shp, palette, frame_index)
}

fn render_entry_by_id(
    mix: &MixArchive,
    id: i32,
    palette: &Palette,
    frame_index: usize,
) -> Option<RenderedChromeEntry> {
    let shp_bytes = mix.get_by_id(id)?;
    let shp = ShpFile::from_bytes(shp_bytes).ok()?;
    render_shp(&shp, palette, frame_index)
}

fn blit_entry(
    atlas_rgba: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    dst_y: u32,
    entry: &RenderedChromeEntry,
) -> SidebarChromeEntry {
    for row in 0..entry.height {
        let src_start = (row * entry.width * 4) as usize;
        let dst_row = dst_y + row;
        if dst_row >= atlas_height {
            break;
        }
        let dst_start = ((dst_row * atlas_width) * 4) as usize;
        let len = (entry.width * 4) as usize;
        if src_start + len <= entry.rgba.len() && dst_start + len <= atlas_rgba.len() {
            atlas_rgba[dst_start..dst_start + len]
                .copy_from_slice(&entry.rgba[src_start..src_start + len]);
        }
    }

    SidebarChromeEntry {
        uv_origin: [0.0, dst_y as f32 / atlas_height as f32],
        uv_size: [
            entry.width as f32 / atlas_width as f32,
            entry.height as f32 / atlas_height as f32,
        ],
        pixel_size: [entry.width as f32, entry.height as f32],
    }
}

#[cfg(test)]
#[path = "sidebar_chrome_tests.rs"]
mod tests;

#[cfg(test)]
mod provenance_tests {
    use super::{SidebarTheme, select_sidebar_theme};

    #[cfg(feature = "ares")]
    #[test]
    fn ares_sidebar_filename_switch_matches_native_independent_side_controls() {
        let corpus = crate::test_fixture::json("tools/rules_oracle/ares_side_sidebar.json");
        for row in corpus["selector"]["rows"].as_array().unwrap() {
            let flag = row["input"]["yuri_file_names"].as_bool().unwrap();
            let names = super::SidebarArtNames::for_yuri(flag);
            let expected = &corpus["filename_sets"][if flag { "yuri" } else { "plain" }];
            assert_eq!(names.radar.to_ascii_uppercase(), expected["radar"]);
            assert_eq!(
                names.radar_palette.to_ascii_uppercase(),
                expected["radar_palette"]
            );
            assert_eq!(
                names.background_palette.to_ascii_uppercase(),
                expected["background_palette"]
            );
            for (actual, expected) in [
                names.backgrounds.0,
                names.backgrounds.1,
                names.backgrounds.2,
            ]
            .iter()
            .zip(expected["backgrounds"].as_array().unwrap())
            {
                assert_eq!(actual.to_ascii_uppercase(), expected.as_str().unwrap());
            }
        }
    }

    #[test]
    fn atlas_resolution_reports_actual_fallback_without_changing_order() {
        let allied_source = "sidec01.mix";
        let soviet_source = "sidec02.mix";
        let yuri_source = "sidec02md.mix";

        let (actual, source) = select_sidebar_theme(
            SidebarTheme::Yuri,
            Some(&allied_source),
            Some(&soviet_source),
            Some(&yuri_source),
        )
        .expect("Yuri atlas should resolve");
        assert_eq!(actual, SidebarTheme::Yuri);
        assert_eq!(*source, yuri_source);

        let (actual, source) = select_sidebar_theme(
            SidebarTheme::Yuri,
            Some(&allied_source),
            Some(&soviet_source),
            None,
        )
        .expect("Yuri should fall back to Soviet before Allied");
        assert_eq!(actual, SidebarTheme::Soviet);
        assert_eq!(*source, soviet_source);

        let (actual, source) =
            select_sidebar_theme(SidebarTheme::Soviet, Some(&allied_source), None, None)
                .expect("Soviet should fall back to Allied");
        assert_eq!(actual, SidebarTheme::Allied);
        assert_eq!(*source, allied_source);

        assert!(
            select_sidebar_theme(
                SidebarTheme::Allied,
                None,
                Some(&soviet_source),
                Some(&yuri_source),
            )
            .is_none(),
            "Allied must not reverse-fallback to another faction",
        );
    }
}
