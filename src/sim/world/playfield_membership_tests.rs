//! Foot4DA63B..670 admits a BalloonHover actor before Process, using the
//! retained physical Cell. Cruise does not substitute a PerCell callback.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::{FrameEffects, ObjectAiCtx};

const PLAYFIELD: crate::sim::cell_rect::PlayfieldBounds =
    crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(20, 0, 0, 20, 8);

fn rules(balloon: bool) -> Option<crate::rules::retail_ini_fixture::RetailBattleRules> {
    let patch = if balloon {
        IniFile::from_str("")
    } else {
        IniFile::from_str("[JUMPJET]\nBalloonHover=no\n")
    };
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    )?;
    let object = retail.rules.object("JUMPJET").unwrap();
    assert_eq!(object.balloon_hover, balloon);
    assert!(object.jumpjet);
    assert_eq!(
        object.locomotor,
        crate::rules::locomotor_type::LocomotorKind::Jumpjet
    );
    assert!(!retail.rules.object("MTNK").unwrap().balloon_hover);
    assert!(!retail.rules.object("SHAD").unwrap().balloon_hover);
    Some(retail)
}

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4DA670);
    sim.playfield_bounds = Some(PLAYFIELD);
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(rules);
    for house in sim.houses.values_mut() {
        house.set_country_for_test(Some(house.name()));
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim
}

fn inside(sim: &Simulation, actor: u64) -> bool {
    sim.entity_playfield_membership_mode_one(actor, sim.resolved_terrain.as_ref())
        .unwrap()
}

#[test]
fn foot_ai_promotes_a_stationary_balloon_using_the_retained_physical_cell() {
    for balloon in [true, false] {
        let Some(retail) = rules(balloon) else {
            return;
        };
        for alias in [false, true] {
            let mut sim = arena(&retail.rules);
            let actor = sim
                .spawn_object("JUMPJET", "Americans", 14, 12, 0, &retail.rules)
                .unwrap();
            sim.resolve_type_handles(&retail.rules);
            let entity = sim.substrate.entities.get_mut(actor).unwrap();
            // Supplied entry to the FootAI leaf, separate from the natural
            // reinforcement producer in the integration test below.
            entity.set_in_playfield(false);
            if alias {
                // Raw world cell (526,11) aliases the allocated (14,12)
                // through Map565730's fixed512 index. Map578540 reads the
                // returned Cell coordinate, not this requested coordinate.
                entity.position.set_rx_for_test(526);
                entity.position.set_ry_for_test(11);
            }
            let before = position_world_coord(&entity.position);
            sim.advance_live_object_turn(actor, Some(&retail.rules), ObjectAiCtx::default())
                .unwrap();
            let entity = sim.entities().get(actor).unwrap();
            assert_eq!(position_world_coord(&entity.position), before);
            assert_eq!(
                entity.in_playfield(),
                balloon,
                "FootAI admits only BalloonHover, including a stationary real-cell alias"
            );
            assert!(entity.lifecycle.object_alive());
        }
    }
}

fn reinforcement_scene(
    balloon: bool,
) -> Option<(Simulation, RuleSet, OverlayTypeRegistry, u64, u64)> {
    let retail = rules(balloon)?;
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    // The authored map's trigger/Team/waypoint readers are real; its terrain
    // and playfield are the declared flat arena, not an XMP03T4 map replay.
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=0,0,20,8\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Basic]\nNewINIFormat=4\n\
          [Triggers]\nARRIVE=Americans,<none>,Arrive,0,1,1,1,0\n\
          [Events]\nARRIVE=1,13,0,0\n\
          [Actions]\nARRIVE=1,7,1,RocketeerEntry,0,0,0,0,A\n\
          [Tags]\nARRIVE_TAG=0,Arrive,ARRIVE\n\
          [TeamTypes]\n0=RocketeerEntry\n\
          [RocketeerEntry]\nHouse=Americans\nScript=EntryMove\nTaskForce=EntryForce\n\
          Waypoint=A\nMax=1\nRecruiter=no\nAutocreate=no\n\
          [ScriptTypes]\n0=EntryMove\n\
          [EntryMove]\n0=3,1\n1=24,0\n\
          [TaskForces]\n0=EntryForce\n\
          [EntryForce]\n0=1,JUMPJET\n\
          [Waypoints]\n0=10010\n1=10018\n",
    )
    .unwrap();
    let (_, assets) = crate::rules::retail_ini_fixture::retail_scenario_assets()?;
    let registry = TeamAiIniRegistry::load_retail(&assets, &map.ini, true).unwrap();
    let mut sim = arena(&retail.rules);
    sim.install_team_ai_registry(&registry, &retail.rules)
        .unwrap();
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(&map, &registry, None, None);
    sim.classify_map_triggers(Some(&retail.rules));
    sim.advance_triggers(
        Some(&retail.rules),
        Some(&overlays),
        FrameEffects::default(),
    );
    let actor = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "JUMPJET")
        .expect("Action7 creates and places the actual registered Rocketeer")
        .stable_id();
    let (team, _) = sim.team_script_vm.team_for_member(actor).unwrap();
    assert!(!inside(&sim, actor), "the authored origin is off-playfield");
    let entity = sim.entities().get(actor).unwrap();
    assert!(
        !entity.in_playfield(),
        "Unlimbo establishes false naturally"
    );
    assert!(entity.lifecycle.object_alive());
    assert!(!entity.lifecycle.in_limbo());
    Some((sim, retail.rules, overlays, team, actor))
}

fn tick(sim: &mut Simulation, rules: &RuleSet, overlays: &OverlayTypeRegistry) {
    let commands = sim.take_due_commands();
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&commands, Some(rules), grid.as_deref(), Some(overlays), 33);
}

#[test]
fn authored_reinforcement_crosses_then_foot_ai_admits_before_the_next_process() {
    for balloon in [true, false] {
        let Some((mut sim, rules, overlays, team, actor)) = reinforcement_scene(balloon) else {
            return;
        };
        let mut crossed = false;
        for _ in 0..240 {
            let was_inside = inside(&sim, actor);
            tick(&mut sim, &rules, &overlays);
            if !was_inside && inside(&sim, actor) {
                let entity = sim.entities().get(actor).unwrap();
                let phase = entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .jumpjet_runtime()
                    .unwrap()
                    .phase();
                assert!(
                    matches!(phase, 2 | 3),
                    "Hold/Translate cruise, phase {phase}"
                );
                assert!(
                    !entity.in_playfield(),
                    "a cruising cell change is not a native PerCell membership writer"
                );
                crossed = true;
                break;
            }
        }
        assert!(
            crossed,
            "the real Team script must fly the Rocketeer inside"
        );
        tick(&mut sim, &rules, &overlays);
        assert_eq!(sim.entities().get(actor).unwrap().in_playfield(), balloon);

        for _ in 0..360 {
            if sim.team_script_vm.team(team).is_none() {
                break;
            }
            tick(&mut sim, &rules, &overlays);
        }
        assert!(
            sim.team_script_vm.team(team).is_none(),
            "Move/24/EOF completes"
        );
        assert!(sim.team_script_vm.team_for_member(actor).is_none());
        let entity = sim.entities().get(actor).unwrap();
        assert!(entity.lifecycle.object_alive());
        assert!(!entity.lifecycle.in_limbo());
        assert!(
            entity.in_playfield(),
            "the nonballoon lands through real PerCell"
        );
        sim.occupancy()
            .validate_memberships(sim.entities())
            .unwrap();
    }
}
