//! Paid Moon Patrol AMCV -> concrete wall -> living sink through the bound runtime.
//!
//! Native Cell4865D0, UnitCanEnter73F48F/73FCD3, launch6CC864/6CC91D,
//! PostWarp718AA9/718AC2 and Unit7364A1. Original gamemd SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
//! Instruction/physical-retail expectations; this is a Rust production regression,
//! not original whole-scene execution. All mutable game state comes from the
//! normal Battle launch and commands; the input chooser only reads live owners.

use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::motion_query;
use crate::sim::production::{
    build_options_for_owner, placement_preview_for_owner_with_overlays, power_balance_for_owner,
    queue_view_for_owner, ready_building_entity_for_owner,
};
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

const OWNER: &str = "VERA-OBSERVER";
const MAX_FRAMES: u64 = 48_000;

fn advance(scene: &mut HeadlessScenario, command: Option<Command>) {
    let sim = scene.sim();
    assert!(
        sim.session.tick < MAX_FRAMES,
        "paid Moon history exceeded frame bound"
    );
    let commands: Vec<_> = command
        .into_iter()
        .map(|command| {
            // Keep chosen ordinary inputs replayable by the existing v3
            // capture driver, whose production/Super names are logical IDs.
            let mut payload = serde_json::to_value(&command).unwrap();
            match &command {
                Command::QueueProduction { type_id } => {
                    payload["QueueProduction"]["type_id"] =
                        serde_json::json!(sim.interner.resolve(*type_id));
                }
                Command::PlaceReadyBuilding { type_id, .. } => {
                    payload["PlaceReadyBuilding"]["type_id"] =
                        serde_json::json!(sim.interner.resolve(*type_id));
                }
                Command::LaunchSuperWeapon { sw_type_id, .. } => {
                    payload["LaunchSuperWeapon"]["sw_type_id"] =
                        serde_json::json!(sim.interner.resolve(*sw_type_id));
                }
                _ => {}
            }
            eprintln!("MOON_INPUT tick={} payload={payload}", sim.session.tick);
            CommandEnvelope::new(sim.interner.get(OWNER).unwrap(), sim.session.tick, command)
        })
        .collect();
    scene
        .runtime
        .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
        .expect("ordinary bound-resource frame");
}

fn wait_for(
    scene: &mut HeadlessScenario,
    frames: u64,
    stage: &str,
    ready: impl Fn(&HeadlessScenario) -> bool,
) {
    for _ in 0..frames {
        if ready(scene) {
            return;
        }
        advance(scene, None);
    }
    let sim = scene.sim();
    let owner = sim.interner.get(OWNER).unwrap();
    let options: Vec<_> = build_options_for_owner(sim, &scene.runtime.resources.rules, OWNER)
        .into_iter()
        .filter(|option| stage.starts_with(sim.interner.resolve(option.type_id)))
        .map(|option| {
            (
                sim.interner.resolve(option.type_id).to_owned(),
                option.reason,
            )
        })
        .collect();
    let movers: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.owner() == owner && sim.interner.resolve(entity.type_ref()) == "AMCV"
        })
        .map(|(id, entity)| {
            (
                id,
                position_world_coord(&entity.position),
                motion_query::is_moving(entity),
                entity.mission.current(),
                entity.mission.queued(),
                entity.navigation.nav_com(),
                entity.locomotor.as_ref().map(|loco| loco.active_kind()),
                entity.radio_contacts.slot(0),
                entity.tether.partner(),
            )
        })
        .collect();
    panic!(
        "{stage}: tick={} credits={} spent={} power={:?} queue={:?} options={options:?} movers={movers:?}",
        sim.session.tick,
        sim.houses[&owner].economy.credits(),
        sim.houses[&owner].economy.spent_credits(),
        power_balance_for_owner(sim, &scene.runtime.resources.rules, OWNER),
        queue_view_for_owner(sim, &scene.runtime.resources.rules, OWNER),
    );
}

fn placed_type(sim: &Simulation, owner: InternedId, name: &str) -> Option<u64> {
    sim.entities().iter_sorted().find_map(|(id, entity)| {
        (entity.owner() == owner
            && sim.interner.resolve(entity.type_ref()) == name
            && entity.is_active()
            && !entity.in_limbo()
            && !entity.building_up())
        .then_some(id)
    })
}

