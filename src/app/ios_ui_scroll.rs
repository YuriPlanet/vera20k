//! iOS direct manipulation of native list contents. Geometry and row ranges
//! come from the same controls that paint and handle desktop mouse input.
//! A swipe changes only their viewport; row selection remains the original tap
//! path, and scrollbar/slider capture remains ordinary mouse input.

use crate::app::frontend::{shell_layout, shell_transition, skirmish_shell_render};
use crate::app::{App, AppState, GameScreen};
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::skirmish_shell::{self, SkirmishComboId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollTarget {
    SkirmishCombo(SkirmishComboId),
    ChooseMode,
    ChooseMap,
    SeedBrowser,
    LoadBrowser,
    GameBrowser,
    Keyboard,
    Sound,
    Movies,
    NetworkCards,
    SetupPopup(u16),
    SetupChat,
    LanGames,
    LanPlayers,
    LanChat,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollRegion {
    pub target: ScrollTarget,
    /// Row pitch in physical window pixels, as delivered by winit Touch.
    pub row_height: f64,
}

#[derive(Clone, Copy)]
struct ScrollSpec {
    target: ScrollTarget,
    geometry: ShellListGeometry,
    game_shell: bool,
}

impl ScrollSpec {
    fn window(target: ScrollTarget, geometry: ShellListGeometry) -> Self {
        Self {
            target,
            geometry,
            game_shell: false,
        }
    }
    fn game(target: ScrollTarget, geometry: ShellListGeometry) -> Self {
        Self {
            target,
            geometry,
            game_shell: true,
        }
    }
}

/// The handler has already projected this touch through set_cursor_from_window.
/// Use the control's own cursor frame, including the fitted in-game modal frame.
pub(crate) fn region_at_cursor(state: &AppState) -> Option<ScrollRegion> {
    for spec in specs(state) {
        let (x, y) = if spec.game_shell {
            skirmish_shell_render::in_game_shell_cursor(state)
        } else {
            state.match_state.input.cursor_px()
        };
        if spec.geometry.content.contains(x, y) {
            let window_scale = if spec.game_shell {
                state.sidebar_frame().fit_native_shell().scale()
                    * state.tactical_zoom().map_or(1.0, |zoom| zoom.scale())
            } else {
                1.0
            };
            return Some(ScrollRegion {
                target: spec.target,
                row_height: f64::from((spec.geometry.metrics.row_h as f32 * window_scale).max(1.0)),
            });
        }
    }
    None
}

pub(crate) fn is_active(state: &AppState, target: ScrollTarget) -> bool {
    specs(state).iter().any(|spec| spec.target == target)
}

fn specs(state: &AppState) -> Vec<ScrollSpec> {
    if state.platform.focus_freeze_active() || shell_transition::blocks_shell_input(state) {
        return Vec::new();
    }
    if state.frontend.shell_message_box.is_some() {
        return Vec::new();
    }
    if let Some(dialog) = state.frontend.keyboard_dialog.as_ref() {
        if dialog.category_open {
            return Vec::new();
        }
        let Some(layout) = crate::app::input::keyboard::layout(state) else {
            return Vec::new();
        };
        let geometry = ShellListGeometry::new(
            layout.commands,
            dialog.rows.len(),
            dialog.top,
            ListMetrics::scaled(layout.scale),
        );
        let game = dialog.parent == crate::ui::shell::keyboard::KeyboardParent::GameControls;
        return vec![ScrollSpec {
            target: ScrollTarget::Keyboard,
            geometry,
            game_shell: game,
        }];
    }
    if let Some(dialog) = state.frontend.network_options.as_ref() {
        if dialog.pending_message().is_some() {
            return Vec::new();
        }
        return dialog
            .card_popup(&crate::app::input::network_options::layout(state))
            .map(|popup| {
                vec![ScrollSpec::window(
                    ScrollTarget::NetworkCards,
                    popup.scroll_geometry(),
                )]
            })
            .unwrap_or_default();
    }
    if state.frontend.screen == GameScreen::InGame {
        let Some(shell) = skirmish_shell_render::current_in_game_shell_layout(state) else {
            return Vec::new();
        };
        match state.match_state.match_presentation.in_game_menu {
            crate::ui::pause_menu::InGameMenuState::SavedGame(_) => {
                if let Some(browser) = state
                    .match_state
                    .match_presentation
                    .saved_game_browser
                    .as_ref()
                {
                    if browser.prompt.is_none() {
                        let layout = crate::ui::shell::saved_games::saved_game_layout(
                            browser.mode,
                            shell.width,
                            shell.height,
                            shell.layout,
                            shell.button_size,
                        );
                        return vec![ScrollSpec::game(
                            ScrollTarget::GameBrowser,
                            ShellListGeometry::new(
                                layout.list,
                                browser.entries.len(),
                                browser.top_index,
                                ListMetrics::scaled(layout.scale),
                            ),
                        )];
                    }
                }
            }
            crate::ui::pause_menu::InGameMenuState::Sound => {
                if let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_ref() {
                    let layout = crate::ui::shell::sound::SoundLayout::new(
                        shell.width,
                        shell.height,
                        shell.layout,
                        shell.button_size,
                    );
                    return vec![ScrollSpec::game(
                        ScrollTarget::Sound,
                        ShellListGeometry::new(
                            layout.list,
                            dialog.rows.len(),
                            dialog.top,
                            ListMetrics::RETAIL,
                        ),
                    )];
                }
            }
            _ => {}
        }
        return Vec::new();
    }
    if state.frontend.screen != GameScreen::MainMenu {
        return Vec::new();
    }
    if App::load_saved_game_active(state) {
        if let Some(browser) = state.frontend.load_saved_game.as_ref() {
            if browser.prompt.is_none() {
                let layout = shell_layout::load_saved_game_layout(state).browser;
                return vec![ScrollSpec::window(
                    ScrollTarget::LoadBrowser,
                    ShellListGeometry::new(
                        layout.list,
                        browser.entries.len(),
                        browser.top_index,
                        ListMetrics::scaled(layout.scale),
                    ),
                )];
            }
        }
        return Vec::new();
    }
    if App::movie_list_active(state) {
        let layout = shell_layout::movie_list_layout(state);
        return state
            .frontend
            .movie_list
            .as_ref()
            .map(|list| {
                vec![ScrollSpec::window(
                    ScrollTarget::Movies,
                    list.geometry(layout.list, ListMetrics::scaled(layout.page.scale)),
                )]
            })
            .unwrap_or_default();
    }
    let shell = &state.frontend.skirmish_shell_state;
    if App::native_skirmish_shell_active(state) || state.frontend.shell_route.network_setup() {
        if shell.validation_modal.is_some() {
            return Vec::new();
        }
        if let Some(browser) = shell.saved_seed_browser.as_ref() {
            if browser.prompt.is_none() {
                let layout = shell_layout::saved_seed_layout(state, browser.mode);
                return vec![ScrollSpec::window(
                    ScrollTarget::SeedBrowser,
                    ShellListGeometry::new(
                        layout.list,
                        browser.entries.len(),
                        browser.top_index,
                        ListMetrics::scaled(layout.scale),
                    ),
                )];
            }
            return Vec::new();
        }
        if shell.random_map_setup_modal.is_some() {
            return Vec::new();
        }
        if let Some(chooser) = shell.choose_map_modal.as_ref() {
            if chooser.is_hidden() || chooser.eject_prompt.is_some() {
                return Vec::new();
            }
            let layout = shell_layout::choose_map_layout(state);
            return vec![
                ScrollSpec::window(ScrollTarget::ChooseMode, chooser.mode_geometry(&layout)),
                ScrollSpec::window(ScrollTarget::ChooseMap, chooser.map_geometry(&layout)),
            ];
        }
        if App::native_skirmish_shell_active(state) {
            if let Some(open) = shell.open_combo_dropdown {
                let layout = shell_layout::skirmish_layout(state);
                return skirmish_shell::combo_dropdown_scroll_geometry(
                    shell,
                    &layout,
                    state.frontend.scenario_catalog.shell_maps(),
                    open.id,
                )
                .map(|geometry| {
                    vec![ScrollSpec::window(
                        ScrollTarget::SkirmishCombo(open.id),
                        geometry,
                    )]
                })
                .unwrap_or_default();
            }
            return Vec::new();
        }
        if let Some(ui) = state.frontend.multiplayer_setup.as_ref() {
            if ui.notice.is_some() {
                return Vec::new();
            }
            let Some(view) = crate::app::shell_network_setup::view(state) else {
                return Vec::new();
            };
            if view.locked {
                return Vec::new();
            }
            let layout = crate::app::input::multiplayer_setup::layout(state);
            if let Some(popup) = ui.popup_geometry(&view, &layout) {
                return vec![ScrollSpec::window(
                    ScrollTarget::SetupPopup(ui.popup.control.unwrap()),
                    popup.scroll_geometry(),
                )];
            }
            return vec![ScrollSpec::window(
                ScrollTarget::SetupChat,
                ui.chat
                    .list
                    .geometry(layout.chat_log, ui.chat.lines.len(), layout.page.scale),
            )];
        }
    }
    if state.frontend.shell_route.lan_lobby() {
        if let Some(lan) = state.frontend.lan.as_ref() {
            if lan.modal.is_some() || !lan.lobby.enabled {
                return Vec::new();
            }
            let layout = crate::app::shell_lan::layout(state);
            let lobby = &lan.lobby;
            return vec![
                ScrollSpec::window(
                    ScrollTarget::LanGames,
                    lobby.game_list.geometry(
                        layout.games,
                        lobby.games.len() + 1,
                        layout.page.scale,
                    ),
                ),
                ScrollSpec::window(
                    ScrollTarget::LanPlayers,
                    lobby.player_list.geometry(
                        layout.players,
                        lobby.players.len(),
                        layout.page.scale,
                    ),
                ),
                ScrollSpec::window(
                    ScrollTarget::LanChat,
                    lobby.chat.list.geometry(
                        layout.chat_log,
                        lobby.chat.lines.len(),
                        layout.page.scale,
                    ),
                ),
            ];
        }
    }
    Vec::new()
}

pub(crate) fn scroll(state: &mut AppState, target: ScrollTarget, rows: i32) {
    let Some(spec) = specs(state).into_iter().find(|spec| spec.target == target) else {
        return;
    };
    let geometry = spec.geometry;
    match target {
        ScrollTarget::SkirmishCombo(id) => {
            skirmish_shell::scroll_open_combo_by_rows(
                &mut state.frontend.skirmish_shell_state,
                state.frontend.scenario_catalog.shell_maps(),
                id,
                rows,
            );
        }
        ScrollTarget::ChooseMode | ScrollTarget::ChooseMap => {
            let layout = shell_layout::choose_map_layout(state);
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_mut()
            {
                modal.scroll_contents(&layout, target == ScrollTarget::ChooseMode, rows);
            }
        }
        ScrollTarget::SeedBrowser => {
            if let Some(browser) = state
                .frontend
                .skirmish_shell_state
                .saved_seed_browser
                .as_mut()
            {
                geometry.scroll_by_rows(&mut browser.top_index, rows);
                browser.last_list_press = None;
            }
        }
        ScrollTarget::LoadBrowser => {
            if let Some(browser) = state.frontend.load_saved_game.as_mut() {
                geometry.scroll_by_rows(&mut browser.top_index, rows);
                browser.last_list_press = None;
            }
        }
        ScrollTarget::GameBrowser => {
            if let Some(browser) = state
                .match_state
                .match_presentation
                .saved_game_browser
                .as_mut()
            {
                geometry.scroll_by_rows(&mut browser.top_index, rows);
                browser.last_list_press = None;
            }
        }
        ScrollTarget::Keyboard => {
            if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                geometry.scroll_by_rows(&mut dialog.top, rows);
            }
        }
        ScrollTarget::Sound => {
            if let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_mut() {
                geometry.scroll_by_rows(&mut dialog.top, rows);
            }
        }
        ScrollTarget::Movies => {
            if let Some(list) = state.frontend.movie_list.as_mut() {
                geometry.scroll_by_rows(&mut list.top, rows);
                list.last_press = None;
            }
        }
        ScrollTarget::NetworkCards => {
            if let Some(dialog) = state.frontend.network_options.as_mut() {
                geometry.scroll_by_rows(&mut dialog.card_top_index, rows);
                dialog.card_hot_row = None;
            }
        }
        ScrollTarget::SetupPopup(_) => {
            if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
                geometry.scroll_by_rows(&mut ui.popup.top_index, rows);
                ui.popup.hot_row = None;
            }
        }
        ScrollTarget::SetupChat => {
            if let Some(ui) = state.frontend.multiplayer_setup.as_mut() {
                geometry.scroll_by_rows(&mut ui.chat.list.top_index, rows);
            }
        }
        ScrollTarget::LanGames | ScrollTarget::LanPlayers | ScrollTarget::LanChat => {
            if let Some(lan) = state.frontend.lan.as_mut() {
                let list = match target {
                    ScrollTarget::LanGames => &mut lan.lobby.game_list,
                    ScrollTarget::LanPlayers => &mut lan.lobby.player_list,
                    _ => &mut lan.lobby.chat.list,
                };
                geometry.scroll_by_rows(&mut list.top_index, rows);
                list.last_press = None;
            }
        }
    }
    state.platform.window.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ios_touch::{IosTouchPointer, TouchTranslation};
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::ui::skirmish_shell::{SkirmishShellLayout, SkirmishShellState};
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, ElementState, Touch, TouchPhase, WindowEvent};

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: None,
            id,
        }
    }

    fn country_shell() -> SkirmishShellState {
        let mut ini = String::from("[Countries]\n");
        for index in 0..10 {
            ini.push_str(&format!("{index}=Country{index}\n"));
        }
        for index in 0..10 {
            ini.push_str(&format!(
                "[Country{index}]\nMultiplay=yes\nUIName=NAME:Country{index}\n"
            ));
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut shell = SkirmishShellState::default();
        shell.refresh_country_catalog(&rules);
        shell
    }

    /// Delivers the adapter's output to the same native dropdown selection and
    /// scrollbar owners the handler uses, without requiring a GPU/window.
    fn deliver(
        output: TouchTranslation,
        shell: &mut SkirmishShellState,
        layout: &SkirmishShellLayout,
        cursor: &mut (i32, i32),
    ) {
        if let Some((ScrollTarget::SkirmishCombo(id), rows)) = output.scroll {
            skirmish_shell::scroll_open_combo_by_rows(shell, &[], id, rows);
        }
        for event in output.events {
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    *cursor = (position.x as i32, position.y as i32);
                    skirmish_shell::handle_option_mouse_move(
                        shell,
                        layout,
                        &[],
                        cursor.0,
                        cursor.1,
                    );
                }
                WindowEvent::MouseInput { state, .. } => {
                    if state == ElementState::Pressed {
                        skirmish_shell::handle_option_mouse_down(
                            shell,
                            layout,
                            &[],
                            cursor.0,
                            cursor.1,
                        );
                    } else {
                        skirmish_shell::handle_option_mouse_up(shell);
                    }
                }
                _ => panic!("unexpected native touch event"),
            }
        }
    }

    #[test]
    fn ios_swipe_keeps_native_country_dropdown_open_then_tap_selects_scrolled_row() {
        let mut shell = country_shell();
        let layout = skirmish_shell::compute_layout(800, 600);
        let id = SkirmishComboId::Side(0);
        let face = skirmish_shell::combo_rect(&layout, id).unwrap();
        skirmish_shell::handle_option_mouse_down(
            &mut shell,
            &layout,
            &[],
            face.x + face.w - 1,
            face.y + 1,
        );
        let geometry =
            skirmish_shell::combo_dropdown_scroll_geometry(&shell, &layout, &[], id).unwrap();
        assert!(geometry.max_top >= 3);
        let region = ScrollRegion {
            target: ScrollTarget::SkirmishCombo(id),
            row_height: f64::from(geometry.metrics.row_h),
        };
        let selected = skirmish_shell::selected_combo_item(&shell, id);
        let mut pointer = IosTouchPointer::default();
        let mut cursor = (0, 0);
        let x = f64::from(geometry.content.x + 3);
        let y = f64::from(geometry.content.y + geometry.metrics.row_h * 3 + 2);
        let mut apply = |touch| {
            let output = pointer.translate_with_scroll(touch, Some(region), true, 1.0);
            deliver(output, &mut shell, &layout, &mut cursor);
        };
        apply(touch(1, TouchPhase::Started, x, y));
        apply(touch(1, TouchPhase::Moved, x, y - region.row_height * 3.0));
        apply(touch(1, TouchPhase::Ended, x, y - region.row_height * 3.0));
        assert_eq!(skirmish_shell::selected_combo_item(&shell, id), selected);
        assert_eq!(shell.open_combo_dropdown.unwrap().top_index, 3);

        let row = 1;
        let expected = skirmish_shell::combo_items(&shell, &[], id)[3 + row];
        let tap_y = f64::from(geometry.content.y + geometry.metrics.row_h * row as i32 + 2);
        let started = pointer.translate_with_scroll(
            touch(2, TouchPhase::Started, x, tap_y),
            Some(region),
            true,
            1.0,
        );
        assert!(started.events.is_empty());
        deliver(started, &mut shell, &layout, &mut cursor);
        assert!(shell.open_combo_dropdown.is_some());
        deliver(
            pointer.translate_with_scroll(
                touch(2, TouchPhase::Ended, x, tap_y),
                Some(region),
                true,
                1.0,
            ),
            &mut shell,
            &layout,
            &mut cursor,
        );
        assert_eq!(
            skirmish_shell::selected_combo_item(&shell, id),
            Some(expected)
        );
        assert!(shell.open_combo_dropdown.is_none());
    }

    #[test]
    fn ios_scroll_cancel_route_loss_and_secondary_contact_never_select_a_row() {
        let region = ScrollRegion {
            target: ScrollTarget::ChooseMap,
            row_height: 19.0,
        };
        let mut pointer = IosTouchPointer::default();
        assert!(
            pointer
                .translate_with_scroll(
                    touch(1, TouchPhase::Started, 50.0, 60.0),
                    Some(region),
                    true,
                    2.0,
                )
                .events
                .is_empty()
        );
        assert!(
            pointer
                .translate_with_scroll(touch(2, TouchPhase::Started, 80.0, 90.0), None, true, 2.0,)
                .events
                .is_empty()
        );
        // Crossing physical slop without a full row still marks a swipe, so
        // the list owner clears stale double-click history before any new tap.
        let moved =
            pointer.translate_with_scroll(touch(1, TouchPhase::Moved, 65.0, 60.0), None, true, 2.0);
        assert_eq!(moved.scroll, Some((ScrollTarget::ChooseMap, 0)));
        let cancelled = pointer.translate_with_scroll(
            touch(1, TouchPhase::Cancelled, 65.0, 60.0),
            None,
            true,
            2.0,
        );
        assert!(cancelled.events.is_empty());
        assert!(
            pointer
                .translate_with_scroll(touch(2, TouchPhase::Ended, 80.0, 90.0), None, true, 2.0,)
                .events
                .is_empty()
        );
        pointer.translate_with_scroll(
            touch(3, TouchPhase::Started, 50.0, 60.0),
            Some(region),
            true,
            2.0,
        );
        assert!(
            pointer
                .translate_with_scroll(
                    touch(3, TouchPhase::Ended, 50.0, 60.0),
                    Some(region),
                    false,
                    2.0,
                )
                .events
                .is_empty()
        );
        pointer.reset();
        // Native slider/scrollbar capture outside list contents remains an
        // immediate press, rather than waiting for a tap or guessing a wheel.
        let native = pointer.translate_with_scroll(
            touch(4, TouchPhase::Started, 50.0, 60.0),
            None,
            true,
            2.0,
        );
        assert!(matches!(
            native.events.as_slice(),
            [
                WindowEvent::CursorMoved { .. },
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
            ]
        ));
    }
}
