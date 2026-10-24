//! Physical Hills + full-HP stock actors, ordinary Attack/ROF/Bullet/receiver.
//! Native sound sites: Techno70218B/7021FB (7509E0, NULL handle), Building
//! 441779 (750E20, flags0), before the fatal AP impact's AnimStart424D2B.
//! The primary chronology needs no device. The separate ignored companion
//! drives the real catalog/SFX service and checks preparation, not audible PCM,
//! native worker timing, an exact chosen clip/pitch, or a native Main endpoint.

use super::super::frame_effects::FrameClientBoundary;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::presentation::building_anim::SoundPlaybackInputs;
use crate::app::process_assets::{ProcessAssets, ProcessAudioCatalog};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::audio::events::SoundEventQueue;
use crate::audio::sfx::{PcmObservationConfig, PcmObservationContext, SfxPlayer, SpatialListener};
use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::rng::trace_draws;
use crate::sim::world::{
    FrameEffects, SimFrameOutput, SimSoundEvent, Simulation, SoundBoundary, TickLane,
};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};
use std::cell::RefCell;

#[derive(Clone, Copy)]
struct Case {
    kind: &'static str,
    strength: i32,
    buildup: &'static str,
    cue: &'static str,
}

const CASES: [Case; 2] = [
    Case {
        kind: "GAPOWR",
        strength: 750,
        buildup: "GAPOWRMK",
        cue: "PowerPlantDie",
    },
    Case {
        kind: "GAPILE",
        strength: 500,
        buildup: "GAPILEMK",
        cue: "BuildingGenericDie",
    },
];

/// Copies only newly delivered real facts, then delegates the production owner.
/// No synthetic event, admission answer, receiver or replacement RNG is supplied.
struct ObservingBoundary<'a, 'b> {
    client: &'a FrameClientBoundary<'b>,
    copied: RefCell<Vec<SimSoundEvent>>,
}

impl SoundBoundary for ObservingBoundary<'_, '_> {
    fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]) {
        self.copied
            .borrow_mut()
            .extend_from_slice(&events[self.client.delivered()..]);
        self.client.consume(sim, rules, events);
    }

    fn delivered(&self) -> usize {
        self.client.delivered()
    }

    fn update_looping_sound(&self, sim: &Simulation, owner: u64, world: AnimWorldCoord) {
        self.client.update_looping_sound(sim, owner, world);
    }
}

fn frame(
    scene: &mut HeadlessScenario,
    audio: &mut AppAudioRuntime,
    assets: &AssetManager,
    catalog: &ProcessAudioCatalog,
    listener: SpatialListener,
    local: &str,
) -> (SimFrameOutput, Vec<SimSoundEvent>) {
    let commands = scene.runtime.simulation.take_due_commands();
    let mut queue = SoundEventQueue::new();
    let mut radar = |_, _, _: &RuleSet| false;
    let playback = audio.sfx_player.as_mut().map(|player| {
        (
            player,
            SoundPlaybackInputs {
                assets,
                catalog,
                listener,
            },
        )
    });
    let client = FrameClientBoundary::new(playback, Some(local), &mut queue, &mut radar);
    let observed = ObservingBoundary {
        client: &client,
        copied: RefCell::new(Vec::new()),
    };
    let output = scene
        .runtime
        .advance_frame(
            &commands,
            crate::app::types::SIM_TICK_MS,
            TickLane::Ordinary,
            FrameEffects::for_sound(&observed),
        )
        .expect("ordinary bound-resource frame");
    assert!(output.tick.frame_committed && !output.network_waiting && !output.outcome_waiting);
    assert_eq!(observed.delivered(), output.sound_events_delivered);
    // Keep this frame's real delivered-prefix cursor: only the suffix passes
    // through the dispatcher/optional SoundPlaybackInputs, just as in sim_tick.
    observed.consume(
        scene.sim(),
        &scene.runtime.resources.rules,
        &output.sound_events,
    );
    assert_eq!(observed.delivered(), output.sound_events.len());
    let copied = observed.copied.into_inner();
    assert_eq!(
        copied.len(),
        output.sound_events.len(),
        "no prefix replay or lost suffix"
    );
    (output, copied)
}

