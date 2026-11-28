//! A lost graphics device. Its driver crashed, Windows reset it after it
//! stopped responding (TDR), or the card ran out of memory: wgpu reports it
//! once (`GpuContext::device_lost`), and nothing reaches the screen after it
//! while the match and its sound go on behind a black window. The game saves
//! a single-player match, closes as a closed window does, which leaves an
//! online or LAN match, and tells the player why as it exits
//! (`system_dialog::report_graphics_lost`). A loss before the frame loop
//! runs, at startup, ends in wgpu's panic on the first buffer it maps on the
//! dead device; the panic hook shows the same box then
//! (`system_dialog::report_panic`). VERA product behavior: the renderer
//! can't move its resources to a new device.

use winit::event_loop::ActiveEventLoop;

use super::{App, AppState};
use crate::app::system_dialog::GraphicsLost;

impl App {
    /// Begin closing the game once its graphics device is lost. True when
    /// this pass began it.
    pub(super) fn close_after_graphics_loss(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        if state.frontend.graphics_lost.is_some() {
            return false;
        }
        let Some(error) = state.renderer.gpu.device_lost().map(str::to_owned) else {
            return false;
        };
        // DirectX 12's own reason tells a GPU hang from a driver fault; wgpu's
        // message says only that the device is lost.
        let error = match state.renderer.gpu.device_removed_reason() {
            Some(reason) => {
                log::error!("{reason}");
                format!("{error}. {reason}")
            }
            None => error,
        };
        log::info!("Closing: the graphics device is lost");
        let saved = save_match(state);
        // Lost before startup finished: the next start draws through the
        // other Windows API, and the box says so.
        let retry = crate::app::persistence::startup_backend::record_loss();
        state.frontend.graphics_lost = Some(GraphicsLost {
            error,
            saved,
            retry,
        });
        Self::close_game(state, event_loop);
        true
    }
}

/// Save a single-player match still being played, under the description the
/// Save dialog offers, so the player can go on from it after a restart. A
/// tick still on the simulation thread finishes first, so the save holds
/// whole frames. Returns the description, when it saved.
fn save_match(state: &mut AppState) -> Option<String> {
    if !state.match_state.session_mode().is_single_player()
        || !state.match_state.sim_runtime.is_installed()
    {
        return None;
    }
    crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
    if state.match_state.scenario_outcome.is_some() || state.match_state.scenario_exit.is_some() {
        return None;
    }
    let description = App::current_save_description(&state.match_state, &state.process_assets);
    match crate::app::persistence::commands::save_game(
        &state.platform,
        &state.renderer,
        &state.frontend.screen,
        &mut state.match_state,
        &state.persistence,
        &state.audio,
        &state.diag,
        &state.process_assets,
        &description,
        None,
    ) {
        Ok(path) => {
            log::info!("Saved the match to {}", path.display());
            Some(description)
        }
        Err(error) => {
            log::error!("Could not save the match: {error}");
            None
        }
    }
}
