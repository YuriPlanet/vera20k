//! Skirmish shell chrome atlas.
//!
//! Loads verified offline Skirmish dialog `0x102` shell art, then packs it
//! into a GPU texture for batched drawing. Assets without direct active
//! Skirmish evidence are research candidates and must not be rendered by the
//! default shell path.

use std::collections::HashMap;

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;

const ATLAS_PADDING: u32 = 2;
const OWNER_DRAW_FLAG_TRANSPARENT_RGB: [u8; 3] = [255, 0, 255];
const PRIMITIVE_BEVEL_COLOR_A_RGB: [u8; 3] = [0xC5, 0xBE, 0xA7];
const PRIMITIVE_BEVEL_COLOR_B_RGB: [u8; 3] = [0x80, 0x7A, 0x68];
const TRACKBAR_CONTROL_W: u32 = 128;
const TRACKBAR_CONTROL_H: u32 = 21;
const TRACKBAR_VALUE_PLAQUE_W: i32 = 50;
const TRACKBAR_FRAME_BORDER: i32 = 2;
const TRACKBAR_VALUE_FRAME_INSET: i32 = 2;
const SKIRMISH_FLAG_PCX_NAMES: [&str; 12] = [
    "usai.pcx", "japi.pcx", "frai.pcx", "geri.pcx", "gbri.pcx", "djbi.pcx", "arbi.pcx", "lati.pcx",
    "rusi.pcx", "yrii.pcx", "obsi.pcx", "rani.pcx",
];

#[derive(Debug, Clone, Copy)]
pub struct SkirmishShellChromeEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

pub struct SkirmishShellChromeAtlas {
    pub texture: BatchTexture,
    pub right_panel_top_sdtp: Option<SkirmishShellChromeEntry>,
    pub right_panel_top_highlight_sdtp_frame1: Option<SkirmishShellChromeEntry>,
    pub right_panel_tile_sdbtnbkgd: Option<SkirmishShellChromeEntry>,
    pub right_panel_overlay_sdbtnanm_frame10: Option<SkirmishShellChromeEntry>,
    pub right_panel_button_sdbtnanm_frame2: Option<SkirmishShellChromeEntry>,
    pub right_panel_button_sdbtnanm_frame3: Option<SkirmishShellChromeEntry>,
    pub right_panel_button_sdbtnanm_frame4: Option<SkirmishShellChromeEntry>,
    /// Full SDBTNANM frame range (indices 0..=16) for the slide-in wave; each
    /// `None` when the loaded SHP lacks that frame (draw clamps, never panics).
    pub right_panel_button_sdbtnanm_frames: [Option<SkirmishShellChromeEntry>; 17],
    pub right_panel_bottom_sdbtm: Option<SkirmishShellChromeEntry>,
    /// D5 static71C: 6038F7->6039EF selects SDWRNANM through SHELL2.PAL.
    pub launcher_warning_frames: Vec<SkirmishShellChromeEntry>,
    pub sd_map_button: Option<SkirmishShellChromeEntry>,
    /// SDMPBTN.SHP frames 0..=6: the map button in place (0) to only its rail
    /// left (6), drawn by the slide engine (`0x006071E0`).
    pub sd_map_button_frames: [Option<SkirmishShellChromeEntry>; 7],
    /// SDWRNTMP.SHP frames 0..=5: the top panel's warning display moving while
    /// the slide engine runs (SHELL.PAL, `0x0072E280`).
    pub sd_warning_frames: [Option<SkirmishShellChromeEntry>; 6],
    pub background_640_mnscrns: Option<SkirmishShellChromeEntry>,
    pub background_800_coop_game_setup: Option<SkirmishShellChromeEntry>,
    pub choose_map_background_800_customize_battle: Option<SkirmishShellChromeEntry>,
    /// Generic common-shell small background, decoded through SHELL.PAL.
    pub generic_background_640_mnscrns_shell: Option<SkirmishShellChromeEntry>,
    /// Generic common-shell large background, decoded through SHELL.PAL.
    pub generic_background_large_mnscrnl_shell: Option<SkirmishShellChromeEntry>,
    pub lower_side_640_lwscrns: Option<SkirmishShellChromeEntry>,
    pub lower_side_large_lwscrnl: Option<SkirmishShellChromeEntry>,
    pub validation_modal_background_pudlgbgn: Option<SkirmishShellChromeEntry>,
    pub button_up_left_30: Option<SkirmishShellChromeEntry>,
    pub button_up_mid_30: Option<SkirmishShellChromeEntry>,
    pub button_up_right_30: Option<SkirmishShellChromeEntry>,
    pub button_down_left_30: Option<SkirmishShellChromeEntry>,
    pub button_down_mid_30: Option<SkirmishShellChromeEntry>,
    pub button_down_right_30: Option<SkirmishShellChromeEntry>,
    pub modal_button_mnbttn_frame0: Option<SkirmishShellChromeEntry>,
    pub modal_button_mnbttn_frame1: Option<SkirmishShellChromeEntry>,
    pub modal_button_mnbttn_frame2: Option<SkirmishShellChromeEntry>,
    pub checkbox_unchecked_cue_i: Option<SkirmishShellChromeEntry>,
    pub checkbox_checked_cce_i: Option<SkirmishShellChromeEntry>,
    pub trackbar_thumb_trakgrip: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_left_trofl: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_mid_trofm: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_right_trofr: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_released: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_pressed: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_gray_released: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_gray_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_up_released: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_up_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_down_released: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_down_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_top: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_mid: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_bottom: Option<SkirmishShellChromeEntry>,
    /// Both adjacent border-2 primitive frames drawn around a `128x21`
    /// owner-draw trackbar, including the two-pixel outside expansion.
    pub trackbar_rail: Option<SkirmishShellChromeEntry>,
    /// Active B8 numeric263x21 rail, original6B6300 retains the50px plaque.
    pub trackbar_numeric_263: Option<SkirmishShellChromeEntry>,
    /// RMG105 Players numeric225x21 rail.
    pub trackbar_numeric_225: Option<SkirmishShellChromeEntry>,
    /// Launcher D5 plain 180x21 trackbar (4AC disables the value plaque).
    pub trackbar_plain_180: Option<SkirmishShellChromeEntry>,
    /// Active BBB plain 192x21 rail, 4E1FE0 disables its plaque.
    pub trackbar_plain_192: Option<SkirmishShellChromeEntry>,
    /// Keyboard A3 category face, original resource138 DLU.
    pub combo_face_207: Option<SkirmishShellChromeEntry>,
    pub combo_face_180: Option<SkirmishShellChromeEntry>,
    pub combo_face_150: Option<SkirmishShellChromeEntry>,
    pub combo_face_117: Option<SkirmishShellChromeEntry>,
    pub combo_face_44: Option<SkirmishShellChromeEntry>,
    pub combo_face_38: Option<SkirmishShellChromeEntry>,
    pub white_pixel: Option<SkirmishShellChromeEntry>,
    pub start_marker: Option<SkirmishShellChromeEntry>,
    pub assigned_player_marker_mmpb: Option<SkirmishShellChromeEntry>,
    pub flags: Vec<(String, SkirmishShellChromeEntry)>,
}

