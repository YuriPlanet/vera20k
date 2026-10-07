//! Original57BAA0 ->487A10 ->MTNK/Drive/C4/lifetime ->70D4A0 comparisons.
//! Physical loading/Engineer repair use production owners. Actor list/head,
//! suspended mission and all three RNG streams are then supplied boundaries,
//! exactly as declared by shrapnel_damage/occupants.md. The joined native packet
//! runs death Anim constructors and Start in the same streams. No ordinary
//! movement, outer area-strength admission or whole-world scheduling parity is claimed.
use super::*;
use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::DriveCoord;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::timer::CdTimer;
use crate::sim::world::bridge_test_evidence::{load_shrapnel, repair_shrapnel_wood};
use crate::sim::world::lifecycle::LifecycleTestEvent;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Observe the shared callbacks without replacing any publication, admission,
/// damage, lifecycle, RNG or navigation body.
struct Observed<'a, 'world> {
    inner: LiveOrdinary<'a, 'world>,
    events: Vec<Value>,
}

impl OrdinaryBridgeHost for Observed<'_, '_> {
    type Cell = Cell;
    type Error = String;
    fn lookup(&mut self, coord: CellCoord) -> Cell {
        OrdinaryBridgeHost::lookup(&mut self.inner, coord)
    }
    fn coord(&self, cell: Cell) -> CellCoord {
        OrdinaryBridgeHost::coord(&self.inner, cell)
    }
    fn overlay(&self, cell: Cell) -> i32 {
        OrdinaryBridgeHost::overlay(&self.inner, cell)
    }
    fn write_overlay(&mut self, cell: Cell, overlay: u8) {
        self.events.push(json!({"kind":"bridge_overlay_write",
            "coord":self.coord(cell),"overlay":overlay}));
        OrdinaryBridgeHost::write_overlay(&mut self.inner, cell, overlay);
    }
    fn redraw(&mut self, cell: Cell) {
        OrdinaryBridgeHost::redraw(&mut self.inner, cell);
    }
    fn radar(&mut self, coord: CellCoord) {
        OrdinaryBridgeHost::radar(&mut self.inner, coord);
    }
    fn recalc(&mut self, cell: Cell) -> Result<(), String> {
        OrdinaryBridgeHost::recalc(&mut self.inner, cell)
    }
    fn occupants(&mut self, cell: Cell, mode: u8) -> Result<(), String> {
        let coord = self.coord(cell);
        let land = self
            .inner
            .live
            .terrain()
            .cell(coord.0 as u16, coord.1 as u16)
            .expect("physical bridge cell")
            .yr_cell_land_type;
        self.events.push(json!({"kind":"occupants","coord":coord,
            "mode_byte":mode,"land":land}));
        OrdinaryBridgeHost::occupants(&mut self.inner, cell, mode)
    }
    fn connectivity(&mut self) -> Result<(), String> {
        OrdinaryBridgeHost::connectivity(&mut self.inner)
    }
    fn rebuild_rectangle(&mut self, rect: Rect) -> Result<(), String> {
        OrdinaryBridgeHost::rebuild_rectangle(&mut self.inner, rect)
    }
}

impl OrdinaryDamageHost for Observed<'_, '_> {
    fn notify_span(&mut self, first: CellCoord, end: CellCoord) -> Result<(), String> {
        self.inner.notify_span(first, end)
    }
}

fn spawn(scene: &mut HeadlessScenario, point: (u16, u16)) -> u64 {
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            point.0,
            point.1,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("retail MTNK on repaired Road")
}

