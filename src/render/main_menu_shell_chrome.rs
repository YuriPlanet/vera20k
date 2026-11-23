//! Initial main-menu shell chrome atlas for dialog 0xE2 owner-draw buttons
//! and the right-panel + lower-side chrome SHPs drawn underneath them by the
//! parent WM_PAINT_Handler.
//!
//! Buttons are SDBTNANM.SHP frames 2 (default), 3 (hover), and 4 (pressed) —
//! drawn through CC_Draw_Shape with the SDBTNANM palette, producing the
//! red / orange / yellow gradient artwork the player sees. The full 0..=16 frame
//! set is also baked so the first-paint controls-reveal slide can ramp each
//! button through its animation frames. The `bue_*30` / `bde_*30` PCXs that ship
//! in the same archives are greyscale and unused on this paint path.

use anyhow::{Context, Result};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::chrome_atlas::{
    ChromeArt, ChromeAtlasEntry, ChromePiece, Need, load_palette, pack_chrome_atlas, pcx_piece,
    read_palette, read_ui_shp, shp_file_piece, shp_piece,
};
use crate::render::gpu::GpuContext;

/// SDBTNANM.SHP frames baked for the first-paint slide ramp. The reveal animates
/// "active" buttons through frames 5..=10 and "inactive" slots through 11..=16,
/// so the full 0..=16 set is loaded; missing frames stay `None` and the wave
/// clamps to the nearest available frame.
const SDBTNANM_WAVE_FRAMES: usize = 17;

const ATLAS_NAME: &str = "Main-menu shell chrome";

pub type MainMenuShellChromeEntry = ChromeAtlasEntry;

pub struct MainMenuShellChromeAtlas {
    pub texture: BatchTexture,
    /// SDBTNANM.SHP frames 0..=16, indexed by frame number, for the first-paint
    /// slide ramp. Shared by the 0xE2 and 0x100 shell renderers (both reuse this
    /// atlas). `None` where the SHP lacks that frame.
    pub button_wave_frames: [Option<MainMenuShellChromeEntry>; SDBTNANM_WAVE_FRAMES],
    pub right_panel_top_sdtp: Option<MainMenuShellChromeEntry>,
    /// SDTP frames 0 and 1 and SDWRNTMP frames 0..=5 (SHELL palette): the
    /// top panel of a `+0xD5` dialog (WOL's custom lobby `0x113`), which
    /// the parent paint covers with SDTP frame 1 (`0x0072E8C0`) and the
    /// slide engine draws with its warning display.
    pub top_panel: TopPanelArt,
    pub right_panel_tile_sdbtnbkgd: Option<MainMenuShellChromeEntry>,
    pub right_panel_bottom_sdbtm: Option<MainMenuShellChromeEntry>,
    pub lower_side_640_lwscrns: Option<MainMenuShellChromeEntry>,
    pub lower_side_large_lwscrnl: Option<MainMenuShellChromeEntry>,
    /// MNSCRNS.SHP frame 0 — parent background that fills the screen behind
    /// everything at 640-wide resolution. Painted through SHELL.PAL.
    pub parent_background_640_mnscrns: Option<MainMenuShellChromeEntry>,
    /// MNSCRNL.SHP frame 0 — parent background for all non-640 widths.
    /// Painted through SHELL.PAL.
    pub parent_background_large_mnscrnl: Option<MainMenuShellChromeEntry>,
    /// MNSCRNS / MNSCRNL one RGB565 unit darker ([`blend_black_rgb565`] at
    /// alpha 0): what a family list (`0x00619230`) shows inside its frame.
    pub parent_background_640_mnscrns_list: Option<MainMenuShellChromeEntry>,
    pub parent_background_large_mnscrnl_list: Option<MainMenuShellChromeEntry>,
    /// Static `0x71C` SDWRNANM frames in order (SHELL2 palette); empty when
    /// the SHP is missing.
    pub warning_monitor_frames: Vec<MainMenuShellChromeEntry>,
}

/// Owner-draw list scrollbar art (`0x0061C690`): 18x22 arrows, released and
/// pressed, and the 18-wide grip top, middle and bottom.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListScrollArt {
    pub up_released: Option<MainMenuShellChromeEntry>,
    pub up_pressed: Option<MainMenuShellChromeEntry>,
    pub down_released: Option<MainMenuShellChromeEntry>,
    pub down_pressed: Option<MainMenuShellChromeEntry>,
    pub grip_top: Option<MainMenuShellChromeEntry>,
    pub grip_mid: Option<MainMenuShellChromeEntry>,
    pub grip_bottom: Option<MainMenuShellChromeEntry>,
}

