//! Modal sprite helpers for the skirmish shell renderer.
//!
//! Covers Choose Map and validation modal instance construction.

use crate::render::batch::SpriteInstance;
use crate::render::shell_paint;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::LOWER_STRIP_H;
use crate::ui::skirmish_shell::{
    ChooseMapModalLayout, RandomMapSetupLayout, RandomMapSetupModalState, RectPx,
    SkirmishShellLayout,
};

use super::chrome::{
    common_shell_origin, push_entry_grown, push_lower_strip_instance, push_rect_outline,
    push_right_panel_base_instances, push_solid_rect,
};
use super::controls::{ControlPaint, paint_control};
use super::draw_order::{GenericBackgroundRole, generic_background_role};
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68, SHELL_DROPDOWN_DEPTH, SHELL_LABEL_TEXT_RGB,
    SHELL_MODAL_BG_RGB, SHELL_MODAL_PANEL_RGB, SHELL_PARENT_BACKGROUND_DEPTH,
};

const VALIDATION_MODAL_SPRITE_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: SHELL_DROPDOWN_DEPTH - 0.00014,
    button: SHELL_DROPDOWN_DEPTH - 0.00016,
    text: 0.0,
};

/// Ordinary saved-load F0: 559D60 -> 623230, mode-2 paint 621E90. The original
/// copies its existing AlternateSurface backing, overlays the selected side's
/// PUDLGBG frame, and paints the plain left/top Static. It neither regenerates a
/// parent shell nor draws Cancel. Executed controls: saved_game_layout.json.
pub(crate) fn render_saved_game_loading(
    state: &mut crate::app::AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use super::in_game_shell::{
        InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
    };
    use crate::ui::shell::modal::{menu_area, message_box_parts};

    // An in-match load paints over the match frame at its source size and
    // zooms with it; a menu load paints over the menu at the window's size.
    let frame = (state.render_width(), state.render_height());
    let restored = state
        .renderer
        .shell_surface_presenter
        .restore_cursor_free_composition(
            &state.renderer.retail_screenshot_frame_cache,
            &state.renderer.gpu.device,
            encoder,
            frame,
            state.renderer.upscale_pass.as_ref(),
        );
    let art = match restored {
        Ok(true) => ShellFrameArt::Retained,
        Ok(false) => ShellFrameArt::Clear,
        Err(error) => {
            log::warn!("Saved-game progress has no compatible retained backing: {error:#}");
            ShellFrameArt::Clear
        }
    };
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("loading shell chrome");
    // 559D60 shows F0 before 69BB40 clears Session+30D8. An active load thus
    // starts with its current side; a menu load starts with the neutral assets.
    let in_match = state.frontend.screen == crate::ui::game_screen::GameScreen::InGame;
    let side =
        in_match.then(|| crate::app::presentation::sidebar_render::current_sidebar_theme(state));
    let (width, height) = (frame.0 as i32, frame.1 as i32);
    // F0 is CE's box: its body over a Cancel it keeps hidden.
    let area = match in_match {
        true => RectPx::new(0, 0, width, height),
        false => menu_area(width, height),
    };
    let layout = message_box_parts(1, area);
    let labels = [shell_paint::PaintLabel {
        text: state
            .csf_label("TXT_LOADING", "Mission is loading. Please wait...")
            .into(),
        rect: layout.body,
        rgb: SHELL_LABEL_TEXT_RGB,
        align: crate::ui::shell::geom::ShellAlign::NONE,
        path_a_reveal: None,
    }];
    // A zoomed match's text is drawn in the window (`window_ui`).
    let raster = crate::app::presentation::window_ui::shell_raster(state, false)
        .unwrap_or(crate::render::bit_font::TextRaster::NATIVE);
    let frame_raster = state.renderer.bit_font.set_raster(raster);
    let modal = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(side),
        shell_paint::ModalButtonFrames::default(),
        layout.dialog,
        &[],
        &labels,
        crate::ui::shell::geom::RETAIL_SCALE,
        VALIDATION_MODAL_SPRITE_DEPTHS,
    );
    let painted = super::in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art: Vec::new(),
            controls: modal.sprites,
            texts: modal.text,
            label: "Saved Game Loading F0",
        },
        art,
        ShellFrameOverlay::default(),
        ShellFrameCursor::Hidden,
    );
    state.renderer.bit_font.set_raster(frame_raster);
    painted
}

/// Native owner-draw listboxes preserve a composed backing surface. On the
/// exact-800 chooser that backing is the Customize Battle artwork; the solid
/// interior exists only to keep the asset-missing fallback readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackdropInteriorPaint {
    PreserveBacking,
    OpaqueFallback,
}

impl BackdropInteriorPaint {
    const fn paints_solid_fill(self) -> bool {
        matches!(self, Self::OpaqueFallback)
    }
}

