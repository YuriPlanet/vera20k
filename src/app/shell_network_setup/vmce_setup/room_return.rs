//! Role preferences for the new setup revision of a continuing LAN room.

use std::net::SocketAddrV4;

use crate::app::AppState;
use crate::app::match_runtime::session::{LocalScenarioRules, LocalScenarioSource, MatchSession};
use crate::map::scenario_menu::LocalScenarioKey;

struct RememberedGuest {
    endpoint: SocketAddrV4,
    name: String,
    house: String,
}

pub(crate) struct ReturningGuestRoles {
    destination: LocalScenarioKey,
    guests: Vec<RememberedGuest>,
}

pub(crate) struct RoomReturn {
    local_house: String,
    guests: ReturningGuestRoles,
    settings: Option<(LocalScenarioSource, LocalScenarioRules)>,
    game_speed: i32,
}

impl RoomReturn {
    pub(crate) fn capture(
        session: &MatchSession,
        destination: &LocalScenarioKey,
        keep_room_rules: bool,
    ) -> Option<Self> {
        let launch = session.network_setup()?;
        let local = launch.local_player_index;
        let local_house = session.frozen_authored_player(local)?.house.clone();
        let guests = session
            .network_connected_source_indices()
            .iter()
            .copied()
            .filter(|&source| source != local)
            .filter_map(|source| {
                Some(RememberedGuest {
                    endpoint: session.network_endpoint(source)?,
                    name: launch.humans.get(source)?.name.clone(),
                    house: session.frozen_authored_player(source)?.house.clone(),
                })
            })
            .collect();
        let (source, rules) = session.frozen_local_scenario_settings()?;
        let settings = (keep_room_rules && &session.local_network_scenario()?.key == destination)
            .then(|| (source, rules.clone()));
        Some(Self {
            local_house,
            guests: ReturningGuestRoles {
                destination: destination.clone(),
                guests,
            },
            settings,
            game_speed: rules.game_speed,
        })
    }

    pub(crate) fn restore(self, state: &mut AppState) {
        let session = &mut state.match_state.session;
        if !session
            .local_network_scenario()
            .is_some_and(|scenario| scenario.key == self.guests.destination)
        {
            return;
        }
        let Some(lobby) = session.local_scenario_lobby() else {
            return;
        };
        let mut settings = self
            .settings
            .filter(|(source, _)| {
                session
                    .verify_local_scenario_source(&self.guests.destination, source)
                    .is_ok()
            })
            .map_or_else(|| lobby.rules.clone(), |(_, rules)| rules);
        settings.game_speed = self.game_speed;
        if let Err(error) = session.set_local_scenario_rules(settings) {
            log::debug!("Could not restore authored room rules: {error:#}");
        }
        if let Some(local) = session
            .network_setup()
            .map(|launch| launch.local_player_index)
        {
            restore_role(session, local, &self.local_house);
        }
        if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
            protocol.scenario_return_roles = Some(self.guests);
        }
    }
}

/// Apply the previous role only after the host's normal authenticated join.
/// Refitting by House keeps the role when its PlayerN ordinal changes.
pub(crate) fn restore_admitted_role(state: &mut AppState, endpoint: SocketAddrV4, name: &str) {
    let session = &mut state.match_state.session;
    let Some(roles) = state
        .frontend
        .network_setup_protocol
        .as_mut()
        .and_then(|protocol| protocol.scenario_return_roles.as_mut())
    else {
        return;
    };
    if !session
        .local_network_scenario()
        .is_some_and(|scenario| scenario.key == roles.destination)
    {
        return;
    }
    let Some(index) = roles
        .guests
        .iter()
        .position(|guest| guest.endpoint == endpoint && guest.name.eq_ignore_ascii_case(name))
    else {
        return;
    };
    let guest = roles.guests.swap_remove(index);
    if let Some(source) = session.network_endpoint_source(endpoint) {
        restore_role(session, source, &guest.house);
    }
}

fn restore_role(session: &mut MatchSession, source: usize, house: &str) {
    let role = session.local_scenario_lobby().and_then(|lobby| {
        lobby
            .roles
            .iter()
            .position(|target| target.eq_ignore_ascii_case(house))
            .and_then(|index| u8::try_from(index).ok())
    });
    if let Some(role) = role
        && let Err(error) = session.accept_local_scenario_role(source, Some(role))
    {
        log::debug!("Could not restore authored room role: {error:#}");
    }
}
