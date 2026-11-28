//! The WOL page dialogs on the shared control owners: `0xCF` over `0x113`
//! or C2/C9, and `0x11E` over the empty backdrop. Both are the 451x326
//! compact popup (background mode 2) with owner-draw buttons, NewEdits and
//! an owner-draw list, their looks read from the dialogs' widgets.

use super::gui_paint::{ButtonArt, GuiStyle, HostRows, paint_gui_styled};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint;
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::modal::{WolPageLayout, WolPageReceivedLayout};
use crate::ui::wol_page::*;

const SCALE: ShellScale = crate::ui::shell::geom::RETAIL_SCALE;

/// The mode-2 paint (`0x006221D1`) picks the neutral background in the
/// shell and the scenario side's in a match.
fn side(
    frontend: &FrontendState,
    match_state: &MatchState,
) -> Option<crate::ui::sidebar::SidebarTheme> {
    (frontend.screen == crate::app::GameScreen::InGame).then(|| {
        crate::app::presentation::sidebar_render::current_sidebar_theme(frontend, match_state)
    })
}

/// The box, for its children to draw over it. The mode-2 paint
/// (`0x006221D1`) picks the neutral background in the shell and the
/// scenario side's in a match.
fn paint_box(frontend: &FrontendState, match_state: &MatchState, dialog: RectPx) -> WolLobbyPaint {
    paint_box_on(frontend, match_state, dialog, SCALE)
}

fn paint_box_on(
    frontend: &FrontendState,
    match_state: &MatchState,
    dialog: RectPx,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    paint.controls = shell_paint::paint_modal_sprites(
        atlas.mode_two_dialog_background(side(frontend, match_state)),
        super::type3_button_frames(atlas),
        dialog,
        &[],
        scale,
        POPUP_DEPTHS,
    );
    paint
}

/// Appends a widget paint's sprites and texts to the box's.
fn extend(paint: &mut WolLobbyPaint, widgets: HudPaint) {
    paint
        .controls
        .extend(widgets.lane(Material::Controls).copied());
    paint.texts.extend(widgets.texts);
}

/// `dialog`'s box and its widgets over it: owner-draw buttons, NewEdits and
/// the transcript, with the statics and rows its model keeps.
fn paint_widget_box(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    dialog: DialogResourceId,
    rect: RectPx,
    text_of: &dyn Fn(i32) -> Option<String>,
    rows_of: &dyn Fn(i32) -> Option<HostRows>,
) -> WolLobbyPaint {
    let mut paint = paint_box(frontend, match_state, rect);
    let c = &frontend.shell_controller;
    let Some(root) = c.root_in(dialog) else {
        return paint;
    };
    let style = GuiStyle {
        text_of: Some(text_of),
        rows_of: Some(rows_of),
        ..GuiStyle::popup()
    };
    extend(
        &mut paint,
        paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            c.gui(),
            root,
            SCALE,
            &[],
            style,
        ),
    );
    paint
}

/// A transcript's rows with their colours.
fn rows(lines: &[LanChatLine]) -> HostRows {
    let text = |line: &LanChatLine| String::from_utf16_lossy(&line.text);
    lines.iter().map(|line| (text(line), line.color)).collect()
}

/// The WOL tournament Surrender box: `0x005C60D0`'s ShellMessageBox `0x120`
/// with `TXT_SURRENDER` over OK `0x5AE` and Cancel (control 2), from its
/// nodes ([`crate::app::input::surrender::sync`]).
fn paint_surrender_box(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) -> WolLobbyPaint {
    use crate::app::input::surrender::{self, BODY, SurrenderButton};
    let layout = surrender::layout(renderer, &frontend.screen);
    let mut paint = paint_box(frontend, match_state, layout.dialog);
    let Some(root) = surrender::root(&match_state.match_presentation) else {
        return paint;
    };
    let text_of = |id: i32| -> Option<String> {
        let key = match u16::try_from(id).ok()? {
            BODY => "TXT_SURRENDER",
            0x5AE => "TXT_OK",
            2 => "TXT_CANCEL",
            _ => return None,
        };
        Some(process_assets.csf_text(key))
    };
    let pressed_of = |id: i32| match id {
        0x5AE => Some(surrender::pressed(
            &match_state.match_presentation,
            SurrenderButton::Ok,
        )),
        2 => Some(surrender::pressed(
            &match_state.match_presentation,
            SurrenderButton::Cancel,
        )),
        _ => None,
    };
    let style = GuiStyle {
        buttons: ButtonArt::Modal,
        text_of: Some(&text_of),
        pressed_of: Some(&pressed_of),
        ..GuiStyle::default()
    };
    let gui = match_state.match_presentation.dialog_ui.gui();
    extend(
        &mut paint,
        paint_gui_styled(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            root,
            SCALE,
            &[],
            style,
        ),
    );
    paint
}

