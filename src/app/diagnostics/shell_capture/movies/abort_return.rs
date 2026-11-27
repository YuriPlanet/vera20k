//! Shared production in-game menu (B5/BBA/FF) and B6 physical-input capture:
//! B6 presented, then optionally Leave (ending at queued EXIT, after a
//! queued DESTRUCT beside Observe), campaign Restart (ending at the raised
//! restart request) or network Observe (ending at the queued DESTRUCT with
//! the match still running).
use super::*;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::abort::{AbortButton, AbortSecondary};
use crate::ui::shell::pause_menu::{PauseMenuButton, PauseMenuTemplate};

fn menu_dialog(match_presentation: &MatchPresentationState) -> u32 {
    match crate::app::input::pause_menu::current_template(match_presentation) {
        PauseMenuTemplate::Offline => 0xb5,
        PauseMenuTemplate::Network => 0xbba,
        PauseMenuTemplate::Internet => 0xff,
    }
}

/// The local House's queued DESTRUCT records.
fn queued_destructs(sim_runtime: &SimHost) -> Result<usize> {
    let sim = &sim_runtime
        .runtime()
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
    AwaitMenu,
    Menu,
    AbortPressed,
    Modal,
    ChoicePressed,
    /// A WOL tournament's B5 Abort opened the Surrender box (state 2).
    SurrenderBox,
    SurrenderOkPressed,
    Queued,
}
pub(in super::super) struct AbortReturnInput {
    phase: AbortInputPhase,
    /// The B6 button to press, or none to stop once B6 has presented.
    choice: Option<AbortButton>,
    /// Stop on the presented menu instead of pressing Abort; on FF, first
    /// open the Game Options page, which FF's group heads, and jump its speed
    /// trackbar to the fastest end.
    stop_at_menu: bool,
    /// Abort opens the Surrender box instead of B6: `Some(true)` presses
    /// its OK, `Some(false)` stops once it has presented.
    surrender: Option<bool>,
}
/// Where an [`AbortReturnInput`] route ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum AbortGoal {
    /// Stop on the presented menu; on FF, first open the Game Options page
    /// and jump its speed trackbar to the fastest end.
    Menu,
    /// Press Abort, then this B6 button, or stop once B6 has presented.
    Choose(Option<AbortButton>),
    /// A WOL tournament game's Abort: the Surrender box, then OK
    /// (`press_ok`) or a stop once it has presented.
    Surrender { press_ok: bool },
}
impl AbortReturnInput {
    /// Ordinary Escape toward `goal`. `exact_step` advances the offline
    /// exact-step owner until B5 opens; without it (a capture whose match the
    /// ordinary frame loop runs, or a live peer that may advance only through
    /// its normal frame boundary) the route waits for the menu the loop's
    /// next tick opens.
    pub(in super::super) fn begin(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
        goal: AbortGoal,
        exact_step: bool,
    ) -> Result<Self> {
        let choice = match goal {
            AbortGoal::Choose(choice) => choice,
            AbortGoal::Menu | AbortGoal::Surrender { .. } => None,
        };
        let mut input = Self::begin_mode(state, route, frame, choice, exact_step)?;
        input.stop_at_menu = goal == AbortGoal::Menu;
        if let AbortGoal::Surrender { press_ok } = goal {
            input.surrender = Some(press_ok);
        }
        Ok(input)
    }