fn insert_records(map: &mut String, section: &str, records: &str) {
    let marker = format!("[{section}]");
    if let Some(start) = map.find(&marker) {
        let end = start + map[start..].find('\n').expect("physical section newline") + 1;
        map.insert_str(end, records);
    } else {
        map.push_str(&format!("\n{marker}\n{records}"));
    }
}

fn audio_runtime(with_device: bool) -> AppAudioRuntime {
    let player = with_device.then(|| {
        let mut player =
            SfxPlayer::new().expect("selected preparation companion requires an audio device");
        player.set_volume(0.0); // Output is muted; request admission/decoding still run.
        player
    });
    AppAudioRuntime {
        master_groups: Default::default(),
        theme: Default::default(),
        service_clock: Default::default(),
        music_player: None,
        sfx_player: player,
        launcher_audio_available: with_device,
        theme_startup_suppressed: false,
    }
}

fn cue_id(event: &SimSoundEvent) -> Option<&str> {
    match event {
        // Untouched714 emits EntityDied; candidate uses native-shaped VocAt.
        SimSoundEvent::EntityDied { die_sound_id, .. } => Some(die_sound_id),
        SimSoundEvent::VocAt { sound_id, .. }
        | SimSoundEvent::VocAtCoord { sound_id, .. }
        | SimSoundEvent::AnimationStarted { sound_id, .. } => Some(sound_id),
        _ => None,
    }
}

fn cue_matches_coordinate(event: &SimSoundEvent, expected: AnimWorldCoord) -> bool {
    match event {
        SimSoundEvent::EntityDied { world, .. } => *world == expected,
        SimSoundEvent::VocAtCoord { coordinate, .. } => {
            AnimWorldCoord::from(*coordinate) == expected
        }
        SimSoundEvent::VocAt {
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons,
            ..
        } => {
            // The public coordinate owner decomposes this ordinary positive
            // Hills location; no private core helper or duplicate math owner.
            let (ex, ey, sx, sy, _) = expected.to_cell_sub_z();
            (*rx, *ry, *sub_x, *sub_y, *world_z_leptons) == (ex, ey, sx, sy, expected.z)
        }
        _ => false,
    }
}

fn hostile_latch(entity: &crate::sim::game_entity::GameEntity) -> bool {
    // Read the canonical serialized owner, rather than widen a private core API.
    serde_json::to_value(entity).unwrap()["ai_sell_repair"]["was_attacked_by_enemy"]
        .as_bool()
        .expect("existing serialized hostile-hit byte")
}

fn building_kills(sim: &Simulation, house: crate::sim::intern::InternedId) -> u64 {
    // The convenience buildings_killed getter is core cfg(test), unavailable
    // to client tests. The existing saved statistics expose the same counter.
    serde_json::to_value(sim.houses().get(&house).unwrap().stats()).unwrap()["buildings_killed"]
        .as_u64()
        .expect("canonical saved building-kill counter")
}

struct History {
    case: Case,
    origin: AnimWorldCoord,
    fatal_frame: u32,
    fatal_context: u32,
    produced: Vec<SimSoundEvent>,
    copied: Vec<SimSoundEvent>,
    impact_id: u64,
    coordinate_requests: Vec<serde_json::Value>,
    prepared: Option<serde_json::Value>,
    audio_main_advances: usize,
}