/// `AlphaBlendRect` `0x00621B80` with colour 0 over encoded RGBA8 texels:
/// every RGB565 channel keeps `floor(c * (255 - alpha) / 256)`. Alpha 0 is one
/// unit darker (what a list or trackbar saves under itself); `0x9F` is the
/// score table's 37.5 %. The shell presenter quantizes encoded bytes with
/// `>> 3` / `>> 2`, so storing the result in the high bits reproduces the
/// 16-bit value.
fn blend_black_rgb565(rgba: &[u8], alpha: u8) -> Vec<u8> {
    let weight = u32::from(255 - alpha);
    let keep = |unit: u8| (u32::from(unit) * weight / 256) as u8;
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|texel| {
            [
                keep(texel[0] >> 3) << 3,
                keep(texel[1] >> 2) << 2,
                keep(texel[2] >> 3) << 3,
                texel[3],
            ]
        })
        .collect()
}

/// `piece` under [`blend_black_rgb565`], labelled `label`.
fn blended_black(piece: &ChromePiece, label: String, alpha: u8) -> ChromePiece {
    ChromePiece {
        label,
        width: piece.width,
        height: piece.height,
        rgba: blend_black_rgb565(&piece.rgba, alpha),
    }
}

/// The `+0xD5` top panel's shapes, indexed by frame; `None` where missing.
#[derive(Debug, Clone, Copy, Default)]
pub struct TopPanelArt {
    pub sdtp: [Option<MainMenuShellChromeEntry>; 2],
    pub warning: [Option<MainMenuShellChromeEntry>; 6],
}

pub fn build_main_menu_shell_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
) -> Result<MainMenuShellChromeAtlas> {
    let mut art = ChromeArt::new(assets, "main-menu shell");
    art.pieces = load_main_menu_button_frames(assets)?;

    // Right-panel + lower-side chrome SHPs. These render with SHELL.PAL,
    // except SDBTNBKGD which uses SHELL2.PAL.
    if let Some(pal) = load_palette(assets, "SHELL.PAL").as_ref() {
        art.shp("SDTP.SHP", pal, 0..1, "sdtp.shp", Need::Optional);
        art.shp_run("SDTP.SHP", pal, "sdtp.shp:top", usize::MAX);
        art.shp_run("SDWRNTMP.SHP", pal, "sdwrntmp.shp:warning", usize::MAX);
        // Parent backgrounds MNSCRNS/MNSCRNL fill the screen behind the movie
        // + chrome; the native SHP canvas size is read at parse time.
        for name in [
            "SDBTM.SHP",
            "LWSCRNS.SHP",
            "LWSCRNL.SHP",
            "MNSCRNS.SHP",
            "MNSCRNL.SHP",
        ] {
            art.shp(name, pal, 0..1, &name.to_ascii_lowercase(), Need::Optional);
        }
        let darkened: Vec<ChromePiece> = art
            .pieces
            .iter()
            .filter_map(|piece| {
                let name = piece.label.strip_suffix("#0")?;
                let list = format!("{name}:list#0");
                matches!(name, "mnscrns.shp" | "mnscrnl.shp").then(|| blended_black(piece, list, 0))
            })
            .collect();
        for piece in darkened {
            art.push(piece);
        }
    } else {
        log::warn!("Missing SHELL.PAL; skipping main-menu right-panel chrome SHPs");
    }
    if let Some(pal) = load_palette(assets, "SHELL2.PAL").as_ref() {
        art.shp("SDBTNBKGD.SHP", pal, 0..1, "sdbtnbkgd.shp", Need::Optional);
        // Static 0x71C: every SDWRNANM frame through the SHELL2 path
        // (0x00603776), in frame order.
        art.shp_run("SDWRNANM.SHP", pal, "sdwrnanm.shp:monitor", usize::MAX);
    } else {
        log::warn!("Missing SHELL2.PAL; skipping main-menu right-panel tile and monitor SHPs");
    }

    let (texture, labels) = art
        .pack(gpu, batch, ATLAS_NAME)
        .context("Could not pack the required game-menu artwork")?;
    log::info!("Main-menu shell chrome atlas loaded");
    let button_wave_frames = labels.frames("sdbtnanm.shp:wave");
    let first = |label: &str| labels.frame(label, 0);
    Ok(MainMenuShellChromeAtlas {
        texture,
        button_wave_frames,
        right_panel_top_sdtp: first("sdtp.shp"),
        top_panel: TopPanelArt {
            sdtp: labels.frames("sdtp.shp:top"),
            warning: labels.frames("sdwrntmp.shp:warning"),
        },
        right_panel_tile_sdbtnbkgd: first("sdbtnbkgd.shp"),
        right_panel_bottom_sdbtm: first("sdbtm.shp"),
        lower_side_640_lwscrns: first("lwscrns.shp"),
        lower_side_large_lwscrnl: first("lwscrnl.shp"),
        parent_background_640_mnscrns: first("mnscrns.shp"),
        parent_background_large_mnscrnl: first("mnscrnl.shp"),
        parent_background_640_mnscrns_list: first("mnscrns.shp:list"),
        parent_background_large_mnscrnl_list: first("mnscrnl.shp:list"),
        warning_monitor_frames: labels.run("sdwrnanm.shp:monitor"),
    })
}