    fn begin_mode(
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
        choice: Option<AbortButton>,
        advance: bool,
    ) -> Result<Self> {
        // Queue_Options647040 publishes an event; EventClass4C7929..4C7939
        // opens the menu only when that event executes. Network diagnostics
        // await the ordinary frame owner, as live Observe already does; their
        // frozen MatchSession start is not an offline exact-step L0 receipt.
        let advance = advance && !state.match_state.session_mode().is_network();
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
                text: None,
            },
            false,
        );
        // Queue_Options647040 publishes an OPTIONS event. Its ordinary
        // EventClass tail, rather than the key edge, requests the modal.
        if advance
            && state.match_state.match_presentation.in_game_menu
                != crate::ui::pause_menu::InGameMenuState::Menu
        {
            crate::app::match_runtime::sim_tick::advance_in_game_runtime_exact_step(state)?;
        }
        ensure!(
            !advance
                || state.match_state.match_presentation.in_game_menu
                    == crate::ui::pause_menu::InGameMenuState::Menu,
            "ordinary Escape did not open B5"
        );
        route.push(json!({"frame": frame, "action": "Escape press", "dialog": menu_dialog(&state.match_state.match_presentation)}));
        Ok(Self {
            phase: if advance {
                AbortInputPhase::Menu
            } else {
                AbortInputPhase::AwaitMenu
            },
            choice,
            stop_at_menu: false,
            surrender: None,
        })
    }
    pub(in super::super) fn after_present(
        &mut self,
        state: &mut AppState,
        route: &mut Vec<Value>,
        frame: u32,
    ) -> Result<bool> {
        if self.phase == AbortInputPhase::AwaitMenu {
            if state.match_state.match_presentation.in_game_menu != InGameMenuState::Menu {
                return Ok(false);
            }
            self.phase = AbortInputPhase::Menu;
        }
        match self.phase {
            AbortInputPhase::AwaitMenu => unreachable!("menu wait returned above"),
            AbortInputPhase::Menu => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Menu
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            &state.platform,
                            &state.renderer,
                            &state.frontend,
                            &state.match_state,
                            &state.persistence
                        ),
                    "the in-game menu did not present with its required loaded artwork"
                );
                let dialog = menu_dialog(&state.match_state.match_presentation);
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
                        text: None,
                    },
                    paused_at_event,
                );
                route.push(json!({"frame": frame, "action": "Escape release", "dialog": dialog}));
                route.push(
                    json!({"frame": frame, "action": "pause menu presented", "dialog": dialog}),
                );
                let mut layout = crate::app::input::pause_menu::current_layout(
                    &state.platform,
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.persistence,
                )
                .context("B5 has no loaded hit geometry")?;
                if self.stop_at_menu {
                    let internet = state.match_state.match_presentation.pause_menu_template
                        == crate::ui::shell::pause_menu::PauseMenuTemplate::Internet;
                    if internet {
                        // The menu opens on Game Controls; FF's group heads
                        // the Game Options page, which Options' column shows:
                        // Keyboard, then a button per category.
                        use crate::ui::main_menu_dialogs::options::Category;
                        let game_options = 1 + Category::ALL
                            .iter()
                            .position(|category| *category == Category::Gameplay)
                            .context("the column has no Game Options page")?;
                        for (step, row) in [("Options", 0), ("Game Options page", game_options)] {
                            let rect = *layout
                                .page_column()
                                .get(row)
                                .with_context(|| format!("FF has no {step} button"))?;
                            let point = click_in_game(state, rect);
                            layout = crate::app::input::pause_menu::current_layout(
                                &state.platform,
                                &state.renderer,
                                &state.frontend,
                                &state.match_state,
                                &state.persistence,
                            )
                            .context("FF has no loaded hit geometry")?;
                            route.push(json!({"frame": frame, "action": step,
                                "dialog": dialog, "point": point}));
                        }
                    }
                    if internet {
                        let internet = layout.internet.context(
                            "FF's Game Options page has no Internet Game Controls group",
                        )?;
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
                        let point = click_in_game(state, target);
                        let pause = state.match_state.match_presentation.internet_pause;
                        ensure!(
                            pause.positions[InternetSlider::Speed as usize] == 6,
                            "physical FF speed jump did not reach the fastest position"
                        );
                        route.push(json!({"frame": frame, "action": "speed trackbar jump",
                            "dialog": dialog, "point": point, "positions": pause.positions}));
                    }
                    MoviesCapture::restore_neutral_pointer(
                        &state.renderer.gpu,
                        &mut state.match_state.input,
                    );
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                }
                let press = PauseMenuButton::Abort;
                let rect = layout
                    .button(press)
                    .with_context(|| format!("the in-game menu has no {press:?}"))?;
                let point = press_in_game(state, rect);
                ensure!(
                    crate::app::input::pause_menu::button_states(&state.match_state)
                        [press as usize]
                        .pressed,
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
                        && crate::app::input::pause_menu::button_states(&state.match_state)
                            [PauseMenuButton::Abort as usize]
                            .pressed,
                    "the menu press changed before its presented release"
                );
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                if self.surrender.is_some() {
                    ensure!(
                        state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Surrender,
                        "a tournament game's B5 Abort release did not open the Surrender box"
                    );
                    route.push(json!({"frame": frame, "action": "Abort release",
                        "state": 2, "dialog": 0x120}));
                    self.phase = AbortInputPhase::SurrenderBox;
                    return Ok(false);
                }
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm,
                    "physical B5 Abort release did not open B6"
                );
                route.push(json!({"frame": frame, "action": "Abort release",
                    "dialog": menu_dialog(&state.match_state.match_presentation)}));
                self.phase = AbortInputPhase::Modal;
            }
            AbortInputPhase::Modal => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(
                            &state.platform,
                            &state.renderer,
                            &state.frontend,
                            &state.match_state,
                            &state.persistence
                        ),
                    "B6 did not present with its required loaded artwork"
                );
                let layout = crate::app::input::abort::current_abort_layout(
                    &state.platform,
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.persistence,
                )
                .context("B6 has no loaded geometry")?;
                route.push(
                    json!({"frame": frame, "action": "abort modal presented", "dialog": 0xb6,
                    "secondary": format!("{:?}", layout.secondary_kind),
                    "buttons": layout.buttons().map(|(button, rect)| json!({
                        "button": format!("{button:?}"), "rect": [rect.x, rect.y, rect.w, rect.h]}))
                        .collect::<Vec<_>>()}),
                );
                let Some(choice) = self.choice else {
                    MoviesCapture::restore_neutral_pointer(
                        &state.renderer.gpu,
                        &mut state.match_state.input,
                    );
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                };
                let rect = layout
                    .button(choice)
                    .with_context(|| format!("B6 does not show {choice:?}"))?;
                let point = press_in_game(state, rect);
                ensure!(
                    crate::app::input::dialog_page::pressed(
                        &state.match_state.match_presentation.dialog_ui,
                        choice.id()
                    ),
                    "physical B6 {choice:?} press missed its loaded control"
                );
                route.push(
                    json!({"frame": frame, "action": format!("{choice:?} press"), "dialog": 0xb6,
                    "point": point, "rect": [rect.x, rect.y, rect.w, rect.h]}),
                );
                self.phase = AbortInputPhase::ChoicePressed;
            }
            AbortInputPhase::SurrenderBox => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Surrender
                        && state.frontend.skirmish_shell_chrome.is_some(),
                    "the Surrender box did not present with its loaded artwork"
                );
                let layout =
                    crate::app::input::surrender::layout(&state.renderer, &state.frontend.screen);
                route.push(json!({"frame": frame, "action": "Surrender box presented",
                    "dialog": 0x120, "rect": [layout.dialog.x, layout.dialog.y,
                    layout.dialog.w, layout.dialog.h]}));
                if self.surrender != Some(true) {
                    MoviesCapture::restore_neutral_pointer(
                        &state.renderer.gpu,
                        &mut state.match_state.input,
                    );
                    self.phase = AbortInputPhase::Queued;
                    return Ok(true);
                }
                let point = press_in_game(state, layout.button(0));
                ensure!(
                    crate::app::input::surrender::pressed(
                        &state.match_state.match_presentation,
                        crate::app::input::surrender::SurrenderButton::Ok
                    ),
                    "the Surrender OK press missed its control"
                );
                route.push(json!({"frame": frame, "action": "Surrender OK press",
                    "point": point}));
                self.phase = AbortInputPhase::SurrenderOkPressed;
            }
            AbortInputPhase::SurrenderOkPressed => {
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    MouseButton::Left,
                    ElementState::Released,
                );
                let sim = &state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .context("Surrender lost its installed runtime")?
                    .simulation;
                let exits = sim
                    .pending_command_snapshot()
                    .into_iter()
                    .filter(|command| {
                        matches!(command.payload, crate::sim::command::Command::ExitMatch)
                            && Some(command.owner) == sim.session().current_house()
                    })
                    .count();
                let destructs = queued_destructs(&state.match_state.sim_runtime)?;
                ensure!(
                    state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                        && !state.match_state.match_presentation.modal_latch
                        && exits == 1
                        && destructs == 1,
                    "Surrender OK must queue the player's DESTRUCT and EXIT: {exits} EXIT, \
                     {destructs} DESTRUCT"
                );
                route.push(json!({"frame": frame, "action": "Surrender OK release",
                    "destruct_queued": true, "exit_queued": true}));
                MoviesCapture::restore_neutral_pointer(
                    &state.renderer.gpu,
                    &mut state.match_state.input,
                );
                self.phase = AbortInputPhase::Queued;
                return Ok(true);
            }
            AbortInputPhase::ChoicePressed if self.choice == Some(AbortButton::Secondary) => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && crate::app::input::dialog_page::pressed(
                            &state.match_state.match_presentation.dialog_ui,
                            AbortButton::Secondary.id()
                        ),
                    "B6 secondary capture changed before its presented press/release"
                );
                let secondary = crate::app::input::abort::current_abort_layout(
                    &state.platform,
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.persistence,
                )
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
                            && queued_destructs(&state.match_state.sim_runtime)? == 1,
                        "physical Observe did not close B6, clear the latch and queue DESTRUCT"
                    );
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .runtime()
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
                MoviesCapture::restore_neutral_pointer(
                    &state.renderer.gpu,
                    &mut state.match_state.input,
                );
                self.phase = AbortInputPhase::Queued;
            }
            AbortInputPhase::ChoicePressed => {
                ensure!(
                    state.match_state.match_presentation.in_game_menu
                        == InGameMenuState::AbortConfirm
                        && crate::app::input::dialog_page::pressed(
                            &state.match_state.match_presentation.dialog_ui,
                            AbortButton::Leave.id()
                        ),
                    "B6 Leave capture changed before its presented press/release"
                );
                let runtime = state
                    .match_state
                    .sim_runtime
                    .runtime()
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
                let destructs = crate::app::input::abort::current_abort_layout(
                    &state.platform,
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.persistence,
                )
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
                    .runtime()
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
                let destruct_count = queued_destructs(&state.match_state.sim_runtime)?;
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
                MoviesCapture::restore_neutral_pointer(
                    &state.renderer.gpu,
                    &mut state.match_state.input,
                );
                self.phase = AbortInputPhase::Queued;
            }
            AbortInputPhase::Queued => {}
        }
        Ok(self.phase == AbortInputPhase::Queued)
    }
}
