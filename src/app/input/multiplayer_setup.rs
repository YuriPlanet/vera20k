//! BC/BD/C2/C9 window input adapter. The dialog produces intents;
//! MatchSession and the setup lifecycle adapter decide admission and effects.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::shell_route::ShellRoute;
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanEditKey;
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::page::PageCue;
use crate::util::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

/// The room's page in the window, before VERA's roster is placed in it.
fn page_layout(state: &AppState) -> MultiplayerSetupLayout {
    let (w, h) = shell_layout::shell_screen_size(state);
    MultiplayerSetupLayout::new(
        crate::app::shell_network_setup::dialog(state),
        w as i32,
        h as i32,
        shell_layout::fills_window(state),
    )
}

/// Where VERA's roster's entries end in this window, in 800x600 pixels:
/// over the team bar, which holds the Teams count in a room with teams and
/// the host's Room size.
pub(crate) fn roster_bottom(state: &AppState, teams: bool, room_size: bool) -> i32 {
    let mut layout = page_layout(state);
    layout.fit_team_bar(
        crate::app::shell_network_setup::dialog(state),
        teams,
        room_size,
    );
    layout.roster_bottom
}

pub(crate) fn layout(state: &AppState) -> MultiplayerSetupLayout {
    let dialog = crate::app::shell_network_setup::dialog(state);
    let mut layout = page_layout(state);
    if dialog.modern {
        if let Some(listing) = crate::app::shell_network_setup::view::room_listing(state) {
            layout.arrange_roster(&listing.places);
            layout.fit_team_bar(
                dialog,
                listing.grouped,
                crate::app::shell_network_setup::view::room_size_shown(state),
            );
        }
    } else if state
        .match_state
        .session
        .network_setup_capacity()
        .is_some_and(|rows| rows > dialog.rows_per_page() as i32)
    {
        layout.arrange_extended_rows();
    }
    layout
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    // VERA's room's chat column changes width with the window: its rows
    // wrap again to fit it.
    let visible = layout
        .chat_list(&ui.chat.list, ui.chat.lines.len())
        .visible_rows;
    ui.chat.rewrap(
        crate::ui::lan_shell::chat_wrap_width(
            layout.chat_log,
            layout.page.scale,
            layout.chat_text_scale(),
        ),
        visible,
        |units| {
            state
                .renderer
                .bit_font
                .text_width(&String::from_utf16_lossy(units)) as i32
        },
    );
    ui.chat.list.reconcile_geometry_for_text(
        layout.chat_log,
        ui.chat.lines.len(),
        layout.page.scale,
        layout.chat_text_scale(),
    );
    ui.chat.edit.scroll_to_caret(
        (layout.page.scale.base_rect(layout.chat).w - 3).max(0) as u32,
        |text| state.renderer.bit_font.text_width(text),
    );
    // The widgets on screen follow the session between inputs too: a list
    // whose row moved away closes before it paints, a box opens.
    synced(state, false);
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
            .is_some_and(|ui| ui.open_box.is_none() && ui.map_offer.is_none())
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
        .filter(|ui| routed && ui.open_box.is_none() && ui.map_offer.is_none())
        .map(|_| layout(state).chat)
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
    // The native 5D3490 message box owns all input while its CE runs; its
    // OK takes Enter and Escape.
    if notice_active(state) {
        use crate::ui::shell::controller::ShellKey;
        if let Some(ShellKey::Enter | ShellKey::Escape) = code.and_then(App::shell_key_for_code) {
            crate::app::shell_network_setup::acknowledge_notice(state);
        }
        return;
    }
    if crate::app::shell_network_setup::view(state).is_none() {
        return;
    }
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    // NewEdit consumes all keyboard input while chat has focus. Escape is
    // inert there; a free Escape opens VERA's Esc menu before it gets here.
    // Enter appends CRLF before shared5EDD00/5DE360 submission. VERA's boxes
    // (Game Options, Teams) take the keys first and close on Escape or Enter.
    let action = if let Some(offer) = &ui.map_offer {
        match code {
            // Keys meant for the chat just before it opened do not answer it.
            _ if !offer.takes_keys(Instant::now()) => None,
            Some(KeyCode::Enter | KeyCode::NumpadEnter) if !offer.downloading => {
                Some(SetupAction::MapOffer { download: true })
            }
            Some(KeyCode::Escape) => Some(SetupAction::MapOffer { download: false }),
            _ => None,
        }
    } else if ui.open_box.is_some() {
        matches!(
            code,
            Some(KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter)
        )
        .then_some(SetupAction::OpenBox(None))
    } else {
        let key = match code {
            Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
            Some(KeyCode::Delete) => Some(LanEditKey::Delete),
            Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
            Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
            Some(KeyCode::Home) => Some(LanEditKey::Home),
            Some(KeyCode::End) => Some(LanEditKey::End),
            Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
            _ => None,
        };
        if let Some(key) = key {
            ui.chat.key(key).map(SetupAction::Chat)
        } else {
            if !matches!(code, Some(KeyCode::Escape | KeyCode::Tab))
                && let Some(text) = text
            {
                ui.chat.insert_text(text);
            }
            None
        }
    };
    if let Some(action) = action {
        crate::app::shell_network_setup::apply_action(state, action);
    }
}

