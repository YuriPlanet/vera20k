//! BC6 (`0xBC6`) window input adapter. The dialog produces intents;
//! `app::cooperative_round` and `MatchSession` decide their effects.

use crate::app::frontend::state::FrontendState;
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{App, AppState};
use crate::audio::sfx::SfxPlayer;
use crate::render::gpu::GpuContext;
use crate::ui::coop_score_shell::*;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::page::PageEvent;
use crate::util::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

pub(crate) fn layout(gpu: &GpuContext) -> CoopScoreLayout {
    let (w, h) = shell_layout::shell_screen_size(gpu);
    CoopScoreLayout::new(w as i32, h as i32, shell_layout::fills_window(gpu))
}

/// BC6 owns the result screen while its page is open.
pub(crate) fn active(frontend: &FrontendState) -> bool {
    matches!(
        frontend.screen,
        crate::app::GameScreen::MissionResult { .. }
    ) && frontend.coop_score_page.is_some()
}

/// BC6's page is the active dialog, its nodes on `layout` and the round:
/// a control the round hides has no rect, the lists and the slider show
/// the slots' picks, the flags their PCX. The chat edit holds the focus
/// throughout (BC6 never moves it), the transcript's list follows new
/// lines, and NewEdit keeps the chat's caret in view.
fn sync(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    layout: &CoopScoreLayout,
) {
    let view = crate::app::cooperative_round::view(frontend, match_state, process_assets);
    let page = frontend.coop_score_page.as_ref();
    let can_continue = page.is_some_and(|page| page.model.can_continue);
    let enabled = [CONTINUE, QUIT].map(|id| (id, enabled_in(view.as_ref(), page, id)));
    let rows = page.map_or(0, |page| page.ui.chat.lines.len());
    let slots = view.as_ref().map_or([false; 2], |view| {
        view.slots.each_ref().map(Option::is_some)
    });
    let font = &renderer.bit_font;
    let controller = &mut frontend.shell_controller;
    controller.ensure_active(COOP_SCORE_DIALOG);
    controller.set_scale_in(COOP_SCORE_DIALOG, layout.page.scale);
    let rect_of = |id| match id {
        TITLE => Some(layout.page.title),
        STATUS => Some(layout.page.status_help),
        id => layout
            .control_rect(id)
            .filter(|_| shown(id, can_continue, slots)),
    };
    crate::ui::shell::menu_page::sync_nodes(
        controller,
        COOP_SCORE_DIALOG,
        nodes,
        rect_of,
        &enabled,
    );
    if let (Some(view), Some(root)) = (&view, controller.root_in(COOP_SCORE_DIALOG)) {
        sync_values(controller.gui_mut(), root, view);
    }
    if controller.focused_control(COOP_SCORE_DIALOG).is_none() {
        controller.focus_control(COOP_SCORE_DIALOG, Some(CHAT));
    }
    if let Some(log) = controller.control(COOP_SCORE_DIALOG, CHAT_LOG) {
        let _ = controller.gui_mut().log_rows(log, rows, 0);
    }
    if let (Some(chat), Some(_)) = (
        controller.control(COOP_SCORE_DIALOG, CHAT),
        layout.control_rect(CHAT),
    ) {
        let _ = controller
            .gui_mut()
            .keep_caret_in_view(chat, &|text| font.text_width(text));
    }
}

