//! Production-frame comparisons for [`super::turret_rotate_sound`].
//!
//! Frame tests run the real object pass (`advance_tick`) on a
//! production-spawned building; lifecycle and snapshot tests use their existing
//! owners. The turret's own facing state is the only rotation input. Most
//! fixtures set its desired heading after an idle tick; the retail mission test
//! lets `BuildingClass::Mission_Attack` aim it during the production pass.
//!
//! `tools/spatial_oracle/turret_rotate_sound.py` executes the
//! sampling/control slices with the audio device disabled. Its corpus below
//! checks these owners; successful playback/RNG and whole native Load remain
//! outside that comparison. The frame tests additionally cover production AI.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use super::turret_rotate_sound_owner;
use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::SoundBoundary;

const TICK_MS: u32 = 67;
/// The stock cue's name, as `[GTGCAN]` authors it.
const CUE: &str = "GrandCannonRotate";

/// Three structures around the one stock shape: `GTGCAN` authors the key and
/// carries `Turret=yes`, `NOKEY` carries the turret without the sound and
/// `NOTURRET` the sound without the turret. Every one refuses passive
/// acquisition so no frame takes a weapon path.
fn rules() -> RuleSet {
    let base = "Strength=1000\nArmor=concrete\nFoundation=1x1\n\
                CanPassiveAquire=no\nHasStupidGuardMode=no\n";
    let layers = RulesLayerStack::new(crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[BuildingTypes]\n0=GTGCAN\n1=NOKEY\n2=NOTURRET\n\
         [GTGCAN]\n{base}Turret=yes\nROT=1\nTurretRotateSound={CUE}\n\
         [NOKEY]\n{base}Turret=yes\nROT=1\n\
         [NOTURRET]\n{base}ROT=1\nTurretRotateSound={CUE}\n"
    )));
    let processed = layers.process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=GrandCannonRotate\n\
         [GrandCannonRotate]\nSounds=bgralo1\nControl=loop random all decay attack\n\
         Range=20\nVolume=30\n",
    ));
    rules.bind_type_sound_references(processed.ini(), &sounds);
    rules
}

fn scene(type_name: &str) -> (Simulation, RuleSet, u64) {
    let rules = rules();
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(16));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let id = sim
        .spawn_object(type_name, "Americans", 10, 10, 0, &rules)
        .expect("fixture building");
    (sim, rules, id)
}

/// The ordered handle effects this object's turret loop published.
fn turret_events(sim: &Simulation, id: u64) -> Vec<String> {
    let owner = turret_rotate_sound_owner(id);
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::AnimationStopped { anim_id, .. } if *anim_id == owner => {
                Some("stop".to_string())
            }
            SimSoundEvent::AnimationStarted {
                anim_id, sound_id, ..
            } if *anim_id == owner => Some(format!("start {sound_id}")),
            SimSoundEvent::ObjectSoundReleased { owner: held } if *held == owner => {
                Some("release".to_string())
            }
            SimSoundEvent::ObjectSoundDetached { owner: held } if *held == owner => {
                Some("detach".to_string())
            }
            _ => None,
        })
        .collect()
}

fn latch(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .turret_rotate_latch
        .is_armed()
}

fn tick(sim: &mut Simulation, rules: &RuleSet) {
    sim.sound_events.clear_for_test();
    sim.advance_tick(&[], Some(rules), None, None, TICK_MS);
}

/// The frame this pass ran at: `advance_tick` commits `binary_frame + 1`, so
/// the pass frame is the committed one minus one.
fn pass_frame(sim: &Simulation) -> u32 {
    sim.session.binary_frame() - 1
}

fn aim(sim: &mut Simulation, id: u64, target: u16) {
    let frame = pass_frame(sim);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .body_facing
        .set(target, frame);
}

/// Rotate to the south-east heading: `0x4000` at `ROT=1` is a 64-frame arc, so
/// the sample covers frames `start + 1 .. start + 63`.
fn begin_rotation(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    tick(sim, rules);
    assert!(
        turret_events(sim, id).is_empty(),
        "an idle turret publishes nothing"
    );
    assert!(latch(sim, id), "the idle frame re-arms the latch");
    aim(sim, id, 0x4000);
}

