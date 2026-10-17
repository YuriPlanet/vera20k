//! Terrain/navigation evidence and snapshot restoration shared by physical bridge witnesses.
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::rules::terrain_rules::LandType;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::Simulation;
use serde_json::{Value, json};

pub(super) fn navigation_snapshot(
    sim: &Simulation,
    name: &str,
    points: impl IntoIterator<Item = (u16, u16)>,
) -> Value {
    let zones = sim.zone_grid.as_ref().unwrap();
    let mut copied = zones.clone();
    let base = copied.base_topology_mut();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let bridges = sim.bridge_state.as_ref().unwrap();
    let hierarchy = zones
        .hierarchy_for(crate::rules::locomotor_type::MovementZone::Normal)
        .unwrap();
    let side = usize::from(zones.width) + 1;
    let graphs: Vec<_> = (0..3)
        .map(|level| {
            let graph = hierarchy.level(level).unwrap();
            let records: Vec<_> =
                (0..graph.record_slot_count())
                    .map(|index| {
                        graph.record(index as u16).map_or(Value::Null, |record| {
                let edges: Vec<_> = graph.edges(index as u16).iter()
                    .map(|edge| [u32::from(edge.neighbor), u32::from(edge.flag)]).collect();
                json!({"parent":record.parent,"zone_type":record.zone_type,"edges":edges})
            })
                    })
                    .collect();
            let padding: Vec<_> = (0..side * side)
                .filter(|index| {
                    index % side >= usize::from(zones.width)
                        || index / side >= usize::from(zones.height)
                })
                .map(|index| graph.native_padding_zone(index))
                .collect();
            json!({"ids":graph.cell_zone_ids(),"padding_ids":padding,"records":records})
        })
        .collect();
    let records: Vec<_> = zones
        .bridge_records()
        .iter()
        .map(|r| {
            json!({
                "a": r.endpoint_a, "b": r.endpoint_b, "active": r.active,
                "kind": if r.is_high() { 0 } else { 1 },
            })
        })
        .collect();
    let mut points: std::collections::BTreeSet<_> = points.into_iter().collect();
    for r in zones.bridge_records() {
        points.extend([r.endpoint_a, r.endpoint_b]);
    }
    let cells: Vec<_> = points.into_iter().filter_map(|(x,y)| {
        let c = terrain.cell(x,y)?;
        let index = usize::from(y) * usize::from(zones.width) + usize::from(x);
        let cluster = *base.zone_ids.get(index)?;
        Some(json!({
            "coord": [x,y], "level": c.level, "slope": c.slope_type,
            "zone_type": c.zone_type, "land": c.yr_cell_land_type,
            "tile": c.final_tile_index, "subtile": c.final_sub_tile,
            "bridge_flags": c.bridge_facts.raw_flags,
            "overlay": c.bridge_facts.overlay_id,
            "cached_class": base.movement_classes[index], "cached_height": base.levels[index],
            "base_id": cluster, "raw_row7": base.raw_zone_ids_by_row[7].get(usize::from(cluster)),
        }))
    }).collect();
    json!({
        "name": name,
        "origin": {"boundary":name,"tick":sim.session.tick,"binary_frame":sim.session.binary_frame},
        "width": zones.width, "height": zones.height,
        "native_size": base.native_bridge_source_size,
        "classes": base.movement_classes, "levels": base.levels, "records": records,
        "graphs": graphs,
        "live_cell_levels": terrain.cells().iter().map(|cell| cell.level).collect::<Vec<_>>(),
        "live_cell_slopes": terrain.cells().iter().map(|cell| cell.slope_type).collect::<Vec<_>>(),
        "live_cell_allocated": terrain.cells().iter().map(|cell| {
            terrain.native_fixed_cell_index(cell.rx as i16, cell.ry as i16).is_some()
        }).collect::<Vec<_>>(),
        "records_match_bridge_authority": zones.bridge_records() == bridges.endpoint_records(),
        "bridge_authority_records": bridges.endpoint_records(),
        "rust": {"base_ids":base.zone_ids,"raw_rows":base.raw_zone_ids_by_row,
            "zone_count":base.raw_zone_ids_by_row[0].len().saturating_sub(1)},
        "cells": cells,
        "dummy": format!("{:?}", terrain.shared_cell_dummy().snapshot()),
    })
}

