//! Native loading-screen chrome.
//!
//! `DrawLoading @ 0x00552D60` selects either country art/`PROGBARM.SHP` or
//! campaign MISSION art/title/bar/`SPLDBR.SHP`. Both use the same UI palette,
//! SHP frame decoder and atlas packer. Standard Skirmish callers may also
//! supply a prepared selected-map preview and house-color marker ramps.

use std::collections::{BTreeMap, HashMap};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::chrome_atlas::{ChromeAtlasEntry, ChromePiece, pack_chrome_atlas};
use crate::render::gpu::GpuContext;
use crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;

/// Synthetic atlas label for the 1x1 white texel used to draw the solid backing
/// fill (G3) as a tinted quad. Not an asset on disk.
const SOLID_TEXEL_LABEL: &str = "__solid_texel__";

/// Synthetic label for the caller-prepared selected-map preview.
const LOADING_PREVIEW_LABEL: &str = "__selected_map_preview__";

// Campaign roles can reference the same filename with different palettes;
// keep their atlas labels distinct from asset names and one another.
const CAMPAIGN_BACKGROUND_LABEL: &str = "__campaign_background__";
const CAMPAIGN_TITLE_LABEL: &str = "__campaign_title__";
const CAMPAIGN_BAR_BACKGROUND_LABEL: &str = "__campaign_bar_background__";
const CAMPAIGN_PROGRESS_LABEL: &str = "__campaign_progress__";

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
        let index = match self {
            Self::Americans => 0,
            Self::Alliance => 1,
            Self::French => 2,
            Self::Germans => 3,
            Self::British => 4,
            Self::Africans => 5,
            Self::Arabs => 6,
            Self::Confederation => 7,
            Self::Russians => 8,
            Self::Yuri => 9,
            Self::Observer => return LoadingArtManifest::new("obs", "MPLSOBS.PAL"),
        };
        let row = crate::rules::country_loading::stock_country_loading(index)
            .expect("stock loading variant has a literal row");
        {
            let (token, palette) = row.art();
            LoadingArtManifest::new(token, palette)
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

pub type LoadingScreenEntry = ChromeAtlasEntry;

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

/// Named background replaces only the country SHP/PAL, never progress or icons.
/// Borrowed names are decoded once into the loading attempt's existing atlas;
/// dropping that atlas on success/failure retires its palette as well.
#[derive(Debug, Clone, Copy)]
pub struct LoadingScreenArtInput<'a> {
    pub variant: Option<LoadingArtVariant>,
    pub background: Option<LoadingCountryBackground<'a>>,
}

/// Late Country SHP with the PAL actually resolved before Full_Init's second
/// Country pass. None palette denotes an explicitly cleared PAL, not fallback
/// to a stock country's assets. Decoding remains with this module's owner.
#[derive(Debug, Clone, Copy)]
pub struct LoadingCountryBackground<'a> {
    pub name: &'a str,
    pub palette: Option<&'a Palette>,
    pub palette_name: &'a str,
}

impl From<Option<LoadingArtVariant>> for LoadingScreenArtInput<'_> {
    fn from(variant: Option<LoadingArtVariant>) -> Self {
        Self {
            variant,
            background: None,
        }
    }
}

impl From<LoadingArtVariant> for LoadingScreenArtInput<'_> {
    fn from(variant: LoadingArtVariant) -> Self {
        Some(variant).into()
    }
}