#[test]
fn the_latch_is_hashed_state_and_load_rebuilds_the_handle() {
    let (mut sim, rules, id) = scene("GTGCAN");
    begin_rotation(&mut sim, &rules, id);
    tick(&mut sim, &rules);
    assert!(!latch(&sim, id));

    // The control latch retains history no other folded field reproduces:
    // re-arming changes the hash, and restoring the bit restores it exactly.
    let live = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .turret_rotate_latch
        .set_for_test(true);
    assert_ne!(sim.state_hash(), live, "the loop's latch is hashed");
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .turret_rotate_latch
        .set_for_test(false);
    assert_eq!(sim.state_hash(), live);

    // TechnoClass::Load rebuilds +0x488 but retains the saved +0x49C.
    // The app clears outgoing sounds; the interrupted arc stays silent.
    let saved = GameSnapshot::save(&sim, 0, 0, "turret_rotate", 0);
    let mut restored = GameSnapshot::load(&saved).expect("snapshot").sim;
    assert!(
        !restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .turret_rotate_latch
            .is_armed(),
        "the saved bit itself round-trips"
    );
    restored
        .restore_after_snapshot_load()
        .expect("structurally valid snapshot");
    assert!(restored.sound_events.is_empty(), "a load is not a cue");
    assert!(
        !restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .turret_rotate_latch
            .is_armed(),
        "Load must preserve the saved unarmed latch"
    );
    tick(&mut restored, &rules);
    assert!(turret_events(&restored, id).is_empty());
    assert!(!latch(&restored, id));
    while restored
        .substrate
        .entities
        .get(id)
        .unwrap()
        .body_facing
        .is_rotating(restored.session.binary_frame())
    {
        tick(&mut restored, &rules);
        assert!(turret_events(&restored, id).is_empty());
    }
    tick(&mut restored, &rules);
    assert_eq!(turret_events(&restored, id), ["release"]);
    assert!(latch(&restored, id));
    aim(&mut restored, id, 0);
    tick(&mut restored, &rules);
    assert_eq!(
        turret_events(&restored, id),
        vec!["stop".to_string(), format!("start {CUE}")]
    );
}