pub(super) fn restored_retail(
    scenario: &crate::headless_scenario::HeadlessScenario,
    pristine: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    bytes: &[u8],
) -> Simulation {
    use crate::sim::snapshot::GameSnapshot;
    let live = scenario.sim();
    let resources = &scenario.runtime.resources;
    let mut restored = GameSnapshot::load_validated(
        bytes,
        scenario.map.ini.content_hash(),
        resources.rules.simulation_config_hash(),
        &live.session.map_name,
    )
    .unwrap()
    .sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(live));
    restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    restored.reconstruct_cellclass_dummy_for_map_resize();
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        pristine.clone(),
        live.terrain_speed_config.clone(),
        &resources.rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(
            &resources.rules,
            &resources.overlay_registry,
            None,
        )
        .unwrap();
    restored.resolve_type_handles(&resources.rules);
    restored.rebuild_lighting_sources_after_load(&resources.rules);
    restored
}

pub(super) fn save_scene(
    scene: &crate::headless_scenario::HeadlessScenario,
    name: &str,
) -> Vec<u8> {
    crate::sim::snapshot::GameSnapshot::save_validated(
        scene.sim(),
        scene.map.ini.content_hash(),
        scene.runtime.resources.rules.simulation_config_hash(),
        name,
        0,
    )
}

