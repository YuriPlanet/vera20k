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

use std::time::{Duration, Instant};

use super::WolPage;
use super::service;
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanEditKey;
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::wol_buddy::*;
use crate::ui::wol_shell::WolMode;
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
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolBuddyLayout::responsive(width as i32, height as i32)
    } else {
        WolBuddyLayout::new(width as i32, height as i32)
    }
}

/// The status help under the pointer (`0x006040B0`). The list's per-row
/// `0x4E8`/`0x4E9` tooltips are not read, so it answers with its own key.
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    page(state)?.ui.buttons.hovered.and_then(tooltip_key)
}

/// `0x0072C060`: BuddyList.shp (800 wide only) through BuddyList.PAL, with
/// Refresh's pictures.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_buddy_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    let mut icons: Vec<&str> = vec![REFRESH_FACES.0, REFRESH_FACES.1];
    icons.extend([
        "wolclan.pcx",
        "woltrny.pcx",
        "wolpriv.pcx",
        "wolreslk.pcx",
        "wolrslkob.pcx",
        "wolob.pcx",
    ]);
    state.frontend.wol_buddy_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("BuddyList.shp", "BuddyList.PAL"),
        &icons,
    );
    state.frontend.wol_buddy_art.is_some()
}

/// `0x0078BF7D`: the lobby's Buddy List writes mode 5 with return mode 4
/// and its runner ends; the mode-loop pass applies, and the login category
/// is unchanged, so the Buddy List runner follows at once.
pub(super) fn enter_from(state: &mut AppState, from: WolMode) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.mode = Some(WolMode::PlayBuddy);
        visit.return_mode = Some(from);
    }
    super::apply_wol_preferences(state);
    open(state);
}

/// The runner's entry (steps 1-2): the chat-lobby list behind its wait.
pub(super) fn open(state: &mut AppState) {
    super::lobby::open_buddy_runner(state);
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
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    open_art(state);
    state.frontend.shell_controller.reset_to(DIALOG, false);
    super::buddies::restart(state);
    state.platform.window.request_redraw();
}

/// `0x116` has ended (`0x00622720`) and its art is released (`0x0072C0B0`):
/// the runner continues over the empty backdrop.
pub(super) fn dialog_ended(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Lobby;
        visit.page_received = None;
    }
    state.frontend.wol_buddy_art = None;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
}