/// Decode mandatory buttons before GPU upload, retaining the asset and parser error.
fn load_main_menu_button_frames(assets: &AssetManager) -> Result<Vec<ChromePiece>> {
    let palette = read_main_menu_button_palette(assets)?;
    let shp = read_main_menu_button_shape(assets)?;
    render_main_menu_button_frames(&shp, &palette)
}

/// The installer reports both missing sources together. Keep palette fallback,
/// shape validation and frame decoding shared with the actual menu loader.
pub(crate) fn inspect_main_menu_button_art(assets: &AssetManager) -> Vec<String> {
    let palette = read_main_menu_button_palette(assets);
    let shp = read_main_menu_button_shape(assets);
    match (palette, shp) {
        (Ok(palette), Ok(shp)) => render_main_menu_button_frames(&shp, &palette)
            .err()
            .map(|error| format!("{error:#}"))
            .into_iter()
            .collect(),
        (palette, shp) => palette
            .err()
            .into_iter()
            .chain(shp.err())
            .map(|error| format!("{error:#}"))
            .collect(),
    }
}

fn read_main_menu_button_palette(assets: &AssetManager) -> Result<Palette> {
    // Preserve the existing SHELL2 fallback when the dedicated palette is unavailable.
    read_palette(assets, "SDBTNANM.PAL").or_else(|primary| {
        read_palette(assets, "SHELL2.PAL").with_context(|| {
            format!("Game-menu button palette failed: {primary:#}; fallback also failed")
        })
    })
}

fn read_main_menu_button_shape(assets: &AssetManager) -> Result<ShpFile> {
    read_ui_shp(assets, "SDBTNANM.SHP", |shp| {
        anyhow::ensure!(
            shp.frames.len() >= 5,
            "SDBTNANM.SHP has {} frames; game-menu buttons require frames 2, 3 and 4",
            shp.frames.len()
        );
        Ok(shp)
    })
}

fn render_main_menu_button_frames(shp: &ShpFile, palette: &Palette) -> Result<Vec<ChromePiece>> {
    (0..shp.frames.len().min(SDBTNANM_WAVE_FRAMES))
        .map(|frame| shp_piece(shp, &format!("sdbtnanm.shp:wave#{frame}"), palette, frame))
        .collect()
}

/// Campaign selection `0x94` art from the loader `0x0072D9A0`: the faction
/// emblems (every frame, for their hover animation) and the dialog
/// background. 640-wide screens use the small set.
pub struct CampaignShellArt {
    pub texture: BatchTexture,
    pub background: Option<MainMenuShellChromeEntry>,
    /// The background one RGB565 unit darker: what the difficulty trackbar
    /// saves and restores under itself (`0x00621B80`, colour 0 at alpha 0).
    pub background_dark: Option<MainMenuShellChromeEntry>,
    pub allied: Vec<MainMenuShellChromeEntry>,
    pub soviet: Vec<MainMenuShellChromeEntry>,
    /// Boot Camp emblem frames. Empty under YR (no boot camp); populated
    /// under `-ra2` from FSBCLG.SHP / FSBCSM.SHP.
    pub bootcamp: Vec<MainMenuShellChromeEntry>,
    /// TRAKGRIP.PCX, the 12x22 trackbar thumb.
    pub slider_thumb: Option<MainMenuShellChromeEntry>,
    /// The trackbar's two bevel boxes (`0x006208F0`) on a canvas two pixels
    /// larger on every side than the control.
    pub slider_frame: Option<MainMenuShellChromeEntry>,
}

