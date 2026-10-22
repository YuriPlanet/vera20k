//! Stock HARV death -> natural TIRE flight -> ordinary Logic expiry.
//!
//! Actors, empty component scene and the pre-death Scenario cursor are supplied.
//! Terrain, rules and bound SHPs are physical Tsunami/retail inputs. This is not
//! a native/full-map bootstrap or an original HARV-death comparison.

#[path = "voxel_water_expiry_controls_tests.rs"]
mod controls;

use std::collections::BTreeSet;

use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::rng::SimRng;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::display_layers::DisplayLayer;
use crate::sim::world::{FrameEffects, TickLane};

const SOURCE_CELL: (u16, u16) = (77, 113);
const WATER_CELL: (u16, u16) = (78, 112);

fn shoreline_runtime(loaded: &crate::headless_scenario::HeadlessScenario) -> SimRuntime {
    let original = &loaded.runtime.simulation;
    let terrain = original.resolved_terrain().unwrap();
    let rules = &loaded.runtime.resources.rules;
    // Reuse the existing human/AI House fixture on an explicitly empty scene,
    // then replace its grid with the unchanged physical map and map authority.
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 17, terrain.width());
    sim.session.set_map_width_for_test(terrain.width());
    sim.session.set_map_height_for_test(terrain.height());
    sim.session.set_map_name_for_test("Tsunami.mmx".to_owned());
    sim.set_playfield_bounds_for_test(original.playfield_bounds());
    sim.set_playfield_size_height_for_test(original.playfield_size_height());
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.resolve_rule_animation_lists(rules);
    assert!(sim.rebuild_dynamic_navigation(rules));
    SimRuntime {
        simulation: sim,
        resources: SimResources {
            overlay_registry: loaded.runtime.resources.overlay_registry.clone(),
            terrain_template: Some(terrain.clone()),
            rules: rules.clone(),
        },
    }
}

fn ordinary_frame(runtime: &mut SimRuntime) {
    let output = runtime
        .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::default())
        .expect("ordinary bound-resource shoreline frame");
    assert!(output.tick.frame_committed);
}

