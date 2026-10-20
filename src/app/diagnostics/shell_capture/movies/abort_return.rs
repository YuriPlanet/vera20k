//! Shared production B5/B6 physical-input capture: B6 presented, then
//! optionally Leave (ending at queued EXIT) or campaign Restart (ending at the
//! raised restart request).
use super::*;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::abort::AbortButton;
use crate::ui::shell::pause_menu::PauseMenuButton;
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
}
impl AbortReturnInput {
    /// Through B6 Leave to the queued EXIT.
    pub(super) fn begin(state: &mut AppState, route: &mut Vec<Value>, frame: u32) -> Result<Self> {
        Self::begin_choosing(state, route, frame, Some(AbortButton::Leave))
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
        route.push(json!({"frame": frame, "action": "Escape press", "dialog": 0xb5}));
        Ok(Self {
            phase: AbortInputPhase::Menu,
            choice,
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
                    "B5 did not present with its required loaded artwork"
                );
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
                route.push(json!({"frame": frame, "action": "Escape release", "dialog": 0xb5}));
                route.push(
                    json!({"frame": frame, "action": "pause menu presented", "dialog": 0xb5}),
                );
                let shell =
                    crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(
                        state,
                    )
                    .context("B5 has no loaded hit geometry")?;
                let layout = crate::ui::shell::pause_menu::pause_menu_layout(
                    shell.width,
                    shell.height,
                    shell.layout,
                    shell.button_size,
                );
                let rect = layout.buttons[PauseMenuButton::Abort as usize];
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
                        .is_pressed(PauseMenuButton::Abort),
                    "physical B5 Abort press missed its loaded control"
                );
                route.push(
                    json!({"frame": frame, "action": "Abort press", "dialog": 0xb5,
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
                            .is_pressed(PauseMenuButton::Abort),
                    "B5 Abort capture changed before its presented press/release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm,
                    "physical B5 Abort release did not open B6"
                );
                route.push(json!({"frame": frame, "action": "Abort release", "dialog": 0xb5}));
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
            AbortInputPhase::ChoicePressed if self.choice == Some(AbortButton::Restart) => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && state
                            .match_state
                            .match_presentation
                            .abort_buttons
                            .is_pressed(AbortButton::Restart),
                    "B6 Restart capture changed before its presented press/release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                        && state.match_state.scenario_restart_requested,
                    "physical Restart did not close B6 and raise the restart request"
                );
                route.push(
                    json!({"frame": frame, "action": "Restart release", "dialog": 0xb6,
                    "restart_requested": true}),
                );
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
                route.push(json!({"frame": frame, "action": "Leave release", "dialog": 0xb6}));
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
