//! Skirmish shell chrome atlas.
//!
//! Packs the shared native shell art into one GPU texture. Screen-specific
//! entries require active caller evidence and are selected by that screen.
//! The offline Skirmish `0x102` background and the LAN/WOL setup pages
//! `0xBB/0xBC/0xBD/0xC2/0xC9` follow `[VERA] YROriginalStyle=`: YR's
//! Cooperative setup art and palette when it is on, RA2's shared
//! MNSCRNS/MNSCRNL through SHELL.PAL by default. Every other entry keeps its
//! fixed RA2 or YR loader art.

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::chrome_atlas::{
    ChromeArt, ChromeAtlasEntry, ChromePiece, Need, load_palette, read_ui_shp, shp_file_piece,
    shp_piece, solid_piece,
};
use crate::render::gpu::GpuContext;
use crate::render::shell_ui_style_assets::{decode_ui_shp, mod_art_candidates, read_client_png};
use crate::ui::sidebar::SidebarTheme;

/// Original 72B060/72B0D0: neutral, Allied, Soviet, Yuri mode-2 assets.
const MODE_TWO_DIALOG_ART: [(&str, &str); 4] = [
    ("PUDLGBGN.SHP", "DIALOGN.PAL"),
    ("PUDLGBGA.SHP", "DIALOG.PAL"),
    ("PUDLGBGS.SHP", "DIALOG.PAL"),
    ("PUDLGBGY.SHP", "DIALOGY.PAL"),
];

/// Ares `DialogBackground.Image/.Palette`: `0x1002ECC0` loads the side's image
/// and palette into two slots at game load, each only from a set name that
/// loads, and the hook at `0x622223` (body `0x1002FB50`) draws them in place
/// of the side-selected stock background only while both are non-null. The
/// entry is labeled by both files, so a pair that reuses a stock SHP name with
/// another palette keeps its own entry.
fn render_side_dialog_art(
    assets: &AssetManager,
    (image, palette_name): (&str, &str),
) -> Option<ChromePiece> {
    let palette = load_palette(assets, palette_name)?;
    shp_file_piece(
        assets,
        image,
        &format!("{image}#{palette_name}"),
        &palette,
        0,
    )
}

/// VERA's neutral dialog background. The selected mod can supply the image
/// and its palette. The fallback is Red Alert 2's original pair, which uses
/// a different palette from the regular mode-2 background.
fn render_ra2_dialog_art(assets: &AssetManager) -> Option<ChromePiece> {
    const SHAPE: &str = "PUDLGBGN.SHP";
    const PALETTE: &str = "DIALOG.PAL";
    const LABEL: &str = "pudlgbgn.shp#ra2";
    let stock = assets.archive_with_source("ra2.mix -> local.mix");
    let palette = mod_art_candidates(assets, PALETTE)
        .find_map(|candidate| Palette::from_bytes(candidate.bytes).ok())
        .or_else(|| Palette::from_bytes(stock?.1.get_by_name(PALETTE)?).ok())?;
    mod_art_candidates(assets, SHAPE)
        .find_map(|candidate| {
            let shape = decode_ui_shp(candidate.bytes).ok()?;
            let art = shp_piece(&shape, LABEL, &palette, 0).ok()?;
            log::debug!("UI art {SHAPE} from {}", candidate.source);
            Some(art)
        })
        .or_else(|| {
            let shp = ShpFile::from_bytes(stock?.1.get_by_name(SHAPE)?).ok()?;
            shp_piece(&shp, LABEL, &palette, 0).ok()
        })
}

/// The atlas label of each mode-2 dialog background slot: the stock art, or
/// for the three side slots the rendered side pair. The no-session slot 0
/// (`0x62225F`) is not patched and keeps `PUDLGBGN`.
fn mode_two_dialog_labels(side_art: Option<&str>) -> [String; 4] {
    std::array::from_fn(|slot| match side_art {
        Some(label) if slot > 0 => label.to_owned(),
        _ => format!("{}#0", MODE_TWO_DIALOG_ART[slot].0.to_ascii_lowercase()),
    })
}

fn mode_two_dialog_background_slot(side: Option<SidebarTheme>) -> usize {
    match side {
        None => 0,
        Some(SidebarTheme::Allied) => 1,
        Some(SidebarTheme::Soviet) => 2,
        Some(SidebarTheme::Yuri) => 3,
    }
}
/// Shell bevel colours `0x00C5BEA7` and `0x00807A68` (`0x0060FA81..0x0060FA9B`),
/// packed `0x00BBGGRR`.
const PRIMITIVE_BEVEL_COLOR_A_RGB: [u8; 3] = [0xA7, 0xBE, 0xC5];
const PRIMITIVE_BEVEL_COLOR_B_RGB: [u8; 3] = [0x68, 0x7A, 0x80];
const TRACKBAR_FRAME_BORDER: i32 = 2;
/// The trackbar windows the shell paints, `(width, height, plaque reserve)`,
/// each with its own frame entry. Retail paints the main-menu shell's sliders
/// one pixel wider and taller than the template conversion
/// (`ui::shell::geom::child_window`): Skirmish `0x102` and the Options audio
/// sliders 129x22, Generate Map's Players 226x22, the Options plain sliders
/// 181x22, the shell's Sound Options `0xD6` volumes 261x22. The in-game B8
/// and BBB sliders keep their 263x21 and 192x21. The network setup dialogs
/// BC/BD/C2/C9 place their speed, credits and units sliders at 129x21 and the
/// co-op difficulty slider at 266x22, or 266x21 on WOL's host C2
/// (`0x0060B950` placement, `tools/storage_oracle/lan_lobby.json`).
pub const TRACKBAR_FRAMES: [(i32, i32, i32); 13] = [
    (129, 22, 50),
    (226, 22, 50),
    (263, 21, 50),
    (181, 22, 0),
    (181, 22, 50),
    (192, 21, 0),
    (261, 22, 50),
    (129, 21, 50),
    (266, 22, 0),
    (266, 21, 0),
    // The online pause menu FF's Connection and Game Speed (60B7A0).
    (210, 21, 50),
    // VERA's room's team bar: Teams and Room size, half again as wide as
    // the room's own 129x21 (`ui::multiplayer_setup`).
    (194, 21, 50),
    (crate::ui::shell::modal::ONLINE_CREATE_PLAYERS_WIDTH, 21, 50),
];
const SKIRMISH_FLAG_PCX_NAMES: [&str; 12] = [
    "usai.pcx", "japi.pcx", "frai.pcx", "geri.pcx", "gbri.pcx", "djbi.pcx", "arbi.pcx", "lati.pcx",
    "rusi.pcx", "yrii.pcx", "obsi.pcx", "rani.pcx",
];
/// Owner-draw preload 61F7FD..61F857 -> PCX cache 6B9D00 (embedded palette).
/// BB 5EE200 selects observer art; 5DAFE0 selects host/accepted glyphs.
const LAN_PCX_NAMES: [&str; 4] = ["wolnob.pcx", "wolob.pcx", "wolhost.pcx", "wolacpt.pcx"];
/// WOL setup C2/C9 art: the 0x3ED emote images (0x49C wodact, 0x4AA
/// wouact, C2 `0x791231`).
const WOL_SETUP_PCX_NAMES: [&str; 2] = ["wodact.pcx", "wouact.pcx"];
/// YR `0x72CF40/0x72D030` setup pair, shared by offline Skirmish `0x102` and
/// the BC/BD/C2/C9 setup family: the large art and the palette that decodes
/// both sizes. Only the offline Skirmish screen selects it, under
/// `[VERA] YROriginalStyle=`.
const COOP_SETUP_SHAPE: &str = "MnScrnLCoopGameSetup.shp";
const COOP_SETUP_PALETTE: &str = "MnScrnLCoopGameSetup.PAL";

