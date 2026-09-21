//! Native standard Skirmish loading-screen chrome.
//!
//! Loads verified `0x00552D60` LS country art and the `PROGBARM.SHP` frame-0
//! progress source into a batch texture. Callers may also supply a prepared
//! selected-map preview and the distinct house-color ramps needed to decode
//! retail `mmpb.shp` marker variants.

use std::collections::{BTreeMap, HashMap};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;

const ATLAS_PADDING: u32 = 2;

/// Synthetic atlas label for the 1x1 white texel used to draw the solid backing
/// fill (G3) as a tinted quad. Not an asset on disk.
const SOLID_TEXEL_LABEL: &str = "__solid_texel__";

/// Synthetic label for the caller-prepared selected-map preview.
const LOADING_PREVIEW_LABEL: &str = "__selected_map_preview__";

const MMPB_ASSET_NAME: &str = "mmpb.shp";
const MMPB_MARKER_LABEL_PREFIX: &str = "__mmpb_marker_color_";

/// Native converts this RGB key to the active DirectDraw format before blitting
/// the standard Skirmish country insignia.
const SIDE_ICON_TRANSPARENT_RGB: [u8; 3] = [255, 0, 255];

/// Native screen widths that select dedicated marker projection rectangles.
const MMPB_REGION_WIDTH_800: u32 = 800;
const MMPB_REGION_WIDTH_1024: u32 = 1024;

/// The one screen width that selects the narrow (`ls640*`) loading art. The app
/// layer keys the matching text table and progress-row origin off the same
/// width; this module keeps its own copy so `render` stays free of app imports.
const NARROW_LOADING_SCREEN_WIDTH: u32 = 640;

/// Fixed `(x, y, width, height)` region used to composite the selected-map
/// preview and `mmpb.shp` frame-0 markers onto the loading background.
///
/// These four values are the verified, screen-width-keyed region constants
/// in loading-destination pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmpbRegionRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Per-axis screen nudge applied to a projected marker before the region origin
/// is added (verified marker projection: X gets `-3`, Y gets `-2`).
pub const MMPB_MARKER_NUDGE_X: i32 = -3;
pub const MMPB_MARKER_NUDGE_Y: i32 = -2;