/// Observe the production effect boundary, before the native latch write and
/// before the object's mission counter advances. No audio state is simulated.
struct TurretBoundary {
    delivered: Cell<usize>,
    batches: RefCell<Vec<(bool, Vec<&'static str>)>>,
    id: u64,
    ai_counter: u32,
}

impl SoundBoundary for TurretBoundary {
    fn consume(&self, sim: &Simulation, _rules: &RuleSet, events: &[SimSoundEvent]) {
        assert_eq!(
            sim.substrate
                .entities
                .get(self.id)
                .unwrap()
                .mission
                .ai_counter(),
            self.ai_counter
        );
        let owner = turret_rotate_sound_owner(self.id);
        let batch = events[self.delivered.get()..]
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnitVoiceVisit { owner } if *owner == self.id => Some("voice"),
                SimSoundEvent::AnimationStopped { anim_id, .. } if *anim_id == owner => {
                    Some("stop")
                }
                SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == owner => {
                    Some("start")
                }
                SimSoundEvent::ObjectSoundReleased { owner: held } if *held == owner => {
                    Some("release")
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if !batch.is_empty() {
            self.batches.borrow_mut().push((latch(sim, self.id), batch));
        }
        self.delivered.set(events.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

#[test]
fn voice_precedes_turret_controls_and_each_control_reaches_its_native_boundary() {
    let (mut sim, rules, id) = scene("GTGCAN");
    begin_rotation(&mut sim, &rules, id);
    sim.prepare_unit_voice_visits([id].into());
    sim.sound_events.clear_for_test();
    let boundary = TurretBoundary {
        delivered: Cell::new(0),
        batches: RefCell::default(),
        id,
        ai_counter: sim.substrate.entities.get(id).unwrap().mission.ai_counter(),
    };
    let effects = FrameEffects::for_sound(&boundary).with_rules(Some(&rules));
    sim.object_ai_visit_one_with_effects(
        id,
        Some(&rules),
        crate::sim::world::techno_ai::ObjectAiCtx {
            effects,
            ..Default::default()
        },
    );
    assert_eq!(
        *boundary.batches.borrow(),
        [(true, vec!["voice", "stop"]), (true, vec!["start"])]
    );
    assert!(!latch(&sim, id));

    sim.sound_events.clear_for_test();
    boundary.delivered.set(0);
    boundary.batches.borrow_mut().clear();
    let release = TurretBoundary {
        ai_counter: sim.substrate.entities.get(id).unwrap().mission.ai_counter(),
        ..boundary
    };
    sim.uninit_with_context(
        id,
        crate::sim::world::UninitContext::with_rules(&rules)
            .with_effects(FrameEffects::for_sound(&release)),
    );
    assert_eq!(*release.batches.borrow(), [(false, vec!["release"])]);
    assert!(latch(&sim, id));
}

#[test]
fn turret_rotate_sound_matches_native_control_corpus() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/turret_rotate_sound.json",
    ))
    .unwrap();
    assert_eq!(corpus["samples"].as_array().unwrap().len(), 10);
    assert_eq!(corpus["histories"].as_array().unwrap().len(), 4);

    for row in corpus["samples"].as_array().unwrap() {
        let type_name = if row["turret"].as_bool().unwrap() {
            "GTGCAN"
        } else {
            "NOTURRET"
        };
        let (mut sim, rules, id) = scene(type_name);
        let mut facing = crate::sim::movement::FacingClass::new(0, 1);
        facing.set(0x4000, 100);
        sim.substrate.entities.get_mut(id).unwrap().body_facing = facing;
        sim.session
            .set_binary_frame_for_test(row["frame"].as_u64().unwrap() as u32);
        assert_eq!(
            sim.sample_building_turret_rotation(id, &rules),
            row["sample"].as_u64().unwrap() != 0,
            "{row}"
        );
    }

    for history in corpus["histories"].as_array().unwrap() {
        let type_name = if !history["turret"].as_bool().unwrap() {
            "NOTURRET"
        } else if history["cue"].as_i64().unwrap() == -1 {
            "NOKEY"
        } else {
            "GTGCAN"
        };
        let (mut sim, rules, id) = scene(type_name);
        let coord = corpus["coord"].as_array().unwrap();
        let xy = [
            coord[0].as_i64().unwrap() as i32,
            coord[1].as_i64().unwrap() as i32,
        ];
        let z = coord[2].as_i64().unwrap() as i32;
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.put_world_xy(xy);
        entity.position.set_exact_z_leptons_for_test(Some(z));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .turret_rotate_latch
            .set_for_test(history["initial_latch"].as_bool().unwrap());
        for visit in history["visits"].as_array().unwrap() {
            let prior = latch(&sim, id);
            sim.sound_events.clear_for_test();
            let boundary = TurretBoundary {
                delivered: Cell::new(0),
                batches: RefCell::default(),
                id,
                ai_counter: sim.substrate.entities.get(id).unwrap().mission.ai_counter(),
            };
            sim.visit_turret_rotate_sound(
                id,
                visit["sample"].as_u64().unwrap() != 0,
                &rules,
                FrameEffects::for_sound(&boundary).with_rules(Some(&rules)),
            );
            let expected = visit["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|call| match call["call"].as_str().unwrap() {
                    "stop" => Some("stop".to_owned()),
                    "play" if history["cue"].as_i64().unwrap() >= 0 => Some(format!("start {CUE}")),
                    // Rust omits release on an already empty, armed handle.
                    "release" if !prior => Some("release".to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(turret_events(&sim, id), expected, "{history}: {visit}");
            assert_eq!(latch(&sim, id), visit["latch"].as_u64().unwrap() != 0);
            let expected_boundaries = visit["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|call| {
                    let event = match call["call"].as_str().unwrap() {
                        "stop" => "stop",
                        "play" if history["cue"].as_i64().unwrap() >= 0 => "start",
                        "release" if !prior => "release",
                        _ => return None,
                    };
                    Some((call["latch"].as_u64().unwrap() != 0, vec![event]))
                })
                .collect::<Vec<_>>();
            assert_eq!(*boundary.batches.borrow(), expected_boundaries);
            for call in visit["calls"].as_array().unwrap() {
                assert_eq!(call["handle_offset"], 0x488);
                if call["call"] == "coordinate" || call["call"] == "play" {
                    let world = sim
                        .looping_sound_owner_coord(turret_rotate_sound_owner(id))
                        .unwrap();
                    assert_eq!(
                        call["coord"],
                        serde_json::json!([world.x, world.y, world.z])
                    );
                }
            }
            for event in &sim.sound_events {
                let world = match event {
                    SimSoundEvent::AnimationStarted { world, .. }
                    | SimSoundEvent::AnimationStopped { world, .. } => world,
                    _ => continue,
                };
                assert_eq!(
                    serde_json::json!([world.x, world.y, world.z]),
                    corpus["coord"]
                );
            }
        }
        let prior = latch(&sim, id);
        sim.sound_events.clear_for_test();
        sim.turret_rotate_sound_limbo(id, FrameEffects::default(), Some(&rules));
        assert_eq!(
            turret_events(&sim, id),
            if prior { vec![] } else { vec!["release"] }
        );
        assert_eq!(
            latch(&sim, id),
            history["limbo"]["latch"].as_u64().unwrap() != 0
        );
    }
}

/// The extracted retail rules, fixed ART and SOUNDMD through the production
/// process owner. `None` leaves the fixture's own `SKIPPED` line when `ini/` is
/// absent, as every retail test here does.
fn retail_rules() -> Option<RuleSet> {
    let (rules_ini, art_ini, sound_ini) = (
        crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?,
        crate::rules::retail_ini_fixture::retail_ini("artmd.ini")?,
        crate::rules::retail_ini_fixture::retail_ini("soundmd.ini")?,
    );
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        rules_ini,
        None,
        art_ini,
        Arc::new(SoundRegistry::from_ini(&sound_ini)),
        Default::default(),
    )
    .expect("retail cold start");
    let (mut rules, _, fixed_art, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .expect("retail scenario rules")
        .into_parts();
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&fixed_art));
    Some(rules)
}

/// The retail skirmish case the issue reports: the extracted retail rules,
/// fixed ART and SOUNDMD decide the type and its cue, and a French Grand
/// Cannon's own turret facing — the `FacingClass` slot
/// `BuildingClass::Mission_Attack` aims — is the whole rotation input.
#[test]
fn retail_grand_cannon_starts_and_releases_its_retail_cue() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(32));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let id = sim
        .spawn_object("GTGCAN", "French", 10, 10, 0, &rules)
        .expect("retail Grand Cannon");
    begin_rotation(&mut sim, &rules, id);
    tick(&mut sim, &rules);
    assert_eq!(
        turret_events(&sim, id),
        vec!["stop".to_string(), format!("start {CUE}")]
    );
    while sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .body_facing
        .is_rotating(sim.session.binary_frame())
    {
        tick(&mut sim, &rules);
        assert!(turret_events(&sim, id).is_empty());
    }
    tick(&mut sim, &rules);
    assert_eq!(turret_events(&sim, id), ["release"]);
}

