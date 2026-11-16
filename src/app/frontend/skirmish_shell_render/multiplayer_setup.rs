//! BC/BD/C2/C9 setup paint using the shared shell compositor and native widgets.
//! Session data is a read-only projection; rendering never changes readiness.

use super::chrome::*;
use super::controls::{
    ControlPaint, dropdown_scroll_part, house_color_tint, list_controls, paint_control,
    push_new_edit, push_popup_instances,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{
    aspect_fit_rect, build_preview_surface_instance, ensure_map_preview_texture,
    ensure_zoom_texture, project_preview_start_positions, push_start_marker_sprites, zoom_draws,
};
use super::text::{
    button_text_rect, localized_label, push_start_marker_labels, rect_to_text_rect, static_label,
    status_line_label,
};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB, SHELL_LOWER_STRIP_DEPTH,
    SHELL_PARENT_BACKGROUND_DEPTH, SHELL_SWATCH_DEPTH,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::geom::RectPx;
use crate::util::time::Instant;

pub(crate) fn render_multiplayer_setup(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    if let Some((completed, total)) = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .filter(|protocol| protocol.blocking_map_transfer())
        .and_then(|protocol| protocol.transfer_progress())
    {
        crate::app::frontend::preview_zoom::close(state);
        let (progress_meter, texts) =
            crate::app::loading::pump::transfer_progress_draws(state, completed, total)?;
        let smooth_art = state.smooth_online_menus();
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
                smooth_art,
                ..Default::default()
            },
            ShellFrameCursor::Hidden,
        );
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        crate::app::frontend::preview_zoom::close(state);
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
        ensure_map_preview_texture(state, index, layout.map_preview);
    } else if !view.host {
        super::preview::ensure_received_preview_texture(state);
    } else {
        state.frontend.skirmish_preview_texture = None;
    }
    let zoom_layout = ensure_zoom_texture(state, scale, !sliding && !view.preview_hidden);
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
            (shown.heading, layout.page.title, ShellAlign::PANEL_TITLE),
            (shown.game_type, layout.mode_label, ShellAlign::H_CENTER),
            (shown.map_label, layout.map_label, ShellAlign::H_CENTER),
        ]
        .into_iter()
        .filter_map(|(shown, rect, align)| shown.map(|shown| static_label(shown, rect, align)))
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
    // LAN 0xBB/BC/BD and WOL C2/C9 follow the persisted shell style, like
    // offline Skirmish: 72CF40's Cooperative setup art under Original YR Style,
    // the RA2 MNSCRN pair by default.
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
    if layout.roster {
        push_room_spread(
            &mut art,
            atlas,
            atlas
                .lan_background(view.page().dialog.0, 800)
                .or(atlas.generic_background_large_mnscrnl_shell),
            &chrome_layout,
        );
    }
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
    let mut overlay = ShellFrameOverlay {
        smooth_art: state.smooth_online_menus(),
        ..Default::default()
    };
    let sized =
        |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32, size: f32| {
            shell_text::draw_in_rect_scaled(
                font,
                value,
                rect_to_text_rect(rect),
                rgb,
                align,
                [0.0; 2],
                depth,
                scale.factor() * size,
            )
        };
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32| {
        sized(value, rect, align, rgb, depth, 1.0)
    };
    // VERA's roster is smaller text than the shell's own, and its room's
    // text is green.
    let roster_size = if layout.roster {
        ROSTER_TEXT_SCALE
    } else {
        1.0
    };
    let label_rgb = if layout.roster {
        ROOM_TEXT_RGB
    } else {
        SHELL_LABEL_TEXT_RGB
    };
    if !leaving && ui.notice.is_none() {
        for (key, rect) in layout
            .labels
            .iter()
            .filter(|(key, _)| view.team_header || *key != "GUI:Team")
        {
            // Over VERA's roster, the narrow cells' labels centre on them.
            let align = if layout.roster && !matches!(*key, "GUI:Players" | "GUI:Side") {
                ShellAlign::H_CENTER
            } else {
                ShellAlign::NONE
            };
            texts.push(sized(
                &localized_label(state, key, key),
                *rect,
                align,
                label_rgb,
                SHELL_CONTROL_TEXT_DEPTH,
                roster_size,
            ));
        }
        if view.pages.len() > 1 {
            // Ordinary MNBTTN buttons, as Game Options' OK; one with no page
            // to go to is hidden.
            let mut buttons = Vec::new();
            for (id, caption) in [(PREVIOUS_ROWS, "VERA:Previous"), (NEXT_ROWS, "VERA:Next")] {
                if !view.enabled(id) {
                    continue;
                }
                let rect = layout.control_rect(id).expect("roster navigation rect");
                let pressed = ui.buttons.is_pressed(id);
                buttons.push(ModalButton {
                    rect,
                    pressed,
                    enabled: true,
                });
                texts.push(text(
                    &crate::i18n::tr(caption),
                    crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed),
                    ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    SHELL_LABEL_TEXT_RGB,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            }
            controls.extend(shell_paint::paint_modal_sprites(
                None,
                type3_button_frames(atlas),
                layout.roster_page_label,
                &buttons,
                scale,
                shell_paint::ModalDepths {
                    background: SHELL_CONTROL_DEPTH,
                    button: SHELL_CONTROL_DEPTH,
                    text: SHELL_CONTROL_TEXT_DEPTH,
                },
            ));
            let page_line = crate::i18n::tr_fill(
                "VERA:RosterPage",
                &[
                    ("first", &(view.first_row + 1)),
                    ("last", &(view.first_row + view.page_len)),
                    ("total", &view.row_count),
                ],
            );
            texts.push(text(
                &page_line,
                layout.roster_page_label,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        // VERA's header rows: the caption where the players' names stand,
        // then Join (or Watch), as text in its compact roster and drawn as
        // the roster's Previous and Next otherwise.
        let mut header_buttons = Vec::new();
        for (row, header) in view.headers.iter().enumerate() {
            let Some(header) = header else {
                continue;
            };
            texts.push(sized(
                &header.caption,
                if layout.roster {
                    layout.combos[row][0]
                } else {
                    crate::ui::skirmish_shell::combo_text_rect(layout.combos[row][0], scale)
                },
                ShellAlign::V_CENTER,
                label_rgb,
                SHELL_CONTROL_TEXT_DEPTH,
                roster_size,
            ));
            let Some(caption) = &header.join else {
                continue;
            };
            let id = team_join(row);
            let rect = layout.control_rect(id).expect("header button rect");
            let enabled = view.enabled(id);
            let pressed = enabled && ui.buttons.is_pressed(id);
            // VERA's roster: Join is text at the column's right end, like
            // its cells, lit with its group's box under the pointer and grey
            // while it is closed.
            if layout.roster {
                texts.push(sized(
                    caption,
                    layout.header_controls[row].0,
                    ShellAlign::H_RIGHT | ShellAlign::V_CENTER,
                    if !enabled {
                        ROOM_CLOSED_TEXT_RGB
                    } else if pressed || ui.buttons.hovered == Some(id) {
                        [1.0, 1.0, 1.0]
                    } else {
                        ROOM_TEXT_RGB
                    },
                    SHELL_CONTROL_TEXT_DEPTH,
                    roster_size,
                ));
                continue;
            }
            header_buttons.push(ModalButton {
                rect,
                pressed,
                enabled,
            });
            texts.push(text(
                caption,
                crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed),
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                if enabled {
                    SHELL_LABEL_TEXT_RGB
                } else {
                    SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
                },
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        controls.extend(shell_paint::paint_modal_sprites(
            None,
            type3_button_frames(atlas),
            RectPx::new(0, 0, 0, 0),
            &header_buttons,
            scale,
            shell_paint::ModalDepths {
                background: SHELL_CONTROL_DEPTH,
                button: SHELL_CONTROL_DEPTH,
                text: SHELL_CONTROL_TEXT_DEPTH,
            },
        ));
        // VERA's roster: a box around each group, lit while a press in it
        // joins the group or the host carries a row over it.
        if layout.roster {
            let target = ui.row_drag.and_then(|drag| drag.target);
            for (row, header) in view.headers.iter().enumerate() {
                let (Some(header), Some(rect)) = (header, layout.group_boxes[row]) else {
                    continue;
                };
                let id = team_join(row);
                let lit = target == Some(row)
                    || header.join.is_some()
                        && view.enabled(id)
                        && (ui.buttons.hovered == Some(id) || ui.buttons.is_pressed(id));
                push_rect_outline(
                    &mut controls,
                    atlas,
                    rect,
                    if lit { ROOM_TEXT_RGB } else { ROSTER_BOX_RGB },
                    SHELL_CONTROL_DEPTH,
                );
            }
            // The carried row's name follows the pointer, centred on it.
            if let Some(drag) = ui.row_drag.filter(|drag| drag.carrying)
                && let Some(name) = view
                    .combos
                    .iter()
                    .find(|combo| {
                        combo.row == drag.row
                            && matches!(
                                SetupColumn::decode(combo.id),
                                Some((SetupColumn::Player, _))
                            )
                    })
                    .and_then(|combo| combo.choices.get(combo.selected?))
            {
                let (x, y) = drag.at;
                let h = (ROSTER_LINE_H * scale.factor()).round() as i32;
                texts.push(sized(
                    &name.text,
                    RectPx::new(x - 10 * h, y - h / 2, 20 * h, h),
                    ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    ROOM_TEXT_RGB,
                    SHELL_CONTROL_TEXT_DEPTH,
                    ROSTER_TEXT_SCALE,
                ));
            }
            // The shell's own scrollbar, when the groups do not all fit.
            if let Some(roster) = layout.roster_scroll(&view)
                && let (Some(scrollbar), Some(thumb)) = (roster.scrollbar, roster.thumb)
            {
                paint_control(
                    &mut controls,
                    &atlas.control_chrome(),
                    ControlPaint::ScrollBar {
                        scrollbar,
                        thumb,
                        pressed_part: ui.roster_scroll_pressed().map(dropdown_scroll_part),
                    },
                    scale,
                );
            }
        }
        // VERA's team bar under the roster, in its own box: the Teams count
        // and the host's Room size (drawn with the room's trackbars), Team
        // Starts, Team Colors and, online, Balance.
        if let Some(bar) = layout.team_bar.filter(|_| view.team_bar()) {
            push_rect_outline(
                &mut controls,
                atlas,
                bar,
                ROSTER_BOX_RGB,
                SHELL_CONTROL_DEPTH,
            );
            // Its art at 78% size, its words at the roster's.
            let small = layout.control_scale(BALANCE);
            for (id, rect, caption) in [
                (TEAM_COUNT, layout.teams_label, "VERA:Teams"),
                (ROOM_SIZE, layout.room_size_label, "VERA:RoomSize"),
            ] {
                if view.trackbars.iter().any(|bar| bar.id == id && bar.visible) {
                    texts.push(sized(
                        &crate::i18n::tr(caption),
                        rect,
                        ShellAlign::V_CENTER,
                        ROOM_TEXT_RGB,
                        SHELL_CONTROL_TEXT_DEPTH,
                        ROSTER_TEXT_SCALE,
                    ));
                }
            }
            for (id, rect, caption) in [
                (TEAM_STARTS, layout.team_starts, "VERA:TeamStarts"),
                (TEAM_COLORS, layout.team_colors, "VERA:TeamColors"),
                (BALANCE, layout.balance, "VERA:Balance"),
            ] {
                let Some(rect) = rect else {
                    continue;
                };
                let enabled = view.enabled(id);
                let pressed = enabled && ui.buttons.is_pressed(id);
                controls.extend(shell_paint::paint_modal_sprites(
                    None,
                    type3_button_frames(atlas),
                    rect,
                    &[ModalButton {
                        rect,
                        pressed,
                        enabled,
                    }],
                    small,
                    shell_paint::ModalDepths {
                        background: SHELL_CONTROL_DEPTH,
                        button: SHELL_CONTROL_DEPTH,
                        text: SHELL_CONTROL_TEXT_DEPTH,
                    },
                ));
                texts.push(sized(
                    &crate::i18n::tr(caption),
                    rect,
                    ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    super::text::button_label_color_for_disabled(!enabled),
                    SHELL_CONTROL_TEXT_DEPTH,
                    ROSTER_TEXT_SCALE,
                ));
            }
        }
        // VERA's roster marks the local player's row with a dark band.
        if layout.roster {
            for (row, icon) in view.row_icons.iter().enumerate() {
                if icon.as_ref().is_some_and(|icon| icon.local) {
                    push_solid_rect(
                        &mut controls,
                        atlas,
                        layout.roster_rows[row],
                        ROSTER_OWN_ROW_RGB,
                        SHELL_CONTROL_DEPTH + 0.00001,
                    );
                }
            }
        }
        for combo in view.combos.iter().filter(|combo| combo.visible) {
            let Some(rect) = layout.control_rect(combo.id) else {
                continue;
            };
            let selected = combo.selected.and_then(|index| combo.choices.get(index));
            if layout.roster {
                push_roster_cell(
                    &mut controls,
                    &mut texts,
                    state,
                    &view,
                    &layout,
                    combo,
                    rect,
                    ui.buttons.hovered == Some(combo.id) || ui.popup.control == Some(combo.id),
                );
                continue;
            }
            paint_control(
                &mut controls,
                &atlas.control_chrome(),
                ControlPaint::Combo {
                    rect,
                    swatch: selected
                        .and_then(|item| item.swatch)
                        .map(|index| house_color_tint(state.rules(), index)),
                    open: ui.popup.control == Some(combo.id),
                    disabled: !view.enabled(combo.id),
                },
                scale,
            );
            if let Some(item) = selected
                && item.swatch.is_none()
            {
                let mut face = crate::ui::skirmish_shell::combo_text_rect(rect, scale);
                // VERA's online room: the name stops short of its badge.
                if let Some((_, badge)) = player_badge(atlas, &view, &layout, combo.id) {
                    face.w = badge.x - face.x;
                }
                texts.push(text(
                    &item.text,
                    face,
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
            // VERA's roster names the side in text and shows no flags or
            // acceptance; its badges shrink to the row.
            if layout.roster {
                if let Some((entry, rect)) =
                    player_badge(atlas, &view, &layout, SetupColumn::Player.control(row))
                {
                    push_entry_fitted(&mut controls, entry, rect, scale, SHELL_SWATCH_DEPTH);
                }
                continue;
            }
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
            // VERA's online room: the badge over the name's face, as the
            // colour list's swatch.
            if let Some((entry, rect)) =
                player_badge(atlas, &view, &layout, SetupColumn::Player.control(row))
            {
                push_flag_entry_clipped_centered(
                    &mut controls,
                    entry,
                    rect,
                    scale,
                    SHELL_SWATCH_DEPTH,
                );
            }
        }
        // C2/C9 ping statics 0x754+row, kind-2 images like the acceptance ones.
        if let Some(rects) = &layout.ping_icons {
            for (row, icon) in view.ping_icons.iter().enumerate() {
                if let Some(entry) = icon.and_then(|icon| atlas.wol_ping_icon(icon)) {
                    if layout.roster {
                        push_entry_fitted(
                            &mut controls,
                            entry,
                            rects[row],
                            scale,
                            SHELL_CONTROL_DEPTH,
                        );
                    } else {
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
        }
        // VERA's room keeps its options in Game Options; Difficulty stays.
        if layout.game_options.is_none() {
            push_option_controls(
                &mut controls,
                &mut texts,
                state,
                &view,
                &layout,
                SHELL_CONTROL_TEXT_DEPTH,
            );
        }
        for bar in view.trackbars.iter().filter(|bar| {
            bar.visible && !OPTION_TRACKBARS.contains(&bar.id) && bar.id != SUPERWEAPON_LIMIT
        }) {
            push_trackbar(
                &mut controls,
                &mut texts,
                state,
                &view,
                &layout,
                bar,
                SHELL_CONTROL_TEXT_DEPTH,
            );
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
        push_new_edit(
            &mut controls,
            &mut texts,
            atlas,
            font,
            &ui.chat.edit,
            state
                .platform
                .ime
                .composition(crate::app::input::ime::ImeTarget::SetupChat),
            layout.chat,
            ui.caret_visible(now),
            label_rgb,
            SHELL_CONTROL_TEXT_DEPTH,
            scale,
        );
        let chat = layout.chat_list(&ui.chat.list, ui.chat.lines.len());
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
                label_rgb
            } else {
                [
                    line.color as u8,
                    (line.color >> 8) as u8,
                    (line.color >> 16) as u8,
                ]
                .map(|byte| f32::from(byte) / 255.0)
            };
            texts.push(sized(
                &String::from_utf16_lossy(&line.text),
                scale.derive_rect(chat.row(visible), |rect| {
                    RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
                }),
                ShellAlign::V_CENTER,
                rgb,
                SHELL_CONTROL_TEXT_DEPTH,
                layout.chat_text_scale(),
            ));
        }
        // C9's 0x71F "Preview unavailable" after the host's Z3 (0x5E3D10),
        // a left-aligned static at its template place.
        if !sliding
            && view.no_preview
            && let Some(rect) = layout.no_preview
        {
            texts.push(text(
                &localized_label(state, "TXT_NAT_NO_PREVIEW", "Preview unavailable"),
                rect,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        if !sliding {
            if let Some(preview) = state
                .frontend
                .skirmish_preview_texture
                .as_ref()
                .filter(|_| !view.preview_hidden)
            {
                overlay.preview = build_preview_surface_instance(
                    layout.map_preview,
                    preview.width,
                    preview.height,
                );
                // A received preview's markers are the host's 'W' pairs over
                // its bounds, drawn when 1 <= count <= 8 (0x00640710).
                let received_bounds = (preview.received_preview.is_some())
                    .then(|| {
                        state
                            .frontend
                            .multiplayer_shell_runtime
                            .start_staging()
                            .preview_source_bounds()
                    })
                    .flatten();
                // The host's RandMap sentinel numbers the last Generate's
                // starts, copied back by 0x0058BB30 (0x005E67ED).
                let entry = view
                    .selected_map_index
                    .and_then(|index| state.frontend.scenario_catalog.shell_maps().get(index));
                let own_bounds = super::preview::start_marker_bounds(
                    entry
                        .is_some_and(super::preview::is_random_map_sentinel_entry)
                        .then(|| state.frontend.random_map_retention.generated_header()),
                    entry.and_then(|entry| entry.preview_source_bounds.as_ref()),
                );
                let bounds = state
                    .frontend
                    .network_setup_protocol
                    .as_ref()
                    .and_then(|protocol| protocol.received_preview_bounds())
                    .or(received_bounds.as_ref())
                    .or(own_bounds.as_ref());
                overlay.zoom = zoom_layout
                    .as_ref()
                    .map(|zoom_layout| zoom_draws(state, atlas, zoom_layout, bounds, scale));
                if let Some(bounds) = bounds {
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
                        &view.dialog.button_label(button.id).map_or_else(
                            || localized_label(state, spec.csf_key, spec.csf_key),
                            crate::i18n::tr,
                        ),
                        button_text_rect(button.rect, pressed, scale),
                        super::text::button_label_color_for_disabled(!view.enabled(button.id)),
                        ShellAlign::BUTTON_CAPTION,
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
                        RectPx::new(row.x + 2, row.y + 2, row.w - 4, row.h - 4)
                    }),
                    house_color_tint(state.rules(), combo.choices.get(index)?.swatch?),
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
                    overlay.texts.push(sized(
                        &item.text,
                        scale.derive_rect(row, |rect| {
                            RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
                        }),
                        ShellAlign::V_CENTER,
                        label_rgb,
                        SHELL_DROPDOWN_TEXT_DEPTH,
                        roster_size,
                    ));
                }
            }
        }
        // VERA's Game Options over the room, on the box art the online
        // lobby's popups use: the box and its OK first, then its controls.
        if view.open_box.is_some()
            && let Some(popup) = layout.game_options
        {
            let button = |id: u16, rect: RectPx, text: String| {
                let pressed = ui.buttons.is_pressed(id);
                let enabled = view.enabled(id);
                let label = PaintLabel {
                    text: text.into(),
                    rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                        rect, pressed,
                    ),
                    align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                    rgb: super::text::button_label_color_for_disabled(!enabled),
                    path_a_reveal: None,
                };
                (
                    ModalButton {
                        rect,
                        pressed,
                        enabled,
                    },
                    label,
                )
            };
            let (buttons, labels): (Vec<_>, Vec<_>) = [button(
                GAME_OPTIONS_OK,
                popup.ok,
                localized_label(state, "GUI:OK", "GUI:OK"),
            )]
            .into_iter()
            .unzip();
            let draw = shell_paint::paint_modal_shp(
                font,
                atlas
                    .ra2_dialog_background
                    .or_else(|| atlas.mode_two_dialog_background(None)),
                type3_button_frames(atlas),
                popup.dialog,
                &buttons,
                &labels,
                scale,
                super::wol_lobby::POPUP_DEPTHS,
            );
            overlay.controls.extend(draw.sprites);
            overlay.texts.extend(draw.text);
            push_option_controls(
                &mut overlay.controls,
                &mut overlay.texts,
                state,
                &view,
                &layout,
                super::wol_lobby::POPUP_DEPTHS.text,
            );
        }
        // VERA asks a guest before downloading a map it does not have, then
        // shows the download, on the art of VERA's other online-service boxes.
        if view.map_offer_open
            && let Some(offer) = ui.map_offer.as_ref()
        {
            use crate::ui::shell::geom::ShellScale;
            let boxed = layout.map_offer;
            let pressed = |id| ui.buttons.is_pressed(id);
            let progress = offer
                .downloading
                .then(|| {
                    state
                        .frontend
                        .network_setup_protocol
                        .as_ref()
                        .and_then(|protocol| protocol.transfer_progress())
                })
                .flatten()
                .map(|(completed, total)| (u64::from(completed), u64::from(total)));
            let body = match progress {
                Some((completed, total)) => crate::i18n::tr_fill(
                    "VERA:DownloadingMapPercent",
                    &[
                        ("name", &offer.name),
                        ("percent", &(completed.min(total) * 100 / total.max(1))),
                    ],
                ),
                None if offer.downloading => {
                    crate::i18n::tr_fill("VERA:DownloadingMap", &[("name", &offer.name)])
                }
                None => offer.message.clone(),
            };
            let button_label = |text: String, rect: RectPx, pressed: bool, rgb| PaintLabel {
                text: text.into(),
                rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed),
                align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                rgb,
                path_a_reveal: None,
            };
            let labels = [
                PaintLabel {
                    text: body.into(),
                    rect: boxed.body,
                    align: super::text::validation_modal_body_text_align(),
                    rgb: SHELL_LABEL_TEXT_RGB,
                    path_a_reveal: None,
                },
                button_label(
                    crate::i18n::tr("VERA:Download"),
                    boxed.ok,
                    pressed(MAP_OFFER_DOWNLOAD),
                    if offer.downloading {
                        SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
                    } else {
                        SHELL_LABEL_TEXT_RGB
                    },
                ),
                button_label(
                    localized_label(state, "GUI:Leave", "Leave"),
                    boxed.cancel,
                    pressed(MAP_OFFER_LEAVE),
                    SHELL_LABEL_TEXT_RGB,
                ),
            ];
            let draw = shell_paint::paint_modal_shp(
                font,
                atlas
                    .ra2_dialog_background
                    .or_else(|| atlas.mode_two_dialog_background(None)),
                type3_button_frames(atlas),
                boxed.dialog,
                &[
                    ModalButton {
                        rect: boxed.ok,
                        pressed: pressed(MAP_OFFER_DOWNLOAD),
                        enabled: !offer.downloading,
                    },
                    ModalButton {
                        rect: boxed.cancel,
                        pressed: pressed(MAP_OFFER_LEAVE),
                        enabled: true,
                    },
                ],
                &labels,
                ShellScale::RETAIL,
                super::wol_lobby::POPUP_DEPTHS,
            );
            overlay.controls.extend(draw.sprites);
            overlay.texts.extend(draw.text);
            if let Some(progress) = progress {
                super::modals::push_download_progress_bar(
                    &mut overlay.controls,
                    atlas,
                    boxed.body,
                    progress,
                    super::wol_lobby::POPUP_DEPTHS.button,
                );
            }
        }
    }
    if let Some(notice) = &ui.notice {
        // 5D3514 replaces the parent dialog with the shared empty shell
        // backdrop, centred under the box as on every other page, VERA's
        // room's too.
        art.clear();
        super::modals::push_message_box_backdrop_instances(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(scale.centred(), width, height),
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
    // The page dialog 0xCF over C2/C9 (0x0077D820 keeps the page painting).
    if let Some((_, page)) = crate::app::frontend::wol_welcome_render::page_dialogs_paint(state) {
        overlay.controls.extend(page.controls);
        overlay.texts.extend(page.texts);
    }
    // Start Game Now's 0x130 over the locked C2/C9; VERA's room waits a
    // moment so a quick check does not flash it.
    let verify = crate::app::shell_network_setup::verify(state);
    if verify.is_some_and(|verify| !verify.shown(view.dialog.modern, now)) {
        state.platform.window.request_redraw();
    }
    if let Some(verify) = verify.filter(|verify| verify.shown(view.dialog.modern, now)) {
        let paint = super::paint_wol_verify(
            state,
            verify,
            &crate::app::shell_network_setup::verify_layout(state),
        );
        overlay.controls.extend(paint.controls);
        overlay.texts.extend(paint.texts);
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
    state.platform.window.request_redraw();
    Ok(())
}

/// VERA's online room: a Player list's badge ([`SetupRowIcon::badge`]) and
/// its cell, one badge wide at the right end of the list's face, as far
/// short of the arrow as the name starts in from the left.
/// VERA's room on a window wider than the shell, which stands at its right
/// edge: the background's art left of the right panel and the lower strip,
/// stretched across from the window's left edge, under the room's columns.
fn push_room_spread(
    art: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    background: Option<SkirmishShellChromeEntry>,
    chrome: &crate::ui::skirmish_shell::SkirmishShellLayout,
) {
    let dialog = chrome.scale.dialog_rect();
    if dialog.x <= 0 || dialog.w <= 0 {
        return;
    }
    let panel_x = chrome.right_panel.top.x;
    if let Some(background) = background {
        // The frame down the art's left edge keeps its own width, and the
        // art from there to the panel stretches over the rest.
        let art_w = background.pixel_size[0].max(1.0);
        let frame_w = ROOM_FRAME_W as f32 * chrome.scale.factor();
        let mut frame = background;
        frame.uv_size[0] *= ROOM_FRAME_W as f32 / art_w;
        let mut rest = background;
        rest.uv_origin[0] += frame.uv_size[0];
        rest.uv_size[0] = background.uv_size[0] * (panel_x - dialog.x) as f32 / dialog.w as f32
            - frame.uv_size[0];
        for (entry, x, w) in [
            (frame, 0.0, frame_w),
            (rest, frame_w, panel_x as f32 - frame_w),
        ] {
            push_entry_sized(
                art,
                entry,
                x,
                dialog.y as f32,
                [w, dialog.h as f32],
                SHELL_PARENT_BACKGROUND_DEPTH - 0.000002,
            );
        }
    }
    if let Some(strip) = lower_strip_entry(atlas, chrome) {
        let rect = lower_strip_rect(chrome, strip);
        push_entry_sized(
            art,
            strip,
            0.0,
            rect.y as f32,
            [(rect.x + rect.w) as f32, rect.h as f32],
            SHELL_LOWER_STRIP_DEPTH - 0.000001,
        );
    }
}

fn player_badge(
    atlas: &SkirmishShellChromeAtlas,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    control: u16,
) -> Option<(SkirmishShellChromeEntry, RectPx)> {
    use crate::ui::skirmish_shell::{COMBO_ARROW_RESERVE_W, COMBO_FACE_H, COMBO_TEXT_LEFT_INSET};
    let (SetupColumn::Player, row) = SetupColumn::decode(control)? else {
        return None;
    };
    let entry = atlas.wol_badge(view.row_icons.get(row)?.as_ref()?.badge?)?;
    let [art_w, art_h] = entry.pixel_size;
    let cell = layout
        .page
        .scale
        .derive_rect(layout.control_rect(control)?, |rect| {
            if layout.roster {
                // At the right end of the name, shrunk to the row.
                let width = (art_w * (rect.h as f32 / art_h.max(1.0)).min(1.0)).round() as i32;
                return RectPx::new(rect.x + rect.w - width, rect.y, width, rect.h);
            }
            let width = art_w.round() as i32;
            let right = rect.x + rect.w - COMBO_ARROW_RESERVE_W - COMBO_TEXT_LEFT_INSET;
            RectPx::new(right - width, rect.y, width, COMBO_FACE_H)
        });
    Some((entry, cell))
}

/// The box around a group of VERA's roster, while a press in it does not
/// join the group.
const ROSTER_BOX_RGB: [f32; 3] = [96.0 / 255.0, 96.0 / 255.0, 96.0 / 255.0];
/// A roster row's height in 800x600 pixels, for the name a drag carries.
const ROSTER_LINE_H: f32 = 20.0;

/// VERA's room's own text: green, where the shell's is yellow, and a dim
/// green, where the shell's is dark red, on what is closed.
const ROOM_TEXT_RGB: [f32; 3] = [0.0, 1.0, 0.0];
const ROOM_CLOSED_TEXT_RGB: [f32; 3] = [0.0, 0.5, 0.0];

/// The local player's row in VERA's roster: a dark red band behind it.
const ROSTER_OWN_ROW_RGB: [f32; 3] = [90.0 / 255.0, 10.0 / 255.0, 4.0 / 255.0];

/// One cell of VERA's roster: no face, frame or arrow, only its value in
/// the roster's small text, or a colour's swatch. A cell whose list opens
/// lights white under the pointer and while its list is open.
#[allow(clippy::too_many_arguments)]
fn push_roster_cell(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    state: &AppState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    combo: &SetupCombo,
    rect: RectPx,
    pointed: bool,
) {
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return;
    };
    let Some(item) = combo.selected.and_then(|index| combo.choices.get(index)) else {
        return;
    };
    let scale = layout.page.scale;
    let lit = pointed && view.enabled(combo.id) && roster_cell_opens(combo);
    let rgb = if lit { [1.0, 1.0, 1.0] } else { ROOM_TEXT_RGB };
    if let Some(swatch) = item.swatch {
        let chip = scale.derive_rect(rect, |rect| {
            RectPx::new(
                rect.x + (rect.w - 12) / 2,
                rect.y + (rect.h - 12) / 2,
                12,
                12,
            )
        });
        push_solid_rect(
            controls,
            atlas,
            chip,
            house_color_tint(state.rules(), swatch),
            SHELL_SWATCH_DEPTH,
        );
        if lit {
            let ring = scale.derive_rect(chip, |rect| {
                RectPx::new(rect.x - 1, rect.y - 1, rect.w + 2, rect.h + 2)
            });
            push_rect_outline(controls, atlas, ring, rgb, SHELL_SWATCH_DEPTH);
        }
        return;
    }
    let mut face = rect;
    // VERA's online room: the name stops short of its badge.
    if let Some((_, badge)) = player_badge(atlas, view, layout, combo.id) {
        face.w = badge.x - face.x;
    }
    let align = match SetupColumn::decode(combo.id) {
        Some((SetupColumn::Start | SetupColumn::Team, _)) => {
            ShellAlign::H_CENTER | ShellAlign::V_CENTER
        }
        _ => ShellAlign::V_CENTER,
    };
    texts.push(shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        &item.text,
        rect_to_text_rect(face),
        rgb,
        align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor() * ROSTER_TEXT_SCALE,
    ));
}

/// The game options, in the room or in VERA's Game Options popup: their
/// labels, the five checkboxes and the trackbars of [`OPTION_TRACKBARS`].
fn push_option_controls(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    state: &AppState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    text_depth: f32,
) {
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return;
    };
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, align: ShellAlign| {
        shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            [0.0; 2],
            text_depth,
            scale.factor(),
        )
    };
    for (key, rect) in &layout.option_labels {
        texts.push(text(
            &localized_label(state, key, key),
            *rect,
            ShellAlign::NONE,
        ));
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
            controls,
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
        ));
    }
    // VERA's own checkboxes, only in its Game Options.
    if layout.game_options.is_some() {
        for (index, label) in crate::ui::multiplayer_setup::ROOM_CHECKBOX_LABELS
            .into_iter()
            .enumerate()
        {
            let rect = layout.room_checkboxes[index];
            paint_control(
                controls,
                &atlas.control_chrome(),
                ControlPaint::Checkbox {
                    rect,
                    checked: view.room_checks[index],
                },
                scale,
            );
            texts.push(text(
                &crate::i18n::tr(label),
                crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
                ShellAlign::V_CENTER,
            ));
        }
        texts.push(text(
            &crate::i18n::tr(crate::ui::skirmish_shell::game_options::SUPERWEAPON_LIMIT_LABEL),
            layout.superweapon_limit_label,
            ShellAlign::NONE,
        ));
        if let Some(bar) = view
            .trackbars
            .iter()
            .find(|bar| bar.id == SUPERWEAPON_LIMIT)
        {
            push_trackbar(controls, texts, state, view, layout, bar, text_depth);
        }
    }
    for bar in view
        .trackbars
        .iter()
        .filter(|bar| bar.visible && OPTION_TRACKBARS.contains(&bar.id))
    {
        push_trackbar(controls, texts, state, view, layout, bar, text_depth);
    }
}

/// One setup trackbar, with its value plaque unless it is Difficulty.
fn push_trackbar(
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
    state: &AppState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    bar: &SetupTrackbar,
    text_depth: f32,
) {
    let (Some(atlas), Some(rect)) = (
        state.frontend.skirmish_shell_chrome.as_ref(),
        layout.control_rect(bar.id),
    ) else {
        return;
    };
    let scale = layout.control_scale(bar.id);
    // The team bar's count keeps its value at the roster's text size and
    // in the room's colours.
    let (text_factor, [rgb, closed]) = if scale == layout.page.scale {
        (
            scale.factor(),
            [
                SHELL_LABEL_TEXT_RGB,
                SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
            ],
        )
    } else {
        (
            layout.page.scale.factor() * ROSTER_TEXT_SCALE,
            [ROOM_TEXT_RGB, ROOM_CLOSED_TEXT_RGB],
        )
    };
    paint_control(
        controls,
        &atlas.control_chrome(),
        ControlPaint::Trackbar {
            rect,
            thumb_left: bar.thumb_left(rect, scale),
            plaque: bar.id != DIFFICULTY,
            // A guest's are disabled windows (`0x005E30EB..0x005E30F4`).
            disabled: !view.enabled(bar.id),
        },
        scale,
    );
    if bar.id != DIFFICULTY {
        let value = match bar.id {
            SPEED => {
                let context = crate::ui::shell::game_speed::SpeedContext::Network;
                context.value_text(context.stored_speed(bar.value))
            }
            CREDITS => state
                .frontend
                .skirmish_shell_state
                .trackbar_bounds
                .credits_from_visual_position(bar.value)
                .to_string(),
            // 1 is Teams Off.
            TEAM_COUNT if bar.value < 2 => crate::i18n::tr("VERA:TeamsOff"),
            SUPERWEAPON_LIMIT => crate::ui::skirmish_shell::game_options::superweapon_limit_text(
                crate::rules::personal_rules::admitted_superweapon_limit(bar.value),
            ),
            _ => bar.range.stepped(bar.value).to_string(),
        };
        texts.push(shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            &value,
            rect_to_text_rect(crate::ui::shell::trackbar::value_text_rect(rect, scale)),
            if view.enabled(bar.id) { rgb } else { closed },
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            [0.0; 2],
            text_depth,
            text_factor,
        ));
    }
}
