//! Render glue for campaign victory dialog `0x103` and the LAN Cooperative
//! campaign end `0xBC7`.
//!
//! Native `GameExit::Victory` opens `0x103` through `0x6C8E80`, and
//! Cooperative `+0xA4` opens `0xBC7` through `0x00471420`; this renderer
//! consumes the open dialog's retail controls and texts from
//! `ui::campaign_score_shell`. It deliberately does not borrow the Skirmish
//! score table (`0x108`) or its side-art owner.

use std::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, DIALOG_BACKGROUND_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::campaign_score_shell::{CampaignScoreDialog, SCORE_ART_REGION_STATIC};

const SHELL_TEXT: [u8; 3] = [255, 255, 0];
const HIGHLIGHT: [u8; 3] = [255, 255, 255];

/// Paint campaign dialog `0x103` or `0xBC7`. It needs the shared shell chrome,
/// but no Skirmish score art: each template has its own static controls.
pub(crate) fn render_campaign_score_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none()
        || state.frontend.campaign_score_page.is_none()
    {
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
    // `0xBC7`'s monitor `0x71C` runs the shared kind-4 timer.
    let monitor_frame = (children && dialog == CampaignScoreDialog::CampaignEnd)
        .then(|| crate::app::frontend::menu_page_render::paint_shell_monitor(state))
        .flatten();
    let continue_id = dialog.continue_button();
    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(dialog.id());
    let hovered = children
        .then(|| active.then(|| controller.hovered()).flatten())
        .flatten();
    let status = hovered
        .and_then(|id| dialog.control(id))
        .and_then(|control| control.tooltip_key)
        .map(|key| resolve_csf(state, key).into_owned())
        .unwrap_or_default();
    let pressed = active.then(|| controller.pressed()).flatten();

    let mut labels: Vec<PaintLabel<'static>> = Vec::new();
    let campaign_page = state
        .frontend
        .campaign_score_page
        .as_mut()
        .expect("checked before render");
    if children {
        campaign_page.set_status_help(&status, now);
        for static_text in campaign_page.statics_mut() {
            let Some(window) = static_text.paint(now) else {
                continue;
            };
            let Some(rect) = layout.control_rect(static_text.id) else {
                continue;
            };
            labels.push(PaintLabel {
                text: static_text.text().to_owned().into(),
                rect,
                align: static_align(dialog, static_text.id),
                rgb: static_text.rgb.map_or(SHELL_TEXT_RGB_ENABLED, rgb_f32),
                path_a_reveal: Some(PathAReveal {
                    count: window.count,
                    range: window.range,
                    base_rgb: static_text.rgb.unwrap_or(SHELL_TEXT),
                    highlight_rgb: HIGHLIGHT,
                }),
            });
        }
        // `0xBC7`'s score cells paint whole in their row colour.
        for cell in campaign_page.cells() {
            let Some(rect) = layout.control_rect(cell.id) else {
                continue;
            };
            labels.push(PaintLabel {
                text: cell.text.clone().into(),
                rect,
                align: static_align(dialog, cell.id),
                rgb: rgb_f32(cell.rgb),
                path_a_reveal: None,
            });
        }
    }

    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let mut chrome_sprites = if state.frontend.campaign_score_art.is_none() {
        crate::app::frontend::main_menu_shell_render::shell_parent_background_instances(
            chrome,
            layout.screen.w,
            layout.screen.h,
            layout.scale,
        )
    } else {
        Vec::new()
    };
    chrome_sprites.extend(shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    ));
    chrome_sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.warning_monitor, frame, layout.scale)
    }));

    if let Some(wave) = wave.as_ref() {
        // The slide engine paints the whole right-panel column; the steady
        // Continue cell is not drawn until the wave ends.
        chrome_sprites.extend(shell_paint::paint_slide_column(
            chrome,
            layout.right_panel,
            &wave.button_draws(),
        ));
    }
    let buttons: Vec<PaintButton> = if wave.is_some() {
        Vec::new()
    } else {
        vec![PaintButton {
            rect: layout.continue_button,
            pressed: pressed == Some(continue_id),
            hovered: false,
            enabled: true,
        }]
    };
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        now,
        None,
    );
    if wave.is_none() {
        labels.push(PaintLabel {
            text: resolve_csf(state, "GUI:Continue").into_owned().into(),
            rect: owner_draw_button_label_rect(
                layout.continue_button,
                pressed == Some(continue_id),
            ),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    let text =
        shell_paint::paint_labels_scaled(&state.renderer.bit_font, &labels, layout.scale.factor());
    let mut draws = Vec::with_capacity(6);
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
        draws.push(TexturedDraw {
            texture: &art.texture,
            instances: background_sprites,
        });
    }
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: chrome_sprites,
    });
    if let Some(art) = state.frontend.campaign_score_art.as_ref() {
        if let (Some(strip), Some(frame)) = (art.scrt.as_ref(), scrt_frame)
            && let Some(entry) = strip.frames.get(frame).copied()
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
            draws.push(TexturedDraw {
                texture: &strip.texture,
                instances: panel_sprites,
            });
        }
        if let (Some(strip), Some(frame)) = (art.scra.as_ref(), emblem_frame)
            && let (Some(entry), Some(window)) = (
                strip.frames.get(frame).copied(),
                layout.control_rect(SCORE_ART_REGION_STATIC),
            )
            && let Some(sprite) = shell_paint::paint_kind4_static(entry, window, layout.scale)
        {
            draws.push(TexturedDraw {
                texture: &strip.texture,
                instances: vec![sprite],
            });
        }
    }
    draws.push(TexturedDraw {
        texture: &chrome.texture,
        instances: button_sprites,
    });
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Campaign Score Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );
    Ok(true)
}

/// The low two bits of Win32 STATIC style select left, centre or right.
fn static_align(dialog: CampaignScoreDialog, id: u16) -> ShellAlign {
    match dialog.control(id).map(|control| control.native_style & 0x3) {
        Some(1) => ShellAlign::H_CENTER,
        Some(2) => ShellAlign::H_RIGHT,
        _ => ShellAlign::NONE,
    }
}

fn rgb_f32(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|channel| f32::from(channel) / 255.0)
}
