//! VERA's room bans on the countries players may play
//! ([`crate::sim::game_options::RoomBans`]). The host enforces them: a banned
//! choice becomes Random, and the host's Random draw passes banned countries
//! over. The ban on building types is the simulation's (`can_build`).

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::MatchSession;
use crate::app::match_runtime::state::MatchState;
use crate::rules::ruleset::CountryIdx;
use crate::sim::game_options::RoomBans;
use crate::skirmish_launch::{LaunchCountry, SkirmishLocalSlot};

/// Every `[Countries]` entry `bans` takes from the players. None in
/// Cooperative, whose campaign decides the countries.
pub(crate) fn banned_countries(
    frontend: &FrontendState,
    match_state: &MatchState,
    bans: RoomBans,
) -> Vec<LaunchCountry> {
    let Some(rules) = crate::app::state::active_rules(frontend, match_state)
        .filter(|_| !bans.is_empty() && !super::cooperative::current(&match_state.session))
    else {
        return Vec::new();
    };
    (0..=u16::MAX)
        .map(CountryIdx)
        .take_while(|&index| rules.country_name(index).is_some())
        .filter(|&index| bans.bans_country(rules, index))
        .map(|index| LaunchCountry::from_country_index(u32::from(index.0)))
        .collect()
}

/// The room's current bans.
fn room_bans(session: &MatchSession) -> RoomBans {
    session
        .network_setup()
        .map_or_else(RoomBans::default, |launch| launch.options.bans)
}

/// A joining or choosing player asking for a banned country gets Random.
pub(super) fn admit_slot(
    frontend: &FrontendState,
    match_state: &MatchState,
    slot: &mut SkirmishLocalSlot,
) {
    if !slot.country_random
        && banned_countries(frontend, match_state, room_bans(&match_state.session))
            .contains(&slot.country)
    {
        slot.country_random = true;
    }
}

/// After the host bans a country, every row playing it goes to Random.
pub(super) fn clear_banned_rows(
    frontend: &FrontendState,
    match_state: &mut MatchState,
) -> anyhow::Result<()> {
    let banned = banned_countries(frontend, match_state, room_bans(&match_state.session));
    match_state
        .session
        .random_for_network_countries(|country| banned.contains(&country))
}