/// The issue's skirmish trigger with no test-side aiming at all: the French
/// Grand Cannon sits in `Mission_Attack` with a Soviet power plant in range and
/// the *production* aim (`0x0044B16F`/`0x0044B1A8`, `[ESI+0x388].Set_Desired`)
/// is the only writer of its turret facing. This is the admission the issue's
/// clarification comment asks about: an Attack mission that changes its aim
/// feeding the rotation sound.
///
/// Supplied: the mission placement (the same test fixture the mission rows use)
/// and the target through its production setter. Everything the sound observes
/// — the aim, the sample, the block, the handle effects — is production.
#[test]
fn a_retail_grand_cannons_mission_aim_starts_and_releases_the_retail_cue() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(32));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let id = sim
        .spawn_object("GTGCAN", "French", 8, 10, 0, &rules)
        .expect("retail Grand Cannon");
    let enemy = sim
        .spawn_object("NAPOWR", "Russians", 12, 10, 0, &rules)
        .expect("retail Soviet power plant");
    let frame = sim.session.binary_frame();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: frame,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(frame as i32, 0),
        });
    sim.assign_target_represented(
        id,
        Some(TargetKind::Entity(enemy)),
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .expect("target assignment");
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .attack_target
            .target()
            .is_some(),
        "the building admits the in-range target"
    );
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .body_facing
            .is_rotating(sim.session.binary_frame()),
        "the spawn faces away from the target"
    );

    let mut started = false;
    let mut released = false;
    for _ in 0..300 {
        tick(&mut sim, &rules);
        let events = turret_events(&sim, id);
        if events.is_empty() {
            continue;
        }
        if !started {
            assert_eq!(
                events,
                vec!["stop".to_string(), format!("start {CUE}")],
                "the aim's first rotating frame starts the retail cue"
            );
            started = true;
            continue;
        }
        assert_eq!(events, ["release"], "the arc's end releases the loop");
        released = true;
        break;
    }
    assert!(started, "the mission's own aim must feed the sound");
    assert!(released, "the loop must be released once the arc ends");
    // The turret's destination is the target due east, so the rotation the
    // sound followed is the mission's own aim and not another writer.
    let aimed = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .body_facing
        .destination();
    let off_target = i32::from(aimed.wrapping_sub(0x4000) as i16).abs();
    // `0x0043ED56` aims from `GetCoords` moved by the type's
    // `PrimaryFirePixelOffset=`/`TurretAnimX/Y=` pixels, so the bearing is the
    // turret's own east (about 0x363e for this ART) rather than the cell
    // centre's. Anything near east at all can only be the target's aim: the
    // spawn facing is north.
    assert!(
        off_target <= 0x1000,
        "aimed {aimed:#06x}, expected the eastward target aim"
    );
}