pub type SkirmishShellChromeEntry = ChromeAtlasEntry;

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
    pub choose_map_background_800_customize_battle: Option<SkirmishShellChromeEntry>,
    /// Choose Map at 640 wide: MNSCRNS through MnScrnLCustomizeBattle.PAL.
    pub choose_map_background_640_customize_battle: Option<SkirmishShellChromeEntry>,
    /// Generic common-shell small background, decoded through SHELL.PAL.
    pub generic_background_640_mnscrns_shell: Option<SkirmishShellChromeEntry>,
    /// Generic common-shell large background, decoded through SHELL.PAL.
    pub generic_background_large_mnscrnl_shell: Option<SkirmishShellChromeEntry>,
    /// Style identity for rebuilding this derived atlas when the Options
    /// preview changes the selected source family.
    pub yr_original_style: bool,
    /// The setup-family background pair, slot 0 at exactly 640 wide and slot 1
    /// otherwise: shared by offline Skirmish `0x102` and the LAN/WOL pages
    /// `0xBB/0xBC/0xBD/0xC2/0xC9`. YR style selects the Cooperative setup art
    /// and palette, RA2 style the shared MNSCRN pair through SHELL.PAL.
    pub setup_backgrounds: [Option<SkirmishShellChromeEntry>; 2],
    /// The client theme backdrop used by offline Skirmish only.
    pub client_skirmish_background: Option<SkirmishShellChromeEntry>,
    lan_icons: [Option<SkirmishShellChromeEntry>; 4],
    wol_setup_icons: [Option<SkirmishShellChromeEntry>; 2],
    /// VERA's online room: the lobby's badge PCX images by file name.
    wol_badges: Vec<(&'static str, SkirmishShellChromeEntry)>,
    pub lower_side_640_lwscrns: Option<SkirmishShellChromeEntry>,
    pub lower_side_large_lwscrnl: Option<SkirmishShellChromeEntry>,
    mode_two_dialog_backgrounds: [Option<SkirmishShellChromeEntry>; 4],
    /// Red Alert 2's neutral dialog background ([`render_ra2_dialog_art`]).
    pub ra2_dialog_background: Option<SkirmishShellChromeEntry>,
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
    /// Dual-state checkbox halves (message 0x4E5 of 0x006163A0): checked
    /// but not answered (`cce_il`), answered but not checked (`cce_ir`).
    checkbox_dual_left_cce_il: Option<SkirmishShellChromeEntry>,
    checkbox_dual_right_cce_ir: Option<SkirmishShellChromeEntry>,
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
    /// Both border-2 frames of each [`TRACKBAR_FRAMES`] window, including the
    /// two-pixel outside expansion.
    pub trackbar_frames: [Option<SkirmishShellChromeEntry>; TRACKBAR_FRAMES.len()],
    /// Keyboard A3 category face, original resource138 DLU.
    pub combo_face_249: Option<SkirmishShellChromeEntry>,
    pub combo_face_207: Option<SkirmishShellChromeEntry>,
    pub combo_face_180: Option<SkirmishShellChromeEntry>,
    pub combo_face_150: Option<SkirmishShellChromeEntry>,
    /// BC/BD resource windows after60C4A0:151/118/42/39px, faceheight24.
    pub combo_face_151: Option<SkirmishShellChromeEntry>,
    pub combo_face_118: Option<SkirmishShellChromeEntry>,
    pub combo_face_42: Option<SkirmishShellChromeEntry>,
    pub combo_face_39: Option<SkirmishShellChromeEntry>,
    pub combo_face_213: Option<SkirmishShellChromeEntry>,
    pub combo_face_117: Option<SkirmishShellChromeEntry>,
    pub combo_face_44: Option<SkirmishShellChromeEntry>,
    pub combo_face_38: Option<SkirmishShellChromeEntry>,
    pub white_pixel: Option<SkirmishShellChromeEntry>,
    pub start_marker: Option<SkirmishShellChromeEntry>,
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
    pub checkbox_dual_left_cce_il: Option<SkirmishShellChromeEntry>,
    pub checkbox_dual_right_cce_ir: Option<SkirmishShellChromeEntry>,
    /// The frames of the [`TRACKBAR_FRAMES`] windows, in that order.
    pub trackbar_frames: [Option<SkirmishShellChromeEntry>; TRACKBAR_FRAMES.len()],
    /// Keyboard A3 category face, original resource138 DLU.
    pub combo_face_249: Option<SkirmishShellChromeEntry>,
    pub combo_face_207: Option<SkirmishShellChromeEntry>,
    pub combo_face_180: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_left_trofl: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_mid_trofm: Option<SkirmishShellChromeEntry>,
    pub trackbar_plaque_right_trofr: Option<SkirmishShellChromeEntry>,
    pub trackbar_thumb_trakgrip: Option<SkirmishShellChromeEntry>,
    pub white_pixel: Option<SkirmishShellChromeEntry>,
    pub combo_face_150: Option<SkirmishShellChromeEntry>,
    pub combo_face_151: Option<SkirmishShellChromeEntry>,
    pub combo_face_118: Option<SkirmishShellChromeEntry>,
    pub combo_face_42: Option<SkirmishShellChromeEntry>,
    pub combo_face_39: Option<SkirmishShellChromeEntry>,
    pub combo_face_213: Option<SkirmishShellChromeEntry>,
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
    /// Original 60CF00 assigns BB the generic shell art, and LAN BC/BD and WOL
    /// C2/C9 the 72D030 Cooperative setup art (MnScrnLCoopGameSetup.shp/.PAL),
    /// which loader72CF40 (LAN5DCE93/5E0963) loads at800 and72CF90 frees.
    ///
    /// VERA follows the persisted shell style here: `[VERA] YROriginalStyle=`
    /// gives the whole setup family the Cooperative art and palette, while the
    /// default RA2 style paints BB's generic MNSCRNS/MNSCRNL through SHELL.PAL.
    pub fn lan_background(&self, dialog_id: u16, width: u32) -> Option<SkirmishShellChromeEntry> {
        match (dialog_id, width) {
            (0xBB | 0xBC | 0xBD | 0xC2 | 0xC9, 640) => self.setup_backgrounds[0],
            (0xBB | 0xBC | 0xBD | 0xC2 | 0xC9, _) => self.setup_backgrounds[1],
            _ => None,
        }
    }

    /// BB5EE200 ->603D30 kind-2 image static, RGB565 magenta keyed.
    pub fn lan_observer_toggle(&self, observer: bool) -> Option<SkirmishShellChromeEntry> {
        self.lan_icons[usize::from(observer)]
    }

    /// BB5DAFE0 -> country insignia owner4E3560. Reuse the flags already
    /// loaded for country selectors; unsupported names have no substitute.
    pub fn lan_player_side(&self, name: &str) -> Option<SkirmishShellChromeEntry> {
        self.flags
            .iter()
            .find(|(file, _)| file.eq_ignore_ascii_case(name))
            .map(|(_, entry)| *entry)
    }

    /// C2/C9 0x3ED images: element+0x14 (`wodact`, message 0x49C) and
    /// element+0x18 (`wouact`, message 0x4AA).
    pub fn wol_emote_images(
        &self,
    ) -> (
        Option<SkirmishShellChromeEntry>,
        Option<SkirmishShellChromeEntry>,
    ) {
        (self.wol_setup_icons[0], self.wol_setup_icons[1])
    }

    /// VERA's online room: a player's badge ([`crate::ui::wol_lobby::badge`]).
    pub fn wol_badge(&self, file: &str) -> Option<SkirmishShellChromeEntry> {
        self.wol_badges
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, entry)| *entry)
    }

    /// BB5DAFE0 gives the host glyph precedence, then accepted, else no icon.
    pub fn lan_player_status(
        &self,
        host: bool,
        accepted: bool,
    ) -> Option<SkirmishShellChromeEntry> {
        if host {
            self.lan_icons[2]
        } else if accepted {
            self.lan_icons[3]
        } else {
            None
        }
    }

    /// Original mode-2 paint (`621E90`, branch `6221D1`) selects neutral art
    /// outside a session, otherwise the scenario's side. Missing selected art or
    /// palette skips the SHP; it does not borrow another side's background.
    pub fn mode_two_dialog_background(
        &self,
        side: Option<SidebarTheme>,
    ) -> Option<SkirmishShellChromeEntry> {
        self.mode_two_dialog_backgrounds[mode_two_dialog_background_slot(side)]
    }

    /// Snapshot the control glyph entries into a Default-able, texture-free subset
    /// the paint seam can take (and tests can build by hand). Entries are `Copy`.
    pub fn control_chrome(&self) -> ControlChrome {
        ControlChrome {
            checkbox_unchecked_cue_i: self.checkbox_unchecked_cue_i,
            checkbox_checked_cce_i: self.checkbox_checked_cce_i,
            checkbox_dual_left_cce_il: self.checkbox_dual_left_cce_il,
            checkbox_dual_right_cce_ir: self.checkbox_dual_right_cce_ir,
            trackbar_frames: self.trackbar_frames,
            combo_face_249: self.combo_face_249,
            combo_face_207: self.combo_face_207,
            combo_face_180: self.combo_face_180,
            trackbar_plaque_left_trofl: self.trackbar_plaque_left_trofl,
            trackbar_plaque_mid_trofm: self.trackbar_plaque_mid_trofm,
            trackbar_plaque_right_trofr: self.trackbar_plaque_right_trofr,
            trackbar_thumb_trakgrip: self.trackbar_thumb_trakgrip,
            white_pixel: self.white_pixel,
            combo_face_150: self.combo_face_150,
            combo_face_151: self.combo_face_151,
            combo_face_118: self.combo_face_118,
            combo_face_42: self.combo_face_42,
            combo_face_39: self.combo_face_39,
            combo_face_213: self.combo_face_213,
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

/// The [`TRACKBAR_FRAMES`] slot of a `width` x `height` trackbar window, with
/// or without the value plaque.
pub fn trackbar_frame_slot(plaque: bool, width: i32, height: i32) -> Option<usize> {
    TRACKBAR_FRAMES
        .iter()
        .position(|&(w, h, reserve)| (w, h, reserve > 0) == (width, height, plaque))
}

/// The [`TRACKBAR_FRAMES`] slot of a `width` x `height` trackbar window
/// drawn at `scale`. Below the shell's own size (VERA's team bar draws its
/// controls at 78%) the window's size rounds back from its authored one by
/// a pixel or two, so it takes the frame of the nearest window within two
/// pixels each way.
pub fn drawn_trackbar_frame_slot(
    plaque: bool,
    width: i32,
    height: i32,
    scale: crate::ui::shell::geom::ShellScale,
) -> Option<usize> {
    if scale.factor() >= 1.0 {
        return trackbar_frame_slot(plaque, width, height);
    }
    TRACKBAR_FRAMES
        .iter()
        .enumerate()
        .filter(|&(_, &(w, h, reserve))| {
            (reserve > 0) == plaque && (w - width).abs() <= 2 && (h - height).abs() <= 2
        })
        .min_by_key(|&(_, &(w, h, _))| (w - width).abs() + (h - height).abs())
        .map(|(slot, _)| slot)
}

impl ControlChrome {
    /// The frame entry of a `width` x `height` trackbar window.
    #[cfg(test)]
    pub fn trackbar_frame(
        &self,
        plaque: bool,
        width: i32,
        height: i32,
    ) -> Option<SkirmishShellChromeEntry> {
        self.trackbar_frames[trackbar_frame_slot(plaque, width, height)?]
    }

    /// The owner-draw list scrollbar art (`0x0061C690`).
    pub fn list_scroll(&self) -> crate::render::main_menu_shell_chrome::ListScrollArt {
        crate::render::main_menu_shell_chrome::ListScrollArt {
            up_released: self.scrollbar_arrow_up_released,
            up_pressed: self.scrollbar_arrow_up_pressed,
            down_released: self.scrollbar_arrow_down_released,
            down_pressed: self.scrollbar_arrow_down_pressed,
            grip_top: self.scrollbar_thumb_top,
            grip_mid: self.scrollbar_thumb_mid,
            grip_bottom: self.scrollbar_thumb_bottom,
        }
    }
}

pub fn build_skirmish_shell_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    assets: &AssetManager,
    yr_original_style: bool,
    extra_flags: &[String],
    dialog_background: Option<(&str, &str)>,
) -> Option<SkirmishShellChromeAtlas> {
    let shell = load_palette(assets, "SHELL.PAL")?;
    let shell2 = load_palette(assets, "SHELL2.PAL")?;
    let mut art = ChromeArt::new(assets, "Skirmish shell");
    // Original SDWRNANM header supplies 91 frames; keep the runtime sequence
    // bounded by the available retail frames, without stretching the canvas.
    art.shp_run("SDWRNANM.SHP", &shell2, "sdwrnanm.shp", 91);
    // Both frames, from one copy that has them.
    shp_frames(
        &mut art,
        assets,
        "SDTP.SHP",
        &shell,
        0..2,
        "sdtp.shp",
        Need::Required,
        2,
    )?;
    art.shp(
        "SDBTNBKGD.SHP",
        &shell2,
        0..1,
        "sdbtnbkgd.shp",
        Need::Required,
    )?;
    art.shp("SDBTM.SHP", &shell, 0..1, "sdbtm.shp", Need::Required)?;
    // Bake the full frame range the slide-in wave references (Group A uses 1/5..=10,
    // Group B uses 0/11..=16) plus the idle frames 2/3/4 and overlay 10; a
    // missing frame is not substituted.
    if let Some(palette) = load_palette(assets, "SDBTNANM.PAL") {
        let min = if assets.mod_selected() { 5 } else { 1 };
        let file = "SDBTNANM.SHP";
        shp_frames(
            &mut art,
            assets,
            file,
            &palette,
            0..17,
            "sdbtnanm.shp",
            Need::Optional,
            min,
        );
    }
    // Without MNBTTN the validation modal falls back to generic button art.
    if let Some(palette) = load_palette(assets, "MAINBTTN.PAL") {
        let min = if assets.mod_selected() { 3 } else { 1 };
        let file = "MNBTTN.SHP";
        shp_frames(
            &mut art,
            assets,
            file,
            &palette,
            0..3,
            "mnbttn.shp",
            Need::Optional,
            min,
        );
    }
    // Original loaders 72B060/72B0D0 and getters 72AFF0/72B010/72B030.
    // AssetManager retains the mounted retail winner and the SHP's dimensions.
    for (name, palette) in MODE_TWO_DIALOG_ART {
        if let Some(palette) = load_palette(assets, palette) {
            art.shp(
                name,
                &palette,
                0..1,
                &name.to_ascii_lowercase(),
                Need::Optional,
            );
        }
    }
    let side_dialog_art = dialog_background
        .and_then(|pair| render_side_dialog_art(assets, pair))
        .map(|piece| {
            let label = piece.label.clone();
            art.push(piece);
            label
        });
    art.push_or(
        render_ra2_dialog_art(assets),
        "PUDLGBGN.SHP from ra2.mix -> local.mix",
        Need::Optional,
    );
    // The slide engine's map-button and warning-display frames.
    art.shp("SDMPBTN.SHP", &shell, 0..1, "sdmpbtn.shp", Need::Required)?;
    art.shp("SDMPBTN.SHP", &shell, 1..7, "sdmpbtn.shp", Need::Silent);
    art.shp("SDWRNTMP.SHP", &shell, 0..6, "sdwrntmp.shp", Need::Silent);
    art.shp("LWSCRNS.SHP", &shell, 0..1, "lwscrns.shp", Need::Required)?;
    art.shp("LWSCRNL.SHP", &shell, 0..1, "lwscrnl.shp", Need::Required)?;
    art.shp(
        "MNSCRNS.SHP",
        &shell,
        0..1,
        "mnscrns.shp:shell",
        Need::Optional,
    );
    art.shp(
        "MNSCRNL.SHP",
        &shell,
        0..1,
        "mnscrnl.shp:shell",
        Need::Optional,
    );
    // `0x72E730` keeps MNSCRNS.SHP at exactly 640 but decodes it through the
    // Cooperative setup palette; the large shape is the Cooperative setup art.
    // RA2 style keeps the shared MNSCRN pair above.
    if yr_original_style && let Some(palette) = load_palette(assets, COOP_SETUP_PALETTE) {
        art.shp(
            "MNSCRNS.SHP",
            &palette,
            0..1,
            "mnscrns.shp:offline-setup",
            Need::Optional,
        );
        art.shp(
            COOP_SETUP_SHAPE,
            &palette,
            0..1,
            "mnscrnlcoopgamesetup.shp",
            Need::Optional,
        );
    }
    for (small, label) in [(true, "mod-setup-small"), (false, "mod-setup-large")] {
        if let Some(piece) = render_mod_setup_background(assets, yr_original_style, small, label) {
            art.push(piece);
        }
    }
    if let Some(path) = assets
        .client_install()
        .and_then(|install| install.theme.skirmish_background.as_deref())
        && let Some(image) = read_client_png(path)
    {
        log::info!("Skirmish client background from {}", path.display());
        art.push(ChromePiece {
            label: "client-skirmish-background".into(),
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        });
    }
    if let Some(palette) = load_palette(assets, "MnScrnLCustomizeBattle.PAL") {
        let shape = "MnScrnLCustomizeBattle.shp";
        art.shp(
            shape,
            &palette,
            0..1,
            "mnscrnlcustomizebattle.shp",
            Need::Optional,
        );
        // At 640 wide `0x6B` draws the small MNSCRNS through its own
        // CustomizeBattle palette (`0x0060CF00`, `0x0072E7AD`).
        art.shp(
            "MNSCRNS.SHP",
            &palette,
            0..1,
            "mnscrns.shp:customizebattle",
            Need::Optional,
        );
    }
    art.shp(
        START_MARKER_SHP,
        &shell,
        0..1,
        "startbut.shp",
        Need::Optional,
    );
    art.pcx(
        [
            "bue_li30.pcx",
            "bue_mi30.pcx",
            "bue_ri30.pcx",
            "bde_li30.pcx",
            "bde_mi30.pcx",
            "bde_ri30.pcx",
            "cue_i.pcx",
            "cce_i.pcx",
            "cce_il.pcx",
            "cce_ir.pcx",
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
        ],
        false,
        Need::Optional,
    );
    for (width, height, reserve) in TRACKBAR_FRAMES {
        art.push(render_trackbar_frame_geometry(
            &trackbar_frame_label(width, height, reserve),
            width as u32,
            height as u32,
            reserve,
        ));
    }
    // BC/BD use the same 617250 face painter: 61745C height 24,
    // 617893->6208F0 thickness 2, at their actual child-window widths; WOL
    // login 0x10F's nickname combo 0x628 is 213 wide.
    for width in COMBO_FACE_WIDTHS {
        let label = format!("combo_face_{width}");
        art.push(render_primitive_bevel_entry(
            &label,
            width,
            24,
            [2, 2, width as i32 - 4, 20],
            2,
        ));
    }
    art.push(solid_piece("skirmish_white_pixel", 1, 1, [255; 4]));

    let mut flag_names: Vec<&str> = SKIRMISH_FLAG_PCX_NAMES.to_vec();
    for name in extra_flags {
        if !flag_names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            flag_names.push(name.as_str());
        }
    }
    // 6B9D00 packs the embedded PCX palette to display RGB565; flags, list
    // images and kind-2 statics compare the same packed magenta key.
    art.pcx(flag_names.iter().copied(), true, Need::Optional);
    art.pcx(
        LAN_PCX_NAMES
            .into_iter()
            .chain(WOL_SETUP_PCX_NAMES)
            .chain(crate::ui::wol_lobby::badge_files()),
        true,
        Need::Optional,
    );

    let (texture, labels) = art.pack(gpu, batch, "Skirmish shell chrome")?;
    let get = |label: &str| labels.get(label);
    let first = |label: &str| labels.frame(label, 0);
    let face = |width: u32| labels.get(&format!("combo_face_{width}"));
    let sdbtnanm: [_; 17] = labels.frames("sdbtnanm.shp");
    Some(SkirmishShellChromeAtlas {
        texture,
        yr_original_style,
        setup_backgrounds: setup_background_pair(yr_original_style, first, get),
        client_skirmish_background: get("client-skirmish-background"),
        launcher_warning_frames: labels.run("sdwrnanm.shp"),
        right_panel_top_sdtp: first("sdtp.shp"),
        right_panel_top_highlight_sdtp_frame1: labels.frame("sdtp.shp", 1),
        right_panel_tile_sdbtnbkgd: first("sdbtnbkgd.shp"),
        right_panel_overlay_sdbtnanm_frame10: sdbtnanm[10],
        right_panel_button_sdbtnanm_frame2: sdbtnanm[2],
        right_panel_button_sdbtnanm_frame3: sdbtnanm[3],
        right_panel_button_sdbtnanm_frame4: sdbtnanm[4],
        right_panel_button_sdbtnanm_frames: sdbtnanm,
        right_panel_bottom_sdbtm: first("sdbtm.shp"),
        sd_map_button: first("sdmpbtn.shp"),
        sd_map_button_frames: labels.frames("sdmpbtn.shp"),
        sd_warning_frames: labels.frames("sdwrntmp.shp"),
        choose_map_background_800_customize_battle: first("mnscrnlcustomizebattle.shp"),
        choose_map_background_640_customize_battle: first("mnscrns.shp:customizebattle"),
        generic_background_640_mnscrns_shell: first("mnscrns.shp:shell"),
        generic_background_large_mnscrnl_shell: first("mnscrnl.shp:shell"),
        lan_icons: LAN_PCX_NAMES.map(get),
        wol_setup_icons: WOL_SETUP_PCX_NAMES.map(get),
        wol_badges: crate::ui::wol_lobby::badge_files()
            .filter_map(|name| Some((name, get(name)?)))
            .collect(),
        lower_side_640_lwscrns: first("lwscrns.shp"),
        lower_side_large_lwscrnl: first("lwscrnl.shp"),
        mode_two_dialog_backgrounds: mode_two_dialog_labels(side_dialog_art.as_deref())
            .map(|label| labels.get(&label)),
        ra2_dialog_background: get("pudlgbgn.shp#ra2"),
        button_up_left_30: get("bue_li30.pcx"),
        button_up_mid_30: get("bue_mi30.pcx"),
        button_up_right_30: get("bue_ri30.pcx"),
        button_down_left_30: get("bde_li30.pcx"),
        button_down_mid_30: get("bde_mi30.pcx"),
        button_down_right_30: get("bde_ri30.pcx"),
        modal_button_mnbttn_frame0: labels.frame("mnbttn.shp", 0),
        modal_button_mnbttn_frame1: labels.frame("mnbttn.shp", 1),
        modal_button_mnbttn_frame2: labels.frame("mnbttn.shp", 2),
        checkbox_unchecked_cue_i: get("cue_i.pcx"),
        checkbox_checked_cce_i: get("cce_i.pcx"),
        checkbox_dual_left_cce_il: get("cce_il.pcx"),
        checkbox_dual_right_cce_ir: get("cce_ir.pcx"),
        trackbar_thumb_trakgrip: get("trakgrip.pcx"),
        trackbar_plaque_left_trofl: get("trofl.pcx"),
        trackbar_plaque_mid_trofm: get("trofm.pcx"),
        trackbar_plaque_right_trofr: get("trofr.pcx"),
        combo_arrow_down_released: get("dnarrowr.pcx"),
        combo_arrow_down_pressed: get("dnarrowp.pcx"),
        combo_arrow_down_gray_released: get("gdnarrowr.pcx"),
        combo_arrow_down_gray_pressed: get("gdnarrowp.pcx"),
        scrollbar_arrow_up_released: get("uparrowr.pcx"),
        scrollbar_arrow_up_pressed: get("uparrowp.pcx"),
        scrollbar_arrow_down_released: get("dnarrowr.pcx"),
        scrollbar_arrow_down_pressed: get("dnarrowp.pcx"),
        scrollbar_thumb_top: get("sbgript.pcx"),
        scrollbar_thumb_mid: get("sbgripm.pcx"),
        scrollbar_thumb_bottom: get("sbgripb.pcx"),
        trackbar_frames: TRACKBAR_FRAMES.map(|(width, height, reserve)| {
            labels.get(&trackbar_frame_label(width, height, reserve))
        }),
        combo_face_249: face(249),
        combo_face_207: face(207),
        combo_face_180: face(180),
        combo_face_150: face(150),
        combo_face_151: face(151),
        combo_face_118: face(118),
        combo_face_42: face(42),
        combo_face_39: face(39),
        combo_face_213: face(213),
        combo_face_117: face(117),
        combo_face_44: face(44),
        combo_face_38: face(38),
        white_pixel: get("skirmish_white_pixel"),
        start_marker: first("startbut.shp"),
        flags: flag_names
            .into_iter()
            .filter_map(|name| Some((name.to_string(), get(&name.to_ascii_lowercase())?)))
            .collect(),
    })
}