/// Default-able subset of the chrome atlas carrying ONLY owner-draw control glyph
/// entries — NO GPU `texture` (that live field is why the full atlas can't derive
/// Default). The Slice 4 paint seam (`paint_control`) takes this so it resolves
/// chrome inside the emitter yet stays unit-testable via
/// `ControlChrome { trackbar_rail: Some(e), ..Default::default() }`. Grows one
/// control family per sub-step (4B trackbar; 4C combo; 4D scrollbar).
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlChrome {
    pub checkbox_unchecked_cue_i: Option<SkirmishShellChromeEntry>,
    pub checkbox_checked_cce_i: Option<SkirmishShellChromeEntry>,
    pub trackbar_rail: Option<SkirmishShellChromeEntry>,
    /// Active B8 numeric263x21 rail, original6B6300 retains the50px plaque.
    pub trackbar_numeric_263: Option<SkirmishShellChromeEntry>,
    /// RMG105 Players numeric225x21 rail.
    pub trackbar_numeric_225: Option<SkirmishShellChromeEntry>,
    pub trackbar_plain_180: Option<SkirmishShellChromeEntry>,
    /// Active BBB plain 192x21 rail, 4E1FE0 disables its plaque.
    pub trackbar_plain_192: Option<SkirmishShellChromeEntry>,
    /// Keyboard A3 category face, original resource138 DLU.
    pub combo_face_207: Option<SkirmishShellChromeEntry>,
    pub combo_face_180: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_left_trofl: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_mid_trofm: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_right_trofr: Option<SkirmishShellChromeEntry>,
    pub trackbar_thumb_trakgrip: Option<SkirmishShellChromeEntry>,
    pub white_pixel: Option<SkirmishShellChromeEntry>,
    pub combo_face_150: Option<SkirmishShellChromeEntry>,
    pub combo_face_117: Option<SkirmishShellChromeEntry>,
    pub combo_face_44: Option<SkirmishShellChromeEntry>,
    pub combo_face_38: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_released: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_pressed: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_gray_released: Option<SkirmishShellChromeEntry>,
    pub combo_arrow_down_gray_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_up_released: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_up_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_down_released: Option<SkirmishShellChromeEntry>,
    pub scrollbar_arrow_down_pressed: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_top: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_mid: Option<SkirmishShellChromeEntry>,
    pub scrollbar_thumb_bottom: Option<SkirmishShellChromeEntry>,
}

