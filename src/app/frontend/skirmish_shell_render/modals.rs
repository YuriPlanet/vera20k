//! Modal sprite helpers for the skirmish shell renderer.
//!
//! Covers Choose Map and validation modal instance construction.

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::LOWER_STRIP_H;
use crate::ui::skirmish_shell::{ChooseMapModalLayout, RectPx, SkirmishShellLayout};

use super::chrome::{
    common_shell_origin, push_entry_grown, push_lower_strip_instance, push_rect_outline,
    push_right_panel_base_instances, push_solid_rect,
};
use super::draw_order::{GenericBackgroundRole, generic_background_role};
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68, SHELL_DROPDOWN_DEPTH, SHELL_MODAL_BG_RGB,
    SHELL_PARENT_BACKGROUND_DEPTH,
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
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use super::in_game_shell::{
        InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
    };
    use crate::ui::shell::modal::{menu_area, message_box_parts};

    // An in-match load paints over the match frame at its source size and
    // zooms with it; a menu load paints over the menu at the window's size.
    let frame = (
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
    );
    let restored = renderer
        .shell_surface_presenter
        .restore_cursor_free_composition(
            &renderer.retail_screenshot_frame_cache,
            &renderer.gpu.device,
            encoder,
            frame,
            renderer.upscale_pass.as_ref(),
        );
    let art = match restored {
        Ok(true) => ShellFrameArt::Retained,
        Ok(false) => ShellFrameArt::Clear,
        Err(error) => {
            log::warn!("Saved-game progress has no compatible retained backing: {error:#}");
            ShellFrameArt::Clear
        }
    };
    let atlas = frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("loading shell chrome");
    // 559D60 shows F0 before 69BB40 clears Session+30D8. An active load thus
    // starts with its current side; a menu load starts with the neutral assets.
    let in_match = frontend.screen == crate::ui::game_screen::GameScreen::InGame;
    let side = in_match.then(|| {
        crate::app::presentation::sidebar_render::current_sidebar_theme(frontend, match_state)
    });
    let (width, height) = (frame.0 as i32, frame.1 as i32);
    // F0 is CE's box: its body over a Cancel it keeps hidden.
    let area = match in_match {
        true => RectPx::new(0, 0, width, height),
        false => menu_area(width, height),
    };
    let layout = message_box_parts(1, area);
    let body = process_assets.csf_label("TXT_LOADING", "Mission is loading. Please wait...");
    let shell_box = crate::ui::shell::modal::ShellBox::new(
        crate::ui::shell::descriptor::DialogResourceId(0xF0),
        body,
    );
    // Its widgets, which only this paint keeps.
    let ui = &mut frontend.loading_box;
    if ui.root_in(shell_box.dialog).is_none() {
        ui.push(shell_box.dialog);
    }
    crate::ui::shell::menu_page::sync_box(ui, &shell_box, &layout);
    // A zoomed match's text is drawn in the window (`window_ui`).
    let raster = crate::app::presentation::window_ui::shell_raster(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
        false,
    )
    .unwrap_or(crate::render::bit_font::TextRaster::NATIVE);
    let frame_raster = renderer.bit_font.set_raster(raster);
    let look = super::BoxLook::new(
        atlas.mode_two_dialog_background(side),
        VALIDATION_MODAL_SPRITE_DEPTHS,
    );
    let tree = super::controller_tree(&frontend.loading_box, shell_box.dialog);
    let modal = super::paint_shell_box(
        &platform.ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        Some(atlas),
        tree,
        &shell_box,
        &layout,
        |_| false,
        look,
    );
    let painted = super::in_game_shell::render_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
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
    renderer.bit_font.set_raster(frame_raster);
    painted
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
) {
    if let Some(background) = choose_map_background_entry(atlas, modal_layout) {
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
}

/// Choose Map's own art beside its widgets ([`super::text::choose_map_paint`]):
/// while no slide runs, the preview static's outline (validate-only while
/// one does, `0x00606800`).
pub(super) fn push_choose_map_modal_control_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &ChooseMapModalLayout,
    steady: bool,
) {
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
) {
    if let Some(background) = generic_shell_background_entry(atlas, layout) {
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
}