/// The owner-draw combo faces the atlas bakes, in packing order: Skirmish
/// 150, the launcher's 180, Keyboard A3's 207 and the network page's 249
/// (resource 138 DLU), Skirmish 117/44/38, LAN BC/BD 151/118/42/39 and WOL
/// login's 213.
const COMBO_FACE_WIDTHS: [u32; 12] = [150, 180, 207, 249, 117, 44, 38, 151, 118, 42, 39, 213];

/// Replace the preferred setup family when the mod supplies the other one.
/// Consider a replaced background before a palette-only change, since the
/// generic shell palette can also be replaced for buttons and borders. At
/// 640 the shared image alone cannot select the other palette.
fn render_mod_setup_background(
    assets: &AssetManager,
    yr_original_style: bool,
    small: bool,
    label: &str,
) -> Option<ChromePiece> {
    if !assets.mod_selected() {
        return None;
    }
    let select_family = |require_mod_shape: bool| {
        [yr_original_style, !yr_original_style]
            .into_iter()
            .find_map(|yr| {
                let (shape_name, palette_name) = match (small, yr) {
                    (true, false) => ("MNSCRNS.SHP", "SHELL.PAL"),
                    (true, true) => ("MNSCRNS.SHP", COOP_SETUP_PALETTE),
                    (false, false) => ("MNSCRNL.SHP", "SHELL.PAL"),
                    (false, true) => (COOP_SETUP_SHAPE, COOP_SETUP_PALETTE),
                };
                let mod_palette = mod_art_candidates(assets, palette_name).find_map(|candidate| {
                    Palette::from_bytes(candidate.bytes)
                        .ok()
                        .map(|palette| (palette, candidate.source))
                });
                if !require_mod_shape {
                    let (palette, source) = mod_palette?;
                    let art = shp_file_piece(assets, shape_name, label, &palette, 0)?;
                    log::debug!(
                        "Setup background {shape_name} with UI palette {palette_name} from {source}"
                    );
                    return Some((yr, art));
                }
                if small && yr != yr_original_style && mod_palette.is_none() {
                    return None;
                }
                let mut candidates = mod_art_candidates(assets, shape_name).peekable();
                candidates.peek()?;
                let palette = mod_palette
                    .map(|(palette, _)| palette)
                    .or_else(|| load_palette(assets, palette_name))?;
                candidates.find_map(|candidate| {
                    let shape = decode_ui_shp(candidate.bytes).ok()?;
                    let art = shp_piece(&shape, label, &palette, 0).ok()?;
                    log::debug!("Setup background {shape_name} from {}", candidate.source);
                    Some((yr, art))
                })
            })
    };
    let (yr, art) = select_family(true).or_else(|| select_family(false))?;
    (yr != yr_original_style).then_some(art)
}

