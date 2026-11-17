//! Action84 consumes the existing authored Group and deferred Mission owners.
//! Native Execute6DDCEC..6DDD74 (WakeupGroup6E03B0) queues Guard(5,false).

use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, Simulation};

#[test]
fn authored_signed_group_queues_guard_across_houses() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = Simulation::with_seed(0x6DDC_EC);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // The clear arena and registered Houses are scene priors. Group, missions,
    // object construction and the live elapsed Trigger use production readers.
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    // This unit-only scene uses the normal non-short-game defeat policy.
    sim.session.game_options_mut_for_test().short_game = false;
    for (name, human) in [("Americans", true), ("Russians", false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, Some(id), human, 0, 10));
    }
    let human = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(human));
    let map = super::tests::trigger_fixture_map(
        "[Units]\n\
         0=Americans,MTNK,256,8,8,0,Sleep,None,0,-7,0,0,1,1\n\
         1=Russians,HTNK,256,20,20,0,Harmless,None,0,-7,0,0,1,1\n\
         2=Americans,MTNK,256,10,8,0,Sleep,None,0,-8,0,0,1,1\n\
         3=Americans,MTNK,256,12,8,0,Guard,None,0,-7,0,0,1,1\n\
         [Triggers]\nWAKE=Americans,<none>,Wake group,0,1,1,1,0\n\
         [Events]\nWAKE=1,13,0,0\n\
         [Actions]\nWAKE=1,84,0,-7,0,0,0,0,A\n\
         [Tags]\nWAKE_TAG=0,Wake group,WAKE\n",
    );
    let teams = TeamAiIniRegistry::from_sources(
        &crate::rules::ini_parser::IniFile::empty(),
        &map.ini,
        true,
    );
    sim.initialize_trigger_definitions(&map, &teams, None, None, None);
    assert_eq!(sim.spawn_from_map(&map.entities, Some(rules)), 4);
    sim.classify_map_triggers(rules);
    let ids = [8, 20, 10, 12].map(|rx| {
        sim.entities()
            .values()
            .find(|entity| entity.position.rx() == rx)
            .unwrap()
            .stable_id()
    });
    for (id, group) in ids.into_iter().zip([-7, -7, -8, -7]) {
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(entity.object_group(), group, "the map's signed Group");
        assert!(
            !sim.object_has_trigger_tag(id),
            "no object attachment prior"
        );
    }
    assert_ne!(sim.entities().get(ids[1]).unwrap().owner(), human);

    let before = ids.map(|id| sim.entities().get(id).unwrap().mission.clone());
    assert_eq!(before[0].current().known(), Some(MissionType::Sleep));
    assert_eq!(before[1].current().known(), Some(MissionType::Harmless));
    let rng = sim.rng_state();

    // Poll the real Event13/Tag separately from per-object AI so an eager
    // Commence, timer reset or unrelated House/Tag gate is observable.
    sim.advance_triggers(
        rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    );
    for (index, id) in ids.into_iter().enumerate() {
        let mission = sim.entities().get(id).unwrap().mission.clone();
        if index < 2 {
            assert_eq!(mission.queued().known(), Some(MissionType::Guard));
            assert_eq!(mission.current(), before[index].current());
            assert_eq!(mission.suspended(), before[index].suspended());
            assert_eq!(mission.handler_state(), before[index].handler_state());
            assert_eq!(mission.dispatch_timer(), before[index].dispatch_timer());
            assert_eq!(
                mission.mission_start_frame(),
                before[index].mission_start_frame()
            );
            assert_eq!(mission.ai_counter(), before[index].ai_counter());
        } else {
            assert_eq!(mission, before[index], "different group or active mission");
        }
    }
    assert_eq!(sim.rng_state(), rng, "WakeupGroup draws no RNG");
}