fn history(
    case: Case,
    root: &std::path::Path,
    assets: &AssetManager,
    catalog: &ProcessAudioCatalog,
    with_device: bool,
) -> History {
    let mut map = String::from_utf8(
        assets
            .resolve_ref("XMP03T4.MAP")
            .expect("physical Hills")
            .bytes
            .to_vec(),
    )
    .expect("physical Hills INI text");
    insert_records(
        &mut map,
        "Units",
        "DeathSoundAttacker=<human player>,MTNK,256,30,85,64,Guard,None,0,-1,0,-1,1,1\n",
    );
    insert_records(
        &mut map,
        "Structures",
        &format!(
            "DeathSoundVictim=Neutral,{},256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n",
            case.kind,
        ),
    );
    let directory = crate::map::source::test_support::TestDirectory::new("building-death-sound");
    let path = directory.path().join(format!("{}.map", case.kind));
    std::fs::write(&path, map).unwrap();
    let name = path.to_str().unwrap();
    let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.bases = true;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    launch.options.crates = false;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    let mut scene = load_session(root, name, 0x1234_5678, launch)
        .expect("physical Hills/Rules/ART/TMP and authored full-health stock actors");
    let sim = scene.sim();
    let owner = sim.session().current_house().unwrap();
    let local = sim.resolve(owner).to_owned();
    let neutral = sim.interner().get("Neutral").unwrap();
    let find = |kind: &str, house, cell| {
        sim.entities()
            .values()
            .find(|e| {
                sim.resolve(e.type_ref()) == kind
                    && e.owner() == house
                    && (e.position.rx(), e.position.ry()) == cell
            })
            .unwrap()
            .stable_id()
    };
    let attacker = find("MTNK", owner, (30, 85));
    let victim = find(case.kind, neutral, (31, 85));
    let rules = &scene.runtime.resources.rules;
    let object = rules.object(case.kind).unwrap();
    let tank = rules.object("MTNK").unwrap();
    let target = sim.entities().get(victim).unwrap();
    assert_eq!(object.strength, case.strength);
    assert_eq!(
        target.health.current(),
        case.strength,
        "full constructor HP, no supplied damage prior"
    );
    assert!(!hostile_latch(target) && !target.repairing.is_on());
    assert!(
        target.lifecycle.object_alive() && target.lifecycle.cell_marked() && !target.in_limbo()
    );
    assert!(sim.logic_order().contains(&victim) && sim.logic_order().contains(&attacker));
    assert!(target.passenger_role.occupants().is_empty());
    assert!(
        target
            .passenger_role
            .cargo()
            .is_none_or(|cargo| cargo.is_empty())
    );
    assert!(!object.can_be_occupied && !object.explodes);
    assert!(!crate::map::houses::is_allied_with(
        sim.house_alliances(),
        sim.resolve(neutral),
        &local
    ));
    assert_eq!(sim.houses().get(&neutral).unwrap().current_iq(), 0);
    assert!(rules.general.iq_repair_sell > 0 && rules.general.iq_sell_back > 0);
    assert_eq!(
        sim.entities().get(attacker).unwrap().health.current(),
        tank.strength,
        "the authored MTNK also starts at full constructor HP"
    );
    assert!(
        object.voice_die.is_empty(),
        "these stock Buildings have no VoiceDie arm"
    );
    if case.kind == "GAPOWR" {
        assert_eq!(object.die_sounds.len(), 1);
        assert!(object.die_sounds[0].eq_ignore_ascii_case(case.cue));
    } else {
        assert!(object.die_sounds.is_empty());
        assert!(
            rules
                .general
                .building_die_sound
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case(case.cue))
        );
    }
    assert_eq!(tank.primary(), Some("105mm"));
    assert_eq!(tank.ammo, -1);
    let weapon = rules.weapon("105mm").unwrap();
    assert_eq!(weapon.damage, 65);
    assert_eq!(weapon.warhead.as_deref(), Some("AP"));
    let ap = rules.warhead("AP").unwrap();
    assert_eq!(ap.anim_list, ["S_CLSN16", "S_CLSN22"]);
    // The actual AP impact can be identified separately from foundation effects
    // even when both report Explosion14; do not identify it by cue name alone.
    assert!(
        object
            .explosion_anims
            .iter()
            .all(|name| !ap.anim_list.contains(name))
    );
    for name in &ap.anim_list {
        assert!(
            rules
                .art()
                .anim_runtime_config(name)
                .unwrap()
                .report
                .as_deref()
                .is_some_and(|report| report.eq_ignore_ascii_case("Explosion14"))
        );
    }
    assert_eq!(
        rules
            .art()
            .resolve_metadata_entry(&object.id, &object.image)
            .unwrap()
            .buildup
            .as_deref(),
        Some(case.buildup)
    );
    assert!(
        rules.has_buildup(case.kind) && rules.buildup_control(case.kind)[1] > 0,
        "real Buildup SHP must be bound by the production loader"
    );
    let terrain = sim.resolved_terrain().unwrap();
    for cell in [
        (30, 85),
        (31, 85),
        (32, 85),
        (33, 85),
        (31, 86),
        (32, 86),
        (33, 86),
    ] {
        let ground = terrain
            .cell(cell.0, cell.1)
            .expect("authored actor/footprint cell");
        assert!(!ground.outside_playfield && !ground.has_bridge_deck && !ground.has_ramp);
        assert!(
            !ground.base_build_blocked
                && !ground.base_ground_walk_blocked
                && !ground.terrain_object_blocks
                && !ground.overlay_blocks
        );
        assert!(
            ground
                .base_speed_costs
                .cost_for_speed_type(tank.speed_type)
                .is_some_and(|cost| cost > 0),
            "actual Track/buildable Hills ground, which can be Road rather than Clear"
        );
    }
    let ap_anims = ap.anim_list.clone();
    let origin: AnimWorldCoord =
        crate::sim::movement::ground_pose::object_location(target, Some(terrain)).into();
    let (sx, sy) = crate::util::lepton::absolute_leptons_to_screen(origin.x, origin.y, origin.z);
    let listener = SpatialListener {
        tactical_width: 640,
        tactical_height: 480,
        origin_x: sx - 320.0,
        origin_y: sy - 240.0,
        zoom: 1.0,
    };
    let lost_before = sim.houses().get(&neutral).unwrap().stats().buildings_lost();
    let kills_before = building_kills(sim, owner);
    let mut audio = audio_runtime(with_device);
    let command = CommandEnvelope::new(
        owner,
        sim.session().tick() + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: victim,
        },
    );
    scene.runtime.simulation.queue_command(command);
    let mut shots = 0;
    let mut survived = false;
    let mut observing = false;
    for completed in 1..=3000_u64 {
        let before = scene
            .sim()
            .entities()
            .get(victim)
            .expect("victim before actual fatal frame");
        let hp_before = before.health.current();
        assert!(hp_before > 0 && before.lifecycle.object_alive());
        assert_ne!(
            before.mission.effective().known(),
            Some(MissionType::Selling)
        );
        assert!(!before.repairing.is_on());
        if with_device && !observing && hp_before < case.strength / 4 {
            audio
                .sfx_player
                .as_mut()
                .unwrap()
                .observe_pcm(PcmObservationConfig {
                    sound_ids: vec![case.cue.into(), "Explosion14".into()],
                    max_events: 32,
                    max_samples_per_event: 262_144,
                })
                .unwrap();
            observing = true;
        }
        // This diagnostic context means the last completed boundary, not an
        // independently sampled timestamp of an in-frame audio callback.
        let context = scene.sim().session().binary_frame();
        if let Some(player) = audio.sfx_player.as_mut() {
            player.set_pcm_observation_context(PcmObservationContext {
                completed_steps: completed - 1,
                simulation_tick: scene.sim().session().tick(),
                binary_frame: context,
            });
        }
        let (output, copied) = frame(&mut scene, &mut audio, assets, catalog, listener, &local);
        shots += output
            .fire_events
            .iter()
            .filter(|event| {
                event.attacker_id == attacker && event.target == TargetKind::Entity(victim)
            })
            .count();
        let fatal = scene
            .sim()
            .entities()
            .get(victim)
            .is_none_or(|entity| !entity.lifecycle.object_alive());
        let wall_ms = completed * u64::from(crate::app::types::SIM_TICK_MS);
        if !fatal {
            let after = scene.sim().entities().get(victim).unwrap();
            assert!(
                after.health.current() <= hp_before,
                "no repair or artificial health reset"
            );
            if after.health.current() < hp_before {
                assert!(
                    hostile_latch(after),
                    "a real surviving hostile impact sets the latch"
                );
                survived = true;
            }
            audio.service_audio(
                wall_ms,
                false,
                Some(assets),
                Some(catalog),
                &mut scene.sim().presentation_main_draws(),
            );
            continue;
        }
        assert!(
            shots >= 2 && survived,
            "fatality must follow genuine full-HP volleys: {}",
            case.kind
        );
        assert_eq!(
            scene
                .sim()
                .houses()
                .get(&neutral)
                .unwrap()
                .stats()
                .buildings_lost(),
            lost_before + 1
        );
        assert_eq!(building_kills(scene.sim(), owner), kills_before + 1);
        let impacts: Vec<_> = output
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::AnimationStarted {
                    anim_id,
                    sound_id,
                    world,
                } if sound_id.eq_ignore_ascii_case("Explosion14") => {
                    let (rx, ry, _, _, _) = world.to_cell_sub_z();
                    if !(31..=33).contains(&rx) || !(85..=86).contains(&ry) {
                        return None; // Other actors elsewhere on the loaded Hills map.
                    }
                    let anim = scene.sim().anim(*anim_id)?;
                    ap_anims
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(scene.sim().resolve(anim.type_id())))
                        .then_some(*anim_id)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            impacts.len(),
            1,
            "one actual AP impact Anim, distinct from foundation Anims"
        );
        let fatal_frame = scene.sim().session().binary_frame();
        let coordinate_requests = audio
            .sfx_player
            .as_ref()
            .map_or_else(Vec::new, SfxPlayer::coordinate_sound_observation);
        let mut audio_main_advances = 0;
        if with_device {
            assert!(observing, "observation begins at naturally reached low HP");
            for pass in 1..=4 {
                let (_, draws) = trace_draws(|| {
                    audio.service_audio(
                        wall_ms + pass * 34,
                        false,
                        Some(assets),
                        Some(catalog),
                        &mut scene.sim().presentation_main_draws(),
                    )
                });
                audio_main_advances += draws.len();
            }
        }
        // Real next frame completes deferred retirement; no death cue may be
        // published twice. No lifecycle method or disposal setter is called.
        let (next, _) = frame(&mut scene, &mut audio, assets, catalog, listener, &local);
        assert!(scene.sim().entities().get(victim).is_none());
        assert!(
            !next
                .sound_events
                .iter()
                .any(|event| cue_id(event).is_some_and(|id| id.eq_ignore_ascii_case(case.cue)))
        );
        let prepared = audio.sfx_player.as_mut().map(|player| {
            player
                .finish_pcm_observation()
                .expect("armed observer")
                .into_json()
        });
        eprintln!(
            "{}: {} genuine shots, fatal frame{}, AP Anim{}; produced={:?}; copied={:?}",
            case.kind,
            shots,
            fatal_frame,
            impacts[0],
            output
                .sound_events
                .iter()
                .filter_map(cue_id)
                .collect::<Vec<_>>(),
            copied.iter().filter_map(cue_id).collect::<Vec<_>>()
        );
        return History {
            case,
            origin,
            fatal_frame,
            fatal_context: context,
            produced: output.sound_events,
            copied,
            impact_id: impacts[0],
            coordinate_requests,
            prepared,
            audio_main_advances,
        };
    }
    panic!("{} did not die through the actual MTNK attack", case.kind);
}

