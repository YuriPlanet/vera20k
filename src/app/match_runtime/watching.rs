//! VERA watching (docs/Networking.md "Watching"): a watcher's game plays the
//! match's stream without a seat of its own. It stands in one player's place
//! for that player's sidebar and sounds, sees the whole map, and Tab moves it
//! to the next player, and after the last to the observer's sidebar.

use crate::app::AppState;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::net::live_commands::NetworkRecoveryStatus;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use winit::keyboard::KeyCode;

/// Whether this game watches its match instead of playing in it.
pub(crate) fn active(state: &AppState) -> bool {
    state
        .match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.watching())
}

/// A watcher still catching up to the match plays minutes of it at speed:
/// its sounds stay off meanwhile, as a returning player's do.
pub(crate) fn catching_up(state: &AppState, now_ms: u32) -> bool {
    let Some(live) = state
        .match_state
        .live_network
        .as_ref()
        .filter(|live| live.watching())
    else {
        return false;
    };
    let frame = state
        .match_state
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
pub(crate) fn key(state: &mut AppState, code: KeyCode, pressed: bool, repeat: bool) -> bool {
    match code {
        KeyCode::Escape => {
            if pressed && let Some(live) = state.match_state.live_network.as_mut() {
                // The next frame's network wait leaves the match
                // (`exit_awaits_unreachable_relay`).
                live.keep_running_for_exit();
            }
            true
        }
        KeyCode::Tab => {
            if pressed && !repeat {
                follow_next(state);
            }
            true
        }
        _ => false,
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

/// Tab: show the next view, and its sidebar from scratch.
fn follow_next(state: &mut AppState) {
    let Some(next) = state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| next_view(&runtime.simulation))
    else {
        return;
    };
    let Some(runtime) = state
        .match_state
        .sim_runtime
        .write_sim(SimWrite::Presentation)
    else {
        return;
    };
    if !show(&mut runtime.simulation, next) {
        return;
    }
    // The strips are the new view's from scratch, as at a match's start; a
    // player's credits counter starts at their balance.
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .reset_cameo_seed();
    state.match_state.refresh_relation_colors();
    state.platform.window.request_redraw();
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