pub struct LoadingParticipantArtInput {
    pub variant: Option<LoadingArtVariant>,
    pub ramp: [Color; 16],
    /// Row insignia file the Country selection resolves through
    /// `Game_GetFlagSurface @ 0x1001DCE0`, replacing the stock chain name.
    pub side_icon: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadingParticipantArt {
    pub progress_frame: LoadingScreenEntry,
    pub side_icon: Option<LoadingScreenEntry>,
}

pub struct LoadingScreenAtlas {
    pub texture: BatchTexture,
    pub background: Option<LoadingScreenEntry>,
    pub progress_frame0: LoadingScreenEntry,
    /// Source-node order, matching ProgressScreen643C0B's row iteration.
    pub participants: Vec<LoadingParticipantArt>,
    /// Campaign title strip, or `None` for the native solid-rectangle fallback.
    /// Standard country loading has no separate title strip.
    pub title_bar: Option<LoadingScreenEntry>,
    /// Campaign bar surround, with the same native missing-SHP fallback.
    pub progress_background: Option<LoadingScreenEntry>,
    /// Caller-prepared selected-map preview, when its RGBA payload is valid.
    pub preview: Option<LoadingScreenEntry>,
    /// Retail `mmpb.shp` frame 0 decoded for each distinct assigned-house color.
    pub mmpb_markers: BTreeMap<u8, LoadingScreenEntry>,
    /// Country insignia drawn right of the bar (G4). `None` when the PCX is
    /// missing — the bar still draws without it.
    pub side_icon: Option<LoadingScreenEntry>,
    /// MeasureRow642F30 and DrawRow643973 always reserve country0's icon
    /// dimensions, independently of whether the actual country's icon exists.
    pub insignia_measure_size: Option<[i32; 2]>,
    /// 1x1 white texel for drawing the solid backing fill (G3) as a tinted quad.
    pub solid_texel: LoadingScreenEntry,
}

/// Row insignia file selected for a loading row. A Country row with retained
/// Ares state answers from its own `File.Flag` through `Game_GetFlagSurface @
/// 0x1001DCE0` (patch site `0x4E3562`); every other selection keeps the stock
/// chain name from `0x00844AE8..0x00844B14`.
fn side_icon_name<'a>(
    variant: Option<LoadingArtVariant>,
    country_file: Option<&'a str>,
) -> Option<&'a str> {
    country_file.or_else(|| variant.map(LoadingArtVariant::side_icon_pcx))
}

/// Resolve the one background layer before atlas packing. Explicit Country
/// input already owns its early palette; only the stock branch loads one here.
fn render_loading_background(
    assets: &AssetManager,
    art: LoadingScreenArtInput<'_>,
    width: LoadingScreenWidth,
) -> Option<Option<ChromePiece>> {
    if let Some(background) = art.background {
        // Original DrawLoading 5535C1..5535CE skips the background if either
        // Convert or SHP is null, then resumes progress drawing at 553603.
        // Missing custom art cannot select a stock fallback or fail the atlas.
        if background.name.is_empty() || background.palette.is_none() {
            Some(None)
        } else {
            Some(mandatory_shp(
                assets,
                background.name,
                background.palette?,
                0,
                background.palette_name,
            ))
        }
    } else if let Some(variant) = art.variant {
        let manifest = variant.manifest();
        let name = manifest.background_asset(width);
        let palette = load_named_ui_palette(assets, manifest.palette_name)?;
        Some(Some(mandatory_shp(
            assets,
            &name,
            &palette,
            0,
            manifest.palette_name,
        )?))
    } else {
        Some(None)
    }
}

