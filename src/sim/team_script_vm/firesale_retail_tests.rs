//! Authored skirmish Script30 through real recruitment and House Strategy.
//! Original gamemd:6E9FEC ->6E99E1 writes House+250 and Team+80 only;
//! due House4F9015/4FD7A0 consumes it through5013A0 then501400.
//! This is a supported map route, not an unchanged-stock AI frequency claim.

use crate::headless_scenario::{SIM_TICK_MS, battle_session, load_session};
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};

fn scenario(stock: &str, action: i32) -> String {
    let mut map = stock.to_owned();
    // Normal map definitions and actors only. Trigger4 must create a real
    // Team; ordinary Team AI must recruit, form and execute its script.
    for (section, records) in [
        (
            "Infantry",
            "FireRifle=Computer,E2,256,30,85,0,Guard,0,None,0,-1,0,1,1\n".to_owned(),
        ),
        (
            "Structures",
            "FireBarracks=Computer,NAHAND,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n\
             FirePlant=Computer,NAPOWR,256,31,89,0,None,0,0,1,0,0,0,None,None,None,0\n"
                .to_owned(),
        ),
        ("TeamTypes", "FS52=F0520001\n".to_owned()),
        (
            "F0520001",
            "Name=FireSale witness\nHouse=Russians\nScript=F0520002\nTaskForce=F0520003\n\
             Waypoint=A\nMax=1\nPriority=7\nGroup=-1\nAutocreate=no\nReinforce=no\n\
             IsBaseDefense=no\nSuicide=no\nAggressive=no\nAreTeamMembersRecruitable=yes\n"
                .to_owned(),
        ),
        ("ScriptTypes", "FS52=F0520002\n".to_owned()),
        (
            "F0520002",
            format!("Name=FireSale step\n0={action},0\n1=2,0\n"),
        ),
        ("TaskForces", "FS52=F0520003\n".to_owned()),
        (
            "F0520003",
            "Name=One stock rifleman\nGroup=-1\n0=1,E2\n".to_owned(),
        ),
        (
            "Triggers",
            "F0520004=Russians,<none>,FireSale,0,1,1,1,0\n".to_owned(),
        ),
        ("Events", "F0520004=1,13,0,1\n".to_owned()),
        ("Actions", "F0520004=1,4,1,F0520001,0,0,0,0,A\n".to_owned()),
        ("Tags", "F0520005=0,FireSale,F0520004\n".to_owned()),
    ] {
        let marker = format!("[{section}]");
        if let Some(start) = map.find(&marker) {
            let end = start + map[start..].find('\n').unwrap() + 1;
            map.insert_str(end, &records);
        } else {
            map.push_str(&format!("\n{marker}\n{records}"));
        }
    }
    map
}

