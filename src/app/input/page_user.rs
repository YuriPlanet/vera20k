//! The PageUser command (`[Hotkey] PageUser`, U in retail): in a WOL match
//! it opens the page dialog `0xCF` over the battlefield.
//!
//! `PageUserCommandClass::Execute` `0x00537E50` runs only in GameMode 4 and
//! calls `Queue_PageUser` `0x006470F0`, which refuses a local House that has
//! won (+0x1F7), lost (+0x1F8) or is flagged to die (+0x1F6) and queues the
//! PAGEUSER event 0x22. `Execute_DoList` executes that event for the
//! queuing House only (`0x0064C8D8..0x0064C911`; other clients mark it done),
//! and `EventClass::Execute` `0x004C80F6` sets scenario state 7, whose
//! `State_Machine` arm `0x0048CC2C` opens `0xCF` through `0x0077D840`.
//!
//! RESIDUAL: VERA opens the state at once, as it opens Options and
//! Diplomacy, instead of after the lockstep delay; the 128-entry OutList
//! capacity refusal and the recording-playback gate (`[0xA8D5F8] & 2`)
//! have nothing to hold. The event draws no RNG and writes no timer.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::match_runtime::session::SessionMode;

/// `0x00537E50` and `0x006470F0`'s gates, then state 7.
pub(crate) fn execute(state: &mut AppState) {
    if state.match_state.session_mode() != SessionMode::Wol {
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    let refused = sim
        .session()
        .current_house()
        .and_then(|local| sim.houses().get(&local))
        .is_none_or(|house| house.has_won() || house.has_lost() || house.is_to_die());
    if refused {
        return;
    }
    crate::app::shell_wol::page::open_in_match(state);
}
