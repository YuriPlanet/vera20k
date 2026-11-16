//! VERA watching (docs/Networking.md "Watching"): a watcher's game plays the
//! match's stream without a seat of its own. It stands in one player's place
//! for that player's sidebar and sounds, sees the whole map, and Tab moves it
//! to the next player.

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
/// diagnostic's key adapter: Escape leaves the match, Tab follows the next
/// player. False for any other key.
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

/// Tab: follow the next player still in the match, in House order, with
/// their sidebar and sounds; after the last comes the first again.
fn follow_next(state: &mut AppState) {
    let Some(next) = state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| next_player(&runtime.simulation))
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
    if !runtime.simulation.follow_house(next) {
        return;
    }
    // The strips are the new player's from scratch, as at a match's start;
    // the credits counter starts at their balance.
    state
        .match_state
        .match_presentation
        .sidebar_projection
        .reset_cameo_seed();
    state.match_state.refresh_relation_colors();
    state.platform.window.request_redraw();
}

/// The player after the one followed: undefeated humans, observers left out.
fn next_player(sim: &Simulation) -> Option<InternedId> {
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
    let current = sim.session().current_house();
    let next = match players.iter().position(|owner| Some(*owner) == current) {
        Some(at) => players[(at + 1) % players.len()],
        None => *players.first()?,
    };
    (Some(next) != current).then_some(next)
}