fn physical_histories(with_device: bool) -> Vec<History> {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("selected regression requires the physical retail install");
    let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, true);
    process
        .initialize_sources_if_needed(&assets)
        .expect("actual process audio source selection");
    let catalog = process
        .audio_catalog()
        .expect("physical ProcessAudioCatalog");
    assert!(catalog.index().is_some(), "real audio IDX/BAG input");
    for (cue, count) in [
        ("PowerPlantDie", 2),
        ("BuildingGenericDie", 6),
        ("Explosion14", 4),
    ] {
        let entry = catalog.sounds().get(cue).expect("physical registered cue");
        assert_eq!(entry.sounds.len(), count);
        assert_eq!(entry.fshift, (-10, 10));
    }
    assert_eq!(
        catalog.sounds().get("BuildingGenericDie").unwrap().vshift,
        10
    );
    // Collect both genuine fatal histories before any intended chronology
    // assertion. Raw714's first expected order failure cannot hide case two.
    CASES
        .into_iter()
        .map(|case| history(case, &root, &assets, catalog, with_device))
        .collect()
}

#[test]
#[ignore = "physical Hills/Rules/ART/TMP; genuine stock MTNK volleys, no audio device required"]
fn stock_building_death_cues_precede_the_fatal_ap_impact() {
    let histories = physical_histories(false);
    let mut wrong_order = Vec::new();
    for history in &histories {
        for (surface, events) in [("produced", &history.produced), ("copied", &history.copied)] {
            let death: Vec<_> = events
                .iter()
                .enumerate()
                .filter(|(_, event)| {
                    cue_id(event).is_some_and(|id| id.eq_ignore_ascii_case(history.case.cue))
                })
                .collect();
            assert_eq!(
                death.len(),
                1,
                "{} {surface}: one real death cue",
                history.case.kind
            );
            let impact = events
                .iter()
                .position(|event| {
                    matches!(event,
                SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == history.impact_id)
                })
                .expect("the identified actual AP impact traversed this boundary");
            assert!(
                cue_matches_coordinate(death[0].1, history.origin),
                "direct Building Location: {:?}",
                death[0].1
            );
            if death[0].0 >= impact {
                wrong_order.push((
                    history.case.kind,
                    surface,
                    history.fatal_frame,
                    death[0].0,
                    impact,
                ));
            }
        }
    }
    assert!(
        wrong_order.is_empty(),
        "native death request must precede its real fatal AP impact: {wrong_order:?}"
    );
    // Deliberately after the primary order assertion: raw EntityDied is accepted
    // for locating the omission, but a fixed global must retain750E20 semantics.
    for history in histories {
        let cue = history
            .produced
            .iter()
            .find(|event| cue_id(event).is_some_and(|id| id.eq_ignore_ascii_case(history.case.cue)))
            .unwrap();
        if history.case.kind == "GAPILE" {
            assert!(matches!(cue, SimSoundEvent::VocAtCoord { flags: 0, .. }));
        } else {
            assert!(matches!(
                cue,
                SimSoundEvent::VocAt {
                    audible_to: None,
                    ..
                }
            ));
        }
    }
}

