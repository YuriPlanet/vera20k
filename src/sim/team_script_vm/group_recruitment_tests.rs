//! SOV07's authored discovery/recruitment chain in a controlled flat arena.
//! This exercises the stock records, not a campaign playthrough.

use std::collections::BTreeSet;

use crate::map::entities::EntityCategory;
use crate::map::map_file::MapFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

#[test]
fn sov07_discovery_recruits_the_nine_authored_group_members_after_reload() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = MapFile::from_bytes(assets.get_ref("SOV07TMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("SOV07TMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "SOV07TMD.INI")
            .unwrap()
            .ini
    });
    let (rules, _, _, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let teams = TeamAiIniRegistry::load_retail(&assets, &map.ini, false).unwrap();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(0x6EAA90);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .initialize_campaign_startup(1, 7, None, 0, CampaignCarryOver::CLEARED)
        .unwrap();
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    crate::sim::scenario_bootstrap::initialize_campaign_current_house(
        &mut sim.session,
        &mut sim.houses,
        &map.ini,
    );
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &teams, None, None);
    // The arena exercises the attached discovery Tag. Its nine original
    // MapEntity records retain health, missions, recruitment bytes and Group;
    // only their coordinates are clustered to bound formation and movement.
    let mut placements: Vec<_> = map
        .entities
        .iter()
        .filter(|entity| {
            entity.category == EntityCategory::Infantry
                && entity.trigger_tag.as_deref() == Some("0CBEB7DC")
        })
        .cloned()
        .collect();
    assert_eq!(placements.len(), 9);
    for (index, entity) in placements.iter_mut().enumerate() {
        entity.cell_x = 10 + (index % 3) as u16;
        entity.cell_y = 10 + (index / 3) as u16;
    }
    assert_eq!(sim.spawn_from_map(&placements, Some(&rules)), 9);
    assert!(
        sim.entities()
            .values()
            .all(|entity| entity.object_group() == 7)
    );
    let members: BTreeSet<_> = sim
        .entities()
        .values()
        .map(|entity| entity.stable_id())
        .collect();
    let owner = Simulation::scenario_house_named(&sim.houses, "BadGuy1 House").unwrap();
    let player = sim.session.current_house().unwrap();
    let team_type = sim.interner.get("0BC12B7C").unwrap();
    assert_eq!(sim.team_script_vm.team_type_ini[&team_type].group, 7);
    assert!(!sim.team_script_vm.team_type_ini[&team_type].recruiter);
    assert!(sim.team_script_vm.first_team_of_type(team_type).is_none());

    // Techno::DiscoveredBy6F4960 -> attached event4 at6F4A18 -> the active
    // TAction::Execute4 call6DEB6E -> CreateTeam6F09C0 constructs an empty Team.
    assert!(sim.discover_techno(
        *members.first().unwrap(),
        player,
        UninitContext::with_rules(&rules),
    ));
    let team = sim.team_script_vm.first_team_of_type(team_type).unwrap();
    let team_id = team.id();
    assert_eq!(team.owner(), owner);
    assert_eq!(team.member_count(), 0);
    assert_eq!(team.cursor(), -1);

    // The unjoined authored groups must survive save/load before Recruit.
    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "sov07_group_arena", 0);
    let mut sim = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let mut reached_attack = false;
    for _ in 0..60 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        let team = sim
            .team_script_vm
            .team(team_id)
            .expect("stock guard script stays live");
        assert!(team.refusal().is_none(), "{:?}", team.refusal());
        reached_attack |= team.formed() && team.cursor() >= 1;
        if reached_attack {
            break;
        }
    }
    // TeamAI6E9281 -> Recruit6EAA90 reads each Infantry's +214, populated
    // by ReadINI51FD89; Recruiter=no admits these matching Group7 members.
    let team = sim.team_script_vm.team(team_id).unwrap();
    assert_eq!(team.members().collect::<BTreeSet<_>>(), members);
    assert!(
        team.formed() && reached_attack,
        "the real script reaches attack-any"
    );
    for &member in &members {
        assert_eq!(
            sim.team_script_vm.team_for_member(member).map(|(id, _)| id),
            Some(team_id)
        );
    }
    sim.destroy_team(team_id, &rules, FrameEffects::default());
    assert!(sim.team_script_vm.team(team_id).is_none());
    for member in members {
        assert_eq!(sim.team_script_vm.team_for_member(member), None);
        assert!(sim.entities().get(member).unwrap().lifecycle.object_alive());
        assert_eq!(sim.entities().get(member).unwrap().object_group(), 7);
    }
}
