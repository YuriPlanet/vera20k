//! Read-only mission briefing and source authority for authored campaigns.

use crate::app::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::sim::control_authority::ControlScope;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::pause_menu::InGameMenuState;

#[derive(Debug, Eq, PartialEq)]
struct RoleRow {
    source: Option<usize>,
    house: InternedId,
    allied: bool,
    control: ControlScope,
}

fn role_rows(sim: &Simulation) -> Vec<RoleRow> {
    let projection = sim.control_projection();
    let local = if sim.session().watcher() {
        None
    } else {
        projection.local_command_house()
    };
    let role_houses: Vec<_> = sim
        .session()
        .network_sources(sim.houses().len())
        .filter_map(|(source, index)| {
            Some((
                Some(usize::try_from(source).ok()?),
                *sim.houses().order().get(index)?,
            ))
        })
        .collect();
    let other_houses = sim
        .houses()
        .order()
        .iter()
        .copied()
        .filter(|house| {
            !role_houses
                .iter()
                .any(|&(_, role_house)| role_house == *house)
                && sim
                    .houses()
                    .get(house)
                    .is_some_and(|house| !house.multiplay_passive())
        })
        .map(|house| (None, house));
    role_houses
        .iter()
        .copied()
        .chain(other_houses)
        .map(|(source, house)| {
            let control = match local {
                Some(local) if projection.can_manage_primary_house(house, local) => {
                    ControlScope::OwnFull
                }
                Some(local) if projection.can_manage_from(house, local) => {
                    ControlScope::FullControl
                }
                Some(local) if projection.can_order_from(house, local) => ControlScope::UnitsOnly,
                Some(_) | None => ControlScope::None,
            };
            RoleRow {
                source,
                house,
                allied: local.is_some_and(|local| {
                    sim.house_alliances()
                        .is_allied(sim.interner(), local, house)
                }),
                control,
            }
        })
        .collect()
}

/// Two players can share a House and still see different authority over other
/// Houses. Use the same command projection as ordinary in-game controls.
pub(crate) fn open(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) -> bool {
    let Some(start) = match_state.session.network_authored_start() else {
        return false;
    };
    let Some(runtime) = match_state.sim_runtime.runtime() else {
        return false;
    };
    let sim = &runtime.simulation;
    let text = |key| process_assets.csf_text(key);
    let briefing = match match_state.loaded_map_source.as_ref() {
        Some(crate::map::source::LoadedMapSource::Loose { path, .. }) => {
            crate::util::file_system::read(path).ok().and_then(|bytes| {
                crate::map::basic::briefing_text_with_csf(&bytes, process_assets.csf.as_ref())
            })
        }
        _ => None,
    }
    .or_else(|| {
        let assets = process_assets.manager()?;
        let mission =
            crate::rules::retail_sources::select_ini(assets, assets.ini_roots().mission()).ok()?;
        let name = sim.session().map_name();
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let key = crate::rules::campaign_loading::restate_briefing_key(&mission.ini, name);
        (key != "Brief:Error").then(|| process_assets.csf_text(&key))
    })
    .unwrap_or_default();
    let mut lines = Vec::new();
    if !briefing.is_empty() {
        lines.push(briefing);
        lines.push(String::new());
    }
    lines.push(text("VERA:CampaignRoles"));
    for row in role_rows(sim) {
        let house = sim.resolve(row.house);
        let access = text(match row.control {
            ControlScope::OwnFull => "VERA:CampaignOwnHouse",
            ControlScope::FullControl => "VERA:CampaignFullControl",
            ControlScope::UnitsOnly => "VERA:CampaignUnitsOnly",
            ControlScope::None if row.allied => "VERA:CampaignAlliedHouse",
            ControlScope::None => "VERA:CampaignViewOnly",
        });
        let role = row
            .source
            .and_then(|source| start.authored.roles().get(source));
        lines.push(match role {
            Some(role) => format!("{} · {house} · {access}", role.name),
            None => format!("{house} · {access}"),
        });
    }
    match_state.match_presentation.restate_text = lines.join("\n");
    App::enter_in_game_menu_state(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        InGameMenuState::MissionBriefing,
    );
    true
}

#[cfg(all(test, tests_app_input_a_c))]
mod tests {
    use super::*;
    use crate::rules::rule_policy::RulePolicy;
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;

    #[test]
    fn watcher_has_no_role_row_or_delegated_permission() {
        let mut sim = Simulation::new();
        sim.install_session_admission(
            ScenarioGameMode::Lan,
            None,
            RulePolicy::stock_campaign().with_counts_player_control_as_human(false),
        );
        let owner = sim.interner_mut_for_test().intern("Americans");
        let third = sim.interner_mut_for_test().intern("Russians");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.houses_mut_for_test()
            .register(HouseState::new(third, 0, None, false, 0, 10));
        let session = sim.session_mut_for_test();
        session.set_current_house_for_test(Some(owner));
        session.install_shared_house_sources_for_test(vec![0, 0], Some(1));
        sim.install_control_grants(vec![ControlGrant {
            source: 1,
            house: 1,
            level: ControlGrantLevel::UnitsOnly,
        }])
        .unwrap();
        let player_rows = role_rows(&sim);
        assert_eq!(player_rows.len(), 3);
        assert!(
            player_rows[..2]
                .iter()
                .all(|row| row.control == ControlScope::OwnFull)
        );
        assert_eq!(player_rows[2].control, ControlScope::UnitsOnly);

        sim.install_watcher();
        let watcher_rows = role_rows(&sim);
        assert_eq!(watcher_rows.len(), 3);
        assert!(watcher_rows[..2].iter().all(|row| row.house == owner));
        assert!(
            watcher_rows
                .iter()
                .all(|row| row.control == ControlScope::None)
        );
    }
}