/// The round's picks on BC6's nodes, in one pass over them, each written
/// only on a change: Difficulty's position, the flags' PCX and the lists'
/// swatches (a colour's pick shows its swatch alone).
fn sync_values(
    gui: &mut vera20k_gui::Gui,
    root: vera20k_gui::NodeId,
    view: &crate::app::cooperative_round::CoopScoreView,
) {
    use crate::ui::shell::page::{set_if_changed, set_node_text};
    use vera20k_gui::{Prop, Value};
    let count = gui.tree().children(root).map_or(0, <[_]>::len);
    for index in 0..count {
        let Some(node) = gui
            .tree()
            .children(root)
            .ok()
            .and_then(|children| children.get(index).copied())
        else {
            break;
        };
        let Some(id) = gui.id_of(node).and_then(|id| u16::try_from(id).ok()) else {
            continue;
        };
        if id == DIFFICULTY {
            set_if_changed(gui, node, Prop::Pos, Value::Int(view.difficulty));
            continue;
        }
        let slot = |ids: [u16; 2]| {
            let row = ids.iter().position(|each| *each == id)?;
            view.slots[row].as_ref()
        };
        if FLAGS.contains(&id) {
            let flag = slot(FLAGS).and_then(|slot| slot.flag.as_deref());
            set_node_text(gui, node, Prop::Art, flag.unwrap_or_default());
            continue;
        }
        let choice = match (slot(COUNTRIES), slot(COLORS), slot(STARTS)) {
            (Some(slot), _, _) => &slot.country,
            (_, Some(slot), _) => &slot.color,
            (_, _, Some(slot)) => &slot.start,
            _ => continue,
        };
        let same = match (gui.get(node, Prop::Swatches).ok().flatten(), choice.swatch) {
            (Some(Value::Items(items)), Some(swatch)) => {
                items.len() == 1 && items[0].parse::<usize>() == Ok(swatch)
            }
            (Some(Value::Items(items)), None) => items.is_empty(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            let items = choice.swatch.map(|swatch| vec![swatch.to_string()]);
            let _ = gui.set(
                node,
                Prop::Swatches,
                Value::Items(items.unwrap_or_default()),
            );
        }
    }
}

pub(crate) fn update_geometry(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) {
    if active(frontend) {
        let layout = layout(&renderer.gpu);
        sync(renderer, frontend, match_state, process_assets, &layout);
    }
}

/// Whether control `id` takes input now (`0x0046FB50`, `0x0046DE20`).
fn enabled(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    id: u16,
) -> bool {
    let view = crate::app::cooperative_round::view(frontend, match_state, process_assets);
    enabled_in(view.as_ref(), frontend.coop_score_page.as_ref(), id)
}

/// [`enabled`] on the round's `view`.
fn enabled_in(
    view: Option<&crate::app::cooperative_round::CoopScoreView>,
    page: Option<&CoopScorePage>,
    id: u16,
) -> bool {
    let (Some(view), Some(page)) = (view, page) else {
        return false;
    };
    if !page.model.can_continue && hidden_on_final_mission(id) {
        return false;
    }
    match id {
        CONTINUE => view.continue_enabled,
        QUIT => view.quit_enabled,
        CHAT | CHAT_LOG => true,
        _ => false,
    }
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if !event.state.is_pressed() || shell_transition::blocks_shell_input(&state.frontend) {
        return;
    }
    let code = if let PhysicalKey::Code(code) = event.physical_key {
        Some(code)
    } else {
        None
    };
    key_press(state, code, event.text.as_deref());
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if state.frontend.coop_score_page.is_none() {
        return;
    }
    let layout = layout(&state.renderer.gpu);
    sync(
        &state.renderer,
        &mut state.frontend,
        &state.match_state,
        &state.process_assets,
        &layout,
    );
    // The chat edit has the focus and takes every key; Enter submits.
    let controller = &mut state.frontend.shell_controller;
    let sent = crate::ui::lan_shell::chat_key(controller, COOP_SCORE_DIALOG, code, text);
    if let Some(action) = sent.map(CoopScoreAction::Chat) {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// One pointer message (`down`: `None` moves) on the page's nodes: hover
/// writes the status help, a press on an enabled Continue or Quit plays
/// GUIMainButtonSound, one on the chat edit keeps its focus and one on the
/// log's scrollbar scrolls it (its rows take no click sound). Returns the
/// activated button.
fn pointer(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    layout: &CoopScoreLayout,
    down: Option<bool>,
) -> Option<u16> {
    sync(renderer, frontend, match_state, process_assets, layout);
    let (x, y) = match_state.input.cursor_px();
    let event = crate::app::input::dialog_page::event_at(x, y, down);
    frontend.shell_controller.on_input(event);
    let events = App::shell_page_events(frontend, match_state, process_assets, sfx_player, false);
    events.into_iter().rev().find_map(|event| match event {
        PageEvent::Activated(id) => Some(id),
        _ => None,
    })
}

pub(crate) fn cursor_moved(
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
) {
    let layout = layout(&renderer.gpu);
    pointer(
        renderer,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        &layout,
        None,
    );
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(&state.frontend) {
        return;
    }
    if crate::app::cooperative_round::view(
        &state.frontend,
        &state.match_state,
        &state.process_assets,
    )
    .is_none()
        || state.frontend.coop_score_page.is_none()
    {
        return;
    }
    let layout = layout(&state.renderer.gpu);
    let controller = &state.frontend.shell_controller;
    let over =
        crate::ui::shell::menu_page::hovered_id(controller, COOP_SCORE_DIALOG).filter(|id| {
            enabled(
                &state.frontend,
                &state.match_state,
                &state.process_assets,
                *id,
            )
        });
    let action = match pointer(
        &state.renderer,
        &mut state.frontend,
        &state.match_state,
        &state.process_assets,
        &mut state.audio.sfx_player,
        &layout,
        Some(pressed),
    ) {
        Some(CONTINUE) => Some(CoopScoreAction::Continue),
        Some(QUIT) => Some(CoopScoreAction::Quit),
        _ => None,
    };
    // A click over a control that takes no input leaves no status help
    // until the pointer moves again.
    if over.is_none() {
        state.frontend.shell_controller.gui_mut().cancel_pointer();
    }
    if let Some(action) = action {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// The chat list's scrollbar repeat timer.
pub(crate) fn poll(window: &Window, controller: &mut DialogController) -> Option<Instant> {
    controller.poll_then(|| window.request_redraw())
}
