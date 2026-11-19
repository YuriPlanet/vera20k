//! Body of the authored LAN room, composed over the existing BC/BD art.

use super::*;

pub(super) const READ_ONLY_TEXT: [f32; 3] = [0.77, 0.72, 0.63];

pub(super) fn button_label(view: &SetupView, id: u16) -> Option<&'static str> {
    let room = view.scenario_room.as_ref()?;
    match id {
        ACCEPT => Some(room.accept_label),
        CHOOSE_MAP => Some(room.choose_label),
        SCENARIO_BRIEFING => Some(room.briefing_label),
        BACK => Some(room.leave_label),
        _ => None,
    }
}

pub(super) fn draw_fields(
    state: &AppState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
    controls: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
) {
    let (Some(room), Some(geometry), Some(atlas), Some(ui)) = (
        &view.scenario_room,
        &layout.scenario,
        &state.frontend.skirmish_shell_chrome,
        &state.frontend.multiplayer_setup,
    ) else {
        return;
    };
    let scale = layout.page.scale;
    let font = &state.renderer.bit_font;
    let mut label = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| {
        let fitted = font.truncate_to_width(value, (rect.w as f32 / scale.factor()) as u32);
        texts.push(shell_text::draw_in_rect_scaled(
            font,
            &fitted,
            rect_to_text_rect(rect),
            rgb,
            align | ShellAlign::V_CENTER,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        ));
    };
    for (key, rect) in [
        (room.players_heading, geometry.players_heading),
        (room.chat_heading, geometry.chat_heading),
        (room.rules_heading, geometry.rules_heading),
    ] {
        label(
            &crate::i18n::tr(key),
            rect,
            ShellAlign::NONE,
            SHELL_LABEL_TEXT_RGB,
        );
    }
    if let Some(waiting) = &room.waiting {
        label(
            waiting,
            geometry.waiting,
            ShellAlign::H_CENTER,
            [1.0, 0.75, 0.34],
        );
    }
    for (row, member) in room.rows.iter().enumerate() {
        let Some(seat) = geometry.seats.get(row).copied() else {
            break;
        };
        let local = view
            .row_icons
            .get(row)
            .and_then(Option::as_ref)
            .is_some_and(|icon| icon.local);
        let shade = if local {
            [0.19, 0.12, 0.08]
        } else {
            [0.09, 0.10, 0.12]
        };
        push_solid_rect(controls, atlas, seat, shade, SHELL_CONTROL_DEPTH + 0.00002);
        label(
            &member.name,
            geometry.names[row],
            ShellAlign::NONE,
            [1.0, 0.93, 0.73],
        );
        label(
            &member.status,
            geometry.statuses[row],
            ShellAlign::H_CENTER,
            [0.89, 0.78, 0.55],
        );
        if let Some(flag) = member
            .flag
            .as_deref()
            .and_then(|name| atlas.lan_player_side(name))
        {
            push_entry_fitted(
                controls,
                flag,
                geometry.flags[row],
                scale,
                SHELL_CONTROL_DEPTH,
            );
        }
        if let Some(color) = member.color {
            push_solid_rect(
                controls,
                atlas,
                geometry.colors[row],
                color.map(|value| f32::from(value) / 255.0),
                SHELL_SWATCH_DEPTH,
            );
        }
        if member.details.is_some() {
            let id = scenario_details(row);
            let button = geometry.details[row];
            let pressed = ui.buttons.is_pressed(id);
            let fill = if pressed {
                [0.37, 0.16, 0.07]
            } else {
                [0.23, 0.13, 0.08]
            };
            push_solid_rect(controls, atlas, button, fill, SHELL_CONTROL_DEPTH);
            label(
                &crate::i18n::tr(room.details_label),
                button,
                ShellAlign::H_CENTER,
                SHELL_LABEL_TEXT_RGB,
            );
        }
    }
    let h = (font.cell_height() * scale.factor()).ceil() as i32 + 3;
    for (line, rule) in room.rules.iter().enumerate() {
        let body = geometry.rules;
        label(
            rule,
            RectPx::new(body.x, body.y + h * line as i32, body.w, h),
            ShellAlign::NONE,
            [0.9, 0.78, 0.57],
        );
    }
    if view.preview_hidden || state.frontend.skirmish_preview_texture.is_none() {
        label(
            &crate::i18n::tr(room.unavailable_preview),
            layout.map_preview,
            ShellAlign::H_CENTER,
            SHELL_LABEL_TEXT_RGB,
        );
    }
}

pub(super) struct Description {
    title: String,
    lines: Vec<String>,
    line_h: i32,
    lines_per_page: usize,
    page: usize,
    pages: usize,
}

