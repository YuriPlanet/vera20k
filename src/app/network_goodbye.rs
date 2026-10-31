//! Closing the game says goodbye on the network first.
//!
//! A window close or Quit leaves the online lobby, the relay and any LAN room
//! or match through the same owners Back and Quit use, then exits once they
//! finish or after `GOODBYE_WAIT`. Without this the others learn of the exit
//! only from timeouts: the relay's 10-minute silence limit, the lobby's
//! session expiry or the LAN lease. VERA product behavior; gamemd closes its
//! WOL and LAN sessions on exit through its own teardown.

use std::time::Duration;

use winit::event_loop::{ActiveEventLoop, ControlFlow};

use super::{App, AppState, Instant};

/// Long enough for a logout and relay departure round trip; short enough that
/// a closed window never seems stuck.
const GOODBYE_WAIT: Duration = Duration::from_secs(3);

impl App {
    /// Start the network goodbyes. False when nothing needs waiting for and
    /// the caller should exit now; otherwise the window is hidden and
    /// `poll_network_goodbye` exits later.
    pub(super) fn begin_network_goodbye(state: &mut AppState) -> bool {
        if state.frontend.network_goodbye_deadline.is_some() {
            return true;
        }
        // External launches report and leave through docs/spawn.md's path.
        if state.frontend.external_launch {
            return false;
        }
        say_goodbye(state);
        if !crate::app::shell_wol::online_cleanup_pending(state) {
            return false;
        }
        state.audio.stop_theme();
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.stop_all();
        }
        // Window events stop now, so the focus loss that frees the cursor
        // lock never arrives.
        crate::app::input::cursor::release_cursor_confinement(state);
        state.platform.window.set_visible(false);
        state.frontend.network_goodbye_deadline = Some(Instant::now() + GOODBYE_WAIT);
        true
    }

    /// While goodbyes are pending, service only them; exit when they finish
    /// or the wait runs out. False when no goodbye is in progress.
    pub(super) fn poll_network_goodbye(state: &mut AppState, event_loop: &ActiveEventLoop) -> bool {
        let Some(deadline) = state.frontend.network_goodbye_deadline else {
            return false;
        };
        let next = crate::app::shell_wol::poll(state);
        let now = Instant::now();
        if crate::app::shell_wol::online_cleanup_pending(state) && now < deadline {
            let wake = next
                .unwrap_or_else(|| now + Duration::from_millis(16))
                .min(deadline);
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        } else {
            event_loop.exit();
        }
        true
    }
}

/// Leave whatever network visit is open. An online match first returns its
/// transport to the visit, which sends the relay departure; a LAN match
/// sends its departure through the match transport and flushes it.
fn say_goodbye(state: &mut AppState) {
    let online =
        state.match_state.session_mode() == crate::app::match_runtime::session::SessionMode::Wol;
    if online && state.match_state.live_network.is_some() {
        crate::app::lan_service::return_transport(state);
    }
    crate::app::shell_wol::retire_visit_for_exit(state);
    if state.match_state.lan_service.is_some() {
        crate::app::lan_service::close(state);
    }
    if let Some(live) = state.match_state.live_network.take() {
        live.into_transport()
            .service_before_close(crate::app::match_runtime::frame_pacer::wall_clock_bucket);
    }
}