#[test]
#[ignore = "physical stock audio device/IDX/BAG; real gameplay sample preparation, not audible/native timing parity"]
fn fatal_stock_cues_reach_actual_sample_preparation_without_actor_owned_handles() {
    // This independent positive control does not replace the primary order Red.
    let histories = physical_histories(true);
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let mut process = ProcessAssets::new(MediaArchiveMode::STOCK_DIGITAL, true);
    process.initialize_sources_if_needed(&assets).unwrap();
    let registry = process.audio_catalog().unwrap().sounds();
    for history in histories {
        let report = history.prepared.as_ref().unwrap();
        assert_eq!(
            report["truncated"], false,
            "event/action budget, distinct from optional PCM pulls"
        );
        let outputs = report["outputs"].as_array().unwrap();
        let submitted_in_fatal_context = |row: &serde_json::Value| {
            row["actions"].as_array().unwrap().iter().any(|action| {
                action["kind"] == "submitted"
                    && action["context"]["binary_frame"].as_u64()
                        == Some(u64::from(history.fatal_context))
            })
        };
        let death_rows: Vec<_> = outputs
            .iter()
            .filter(|row| {
                row["sound_id"]
                    .as_str()
                    .is_some_and(|id| id.eq_ignore_ascii_case(history.case.cue))
                    && submitted_in_fatal_context(row)
            })
            .collect();
        assert_eq!(death_rows.len(), 1, "one fatal-context dying cue");
        let death = death_rows[0];
        let impact_rows: Vec<_> = outputs
            .iter()
            .filter(|row| {
                row["owner_role"] == "positional"
                    && row["owner"].as_u64() == Some(history.impact_id)
                    && row["sound_id"]
                        .as_str()
                        .is_some_and(|id| id.eq_ignore_ascii_case("Explosion14"))
                    && submitted_in_fatal_context(row)
            })
            .collect();
        assert_eq!(
            impact_rows.len(),
            1,
            "the identified fatal AP Anim's SFX row"
        );
        let impact = impact_rows[0];
        for (cue, row) in [(history.case.cue, death), ("Explosion14", impact)] {
            let samples = &registry.get(cue).unwrap().sounds;
            assert!(
                row["source_sample_count"]
                    .as_u64()
                    .is_some_and(|count| count > 0)
                    && row["resolved_samples"]
                        .as_array()
                        .is_some_and(|resolved| !resolved.is_empty()
                            && resolved
                                .iter()
                                .all(|value| value.as_str().is_some_and(|name| {
                                    samples
                                        .iter()
                                        .any(|sample| sample.eq_ignore_ascii_case(name))
                                }))),
                "the same identified {cue} row has registered decoded preparation: {row:?}"
            );
        }
        // Decoded registered sources can only be prepared after the real
        // channel-acquired/PlayShifts path. Count actual service draws; do not
        // assert optimization-dependent backtrace spellings or numeric answers.
        assert!(
            history.audio_main_advances > 0,
            "real service retained the loaded Main owner"
        );
        assert!(
            death["submission"].as_u64().unwrap() < impact["submission"].as_u64().unwrap(),
            "the actual SFX pool received the dying cue before its fatal AP impact"
        );
        if history.case.kind == "GAPILE" {
            assert_eq!(death["owner_role"], "coordinate_request");
            let request_id = death["owner"]
                .as_u64()
                .expect("the dying cue owns this independent coordinate request");
            assert!(
                history
                    .coordinate_requests
                    .iter()
                    .any(|request| request["request_id"].as_u64() == Some(request_id)
                        && request["sound_id"]
                            .as_str()
                            .is_some_and(|id| id.eq_ignore_ascii_case(history.case.cue))
                        && request["flags"] == 0
                        && request["coordinate_leptons"]
                            == serde_json::json!([
                                history.origin.x,
                                history.origin.y,
                                history.origin.z
                            ])),
                "independent flags0 coordinate request survives the Building's UnInit"
            );
        } else {
            assert!(
                death["owner_role"].is_null() && death["owner"].is_null(),
                "7509E0 NULL handle"
            );
            assert!(!history.coordinate_requests.iter().any(|request| {
                request["sound_id"]
                    .as_str()
                    .is_some_and(|id| id.eq_ignore_ascii_case(history.case.cue))
            }));
        }
        // No PCM sample/pitch hash, natural completion or exact Main golden.
        eprintln!(
            "{} preparation: {} observed events, {} actual Main advances",
            history.case.kind,
            outputs.len(),
            history.audio_main_advances
        );
    }
}
