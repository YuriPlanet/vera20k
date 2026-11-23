//! The Buddy List runner `0x0079F140` (WOL mode 5) and `0x116`'s app
//! adapter. The runner lists the chat lobbies, leaves a channel it is in,
//! then loops on `0x116` (proc `0x0079E520`). Back returns to the mode in
//! `[0xA8B248]`; Create and Join end the dialog and run the lobby's Create
//! (`0x00789B60`) and Join (`0x00789960`) over the empty backdrop with the
//! runner's records ([`super::lobby`] with [`super::lobby::Runner::Buddy`]);
//! their stop paths and C2/C9 Back loop back to `0x116`. A buddy in a chat
//! lobby is joined by switching to mode 4. The model is
//! [`crate::ui::wol_buddy`]; the mirror is [`super::buddies`]. No RNG is
//! drawn on these paths.

use crate::app::shell_route::ShellRoute;
use crate::util::time::{Duration, Instant};

use super::WolPage;
use super::leaf::{self, Pointer};
use super::service;
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::shell::page::{PageCue, PageEvent};
use crate::ui::wol_buddy::*;
use crate::ui::wol_shell::WolMode;
use vera20k_gui::NodeId;
use winit::keyboard::KeyCode;

/// `SetTimer(dlg, 0, 1000)`.
const TIMER_PERIOD: Duration = Duration::from_millis(1000);
/// Refresh `0x704` stays disabled for `SetTimer(dlg, 0x704, 3000)`.
const REFRESH_HOLD: Duration = Duration::from_millis(3000);

/// `0x116` while it runs: its children and its two timers.
#[derive(Debug, Clone)]
pub(crate) struct BuddyDialog {
    pub(crate) ui: WolBuddyPage,
    timer_at: Instant,
    refresh_at: Option<Instant>,
}

pub(crate) fn page(state: &AppState) -> Option<&BuddyDialog> {
    match &state.frontend.wol.as_ref()?.page {
        WolPage::Buddy(dialog) => Some(dialog),
        _ => None,
    }
}

fn page_mut(state: &mut AppState) -> Option<&mut BuddyDialog> {
    match &mut state.frontend.wol.as_mut()?.page {
        WolPage::Buddy(dialog) => Some(dialog),
        _ => None,
    }
}

pub(super) fn page_open(state: &AppState) -> bool {
    page(state).is_some()
}

pub(crate) fn layout(state: &AppState) -> WolBuddyLayout {
    shell_layout::on(state, WolBuddyLayout::new)
}

/// Builds and places `0x116`'s widgets; the root.
pub(crate) fn sync(state: &mut AppState) -> Option<NodeId> {
    let layout = layout(state);
    leaf::sync(state, DIALOG, layout.page.scale, spec, |id| {
        layout.rect_of(id)
    })
}

/// Shows the page in its widgets (under `0x115` too).
fn present(state: &mut AppState) {
    let unknown = state.csf_text(UNKNOWN_STATS_KEY);
    sync(state);
    let image = layout(state).rect_of(crate::ui::wol_buddy::OBSERVER_IMAGE);
    let frontend = &mut state.frontend;
    let Some(root) = frontend.shell_controller.root_in(DIALOG) else {
        return;
    };
    // The observer image 0x75F toggles on a press anywhere on it.
    let gui = frontend.shell_controller.gui_mut();
    let hit = vera20k_gui::Value::Rect(image.unwrap_or_default());
    crate::ui::shell::page::set(
        gui,
        root,
        crate::ui::wol_buddy::OBSERVER_IMAGE,
        vera20k_gui::Prop::HitRect,
        hit,
    );
    if let Some(WolPage::Buddy(dialog)) = frontend.wol.as_ref().map(|visit| &visit.page) {
        dialog
            .ui
            .present(frontend.shell_controller.gui_mut(), root, &unknown);
    }
}

