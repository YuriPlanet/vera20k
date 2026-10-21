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
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;

const ATLAS_PADDING: u32 = 2;

/// SDBTNANM.SHP frames baked for the first-paint slide ramp. The reveal animates
/// "active" buttons through frames 5..=10 and "inactive" slots through 11..=16,
/// so the full 0..=16 set is loaded; missing frames stay `None` and the wave
/// clamps to the nearest available frame.
const SDBTNANM_WAVE_FRAMES: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainMenuShellChromeEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

impl MainMenuShellChromeEntry {
    /// The image's native width and height in pixels.
    pub fn native_size(self) -> (i32, i32) {
        (
            self.pixel_size[0].round() as i32,
            self.pixel_size[1].round() as i32,
        )
    }
}

pub struct MainMenuShellChromeAtlas {
    pub texture: BatchTexture,
    /// SDBTNANM.SHP frame 2 — default (unhovered, unpressed) button art.
    pub button_default: MainMenuShellChromeEntry,
    /// SDBTNANM.SHP frame 3 — hover state.
    pub button_hover: MainMenuShellChromeEntry,
    /// SDBTNANM.SHP frame 4 — pressed state.
    pub button_pressed: MainMenuShellChromeEntry,
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
    /// MNSCRNS / MNSCRNL with every RGB565 channel one unit darker
    /// (saturating): what an owner-draw ListBox with translucency 0 paints
    /// under its rows (`0x00619230`), derived here so the list interior can be
    /// composited exactly before the rows are drawn over it.
    pub parent_background_640_mnscrns_list: Option<MainMenuShellChromeEntry>,
    pub parent_background_large_mnscrnl_list: Option<MainMenuShellChromeEntry>,
    /// Static `0x71C` SDWRNANM frames in order (SHELL2 palette); empty when
    /// the SHP is missing.
    pub warning_monitor_frames: Vec<MainMenuShellChromeEntry>,
    /// Opaque white texel block for solid fills (list frames, selection).
    pub white_pixel: Option<MainMenuShellChromeEntry>,
    /// Owner-draw list scrollbar art (`0x0061C690`): 18x22 arrows, released
    /// and pressed, and the 18-wide grip top, middle and bottom.
    pub list_scroll: ListScrollArt,
}

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

struct RenderedChromeEntry {
    label: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
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
    let mut rendered = load_main_menu_button_frames(assets)?;

    // Right-panel + lower-side chrome SHPs. These render with SHELL.PAL,
    // except SDBTNBKGD which uses SHELL2.PAL.
    let shell_palette = load_named_palette(assets, "SHELL.PAL");
    let shell2_palette = load_named_palette(assets, "SHELL2.PAL");
    if let Some(pal) = shell_palette.as_ref() {
        push_optional_shp(&mut rendered, assets, "SDTP.SHP", pal, 0);
        rendered.extend(render_shp_frames(assets, "SDTP.SHP", pal, "top"));
        rendered.extend(render_shp_frames(assets, "SDWRNTMP.SHP", pal, "warning"));
        push_optional_shp(&mut rendered, assets, "SDBTM.SHP", pal, 0);
        push_optional_shp(&mut rendered, assets, "LWSCRNS.SHP", pal, 0);
        push_optional_shp(&mut rendered, assets, "LWSCRNL.SHP", pal, 0);
        // Parent backgrounds that fill the screen behind the movie + chrome.
        // Native SHP canvas size is read at parse time (not hardcoded).
        push_optional_shp(&mut rendered, assets, "MNSCRNS.SHP", pal, 0);
        push_optional_shp(&mut rendered, assets, "MNSCRNL.SHP", pal, 0);
        let darkened: Vec<RenderedChromeEntry> = rendered
            .iter()
            .filter(|entry| entry.label == "mnscrns.shp" || entry.label == "mnscrnl.shp")
            .map(|entry| RenderedChromeEntry {
                label: format!("{}:list", entry.label),
                width: entry.width,
                height: entry.height,
                rgba: blend_black_rgb565(&entry.rgba, 0),
            })
            .collect();
        rendered.extend(darkened);
    } else {
        log::warn!("Missing SHELL.PAL; skipping main-menu right-panel chrome SHPs");
    }
    if let Some(pal) = shell2_palette.as_ref() {
        push_optional_shp(&mut rendered, assets, "SDBTNBKGD.SHP", pal, 0);
        // Static 0x71C: every SDWRNANM frame through the SHELL2 path
        // (0x00603776), in frame order.
        rendered.extend(render_shp_frames(assets, "SDWRNANM.SHP", pal, "monitor"));
    } else {
        log::warn!("Missing SHELL2.PAL; skipping main-menu right-panel tile and monitor SHPs");
    }

