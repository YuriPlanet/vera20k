//! Shared room result chrome (`0xBC6`) with a caller-supplied body.

use crate::app::AppState;
use crate::app::frontend::score_overlay::{ScoreLayout, room_result_layout};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint;
use crate::render::shell_text::{self, ShellAlign};
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::score_shell::{ROOM_RESULT_DIALOG, ROOM_RESULT_PAGE};
use crate::ui::shell::button::ButtonVisual;
use crate::ui::shell::geom::{self, RectPx, ShellScale};

use super::chrome::{
    push_lower_strip_instance, push_right_panel_base_instances, push_right_panel_button_shp,
    push_steady_optional_chrome_instances,
};
use super::draw_order::ShellDialogChromeProfile;
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{build_preview_surface_instance, ensure_local_scenario_preview};
use super::text::{
    button_label_color_for_disabled, button_text_rect, localized_label, rect_to_text_rect,
};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
    SHELL_PARENT_BACKGROUND_DEPTH,
};

/// Native BC6 uses D5/D6 without the radar-background monitor (D7).
pub(super) fn room_result_chrome(
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    scale: ShellScale,
) -> (Vec<SpriteInstance>, Option<SpriteInstance>) {
    let (width, height) = (state.render_width(), state.render_height());
    let layout = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    let mut art = Vec::new();
    push_right_panel_base_instances(&mut art, atlas, &layout, false);
    push_lower_strip_instance(&mut art, atlas, &layout);
    push_steady_optional_chrome_instances(
        &mut art,
        atlas,
        &layout,
        ShellDialogChromeProfile::SkirmishSetup0x102,
    );
    let backdrop = state
        .frontend
        .campaign_score_art
        .as_ref()
        .and_then(|scores| {
            let (aw, ah) = if crate::app::frontend::shell_layout::fills_window(state) {
                (800, 600)
            } else {
                (width as i32, height as i32)
            };
            let (x, y) = geom::dialog_origin(aw, ah);
            let small = layout.authored_width() == 640;
            let rect = scale.scale_rect(RectPx::new(
                x,
                y,
                if small { 472 } else { 632 },
                if small { 448 } else { 568 },
            ));
            let mut sprites = Vec::with_capacity(1);
            shell_paint::push_entry_sized(
                &mut sprites,
                scores.background,
                rect.x as f32,
                rect.y as f32,
                [rect.w as f32, rect.h as f32],
                SHELL_PARENT_BACKGROUND_DEPTH,
            );
            sprites.pop()
        });
    (art, backdrop)
}

pub(crate) fn render_result_overlay(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<bool> {
    let Some(overlay) = state.frontend.score_overlay.clone() else {
        return Ok(false);
    };
    let map = overlay.map_preview();
    if let Some(map) = map {
        ensure_local_scenario_preview(
            state,
            &map.scenario.key,
            map.scenario.presentation.as_deref(),
        );
    }
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return Ok(false);
    };
    let page = room_result_layout(state);
    let layout = ScoreLayout::from(&page);
    let (mut art, score_backdrop) = room_result_chrome(state, atlas, layout.scale);
    let font = &state.renderer.bit_font;
    let body = overlay.body(state, &layout);
    let mut texts = shell_paint::paint_labels_at_depth_scaled(
        font,
        &body.labels,
        SHELL_CONTROL_TEXT_DEPTH,
        body.glyph_scale,
    );
    let title = overlay
        .title()
        .unwrap_or_else(|| localized_label(state, ROOM_RESULT_PAGE.title_key, "Campaign Results"));
    texts.push(shell_text::draw_in_rect_scaled(
        font,
        &title,
        rect_to_text_rect(page.title),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::PANEL_TITLE,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        layout.scale.factor(),
    ));
    let controller = &state.frontend.shell_controller;
    let pressed = controller.pressed_in(ROOM_RESULT_DIALOG);
    let hovered = controller.hovered_in(ROOM_RESULT_DIALOG);
    for button in overlay.buttons(&layout) {
        let enabled = overlay.button_enabled(state, button.id);
        let pressed = enabled && pressed == Some(button.id);
        push_right_panel_button_shp(
            &mut art,
            atlas,
            button.rect,
            ButtonVisual {
                pressed,
                highlighted: enabled && hovered == Some(button.id),
            },
            !enabled,
            SHELL_CONTROL_DEPTH,
        );
        let caption = localized_label(state, button.caption, button.caption);
        texts.push(shell_text::draw_in_rect_scaled(
            font,
            &caption,
            button_text_rect(button.rect, pressed, layout.scale),
            button_label_color_for_disabled(!enabled),
            ShellAlign::BUTTON_CAPTION,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            layout.scale.factor(),
        ));
    }

    let mut preview = None;
    if let Some(map) = map {
        let (aw, ah) = if crate::app::frontend::shell_layout::fills_window(state) {
            (800, 600)
        } else {
            (page.screen.w, page.screen.h)
        };
        let (picture, name) = crate::ui::coop_score_shell::room_map_rects(layout.scale, aw, ah);
        if let Some(texture) = &state.frontend.skirmish_preview_texture {
            preview = build_preview_surface_instance(picture, texture.width, texture.height);
        }
        let label = |text: &str, rect| {
            shell_text::draw_in_rect_scaled(
                font,
                text,
                rect_to_text_rect(rect),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                [0.0; 2],
                SHELL_CONTROL_TEXT_DEPTH,
                layout.scale.factor(),
            )
        };
        if preview.is_none() {
            let unavailable = localized_label(state, map.unavailable_label, map.unavailable_label);
            texts.push(label(&unavailable, picture));
        }
        let name_text = map
            .scenario
            .presentation
            .as_ref()
            .map_or(map.scenario.title.as_str(), |presentation| {
                presentation.stage_title.as_str()
            });
        texts.push(label(name_text, name));
    }

    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls: Vec::new(),
            texts,
            label: "Room Result 0xBC6",
        },
        ShellFrameArt::Launcher,
        ShellFrameOverlay {
            score_backdrop,
            preview,
            ..Default::default()
        },
        ShellFrameCursor::Visible,
    )?;
    Ok(true)
}