fn prime(scene: &mut HeadlessScenario, input: &Value, initial: &Value, rng: &Value) -> Vec<u64> {
    let current = (
        input["current_cell"][0].as_u64().unwrap() as u16,
        input["current_cell"][1].as_u64().unwrap() as u16,
    );
    let primary = spawn(scene, current);
    let mut actors = vec![primary];
    if input["second_member"] == true {
        let second = spawn(scene, (current.0 + 1, current.1));
        // Native supplies two already-marked tanks in one cell. Produce that
        // boundary through existing Mark transactions, with primary -> second
        // order, rather than asserting natural placement permits the overlap.
        let sim = &mut scene.runtime.simulation;
        sim.remove_entity_occupancy(primary);
        sim.remove_entity_occupancy(second);
        let entity = sim.substrate.entities.get_mut(second).unwrap();
        entity.position.rx = current.0;
        entity.position.ry = current.1;
        sim.add_entity_occupancy(second);
        sim.add_entity_occupancy(primary);
        actors.push(second);
    }
    let sim = &mut scene.runtime.simulation;
    for &id in &actors {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.on_bridge = false;
        actor.rearm_timer = CdTimer::from_raw(0, 0);
        actor.movement_target = None;
        actor.navigation.nav_com = None;
        actor.navigation.suspended_nav_com = None;
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(initial["mission"].as_i64().unwrap() as i32),
            suspended: MissionId::from_raw(if id == primary {
                initial["suspended"].as_i64().unwrap() as i32
            } else {
                -1
            }),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        actor.attack_target = (id == primary && input["target_impact"] == true)
            .then(|| AttackTarget::for_cell(115, 59));
        actor.suspended_attack_target = (id == primary)
            .then(|| input.get("restore_target"))
            .flatten()
            .map(|point| {
                TargetKind::Cell(
                    point[0].as_u64().unwrap() as u16,
                    point[1].as_u64().unwrap() as u16,
                )
            });
        // Spawn installs the active Drive locomotor; its external class state
        // is lazy (locomotor_owner/AtCoord interpret None as constructor state).
        // Use the existing Stop_Moving owner to materialize that default before
        // supplying the native packet's head/fraction. No movement tick runs.
        assert!(crate::sim::movement::track_stop_moving(actor));
        let drive = actor.locomotor.as_mut().unwrap();
        assert!(drive.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            None
        ));
        assert!(drive.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            input.get("head_cell").map(|p| {
                DriveCoord::cell(
                    p[0].as_u64().unwrap() as u16,
                    p[1].as_u64().unwrap() as u16,
                    208,
                )
            })
        ));
        assert!(
            drive.store_track_valid(
                crate::sim::movement::track_process::TrackFamily::Drive,
                drive
                    .track_head(crate::sim::movement::track_process::TrackFamily::Drive)
                    .is_some()
            )
        );
        assert!(drive.store_track_target_fraction(
            crate::sim::movement::track_process::TrackFamily::Drive,
            SimFixed::from_num(initial["fraction"].as_f64().unwrap())
        ));
    }
    // The joined native constructors receive frame 1000 and GameSpeed 4 at
    // their supplied boundary (0x00A8EB60); normalized Anim rates depend on it.
    sim.session.binary_frame = 1000;
    assert!(sim.session.game_options.apply_in_game_speed(4));
    sim.main_rng = serde_json::from_value(rng.clone()).unwrap();
    sim.scenario_rng = serde_json::from_value(rng.clone()).unwrap();
    sim.mapgen_rng = serde_json::from_value(rng.clone()).unwrap();
    actors
}

fn object_name(object: Option<CellObjectMember>, actors: &[u64]) -> String {
    match object {
        None => "null".into(),
        Some(CellObjectMember::Entity(id)) if id == actors[0] => "mtnk".into(),
        Some(CellObjectMember::Entity(id)) if actors.get(1) == Some(&id) => "mtnk2".into(),
        other => panic!("unexpected physical strip member {other:?}"),
    }
}

fn target_name(target: Option<&TargetKind>) -> String {
    match target {
        None => "null".into(),
        Some(TargetKind::Cell(x, y)) => format!("cell_{x}_{y}"),
        other => panic!("unexpected target {other:?}"),
    }
}

fn snapshot(
    sim: &Simulation,
    actors: &[u64],
    initial_losses: u32,
    other_active_units: i32,
) -> Value {
    let actor = sim.substrate.entities.get(actors[0]).unwrap();
    let owner = actor.owner;
    let second = actors.get(1).map(|&id| {
        let member = sim.substrate.entities.get(id).unwrap();
        json!({"health":member.health.current,"alive":u8::from(member.lifecycle.object_alive),
            "limbo":u8::from(member.lifecycle.in_limbo),"marked":u8::from(member.lifecycle.cell_marked),
            "object_next":object_name(sim.next_cell_object(CellObjectMember::Entity(id)),actors),
            "target":target_name(member.attack_target.as_ref().map(|target| &target.target))})
    });
    let current = (actor.position.rx, actor.position.ry);
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let xyz = position_world_coord(&actor.position);
    let head = actor
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
        .unwrap()
        .head_to()
        .map_or([0; 3], |head| [head.x, head.y, head.z]);
    json!({
        "second":second,"health":actor.health.current,"alive":u8::from(actor.lifecycle.object_alive),
        "marked":u8::from(actor.lifecycle.cell_marked),"limbo":u8::from(actor.lifecycle.in_limbo),
        "on_bridge":u8::from(actor.on_bridge),"sink":u8::from(actor.sinking.is_active()),
        "ground_head":object_name(sim.cell_objects(current,MovementLayer::Ground).next(),actors),
        "object_next":object_name(sim.next_cell_object(CellObjectMember::Entity(actors[0])),actors),
        "deferred_count":actors.iter().filter(|id| sim.substrate.pending_delete.contains(id)).count(),
        "owner_losses":sim.houses[&owner].stats.units_lost()-initial_losses,
        // Native House+5574 counts on-map units. EntityStore's type index
        // counts stored objects, including these retained dead receivers.
        "owner_active_units":sim.houses[&owner].tracking.active_for_test().0-other_active_units,
        "current_land":terrain.cell(current.0,current.1).unwrap().yr_cell_land_type,
        "impact_land":terrain.cell(115,59).unwrap().yr_cell_land_type,
        "target":target_name(actor.attack_target.as_ref().map(|target| &target.target)),
        "mission":actor.mission.current().raw(),"suspended":actor.mission.suspended().raw(),
        "suspended_target":target_name(actor.suspended_attack_target.as_ref()),
        "nav":if actor.navigation.nav_com.is_none(){"null"}else{"unexpected nav"},
        "head":head,"xyz":[xyz.x,xyz.y,xyz.z],
        "timer":[actor.rearm_timer.start_frame(),0,actor.rearm_timer.duration()],
    })
}