/// Ordinary cursor choices, admitted only by the existing production preview.
/// Refineries prefer the actual resource field; this neither changes terrain
/// nor supplies a path. Preview is queried only for a genuinely ready product.
fn placement(scene: &HeadlessScenario, name: &str, anchor: (u16, u16)) -> (u16, u16) {
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let registry = &scene.runtime.resources.overlay_registry;
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let resources: Vec<_> = terrain
        .cells()
        .iter()
        .filter(|cell| {
            sim.overlay_grid.as_ref().is_some_and(|grid| {
                grid.cell(cell.rx, cell.ry)
                    .overlay_id
                    .and_then(|id| registry.flags(id))
                    .is_some_and(|flags| flags.tiberium)
            })
        })
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    let mut choices: Vec<_> = terrain
        .cells()
        .iter()
        .filter(|cell| !cell.outside_playfield)
        .filter(|cell| name != "GAWALL" || cell.final_tile_index == 0)
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    choices.sort_by_key(|&(x, y)| {
        let base_distance = x.abs_diff(anchor.0) + y.abs_diff(anchor.1);
        let resource_distance = if name == "GAREFN" {
            resources
                .iter()
                .map(|&(rx, ry)| x.abs_diff(rx) + y.abs_diff(ry))
                .min()
                .unwrap_or(u16::MAX)
        } else {
            0
        };
        (resource_distance, base_distance, x, y)
    });
    choices
        .into_iter()
        .find(|&(x, y)| {
            placement_preview_for_owner_with_overlays(sim, rules, OWNER, name, x, y, Some(registry))
                .is_some_and(|preview| preview.valid)
        })
        .unwrap_or_else(|| {
            panic!(
                "no ordinary legal {name} placement at tick {}",
                sim.session.tick
            )
        })
}

fn buy_building(scene: &mut HeadlessScenario, name: &str, anchor: (u16, u16)) -> (u16, u16) {
    wait_for(scene, 1200, &format!("{name} becomes buildable"), |scene| {
        build_options_for_owner(scene.sim(), &scene.runtime.resources.rules, OWNER)
            .iter()
            .any(|option| option.enabled && scene.sim().interner.resolve(option.type_id) == name)
    });
    let sim = scene.sim();
    let owner = sim.interner.get(OWNER).unwrap();
    let type_id = sim.interner.get(name).unwrap();
    let paid_before = sim.houses[&owner].economy.spent_credits();
    let price = sim.cost_of(
        owner,
        scene.runtime.resources.rules.object(name).unwrap(),
        &scene.runtime.resources.rules,
    );
    advance(scene, Some(Command::QueueProduction { type_id }));
    wait_for(scene, 6000, &format!("{name} paid completion"), |scene| {
        ready_building_entity_for_owner(scene.sim(), &scene.runtime.resources.rules, owner, type_id)
            .is_some()
    });
    let held = ready_building_entity_for_owner(
        scene.sim(),
        &scene.runtime.resources.rules,
        owner,
        type_id,
    )
    .unwrap();
    assert!(scene.sim().entities().get(held).unwrap().in_limbo());
    assert_eq!(
        scene.sim().houses[&owner].economy.spent_credits() - paid_before,
        price
    );
    let (rx, ry) = placement(scene, name, anchor);
    assert!(
        placement_preview_for_owner_with_overlays(
            scene.sim(),
            &scene.runtime.resources.rules,
            OWNER,
            name,
            rx,
            ry,
            Some(&scene.runtime.resources.overlay_registry),
        )
        .unwrap()
        .valid
    );
    advance(scene, Some(Command::PlaceReadyBuilding { type_id, rx, ry }));
    wait_for(scene, 1200, &format!("{name} placed and opened"), |scene| {
        if name == "GAWALL" {
            scene
                .sim()
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(rx, ry)
                .overlay_id
                .and_then(|id| scene.runtime.resources.overlay_registry.flags(id))
                .is_some_and(|flags| flags.wall)
        } else {
            scene
                .sim()
                .entities()
                .get(held)
                .is_some_and(|entity| !entity.in_limbo() && !entity.building_up())
        }
    });
    (rx, ry)
}