/// The setup-family background pair for the selected style: YR's Cooperative
/// setup art, or RA2's shared MNSCRN pair. Shared by offline Skirmish `0x102`
/// and the LAN/WOL pages. A YR entry that failed to load falls back per slot,
/// so a missing setup palette keeps the backdrop rather than blanking it.
fn setup_background_pair(
    yr_original_style: bool,
    first: impl Fn(&str) -> Option<SkirmishShellChromeEntry>,
    get: impl Fn(&str) -> Option<SkirmishShellChromeEntry>,
) -> [Option<SkirmishShellChromeEntry>; 2] {
    let ra2 = [first("mnscrns.shp:shell"), first("mnscrnl.shp:shell")];
    let stock = if yr_original_style {
        [
            first("mnscrns.shp:offline-setup").or(ra2[0]),
            first("mnscrnlcoopgamesetup.shp").or(ra2[1]),
        ]
    } else {
        ra2
    };
    // A mod's replacement for the other family wins
    // ([`render_mod_setup_background`]).
    std::array::from_fn(|slot| get(["mod-setup-small", "mod-setup-large"][slot]).or(stock[slot]))
}

/// `frames` of SHP `file` through `palette` onto `art`, each labelled
/// `label#frame` as [`ChromeArt::shp`] labels them, from the first copy
/// with at least `min_frames` frames: a selected mod's copy with fewer (a
/// series it cut short, or a broken file) falls through to the next copy,
/// so outside data never leaves a half animation. `None` when any frame is
/// missing.
#[allow(clippy::too_many_arguments)]
fn shp_frames(
    art: &mut ChromeArt<'_>,
    assets: &AssetManager,
    file: &str,
    palette: &Palette,
    frames: std::ops::Range<usize>,
    label: &str,
    need: Need,
    min_frames: usize,
) -> Option<()> {
    let shp = read_ui_shp(assets, file, |shp| {
        anyhow::ensure!(
            shp.frames.len() >= min_frames,
            "{file} requires {min_frames} frames"
        );
        Ok(shp)
    })
    .map_err(|error| log::debug!("{error:#}"))
    .ok();
    let mut complete = true;
    for frame in frames {
        let piece = shp
            .as_ref()
            .and_then(|shp| shp_piece(shp, &format!("{label}#{frame}"), palette, frame).ok());
        complete &= piece.is_some();
        art.push_or(piece, &format!("{file} frame {frame}"), need);
    }
    complete.then_some(())
}