fn assert_state(
    sim: &Simulation,
    actors: &[u64],
    native: &Value,
    initial_losses: u32,
    other_active_units: i32,
    context: &str,
) {
    let actual = snapshot(sim, actors, initial_losses, other_active_units);
    for (field, expected) in native.as_object().unwrap() {
        if field == "fraction" {
            // Native Drive+50 is a double; the existing Drive owner stores
            // SimFixed. Compare its fixed representation, not float text.
            assert_eq!(
                sim.substrate
                    .entities
                    .get(actors[0])
                    .unwrap()
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .target_speed_fraction(),
                SimFixed::from_num(expected.as_f64().unwrap()),
                "{context}: Drive+50"
            );
        } else {
            assert_eq!(&actual[field], expected, "{context}: {field}");
        }
    }
}

/// Only filter unrelated scene members; preserve the production owners' order.
/// Native IDs are compared relative to the supplied pre-call cursor, because
/// this bounded packet does not execute complete ScenarioLoad construction.
struct EffectBoundary {
    existing_anims: BTreeSet<u64>,
    native_id_cursor: u32,
}

impl EffectBoundary {
    fn capture(sim: &Simulation) -> Self {
        Self {
            existing_anims: sim.substrate.anims.iter().map(|(id, _)| *id).collect(),
            native_id_cursor: sim.native_unique_ids.as_ref().unwrap().current_raw(),
        }
    }

