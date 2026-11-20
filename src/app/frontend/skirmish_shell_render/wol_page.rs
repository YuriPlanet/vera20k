//! The WOL page dialogs on the shared control owners: `0xCF` over `0x113`
//! or C2/C9, and `0x11E` over the empty backdrop. Both are the 451x326
//! compact popup (background mode 2) with owner-draw buttons, NewEdits and
//! an owner-draw list, their looks read from the dialogs' widgets.

use super::SHELL_LABEL_TEXT_RGB;
use super::text::rect_to_text_rect;
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint, label, push_edit, row_rgb};
use crate::app::AppState;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::menu_page::MenuPageButtonRect;
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
    let disabled = shell_paint::SHELL_TEXT_RGB_DISABLED;
    let rgb = if enabled {
        SHELL_LABEL_TEXT_RGB
    } else {
        disabled
    };
    PaintLabel {
        rgb,
        ..value(&label(state, key), rect, align)
    }
}

/// A yellow static: a caption, a status or a name.
fn value(text: &str, rect: RectPx, align: ShellAlign) -> PaintLabel<'static> {
    let text = text.to_owned().into();
    PaintLabel {
        text,
        rect,
        align,
        rgb: SHELL_LABEL_TEXT_RGB,
        path_a_reveal: None,
    }
}

/// Owner-draw buttons `(rect, pushed, enabled, caption)` with their centred
/// captions, greyed when disabled.
fn buttons<'a>(
    state: &AppState,
    looks: impl IntoIterator<Item = (RectPx, bool, bool, &'a str)>,
    labels: &mut Vec<PaintLabel<'static>>,
) -> Vec<ModalButton> {
    let centre = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    let label_rect = crate::app::frontend::shell_pass::owner_draw_button_label_rect;
    let mut button = |(rect, pressed, enabled, key)| {
        labels.push(caption(
            state,
            key,
            label_rect(rect, pressed),
            centre,
            enabled,
        ));
        ModalButton {
            rect,
            pressed,
            enabled,
        }
    };
    looks.into_iter().map(&mut button).collect()
}

/// `dialog`'s two buttons as its widgets show them (pushed while captured
/// under the pointer).
fn widget_buttons(
    state: &AppState,
    dialog: DialogId,
    pair: [(u16, RectPx, &str); 2],
    labels: &mut Vec<PaintLabel<'static>>,
) -> Vec<ModalButton> {
    let rects = pair.map(|(id, rect, _)| MenuPageButtonRect { id, rect });
    let looks = crate::app::shell_wol::leaf::buttons(state, dialog, &rects);
    let looks = looks
        .into_iter()
        .zip(pair)
        .map(|((b, pushed, enabled), (.., key))| (b.rect, pushed, enabled, key));
    buttons(state, looks, labels)
}

/// `dialog`'s edits at the popup's text depth, without the IME composition.
fn edits(paint: &mut WolLobbyPaint, state: &AppState, dialog: DialogId, edits: &[(u16, RectPx)]) {
    let c = &state.frontend.shell_controller;
    for (id, rect) in edits {
        if let Some(node) = c.control(dialog, *id) {
            let edit = super::gui_paint::new_edit(c.gui(), node);
            let caret = c.caret_visible(node);
            push_edit(paint, state, &edit, *rect, caret, SCALE, POPUP_DEPTHS.text);
        }
    }
}

