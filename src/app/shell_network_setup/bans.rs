//! VERA's room bans on the countries players may play
//! ([`crate::sim::game_options::RoomBans`]). The host enforces them: a banned
//! choice becomes Random, and the host's Random draw passes banned countries
//! over. The ban on building types is the simulation's (`can_build`).

use crate::app::AppState;
use crate::rules::ruleset::CountryIdx;
use crate::sim::game_options::RoomBans;
use crate::skirmish_launch::{LaunchCountry, SkirmishLocalSlot};

/// Every `[Countries]` entry `bans` takes from the players. None in
/// Cooperative, whose campaign decides the countries.
pub(crate) fn banned_countries(state: &AppState, bans: RoomBans) -> Vec<LaunchCountry> {
    let Some(rules) = state
        .rules()
        .filter(|_| !bans.is_empty() && !super::cooperative::current(state))
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
fn room_bans(state: &AppState) -> RoomBans {
    state
        .match_state
        .session
        .network_setup()
        .map_or_else(RoomBans::default, |launch| launch.options.bans)
}

/// A joining or choosing player asking for a banned country gets Random.
pub(super) fn admit_slot(state: &AppState, slot: &mut SkirmishLocalSlot) {
    if !slot.country_random && banned_countries(state, room_bans(state)).contains(&slot.country) {
        slot.country_random = true;
    }
}

/// After the host bans a country, every row playing it goes to Random.
pub(super) fn clear_banned_rows(state: &mut AppState) -> anyhow::Result<()> {
    let banned = banned_countries(state, room_bans(state));
    state
        .match_state
        .session
        .random_for_network_countries(|country| banned.contains(&country))
}
