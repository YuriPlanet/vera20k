//! Retail launcher Options (D5): physical shell paint over the retained model.
//! Active owner55FC80, resourceD5, common static6153E0 and trackbar61D950.
//! At the main menu D5 is a family page: its heading, status line and
//! monitor are the shared family statics, and its column slides in and out
//! (`0x006071E0`). Every control paints from the dialog's tree.

use super::gui_paint::{ButtonArt, GuiStyle, SideButtons, paint_gui_styled};
use super::{chrome::*, *};
use crate::app::frontend::menu_page_render::{PanelStatic, paint_panel_statics};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::ui::main_menu_dialogs::options::OptionsDialogState;
use crate::ui::main_menu_dialogs::options::shell::{Control, LauncherOptionsLayout};
use crate::ui::shell::geom::{RectPx, ShellAlign, ShellScale};
use crate::util::time::Instant;

/// How the page's right-hand column buttons paint ([`paint_page`]).
#[derive(Clone, Copy)]
pub(crate) enum ColumnArt {
    /// Not at all: a slide's column covers them, or the box has none.
    Hidden,
    /// On the launcher panel's art, the page that shows lit (612B70
    /// record+C5, SDBTNANM 3).
    Panel,
    /// The in-game menu's own buttons on the side's type-2 art
    /// ([`ButtonArt::Side`]), the page that shows lit.
    Side(SideButtons),
}

/// The page's controls and its open list, painted from the dialog's tree
/// (synced to its layout by the caller). The page's own buttons are drawn as
/// Sound Options' Play and Stop are (609FC7..609FE2/60A330): owner-draw type
/// 3, MNBTTN centred on the control; the right-hand column's in `column`.
/// Captions under `covered` are left out: a box draws all its art before its
/// text, so that text would otherwise show through the list.
pub(crate) fn paint_page(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    dialog: &OptionsDialogState,
    scale: ShellScale,
    column: ColumnArt,
    covered: &[RectPx],
) -> HudPaint {
    let (gui, panel) = dialog.shell_gui();
    let art = |id| match dialog.shell_control(id)? {
        Control::Button(button) if button.on_page() => Some(ButtonArt::ModalSqueezed),
        Control::Button(_) => match column {
            ColumnArt::Hidden => None,
            ColumnArt::Panel => Some(ButtonArt::Panel),
            ColumnArt::Side(_) => Some(ButtonArt::Side),
        },
        _ => None,
    };
    let style = GuiStyle {
        art_of: Some(&art),
        side: match column {
            ColumnArt::Side(side) => Some(side),
            _ => None,
        },
        ..Default::default()
    };
    if frontend.skirmish_shell_chrome.is_some() {
        return paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            panel,
            scale,
            covered,
            style,
        );
    }
    // Without the control art only the side's column paints: its art is
    // the sidebar's.
    let mut paint = HudPaint::default();
    if let ColumnArt::Side(_) = column {
        let side = gui
            .tree()
            .children(panel)
            .unwrap_or_default()
            .iter()
            .copied();
        for node in side.filter(|node| {
            gui.id_of(*node)
                .and_then(|id| art(id))
                .is_some_and(|art| art == ButtonArt::Side)
        }) {
            super::gui_paint::paint_gui_into(
                &mut paint,
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                None,
                gui,
                node,
                scale,
                covered,
                style,
            );
        }
    }
    paint
}

/// Options' controls on another box's art: VERA's Esc menu shows a few of
/// them. They paint in submission order, after the box and its buttons.
pub(crate) fn paint_options_settings(
    draw: &mut crate::render::shell_paint::ModalDraw,
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    layout: &LauncherOptionsLayout,
    settings: &OptionsDialogState,
) {
    let list = settings.shell_popup_rect();
    let page = paint_page(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        settings,
        layout.scale,
        ColumnArt::Hidden,
        list.as_slice(),
    );
    draw.sprites.extend(page.lane(Material::Controls).copied());
    draw.sprites.extend(page.lane(Material::Popup).copied());
    draw.text.extend(page.texts);
    draw.text.extend(page.popup_texts);
}

/// Shared mode1 launcher frame,60CF00 default background and621E90 composition.
/// Resource-specific children (such as D5 warning71C) remain with their owner.
pub(super) fn launcher_background_instances(
    atlas: &SkirmishShellChromeAtlas,
    width: u32,
    height: u32,
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let common = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    let mut out = Vec::new();
    if let Some(background) = super::modals::generic_shell_background_entry(atlas, &common) {
        if scale.is_unscaled() {
            push_entry_native(&mut out, background, 0, 0, SHELL_PARENT_BACKGROUND_DEPTH);
        } else {
            // A scaled page grows the MNSCRN canvas with its dialog, as the
            // family menu pages do.
            let dialog = scale.dialog_rect();
            push_entry_grown(
                &mut out,
                background,
                dialog.x,
                dialog.y,
                scale,
                SHELL_PARENT_BACKGROUND_DEPTH,
            );
        }
    }
    push_right_panel_base_instances(&mut out, atlas, &common, false);
    push_lower_strip_instance(&mut out, atlas, &common);
    out
}

