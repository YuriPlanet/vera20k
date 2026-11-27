//! What the WOL leaf pages (the persona questions, My Information, the
//! Buddy List and Quick Match) do with their `vera20k-gui`
//! widgets, done once: build them under the dialog, put them on the layout,
//! feed the pointer, play the controls' cues and read the hovered
//! control's status help. Each page keeps its node table, its rects and what
//! its control-id events do.

use vera20k_gui::{NodeId, NodeSpec, Prop};

use crate::app::App;
use crate::app::frontend::shell_layout;
use crate::app::frontend::skirmish_shell_render::gui_paint;
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::render::bit_font::BitFont;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::page::{self, PageEvent};
use winit::window::Window;

/// A WOL page's art loader: its background shape and palette (loaded only at
/// 800 wide, [`shell_layout::art_key_screen_width`]) and its PCX images.
/// `0xFE` and `0x122` load none.
fn art_files(
    dialog: DialogResourceId,
) -> Option<((&'static str, &'static str), Vec<&'static str>)> {
    use crate::ui::{wol_buddy, wol_persona, wol_shell};
    let files = match dialog {
        // 0x0072C7E0
        wol_shell::WOL_WELCOME_DIALOG => (
            ("MultiplaySelection.shp", "MultiplaySelection.pal"),
            wol_shell::WOL_ICONS.iter().map(|icon| icon.file).collect(),
        ),
        // 0x0072C600
        crate::ui::wol_login::DIALOG => (("LoginScreen.shp", "LoginScreen.PAL"), Vec::new()),
        // 0x0072CD60: with the list and button images.
        crate::ui::wol_lobby::DIALOG => (
            ("MnScrnLCustomMatchLobby.shp", "MnScrnLCustomMatchLobby.pal"),
            crate::ui::wol_lobby::pcx_files(),
        ),
        // 0x0072BAC0
        crate::ui::wol_options::DIALOG => (("WOLOptions.shp", "WOLOptions.PAL"), Vec::new()),
        // 0x0072B8E0
        dialog if dialog == crate::ui::shell::sound::SHELL_SOUND_PAGE.dialog => {
            (("WOLSoundOptions.shp", "WOLSoundOptions.PAL"), Vec::new())
        }
        // 0x0072BCA0
        wol_persona::AUTO_LOGIN_DIALOG => {
            (("AutoLoginQuery.shp", "AutoLoginQuery.PAL"), Vec::new())
        }
        // 0x0072C9C0
        crate::ui::wol_my_info::DIALOG => (
            ("MnScrnLMyInformation.shp", "MnScrnLMyInformation.PAL"),
            Vec::new(),
        ),
        // 0x0072C060: with Refresh's pictures.
        wol_buddy::DIALOG => (
            ("BuddyList.shp", "BuddyList.PAL"),
            vec![
                wol_buddy::REFRESH_FACES.0,
                wol_buddy::REFRESH_FACES.1,
                "wolclan.pcx",
                "woltrny.pcx",
                "wolpriv.pcx",
                "wolreslk.pcx",
                "wolrslkob.pcx",
                "wolob.pcx",
            ],
        ),
        // 0x0072BE80
        crate::ui::wol_quick_match::DIALOG => (("quickmatch.shp", "quickmatch.PAL"), Vec::new()),
        _ => return None,
    };
    Some(files)
}

/// Loads `dialog`'s art once (its loader above); whether it is loaded.
pub(crate) fn open_art(
    frontend: &mut FrontendState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    dialog: DialogResourceId,
) -> bool {
    if art(&frontend.wol_art, dialog).is_some() {
        return true;
    }
    let (Some((background, icons)), Some(assets)) = (art_files(dialog), process_assets.manager())
    else {
        return false;
    };
    let art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &renderer.gpu,
        &renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(&renderer.gpu),
        background,
        &icons,
    );
    let loaded = art.is_some();
    frontend.wol_art.extend(art.map(|art| (dialog, art)));
    loaded
}

/// `dialog`'s loaded art.
pub(crate) fn art(
    wol_art: &[(DialogResourceId, WolPageArt)],
    dialog: DialogResourceId,
) -> Option<&crate::render::main_menu_shell_chrome::WolPageArt> {
    wol_art
        .iter()
        .find(|(id, _)| *id == dialog)
        .map(|(_, art)| art)
}

/// Frees `dialog`'s art (its page's teardown).
pub(crate) fn free_art(
    wol_art: &mut Vec<(DialogResourceId, WolPageArt)>,
    dialog: DialogResourceId,
) {
    wol_art.retain(|(id, _)| *id != dialog);
}

/// `dialog`'s root while it takes the input: the screen's top dialog, or the top overlay over it.
pub(crate) fn root(controller: &DialogController, dialog: DialogResourceId) -> Option<NodeId> {
    let takes = |top: Option<DialogResourceId>| top == Some(dialog);
    controller
        .root_in(dialog)
        .filter(|_| takes(controller.top_id()) || takes(controller.overlay_id()))
}