    for name in [
        "UPARROWR.PCX",
        "UPARROWP.PCX",
        "DNARROWR.PCX",
        "DNARROWP.PCX",
        "SBGRIPT.PCX",
        "SBGRIPM.PCX",
        "SBGRIPB.PCX",
    ] {
        if let Some(entry) = render_pcx_entry(assets, name) {
            rendered.push(entry);
        }
    }

    rendered.push(RenderedChromeEntry {
        label: "white".into(),
        width: 2,
        height: 2,
        rgba: vec![0xFF; 2 * 2 * 4],
    });

    let (texture, packed) = pack_entries(gpu, batch, &rendered)
        .context("Could not pack the required game-menu artwork")?;
    let mut by_label: std::collections::HashMap<String, MainMenuShellChromeEntry> =
        std::collections::HashMap::new();
    for (entry, placed) in rendered.iter().zip(packed.iter().copied()) {
        by_label.insert(entry.label.clone(), placed);
    }
    log::info!("Main-menu shell chrome atlas loaded");
    // The loader validates these three required states before uploading the atlas.
    let button_default = by_label["sdbtnanm.shp:wave#2"];
    let button_hover = by_label["sdbtnanm.shp:wave#3"];
    let button_pressed = by_label["sdbtnanm.shp:wave#4"];
    Ok(MainMenuShellChromeAtlas {
        texture,
        button_default,
        button_hover,
        button_pressed,
        button_wave_frames: std::array::from_fn(|frame| {
            by_label.get(&format!("sdbtnanm.shp:wave#{frame}")).copied()
        }),
        right_panel_top_sdtp: by_label.get("sdtp.shp").copied(),
        top_panel: TopPanelArt {
            sdtp: std::array::from_fn(|frame| {
                by_label.get(&format!("sdtp.shp:top#{frame}")).copied()
            }),
            warning: std::array::from_fn(|frame| {
                by_label
                    .get(&format!("sdwrntmp.shp:warning#{frame}"))
                    .copied()
            }),
        },
        right_panel_tile_sdbtnbkgd: by_label.get("sdbtnbkgd.shp").copied(),
        right_panel_bottom_sdbtm: by_label.get("sdbtm.shp").copied(),
        lower_side_640_lwscrns: by_label.get("lwscrns.shp").copied(),
        lower_side_large_lwscrnl: by_label.get("lwscrnl.shp").copied(),
        parent_background_640_mnscrns: by_label.get("mnscrns.shp").copied(),
        parent_background_large_mnscrnl: by_label.get("mnscrnl.shp").copied(),
        parent_background_640_mnscrns_list: by_label.get("mnscrns.shp:list").copied(),
        parent_background_large_mnscrnl_list: by_label.get("mnscrnl.shp:list").copied(),
        list_scroll: ListScrollArt {
            up_released: by_label.get("uparrowr.pcx").copied(),
            up_pressed: by_label.get("uparrowp.pcx").copied(),
            down_released: by_label.get("dnarrowr.pcx").copied(),
            down_pressed: by_label.get("dnarrowp.pcx").copied(),
            grip_top: by_label.get("sbgript.pcx").copied(),
            grip_mid: by_label.get("sbgripm.pcx").copied(),
            grip_bottom: by_label.get("sbgripb.pcx").copied(),
        },
        warning_monitor_frames: (0..)
            .map_while(|frame| {
                by_label
                    .get(&format!("sdwrnanm.shp:monitor#{frame}"))
                    .copied()
            })
            .collect(),
        white_pixel: by_label.get("white").copied().map(|mut entry| {
            // Sample only the block's center so filtering never reaches padding.
            entry.uv_origin[0] += entry.uv_size[0] * 0.25;
            entry.uv_origin[1] += entry.uv_size[1] * 0.25;
            entry.uv_size = [entry.uv_size[0] * 0.5, entry.uv_size[1] * 0.5];
            entry
        }),
    })
}

