//! Original 739AC0/739CD0 and 73DE6E/6FABC4 observations, plus production
//! command/lifecycle regression tests. The native animation constructor is
//! observed, not executed, in the updater corpus. A separate constructor pair
//! executes it with retail SCHPDEPL and pins its timer/direction/Scenario RNG.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::Command;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};
use std::sync::OnceLock;

const ART: &str = "[SCHPDEPL]\nShadow=yes\nRate=100\n";

fn corpus() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/unit_simple_deploy.json",
        ))
        .expect("saved original-executable observations")
    })
}

/// Controlled Unit/Jumpjet arena, parsed by the production rules and ART owners.
/// Animation bounds come from the corpus's physical retail SHP, not a Rust timer.
fn fixture(input: &Value) -> (Simulation, RuleSet, u64) {
    let animation = if input["has_anim_type"].as_bool().unwrap_or(true) {
        "DeployingAnim=SCHPDEPL\n"
    } else {
        ""
    };
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [VehicleTypes]\n0=SCHP\n[Animations]\n0=SCHPDEPL\n\
         [SCHP]\nStrength=300\nSpeed=12\nROT=5\n\
         Locomotor={{92612C46-F71F-11d1-AC9F-006008055BB5}}\n\
         SpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\n\
         JumpjetClimb=10\nJumpjetHeight=500\nJumpjetNoWobbles=yes\n\
         IsSimpleDeployer={}\nDeployToLand={}\n{animation}\
         [AudioVisual]\nDeployDir=2\n\
         [Unload]\nRate=0.016\n[Guard]\nRate=0.1\n[Clear]\nBuildable=yes\n",
        input["simple"].as_bool().unwrap_or(true),
        input["deploy_to_land"].as_bool().unwrap_or(true),
    ));
    let art_ini = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    let physical_frames = corpus()["retail_art"]["result"]["raw_shp_frame_count"]
        .as_i64()
        .unwrap() as i32;
    art.bind_anim_frame_count_for_test("SCHPDEPL", physical_frames);
    rules.install_art_data(art);
    let mut sim = Simulation::with_seed(0);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let id = sim
        .spawn_object("SCHP", "Russians", 10, 10, 64, &rules)
        .unwrap();
    (sim, rules, id)
}

fn native_fixture(input: &Value, before: &Value) -> (Simulation, RuleSet, u64) {
    let (mut sim, mut rules, id) = fixture(input);
    sim.session.binary_frame = 200;
    sim.session.tick = 200;
    sim.mission_assign_exact(
        id,
        MissionId::from_raw(before["mission"].as_i64().unwrap() as i32),
        200,
    )
    .unwrap();
    if before["anim_present"].as_bool().unwrap() {
        // Construct the supplied existing attachment through the production
        // owner while its physical ART range is valid. These setup RNG draws
        // precede the updater being compared.
        sim.start_unit_deploy_anim(id, "SCHPDEPL", false, &rules)
            .unwrap();
    }
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let flags = before["flags"].as_array().unwrap();
    entity.set_unit_simple_deploy_for_test(
        flags[0].as_u64().unwrap() != 0,
        flags[1].as_u64().unwrap() != 0,
        flags[2].as_u64().unwrap() != 0,
    );
    entity.set_landing_for_deploy(before["landing"].as_u64().unwrap() != 0);
    entity.install_native_stage_fixture(native_stage(before));
    let height = input["height"].as_i64().unwrap_or(0) as i32;
    entity.position.exact_z_leptons = Some(height);
    let locomotor = entity.locomotor.as_mut().expect("production Jumpjet");
    locomotor.altitude = SimFixed::from_num(height);
    if height > 0 {
        locomotor.layer = crate::sim::movement::locomotor::MovementLayer::Air;
    }
    if input.get("start").is_some() || input.get("count").is_some() {
        // The signed/overflow corpus supplies raw native AnimType fields, some
        // deliberately impossible for a physical SHP. Preserve those controls
        // after the ordinary retained Anim was constructed; never bypass the
        // physical binder for a production allocation.
        let raw_art = IniFile::from_str(&format!(
            "{ART}Start={}\nEnd={}\n",
            input["start"].as_i64().unwrap_or(0),
            input["count"].as_i64().unwrap_or(11),
        ));
        rules.replace_art_registry_for_test(ArtRegistry::from_ini(&raw_art));
    }
    assert_native_state(&sim, id, before, "supplied native state");
    (sim, rules, id)
}

