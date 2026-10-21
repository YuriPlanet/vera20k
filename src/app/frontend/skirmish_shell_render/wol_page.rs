//! The WOL page dialogs on the shared control owners: `0xCF` over `0x113`
//! or C2/C9, and `0x11E` over the empty backdrop. Both are the 451x326
//! compact popup (background mode 2) with owner-draw buttons, NewEdits and
//! an owner-draw list.

use std::time::Instant;

use super::SHELL_LABEL_TEXT_RGB;
use super::controls::list_controls;
use super::text::rect_to_text_rect;
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint, label, push_edit, row_rgb};
use crate::app::AppState;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::lan_shell::{LanChatLine, LanListState};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::ShellListGeometry;
use crate::ui::shell::modal::{WolPageLayout, WolPageReceivedLayout};
use crate::ui::wol_page::*;

const SCALE: ShellScale = ShellScale::RETAIL;

fn caption(
    state: &AppState,
    key: &str,
    rect: RectPx,
    align: ShellAlign,
    enabled: bool,
) -> PaintLabel<'static> {
    PaintLabel {
        text: label(state, key).into(),
        rect,
        align,
        rgb: if enabled {
            SHELL_LABEL_TEXT_RGB
        } else {
            shell_paint::SHELL_TEXT_RGB_DISABLED
        },
        path_a_reveal: None,
    }
}

fn button_caption(
    state: &AppState,
    key: &str,
    rect: RectPx,
    buttons: &ShellButtonInteraction<u16>,
    id: u16,
    enabled: bool,
) -> PaintLabel<'static> {
    caption(
        state,
        key,
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            rect,
            buttons.is_pressed(id),
        ),
        ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
        enabled,
    )
}

/// The frame, scrollbar and rows of a transcript.
fn paint_transcript(
    paint: &mut WolLobbyPaint,
    state: &AppState,
    geometry: ShellListGeometry,
    list: &LanListState,
    lines: &[LanChatLine],
    selectable: bool,
) {
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return;
    };
    list_controls(
        &mut paint.controls,
        atlas,
        geometry,
        list,
        selectable,
        SCALE,
    );
    for (visible, line) in lines
        .iter()
        .skip(list.top_index)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let row = geometry.row(visible);
        paint.texts.push(shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            &String::from_utf16_lossy(&line.text),
            rect_to_text_rect(RectPx::new(row.x + 2, row.y, row.w - 2, row.h)),
            row_rgb(line.color),
            ShellAlign::V_CENTER,
            [0.0; 2],
            POPUP_DEPTHS.text,
            SCALE.factor(),
        ));
    }
}

/// Draw the box first, then its children over it. The mode-2 paint
/// (`0x006221D1`) picks the neutral background in the shell and the
/// scenario side's in a match.
fn paint_box(
    state: &AppState,
    dialog: RectPx,
    buttons: &[ModalButton],
    labels: &[PaintLabel],
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let side = (state.frontend.screen == crate::app::GameScreen::InGame)
        .then(|| crate::app::presentation::sidebar_render::current_sidebar_theme(state));
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(side),
        super::type3_button_frames(atlas),
        dialog,
        buttons,
        labels,
        SCALE,
        POPUP_DEPTHS,
    );
    paint.controls = draw.sprites;
    paint.texts.extend(draw.text);
    paint
}

/// `0xCF`: no title; the name, text, transcript, status and two buttons.
pub(crate) fn paint_wol_page_dialog(
    state: &AppState,
    page: &WolPageDialog,
    layout: &WolPageLayout,
) -> WolLobbyPaint {
    let labels = vec![
        // SS_RIGHT, then two left-aligned statics.
        caption(
            state,
            "GUI:PageBuddy",
            layout.name_label,
            ShellAlign::H_RIGHT,
            true,
        ),
        caption(
            state,
            "GUI:TextToSend",
            layout.text_label,
            ShellAlign::NONE,
            true,
        ),
        caption(
            state,
            "GUI:FindStatus",
            layout.status_label,
            ShellAlign::NONE,
            true,
        ),
        PaintLabel {
            text: page.status.clone().into(),
            rect: layout.status,
            align: ShellAlign::NONE,
            rgb: SHELL_LABEL_TEXT_RGB,
            path_a_reveal: None,
        },
        button_caption(
            state,
            "GUI:Page",
            layout.send,
            &page.buttons,
            SEND,
            page.send_enabled,
        ),
        button_caption(
            state,
            "GUI:Cancel",
            layout.cancel,
            &page.buttons,
            CANCEL,
            true,
        ),
    ];
    let buttons = [
        ModalButton {
            rect: layout.send,
            pressed: page.buttons.is_pressed(SEND),
            enabled: page.send_enabled,
        },
        ModalButton {
            rect: layout.cancel,
            pressed: page.buttons.is_pressed(CANCEL),
            enabled: true,
        },
    ];
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    let caret = caret_visible(page.focus_at, Instant::now());
    for (edit, rect) in [(&page.name, layout.name), (&page.text, layout.text)] {
        push_edit(
            &mut paint,
            state,
            edit,
            rect,
            edit.focused && caret,
            SCALE,
            POPUP_DEPTHS.text,
        );
    }
    let geometry = crate::app::shell_wol::page::page_geometry(layout, page);
    paint_transcript(&mut paint, state, geometry, &page.list, &page.lines, false);
    paint
}