/// SHP names by screen size (`0x0072D9A0`, `[0x008A00A4] == 640`): Allied
/// emblem, Soviet emblem, background.
pub fn campaign_art_names(small: bool) -> [&'static str; 3] {
    if small {
        ["FSASM.SHP", "FSSSM.SHP", "FSBKGDSM.SHP"]
    } else {
        ["FSALG.SHP", "FSSLG.SHP", "FSBKGDLG.SHP"]
    }
}

/// The palettes `0x0072D9A0` loads for them (`0x0072DA5C..0x0072DA96`), in
/// the same order; both screen sizes use these.
pub const CAMPAIGN_ART_PALETTES: [&str; 3] = ["FSALG.PAL", "FSSLG.PAL", "FSBKGDLG.PAL"];

/// `slider` is the difficulty trackbar's control size for this screen.
pub fn build_campaign_shell_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    small: bool,
    slider: (u32, u32),
) -> Option<CampaignShellArt> {
    let [allied_pal, soviet_pal, background_pal] = if crate::util::ra2_launch::enabled() {
        // Original RA2 ships the campaign emblems and background in ra2.mix ->
        // neutral.mix, coloured by the single shared FSSCRN.PAL beside them.
        // YR instead has three per-art palettes in ra2md.mix -> ntrlmd.mix.
        // Using the YR palettes recolours the RA2 shapes into a YR-looking
        // image, so RA2 mode uses its own campaign palette throughout.
        let campaign = load_palette(assets, "FSSCRN.PAL");
        log::debug!(
            "RA2 campaign art: FSSCRN.PAL available={}",
            campaign.is_some()
        );
        [campaign.clone(), campaign.clone(), campaign]
    } else {
        CAMPAIGN_ART_PALETTES.map(|name| load_palette(assets, name))
    };
    let [allied, soviet, background] = campaign_art_names(small);
    let allied_pal = allied_pal?;
    let soviet_pal = soviet_pal?;
    let background_pal = background_pal?;
    let mut art = ChromeArt::new(assets, "campaign shell");
    art.shp_run(allied, &allied_pal, "allied", usize::MAX);
    art.shp_run(soviet, &soviet_pal, "soviet", usize::MAX);
    // Original RA2 has a third Boot Camp emblem (control 0x6EB) between Allied
    // and Soviet. YR has no boot camp, so leave it empty there. FSBCLG/FSBCSM
    // are the large/small shapes in ra2.mix -> neutral.mix, coloured by the
    // same campaign palette as the other emblems.
    if crate::util::ra2_launch::enabled() {
        let bootcamp_shape = if small { "FSBCSM.SHP" } else { "FSBCLG.SHP" };
        art.shp_run(bootcamp_shape, &allied_pal, "bootcamp", usize::MAX);
    }
    if let Some(piece) = shp_file_piece(assets, background, "background", &background_pal, 0) {
        art.push(blended_black(&piece, "background:dark".into(), 0));
        art.push(piece);
    }
    art.pcx(["TRAKGRIP.PCX"], false, Need::Silent);
    let (width, height, rgba) =
        crate::render::skirmish_shell_chrome::trackbar_frame_rgba(slider.0, slider.1, 0);
    art.push(ChromePiece {
        label: "slider:frame".into(),
        width,
        height,
        rgba,
    });
    let (texture, labels) = art.pack(gpu, batch, ATLAS_NAME)?;
    Some(CampaignShellArt {
        texture,
        allied: labels.run("allied"),
        soviet: labels.run("soviet"),
        bootcamp: labels.run("bootcamp"),
        background: labels.get("background"),
        background_dark: labels.get("background:dark"),
        slider_thumb: labels.get("trakgrip.pcx"),
        slider_frame: labels.get("slider:frame"),
    })
}