fn native_stage(state: &Value) -> StageClass {
    StageClass::from_native_fixture(
        state["stage"].as_i64().unwrap() as i32,
        state["changed"].as_u64().unwrap() as u8,
        CdTimer::from_raw(
            state["timer_start"].as_i64().unwrap() as i32,
            state["duration"].as_i64().unwrap() as i32,
        ),
        state["rate"].as_i64().unwrap() as i32,
        state["increment"].as_i64().unwrap() as i32,
    )
}

fn assert_native_state(sim: &Simulation, id: u64, expected: &Value, context: &str) {
    let entity = sim.substrate.entities.get(id).unwrap();
    let leaf = entity.mission_leaf.as_unit().unwrap();
    assert_eq!(
        json!([
            leaf.deployed(),
            leaf.deploy_begin_active(),
            leaf.deploy_reverse_active()
        ]),
        expected["flags"],
        "{context}: Unit flags",
    );
    assert_eq!(
        u8::from(entity.landing_for_deploy()),
        expected["landing"].as_u64().unwrap() as u8,
        "{context}: landing byte"
    );
    assert_eq!(
        entity.deploy_anim().is_some(),
        expected["anim_present"].as_bool().unwrap(),
        "{context}: retained Anim"
    );
    assert_eq!(
        *entity.native_stage(),
        native_stage(expected),
        "{context}: shared StageClass"
    );
    assert_eq!(
        entity.mission.current().raw(),
        expected["mission"].as_i64().unwrap() as i32,
        "{context}: current mission"
    );
    assert_eq!(
        entity.mission.queued().raw(),
        expected["queued"].as_i64().unwrap() as i32,
        "{context}: queued mission"
    );
}

fn allocated(events: &Value) -> bool {
    events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event[0] == "allocate")
}

#[test]
fn updater_flags_stage_and_signed_boundaries_match_native_lifecycle() {
    let data = corpus();
    let rows = data["lifecycle"].as_array().unwrap();
    assert_eq!(rows.len(), 100);
    for (index, row) in rows.iter().enumerate() {
        let (mut sim, rules, id) = native_fixture(&row["input"], &row["before"]);
        let rng = sim.rng_state();
        let deploying = row["input"]["entry"].as_u64().unwrap_or(0x739ac0) != 0x739cd0;
        sim.update_unit_simple_deploy(id, deploying, &rules)
            .unwrap();
        let context = format!("lifecycle[{index}] {}", row["input"]);
        assert_native_state(&sim, id, &row["after"], &context);
        if !allocated(&row["events"]) {
            assert_eq!(sim.rng_state(), rng, "{context}: no allocation draws");
        }
    }
}

#[test]
fn mission_then_shared_stage_matches_original_frame_timelines() {
    let data = corpus();
    let rows = data["mission_timelines"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        if let Some(frames) = row["frames"].as_array() {
            let (mut sim, rules, id) = native_fixture(&json!({}), &frames[0]["before"]);
            for frame in frames {
                let now = frame["frame"].as_u64().unwrap() as u32;
                sim.session.binary_frame = now;
                let context = format!("{} frame {now}", row["name"]);
                assert_native_state(&sim, id, &frame["before"], &context);
                let rng = sim.rng_state();
                let result = sim.unit_simple_mission_unload(id, &rules).unwrap();
                assert_eq!(
                    result,
                    frame["return_value"].as_i64().unwrap() as i32,
                    "{context}"
                );
                assert_native_state(&sim, id, &frame["after"], &context);
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .tick_native_stage(now as i32);
                assert_native_state(&sim, id, &frame["after_stage_tick"], &context);
                if !allocated(&frame["events"]) {
                    assert_eq!(sim.rng_state(), rng, "{context}: no allocation draws");
                }
            }
        } else {
            let result = &row["result"];
            let (mut sim, rules, id) = native_fixture(&row["input"], &result["before"]);
            let rng = sim.rng_state();
            let actual = sim.unit_simple_mission_unload(id, &rules).unwrap();
            assert_eq!(
                actual,
                result["return_value"].as_i64().unwrap() as i32,
                "{}",
                row["name"]
            );
            assert_native_state(&sim, id, &result["after"], row["name"].as_str().unwrap());
            assert_eq!(sim.rng_state(), rng);
        }
    }
}

