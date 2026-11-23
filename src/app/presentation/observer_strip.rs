//! The observer's sidebar strip: in place of build pictures, a row pair for
//! each player with their name, kills, units alive and money
//! (`StripClass::Draw @ 0x006A9540`, `0x006AA043..0x006AA59B`).
//!
//! Native shows it only to the House in `[0xAC1198]`, the last observer
//! created. VERA's own rooms admit several observers and give each the
//! observer's treatment ([`crate::sim::scenario_session`]), so every
//! observer sees this strip, and the list leaves out every observer's House
//! where native leaves out `[0xAC1198]` alone. With one observer the two
//! agree; the maintainer chose to keep this for several (2026-11-15).
//!
//! A VERA watcher on its observer step (Tab after the last player) sees it
//! too, with the time counter, in the place of the player it last followed,
//! who stays listed ([`shows_observer_sidebar`]).

use crate::app::AppState;
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::sim::intern::InternedId;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::Simulation;
use crate::ui::sidebar::ObserverEntry;

/// `Sidebar+0x53AC` holds eight Houses (`0x006A55B7..0x006A581A`).
const OBSERVER_HOUSES: usize = 8;

/// One listed House and the values `StripClass::AI @ 0x006A8B30` keeps for
/// it each frame (`0x006A903E..0x006A9235`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObserverHouse {
    pub(crate) house: InternedId,
    /// Both 20-entry kill tables (`+0x53E4`, `+0x5438`).
    pub(crate) kills: i32,
    /// [`crate::sim::house_tracking::HouseTracking::observer_units`].
    pub(crate) units: i32,
    /// Available money, a negative balance shown as 0.
    pub(crate) credits: i32,
    /// `+0x1F5`, which greys the House's lines.
    pub(crate) defeated: bool,
}

/// Whether the local player, whose House is `local`, is shown the observer's
/// sidebar: that House is an observer's, or this process watches on its
/// observer step (`ScenarioSession::watcher_observes`). The rest of what a
/// watcher sees and hears stays the followed player's.
pub(crate) fn shows_observer_sidebar(sim: &Simulation, local: InternedId) -> bool {
    let session = sim.session();
    session.is_observer_house(local) || session.watcher_observes()
}

/// The observer strip's Houses while the local player, whose House is
/// `local`, is shown the observer's sidebar, `None` otherwise. `SidebarClass::Init @ 0x006A5310` lists, only in a LAN or
/// online game, the human Houses in House-array order that are neither
/// MultiplayPassive nor the observer, at most eight (`0x006A55B7..`). Init
/// runs at every scenario start and load; nothing it reads changes during a
/// match, so the list is read again each frame.
pub(crate) fn observer_houses(sim: &Simulation, local: InternedId) -> Option<Vec<ObserverHouse>> {
    let session = sim.session();
    if !shows_observer_sidebar(sim, local) {
        return None;
    }
    if !matches!(
        session.game_mode(),
        ScenarioGameMode::Lan | ScenarioGameMode::Wol
    ) {
        return Some(Vec::new());
    }
    Some(
        sim.houses()
            .order()
            .iter()
            .filter_map(|&id| {
                let house = sim.houses().get(&id)?;
                (house.is_human() && !house.multiplay_passive() && !session.is_observer_house(id))
                    .then(|| ObserverHouse {
                        house: id,
                        kills: house.stats().kills() as i32,
                        units: house.tracking().observer_units(),
                        credits: crate::sim::production::credits_for_owner(
                            sim,
                            sim.interner().resolve(id),
                        )
                        .max(0),
                        defeated: house.is_defeated(),
                    })
            })
            .take(OBSERVER_HOUSES)
            .collect(),
    )
}

/// The strip's texts for `houses`: the House's UI name in its colour
/// (StripClass::AI's colour, `0x006A9142..0x006A9185`: "Grey" once
/// defeated; the observer's "LightGrey" never lists), then each line through
/// its string-table format with the value as text (`_itow` `0x007CBCAF`,
/// `swprintf` `0x007CA564`, `0x006AA2CE..0x006AA583`). The rank line shows
/// only online and VERA keeps no ladder rank, so it is always
/// `TXT_UNRANKED`.
pub(crate) fn observer_entries(
    state: &AppState,
    sim: &Simulation,
    houses: &[ObserverHouse],
) -> Vec<ObserverEntry> {
    let Some(rules) = state.rules() else {
        return Vec::new();
    };
    let computer = state.csf_text("TXT_COMPUTER");
    let sources = crate::app::presentation::house_ui::HouseUiSources::from_state(state, &computer);
    let text = |key| state.csf_text(key);
    let online = sim.session().game_mode() == ScenarioGameMode::Wol;
    houses
        .iter()
        .filter_map(|listed| {
            let house = sim.houses().get(&listed.house)?;
            let country = house.country();
            let country_name = country.map_or("", |id| sim.interner().resolve(id));
            let mut lines = Vec::with_capacity(4);
            if online {
                lines.push(text("TXT_UNRANKED"));
            }
            for (key, value) in [
                ("GUI:ObsKills", listed.kills),
                ("GUI:ObsUnits", listed.units),
                ("GUI:ObsCredits", listed.credits),
            ] {
                lines.push(format_csf(&text(key), &[CsfArg::Str(&value.to_string())]));
            }
            Some(ObserverEntry {
                origin: [0.0, 0.0],
                side: rules.country_side_index_raw(country_name).unwrap_or(-1),
                country: rules
                    .country_index(country_name)
                    .map_or(-1, |index| i32::from(index.0)),
                name: sources.ui_name(sim, listed.house, country, |country| {
                    crate::app::presentation::house_ui::country_display_name(state, sim, country)
                }),
                rgb: if listed.defeated {
                    sources.named_scheme_rgb("Grey").unwrap_or([0xFF; 3])
                } else {
                    sources.text_rgb(sim, listed.house)
                },
                lines,
            })
        })
        .collect()
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    /// Two humans, an observer, a passive civilian and a computer, in that
    /// House order, with the observer the local player.
    fn room(mode: ScenarioGameMode) -> (Simulation, InternedId) {
        let mut sim = Simulation::with_seed(1);
        let mut ids = Vec::new();
        for (index, name) in ["Host", "Watcher", "Guest", "Neutral", "Computer"]
            .into_iter()
            .enumerate()
        {
            let id = sim.intern(name);
            let mut house =
                crate::sim::house_state::HouseState::new(id, 0, None, index != 4, 0, 10);
            house.set_multiplay_passive_for_test(index == 3);
            sim.houses_mut_for_test().register(house);
            ids.push(id);
        }
        let session = sim.session_mut_for_test();
        session.set_game_mode(mode);
        session.install_observer_houses_for_test(vec![ids[1]]);
        (sim, ids[1])
    }

    #[test]
    fn the_observer_lists_the_human_players_of_a_network_game_only() {
        let (sim, observer) = room(ScenarioGameMode::Lan);
        let listed: Vec<_> = observer_houses(&sim, observer)
            .expect("an observer's strip")
            .iter()
            .map(|house| sim.interner().resolve(house.house).to_owned())
            .collect();
        assert_eq!(listed, ["Host", "Guest"]);
        // A player has no strip; a skirmish observer's lists nobody.
        let host = sim.interner().get("Host").expect("host");
        assert_eq!(observer_houses(&sim, host), None);
        let (sim, observer) = room(ScenarioGameMode::Skirmish);
        assert_eq!(observer_houses(&sim, observer), Some(Vec::new()));
    }
}