/// A Westwood Online page's art: its dialog background and, for the welcome
/// `0x10E`, the icon glossary images.
pub struct WolPageArt {
    pub texture: BatchTexture,
    /// Frame 0 of the page's background shape through its own palette:
    /// MultiplaySelection for `0x10E` (`0x0072C7E0`), LoginScreen for
    /// `0x10F` (`0x0072C600`). Both loaders load the shape only when the
    /// screen is exactly 800 wide; otherwise no background is drawn.
    pub background: Option<MainMenuShellChromeEntry>,
    /// The kind-2 icon statics' PCX images by lower-case file name.
    pub icons: Vec<(String, MainMenuShellChromeEntry)>,
}

/// Score screen `0x108` art for the local side: the dialog background and the
/// band bars.
pub struct ScoreArt {
    pub texture: BatchTexture,
    pub background: Option<MainMenuShellChromeEntry>,
    /// The background at 37.5 % (`0x00621B80`, colour 0, weight `0x9F`): what
    /// the proc's `WM_PAINT` leaves behind the table.
    pub background_shaded: Option<MainMenuShellChromeEntry>,
    /// The ten band images in the proc's band order; `None` where the side has
    /// no bar or the PCX is missing (the band then keeps kind 0).
    pub bars: [Option<MainMenuShellChromeEntry>; 10],
}

/// One side-specific campaign score SHP, packed with all of its frames.
pub struct CampaignScoreFrameStrip {
    pub texture: BatchTexture,
    pub frames: Vec<MainMenuShellChromeEntry>,
}

/// Art owned by the campaign victory dialog `0x103`: the SCRBK background,
/// panel composition SCRT, and the SCRA sequence installed into kind-4 static
/// control `0x72B`.
pub struct CampaignScoreArt {
    pub texture: BatchTexture,
    pub background: MainMenuShellChromeEntry,
    /// Every frame from the side/resolution-specific campaign score panel SHP.
    pub scrt: Option<CampaignScoreFrameStrip>,
    /// Native side/width selection's SCRA shape (44 campaign-emblem frames).
    pub scra: Option<CampaignScoreFrameStrip>,
}

/// The stock Campaign StandaloneScore asset names selected by side and native
/// width branch (`0x72D300`/`0x72D460`): width exactly 640 chooses small; every
/// other width chooses medium. `Scenario+0x34B8` supplies side. An installed
/// Ares replaces each name with its own side buffer, so the width branch no
/// longer selects the small shapes there.
pub fn campaign_score_art_names(
    side: i32,
    screen_w: u32,
) -> (&'static str, &'static str, &'static str, &'static str) {
    let small = screen_w == 640;
    match side {
        0 if small => ("ASCRBKSM.SHP", "ASCRTSM.SHP", "ASCRASM.SHP", "ASCORE.PAL"),
        0 => ("ASCRBKMD.SHP", "ASCRTMD.SHP", "ASCRAMD.SHP", "ASCORE.PAL"),
        1 if small => ("SSCRBKSM.SHP", "SSCRTSM.SHP", "SSCRASM.SHP", "SSCORE.PAL"),
        1 => ("SSCRBKMD.SHP", "SSCRTMD.SHP", "SSCRAMD.SHP", "SSCORE.PAL"),
        _ if small => ("SSCRBKSM.SHP", "SSCRTSM.SHP", "SSCRASM.SHP", "YSCORE.PAL"),
        _ => ("SYCRBKMD.SHP", "SYCRTMD.SHP", "SYCRAMD.SHP", "YSCORE.PAL"),
    }
}

pub fn build_campaign_score_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    side: i32,
    screen_w: u32,
    art: crate::rules::ruleset::SideScoreArt<'_>,
) -> Option<CampaignScoreArt> {
    let (background_name, scrt_name, scra_name, palette_name) =
        campaign_score_art_names(side, screen_w);
    let background_name = score_name(art.campaign_background, background_name);
    let palette = load_palette(assets, score_name(art.campaign_palette, palette_name))?;
    let background = shp_file_piece(assets, background_name, background_name, &palette, 0)?;
    let (texture, mut entries) = pack_chrome_atlas(gpu, batch, ATLAS_NAME, &[background])?;
    // SCRT and SCRA are distinct native SHPs. Keep their strips in separate
    // atlases so every frame remains independently addressable.
    let strip = |name: &str| {
        let mut frames = ChromeArt::new(assets, "campaign score");
        if frames.shp_run(name, &palette, name, usize::MAX) == 0 {
            log::warn!("Could not load campaign score shape frames {name}");
            return None;
        }
        let (texture, frames) = pack_chrome_atlas(gpu, batch, ATLAS_NAME, &frames.pieces)?;
        Some(CampaignScoreFrameStrip { texture, frames })
    };
    Some(CampaignScoreArt {
        texture,
        background: entries.pop()?,
        scrt: strip(score_name(art.campaign_transition, scrt_name)),
        scra: strip(score_name(art.campaign_animation, scra_name)),
    })
}