#[test]
fn ordinary_synchronized_deploy_command_accepts_schp() {
    let (mut sim, rules, id) = fixture(&json!({}));
    assert!(sim.apply_command(
        "Russians",
        &Command::DeployMcv { entity_id: id },
        Some(&rules)
    ));
    let entity = sim.substrate.entities.get(id).unwrap();
    // Event9's Queue_Mission(Unload) at4C7812 leaves the current Guard
    // intact; the ordinary scheduler owns Commence on a later frame.
    assert_eq!(entity.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(entity.mission.queued().known(), Some(MissionType::Unload));
    assert!(
        !entity.is_fully_deployed(),
        "the command queues the owner; it does not complete deployment"
    );
}

#[test]
fn synchronized_deploy_obeys_native_event9_actor_guards() {
    // Event9's raw actor guards (4C76CF..4C7774), recorded alongside the
    // native lifecycle corpus, inspect Alive/Limbo, tether and current
    // Construction/Selling. Health, dying and crashing are not admission
    // inputs; a still-alive falling Unit can receive this event.
    for (case, admitted) in [
        ("not_alive", false),
        ("limbo", false),
        ("tethered", false),
        ("construction", false),
        ("selling", false),
        ("zero_health_crashing", true),
        ("dying", true),
    ] {
        let (mut sim, rules, id) = fixture(&json!({}));
        match case {
            "not_alive" => {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .lifecycle
                    .object_alive = false
            }
            "limbo" => {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .lifecycle
                    .in_limbo = true
            }
            "tethered" => {
                let partner = sim
                    .spawn_object("SCHP", "Russians", 12, 10, 64, &rules)
                    .unwrap();
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .dock_entered_with = Some(partner);
            }
            "construction" | "selling" => {
                let mission = if case == "construction" {
                    MissionType::Construction
                } else {
                    MissionType::Selling
                };
                sim.mission_assign_exact(id, MissionId::from_known(mission), 0)
                    .unwrap();
            }
            "zero_health_crashing" => {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.health.current = 0;
                entity.crashing = true;
            }
            "dying" => sim.substrate.entities.get_mut(id).unwrap().dying = true,
            _ => unreachable!(),
        }
        let before = sim.state_hash();
        assert_eq!(
            sim.apply_command(
                "Russians",
                &Command::DeployMcv { entity_id: id },
                Some(&rules)
            ),
            admitted,
            "{case}",
        );
        if admitted {
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .mission
                    .queued()
                    .known(),
                Some(MissionType::Unload),
                "{case}",
            );
        } else {
            assert_eq!(sim.state_hash(), before, "{case}: rejected event is inert");
        }
    }
}

#[test]
fn stock_animation_constructor_matches_native_forward_reverse_and_rng() {
    for row in corpus()["animation_constructor"].as_array().unwrap() {
        let (mut sim, rules, id) = fixture(&json!({}));
        let expected = &row["state"];
        let timer = &expected["runtime"]["frame_timer"];
        sim.session.binary_frame = timer[0].as_u64().unwrap() as u32;
        let rng = sim.rng_state();
        let reverse = row["reverse"].as_bool().unwrap();
        let anim_id = sim
            .start_unit_deploy_anim(id, "SCHPDEPL", reverse, &rules)
            .unwrap();
        let anim = sim.anim(anim_id).unwrap();
        let runtime = &anim.runtime;
        let actual = json!({
            "constructor_reverse": u8::from(runtime.constructor_reverse),
            "current_frame": runtime.current_frame,
            "delay_remaining": runtime.delay_remaining,
            "first_ai_guard": u8::from(runtime.first_ai_guard),
            "frame_step": runtime.frame_step,
            "frame_timer": [runtime.frame_timer.start_frame(), runtime.frame_timer.duration()],
            "inactive": u8::from(runtime.inactive),
            "loop_remaining": runtime.loop_remaining,
            "paused": u8::from(runtime.paused),
            "rate_reload": runtime.rate_reload,
        });
        assert_eq!(actual, expected["runtime"], "reverse={reverse}");
        assert_eq!(
            u64::from(anim.draw_flags),
            expected["draw_flags"].as_u64().unwrap()
        );
        assert_eq!(
            i64::from(anim.z_adjust),
            expected["z_adjust"].as_i64().unwrap()
        );
        let coord = sim.anim_absolute_coord(anim_id).unwrap();
        assert_eq!(json!([coord.x, coord.y, coord.z]), expected["location"]);
        assert_eq!(anim.owner_entity, Some(id));
        assert_eq!(
            anim.remap(),
            Some(crate::sim::anim_class::AnimRemap::House(
                sim.substrate.entities.get(id).unwrap().owner()
            )),
        );
        assert_eq!(row["raw_scenario_draw_count"], 0);
        assert_eq!(row["scenario_rng_unchanged"], true);
        assert_eq!(
            sim.rng_state(),
            rng,
            "stock SCHPDEPL takes no constructor RNG draws"
        );
    }
}