/// The stop paths of Create and Join (`0x0079F4D7`), a failed or refused
/// Join and C2/C9 Back: back to the dialog loop.
/// RESIDUAL: Create's stop path shows a set `[0xB77AD0]` in a box first;
/// no path here leaves one set.
pub(crate) fn stopped(state: &mut AppState) {
    if state.frontend.shell_route.network_setup() {
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
    state.frontend.wol_buddy_art = None;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let mode = visit.return_mode.take();
    super::lobby::drop_runner(state);
    match mode {
        Some(WolMode::CustomMatch) => {
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.mode = Some(WolMode::CustomMatch);
            }
            super::apply_wol_preferences(state);
            super::lobby::open(state, false);
        }
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
    ] = BuddyTexts::KEYS.map(|key| service::resolve(state, key));
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
    let join = join_enabled(selected, status, buddies.channel(selected), games, observer);
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
            &service::resolve(state, "GUI:UserProfileLabel"),
            &[CsfArg::Str(&selected)],
        );
        let record = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.ladder.get(&selected.to_ascii_lowercase()));
        let values = match record {
            Some(record) => [
                record.wins,
                record.losses,
                record.disconnects,
                record.rank,
                record.points,
            ]
            .map(|value| value.to_string()),
            None => std::array::from_fn(|_| service::resolve(state, UNKNOWN_STATS_KEY)),
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
                shell_transition::ShellExitThen::WolBuddyClose(control),
            );
        }
        WolBuddyAction::Select(name) => {
            state.frontend.wol_buddies.selected = name;
            enables(state);
        }
        WolBuddyAction::Page => super::page::open_received_modal(state),
        WolBuddyAction::OpenAdd => {
            if let Some(dialog) = page_mut(state) {
                dialog.ui.cancel_pointer();
                dialog.ui.add = Some(WolAddBuddy::new());
            }
            state.frontend.shell_controller.reset_to(ADD_DIALOG, false);
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
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

/// `0x115`'s modal loop ends; `0x116` is current again.
fn close_add(state: &mut AppState) {
    if let Some(dialog) = page_mut(state) {
        dialog.ui.add = None;
    }
    state.frontend.shell_controller.reset_to(DIALOG, false);
}

/// The `0x115` control under the pointer.
fn add_control_at(state: &AppState, x: i32, y: i32) -> Option<u16> {
    let (width, height) = (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let layout = crate::ui::shell::modal::wol_add_buddy_layout(width, height);
    [(ADD, layout.add), (BACK, layout.back), (NAME, layout.name)]
        .into_iter()
        .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let add_over = add_control_at(state, x, y);
    let Some(dialog) = page_mut(state) else {
        return;
    };
    if let Some(add) = dialog.ui.add.as_mut() {
        add.buttons.hovered = add_over.filter(|id| matches!(*id, ADD | BACK));
        state.platform.window.request_redraw();
        return;
    }
    let ui = &mut dialog.ui;
    ui.buttons.hovered = layout.control_at(x, y);
    let geometry = ui
        .list
        .geometry(layout.list, ui.rows.len(), layout.page.scale);
    ui.list.hot_row = geometry.row_at(ui.rows.len(), ui.list.top_index, x, y);
    ui.list
        .scroll
        .pointer_moved(geometry, &mut ui.list.top_index, x, y);
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state) {
        return;
    }
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let scale = layout.page.scale;
    let add_over = add_control_at(state, x, y);
    let over = layout.control_at(x, y);
    let Some(dialog) = page_mut(state) else {
        return;
    };
    let mut action = None;
    let mut cue = None;
    let mut button_sound = false;
    let mut doubled = false;
    if let Some(add) = dialog.ui.add.as_mut() {
        if pressed {
            match add_over {
                Some(ADD) if !add.add_enabled => {}
                Some(id @ (ADD | BACK)) => {
                    add.buttons.press(Some(id));
                    button_sound = true;
                }
                _ => add.buttons.press(None),
            }
        } else {
            let over = add_over.filter(|id| matches!(*id, ADD | BACK));
            action = add.buttons.release(over).and_then(|id| add.activate(id));
        }
    } else if !pressed {
        let ui = &mut dialog.ui;
        ui.list.scroll.cancel();
        let button = over.filter(|id| PAGE.button(*id).is_some() || *id == REFRESH);
        if let Some(control) = ui.buttons.release(button) {
            action = ui.activate(control);
        }
    } else {
        let ui = &mut dialog.ui;
        let now = Instant::now();
        let geometry = ui.list.geometry(layout.list, ui.rows.len(), scale);
        if let Some(part) = geometry.scroll_part_at(x, y) {
            ui.list
                .scroll
                .press(part, geometry, &mut ui.list.top_index, y, now);
        } else {
            match over {
                Some(LIST) => {
                    if let Some(row) = geometry.row_at(ui.rows.len(), ui.list.top_index, x, y) {
                        let double = crate::ui::shell::list::is_double_click(
                            &mut ui.list.last_press,
                            now,
                            x,
                            y,
                            crate::ui::shell::saved_file_input::host_double_click_limits(),
                        );
                        // LBN_SELCHANGE runs first, then LBN_DBLCLK.
                        action = ui.select(row);
                        doubled = double;
                        cue = Some(LauncherCue::GenericClick);
                    }
                }
                Some(control @ (OBSERVER_IMAGE | OBSERVER_TOGGLE)) => {
                    let icon_hit = control == OBSERVER_IMAGE
                        || crate::ui::skirmish_shell::checkbox_icon_rect(
                            layout.observer_toggle,
                            scale,
                        )
                        .contains(x, y);
                    if icon_hit {
                        action = ui.activate(control);
                        cue = Some(LauncherCue::Checkbox);
                    }
                }
                Some(control) if (PAGE.button(control).is_some() || control == REFRESH) => {
                    if ui.enabled(control) {
                        ui.buttons.press(Some(control));
                        button_sound = true;
                    }
                }
                _ => ui.buttons.press(None),
            }
        }
    }
    if button_sound {
        App::play_main_menu_button_sound(state);
    }
    if let Some(cue) = cue {
        App::play_launcher_options_cue(state, cue);
    }
    if let Some(action) = action {
        apply(state, action);
    }
    if doubled {
        double_click(state);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
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
    state.platform.window.request_redraw();
}

/// Keys reach only `0x115`'s edit while it runs.
pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if shell_transition::blocks_shell_input(state) {
        return;
    }
    let local = local_nickname(state);
    let names = state.frontend.wol_buddies.names().to_vec();
    let valid = |name: &str| super::buddies::valid_new_buddy(&names, name, &local);
    let key = match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Tab) => Some(LanEditKey::Tab),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        Some(KeyCode::Escape) => return,
        _ => None,
    };
    let Some(add) = page_mut(state).and_then(|dialog| dialog.ui.add.as_mut()) else {
        return;
    };
    let action = match key {
        Some(key) => add.key(key, &valid),
        None => {
            add.insert_text(text.unwrap_or_default(), &valid);
            None
        }
    };
    if let Some(action) = action {
        apply(state, action);
    }
    state.platform.window.request_redraw();
}

/// `0x116`'s timers (the 1000 ms status refresh and Refresh's 3000 ms
/// hold) and the list's scroll repeats.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    let now = Instant::now();
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let dialog = page_mut(state)?;
    let mut tick = false;
    if now >= dialog.timer_at {
        dialog.timer_at = now + TIMER_PERIOD;
        tick = true;
    }
    let mut redraw = false;
    if dialog.refresh_at.is_some_and(|at| now >= at) {
        dialog.refresh_at = None;
        dialog.ui.set_enabled(REFRESH, true);
        redraw = true;
    }
    let ui = &mut dialog.ui;
    let geometry = ui
        .list
        .geometry(layout.list, ui.rows.len(), layout.page.scale);
    redraw |= ui
        .list
        .scroll
        .poll(geometry, &mut ui.list.top_index, x, y, now);
    let deadlines = [
        Some(dialog.timer_at),
        dialog.refresh_at,
        ui.list.scroll.repeat_at(),
    ];
    if tick {
        super::buddies::timer(state, now);
    }
    if redraw {
        state.platform.window.request_redraw();
    }
    deadlines.into_iter().flatten().min()
}
