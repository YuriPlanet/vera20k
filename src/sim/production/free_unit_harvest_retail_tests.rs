//! Legal Scenario FreeUnit override through paid retail construction.
//! Native: GrandOpening445F80 success446EA7..446EB1 queues Harvest and
//! Commences; non-harvester UnitMissionHarvest73E5E0 returns450 at73E62E.
//! This is a Rust production history with semantic command delivery, not
//! original execution, unmodified stock gameplay or whole-frame/RNG parity.

use super::{
    FactoryHolder, ProductionCategory, placement_preview_for_owner_with_overlays,
    ready_building_entity_for_owner,
};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::map::entities::EntityCategory;
use crate::map::source::test_support::TestDirectory;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{CellActionQuery, FrameEffects, MoveAction, TickLane};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, SkirmishLaunchOptions,
};

const OWNER: &str = "VERA-OBSERVER";

fn frame(scene: &mut HeadlessScenario, owner: InternedId, command: Option<Command>) {
    let commands: Vec<_> = command
        .into_iter()
        .map(|command| CommandEnvelope::new(owner, scene.sim().session.tick(), command))
        .collect();
    let output = scene
        .runtime
        .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
        .expect("ordinary bound-resource frame");
    assert!(output.tick.frame_committed);
}

#[test]
#[ignore = "slow; paid retail refinery opening and one 450-frame dispatch interval"]
fn nonharvester_free_unit_commits_harvest_through_paid_refinery_completion() {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    let assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let stock_map = assets.resolve_ref("XMP03T4.MAP").expect("retail Hills map");
    assert!(
        IniFile::from_bytes(stock_map.bytes)
            .unwrap()
            .section("NAREFN")
            .is_none()
    );
    // One legal map input. Preserve the physical terrain, overlay, waypoint and
    // existing Scenario bytes; no runtime rules/state or extension directives.
    let directory = TestDirectory::new("free-unit-harvest");
    let mut map_bytes = stock_map.bytes.to_vec();
    map_bytes.extend_from_slice(b"\r\n[NAREFN]\r\nFreeUnit=MTNK\r\n");
    directory.write("FreeUnitHarvest.map", &map_bytes);
    drop(assets);
    let map_path = directory.path().join("FreeUnitHarvest.map");
    let map_name = map_path.to_str().unwrap();
    let mut launch = battle_session(map_name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_player_mut().name = OWNER.to_owned();
    launch.local_slot_mut().country = LaunchCountry::Russia;
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options = SkirmishLaunchOptions {
        starting_credits: 10_000,
        unit_count: 0,
        tech_level: 10,
        game_speed: 1,
        default_ai_difficulty: 0,
        short_game: false,
        bases: true,
        bridges_destroyable: true,
        super_weapons: true,
        build_off_ally: true,
        crates: false,
        mcv_redeploy: true,
        fog_of_war: false,
        shroud: true,
        tiberium_grows: true,
        multi_engineer: false,
        harvester_truce: false,
        ally_change_allowed: true,
    };
    let mut scene = load_session(&root, map_name, 0x1234_5678, launch).unwrap();
    // Observation cadence only; every gameplay frame still uses the real runtime.
    scene
        .runtime
        .simulation
        .hash_state_every(std::num::NonZeroU64::new(4096).unwrap());
    let owner = scene.sim().interner.get(OWNER).unwrap();
    let refinery_type = scene.sim().interner.get("NAREFN").unwrap();
    let tank_type = scene.sim().interner.get("MTNK").unwrap();
    let local: Vec<_> = scene
        .sim()
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| entity.owner() == owner)
        .collect();
    assert_eq!(local.len(), 1);
    let mcv = local[0].0;
    assert_eq!(scene.sim().interner.resolve(local[0].1.type_ref()), "SMCV");
    let rules = &scene.runtime.resources.rules;
    assert_eq!(rules.building_free_unit("NAREFN"), Some("MTNK"));
    assert!(!rules.object("MTNK").unwrap().harvester);
    let [_, buildup_frames, buildup_rate] = rules.buildup_control("NAREFN");
    assert!(rules.has_buildup("NAREFN") && buildup_frames > 1 && buildup_rate > 0);

    let mut charged = false;
    let mut refinery_spent_before = None;
    let mut refinery = None;
    let mut saw_buildup = false;
    let mut tank = None;
    for step in 0..3300 {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        if step == 1210 {
            refinery_spent_before = Some(sim.houses[&owner].economy().spent_credits());
        }
        let order = match step {
            // Existing industrial_plant_retail_tests opening choices, ending
            // at the first refinery. These are inputs, not native timestamps.
            70 => Some(("NAPOWR", None)),
            650 => Some(("NAPOWR", Some((31, 89)))),
            720 => Some(("NAHAND", None)),
            1200 => Some(("NAHAND", Some((31, 85)))),
            1210 => Some(("NAREFN", None)),
            2900 => Some(("NAREFN", Some((26, 83)))),
            _ => None,
        };
        let command = if step == 0 {
            Some(Command::DeployMcv { entity_id: mcv })
        } else {
            order.map(|(kind, cell)| {
                let type_id = sim.interner.get(kind).unwrap();
                match cell {
                    None => Command::QueueProduction { type_id },
                    Some((rx, ry)) => {
                        let held = ready_building_entity_for_owner(sim, rules, owner, type_id)
                            .expect("ordinary paid Factory has a completed held product");
                        assert!(sim.entities().get(held).unwrap().lifecycle.in_limbo());
                        assert!(
                            placement_preview_for_owner_with_overlays(
                                sim,
                                rules,
                                OWNER,
                                kind,
                                rx,
                                ry,
                                Some(&scene.runtime.resources.overlay_registry),
                            )
                            .unwrap()
                            .valid
                        );
                        if kind == "NAREFN" {
                            refinery = Some(held);
                        }
                        Command::PlaceReadyBuilding { type_id, rx, ry }
                    }
                }
            })
        };
        if let Some(factory) = sim
            .production
            .factories()
            .factory(FactoryHolder::House(owner, ProductionCategory::Building))
            && factory
                .object()
                .is_some_and(|object| object.type_id == refinery_type)
        {
            charged |= factory.progress() > 0
                && factory.balance() > 0
                && refinery_spent_before
                    .is_some_and(|before| sim.houses[&owner].economy().spent_credits() > before);
        }
        frame(&mut scene, owner, command);
        let sim = scene.sim();
        if let Some(id) = refinery {
            saw_buildup |= sim.entities().get(id).unwrap().building_up();
        }
        let born: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter_map(|(id, entity)| {
                (entity.owner() == owner && entity.type_ref() == tank_type).then_some(id)
            })
            .collect();
        if !born.is_empty() {
            assert_eq!(born.len(), 1);
            tank = Some(born[0]);
            break;
        }
    }
    assert!(
        charged && saw_buildup,
        "real charging and finite buildup precede birth"
    );
    let tank = tank.expect("actual refinery completion created its single MTNK");
    let building = scene.sim().entities().get(refinery.unwrap()).unwrap();
    assert!(building.building_art.actually_placed() && !building.building_up());
    let actor = scene.sim().entities().get(tank).unwrap();
    assert_eq!(actor.category(), EntityCategory::Unit);
    assert!(actor.is_alive() && !actor.lifecycle.in_limbo() && actor.miner.is_none());
    assert!(actor.lifecycle.cell_marked() && scene.sim().logic_order().contains(&tank));
    assert_eq!(actor.mission.current().known(), Some(MissionType::Harvest));
    assert_eq!(actor.mission.queued(), MissionId::NONE);
    assert_eq!(actor.mission.handler_state(), 0);

    // Newly appended Logic entries may already run in the birth frame. Observe
    // the actual timer rather than assuming that first visit is next frame.
    for _ in 0..2 {
        frame(&mut scene, owner, None);
    }
    let actor = scene.sim().entities().get(tank).unwrap();
    assert!(actor.mission.ai_counter() > 0);
    let first_dispatch = actor.mission.dispatch_timer();
    assert_eq!(first_dispatch.delay(), 450);
    let counter = actor.mission.ai_counter();
    let mut redispatched = false;
    for _ in 0..450 {
        frame(&mut scene, owner, None);
        let actor = scene.sim().entities().get(tank).unwrap();
        assert_eq!(actor.mission.current().known(), Some(MissionType::Harvest));
        assert_eq!(actor.mission.queued(), MissionId::NONE);
        assert_eq!(actor.mission.handler_state(), 0);
        assert!(actor.attack_target.target().is_none() && actor.navigation.nav_com().is_none());
        let timer = actor.mission.dispatch_timer();
        assert_eq!(timer.delay(), 450);
        if timer.start_frame() != first_dispatch.start_frame() {
            assert_eq!(timer.start_frame(), first_dispatch.start_frame() + 450);
            assert!(actor.mission.ai_counter() > counter);
            redispatched = true;
            break;
        }
    }
    assert!(
        redispatched,
        "ordinary Logic reaches the next non-miner Harvest dispatch"
    );

    let sim = scene.sim();
    let actor = sim.entities().get(tank).unwrap();
    let start = (actor.position.rx(), actor.position.ry());
    let rules = &scene.runtime.resources.rules;
    let registry = Some(&scene.runtime.resources.overlay_registry);
    let destination = [(-1, 0), (0, -1), (1, 0), (0, 1)]
        .into_iter()
        .map(|(dx, dy)| {
            (
                start.0.checked_add_signed(dx).unwrap(),
                start.1.checked_add_signed(dy).unwrap(),
            )
        })
        .find(|&cell| {
            sim.cell_move_action(
                owner,
                tank,
                cell,
                CellActionQuery {
                    mouse_over: false,
                    shift: false,
                },
                rules,
                registry,
            )
            .unwrap()
                == MoveAction::Move
        })
        .expect("the real revealed map has an admitted neighbouring Move");
    let (target_rx, target_ry) = sim
        .cell_click_destination(owner, tank, destination, false, rules, registry)
        .unwrap()
        .unwrap();
    assert_ne!((target_rx, target_ry), start);
    frame(
        &mut scene,
        owner,
        Some(Command::Move {
            entity_id: tank,
            target_rx,
            target_ry,
        }),
    );
    let mut moved = false;
    for _ in 0..180 {
        frame(&mut scene, owner, None);
        let actor = scene.sim().entities().get(tank).unwrap();
        if (actor.position.rx(), actor.position.ry()) != start {
            moved = true;
            break;
        }
    }
    assert!(
        moved,
        "normal owned Move still drives the completion-created tank"
    );
}
