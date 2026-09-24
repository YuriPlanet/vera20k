//! End-of-match score screen render glue for dialog `0x108`.
//!
//! Composes the same right-panel shell the main menu and single-player screens
//! use — centred parent background, SDTP/SDBTNBKGD/SDBTM panel column, lower
//! strip, one SDBTNANM owner-draw button — and lays the score table over it with
//! the shared bit font. Reuses `render::shell_paint` and the startup
//! `MainMenuShellChromeAtlas` outright; no second chrome or font path is added.
//!
//! The screen is presented through `ShellSurfacePresenter`, so it quantises
//! through the same active-retail 16-bit presentation boundary the main menu
//! does. That matters here specifically: the score screen is entered from the
//! battlefield and left to the main menu, and a screen drawn outside the boundary
//! shows a visible tint step against the menu it hands off to.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_pass::{owner_draw_button_label_rect, resolve_csf};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_transition_pass::ShellRenderTarget;
use crate::ui::score_shell::{ScoreScreenModel, ScoreShellLayout, compute_layout};

/// The Continue button is an ordinary owner-draw cell: native art, no hover
/// flash, no disabled state (it is the only control and always enabled). The
/// pressed sink matches the other non-main-menu shells.
const SCORE_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::Native,
    hover_flash: false,
    art_sink_y: 0.0,
    disabled_dim: true,
};
const BUTTON_LABEL_ALIGN: ShellAlign = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
const LEFT_CELL_ALIGN: ShellAlign = ShellAlign::V_CENTER;
const RIGHT_CELL_ALIGN: ShellAlign = ShellAlign(ShellAlign::H_RIGHT.0 | ShellAlign::V_CENTER.0);
const TITLE_ALIGN: ShellAlign = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);

/// CSF keys for the five column headings, in template order.
const COLUMN_KEYS: [&str; 5] = [
    "GUI:Player",
    "GUI:Kills",
    "GUI:Losses",
    "GUI:Built",
    "GUI:Score",
];

pub(crate) enum ScoreShellRenderResult {
    Rendered,
    /// The shell chrome atlas is unavailable, so the caller must fall back to its
    /// non-art result screen rather than present an empty frame.
    Fallback,
}



/// Format the elapsed match time through the retail time format string.
///
/// The format key carries the literal `Time:` prefix and three `%02d` fields, so
/// the whole label — prefix included — comes from the string table rather than
/// being assembled here.
pub(crate) fn format_elapsed(model: &ScoreScreenModel, format: &str) -> String {
    let (h, m, s) = model.elapsed_hms();
    substitute_numbers(format, &[h, m, s], 2)
}

/// Format the `Game: n` label through its retail format string.
pub(crate) fn format_game_number(model: &ScoreScreenModel, format: &str) -> String {
    substitute_numbers(format, &[model.game_number], 0)
}

/// Substitute successive integers into a printf-style CSF format string.
///
/// Only the two directives these two strings use are honoured (`%d` and
/// `%02d`); any other `%` run is copied through verbatim so a translated string
/// that does something unexpected degrades to visible text instead of a panic.
/// `default_pad` is the zero-pad width applied to a bare `%d` in a format whose
/// fields are all padded, which keeps a shortened translation aligned.
fn substitute_numbers(format: &str, values: &[u32], default_pad: usize) -> String {
    let mut out = String::with_capacity(format.len() + values.len() * 2);
    let mut next = values.iter();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut pad = String::new();
        while chars.peek().is_some_and(|p| p.is_ascii_digit()) {
            pad.push(chars.next().unwrap_or_default());
        }
        match chars.peek() {
            Some('d') => {
                chars.next();
                let width = pad.parse::<usize>().unwrap_or(default_pad);
                match next.next() {
                    Some(value) => out.push_str(&format!("{value:0width$}")),
                    None => out.push_str(&format!("%{pad}d")),
                }
            }
            Some('%') => {
                chars.next();
                out.push('%');
            }
            _ => {
                out.push('%');
                out.push_str(&pad);
            }
        }
    }
    out
}

fn rgb_f32(rgb: [u8; 3]) -> [f32; 3] {
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ]
}