/// Background shape and palette by side (`0x0072D830`, `0x0072D730`): the
/// small shape only at exactly 640 wide, and Yuri's small shape is Soviet's.
pub fn score_art_names(side: i32, screen_w: u32) -> (&'static str, &'static str) {
    let (small, large, palette) = match side {
        0 => ("MPASCRNS.SHP", "MPASCRNL.SHP", "MPASCRN.PAL"),
        1 => ("MPSSCRNS.SHP", "MPSSCRNL.SHP", "MPSSCRN.PAL"),
        _ => ("MPSSCRNS.SHP", "MPYSCRNL.SHP", "MPYSCRN.PAL"),
    };
    (if screen_w == 640 { small } else { large }, palette)
}

/// Each score name is substituted on its own, the way the hook copies every
/// SideExt field over its stock buffer (`0x1002F270`).
fn score_name<'a>(name: Option<&'a str>, stock: &'a str) -> &'a str {
    name.unwrap_or(stock)
}

/// Band `band`'s bar: the side's `MultiplayerScore.Bars=` pattern with the first
/// `~~` replaced by the two-digit band number (`Game_GetMultiplayerScoreScreenBar`
/// `0x1002F300`), else the stock name by side (tables `0x00844B24`, `0x00844B4C`,
/// `0x00844B74`); any other side has none.
pub fn score_bar_name(bars: Option<&str>, side: i32, band: usize) -> Option<String> {
    if let Some(pattern) = bars {
        return Some(pattern.replacen("~~", &format!("{:02}", band + 1), 1));
    }
    let side = match side {
        0 => 'a',
        1 => 's',
        2 => 'y',
        _ => return None,
    };
    Some(format!("mp{side}scrnlbar{:02}.pcx", band + 1))
}

/// A page background: frame 0 of `shape` through `palette`, `None` (logged)
/// when either is unavailable.
fn background_piece(assets: &AssetManager, (shape, palette): (&str, &str)) -> Option<ChromePiece> {
    shp_file_piece(
        assets,
        shape,
        "background",
        &load_palette(assets, palette)?,
        0,
    )
}

pub fn build_score_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    side: i32,
    screen_w: u32,
    art: crate::rules::ruleset::SideScoreArt<'_>,
) -> Option<ScoreArt> {
    let (shape, palette) = score_art_names(side, screen_w);
    let shape = score_name(art.multiplayer_background, shape);
    let palette = score_name(art.multiplayer_palette, palette);
    let mut pieces = ChromeArt::new(assets, "score");
    if let Some(piece) = background_piece(assets, (shape, palette)) {
        pieces.push(blended_black(&piece, "background:shaded".into(), 0x9F));
        pieces.push(piece);
    } else {
        log::warn!("Missing score background {shape}");
    }
    for band in 0..10 {
        let Some(file) = score_bar_name(art.multiplayer_bars, side, band) else {
            break;
        };
        // Kind-2 image statics key magenta (`0x006153E0`).
        let bar = pcx_piece(assets, &file, true).map(|piece| ChromePiece {
            label: format!("bar:{band}"),
            ..piece
        });
        pieces.push_or(bar, &file, Need::Optional);
    }
    let (texture, labels) = pieces.pack(gpu, batch, ATLAS_NAME)?;
    Some(ScoreArt {
        texture,
        background: labels.get("background"),
        background_shaded: labels.get("background:shaded"),
        bars: std::array::from_fn(|band| labels.get(&format!("bar:{band}"))),
    })
}

