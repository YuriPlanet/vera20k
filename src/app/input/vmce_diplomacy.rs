//! Player identities follow the frozen authored seats, even when the House's
//! ordinary UI name is just its country's name.

use crate::app::AppState;
use crate::app::match_runtime::session::SessionMode;
use crate::app::presentation::house_ui::faction_display_name;
use crate::map::scenario_menu::LocalScenario;
use crate::rules::ruleset::{RuleSet, SideIdx};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::RoleIdentity;

pub(crate) fn selection(state: &AppState) -> Option<&LocalScenario> {
    let session = &state.match_state.session;
    (state.match_state.session_mode() == SessionMode::Lan)
        .then(|| session.local_network_scenario())
        .flatten()
}

pub(crate) fn briefing_title() -> String {
    crate::i18n::tr("VERA:VmceBriefing")
}

pub(crate) fn briefing_text(state: &AppState) -> Option<String> {
    let selection = selection(state)?;
    Some(
        selection
            .presentation
            .as_ref()
            .and_then(|presentation| presentation.briefing.clone())
            .unwrap_or_else(|| crate::i18n::tr("VERA:VmceNoBriefing")),
    )
}

pub(super) fn names(
    state: &AppState,
    sim: &Simulation,
    rules: &RuleSet,
    selection: &LocalScenario,
    house: InternedId,
) -> Option<RoleIdentity> {
    let session = &state.match_state.session;
    let (source, human) = session.network_setup().and_then(|launch| {
        let source = *session
            .network_owner_sources(sim.interner().resolve(house))
            .first()?;
        Some((source, launch.humans.get(source)?))
    })?;
    let (role, faction) = if let Some(player) = session.frozen_authored_player(source) {
        let role = crate::app::shell_network_setup::local_scenarios::role_title(
            selection,
            &player.role,
            None,
        );
        let faction = sim
            .houses()
            .get(&house)
            .and_then(|house| u8::try_from(house.side_index()).ok())
            .and_then(|side| rules.side_name(SideIdx(side)))
            .map(faction_display_name);
        (role, faction)
    } else {
        (crate::i18n::tr("VERA:VmceDiplomacyObserver"), None)
    };
    let name = if sim.session().current_house() == Some(house) {
        crate::i18n::tr_fill("VERA:VmceDiplomacySelf", &[("name", &human.name)])
    } else {
        human.name.clone()
    };
    Some(RoleIdentity {
        player: name,
        role,
        faction,
    })
}