/// Build the native loading atlas with optional selected-map preview/markers.
/// A stock background and progress are mandatory; invalid optional preview or
/// marker input removes only those layers. Empty or unavailable Country background
/// art omits that layer without a stock fallback.
pub fn build_loading_screen_atlas_with_composition<'a>(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    art: impl Into<LoadingScreenArtInput<'a>>,
    width: LoadingScreenWidth,
    progress_ramp: &[Color; 16],
    composition: Option<LoadingScreenCompositionAtlasInput<'_>>,
    participants: &[LoadingParticipantArtInput],
) -> Option<LoadingScreenAtlas> {
    let art = art.into();
    let variant = art.variant;
    let background = render_loading_background(assets, art, width)?;
    let background_label = background.as_ref().map(|entry| entry.label.clone());
    // gamemd draws the Skirmish meter and the map-preview house-colour markers through
    // an assigned house's ColorScheme convert: `ProgressMeterClass__DrawFill @ 0x00643486`
    // reads `ColorScheme+0x30C` (the `+0x74` override stays null for `g_GameMode != 0`,
    // set in `ScenarioClass__Read_Scenario @ 0x00684805..0x0068482F`), and the marker loop
    // at `0x00640A40` passes `g_ColorSchemeArray[House+0x16054] * 4 + 0x30C`. Each scheme's
    // palette is `g_FileSystem_TEMPERAT_PAL` with only indices 16..31 overwritten:
    // `ColorScheme__Constructor @ 0x0068C710` hands that fixed buffer to
    // `ColorScheme__BuildRampPalette @ 0x0068C3B0`, whose first step is the 768-byte base
    // copy, and `Init_Game` fills it from `TEMPERAT.PAL` whatever the selected theater.
    let house_convert_base_palette = load_named_ui_palette(assets, "TEMPERAT.PAL")?;
    let progress_palette =
        progress_palette_with_player_ramp(&house_convert_base_palette, progress_ramp);

    let mut rendered = vec![
        mandatory_shp(assets, "PROGBARM.SHP", &progress_palette, 0, "TEMPERAT.PAL")?,
        solid_texel_entry(),
    ];
    if let Some(background) = background {
        rendered.push(background);
    }
    let insignia_measure_size =
        render_pcx_entry(assets, LoadingArtVariant::Americans.side_icon_pcx())
            .map(|entry| [entry.width as i32, entry.height as i32]);

    let (preview_label, marker_labels) = append_optional_composition_entries(
        assets,
        &house_convert_base_palette,
        composition,
        &mut rendered,
    );

    // Side-icon is non-fatal: if its PCX is missing or malformed the bar still
    // draws. Record its label so it can be looked up after packing.
    let side_icon_label = variant.and_then(|variant| {
        let entry = render_pcx_entry(assets, variant.side_icon_pcx())?;
        let label = entry.label.clone();
        rendered.push(entry);
        Some(label)
    });

    let mut participant_labels = Vec::with_capacity(participants.len());
    for (index, participant) in participants.iter().enumerate() {
        let frame_label = if participant.ramp == *progress_ramp {
            "progbarm.shp".to_string()
        } else {
            let palette =
                progress_palette_with_player_ramp(&house_convert_base_palette, &participant.ramp);
            let mut entry = mandatory_shp(assets, "PROGBARM.SHP", &palette, 0, "TEMPERAT.PAL")?;
            entry.label = format!("player-progress-{index}");
            let label = entry.label.clone();
            rendered.push(entry);
            label
        };
        let icon_label = side_icon_name(participant.variant, participant.side_icon.as_deref())
            .and_then(|name| {
                let label = name.to_ascii_lowercase();
                if rendered.iter().any(|entry| entry.label == label) {
                    Some(label)
                } else {
                    let entry = render_pcx_entry(assets, name)?;
                    let label = entry.label.clone();
                    rendered.push(entry);
                    Some(label)
                }
            });
        participant_labels.push((frame_label, icon_label));
    }

    let (texture, packed) = pack_chrome_atlas(gpu, batch, "Loading", &rendered)?;
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
        background: background_label.and_then(|label| by_label.get(&label).copied()),
        progress_frame0: *by_label.get("progbarm.shp")?,
        participants: participant_labels
            .into_iter()
            .map(|(frame, icon)| LoadingParticipantArt {
                progress_frame: *by_label.get(&frame).expect("packed participant progress"),
                side_icon: icon.and_then(|label| by_label.get(&label).copied()),
            })
            .collect(),
        title_bar: None,
        progress_background: None,
        preview,
        mmpb_markers,
        side_icon,
        insignia_measure_size,
        solid_texel: *by_label.get(SOLID_TEXEL_LABEL)?,
    })
}