/// `background` is the page's `(shape, palette)` file pair.
pub fn build_wol_page_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    screen_w: u32,
    background: (&str, &str),
    icon_files: &[&str],
) -> Option<WolPageArt> {
    let mut art = ChromeArt::new(assets, "Westwood Online page");
    if screen_w == 800
        && let Some(piece) = background_piece(assets, background)
    {
        art.push(piece);
    }
    for file in icon_files {
        let name = file.to_ascii_lowercase();
        if !art.pieces.iter().any(|piece| piece.label == name) {
            // Kind-2 image statics key magenta (`0x006153E0`).
            art.pcx([*file], true, Need::Optional);
        }
    }
    let (texture, labels) = art.pack(gpu, batch, ATLAS_NAME)?;
    Some(WolPageArt {
        texture,
        background: labels.get("background"),
        icons: art
            .pieces
            .iter()
            .filter(|piece| piece.label != "background")
            .filter_map(|piece| Some((piece.label.clone(), labels.get(&piece.label)?)))
            .collect(),
    })
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn mandatory_button_loader_preserves_failure_details_and_palette_fallback() {
        let root = std::env::temp_dir().join(format!("vera-menu-buttons-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let load = || load_main_menu_button_frames(&AssetManager::from_loose_root_for_test(&root));
        let missing_palettes = format!("{:#}", load().err().expect("palettes are absent"));
        assert!(
            missing_palettes.contains("SDBTNANM.PAL"),
            "{missing_palettes}"
        );
        assert!(
            missing_palettes.contains("SHELL2.PAL"),
            "{missing_palettes}"
        );

        std::fs::write(root.join("SDBTNANM.PAL"), b"bad").unwrap();
        let corrupt_palette = format!("{:#}", load().err().expect("palette is malformed"));
        assert!(
            corrupt_palette.contains("SDBTNANM.PAL"),
            "{corrupt_palette}"
        );
        assert!(corrupt_palette.contains("768"), "{corrupt_palette}");
        // The existing SHELL2 fallback remains usable even with a bad primary palette.
        std::fs::write(root.join("SHELL2.PAL"), [0u8; 768]).unwrap();
        let missing_shape = format!("{:#}", load().err().expect("button shape is absent"));
        assert!(missing_shape.contains("SDBTNANM.SHP"), "{missing_shape}");
        assert!(missing_shape.contains("missing"), "{missing_shape}");

        std::fs::write(root.join("SDBTNANM.SHP"), b"bad").unwrap();
        let corrupt_shape = format!("{:#}", load().err().expect("button shape is malformed"));
        assert!(corrupt_shape.contains("SDBTNANM.SHP"), "{corrupt_shape}");
        assert!(corrupt_shape.contains("3 bytes"), "{corrupt_shape}");

        // A valid four-frame file still lacks the required pressed state (frame 4).
        let mut shp = vec![0u8; 8 + 4 * 24];
        shp[2..4].copy_from_slice(&1u16.to_le_bytes());
        shp[4..6].copy_from_slice(&1u16.to_le_bytes());
        shp[6..8].copy_from_slice(&4u16.to_le_bytes());
        std::fs::write(root.join("SDBTNANM.SHP"), &shp).unwrap();
        let short_shape = format!("{:#}", load().err().expect("pressed state is absent"));
        assert!(
            short_shape.contains("SDBTNANM.SHP has 4 frames"),
            "{short_shape}"
        );

        shp.resize(8 + 5 * 24, 0);
        shp[6..8].copy_from_slice(&5u16.to_le_bytes());
        std::fs::write(root.join("SDBTNANM.SHP"), &shp).unwrap();
        let frames = load().expect("five states and fallback palette are sufficient");
        assert_eq!(frames.len(), 5, "extra reveal frames remain optional");
        assert_eq!(frames[4].label, "sdbtnanm.shp:wave#4");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn menu_buttons_try_a_compatible_mod_shape_before_stock() {
        use crate::assets::asset_manager::MediaArchiveMode;
        use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};

        let shape = |count: u16| {
            let mut bytes = vec![0u8; 8 + usize::from(count) * 24];
            bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
            bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
            bytes[6..8].copy_from_slice(&count.to_le_bytes());
            bytes
        };
        let install = audio_test_install("shell-button-install");
        let mod_dir = TestDirectory::new("shell-button-mod");
        install.write_mix("langmd.mix", "SDBTNANM.SHP", &shape(5));
        mod_dir.write_mix("expandmd99.mix", "SDBTNANM.SHP", &shape(4));
        mod_dir.write_mix("expandmd98.mix", "SDBTNANM.SHP", &shape(6));
        std::fs::write(install.path().join("SDBTNANM.PAL"), [0u8; 768]).unwrap();
        let assets = AssetManager::with_mod(
            install.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let frames = load_main_menu_button_frames(&assets).unwrap();
        assert_eq!(frames.len(), 6);
    }

    #[test]
    fn score_shading_keeps_three_eighths_of_every_rgb565_unit() {
        // Retail still at 800x600: art units -> units inside the rectangle.
        for (art, shaded) in [
            ([23u8, 27, 7], [8u8, 10, 2]),
            ([20, 24, 5], [7, 9, 1]),
            ([9, 10, 1], [3, 3, 0]),
        ] {
            let texel = [art[0] << 3, art[1] << 2, art[2] << 3, 255];
            let out = blend_black_rgb565(&texel, 0x9F);
            assert_eq!([out[0] >> 3, out[1] >> 2, out[2] >> 3], shaded, "{art:?}");
        }
    }

    #[test]
    fn score_art_follows_the_side_and_the_width() {
        assert_eq!(score_art_names(0, 800), ("MPASCRNL.SHP", "MPASCRN.PAL"));
        assert_eq!(score_art_names(1, 640), ("MPSSCRNS.SHP", "MPSSCRN.PAL"));
        assert_eq!(score_art_names(2, 640), ("MPSSCRNS.SHP", "MPYSCRN.PAL"));
        assert_eq!(score_art_names(2, 1024), ("MPYSCRNL.SHP", "MPYSCRN.PAL"));
        assert_eq!(score_art_names(4, 800), ("MPYSCRNL.SHP", "MPYSCRN.PAL"));
        assert_eq!(
            score_bar_name(None, 0, 0).as_deref(),
            Some("mpascrnlbar01.pcx")
        );
        assert_eq!(
            score_bar_name(None, 2, 9).as_deref(),
            Some("mpyscrnlbar10.pcx")
        );
        assert_eq!(score_bar_name(None, 3, 0), None);
        // The side's own pattern receives the band number at its first `~~`,
        // and a pattern without the marker keeps every band on one file.
        assert_eq!(
            score_bar_name(Some("MyBars~~.pcx"), 7, 2).as_deref(),
            Some("MyBars03.pcx")
        );
        assert_eq!(
            score_bar_name(Some("only.pcx"), 7, 5).as_deref(),
            Some("only.pcx")
        );
    }

    #[test]
    fn campaign_score_art_uses_standalone_side_tables_and_native_width_branch() {
        assert_eq!(
            campaign_score_art_names(0, 640),
            ("ASCRBKSM.SHP", "ASCRTSM.SHP", "ASCRASM.SHP", "ASCORE.PAL")
        );
        assert_eq!(
            campaign_score_art_names(1, 800),
            ("SSCRBKMD.SHP", "SSCRTMD.SHP", "SSCRAMD.SHP", "SSCORE.PAL")
        );
        assert_eq!(
            campaign_score_art_names(2, 640),
            ("SSCRBKSM.SHP", "SSCRTSM.SHP", "SSCRASM.SHP", "YSCORE.PAL")
        );
        assert_eq!(
            campaign_score_art_names(2, 800),
            ("SYCRBKMD.SHP", "SYCRTMD.SHP", "SYCRAMD.SHP", "YSCORE.PAL")
        );
    }

    #[test]
    fn list_darkening_removes_one_rgb565_unit_and_saturates() {
        let texels = [
            8, 4, 8, 255, // one unit each
            0, 0, 0, 255, // already black
            255, 255, 255, 7, // full intensity keeps alpha
            15, 7, 23, 255, // low bits below one unit are dropped
        ];
        let dark = blend_black_rgb565(&texels, 0);
        assert_eq!(&dark[0..4], &[0, 0, 0, 255]);
        assert_eq!(&dark[4..8], &[0, 0, 0, 255]);
        assert_eq!(&dark[8..12], &[240, 248, 240, 7]);
        assert_eq!(&dark[12..16], &[0, 0, 8, 255]);
        // Presenter view: every non-zero 565 unit drops by exactly one.
        for (before, after) in texels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dark.as_chunks::<4>().0)
        {
            assert_eq!(after[0] >> 3, (before[0] >> 3).saturating_sub(1));
            assert_eq!(after[1] >> 2, (before[1] >> 2).saturating_sub(1));
            assert_eq!(after[2] >> 3, (before[2] >> 3).saturating_sub(1));
        }
    }
}
