//! BC/BD/C2/C9 window input adapter. The dialog produces intents;
//! MatchSession and the setup lifecycle adapter decide admission and effects.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::shell_route::ShellRoute;
use crate::app::{App, AppState};
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::page::{PageCue, PageEvent};
use crate::util::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

/// The room's page in the window, before VERA's roster is placed in it.
pub(in crate::app) fn page_layout(state: &AppState) -> MultiplayerSetupLayout {
    let (w, h) = shell_layout::shell_screen_size(state);
    MultiplayerSetupLayout::new(
        crate::app::shell_network_setup::dialog(state),
        w as i32,
        h as i32,
        shell_layout::fills_window(state),
    )
}

/// Where VERA's roster's entries fit in this window: over the team bar,
/// which holds the Teams count in a room with teams and the host's Room
/// size, and two groups abreast on a wide one.
pub(crate) fn roster_frame(
    state: &AppState,
    teams: bool,
    room_size: bool,
) -> crate::ui::multiplayer_setup::RosterFrame {
    let mut layout = page_layout(state);
    layout.fit_team_bar(
        crate::app::shell_network_setup::dialog(state),
        teams,
        room_size,
    );
    layout.roster_frame()
}

pub(crate) fn layout(state: &AppState) -> MultiplayerSetupLayout {
    match crate::app::shell_network_setup::view(state) {
        Some(view) => layout_of(state, &view),
        None => page_layout(state),
    }
}

/// The room's layout for `view`: VERA's roster placed with its team bar,
/// or the rows of a native roster of several pages moved up for Previous
/// and Next.
pub(crate) fn layout_of(state: &AppState, view: &SetupView) -> MultiplayerSetupLayout {
    let mut layout = page_layout(state);
    if view.dialog.modern {
        layout.arrange_roster(&view.places);
        let shown = |id| view.trackbars.iter().any(|bar| bar.id == id && bar.visible);
        layout.fit_team_bar(view.dialog, shown(TEAM_COUNT), shown(ROOM_SIZE));
    } else if view.pages.len() > 1 {
        layout.arrange_extended_rows();
    }
    layout
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = page_layout(state);
    let page = crate::app::shell_network_setup::dialog(state).page().dialog;
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    // VERA's room's chat column changes width with the window: its rows
    // wrap again to fit it; the log's list keeps its view.
    let font = &state.renderer.bit_font;
    let width = crate::ui::lan_shell::chat_wrap_width(
        layout.chat_log,
        layout.page.scale,
        layout.chat_text_scale(),
    );
    let measure = |units: &[u16]| font.text_width(&String::from_utf16_lossy(units)) as i32;
    let controller = &mut state.frontend.shell_controller;
    if ui.chat.rewrap(width, measure) {
        let rows = ui.chat.lines.len();
        match (controller.control(page, CHAT_LOG), &mut ui.parked_chat) {
            (Some(log), _) => {
                let _ = controller.gui_mut().log_rows(log, rows, 0);
            }
            (None, Some(parked)) => parked.log(rows, 0, layout.chat_visible_rows()),
            (None, None) => {}
        }
    }
    // NewEdit keeps the chat's caret in view.
    if let Some(chat) = controller.control(page, CHAT) {
        let _ = controller
            .gui_mut()
            .keep_caret_in_view(chat, &|text| font.text_width(text));
    }
    // The widgets on screen follow the session between inputs too: a list
    // whose row moved away closes before it paints, a box opens.
    synced(state, false);
}

/// The room hides behind the lobby and its dialog goes: its chat draft and
/// log view park on the room's state ([`unpark_chat`]).
pub(crate) fn park_chat(state: &mut AppState) {
    let page = crate::app::shell_network_setup::dialog(state).page().dialog;
    let controller = &state.frontend.shell_controller;
    let (Some(edit), Some(log)) = (
        controller.control(page, CHAT),
        controller.control(page, CHAT_LOG),
    ) else {
        return;
    };
    let gui = controller.gui();
    let view = gui.list_view(log);
    let parked = crate::ui::lan_shell::ParkedChat {
        edit: gui.edit_state(edit).unwrap_or_default(),
        top: view.map_or(0, |view| view.top),
        rows: view.map_or(0, |view| view.rows),
    };
    // VERA's roster's place, which its scrollbar node keeps.
    let roster_top = controller
        .control(page, ROSTER_SCROLL)
        .and_then(|scroll| gui.list_view(scroll))
        .map(|view| view.top);
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.parked_chat = Some(parked);
        ui.parked_roster_top = roster_top;
    }
}