/// `0x11E`: the centred title, the selectable transcript, the reply edit,
/// "Send to:" with the pager and two buttons.
pub(crate) fn paint_wol_page_received(
    state: &AppState,
    received: &WolPageReceived,
    layout: &WolPageReceivedLayout,
) -> WolLobbyPaint {
    let labels = vec![
        caption(
            state,
            "GUI:PageReceived",
            layout.title,
            ShellAlign::H_CENTER,
            true,
        ),
        caption(
            state,
            "GUI:ReplyTo",
            layout.reply_to_label,
            ShellAlign::H_RIGHT,
            true,
        ),
        PaintLabel {
            text: received.reply_to.clone().into(),
            rect: layout.reply_to,
            align: ShellAlign::NONE,
            rgb: SHELL_LABEL_TEXT_RGB,
            path_a_reveal: None,
        },
        button_caption(
            state,
            "GUI:ReplyPage",
            layout.send,
            &received.buttons,
            SEND,
            received.send_enabled,
        ),
        button_caption(
            state,
            "GUI:Back",
            layout.back,
            &received.buttons,
            BACK,
            true,
        ),
    ];
    let buttons = [
        ModalButton {
            rect: layout.send,
            pressed: received.buttons.is_pressed(SEND),
            enabled: received.send_enabled,
        },
        ModalButton {
            rect: layout.back,
            pressed: received.buttons.is_pressed(BACK),
            enabled: true,
        },
    ];
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    push_edit(
        &mut paint,
        state,
        &received.reply,
        layout.reply,
        received.reply.focused && caret_visible(received.focus_at, Instant::now()),
        SCALE,
        POPUP_DEPTHS.text,
    );
    let geometry = crate::app::shell_wol::page::received_geometry(layout, received);
    paint_transcript(
        &mut paint,
        state,
        geometry,
        &received.list,
        &received.lines,
        true,
    );
    paint
}

/// Add Buddy `0x115` over `0x116`: the centred caption, the name edit and
/// two buttons (Add created disabled). Its status line `0x695` has no help
/// to show.
pub(crate) fn paint_wol_add_buddy(
    state: &AppState,
    add: &crate::ui::wol_buddy::WolAddBuddy,
    layout: &crate::ui::shell::modal::WolAddBuddyLayout,
) -> WolLobbyPaint {
    use crate::ui::wol_buddy::{ADD, BACK as ADD_BACK};
    let labels = vec![
        caption(
            state,
            "GUI:BuddyNickname",
            layout.label,
            ShellAlign::H_CENTER,
            true,
        ),
        button_caption(
            state,
            "GUI:AddBuddy",
            layout.add,
            &add.buttons,
            ADD,
            add.add_enabled,
        ),
        button_caption(state, "GUI:Back", layout.back, &add.buttons, ADD_BACK, true),
    ];
    let buttons = [
        ModalButton {
            rect: layout.add,
            pressed: add.buttons.is_pressed(ADD),
            enabled: add.add_enabled,
        },
        ModalButton {
            rect: layout.back,
            pressed: add.buttons.is_pressed(ADD_BACK),
            enabled: true,
        },
    ];
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    push_edit(
        &mut paint,
        state,
        &add.edit,
        layout.name,
        add.edit.focused && caret_visible(add.focus_at, Instant::now()),
        SCALE,
        POPUP_DEPTHS.text,
    );
    paint
}