/// The lobby list arrived: the dialog loop starts. Step 3 leaves the
/// channel in `[0xB77DD0]` first (`WOL:LeavingChannel`); no entry here is
/// in one, since the lobby's Buddy List left its channel (API+0x24) and the
/// welcome and `0x114` have none, so that wait is not modelled.
pub(super) fn lobbies_listed(state: &mut AppState) {
    open_dialog(state);
}

/// The dialog loop (`0x0079F312`): the art, then `0x116` and its init
/// (`0x0079E62D..0x0079E7FE`): the mirror restarts and the 1000 ms timer
/// runs. The rows wait for the buddy list.
pub(super) fn open_dialog(state: &mut AppState) {
    super::lobby::buddy_dialog_opens(state);
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Buddy(Box::new(BuddyDialog {
        ui: WolBuddyPage::new(observer),
        timer_at: Instant::now() + TIMER_PERIOD,
        refresh_at: None,
    }));
    state.frontend.shell_route = ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    super::leaf::open_art(state, DIALOG);
    state.frontend.shell_controller.reset_to(DIALOG);
    super::buddies::restart(state);
    present(state);
    state.platform.window.request_redraw();
}

/// `0x116` has ended (`0x00622720`) and its art is released (`0x0072C0B0`):
/// the runner continues over the empty backdrop.
pub(super) fn dialog_ended(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Lobby;
    }
    super::page::close_received(state);
    super::leaf::free_art(state, DIALOG);
    state.frontend.shell_route = ShellRoute::Wol;
}

/// The stop paths of Create and Join (`0x0079F4D7`), a failed or refused
/// Join and C2/C9 Back: back to the dialog loop.
/// RESIDUAL: Create's stop path shows a set `[0xB77AD0]` in a box first;
/// no path here leaves one set.
pub(crate) fn stopped(state: &mut AppState) {
    if state.frontend.shell_route == ShellRoute::NetworkSetup {
        crate::app::shell_network_setup::abandon_for_wol_runner(state);
    }
    dialog_ended(state);
    super::lobby::buddy_runner_resumes(state);
    open_dialog(state);
}

/// The runner returned 0 with mode 5: `WOL_Main` takes the return mode and
/// clears it (`0x0077B491..0x0077B4A6`). Mode 4 keeps the login and runs
/// the custom lobby; mode 6 is My Information's driver; mode 0 is the
/// welcome.
fn returned(state: &mut AppState) {
    super::leaf::free_art(state, DIALOG);
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let mode = visit.return_mode.take();
    super::lobby::drop_runner(state);
    match mode {
        Some(WolMode::CustomMatch) => super::reopen_lobby(state),
        Some(WolMode::MyInformation) => {
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.mode = Some(WolMode::MyInformation);
                visit.page = WolPage::Backdrop { wait: None };
            }
            super::apply_wol_preferences(state);
            service::begin_my_information(state);
        }
        _ => {
            state.match_state.session.leave_network();
            App::reopen_wol_welcome(state);
        }
    }
}

/// `0x116`'s teardown has run with the button that ended it.
pub(crate) fn commit_close(state: &mut AppState, control: u16) {
    match control {
        // 0x0079FB06: back to the lobby rejoins its lobby whatever its
        // population.
        BACK => {
            if let Some(visit) = state.frontend.wol.as_mut()
                && visit.return_mode == Some(WolMode::CustomMatch)
            {
                visit.lobby_joined_before = false;
            }
            returned(state);
        }
        CREATE => {
            dialog_ended(state);
            super::lobby::begin_create(state);
        }
        JOIN => join_selected(state),
        _ => {
            dialog_ended(state);
            open_dialog(state);
        }
    }
}

