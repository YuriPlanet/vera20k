//! VERA watching (docs/Networking.md "Watching"): a watcher's game plays the
//! match's stream without a seat of its own. It stands in one player's place
//! for that player's sight, sidebar and sounds, and Tab moves it to the next
//! player, and after the last to the observer's sidebar, which sees the whole
//! map. Its mouse moves only the view.

use crate::app::PlatformState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::audio::sfx::SfxPlayer;
use crate::net::live_commands::NetworkRecoveryStatus;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// Whether this game watches its match instead of playing in it.
pub(crate) fn active(match_state: &crate::app::match_runtime::state::MatchState) -> bool {
    match_state.session.local_connection_watches() && match_state.live_network.is_some()
}

/// Whether this game watches and sees the whole map: on the observer's
/// sidebar. On a player's view it sees what that player sees.
pub(crate) fn whole_map(match_state: &crate::app::match_runtime::state::MatchState) -> bool {
    active(match_state)
        && match_state
            .sim_runtime
            .runtime()
            .is_some_and(|runtime| runtime.simulation.session().watcher_observes())
}

/// A watcher still catching up to the match plays minutes of it at speed:
/// its sounds stay off meanwhile, as a returning player's do.
pub(crate) fn catching_up(
    match_state: &crate::app::match_runtime::state::MatchState,
    now_ms: u32,
) -> bool {
    if !active(match_state) {
        return false;
    }
    let Some(live) = match_state.live_network.as_ref() else {
        return false;
    };
    let frame = match_state
        .sim_runtime
        .runtime()
        .map_or(0, |runtime| runtime.simulation.session().binary_frame());
    matches!(
        live.recovery_status(now_ms, frame),
        Some(NetworkRecoveryStatus::CatchingUp { .. })
    )
}

/// A watcher's own keys, from the window (`App::window_event`) or a
/// diagnostic's key adapter: Escape leaves the match, Tab moves to the next
/// view ([`next_view`]). False for any other key.
pub(crate) fn key(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    sfx_player: &mut Option<SfxPlayer>,
    code: KeyCode,
    pressed: bool,
    repeat: bool,
) -> bool {
    match code {
        KeyCode::Escape => {
            if pressed && let Some(live) = match_state.live_network.as_mut() {
                // The next frame's network wait leaves the match
                // (`exit_awaits_unreachable_relay`).
                live.keep_running_for_exit();
            }
            true
        }
        KeyCode::Tab => {
            if pressed && !repeat {
                follow_next(platform, frontend, match_state, sfx_player);
            }
            true
        }
        _ => false,
    }
}

/// A watcher's mouse buttons, from the window (`App::window_event`) or a
/// diagnostic's mouse adapter. They move only the view, as a player's do: a
/// right drag over the battlefield pans it (`input::camera`), a press on the
/// radar centres it there (`input::radar::centre_view_at_cursor`), and with
/// VERA's wheel zoom a middle press over the battlefield resets the view
/// distance. Nothing selects or orders, so a right click that did not pan
/// does nothing.
pub(crate) fn mouse(
    platform: &PlatformState,
    renderer: &mut crate::app::renderer_state::RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &crate::app::persistence::PersistenceState,
    button: MouseButton,
    pressed: bool,
) {
    let tactical = &mut match_state.input.tactical_mouse;
    tactical.record_physical_button(button, pressed);
    if !pressed {
        if button == MouseButton::Right {
            let _ = tactical.end_right_press();
        }
        return;
    }
    if crate::app::presentation::sidebar_render::is_cursor_over_minimap(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
    ) {
        if matches!(button, MouseButton::Left | MouseButton::Right) {
            crate::app::input::radar::centre_view_at_cursor(
                platform,
                renderer,
                frontend,
                match_state,
                persistence,
            );
        }
        return;
    }
    match button {
        MouseButton::Right
            if crate::app::input::camera::cursor_over_battlefield(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            ) && match_state.input.tactical_mouse.press_may_arm() =>
        {
            let cursor = (match_state.input.cursor_x, match_state.input.cursor_y);
            match_state.input.tactical_mouse.begin_right_drag(cursor);
        }
        MouseButton::Middle => {
            crate::app::input::camera::middle_press_resets_view_distance(
                platform,
                renderer,
                &frontend.screen,
                match_state,
                persistence,
            );
        }
        _ => {}
    }
}

/// One of the views Tab moves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// A player's sidebar and sounds (`Simulation::follow_house`).
    Player(InternedId),
    /// The observer's sidebar, with the last player's sounds
    /// (`Simulation::observe`).
    Observer,
}

/// Tab: show the next view, its sidebar from scratch and its sight
/// ([`show_sight`]).
fn follow_next(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    sfx_player: &mut Option<SfxPlayer>,
) {
    let Some(next) = match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| next_view(&runtime.simulation))
    else {
        return;
    };
    switch_view(platform, frontend, match_state, sfx_player, next);
}