/// The skirmish preview's start marker, drawn by `DrawStartPositions 0x00640710`.
const START_MARKER_SHP: &str = "STARTBUT.SHP";

/// The start marker piece the atlas packs (frame 0 on its canvas in
/// SHELL.PAL), for pictures of a map drawn outside the shell.
pub(crate) fn start_marker_art(assets: &AssetManager) -> Option<ChromePiece> {
    let palette = load_palette(assets, "SHELL.PAL")?;
    shp_file_piece(assets, START_MARKER_SHP, "startbut.shp", &palette, 0)
}

fn render_primitive_bevel_entry(
    label: &str,
    width: u32,
    height: u32,
    box_xywh: [i32; 4],
    border: i32,
) -> ChromePiece {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    draw_primitive_bevel(&mut rgba, width, height, box_xywh, border);
    ChromePiece {
        label: label.to_ascii_lowercase(),
        width,
        height,
        rgba,
    }
}

fn trackbar_frame_label(width: i32, height: i32, reserve: i32) -> String {
    format!("trackbar_frame_{width}x{height}_{reserve}")
}

/// Original 61E1B9..61E269: draw the rail frame at the current control size,
/// then the value frame `reserve - inset` wide past it, where the inset is 2
/// with the value plaque on and 1 without (`0x0061E22A..0x0061E235`). A
/// plaque-less control still draws that second box, one pixel wide negative,
/// which leaves three extra columns at the rail's right end. Both use
/// 6208F0's two-pixel outside expansion. Never stretch a split-frame bitmap.
fn render_trackbar_frame_geometry(
    label: &str,
    control_width: u32,
    control_height: u32,
    reserve: i32,
) -> ChromePiece {
    let border = TRACKBAR_FRAME_BORDER;
    let width = control_width + (border as u32) * 2;
    let height = control_height + (border as u32) * 2;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let control_w = control_width as i32;
    let control_h = control_height as i32;
    let left_frame_w = control_w - reserve;
    let inset = if reserve > 0 { 2 } else { 1 };
    let value_frame_x = left_frame_w + inset;
    let value_frame_w = reserve - inset;

    // `ShellTrackbar__WndProc` (`0x0061D950`) supplies control-relative boxes and
    // FUN_006208F0 expands each by two pixels. Shift both inputs by the canvas
    // border so the complete outside rings fit in this transparent entry.
    draw_primitive_bevel(
        &mut rgba,
        width,
        height,
        [border, border, left_frame_w, control_h],
        border,
    );
    draw_primitive_bevel(
        &mut rgba,
        width,
        height,
        [border + value_frame_x, border, value_frame_w, control_h],
        border,
    );

    ChromePiece {
        label: label.to_ascii_lowercase(),
        width,
        height,
        rgba,
    }
}