/// The room shows again: its rebuilt chat edit and log take the parked
/// draft and view back, as the room left them, and VERA's roster its place.
pub(crate) fn unpark_chat(state: &mut AppState) {
    let Some((parked, roster_top)) = state
        .frontend
        .multiplayer_setup
        .as_mut()
        .and_then(|ui| Some((ui.parked_chat.take()?, ui.parked_roster_top.take())))
    else {
        return;
    };
    let Some((view, _)) = synced(state, true) else {
        return;
    };
    let page = view.page().dialog;
    if let Some(top) = roster_top {
        let top = vera20k_gui::Value::Int(top as i32);
        let controller = &mut state.frontend.shell_controller;
        controller.set_prop(page, ROSTER_SCROLL, vera20k_gui::Prop::Top, top);
        // Its rows' controls follow the place before they take input.
        synced(state, false);
    }
    let controller = &mut state.frontend.shell_controller;
    if let Some(edit) = controller.control(page, CHAT) {
        let _ = controller.gui_mut().set_edit_state(edit, &parked.edit);
    }
    if let Some(log) = controller.control(page, CHAT_LOG) {
        let gui = controller.gui_mut();
        let _ = gui.set(
            log,
            vera20k_gui::Prop::RowCount,
            vera20k_gui::Value::Int(parked.rows as i32),
        );
        let _ = gui.set(
            log,
            vera20k_gui::Prop::Top,
            vera20k_gui::Value::Int(parked.top as i32),
        );
    }
}

/// The room's chat edit `0x540`'s text, as its widget holds it.
pub(crate) fn chat_text(state: &AppState) -> String {
    let page = crate::app::shell_network_setup::dialog(state).page().dialog;
    state.frontend.shell_controller.text(page, CHAT)
}

/// Whether the room's map thumbnail shows a picture with nothing over it: no
/// combo list, box (Game Options, the missing-map offer, a notice).
fn preview_box_shows(state: &AppState, view: &SetupView) -> bool {
    state.frontend.shell_controller.top_id() == Some(view.page().dialog)
        && state.frontend.skirmish_preview_texture.is_some()
        && state.frontend.multiplayer_setup.is_some()
        && open_list(state).is_none()
}

fn notice_active(state: &AppState) -> bool {
    state
        .frontend
        .multiplayer_setup
        .as_ref()
        .is_some_and(|ui| ui.notice.is_some())
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if !event.state.is_pressed() || shell_transition::blocks_shell_input(state) {
        return;
    }
    let code = if let PhysicalKey::Code(code) = event.physical_key {
        Some(code)
    } else {
        None
    };
    key_press(state, code, event.text.as_deref());
}

/// [`key_press`] reaches the room: no verify box, page dialog or notice is
/// over it.
fn room_takes_keys(state: &AppState) -> bool {
    crate::app::shell_network_setup::verify(state).is_none()
        && !crate::app::shell_wol::page::open(state)
        && !notice_active(state)
        && crate::app::shell_network_setup::view(state).is_some()
}

/// VERA's Esc menu (`app::shell_menu`): the boxes and popups over the room
/// keep Esc, and a locked room is about to launch.
pub(crate) fn leaves_escape(state: &AppState) -> bool {
    room_takes_keys(state)
        && crate::app::shell_network_setup::view(state).is_some_and(|view| !view.locked)
        && state
            .frontend
            .multiplayer_setup
            .as_ref()
            .is_some_and(|ui| !ui.game_options && ui.map_offer.is_none())
}