fn tick(sim: &mut Simulation, rules: &RuleSet) {
    let grid = sim.path_grid.clone();
    sim.advance_tick(&[], Some(rules), grid.as_deref(), None, 22);
}

fn advance_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    budget: usize,
    description: &str,
    condition: impl Fn(&Simulation) -> bool,
) {
    for _ in 0..budget {
        tick(sim, rules);
        if condition(sim) {
            return;
        }
    }
    panic!("did not reach {description} within {budget} ordinary frames");
}

fn altitude(sim: &Simulation, id: u64) -> i32 {
    crate::sim::movement::ground_pose::object_altitude_leptons(
        sim.substrate.entities.get(id).unwrap(),
    )
}

fn begin_landing_cycle() -> (Simulation, RuleSet, u64) {
    let (mut sim, rules, id) = fixture(&json!({}));
    assert!(sim.apply_command(
        "Russians",
        &Command::Move {
            entity_id: id,
            target_rx: 12,
            target_ry: 12,
            queue: false,
        },
        Some(&rules)
    ));
    advance_until(
        &mut sim,
        &rules,
        400,
        "flight at the ordinary move destination",
        |sim| {
            let entity = sim.substrate.entities.get(id).unwrap();
            altitude(sim, id) == 500 && (entity.position.rx, entity.position.ry) == (12, 12)
        },
    );
    // Event9 itself clears the previous destination; no test-only Stop, height
    // mutation or state injection stands in for that prerequisite.
    assert!(sim.apply_command(
        "Russians",
        &Command::DeployMcv { entity_id: id },
        Some(&rules)
    ));
    advance_until(&mut sim, &rules, 40, "deployment landing request", |sim| {
        sim.substrate.entities.get(id).unwrap().landing_for_deploy()
    });
    assert!(altitude(&sim, id) > 0);
    (sim, rules, id)
}

#[test]
fn ordinary_commands_land_animate_deploy_reverse_and_resume_flight() {
    let (mut sim, rules, id) = begin_landing_cycle();
    advance_until(
        &mut sim,
        &rules,
        250,
        "forward deployment animation",
        |sim| {
            let entity = sim.substrate.entities.get(id).unwrap();
            entity.unit_deploying() && entity.deploy_anim().is_some()
        },
    );
    let forward = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .deploy_anim()
        .unwrap();
    assert_eq!(altitude(&sim, id), 0);
    assert!(!sim.anim(forward).unwrap().runtime.constructor_reverse);
    assert_eq!(sim.anim(forward).unwrap().owner_entity, Some(id));
    advance_until(&mut sim, &rules, 200, "fully deployed Unit", |sim| {
        let entity = sim.substrate.entities.get(id).unwrap();
        entity.is_fully_deployed() && !entity.unit_deploying()
    });
    assert_eq!(altitude(&sim, id), 0);
    advance_until(
        &mut sim,
        &rules,
        100,
        "forward animation expiry and pointer cleanup",
        |sim| {
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .deploy_anim()
                .is_none()
        },
    );
    assert!(sim.anim(forward).is_none());
    assert!(sim.apply_command(
        "Russians",
        &Command::DeployMcv { entity_id: id },
        Some(&rules)
    ));
    advance_until(
        &mut sim,
        &rules,
        40,
        "reverse deployment animation",
        |sim| {
            let entity = sim.substrate.entities.get(id).unwrap();
            entity.unit_deploying() && entity.deploy_anim().is_some()
        },
    );
    let reverse = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .deploy_anim()
        .unwrap();
    assert_ne!(reverse, forward);
    assert!(sim.anim(reverse).unwrap().runtime.constructor_reverse);
    assert_eq!(sim.anim(reverse).unwrap().owner_entity, Some(id));
    advance_until(&mut sim, &rules, 200, "completed undeployment", |sim| {
        let entity = sim.substrate.entities.get(id).unwrap();
        !entity.is_fully_deployed() && !entity.unit_deploying()
    });
    // 739D31's NearbyLocation -> Unit Assign_Destination must reach the
    // existing Jumpjet Move_To owner. A NavCom-only fallback leaves the
    // chopper grounded even though the deployed/reverse flags are clear.
    let jumpjet = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap();
    assert!(
        jumpjet.moving(),
        "reverse completion must start Jumpjet movement: {jumpjet:?}"
    );
    assert_ne!(
        jumpjet.destination(),
        crate::sim::movement::jumpjet_movement::JumpjetRuntime::NULL
    );
    advance_until(
        &mut sim,
        &rules,
        250,
        "flight after the ordinary undeploy destination",
        |sim| altitude(sim, id) == 500,
    );
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .deploy_anim()
            .is_none()
    );
    assert!(sim.anim(reverse).is_none());
}