///642C20 transfer callers load only PROGBARM; their shell backdrop is
/// composed by the shell owner. Reuse this atlas/decoder/palette owner without
/// requiring an unrelated country's loading background to exist.
pub fn build_transfer_progress_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    progress_ramp: &[Color; 16],
) -> Option<LoadingScreenAtlas> {
    let base = load_named_ui_palette(assets, "TEMPERAT.PAL")?;
    let palette = progress_palette_with_player_ramp(&base, progress_ramp);
    let entries = [
        mandatory_shp(assets, "PROGBARM.SHP", &palette, 0, "TEMPERAT.PAL")?,
        solid_texel_entry(),
    ];
    let (texture, packed) = pack_chrome_atlas(gpu, batch, "Loading", &entries)?;
    Some(LoadingScreenAtlas {
        texture,
        background: None,
        progress_frame0: packed[0],
        participants: Vec::new(),
        title_bar: None,
        progress_background: None,
        preview: None,
        mmpb_markers: BTreeMap::new(),
        side_icon: None,
        insignia_measure_size: None,
        solid_texel: packed[1],
    })
}

/// Pack the native campaign art and frame-0 progress source in the existing
/// loading atlas. Register the LOADMD/LOAD archives through AssetManager first.
///
/// `DrawLoading @ 0x00552D60` selects the caller's MISSION background and its
/// `LS800BkgdPal` at either width. `0x0072B2F0/0x0072B310` select the title/bar
/// strips; `0x0072B340/0x0072B380` provide LDSCRN/SPLDBR palettes. The progress
/// source is SPLDBR with that same SPLDBR palette, without a house-color remap.
/// Missing title/bar shapes remain optional so the drawing owner can fill the
/// corresponding native rectangles. Empty background names return no atlas;
/// the caller must preserve the campaign family and its empty-art branch.
pub fn build_campaign_loading_screen_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    width: LoadingScreenWidth,
    background_name: &str,
    background_palette_name: &str,
) -> Option<LoadingScreenAtlas> {
    let rendered =
        render_campaign_entries(assets, width, background_name, background_palette_name)?;
    let (texture, packed) = pack_chrome_atlas(gpu, batch, "Loading", &rendered)?;
    let by_label: HashMap<String, LoadingScreenEntry> = rendered
        .iter()
        .map(|entry| entry.label.clone())
        .zip(packed)
        .collect();
    Some(LoadingScreenAtlas {
        texture,
        background: Some(*by_label.get(CAMPAIGN_BACKGROUND_LABEL)?),
        progress_frame0: *by_label.get(CAMPAIGN_PROGRESS_LABEL)?,
        participants: Vec::new(),
        title_bar: by_label.get(CAMPAIGN_TITLE_LABEL).copied(),
        progress_background: by_label.get(CAMPAIGN_BAR_BACKGROUND_LABEL).copied(),
        preview: None,
        mmpb_markers: BTreeMap::new(),
        side_icon: None,
        insignia_measure_size: None,
        solid_texel: *by_label.get(SOLID_TEXEL_LABEL)?,
    })
}

