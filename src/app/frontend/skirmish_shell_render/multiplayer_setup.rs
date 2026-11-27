//! BC/BD/C2/C9 setup paint using the shared shell compositor and native widgets.
//! Session data is a read-only projection; rendering never changes readiness.

use super::chrome::*;
use super::controls::team_caption_tint;
use super::gui_paint::{
    ButtonArt, ComboArt, FrameArt, GuiStyle, Picture, paint_gui_ids, paint_gui_styled,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::preview::{
    aspect_fit_rect, build_preview_surface_instance, ensure_map_preview_texture,
    ensure_zoom_texture, project_preview_start_positions, push_start_marker_sprites, zoom_draws,
};
use super::text::{push_start_marker_labels, static_label, status_line_label};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB, SHELL_LOWER_STRIP_DEPTH,
    SHELL_PARENT_BACKGROUND_DEPTH,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::hud_paint::Material;
use crate::render::shell_text;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::skirmish_shell::SkirmishShellState;
use crate::util::time::Instant;
use vera20k_gui::NodeId;

pub(crate) fn render_multiplayer_setup(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let Some(view) =
        crate::app::shell_network_setup::view(renderer, frontend, match_state, process_assets)
    else {
        crate::app::frontend::preview_zoom::close(&platform.window, frontend);
        return Ok(());
    };
    crate::app::input::multiplayer_setup::update_geometry(
        renderer,
        frontend,
        match_state,
        process_assets,
    );
    crate::app::shell_network_setup::verify_sync(&renderer.gpu, frontend, match_state);
    let layout = crate::app::input::multiplayer_setup::layout_of(
        &renderer.gpu,
        frontend,
        match_state,
        &view,
    )
    .with_touch_send(platform.touch_send);
    let scale = layout.page.scale;
    let (width, height) = (
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
    );
    // The notice on its widgets before it paints.
    let notice_parts = (frontend.multiplayer_setup.as_ref())
        .and_then(|ui| ui.notice.as_ref())
        .map(|notice| super::shell_box::skirmish_parts(notice, (width as i32, height as i32)));
    {
        let notice = frontend
            .multiplayer_setup
            .as_ref()
            .and_then(|ui| ui.notice.as_ref());
        if let (Some(notice), Some(parts)) = (notice, notice_parts.as_ref()) {
            let controller = &mut frontend.shell_controller;
            if let Some(root) = controller.root_in(notice.dialog) {
                notice.sync(controller.gui_mut(), root, parts);
            }
        }
    }
    let now = Instant::now();
    let exit = crate::app::frontend::shell_transition::shell_exit_wave(
        &frontend.shell_exit,
        ShellSlideKind::NetworkSetup(view.dialog),
    )
    .cloned();
    let leaving = exit.is_some();
    let wave = exit.or_else(|| frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    if let Some(index) = view.selected_map_index {
        ensure_map_preview_texture(
            platform,
            renderer,
            frontend,
            process_assets,
            persistence,
            index,
            layout.map_preview,
        );
    } else if !view.dialog.host {
        super::preview::ensure_received_preview_texture(renderer, frontend);
    } else {
        frontend.skirmish_preview_texture = None;
    }
    let zoom_layout = ensure_zoom_texture(
        platform,
        renderer,
        frontend,
        &match_state.input,
        process_assets,
        scale,
        !sliding,
    );
    let heading = process_assets.csf_label(view.page().title_key, view.page().title_key);
    let accept_highlight = frontend
        .multiplayer_setup
        .as_mut()
        .expect("active setup")
        .accept_highlight(!view.dialog.host && view.start_requested, now);
    let controller = &mut frontend.shell_controller;
    if let Some(node) = controller.control(view.page().dialog, ACCEPT) {
        let lit = vera20k_gui::Value::Bool(accept_highlight);
        let _ = controller
            .gui_mut()
            .set(node, vera20k_gui::Prop::Toggled, lit);
    }
    let labels = {
        let ui = frontend.multiplayer_setup.as_mut().expect("active setup");
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
    let look = crate::app::input::multiplayer_setup::room_look(frontend, &match_state.input, &view);
    let ui = frontend.multiplayer_setup.as_ref().expect("active setup");
    let atlas = frontend.skirmish_shell_chrome.as_ref().expect("setup art");
    let font = &renderer.bit_font;
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
        smooth_art: persistence.smooth_online_menus(frontend.native_shell_ui),
        ..Default::default()
    };
    let sized =
        |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32, size: f32| {
            shell_text::draw_in_rect_scaled(
                font,
                value,
                shell_text::TextRect::of(rect),
                rgb,
                align,
                [0.0; 2],
                depth,
                scale.factor() * size,
            )
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
    let controller = &frontend.shell_controller;
    let page_root = controller.root_in(view.page().dialog);
    let text_of = |id: i32| {
        widget_text(
            &frontend.skirmish_shell_state,
            process_assets,
            &view,
            &layout,
            id,
        )
    };
    let room_style = GuiStyle {
        buttons: ButtonArt::Modal,
        text_of: Some(&text_of),
        ..GuiStyle::default()
    };
    // The room's own looks: its texts' and cells' colours, its boxes and
    // bands, and its pictures.
    let drag = row_drag(controller, &view);
    let rgb_of = |id: i32| room_rgb(frontend, match_state, &view, &look, id);
    let frame_of = |id: i32| room_frame(&view, &look, drag, id);
    let picture_of = |id: i32| room_picture(&view, id);
    let frames = GuiStyle {
        frame_of: Some(&frame_of),
        ..room_style
    };
    let paint_ids = |root: Option<NodeId>, ids: &[u16], scale, style| {
        root.map(|root| {
            paint_gui_ids(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                controller.gui(),
                root,
                ids.iter().copied(),
                scale,
                style,
            )
        })
        .unwrap_or_default()
    };
    // The room's own nodes, found when its page was built.
    let nodes = Some(ui.room_nodes.get())
        .filter(|nodes| page_root.is_some() && nodes.root == page_root)
        .unwrap_or_default();
    let paint_node = |node: Option<NodeId>, style| {
        node.map(|node| {
            paint_gui_styled(
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                controller.gui(),
                node,
                scale,
                &[],
                style,
            )
        })
        .unwrap_or_default()
    };
    // The column labels and the roster's headers in the room's text, at
    // the roster's size over VERA's roster.
    let room_text = GuiStyle {
        text_factor: Some(scale.factor() * roster_size),
        rgb: label_rgb,
        rgb_of: Some(&rgb_of),
        ..room_style
    };
    if !leaving && ui.notice.is_none() {
        let statics = paint_node(nodes.labels, room_text);
        texts.extend(statics.texts);
        if view.pages.len() > 1 {
            // Ordinary MNBTTN buttons, as Game Options' OK; one with no page
            // to go to is hidden. The line between them names the page.
            let ids = [PREVIOUS_ROWS, NEXT_ROWS];
            let buttons = paint_ids(page_root, &ids, scale, room_style);
            controls.extend(buttons.lane(Material::Controls).copied());
            texts.extend(buttons.texts);
            texts.extend(paint_node(nodes.page_line, room_style).texts);
        }
        // VERA's roster's headers: the caption where the players' names
        // stand, then Join (or Watch), text at the column's right end like
        // its cells, lit with its group's box under the pointer and grey
        // while it is closed; and a box around each group, lit while a press
        // in it joins the group or the host carries a row over it.
        if layout.roster {
            let headers = paint_node(nodes.headers, room_text);
            texts.extend(headers.texts);
            // A box shows round each group with a header.
            let boxes = paint_node(nodes.boxes, frames);
            controls.extend(boxes.lane(Material::Controls).copied());
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
            let bar = paint_node(nodes.scroll, frames);
            controls.extend(bar.lane(Material::Controls).copied());
        }
        // VERA's team bar under the roster, in its own box: the Teams count
        // and the host's Room size (drawn with the room's trackbars), and the
        // host's two lists (drawn with the room's combos).
        let team_bar = GuiStyle {
            text_factor: Some(scale.factor() * ROSTER_TEXT_SCALE),
            ..frames
        };
        let team_bar = paint_node(nodes.team_bar, team_bar);
        controls.extend(team_bar.lane(Material::Controls).copied());
        texts.extend(team_bar.texts);
        // VERA's roster marks the local player's row with a dark band.
        if layout.roster {
            let bands = paint_node(nodes.bands, frames);
            controls.extend(bands.lane(Material::Controls).copied());
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
        // VERA's roster's cells show only their value or swatch.
        let combo_style = GuiStyle {
            combos: match layout.roster {
                true => ComboArt::Cell,
                false => ComboArt::Plain,
            },
            rows_of: Some(&rows_of),
            rgb_of: Some(&rgb_of),
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
            controls.extend(combos.lane(Material::Controls).copied());
            overlay
                .controls
                .extend(combos.lane(Material::Popup).copied());
            texts.extend(combos.texts);
            overlay.texts.extend(combos.popup_texts);
        }
        // The rows' pictures: VERA's roster names the side in text and
        // shows no flags or acceptance; a player's badge stands in front of
        // the name, shrunk to fit.
        let pictures = GuiStyle {
            picture_of: Some(&picture_of),
            ..room_style
        };
        let icons = paint_node(nodes.icons, pictures);
        controls.extend(icons.lane(Material::Controls).copied());
        // VERA's room keeps its options in Game Options; Difficulty stays.
        if layout.game_options.is_none() {
            let options = paint_ids(page_root, &option_ids(&view, &layout), scale, room_style);
            controls.extend(options.lane(Material::Controls).copied());
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
            controls.extend(bar.lane(Material::Controls).copied());
            texts.extend(bar.texts);
        }
        // A cooperative map's description and Difficulty's words.
        let notes = paint_node(nodes.notes, room_style);
        texts.extend(notes.texts);
        // The chat edit as its widget holds it: it keeps the room's focus,
        // and its caret, under the room's boxes too.
        let dialog = view.page().dialog;
        let chat_focus = |_: NodeId| controller.focused_control(dialog) == Some(CHAT);
        let chat_caret = |node: NodeId| controller.caret_visible_in(dialog, node);
        let chat_edit = GuiStyle {
            focus_of: Some(&chat_focus),
            caret_of: Some(&chat_caret),
            #[cfg(not(target_os = "android"))]
            ime_target: Some(crate::app::input::ime::ImeTarget::SetupChat),
            rgb: label_rgb,
            ..GuiStyle::default()
        };
        let chat = paint_node(nodes.chat, chat_edit);
        controls.extend(chat.lane(Material::Controls).copied());
        texts.extend(chat.texts);
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
        controls.extend(chat.lane(Material::Controls).copied());
        texts.extend(chat.texts);
        if !sliding {
            if let Some(preview) = frontend.skirmish_preview_texture.as_ref() {
                overlay.preview = build_preview_surface_instance(
                    layout.map_preview,
                    preview.width,
                    preview.height,
                );
                // A received preview's markers are the host's 'W' pairs over
                // its bounds, drawn when 1 <= count <= 8 (0x00640710).
                let received_bounds = (preview.received_preview.is_some())
                    .then(|| {
                        frontend
                            .multiplayer_shell_runtime
                            .start_staging()
                            .preview_source_bounds()
                    })
                    .flatten();
                // The host's RandMap sentinel numbers the last Generate's
                // starts, copied back by 0x0058BB30 (0x005E67ED).
                let entry = view
                    .selected_map_index
                    .and_then(|index| frontend.scenario_catalog.shell_maps().get(index));
                let own_bounds = super::preview::start_marker_bounds(
                    entry
                        .is_some_and(super::preview::is_random_map_sentinel_entry)
                        .then(|| frontend.random_map_retention.generated_header()),
                    entry.and_then(|entry| entry.preview_source_bounds.as_ref()),
                );
                let bounds = frontend
                    .network_setup_protocol
                    .as_ref()
                    .and_then(|protocol| protocol.received_preview_bounds())
                    .or(received_bounds.as_ref())
                    .or(own_bounds.as_ref());
                overlay.zoom = zoom_layout.as_ref().map(|zoom_layout| {
                    zoom_draws(
                        renderer,
                        &frontend.screen,
                        atlas,
                        zoom_layout,
                        bounds,
                        scale,
                    )
                });
                if let Some(bounds) = bounds {
                    let fitted = aspect_fit_rect(layout.map_preview, preview.width, preview.height);
                    let positions = project_preview_start_positions(bounds, fitted);
                    push_start_marker_sprites(&mut controls, atlas, &positions, scale, 0.00056);
                    // BC5DE644/BD5E1D51 call shared DrawStartPositions640710:
                    // 6409D7..640A15 draws1-based numbers after STARTBUT, using
                    // the destination surface clip, not the thumbnail bounds.
                    let mut instances = Vec::new();
                    push_start_marker_labels(
                        &mut instances,
                        &renderer.bit_font,
                        &positions,
                        scale,
                        0.00040,
                    );
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
            art.extend(buttons.lane(Material::Controls).copied());
            texts.extend(buttons.texts);
        }
        // C2/C9 0x3ED image button (00612B70, frame type 0): +0x14 wodact
        // released, +0x18 wouact while pressed, copied 1:1 at the rect's
        // top-left with no frame or caption (0x6131D7..0x613237).
        let emote_pressed = |id: i32| (id == i32::from(EMOTE)).then(|| look.pressed(EMOTE));
        let emote = GuiStyle {
            buttons: ButtonArt::Image,
            picture_of: Some(&picture_of),
            pressed_of: Some(&emote_pressed),
            ..GuiStyle::default()
        };
        let emote = paint_node(nodes.emote, emote);
        controls.extend(emote.lane(Material::Controls).copied());
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
            overlay
                .controls
                .extend(widgets.lane(Material::Controls).copied());
            overlay.texts.extend(widgets.texts);
        }
        // VERA asks a guest before downloading a map it does not have, then
        // shows the download, on the art of VERA's other online-service boxes.
        if let Some(offer) = ui.map_offer.as_ref() {
            let boxed = &layout.map_offer;
            let progress = offer
                .downloading
                .then(|| {
                    frontend
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
                MAP_OFFER_LEAVE => Some(process_assets.csf_label("GUI:Leave", "Leave")),
                _ => None,
            };
            // Download greys while the map comes, but still takes a press.
            let enabled_of =
                |id: i32| (id == i32::from(MAP_OFFER_DOWNLOAD)).then_some(!offer.downloading);
            // How much of the map has arrived, in the label colour.
            let frame_of = |id: i32| {
                let (done, total) = progress.filter(|_| id == i32::from(MAP_OFFER_PROGRESS))?;
                Some(FrameArt::Progress {
                    rgb: SHELL_LABEL_TEXT_RGB,
                    done,
                    total,
                    depth: super::wol_lobby::POPUP_DEPTHS.button,
                })
            };
            let style = GuiStyle {
                text_of: Some(&text_of),
                enabled_of: Some(&enabled_of),
                frame_of: Some(&frame_of),
                ..GuiStyle::popup()
            };
            // Its text, buttons and bar in tree order.
            let scale = crate::ui::shell::geom::RETAIL_SCALE;
            let widgets = controller
                .root_in(MAP_OFFER_BOX)
                .map(|root| {
                    paint_gui_styled(
                        &platform.ime,
                        &renderer.bit_font,
                        frontend,
                        match_state,
                        process_assets,
                        controller.gui(),
                        root,
                        scale,
                        &[],
                        style,
                    )
                })
                .unwrap_or_default();
            overlay
                .controls
                .extend(widgets.lane(Material::Controls).copied());
            overlay.texts.extend(widgets.texts);
        }
    }
    if let (Some(notice), Some(parts)) = (&ui.notice, notice_parts.as_ref()) {
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
        let (sprites, texts) = (&mut overlay.controls, (&mut overlay.texts, false));
        let tree = super::controller_tree(&frontend.shell_controller, notice.dialog);
        super::shell_box::push_skirmish_box(
            &platform.ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            tree,
            notice,
            parts,
            pressed,
            box_look,
            sprites,
            texts,
        );
    }
    // The page dialog 0xCF over C2/C9 (0x0077D820 keeps the page painting).
    if let Some((_, page)) = crate::app::frontend::wol_welcome_render::page_dialogs_paint(
        &platform.ime,
        renderer,
        frontend,
        match_state,
        process_assets,
    ) {
        overlay.controls.extend(page.controls);
        overlay.texts.extend(page.texts);
    }
    // Start Game Now's 0x130 over the locked C2/C9; VERA's room waits a
    // moment so a quick check does not flash it.
    let verify = crate::app::shell_network_setup::verify(frontend.network_setup_protocol.as_ref());
    if verify.is_some_and(|verify| !verify.shown(view.dialog.modern, now)) {
        platform.window.request_redraw();
    }
    if let Some(verify) = verify.filter(|verify| verify.shown(view.dialog.modern, now)) {
        let paint = super::paint_wol_verify(
            &platform.ime,
            renderer,
            frontend,
            match_state,
            process_assets,
            verify,
            &crate::app::shell_network_setup::verify_layout(&renderer.gpu, frontend, match_state),
        );
        overlay.controls.extend(paint.controls);
        overlay.texts.extend(paint.texts);
    }
    // VERA's send key beside the chat box. Only the LAN room needs it: the
    // online room already has its native emote button in the chat row
    // (`0x3ED`, WOL rooms only). It goes with the page's slide, like the LAN
    // lobby's.
    if !leaving && let Some(rect) = layout.chat_send(view.dialog) {
        super::push_chat_send_key(&mut overlay.controls, atlas, scale, rect);
    }
    in_game_shell::render_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
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
    platform.window.request_redraw();
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

/// The room's own colours by node: a header's caption in its team's tint,
/// Join grey while it is closed and white under the pointer or while it is
/// pressed, and a roster cell whose list opens white under the pointer and
/// while its list is open.
fn room_rgb(
    frontend: &FrontendState,
    match_state: &MatchState,
    view: &SetupView,
    look: &RoomLook,
    id: i32,
) -> Option<[f32; 3]> {
    let id = u16::try_from(id).ok()?;
    if let Some(row) = painted_row(id, HEADER_CAPTION) {
        let header = view.headers[row].as_ref()?;
        return team_caption_tint(
            crate::app::state::active_rules(frontend, match_state),
            &header.member_colors,
        );
    }
    if let Some(row) = painted_row(id, JOIN_CAPTION) {
        let join = team_join(row);
        return Some(if !view.enabled(join) {
            ROOM_CLOSED_TEXT_RGB
        } else if look.pressed(join) || look.target == Some(join) {
            [1.0, 1.0, 1.0]
        } else {
            ROOM_TEXT_RGB
        });
    }
    let combo = view.combo(id)?;
    let pointed =
        look.target == Some(id) || look.open.as_ref().is_some_and(|(open, _)| *open == id);
    (pointed && view.enabled(id) && roster_cell_opens(combo)).then_some([1.0, 1.0, 1.0])
}

/// The room's boxes by node: the roster's scrollbar; the team bar's box; a
/// group's box, lit while a press in it joins the group or the host carries
/// a row over it; the band behind the local player's row.
fn room_frame(
    view: &SetupView,
    look: &RoomLook,
    drag: Option<RowDrag>,
    id: i32,
) -> Option<FrameArt> {
    let id = u16::try_from(id).ok()?;
    match id {
        ROSTER_SCROLL => return Some(FrameArt::ListBar),
        TEAM_BAR_BOX => {
            return Some(FrameArt::Outline {
                rgb: ROSTER_BOX_RGB,
            });
        }
        _ => {}
    }
    if painted_row(id, OWN_ROW).is_some() {
        return Some(FrameArt::Fill {
            rgb: ROSTER_OWN_ROW_RGB,
            depth: SHELL_CONTROL_DEPTH + 0.00001,
        });
    }
    let row = usize::from(id.checked_sub(GROUP_BOX)?);
    let header = view.headers.get(row)?.as_ref()?;
    let join = team_join(row);
    let lit = drag.and_then(|drag| drag.target) == Some(row)
        || header.join.is_some()
            && view.enabled(join)
            && (look.target == Some(join) || look.pressed(join));
    let rgb = if lit { ROOM_TEXT_RGB } else { ROSTER_BOX_RGB };
    Some(FrameArt::Outline { rgb })
}

/// The room's pictures by node: the emote button's, a player's badge, a
/// row's host or accepted icon (its flag is its node's `Art`).
fn room_picture(view: &SetupView, id: i32) -> Option<Picture> {
    let id = u16::try_from(id).ok()?;
    if id == EMOTE {
        return Some(Picture::Emote);
    }
    if let Some(row) = painted_row(id, BADGE) {
        return Some(Picture::Badge(view.row_icons[row].as_ref()?.badge?));
    }
    let icon = view.row_icons[painted_row(id, ROW_STATUS)?].as_ref()?;
    Some(Picture::Status {
        host: icon.host,
        accepted: icon.accepted,
    })
}

/// The room's own texts for its widgets: the lists' faces, the page
/// buttons' captions and the trackbars' values the room words itself, and
/// its statics' words: the column labels, the roster's page line, its
/// headers' captions and Joins, the team bar's captions, a cooperative
/// map's description.
fn widget_text(
    skirmish_shell_state: &SkirmishShellState,
    process_assets: &ProcessAssets,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    id: i32,
) -> Option<String> {
    let id = u16::try_from(id).ok()?;
    match id {
        ROSTER_PAGE_LINE => {
            return Some(crate::i18n::tr_fill(
                "VERA:RosterPage",
                &[
                    ("first", &(view.first_row + 1)),
                    ("last", &(view.first_row + view.page_len)),
                    ("total", &view.row_count),
                ],
            ));
        }
        TEAMS_CAPTION => return Some(crate::i18n::tr("VERA:Teams")),
        ROOM_SIZE_CAPTION => return Some(crate::i18n::tr("VERA:RoomSize")),
        COOPERATIVE_TEXT => return view.cooperative_description.clone(),
        id if (COLUMN_LABEL..ROSTER_PAGE_LINE).contains(&id) => {
            let (key, _) = column_label(view, layout, usize::from(id - COLUMN_LABEL))?;
            return Some(process_assets.csf_label(key, key));
        }
        _ => {}
    }
    if let Some(row) = painted_row(id, HEADER_CAPTION) {
        return Some(view.headers[row].as_ref()?.caption.clone());
    }
    if let Some(row) = painted_row(id, JOIN_CAPTION) {
        return view.headers[row].as_ref()?.join.clone();
    }
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
        return Some(label.map_or_else(|| process_assets.csf_label(key, key), crate::i18n::tr));
    }
    let bar = view.trackbars.iter().find(|bar| bar.id == id)?;
    Some(match id {
        SPEED => {
            let context = crate::ui::shell::game_speed::SpeedContext::Network;
            context.value_text(context.stored_speed(bar.value))
        }
        CREDITS => skirmish_shell_state
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