/// The chat NewEdit while it takes keys, for the input method
/// (`app::input::ime`): [`key_press`] reaches the focused chat with no
/// verify box, page dialog, notice or ChooseMap over the dialog.
pub(crate) fn ime_chat_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
        && state.frontend.shell_route == ShellRoute::NetworkSetup
        && state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
        && !shell_transition::blocks_shell_input(state)
        && room_takes_keys(state);
    state
        .frontend
        .multiplayer_setup
        .as_ref()
        .filter(|ui| routed && !ui.game_options && ui.map_offer.is_none())
        .map(|_| page_layout(state).chat)
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    // WOL Start Game Now's 0x130 has no key handling; the locked C2/C9
    // under it takes none either.
    if crate::app::shell_network_setup::verify(state).is_some() {
        return;
    }
    // The page dialog 0xCF runs its own modal loop over C2/C9.
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::key_press(state, code, text);
        return;
    }
    // The native 5D3490 message box owns all input while its CE runs, and
    // VERA's boxes (Game Options, the map offer) take the keys before the
    // chat: their Default/Cancel buttons answer Enter and Escape.
    let boxed = notice_active(state)
        || state
            .frontend
            .multiplayer_setup
            .as_ref()
            .is_some_and(|ui| ui.game_options || ui.map_offer.is_some());
    if boxed {
        let key = code.and_then(|code| crate::ui::shell::controller::gui_key(code, false));
        if let Some(key) = key
            && let Some((view, _)) = synced(state, true)
        {
            state
                .frontend
                .shell_controller
                .on_input(vera20k_gui::InputEvent::Key(key));
            handle_events(state, &view);
        }
        return;
    }
    let Some((view, _)) = synced(state, true) else {
        return;
    };
    // NewEdit consumes all keyboard input while chat has focus. Escape is
    // inert there; a free Escape opens VERA's Esc menu before it gets here.
    // Enter appends CRLF before shared5EDD00/5DE360 submission.
    let controller = &mut state.frontend.shell_controller;
    let chat = crate::ui::lan_shell::chat_key(controller, view.page().dialog, code, text);
    let action = chat.map(SetupAction::Chat);
    if let Some(action) = action {
        crate::app::shell_network_setup::apply_action(state, action);
    }
}

/// The room's view and layout, its widgets synced to them; with
/// `activate`, input routed to the room makes its page the active dialog.
fn synced(state: &mut AppState, activate: bool) -> Option<(SetupView, MultiplayerSetupLayout)> {
    let view = crate::app::shell_network_setup::view(state)?;
    let layout = layout_of(state, &view);
    let ui = state.frontend.multiplayer_setup.as_ref()?;
    sync(
        &mut state.frontend.shell_controller,
        ui,
        &view,
        &layout,
        activate,
    )?;
    // The preview box `0x468` is there only while it shows a picture.
    let shows = preview_box_shows(state, &view);
    let page = view.page().dialog;
    let controller = &mut state.frontend.shell_controller;
    controller.set_prop(
        page,
        MAP_PREVIEW,
        vera20k_gui::Prop::Visible,
        vera20k_gui::Value::Bool(shows),
    );
    Some((view, layout))
}

/// The open list over the room: its combo and the list as it paints.
pub(crate) fn open_list(state: &AppState) -> Option<(u16, vera20k_gui::ComboDropDown)> {
    let controller = &state.frontend.shell_controller;
    let page = crate::app::shell_network_setup::dialog(state).page().dialog;
    (controller.top_id() == Some(page))
        .then(|| crate::ui::multiplayer_setup::open_list(controller.gui()))
        .flatten()
}

/// Whether a trackbar of the room holds the pointer.
pub(crate) fn slider_held(state: &AppState) -> bool {
    crate::ui::skirmish_shell::page::slider_held(state.frontend.shell_controller.gui())
}

/// What the room paints of its widgets at the pointer: VERA's roster
/// lights the control a press works, the original rooms the one under it.
pub(crate) fn room_look(state: &AppState, view: &SetupView) -> RoomLook {
    if state.frontend.multiplayer_setup.is_none() {
        return RoomLook::default();
    }
    let controller = &state.frontend.shell_controller;
    let carrying = row_drag(controller, view).is_some_and(|drag| drag.carrying);
    let still = carrying || slider_held(state) || notice_active(state);
    let target = pointer_target(state, view, !view.dialog.modern).filter(|_| !still);
    look(&state.frontend.shell_controller, view, target)
}