fn isolated_source(sim: &Simulation, mover: u64, at: (u16, u16)) -> bool {
    sim.entities().iter_sorted().all(|(id, entity)| {
        id == mover
            || entity.in_limbo()
            || !entity.is_active()
            || !matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            || entity.position.rx().abs_diff(at.0) > 1
            || entity.position.ry().abs_diff(at.1) > 1
    })
}

#[test]
#[ignore = "slow; paid retail Moon Patrol base, AMCV and natural Chronosphere recharge"]
fn paid_moon_amcv_on_concrete_wall_sinks_alive_then_cleans_up() {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    let selected =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "MoonPatr.yro", &assets)
            .expect("resolve physical stock Moon Patrol");
    assert_eq!(
        serde_json::to_value(selected.source).unwrap()["source_sha256"],
        "580a1d2e9c941189c36ba03ee61ac51c82e7616594762e8c3b77e5e19a9a8580",
        "exact consumed retail YRO wrapper, including loose-file precedence"
    );
    let mut launch = battle_session(
        "MoonPatr.yro",
        &[(LaunchCountry::Russia, AiDifficulty::Easy)],
    );
    launch.local_player_mut().name = OWNER.to_owned();
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.starting_credits = 10_000;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.game_speed = 1;
    launch.options.short_game = false;
    launch.options.bases = true;
    launch.options.super_weapons = true;
    launch.options.crates = false;
    launch.options.shroud = true;
    let mut scene =
        load_session(&root, "MoonPatr.yro", 0x1234_5678, launch).expect("stock Lunar Battle loads");
    assert!(scene.map.header.theater.eq_ignore_ascii_case("LUNAR"));
    // Parsed no is not assumed to ban manual deployment/undeployment. Native
    // 6B8CEC..6B8D00 stores Scenario bit100; its startup consumer is separate.
    assert_eq!(scene.map.special_flags.mcv_deploy, Some(false));
    scene
        .runtime
        .simulation
        .hash_state_every(std::num::NonZeroU64::new(MAX_FRAMES).unwrap());
    let sim = scene.sim();
    let owner = sim.interner.get(OWNER).unwrap();
    assert_eq!(sim.houses[&owner].economy.credits(), 10_000);
    let starting: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| entity.owner() == owner && !entity.in_limbo())
        .collect();
    assert_eq!(starting.len(), 1);
    let initial_mcv = starting[0].0;
    assert_eq!(sim.interner.resolve(starting[0].1.type_ref()), "AMCV");
    let anchor = (starting[0].1.position.rx(), starting[0].1.position.ry());
    advance(
        &mut scene,
        Some(Command::DeployMcv {
            entity_id: initial_mcv,
        }),
    );
    wait_for(
        &mut scene,
        1200,
        "initial paid-input base deployment",
        |scene| placed_type(scene.sim(), owner, "GACNST").is_some(),
    );
    for name in [
        "GAPOWR", "GAPILE", "GAREFN", "GAWEAP", "AMRADR", "GADEPT", "GAPOWR", "GATECH", "GAPOWR",
        "GACSPH",
    ] {
        buy_building(&mut scene, name, anchor);
    }
    let wall = buy_building(&mut scene, "GAWALL", anchor);
    eprintln!("MOON_WALL real clear target={wall:?} initial_mcv={initial_mcv}");

    let type_id = scene.sim().interner.get("AMCV").unwrap();
    assert!(
        build_options_for_owner(scene.sim(), &scene.runtime.resources.rules, OWNER)
            .iter()
            .any(|option| option.type_id == type_id && option.enabled)
    );
    let spent = scene.sim().houses[&owner].economy.spent_credits();
    let price = scene.sim().cost_of(
        owner,
        scene.runtime.resources.rules.object("AMCV").unwrap(),
        &scene.runtime.resources.rules,
    );
    advance(&mut scene, Some(Command::QueueProduction { type_id }));
    wait_for(&mut scene, 6000, "extra paid AMCV delivered", |scene| {
        placed_type(scene.sim(), owner, "AMCV").is_some()
    });
    let mover = placed_type(scene.sim(), owner, "AMCV").unwrap();
    assert_ne!(mover, initial_mcv);
    // First Unlimbo still leaves the product inside its WeaponsFactory.
    // Unit741970 defers ordinary orders while that radio contact remains;
    // let the existing exit track and clearance release it before clicking.
    wait_for(
        &mut scene,
        1200,
        "paid AMCV finishes factory exit",
        |scene| {
            scene.sim().entities().get(mover).is_some_and(|entity| {
                entity.radio_contacts.is_empty() && entity.tether.partner().is_none()
            })
        },
    );
    assert_eq!(
        scene.sim().houses[&owner].economy.spent_credits() - spent,
        price
    );
    let wallet = &scene.sim().houses[&owner].economy;
    assert!(
        i64::from(wallet.credits()) + i64::from(wallet.spent_credits()) > 10_000,
        "normal refinery income funds the opening"
    );
    let rules = &scene.runtime.resources.rules;
    let object = rules.object("AMCV").unwrap();
    assert_eq!(
        scene.sim().entities().get(mover).unwrap().category(),
        EntityCategory::Unit
    );
    assert!(!object.organic && !object.naval && object.speed_type != SpeedType::Hover);
    assert!(object.primary().is_none() && object.secondary().is_none());

    let sim = scene.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let mut sources: Vec<_> = terrain
        .cells()
        .iter()
        .filter(|cell| !cell.outside_playfield && cell.level == 0 && cell.slope_type == 0)
        .map(|cell| (cell.rx, cell.ry))
        .filter(|&(x, y)| x.abs_diff(wall.0) + y.abs_diff(wall.1) >= 4)
        .filter(|&at| sim.cell_objects(at, MovementLayer::Ground).next().is_none())
        .filter(|&at| isolated_source(sim, mover, at))
        .collect();
    let from = sim.entities().get(mover).unwrap();
    let from = (from.position.rx(), from.position.ry());
    sources.sort_by_key(|&(x, y)| (x.abs_diff(from.0) + y.abs_diff(from.1), x, y));
    let source = sources
        .into_iter()
        .find(|&at| {
            at != from
                && sim.foot_can_enter(
                    mover,
                    terrain.native_cell_identity((at.0 as i16, at.1 as i16)),
                    InfantryEntryArgs::REPAIR,
                    rules,
                    Some(&scene.runtime.resources.overlay_registry),
                ) == Ok(0)
        })
        .expect("ordinary empty AMCV source choice");
    eprintln!(
        "MOON_STAGING tick={} mover={mover} from={from:?} source={source:?} moving={:?} mission={:?} queued={:?} nav={:?}",
        sim.session.tick,
        motion_query::is_moving(sim.entities().get(mover).unwrap()),
        sim.entities().get(mover).unwrap().mission.current(),
        sim.entities().get(mover).unwrap().mission.queued(),
        sim.entities().get(mover).unwrap().navigation.nav_com()
    );
    advance(
        &mut scene,
        Some(Command::Move {
            entity_id: mover,
            target_rx: source.0,
            target_ry: source.1,
        }),
    );
    wait_for(&mut scene, 2400, "paid AMCV reaches source", |scene| {
        scene.sim().entities().get(mover).is_some_and(|entity| {
            (entity.position.rx(), entity.position.ry()) == source
                && motion_query::is_moving(entity) == Some(false)
        })
    });
    let sphere = scene.sim().interner.get("ChronoSphereSpecial").unwrap();
    let warp = scene.sim().interner.get("ChronoWarpSpecial").unwrap();
    wait_for(
        &mut scene,
        7000,
        "natural powered Chronosphere recharge",
        |scene| {
            scene.sim().super_weapons[&owner][&sphere].is_active
                && scene.sim().super_weapons[&owner][&sphere].is_ready
                && !scene.sim().super_weapons[&owner][&sphere].is_suspended
        },
    );
    let sim = scene.sim();
    assert!(isolated_source(sim, mover, source));
    let (output, drain) = power_balance_for_owner(sim, &scene.runtime.resources.rules, OWNER);
    assert!(output >= drain && output > 0);
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let query = NativeCellQuery::canonical(terrain);
    let target_cell = query.lookup((wall.0 as i16, wall.1 as i16));
    assert_eq!(terrain.cell(wall.0, wall.1).unwrap().final_tile_index, 0);
    // Exclude the earlier LandType::Water sink branch718968: the selected
    // regression must reach CanEnter7 -> Cell4865D0 -> living sink718AC2.
    assert_ne!(query.land_type(target_cell), 2);
    assert_eq!(
        query.flags(target_cell) & 0x100,
        0,
        "no native Cell bridge flag"
    );
    assert!(
        sim.cell_objects(wall, MovementLayer::Ground)
            .next()
            .is_none(),
        "paid wall is overlay only; no Building/Terrain retarget blocker"
    );
    let overlay = sim.overlay_grid.as_ref().unwrap().cell(wall.0, wall.1);
    assert_eq!(overlay.wall_owner, Some(owner));
    let flags = scene
        .runtime
        .resources
        .overlay_registry
        .flags(overlay.overlay_id.unwrap())
        .unwrap();
    assert!(flags.wall && !flags.crushable);
    assert_eq!(
        sim.foot_can_enter(
            mover,
            target_cell,
            InfantryEntryArgs::REPAIR,
            &scene.runtime.resources.rules,
            Some(&scene.runtime.resources.overlay_registry)
        ),
        Ok(7)
    );
    let health = sim.entities().get(mover).unwrap().health.current();
    let losses = sim.houses[&owner].stats.units_lost();
    advance(
        &mut scene,
        Some(Command::LaunchSuperWeapon {
            sw_type_id: sphere,
            target_rx: source.0,
            target_ry: source.1,
        }),
    );
    assert_eq!(
        scene.sim().super_weapons[&owner][&sphere].chrono_cell(),
        source
    );
    assert_eq!(scene.sim().super_placement_anims().len(), 1);
    advance(
        &mut scene,
        Some(Command::LaunchSuperWeapon {
            sw_type_id: warp,
            target_rx: wall.0,
            target_ry: wall.1,
        }),
    );
    let sim = scene.sim();
    assert!(sim.super_placement_anims().is_empty());
    assert!(!sim.super_weapons[&owner][&sphere].is_ready);
    assert_eq!(sim.super_weapons[&owner][&sphere].charge_duration, 6300);
    let entity = sim.entities().get(mover).unwrap();
    assert!(entity.chrono_warp_latch());
    let locomotor = entity.locomotor.as_ref().unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Teleport);
    assert_eq!(
        locomotor
            .piggyback()
            .as_ref()
            .map(|stash| stash.active_kind()),
        Some(LocomotorKind::Drive)
    );
    wait_for(
        &mut scene,
        600,
        "arrival enters living sinking rather than C4",
        |scene| {
            let entity = scene
                .sim()
                .entities()
                .get(mover)
                .expect("C4 removed AMCV before living sink");
            assert!(
                entity.health.current() > 0 && entity.is_object_alive(),
                "AMCV took C4 instead of living sink"
            );
            entity.sinking.is_active()
        },
    );
    let sim = scene.sim();
    let entity = sim.entities().get(mover).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), wall);
    assert_eq!(entity.health.current(), health);
    assert!(entity.is_object_alive() && entity.lifecycle.cell_marked());
    assert!(entity.navigation.nav_com().is_none());
    assert_eq!(
        sim.houses[&owner].stats.units_lost(),
        losses,
        "718AC2 branch does not book an immediate kill"
    );
    let sink_z = position_world_coord(&entity.position).z;
    eprintln!(
        "MOON_SINK tick={} mover={mover} source={source:?} wall={wall:?} health={health} raw_z={sink_z} losses={losses}",
        sim.session.tick
    );
    advance(&mut scene, None);
    assert!(position_world_coord(&scene.sim().entities().get(mover).unwrap().position).z < sink_z);
    wait_for(
        &mut scene,
        200,
        "ordinary sinking terminal cleanup",
        |scene| !scene.sim().entities().contains(mover),
    );
    let sim = scene.sim();
    assert_eq!(sim.houses[&owner].stats.units_lost(), losses + 1);
    assert!(!sim.occupancy().contains_entity(wall.0, wall.1, mover));
    assert!(!sim.occupancy().contains_entity(source.0, source.1, mover));
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(wall.0, wall.1)
            .wall_owner,
        Some(owner)
    );
    assert!(placed_type(sim, owner, "GACNST").is_some());
}