/// Normal starting MTNK movement and AP fire, with frame publications exposed
/// to the ordinary Cell overlay/GPU consumer. Native mission/AreaDamage body
/// authority: anytown_damage/mission.{py,json.gz}, projected mission_test_vectors.
/// Those isolated native frames/RNG are not normal-opening expectations.
pub fn visit_starting_mtnk_concrete_attack(
    mut visit: impl FnMut(
        Option<&str>,
        &mut HeadlessScenario,
        Option<&crate::sim::world::SimFrameOutput>,
    ),
) {
    use crate::sim::world::{FrameEffects, TickLane};
    let profile =
        crate::test_fixture::json("tools/map_observation.low-concrete-bridge.example.json");
    let native =
        crate::test_fixture::json("tools/spatial_oracle/anytown_damage/mission_test_vectors.json");
    let native = &native["seed0"];
    let stages = [
        native["impacts"][0]["overlay_before"].as_u64().unwrap() as u8,
        native["impacts"][1]["overlay_after"].as_u64().unwrap() as u8,
        native["impacts"][2]["overlay_after"].as_u64().unwrap() as u8,
    ];
    let retail =
        std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map = launch.selected_map_file.as_deref().unwrap().to_owned();
    let (_, assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("retail scenario inputs");
    let selected =
        crate::map::source::load_map_by_name_or_path_with_assets(&retail, &map, &assets).unwrap();
    assert_eq!(
        serde_json::to_value(selected.source).unwrap()["source_sha256"],
        "7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e"
    );
    let mut scene = crate::headless_scenario::load_session(
        &retail,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    let owner = scene.sim().session.current_house.unwrap();
    let orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|order| {
            assert_eq!(order["owner"], "VERA-OBSERVER");
            CommandEnvelope::new(
                owner,
                order["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(order["payload"].clone()).unwrap(),
            )
        })
        .collect();
    let (source, target) = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => Some((attacker_id, (target_rx, target_ry))),
            _ => None,
        })
        .unwrap();
    let actor = scene
        .sim()
        .entities()
        .get(source)
        .expect("normal starting MTNK");
    assert_eq!(scene.sim().resolve(actor.type_ref()), "MTNK");
    assert_eq!(actor.owner(), owner);
    assert_eq!((actor.position.rx(), actor.position.ry()), (88, 35));
    let weapon = scene.runtime.resources.rules.weapon("105mm").unwrap();
    assert_eq!(
        weapon.damage as u64,
        native["shots"][0]["damage"].as_u64().unwrap()
    );
    assert_eq!(weapon.warhead.as_deref(), Some("AP"));
    let center = |scene: &HeadlessScenario| {
        scene
            .sim()
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(target.0, target.1)
            .overlay_id
    };
    assert_eq!(center(&scene), Some(stages[0]));
    visit(Some("loaded"), &mut scene, None);
    let mut phase = 0;
    let mut collapsed_at = None;
    let mut guarding = false;
    let mut shots = 0;
    let mut dispatched = 0;
    for step in 1..=profile["ticks"].as_u64().unwrap() {
        let due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step - 1)
            .cloned()
            .collect();
        if due
            .iter()
            .any(|order| matches!(order.payload, Command::ForceAttackCell { .. }))
        {
            let actor = scene.sim().entities().get(source).unwrap();
            assert_eq!(
                (actor.position.rx(), actor.position.ry()),
                (87, 50),
                "ordinary Move reaches the bank before firing"
            );
        }
        dispatched += due.len();
        let output = scene
            .runtime
            .advance_frame(&due, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        let fired = output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == source)
            .count();
        if collapsed_at.is_some() {
            assert_eq!(fired, 0, "detached bridge target cannot keep firing");
        }
        shots += fired;
        let next = center(&scene);
        let changed = next != Some(stages[phase]);
        let mut label = None;
        if changed {
            phase += 1;
            assert!(
                phase < stages.len(),
                "no extra center transition after empty-art collapse"
            );
            assert_eq!(next, Some(stages[phase]));
            for (x, data) in [(86, 0), (87, 1), (88, 2)] {
                let live = scene.sim().overlay_grid.as_ref().unwrap().cell(x, 54);
                assert_eq!(
                    (live.overlay_id, live.overlay_data),
                    (Some(stages[phase]), data)
                );
                // Ordinary bridge publication mirrors identity synchronously
                // into Terrain and OverlayGrid; it deliberately bypasses the
                // generic placement dirty/upsert channel. The retained index
                // remains an anchor whose bridge identity resolves live.
                let terrain = scene
                    .sim()
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(x, 54)
                    .unwrap();
                assert_eq!(
                    (
                        terrain.bridge_facts.overlay_id,
                        terrain.bridge_facts.state_byte
                    ),
                    (Some(stages[phase]), data)
                );
                assert!(scene.sim().radar_terrain_dirty_cells.contains(&(x, 54)));
                assert!(
                    !output.overlay_removals.contains(&(x, 54)),
                    "collapsed232 is retained empty art"
                );
            }
            if phase == 1 {
                assert!(
                    scene
                        .sim()
                        .entities()
                        .get(source)
                        .unwrap()
                        .attack_target
                        .target()
                        .is_some(),
                    "first damage leaves a live bridge target"
                );
                label = Some("damaged");
            } else {
                assert!(shots > 0, "real AP fire reaches the bridge controller");
                assert!(
                    scene
                        .sim()
                        .entities()
                        .get(source)
                        .unwrap()
                        .attack_target
                        .target()
                        .is_none(),
                    "70D4A0 clears the Cell target synchronously with collapse"
                );
                collapsed_at = Some(step);
                label = Some("collapsed");
            }
        }
        if let Some(collapsed_at) = collapsed_at {
            let actor = scene.sim().entities().get(source).unwrap();
            assert!(actor.is_alive());
            assert!(actor.attack_target.target().is_none());
            guarding |=
                actor.mission.current().known() == Some(crate::sim::mission::MissionType::Guard);
            if step >= collapsed_at + 32 {
                assert!(guarding, "Attack null-target handling reaches Guard");
                visit(Some("settled"), &mut scene, Some(&output));
                assert_eq!(dispatched, orders.len());
                return;
            }
        }
        visit(label, &mut scene, Some(&output));
    }
    panic!(
        "normal starting MTNK must reach damage, collapse and settled Guard within the profile bound"
    );
}

pub(super) fn load_anytown_concrete() -> HeadlessScenario {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let map = std::env::var("VERA20K_ANYTOWN_MAP").unwrap_or_else(|_| "XMP03T4.MAP".to_owned());
    crate::headless_scenario::load(&retail, &map, 0).unwrap()
}

pub(super) fn load_shrapnel() -> HeadlessScenario {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let map = std::env::var("VERA20K_SHRAPNEL_MAP").unwrap_or_else(|_| "XShrapnel.MAP".to_owned());
    crate::headless_scenario::load(&retail, &map, 0x0B21_D6E5).unwrap()
}

pub(super) fn damage_anytown_concrete(scene: &mut HeadlessScenario) -> bool {
    damage_bridge(scene, (87, 54), 416, 1501, "AP")
}

