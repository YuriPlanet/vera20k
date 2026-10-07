//! Physical Anytown bridge consumers. The admitted damage fixture isolates
//!487A10's incoming-head receiver; ordinary firing is a separate scene below.
use super::*;
use crate::headless_scenario::HeadlessScenario;
use crate::headless_scenario::SIM_TICK_MS;
use crate::rules::terrain_rules::LandType;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::DriveCoord;
use crate::sim::world::bridge_test_evidence::{
    assert_navigation_fields_equal, damage_anytown_concrete, load_anytown_concrete as loaded,
    navigation_authority, rebuilt_graphs, repair_anytown_concrete, restored_retail, save_scene,
};
use serde_json::{Value, json};

fn export(scene: &HeadlessScenario, phase: &str) {
    let Some(root) = std::env::var_os("VERA20K_ANYTOWN_EXPORT") else {
        return;
    };
    let sim = scene.sim();
    let result = json!({
        "phase": phase, "frame": sim.session.binary_frame,
        "navigation": crate::sim::world::bridge_test_evidence::navigation_snapshot(
            sim, phase, (49..=59).flat_map(|y| (84..=90).map(move |x| (x,y))),
        ),
        "entities": sim.entities().values().filter(|e|
            (84..=90).contains(&e.position.rx) && (49..=59).contains(&e.position.ry)
        ).map(|e| json!({"type":sim.resolve(e.type_ref()),"entity":e})).collect::<Vec<_>>(),
        "rng": {"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng},
    });
    let path = std::path::PathBuf::from(root).with_extension(format!("{phase}.json"));
    serde_json::to_writer_pretty(std::fs::File::create(path).unwrap(), &result).unwrap();
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_concrete_damage_repair_and_restore_publish_navigation() {
    let mut scene = loaded();
    let pristine = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    let mut saved = Vec::new();
    export(&scene, "loaded");
    for (phase, collapsed) in [("damaged", false), ("collapsed", true)] {
        assert_eq!(damage_anytown_concrete(&mut scene), collapsed);
        export(&scene, phase);
        saved.push((
            phase,
            save_scene(&scene, phase),
            navigation_authority(
                scene.sim(),
                (49..=59).flat_map(|y| (84..=90).map(move |x| (x, y))),
            ),
        ));
        for hut in [(85, 58), (89, 51)] {
            assert_eq!(
                crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(scene.sim(), hut),
                collapsed
            );
        }
    }
    repair_anytown_concrete(&mut scene);
    export(&scene, "repaired");
    saved.push((
        "repaired",
        save_scene(&scene, "repaired"),
        navigation_authority(
            scene.sim(),
            (49..=59).flat_map(|y| (84..=90).map(move |x| (x, y))),
        ),
    ));
    for (phase, bytes, expected_navigation) in saved {
        let first = restored_retail(&scene, &pristine, &bytes);
        let mut second = restored_retail(&scene, &pristine, &bytes);
        assert_navigation_fields_equal(
            &navigation_authority(
                &first,
                (49..=59).flat_map(|y| (84..=90).map(move |x| (x, y))),
            ),
            &expected_navigation,
            phase,
        );
        let first_graphs = rebuilt_graphs(&first);
        assert!(
            first_graphs == rebuilt_graphs(&second),
            "{phase} independently rebuilt graphs"
        );
        scene.runtime.simulation = first;
        export(&scene, &format!("restored_{phase}"));
        // Native load reseeds Scenario. Compare separately restored futures;
        // don't normalize an uninterrupted live future into the same stream.
        for frame in 0..40 {
            for _ in 0..2 {
                scene
                    .runtime
                    .advance_frame_for_tooling(&[], SIM_TICK_MS)
                    .unwrap();
                std::mem::swap(&mut scene.runtime.simulation, &mut second);
            }
            assert_eq!(
                scene.sim().state_hash(),
                second.state_hash(),
                "{phase} restored frame{frame}"
            );
        }
        assert!(
            rebuilt_graphs(scene.sim()) == rebuilt_graphs(&second),
            "{phase} restored graph futures"
        );
    }
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_concrete_hut_damage_matches_native_both_huts_and_three_states() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/hut_test_vectors.json",
    ))
    .unwrap();
    let mut scene = loaded();
    let pristine = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    let initial = save_scene(&scene, "healthy concrete hut comparison");
    for case in native["cases"].as_array().unwrap() {
        scene.runtime.simulation = restored_retail(&scene, &pristine, &initial);
        let runtime = &mut scene.runtime;
        let stages = match case["starting_stage"].as_str().unwrap() {
            "healthy" => 0,
            "first_damaged" | "first_damage" => 1,
            "collapsed" => 2,
            stage => panic!("unknown native hut stage {stage}"),
        };
        for _ in 0..stages {
            // The native witness produces these prerequisites with the same
            // already-admitted primitive, before the measured hut call.
            damage_ordinary(
                &mut runtime.simulation,
                &runtime.resources.rules,
                Some(&runtime.resources.overlay_registry),
                (87, 54),
                crate::sim::bridge_state::ramp_repair::Family::High,
            )
            .unwrap();
        }
        let sim = &mut runtime.simulation;
        let expected = &case["result"];
        // Import the original streams at the declared immediate caller
        // boundary. This does not claim native whole-Scenario startup parity.
        sim.main_rng = serde_json::from_value(expected["rng_before"]["main"].clone()).unwrap();
        sim.scenario_rng =
            serde_json::from_value(expected["rng_before"]["scenario"].clone()).unwrap();
        sim.mapgen_rng = serde_json::from_value(expected["rng_before"]["mapgen"].clone()).unwrap();
        sim.session.binary_frame = 1000;
        let before_anims: std::collections::BTreeSet<_> =
            sim.substrate.anims.iter().map(|(id, _)| *id).collect();
        let hut = (
            case["hut"][0].as_u64().unwrap() as u16,
            case["hut"][1].as_u64().unwrap() as u16,
        );
        assert!(crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
            sim, &runtime.resources.rules, hut, Some(&runtime.resources.overlay_registry)
        ));
        assert_eq!(
            json!({"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng}),
            expected["rng_after"],
            "hut{hut:?} state{}",
            case["starting_stage"]
        );
        let actual_cells: Vec<_> = (51..=57)
            .flat_map(|y| (86..=88).map(move |x| (x, y)))
            .map(|(x, y)| {
                let c = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
                json!({"coord":[x,y],"overlay":c.bridge_facts.overlay_id,
                    "land":c.yr_cell_land_type,"level":c.level,"slope":c.slope_type,
                    "zone_type":c.zone_type,"tile":c.final_tile_index,"subtile":c.final_sub_tile,
                    "flags":c.bridge_facts.raw_flags,"state":c.bridge_facts.state_byte})
            })
            .collect();
        assert_eq!(
            json!(actual_cells),
            expected["final_bridge_cells"],
            "hut{hut:?} state{}",
            case["starting_stage"]
        );
        let actual_anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .filter(|(id, _)| !before_anims.contains(id))
            .map(|(_, anim)| {
                json!({"type":sim.resolve(anim.type_id),
                "position":[anim.world_coord.x,anim.world_coord.y,anim.world_coord.z],
                "delay":anim.runtime.delay_remaining,"flags":anim.draw_flags,
                "loops":anim.runtime.loop_remaining,"z_adjust":anim.z_adjust,
                "reverse":u8::from(anim.runtime.constructor_reverse)})
            })
            .collect();
        assert_eq!(
            json!(actual_anims),
            expected["animation_requests"],
            "hut{hut:?} state{}",
            case["starting_stage"]
        );
        eprintln!(
            "ANYTOWN_HUT {hut:?} {} native cells/RNG/animations matched",
            case["starting_stage"]
        );
    }
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_concrete_collapse_rechecks_an_incoming_drive_head() {
    let mut scene = loaded();
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    let owner = sim.session.current_house.unwrap();
    let owner_name = sim.resolve(owner).to_owned();
    let tank = sim
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            87,
            53,
            0,
            rules,
            &runtime.resources.overlay_registry,
        )
        .expect("native fixture Road resident");
    let entity = sim.substrate.entities.get_mut(tank).unwrap();
    assert_eq!(entity.health.current, 300);
    // Supplied retained head matches native north_head_center. Normal spawn
    // owns placement/type/locomotor; this fixture does not claim path production.
    {
        let loco = entity.locomotor.as_mut().unwrap();
        assert!(loco.ensure_installed_track_state());
        assert!(loco.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(DriveCoord {
                x: 87 * 256 + 128,
                y: 54 * 256 + 128,
                z: 416,
            })
        ));
    };
    let event = BridgeDamageEvent {
        rx: 87,
        ry: 54,
        damage: 1501,
        warhead_ref: sim.intern("AP"),
        impact_z_leptons: 416,
        is_ion_cannon: false,
    };
    let registry = &runtime.resources.overlay_registry;
    assert!(
        !crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            sim,
            rules,
            &[event],
            Some(registry)
        )
    );
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().health.current,
        300
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(87, 54)
            .unwrap()
            .bridge_facts
            .overlay_id,
        Some(220)
    );
    assert!(
        crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            sim,
            rules,
            &[event],
            Some(registry)
        )
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(87, 54)
            .unwrap()
            .yr_cell_land_type,
        LandType::Water.as_index()
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(87, 53)
            .unwrap()
            .yr_cell_land_type,
        LandType::Road.as_index()
    );
    // Original487A10 checks the active Drive Is_At_Coord even though the
    // object's current cell stays Road. Actual native MTNK receiver then dies.
    assert!(
        !sim.substrate
            .entities
            .get(tank)
            .is_some_and(|e| e.is_alive()),
        "the incoming tank must receive copied-Health C4 before damage returns"
    );
}