    fn assert_matches(
        &self,
        sim: &Simulation,
        actors: &[u64],
        stage: &Value,
        boundary: &str,
        context: &str,
    ) {
        use crate::sim::world::display_layers::DisplayLayer;

        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .filter(|(id, _)| !self.existing_anims.contains(id))
            .collect();
        let mut names: BTreeMap<u64, String> = actors
            .iter()
            .map(|&id| (id, object_name(Some(CellObjectMember::Entity(id)), actors)))
            .collect();
        names.extend(
            anims
                .iter()
                .enumerate()
                .map(|(index, (id, _))| (**id, format!("anim_{index}"))),
        );
        let project = |members: &[u64]| -> Vec<_> {
            members
                .iter()
                .filter_map(|id| names.get(id).cloned())
                .collect()
        };
        let display: Vec<_> = (0..5)
            .map(|index| {
                project(
                    sim.display_layers()
                        .members(DisplayLayer::from_index(index).unwrap()),
                )
            })
            .collect();
        assert_eq!(
            json!({
                "logic":project(sim.logic_order()), "display":display,
                "anim_registry":anims.iter().map(|&(id, _)| &names[id]).collect::<Vec<_>>()
            }),
            stage[format!("memberships_{boundary}")],
            "{context}: ordered memberships {boundary}"
        );
        assert_eq!(
            sim.native_unique_ids
                .as_ref()
                .unwrap()
                .current_raw()
                .wrapping_sub(self.native_id_cursor) as i32,
            stage[format!("native_id_cursor_offset_{boundary}")]
                .as_i64()
                .unwrap() as i32,
            "{context}: native ID cursor {boundary}"
        );
        let native = stage[format!("anims_{boundary}")].as_array().unwrap();
        assert_eq!(
            anims.len(),
            native.len(),
            "{context}: admitted Anims {boundary}"
        );
        for (&(id, anim), expected) in anims.iter().zip(native) {
            let runtime = &anim.runtime;
            let actual = json!({
                "object":names[id], "anim_type":sim.resolve(anim.type_id),
                "location":[anim.world_coord.x,anim.world_coord.y,anim.world_coord.z],
                "native_id_offset":(anim.native_unique_id as u32).wrapping_sub(self.native_id_cursor) as i32,
                "draw_flags":anim.draw_flags, "z_adjust":anim.z_adjust,
                "is_bouncing":u8::from(anim.bounce.is_some()),
                "runtime":{
                    "current_frame":runtime.current_frame,"frame_step":runtime.frame_step,
                    "delay_remaining":runtime.delay_remaining,"rate_reload":runtime.rate_reload,
                    "frame_timer":[runtime.frame_timer.start_frame(),runtime.frame_timer.duration()],
                    "loop_remaining":runtime.loop_remaining,"first_ai_guard":u8::from(runtime.first_ai_guard),
                    "constructor_reverse":u8::from(runtime.constructor_reverse),
                    "inactive":u8::from(runtime.inactive),"paused":u8::from(runtime.paused)
                }
            });
            for (key, value) in actual.as_object().unwrap() {
                assert_eq!(
                    value, &expected[key],
                    "{context}: {} {key} {boundary}",
                    names[id]
                );
            }
            if let Some(bounce) = &anim.bounce {
                let actual = json!({
                    "elasticity_bits":bounce.elasticity.bits(),"gravity_bits":bounce.gravity.bits(),
                    "clamp_bits":bounce.angular_velocity_magnitude.bits(),
                    "position_bits":bounce.position.map(|v| v.bits()),
                    "velocity_bits":bounce.velocity.map(|v| v.bits())
                });
                for (key, value) in actual.as_object().unwrap() {
                    assert_eq!(
                        value, &expected["bounce"][key],
                        "{context}: {} {key} {boundary}",
                        names[id]
                    );
                }
                // Quaternion bytes stay native evidence. BounceState owns
                // axis/angle for presentation; this comparison covers its
                // complete gameplay position/velocity fields and RNG draws.
            }
        }
    }
}

fn native_events(stage: &Value) -> Vec<Value> {
    stage["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| {
            Some(match event["kind"].as_str().unwrap() {
                "bridge_overlay_write" => json!({"kind":"bridge_overlay_write",
                "coord":event["coord"],"overlay":event["overlay"]}),
                "occupants" => json!({"kind":"occupants","coord":event["coord"],
                "mode_byte":event["mode_byte"],"land":event["land"]}),
                "driver_return" => json!({"kind":"driver_return","al":event["al"]}),
                "detach_target" => json!({"kind":"detach_target","this":event["this"]}),
                _ => return None,
            })
        })
        .collect()
}

fn assert_span(sim: &Simulation, native: &Value, context: &str) {
    for row in native.as_array().unwrap() {
        let x = row[0].as_u64().unwrap() as u16;
        let y = row[1].as_u64().unwrap() as u16;
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
        assert_eq!(
            json!([
                x,
                y,
                cell.bridge_facts.overlay_id.map_or(-1, i32::from),
                cell.yr_cell_land_type
            ]),
            *row,
            "{context}: raw cell {x},{y}"
        );
        assert_eq!(
            cell.bridge_facts.raw_flags, 0,
            "{context}: no structural47DD70 input"
        );
    }
}

