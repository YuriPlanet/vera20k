//! The WOL page dialogs on the shared control owners: `0xCF` over `0x113`
//! or C2/C9, and `0x11E` over the empty backdrop. Both are the 451x326
//! compact popup (background mode 2) with owner-draw buttons, NewEdits and
//! an owner-draw list, their looks read from the dialogs' widgets.

use super::SHELL_LABEL_TEXT_RGB;
use super::gui_paint::{GuiStyle, HostRows};
use super::text::rect_to_text_rect;
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint};
use crate::app::AppState;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellAlign, ShellScale};
use crate::ui::shell::modal::{WolPageLayout, WolPageReceivedLayout};
use crate::ui::wol_page::*;

const SCALE: ShellScale = crate::ui::shell::geom::RETAIL_SCALE;

/// A yellow static of string `key`.
fn caption(state: &AppState, key: &str, rect: RectPx, align: ShellAlign) -> PaintLabel<'static> {
    PaintLabel {
        text: state.csf_text(key).into(),
        rect,
        align,
        rgb: SHELL_LABEL_TEXT_RGB,
        path_a_reveal: None,
    }
}

/// The mode-2 paint (`0x006221D1`) picks the neutral background in the
/// shell and the scenario side's in a match.
fn side(state: &AppState) -> Option<crate::ui::sidebar::SidebarTheme> {
    (state.frontend.screen == crate::app::GameScreen::InGame)
        .then(|| crate::app::presentation::sidebar_render::current_sidebar_theme(state))
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
    paint_box_on(state, dialog, buttons, labels, SCALE)
}

fn paint_box_on(
    state: &AppState,
    dialog: RectPx,
    buttons: &[ModalButton],
    labels: &[PaintLabel],
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(side(state)),
        super::type3_button_frames(atlas),
        dialog,
        buttons,
        labels,
        scale,
        POPUP_DEPTHS,
    );
    paint.controls = draw.sprites;
    paint.texts.extend(draw.text);
    paint
}

/// `dialog`'s box and its widgets over it: owner-draw buttons, NewEdits and
/// the transcript, with the statics and rows its model keeps.
fn paint_widget_box(
    state: &AppState,
    dialog: DialogResourceId,
    rect: RectPx,
    text_of: &dyn Fn(i32) -> Option<String>,
    rows_of: &dyn Fn(i32) -> Option<HostRows>,
) -> WolLobbyPaint {
    let mut paint = paint_box(state, rect, &[], &[]);
    let c = &state.frontend.shell_controller;
    let Some(root) = c.root_in(dialog) else {
        return paint;
    };
    let style = GuiStyle {
        text_of: Some(text_of),
        rows_of: Some(rows_of),
        ..GuiStyle::popup()
    };
    let widgets = super::gui_paint::paint_gui_styled(state, c.gui(), root, SCALE, &[], style);
    paint.controls.extend(widgets.controls);
    paint.texts.extend(widgets.texts);
    paint
}

/// A transcript's rows with their colours.
fn rows(lines: &[LanChatLine]) -> HostRows {
    let text = |line: &LanChatLine| String::from_utf16_lossy(&line.text);
    lines.iter().map(|line| (text(line), line.color)).collect()
}

/// The WOL tournament Surrender box: `0x005C60D0`'s ShellMessageBox `0x120`
/// with `TXT_SURRENDER` over OK `0x5AE` and Cancel (control 2).
fn paint_surrender_box(state: &AppState) -> WolLobbyPaint {
    use crate::app::input::surrender::SurrenderButton::{Cancel, Ok};
    let layout = crate::app::input::surrender::layout(state);
    let centre = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
    let mut labels = vec![caption(
        state,
        "TXT_SURRENDER",
        layout.body,
        ShellAlign::NONE,
    )];
    let mut buttons = Vec::new();
    for (rect, button, key) in [
        (layout.button(0), Ok, "TXT_OK"),
        (layout.button(1), Cancel, "TXT_CANCEL"),
    ] {
        let pressed = crate::app::input::surrender::pressed(state, button);
        let at = crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed);
        labels.push(caption(state, key, at, centre));
        buttons.push(ModalButton {
            rect,
            pressed,
            enabled: true,
        });
    }
    paint_box(state, layout.dialog, &buttons, &labels)
}

/// `0xCF`: no title; the name, text, transcript, status and two buttons.
pub(crate) fn paint_wol_page_dialog(
    state: &AppState,
    page: &WolPageDialog,
    layout: &WolPageLayout,
) -> WolLobbyPaint {
    let text_of = |id| (id == i32::from(STATUS)).then(|| page.status.clone());
    let rows_of = |id| (id == i32::from(PAGE_TRANSCRIPT)).then(|| rows(&page.lines));
    paint_widget_box(state, PAGE_DIALOG, layout.dialog, &text_of, &rows_of)
}

/// `0x11E`: the centred title, the selectable transcript, the reply edit,
/// "Send to:" with the pager and two buttons.
pub(crate) fn paint_wol_page_received(
    state: &AppState,
    received: &WolPageReceived,
    layout: &WolPageReceivedLayout,
) -> WolLobbyPaint {
    let text_of = |id| (id == i32::from(REPLY_TO)).then(|| received.reply_to.clone());
    let rows_of = |id| (id == i32::from(RECEIVED_TRANSCRIPT)).then(|| rows(&received.lines));
    paint_widget_box(state, RECEIVED_DIALOG, layout.dialog, &text_of, &rows_of)
}

/// Add Buddy `0x115` over `0x116`: the centred caption, the name edit and
/// two buttons (Add created disabled). Its status line `0x695` has no help
/// to show.
pub(crate) fn paint_wol_add_buddy(
    state: &AppState,
    layout: &crate::ui::shell::modal::WolAddBuddyLayout,
) -> WolLobbyPaint {
    let dialog = crate::ui::wol_buddy::ADD_DIALOG;
    paint_widget_box(state, dialog, layout.dialog, &|_| None, &|_| None)
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
        let mut paint = WolLobbyPaint::default();
        let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
            return paint;
        };
        let parts = crate::app::shell_network_setup::verify_mismatch_parts(state, message);
        let pressed = verify.ok_pressed();
        let look = super::BoxLook::new(atlas.mode_two_dialog_background(side(state)), POPUP_DEPTHS);
        let ok = |id| pressed && id == crate::ui::wol_shell::BOX_OK;
        let draw = super::paint_shell_box(state, Some(atlas), message, &parts, ok, look);
        paint.controls = draw.sprites;
        paint.texts.extend(draw.text);
        return paint;
    }
    let labels = [caption(
        state,
        "GUI:ConnectingToPlayers",
        layout.title,
        ShellAlign::H_CENTER,
    )];
    let scale = layout.scale;
    let mut paint = paint_box_on(state, layout.dialog, &[], &labels, scale);
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
            scale,
        );
    }
    let checked = verify.checked();
    for node in verify.visible_nodes(layout) {
        let name = &verify.names[node];
        let rect = verify.node_rect(layout, node).expect("visible verify node");
        super::controls::paint_control(
            &mut paint.controls,
            &chrome,
            super::controls::ControlPaint::Checkbox {
                rect,
                checked: checked[node],
            },
            scale,
        );
        paint.texts.push(shell_text::draw_in_rect_scaled(
            &state.renderer.bit_font,
            name,
            rect_to_text_rect(crate::ui::skirmish_shell::checkbox_text_rect(rect, scale)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            [0.0; 2],
            POPUP_DEPTHS.text,
            scale.factor(),
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