fn force_fire_scene() -> (HeadlessScenario, u64, String) {
    let mut scene = loaded();
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let tank = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            87,
            50,
            128,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary bank placement");
    export(&scene, "attack_before_command");
    let command = CommandEnvelope::new(
        owner,
        scene.sim().session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: tank,
            target_rx: 87,
            target_ry: 54,
        },
    );
    // Production commands run at the tail of frame0. The native continuation
    // imports this boundary, before Logic visits the source at frame1.
    scene
        .runtime
        .advance_frame(
            std::slice::from_ref(&command),
            SIM_TICK_MS,
            crate::sim::world::TickLane::Ordinary,
        )
        .unwrap();
    export(&scene, "attack_command_applied");
    (scene, tank, owner_name)
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_force_fire_emits_native_muzzle_from_real_command() {
    let (mut scene, tank, _) = force_fire_scene();
    let (output, draws) = crate::sim::rng::trace_draws(|| {
        scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .unwrap()
    });
    if let Some(root) = std::env::var_os("VERA20K_ANYTOWN_EXPORT") {
        let path = std::path::PathBuf::from(root).with_extension("attack_first_tick_draws.json");
        serde_json::to_writer_pretty(std::fs::File::create(path).unwrap(), &draws).unwrap();
    }
    let shot = output
        .fire_events
        .iter()
        .find(|event| event.attacker_id == tank)
        .expect("the first native Logic visit begins firing at frame1");
    // Original Event4C6CB0 -> Unit/Foot AI ->736DF0 -> FireAt. Actual layered
    // MTNK Image=GTNK uses ART[GTNK]150,0,100. Native mission_order packet.
    assert_eq!(
        (shot.fire_coord.x, shot.fire_coord.y, shot.fire_coord.z),
        (22400, 13077, 516)
    );
    assert_eq!(scene.sim().session.binary_frame, 2);
    assert_eq!(
        scene
            .sim()
            .entities()
            .get(tank)
            .unwrap()
            .mission
            .ai_counter(),
        1
    );
    export(&scene, "attack_first_tick");
    let mut following = Vec::new();
    for _ in 0..10 {
        let frame = scene.sim().session.binary_frame;
        let before = json!({"main":scene.sim().main_rng,"scenario":scene.sim().scenario_rng,"mapgen":scene.sim().mapgen_rng});
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            scene
                .runtime
                .advance_frame_for_tooling(&[], SIM_TICK_MS)
                .unwrap()
        });
        following.push(json!({"completed_logic_frame":frame,"rng_before":before,
            "draws":draws,"rng_after":{"main":scene.sim().main_rng,"scenario":scene.sim().scenario_rng,"mapgen":scene.sim().mapgen_rng}}));
    }
    if let Some(root) = std::env::var_os("VERA20K_ANYTOWN_EXPORT") {
        let path = std::path::PathBuf::from(root).with_extension("attack_first_impact_draws.json");
        serde_json::to_writer_pretty(std::fs::File::create(path).unwrap(), &following).unwrap();
    }
    export(&scene, "attack_first_impact");
}

