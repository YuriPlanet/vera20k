//! BC/BD/C2/C9 setup paint using the shared shell compositor and native widgets.
//! Session data is a read-only projection; rendering never changes readiness.

use super::chrome::*;
use super::controls::{
    ControlPaint, dropdown_scroll_part, house_color_tint, paint_control, push_new_edit,
    team_caption_tint,
};
use super::gui_paint::{ComboArt, GuiStyle, paint_gui_ids};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{
    aspect_fit_rect, build_preview_surface_instance, ensure_map_preview_texture,
    ensure_zoom_texture, project_preview_start_positions, push_start_marker_sprites, zoom_draws,
};
use super::text::{push_start_marker_labels, rect_to_text_rect, static_label, status_line_label};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB, SHELL_LOWER_STRIP_DEPTH,
    SHELL_PARENT_BACKGROUND_DEPTH, SHELL_SWATCH_DEPTH,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellTextDraw};
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::util::time::Instant;
use vera20k_gui::NodeId;

pub(crate) fn render_multiplayer_setup(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        crate::app::frontend::preview_zoom::close(state);
        return Ok(());
    };
    crate::app::input::multiplayer_setup::update_geometry(state);
    let layout = crate::app::input::multiplayer_setup::layout_of(state, &view);
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
    } else if !view.dialog.host {
        super::preview::ensure_received_preview_texture(state);
    } else {
        state.frontend.skirmish_preview_texture = None;
    }
    let zoom_layout = ensure_zoom_texture(state, scale, !sliding);
    let heading = state.csf_label(view.page().title_key, view.page().title_key);
    let accept_highlight = state
        .frontend
        .multiplayer_setup
        .as_mut()
        .expect("active setup")
        .accept_highlight(!view.dialog.host && view.start_requested, now);
    let controller = &mut state.frontend.shell_controller;
    if let Some(node) = controller.control(view.page().dialog, ACCEPT) {
        let lit = vera20k_gui::Value::Bool(accept_highlight);
        let _ = controller
            .gui_mut()
            .set(node, vera20k_gui::Prop::Toggled, lit);
    }
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
            (
                shown
                    .game_type
                    .filter(|_| view.combo(CAMPAIGN_SERIES).is_none()),
                layout.mode_label,
                ShellAlign::H_CENTER,
            ),
            (
                shown
                    .map_label
                    .filter(|_| view.combo(CAMPAIGN_STAGE).is_none()),
                layout.map_label,
                ShellAlign::H_CENTER,
            ),
        ]
        .into_iter()
        .filter_map(|(shown, rect, align)| shown.map(|shown| static_label(shown, rect, align)))
        .chain(status_line_label(
            shown.status_line,
            layout.page.status_help,
        ))
        .collect::<Vec<_>>()
    };
    let look = crate::app::input::multiplayer_setup::room_look(state, &view);
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
        if scale.is_unscaled() { width } else { 800 },
    ) {
        if scale.is_unscaled() {
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
    // The room's widgets paint from its trees, in the order the room drew.
    let controller = &state.frontend.shell_controller;
    let page_root = controller.root_in(view.page().dialog);
    let text_of = |id: i32| widget_text(state, &view, id);
    let room_style = GuiStyle {
        buttons: super::gui_paint::ButtonArt::Modal,
        text_of: Some(&text_of),
        ..GuiStyle::default()
    };
    let paint_ids = |root: Option<NodeId>, ids: &[u16], scale, style| {
        root.map(|root| {
            paint_gui_ids(
                state,
                controller.gui(),
                root,
                ids.iter().copied(),
                scale,
                style,
            )
        })
        .unwrap_or_default()
    };
    if !leaving && ui.notice.is_none() {
        for (key, rect) in layout
            .labels
            .iter()
            .filter(|(key, _)| view.team_header || *key != "GUI:Team")
        {
            let key = if view.campaign {
                match *key {
                    "GUI:Side" => &"VERA:CampaignRole",
                    "GUI:Color" | "GUI:StartPosition" | "GUI:Team" => continue,
                    _ => key,
                }
            } else {
                key
            };
            // Over VERA's roster, the narrow cells' labels centre on them.
            let align = if layout.roster
                && !matches!(*key, "GUI:Players" | "GUI:Side" | "VERA:CampaignRole")
            {
                ShellAlign::H_CENTER
            } else {
                ShellAlign::NONE
            };
            texts.push(sized(
                &state.csf_label(key, key),
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
            let buttons = paint_ids(page_root, &[PREVIOUS_ROWS, NEXT_ROWS], scale, room_style);
            controls.extend(buttons.controls);
            texts.extend(buttons.texts);
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
        // VERA's roster's headers: the caption where the players' names
        // stand, then Join (or Watch), text at the column's right end like
        // its cells, lit with its group's box under the pointer and grey
        // while it is closed; and a box around each group, lit while a press
        // in it joins the group or the host carries a row over it.
        let drag = row_drag(controller, &view);
        if layout.roster {
            let target = drag.and_then(|drag| drag.target);
            for (row, header) in view.headers.iter().enumerate() {
                let Some(header) = header else {
                    continue;
                };
                texts.push(sized(
                    &header.caption,
                    layout.combos[row][0],
                    ShellAlign::V_CENTER,
                    team_caption_tint(state.rules(), &header.member_colors).unwrap_or(label_rgb),
                    SHELL_CONTROL_TEXT_DEPTH,
                    roster_size,
                ));
                let id = team_join(row);
                if let Some(caption) = &header.join {
                    texts.push(sized(
                        caption,
                        layout.header_controls[row].0,
                        ShellAlign::H_RIGHT | ShellAlign::V_CENTER,
                        if !view.enabled(id) {
                            ROOM_CLOSED_TEXT_RGB
                        } else if look.pressed(id) || look.target == Some(id) {
                            [1.0, 1.0, 1.0]
                        } else {
                            ROOM_TEXT_RGB
                        },
                        SHELL_CONTROL_TEXT_DEPTH,
                        roster_size,
                    ));
                }
                let lit = target == Some(row)
                    || header.join.is_some()
                        && view.enabled(id)
                        && (look.target == Some(id) || look.pressed(id));
                if let Some(rect) = layout.group_boxes[row] {
                    push_rect_outline(
                        &mut controls,
                        atlas,
                        rect,
                        if lit { ROOM_TEXT_RGB } else { ROSTER_BOX_RGB },
                        SHELL_CONTROL_DEPTH,
                    );
                }
            }
            // The carried row's name follows the pointer, centred on it.
            if let Some(drag) = drag.filter(|drag| drag.carrying)
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
                        pressed_part: look.roster_scroll.map(dropdown_scroll_part),
                    },
                    scale,
                );
            }
        }
        // VERA's team bar under the roster, in its own box: the Teams count
        // and the host's Room size (drawn with the room's trackbars), and the
        // host's two lists (drawn with the room's combos).
        if let Some(bar) = layout.team_bar.filter(|_| view.team_bar()) {
            push_rect_outline(
                &mut controls,
                atlas,
                bar,
                ROSTER_BOX_RGB,
                SHELL_CONTROL_DEPTH,
            );
            for (id, rect, caption) in [
                (TEAM_COUNT, layout.teams_label, "VERA:Teams"),
                (ROOM_SIZE, layout.room_size_label, "VERA:RoomSize"),
            ] {
                if view.trackbars.iter().any(|bar| bar.id == id && bar.visible) {
                    texts.push(sized(
                        &crate::i18n::tr(caption),
                        rect,
                        ShellAlign::V_CENTER,
                        SHELL_LABEL_TEXT_RGB,
                        SHELL_CONTROL_TEXT_DEPTH,
                        ROSTER_TEXT_SCALE,
                    ));
                }
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
        // The rows' lists, VERA's roster's as its cells; the team bar's and a
        // campaign's lists on faces. An open list goes over the page in the
        // room's text, a grey item (one a team bar list cannot have now) in
        // the disabled colour.
        let shown: Vec<&SetupCombo> = view.combos.iter().filter(|combo| combo.visible).collect();
        let faced = |id: u16| {
            !layout.roster || matches!(id, TEAM_MENU | SEAT_MENU | CAMPAIGN_SERIES | CAMPAIGN_STAGE)
        };
        let team_bar = |id: u16| layout.roster && matches!(id, TEAM_MENU | SEAT_MENU);
        for combo in shown.iter().filter(|combo| !faced(combo.id)) {
            let Some(rect) = layout.control_rect(combo.id) else {
                continue;
            };
            let pointed =
                look.target == Some(combo.id) || look.open.is_some_and(|(id, _)| id == combo.id);
            push_roster_cell(
                &mut controls,
                &mut texts,
                state,
                &view,
                &layout,
                combo,
                rect,
                pointed,
            );
        }
        let rows_of = |id: i32| {
            let combo = view.combo(u16::try_from(id).ok()?)?;
            let row = |choice: &SetupChoice| {
                let text = choice
                    .swatch
                    .map_or_else(|| choice.text.clone(), |_| String::new());
                (text, if choice.greyed { 0x0000_009F } else { u32::MAX })
            };
            Some(combo.choices.iter().map(row).collect())
        };
        let combo_style = GuiStyle {
            combos: match layout.roster {
                true => ComboArt::Host,
                false => ComboArt::Plain,
            },
            rows_of: Some(&rows_of),
            text_factor: layout.roster.then(|| scale.factor() * ROSTER_TEXT_SCALE),
            rgb: label_rgb,
            ..room_style
        };
        let ids = |pick: &dyn Fn(u16) -> bool| -> Vec<u16> {
            shown
                .iter()
                .map(|combo| combo.id)
                .filter(|id| pick(*id))
                .collect()
        };
        // The team bar's and a campaign's lists on the shell's faces, their
        // pick in the face colour (yellow, the team bar's at the roster's
        // size), their open lists in the room's text at the roster's size.
        let face_style = GuiStyle {
            combos: ComboArt::Plain,
            ..combo_style
        };
        for (ids, style) in [
            (ids(&|id| !faced(id)), combo_style),
            (
                ids(&team_bar),
                GuiStyle {
                    face_factor: Some(scale.factor() * ROSTER_TEXT_SCALE),
                    ..face_style
                },
            ),
            (ids(&|id| faced(id) && !team_bar(id)), face_style),
        ] {
            let combos = paint_ids(page_root, &ids, scale, style);
            controls.extend(combos.controls);
            texts.extend(combos.texts);
            overlay.controls.extend(combos.popup);
            overlay.texts.extend(combos.popup_texts);
        }
        for (row, icon) in view.row_icons.iter().enumerate() {
            let Some(icon) = icon else {
                continue;
            };
            // VERA's roster names the side in text and shows no flags or
            // acceptance; a player's badge stands in front of the name,
            // shrunk to fit.
            if layout.roster {
                if let Some(entry) = icon.badge.and_then(|file| atlas.wol_badge(file))
                    && let Some(rect) = layout.badge_rect(&view, SetupColumn::Player.control(row))
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
        }
        // VERA's room keeps its options in Game Options; Difficulty stays.
        if layout.game_options.is_none() {
            let options = paint_ids(page_root, &option_ids(&view, &layout), scale, room_style);
            controls.extend(options.controls);
            texts.extend(options.texts);
        }
        // The team bar's trackbars draw at its 78% and keep their values at
        // the roster's text size.
        for bar in view.trackbars.iter().filter(|bar| {
            bar.visible
                && !OPTION_TRACKBARS.contains(&bar.id)
                && !matches!(bar.id, SUPERWEAPON_LIMIT | TECH_LEVEL)
        }) {
            let bar_scale = layout.control_scale(bar.id);
            let text_factor = (bar_scale != scale).then(|| scale.factor() * ROSTER_TEXT_SCALE);
            let style = GuiStyle {
                text_factor,
                ..room_style
            };
            let bar = paint_ids(page_root, &[bar.id], bar_scale, style);
            controls.extend(bar.controls);
            texts.extend(bar.texts);
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
        if view.cooperative || view.campaign {
            for (key, x) in [("TXT_Easy", 170), ("TXT_Normal", 295), ("TXT_Hard", 430)] {
                texts.push(text(
                    &state.csf_label(key, key),
                    scale.scale_rect(RectPx::new(x, 190, 75, 20)),
                    ShellAlign::H_CENTER,
                    SHELL_LABEL_TEXT_RGB,
                    SHELL_CONTROL_TEXT_DEPTH,
                ));
            }
        }
        // The chat edit as its widget holds it: it keeps the room's focus,
        // and its caret, under the room's boxes too.
        if let Some(node) = controller.control(view.page().dialog, CHAT) {
            let mut edit = controller.gui().edit_state(node).unwrap_or_default();
            edit.focused = controller.focused_control(view.page().dialog) == Some(CHAT);
            push_new_edit(
                &mut controls,
                &mut texts,
                atlas,
                font,
                &edit,
                state
                    .platform
                    .ime
                    .composition(crate::app::input::ime::ImeTarget::SetupChat),
                layout.chat,
                controller.caret_visible_in(view.page().dialog, node),
                label_rgb,
                SHELL_CONTROL_TEXT_DEPTH,
                scale,
            );
        }
        let chat_rows = |_| {
            let line = |line: &crate::ui::lan_shell::LanChatLine| {
                (String::from_utf16_lossy(&line.text), line.color)
            };
            Some(ui.chat.lines.iter().map(line).collect())
        };
        let chat_style = GuiStyle {
            rows_of: Some(&chat_rows),
            control_lists: true,
            text_factor: Some(scale.factor() * layout.chat_text_scale()),
            rgb: label_rgb,
            ..GuiStyle::default()
        };
        let chat = paint_ids(page_root, &[CHAT_LOG], scale, chat_style);
        controls.extend(chat.controls);
        texts.extend(chat.texts);
        if !sliding {
            if let Some(preview) = state.frontend.skirmish_preview_texture.as_ref() {
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
            let ids: Vec<u16> = view.page().buttons().map(|button| button.id).collect();
            let style = GuiStyle {
                buttons: super::gui_paint::ButtonArt::Panel,
                ..room_style
            };
            let buttons = paint_ids(page_root, &ids, scale, style);
            art.extend(buttons.controls);
            texts.extend(buttons.texts);
        }
        // C2/C9 0x3ED image button (00612B70, frame type 0): +0x14 wodact
        // released, +0x18 wouact while pressed, copied 1:1 at the rect's
        // top-left with no frame or caption (0x6131D7..0x613237).
        if let Some(rect) = layout.emote
            && let (Some(released), pressed_art) = atlas.wol_emote_images()
        {
            let mut entry = if look.pressed(EMOTE) {
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
        // VERA's Game Options over the room, on the box art the online
        // lobby's popups use: the box and its OK first, then its controls.
        if ui.game_options
            && let Some(popup) = layout.game_options
        {
            overlay
                .controls
                .extend(super::wol_lobby::online_box(atlas, popup.dialog, scale));
            let ids: Vec<u16> = [GAME_OPTIONS_OK]
                .into_iter()
                .chain(option_ids(&view, &layout))
                .collect();
            let style = GuiStyle {
                text_of: Some(&text_of),
                ..GuiStyle::popup()
            };
            let widgets = paint_ids(controller.root_in(GAME_OPTIONS_BOX), &ids, scale, style);
            overlay.controls.extend(widgets.controls);
            overlay.texts.extend(widgets.texts);
        }
        // VERA asks a guest before downloading a map it does not have, then
        // shows the download, on the art of VERA's other online-service boxes.
        if let Some(offer) = ui.map_offer.as_ref() {
            let boxed = &layout.map_offer;
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
            overlay.controls.extend(super::wol_lobby::online_box(
                atlas,
                boxed.dialog,
                crate::ui::shell::geom::RETAIL_SCALE,
            ));
            let text_of = |id: i32| match u16::try_from(id).ok()? {
                MAP_OFFER_BODY => Some(body.clone()),
                MAP_OFFER_DOWNLOAD => Some(crate::i18n::tr(offer.button_key)),
                MAP_OFFER_LEAVE => Some(state.csf_label("GUI:Leave", "Leave")),
                _ => None,
            };
            // Download greys while the map comes, but still takes a press.
            let enabled_of =
                |id: i32| (id == i32::from(MAP_OFFER_DOWNLOAD)).then_some(!offer.downloading);
            let style = GuiStyle {
                text_of: Some(&text_of),
                enabled_of: Some(&enabled_of),
                ..GuiStyle::popup()
            };
            let ids = [MAP_OFFER_BODY, MAP_OFFER_DOWNLOAD, MAP_OFFER_LEAVE];
            let widgets = paint_ids(
                controller.root_in(MAP_OFFER_BOX),
                &ids,
                crate::ui::shell::geom::RETAIL_SCALE,
                style,
            );
            overlay.controls.extend(widgets.controls);
            overlay.texts.extend(widgets.texts);
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
        let ok = look.notice_ok;
        let pressed = |id| ok && id == crate::ui::shell::modal::control::OK;
        let box_look = super::BoxLook::skirmish(atlas.mode_two_dialog_background(None));
        let size = (width as i32, height as i32);
        let (sprites, texts) = (
            Some(&mut overlay.controls),
            Some((&mut overlay.texts, false)),
        );
        super::shell_box::push_skirmish_box(
            state,
            Some(atlas),
            size,
            notice,
            pressed,
            box_look,
            sprites,
            texts,
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

/// The box around a group of VERA's roster, while a press in it does not
/// join the group.
const ROSTER_BOX_RGB: [f32; 3] = [96.0 / 255.0, 96.0 / 255.0, 96.0 / 255.0];
/// A roster row's height in 800x600 pixels, for the name a drag carries.
const ROSTER_LINE_H: f32 = 20.0;

/// VERA's room's own text: green, where the shell's is yellow, and a dim
/// green, where the shell's is dark red, on what is closed.
const ROOM_TEXT_RGB: [f32; 3] = [0.0, 0.75, 0.0];
const ROOM_CLOSED_TEXT_RGB: [f32; 3] = [0.0, 0.5, 0.0];

/// The local player's row in VERA's roster: a dark red band behind it.
const ROSTER_OWN_ROW_RGB: [f32; 3] = [90.0 / 255.0, 10.0 / 255.0, 4.0 / 255.0];

/// One cell of VERA's roster: no face, frame or arrow, only its value in
/// the roster's small text, or a colour's swatch. A cell whose list opens
/// lights white under the pointer and while its list is open.
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
    let align = match SetupColumn::decode(combo.id) {
        Some((SetupColumn::Start | SetupColumn::Team, _)) => {
            ShellAlign::H_CENTER | ShellAlign::V_CENTER
        }
        _ => ShellAlign::V_CENTER,
    };
    texts.push(shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        &item.text,
        rect_to_text_rect(rect),
        rgb,
        align,
        [0.0; 2],
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor() * ROSTER_TEXT_SCALE,
    ));
}

/// The room's own texts for its widgets: the lists' faces, the page
/// buttons' captions and the trackbars' values the room words itself.
fn widget_text(state: &AppState, view: &SetupView, id: i32) -> Option<String> {
    let id = u16::try_from(id).ok()?;
    // A list's face shows its pick, a colour as its swatch alone.
    if let Some(combo) = view.combo(id) {
        let selected = combo.selected.and_then(|row| combo.choices.get(row));
        return Some(
            selected
                .filter(|choice| choice.swatch.is_none())?
                .text
                .clone(),
        );
    }
    if let Some(spec) = view.page().button(id) {
        let key = spec.csf_key;
        let label = view.dialog.button_label(id);
        return Some(label.map_or_else(|| state.csf_label(key, key), crate::i18n::tr));
    }
    let bar = view.trackbars.iter().find(|bar| bar.id == id)?;
    Some(match id {
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
        _ => return None,
    })
}

/// The game options in the order they paint, in the room or in VERA's Game
/// Options: the trackbars' captions, the five checkboxes, in Game Options
/// its own seven and Superweapon Limit, then the trackbars shown (their
/// plaques worded by [`widget_text`]).
fn option_ids(view: &SetupView, layout: &MultiplayerSetupLayout) -> Vec<u16> {
    let mut ids: Vec<u16> = OPTION_CAPTIONS.into_iter().chain(CHECKBOXES).collect();
    // VERA's Game Options: both pages' options, the box showing one page's
    // (`multiplayer_setup::sync`).
    if layout.game_options.is_some() {
        ids.extend(SESSION_CHECKBOXES);
        ids.extend([TECH_LEVEL_CAPTION, TECH_LEVEL]);
        ids.extend(ROOM_CHECKBOXES);
        ids.extend([
            SUPERWEAPON_LIMIT_CAPTION,
            SUPERWEAPON_LIMIT,
            GAME_OPTIONS_PAGE,
        ]);
    }
    let shown = view
        .trackbars
        .iter()
        .filter(|bar| bar.visible && OPTION_TRACKBARS.contains(&bar.id));
    ids.extend(shown.map(|bar| bar.id));
    ids
}