/// `0xCF`: no title; the name, text, transcript, status and two buttons.
pub(crate) fn paint_wol_page_dialog(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    page: &WolPageDialog,
    layout: &WolPageLayout,
) -> WolLobbyPaint {
    let text_of = |id| (id == i32::from(STATUS)).then(|| page.status.clone());
    let rows_of = |id| (id == i32::from(PAGE_TRANSCRIPT)).then(|| rows(&page.lines));
    paint_widget_box(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        PAGE_DIALOG,
        layout.dialog,
        &text_of,
        &rows_of,
    )
}

/// `0x11E`: the centred title, the selectable transcript, the reply edit,
/// "Send to:" with the pager and two buttons.
pub(crate) fn paint_wol_page_received(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    received: &WolPageReceived,
    layout: &WolPageReceivedLayout,
) -> WolLobbyPaint {
    let text_of = |id| (id == i32::from(REPLY_TO)).then(|| received.reply_to.clone());
    let rows_of = |id| (id == i32::from(RECEIVED_TRANSCRIPT)).then(|| rows(&received.lines));
    paint_widget_box(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        RECEIVED_DIALOG,
        layout.dialog,
        &text_of,
        &rows_of,
    )
}

/// Add Buddy `0x115` over `0x116`: the centred caption, the name edit and
/// two buttons (Add created disabled). Its status line `0x695` has no help
/// to show.
pub(crate) fn paint_wol_add_buddy(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    layout: &crate::ui::shell::modal::WolAddBuddyLayout,
) -> WolLobbyPaint {
    let dialog = crate::ui::wol_buddy::ADD_DIALOG;
    paint_widget_box(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        dialog,
        layout.dialog,
        &|_| None,
        &|_| None,
    )
}

/// The connection verify `0x130` over C2/C9: the SS_CENTER title, then one
/// owner-draw checkbox (`0x006213A0`) per node captioned with its name. The
/// mismatch box `0xD0` has the same centred 451x326 rect and covers it
/// whole, so only the box paints while it is up.
pub(crate) fn paint_wol_verify(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    verify: &crate::ui::wol_verify::WolVerifyDialog,
    layout: &crate::ui::shell::modal::WolVerifyLayout,
) -> WolLobbyPaint {
    if let Some(message) = &verify.mismatch {
        let mut paint = WolLobbyPaint::default();
        let Some(atlas) = frontend.skirmish_shell_chrome.as_ref() else {
            return paint;
        };
        let parts = crate::app::shell_network_setup::verify_mismatch_parts(&renderer.gpu, message);
        let pressed = verify.ok_pressed();
        let look = super::BoxLook::new(
            atlas.mode_two_dialog_background(side(frontend, match_state)),
            POPUP_DEPTHS,
        );
        let ok = |id| pressed && id == crate::ui::wol_shell::BOX_OK;
        // The box's widgets in the verify's own tree
        // ([`crate::ui::wol_verify::WolVerifyDialog::sync_mismatch`]).
        let tree = verify.widgets();
        let draw = super::paint_shell_box(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            tree,
            message,
            &parts,
            ok,
            look,
        );
        paint.controls = draw.sprites;
        paint.texts.extend(draw.text);
        return paint;
    }
    let scale = layout.scale;
    let mut paint = paint_box_on(frontend, match_state, layout.dialog, scale);
    let Some((gui, root)) = verify.widgets() else {
        return paint;
    };
    // The title, each box captioned with its node's name; the list holds
    // no rows of its own, only its frame and scrollbar under the boxes.
    let text_of = |id: i32| -> Option<String> {
        let id = u16::try_from(id).ok()?;
        if id == crate::ui::wol_verify::TITLE {
            return Some(process_assets.csf_text("GUI:ConnectingToPlayers"));
        }
        verify.names.get(verify.node_of(id)?).cloned()
    };
    let list = i32::from(crate::ui::wol_verify::LIST);
    let rows_of = |id: i32| (id == list).then(HostRows::new);
    let style = GuiStyle {
        text_of: Some(&text_of),
        rows_of: Some(&rows_of),
        ..GuiStyle::default()
    };
    let children = gui.tree().children(root).unwrap_or_default();
    let id_of = |node| gui.id_of(node).and_then(|id| u16::try_from(id).ok());
    // The title and the list, then the boxes over the list.
    for pass in [
        Some(crate::ui::wol_verify::TITLE),
        Some(crate::ui::wol_verify::LIST),
        None,
    ] {
        for &node in children {
            let id = id_of(node);
            let painted = match pass {
                Some(wanted) => id == Some(wanted),
                None => id.and_then(|id| verify.node_of(id)).is_some(),
            };
            if painted {
                extend(
                    &mut paint,
                    paint_gui_styled(
                        ime,
                        &renderer.bit_font,
                        frontend,
                        match_state,
                        process_assets,
                        gui,
                        node,
                        scale,
                        &[],
                        style,
                    ),
                );
            }
        }
    }
    paint
}