pub(super) fn damage_shrapnel_wood(scene: &mut HeadlessScenario) -> bool {
    damage_bridge(scene, (115, 59), 208, 1501, "AP")
}

fn damage_bridge(
    scene: &mut HeadlessScenario,
    point: (u16, u16),
    z: i32,
    damage: i32,
    warhead: &str,
) -> bool {
    let runtime = &mut scene.runtime;
    let event = crate::sim::bridge_state::BridgeDamageEvent {
        rx: point.0,
        ry: point.1,
        damage,
        warhead_ref: runtime.simulation.intern(warhead),
        impact_z_leptons: z,
        is_ion_cannon: false,
    };
    super::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
        &mut runtime.simulation,
        &runtime.resources.rules,
        &[event],
        Some(&runtime.resources.overlay_registry),
        crate::sim::world::FrameEffects::default(),
    )
}

#[derive(Debug, Clone, Copy)]
pub enum RetailHighBridgeScene {
    HillsWood,
    PacificConcrete,
}

/// Real high-bridge receiver publication and ordinary Engineer repair.
/// Hills uses physical BRIDGEB2; Pacific uses physical BRIDGE1. The Hills
/// receiver/approach continue the existing techno_ai bridge witnesses. Pacific
/// uses its actual CABHUT and an empty level-six bank; production spawn/path
/// admission and hut entry must succeed. No bridge facts are assigned here.
/// Native body publication: tools/spatial_oracle/bridge_body_publication.
/// Damage enters at the receiver; an ordinary projectile launch is not claimed.
pub fn visit_retail_high_bridge_stages(
    kind: RetailHighBridgeScene,
    mut visit: impl FnMut(&str, &HeadlessScenario),
) {
    let (map, target, expected_anchor, impact_z, overlay, hut, start) = match kind {
        RetailHighBridgeScene::HillsWood => (
            "Hills.mmx",
            (64, 69),
            (65, 69),
            1040,
            238,
            (68, 74),
            (66, 76),
        ),
        RetailHighBridgeScene::PacificConcrete => (
            "Pacific.mmx",
            (81, 95),
            (81, 95),
            624,
            24,
            (72, 93),
            (70, 95),
        ),
    };
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let mut scene = crate::headless_scenario::load(&retail, map, 0x0B21_D6E5).unwrap();
    let facts = |scene: &HeadlessScenario, point: (u16, u16)| {
        scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(point.0, point.1)
            .unwrap()
            .bridge_facts
    };
    let loaded_target = facts(&scene, target);
    assert!(loaded_target.has_structural_bridge());
    let anchor = loaded_target.anchor.expect("loaded bridge anchor").anchor;
    assert_eq!(
        anchor, expected_anchor,
        "{kind:?}: physical anchor relation"
    );
    let loaded_anchor = facts(&scene, anchor);
    assert_eq!(loaded_anchor.overlay_id, Some(overlay), "{kind:?}");
    assert!(matches!(loaded_anchor.state_byte, 0..=5 | 9..=14));
    visit("loaded", &scene);

    // Keep the real RNG/admission/publication callbacks. Observe transitions
    // after each event instead of assuming a fixed number of hits per stage.
    let damage = scene.runtime.resources.rules.bridge_rules.strength + 1;
    let mut saw_damage = false;
    let mut published_collapse = false;
    for _ in 0..8 {
        published_collapse |= damage_bridge(&mut scene, target, impact_z, damage, "HE");
        if !facts(&scene, target).has_structural_bridge() {
            break;
        }
        let damaged_anchor = facts(&scene, anchor);
        if !saw_damage && matches!(damaged_anchor.state_byte, 6 | 15) {
            assert_ne!(
                damaged_anchor, loaded_anchor,
                "receiver must publish damage"
            );
            visit("damaged", &scene);
            saw_damage = true;
        }
    }
    assert!(
        saw_damage,
        "{kind:?}: no published damaged state before collapse"
    );
    assert!(
        published_collapse,
        "{kind:?}: receiver must publish collapse"
    );
    assert!(!facts(&scene, target).has_structural_bridge());
    visit("collapsed", &scene);

    repair_ordinary_bridge(&mut scene, hut, start, &mut ordinary_repair_orders);
    assert!(facts(&scene, target).has_structural_bridge());
    assert_eq!(facts(&scene, anchor).overlay_id, Some(overlay), "{kind:?}");
    visit("repaired", &scene);
}

