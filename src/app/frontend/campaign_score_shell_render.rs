//! Render glue for campaign victory dialog `0x103` and the LAN Cooperative
//! campaign end `0xBC7`.
//!
//! Native `GameExit::Victory` opens `0x103` through `0x6C8E80`, and
//! Cooperative `+0xA4` opens `0xBC7` through `0x00471420`; this renderer
//! consumes the open dialog's retail controls and texts from
//! `ui::campaign_score_shell`. It deliberately does not borrow the Skirmish
//! score table (`0x108`) or its side-art owner.

use crate::util::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, PanelState, encode_family_page, family_art_ready, page_tree,
};
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::render::shell_paint::{
    self, DIALOG_BACKGROUND_DEPTH, PARENT_BACKGROUND_DEPTH, PaintLabel,
};
use crate::ui::campaign_score_shell::{
    CampaignScoreDialog, SCORE_ART_REGION_STATIC, STATUS_HELP_STATIC,
};
use crate::ui::shell::menu_page::hovered_id;

/// Paint campaign dialog `0x103` or `0xBC7`. It needs the shared shell chrome,
/// but no Skirmish score art: each template has its own static controls.
pub(crate) fn render_campaign_score_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.campaign_score_page.is_none() || !family_art_ready(state) {
        return Ok(false);
    }

    let (width, height) = crate::app::frontend::shell_layout::shell_screen_size(state);
    let fills_window = crate::app::frontend::shell_layout::fills_window(state);
    let now = Instant::now();
    // `0x006071E0` owns both slides; `SetSlideChildSuppressProc` hides the
    // dialog's statics (including the SCRA kind-4 static) until SHOW completes,
    // and the teardown repaint covers them while the column slides out.
    let dialog = state
        .frontend
        .campaign_score_page
        .as_ref()
        .expect("checked before render")
        .dialog;
    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::campaign_score(dialog),
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let children = wave.is_none();
    // `0x006071E0` composites SCRT itself during a slide: frames 5..0 on SHOW,
    // 0..5 on CLOSE, then SCRBK at the dialog origin. Steady WM_PAINT draws
    // RadarBackground's frame 0.
    let scrt_frame = match wave.as_ref() {
        None => Some(0),
        Some(wave) => wave.panel_art().iter().find_map(|art| match art {
            crate::ui::shell::slide::PanelArt::Scrt(frame) => Some(*frame),
            _ => None,
        }),
    };
    let animation_frames = state
        .frontend
        .campaign_score_art
        .as_ref()
        .and_then(|art| art.scra.as_ref())
        .map_or(0, |strip| strip.frames.len());
    let (layout, emblem_frame) = {
        let page = state
            .frontend
            .campaign_score_page
            .as_mut()
            .expect("checked before render");
        // Production always arms a SHOW wave; if one cannot be created (or a
        // future route reuses the page without a new target edge), show the
        // page instead of leaving every static hidden forever.
        if wave.is_none() && !leaving && !page.presentation_started() {
            page.start_presentation(animation_frames, now);
        }
        let frame = if leaving {
            None
        } else if wave.is_some() {
            page.emblem_animation_frame()
        } else {
            page.poll_emblem_animation(now)
        };
        (page.layout(width, height, fills_window), frame)
    };
    let rect_of = |id| layout.control_rect(id);
    let tree = page_tree(state, dialog.id(), || dialog.nodes(), rect_of, &[]);
    // `0xBC7`'s monitor `0x71C` runs the shared kind-4 timer.
    let monitor_frame = (children && dialog == CampaignScoreDialog::CampaignEnd)
        .then(|| crate::app::frontend::menu_page_render::paint_shell_monitor(state))
        .flatten();
    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(dialog.id());
    let hovered = (children && active)
        .then(|| hovered_id(controller, dialog.id()))
        .flatten();
    let status = hovered
        .and_then(|id| dialog.control(id))
        .and_then(|control| control.tooltip_key)
        .map(|key| state.csf_text(key))
        .unwrap_or_default();

    let mut labels: Vec<PaintLabel<'static>> = Vec::new();
    let campaign_page = state
        .frontend
        .campaign_score_page
        .as_mut()
        .expect("checked before render");
    if children {
        let statics = &mut campaign_page.statics;
        statics.set_text(STATUS_HELP_STATIC, &status, now);
        let rect_of = |id| layout.control_rect(id);
        crate::app::frontend::score_shell_render::score_static_labels(
            statics,
            now,
            rect_of,
            &mut labels,
        );
        // `0xBC7`'s score cells paint whole in their row colour.
        for cell in campaign_page.cells() {
            let Some(rect) = layout.control_rect(cell.id) else {
                continue;
            };
            labels.push(PaintLabel {
                text: cell.text.clone().into(),
                rect,
                align: cell.align,
                rgb: cell.rgb.map(|channel| f32::from(channel) / 255.0),
                path_a_reveal: None,
            });
        }
    }

    // The family compositor paints the right panel, the monitor, the column
    // or Continue with its caption; the heading and status line are this
    // dialog's own statics.
    let panel = PanelState {
        leaving,
        // The slide engine paints the whole right-panel column; the steady
        // Continue cell is not drawn until the wave ends.
        column: wave.as_ref().map(|wave| wave.button_draws()),
        monitor: monitor_frame,
        ..PanelState::default()
    };
    let title = dialog.page().title_key;
    let mut frame = FamilyFrame::new("Campaign Score Shell", &layout.page, panel, title).on(
        state,
        dialog.id(),
        &tree,
    );
    frame.labels = labels;
    if let Some(art) = state.frontend.campaign_score_art.as_ref() {
        let background = layout.background;
        let mut background_sprites = Vec::with_capacity(1);
        // Paint mode 1's `Background_Overlay` (`0x0072E730` from
        // `WM_PAINT_Handler 0x00621E90`) draws SCRBK, `0x0060CF00`'s
        // background shape for both dialogs, at `[0x00B0FC1C]` with its
        // native 472x448 or 632x568 size (`0x103` also draws it once
        // before it runs, `0x0072E9F0`). At 1024x768 the origin is the
        // centred (112, 84), never (0, 0).
        shell_paint::push_entry_sized(
            &mut background_sprites,
            art.background,
            background.x as f32,
            background.y as f32,
            [background.w as f32, background.h as f32],
            PARENT_BACKGROUND_DEPTH,
        );
        frame.under.push(TexturedDraw {
            texture: &art.texture,
            instances: background_sprites,
        });
        if let (Some(strip), Some(frame_index)) = (art.scrt.as_ref(), scrt_frame)
            && let Some(entry) = strip.frames.get(frame_index).copied()
        {
            // `WM_PAINT_Handler 0x00621E90` draws the right-panel chrome
            // first, then `RadarBackground 0x0072E920` submits SCRT frame 0
            // at `[0x00B0FC1C]`, plus x=0x50 at 800 pixels and wider.
            let panel = layout.panel;
            let mut panel_sprites = Vec::with_capacity(1);
            shell_paint::push_entry_sized(
                &mut panel_sprites,
                entry,
                panel.x as f32,
                panel.y as f32,
                [panel.w as f32, panel.h as f32],
                DIALOG_BACKGROUND_DEPTH,
            );
            frame.over_panel_art.push(TexturedDraw {
                texture: &strip.texture,
                instances: panel_sprites,
            });
        }
        if let (Some(strip), Some(frame_index)) = (art.scra.as_ref(), emblem_frame)
            && let (Some(entry), Some(window)) = (
                strip.frames.get(frame_index).copied(),
                layout.control_rect(SCORE_ART_REGION_STATIC),
            )
            && let Some(sprite) = shell_paint::paint_kind4_static(entry, window, layout.page.scale)
        {
            frame.over_panel_art.push(TexturedDraw {
                texture: &strip.texture,
                instances: vec![sprite],
            });
        }
    } else {
        frame.backdrop = true;
    }
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
