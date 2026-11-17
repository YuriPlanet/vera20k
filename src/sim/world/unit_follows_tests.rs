//! UnitReadINI743270/743664 links FOLLOWS by the Unit array; Unit7463A0
//! changes the linked peer's House before its own inherited owner callback.

use super::{FrameEffects, Simulation, TickLane, UninitContext};
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;

fn arena(rules: &RuleSet) -> Simulation {
    let tank = rules.object("MTNK").unwrap();
    assert!(
        !tank.is_train && !tank.accelerates,
        "selected stock MTNK route"
    );
    let mut sim = Simulation::with_seed(0x7463_A0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // Clear ground, Houses and non-short-game policy are scene priors. Units,
    // FOLLOWS, object Tag and Event13/Action14 all use production readers.
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses.register(HouseState::new(
            house,
            index as u8,
            Some(house),
            index == 0,
            0,
            10,
        ));
    }
    let current = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(current));
    sim
}

fn map(body: &str) -> MapFile {
    let text = format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n{body}"
    );
    MapFile::from_bytes(text.as_bytes()).unwrap()
}

fn at_x(sim: &Simulation, x: u16) -> u64 {
    sim.entities()
        .values()
        .find(|entity| entity.position.rx() == x)
        .unwrap()
        .stable_id()
}

fn restore(sim: &Simulation, rules: &RuleSet) -> Simulation {
    let terrain = sim.resolved_terrain().unwrap().clone();
    let terrain_speed = sim.terrain_speed_config().clone();
    let bytes = GameSnapshot::save(sim, 0, rules.simulation_config_hash(), "Unit FOLLOWS", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(terrain, terrain_speed, rules);
    loaded
        .restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
        .unwrap();
    loaded.resolve_type_handles(rules);
    loaded
}

#[test]
fn authored_follows_peer_changes_house_with_only_head_tagged() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let original = sim.interner.get("Russians").unwrap();
    let target = sim.interner.get("Americans").unwrap();
    let country = rules.country_index("Americans").unwrap().0;
    assert_eq!(
        sim.house_by_trigger_selector(i32::from(country), rules),
        Some(target)
    );
    let map = map(&format!(
        "[Units]\n\
         0=Russians,MTNK,256,8,8,0,Sleep,TRANSFER_TAG,0,-1,0,1,1,1\n\
         1=Russians,MTNK,256,10,8,0,Sleep,None,0,-1,0,-1,1,1\n\
         2=Russians,MTNK,256,28,28,0,Sleep,None,0,-1,0,-1,1,1\n\
         [Triggers]\nTRANSFER=Americans,<none>,Transfer linked units,0,1,1,1,0\n\
         [Events]\nTRANSFER=1,13,0,0\n\
         [Actions]\nTRANSFER=1,14,0,{country},0,0,0,0,A\n\
         [Tags]\nTRANSFER_TAG=0,Transfer linked units,TRANSFER\n"
    ));
    let teams = TeamAiIniRegistry::from_sources(&IniFile::empty(), &map.ini, true);
    sim.initialize_trigger_definitions(&map, &teams, None, None, None);
    assert_eq!(sim.spawn_from_map(&map.entities, Some(rules)), 3);
    sim.classify_map_triggers(rules);
    let [head, peer, control] = [8, 10, 28].map(|x| at_x(&sim, x));
    assert!(sim.object_has_trigger_tag(head));
    assert!(!sim.object_has_trigger_tag(peer));
    for id in [head, peer] {
        assert_eq!(sim.entities().get(id).unwrap().owner(), original);
    }
    let mut sim = restore(&sim, rules);

    // TAction14's native6E0AA0 scans attached objects, while the Unit's
    // shared7463A0 callback reaches its untagged peer synchronously.
    sim.advance_triggers(
        rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    );
    assert_eq!(sim.entities().get(head).unwrap().owner(), target);
    assert_eq!(
        sim.entities().get(peer).unwrap().owner(),
        target,
        "FOLLOWS peer transfers through the tagged head's Unit callback"
    );
    assert!(!sim.object_has_trigger_tag(peer));
    assert_eq!(sim.entities().get(control).unwrap().owner(), original);

    // Find_Path4D3ECD has no IsTrain gate: an ordinary order must publish
    // the peer's trailing destination and queue Move through shared owners.
    // Transfer to the real current human House. A base-less computer's
    // ordinary House AI immediately queues Hunt, replacing a manual order.
    let command = CommandEnvelope::new(
        target,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: head,
            target_rx: 20,
            target_ry: 8,
        },
    );
    let mut followed = false;
    for frame in 0..90 {
        let commands = if frame == 0 {
            std::slice::from_ref(&command)
        } else {
            &[]
        };
        let output = sim
            .advance_app_frame(
                commands,
                rules,
                Some(rules.overlay_registry()),
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(output.tick.frame_committed);
        let follower = sim.entities().get(peer).unwrap();
        if follower.navigation.nav_com() == Some(NavTargetRef::cell(19, 8)) {
            assert_eq!(
                sim.entities().get(head).unwrap().navigation.nav_com(),
                Some(NavTargetRef::cell(20, 8))
            );
            assert!(
                follower
                    .navigation
                    .path_replay()
                    .directions()
                    .iter()
                    .any(|&d| d != u8::MAX)
            );
            // A later peer visit in this same frame may already Commence it.
            assert!(
                follower.mission.queued().known() == Some(MissionType::Move)
                    || follower.mission.current().known() == Some(MissionType::Move)
            );
            followed = true;
            break;
        }
    }
    assert!(
        followed,
        "ordinary Move must reach the linked peer's path continuation"
    );
}

