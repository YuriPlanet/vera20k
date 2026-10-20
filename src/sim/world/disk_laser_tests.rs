//! Stock DISK's ordinary Attack through the shared master-frame owner.
//! Native FireAt6FE460 returns before Bullet/report/muzzle; DiskAI4A7340
//! later delivers directly. Supplied flat terrain is a Rust fixture, not
//! an executed native Scenario or app/map-load comparison.

use super::{
    DeferredDeleteTicket, FrameEffects, LifecycleOutput, LifecycleTestEvent, SimSoundEvent,
    Simulation, TickLane, UninitContext,
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::laser::{LaserBirth, LaserColor};
use crate::sim::combat::receiver_fixture::trace_area_visits;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::FacingClass;
use crate::sim::rng::SimRng;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/disk_laser.json")
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn xyz(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| int(&value[axis]))
}

fn install_rng(sim: &mut Simulation, expected: &Value) {
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(expected["main"].as_str().unwrap()).into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(expected["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(expected["mapgen"].as_str().unwrap());
}

fn rng(sim: &Simulation) -> Value {
    json!({
        "main": sim.main_rng.native_state_hex(),
        "scenario": sim.scenario_rng.native_state_hex(),
        "mapgen": sim.mapgen_rng.native_state_hex(),
    })
}

fn position(sim: &mut Simulation, id: u64, at: &Value) {
    let [x, y, z] = xyz(at);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.position.put_world_xy([x, y]);
    entity.position.set_exact_z_leptons_for_test(Some(z));
}

/// Native object locations, lifecycle flags, facings and zero rocking are
/// retained component inputs. Spawn/Rules/ART use the production owners;
/// no returned native muzzle, index or admission answer enters this scene.
struct Scene {
    sim: Simulation,
    source: u64,
    target: u64,
    different: u64,
    shots: BTreeMap<String, u64>,
    cursor: u32,
}

impl Scene {
    fn new(rules: &RuleSet, row: &Value) -> Self {
        let input = &row["input"];
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 31);
        sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        let mut actors = Vec::new();
        for (kind, owner, at) in [
            ("DISK", "Americans", input["source"].clone()),
            ("MTNK", "Russians", input["target"].clone()),
            ("MTNK", "Russians", json!([6528, 6528, 0])),
        ] {
            let [x, y, _] = xyz(&at);
            let id = sim
                .spawn_object(kind, owner, (x / 256) as u16, (y / 256) as u16, 0, rules)
                .unwrap();
            position(&mut sim, id, &at);
            assert_eq!(
                sim.substrate.entities.get(id).unwrap().health.current(),
                rules.object(kind).unwrap().strength
            );
            actors.push(id);
        }
        let [source, target, different] = actors.try_into().unwrap();
        let entity = sim.substrate.entities.get_mut(source).unwrap();
        entity.body_facing =
            FacingClass::new(input["primary_facing"].as_u64().unwrap() as u16, 100);
        entity.barrel_facing = Some(FacingClass::new(
            input["secondary_facing"].as_u64().unwrap() as u16,
            100,
        ));
        entity
            .lifecycle
            .set_cell_marked(input["marked"].as_bool().unwrap());
        entity.set_techno_ctor_random_word_for_test(int(
            &native()["lifecycle_priors"]["source_report_selector"],
        ) as u16);
        assert!(!input["on_bridge"].as_bool().unwrap());
        assert_eq!(entity.weapon_burst.index(), int(&input["burst"]));
        sim.resolve_type_handles(rules);
        sim.session
            .set_binary_frame_for_test(int(&input["frame"]) as u32);
        sim.session
            .set_tick_for_test(u64::from(sim.session.binary_frame()));
        install_rng(&mut sim, &row["rng_before"]);
        let cursor = sim.native_identity_cursor().unwrap();
        Self {
            sim,
            source,
            target,
            different,
            shots: BTreeMap::new(),
            cursor,
        }
    }

    fn role(&self, role: &Value) -> Option<u64> {
        role.as_str().map(|name| match name {
            "source" => self.source,
            "target" => self.target,
            "different" => self.different,
            _ => panic!("unknown actor role {name}"),
        })
    }

    fn name(&self, id: u64) -> &str {
        self.shots
            .iter()
            .find(|(_, candidate)| **candidate == id)
            .unwrap()
            .0
    }

    fn construct(&mut self, spec: &Value) -> u64 {
        if let Some(at) = spec.get("target_coords") {
            position(&mut self.sim, self.target, at);
        }
        let id = self.sim.construct_disk_laser();
        self.shots
            .insert(spec["name"].as_str().unwrap().to_owned(), id);
        id
    }

    fn fire(&mut self, spec: &Value, id: u64) {
        let source = spec
            .get("owner")
            .map_or(Some(self.source), |role| self.role(role));
        let target = spec
            .get("target")
            .map_or(Some(self.target), |role| self.role(role))
            .map(TargetKind::Entity);
        let weapon =
            (!spec.get("weapon").is_some_and(Value::is_null)).then(|| self.sim.intern("DiskLaser"));
        self.sim
            .fire_disk_laser(id, source, target, weapon, int(&spec["damage"]));
    }

    fn assert_body(&self, id: u64, expected: &Value, context: &str) {
        let actual = self
            .sim
            .disk_laser_observations()
            .find(|shot| shot.id == id)
            .unwrap();
        assert_eq!(
            actual.source,
            self.role(&expected["owner"]),
            "{context}: owner"
        );
        assert_eq!(
            actual.target,
            self.role(&expected["target"]).map(TargetKind::Entity),
            "{context}: target"
        );
        assert_eq!(
            actual.weapon.map(|id| self.sim.interner.resolve(id)),
            expected["weapon"].as_str().map(|_| "DiskLaser"),
            "{context}: weapon"
        );
        assert_eq!(
            actual.countdown,
            int(&expected["countdown"]),
            "{context}: countdown"
        );
        assert_eq!(
            i32::from(actual.start),
            int(&expected["start_index"]),
            "{context}: start"
        );
        assert_eq!(actual.step, int(&expected["step"]), "{context}: step");
        assert_eq!(actual.damage, int(&expected["damage"]), "{context}: damage");
        // The represented native identity is the constant Abstract ctor -1;
        // Rust retains its own stable handle, and spends no native ID here.
        assert_eq!(int(&expected["native_id"]), -1);
    }

    fn assert_state(&self, expected: &Value, row: &Value, paired: bool) {
        let context = format!(
            "{} frame{} {}",
            row["input"]["name"], expected["frame"], expected["phase"]
        );
        let order: Vec<_> = self
            .sim
            .disk_laser_observations()
            .map(|shot| self.name(shot.id))
            .collect();
        assert_eq!(
            json!(order),
            expected["disk_order"],
            "{context}: stable registry order"
        );
        let pending: Vec<_> = self
            .sim
            .substrate
            .pending_delete
            .retained_tickets()
            .filter_map(|ticket| {
                match ticket {
                    DeferredDeleteTicket::DiskLaser(id) => Some(self.name(id)),
                    _ => None, // Real downstream receiver families are outside the native service boundary.
                }
            })
            .collect();
        assert_eq!(
            json!(pending),
            expected["pending"],
            "{context}: common pending queue"
        );
        for (name, state) in expected["disks"].as_object().unwrap() {
            let id = self.shots[name];
            if state.is_null() {
                assert!(
                    !self.sim.has_disk_laser(id),
                    "{context}: {name} physically removed"
                );
            } else {
                self.assert_body(id, state, &context);
                assert!(!self.sim.live_object_order_snapshot().contains(&id));
                assert!(self.sim.display_layers().layer_of(id).is_none());
            }
        }
        assert_eq!(
            expected["native_id_cursor"], row["native_id_cursor"],
            "native Disk does not spend Scenario+214"
        );
        if paired {
            assert_eq!(
                self.sim.native_identity_cursor(),
                Some(self.cursor),
                "{context}: no native-ID spend"
            );
            assert_eq!(
                rng(&self.sim),
                row["rng_before"],
                "{context}: all three complete streams"
            );
        }
    }
}

fn assert_laser(actual: LaserBirth, expected: &Value, context: &str) {
    let state = &expected["laser"];
    assert_eq!(
        actual.frame,
        int(&expected["frame"]),
        "{context}: birth frame"
    );
    assert_eq!(
        [actual.from.x, actual.from.y, actual.from.z],
        xyz(&state["source"]),
        "{context}: copied source"
    );
    assert_eq!(
        [actual.to.x, actual.to.y, actual.to.z],
        xyz(&state["target"]),
        "{context}: copied target"
    );
    assert_eq!(
        actual.z_adjust,
        int(&state["z_adjust"]),
        "{context}: Z adjustment"
    );
    assert_eq!(
        actual.duration,
        int(&state["duration"]),
        "{context}: duration"
    );
    assert_eq!(actual.width, int(&state["width"]), "{context}: width");
    assert_eq!(
        actual.supported,
        int(&state["supported"]) != 0,
        "{context}: support"
    );
    assert_eq!(
        actual.fade_end.bits(),
        state["fade_end_bits"].as_u64().unwrap() as u32,
        "{context}: fade endpoint"
    );
    let bytes = |key: &str| std::array::from_fn(|axis| int(&state[key][axis]) as u8);
    assert_eq!(
        actual.color,
        LaserColor::Explicit {
            inner: bytes("inner"),
            outer: bytes("outer"),
            spread: bytes("spread")
        },
        "{context}: explicit material"
    );
}

fn sounds(sim: &Simulation, begin: usize) -> Vec<Value> {
    sim.sound_events[begin..].iter().filter_map(|event| match event {
        SimSoundEvent::VocAt { sound_id, audible_to, rx, ry, sub_x, sub_y, world_z_leptons } => {
            assert!(audible_to.is_none(), "Disk native NULL handle has no House admission gate");
            Some(json!({"name": sound_id, "coords": [i32::from(*rx) * 256 + sub_x.to_num::<i32>(), i32::from(*ry) * 256 + sub_y.to_num::<i32>(), *world_z_leptons]}))
        }
        _ => None,
    }).collect()
}

#[test]
fn native_disk_constructor_fire_ai_expiry_and_common_drain_histories() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = &retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let priors = &native()["lifecycle_priors"];
    let weapon = rules.weapon("DiskLaser").unwrap();
    assert_eq!(weapon.range_leptons, int(&priors["range_leptons"]));
    assert_eq!(weapon.laser_duration, int(&priors["duration_byte"]));
    assert_eq!(
        rules.art().get("DISK").unwrap().primary_fire_flh,
        [0, 0, 75].into()
    );
    assert_eq!(
        weapon.report.len(),
        priors["report_sounds"].as_array().unwrap().len()
    );
    let rows = native()["lifecycle"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(
        rows.iter()
            .map(|row| row["births"].as_array().unwrap().len())
            .sum::<usize>(),
        20
    );
    for row in rows {
        let mut scene = Scene::new(rules, row);
        let mut next_birth = 0;
        let mut last_birth = 0;
        let mut next_action = 0;
        let mut laser_count = 0;
        let mut damage_count = 0;
        let mut sound_count = 0;
        let mut visit_count = 0;
        let mut paired = true;
        let mut selected = Vec::new();
        for expected in row["states"].as_array().unwrap() {
            let frame = int(&expected["frame"]) as u32;
            scene.sim.session.set_binary_frame_for_test(frame);
            scene.sim.session.set_tick_for_test(u64::from(frame));
            let context = format!("{} frame{frame}", row["input"]["name"]);
            match expected["phase"].as_str().unwrap() {
                "constructor" => {
                    last_birth = next_birth;
                    let id = scene.construct(&row["input"]["births"][last_birth]);
                    scene.assert_body(id, &row["births"][last_birth]["constructed"], &context);
                    next_birth += 1;
                }
                "fire" => {
                    let spec = &row["input"]["births"][last_birth];
                    let id = scene.shots[spec["name"].as_str().unwrap()];
                    scene.fire(spec, id);
                    scene.assert_body(id, &row["births"][last_birth]["after_fire"], &context);
                }
                "action" => {
                    let action = &row["input"]["actions"][next_action];
                    assert_eq!(action["frame"], expected["frame"]);
                    next_action += 1;
                    match action["kind"].as_str().unwrap() {
                        "coords" => {
                            let id = scene.role(&action["role"]).unwrap();
                            position(&mut scene.sim, id, &action["value"]);
                        }
                        "crash" => scene
                            .sim
                            .substrate
                            .entities
                            .get_mut(scene.source)
                            .unwrap()
                            .crash
                            .set_crashing_for_test(true),
                        "expire" => {
                            // Native runs only the Disk expiry rung. Use the real
                            // whole dispatcher here; its other callback families
                            // are not paired native RNG/ID boundaries.
                            paired = false;
                            let id = scene.role(&action["role"]).unwrap();
                            scene.sim.notify_pointer_expired(
                                id,
                                UninitContext::new(Some(rules), Some(&registry)),
                            );
                        }
                        kind => panic!("unexpected action {kind}"),
                    }
                }
                "disk" => {
                    let visits: Vec<_> = row["ai_visits"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|visit| visit["frame"] == expected["frame"])
                        .cloned()
                        .collect();
                    let before: Vec<_> = scene.sim.disk_laser_observations().rev().map(|shot| json!({"frame": frame, "shot": scene.name(shot.id), "countdown": shot.countdown, "step": shot.step})).collect();
                    assert_eq!(
                        before, visits,
                        "{context}: native reverse-walk entry states"
                    );
                    visit_count += visits.len();
                    let output_start = scene.sim.lifecycle_outputs.len();
                    let sound_start = scene.sim.sound_events.len();
                    let (_, receipts) = trace_area_visits(|| {
                        scene
                            .sim
                            .tick_disk_lasers(rules, Some(&registry), FrameEffects::default())
                    });
                    let lasers: Vec<_> = scene.sim.lifecycle_outputs[output_start..]
                        .iter()
                        .filter_map(|event| match event {
                            LifecycleOutput::LaserCreated(laser) => Some(*laser),
                            _ => None,
                        })
                        .collect();
                    for laser in lasers {
                        assert_laser(laser, &row["laser_births"][laser_count], &context);
                        laser_count += 1;
                    }
                    for receipt in receipts {
                        let packet = &row["damage_calls"][damage_count];
                        assert_eq!(receipt["frame"], packet["frame"], "{context}: area frame");
                        assert_eq!(
                            receipt["coord"], packet["coords"],
                            "{context}: actual489280 coordinate"
                        );
                        assert_eq!(
                            receipt["damage"], packet["damage"],
                            "{context}: actual489280 damage"
                        );
                        assert_eq!(
                            receipt["source_id"],
                            json!(scene.source),
                            "{context}: firer"
                        );
                        assert_eq!(packet["firer"], "source");
                        assert_eq!(
                            packet["affects_tiberium"], 1,
                            "canonical area contract affects tiberium"
                        );
                        assert!(packet["house"].is_null());
                        assert_eq!(
                            receipt["no_source_house"], true,
                            "{context}: NULL source House"
                        );
                        assert_eq!(
                            receipt["warhead"],
                            json!(weapon.warhead.as_deref().unwrap())
                        );
                        if paired {
                            assert_eq!(
                                receipt["rng_streams"], row["rng_before"],
                                "{context}: before real receiver"
                            );
                        }
                        paired = false;
                        damage_count += 1;
                    }
                    for actual in sounds(&scene.sim, sound_start).into_iter().filter(|event| {
                        event["name"] == rules.general.disk_laser_charge_up().unwrap()
                            || event["name"] == weapon.report[0]
                    }) {
                        let call = &row["sound_calls"][sound_count];
                        let name = match int(&call["sound_id"]) {
                            0 => rules.general.disk_laser_charge_up().unwrap(),
                            1 => weapon.report[0].as_str(),
                            _ => unreachable!(),
                        };
                        assert_eq!(call["frame"], json!(frame), "{context}: sound frame");
                        assert!(call["handle"].is_null());
                        assert_eq!(
                            actual,
                            json!({"name": name, "coords": call["coords"]}),
                            "{context}: ordered request"
                        );
                        sound_count += 1;
                    }
                }
                // Presentation owns LaserUpdate. This core comparison never
                // implements an age counter or consumes native ages as inputs.
                "laser_update" => {}
                "drain" => {
                    let begin = scene.sim.lifecycle_test_events_for_test().len();
                    scene.sim.process_pending_delete_with(
                        Some(rules),
                        Some(&registry),
                        FrameEffects::default(),
                        1,
                        false,
                    );
                    selected.extend(
                        scene.sim.lifecycle_test_events_for_test()[begin..]
                            .iter()
                            .filter_map(|event| match event {
                                LifecycleTestEvent::DeferredDeleteSelected {
                                    ticket: DeferredDeleteTicket::DiskLaser(id),
                                } => Some((frame, *id)),
                                _ => None,
                            }),
                    );
                }
                phase => panic!("unexpected native phase {phase}"),
            }
            scene.assert_state(expected, row, paired);
        }
        assert_eq!(next_birth, row["births"].as_array().unwrap().len());
        assert_eq!(
            next_action,
            row["input"]["actions"].as_array().unwrap().len()
        );
        assert_eq!(laser_count, row["laser_births"].as_array().unwrap().len());
        assert_eq!(damage_count, row["damage_calls"].as_array().unwrap().len());
        assert_eq!(sound_count, row["sound_calls"].as_array().unwrap().len());
        assert_eq!(visit_count, row["ai_visits"].as_array().unwrap().len());
        let actual: Vec<_> = selected
            .iter()
            .map(|(frame, id)| json!({"frame": frame, "shot": scene.name(*id)}))
            .collect();
        let expected: Vec<_> = row["retire_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["entry"] == "0x4a7c90")
            .map(|call| json!({"frame": call["frame"], "shot": call["shot"]}))
            .collect();
        assert_eq!(
            actual, expected,
            "{}: duplicate tickets select one physical retirement",
            row["input"]["name"]
        );
        for id in scene.shots.values() {
            assert!(!scene.sim.has_disk_laser(*id));
        }
        assert!(
            scene
                .sim
                .substrate
                .pending_delete
                .retained_tickets()
                .all(|ticket| !matches!(ticket, DeferredDeleteTicket::DiskLaser(_)))
        );
        if paired {
            assert_eq!(rng(&scene.sim), row["rng_after"]);
        }
    }
}

fn snapshot_copy(sim: &Simulation) -> Simulation {
    let bytes = GameSnapshot::save(sim, 0, 0, "Disk component scene", 0);
    GameSnapshot::load(&bytes).unwrap().sim
}

#[test]
fn disk_snapshot_resumes_active_and_duplicate_pending_shots() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = &retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let row = &native()["lifecycle"][0];
    for pending in [false, true] {
        let mut scene = Scene::new(rules, row);
        let spec = &row["input"]["births"][0];
        let id = scene.construct(spec);
        scene.fire(spec, id);
        for frame in 201..=205 {
            scene.sim.session.set_binary_frame_for_test(frame);
            scene.sim.session.set_tick_for_test(u64::from(frame));
            scene
                .sim
                .tick_disk_lasers(rules, Some(&registry), FrameEffects::default());
        }
        if pending {
            for _ in 0..2 {
                scene.sim.notify_pointer_expired(
                    scene.source,
                    UninitContext::new(Some(rules), Some(&registry)),
                );
            }
            assert_eq!(
                scene
                    .sim
                    .substrate
                    .pending_delete
                    .retained_tickets()
                    .filter(|ticket| *ticket == DeferredDeleteTicket::DiskLaser(id))
                    .count(),
                2
            );
        }
        let saved_body = serde_json::to_value(&scene.sim.disk_lasers).unwrap();
        let saved_pending: Vec<_> = scene
            .sim
            .substrate
            .pending_delete
            .retained_tickets()
            .collect();
        // This is a Rust save/resume regression, not native RNG persistence:
        // retail's existing load owner resets Scenario to Seed0. Main/MapGen
        // are retained through that owner's process-state seam.
        scene.sim.scenario_rng = SimRng::new(0);
        let mut resumed = snapshot_copy(&scene.sim);
        resumed.retain_process_state_for_load(super::SnapshotProcessState::Live(&scene.sim));
        resumed.resolved_terrain = scene.sim.resolved_terrain.clone();
        resumed.restore_after_snapshot_load().unwrap();
        resumed.resolve_type_handles(rules);
        resumed
            .restore_map_authority_after_snapshot_load(rules, &registry, None)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&resumed.disk_lasers).unwrap(),
            saved_body
        );
        assert_eq!(
            resumed
                .substrate
                .pending_delete
                .retained_tickets()
                .collect::<Vec<_>>(),
            saved_pending
        );
        assert_eq!(rng(&resumed), rng(&scene.sim));
        assert_eq!(
            resumed.native_identity_cursor(),
            scene.sim.native_identity_cursor()
        );

        let mut damage_calls = 0;
        let target_id = scene.target;
        for frame in 206..=224 {
            let observations: Vec<_> = [&mut scene.sim, &mut resumed]
                .into_iter()
                .map(|sim| {
                    sim.session.set_binary_frame_for_test(frame);
                    sim.session.set_tick_for_test(u64::from(frame));
                    let output_start = sim.lifecycle_outputs.len();
                    let sound_start = sim.sound_events.len();
                    let (_, area) = trace_area_visits(|| {
                        sim.tick_disk_lasers(rules, Some(&registry), FrameEffects::default())
                    });
                    let lasers: Vec<_> = sim.lifecycle_outputs[output_start..]
                        .iter()
                        .filter_map(|output| match output {
                            LifecycleOutput::LaserCreated(birth) => Some(*birth),
                            _ => None,
                        })
                        .collect();
                    let sound = sounds(sim, sound_start);
                    sim.process_pending_delete_with(
                        Some(rules),
                        Some(&registry),
                        FrameEffects::default(),
                        1,
                        false,
                    );
                    (
                        lasers,
                        sound,
                        area,
                        serde_json::to_value(&sim.disk_lasers).unwrap(),
                        sim.substrate
                            .pending_delete
                            .retained_tickets()
                            .collect::<Vec<_>>(),
                        rng(sim),
                        sim.substrate
                            .entities
                            .get(target_id)
                            .unwrap()
                            .health
                            .current(),
                        sim.native_identity_cursor(),
                    )
                })
                .collect();
            assert_eq!(
                observations[0], observations[1],
                "pending={pending} frame{frame}: resume through actual AI, receiver and common drain"
            );
            damage_calls += observations[0].2.len();
        }
        assert_eq!(damage_calls, usize::from(!pending));
        assert!(!scene.sim.has_disk_laser(id));
        assert!(!resumed.has_disk_laser(id));
    }
}