/// The connection verify `0x130` over C2/C9: the SS_CENTER title, then one
/// owner-draw checkbox (`0x006213A0`) per node captioned with its name. The
/// mismatch box `0xD0` has the same centred 451x326 rect and covers it
/// whole, so only the box paints while it is up.
pub(crate) fn paint_wol_verify(
    state: &AppState,
    verify: &crate::ui::wol_verify::WolVerifyDialog,
    layout: &crate::ui::shell::modal::WolVerifyLayout,
) -> WolLobbyPaint {
    if let Some(message) = &verify.mismatch {
        let pressed = verify
            .ok
            .is_pressed(crate::ui::wol_shell::BOX_OK)
            .then_some(crate::ui::wol_shell::BOX_OK);
        let (dialog, buttons, labels) = crate::app::frontend::wol_welcome_render::message_box_parts(
            message,
            pressed,
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let buttons: Vec<ModalButton> = buttons
            .into_iter()
            .map(|(rect, pressed)| ModalButton {
                rect,
                pressed,
                enabled: true,
            })
            .collect();
        return paint_box(state, dialog, &buttons, &labels);
    }
    let labels = [caption(
        state,
        "GUI:ConnectingToPlayers",
        layout.title,
        ShellAlign::H_CENTER,
        true,
    )];
    let mut paint = paint_box(state, layout.dialog, &[], &labels);
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let chrome = atlas.control_chrome();
    for (node, name) in verify.names.iter().enumerate() {
        let rect = layout.boxes[node];
        super::controls::paint_control(
            &mut paint.controls,
            &chrome,
            super::controls::ControlPaint::Checkbox {
                rect,
                checked: verify.checked[node],
            },
            SCALE,
        );
        paint.texts.push(shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            name,
            rect_to_text_rect(crate::ui::skirmish_shell::checkbox_text_rect(rect, SCALE)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            [0.0; 2],
            POPUP_DEPTHS.text,
            SCALE.factor(),
        ));
    }
    paint
}

/// The in-match `0xCF` (state 7) as GPU buffers for the screen-fixed UI
/// pass: the battlefield keeps drawing under it, and the dialog goes down
/// after the tooltips and before the cursor.
/// RESIDUAL: in a match `0x00622B50` skips `OwnerDraw__LoadNotInGameResources`;
/// the buttons, edits and list here keep the shell's control art.
pub(crate) struct InMatchPageDraw {
    controls: Option<(wgpu::Buffer, u32)>,
    texts: Vec<(
        crate::render::shell_text::ScissorRect,
        Option<(wgpu::Buffer, u32)>,
    )>,
}

pub(crate) fn in_match_page_draw(state: &AppState) -> Option<InMatchPageDraw> {
    if state.match_state.match_presentation.in_game_menu
        != crate::ui::pause_menu::InGameMenuState::Page
    {
        return None;
    }
    let page = crate::app::shell_wol::page::page_dialog(state)?;
    let paint = paint_wol_page_dialog(
        state,
        page,
        &crate::app::shell_wol::page::page_layout(state),
    );
    let batch = &state.renderer.batch_renderer;
    let gpu = &state.renderer.gpu;
    // The UI camera follows the world camera's scroll (see the message
    // rows): add it back so the dialog stays at its screen pixels.
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let screen = |instances: &[crate::render::batch::SpriteInstance]| {
        let shifted: Vec<_> = instances
            .iter()
            .map(|instance| {
                let mut instance = *instance;
                instance.position[0] += camera[0];
                instance.position[1] += camera[1];
                instance
            })
            .collect();
        batch.create_instance_buffer(gpu, &shifted)
    };
    Some(InMatchPageDraw {
        controls: screen(&paint.controls),
        texts: paint
            .texts
            .iter()
            .map(|text| (text.scissor, screen(&text.instances)))
            .collect(),
    })
}

impl InMatchPageDraw {
    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, state: &'a AppState) {
        let batch = &state.renderer.batch_renderer;
        let (width, height) = (state.render_width(), state.render_height());
        if let (Some((buffer, count)), Some(atlas)) = (
            &self.controls,
            state.frontend.skirmish_shell_chrome.as_ref(),
        ) {
            batch.draw_with_buffer_ui_passthrough(pass, &atlas.texture, buffer, *count);
        }
        for (scissor, buffer) in &self.texts {
            let Some((buffer, count)) = buffer else {
                continue;
            };
            let x = scissor.x.min(width);
            let y = scissor.y.min(height);
            let w = scissor.w.min(width - x);
            let h = scissor.h.min(height - y);
            if w == 0 || h == 0 {
                continue;
            }
            pass.set_scissor_rect(x, y, w, h);
            batch.draw_with_buffer_ui_passthrough(
                pass,
                state.renderer.bit_font.atlas(),
                buffer,
                *count,
            );
        }
        pass.set_scissor_rect(0, 0, width, height);
    }
}