/// The control at the pointer that a press works (enabled) on the top
/// dialog; with `help`, the one whose status help shows (VERA's roster
/// keeps the press's).
fn pointer_target(state: &AppState, view: &SetupView, help: bool) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &state.frontend.shell_controller;
    let root = controller.root_in(controller.top_id()?)?;
    let at = |press| control_at(controller.gui(), root, x, y, press);
    match help {
        true => at(false),
        false => at(true).filter(|id| view.enabled(*id)),
    }
}

/// The room loses the pointer: a box over it, a page of its roster or the
/// missing-map offer takes it, or the window loses the focus. Its open list
/// closes too, but for the focus.
pub(crate) fn drop_pointer(state: &mut AppState, close_list: bool) {
    let page = crate::app::shell_network_setup::dialog(state).page().dialog;
    let controller = &mut state.frontend.shell_controller;
    if close_list && controller.top_id() == Some(page) {
        controller.gui_mut().close_popup();
    }
    // A carried roster row goes with the pointer.
    for dialog in [page, GAME_OPTIONS_BOX, MAP_OFFER_BOX] {
        controller.drop_pointer_in(dialog);
    }
}

/// The widgets' cues, then the session actions they asked for, in order
/// (the roster's scrollbar and the chat log keep their own tops; the chat
/// log's rows take no selection).
fn handle_events(state: &mut AppState, view: &SetupView) {
    if state.frontend.multiplayer_setup.is_none() {
        return;
    }
    for event in App::shell_page_events(state, false) {
        match event {
            PageEvent::Activated(crate::ui::shell::modal::control::OK) => {
                crate::app::shell_network_setup::acknowledge_notice(state)
            }
            // VERA's roster scrolled (its node keeps the place): its rows'
            // controls move under the pointer, so an open list closes.
            PageEvent::Scrolled(ROSTER_SCROLL) => {
                state.frontend.shell_controller.gui_mut().close_popup();
            }
            // The host carried a roster row off its cell: its list closed.
            PageEvent::DragStarted(_) => {
                if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
                    ui.popup_row = None;
                }
            }
            // ... and dropped it on a box that takes it.
            PageEvent::Dropped(cell, Some(target)) => {
                let row = view.combo(cell).map(|combo| combo.row);
                let header = usize::try_from(i32::from(target) - i32::from(GROUP_BOX)).ok();
                let group = row
                    .zip(header)
                    .and_then(|(row, header)| Some((row, view.drop_group(row, header)?)));
                if let Some((row, group)) = group {
                    App::play_page_cue(state, PageCue::Button);
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::DropRow { row, group },
                    );
                }
            }
            // A team bar list stays open on an item it cannot have now
            // (the caption, a grey item); the session ignores the pick.
            PageEvent::Picked(control @ (TEAM_MENU | SEAT_MENU), row)
                if view.combo(control).is_some_and(|combo| {
                    usize::try_from(row)
                        .ok()
                        .and_then(|row| combo.choices.get(row))
                        .is_none_or(|choice| choice.greyed)
                }) =>
            {
                let controller = &mut state.frontend.shell_controller;
                if let Some(list) = controller.control(view.page().dialog, control) {
                    let _ = controller.gui_mut().open_popup(list, 0);
                }
            }
            event => {
                let page = state
                    .frontend
                    .multiplayer_setup
                    .as_ref()
                    .map_or_else(RoomBox::default, |ui| ui.game_options_page);
                if let Some(action) = action(view, page, event) {
                    crate::app::shell_network_setup::apply_action(state, action)
                }
            }
        }
    }
    state.platform.window.request_redraw();
}