/// A trackbar frame canvas (`width`, `height`, RGBA) for a control of
/// `control_width` x `control_height`, the control at `(2, 2)` inside it.
pub(crate) fn trackbar_frame_rgba(
    control_width: u32,
    control_height: u32,
    reserve: i32,
) -> (u32, u32, Vec<u8>) {
    let entry = render_trackbar_frame_geometry("trackbar", control_width, control_height, reserve);
    (entry.width, entry.height, entry.rgba)
}

/// `0x006208F0`: `border` rings outside `box_xywh`, light top-left and dark
/// bottom-right (swapped on the inner ring of a two-pixel border), averaged
/// corners. Like the original it draws whatever its edges give, a box of
/// width -1 included.
fn draw_primitive_bevel(rgba: &mut [u8], width: u32, height: u32, box_xywh: [i32; 4], border: i32) {
    if border <= 0 {
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

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::{
        AssetManager, COOP_SETUP_PALETTE, COOP_SETUP_SHAPE, ChromePiece,
        PRIMITIVE_BEVEL_COLOR_A_RGB, PRIMITIVE_BEVEL_COLOR_B_RGB, SkirmishShellChromeEntry,
        TRACKBAR_FRAME_BORDER, average_rgb, draw_axis_line_inclusive_clipped,
        render_primitive_bevel_entry, render_trackbar_frame_geometry, rgba_color,
        setup_background_pair, trackbar_frame_rgba,
    };
    use super::{TRACKBAR_FRAMES, drawn_trackbar_frame_slot};
    use crate::assets::asset_manager::MediaArchiveMode;
    use crate::render::chrome_atlas::{
        load_palette as load_named_palette, pcx_piece, shp_file_piece,
    };
    use std::collections::HashMap;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ShellAssetRole {
        VerifiedParentBackground,
        VerifiedChooseMapBackground,
        VerifiedLanIcon,
        VerifiedOfflineStartMarker,
        AssignedPlayerMarker,
        RightPanelChrome,
        VerifiedModalDialogBackground,
        VerifiedOwnerDrawButton,
        VerifiedFlag,
        ResearchCandidate,
        Other,
    }

    fn classify_shell_asset(name: &str) -> ShellAssetRole {
        match name.to_ascii_lowercase().as_str() {
            "mnscrns.shp" | "mnscrnl.shp" => ShellAssetRole::VerifiedParentBackground,
            "mnscrnlcustomizebattle.shp" => ShellAssetRole::VerifiedChooseMapBackground,
            "wolnob.pcx" | "wolob.pcx" | "wolhost.pcx" | "wolacpt.pcx" => {
                ShellAssetRole::VerifiedLanIcon
            }
            "startbut.shp" => ShellAssetRole::VerifiedOfflineStartMarker,
            "mmpb.shp" => ShellAssetRole::AssignedPlayerMarker,
            "sdtp.shp" | "sdbtnbkgd.shp" | "sdbtm.shp" | "sdbtnanm.shp" | "sdmpbtn.shp"
            | "lwscrns.shp" | "lwscrnl.shp" => ShellAssetRole::RightPanelChrome,
            "bue_li30.pcx" | "bue_mi30.pcx" | "bue_ri30.pcx" | "bde_li30.pcx" | "bde_mi30.pcx"
            | "bde_ri30.pcx" | "cue_i.pcx" | "cce_i.pcx" | "cce_il.pcx" | "cce_ir.pcx"
            | "trakgrip.pcx" | "trofl.pcx" | "trofm.pcx" | "trofr.pcx" | "dnarrowr.pcx"
            | "dnarrowp.pcx" | "gdnarrowr.pcx" | "gdnarrowp.pcx" | "mnbttn.shp"
            | "sidebttn.shp" => ShellAssetRole::VerifiedOwnerDrawButton,
            "pudlgbgn.shp" | "pudlgbga.shp" | "pudlgbgs.shp" | "pudlgbgy.shp" => {
                ShellAssetRole::VerifiedModalDialogBackground
            }
            "usai.pcx" | "japi.pcx" | "frai.pcx" | "geri.pcx" | "gbri.pcx" | "djbi.pcx"
            | "arbi.pcx" | "lati.pcx" | "rusi.pcx" | "yrii.pcx" | "obsi.pcx" | "rani.pcx" => {
                ShellAssetRole::VerifiedFlag
            }
            "dbak6440.pcx" | "dlgsysa.pcx" | "dlgsysi.pcx" => ShellAssetRole::ResearchCandidate,
            _ => ShellAssetRole::Other,
        }
    }

    fn render_shp_entry(
        assets: &AssetManager,
        file: &str,
        palette: &crate::assets::pal_file::Palette,
        frame: usize,
    ) -> Option<ChromePiece> {
        shp_file_piece(assets, file, file, palette, frame)
    }

    /// The hook at `0x622223` keeps the stock selection unless both slots of
    /// the side's pair loaded; a loaded pair takes the three side slots, keyed
    /// on both files.
    #[test]
    fn ares_dialog_background_takes_the_side_slots_only_once_loaded() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let stock = super::mode_two_dialog_labels(None);
        assert_eq!(stock[1], "pudlgbga.shp#0");
        for missing in [("NOSUCH.SHP", "DIALOG.PAL"), ("PUDLGBGA.SHP", "NOSUCH.PAL")] {
            assert!(super::render_side_dialog_art(&assets, missing).is_none());
        }
        let side = super::render_side_dialog_art(&assets, ("PUDLGBGA.SHP", "DIALOGY.PAL"))
            .expect("a stock SHP under another palette renders");
        let labels = super::mode_two_dialog_labels(Some(&side.label));
        assert_eq!(labels[0], stock[0]);
        assert_eq!(
            labels[1..],
            [side.label.clone(), side.label.clone(), side.label]
        );
        assert_ne!(labels[1], stock[1]);
    }

    /// The updater's art is Red Alert 2's own copy, not the one the ordinary
    /// lookup finds.
    #[test]
    fn ra2_dialog_art_is_red_alert_2s_own_copy() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let ra2 = super::render_ra2_dialog_art(&assets).expect("RA2's dialog art renders");
        let palette = load_named_palette(&assets, "DIALOGN.PAL").unwrap();
        let active = render_shp_entry(&assets, "PUDLGBGN.SHP", &palette, 0).unwrap();
        assert_eq!((ra2.width, ra2.height), (451, 326));
        assert_ne!(ra2.rgba, active.rgba);
    }

    #[test]
    fn setup_looks_for_the_mods_alternate_style_image_and_palette() {
        use crate::assets::asset_manager::test_support::{TestDirectory, audio_test_install};

        let install = audio_test_install("setup-art-install");
        let mod_dir = TestDirectory::new("setup-art-mod");
        let mut image = [0u8; 33];
        for offset in [2, 4, 6, 12, 14] {
            image[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        image[28..32].copy_from_slice(&32u32.to_le_bytes());
        image[32] = 1;
        let mut palette = [0u8; 768];
        palette[3] = 11;
        install.write_mix("language.mix", "MNSCRNL.SHP", &image);
        std::fs::write(mod_dir.path().join("SHELL.PAL"), [0u8; 768]).unwrap();
        mod_dir.write_mix("expandmd98.mix", COOP_SETUP_SHAPE, &image);
        mod_dir.write_mix("expandmd99.mix", COOP_SETUP_PALETTE, &palette);
        let assets = AssetManager::with_mod(
            install.path(),
            Some(mod_dir.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let art = super::render_mod_setup_background(&assets, false, false, "test").unwrap();
        assert_eq!(art.rgba, [44, 0, 0, 255]);
        assert!(super::render_mod_setup_background(&assets, false, true, "test").is_none());
    }

    #[test]
    fn mode_two_background_binding_matches_original_progress_paint() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_game_layout.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["progress_background"].as_array().unwrap() {
            let input = &row["input"];
            if !input["missing"].is_null() {
                continue;
            }
            let side = (input["session_byte30D8"].as_u64().unwrap() != 0).then(|| {
                match input["scenario_side34B8"].as_u64().unwrap() {
                    0 => super::SidebarTheme::Allied,
                    1 => super::SidebarTheme::Soviet,
                    2 => super::SidebarTheme::Yuri,
                    _ => panic!("unsupported fixture side"),
                }
            });
            let draw = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["event"] == "draw")
                .unwrap();
            let (shape, palette) =
                super::MODE_TWO_DIALOG_ART[super::mode_two_dialog_background_slot(side)];
            assert_eq!(shape, draw["shape"].as_str().unwrap());
            assert_eq!(palette, draw["palette"].as_str().unwrap());
            assert_eq!(
                classify_shell_asset(shape),
                ShellAssetRole::VerifiedModalDialogBackground
            );
            compared += 1;
        }
        assert_eq!(compared, 4);
    }

    fn pixel(entry: &ChromePiece, x: u32, y: u32) -> [u8; 4] {
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
        let mut assets = AssetManager::new(&config.paths.ra2_dir, MediaArchiveMode::STOCK_DIGITAL)
            .expect("asset manager");
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
    fn primitive_bevel_line_spans_include_both_endpoints() {
        let color = [1, 2, 3, 255];
        let mut rgba = vec![0u8; 5 * 3 * 4];

        draw_axis_line_inclusive_clipped(&mut rgba, 5, 3, (1, 1), (3, 1), color);
        let entry = ChromePiece {
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
        let entry = ChromePiece {
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

        assert_eq!(mixed, [0x87, 0x9C, 0xA2, 255]);
        assert_eq!(pixel(&entry, 5, 0), mixed);
        assert_eq!(pixel(&entry, 0, 5), mixed);
        assert_eq!(pixel(&entry, 4, 1), mixed);
        assert_eq!(pixel(&entry, 1, 4), mixed);
    }

    /// `0x006208F0` executed for both frame boxes of a plaque-less and a
    /// plaque trackbar (`tools/storage_oracle/shell_bevel.py`): replaying its
    /// lines and pixels in order gives exactly the frame canvas, in RGB565.
    #[test]
    fn trackbar_frames_match_the_executed_bevel() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/shell_bevel.json",
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let label = case["trackbar"].as_str().unwrap();
            let width = case["width"].as_u64().unwrap() as u32;
            let height = case["height"].as_u64().unwrap() as u32;
            let reserve = case["reserve"].as_i64().unwrap() as i32;
            let (canvas_w, canvas_h, rgba) = trackbar_frame_rgba(width, height, reserve);
            let border = TRACKBAR_FRAME_BORDER;
            let mut expected = vec![None; (canvas_w * canvas_h) as usize];
            let mut plot = |x: i64, y: i64, color: u64| {
                let (x, y) = (x + i64::from(border), y + i64::from(border));
                if (0..i64::from(canvas_w)).contains(&x) && (0..i64::from(canvas_h)).contains(&y) {
                    expected[(y as u32 * canvas_w + x as u32) as usize] = Some(color as u16);
                }
            };
            for bevel in case["boxes"].as_array().unwrap() {
                for op in bevel["ops"].as_array().unwrap() {
                    let op = op.as_array().unwrap();
                    let n = |i: usize| op[i].as_i64().unwrap();
                    match op[0].as_str().unwrap() {
                        "line" => {
                            let color = op[5].as_u64().unwrap();
                            let (x0, y0, x1, y1) = (n(1), n(2), n(3), n(4));
                            for y in y0.min(y1)..=y0.max(y1) {
                                for x in x0.min(x1)..=x0.max(x1) {
                                    plot(x, y, color);
                                }
                            }
                        }
                        _ => plot(n(1), n(2), op[3].as_u64().unwrap()),
                    }
                }
            }
            for (index, want) in expected.iter().enumerate() {
                let texel = &rgba[index * 4..index * 4 + 4];
                let got = (texel[3] != 0).then(|| {
                    (u16::from(texel[0] >> 3) << 11)
                        | (u16::from(texel[1] >> 2) << 5)
                        | u16::from(texel[2] >> 3)
                });
                let (x, y) = (index as u32 % canvas_w, index as u32 / canvas_w);
                assert_eq!(got, *want, "{label}: canvas pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn trackbar_frame_entry_contains_both_native_primitive_boxes() {
        let entry = render_trackbar_frame_geometry("trackbar", 128, 21, 50);
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

    /// A trackbar drawn below the shell's size takes the nearest window's
    /// frame; at the shell's own size only its exact window has one.
    #[test]
    fn trackbar_below_the_shell_size_takes_the_nearest_frame() {
        let half = crate::ui::shell::geom::shell_scale(400, 300);
        let slot = |plaque, w, h, scale| {
            drawn_trackbar_frame_slot(plaque, w, h, scale).map(|slot| TRACKBAR_FRAMES[slot])
        };
        assert_eq!(slot(true, 130, 22, half), Some((129, 22, 50)));
        assert_eq!(slot(true, 129, 21, half), Some((129, 21, 50)));
        assert_eq!(slot(false, 130, 22, half), None);
        assert_eq!(slot(true, 132, 22, half), None);
        assert_eq!(
            slot(true, 130, 22, crate::ui::shell::geom::RETAIL_SCALE),
            None
        );
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

    /// A distinct width per label, so the selected slot is readable.
    fn background_labeled(width: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [width, 1.0],
        }
    }

    #[test]
    fn yr_style_skirmish_background_selects_the_coop_setup_pair() {
        let mut by_label = HashMap::new();
        by_label.insert("mnscrns.shp:shell".to_string(), background_labeled(1.0));
        by_label.insert("mnscrnl.shp:shell".to_string(), background_labeled(2.0));
        by_label.insert(
            "mnscrns.shp:offline-setup".to_string(),
            background_labeled(3.0),
        );
        by_label.insert(
            "mnscrnlcoopgamesetup.shp".to_string(),
            background_labeled(4.0),
        );
        let widths = |backgrounds: [Option<SkirmishShellChromeEntry>; 2]| {
            backgrounds.map(|entry| entry.map(|entry| entry.pixel_size[0]))
        };

        assert_eq!(
            widths(setup_background_pair(
                false,
                |label| by_label.get(label).copied(),
                |_| None
            )),
            [Some(1.0), Some(2.0)]
        );
        assert_eq!(
            widths(setup_background_pair(
                true,
                |label| by_label.get(label).copied(),
                |_| None
            )),
            [Some(3.0), Some(4.0)]
        );
    }

    #[test]
    fn yr_style_skirmish_background_falls_back_to_ra2_per_slot() {
        let mut by_label = HashMap::new();
        by_label.insert("mnscrns.shp:shell".to_string(), background_labeled(1.0));
        by_label.insert("mnscrnl.shp:shell".to_string(), background_labeled(2.0));
        by_label.insert(
            "mnscrns.shp:offline-setup".to_string(),
            background_labeled(3.0),
        );

        let selected = setup_background_pair(true, |label| by_label.get(label).copied(), |_| None);
        assert_eq!(
            selected.map(|entry| entry.map(|entry| entry.pixel_size[0])),
            [Some(3.0), Some(2.0)]
        );
    }

    #[test]
    #[ignore = "requires the configured stock retail RA2/YR install"]
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
    #[ignore = "requires the configured stock retail RA2/YR install"]
    fn retail_generic_shell_backgrounds_decode_with_shell_palette() {
        let assets = retail_assets();
        let palette = load_named_palette(&assets, "SHELL.PAL").expect("SHELL.PAL");
        let small = render_shp_entry(&assets, "MNSCRNS.SHP", &palette, 0).expect("MNSCRNS frame 0");
        let large = render_shp_entry(&assets, "MNSCRNL.SHP", &palette, 0).expect("MNSCRNL frame 0");

        assert_eq!((small.width, small.height), (472, 448));
        assert_eq!((large.width, large.height), (632, 568));
    }

    #[test]
    #[ignore = "requires the configured stock retail RA2/YR install"]
    fn retail_coop_setup_background_decodes_through_its_palette() {
        let assets = retail_assets();
        let palette = load_named_palette(&assets, COOP_SETUP_PALETTE).expect(COOP_SETUP_PALETTE);
        let background =
            render_shp_entry(&assets, COOP_SETUP_SHAPE, &palette, 0).expect(COOP_SETUP_SHAPE);

        // Same content-area canvas as MNSCRNL: the 800-wide setup art.
        assert_eq!((background.width, background.height), (632, 568));
    }

    #[test]
    #[ignore = "requires the configured stock retail RA2/YR install"]
    fn retail_lan_player_glyphs_match_their_pcx_headers() {
        let assets = retail_assets();
        for (name, width) in [
            ("wolnob.pcx", 14),
            ("wolob.pcx", 14),
            ("wolhost.pcx", 15),
            ("wolacpt.pcx", 15),
        ] {
            let icon = pcx_piece(&assets, name, true).expect(name);
            // Dimensions from original retail PCX xmin/ymin/xmax/ymax bytes.
            assert_eq!((icon.width, icon.height), (width, 14), "{name}");
        }
    }
}