pub(super) fn description(
    state: &mut AppState,
    view: &SetupView,
    layout: &MultiplayerSetupLayout,
) -> Option<Description> {
    let room = view.scenario_room.as_ref()?;
    let (title, content) = match view.open_box? {
        RoomBox::Briefing => (crate::i18n::tr(room.briefing_label), room.briefing.as_str()),
        RoomBox::RoleDetails(row) => {
            let Some(details) = room.rows.get(row).and_then(|row| row.details.as_ref()) else {
                state.frontend.multiplayer_setup.as_mut()?.show_box(None);
                return None;
            };
            (details.title.clone(), details.body.as_str())
        }
        RoomBox::GameOptions => return None,
    };
    let overlay = &layout.scenario.as_ref()?.description;
    let scale = layout.page.scale;
    let body = overlay.body;
    let font = &state.renderer.bit_font;
    let line_h = (font.cell_height() * scale.factor()).ceil() as i32 + 4;
    let lines_per_page = (body.h / line_h).max(1) as usize;
    let wrapped = font.wrap_layout(content, (body.w as f32 / scale.factor()) as u32);
    let lines = wrapped
        .lines
        .iter()
        .filter_map(|line| Some(content.get(line.start_byte..line.end_byte)?.to_owned()))
        .collect::<Vec<_>>();
    let ui = state.frontend.multiplayer_setup.as_mut()?;
    let pages = lines.len().div_ceil(lines_per_page).max(1);
    ui.description_pages = pages;
    ui.page_description(0);
    Some(Description {
        title,
        lines,
        line_h,
        lines_per_page,
        page: ui.description_page,
        pages,
    })
}

pub(super) fn draw_description(
    state: &AppState,
    layout: &MultiplayerSetupLayout,
    description: &Description,
    overlay: &mut ShellFrameOverlay,
) {
    let (Some(geometry), Some(atlas), Some(ui)) = (
        &layout.scenario,
        &state.frontend.skirmish_shell_chrome,
        &state.frontend.multiplayer_setup,
    ) else {
        return;
    };
    let popup = &geometry.description;
    let scale = layout.page.scale;
    let font = &state.renderer.bit_font;
    let depths = super::super::wol_lobby::POPUP_DEPTHS;
    let mut controls = Vec::new();
    let mut captions = Vec::new();
    let mut button = |id, rect, caption: String, enabled| {
        let pressed = enabled && ui.buttons.is_pressed(id);
        controls.push(ModalButton {
            rect,
            pressed,
            enabled,
        });
        captions.push(PaintLabel {
            text: caption.into(),
            rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed),
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            rgb: if enabled {
                SHELL_LABEL_TEXT_RGB
            } else {
                READ_ONLY_TEXT
            },
            path_a_reveal: None,
        });
    };
    button(
        GAME_OPTIONS_OK,
        popup.ok,
        localized_label(state, "GUI:OK", "OK"),
        true,
    );
    if description.pages > 1 {
        button(
            SCENARIO_DESCRIPTION_PREVIOUS,
            popup.previous,
            crate::i18n::tr("VERA:Previous"),
            description.page > 0,
        );
        button(
            SCENARIO_DESCRIPTION_NEXT,
            popup.next,
            crate::i18n::tr("VERA:Next"),
            description.page + 1 < description.pages,
        );
    }
    let button_frames = type3_button_frames(atlas);
    let draw = shell_paint::paint_modal_shp(
        font,
        atlas
            .ra2_dialog_background
            .or_else(|| atlas.mode_two_dialog_background(None)),
        shell_paint::ModalButtonFrames {
            disabled: None,
            ..button_frames
        },
        popup.dialog,
        &controls,
        &captions,
        scale,
        depths,
    );
    overlay.controls.extend(draw.sprites);
    // Native type-3 uses the released image for disabled buttons. The pager
    // dims that same art so the unavailable direction is apparent.
    controls.retain(|button| !button.enabled);
    overlay.controls.extend(
        shell_paint::paint_modal_sprites(
            None,
            button_frames,
            popup.dialog,
            &controls,
            scale,
            depths,
        )
        .into_iter()
        .map(|mut sprite| {
            sprite.alpha = super::super::BUTTON_DISABLED_ALPHA;
            sprite
        }),
    );
    overlay.texts.extend(draw.text);
    let body = popup.body;
    let label = |value: &str, rect: RectPx, align| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            [0.0; 2],
            depths.text,
            scale.factor(),
        )
    };
    let title = font.truncate_to_width(
        &description.title,
        scale.base_rect(popup.title).w.max(0) as u32,
    );
    overlay.texts.push(label(
        &title,
        popup.title,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    ));
    if description.pages > 1 {
        let page = crate::i18n::tr_fill(
            "VERA:PageOf",
            &[
                ("page", &(description.page + 1)),
                ("pages", &description.pages),
            ],
        );
        overlay.texts.push(label(
            &page,
            popup.page,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        ));
    }
    for (index, line) in description
        .lines
        .iter()
        .skip(description.page * description.lines_per_page)
        .take(description.lines_per_page)
        .enumerate()
    {
        overlay.texts.push(label(
            line,
            RectPx::new(
                body.x,
                body.y + index as i32 * description.line_h,
                body.w,
                description.line_h,
            ),
            ShellAlign::NONE,
        ));
    }
}