fn compare_snapshot_continuation(mut sim: Simulation, rules: &RuleSet, id: u64, description: &str) {
    let attachment = sim.substrate.entities.get(id).unwrap().deploy_anim();
    assert!(rules.object("SCHP").unwrap().move_sound.is_empty());
    let sound = sim.substrate.entities.get(id).unwrap().move_sound;
    assert!(!sound.is_active());
    assert_eq!(sound.countdown(), 3);
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, description, 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    // Map geometry is an external save input, and native load deliberately
    // reseeds Scenario RNG. FootLoad4DB60D..4DB624 also clears the inactive
    // +540 countdown refreshed while airborne, even with no MoveSound list.
    // Align these load transitions without changing the control's Jumpjet,
    // deployment or attached Anim state; full hashes still compare all of them.
    crate::sim::arena_fixture::flat_ground(&mut loaded, rules);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let loaded_sound = loaded.substrate.entities.get(id).unwrap().move_sound;
    assert!(!loaded_sound.is_active());
    assert_eq!(loaded_sound.countdown(), 0);
    sim.restore_move_sound_state_after_load(id);
    assert_eq!(
        loaded.substrate.entities.get(id).unwrap().deploy_anim(),
        attachment
    );
    if let Some(anim) = attachment {
        assert_eq!(loaded.anim(anim).unwrap().owner_entity, Some(id));
    }
    assert_eq!(
        sim.state_hash(),
        loaded.state_hash(),
        "{description}: restored state"
    );
    for frame in 0..220 {
        tick(&mut sim, rules);
        tick(&mut loaded, rules);
        assert_eq!(
            sim.state_hash(),
            loaded.state_hash(),
            "{description}: continuation frame {frame}"
        );
    }
    assert!(
        loaded
            .substrate
            .entities
            .get(id)
            .unwrap()
            .is_fully_deployed()
    );
    assert!(
        !loaded
            .substrate
            .entities
            .get(id)
            .unwrap()
            .landing_for_deploy()
    );
    assert!(
        loaded
            .substrate
            .entities
            .get(id)
            .unwrap()
            .deploy_anim()
            .is_none()
    );
}

#[test]
fn landing_and_attached_animation_survive_snapshot_and_continue_identically() {
    let (sim, rules, id) = begin_landing_cycle();
    // Save a landing request while the locomotor still has real airborne state.
    compare_snapshot_continuation(sim, &rules, id, "simple deploy landing");

    let (mut sim, rules, id) = begin_landing_cycle();
    advance_until(&mut sim, &rules, 250, "attached forward animation", |sim| {
        sim.substrate.entities.get(id).unwrap().unit_deploying()
    });
    for _ in 0..17 {
        tick(&mut sim, &rules);
    }
    let entity = sim.substrate.entities.get(id).unwrap();
    assert!(entity.unit_deploying());
    assert!(entity.deploy_anim().is_some());
    compare_snapshot_continuation(sim, &rules, id, "simple deploy animation");
}