#[test]
#[ignore = "requires unmodified physical Anytown and retail TEMPERATE assets"]
fn retail_force_fire_collapses_concrete_then_engineer_rebuilds_it() {
    let (mut scene, tank, _) = force_fire_scene();
    let mut shots = Vec::new();
    let mut transitions = Vec::new();
    let mut prior = Some(216);
    let mut collapsed = false;
    // Retail shots each face the native BridgeStrength roll. This bound
    // allows the fixed loaded-world stream to reach both admitted hits; the
    // isolated MTNK native packet has a different global caller history.
    for _ in 0..18_000 {
        let logic_frame = scene.sim().session.binary_frame;
        let output = scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .unwrap();
        for shot in output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == tank)
        {
            shots.push(json!({"completed_logic_frame": logic_frame,
                "shot": format!("{shot:?}"),
                "actor": scene.sim().entities().get(tank),
                "rng": {"main":scene.sim().main_rng,"scenario":scene.sim().scenario_rng,"mapgen":scene.sim().mapgen_rng},
            }));
        }
        let overlay = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(87, 54)
            .unwrap()
            .bridge_facts
            .overlay_id;
        if overlay != prior {
            transitions.push(json!({"completed_logic_frame":logic_frame,"overlay":overlay}));
            eprintln!(
                "ANYTOWN_ATTACK transition frame{} {prior:?}->{overlay:?}",
                logic_frame
            );
            prior = overlay;
            export(
                &scene,
                if overlay == Some(232) {
                    "attack_collapsed"
                } else {
                    "attack_damaged"
                },
            );
        }
        if overlay == Some(232) {
            collapsed = true;
            break;
        }
    }
    if let Some(root) = std::env::var_os("VERA20K_ANYTOWN_EXPORT") {
        let path = std::path::PathBuf::from(root).with_extension("attack_trace.json");
        serde_json::to_writer_pretty(
            std::fs::File::create(path).unwrap(),
            &json!({"shots":shots,"transitions":transitions}),
        )
        .unwrap();
    }
    assert!(
        collapsed,
        "ordinary force fire must reach the concrete damage controller"
    );
    let source = scene.sim().entities().get(tank).unwrap();
    assert!(source.is_alive());
    assert!(
        source.attack_target.is_none(),
        "native70D4A0 detaches the collapsed Cell target immediately"
    );
    let mut guarding = false;
    for _ in 0..32 {
        let output = scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .unwrap();
        assert!(
            !output
                .fire_events
                .iter()
                .any(|event| event.attacker_id == tank),
            "a detached source must not keep firing at the broken bridge"
        );
        if scene
            .sim()
            .entities()
            .get(tank)
            .unwrap()
            .mission
            .current()
            .known()
            == Some(crate::sim::mission::MissionType::Guard)
        {
            guarding = true;
            break;
        }
    }
    assert!(
        guarding,
        "native Attack null-target return reaches the next Unit Ready/Commence"
    );
    export(&scene, "attack_retired");
    repair_anytown_concrete(&mut scene);
    export(&scene, "attack_repaired");
}