/// Build every text draw on the screen: heading, the Game/Time summary pair, the
/// five column headings, the populated row cells, the button caption, and the
/// hover status line.
///
/// Row cells carry the owning house's colour; headings and chrome captions use
/// the shell's ordinary caption colour. Empty row slots emit nothing at all —
/// the template's blank statics have no caption to paint.
fn build_labels<'a>(
    state: &'a AppState,
    layout: &ScoreShellLayout,
    model: &'a ScoreScreenModel,
    cells: &'a [RowCellText],
    game_text: &'a str,
    time_text: &'a str,
) -> Vec<PaintLabel<'a>> {
    let mut out = Vec::with_capacity(cells.len() * 5 + 10);
    out.push(PaintLabel {
        text: resolve_csf(state, model.title_key),
        rect: layout.title,
        align: TITLE_ALIGN,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    out.push(PaintLabel {
        text: std::borrow::Cow::Borrowed(game_text),
        rect: layout.game_label,
        align: LEFT_CELL_ALIGN,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    out.push(PaintLabel {
        text: std::borrow::Cow::Borrowed(time_text),
        rect: layout.time_label,
        align: RIGHT_CELL_ALIGN,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    let header_rects = [
        (layout.header.name, LEFT_CELL_ALIGN),
        (layout.header.kills, RIGHT_CELL_ALIGN),
        (layout.header.losses, RIGHT_CELL_ALIGN),
        (layout.header.built, RIGHT_CELL_ALIGN),
        (layout.header.score, RIGHT_CELL_ALIGN),
    ];
    for (key, (rect, align)) in COLUMN_KEYS.iter().zip(header_rects) {
        out.push(PaintLabel {
            text: resolve_csf(state, key),
            rect,
            align,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    for (slot, cell) in cells.iter().enumerate() {
        let Some(rects) = layout.rows.get(slot) else {
            break;
        };
        let rgb = rgb_f32(cell.rgb);
        let cells = [
            (cell.name.as_str(), rects.name, LEFT_CELL_ALIGN),
            (cell.kills.as_str(), rects.kills, RIGHT_CELL_ALIGN),
            (cell.losses.as_str(), rects.losses, RIGHT_CELL_ALIGN),
            (cell.built.as_str(), rects.built, RIGHT_CELL_ALIGN),
            (cell.score.as_str(), rects.score, RIGHT_CELL_ALIGN),
        ];
        for (text, rect, align) in cells {
            out.push(PaintLabel {
                text: std::borrow::Cow::Borrowed(text),
                rect,
                align,
                rgb,
                path_a_reveal: None,
            });
        }
    }
    out.push(PaintLabel {
        text: resolve_csf(state, "GUI:Continue"),
        rect: owner_draw_button_label_rect(
            layout.continue_button,
            state.frontend.score_shell_state.continue_pressed,
        ),
        align: BUTTON_LABEL_ALIGN,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    if state.frontend.score_shell_state.continue_hovered {
        out.push(PaintLabel {
            text: resolve_csf(state, "STT:MPScoreButtonContinue"),
            rect: layout.status_help,
            align: LEFT_CELL_ALIGN,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    out
}

/// Bind the score screen's otherwise shared paint labels to native ScoreFont
/// glyph selection without changing the shared shell/BitFont text path.
///
/// Retail provenance: post-match score-font binding — `ScoreFontClass__Constructor @ 0x00690580`.
fn bind_score_font_text(labels: &mut [PaintLabel<'_>]) {
    for label in labels {
        let converted = crate::util::native_string::score_font_text(label.text.as_ref());
        label.text = std::borrow::Cow::Owned(converted);
    }
}

/// One row's five cells rendered to strings, kept alive for the paint pass.
pub(crate) struct RowCellText {
    rgb: [u8; 3],
    name: String,
    kills: String,
    losses: String,
    built: String,
    score: String,
}

pub(crate) fn row_cell_text(model: &ScoreScreenModel) -> Vec<RowCellText> {
    model
        .rows
        .iter()
        .take(crate::ui::score_shell::SCORE_ROW_SLOTS)
        .map(|row| RowCellText {
            rgb: row.rgb,
            name: row.name.clone(),
            kills: row.kills.to_string(),
            losses: row.losses.to_string(),
            built: row.built.to_string(),
            score: row.score.to_string(),
        })
        .collect()
}

fn parent_background_instances(
    atlas: &crate::render::main_menu_shell_chrome::MainMenuShellChromeAtlas,
    layout: &ScoreShellLayout,
) -> Vec<SpriteInstance> {
    // MNSCRNS only at exactly 640 wide, else MNSCRNL — the same switch the other
    // shells make, drawn at native canvas size at the centred shell origin.
    let entry = if layout.screen.w == 640 {
        atlas.parent_background_640_mnscrns
    } else {
        atlas.parent_background_large_mnscrnl
    };
    let Some(entry) = entry else {
        return Vec::new();
    };
    let x = if layout.screen.w > 1023 {
        (layout.screen.w - 800) / 2
    } else {
        0
    };
    let y = if layout.screen.h > 767 {
        (layout.screen.h - 600) / 2
    } else {
        0
    };
    vec![SpriteInstance {
        position: [x as f32, y as f32],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth: PARENT_BACKGROUND_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }]
}


/// Paint the score screen through the shell presentation boundary.
pub(crate) fn render_score_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<ScoreShellRenderResult> {
    if state.frontend.main_menu_shell_chrome.is_none() || state.frontend.score_screen.is_none() {
        return Ok(ScoreShellRenderResult::Fallback);
    }
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let depth = state.renderer.depth_view.clone();
    let result = render_score_shell_to_target(
        state,
        encoder,
        ShellRenderTarget {
            color: &color,
            depth: &depth,
        },
    )?;
    if matches!(result, ScoreShellRenderResult::Rendered) {
        state
            .renderer.shell_surface_presenter
            .encode_present(encoder, destination);
    }
    Ok(result)
}

fn render_score_shell_to_target(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
) -> Result<ScoreShellRenderResult> {
    let layout = compute_layout(state.renderer.gpu.config.width, state.renderer.gpu.config.height);
    let Some(model) = state.frontend.score_screen.clone() else {
        return Ok(ScoreShellRenderResult::Fallback);
    };
    let Some(chrome) = state.frontend.main_menu_shell_chrome.as_ref() else {
        return Ok(ScoreShellRenderResult::Fallback);
    };

    let background_instances = parent_background_instances(chrome, &layout);
    let chrome_instances = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    );
    let buttons = [PaintButton {
        rect: layout.continue_button,
        pressed: state.frontend.score_shell_state.continue_pressed,
        hovered: state.frontend.score_shell_state.continue_hovered,
        enabled: true,
    }];
    let button_instances = shell_paint::paint_buttons(
        chrome,
        &buttons,
        SCORE_BUTTON_POLICY,
        std::time::Instant::now(),
        None,
    );

    let cells = row_cell_text(&model);
    let game_text = format_game_number(&model, resolve_csf(state, "TXT_GAME").as_ref());
    let time_text = format_elapsed(&model, resolve_csf(state, "TXT_TIME_FORMAT_HOURS").as_ref());
    let mut labels = build_labels(state, &layout, &model, &cells, &game_text, &time_text);
    bind_score_font_text(&mut labels);
    let text_draws = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
    drop(labels);

    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        state.renderer.gpu.config.width as f32,
        state.renderer.gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let background_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &background_instances);
    let chrome_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &chrome_instances);
    let button_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &button_instances);
    let text_buffers: Vec<_> = text_draws
        .iter()
        .map(|draw| {
            state
                .renderer.batch_renderer
                .create_instance_buffer(&state.renderer.gpu, &draw.instances)
        })
        .collect();
    let cursor_instances: Vec<SpriteInstance> =
        crate::app::frontend::shell_pass::software_cursor(state, CURSOR_DEPTH)
            .map(|(_, instance)| instance)
            .into_iter()
            .collect();
    let cursor_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &cursor_instances);
    let cursor_texture = state
        .match_state.match_presentation.software_cursor
        .as_ref()
        .and_then(|cursor| cursor.get(crate::app::types::CursorId::Default))
        .and_then(|sequence| sequence.frames.first())
        .map(|frame| &frame.texture);
    let chrome = state
        .frontend.main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Score Shell"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: target.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    for buffer in [&background_buffer, &chrome_buffer, &button_buffer] {
        if let Some((buffer, count)) = buffer.as_ref() {
            state.renderer.batch_renderer.draw_with_buffer_passthrough(
                &mut pass,
                &chrome.texture,
                buffer,
                *count,
            );
        }
    }
    for (draw, buffer) in text_draws.iter().zip(text_buffers.iter()) {
        let Some((buffer, count)) = buffer.as_ref() else {
            continue;
        };
        pass.set_scissor_rect(
            draw.scissor.x,
            draw.scissor.y,
            draw.scissor.w,
            draw.scissor.h,
        );
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    pass.set_scissor_rect(0, 0, state.renderer.gpu.config.width, state.renderer.gpu.config.height);
    if let (Some((buffer, count)), Some(texture)) = (cursor_buffer.as_ref(), cursor_texture) {
        state
            .renderer.batch_renderer
            .draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    Ok(ScoreShellRenderResult::Rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::score_shell::ScoreRow;
    use crate::ui::shell::geom::RectPx;

    fn model(seconds: u32, game: u32) -> ScoreScreenModel {
        ScoreScreenModel {
            title_key: "GUI:SkirmishScore",
            game_number: game,
            elapsed_seconds: seconds,
            rows: vec![ScoreRow {
                name: "Player".into(),
                rgb: [255, 0, 0],
                kills: 4,
                losses: 1,
                built: 9,
                score: 1200,
            }],
        }
    }

    #[test]
    fn time_label_uses_the_retail_zero_padded_format() {
        // Retail TXT_TIME_FORMAT_HOURS is "Time: %02d:%02d:%02d"; the prefix is
        // part of the string table entry, not assembled by the renderer.
        assert_eq!(
            format_elapsed(&model(3661, 1), "Time: %02d:%02d:%02d"),
            "Time: 01:01:01"
        );
        assert_eq!(
            format_elapsed(&model(59, 1), "Time: %02d:%02d:%02d"),
            "Time: 00:00:59"
        );
    }

    #[test]
    fn time_label_applies_the_native_ceiling() {
        assert_eq!(
            format_elapsed(&model(u32::MAX, 1), "Time: %02d:%02d:%02d"),
            "Time: 99:59:59"
        );
    }

    #[test]
    fn game_label_uses_the_retail_unpadded_format() {
        assert_eq!(format_game_number(&model(0, 3), "Game: %d"), "Game: 3");
    }

    #[test]
    fn format_substitution_survives_a_translation_with_missing_or_extra_fields() {
        // Too few values: the surplus directive is left visible rather than
        // panicking or silently dropping the label.
        assert_eq!(
            format_elapsed(&model(61, 1), "%02d:%02d:%02d:%02d"),
            "00:01:01:%02d"
        );
        // A literal percent and an unknown directive both pass through.
        assert_eq!(format_game_number(&model(0, 2), "%d%% %s"), "2% %s");
    }

    #[test]
    fn row_cells_are_capped_at_the_template_slot_count() {
        let mut m = model(0, 1);
        m.rows = (0..12)
            .map(|i| ScoreRow {
                name: format!("P{i}"),
                rgb: [1, 2, 3],
                kills: i,
                losses: 0,
                built: 0,
                score: 0,
            })
            .collect();
        assert_eq!(
            row_cell_text(&m).len(),
            crate::ui::score_shell::SCORE_ROW_SLOTS
        );
    }

    #[test]
    fn pressed_button_caption_uses_the_native_owner_draw_sink() {
        let rect = RectPx::new(644, 535, 156, 42);
        assert_eq!(
            owner_draw_button_label_rect(rect, false),
            RectPx::new(644, 536, 154, 41)
        );
        assert_eq!(
            owner_draw_button_label_rect(rect, true),
            RectPx::new(646, 540, 152, 37)
        );
    }
}