/// A launcher's watcher (docs/spawn.md "Watching") is none of the players: it
/// starts on the observer's sidebar, with the whole map, as Tab's stop after
/// the last player, and Tab goes on from there.
pub(crate) fn begin_on_observers_view(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    sfx_player: &mut Option<SfxPlayer>,
) {
    switch_view(platform, frontend, match_state, sfx_player, View::Observer);
    log::info!(
        "Watching from the observer's view: whole map {}",
        whole_map(match_state)
    );
}

fn switch_view(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    sfx_player: &mut Option<SfxPlayer>,
    next: View,
) {
    let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Presentation) else {
        return;
    };
    if !show(&mut runtime.simulation, next) {
        return;
    }
    // The strips are the new view's from scratch, as at a match's start; a
    // player's credits counter starts at their balance.
    match_state
        .match_presentation
        .sidebar_projection
        .model
        .reset_cameo_seed();
    match_state.refresh_relation_colors();
    show_sight(match_state);
    // No House update reports the radar of the House now followed: its
    // housing opens or closes to that House's as a change there would.
    let available = crate::app::presentation::building_anim::local_has_radar(frontend, match_state);
    crate::app::presentation::building_anim::set_local_tactical_radar(
        match_state,
        sfx_player,
        available,
    );
    platform.window.request_redraw();
}

/// A watcher sees what the player it follows sees, and the whole map on the
/// observer's sidebar ([`whole_map`]). Every House keeps its own fog in the
/// shared world, so a new view only draws another's: the fog picture and the
/// radar start again from it, as after a load (`match_runtime::restore`).
fn show_sight(match_state: &mut crate::app::match_runtime::state::MatchState) {
    match_state.sandbox_full_visibility = whole_map(match_state);
    let presentation = &mut match_state.match_presentation;
    if let Some(shroud) = presentation.shroud_buffer.as_mut() {
        shroud.mark_stale();
    }
    if let Some(minimap) = presentation.minimap.as_mut() {
        minimap.mark_stale();
    }
}

/// The view after the one shown: the next player still in the match
/// (undefeated humans in House order, observers left out), after the last the
/// observer's sidebar, and after it the first player again. `None` on the
/// observer's sidebar when no player is left.
fn next_view(sim: &Simulation) -> Option<View> {
    let players: Vec<InternedId> = sim
        .houses()
        .in_order()
        .filter(|house| {
            house.is_human()
                && !house.is_defeated()
                && !sim.session().is_observer_house(house.name())
        })
        .map(|house| house.name())
        .collect();
    let session = sim.session();
    let first = players.first().map(|&owner| View::Player(owner));
    if session.watcher_observes() {
        return first;
    }
    let current = session.current_house();
    Some(
        match players.iter().position(|owner| Some(*owner) == current) {
            Some(at) => players
                .get(at + 1)
                .map_or(View::Observer, |&owner| View::Player(owner)),
            None => first.unwrap_or(View::Observer),
        },
    )
}

fn show(sim: &mut Simulation, view: View) -> bool {
    match view {
        View::Player(owner) => sim.follow_house(owner),
        View::Observer => sim.observe(),
    }
}

#[cfg(all(test, tests_app_match))]
mod tests {
    use super::*;
    use crate::app::presentation::observer_strip::observer_houses;
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;

    fn tab(sim: &mut Simulation) -> View {
        let view = next_view(sim).expect("Tab moves on");
        assert!(show(sim, view), "{view:?}");
        view
    }

    /// Tab moves through the players in House order, then to the observer's
    /// sidebar, which lists them, then to the first player again.
    #[test]
    fn tab_shows_the_observers_sidebar_after_the_last_player() {
        let mut sim = Simulation::with_seed(1);
        let [ann, bob] = ["Ann", "Bob"].map(|name| {
            let id = sim.intern(name);
            sim.houses_mut_for_test()
                .register(HouseState::new(id, 0, None, true, 0, 10));
            id
        });
        let session = sim.session_mut_for_test();
        session.set_game_mode(ScenarioGameMode::Wol);
        session.set_current_house_for_test(Some(ann));
        sim.install_watcher();
        assert_eq!(tab(&mut sim), View::Player(bob));
        assert_eq!(observer_houses(&sim, bob), None);
        assert_eq!(tab(&mut sim), View::Observer);
        let listed: Vec<_> = observer_houses(&sim, bob)
            .expect("the observer's strip")
            .iter()
            .map(|house| house.house)
            .collect();
        assert_eq!(listed, [ann, bob]);
        assert_eq!(tab(&mut sim), View::Player(ann));
        assert_eq!(observer_houses(&sim, ann), None);
    }
}