/// Join (`0x0079F566`) for `[0xB76E24]` with its record and status.
fn join_selected(state: &mut AppState) {
    dialog_ended(state);
    let buddies = &state.frontend.wol_buddies;
    let name = buddies.selected.clone();
    let status = buddies.status(&name);
    let Some(channel) = buddies.channel(&name).cloned().filter(|_| !name.is_empty()) else {
        open_dialog(state);
        return;
    };
    match status {
        // 0x0079FA75: mode 4 through the return mode; the buddy's chat
        // lobby, when listed, is the one the picker rejoins.
        Some(4) => {
            let listed = super::lobby::lobbies(state).iter().any(|lobby| {
                crate::util::native_string::crt_strcmpi(&lobby.name, &channel.name).is_eq()
            });
            if listed {
                state.frontend.wol_lobby_globals.lobby_number = channel
                    .name
                    .get(crate::ui::wol_lobby::LOBBY_PREFIX.len()..)
                    .map(crate::rules::ini_value::crt_atoi)
                    .unwrap_or_default();
            }
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.return_mode = Some(WolMode::CustomMatch);
                if listed {
                    visit.lobby_joined_before = false;
                }
            }
            returned(state);
        }
        Some(5) => game_listed(state, &channel.name, false),
        _ => open_dialog(state),
    }
}

/// A buddy's game is searched in the game list (`strcmpi`): found, the
/// lobby's Join runs; missing the first time, the list is asked again;
/// missing again (`again`), the loop goes on without a box.
pub(super) fn game_listed(state: &mut AppState, name: &str, again: bool) {
    let game = super::lobby::games(state)
        .iter()
        .find(|game| crate::util::native_string::crt_strcmpi(&game.name, name).is_eq())
        .cloned();
    match (game, again) {
        (Some(game), _) => super::lobby::buddy_join(state, game),
        (None, false) => super::lobby::buddy_find_game(state, name.to_owned()),
        (None, true) => {
            log::debug!("WOL Buddy List Join: no game {name} (Find Channel by name failed)");
            open_dialog(state);
        }
    }
}

fn texts(state: &AppState) -> BuddyTexts {
    let [
        find_disabled,
        offline,
        online,
        playing,
        quick_matching,
        unknown,
    ] = BuddyTexts::KEYS.map(|key| state.csf_text(key));
    BuddyTexts {
        find_disabled,
        offline,
        online,
        playing,
        quick_matching,
        unknown,
        lobby_names: super::lobby::base_names(state),
    }
}

/// The mirror changed: an open `0x116` rebuilds its rows (`0x0079AF10`);
/// after the buddy list Refresh is enabled again (`KillTimer(0x704)`).
pub(super) fn buddies_changed(state: &mut AppState, list_arrived: bool) {
    if !page_open(state) {
        return;
    }
    rows(state);
    if list_arrived && let Some(dialog) = page_mut(state) {
        dialog.refresh_at = None;
        dialog.ui.set_enabled(REFRESH, true);
    }
    present(state);
    state.platform.window.request_redraw();
}

/// `0x0079AF10`: every buddy's row, the selection found again, then the
/// enables and statistics.
fn rows(state: &mut AppState) {
    let texts = texts(state);
    let games = super::lobby::games(state);
    let buddies = &state.frontend.wol_buddies;
    let rows: Vec<WolBuddyRow> = buddies
        .names()
        .iter()
        .map(|name| {
            buddy_row(
                name,
                buddies.status(name),
                buddies.channel(name),
                games,
                &texts,
            )
        })
        .collect();
    let selected = buddies.selected.clone();
    if let Some(dialog) = page_mut(state) {
        dialog.ui.update_rows(rows, &selected);
    }
    enables(state);
}

/// The Remove, Join and Page enables, then `0x0079C040`.
fn enables(state: &mut AppState) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let games = super::lobby::games(state);
    let buddies = &state.frontend.wol_buddies;
    let selected = buddies.selected.as_str();
    let status = buddies.status(selected);
    let join = join_enabled(
        selected,
        status,
        buddies.channel(selected),
        games,
        observer,
        super::lobby::squad_id(state),
    );
    let page = page_enabled(selected, status);
    let remove = !selected.is_empty();
    if let Some(dialog) = page_mut(state) {
        dialog.ui.set_enables(remove, join, page);
    }
    stats(state);
}

/// The ladder sink while `0x116` is open: `0x0079C040`.
pub(super) fn stats_changed(state: &mut AppState) {
    if page_open(state) {
        stats(state);
        present(state);
        state.platform.window.request_redraw();
    }
}