/// The room's view and layout, its widgets synced to them; with
/// `activate`, input routed to the room makes its page the active dialog.
fn synced(state: &mut AppState, activate: bool) -> Option<(SetupView, MultiplayerSetupLayout)> {
    let view = crate::app::shell_network_setup::view(state)?;
    let layout = layout(state);
    let ui = state.frontend.multiplayer_setup.as_ref()?;
    sync(
        &mut state.frontend.shell_controller,
        ui,
        &view,
        &layout,
        activate,
    )?;
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

/// What the room paints of its widgets at the pointer.
pub(crate) fn room_look(state: &AppState, view: &SetupView) -> RoomLook {
    match state.frontend.multiplayer_setup.as_ref() {
        Some(ui) => look(&state.frontend.shell_controller, ui, view),
        None => RoomLook::default(),
    }
}

/// The control at the pointer that a press works (enabled) on the top
/// dialog; with `help`, the one whose status help shows (VERA's roster
/// keeps the press's).
fn pointer_target(state: &AppState, view: &SetupView, help: bool) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &state.frontend.shell_controller;
    let root = controller.root_in(controller.top_id()?)?;
    match help {
        true => control_at(controller.gui(), root, x, y, false),
        false => control_at(controller.gui(), root, x, y, true).filter(|id| view.enabled(*id)),
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
    for dialog in [page, GAME_OPTIONS_BOX, MAP_OFFER_BOX] {
        controller.drop_pointer_in(dialog);
    }
    if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
        ui.cancel_pointer();
    }
}

/// The widgets' cues and the session actions they asked for, in order.
fn handle_events(state: &mut AppState, view: &SetupView) {
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    for event in room_events(&mut state.frontend.shell_controller, ui, view) {
        match event {
            RoomEvent::Cue(cue) => App::play_page_cue(state, cue),
            RoomEvent::Action(action) => {
                crate::app::shell_network_setup::apply_action(state, action)
            }
            RoomEvent::Acknowledge => crate::app::shell_network_setup::acknowledge_notice(state),
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
    // name is under the pointer.
    let player =
        over.and_then(|id| crate::app::shell_network_setup::view::player_help(state, view, id));
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
        // The host carries a pressed roster row to another box.
        let carried = ui.drag_row(x, y, |row| {
            layout
                .group_box_at(x, y)
                .filter(|header| view.drop_group(row, *header).is_some())
        });
        if carried {
            state.frontend.shell_controller.gui_mut().close_popup();
        }
        ui.statics.hover(&help, Instant::now());
        let carrying = ui.row_drag.is_some_and(|drag| drag.carrying);
        let hovered = pointer_target(state, &view, !layout.roster).filter(|_| !carrying);
        if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
            ui.hovered = hovered;
        }
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
    let target = pointer_target(state, &view, false);
    if pressed {
        if target.is_none()
            && preview_box_shows(state, &view)
            && crate::app::frontend::preview_zoom::press_on_box(state, layout.map_preview)
        {
            return;
        }
        // A press on a button takes it as the control under the pointer.
        let page = view.page().dialog;
        let button = open_list(state).is_none()
            && [page, GAME_OPTIONS_BOX, MAP_OFFER_BOX]
                .into_iter()
                .any(|dialog| {
                    let controller = &state.frontend.shell_controller;
                    let node = target.and_then(|id| controller.control(dialog, id));
                    node.is_some_and(|node| {
                        controller.gui().tree().kind(node) == Ok(vera20k_gui::NodeKind::Button)
                    })
                });
        let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
            return;
        };
        if button {
            ui.hovered = target;
        }
        press(
            &mut state.frontend.shell_controller,
            ui,
            &view,
            &layout,
            x,
            y,
        );
    } else {
        let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
            return;
        };
        ui.hovered = target;
        let dropped = ui
            .drop_row()
            .and_then(|(row, header)| Some((row, view.drop_group(row, header)?)));
        state.frontend.shell_controller.on_pointer_up(x, y);
        if let Some((row, group)) = dropped {
            App::play_page_cue(state, PageCue::Button);
            crate::app::shell_network_setup::apply_action(
                state,
                SetupAction::DropRow { row, group },
            );
        }
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
    let gui = state.frontend.shell_controller.gui_mut();
    let taken = ui.wheel(gui, &view, &layout, x, y, lines);
    handle_events(state, &view);
    taken
}