#[test]
fn uninit_clears_incoming_unit_link_but_preserves_orphan_follower_byte_after_restore() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let map = map("[Units]\n\
         0=Americans,MTNK,256,8,8,0,Sleep,None,0,-1,0,1,1,1\n\
         1=Americans,MTNK,256,10,8,0,Sleep,None,0,-1,0,2,1,1\n\
         2=Americans,MTNK,256,12,8,0,Sleep,None,0,-1,0,-1,1,1\n\
         3=Russians,MTNK,256,28,28,0,Sleep,None,0,-1,0,-1,1,1\n");
    assert_eq!(sim.spawn_from_map(&map.entities, Some(rules)), 4);
    let [head, middle, tail] = [8, 10, 12].map(|x| at_x(&sim, x));
    assert_eq!(
        sim.entities().get(head).unwrap().unit_follower(),
        Some(middle)
    );
    assert_eq!(
        sim.entities().get(middle).unwrap().unit_follower(),
        Some(tail)
    );
    assert!(sim.entities().get(tail).unwrap().is_unit_follower());

    // UnitPointerExpired7446E0 clears its pointer after inherited forwards;
    // neither it nor the deleting destructor resets the surviving tail byte.
    sim.uninit_with_context(
        middle,
        UninitContext::new(rules, Some(rules.overlay_registry())),
    );
    assert_eq!(sim.entities().get(head).unwrap().unit_follower(), None);
    assert!(sim.entities().get(tail).unwrap().is_unit_follower());
    sim.advance_app_frame(
        &[],
        rules,
        Some(rules.overlay_registry()),
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.entities().get(middle).is_none(),
        "ordinary pending-delete drain"
    );

    let sim = restore(&sim, rules);
    assert_eq!(sim.entities().get(head).unwrap().unit_follower(), None);
    assert_eq!(sim.entities().get(tail).unwrap().unit_follower(), None);
    assert!(
        sim.entities().get(tail).unwrap().is_unit_follower(),
        "semantic restore must retain the byte rather than derive incoming links"
    );
}

/// A `[Units]` FOLLOWS cycle behind the ordered head: gamemd's loop at
/// 0x004D3ECD never ends; VERA routes the head alone and the frame goes on.
#[test]
fn a_follower_cycle_does_not_stop_the_frame() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let owner = sim.interner.get("Americans").unwrap();
    let map = map("[Units]\n\
         0=Americans,MTNK,256,8,8,0,Sleep,None,0,-1,0,1,1,1\n\
         1=Americans,MTNK,256,10,8,0,Sleep,None,0,-1,0,2,1,1\n\
         2=Americans,MTNK,256,12,8,0,Sleep,None,0,-1,0,1,1,1\n");
    assert_eq!(sim.spawn_from_map(&map.entities, Some(rules)), 3);
    let head = at_x(&sim, 8);
    let command = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: head,
            target_rx: 20,
            target_ry: 8,
        },
    );
    let mut routed = false;
    for frame in 0..30 {
        let commands = if frame == 0 {
            std::slice::from_ref(&command)
        } else {
            &[]
        };
        sim.advance_app_frame(
            commands,
            rules,
            Some(rules.overlay_registry()),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
        routed |= sim.entities().get(head).unwrap().navigation.nav_com()
            == Some(NavTargetRef::cell(20, 8));
    }
    assert!(routed, "the head still takes its order");
}
