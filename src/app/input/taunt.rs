//! In-battle multiplayer taunts: `TauntCommandClass::Execute` `0x00536350`
//! (the Taunt_1..Taunt_8 keys), the taunt packet `0x1D`'s receive arm
//! `0x0048D9BD`, and the TauntsOn switch they share. Taunts are out of band:
//! not EventClass records and not part of lockstep.
//!
//! The transport is the controlled network service: a send queues
//! [`NetworkLoadingRequest::Taunt`](crate::app::match_runtime::session::NetworkLoadingRequest)
//! and a received packet arrives as `NetworkMatchEvent::Taunt`. Playback is
//! the taunt stream, `SfxPlayer::play_taunt`.
//!
//! RESIDUAL: the packet's name, colour and `-MESSAGES` hash fields are not
//! carried; only the hash filter (`[0xA8D60D]`, off by default) reads one of
//! them. WOL's relay mask (`0x00536532..0x00536570`) is WOL backend. The
//! second receive arm `0x0064A739` (packet dword `0x78`) has no sender in
//! the binary and is not ported.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;

/// The rate limit `[0x00825C88]`: a taunt is sent at most once per 5000 ms
/// of `timeGetTime`.
const TAUNT_INTERVAL_MS: i32 = 5000;

/// The process-static `timeGetTime` of the last sent taunt, `[0x00825C88]`.
/// It starts at -1 (`None`) and nothing resets it between games; its only
/// writer runs once every gate has passed (`0x005363C6`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TauntClock {
    last_ms: Option<u32>,
}

impl TauntClock {
    /// `last == -1 || now - last > 5000`: a 32-bit subtraction and a signed
    /// compare (`0x0053638C..0x00536394`), so the gap survives the
    /// `timeGetTime` wrap.
    pub(crate) fn admits(self, now_ms: u64) -> bool {
        self.last_ms
            .is_none_or(|last| (now_ms as u32).wrapping_sub(last) as i32 > TAUNT_INTERVAL_MS)
    }

    pub(crate) fn sent(&mut self, now_ms: u64) {
        self.last_ms = Some(now_ms as u32);
    }
}

/// The taunt byte `(country << 4) | (n & 0xF)` (`0x0053641E..0x00536432`).
/// The country is the local node's `+0x4B`, its `[Countries]` index.
fn taunt_byte(country: u16, number: u8) -> u8 {
    ((country as u8) << 4) | (number & 0xF)
}

/// `TauntCommandClass::Execute` `0x00536350` for taunt `number` (1..8, the
/// command's `+4` parameter). Each gate returns silently, in order:
/// GameMode 3 or 4; the local player not out (Session+0x300); the rate
/// limit; the player's own TauntsOn. Then the clock is written, the taunt
/// plays locally before any send (`0x00536438`), and the packet goes to
/// every Multiplay House whose chat-mask byte is set
/// (`0x0053645A..0x0053652B`); alliance does not matter.
pub(crate) fn execute(state: &mut AppState, number: u8, now_ms: u64) {
    let mode = state.match_state.session_mode();
    if !mode.is_network() {
        return;
    }
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let sim = &runtime.simulation;
    if sim.local_player_defeated() {
        return;
    }
    let Some(local) = sim.session().current_house() else {
        return;
    };
    let country = sim
        .houses()
        .get(&local)
        .and_then(|house| house.country())
        .and_then(|country| {
            runtime
                .resources
                .rules
                .country_index(sim.interner().resolve(country))
        });
    if !state
        .frontend
        .multiplayer_shell_runtime
        .admit_taunt(mode, now_ms)
    {
        return;
    }
    // RESIDUAL: a House with no resolved country stands as 0; the local
    // node always has one natively (the 0x0069B8C0 assignment).
    let byte = taunt_byte(country.map_or(0, |index| index.0), number);
    let pattern = runtime
        .resources
        .rules
        .country_taunt_pattern_by_index(u16::from(byte >> 4))
        .map(str::to_owned);
    play(state, byte, pattern.as_deref());
    let recipients = crate::app::input::chat::listener_peers(state, local);
    if !recipients.is_empty() {
        state.match_state.session.request_taunt(recipients, byte);
    }
}

/// Receive `0x0048D9BD..0x0048DA35`: the receiver's own TauntsOn is the only
/// gate; it plays the sender's country voice from the byte, whoever sent it
/// and whatever the receiver's chat mask, alliances or defeat.
pub(crate) fn receive(state: &mut AppState, byte: u8) {
    let mode = state.match_state.session_mode();
    if state.frontend.multiplayer_shell_runtime.taunts_on(mode) == Some(true) {
        let pattern = state
            .match_state
            .sim_runtime
            .as_ref()
            .and_then(|runtime| {
                runtime
                    .resources
                    .rules
                    .country_taunt_pattern_by_index(u16::from(byte >> 4))
            })
            .map(str::to_owned);
        play(state, byte, pattern.as_deref());
    }
}

fn play(state: &mut AppState, byte: u8, pattern: Option<&str>) {
    if let (Some(sfx), Some(assets)) = (
        state.audio.sfx_player.as_mut(),
        state.process_assets.manager(),
    ) {
        sfx.play_taunt(byte, assets, pattern);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_admits_the_first_taunt_then_one_per_five_seconds() {
        let fresh = TauntClock::default();
        assert!(fresh.admits(0), "-1 admits at any time");
        let mut sent = TauntClock::default();
        sent.sent(10_000);
        assert!(!sent.admits(15_000), "5000 ms is not more than 5000");
        assert!(sent.admits(15_001));
        assert!(!sent.admits(9_000), "an earlier time is a negative delta");
        let mut wrapped = TauntClock::default();
        wrapped.sent(u64::from(u32::MAX - 1_000));
        assert!(!wrapped.admits(1u64 << 32), "1001 ms across the wrap");
        assert!(
            wrapped.admits((1u64 << 32) + 4_000),
            "5001 ms across the wrap"
        );
    }

    #[test]
    fn the_byte_packs_the_country_over_the_taunt_number() {
        assert_eq!(taunt_byte(0, 1), 0x01);
        assert_eq!(taunt_byte(8, 8), 0x88);
        assert_eq!(taunt_byte(9, 3), 0x93);
    }
}