#[test]
#[ignore = "slow; authored skirmish fire sale compares two 400-frame histories"]
fn authored_team_fire_sale_waits_for_strategy_then_sells_and_hunts() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let physical = assets.resolve_ref("XMP03T4.MAP").unwrap();
    let stock = String::from_utf8(physical.bytes.to_vec()).unwrap();
    // Census through the production fixed-AIMD/scenario reader, not grep.
    // It bounds this fixture to authored reachability; other maps remain open.
    let stock_registry =
        TeamAiIniRegistry::load_retail(&assets, &IniFile::from_str(&stock), true).unwrap();
    assert!(!stock_registry.scripts.is_empty() && !stock_registry.team_types.is_empty());
    assert!(
        stock_registry
            .scripts
            .iter()
            .flat_map(|script| &script.actions)
            .all(|action| action.action_id != 30)
    );
    let directory = crate::map::source::test_support::TestDirectory::new("team-fire-sale");
    for action in [2, 30] {
        let map = scenario(&stock, action);
        let path = directory.path().join(format!("fire-sale-{action}.map"));
        std::fs::write(&path, &map).unwrap();
        let name = path.to_str().unwrap();
        let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
        launch.local_slot_mut().country = LaunchCountry::America;
        launch.local_slot_mut().start_position = LaunchStartPosition::Position(2);
        launch.local_slot_mut().team = LaunchTeam::None;
        launch.opponents[0].start_position = LaunchStartPosition::Position(1);
        launch.opponents[0].team = LaunchTeam::None;
        launch.options.starting_credits = 5000;
        launch.options.bases = true;
        launch.options.unit_count = 0;
        launch.options.tech_level = 10;
        launch.options.short_game = false;
        launch.options.crates = false;
        launch.options.shroud = false;
        let mut scene = load_session(&root, name, 0x1234_5678, launch).unwrap();
        let rules = &scene.runtime.resources.rules;
        assert!(rules.object("NAHAND").unwrap().has_factory());
        assert!(rules.has_buildup("NAHAND") && rules.has_buildup("NAPOWR"));
        let sim = scene.sim();
        let computer =
            crate::sim::world::Simulation::scenario_house_named(&sim.houses, "Computer").unwrap();
        let local = sim.session.current_house().unwrap();
        let team_type = sim.interner.get("F0520001").unwrap();
        let script = sim.interner.get("F0520002").unwrap();
        assert_eq!(
            sim.team_script_vm.scripts[&script].actions[0].action_id,
            action
        );
        let actor = |kind: &str, cell| {
            sim.entities()
                .values()
                .find(|entity| {
                    entity.owner() == computer
                        && sim.interner.resolve(entity.type_ref()) == kind
                        && (entity.position.rx(), entity.position.ry()) == cell
                })
                .unwrap()
                .stable_id()
        };
        let rifle = actor("E2", (30, 85));
        let buildings = [actor("NAHAND", (31, 85)), actor("NAPOWR", (31, 89))];
        for (id, health) in [(rifle, 125), (buildings[0], 500), (buildings[1], 750)] {
            assert_eq!(sim.entities().get(id).unwrap().health.current(), health);
        }
        assert!(buildings.iter().all(|&id| {
            let entity = sim.entities().get(id).unwrap();
            !entity.in_limbo() && entity.building_art.actually_placed() && !entity.building_down()
        }));
        let mut dispatch = None;
        let mut response = None;
        let mut next_line = false;
        let mut team_retired = false;
        let mut created = std::collections::BTreeSet::new();
        let mut sold_events = 0;
        for _ in 0..400 {
            let sim = scene.sim();
            let native_frame = sim.session.binary_frame() as i32;
            let before = *sim.houses[&computer].strategy_emergency();
            let before_timer = *sim.houses[&computer].strategy_timer();
            let before_economy_mode = sim.houses[&computer].ai_production().mode();
            let commands = scene.runtime.simulation.take_due_commands();
            let frame = scene
                .runtime
                .advance_frame(
                    &commands,
                    SIM_TICK_MS,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            sold_events += frame.sound_events.iter().filter(|event| {
                matches!(event, SimSoundEvent::StructureSold { owner } if *owner == computer)
            }).count();
            let sim = scene.sim();
            let house = &sim.houses[&computer];
            let team = sim
                .team_script_vm
                .teams_in_order()
                .find(|team| team.team_type_id() == Some(team_type));
            if let Some(team) = team {
                created.insert(team.id());
                assert_eq!(
                    team.owner(),
                    computer,
                    "country reader resolves the real AI House"
                );
                if dispatch.is_none() && team.formed() && team.cursor() == 0 {
                    assert_eq!(team.members().collect::<Vec<_>>(), [rifle]);
                    assert!(
                        before_timer.remaining(native_frame) > 0,
                        "admit a naturally non-due Strategy"
                    );
                    assert_eq!(*house.strategy_timer(), before_timer);
                    assert_eq!(house.ai_production().mode(), before_economy_mode);
                    assert!(!house.strategy_emergency().all_to_hunt_bias());
                    assert!(
                        buildings.iter().all(|&id| !sim
                            .entities()
                            .get(id)
                            .unwrap()
                            .building_down())
                    );
                    let mut expected = before;
                    if action == 30 {
                        assert_eq!(
                            house.strategy_emergency().mode(),
                            4,
                            "original6E99E5; admitted Team refusal={:?}",
                            team.refusal()
                        );
                        expected.set_state_four();
                    }
                    assert_eq!(
                        *house.strategy_emergency(),
                        expected,
                        "real formed Team reached action{action}; refusal={:?}",
                        team.refusal()
                    );
                    assert_eq!(team.advance_pending(), action == 30);
                    assert!(team.refusal().is_none());
                    dispatch = Some((sim.session.binary_frame(), team.id()));
                }
                next_line |= team.cursor() == 1;
            }
            if let Some((_, id)) = dispatch {
                team_retired |= sim.team_script_vm.team(id).is_none();
            }
            if response.is_none() && house.strategy_emergency().all_to_hunt_bias() {
                assert_eq!(action, 30);
                assert!(sim.session.binary_frame() > dispatch.unwrap().0);
                assert!(before_timer.expired(native_frame));
                assert_eq!(house.strategy_timer().start_frame(), native_frame);
                assert!(house.strategy_timer().duration() > 0);
                assert_eq!(house.strategy_emergency().mode(), 4);
                assert!(
                    buildings
                        .iter()
                        .all(|&id| sim.entities().get(id).unwrap().building_down())
                );
                assert_eq!(
                    sim.entities().get(rifle).unwrap().mission.queued().known(),
                    Some(MissionType::Hunt)
                );
                assert_eq!(sim.team_script_vm.team_for_member(rifle), None);
                response = Some(sim.session.binary_frame());
            }
        }
        assert!(
            dispatch.is_some(),
            "real Trigger4/Team recruitment must reach its first line"
        );
        assert_eq!(
            created.len(),
            1,
            "one-shot map trigger creates one real Team"
        );
        assert_eq!(
            next_line,
            action == 30,
            "the real next Script visit consumes Team+80"
        );
        assert_eq!(response.is_some(), action == 30);
        assert_eq!(
            team_retired,
            action == 30,
            "AllToHunt releases the last member before normal Team destruction"
        );
        let sim = scene.sim();
        assert!(sim.entities().get(rifle).unwrap().lifecycle.object_alive());
        assert_eq!(sim.houses[&local].economy().credits(), 5000);
        for id in buildings {
            assert_eq!(
                sim.entities().get(id).is_none(),
                action == 30,
                "healthy authored building follows normal sale retirement"
            );
        }
        if action == 30 {
            assert!(sold_events >= 2);
            let economy = sim.houses[&computer].economy();
            assert!(
                i64::from(economy.credits()) + i64::from(economy.spent_credits()) > 5000,
                "ordinary completed sales return money through the existing Economy owner"
            );
        } else {
            assert_eq!(sold_events, 0);
        }
    }
}