/// The physical map's one concrete chain, shared by simulation and GPU witnesses.
/// Damage is admitted through the area-damage owner; rebuilding uses the ordinary
/// Engineer command and object turns, including its entry and consumption.
pub fn visit_anytown_concrete_stages(mut visit: impl FnMut(&str, &HeadlessScenario)) {
    visit_anytown_concrete_stages_with_repair_orders(
        |phase, scene| visit(phase, scene),
        ordinary_repair_orders,
    );
}

/// The same physical damage/repair witness with an app-owned input producer.
/// This callback seam replaces only the previously supplied Capture envelope;
/// it does not replace Engineer construction, the repair loop or its resources.
/// Mutable phase observation permits an app test to construct a fresh ordinary
/// actor for a healthy no-order probe after the repaired actor was consumed.
pub fn visit_anytown_concrete_stages_with_repair_orders(
    mut visit: impl FnMut(&str, &mut HeadlessScenario),
    mut producer: impl FnMut(
        &Simulation,
        &crate::rules::ruleset::RuleSet,
        u64,
        u64,
    ) -> Vec<CommandEnvelope>,
) {
    let mut scene = load_anytown_concrete();
    visit("loaded", &mut scene);
    assert!(!damage_anytown_concrete(&mut scene));
    visit("damaged", &mut scene);
    assert!(damage_anytown_concrete(&mut scene));
    visit("collapsed", &mut scene);
    repair_anytown_concrete_with_orders(&mut scene, &mut producer);
    visit("repaired", &mut scene);
}

pub(super) fn repair_anytown_concrete(scene: &mut HeadlessScenario) {
    repair_anytown_concrete_with_orders(scene, &mut ordinary_repair_orders);
}

fn repair_anytown_concrete_with_orders(
    scene: &mut HeadlessScenario,
    producer: &mut impl FnMut(
        &Simulation,
        &crate::rules::ruleset::RuleSet,
        u64,
        u64,
    ) -> Vec<CommandEnvelope>,
) {
    repair_ordinary_bridge(scene, (89, 51), (89, 50), producer);
    assert_repaired_ground_row(scene, 86..=88, 54, 214..=217);
}

pub(super) fn repair_shrapnel_wood(scene: &mut HeadlessScenario) {
    repair_shrapnel_wood_with_orders(scene, &mut ordinary_repair_orders);
}

fn repair_shrapnel_wood_with_orders(
    scene: &mut HeadlessScenario,
    producer: &mut impl FnMut(
        &Simulation,
        &crate::rules::ruleset::RuleSet,
        u64,
        u64,
    ) -> Vec<CommandEnvelope>,
) {
    repair_ordinary_bridge(scene, (117, 56), (117, 55), producer);
    assert_repaired_ground_row(scene, 114..=116, 59, 83..=86);
}

pub fn visit_shrapnel_wood_stages(mut visit: impl FnMut(&str, &HeadlessScenario)) {
    visit_shrapnel_wood_stages_with_repair_orders(
        |phase, scene| visit(phase, scene),
        ordinary_repair_orders,
    );
}

pub fn visit_shrapnel_wood_stages_with_repair_orders(
    mut visit: impl FnMut(&str, &mut HeadlessScenario),
    mut producer: impl FnMut(
        &Simulation,
        &crate::rules::ruleset::RuleSet,
        u64,
        u64,
    ) -> Vec<CommandEnvelope>,
) {
    let mut scene = load_shrapnel();
    visit("loaded", &mut scene);
    repair_shrapnel_wood_with_orders(&mut scene, &mut producer);
    visit("healthy", &mut scene);
    assert!(!damage_shrapnel_wood(&mut scene));
    visit("damaged", &mut scene);
    assert!(damage_shrapnel_wood(&mut scene));
    visit("collapsed", &mut scene);
    repair_shrapnel_wood_with_orders(&mut scene, &mut producer);
    visit("repaired", &mut scene);
}