/// The tournament Surrender box (state 2) as GPU buffers. The screen-fixed UI
/// pass draws it over the battlefield after the tooltips and before the cursor.
/// RESIDUAL: in a match `0x00622B50` skips `OwnerDraw__LoadNotInGameResources`;
/// the buttons here keep the shell's control art.
pub(crate) struct SurrenderDraw {
    controls: Option<(wgpu::Buffer, u32)>,
    texts: Vec<(
        crate::render::shell_text::ScissorRect,
        Option<(wgpu::Buffer, u32)>,
    )>,
}

/// A zoomed match draws the dialog's text at the window's pixels after the
/// zoom (`window_ui`), laid out in the frame as ever: that text comes back
/// beside the draw, for `WindowUi::set_surrender_texts`, and is empty without a
/// zoom.
pub(crate) fn surrender_draw(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) -> (
    Option<SurrenderDraw>,
    crate::app::presentation::window_ui::SurrenderTexts,
) {
    if match_state.match_presentation.in_game_menu
        != crate::ui::pause_menu::InGameMenuState::Surrender
    {
        return (None, Vec::new());
    }
    let raster = crate::app::presentation::window_ui::dialog_raster(
        renderer,
        &frontend.screen,
        &match_state.input,
    );
    let frame_raster = renderer
        .bit_font
        .set_raster(raster.unwrap_or(crate::render::bit_font::TextRaster::NATIVE));
    let paint = paint_surrender_box(ime, renderer, frontend, match_state, process_assets);
    renderer.bit_font.set_raster(frame_raster);
    let batch = &renderer.batch_renderer;
    let gpu = &renderer.gpu;
    // The UI camera follows the world camera's scroll (see the message
    // rows): add it back so the dialog stays at its screen pixels.
    let camera = [match_state.input.camera_x, match_state.input.camera_y];
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
        let window_texts = texts
            .map(|text| {
                let buffer = batch.create_instance_buffer(gpu, &text.instances);
                (text.scissor, buffer)
            })
            .collect();
        let draw = SurrenderDraw {
            controls: screen(&paint.controls),
            texts: Vec::new(),
        };
        return (Some(draw), window_texts);
    }
    let draw = SurrenderDraw {
        controls: screen(&paint.controls),
        texts: texts
            .map(|text| (text.scissor, screen(&text.instances)))
            .collect(),
    };
    (Some(draw), Vec::new())
}

impl SurrenderDraw {
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        renderer: &'a RendererState,
        frontend: &'a FrontendState,
    ) {
        let batch = &renderer.batch_renderer;
        let (width, height) = (
            renderer.render_width(&frontend.screen),
            renderer.render_height(&frontend.screen),
        );
        if let (Some((buffer, count)), Some(atlas)) =
            (&self.controls, frontend.skirmish_shell_chrome.as_ref())
        {
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
            batch.draw_with_buffer_ui_passthrough(pass, renderer.bit_font.atlas(), buffer, *count);
        }
        pass.set_scissor_rect(0, 0, width, height);
    }
}