impl SkirmishShellChromeAtlas {
    /// Snapshot the control glyph entries into a Default-able, texture-free subset
    /// the paint seam can take (and tests can build by hand). Entries are `Copy`.
    pub fn control_chrome(&self) -> ControlChrome {
        ControlChrome {
            checkbox_unchecked_cue_i: self.checkbox_unchecked_cue_i,
            checkbox_checked_cce_i: self.checkbox_checked_cce_i,
            trackbar_rail: self.trackbar_rail,
            trackbar_numeric_263: self.trackbar_numeric_263,
            trackbar_numeric_225: self.trackbar_numeric_225,
            trackbar_plain_180: self.trackbar_plain_180,
            trackbar_plain_192: self.trackbar_plain_192,
            combo_face_207: self.combo_face_207,
            combo_face_180: self.combo_face_180,
            trackbar_plaque_left_trofl: self.trackbar_plaque_left_trofl,
            trackbar_plaque_mid_trofm: self.trackbar_plaque_mid_trofm,
            trackbar_plaque_right_trofr: self.trackbar_plaque_right_trofr,
            trackbar_thumb_trakgrip: self.trackbar_thumb_trakgrip,
            white_pixel: self.white_pixel,
            combo_face_150: self.combo_face_150,
            combo_face_117: self.combo_face_117,
            combo_face_44: self.combo_face_44,
            combo_face_38: self.combo_face_38,
            combo_arrow_down_released: self.combo_arrow_down_released,
            combo_arrow_down_pressed: self.combo_arrow_down_pressed,
            combo_arrow_down_gray_released: self.combo_arrow_down_gray_released,
            combo_arrow_down_gray_pressed: self.combo_arrow_down_gray_pressed,
            scrollbar_arrow_up_released: self.scrollbar_arrow_up_released,
            scrollbar_arrow_up_pressed: self.scrollbar_arrow_up_pressed,
            scrollbar_arrow_down_released: self.scrollbar_arrow_down_released,
            scrollbar_arrow_down_pressed: self.scrollbar_arrow_down_pressed,
            scrollbar_thumb_top: self.scrollbar_thumb_top,
            scrollbar_thumb_mid: self.scrollbar_thumb_mid,
            scrollbar_thumb_bottom: self.scrollbar_thumb_bottom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
enum ShellAssetRole {
    VerifiedParentBackground,
    VerifiedChooseMapBackground,
    VerifiedOfflineStartMarker,
    AssignedPlayerMarker,
    RightPanelChrome,
    VerifiedModalDialogBackground,
    VerifiedOwnerDrawButton,
    VerifiedFlag,
    ResearchCandidate,
    Other,
}

struct RenderedShellEntry {
    label: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

pub fn build_skirmish_shell_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
) -> Option<SkirmishShellChromeAtlas> {
    let shell_palette = load_shell_palette(assets)?;
    let shell2_palette = load_named_palette(assets, "SHELL2.PAL")?;
    let sdbtnanm_palette = load_named_palette(assets, "SDBTNANM.PAL");
    let main_button_palette = load_named_palette(assets, "MAINBTTN.PAL");
    let neutral_dialog_palette = load_named_palette(assets, "DIALOGN.PAL");
    let parent_background_palette = load_parent_background_palette(assets);
    let choose_map_background_palette = load_choose_map_background_palette(assets);

    let mut rendered = Vec::new();
    // Original SDWRNANM header supplies 91 frames; keep the runtime sequence
    // bounded by the available retail frames, without stretching the canvas.
    for frame in 0..91 {
        let Some(entry) = render_shp_entry_labeled(
            assets,
            "SDWRNANM.SHP",
            &format!("sdwrnanm.shp#{frame}"),
            &shell2_palette,
            frame,
        ) else {
            break;
        };
        rendered.push(entry);
    }
    rendered.push(mandatory_shp(
        assets,
        "SDTP.SHP",
        &shell_palette,
        0,
        "SHELL.PAL",
    )?);
    rendered.push(render_shp_entry_labeled(
        assets,
        "SDTP.SHP",
        "sdtp.shp#1",
        &shell_palette,
        1,
    )?);
    rendered.push(mandatory_shp(
        assets,
        "SDBTNBKGD.SHP",
        &shell2_palette,
        0,
        "SHELL2.PAL",
    )?);
    rendered.push(mandatory_shp(
        assets,
        "SDBTM.SHP",
        &shell_palette,
        0,
        "SHELL.PAL",
    )?);
    if let Some(sdbtnanm_palette) = sdbtnanm_palette.as_ref() {
        // Bake the full frame range the slide-in wave references (Group A uses 1/5..=10,
        // Group B uses 0/11..=16) plus the existing idle frames 2/3/4 and overlay 10.
        for frame in 0usize..=16 {
            match render_shp_entry_labeled(
                assets,
                "SDBTNANM.SHP",
                &format!("sdbtnanm.shp#{frame}"),
                sdbtnanm_palette,
                frame,
            ) {
                Some(entry) => rendered.push(entry),
                None => log::warn!(
                    "Missing optional Skirmish shell asset SDBTNANM.SHP frame {frame}; not substituting frame 0"
                ),
            }
        }
    }
    if sdbtnanm_palette.is_none() {
        log::warn!("Missing optional Skirmish shell palette SDBTNANM.PAL");
    }
    if let Some(main_button_palette) = main_button_palette.as_ref() {
        for frame in [0usize, 1, 2] {
            match render_shp_entry_labeled(
                assets,
                "MNBTTN.SHP",
                &format!("mnbttn.shp#{frame}"),
                main_button_palette,
                frame,
            ) {
                Some(entry) => rendered.push(entry),
                None => log::warn!(
                    "Missing verified modal button asset MNBTTN.SHP frame {frame}; validation modal will fall back to generic button art"
                ),
            }
        }
    } else {
        log::warn!(
            "Skipping verified modal button art MNBTTN.SHP because MAINBTTN.PAL is missing or invalid"
        );
    }
    if let Some(neutral_dialog_palette) = neutral_dialog_palette.as_ref() {
        push_optional(
            &mut rendered,
            render_shp_entry_labeled(
                assets,
                "PUDLGBGN.SHP",
                "pudlgbgn.shp#0",
                neutral_dialog_palette,
                0,
            ),
            "PUDLGBGN.SHP",
        );
    } else {
        log::warn!(
            "Skipping native validation modal background PUDLGBGN.SHP because DIALOGN.PAL is missing or invalid"
        );
    }
    rendered.push(mandatory_shp(
        assets,
        "SDMPBTN.SHP",
        &shell_palette,
        0,
        "SHELL.PAL",
    )?);
    // The slide engine's map-button and warning-display frames.
    for frame in 1..=6 {
        rendered.extend(render_shp_entry_labeled(
            assets,
            "SDMPBTN.SHP",
            &format!("sdmpbtn.shp#{frame}"),
            &shell_palette,
            frame,
        ));
    }
    for frame in 0..=5 {
        rendered.extend(render_shp_entry_labeled(
            assets,
            "SDWRNTMP.SHP",
            &format!("sdwrntmp.shp#{frame}"),
            &shell_palette,
            frame,
        ));
    }
    rendered.push(mandatory_shp(
        assets,
        "LWSCRNS.SHP",
        &shell_palette,
        0,
        "SHELL.PAL",
    )?);
    rendered.push(mandatory_shp(
        assets,
        "LWSCRNL.SHP",
        &shell_palette,
        0,
        "SHELL.PAL",
    )?);

    for (name, label) in [
        ("MNSCRNS.SHP", "mnscrns.shp#shell"),
        ("MNSCRNL.SHP", "mnscrnl.shp#shell"),
    ] {
        push_optional(
            &mut rendered,
            render_shp_entry_labeled(assets, name, label, &shell_palette, 0),
            name,
        );
    }

    if let Some(parent_background_palette) = parent_background_palette.as_ref() {
        for name in ["MNSCRNS.SHP", "MnScrnLCoopGameSetup.shp"] {
            push_optional(
                &mut rendered,
                render_shp_entry(assets, name, parent_background_palette, 0),
                name,
            );
        }
    } else {
        log::warn!(
            "Skipping verified Skirmish parent backgrounds because MnScrnLCoopGameSetup.PAL is missing or invalid"
        );
    }
    if let Some(choose_map_background_palette) = choose_map_background_palette.as_ref() {
        push_optional(
            &mut rendered,
            render_shp_entry(
                assets,
                "MnScrnLCustomizeBattle.shp",
                choose_map_background_palette,
                0,
            ),
            "MnScrnLCustomizeBattle.shp",
        );
    } else {
        log::warn!(
            "Skipping verified Choose Map modal background because MnScrnLCustomizeBattle.PAL is missing or invalid"
        );
    }

    for name in ["STARTBUT.SHP", "mmpb.shp"] {
        push_optional(
            &mut rendered,
            render_shp_entry(assets, name, &shell_palette, 0),
            name,
        );
    }

    for name in [
        "bue_li30.pcx",
        "bue_mi30.pcx",
        "bue_ri30.pcx",
        "bde_li30.pcx",
        "bde_mi30.pcx",
        "bde_ri30.pcx",
        "cue_i.pcx",
        "cce_i.pcx",
        "trakgrip.pcx",
        "trofl.pcx",
        "trofm.pcx",
        "trofr.pcx",
        "dnarrowr.pcx",
        "dnarrowp.pcx",
        "gdnarrowr.pcx",
        "gdnarrowp.pcx",
        "uparrowr.pcx",
        "uparrowp.pcx",
        "sbgript.pcx",
        "sbgripm.pcx",
        "sbgripb.pcx",
    ] {
        push_optional(&mut rendered, render_pcx_entry(assets, name), name);
    }

    rendered.push(render_trackbar_frame_entry("skirmish_trackbar_rail"));
    rendered.push(render_trackbar_frame_geometry(
        "rmg_trackbar_numeric_225",
        225,
        21,
        50,
    ));
    rendered.push(render_trackbar_frame_geometry(
        "sound_trackbar_numeric_263",
        263,
        21,
        50,
    ));
    rendered.push(render_trackbar_frame_geometry(
        "launcher_trackbar_plain_180",
        180,
        21,
        0,
    ));
    rendered.push(render_trackbar_frame_geometry(
        "in_game_trackbar_plain_192",
        192,
        21,
        0,
    ));
    for (label, width) in [
        ("skirmish_combo_face_150", 150),
        ("launcher_combo_face_180", 180),
        ("keyboard_combo_face_207", 207),
        ("skirmish_combo_face_117", 117),
        ("skirmish_combo_face_44", 44),
        ("skirmish_combo_face_38", 38),
    ] {
        rendered.push(render_primitive_bevel_entry(
            label,
            width,
            24,
            [2, 2, width as i32 - 4, 20],
            2,
        ));
    }
    rendered.push(render_solid_entry(
        "skirmish_white_pixel",
        1,
        1,
        [255, 255, 255, 255],
    ));

    for name in SKIRMISH_FLAG_PCX_NAMES {
        push_optional(&mut rendered, render_flag_pcx_entry(assets, name), name);
    }

    let (texture, packed) = pack_entries(gpu, batch, &rendered)?;
    let by_label: HashMap<String, SkirmishShellChromeEntry> = rendered
        .iter()
        .map(|entry| entry.label.clone())
        .zip(packed)
        .collect();
    let flags = SKIRMISH_FLAG_PCX_NAMES
        .into_iter()
        .filter_map(|name| {
            by_label
                .get(name)
                .copied()
                .map(|entry| (name.to_string(), entry))
        })
        .collect();

    Some(SkirmishShellChromeAtlas {
        texture,
        launcher_warning_frames: (0..91)
            .map_while(|frame| by_label.get(&format!("sdwrnanm.shp#{frame}")).copied())
            .collect(),
        right_panel_top_sdtp: by_label.get("sdtp.shp").copied(),
        right_panel_top_highlight_sdtp_frame1: by_label.get("sdtp.shp#1").copied(),
        right_panel_tile_sdbtnbkgd: by_label.get("sdbtnbkgd.shp").copied(),
        right_panel_overlay_sdbtnanm_frame10: by_label.get("sdbtnanm.shp#10").copied(),
        right_panel_button_sdbtnanm_frame2: by_label.get("sdbtnanm.shp#2").copied(),
        right_panel_button_sdbtnanm_frame3: by_label.get("sdbtnanm.shp#3").copied(),
        right_panel_button_sdbtnanm_frame4: by_label.get("sdbtnanm.shp#4").copied(),
        right_panel_button_sdbtnanm_frames: std::array::from_fn(|frame| {
            by_label.get(&format!("sdbtnanm.shp#{frame}")).copied()
        }),
        right_panel_bottom_sdbtm: by_label.get("sdbtm.shp").copied(),
        sd_map_button: by_label.get("sdmpbtn.shp").copied(),
        sd_map_button_frames: std::array::from_fn(|frame| match frame {
            0 => by_label.get("sdmpbtn.shp").copied(),
            frame => by_label.get(&format!("sdmpbtn.shp#{frame}")).copied(),
        }),
        sd_warning_frames: std::array::from_fn(|frame| {
            by_label.get(&format!("sdwrntmp.shp#{frame}")).copied()
        }),
        background_640_mnscrns: by_label.get("mnscrns.shp").copied(),
        background_800_coop_game_setup: by_label.get("mnscrnlcoopgamesetup.shp").copied(),
        choose_map_background_800_customize_battle: by_label
            .get("mnscrnlcustomizebattle.shp")
            .copied(),
        generic_background_640_mnscrns_shell: by_label.get("mnscrns.shp#shell").copied(),
        generic_background_large_mnscrnl_shell: by_label.get("mnscrnl.shp#shell").copied(),
        lower_side_640_lwscrns: by_label.get("lwscrns.shp").copied(),
        lower_side_large_lwscrnl: by_label.get("lwscrnl.shp").copied(),
        validation_modal_background_pudlgbgn: by_label.get("pudlgbgn.shp#0").copied(),
        button_up_left_30: by_label.get("bue_li30.pcx").copied(),
        button_up_mid_30: by_label.get("bue_mi30.pcx").copied(),
        button_up_right_30: by_label.get("bue_ri30.pcx").copied(),
        button_down_left_30: by_label.get("bde_li30.pcx").copied(),
        button_down_mid_30: by_label.get("bde_mi30.pcx").copied(),
        button_down_right_30: by_label.get("bde_ri30.pcx").copied(),
        modal_button_mnbttn_frame0: by_label.get("mnbttn.shp#0").copied(),
        modal_button_mnbttn_frame1: by_label.get("mnbttn.shp#1").copied(),
        modal_button_mnbttn_frame2: by_label.get("mnbttn.shp#2").copied(),
        checkbox_unchecked_cue_i: by_label.get("cue_i.pcx").copied(),
        checkbox_checked_cce_i: by_label.get("cce_i.pcx").copied(),
        trackbar_thumb_trakgrip: by_label.get("trakgrip.pcx").copied(),
        trackbar_plaque_left_trofl: by_label.get("trofl.pcx").copied(),
        trackbar_plaque_mid_trofm: by_label.get("trofm.pcx").copied(),
        trackbar_plaque_right_trofr: by_label.get("trofr.pcx").copied(),
        combo_arrow_down_released: by_label.get("dnarrowr.pcx").copied(),
        combo_arrow_down_pressed: by_label.get("dnarrowp.pcx").copied(),
        combo_arrow_down_gray_released: by_label.get("gdnarrowr.pcx").copied(),
        combo_arrow_down_gray_pressed: by_label.get("gdnarrowp.pcx").copied(),
        scrollbar_arrow_up_released: by_label.get("uparrowr.pcx").copied(),
        scrollbar_arrow_up_pressed: by_label.get("uparrowp.pcx").copied(),
        scrollbar_arrow_down_released: by_label.get("dnarrowr.pcx").copied(),
        scrollbar_arrow_down_pressed: by_label.get("dnarrowp.pcx").copied(),
        scrollbar_thumb_top: by_label.get("sbgript.pcx").copied(),
        scrollbar_thumb_mid: by_label.get("sbgripm.pcx").copied(),
        scrollbar_thumb_bottom: by_label.get("sbgripb.pcx").copied(),
        trackbar_rail: by_label.get("skirmish_trackbar_rail").copied(),
        trackbar_numeric_263: by_label.get("sound_trackbar_numeric_263").copied(),
        trackbar_numeric_225: by_label.get("rmg_trackbar_numeric_225").copied(),
        trackbar_plain_180: by_label.get("launcher_trackbar_plain_180").copied(),
        trackbar_plain_192: by_label.get("in_game_trackbar_plain_192").copied(),
        combo_face_207: by_label.get("keyboard_combo_face_207").copied(),
        combo_face_180: by_label.get("launcher_combo_face_180").copied(),
        combo_face_150: by_label.get("skirmish_combo_face_150").copied(),
        combo_face_117: by_label.get("skirmish_combo_face_117").copied(),
        combo_face_44: by_label.get("skirmish_combo_face_44").copied(),
        combo_face_38: by_label.get("skirmish_combo_face_38").copied(),
        white_pixel: by_label.get("skirmish_white_pixel").copied(),
        start_marker: by_label.get("startbut.shp").copied(),
        assigned_player_marker_mmpb: by_label.get("mmpb.shp").copied(),
        flags,
    })
}

fn load_parent_background_palette(assets: &AssetManager) -> Option<Palette> {
    let Some(palette_bytes) = assets.get_ref("MnScrnLCoopGameSetup.PAL") else {
        log::warn!("Missing verified Skirmish parent-background palette MnScrnLCoopGameSetup.PAL");
        return None;
    };
    Palette::from_bytes(palette_bytes)
        .map_err(|err| {
            log::warn!(
                "Could not parse verified Skirmish parent-background palette MnScrnLCoopGameSetup.PAL: {err:#}"
            );
            err
        })
        .ok()
}

fn load_choose_map_background_palette(assets: &AssetManager) -> Option<Palette> {
    let Some(palette_bytes) = assets.get_ref("MnScrnLCustomizeBattle.PAL") else {
        log::warn!("Missing verified Choose Map modal palette MnScrnLCustomizeBattle.PAL");
        return None;
    };
    Palette::from_bytes(palette_bytes)
        .map_err(|err| {
            log::warn!(
                "Could not parse verified Choose Map modal palette MnScrnLCustomizeBattle.PAL: {err:#}"
            );
            err
        })
        .ok()
}

fn load_shell_palette(assets: &AssetManager) -> Option<Palette> {
    load_named_palette(assets, "SHELL.PAL")
}

fn load_named_palette(assets: &AssetManager, name: &str) -> Option<Palette> {
    let Some(palette_bytes) = assets.get_ref(name) else {
        log::warn!("Missing verified Skirmish shell palette {name}");
        return None;
    };
    Palette::from_bytes(palette_bytes)
        .map_err(|err| {
            log::warn!("Could not parse verified Skirmish shell palette {name}: {err:#}");
            err
        })
        .ok()
}

#[cfg(test)]
fn classify_shell_asset(name: &str) -> ShellAssetRole {
    match name.to_ascii_lowercase().as_str() {
        "mnscrns.shp" | "mnscrnl.shp" | "mnscrnlcoopgamesetup.shp" => {
            ShellAssetRole::VerifiedParentBackground
        }
        "mnscrnlcustomizebattle.shp" => ShellAssetRole::VerifiedChooseMapBackground,
        "startbut.shp" => ShellAssetRole::VerifiedOfflineStartMarker,
        "mmpb.shp" => ShellAssetRole::AssignedPlayerMarker,
        "sdtp.shp" | "sdbtnbkgd.shp" | "sdbtm.shp" | "sdbtnanm.shp" | "sdmpbtn.shp"
        | "lwscrns.shp" | "lwscrnl.shp" => ShellAssetRole::RightPanelChrome,
        "bue_li30.pcx" | "bue_mi30.pcx" | "bue_ri30.pcx" | "bde_li30.pcx" | "bde_mi30.pcx"
        | "bde_ri30.pcx" | "cue_i.pcx" | "cce_i.pcx" | "trakgrip.pcx" | "trofl.pcx"
        | "trofm.pcx" | "trofr.pcx" | "dnarrowr.pcx" | "dnarrowp.pcx" | "gdnarrowr.pcx"
        | "gdnarrowp.pcx" | "mnbttn.shp" | "sidebttn.shp" => {
            ShellAssetRole::VerifiedOwnerDrawButton
        }
        "pudlgbgn.shp" => ShellAssetRole::VerifiedModalDialogBackground,
        "usai.pcx" | "japi.pcx" | "frai.pcx" | "geri.pcx" | "gbri.pcx" | "djbi.pcx"
        | "arbi.pcx" | "lati.pcx" | "rusi.pcx" | "yrii.pcx" | "obsi.pcx" | "rani.pcx" => {
            ShellAssetRole::VerifiedFlag
        }
        "dbak6440.pcx" | "dlgsysa.pcx" | "dlgsysi.pcx" => ShellAssetRole::ResearchCandidate,
        _ => ShellAssetRole::Other,
    }
}

fn push_optional(
    entries: &mut Vec<RenderedShellEntry>,
    entry: Option<RenderedShellEntry>,
    name: &str,
) {
    if let Some(entry) = entry {
        entries.push(entry);
    } else {
        log::warn!("Missing optional Skirmish shell asset {name}");
    }
}

fn mandatory_shp(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
    palette_name: &str,
) -> Option<RenderedShellEntry> {
    render_shp_entry(assets, file_name, palette, frame).or_else(|| {
        log::warn!(
            "Missing mandatory Skirmish shell asset {file_name} frame {frame} decoded with {palette_name}"
        );
        None
    })
}

fn render_shp_entry(
    assets: &AssetManager,
    file_name: &str,
    palette: &Palette,
    frame: usize,
) -> Option<RenderedShellEntry> {
    render_shp_entry_labeled(
        assets,
        file_name,
        &file_name.to_ascii_lowercase(),
        palette,
        frame,
    )
}

fn render_shp_entry_labeled(
    assets: &AssetManager,
    file_name: &str,
    label: &str,
    palette: &Palette,
    frame: usize,
) -> Option<RenderedShellEntry> {
    let bytes = assets.get_ref(file_name)?;
    let shp = ShpFile::from_bytes(bytes).ok()?;
    if frame >= shp.frames.len() {
        return None;
    }
    let frame_rgba = shp.frame_to_rgba(frame, palette).ok()?;
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
    Some(RenderedShellEntry {
        label: label.to_ascii_lowercase(),
        width: canvas_w,
        height: canvas_h,
        rgba,
    })
}

fn render_pcx_entry(assets: &AssetManager, file_name: &str) -> Option<RenderedShellEntry> {
    let bytes = assets.get_ref(file_name)?;
    let pcx = PcxFile::from_bytes(bytes).ok()?;
    Some(RenderedShellEntry {
        label: file_name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba: pcx.to_rgba(None),
    })
}

fn render_flag_pcx_entry(assets: &AssetManager, file_name: &str) -> Option<RenderedShellEntry> {
    let bytes = assets.get_ref(file_name)?;
    let pcx = PcxFile::from_bytes(bytes).ok()?;
    let mut rgba = pcx.to_rgba(None);
    ACTIVE_RETAIL_RGB565_PRESENTATION
        .apply_packed_color_key_rgba8(&mut rgba, OWNER_DRAW_FLAG_TRANSPARENT_RGB);
    Some(RenderedShellEntry {
        label: file_name.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
}

fn render_solid_entry(label: &str, width: u32, height: u32, color: [u8; 4]) -> RenderedShellEntry {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&color);
    }
    RenderedShellEntry {
        label: label.to_ascii_lowercase(),
        width,
        height,
        rgba,
    }
}

fn render_primitive_bevel_entry(
    label: &str,
    width: u32,
    height: u32,
    box_xywh: [i32; 4],
    border: i32,
) -> RenderedShellEntry {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    draw_primitive_bevel(&mut rgba, width, height, box_xywh, border);
    RenderedShellEntry {
        label: label.to_ascii_lowercase(),
        width,
        height,
        rgba,
    }
}

fn render_trackbar_frame_entry(label: &str) -> RenderedShellEntry {
    render_trackbar_frame_geometry(
        label,
        TRACKBAR_CONTROL_W,
        TRACKBAR_CONTROL_H,
        TRACKBAR_VALUE_PLAQUE_W,
    )
}

/// Original 61E1B9..61E269: draw the rail frame at the current control size;
/// only a plaque-enabled control draws the adjacent value frame. Both use
/// 6208F0's two-pixel outside expansion. Never stretch a split-frame bitmap.
fn render_trackbar_frame_geometry(
    label: &str,
    control_width: u32,
    control_height: u32,
    reserve: i32,
) -> RenderedShellEntry {
    let border = TRACKBAR_FRAME_BORDER;
    let width = control_width + (border as u32) * 2;
    let height = control_height + (border as u32) * 2;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let control_w = control_width as i32;
    let control_h = control_height as i32;
    let left_frame_w = control_w - reserve;
    let value_frame_x = left_frame_w + TRACKBAR_VALUE_FRAME_INSET;
    let value_frame_w = reserve - TRACKBAR_VALUE_FRAME_INSET;

    // `OwnerDraw_Trackbar_0061D950` supplies control-relative boxes and
    // FUN_006208F0 expands each by two pixels. Shift both inputs by the canvas
    // border so the complete outside rings fit in this transparent entry.
    draw_primitive_bevel(
        &mut rgba,
        width,
        height,
        [border, border, left_frame_w, control_h],
        border,
    );
    if reserve > 0 {
        draw_primitive_bevel(
            &mut rgba,
            width,
            height,
            [border + value_frame_x, border, value_frame_w, control_h],
            border,
        );
    }

    RenderedShellEntry {
        label: label.to_ascii_lowercase(),
        width,
        height,
        rgba,
    }
}

fn draw_primitive_bevel(rgba: &mut [u8], width: u32, height: u32, box_xywh: [i32; 4], border: i32) {
    if border <= 0 || box_xywh[2] <= 0 || box_xywh[3] <= 0 {
        return;
    }

    let left0 = box_xywh[0] - border;
    let top0 = box_xywh[1] - border;
    let right0 = box_xywh[0] + box_xywh[2] + border - 1;
    let bottom0 = box_xywh[1] + box_xywh[3] + border - 1;
    let color_a = rgba_color(PRIMITIVE_BEVEL_COLOR_A_RGB);
    let color_b = rgba_color(PRIMITIVE_BEVEL_COLOR_B_RGB);
    let mixed = rgba_color(average_rgb(
        PRIMITIVE_BEVEL_COLOR_A_RGB,
        PRIMITIVE_BEVEL_COLOR_B_RGB,
    ));

    for ring in 0..border {
        let left = left0 + ring;
        let top = top0 + ring;
        let right = right0 - ring;
        let bottom = bottom0 - ring;
        let (top_left_color, bottom_right_color) = if border == 2 && ring == 1 {
            (color_b, color_a)
        } else {
            (color_a, color_b)
        };

        draw_axis_line_inclusive_clipped(
            rgba,
            width,
            height,
            (left, top),
            (right - 1, top),
            top_left_color,
        );
        draw_axis_line_inclusive_clipped(
            rgba,
            width,
            height,
            (left, top + 1),
            (left, bottom),
            top_left_color,
        );
        draw_axis_line_inclusive_clipped(
            rgba,
            width,
            height,
            (right, bottom),
            (left, bottom),
            bottom_right_color,
        );
        draw_axis_line_inclusive_clipped(
            rgba,
            width,
            height,
            (right, bottom - 1),
            (right, top + 1),
            bottom_right_color,
        );

        if border == 2 {
            put_pixel_clipped(rgba, width, height, right, top, mixed);
            put_pixel_clipped(rgba, width, height, left, bottom, mixed);
        }
    }
}

fn draw_axis_line_inclusive_clipped(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
) {
    if width == 0 || height == 0 {
        return;
    }

    let max_x = width.saturating_sub(1) as i32;
    let max_y = height.saturating_sub(1) as i32;
    if start.1 == end.1 {
        let y = start.1;
        if y < 0 || y > max_y {
            return;
        }
        let from = start.0.min(end.0).max(0);
        let to = start.0.max(end.0).min(max_x);
        if from > to {
            return;
        }
        for x in from..=to {
            put_pixel_clipped(rgba, width, height, x, y, color);
        }
    } else if start.0 == end.0 {
        let x = start.0;
        if x < 0 || x > max_x {
            return;
        }
        let from = start.1.min(end.1).max(0);
        let to = start.1.max(end.1).min(max_y);
        if from > to {
            return;
        }
        for y in from..=to {
            put_pixel_clipped(rgba, width, height, x, y, color);
        }
    }
}

fn put_pixel_clipped(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let offset = ((y as u32 * width + x as u32) * 4) as usize;
    if offset + 4 <= rgba.len() {
        rgba[offset..offset + 4].copy_from_slice(&color);
    }
}

fn rgba_color(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255]
}

fn average_rgb(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    [
        ((u16::from(a[0]) + u16::from(b[0])) / 2) as u8,
        ((u16::from(a[1]) + u16::from(b[1])) / 2) as u8,
        ((u16::from(a[2]) + u16::from(b[2])) / 2) as u8,
    ]
}

fn pack_entries(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    entries: &[RenderedShellEntry],
) -> Option<(BatchTexture, Vec<SkirmishShellChromeEntry>)> {
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
        "Skirmish shell chrome atlas: {}x{} px, {} pieces",
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
        .map(|(entry, (px, py))| SkirmishShellChromeEntry {
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
        AssetManager, OWNER_DRAW_FLAG_TRANSPARENT_RGB, PRIMITIVE_BEVEL_COLOR_A_RGB,
        PRIMITIVE_BEVEL_COLOR_B_RGB, RenderedShellEntry, ShellAssetRole, average_rgb,
        classify_shell_asset, draw_axis_line_inclusive_clipped, load_named_palette,
        load_parent_background_palette, render_primitive_bevel_entry, render_shp_entry,
        render_trackbar_frame_entry, rgba_color,
    };

    fn pixel(entry: &RenderedShellEntry, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * entry.width + x) * 4) as usize;
        [
            entry.rgba[offset],
            entry.rgba[offset + 1],
            entry.rgba[offset + 2],
            entry.rgba[offset + 3],
        ]
    }

    fn retail_assets() -> AssetManager {
        let config = crate::util::config::GameConfig::load().expect("game config");
        let mut assets = AssetManager::new(&config.paths.ra2_dir).expect("asset manager");
        assert!(
            assets
                .register_neutral_archives()
                .expect("neutral archive registration"),
            "retail neutral archive pair"
        );
        assets
    }

    #[test]
    fn skirmish_shell_asset_classification_matches_live_render_path() {
        assert_eq!(
            classify_shell_asset("MNSCRNS.SHP"),
            ShellAssetRole::VerifiedParentBackground
        );
        assert_eq!(
            classify_shell_asset("MnScrnLCoopGameSetup.shp"),
            ShellAssetRole::VerifiedParentBackground
        );
        assert_eq!(
            classify_shell_asset("STARTBUT.SHP"),
            ShellAssetRole::VerifiedOfflineStartMarker
        );
        assert_eq!(
            classify_shell_asset("bue_li30.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("cue_i.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("cce_i.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("trakgrip.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("trofm.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("dnarrowr.pcx"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("MNBTTN.SHP"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("PUDLGBGN.SHP"),
            ShellAssetRole::VerifiedModalDialogBackground
        );
        assert_eq!(
            classify_shell_asset("mmpb.shp"),
            ShellAssetRole::AssignedPlayerMarker
        );
        assert_ne!(
            classify_shell_asset("mmpb.shp"),
            ShellAssetRole::VerifiedOfflineStartMarker
        );
        assert_eq!(
            classify_shell_asset("SDMPBTN.SHP"),
            ShellAssetRole::RightPanelChrome
        );
        assert_eq!(
            classify_shell_asset("LWSCRNS.SHP"),
            ShellAssetRole::RightPanelChrome
        );
        assert_eq!(
            classify_shell_asset("LWSCRNL.SHP"),
            ShellAssetRole::RightPanelChrome
        );
        assert_ne!(
            classify_shell_asset("SDMPBTN.SHP"),
            ShellAssetRole::VerifiedOfflineStartMarker
        );
        assert_eq!(
            classify_shell_asset("MNSCRNL.SHP"),
            ShellAssetRole::VerifiedParentBackground
        );
        assert_eq!(
            classify_shell_asset("MnScrnLCustomizeBattle.shp"),
            ShellAssetRole::VerifiedChooseMapBackground
        );
        assert_ne!(
            classify_shell_asset("MnScrnLCustomizeBattle.shp"),
            ShellAssetRole::VerifiedParentBackground
        );
        assert_ne!(
            classify_shell_asset("sidebar.pal"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
        assert_eq!(
            classify_shell_asset("SIDEBTTN.SHP"),
            ShellAssetRole::VerifiedOwnerDrawButton
        );
    }

    #[test]
    fn verified_flag_pcxs_use_magenta_color_key() {
        assert_eq!(OWNER_DRAW_FLAG_TRANSPARENT_RGB, [255, 0, 255]);
    }

    #[test]
    fn primitive_bevel_line_spans_include_both_endpoints() {
        let color = [1, 2, 3, 255];
        let mut rgba = vec![0u8; 5 * 3 * 4];

        draw_axis_line_inclusive_clipped(&mut rgba, 5, 3, (1, 1), (3, 1), color);
        let entry = RenderedShellEntry {
            label: "line".to_string(),
            width: 5,
            height: 3,
            rgba,
        };

        assert_eq!(pixel(&entry, 0, 1), [0, 0, 0, 0]);
        assert_eq!(pixel(&entry, 1, 1), color);
        assert_eq!(pixel(&entry, 2, 1), color);
        assert_eq!(pixel(&entry, 3, 1), color);
        assert_eq!(pixel(&entry, 4, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn primitive_bevel_line_spans_clip_to_destination_extents() {
        let color = [4, 5, 6, 255];
        let mut rgba = vec![0u8; 3 * 3 * 4];

        draw_axis_line_inclusive_clipped(&mut rgba, 3, 3, (-2, 1), (5, 1), color);
        draw_axis_line_inclusive_clipped(&mut rgba, 3, 3, (2, -3), (2, 8), color);
        let entry = RenderedShellEntry {
            label: "clip".to_string(),
            width: 3,
            height: 3,
            rgba,
        };

        assert_eq!(pixel(&entry, 0, 1), color);
        assert_eq!(pixel(&entry, 1, 1), color);
        assert_eq!(pixel(&entry, 2, 1), color);
        assert_eq!(pixel(&entry, 2, 0), color);
        assert_eq!(pixel(&entry, 2, 2), color);
        assert_eq!(pixel(&entry, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn primitive_bevel_border_two_swaps_outer_and_inner_ring_colors() {
        let entry = render_primitive_bevel_entry("bevel", 8, 8, [2, 2, 2, 2], 2);
        let color_a = rgba_color(PRIMITIVE_BEVEL_COLOR_A_RGB);
        let color_b = rgba_color(PRIMITIVE_BEVEL_COLOR_B_RGB);

        assert_eq!(pixel(&entry, 0, 0), color_a);
        assert_eq!(pixel(&entry, 0, 1), color_a);
        assert_eq!(pixel(&entry, 5, 1), color_b);
        assert_eq!(pixel(&entry, 2, 5), color_b);

        assert_eq!(pixel(&entry, 1, 1), color_b);
        assert_eq!(pixel(&entry, 1, 2), color_b);
        assert_eq!(pixel(&entry, 4, 2), color_a);
        assert_eq!(pixel(&entry, 2, 4), color_a);
    }

    #[test]
    fn primitive_bevel_border_two_averages_mixed_corners() {
        let entry = render_primitive_bevel_entry("bevel", 8, 8, [2, 2, 2, 2], 2);
        let mixed = rgba_color(average_rgb(
            PRIMITIVE_BEVEL_COLOR_A_RGB,
            PRIMITIVE_BEVEL_COLOR_B_RGB,
        ));

        assert_eq!(mixed, [0xA2, 0x9C, 0x87, 255]);
        assert_eq!(pixel(&entry, 5, 0), mixed);
        assert_eq!(pixel(&entry, 0, 5), mixed);
        assert_eq!(pixel(&entry, 4, 1), mixed);
        assert_eq!(pixel(&entry, 1, 4), mixed);
    }

    #[test]
    fn trackbar_frame_entry_contains_both_native_primitive_boxes() {
        let entry = render_trackbar_frame_entry("trackbar");
        let color_a = rgba_color(PRIMITIVE_BEVEL_COLOR_A_RGB);
        let color_b = rgba_color(PRIMITIVE_BEVEL_COLOR_B_RGB);
        let mixed = rgba_color(average_rgb(
            PRIMITIVE_BEVEL_COLOR_A_RGB,
            PRIMITIVE_BEVEL_COLOR_B_RGB,
        ));

        assert_eq!((entry.width, entry.height), (132, 25));
        assert_eq!(pixel(&entry, 0, 0), color_a);
        assert_eq!(pixel(&entry, 131, 0), mixed);
        assert_eq!(pixel(&entry, 80, 10), color_a);
        assert_eq!(pixel(&entry, 81, 10), color_b);
        assert_eq!(pixel(&entry, 79, 10), [0, 0, 0, 0]);
        assert_eq!(pixel(&entry, 100, 10), [0, 0, 0, 0]);
        assert_eq!(pixel(&entry, 80, 24), mixed);
    }

    #[test]
    fn sound_trackbar_frames_keep_native_wide_rail_and_fixed_plaque() {
        // Original61E1B9..61E269 +6208F0: rail213, value x215/w48;
        // both border2 expanded. Reusing the128 bitmap misplaced this divider.
        let entry = super::render_trackbar_frame_geometry("sound", 263, 21, 50);
        assert_eq!((entry.width, entry.height), (267, 25));
        assert_eq!(
            pixel(&entry, 215, 10),
            rgba_color(PRIMITIVE_BEVEL_COLOR_A_RGB)
        );
        assert_eq!(
            pixel(&entry, 216, 10),
            rgba_color(PRIMITIVE_BEVEL_COLOR_B_RGB)
        );
        assert_eq!(pixel(&entry, 214, 10), [0, 0, 0, 0]);
        assert_eq!(pixel(&entry, 235, 10), [0, 0, 0, 0]);
    }

    #[test]
    #[ignore]
    fn retail_shell_shp_dimensions_match_research() {
        let assets = retail_assets();
        let shell_palette = load_named_palette(&assets, "SHELL.PAL").expect("SHELL.PAL");
        let anim_palette = load_named_palette(&assets, "SDBTNANM.PAL").expect("SDBTNANM.PAL");
        let main_button_palette =
            load_named_palette(&assets, "MAINBTTN.PAL").expect("MAINBTTN.PAL");
        let sdbtn = render_shp_entry(&assets, "SDBTNANM.SHP", &anim_palette, 10)
            .expect("SDBTNANM frame 10");
        let mnbttn0 = render_shp_entry(&assets, "MNBTTN.SHP", &main_button_palette, 0)
            .expect("MNBTTN frame 0");
        let mnbttn1 = render_shp_entry(&assets, "MNBTTN.SHP", &main_button_palette, 1)
            .expect("MNBTTN frame 1");
        let mnbttn2 = render_shp_entry(&assets, "MNBTTN.SHP", &main_button_palette, 2)
            .expect("MNBTTN frame 2");
        let lwscrns = render_shp_entry(&assets, "LWSCRNS.SHP", &shell_palette, 0).expect("LWSCRNS");
        let lwscrnl = render_shp_entry(&assets, "LWSCRNL.SHP", &shell_palette, 0).expect("LWSCRNL");
        assert_eq!((sdbtn.width, sdbtn.height), (156, 42));
        assert_eq!((mnbttn0.width, mnbttn0.height), (126, 25));
        assert_eq!((mnbttn1.width, mnbttn1.height), (126, 25));
        assert_eq!((mnbttn2.width, mnbttn2.height), (126, 25));
        assert_eq!((lwscrns.width, lwscrns.height), (472, 32));
        assert_eq!((lwscrnl.width, lwscrnl.height), (632, 32));
    }

    #[test]
    #[ignore]
    fn retail_parent_backgrounds_decode_with_verified_palette() {
        let assets = retail_assets();
        let palette = load_parent_background_palette(&assets).expect("parent palette");
        let mnscrns = render_shp_entry(&assets, "MNSCRNS.SHP", &palette, 0).expect("MNSCRNS");
        let coop = render_shp_entry(&assets, "MnScrnLCoopGameSetup.shp", &palette, 0)
            .expect("MnScrnLCoopGameSetup");
        assert_eq!((mnscrns.width, mnscrns.height), (472, 448));
        assert_eq!((coop.width, coop.height), (632, 568));
    }

    #[test]
    #[ignore]
    fn retail_generic_shell_backgrounds_decode_with_shell_palette() {
        let assets = retail_assets();
        let palette = load_named_palette(&assets, "SHELL.PAL").expect("SHELL.PAL");
        let small = render_shp_entry(&assets, "MNSCRNS.SHP", &palette, 0).expect("MNSCRNS frame 0");
        let large = render_shp_entry(&assets, "MNSCRNL.SHP", &palette, 0).expect("MNSCRNL frame 0");

        assert_eq!((small.width, small.height), (472, 448));
        assert_eq!((large.width, large.height), (632, 568));
    }
}