#[test]
fn retail_tsunami_harv_tire_water_expiry_constructs_wake_then_first_splash() {
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let loaded = crate::headless_scenario::load(&root, "Tsunami.mmx", 17).unwrap();
    assert!(loaded.map.header.theater.eq_ignore_ascii_case("TEMPERATE"));
    let mut runtime = shoreline_runtime(&loaded);
    let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
    let rules = &resources.rules;
    let harv = rules.object("HARV").unwrap();
    assert_eq!(harv.max_debris, 6);
    assert_eq!(harv.debris_types, vec!["TIRE".to_owned()]);
    assert_eq!(harv.debris_maximums, vec![4]);
    let tire_id = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
    let tire = rules.voxel_anim_type(tire_id);
    assert_eq!(tire.duration, 150);
    assert!(!tire.is_meteor);
    assert_eq!(rules.general.wake.name, "WAKE1");
    assert_eq!(rules.combat_damage.splash_list.first().unwrap(), "H2O_EXP3");
    for name in ["WAKE1", "H2O_EXP3"] {
        assert!(
            rules
                .art()
                .anim_runtime_config(name)
                .unwrap()
                .raw_shp_frame_count
                .is_some_and(|count| count > 0)
        );
    }
    let terrain = sim.resolved_terrain().unwrap();
    for (cell, expected) in [
        (SOURCE_CELL, (96, 0, 0, 0, 7)),
        (WATER_CELL, (129, 19, 0, 0, 2)),
    ] {
        let actual = terrain.cell(cell.0, cell.1).unwrap();
        assert_eq!(
            (
                actual.source_tile_index,
                actual.source_sub_tile,
                actual.level,
                actual.slope_type,
                actual.land_type
            ),
            expected
        );
        assert!(!actual.outside_playfield);
    }
    // The auxiliary MCV stands at the farthest passable physical cell so the
    // human House keeps a live unit after the selected miner's fatal receiver.
    let safe = terrain
        .iter()
        .filter(|cell| {
            !cell.outside_playfield
                && !cell.ground_walk_blocked
                && cell.land_type != crate::rules::terrain_rules::LandType::Water.as_index()
                && cell.speed_costs.track.is_some_and(|cost| cost > 0)
        })
        .max_by_key(|cell| cell.rx.abs_diff(SOURCE_CELL.0) + cell.ry.abs_diff(SOURCE_CELL.1))
        .map(|cell| (cell.rx, cell.ry))
        .unwrap();
    sim.spawn_object_with_overlay_registry(
        "AMCV",
        "Americans",
        safe.0,
        safe.1,
        0,
        rules,
        &resources.overlay_registry,
    )
    .expect("ordinary MCV Unlimbo");
    let miner = sim
        .spawn_object_with_overlay_registry(
            "HARV",
            "Americans",
            SOURCE_CELL.0,
            SOURCE_CELL.1,
            0,
            rules,
            &resources.overlay_registry,
        )
        .expect("ordinary HARV Unlimbo on the physical shore");
    let owner = sim.entities().get(miner).unwrap().owner();
    assert!(sim.houses.get(&owner).unwrap().is_human());
    // Fixed pre-death component cursor, admitted by a finite fixture search.
    // The real receiver and constructors consume every subsequent draw; no
    // duration, velocity, location or bounce outcome is overwritten.
    sim.scenario_rng = SimRng::new(2);
    sim.receive_own_health_c4(
        miner,
        rules,
        Some(&resources.overlay_registry),
        FrameEffects::default(),
    );
    let pieces = sim.voxel_anims().ids();
    assert_eq!(
        pieces.len(),
        2,
        "the stock HARV receiver must throw its TIREs"
    );
    let piece_id = pieces[0];
    for &id in &pieces {
        let piece = sim.voxel_anims().get(id).unwrap();
        assert_eq!(piece.type_id(), tire_id);
        assert_eq!(piece.duration(), 150);
        assert!(piece.in_logic_vector());
        assert_eq!(sim.display_layers().layer_of(id), Some(DisplayLayer::AIR));
    }
    for frame in 1..=44 {
        ordinary_frame(&mut runtime);
        let sim = &runtime.simulation;
        assert!(
            sim.anims().all(|(_, anim)| !matches!(
                sim.interner.resolve(anim.type_id()),
                "WAKE1" | "H2O_EXP3"
            )),
            "the expiry pair must not be emitted on contact, frame{frame}"
        );
    }
    let sim = &runtime.simulation;
    let piece = sim.voxel_anims().get(piece_id).unwrap();
    assert_eq!(piece.duration(), 0, "natural Water contact at frame44");
    assert!(!piece.marked_for_deletion());
    assert_eq!(piece.world_coord(), glam::IVec3::new(19972, 28836, 0));
    assert!(piece.in_logic_vector());
    assert_eq!(
        sim.resolved_terrain()
            .unwrap()
            .cell(WATER_CELL.0, WATER_CELL.1)
            .unwrap()
            .land_type,
        crate::rules::terrain_rules::LandType::Water.as_index()
    );
    let prior_anims: BTreeSet<_> = sim.anims().map(|(&id, _)| id).collect();
    ordinary_frame(&mut runtime);
    let sim = &runtime.simulation;
    assert!(sim.voxel_anims().get(piece_id).is_none());
    assert!(!sim.live_object_order_snapshot().contains(&piece_id));
    assert_eq!(sim.display_layers().layer_of(piece_id), None);
    let mut created: Vec<_> = sim
        .anims()
        .filter(|(id, _)| !prior_anims.contains(id))
        .collect();
    created.sort_unstable_by_key(|(id, _)| **id);
    assert_eq!(
        created.len(),
        2,
        "stock TIRE water expiry must construct its Wake/Splash pair"
    );
    let (wake_id, wake) = created[0];
    let (splash_id, splash) = created[1];
    assert_eq!(sim.interner.resolve(wake.type_id()), "WAKE1");
    assert_eq!(sim.interner.resolve(splash.type_id()), "H2O_EXP3");
    assert_eq!(
        wake.world_coord(),
        AnimWorldCoord {
            x: 19972,
            y: 28836,
            z: 0
        }
    );
    assert_eq!(
        splash.world_coord(),
        AnimWorldCoord {
            x: 19972,
            y: 28836,
            z: 10
        }
    );
    assert_eq!(*splash_id, *wake_id + 1);
    assert_eq!(
        splash.native_unique_id(),
        wake.native_unique_id().wrapping_add(1)
    );
    let logic = sim.live_object_order_snapshot();
    assert!(
        logic.iter().position(|id| id == wake_id).unwrap()
            < logic.iter().position(|id| id == splash_id).unwrap()
    );
    for &(id, anim) in &created {
        assert_eq!(anim.draw_flags(), 0x600);
        assert_eq!(anim.z_adjust(), 0);
        assert_eq!(anim.owner_entity(), None);
        assert_eq!(anim.owner_house(), None);
        assert_eq!(anim.remap(), None);
        assert_eq!(anim.attached_bullet(), None);
        assert!(anim.in_logic_vector());
        assert_eq!(logic.iter().filter(|&&member| member == *id).count(), 1);
        assert_eq!(
            sim.display_layers().layer_of(*id),
            Some(DisplayLayer::GROUND)
        );
    }
    // This is the completed ordinary master frame. Child first-AI guard/timer
    // state is deliberately not compared to a constructor-only native row.
}