/// `0x0079C040`: no selection empties the heading and statistics;
/// otherwise `GUI:UserProfileLabel` names the buddy and the ladder table
/// (by lowered name) gives the five values, or `GUI:UnknownStats`.
fn stats(state: &mut AppState) {
    use crate::assets::csf_file::{CsfArg, format_csf};
    let selected = state.frontend.wol_buddies.selected.clone();
    let texts = if selected.is_empty() {
        [HEADING]
            .into_iter()
            .chain(STATS)
            .map(|id| (id, String::new()))
            .collect()
    } else {
        let heading = format_csf(
            &state.csf_text("GUI:UserProfileLabel"),
            &[CsfArg::Str(&selected)],
        );
        let record = state
            .frontend
            .wol_lobby_globals
            .ladder
            .get(&selected.to_ascii_lowercase());
        let values = match record {
            Some(record) => [
                record.wins,
                record.losses,
                record.disconnects,
                record.rank,
                record.points,
            ]
            .map(|value| value.to_string()),
            None => std::array::from_fn(|_| state.csf_text(UNKNOWN_STATS_KEY)),
        };
        std::iter::once((HEADING, heading))
            .chain(STATS.into_iter().zip(values))
            .collect()
    };
    if let Some(dialog) = page_mut(state) {
        dialog.ui.texts = Some(texts);
    }
}

fn local_nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