const fn backdrop_interior(retail_background_available: bool) -> BackdropInteriorPaint {
    if retail_background_available {
        BackdropInteriorPaint::PreserveBacking
    } else {
        BackdropInteriorPaint::OpaqueFallback
    }
}

pub(super) fn choose_map_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &ChooseMapModalLayout,
) -> Option<SkirmishShellChromeEntry> {
    // `0x0072E730` draws the small shape at 640 wide and the large one
    // (loaded only at exactly 800) elsewhere. A stretched chooser is authored at
    // 800, so it takes the large shape instead of falling back to a flat rect.
    match layout.authored_width() {
        640 => atlas.choose_map_background_640_customize_battle,
        800 => atlas.choose_map_background_800_customize_battle,
        _ => None,
    }
}

pub(super) fn shell_content_fallback_rect(layout: &SkirmishShellLayout) -> RectPx {
    let (origin_x, origin_y) = layout.shell_origin();
    let strip_h = (LOWER_STRIP_H as f32 * layout.scale.factor()).round() as i32;
    RectPx::new(
        origin_x,
        origin_y,
        (layout.right_panel.top.x - origin_x).max(0),
        (layout.shell_height() - strip_h).max(0),
    )
}

/// VERA: how bright Choose Map draws its background art, so list text
/// reads over it. gamemd draws it at full brightness, as the saved-seed
/// browser on the same art still does.
pub(super) const CHOOSE_MAP_BACKGROUND_BRIGHTNESS: f32 = 0.8;

pub(super) fn push_choose_map_background_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    shell_layout: &SkirmishShellLayout,
    modal_layout: &ChooseMapModalLayout,
    brightness: f32,
) -> BackdropInteriorPaint {
    let background = choose_map_background_entry(atlas, modal_layout);
    let interior = backdrop_interior(background.is_some());
    if let Some(background) = background {
        let (x, y) = modal_layout
            .scale
            .dialog_origin_or((modal_layout.screen.x, modal_layout.screen.y));
        push_entry_grown(
            out,
            background,
            x,
            y,
            modal_layout.scale,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
        if let Some(art) = out.last_mut() {
            art.tint = [brightness; 3];
        }
    } else {
        let fallback = shell_content_fallback_rect(shell_layout);
        push_solid_rect(
            out,
            atlas,
            fallback,
            SHELL_MODAL_BG_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00008,
        );
        push_rect_outline(
            out,
            atlas,
            fallback,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00009,
        );
    }
    interior
}

/// Choose Map's own art beside its widgets ([`super::text::choose_map_paint`]).
pub(super) fn push_choose_map_modal_control_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    font: &crate::render::bit_font::BitFont,
    layout: &ChooseMapModalLayout,
    interior: BackdropInteriorPaint,
    search_display: &str,
    steady: bool,
) {
    // VERA: the search box wears the list's frame, with a caret after its
    // text. It has no focus to lose, so the caret does not blink.
    if interior.paints_solid_fill() {
        push_solid_rect(
            out,
            atlas,
            layout.map_search,
            SHELL_MODAL_PANEL_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00010,
        );
    }
    super::list::paint_list_frame(out, atlas, layout.map_search);
    if let Some(caret) =
        super::text::map_search_caret_rect(font, search_display, layout.map_search, layout.scale)
    {
        push_solid_rect(
            out,
            atlas,
            caret,
            SHELL_LABEL_TEXT_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00012,
        );
    }
    // While a slide runs the preview static is validate-only (`0x00606800`).
    if steady {
        push_rect_outline(
            out,
            atlas,
            layout.preview,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00012,
        );
    }
}

/// The empty backdrop `ShellMessageBox__Run` draws before its box
/// (`0x005D3514` -> `Shell__DrawEmptyBackdrop` `0x0052FEC0` ->
/// `0x0072E820`): the generic shell background and `RightPanel__Draw(0)`
/// (`0x0072E450`: top cap, tiles with every slot shuttered by SDBTNANM frame
/// 10, bottom cap, lower strip). The dialog behind does not show.
pub(super) fn push_message_box_backdrop_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) {
    push_generic_shell_background_instances(out, atlas, layout);
    push_right_panel_base_instances(out, atlas, layout, true);
    push_lower_strip_instance(out, atlas, layout);
}

/// VERA's download boxes show how much has arrived: an outlined bar under
/// the box's text, filled in the label colour.
pub(super) fn push_download_progress_bar(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    body: RectPx,
    (completed, total): (u64, u64),
    depth: f32,
) {
    let track = RectPx::new(body.x, body.y + body.h + 6, body.w, 10);
    push_rect_outline(out, atlas, track, SHELL_LABEL_TEXT_RGB, depth);
    let inner = track.w - 4;
    let filled = (inner as u64 * completed.min(total) / total.max(1)) as i32;
    if filled > 0 {
        push_solid_rect(
            out,
            atlas,
            RectPx::new(track.x + 2, track.y + 2, filled, track.h - 4),
            SHELL_LABEL_TEXT_RGB,
            depth,
        );
    }
}