/// Preserve the original simulation/GPU witness's next-frame envelope.
fn ordinary_repair_orders(
    sim: &Simulation,
    _rules: &crate::rules::ruleset::RuleSet,
    engineer: u64,
    hut: u64,
) -> Vec<CommandEnvelope> {
    vec![CommandEnvelope::new(
        sim.entities().get(engineer).unwrap().owner(),
        sim.session.tick + 1,
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: hut,
        },
    )]
}

fn repair_ordinary_bridge(
    scene: &mut HeadlessScenario,
    hut_coord: (u16, u16),
    start: (u16, u16),
    producer: &mut impl FnMut(
        &Simulation,
        &crate::rules::ruleset::RuleSet,
        u64,
        u64,
    ) -> Vec<CommandEnvelope>,
) {
    let owner = scene.sim().session.current_house.unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    assert!(crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(scene.sim(), hut_coord));
    let runtime = &mut scene.runtime;
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|e| {
            (runtime.simulation.resolve(e.type_ref()) == "CABHUT"
                && (e.position.rx(), e.position.ry()) == hut_coord)
                .then_some(e.stable_id())
        })
        .unwrap();
    // The bank can carry ordinary ore. Foot's class-entry query needs the
    // live type table even for Infantry; a registry-less spawn rejects it.
    let engineer = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "ENGINEER",
            &owner_name,
            start.0,
            start.1,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary Engineer bank placement");
    let orders = producer(&runtime.simulation, &runtime.resources.rules, engineer, hut);
    assert!(
        !orders.is_empty(),
        "collapsed bridge input must produce a repair order"
    );
    runtime.simulation.queue_commands(orders);
    let mut consumed = false;
    let mut published = false;
    for _ in 0..1200 {
        let due = scene.runtime.simulation.take_due_commands();
        let output = scene
            .runtime
            .advance_frame(
                &due,
                SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        published |= output.tick.bridge_state_changed;
        if scene
            .sim()
            .entities()
            .get(engineer)
            .is_none_or(|e| !e.lifecycle.object_alive())
        {
            consumed = true;
            eprintln!(
                "BRIDGE_ENGINEER hut{hut_coord:?} consumed frame{}",
                scene.sim().session.binary_frame - 1
            );
            break;
        }
    }
    assert!(
        consumed,
        "ordinary Engineer must enter the hut and be consumed"
    );
    assert!(
        published,
        "ordinary repair must publish the bridge mutation to the frame output"
    );
    assert!(!crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(scene.sim(), hut_coord));
}

fn assert_repaired_ground_row(
    scene: &HeadlessScenario,
    width: std::ops::RangeInclusive<u16>,
    y: u16,
    overlays: std::ops::RangeInclusive<u8>,
) {
    for x in width {
        let c = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(x, y)
            .unwrap();
        assert!(
            c.bridge_facts
                .overlay_id
                .is_some_and(|id| overlays.contains(&id))
        );
        assert_eq!(c.yr_cell_land_type, LandType::Road.as_index());
        assert!(!c.bridge_facts.has_structural_bridge());
    }
}

pub(super) fn assert_navigation_fields_equal(actual: &Value, expected: &Value, phase: &str) {
    let mismatched: Vec<_> = expected
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| actual[*key] != expected[*key])
        .collect();
    assert!(
        mismatched.is_empty(),
        "{phase} retained navigation fields differ: {mismatched:?}"
    );
}

pub(super) fn rebuilt_graphs(sim: &Simulation) -> Value {
    crate::sim::world::bridge_test_evidence::navigation_snapshot(sim, "restored", [])["graphs"]
        .take()
}

pub(super) fn navigation_authority(
    sim: &Simulation,
    points: impl IntoIterator<Item = (u16, u16)>,
) -> Value {
    let mut result =
        crate::sim::world::bridge_test_evidence::navigation_snapshot(sim, "saved", points);
    // Map resize reconstructs the shared dummy; it isn't saved cell authority.
    result.as_object_mut().unwrap().remove("dummy");
    result.as_object_mut().unwrap().remove("origin");
    // Native successful Load_Game_Content67E8CD calls581F50: it clears all
    // three graph record vectors and rebuilds via581F90 (IDs reset58200F).
    // Live incremental graph IDs/history do not survive load; retained Cell
    // and base-zone facts above do. Compare separately rebuilt graphs/futures.
    result.as_object_mut().unwrap().remove("graphs");
    result
}