fn apply(state: &mut AppState, action: WolBuddyAction) {
    match action {
        WolBuddyAction::End(control) => {
            App::leave_shell_dialog(
                state,
                shell_transition::ShellSlideKind::WolBuddyList,
                move |state| crate::app::shell_wol::buddy::commit_close(state, control),
            );
        }
        WolBuddyAction::Select(name) => {
            state.frontend.wol_buddies.selected = name;
            enables(state);
        }
        WolBuddyAction::Page => super::page::open_received_modal(state),
        WolBuddyAction::OpenAdd => {
            if let Some(dialog) = page_mut(state) {
                dialog.ui.add = Some(WolAddBuddy);
            }
            // 0x115 runs over 0x116, which keeps its list but loses the
            // pointer: no hover, so no status help, while the box is up.
            let controller = &mut state.frontend.shell_controller;
            controller.drop_pointer_in(DIALOG);
            controller.push(ADD_DIALOG);
            if sync_add(state).is_some() {
                leaf::focus(state, ADD_DIALOG, Some(NAME));
            }
        }
        WolBuddyAction::Add(name) => {
            close_add(state);
            let local = local_nickname(state);
            if state.frontend.wol_buddies.valid_new(&name, &local) {
                super::buddies::add(state, name);
            }
            rows(state);
        }
        WolBuddyAction::CloseAdd => {
            close_add(state);
            rows(state);
        }
        WolBuddyAction::Remove(name) => {
            super::buddies::remove(state, &name);
            state.frontend.wol_buddies.selected.clear();
            if let Some(dialog) = page_mut(state) {
                dialog.ui.delete_row(&name);
            }
            rows(state);
        }
        WolBuddyAction::Refresh => {
            super::buddies::refresh(state);
            if let Some(dialog) = page_mut(state) {
                dialog.refresh_at = Some(Instant::now() + REFRESH_HOLD);
            }
        }
        WolBuddyAction::Observer(observer) => state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(observer),
    }
    present(state);
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

/// `0x115`'s modal loop ends; `0x116` is current again.
fn close_add(state: &mut AppState) {
    if let Some(dialog) = page_mut(state) {
        dialog.ui.add = None;
    }
    let controller = &mut state.frontend.shell_controller;
    if controller.top_id() == Some(ADD_DIALOG) {
        controller.pop();
    }
    controller.ensure_active(DIALOG);
}

/// Builds and places `0x115`'s widgets (the box's retail pixels); its
/// NewEdit keeps its caret in view (`0x00623880`).
fn sync_add(state: &mut AppState) -> Option<NodeId> {
    let (width, height) = (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let layout = crate::ui::shell::modal::wol_add_buddy_layout(width, height);
    leaf::sync(
        state,
        ADD_DIALOG,
        crate::ui::shell::geom::RETAIL_SCALE,
        add_spec,
        |id| layout.rect_of(id),
    )
}

fn add_open(state: &AppState) -> bool {
    page(state).is_some_and(|dialog| dialog.ui.add.is_some())
}

pub(super) fn cursor_moved(state: &mut AppState) {
    if add_open(state) {
        sync_add(state);
    } else {
        sync(state);
    }
    leaf::pointer(state, Pointer::Move);
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state) {
        return;
    }
    let at = if pressed { Pointer::Down } else { Pointer::Up };
    if add_open(state) {
        sync_add(state);
        for event in leaf::pointer(state, at) {
            if let PageEvent::Activated(id) = event
                && let Some(action) =
                    WolAddBuddy::activate(&mut state.frontend.shell_controller, id)
            {
                apply(state, action);
            }
        }
        return;
    }
    if sync(state).is_none() {
        return;
    }
    present(state);
    let events = leaf::pointer(state, at);
    for event in events {
        let Some(ui) = page_mut(state).map(|dialog| &mut dialog.ui) else {
            return;
        };
        let (action, doubled) = match event {
            PageEvent::Cue(cue) => {
                App::play_page_cue(state, cue);
                continue;
            }
            // LBN_SELCHANGE runs first, then LBN_DBLCLK.
            PageEvent::Selected(LIST, row) => (ui.select(row as usize), false),
            PageEvent::DoubleClicked(LIST, row) => (ui.select(row as usize), true),
            PageEvent::Toggled(id, _) | PageEvent::Activated(id) => (ui.activate(id), false),
            _ => continue,
        };
        if doubled {
            App::play_page_cue(state, PageCue::ListClick);
        }
        if let Some(action) = action {
            apply(state, action);
        }
        if doubled {
            double_click(state);
        }
    }
    present(state);
}

/// `LBN_DBLCLK` (`0x0079EB52`) after the selection.
fn double_click(state: &mut AppState) {
    let enabled =
        page(state).map(|dialog| (dialog.ui.enabled(JOIN), dialog.ui.enabled(PAGE_BUTTON)));
    let Some((join, page_enabled)) = enabled else {
        return;
    };
    let action = page_mut(state).and_then(|dialog| dialog.ui.double_click(join, page_enabled));
    if let Some(action) = action {
        apply(state, action);
    }
}

/// Keys reach only `0x115`'s edit while it runs.
pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if shell_transition::blocks_shell_input(state) {
        return;
    }
    let local = local_nickname(state);
    let names = state.frontend.wol_buddies.names().to_vec();
    let valid = |name: &str| super::buddies::valid_new_buddy(&names, name, &local);
    let shift = state.platform.live_modifiers.shift_key();
    let event = crate::ui::shell::controller::gui_input(code, shift, text);
    let Some(event) = event.filter(|_| add_open(state)) else {
        return;
    };
    if let Some(action) = WolAddBuddy::input(&mut state.frontend.shell_controller, event, &valid) {
        apply(state, action);
    }
    sync_add(state);
}

/// `0x116`'s timers (the 1000 ms status refresh and Refresh's 3000 ms
/// hold) and the list's scroll repeats.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    let now = Instant::now();
    let dialog = page_mut(state)?;
    let mut tick = false;
    if now >= dialog.timer_at {
        dialog.timer_at = now + TIMER_PERIOD;
        tick = true;
    }
    let reenabled = dialog.refresh_at.is_some_and(|at| now >= at);
    if reenabled {
        dialog.refresh_at = None;
        dialog.ui.set_enabled(REFRESH, true);
    }
    let deadlines = [
        Some(dialog.timer_at),
        dialog.refresh_at,
        leaf::poll(state, true),
    ];
    if tick {
        super::buddies::timer(state, now);
    }
    if reenabled {
        present(state);
        state.platform.window.request_redraw();
    }
    deadlines.into_iter().flatten().min()
}