/// Builds `dialog`'s widgets once and puts them on the layout; the root.
pub(super) fn sync(
    controller: &mut DialogController,
    font: &BitFont,
    dialog: DialogResourceId,
    scale: ShellScale,
    spec: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
) -> Option<NodeId> {
    let root = place(controller, dialog, scale, spec, rect_of)?;
    // Only the focused edit takes keys, so only it scrolls; an assigned text
    // shows from its start.
    if let Some(node) = controller.gui().focus() {
        scroll_edit(controller, font, node);
    }
    Some(root)
}

/// [`sync`] without scrolling the edits, for a screen that scrolls them
/// itself ([`scroll_edit`]).
pub(super) fn place(
    controller: &mut DialogController,
    dialog: DialogResourceId,
    scale: ShellScale,
    spec: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
) -> Option<NodeId> {
    let root = root(controller, dialog)?;
    controller.set_scale_in(dialog, scale);
    page::ensure_built(controller.gui_mut(), root, spec);
    page::sync_rects(controller.gui_mut(), root, rect_of);
    Some(root)
}

/// NewEdit keeps its caret in view (text widths in the box's authored
/// pixels). Anything but an edit is left alone.
pub(super) fn scroll_edit(controller: &mut DialogController, font: &BitFont, node: NodeId) {
    let gui = controller.gui_mut();
    let _ = gui.keep_caret_in_view(node, &|text| font.text_width(text));
}

/// Control `id`'s text under `dialog`'s root ("" when it has none).
pub(crate) fn text(controller: &DialogController, dialog: DialogResourceId, id: u16) -> String {
    controller.text(dialog, id)
}

/// Whether control `id` under `dialog`'s root is there and enabled.
pub(crate) fn enabled(controller: &DialogController, dialog: DialogResourceId, id: u16) -> bool {
    controller.control(dialog, id).is_some() && controller.flag(dialog, id, Prop::Enabled, true)
}

/// The edit holding the top dialog's keyboard focus, by control id.
pub(crate) fn focused(controller: &DialogController) -> Option<u16> {
    let gui = controller.gui();
    gui.id_of(gui.focus()?)
        .and_then(|id| u16::try_from(id).ok())
}

/// `SetFocus` on edit `id` under `dialog`'s root (`None` clears the focus).
pub(super) fn focus(controller: &mut DialogController, dialog: DialogResourceId, id: Option<u16>) {
    controller.focus_control(dialog, id);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pointer {
    Move,
    Down,
    Up,
}

/// The pointer at the cursor into the top dialog's widgets: their cues play
/// (a list row's GenericClick is the page's to play) and the rest returns.
pub(super) fn pointer(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    at: Pointer,
) -> Vec<PageEvent> {
    let events = pointer_quiet(
        frontend,
        window,
        match_state,
        process_assets,
        sfx_player,
        at,
    );
    frontend.shell_status_line.repaint();
    events
}

/// [`pointer`] without repainting the status line, for a dialog whose hit
/// test writes its status help only on a move (a hover message, `0x4B2`).
pub(super) fn pointer_quiet(
    frontend: &mut FrontendState,
    window: &Window,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    at: Pointer,
) -> Vec<PageEvent> {
    let (x, y) = match_state.input.cursor_px();
    let controller = &mut frontend.shell_controller;
    match at {
        Pointer::Move => controller.on_pointer_move(x, y),
        Pointer::Down => controller.on_pointer_down(x, y),
        Pointer::Up => {
            controller.on_pointer_up(x, y);
        }
    }
    let events = App::shell_page_events(frontend, match_state, process_assets, sfx_player, false);
    window.request_redraw();
    events
}

/// The open page's widget timers (a list's scroll-arrow repeat); the next
/// deadline.
pub(super) fn poll(
    controller: &mut DialogController,
    window: &Window,
    open: bool,
) -> Option<crate::util::time::Instant> {
    if !open {
        return None;
    }
    controller.poll_then(|| window.request_redraw())
}

/// The hovered control's status help on `dialog`, under a box too.
pub(crate) fn status_key(controller: &DialogController, dialog: DialogResourceId) -> Option<&str> {
    page::hovered_key_in(controller.gui(), controller.root_in(dialog)?)
}

/// The widgets under `dialog`'s root through the generic painter.
pub(crate) fn paint(
    frontend: &FrontendState,
    ime: &ImeState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    bit_font: &BitFont,
    dialog: DialogResourceId,
    scale: ShellScale,
) -> crate::render::hud_paint::HudPaint {
    let controller = &frontend.shell_controller;
    match controller.root_in(dialog) {
        Some(root) => gui_paint::paint_gui_children(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            controller.gui(),
            root,
            scale,
        ),
        None => Default::default(),
    }
}
