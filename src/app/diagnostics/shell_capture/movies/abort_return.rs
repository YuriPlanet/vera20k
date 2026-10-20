//! Shared production in-game menu (B5/BBA/FF) and B6 physical-input capture:
//! B6 presented, then optionally Leave (ending at queued EXIT, after a
//! queued DESTRUCT beside Observe), campaign Restart (ending at the raised
//! restart request) or network Observe (ending at the queued DESTRUCT with
//! the match still running).
use super::*;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::abort::{AbortButton, AbortSecondary};
use crate::ui::shell::pause_menu::{PauseMenuButton, PauseMenuTemplate};

fn menu_dialog(state: &AppState) -> u32 {
    match crate::app::input::pause_menu::current_template(state) {
        PauseMenuTemplate::Offline => 0xb5,
        PauseMenuTemplate::Network => 0xbba,
        PauseMenuTemplate::Internet => 0xff,
    }
}

/// The local House's queued DESTRUCT records.
fn queued_destructs(state: &AppState) -> Result<usize> {
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("abort lost its installed runtime")?
        .simulation;
    Ok(sim
        .pending_command_snapshot()
        .iter()
        .filter(|command| {
            matches!(command.payload, crate::sim::command::Command::Destruct)
                && Some(command.owner) == sim.session().current_house()
        })
        .count())
}
use winit::event::{ElementState, MouseButton};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AbortInputPhase {
    Menu,
    AbortPressed,
    Modal,
    ChoicePressed,
    Queued,
}
pub(super) struct AbortReturnInput {
    phase: AbortInputPhase,
    /// The B6 button to press, or none to stop once B6 has presented.
    choice: Option<AbortButton>,
    /// Stop on the presented menu instead of pressing Abort; on FF, first
    /// jump the speed trackbar to its fastest end.
    stop_at_menu: bool,
    /// The menu button to press: Abort, or Game Controls to stop on BBB.
    menu_press: PauseMenuButton,
}
impl AbortReturnInput {
    /// Through B6 Leave to the queued EXIT.
    pub(super) fn begin(state: &mut AppState, route: &mut Vec<Value>, frame: u32) -> Result<Self> {
        Self::begin_choosing(state, route, frame, Some(AbortButton::Leave))
    }