/// The generic shell background (MNSCRNS/MNSCRNL through SHELL.PAL): the
/// random-map dialog `0x105`'s and the empty backdrop's
/// (`0x0072AB49..0x0072AB7E`), also selected for the RA2 skirmish skin.
pub(super) fn generic_shell_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    match generic_background_role(layout) {
        GenericBackgroundRole::Mnscrns640 => atlas.generic_background_640_mnscrns_shell,
        GenericBackgroundRole::MnscrnlLarge => atlas.generic_background_large_mnscrnl_shell,
    }
}

/// The offline Skirmish `0x102` backdrop selected by the atlas's shell style:
/// YR's Cooperative setup art, or the shared RA2 MNSCRN pair.
pub(super) fn offline_setup_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    let (index, pane) = match generic_background_role(layout) {
        GenericBackgroundRole::Mnscrns640 => (0, [472.0, 448.0]),
        GenericBackgroundRole::MnscrnlLarge => (1, [632.0, 568.0]),
    };
    atlas
        .client_skirmish_background
        .map(|mut background| {
            // Fill the dialog's existing picture area. Center-crop the wide
            // client artwork so the backdrop keeps its proportions.
            let [width, height] = background.pixel_size;
            let ratio = (pane[0] / width).max(pane[1] / height);
            for (axis, shown) in [pane[0] / (width * ratio), pane[1] / (height * ratio)]
                .into_iter()
                .enumerate()
            {
                background.uv_origin[axis] += background.uv_size[axis] * (1.0 - shown) / 2.0;
                background.uv_size[axis] *= shown;
            }
            background.pixel_size = pane;
            background
        })
        .or(atlas.setup_backgrounds[index])
}

pub(super) fn push_generic_shell_background_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> BackdropInteriorPaint {
    let background = generic_shell_background_entry(atlas, layout);
    let interior = backdrop_interior(background.is_some());
    if let Some(background) = background {
        let (x, y) = common_shell_origin(layout);
        push_entry_grown(
            out,
            background,
            x,
            y,
            layout.scale,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
    } else {
        let fallback = shell_content_fallback_rect(layout);
        push_solid_rect(
            out,
            atlas,
            fallback,
            SHELL_MODAL_BG_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00008,
        );
        push_rect_outline(
            out,
            atlas,
            fallback,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00009,
        );
    }
    interior
}

/// The random-map dialog `0x105`'s own art beside its widgets
/// ([`super::text::random_map_paint`]): VERA's Advanced scrollbar, Ares'
/// Urban Areas box and the progress bar while a map generates.
pub(super) fn push_random_map_setup_modal_control_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &RandomMapSetupLayout,
    modal: &RandomMapSetupModalState,
    // The Advanced scrollbar's widget.
    scrollbar: Option<vera20k_gui::ScrollBarView>,
) {
    use crate::ui::skirmish_shell::AdvancedField;
    let chrome = atlas.control_chrome();
    // The native vertical scrollbar the dropdowns use, paging the seven
    // reusable rows over every Advanced slider.
    if let Some(bar) = scrollbar
        .filter(|_| modal.advanced && modal.extras_page.is_none() && AdvancedField::PAGE_COUNT > 1)
    {
        paint_control(
            out,
            &chrome,
            ControlPaint::ScrollBar {
                scrollbar: bar.bar,
                thumb: bar.thumb,
                pressed_part: None,
            },
            layout.scale,
        );
    }
    // Ares' Urban Areas box (control 5000 of Ares' dialog 261), drawn as the
    // shell's own checkbox when Ares.dll is loaded; the Advanced view's
    // seventh slider takes its place.
    if modal.ares && !modal.advanced {
        paint_control(
            out,
            &chrome,
            ControlPaint::Checkbox {
                checked: modal.options.urban_areas,
                rect: layout.rect(5000),
            },
            layout.scale,
        );
    }
    // The preview `0x468` is an SS_BLACKRECT static in the top panel: black,
    // with no frame of its own (retail `rmg-steady.png`). The progress
    // widgets are hidden in the resource and shown only while the
    // synchronous generate block owns the dialog.
    if modal.generating {
        push_solid_rect(
            out,
            atlas,
            layout.rect(RandomMapSetupLayout::PROGRESS_BAR),
            SHELL_MODAL_PANEL_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00013,
        );
        push_rect_outline(
            out,
            atlas,
            layout.rect(RandomMapSetupLayout::PROGRESS_BAR),
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00014,
        );
    }
}