/// Decode mandatory buttons before GPU upload, retaining the asset and parser error.
fn load_main_menu_button_frames(assets: &AssetManager) -> Result<Vec<RenderedChromeEntry>> {
    // Preserve the existing SHELL2 fallback when the dedicated palette is unavailable.
    let palette = read_named_palette(assets, "SDBTNANM.PAL").or_else(|primary| {
        read_named_palette(assets, "SHELL2.PAL").with_context(|| {
            format!("Game-menu button palette failed: {primary:#}; fallback also failed")
        })
    })?;
    let load = assets
        .load_file_from_mix("SDBTNANM.SHP")
        .context("Required game-menu artwork SDBTNANM.SHP is missing or unreadable")?;
    let shp = ShpFile::from_bytes(&load.bytes).context("Could not parse SDBTNANM.SHP")?;
    anyhow::ensure!(
        shp.frames.len() >= 5,
        "SDBTNANM.SHP has {} frames; game-menu buttons require frames 2, 3 and 4",
        shp.frames.len()
    );
    (0..shp.frames.len().min(SDBTNANM_WAVE_FRAMES))
        .map(|frame| {
            shp_frame_entry(
                &shp,
                "SDBTNANM.SHP",
                &palette,
                frame,
                Some(&format!("wave#{frame}")),
            )
        })
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
    let [allied_pal, soviet_pal, background_pal] =
        CAMPAIGN_ART_PALETTES.map(|name| load_named_palette(assets, name));
    let [allied, soviet, background] = campaign_art_names(small);
    let mut rendered = render_shp_frames(assets, allied, &allied_pal?, "frame");
    let allied_count = rendered.len();
    rendered.extend(render_shp_frames(assets, soviet, &soviet_pal?, "frame"));
    let soviet_count = rendered.len() - allied_count;
    let mut labels: Vec<&str> = Vec::new();
    if let Some(entry) = render_shp_entry(assets, background, &background_pal?, 0, None) {
        rendered.push(RenderedChromeEntry {
            label: "background:dark".into(),
            width: entry.width,
            height: entry.height,
            rgba: blend_black_rgb565(&entry.rgba, 0),
        });
        rendered.push(entry);
        labels.extend(["background:dark", "background"]);
    }
    if let Some(entry) = render_pcx_entry(assets, "TRAKGRIP.PCX") {
        rendered.push(entry);
        labels.push("thumb");
    }
    let (frame_w, frame_h, frame_rgba) =
        crate::render::skirmish_shell_chrome::trackbar_frame_rgba(slider.0, slider.1, 0);
    rendered.push(RenderedChromeEntry {
        label: "slider:frame".into(),
        width: frame_w,
        height: frame_h,
        rgba: frame_rgba,
    });
    labels.push("frame");
    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    let extra = |name: &str| {
        labels
            .iter()
            .position(|label| *label == name)
            .map(|index| packed[allied_count + soviet_count + index])
    };
    Some(CampaignShellArt {
        texture,
        allied: packed[..allied_count].to_vec(),
        soviet: packed[allied_count..allied_count + soviet_count].to_vec(),
        background: extra("background"),
        background_dark: extra("background:dark"),
        slider_thumb: extra("thumb"),
        slider_frame: extra("frame"),
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

/// Magenta is transparent in a kind-2 image static, compared in the surface's
/// packed format (`OwnerDraw_Static_006153E0`).
const IMAGE_STATIC_TRANSPARENT_RGB: [u8; 3] = [255, 0, 255];

/// A kind-2 image static's PCX (the PCX cache `0x006BA140`), keyed to its
/// transparent colour and labelled by its lower-case name.
fn render_image_static_pcx(assets: &AssetManager, file: &str) -> Option<RenderedChromeEntry> {
    let bytes = assets.get_ref(file)?;
    let pcx = PcxFile::from_bytes(bytes)
        .map_err(|err| log::warn!("Could not parse {file}: {err:#}"))
        .ok()?;
    let mut rgba = pcx.to_rgba(None);
    crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
        .apply_packed_color_key_rgba8(&mut rgba, IMAGE_STATIC_TRANSPARENT_RGB);
    Some(RenderedChromeEntry {
        label: file.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
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

/// Campaign StandaloneScore asset names selected by side and native width
/// branch (`0x72D300`/`0x72D460`): width exactly 640 chooses small; every
/// other width chooses medium. `Scenario+0x34B8` supplies side.
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
) -> Option<CampaignScoreArt> {
    let (background_name, scrt_name, scra_name, palette_name) =
        campaign_score_art_names(side, screen_w);
    let palette = load_named_palette(assets, palette_name)?;
    let background = render_shp_entry(assets, background_name, &palette, 0, None)?;
    let (texture, mut entries) = pack_entries(gpu, batch, &[background])?;
    // SCRT and SCRA are distinct native SHPs. Keep their strips in separate
    // atlases so every frame remains independently addressable.
    let scrt = build_campaign_score_frame_strip(gpu, batch, assets, &palette, scrt_name);
    let scra = build_campaign_score_frame_strip(gpu, batch, assets, &palette, scra_name);
    Some(CampaignScoreArt {
        texture,
        background: entries.pop()?,
        scrt,
        scra,
    })
}

fn build_campaign_score_frame_strip(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    palette: &Palette,
    name: &str,
) -> Option<CampaignScoreFrameStrip> {
    let rendered = render_shp_frames(assets, name, palette, "frame");
    if rendered.is_empty() {
        log::warn!("Could not load campaign score shape frames {name}");
        return None;
    }
    let (texture, frames) = pack_entries(gpu, batch, &rendered)?;
    Some(CampaignScoreFrameStrip { texture, frames })
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

/// Band `band`'s bar by side (`0x005CA110`, tables `0x00844B24`,
/// `0x00844B4C`, `0x00844B74`); any other side has none.
pub fn score_bar_name(side: i32, band: usize) -> Option<String> {
    let side = match side {
        0 => 'a',
        1 => 's',
        2 => 'y',
        _ => return None,
    };
    Some(format!("mp{side}scrnlbar{:02}.pcx", band + 1))
}

pub fn build_score_art(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    side: i32,
    screen_w: u32,
) -> Option<ScoreArt> {
    let (shape, palette) = score_art_names(side, screen_w);
    let mut rendered = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    if let Some(palette) = load_named_palette(assets, palette)
        && let Some(entry) = render_shp_entry(assets, shape, &palette, 0, None)
    {
        rendered.push(RenderedChromeEntry {
            label: "background:shaded".into(),
            width: entry.width,
            height: entry.height,
            rgba: blend_black_rgb565(&entry.rgba, 0x9F),
        });
        rendered.push(entry);
        labels.extend(["background:shaded".into(), "background".into()]);
    } else {
        log::warn!("Missing score background {shape}");
    }
    for band in 0..10 {
        let Some(file) = score_bar_name(side, band) else {
            break;
        };
        match render_image_static_pcx(assets, &file) {
            Some(entry) => {
                rendered.push(entry);
                labels.push(format!("bar:{band}"));
            }
            None => log::warn!("Missing score bar {file}"),
        }
    }
    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    let find = |name: &str| {
        labels
            .iter()
            .position(|label| label == name)
            .map(|index| packed[index])
    };
    Some(ScoreArt {
        texture,
        background: find("background"),
        background_shaded: find("background:shaded"),
        bars: std::array::from_fn(|band| find(&format!("bar:{band}"))),
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
    let mut rendered = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let (shape, palette) = background;
    if screen_w == 800
        && let Some(palette) = load_named_palette(assets, palette)
        && let Some(entry) = render_shp_entry(assets, shape, &palette, 0, None)
    {
        rendered.push(entry);
        labels.push("background".into());
    }
    for file in icon_files {
        let name = file.to_ascii_lowercase();
        if labels.contains(&name) {
            continue;
        }
        let Some(entry) = render_image_static_pcx(assets, file) else {
            log::warn!("Missing Westwood Online glossary icon {file}");
            continue;
        };
        rendered.push(entry);
        labels.push(name);
    }
    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    let mut background = None;
    let mut icons = Vec::new();
    for (label, entry) in labels.into_iter().zip(packed) {
        if label == "background" {
            background = Some(entry);
        } else {
            icons.push((label, entry));
        }
    }
    Some(WolPageArt {
        texture,
        background,
        icons,
    })
}

fn load_named_palette(assets: &AssetManager, name: &str) -> Option<Palette> {
    read_named_palette(assets, name)
        .map_err(|err| log::warn!("{err:#}"))
        .ok()
}

/// Shell palettes come from ConvertClass__BuildFromPaletteFile `0x0072ADE0`
/// (SHELL.PAL is read at `0x0072ABDC`), the shared palette conversion.
fn read_named_palette(assets: &AssetManager, name: &str) -> Result<Palette> {
    let bytes = assets
        .get_ref(name)
        .with_context(|| format!("Palette {name} is missing or unreadable"))?;
    Palette::from_bytes(bytes).with_context(|| format!("Could not parse palette {name}"))
}

fn push_optional_shp(
    out: &mut Vec<RenderedChromeEntry>,
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
) {
    match render_shp_entry(assets, file_name, palette, frame, None) {
        Some(entry) => out.push(entry),
        None => log::warn!("Missing optional main-menu shell asset {file_name}"),
    }
}

fn render_shp_entry(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
    tag: Option<&str>,
) -> Option<RenderedChromeEntry> {
    let load = assets.load_file_from_mix(file_name)?;
    let shp = ShpFile::from_bytes(&load.bytes).ok()?;
    shp_frame_entry(&shp, file_name, palette, frame, tag).ok()
}

/// Every frame of one SHP, parsed once, labelled `<file>:<prefix>#<frame>`.
fn render_shp_frames(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    tag_prefix: &str,
) -> Vec<RenderedChromeEntry> {
    let Some(load) = assets.load_file_from_mix(file_name) else {
        return Vec::new();
    };
    let Ok(shp) = ShpFile::from_bytes(&load.bytes) else {
        log::warn!("Could not parse {file_name}");
        return Vec::new();
    };
    (0..shp.frames.len())
        .map_while(|frame| {
            shp_frame_entry(
                &shp,
                file_name,
                palette,
                frame,
                Some(&format!("{tag_prefix}#{frame}")),
            )
            .ok()
        })
        .collect()
}

/// One frame placed on its full SHP canvas.
fn shp_frame_entry(
    shp: &ShpFile,
    file_name: &str,
    palette: &Palette,
    frame: usize,
    tag: Option<&str>,
) -> Result<RenderedChromeEntry> {
    let frame_rgba = shp
        .frame_to_rgba(frame, palette)
        .with_context(|| format!("Could not render {file_name} frame {frame}"))?;
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
    let label = match tag {
        Some(t) => format!("{}:{}", file_name.to_ascii_lowercase(), t),
        None => file_name.to_ascii_lowercase(),
    };
    Ok(RenderedChromeEntry {
        label,
        width: canvas_w,
        height: canvas_h,
        rgba,
    })
}

/// A PCX in its own palette, full canvas.
fn render_pcx_entry(assets: &AssetManager, file_name: &str) -> Option<RenderedChromeEntry> {
    let bytes = assets.get_ref(file_name)?;
    let pcx = PcxFile::from_bytes(bytes).ok()?;
    Some(RenderedChromeEntry {
        label: file_name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba: pcx.to_rgba(None),
    })
}

fn pack_entries(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    entries: &[RenderedChromeEntry],
) -> Option<(BatchTexture, Vec<MainMenuShellChromeEntry>)> {
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
        "Main-menu shell chrome atlas: {}x{} px, {} pieces",
        atlas_width,
        atlas_height,
        entries.len()
    );
    for entry in entries {
        log::info!("  {}: {}x{}", entry.label, entry.width, entry.height);
    }

    let texture = batch.create_texture(gpu, &rgba, atlas_width, atlas_height);
    let atlas_entries = entries
        .iter()
        .zip(placements)
        .map(|(entry, (px, py))| MainMenuShellChromeEntry {
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
        assert_eq!(score_bar_name(0, 0).as_deref(), Some("mpascrnlbar01.pcx"));
        assert_eq!(score_bar_name(2, 9).as_deref(), Some("mpyscrnlbar10.pcx"));
        assert_eq!(score_bar_name(3, 0), None);
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