#[test]
#[ignore = "requires physical Shrapnel and retail SNOW assets"]
fn retail_wood_occupants_match_native_list_lifetime_detach_and_rng() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shrapnel_damage/occupants_joined_test_vectors.json",
    ))
    .unwrap();
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 7);
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["input"]["name"].as_str().unwrap();
        let mut scene = load_shrapnel();
        repair_shrapnel_wood(&mut scene);
        let actors = prime(
            &mut scene,
            &case["input"],
            &case["stages"][0]["before"],
            &corpus["rng_seed0"],
        );
        let sim = scene.sim();
        let actor = sim.substrate.entities.get(actors[0]).unwrap();
        let initial_losses = sim.houses[&actor.owner].stats.units_lost();
        let other_active_units =
            sim.houses[&actor.owner].tracking.active_for_test().0 - actors.len() as i32;
        let effects = EffectBoundary::capture(sim);
        for stage in case["stages"].as_array().unwrap() {
            let context = format!("{name}/{}", stage["stage"].as_str().unwrap());
            assert_state(
                scene.sim(),
                &actors,
                &stage["before"],
                initial_losses,
                other_active_units,
                &format!("{context}/before"),
            );
            effects.assert_matches(scene.sim(), &actors, stage, "before", &context);
            let runtime = &mut scene.runtime;
            let sim = &mut runtime.simulation;
            sim.clear_lifecycle_test_events_for_test();
            let mut live = LivePublication {
                sim,
                rules: &runtime.resources.rules,
                registry: Some(&runtime.resources.overlay_registry),
                collapsed: false,
            };
            let mut host = Observed {
                inner: LiveOrdinary {
                    live: &mut live,
                    changed: false,
                },
                events: Vec::new(),
            };
            let returned = ordinary_damage::damage(&mut host, (115, 59), Family::Low).unwrap();
            host.events
                .push(json!({"kind":"driver_return","al":u8::from(returned)}));
            if returned {
                // Actual already-admitted caller48A25F..48A265. This retains
                // the existing shared Cell Detach/Foot Restore owners.
                host.events
                    .push(json!({"kind":"detach_target","this":"cell_115_59"}));
                host.inner.live.sim.stop_all_targeting_cell(
                    115,
                    59,
                    Some(&runtime.resources.rules),
                    Some(&runtime.resources.overlay_registry),
                );
            }
            assert_eq!(
                host.events,
                native_events(stage),
                "{context}: publication/caller order"
            );
            let sim = &*host.inner.live.sim;
            assert_state(
                sim,
                &actors,
                &stage["after"],
                initial_losses,
                other_active_units,
                &format!("{context}/after"),
            );
            effects.assert_matches(sim, &actors, stage, "after", &context);
            let killed: Vec<_> = sim
                .lifecycle_test_events_for_test()
                .iter()
                .filter_map(|event| {
                    if let LifecycleTestEvent::PendingDeleteQueued { stable_id } = event {
                        actors.contains(stable_id).then(|| {
                            object_name(Some(CellObjectMember::Entity(*stable_id)), &actors)
                        })
                    } else {
                        None
                    }
                })
                .collect();
            let native_killed: Vec<_> = stage["damage_packets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|packet| packet["actor"].as_str().unwrap().to_owned())
                .collect();
            assert_eq!(
                killed, native_killed,
                "{context}: lethal receiver/deferred-delete order"
            );
            assert_eq!(
                json!({"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng}),
                stage["rng_after"],
                "{context}: all three complete native RNG streams"
            );
            assert_span(sim, &stage["span"], &context);
            // The native packet also preserves RadarCombatFlash+174/+17C
            // writes. This test does not claim parity for that presentation
            // timer; see occupants.md for its selected lethal-case boundary.
        }
    }
}

#[test]
#[ignore = "requires physical Shrapnel and retail SNOW assets"]
fn retail_wood_hut_collapse_matches_native_live_occupants_and_animation_order() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shrapnel_damage/hut_joined_test_vectors.json",
    ))
    .unwrap();
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 2);
    for case in corpus["cases"].as_array().unwrap() {
        let context = case["input"]["name"].as_str().unwrap();
        let stage = &case["stages"][0];
        let mut scene = load_shrapnel();
        repair_shrapnel_wood(&mut scene);
        let actors = prime(
            &mut scene,
            &case["input"],
            &stage["before"],
            &corpus["rng_seed0"],
        );
        let sim = scene.sim();
        let owner = sim.substrate.entities.get(actors[0]).unwrap().owner;
        let initial_losses = sim.houses[&owner].stats.units_lost();
        let other_units = sim.houses[&owner].tracking.active_for_test().0 - actors.len() as i32;
        let effects = EffectBoundary::capture(sim);
        assert_state(
            sim,
            &actors,
            &stage["before"],
            initial_losses,
            other_units,
            context,
        );
        effects.assert_matches(sim, &actors, stage, "before", context);

        let runtime = &mut scene.runtime;
        let hut = (
            case["input"]["hut"][0].as_u64().unwrap() as u16,
            case["input"]["hut"][1].as_u64().unwrap() as u16,
        );
        // Already-admitted Map574C20 boundary. Original Bomb/Building prefix
        // and later AnimAI are separate consumers, not supplied return values.
        assert!(crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
            &mut runtime.simulation, &runtime.resources.rules, hut,
            Some(&runtime.resources.overlay_registry)));
        let sim = &runtime.simulation;
        assert_state(
            sim,
            &actors,
            &stage["after"],
            initial_losses,
            other_units,
            context,
        );
        effects.assert_matches(sim, &actors, stage, "after", context);
        assert_span(sim, &stage["span"], context);
        assert_eq!(
            json!({"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng}),
            stage["rng_after"],
            "{context}: complete native RNG streams"
        );
        eprintln!("WOOD_HUT {hut:?}: native live occupant, cells, effects, IDs and RNG matched");
    }
}
