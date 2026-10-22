//! Selling44A002/44A16B: real Track-on-Water refusal, refund and kept constructor.
//! Legal custom-map placement; no shipped water-yard or native playback claim.
use super::*;
use crate::map::map_file::{MapCell, MapFile};
use crate::rules::locomotor_type::SpeedType;
use crate::rules::terrain_rules::LandType;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{CellActionQuery, MasterFrameTestRung, MoveAction, TickLane};

fn scene() -> Option<(SimRuntime, u64)> {
    let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_building_buildup_assets(&assets, "TEMPERATE");
    let [_, frames, rate] = retail.rules.buildup_control("GACNST");
    assert!(retail.rules.has_buildup("GACNST") && frames > 1 && rate > 0);
    assert_eq!(
        retail.rules.object("AMCV").unwrap().speed_type,
        SpeedType::Track
    );
    let mut map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Basic]\nNewINIFormat=4\nPlayer=Americans\n\
          [Structures]\n0=Americans,GACNST,256,16,16,128,None,0,0,1,0,0,None,None,None,0,0\n\
          1=Russians,GAPOWR,256,12,20,0,None,0,0,1,0,0,None,None,None,0,0\n",
    )
    .unwrap();
    // Supplied map layout, not a new pack encoder: real registered TMPs and
    // the canonical materializer supply land/slope/buildability/speed facts.
    let clear = i32::from(theater.rmg_tiles.clear_tile.unwrap());
    let water = i32::from(theater.rmg_tiles.water_set.unwrap());
    map.iso_map_pack_lookups.clear();
    map.cells = crate::map::authored_overlay::NativeOverlayMapShape::new(16, 16)
        .recalc_cells()
        .into_iter()
        .map(|(x, y)| MapCell {
            rx: x as u16,
            ry: y as u16,
            tile_index: if (14..=22).contains(&x) && (14..=22).contains(&y) {
                water
            } else {
                clear
            },
            sub_tile: 0,
            z: 0,
        })
        .collect();
    let rules = retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let terrain = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules.terrain_rules),
        Some(&registry),
        false,
        0,
    );
    let failed_cell = terrain.cell(17, 17).unwrap();
    assert_eq!(failed_cell.land_type, LandType::Water.as_index());
    assert_eq!(failed_cell.speed_costs.track, Some(0));
    assert_eq!(failed_cell.slope_type, 0);
    assert!(!failed_cell.bridge_facts.has_structural_bridge());
    assert_eq!(
        terrain.cell(12, 14).unwrap().land_type,
        LandType::Clear.as_index()
    );
    let mut sim = Simulation::with_seed(0x44a16b);
    sim.session.set_map_width_for_test(terrain.width());
    sim.session.set_map_height_for_test(terrain.height());
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().mcv_redeploy = true;
    sim.session.game_options_mut_for_test().short_game = true;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // Supplied ordinary human roster; authored entities still go through the
    // same Scenario population/placement-scope owner used by app loading.
    crate::sim::runtime::populate_staged_scenario_with_generated_inits(
        &mut sim,
        &map,
        &terrain,
        "TEMPERATE",
        &rules,
        Some(&registry),
        None,
        crate::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
        &crate::sim::scenario_session::ScenarioDescriptor::default(),
        None,
        |sim| {
            for name in ["Americans", "Russians"] {
                let owner = sim.intern(name);
                sim.houses
                    .register(HouseState::new(owner, 0, None, true, 0, 10));
            }
            let owner = sim.interner().get("Americans").unwrap();
            sim.session.set_current_house_for_test(Some(owner));
        },
    )
    .unwrap();
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let yard = sim
        .entities()
        .values()
        .find(|entity| sim.interner().resolve(entity.type_ref()) == "GACNST")
        .unwrap();
    assert!(yard.lifecycle.cell_marked() && !yard.in_limbo() && !yard.building_up());
    assert_eq!(yard.archive_target(), None);
    let yard = yard.stable_id();
    Some((
        SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template: Some(terrain),
            },
        },
        yard,
    ))
}

fn assert_retained(sim: &Simulation, owner: InternedId, id: u64) {
    let unit = sim
        .entities()
        .get(id)
        .expect("failed placement keeps its constructed AMCV");
    assert_eq!(sim.interner().resolve(unit.type_ref()), "AMCV");
    assert_eq!(unit.owner(), owner);
    assert!(unit.is_object_alive() && unit.in_limbo());
    assert!(!unit.lifecycle.cell_marked() && !unit.death.is_dying());
    assert_eq!(
        (unit.position.rx(), unit.position.ry(), unit.position.z()),
        (0, 0, 0)
    );
    assert_eq!(unit.body_facing_byte(sim.session().binary_frame()), 0);
    assert!(!sim.logic_order().contains(&id));
    let tracked = sim.houses.get(&owner).unwrap().tracking();
    assert_eq!(
        tracked.owned_count(EntityCategory::Unit, unit.type_ref()),
        1
    );
    assert_eq!(
        tracked.active_count(EntityCategory::Unit, unit.type_ref()),
        0
    );
    assert_eq!(tracked.buildings(), 0);
}

