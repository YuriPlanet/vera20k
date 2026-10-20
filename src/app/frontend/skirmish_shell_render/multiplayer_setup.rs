//! BC/BD/C2/C9 setup paint using the shared shell compositor and native widgets.
//! Session data is a read-only projection; rendering never changes readiness.

use super::chrome::*;
use super::controls::{
    ControlPaint, NewEditPaint, dropdown_scroll_part, house_color_tint, list_controls,
    paint_control, push_new_edit_instances, push_popup_instances,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{
    aspect_fit_rect, build_preview_surface_instance, ensure_map_preview_texture,
    project_preview_start_positions, push_start_marker_sprites,
};
use super::text::{
    button_text_rect, localized_label, push_start_marker_labels, rect_to_text_rect, static_label,
    status_line_label,
};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
    SHELL_PARENT_BACKGROUND_DEPTH,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::geom::RectPx;
use std::time::Instant;

pub(crate) fn render_multiplayer_setup(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    if let Some((completed, total)) = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .and_then(|protocol| protocol.transfer_progress())
    {
        let (progress_meter, texts) =
            crate::app::loading::pump::transfer_progress_draws(state, completed, total)?;
        let layout = crate::app::input::multiplayer_setup::layout(state);
        let atlas = state
            .frontend
            .skirmish_shell_chrome
            .as_ref()
            .expect("setup art");
        let mut art = Vec::new();
        super::modals::push_message_box_backdrop_instances(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(
                layout.page.scale,
                state.render_width(),
                state.render_height(),
            ),
        );
        return in_game_shell::render_shell_frame(
            state,
            encoder,
            destination,
            InGameShellFrame {
                art,
                controls: Vec::new(),
                texts: Vec::new(),
                label: "LAN map transfer",
            },
            ShellFrameArt::Launcher,
            ShellFrameOverlay {
                progress_meter,
                texts,
                ..Default::default()
            },
            ShellFrameCursor::Hidden,
        );
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        return Ok(());
    };
    crate::app::input::multiplayer_setup::update_geometry(state);
    let layout = crate::app::input::multiplayer_setup::layout(state);
    let scale = layout.page.scale;
    let (width, height) = (state.render_width(), state.render_height());
    let now = Instant::now();
    let exit = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        ShellSlideKind::NetworkSetup(view.dialog),
    )
    .cloned();
    let leaving = exit.is_some();
    let wave = exit.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    if let Some(index) = view.selected_map_index {
        ensure_map_preview_texture(state, index);
    } else {
        state.frontend.skirmish_preview_texture = None;
    }
    let heading = localized_label(state, view.page().title_key, view.page().title_key);
    let accept_highlight = state
        .frontend
        .multiplayer_setup
        .as_mut()
        .expect("active setup")
        .accept_highlight(!view.host && view.start_requested, now);
    let labels = {
        let ui = state
            .frontend
            .multiplayer_setup
            .as_mut()
            .expect("active setup");
        if !sliding && !ui.shown {
            ui.statics
                .show(&heading, &view.mode_label, &view.map_label, now);
            ui.shown = true;
        }
        ui.statics
            .set_map_labels(&view.mode_label, &view.map_label, now);
        let shown = ui.statics.paint(now, sliding);
        [
            (shown.heading, layout.page.title),
            (shown.game_type, layout.mode_label),
            (shown.map_label, layout.map_label),
        ]
        .into_iter()
        .filter_map(|(shown, rect)| {
            shown.map(|shown| static_label(shown, rect, ShellAlign::H_CENTER))
        })
        .chain(status_line_label(
            shown.status_line,
            layout.page.status_help,
        ))
        .collect::<Vec<_>>()
    };
    let ui = state
        .frontend
        .multiplayer_setup
        .as_ref()
        .expect("active setup");
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("setup art");
    let font = &state.renderer.bit_font;
    let mut art =
        super::launcher_options::launcher_background_instances(atlas, width, height, scale);
    //72CF40 installs Cooperative setup art for BC/BD; it is independent of
    // the user's intentional RA2 art choice for offline Skirmish.
    if let Some(background) = atlas.lan_background(
        view.page().dialog.0,
        if scale.is_retail() { width } else { 800 },
    ) {
        if scale.is_retail() {
            push_entry_native(
                &mut art,
                background,
                0,
                0,
                SHELL_PARENT_BACKGROUND_DEPTH - 0.000001,
            );
        } else {
            let origin = scale.dialog_rect();
            push_entry_grown(
                &mut art,
                background,
                origin.x,
                origin.y,
                scale,
                SHELL_PARENT_BACKGROUND_DEPTH - 0.000001,
            );
        }
    }
    // BC/BD share 102's D5/D6 = 1/1 chrome (6228E4..62298A).
    // Parent paint622130/622145 adds SDTP frame1 (72E8C0) and SDMPBTN
    // frame0 (72E860); slide6071E0 temporarily owns those panel draws.
    let chrome_layout = crate::ui::skirmish_shell::compute_layout_on(scale, width, height);
    if let Some(wave) = &wave {
        push_slide_panel_art(&mut art, atlas, &chrome_layout, &wave.panel_art());
        push_slide_column(
            &mut art,
            atlas,
            &chrome_layout,
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    } else {
        push_steady_optional_chrome_instances(
            &mut art,
            atlas,
            &chrome_layout,
            super::draw_order::ShellDialogChromeProfile::SkirmishSetup0x102,
        );
    }
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let mut overlay = ShellFrameOverlay::default();
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            depth,
            scale.factor(),
        )
    };
    if !leaving && ui.notice.is_none() {
        for (key, rect) in &layout.labels {
            texts.push(text(
                &localized_label(state, key, key),
                *rect,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        for combo in view.combos.iter().filter(|combo| combo.visible) {
            let Some(rect) = layout.control_rect(combo.id) else {
                continue;
            };
            let selected = combo.selected.and_then(|index| combo.choices.get(index));
            paint_control(
                &mut controls,
                &atlas.control_chrome(),
                ControlPaint::Combo {
                    rect,
                    swatch: selected
                        .and_then(|item| item.swatch)
                        .map(|index| house_color_tint(&[], index)),
                    open: ui.popup.control == Some(combo.id),
                    disabled: !view.enabled(combo.id),
                },
                scale,
            );
            if let Some(item) = selected
                && item.swatch.is_none()
            {
                texts.push(text(
                    &item.text,
                    crate::ui::skirmish_shell::combo_text_rect(rect, scale),
                    ShellAlign::V_CENTER,
                    super::text::combo_face_text_color(!view.enabled(combo.id)),
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            }
        }
        for (row, icon) in view.row_icons.iter().enumerate() {
            let Some(icon) = icon else {
                continue;
            };
            if let Some(flag) = icon
                .country
                .as_deref()
                .and_then(|country| atlas.lan_player_side(country))
            {
                push_flag_entry_clipped_centered(
                    &mut controls,
                    flag,
                    layout.flags[row],
                    scale,
                    SHELL_CONTROL_DEPTH,
                );
            }
            if let Some(status) = atlas.lan_player_status(icon.host, icon.accepted) {
                push_flag_entry_clipped_centered(
                    &mut controls,
                    status,
                    layout.acceptance[row],
                    scale,
                    SHELL_CONTROL_DEPTH,
                );
            }
        }
        // C2/C9 ping statics 0x754+row, kind-2 images like the acceptance ones.
        if let Some(rects) = &layout.ping_icons {
            for (row, icon) in view.ping_icons.iter().enumerate() {
                if let Some(entry) = icon.and_then(|icon| atlas.wol_ping_icon(icon)) {
                    push_flag_entry_clipped_centered(
                        &mut controls,
                        entry,
                        rects[row],
                        scale,
                        SHELL_CONTROL_DEPTH,
                    );
                }
            }
        }
        for (index, key) in [
            "GUI:ShortGame",
            "GUI:MCVRepacks",
            "GUI:CratesAppear",
            "GUI:SuperWeaponsAllowed",
            "GUI:BuildOffAlly",
        ]
        .into_iter()
        .enumerate()
        {
            let rect = layout.checkboxes[index];
            paint_control(
                &mut controls,
                &atlas.control_chrome(),
                ControlPaint::Checkbox {
                    rect,
                    checked: view.checks[index],
                },
                scale,
            );
            texts.push(text(
                &localized_label(state, key, key),
                crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        for bar in view.trackbars.iter().filter(|bar| bar.visible) {
            let Some(rect) = layout.control_rect(bar.id) else {
                continue;
            };
            paint_control(
                &mut controls,
                &atlas.control_chrome(),
                ControlPaint::Trackbar {
                    rect,
                    thumb_left: bar.thumb_left(rect, scale),
                    plaque: bar.id != DIFFICULTY,
                },
                scale,
            );
            if bar.id != DIFFICULTY {
                texts.push(text(
                    &bar.range.stepped(bar.value).to_string(),
                    crate::ui::shell::trackbar::value_text_rect(rect, scale),
                    ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    SHELL_LABEL_TEXT_RGB,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            }
        }
        if let Some(description) = &view.cooperative_description {
            texts.push(text(
                description,
                layout.cooperative_description,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        if view.cooperative {
            for (key, x) in [("TXT_Easy", 170), ("TXT_Normal", 295), ("TXT_Hard", 430)] {
                texts.push(text(
                    &localized_label(state, key, key),
                    scale.scale_rect(RectPx::new(x, 190, 75, 20)),
                    ShellAlign::H_CENTER,
                    SHELL_LABEL_TEXT_RGB,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            }
        }
        let edit = &ui.chat.edit;
        let field = scale.derive_rect(layout.chat, |rect| {
            RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
        });
        push_new_edit_instances(
            &mut controls,
            atlas,
            font,
            edit,
            NewEditPaint {
                frame: scale.derive_rect(layout.chat, |rect| {
                    RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                }),
                text: field,
                background: None,
                frame_depth: SHELL_EDIT_FRAME_DEPTH,
                caret_depth: SHELL_EDIT_CARET_DEPTH,
                caret_visible: ui.caret_visible(now),
            },
            scale,
        );
        texts.push(text(
            &String::from_utf16_lossy(&edit.units[edit.first_visible_unit.min(edit.units.len())..]),
            field,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        let chat = ui
            .chat
            .list
            .geometry(layout.chat_log, ui.chat.lines.len(), scale);
        list_controls(&mut controls, atlas, chat, &ui.chat.list, false, scale);
        for (visible, line) in ui
            .chat
            .lines
            .iter()
            .skip(ui.chat.list.top_index)
            .take(chat.visible_rows)
            .enumerate()
        {
            let rgb = if line.color == u32::MAX {
                SHELL_LABEL_TEXT_RGB
            } else {
                [
                    line.color as u8,
                    (line.color >> 8) as u8,
                    (line.color >> 16) as u8,
                ]
                .map(|byte| f32::from(byte) / 255.0)
            };
            texts.push(text(
                &String::from_utf16_lossy(&line.text),
                scale.derive_rect(chat.row(visible), |rect| {
                    RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
                }),
                ShellAlign::V_CENTER,
                rgb,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        if !sliding {
            if let Some(preview) = state.frontend.skirmish_preview_texture.as_ref() {
                overlay.preview = build_preview_surface_instance(
                    layout.map_preview,
                    preview.width,
                    preview.height,
                );
                if let Some(entry) = view
                    .selected_map_index
                    .and_then(|index| state.frontend.scenario_catalog.shell_maps().get(index))
                    && !super::preview::is_random_map_sentinel_entry(entry)
                    && let Some(bounds) = entry.preview_source_bounds.as_ref()
                {
                    let fitted = aspect_fit_rect(layout.map_preview, preview.width, preview.height);
                    let positions = project_preview_start_positions(bounds, fitted);
                    push_start_marker_sprites(&mut controls, atlas, &positions, scale, 0.00056);
                    // BC5DE644/BD5E1D51 call shared DrawStartPositions640710:
                    // 6409D7..640A15 draws1-based numbers after STARTBUT, using
                    // the destination surface clip, not the thumbnail bounds.
                    let mut instances = Vec::new();
                    push_start_marker_labels(&mut instances, state, &positions, scale, 0.00040);
                    texts.push(shell_text::ShellTextDraw {
                        instances,
                        scissor: shell_text::ScissorRect {
                            x: 0,
                            y: 0,
                            w: width,
                            h: height,
                        },
                    });
                }
            }
            for button in &layout.page.buttons {
                let pressed = ui.buttons.is_pressed(button.id);
                push_right_panel_button_shp(
                    &mut art,
                    atlas,
                    button.rect,
                    crate::ui::shell::button::ButtonVisual {
                        pressed,
                        highlighted: button.id == ACCEPT && accept_highlight,
                    },
                    false,
                    SHELL_CONTROL_DEPTH,
                );
                if let Some(spec) = view.page().button(button.id) {
                    texts.push(shell_text::draw_in_rect_scaled(
                        font,
                        &localized_label(state, spec.csf_key, spec.csf_key),
                        button_text_rect(button.rect, pressed, scale),
                        super::text::button_label_color_for_disabled(!view.enabled(button.id)),
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        [0.0; 2],
                        SHELL_CONTROL_TEXT_DEPTH,
                        scale.factor(),
                    ));
                }
            }
        }
        // C2/C9 0x3ED image button (00612B70, frame type 0): +0x14 wodact
        // released, +0x18 wouact while pressed, copied 1:1 at the rect's
        // top-left with no frame or caption (0x6131D7..0x613237).
        if let Some(rect) = layout.emote
            && let (Some(released), pressed_art) = atlas.wol_emote_images()
        {
            let mut entry = if ui.buttons.is_pressed(EMOTE) {
                pressed_art.unwrap_or(released)
            } else {
                released
            };
            let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
            if art_w > 0.0 && art_h > 0.0 {
                let draw = [art_w.min(rect.w as f32), art_h.min(rect.h as f32)];
                entry.uv_size[0] *= draw[0] / art_w;
                entry.uv_size[1] *= draw[1] / art_h;
                push_entry_sized(
                    &mut controls,
                    entry,
                    rect.x as f32,
                    rect.y as f32,
                    draw,
                    SHELL_CONTROL_DEPTH,
                );
            }
        }
        texts.extend(crate::render::shell_paint::paint_labels_scaled(
            font,
            &labels,
            scale.factor(),
        ));
        if let Some(popup) = ui.popup_geometry(&view, &layout)
            && let Some(combo) = ui.popup.control.and_then(|id| view.combo(id))
        {
            let swatches = (popup.first..popup.first + popup.visible_rows).filter_map(|index| {
                Some((
                    scale.derive_rect(popup.row_rect(index)?, |row| {
                        RectPx::new(row.x + 2, row.y + 2, row.w - 4, 19)
                    }),
                    house_color_tint(&[], combo.choices.get(index)?.swatch?),
                ))
            });
            push_popup_instances(
                &mut overlay.controls,
                atlas,
                popup.rect,
                ui.popup.hot_row.and_then(|index| popup.row_rect(index)),
                swatches,
                popup.scrollbar().zip(popup.thumb()),
                ui.popup.scroll.pressed_part().map(dropdown_scroll_part),
                scale,
                SHELL_DROPDOWN_DEPTH - 0.00003,
            );
            for index in popup.first..popup.first + popup.visible_rows {
                if let Some(item) = combo.choices.get(index)
                    && item.swatch.is_none()
                    && let Some(row) = popup.row_rect(index)
                {
                    overlay.texts.push(text(
                        &item.text,
                        scale.derive_rect(row, |rect| {
                            RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
                        }),
                        ShellAlign::V_CENTER,
                        SHELL_LABEL_TEXT_RGB,
                        SHELL_DROPDOWN_TEXT_DEPTH,
                    ));
                }
            }
        }
    }
    if let Some(notice) = &ui.notice {
        // 5D3514 replaces the parent dialog with the shared empty shell backdrop.
        art.clear();
        super::modals::push_message_box_backdrop_instances(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(scale, width, height),
        );
        let modal = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
        let pressed = state.frontend.shell_controller.top_id()
            == Some(crate::ui::shell::descriptor::DialogId(0xCE))
            && state.frontend.shell_controller.pressed()
                == Some(crate::ui::shell::modal::control::OK);
        super::modals::push_validation_modal_instances(
            &mut overlay.controls,
            atlas,
            &modal,
            pressed,
        );
        super::text::push_validation_modal_text_draws(
            &mut overlay.texts,
            state,
            notice,
            &modal,
            pressed,
        );
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: match view.page().dialog.0 {
                0xBC => "LAN Host BC",
                0xBD => "LAN Guest BD",
                0xC2 => "WOL Host C2",
                _ => "WOL Guest C9",
            },
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.statics.commit_presented();
    }
    state.platform.window.request_redraw();
    Ok(())
}