pub(crate) fn render_launcher_options(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let now = Instant::now();
    let category = frontend
        .options_dialog
        .as_ref()
        .expect("launcher active")
        .category();
    let layout = crate::app::frontend::shell_layout::launcher_options_layout(
        platform,
        renderer,
        &frontend.screen,
        &persistence.options_profile,
        category,
    );
    if let Some(dialog) = frontend.options_dialog.as_mut() {
        dialog.shell_sync(&layout);
    }
    let scale = layout.scale;
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`) and
    // pumps no messages until it ends: every child stays blank. The entry
    // slide suppresses the heading, status line, monitor and column buttons
    // (`0x00606800`); the left-side controls paint.
    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        &frontend.shell_exit,
        ShellSlideKind::Options,
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    // Like the sibling family pages, the monitor shows its current frame
    // with no timer while the entry slide runs.
    let frames = frontend
        .skirmish_shell_chrome
        .as_ref()
        .map_or(0, |atlas| atlas.launcher_warning_frames.len());
    let monitor_frame = if leaving {
        None
    } else {
        frontend.shell_monitor.paint(now, frames, !sliding)
    };
    let (title, status_label) = if sliding {
        (None, None)
    } else {
        let title = frontend.shell_page_title.paint(now);
        let status_text = frontend
            .options_dialog
            .as_ref()
            .and_then(|dialog| dialog.shell_status_help())
            .map(|key| process_assets.csf_text(key))
            .unwrap_or_default();
        let status = crate::app::frontend::menu_page_render::paint_shell_status_line(
            &mut frontend.shell_status_line,
            status_text,
            layout.status_help,
        );
        (title, status)
    };
    let dialog = frontend.options_dialog.as_ref().expect("launcher active");
    let atlas = frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("launcher atlas");
    let mut instances = launcher_background_instances(
        atlas,
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
        scale,
    );
    if let Some(frame) = monitor_frame.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_clipped_centered(
            &mut instances,
            *frame,
            layout.warning,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    let mut texts = Vec::new();
    if let Some(wave) = wave.as_ref() {
        let common = crate::ui::skirmish_shell::compute_layout_on(
            scale,
            renderer.render_width(&frontend.screen),
            renderer.render_height(&frontend.screen),
        );
        push_slide_column(
            &mut instances,
            atlas,
            &common,
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    }
    // The heading and the status line from the panel's widgets in one
    // paint, the heading before the page's text and the status line after
    // it (each shown `Text` gives one draw).
    let heading = title.map(|window| {
        let align = ShellAlign::H_CENTER | ShellAlign::PANEL_FACE;
        PanelStatic::title(dialog.shell_title_text(), layout.title, align, window)
    });
    let heading_draws = usize::from(heading.as_ref().is_some_and(|h| !h.text.is_empty()));
    let status = status_label.map(PanelStatic::status);
    let mut statics = paint_panel_statics(
        &platform.ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        [heading, status],
        scale,
    );
    let status_texts = statics.split_off(heading_draws.min(statics.len()));
    texts.extend(statics);
    let column = if sliding {
        ColumnArt::Hidden
    } else {
        ColumnArt::Panel
    };
    let page = (!leaving).then(|| {
        paint_page(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            dialog,
            scale,
            column,
            &[],
        )
    });
    let mut page = page.unwrap_or_default();
    instances.extend(page.lane(Material::Controls).copied());
    let popup = page.take_lane(Material::Popup);
    texts.extend(page.texts);
    texts.extend(status_texts);
    super::in_game_shell::render_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        super::in_game_shell::InGameShellFrame {
            art: instances,
            controls: Vec::new(),
            texts,
            label: "Launcher Options D5",
        },
        super::in_game_shell::ShellFrameArt::Launcher,
        super::in_game_shell::ShellFrameOverlay {
            controls: popup,
            texts: page.popup_texts,
            ..Default::default()
        },
        super::in_game_shell::ShellFrameCursor::Visible,
    )?;
    platform.window.request_redraw();
    Ok(())
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use crate::ui::main_menu_dialogs::options::{Zoom, zoom_label};

    #[test]
    fn zoom_rows_name_the_battle_size() {
        let hd = (1920, 1080);
        let rows: Vec<_> = Zoom::choices(hd.0, hd.1)
            .into_iter()
            .map(|zoom| zoom_label(zoom, hd, false))
            .collect();
        assert_eq!(
            rows,
            [
                "Auto (1920x1080)",
                "100% (1920x1080)",
                "150% (1280x720)",
                "200% (960x540)"
            ]
        );
        // The original 4:3 shape trims the width.
        assert_eq!(zoom_label(Zoom::Percent(150), hd, true), "150% (960x720)");
    }
}