#[test]
fn authored_water_yard_refusal_refunds_keeps_constructor_and_survives_short_game() {
    let Some((mut runtime, yard)) = scene() else {
        return;
    };
    let owner = runtime.simulation.entities().get(yard).unwrap().owner();
    let initial = i32::MAX - 1000;
    runtime
        .simulation
        .houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(initial);
    let refund =
        production::techno_refund(&runtime.simulation, &runtime.resources.rules, yard).unwrap();
    assert_eq!(
        refund, 1500,
        "stock3000, neutral human factors and stock50% refund"
    );
    // LocalSize normalization reserves native margins even for a fully
    // materialized map. This clear Cell is inside that clipped playfield.
    let clicked = (12, 14);
    assert!(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(clicked.0), i32::from(clicked.1)),
        runtime.simulation.playfield_bounds(),
        runtime.simulation.resolved_terrain.as_ref(),
    ));
    assert_eq!(
        runtime
            .simulation
            .building_can_enter(
                yard,
                (clicked.0 as i16, clicked.1 as i16),
                &runtime.resources.rules,
                Some(&runtime.resources.overlay_registry),
            )
            .unwrap(),
        0,
        "ordinary marked yard admits the empty clear clicked Cell"
    );
    assert_eq!(
        runtime
            .simulation
            .cell_move_action(
                owner,
                yard,
                clicked,
                CellActionQuery {
                    mouse_over: false,
                    shift: false
                },
                &runtime.resources.rules,
                Some(&runtime.resources.overlay_registry)
            )
            .unwrap(),
        MoveAction::Move
    );
    let archive = runtime
        .simulation
        .factory_rally_cell_input(yard, clicked, &runtime.resources.rules)
        .unwrap()
        .expect("SetRally search or its real redeploy-yard fallback");
    // Core receiver timing is supplied. The actual input query prepares the
    // archive before semantic Archive+SELL; no client/raw-wire replay claim.
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::SetRally {
            rx: archive.0,
            ry: archive.1,
            producer_ids: vec![yard],
        }
    ));
    assert!(
        runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .archive_target()
            .is_some()
    );
    assert!(
        runtime.apply_command_for_test("Americans", &Command::SellBuilding { entity_id: yard })
    );
    let [_, frames, rate] = runtime.resources.rules.buildup_control("GACNST");
    for _ in 0..(frames * rate * 4 + 240) {
        runtime
            .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        if runtime.simulation.entities().get(yard).is_none() {
            break;
        }
    }
    assert!(
        runtime.simulation.entities().get(yard).is_none(),
        "scheduled Selling retires old yard"
    );
    assert_eq!(
        runtime
            .simulation
            .houses
            .get(&owner)
            .unwrap()
            .economy()
            .credits(),
        initial.wrapping_add(refund)
    );
    let held = runtime
        .simulation
        .entities()
        .values()
        .find(|entity| runtime.simulation.interner().resolve(entity.type_ref()) == "AMCV")
        .expect("real Track/Water Unlimbo refusal retains the constructor")
        .stable_id();
    assert_retained(&runtime.simulation, owner, held);
    runtime.simulation.take_master_frame_test_trace();
    runtime
        .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(
        runtime
            .simulation
            .take_master_frame_test_trace()
            .contains(&MasterFrameTestRung::Houses)
    );
    assert!(!runtime.simulation.houses.get(&owner).unwrap().is_defeated());
    assert_retained(&runtime.simulation, owner, held);
    assert_eq!(
        runtime
            .simulation
            .houses
            .get(&owner)
            .unwrap()
            .economy()
            .credits(),
        initial.wrapping_add(refund),
        "no repeated failure refund"
    );
    let base_units = runtime
        .resources
        .rules
        .general
        .base_unit_types
        .iter()
        .map(|name| runtime.simulation.interner().get(name))
        .collect::<Vec<_>>();
    let tracked = runtime.simulation.houses.get(&owner).unwrap().tracking();
    assert!(tracked.game_alive(true, base_units.iter().copied(), None, |_| false));
    assert!(
        !tracked.game_alive(false, base_units.iter().copied(), None, |_| false),
        "normal-game active counters stay empty"
    );
    let saved = GameSnapshot::save_validated(&runtime.simulation, 0, 0, "failed-yard-repack", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_retained(&restored, owner, held);
    assert!(restored.entities().get(yard).is_none());
    assert_eq!(
        restored.houses.get(&owner).unwrap().economy().credits(),
        initial.wrapping_add(refund)
    );
}