/// The frame, scrollbar and rows of `dialog`'s transcript `id`.
fn paint_transcript(
    paint: &mut WolLobbyPaint,
    state: &AppState,
    (dialog, id, rect): (DialogId, u16, RectPx),
    lines: &[LanChatLine],
    selectable: bool,
) {
    let c = &state.frontend.shell_controller;
    let atlas = state.frontend.skirmish_shell_chrome.as_ref();
    let view = c
        .control(dialog, id)
        .and_then(|node| c.gui().list_view(node));
    let (Some(atlas), Some(view)) = (atlas, view) else {
        return;
    };
    let geometry = view.window_geometry(rect, ListMetrics::scaled(SCALE));
    let selected = view.selected.filter(|_| selectable);
    super::controls::list_view_controls(
        &mut paint.controls,
        atlas,
        geometry,
        (selected, view.top, view.pressed),
        SCALE,
    );
    for (visible, line) in lines
        .iter()
        .skip(view.top)
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

/// The WOL tournament Surrender box: `0x005C60D0`'s ShellMessageBox `0x120`
/// with `TXT_SURRENDER` over OK `0x5AE` and Cancel (control 2).
fn paint_surrender_box(state: &AppState) -> WolLobbyPaint {
    use crate::app::input::surrender::SurrenderButton::{Cancel, Ok};
    let layout = crate::app::input::surrender::layout(state);
    let pressed = |button| {
        state
            .match_state
            .match_presentation
            .surrender_buttons
            .is_pressed(button)
    };
    let mut labels = vec![caption(
        state,
        "TXT_SURRENDER",
        layout.body,
        ShellAlign::NONE,
        true,
    )];
    let looks = [
        (layout.ok, pressed(Ok), true, "TXT_OK"),
        (layout.cancel, pressed(Cancel), true, "TXT_CANCEL"),
    ];
    let buttons = buttons(state, looks, &mut labels);
    paint_box(state, layout.dialog, &buttons, &labels)
}

/// `0xCF`: no title; the name, text, transcript, status and two buttons.
pub(crate) fn paint_wol_page_dialog(
    state: &AppState,
    page: &WolPageDialog,
    layout: &WolPageLayout,
) -> WolLobbyPaint {
    let mut labels = vec![
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
        value(&page.status, layout.status, ShellAlign::NONE),
    ];
    let pair = [
        (SEND, layout.send, "GUI:Page"),
        (CANCEL, layout.cancel, "GUI:Cancel"),
    ];
    let buttons = widget_buttons(state, PAGE_DIALOG, pair, &mut labels);
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    edits(
        &mut paint,
        state,
        PAGE_DIALOG,
        &[(NAME, layout.name), (TEXT, layout.text)],
    );
    let list = (PAGE_DIALOG, PAGE_TRANSCRIPT, layout.transcript);
    paint_transcript(&mut paint, state, list, &page.lines, false);
    paint
}

/// `0x11E`: the centred title, the selectable transcript, the reply edit,
/// "Send to:" with the pager and two buttons.
pub(crate) fn paint_wol_page_received(
    state: &AppState,
    received: &WolPageReceived,
    layout: &WolPageReceivedLayout,
) -> WolLobbyPaint {
    let mut labels = vec![
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
        value(&received.reply_to, layout.reply_to, ShellAlign::NONE),
    ];
    let pair = [
        (SEND, layout.send, "GUI:ReplyPage"),
        (BACK, layout.back, "GUI:Back"),
    ];
    let buttons = widget_buttons(state, RECEIVED_DIALOG, pair, &mut labels);
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    edits(&mut paint, state, RECEIVED_DIALOG, &[(TEXT, layout.reply)]);
    let list = (RECEIVED_DIALOG, RECEIVED_TRANSCRIPT, layout.transcript);
    paint_transcript(&mut paint, state, list, &received.lines, true);
    paint
}

/// Add Buddy `0x115` over `0x116`: the centred caption, the name edit and
/// two buttons (Add created disabled). Its status line `0x695` has no help
/// to show.
pub(crate) fn paint_wol_add_buddy(
    state: &AppState,
    layout: &crate::ui::shell::modal::WolAddBuddyLayout,
) -> WolLobbyPaint {
    use crate::ui::wol_buddy::{ADD, ADD_DIALOG, BACK as ADD_BACK, NAME as ADD_NAME};
    let mut labels = vec![caption(
        state,
        "GUI:BuddyNickname",
        layout.label,
        ShellAlign::H_CENTER,
        true,
    )];
    let pair = [
        (ADD, layout.add, "GUI:AddBuddy"),
        (ADD_BACK, layout.back, "GUI:Back"),
    ];
    let buttons = widget_buttons(state, ADD_DIALOG, pair, &mut labels);
    let mut paint = paint_box(state, layout.dialog, &buttons, &labels);
    edits(&mut paint, state, ADD_DIALOG, &[(ADD_NAME, layout.name)]);
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
            ShellScale::RETAIL,
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
    if let Some(geometry) = verify.list_geometry(layout) {
        super::list::paint_list(
            &mut paint.controls,
            atlas,
            geometry,
            verify.visible_nodes(layout).start,
            None,
            verify.pressed_scroll_part(),
            false,
            SCALE,
        );
    }
    for node in verify.visible_nodes(layout) {
        let name = &verify.names[node];
        let rect = verify.node_rect(layout, node).expect("visible verify node");
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

/// The in-match `0xCF` (state 7) or the Surrender box (state 2) as GPU
/// buffers for the screen-fixed UI pass: the battlefield keeps drawing under
/// it, and the dialog goes down after the tooltips and before the cursor.
/// RESIDUAL: in a match `0x00622B50` skips `OwnerDraw__LoadNotInGameResources`;
/// the buttons, edits and list here keep the shell's control art.
pub(crate) struct InMatchPageDraw {
    controls: Option<(wgpu::Buffer, u32)>,
    texts: Vec<(
        crate::render::shell_text::ScissorRect,
        Option<(wgpu::Buffer, u32)>,
    )>,
}

/// A zoomed match draws the dialog's text at the window's pixels after the
/// zoom (`window_ui`), laid out in the frame as ever.
pub(crate) fn in_match_page_draw(state: &AppState) -> Option<InMatchPageDraw> {
    let window_ui = &state.renderer.window_ui;
    window_ui.set_page_texts(Vec::new());
    let raster = crate::app::presentation::window_ui::dialog_raster(state);
    let frame_raster = state
        .renderer
        .bit_font
        .set_raster(raster.unwrap_or(crate::render::bit_font::TextRaster::NATIVE));
    let paint = match state.match_state.match_presentation.in_game_menu {
        crate::ui::pause_menu::InGameMenuState::Page => {
            crate::app::shell_wol::page::page_dialog(state).map(|page| {
                paint_wol_page_dialog(
                    state,
                    page,
                    &crate::app::shell_wol::page::page_layout(state),
                )
            })
        }
        crate::ui::pause_menu::InGameMenuState::Surrender => Some(paint_surrender_box(state)),
        _ => None,
    };
    state.renderer.bit_font.set_raster(frame_raster);
    let paint = paint?;
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
    let texts = paint.texts.iter();
    if raster.is_some() {
        window_ui.set_page_texts(
            texts
                .map(|text| {
                    let buffer = batch.create_instance_buffer(gpu, &text.instances);
                    (text.scissor, buffer)
                })
                .collect(),
        );
        return Some(InMatchPageDraw {
            controls: screen(&paint.controls),
            texts: Vec::new(),
        });
    }
    Some(InMatchPageDraw {
        controls: screen(&paint.controls),
        texts: texts
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