fn render_campaign_entries(
    assets: &AssetManager,
    width: LoadingScreenWidth,
    background_name: &str,
    background_palette_name: &str,
) -> Option<Vec<ChromePiece>> {
    if background_name.is_empty() {
        return None;
    }
    let background_palette = load_named_ui_palette(assets, background_palette_name)?;
    let title_palette = load_named_ui_palette(assets, "LDSCRN.PAL")?;
    let bar_palette = load_named_ui_palette(assets, "SPLDBR.PAL")?;
    let mut background = mandatory_shp(
        assets,
        background_name,
        &background_palette,
        0,
        background_palette_name,
    )?;
    background.label = CAMPAIGN_BACKGROUND_LABEL.to_owned();
    let mut progress = mandatory_shp(assets, "SPLDBR.SHP", &bar_palette, 0, "SPLDBR.PAL")?;
    progress.label = CAMPAIGN_PROGRESS_LABEL.to_owned();
    let mut rendered = vec![background, progress, solid_texel_entry()];
    let (title_name, bar_name) = match width {
        LoadingScreenWidth::W640 => ("TTLBR640.SHP", "SPLDBRS.SHP"),
        LoadingScreenWidth::W800 => ("TTLBR800.SHP", "SPLDBRL.SHP"),
    };
    if let Some(mut title) = render_shp_entry(assets, title_name, &title_palette, 0) {
        title.label = CAMPAIGN_TITLE_LABEL.to_owned();
        rendered.push(title);
    }
    if let Some(mut bar) = render_shp_entry(assets, bar_name, &bar_palette, 0) {
        bar.label = CAMPAIGN_BAR_BACKGROUND_LABEL.to_owned();
        rendered.push(bar);
    }
    Some(rendered)
}

fn progress_palette_with_player_ramp(palette: &Palette, progress_ramp: &[Color; 16]) -> Palette {
    palette.with_house_colors(progress_ramp)
}