/// Select the marker projection region rect for the current screen width.
///
/// Native uses equality for its dedicated 800 and 1024 branches. Every other
/// width uses the 640-style fallback; the rectangles are never interpolated.
pub fn mmpb_region_rect(render_width: u32) -> MmpbRegionRect {
    match render_width {
        MMPB_REGION_WIDTH_800 => MmpbRegionRect {
            x: 499,
            y: 379,
            width: 216,
            height: 166,
        },
        MMPB_REGION_WIDTH_1024 => MmpbRegionRect {
            x: 570,
            y: 424,
            width: 300,
            height: 260,
        },
        _ => MmpbRegionRect {
            x: 385,
            y: 270,
            width: 200,
            height: 200,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingScreenWidth {
    W640,
    W800,
}

impl LoadingScreenWidth {
    /// Native selects the 640 art by comparing the screen width for equality
    /// with 640; every other width, wider or narrower, takes the 800 art.
    pub fn for_render_width(width: u32) -> Self {
        if width == NARROW_LOADING_SCREEN_WIDTH {
            Self::W640
        } else {
            Self::W800
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::W640 => "640",
            Self::W800 => "800",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingArtVariant {
    Yuri,
    Observer,
    Americans,
    Russians,
    Africans,
    Alliance,
    Arabs,
    Germans,
    French,
    Confederation,
    British,
}

impl LoadingArtVariant {
    #[cfg(test)]
    pub fn from_country_name(country: &str) -> Option<Self> {
        match country.to_ascii_lowercase().as_str() {
            "yuricountry" | "yuri" => Some(Self::Yuri),
            "observer" | "obs" => Some(Self::Observer),
            "americans" | "usa" | "ustates" => Some(Self::Americans),
            "russians" | "russia" => Some(Self::Russians),
            "africans" | "libya" | "lybia" => Some(Self::Africans),
            "alliance" | "korea" => Some(Self::Alliance),
            "arabs" | "iraq" => Some(Self::Arabs),
            "germans" | "germany" => Some(Self::Germans),
            "french" | "france" => Some(Self::French),
            "confederation" | "cuba" => Some(Self::Confederation),
            "british" | "ukingdom" => Some(Self::British),
            _ => None,
        }
    }

    /// Country insignia PCX drawn to the right of the loading bar (G4).
    ///
    /// Verified asset mapping from gamemd's loading-side-icon resolver
    /// (`FUN_004e3560`): each launch country maps to a 4-letter insignia PCX.
    pub fn side_icon_pcx(self) -> &'static str {
        match self {
            Self::Americans => "usai.pcx",
            Self::Alliance => "japi.pcx",
            Self::French => "frai.pcx",
            Self::Germans => "geri.pcx",
            Self::British => "gbri.pcx",
            Self::Africans => "djbi.pcx",
            Self::Arabs => "arbi.pcx",
            Self::Confederation => "lati.pcx",
            Self::Russians => "rusi.pcx",
            Self::Yuri => "yrii.pcx",
            Self::Observer => "obsi.pcx",
        }
    }

    pub fn manifest(self) -> LoadingArtManifest {
        match self {
            Self::Yuri => LoadingArtManifest::new("yuri", "MPYLS.PAL"),
            Self::Observer => LoadingArtManifest::new("obs", "MPLSOBS.PAL"),
            Self::Americans => LoadingArtManifest::new("ustates", "MPLSU.PAL"),
            Self::Russians => LoadingArtManifest::new("russia", "MPLSR.PAL"),
            Self::Africans => LoadingArtManifest::new("libya", "MPLSL.PAL"),
            Self::Alliance => LoadingArtManifest::new("korea", "MPLSK.PAL"),
            Self::Arabs => LoadingArtManifest::new("iraq", "MPLSI.PAL"),
            Self::Germans => LoadingArtManifest::new("germany", "MPLSG.PAL"),
            Self::French => LoadingArtManifest::new("france", "MPLSF.PAL"),
            Self::Confederation => LoadingArtManifest::new("cuba", "MPLSC.PAL"),
            Self::British => LoadingArtManifest::new("ukingdom", "MPLSUK.PAL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadingArtManifest {
    pub country_token: &'static str,
    pub palette_name: &'static str,
}

impl LoadingArtManifest {
    const fn new(country_token: &'static str, palette_name: &'static str) -> Self {
        Self {
            country_token,
            palette_name,
        }
    }

    pub fn background_asset(self, width: LoadingScreenWidth) -> String {
        format!("ls{}{}.shp", width.prefix(), self.country_token)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadingScreenEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

/// CPU-side selected-map preview prepared by the loading composition owner.
///
/// Invalid dimensions or byte length omit the optional preview and marker
/// layers without affecting mandatory loading chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedLoadingPreviewRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One distinct assigned-house color variant requested for retail
/// `mmpb.shp` frame 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmpbMarkerRemap {
    /// Stable scheme-entry / `HouseColorIndex` byte supplied by app composition.
    pub color_key: u8,
    /// The 16-color house ramp replacing palette indices 16..31.
    pub ramp: [Color; 16],
}

/// Optional selected-map layers packed alongside mandatory loading chrome.
#[derive(Debug, Clone, Copy)]
pub struct LoadingScreenCompositionAtlasInput<'a> {
    pub preview: &'a PreparedLoadingPreviewRgba,
    pub marker_remaps: &'a [MmpbMarkerRemap],
}

pub struct LoadingScreenAtlas {
    pub texture: BatchTexture,
    pub background: LoadingScreenEntry,
    pub progress_frame0: LoadingScreenEntry,
    /// Caller-prepared selected-map preview, when its RGBA payload is valid.
    pub preview: Option<LoadingScreenEntry>,
    /// Retail `mmpb.shp` frame 0 decoded for each distinct assigned-house color.
    pub mmpb_markers: BTreeMap<u8, LoadingScreenEntry>,
    /// Country insignia drawn right of the bar (G4). `None` when the PCX is
    /// missing — the bar still draws without it.
    pub side_icon: Option<LoadingScreenEntry>,
    /// 1x1 white texel for drawing the solid backing fill (G3) as a tinted quad.
    pub solid_texel: LoadingScreenEntry,
}

struct RenderedLoadingEntry {
    label: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Build the native loading atlas with optional selected-map preview/marker
/// layers.
///
/// Background and progress assets remain mandatory. Invalid preview input,
/// missing `mmpb.shp`, or an unavailable color variant only removes optional
/// layers.
pub fn build_loading_screen_atlas_with_composition(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    variant: LoadingArtVariant,
    width: LoadingScreenWidth,
    progress_ramp: &[Color; 16],
    composition: Option<LoadingScreenCompositionAtlasInput<'_>>,
) -> Option<LoadingScreenAtlas> {
    let manifest = variant.manifest();
    let background_name = manifest.background_asset(width);
    let background_palette = load_named_ui_palette(assets, manifest.palette_name)?;
    let remap_base_palette = load_named_ui_palette(assets, "MPLS.PAL")
        .or_else(|| load_named_ui_palette(assets, manifest.palette_name))?;
    let progress_palette = progress_palette_with_player_ramp(&remap_base_palette, progress_ramp);

    let mut rendered = vec![
        mandatory_shp(
            assets,
            &background_name,
            &background_palette,
            0,
            manifest.palette_name,
        )?,
        mandatory_shp(assets, "PROGBARM.SHP", &progress_palette, 0, "MPLS.PAL")?,
        solid_texel_entry(),
    ];

    let (preview_label, marker_labels) = append_optional_composition_entries(
        assets,
        &remap_base_palette,
        composition,
        &mut rendered,
    );

    // Side-icon is non-fatal: if its PCX is missing or malformed the bar still
    // draws. Record its label so it can be looked up after packing.
    let side_icon_name = variant.side_icon_pcx();
    let side_icon_label = match render_pcx_entry(assets, side_icon_name) {
        Some(entry) => {
            let label = entry.label.clone();
            rendered.push(entry);
            Some(label)
        }
        None => {
            log::warn!("Missing standard Skirmish loading side icon {side_icon_name}");
            None
        }
    };

    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    let by_label: HashMap<String, LoadingScreenEntry> = rendered
        .iter()
        .map(|entry| entry.label.clone())
        .zip(packed)
        .collect();
    let side_icon = side_icon_label.and_then(|label| by_label.get(&label).copied());
    let (preview, mmpb_markers) =
        lookup_optional_atlas_entries(&by_label, preview_label.as_deref(), &marker_labels);
    Some(LoadingScreenAtlas {
        texture,
        background: *by_label.get(&background_name.to_ascii_lowercase())?,
        progress_frame0: *by_label.get("progbarm.shp")?,
        preview,
        mmpb_markers,
        side_icon,
        solid_texel: *by_label.get(SOLID_TEXEL_LABEL)?,
    })
}

fn progress_palette_with_player_ramp(palette: &Palette, progress_ramp: &[Color; 16]) -> Palette {
    palette.with_house_colors(progress_ramp)
}

fn append_optional_composition_entries(
    assets: &AssetManager,
    remap_base_palette: &Palette,
    composition: Option<LoadingScreenCompositionAtlasInput<'_>>,
    rendered: &mut Vec<RenderedLoadingEntry>,
) -> (Option<String>, Vec<(u8, String)>) {
    let Some(composition) = composition else {
        return (None, Vec::new());
    };
    let Some(preview_entry) = prepared_preview_entry(composition.preview) else {
        return (None, Vec::new());
    };

    let preview_label = preview_entry.label.clone();
    rendered.push(preview_entry);

    if composition.marker_remaps.is_empty() {
        return (Some(preview_label), Vec::new());
    }
    if assets.load_file_from_mix(MMPB_ASSET_NAME).is_none() {
        log::warn!(
            "Missing optional standard Skirmish loading marker asset {MMPB_ASSET_NAME}; preview remains available"
        );
        return (Some(preview_label), Vec::new());
    }

    let mut distinct_remaps = BTreeMap::new();
    for remap in composition.marker_remaps {
        distinct_remaps
            .entry(remap.color_key)
            .or_insert(&remap.ramp);
    }

    let mut marker_labels = Vec::with_capacity(distinct_remaps.len());
    for (color_key, ramp) in distinct_remaps {
        let palette = remap_base_palette.with_house_colors(ramp);
        let Some(mut marker_entry) = render_shp_entry(assets, MMPB_ASSET_NAME, &palette, 0) else {
            log::warn!(
                "Could not prepare optional standard Skirmish loading marker color {color_key}"
            );
            continue;
        };
        let label = mmpb_marker_label(color_key);
        marker_entry.label.clone_from(&label);
        rendered.push(marker_entry);
        marker_labels.push((color_key, label));
    }

    (Some(preview_label), marker_labels)
}

fn prepared_preview_entry(preview: &PreparedLoadingPreviewRgba) -> Option<RenderedLoadingEntry> {
    let expected_len = preview
        .width
        .checked_mul(preview.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok());
    if preview.width == 0 || preview.height == 0 || expected_len != Some(preview.rgba.len()) {
        log::warn!(
            "Invalid optional standard Skirmish loading preview: {}x{} with {} RGBA bytes",
            preview.width,
            preview.height,
            preview.rgba.len()
        );
        return None;
    }
    Some(RenderedLoadingEntry {
        label: LOADING_PREVIEW_LABEL.to_string(),
        width: preview.width,
        height: preview.height,
        rgba: preview.rgba.clone(),
    })
}

fn mmpb_marker_label(color_key: u8) -> String {
    format!("{MMPB_MARKER_LABEL_PREFIX}{color_key}")
}

fn lookup_optional_atlas_entries(
    by_label: &HashMap<String, LoadingScreenEntry>,
    preview_label: Option<&str>,
    marker_labels: &[(u8, String)],
) -> (Option<LoadingScreenEntry>, BTreeMap<u8, LoadingScreenEntry>) {
    let preview = preview_label.and_then(|label| by_label.get(label).copied());
    let markers = marker_labels
        .iter()
        .filter_map(|(color_key, label)| {
            by_label
                .get(label)
                .copied()
                .map(|entry| (*color_key, entry))
        })
        .collect();
    (preview, markers)
}

/// A 1x1 opaque-white texel. Drawn scaled and tinted to produce the G3 solid
/// backing fill; tinting an all-white texel yields a flat color rect.
fn solid_texel_entry() -> RenderedLoadingEntry {
    RenderedLoadingEntry {
        label: SOLID_TEXEL_LABEL.to_string(),
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 255],
    }
}

/// Decode a country-insignia PCX into an atlas entry using the native
/// RGB-magenta transparency key.
fn render_pcx_entry(assets: &AssetManager, file_name: &str) -> Option<RenderedLoadingEntry> {
    let bytes = assets.get_ref(file_name)?;
    let pcx = PcxFile::from_bytes(bytes)
        .map_err(|err| {
            log::warn!("Could not parse standard Skirmish loading side icon {file_name}: {err:#}");
            err
        })
        .ok()?;
    let mut rgba = pcx.to_rgba(None);
    ACTIVE_RETAIL_RGB565_PRESENTATION
        .apply_packed_color_key_rgba8(&mut rgba, SIDE_ICON_TRANSPARENT_RGB);
    Some(RenderedLoadingEntry {
        label: file_name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
}

fn load_named_ui_palette(assets: &AssetManager, name: &str) -> Option<Palette> {
    let Some(bytes) = assets.get_ref(name) else {
        log::warn!("Missing standard Skirmish loading palette {name}");
        return None;
    };
    Palette::from_bytes_gamemd_ui(bytes)
        .map_err(|err| {
            log::warn!("Could not parse standard Skirmish loading palette {name}: {err:#}");
            err
        })
        .ok()
}

fn mandatory_shp(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
    palette_name: &str,
) -> Option<RenderedLoadingEntry> {
    render_shp_entry(assets, file_name, palette, frame).or_else(|| {
        log::warn!(
            "Missing mandatory standard Skirmish loading asset {file_name} frame {frame} decoded with {palette_name}"
        );
        None
    })
}

fn render_shp_entry(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
) -> Option<RenderedLoadingEntry> {
    let Some(load) = assets.load_file_from_mix(file_name) else {
        log::warn!("Missing standard Skirmish loading SHP {file_name}");
        return None;
    };
    let shp = ShpFile::from_bytes(&load.bytes)
        .map_err(|err| {
            log::warn!("Could not parse standard Skirmish loading SHP {file_name}: {err:#}");
            err
        })
        .ok()?;
    if frame >= shp.frames.len() {
        log::warn!(
            "Standard Skirmish loading SHP {file_name} missing frame {frame}; frame count {}",
            shp.frames.len()
        );
        return None;
    }
    let frame_rgba = shp
        .frame_to_rgba_ui(frame, palette)
        .map_err(|err| {
            log::warn!(
                "Could not decode standard Skirmish loading SHP {file_name} frame {frame}: {err:#}"
            );
            err
        })
        .ok()?;
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let shp_frame = &shp.frames[frame];
    let frame_w = shp_frame.frame_width as u32;
    let frame_h = shp_frame.frame_height as u32;
    let frame_x = shp_frame.frame_x as u32;
    let frame_y = shp_frame.frame_y as u32;
    let rgba = if frame_w == canvas_w && frame_h == canvas_h && frame_x == 0 && frame_y == 0 {
        frame_rgba
    } else {
        let mut canvas = vec![0u8; (canvas_w * canvas_h * 4) as usize];
        for row in 0..frame_h {
            let src = (row * frame_w * 4) as usize;
            let dst = (((frame_y + row) * canvas_w + frame_x) * 4) as usize;
            let len = (frame_w * 4) as usize;
            if src + len <= frame_rgba.len() && dst + len <= canvas.len() {
                canvas[dst..dst + len].copy_from_slice(&frame_rgba[src..src + len]);
            }
        }
        canvas
    };
    Some(RenderedLoadingEntry {
        label: file_name.to_ascii_lowercase(),
        width: canvas_w,
        height: canvas_h,
        rgba,
    })
}

fn pack_entries(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    entries: &[RenderedLoadingEntry],
) -> Option<(BatchTexture, Vec<LoadingScreenEntry>)> {
    if entries.is_empty() {
        return None;
    }

    let atlas_width = entries
        .iter()
        .map(|entry| entry.width)
        .max()
        .unwrap_or(1)
        .max(1024);
    let mut x = 0u32;
    let mut y = 0u32;
    let mut row_h = 0u32;
    let mut placements = Vec::with_capacity(entries.len());

    for entry in entries {
        if x > 0 && x + entry.width + ATLAS_PADDING > atlas_width {
            x = 0;
            y += row_h + ATLAS_PADDING;
            row_h = 0;
        }
        placements.push((x, y));
        x += entry.width + ATLAS_PADDING;
        row_h = row_h.max(entry.height);
    }

    let atlas_height = (y + row_h).next_power_of_two().max(1);
    let mut rgba = vec![0u8; (atlas_width * atlas_height * 4) as usize];

    for (entry, (px, py)) in entries.iter().zip(placements.iter().copied()) {
        for row in 0..entry.height {
            let src = (row * entry.width * 4) as usize;
            let dst = (((py + row) * atlas_width + px) * 4) as usize;
            let len = (entry.width * 4) as usize;
            rgba[dst..dst + len].copy_from_slice(&entry.rgba[src..src + len]);
        }
    }

    log::info!(
        "Standard Skirmish loading atlas: {}x{} px, {} pieces",
        atlas_width,
        atlas_height,
        entries.len()
    );

    let texture = batch.create_texture(gpu, &rgba, atlas_width, atlas_height);
    let atlas_entries = entries
        .iter()
        .zip(placements)
        .map(|(entry, (px, py))| LoadingScreenEntry {
            uv_origin: [
                px as f32 / atlas_width as f32,
                py as f32 / atlas_height as f32,
            ],
            uv_size: [
                entry.width as f32 / atlas_width as f32,
                entry.height as f32 / atlas_height as f32,
            ],
            pixel_size: [entry.width as f32, entry.height as f32],
        })
        .collect();
    Some((texture, atlas_entries))
}

#[cfg(test)]
mod tests {
    use super::{
        LOADING_PREVIEW_LABEL, LoadingArtVariant, LoadingScreenEntry, LoadingScreenWidth,
        MmpbRegionRect, PreparedLoadingPreviewRgba, lookup_optional_atlas_entries,
        mmpb_marker_label, mmpb_region_rect, prepared_preview_entry,
        progress_palette_with_player_ramp, render_pcx_entry, render_shp_entry,
    };
    use crate::assets::pal_file::{Color, Palette};
    use std::collections::HashMap;

    #[test]
    fn progress_palette_replaces_only_the_house_color_band() {
        let mut colors = [Color::rgb(3, 4, 5); 256];
        colors[16] = Color::rgb(7, 8, 9);
        colors[31] = Color::rgb(10, 11, 12);
        let palette = Palette { colors };
        let ramp = std::array::from_fn(|index| {
            Color::rgb(index as u8, (index as u8) + 32, (index as u8) + 64)
        });

        let remapped = progress_palette_with_player_ramp(&palette, &ramp);

        assert_eq!(&remapped.colors[16..32], &ramp);
        assert_eq!(remapped.colors[15], palette.colors[15]);
        assert_eq!(remapped.colors[32], palette.colors[32]);
    }

    #[test]
    fn mmpb_region_rect_uses_exact_native_width_equality() {
        assert_eq!(
            mmpb_region_rect(640),
            MmpbRegionRect {
                x: 385,
                y: 270,
                width: 200,
                height: 200,
            }
        );
        assert_eq!(
            mmpb_region_rect(800),
            MmpbRegionRect {
                x: 499,
                y: 379,
                width: 216,
                height: 166,
            }
        );
        assert_eq!(
            mmpb_region_rect(1024),
            MmpbRegionRect {
                x: 570,
                y: 424,
                width: 300,
                height: 260,
            }
        );
        assert_eq!(mmpb_region_rect(799), mmpb_region_rect(640));
        assert_eq!(mmpb_region_rect(801), mmpb_region_rect(640));
        assert_eq!(mmpb_region_rect(1023), mmpb_region_rect(640));
        assert_eq!(mmpb_region_rect(1920), mmpb_region_rect(640));
    }

    #[test]
    fn prepared_preview_entry_validates_rgba_shape() {
        let valid = PreparedLoadingPreviewRgba {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let entry = prepared_preview_entry(&valid).expect("valid preview");
        assert_eq!(entry.label, LOADING_PREVIEW_LABEL);
        assert_eq!((entry.width, entry.height), (2, 1));
        assert_eq!(entry.rgba, valid.rgba);

        let invalid = PreparedLoadingPreviewRgba {
            width: 2,
            height: 1,
            rgba: vec![0; 4],
        };
        assert!(prepared_preview_entry(&invalid).is_none());
    }

    #[test]
    fn optional_entry_bookkeeping_keeps_marker_colors_distinct() {
        let preview = LoadingScreenEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [0.25, 0.25],
            pixel_size: [200.0, 100.0],
        };
        let red = LoadingScreenEntry {
            uv_origin: [0.25, 0.0],
            uv_size: [0.02, 0.02],
            pixel_size: [12.0, 12.0],
        };
        let blue = LoadingScreenEntry {
            uv_origin: [0.27, 0.0],
            uv_size: [0.02, 0.02],
            pixel_size: [12.0, 12.0],
        };
        let red_label = mmpb_marker_label(3);
        let blue_label = mmpb_marker_label(11);
        let by_label = HashMap::from([
            (LOADING_PREVIEW_LABEL.to_string(), preview),
            (red_label.clone(), red),
            (blue_label.clone(), blue),
        ]);

        let (resolved_preview, markers) = lookup_optional_atlas_entries(
            &by_label,
            Some(LOADING_PREVIEW_LABEL),
            &[
                (3, red_label),
                (11, blue_label),
                (19, mmpb_marker_label(19)),
            ],
        );

        assert_eq!(resolved_preview, Some(preview));
        assert_eq!(markers.get(&3), Some(&red));
        assert_eq!(markers.get(&11), Some(&blue));
        assert!(!markers.contains_key(&19));
    }

    #[test]
    fn loading_art_manifest_uses_verified_binary_string_table_names() {
        let cases = [
            (
                LoadingArtVariant::Yuri,
                "ls640yuri.shp",
                "ls800yuri.shp",
                "MPYLS.PAL",
            ),
            (
                LoadingArtVariant::Observer,
                "ls640obs.shp",
                "ls800obs.shp",
                "MPLSOBS.PAL",
            ),
            (
                LoadingArtVariant::Americans,
                "ls640ustates.shp",
                "ls800ustates.shp",
                "MPLSU.PAL",
            ),
            (
                LoadingArtVariant::Russians,
                "ls640russia.shp",
                "ls800russia.shp",
                "MPLSR.PAL",
            ),
            (
                LoadingArtVariant::Africans,
                "ls640libya.shp",
                "ls800libya.shp",
                "MPLSL.PAL",
            ),
            (
                LoadingArtVariant::Alliance,
                "ls640korea.shp",
                "ls800korea.shp",
                "MPLSK.PAL",
            ),
            (
                LoadingArtVariant::Arabs,
                "ls640iraq.shp",
                "ls800iraq.shp",
                "MPLSI.PAL",
            ),
            (
                LoadingArtVariant::Germans,
                "ls640germany.shp",
                "ls800germany.shp",
                "MPLSG.PAL",
            ),
            (
                LoadingArtVariant::French,
                "ls640france.shp",
                "ls800france.shp",
                "MPLSF.PAL",
            ),
            (
                LoadingArtVariant::Confederation,
                "ls640cuba.shp",
                "ls800cuba.shp",
                "MPLSC.PAL",
            ),
            (
                LoadingArtVariant::British,
                "ls640ukingdom.shp",
                "ls800ukingdom.shp",
                "MPLSUK.PAL",
            ),
        ];

        for (variant, expected_640, expected_800, expected_palette) in cases {
            let manifest = variant.manifest();
            assert_eq!(
                manifest.background_asset(LoadingScreenWidth::W640),
                expected_640
            );
            assert_eq!(
                manifest.background_asset(LoadingScreenWidth::W800),
                expected_800
            );
            assert_eq!(manifest.palette_name, expected_palette);
        }
    }

    #[test]
    fn loading_art_manifest_does_not_use_mode2_or_campaign_assets() {
        for variant in [
            LoadingArtVariant::Yuri,
            LoadingArtVariant::Observer,
            LoadingArtVariant::Americans,
            LoadingArtVariant::Russians,
            LoadingArtVariant::Africans,
            LoadingArtVariant::Alliance,
            LoadingArtVariant::Arabs,
            LoadingArtVariant::Germans,
            LoadingArtVariant::French,
            LoadingArtVariant::Confederation,
            LoadingArtVariant::British,
        ] {
            let manifest = variant.manifest();
            let name = manifest.background_asset(LoadingScreenWidth::W800);
            assert!(!name.to_ascii_lowercase().contains("pudlgbg"));
            assert_ne!(name.to_ascii_lowercase(), "spldbr.shp");
        }
    }

    #[test]
    #[ignore = "diagnostic asset dump; run explicitly when inspecting PROGBARM palette indices"]
    fn zz_dump_progbarm_frame0_indices() {
        let config = match crate::util::config::GameConfig::load() {
            Ok(config) => config,
            Err(_) => return,
        };
        if !config.paths.ra2_dir.exists() {
            return;
        }
        let assets = crate::assets::asset_manager::AssetManager::new(&config.paths.ra2_dir)
            .expect("install loads");
        let bytes = assets.get_ref("PROGBARM.SHP").expect("progbarm");
        let shp = crate::assets::shp_file::ShpFile::from_bytes(bytes).expect("parse shp");
        let f = &shp.frames[0];
        let mut hist = [0u32; 256];
        for &p in &f.pixels {
            hist[p as usize] += 1;
        }
        eprintln!(
            "PROGBARM frame0 fw={} fh={} fx={} fy={} npix={}",
            f.frame_width,
            f.frame_height,
            f.frame_x,
            f.frame_y,
            f.pixels.len()
        );
        for i in 0..256 {
            if hist[i] > 0 {
                eprintln!("  idx {i:3} count {}", hist[i]);
            }
        }
        // Dump MPLS.PAL RGB for the house-color band 16..31 (raw 6-bit *4 not applied here).
        if let Some(pal) = assets.get_ref("MPLS.PAL") {
            eprintln!("MPLS.PAL raw bytes 16..32:");
            for i in 16..32 {
                let o = i * 3;
                eprintln!("  pal {i:2} = {} {} {}", pal[o], pal[o + 1], pal[o + 2]);
            }
        }
        panic!("diagnostic dump");
    }

    #[test]
    fn side_icon_pcx_uses_verified_insignia_mapping() {
        let cases = [
            (LoadingArtVariant::Americans, "usai.pcx"),
            (LoadingArtVariant::Alliance, "japi.pcx"),
            (LoadingArtVariant::French, "frai.pcx"),
            (LoadingArtVariant::Germans, "geri.pcx"),
            (LoadingArtVariant::British, "gbri.pcx"),
            (LoadingArtVariant::Africans, "djbi.pcx"),
            (LoadingArtVariant::Arabs, "arbi.pcx"),
            (LoadingArtVariant::Confederation, "lati.pcx"),
            (LoadingArtVariant::Russians, "rusi.pcx"),
            (LoadingArtVariant::Yuri, "yrii.pcx"),
            (LoadingArtVariant::Observer, "obsi.pcx"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.side_icon_pcx(), expected, "{variant:?}");
        }
    }

    #[test]
    fn loading_art_variant_resolves_rules_country_names() {
        assert_eq!(
            LoadingArtVariant::from_country_name("Americans"),
            Some(LoadingArtVariant::Americans)
        );
        assert_eq!(
            LoadingArtVariant::from_country_name("Alliance"),
            Some(LoadingArtVariant::Alliance)
        );
        assert_eq!(
            LoadingArtVariant::from_country_name("YuriCountry"),
            Some(LoadingArtVariant::Yuri)
        );
    }

    #[test]
    fn loading_art_manifest_assets_resolve_and_decode_from_configured_install_when_available() {
        let config = match crate::util::config::GameConfig::load() {
            Ok(config) => config,
            Err(_) => return,
        };
        if !config.paths.ra2_dir.exists() {
            return;
        }

        let mut assets = crate::assets::asset_manager::AssetManager::new(&config.paths.ra2_dir)
            .expect("configured RA2 install should load");
        assert!(
            assets
                .register_loading_archives()
                .expect("configured loading archive setup"),
            "LOADMD.MIX and LOAD.MIX should register from configured RA2 install"
        );

        assert!(
            assets.get_ref("PROGBARM.SHP").is_some(),
            "PROGBARM.SHP should resolve from configured RA2 install"
        );
        assert!(
            assets.get_ref("MPLS.PAL").is_some(),
            "MPLS.PAL should resolve from configured RA2 install"
        );
        let marker_palette = crate::assets::pal_file::Palette::from_bytes_gamemd_ui(
            assets.get_ref("MPLS.PAL").expect("MPLS palette"),
        )
        .expect("MPLS palette should decode");
        let marker = render_shp_entry(&assets, super::MMPB_ASSET_NAME, &marker_palette, 0)
            .expect("retail mmpb.shp frame 0 should decode");
        assert_eq!((marker.width, marker.height), (12, 12));

        for variant in [
            LoadingArtVariant::Yuri,
            LoadingArtVariant::Observer,
            LoadingArtVariant::Americans,
            LoadingArtVariant::Russians,
            LoadingArtVariant::Africans,
            LoadingArtVariant::Alliance,
            LoadingArtVariant::Arabs,
            LoadingArtVariant::Germans,
            LoadingArtVariant::French,
            LoadingArtVariant::Confederation,
            LoadingArtVariant::British,
        ] {
            let manifest = variant.manifest();
            let palette_bytes = assets
                .get_ref(manifest.palette_name)
                .unwrap_or_else(|| panic!("{variant:?} loading palette should resolve"));
            let palette = crate::assets::pal_file::Palette::from_bytes_gamemd_ui(palette_bytes)
                .unwrap_or_else(|err| panic!("{variant:?} loading palette should decode: {err:#}"));
            assert!(
                assets
                    .get_ref(&manifest.background_asset(LoadingScreenWidth::W800))
                    .is_some(),
                "{variant:?} 800px loading background should resolve"
            );
            let decoded = render_shp_entry(
                &assets,
                &manifest.background_asset(LoadingScreenWidth::W800),
                &palette,
                0,
            )
            .unwrap_or_else(|| panic!("{variant:?} 800px loading background should decode"));
            assert_eq!(decoded.width, 800);
            assert_eq!(decoded.height, 600);
            let icon = render_pcx_entry(&assets, variant.side_icon_pcx())
                .unwrap_or_else(|| panic!("{variant:?} country insignia should decode"));
            assert!(
                icon.rgba.chunks_exact(4).any(|pixel| pixel[3] == 0),
                "{variant:?} country insignia should contain transparent keyed pixels"
            );
        }
    }
}
