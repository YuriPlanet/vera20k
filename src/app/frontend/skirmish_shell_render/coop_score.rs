//! Paint the LAN co-op result dialog `0xBC6`. `UI__RegisterWindow`
//! (`0x00622820`) gives it BC's D5/D6 chrome (SDTP frame 1 and SDMPBTN)
//! and no RadarBackground (D7); `0x0060CF00` gives it `SCRBK` for its
//! background. The children are the setup page's native widgets.
//! RESIDUAL: the statics paint at once, without their kind-1 reveal, and
//! the page has no entry or exit slide.

use super::chrome::*;
use super::controls::{
    ControlPaint, house_color_tint, list_view_controls, paint_control, push_new_edit,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{build_preview_surface_instance, ensure_map_preview_texture};
use super::text::rect_to_text_rect;
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
    SHELL_PARENT_BACKGROUND_DEPTH,
};
use crate::app::AppState;
use crate::render::shell_text;
use crate::ui::campaign_score_shell::campaign_score_background_rect;
use crate::ui::coop_score_shell::*;
use crate::ui::shell::geom::{RectPx, ShellAlign};

pub(crate) fn render_coop_score(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<bool> {
    if state.frontend.skirmish_shell_chrome.is_none() {
        return Ok(false);
    }
    let Some(view) = crate::app::cooperative_round::view(state) else {
        return Ok(false);
    };
    crate::app::input::coop_score::update_geometry(state);
    let layout = crate::app::input::coop_score::layout(state);
    let scale = layout.page.scale;
    let (width, height) = (state.render_width(), state.render_height());
    let fills = crate::app::frontend::shell_layout::fills_window(state);
    let (can_continue, map_index) = {
        let model = &state.frontend.coop_score_page.as_ref().expect("view").model;
        (model.can_continue, model.map_index)
    };
    match map_index.filter(|_| can_continue) {
        Some(index) => {
            let preview = layout.control_rect(PREVIEW).unwrap_or_default();
            ensure_map_preview_texture(state, index, preview)
        }
        None => state.frontend.skirmish_preview_texture = None,
    }
    let shown = |id: u16| can_continue || !hidden_on_final_mission(id);
    let controller = &state.frontend.shell_controller;
    let hovered = crate::ui::shell::menu_page::hovered_id(controller, COOP_SCORE_PAGE.dialog);
    let status = hovered
        .and_then(help_key)
        .map(|key| state.csf_label(key, ""))
        .unwrap_or_default();
    let title = state.csf_label(COOP_SCORE_PAGE.title_key, COOP_SCORE_PAGE.title_key);
    let page = state.frontend.coop_score_page.as_ref().expect("view");
    let model = &page.model;
    let ui = &page.ui;
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("checked above");
    let font = &state.renderer.bit_font;
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        )
    };
    let rgb = |rgb: [u8; 3]| rgb.map(|channel| f32::from(channel) / 255.0);

    // Paint mode 1: RightPanel__Draw, the dialog background, then D5/D6.
    let chrome_layout = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    let mut art = Vec::new();
    push_right_panel_base_instances(&mut art, atlas, &chrome_layout, false);
    push_lower_strip_instance(&mut art, atlas, &chrome_layout);
    push_steady_optional_chrome_instances(
        &mut art,
        atlas,
        &chrome_layout,
        super::draw_order::ShellDialogChromeProfile::SkirmishSetup0x102,
    );
    let score_backdrop = state.frontend.campaign_score_art.as_ref().map(|scores| {
        let (aw, ah) = if fills {
            (800, 600)
        } else {
            (width as i32, height as i32)
        };
        let rect = scale.scale_rect(campaign_score_background_rect(aw, ah));
        let mut sprites = Vec::with_capacity(1);
        crate::render::shell_paint::push_entry_sized(
            &mut sprites,
            scores.background,
            rect.x as f32,
            rect.y as f32,
            [rect.w as f32, rect.h as f32],
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
        sprites[0]
    });

    let mut controls = Vec::new();
    let mut texts = Vec::new();
    texts.push(text(
        &title,
        layout.page.title,
        ShellAlign::PANEL_TITLE,
        SHELL_LABEL_TEXT_RGB,
    ));
    texts.push(text(
        &status,
        layout.page.status_help,
        ShellAlign::V_CENTER,
        SHELL_LABEL_TEXT_RGB,
    ));
    for spec in COOP_SCORE_CONTROLS.iter().filter(|spec| shown(spec.id)) {
        let Some(rect) = layout.control_rect(spec.id) else {
            continue;
        };
        let value = match (spec.id, spec.caption_key) {
            (TIME, _) => model.time.clone(),
            (NEXT_MISSION, _) => model.next_mission.clone(),
            (DESCRIPTION, _) => model.description.clone(),
            (_, Some(key)) => state.csf_label(key, key),
            (_, None) => continue,
        };
        texts.push(text(&value, rect, spec.align, SHELL_LABEL_TEXT_RGB));
    }
    for (label, id) in [
        (&model.mode_label, MODE_LABEL),
        (&model.map_label, MAP_LABEL),
    ] {
        if shown(id)
            && let Some(rect) = layout.control_rect(id)
        {
            texts.push(text(
                label,
                rect,
                ShellAlign::H_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
    }
    // The tail's cells: name, then "%d" for each count, in the row colour.
    for (row, cells) in model.rows.iter().zip(ROW_CELLS) {
        for (column, (value, id)) in row.cells().iter().zip(cells).enumerate() {
            let align = if column == 0 {
                ShellAlign::NONE
            } else {
                ShellAlign::H_RIGHT
            };
            if let Some(rect) = layout.control_rect(id) {
                texts.push(text(value, rect, align, rgb(row.rgb)));
            }
        }
    }
    let chrome = atlas.control_chrome();
    for (row, slot) in view.slots.iter().enumerate() {
        let Some(slot) = slot else {
            continue;
        };
        // 0x0046FA20: wolhost.pcx on slot 0 and wolacpt.pcx on slot 1 while
        // ready, on human slots only.
        if let Some(icon) = atlas.lan_player_status(row == 0 && slot.ready, row == 1 && slot.ready)
            && let Some(rect) = layout.control_rect(READY_ICONS[row])
        {
            push_flag_entry_clipped_centered(&mut controls, icon, rect, scale, SHELL_CONTROL_DEPTH);
        }
        if !can_continue {
            continue;
        }
        if let Some(flag) = slot
            .flag
            .as_deref()
            .and_then(|name| atlas.lan_player_side(name))
            && let Some(rect) = layout.control_rect(FLAGS[row])
        {
            push_flag_entry_clipped_centered(&mut controls, flag, rect, scale, SHELL_CONTROL_DEPTH);
        }
        let name = crate::ui::multiplayer_setup::SetupChoice {
            value: 0,
            text: slot.name.clone(),
            swatch: None,
            greyed: false,
        };
        // 0x0046DCC0 disables the other human's combos; the name combo has
        // no handler and stays disabled for both.
        for (id, item, enabled) in [
            (NAMES[row], &name, false),
            (COUNTRIES[row], &slot.country, slot.enabled),
            (COLORS[row], &slot.color, slot.enabled),
            (STARTS[row], &slot.start, slot.enabled),
        ] {
            let Some(rect) = layout.control_rect(id) else {
                continue;
            };
            paint_control(
                &mut controls,
                &chrome,
                ControlPaint::Combo {
                    rect,
                    swatch: item
                        .swatch
                        .map(|index| house_color_tint(state.rules(), index)),
                    open: false,
                    disabled: !enabled,
                },
                scale,
            );
            if item.swatch.is_none() {
                texts.push(text(
                    &item.text,
                    crate::ui::skirmish_shell::combo_text_rect(rect, scale),
                    ShellAlign::V_CENTER,
                    super::text::combo_face_text_color(!enabled),
                ));
            }
        }
    }
    if can_continue && let Some(rect) = layout.control_rect(DIFFICULTY) {
        // 0x0046DE20's slider: range 0..2 at [0xA8EB68], no value plaque.
        let bar = crate::ui::multiplayer_setup::SetupTrackbar {
            id: crate::ui::multiplayer_setup::DIFFICULTY,
            value: view.difficulty,
            range: DIFFICULTY_RANGE,
            enabled: view.host,
            visible: true,
        };
        paint_control(
            &mut controls,
            &chrome,
            ControlPaint::Trackbar {
                rect,
                thumb_left: bar.thumb_left(rect, scale),
                plaque: false,
                // EnableWindow(0x732) at `0x0046DDF3..0x0046DE10`.
                disabled: !bar.enabled,
            },
            scale,
        );
    }
    let (Some(chat_rect), Some(chat_log_rect)) =
        (layout.control_rect(CHAT), layout.control_rect(CHAT_LOG))
    else {
        return Ok(false);
    };
    // The NewEdit paints inside its one-pixel resource frame.
    let chat_rect = scale.derive_rect(chat_rect, |rect| {
        RectPx::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2)
    });
    // The edit and the log as their widgets hold them. The caret shows
    // steadily while the edit has BC6's focus.
    let gui = controller.gui();
    let edit_node = controller.control(COOP_SCORE_PAGE.dialog, CHAT);
    let mut edit = edit_node.map_or_else(Default::default, |node| {
        gui.edit_state(node).unwrap_or_default()
    });
    edit.focused = controller.focused_control(COOP_SCORE_PAGE.dialog) == Some(CHAT);
    let caret = edit.focused;
    let (yellow, depth) = (SHELL_LABEL_TEXT_RGB, SHELL_CONTROL_TEXT_DEPTH);
    push_new_edit(
        &mut controls,
        &mut texts,
        atlas,
        font,
        &edit,
        None,
        chat_rect,
        caret,
        yellow,
        depth,
        scale,
    );
    let log = controller
        .control(COOP_SCORE_PAGE.dialog, CHAT_LOG)
        .and_then(|node| gui.list_view(node));
    let top = log.map_or(0, |log| log.top);
    let pressed = log.and_then(|log| log.pressed);
    let metrics = vera20k_gui::shell::list::ListMetrics::scaled(scale);
    let chat = vera20k_gui::shell::list::ShellListGeometry::new(
        chat_log_rect,
        ui.chat.lines.len(),
        top,
        metrics,
    );
    list_view_controls(&mut controls, atlas, chat, (None, top, pressed), scale);
    for (visible, line) in ui
        .chat
        .lines
        .iter()
        .skip(top)
        .take(chat.visible_rows)
        .enumerate()
    {
        let color = if line.color == u32::MAX {
            SHELL_LABEL_TEXT_RGB
        } else {
            rgb([
                line.color as u8,
                (line.color >> 8) as u8,
                (line.color >> 16) as u8,
            ])
        };
        texts.push(text(
            &String::from_utf16_lossy(&line.text),
            scale.derive_rect(chat.row(visible), |rect| {
                RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
            }),
            ShellAlign::V_CENTER,
            color,
        ));
    }
    let mut overlay = ShellFrameOverlay {
        score_backdrop,
        ..Default::default()
    };
    if let (Some(preview), Some(rect)) = (
        state.frontend.skirmish_preview_texture.as_ref(),
        layout.control_rect(PREVIEW),
    ) {
        overlay.preview = build_preview_surface_instance(rect, preview.width, preview.height);
    }
    if let Some(root) = controller.root_in(COOP_SCORE_PAGE.dialog) {
        let enabled_of = |id: i32| match u16::try_from(id).ok()? {
            CONTINUE => Some(view.continue_enabled),
            _ => Some(view.quit_enabled),
        };
        let style = super::gui_paint::GuiStyle {
            buttons: super::gui_paint::ButtonArt::Panel,
            enabled_of: Some(&enabled_of),
            ..Default::default()
        };
        let ids = layout.page.buttons.iter().map(|button| button.id);
        let buttons = super::gui_paint::paint_gui_ids(
            state,
            controller.gui(),
            root,
            ids.filter(|id| shown(*id)),
            scale,
            style,
        );
        art.extend(buttons.controls);
        texts.extend(buttons.texts);
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "LAN Co-op Result BC6",
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    state.platform.window.request_redraw();
    Ok(true)
}
