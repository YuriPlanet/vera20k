//! Render glue for campaign victory dialog `0x103` and the LAN Cooperative
//! campaign end `0xBC7`.
//!
//! Native `GameExit::Victory` opens `0x103` through `0x6C8E80`, and
//! Cooperative `+0xA4` opens `0xBC7` through `0x00471420`; this renderer
//! consumes the open dialog's retail controls and texts from
//! `ui::campaign_score_shell`. It deliberately does not borrow the Skirmish
//! score table (`0x108`) or its side-art owner. The statics, `0xBC7`'s
//! cells and the SCRA emblem `0x72B` paint from the dialog's nodes
//! ([`CampaignScoreDialog::nodes`]).

use crate::util::time::Instant;

use anyhow::Result;
use vera20k_gui::Prop;

use crate::app::frontend::menu_page_render::{
    FamilyFrame, PanelState, encode_family_page, family_art_ready, page_tree,
};
use crate::app::frontend::score_shell_render::static_reveal;
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::skirmish_shell_render::gui_paint::{GuiStyle, PageArt, paint_gui_into};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint::{
    self, DIALOG_BACKGROUND_DEPTH, PARENT_BACKGROUND_DEPTH, SHELL_TEXT_RGB_ENABLED,
};
use crate::ui::campaign_score_shell::{
    CampaignScoreDialog, SCORE_ART_REGION_STATIC, STATUS_HELP_STATIC,
};
use crate::ui::score_shell::sync_static_texts;
use crate::ui::shell::menu_page::hovered_id;

/// Paint campaign dialog `0x103` or `0xBC7`. It needs the shared shell chrome,
/// but no Skirmish score art: each template has its own static controls.
pub(crate) fn render_campaign_score_page(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if frontend.campaign_score_page.is_none()
        || !family_art_ready(
            renderer,
            frontend,
            process_assets,
            &persistence.options_profile,
        )
    {
        return Ok(false);
    }

    let (width, height) = crate::app::frontend::shell_layout::shell_screen_size(&renderer.gpu);
    let fills_window = crate::app::frontend::shell_layout::fills_window(&renderer.gpu);
    let now = Instant::now();
    // `0x006071E0` owns both slides; `SetSlideChildSuppressProc` hides the
    // dialog's statics (including the SCRA kind-4 static) until SHOW completes,
    // and the teardown repaint covers them while the column slides out.
    let dialog = frontend
        .campaign_score_page
        .as_ref()
        .expect("checked before render")
        .dialog;
    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        &frontend.shell_exit,
        crate::app::frontend::shell_transition::ShellSlideKind::campaign_score(dialog),
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| frontend.shell_first_paint_slide.clone());
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
    let animation_frames = frontend
        .campaign_score_art
        .as_ref()
        .and_then(|art| art.scra.as_ref())
        .map_or(0, |strip| strip.frames.len());
    let (layout, emblem_frame) = {
        let page = frontend
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
    let mut tree = page_tree(
        &mut frontend.shell_controller,
        dialog.id(),
        || dialog.nodes(),
        rect_of,
        &[],
    );
    // `0xBC7`'s monitor `0x71C` runs the shared kind-4 timer.
    let monitor_frame = (children && dialog == CampaignScoreDialog::CampaignEnd)
        .then(|| crate::app::frontend::menu_page_render::paint_shell_monitor(frontend))
        .flatten();
    let controller = &frontend.shell_controller;
    let active = controller.top_id() == Some(dialog.id());
    let hovered = (children && active)
        .then(|| hovered_id(controller, dialog.id()))
        .flatten();
    let status = hovered
        .and_then(|id| dialog.control(id))
        .and_then(|control| control.tooltip_key)
        .map(|key| process_assets.csf_text(key))
        .unwrap_or_default();

    {
        let page = frontend
            .campaign_score_page
            .as_mut()
            .expect("checked before render");
        if children {
            page.statics.set_text(STATUS_HELP_STATIC, &status, now);
        }
        page.statics.paint(now, children);
        let page = &*page;
        let text_of = |id: u16| {
            let cell = || page.cell(id).map(|cell| cell.text.as_str());
            let text = page.statics.text(id).or_else(cell);
            Some((Prop::Text, text.unwrap_or_default()))
        };
        let controller = &mut frontend.shell_controller;
        match tree.as_mut() {
            Some((gui, root)) => sync_static_texts(gui, *root, text_of),
            None => {
                if let Some(root) = controller.root_in(dialog.id()) {
                    sync_static_texts(controller.gui_mut(), root, text_of);
                }
            }
        }
    }

    // The statics while they reveal, then `0xBC7`'s score cells whole in
    // their row colour, and the SCRA frame the kind-4 timer selected.
    let campaign_page = frontend
        .campaign_score_page
        .as_ref()
        .expect("checked before render");
    let scra = (frontend.campaign_score_art.as_ref()).and_then(|art| art.scra.as_ref());
    let text_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        let cell = children && campaign_page.cell(id).is_some();
        let shown = cell || campaign_page.statics.shown(id).is_some();
        (!shown).then(String::new)
    };
    let reveal_of = |id: i32| static_reveal(&campaign_page.statics, u16::try_from(id).ok()?);
    let rgb_of = |id: i32| {
        let cell = campaign_page.cell(u16::try_from(id).ok()?)?;
        Some(cell.rgb.map(|channel| f32::from(channel) / 255.0))
    };
    let page_art_of = |id: i32| {
        let frame = scra?.frames.get(emblem_frame?)?;
        (id == i32::from(SCORE_ART_REGION_STATIC)).then_some(PageArt::Shape(*frame))
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        reveal_of: Some(&reveal_of),
        rgb_of: Some(&rgb_of),
        page_art_of: Some(&page_art_of),
        rgb: SHELL_TEXT_RGB_ENABLED,
        ..Default::default()
    };
    let controller = &frontend.shell_controller;
    let nodes = match &tree {
        Some((gui, root)) => Some((gui, *root)),
        None => (controller.root_in(dialog.id())).map(|root| (controller.gui(), root)),
    };
    let mut paint = HudPaint::default();
    if let Some((gui, root)) = nodes {
        let scale = layout.page.scale;
        paint_gui_into(
            &mut paint,
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            None,
            gui,
            root,
            scale,
            &[],
            style,
        );
    }
    let emblem = paint.take_lane(Material::PageArt);

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
        &frontend.shell_controller,
        dialog.id(),
        &tree,
    );
    frame.over = paint;
    if let Some(art) = frontend.campaign_score_art.as_ref() {
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
        if let Some(strip) = art.scra.as_ref().filter(|_| !emblem.is_empty()) {
            frame.over_panel_art.push(TexturedDraw {
                texture: &strip.texture,
                instances: emblem,
            });
        }
    } else {
        frame.backdrop = true;
    }
    encode_family_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        encoder,
        destination,
        frame,
    );
    Ok(true)
}