/// The status help of the control under the pointer at `x`, `y`.
fn hovered_help(state: &AppState, view: &SetupView, layout: &MultiplayerSetupLayout) -> String {
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &state.frontend.shell_controller;
    let over = controller
        .top_id()
        .and_then(|top| controller.root_in(top))
        .and_then(|root| control_at(controller.gui(), root, x, y, false));
    // `0x468` is no control the room's hit test finds; VERA's enlarged preview
    // gives the box its thumbnail help and the hint.
    let over_preview =
        over.is_none() && layout.map_preview.contains(x, y) && preview_box_shows(state, view);
    // VERA's online room names the badge, rank and OS of the player whose
    // name or painted badge (no control) is under the pointer.
    let badge = (controller.top_id() == Some(view.page().dialog))
        .then(|| layout.badge_at(view, x, y))
        .flatten();
    let player = badge
        .or(over)
        .and_then(|id| crate::app::shell_network_setup::view::player_help(state, view, id));
    let help = player
        .or_else(|| {
            over.or(over_preview.then_some(MAP_PREVIEW))
                .and_then(|id| tooltip_key(view.dialog, id))
                .and_then(|key| {
                    state
                        .process_assets
                        .csf
                        .as_ref()
                        .and_then(|csf| csf.get(&key))
                        .map(str::to_owned)
                })
        })
        .unwrap_or_default();
    if over_preview {
        crate::app::frontend::preview_zoom::box_help(state, help)
    } else {
        help
    }
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    if crate::app::shell_network_setup::verify(state).is_some() {
        crate::app::shell_network_setup::verify_pointer(state, None);
        return;
    }
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::cursor_moved(state);
        return;
    }
    let Some((view, layout)) = synced(state, true) else {
        return;
    };
    let (x, y) = state.match_state.input.cursor_px();
    // A held trackbar keeps the pointer from the status help too.
    let held = slider_held(state) || notice_active(state);
    state.frontend.shell_controller.on_pointer_move(x, y);
    if !held {
        let help = hovered_help(state, &view, &layout);
        let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
            return;
        };
        ui.statics.hover(&help, Instant::now());
    }
    handle_events(state, &view);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    if crate::app::shell_network_setup::verify(state).is_some() {
        crate::app::shell_network_setup::verify_pointer(state, Some(pressed));
        return;
    }
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::mouse(state, pressed);
        return;
    }
    let Some((view, layout)) = synced(state, true) else {
        return;
    };
    let (x, y) = state.match_state.input.cursor_px();
    if pressed {
        let over_preview = crate::app::frontend::preview_zoom::over_box(state, view.page().dialog);
        if pointer_target(state, &view, false).is_none()
            && preview_box_shows(state, &view)
            && crate::app::frontend::preview_zoom::press_on_box(state, over_preview)
        {
            return;
        }
        let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
            return;
        };
        press(
            &mut state.frontend.shell_controller,
            ui,
            &view,
            &layout,
            x,
            y,
        );
    } else {
        // A carried roster row drops (`PageEvent::Dropped`, below).
        state.frontend.shell_controller.on_pointer_up(x, y);
    }
    handle_events(state, &view);
}

/// Original scrollbar timers: one callback, no catch-up burst; no wheel or
/// arrow-key path exists in the native ComboDropWin/list subclasses.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    if crate::app::shell_network_setup::verify(state).is_some() {
        return crate::app::shell_network_setup::verify_poll_scroll(state);
    }
    if notice_active(state) {
        return None;
    }
    let (view, _) = synced(state, false)?;
    // Another screen's dialog over the room polls its own widgets.
    let controller = &state.frontend.shell_controller;
    if ![view.page().dialog, GAME_OPTIONS_BOX, MAP_OFFER_BOX]
        .map(Some)
        .contains(&controller.top_id())
    {
        return None;
    }
    let deadline = state.frontend.shell_controller.poll();
    handle_events(state, &view);
    deadline
}

/// Route wheel input to VERA's room when it owns the pointer.
pub(crate) fn wheel(state: &mut AppState, lines: f32) -> bool {
    if lines == 0.0
        || notice_active(state)
        || crate::app::shell_network_setup::verify(state).is_some()
        || crate::app::shell_wol::page::open(state)
    {
        return false;
    }
    let Some((view, layout)) = synced(state, true) else {
        return false;
    };
    let (x, y) = state.match_state.input.cursor_px();
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return false;
    };
    let controller = &mut state.frontend.shell_controller;
    let area = controller.control(view.page().dialog, ROSTER_AREA);
    let scroll = controller.control(view.page().dialog, ROSTER_SCROLL);
    let now = controller.now();
    let taken = ui.wheel(
        controller.gui_mut(),
        area,
        scroll,
        &view,
        &layout,
        x,
        y,
        lines,
        now,
    );
    handle_events(state, &view);
    taken
}