#[test]
fn retail_disk_attack_delivers_without_ordinary_bullet_fire() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = retail.rules;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    let source = sim
        .spawn_object_with_overlay_registry("DISK", "Americans", 10, 10, 64, &rules, &registry)
        .expect("stock DISK enters through the ordinary constructor");
    let target = sim
        // Six cells preserves this witness's unrocked source; the MTNK's
        // five-cell weapon cannot counterfire. The first RED used four cells.
        .spawn_object_with_overlay_registry("MTNK", "Russians", 16, 10, 0, &rules, &registry)
        .expect("full-health stock MTNK");
    sim.resolve_type_handles(&rules);
    let health = sim.substrate.entities.get(target).unwrap().health.current();
    assert_eq!(health, rules.object("MTNK").unwrap().strength);
    let command = CommandEnvelope::new(
        sim.substrate.entities.get(source).unwrap().owner(),
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: source,
            target_id: target,
        },
    );
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            ..SimResources::empty()
        },
    };
    let mut commands = vec![command];
    for frame in 0..300 {
        let output = runtime
            .advance_frame(
                &std::mem::take(&mut commands),
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary frame");
        assert!(
            output
                .fire_events
                .iter()
                .all(|event| event.attacker_id != source),
            "Disk FireAt reached the ordinary Bullet/report/muzzle tail at frame {frame}"
        );
        let remaining = runtime
            .simulation
            .substrate
            .entities
            .get(target)
            .unwrap()
            .health
            .current();
        if remaining < health {
            assert!(remaining > 0, "the first stock discharge is nonfatal");
            return;
        }
    }
    panic!("ordinary Attack never delivered the first Disk discharge");
}