fn append_optional_composition_entries(
    assets: &AssetManager,
    remap_base_palette: &Palette,
    composition: Option<LoadingScreenCompositionAtlasInput<'_>>,
    rendered: &mut Vec<ChromePiece>,
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

fn prepared_preview_entry(preview: &PreparedLoadingPreviewRgba) -> Option<ChromePiece> {
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
    Some(ChromePiece {
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
fn solid_texel_entry() -> ChromePiece {
    ChromePiece {
        label: SOLID_TEXEL_LABEL.to_string(),
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 255],
    }
}

/// Decode a country-insignia PCX into an atlas entry using the native
/// RGB-magenta transparency key.
fn render_pcx_entry(assets: &AssetManager, file_name: &str) -> Option<ChromePiece> {
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
    Some(ChromePiece {
        label: file_name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
}

/// Loading-screen palettes come from ConvertClass__BuildFromPaletteFile
/// `0x0072ADE0` and the meter base from Init_Game's TEMPERAT.PAL loop
/// (`0x0052BC6A..0x0052BCB9`); both convert like every other palette load.
pub(crate) fn load_named_ui_palette(assets: &AssetManager, name: &str) -> Option<Palette> {
    let Some(bytes) = assets.get_ref(name) else {
        log::warn!("Missing loading palette {name}");
        return None;
    };
    Palette::from_bytes(bytes)
        .map_err(|err| {
            log::warn!("Could not parse loading palette {name}: {err:#}");
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
) -> Option<ChromePiece> {
    render_shp_entry(assets, file_name, palette, frame).or_else(|| {
        log::warn!(
            "Missing mandatory loading asset {file_name} frame {frame} decoded with {palette_name}"
        );
        None
    })
}

fn render_shp_entry(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
) -> Option<ChromePiece> {
    let Some(load) = assets.load_file_from_mix(file_name) else {
        log::warn!("Missing loading SHP {file_name}");
        return None;
    };
    let shp = ShpFile::from_bytes(&load.bytes)
        .map_err(|err| {
            log::warn!("Could not parse loading SHP {file_name}: {err:#}");
            err
        })
        .ok()?;
    if frame >= shp.frames.len() {
        log::warn!(
            "Loading SHP {file_name} missing frame {frame}; frame count {}",
            shp.frames.len()
        );
        return None;
    }
    let frame_rgba = shp
        .frame_to_rgba(frame, palette)
        .map_err(|err| {
            log::warn!("Could not decode loading SHP {file_name} frame {frame}: {err:#}");
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
    Some(ChromePiece {
        label: file_name.to_ascii_lowercase(),
        width: canvas_w,
        height: canvas_h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CAMPAIGN_BACKGROUND_LABEL, CAMPAIGN_BAR_BACKGROUND_LABEL, CAMPAIGN_PROGRESS_LABEL,
        CAMPAIGN_TITLE_LABEL, LOADING_PREVIEW_LABEL, LoadingArtVariant, LoadingScreenWidth,
        MmpbRegionRect, PreparedLoadingPreviewRgba, mmpb_region_rect, prepared_preview_entry,
        progress_palette_with_player_ramp, render_campaign_entries, render_pcx_entry,
        render_shp_entry,
    };
    use crate::assets::pal_file::{Color, Palette};

    #[cfg(feature = "ares")]
    #[test]
    fn ares_country_loading_background_consumes_frozen_palette_and_late_shape() {
        let Some((_, mut retail)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assert!(retail.register_loading_archives().unwrap());
        let name = LoadingArtVariant::Americans
            .manifest()
            .background_asset(LoadingScreenWidth::W800);
        let dir =
            crate::map::source::test_support::TestDirectory::new("ares-country-loading-render");
        dir.write(&name, retail.get_ref(&name).unwrap());
        let mut red = [0; 768];
        let mut green = [0; 768];
        for index in 0..256 {
            red[index * 3] = 63;
            green[index * 3 + 1] = 63;
        }
        dir.write("early.pal", &red);
        dir.write("late.pal", &green);
        let assets =
            crate::assets::asset_manager::AssetManager::from_loose_root_for_test(dir.path());
        let frozen = super::load_named_ui_palette(&assets, "early.pal").unwrap();
        let entry = super::render_loading_background(
            &assets,
            super::LoadingScreenArtInput {
                variant: Some(LoadingArtVariant::Arabs),
                background: Some(super::LoadingCountryBackground {
                    name: &name,
                    palette: Some(&frozen),
                    palette_name: "late.pal",
                }),
            },
            LoadingScreenWidth::W800,
        )
        .unwrap()
        .unwrap();
        let opaque: Vec<_> = entry
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .collect();
        assert!(!opaque.is_empty());
        assert!(
            opaque
                .iter()
                .all(|pixel| *pixel == [252u8, 0, 0, 255].as_slice()),
            "late palette name cannot trigger a new read"
        );
        assert!(
            super::render_loading_background(
                &assets,
                super::LoadingScreenArtInput {
                    variant: Some(LoadingArtVariant::Americans),
                    background: Some(super::LoadingCountryBackground {
                        name: &name,
                        palette: None,
                        palette_name: ""
                    }),
                },
                LoadingScreenWidth::W800,
            )
            .unwrap()
            .is_none(),
            "an explicitly cleared Country PAL cannot fall back to stock art"
        );
        assert!(
            super::render_loading_background(
                &assets,
                super::LoadingScreenArtInput {
                    variant: Some(LoadingArtVariant::Arabs),
                    background: Some(super::LoadingCountryBackground {
                        name: "missing-country.shp",
                        palette: Some(&frozen),
                        palette_name: "early.pal",
                    }),
                },
                LoadingScreenWidth::W800,
            )
            .unwrap()
            .is_none(),
            "missing Country SHP skips the background and retains progress"
        );
    }

    #[test]
    fn first_campaign_art_decodes_through_the_shared_loading_palette_and_shp_path() {
        let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        assert!(
            assets
                .register_loading_archives()
                .expect("retail loading archive setup"),
            "LOADMD and LOAD archives must register for campaign art"
        );
        let mission = crate::rules::ini_parser::IniFile::from_bytes(
            assets.get_ref("MISSIONMD.INI").expect("retail MISSIONMD"),
        )
        .expect("retail MISSIONMD parses");
        for filename in ["ALL01UMD.MAP", "SOV01UMD.MAP"] {
            let mut metadata = crate::rules::campaign_loading::CampaignLoadingMetadata::new();
            metadata.apply_ini(&mission, filename);
            for (width, expected_body, expected_strip_width) in [
                (LoadingScreenWidth::W640, [640, 400], 640),
                (LoadingScreenWidth::W800, [800, 520], 800),
            ] {
                let background_name = match width {
                    LoadingScreenWidth::W640 => metadata.background_name_640(),
                    LoadingScreenWidth::W800 => metadata.background_name_800(),
                };
                let rendered = render_campaign_entries(
                    &assets,
                    width,
                    background_name,
                    metadata.background_palette_name(),
                )
                .expect("stock first-mission loading art decodes");
                // Full frame/canvas geometries from the physical retail
                // shapes, not from the atlas layout implementation.
                for (label, expected) in [
                    (CAMPAIGN_BACKGROUND_LABEL, expected_body),
                    (CAMPAIGN_TITLE_LABEL, [expected_strip_width, 40]),
                    (CAMPAIGN_BAR_BACKGROUND_LABEL, [expected_strip_width, 40]),
                    (CAMPAIGN_PROGRESS_LABEL, [456, 25]),
                ] {
                    let entry = rendered
                        .iter()
                        .find(|entry| entry.label == label)
                        .unwrap_or_else(|| panic!("{filename} {width:?} missing {label}"));
                    assert_eq!([entry.width, entry.height], expected, "{filename} {label}");
                    assert_eq!(entry.rgba.len(), (expected[0] * expected[1] * 4) as usize);
                }
            }
        }
    }

    #[test]
    fn progress_palette_replaces_only_the_house_color_band() {
        let palette = Palette::from_rgb(|index| match index {
            16 => [7, 8, 9],
            31 => [10, 11, 12],
            _ => [3, 4, 5],
        });
        let ramp = std::array::from_fn(|index| {
            Color::rgb(index as u8, (index as u8) + 32, (index as u8) + 64)
        });

        let remapped = progress_palette_with_player_ramp(&palette, &ramp);

        assert_eq!(&remapped.colors()[16..32], &ramp);
        assert_eq!(remapped.color(15), palette.color(15));
        assert_eq!(remapped.color(32), palette.color(32));
    }

    /// The meter and the map-preview markers both take the house scheme's convert, so
    /// that base only matters if it differs from the UI palette outside the house band.
    /// If these two agreed, switching the base would be inert.
    #[test]
    fn temperat_meter_base_differs_from_the_ui_palette_outside_the_band() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let load = |name: &str| {
            Palette::from_bytes(
                assets
                    .get_ref(name)
                    .unwrap_or_else(|| panic!("retail {name}")),
            )
            .unwrap_or_else(|err| panic!("{name} parses: {err:#}"))
        };
        let temperat = load("TEMPERAT.PAL");
        let ui = load("MPLS.PAL");

        let outside_band = (0..16)
            .chain(32..=255)
            .filter(|index| temperat.color(*index) != ui.color(*index))
            .count();
        assert!(
            outside_band > 0,
            "TEMPERAT.PAL and MPLS.PAL agree outside 16..31, so the meter base would be inert"
        );
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
    fn loading_art_manifest_assets_resolve_and_decode_from_configured_install_when_available() {
        let config = match crate::util::config::GameConfig::load() {
            Ok(config) => config,
            Err(_) => return,
        };
        // Without config.toml the fallback root is the test binary's folder;
        // require an actual install.
        if !config.paths.ra2_dir.join("ra2md.mix").exists() {
            return;
        }

        let mut assets = crate::assets::asset_manager::AssetManager::new(
            &config.paths.ra2_dir,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
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
        let marker_palette = crate::assets::pal_file::Palette::from_bytes(
            assets.get_ref("TEMPERAT.PAL").expect("TEMPERAT palette"),
        )
        .expect("TEMPERAT palette should decode");
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
            let palette = crate::assets::pal_file::Palette::from_bytes(palette_bytes)
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
                icon.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 0),
                "{variant:?} country insignia should contain transparent keyed pixels"
            );
        }
    }
}