    /// Open the in-game menu and stop there.
    pub(super) fn begin_menu(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<Self> {
        let mut input = Self::begin_choosing(state, route, frame, None)?;
        input.stop_at_menu = true;
        Ok(input)
    }

    /// Open the in-game menu, press Game Controls and stop on BBB.
    pub(super) fn begin_game_controls(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<Self> {
        let mut input = Self::begin_choosing(state, route, frame, None)?;
        input.menu_press = PauseMenuButton::GameControls;
        Ok(input)
    }

    pub(super) fn begin_choosing(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
        choice: Option<AbortButton>,
    ) -> Result<Self> {
        // Ordinary world Escape resolves the actual binding before
        // Options enters B5, exactly as the window handler does.
        ensure!(
            state.platform.live_modifiers.is_empty(),
            "pause menu requires bare Escape"
        );
        let escape = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape);
        crate::app::input::keyboard::in_game_key_edge(
            state,
            crate::app::input::keyboard::InGameKeyEdge {
                physical: winit::keyboard::KeyCode::Escape,
                logical: &escape,
                unmodified: &escape,
                location: winit::keyboard::KeyLocation::Standard,
                state: winit::event::ElementState::Pressed,
                repeat: false,
            },
            false,
        );
        ensure!(
            state.match_state.match_presentation.in_game_menu
                == crate::ui::pause_menu::InGameMenuState::Menu,
            "ordinary Escape did not open B5"
        );
        route.push(json!({"frame": frame, "action": "Escape press", "dialog": menu_dialog(state)}));
        Ok(Self {
            phase: AbortInputPhase::Menu,
            choice,
            stop_at_menu: false,
            menu_press: PauseMenuButton::Abort,
        })
    }
    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<bool> {
        match self.phase {
            AbortInputPhase::Menu => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Menu
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            state
                        ),
                    "the in-game menu did not present with its required loaded artwork"
                );
                let dialog = menu_dialog(state);
                // The window handler's modal branch consumes the press. Its
                // paused Escape release reaches this existing KeyEdge owner.
                let escape = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape);
                let paused_at_event = state.match_state.paused();
                crate::app::input::keyboard::in_game_key_edge(
                    state,
                    crate::app::input::keyboard::InGameKeyEdge {
                        physical: winit::keyboard::KeyCode::Escape,
                        logical: &escape,
                        unmodified: &escape,
                        location: winit::keyboard::KeyLocation::Standard,
                        state: ElementState::Released,
                        repeat: false,
                    },
                    paused_at_event,
                );
                route.push(json!({"frame": frame, "action": "Escape release", "dialog": dialog}));
                route.push(
                    json!({"frame": frame, "action": "pause menu presented", "dialog": dialog}),
                );
                let layout = crate::app::input::pause_menu::current_layout(state)
                    .context("B5 has no loaded hit geometry")?;
                if self.stop_at_menu {
                    if let Some(internet) = layout.internet {
                        use crate::ui::shell::pause_menu::InternetSlider;
                        let rect = internet.sliders[InternetSlider::Speed as usize];
                        // The admitted strip's last pixel row, beside the
                        // value plaque: a jump to the fastest position.
                        let target = crate::ui::shell::geom::RectPx::new(
                            rect.x + rect.w - 52,
                            rect.y + rect.h - 2,
                            1,
                            1,
                        );
                        let point = set_window_pointer_to_rect_center(state, target);
                        crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                        crate::app::input::dispatch::handle_mouse_input(
                            state,
                            MouseButton::Left,
                            ElementState::Pressed,
                        );
                        crate::app::input::dispatch::handle_mouse_input(
                            state,
                            MouseButton::Left,
                            ElementState::Released,
                        );
                        let pause = state.match_state.match_presentation.internet_pause;
                        ensure!(
                            pause.positions[InternetSlider::Speed as usize] == 6,
                            "physical FF speed jump did not reach the fastest position"
                        );
                        route.push(json!({"frame": frame, "action": "speed trackbar jump",
                            "dialog": dialog, "point": point, "positions": pause.positions}));
                    }
                    MoviesCapture::restore_neutral_pointer(state);
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                }
                let press = self.menu_press;
                let rect = layout
                    .button(press)
                    .with_context(|| format!("the in-game menu has no {press:?}"))?;
                let point = set_window_pointer_to_rect_center(state, rect);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Pressed,
                );
                ensure!(
                    state
                        .match_state
                        .match_presentation
                        .pause_menu_interaction
                        .is_pressed(press),
                    "physical {press:?} press missed its loaded control"
                );
                route.push(
                    json!({"frame": frame, "action": format!("{press:?} press"), "dialog": dialog,
                    "point": point, "rect": [rect.x, rect.y, rect.w, rect.h]}),
                );
                self.phase = AbortInputPhase::AbortPressed;
            }
            AbortInputPhase::AbortPressed => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Menu
                        && state
                            .match_state
                            .match_presentation
                            .pause_menu_interaction
                            .is_pressed(self.menu_press),
                    "the menu press changed before its presented release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                if self.menu_press == PauseMenuButton::GameControls {
                    ensure!(
                        state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Options,
                        "physical Game Controls release did not open BBB"
                    );
                    route.push(json!({"frame": frame, "action": "Game Controls release"}));
                    MoviesCapture::restore_neutral_pointer(state);
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                }
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm,
                    "physical B5 Abort release did not open B6"
                );
                route.push(json!({"frame": frame, "action": "Abort release",
                    "dialog": menu_dialog(state)}));
                self.phase = AbortInputPhase::Modal;
            }
            AbortInputPhase::Modal => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            state
                        ),
                    "B6 did not present with its required loaded artwork"
                );
                let layout = crate::app::input::abort::current_abort_layout(state)
                    .context("B6 has no loaded geometry")?;
                route.push(
                    json!({"frame": frame, "action": "abort modal presented", "dialog": 0xb6,
                    "secondary": format!("{:?}", layout.secondary_kind),
                    "buttons": layout.buttons().map(|(button, rect)| json!({
                        "button": format!("{button:?}"), "rect": [rect.x, rect.y, rect.w, rect.h]}))
                        .collect::<Vec<_>>()}),
                );
                let Some(choice) = self.choice else {
                    MoviesCapture::restore_neutral_pointer(state);
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                };
                let rect = layout
                    .button(choice)
                    .with_context(|| format!("B6 does not show {choice:?}"))?;
                let point = set_window_pointer_to_rect_center(state, rect);
                crate::app::input::dispatch::handle_cursor_moved_in_game(state);
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Pressed,
                );
                ensure!(
                    state
                        .match_state
                        .match_presentation
                        .abort_buttons
                        .is_pressed(choice),
                    "physical B6 {choice:?} press missed its loaded control"
                );
                route.push(
                    json!({"frame": frame, "action": format!("{choice:?} press"), "dialog": 0xb6,
                    "point": point, "rect": [rect.x, rect.y, rect.w, rect.h]}),
                );
                self.phase = AbortInputPhase::ChoicePressed;
            }
            AbortInputPhase::ChoicePressed if self.choice == Some(AbortButton::Secondary) => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && state
                            .match_state
                            .match_presentation
                            .abort_buttons
                            .is_pressed(AbortButton::Secondary),
                    "B6 secondary capture changed before its presented press/release"
                );
                let secondary = crate::app::input::abort::current_abort_layout(state)
                    .context("B6 has no loaded geometry")?
                    .secondary_kind;
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                if secondary == AbortSecondary::Observe {
                    ensure!(
                        state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Closed
                            && !state.match_state.match_presentation.modal_latch
                            && queued_destructs(state)? == 1,
                        "physical Observe did not close B6, clear the latch and queue DESTRUCT"
                    );
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("Observe lost its installed runtime")?
                        .simulation;
                    ensure!(
                        !sim.pending_command_snapshot()
                            .iter()
                            .any(|command| matches!(
                                command.payload,
                                crate::sim::command::Command::ExitMatch
                            )),
                        "Observe queued EXIT"
                    );
                    route.push(
                        json!({"frame": frame, "action": "Observe release", "dialog": 0xb6,
                        "destruct_queued": true, "exit_queued": false, "latch": false}),
                    );
                } else {
                    ensure!(
                        state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Closed
                            && state.match_state.scenario_restart_requested,
                        "physical Restart did not close B6 and raise the restart request"
                    );
                    route.push(
                        json!({"frame": frame, "action": "Restart release", "dialog": 0xb6,
                        "restart_requested": true}),
                    );
                }
                MoviesCapture::restore_neutral_pointer(state);
                self.phase = AbortInputPhase::Queued;
            }
            AbortInputPhase::ChoicePressed => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && state
                            .match_state
                            .match_presentation
                            .abort_buttons
                            .is_pressed(AbortButton::Leave),
                    "B6 Leave capture changed before its presented press/release"
                );
                let runtime = state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("B6 lost its installed runtime")?;
                ensure!(
                    !runtime
                        .simulation
                        .pending_command_snapshot()
                        .iter()
                        .any(|command| matches!(
                            command.payload,
                            crate::sim::command::Command::ExitMatch
                        )),
                    "an EXIT was already queued before physical Leave release"
                );
                let destructs = crate::app::input::abort::current_abort_layout(state)
                    .context("B6 has no loaded geometry")?
                    .secondary_kind
                    == AbortSecondary::Observe;
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                        && !state.match_state.paused(),
                    "physical Leave did not release the ordinary modal pause"
                );
                let sim = &state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("Leave lost its installed runtime")?
                    .simulation;
                let exits = sim
                    .pending_command_snapshot()
                    .into_iter()
                    .filter(|command| {
                        matches!(command.payload, crate::sim::command::Command::ExitMatch)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    exits.len() == 1 && Some(exits[0].owner) == sim.session().current_house(),
                    "physical Leave did not queue exactly the current House's EXIT"
                );
                let exit = &exits[0];
                ensure!(
                    !sim.quit_requested(),
                    "Leave confirmation executed EXIT instead of only queueing it"
                );
                let destruct_count = queued_destructs(state)?;
                ensure!(
                    destruct_count == usize::from(destructs),
                    "Leave queued {destruct_count} DESTRUCT records; result {} expects {}",
                    if destructs { 3 } else { 2 },
                    usize::from(destructs)
                );
                route.push(
                    json!({"frame": frame, "action": "Leave release", "dialog": 0xb6,
                    "leave_result": if destructs { 3 } else { 2 }, "destruct_queued": destructs}),
                );
                route.push(json!({"frame": frame, "action": "EXIT queued", "queued_at_tick": sim.session().tick(),
                    "execute_tick": exit.execute_tick, "owner": sim.interner().resolve(exit.owner),
                    "owner_native_unique_id": sim.houses().get(&exit.owner).and_then(|house| house.native_unique_id()),
                    "modal_closed": !state.match_state.paused(), "diagnostic_simulation_freeze": false,
                    "quit_requested": sim.quit_requested()}));
                MoviesCapture::restore_neutral_pointer(state);
                self.phase = AbortInputPhase::Queued;
            }
            AbortInputPhase::Queued => {}
        }
        Ok(self.phase == AbortInputPhase::Queued)
    }
}
