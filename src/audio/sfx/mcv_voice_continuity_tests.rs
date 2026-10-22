//! UnitDeploy739971 copies the existing voice handle/latches before UnInit.
//! Stock rules, SHP and audio; arena, aligned placement and service times are
//! supplied inputs. These are Rust continuation tests, not native PCM captures.
use super::*;
use crate::app::match_runtime::FrameClientBoundary;
use crate::app::presentation::building_anim::SoundPlaybackInputs;
use crate::app::process_assets::ProcessAssets;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::sim::command::Command;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, SimFrameOutput, SimSoundEvent, SoundBoundary, TickLane};

struct Scene {
    runtime: SimRuntime,
    assets: AssetManager,
    process: ProcessAssets,
    player: SfxPlayer,
    mcv: u64,
}

// Observe the actual inline app boundary without supplying any audio decision.
struct NoDrawBoundary<'a>(FrameClientBoundary<'a>);
impl SoundBoundary for NoDrawBoundary<'_> {
    fn consume(
        &self,
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        events: &[SimSoundEvent],
    ) {
        let before = sim.main_rng().native_state_hex();
        self.0.consume(sim, rules, events);
        assert_eq!(sim.main_rng().native_state_hex(), before);
    }
    fn delivered(&self) -> usize {
        self.0.delivered()
    }
}

fn listener() -> SpatialListener {
    SpatialListener {
        tactical_width: 640,
        tactical_height: 480,
        origin_x: 0.0,
        origin_y: 0.0,
        zoom: 1.0,
    }
}

fn scene() -> Option<Scene> {
    let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let Some(mut player) = SfxPlayer::new() else {
        eprintln!("MCV voice continuity needs an audio output device");
        return None;
    };
    player.set_volume(0.0);
    player
        .observe_pcm(PcmObservationConfig {
            sound_ids: vec!["MCVAlliedSelect".into()],
            max_events: 1,
            max_samples_per_event: 64,
        })
        .unwrap();
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
    assert!(retail.rules.has_buildup("GACNST"));
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x739971);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let rival = sim.interner().get("Russians").unwrap();
    sim.houses_mut_for_test()
        .get_mut(&rival)
        .unwrap()
        .set_is_human_for_test(true);
    sim.houses_mut_for_test()
        .get_mut(&rival)
        .unwrap()
        .set_current_iq_for_test(0);
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.spawn_object("GAPOWR", "Russians", 3, 3, 0, &rules)
        .unwrap();
    sim.spawn_object("GAPOWR", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 12, 12, 128, &rules)
        .unwrap();
    let terrain_template = sim.resolved_terrain().cloned();
    let mut process = ProcessAssets::new(
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        true,
    );
    process.initialize_sources_if_needed(&assets).unwrap();
    Some(Scene {
        runtime: SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        },
        assets,
        process,
        player,
        mcv,
    })
}

impl Scene {
    fn command(&mut self, command: Command) {
        assert!(self.runtime.simulation.apply_command(
            "Americans",
            &command,
            Some(&self.runtime.resources.rules),
        ));
    }

    fn select(&mut self) {
        self.command(Command::Stop {
            entity_id: self.mcv,
        });
        self.command(Command::Select {
            entity_ids: vec![self.mcv],
            additive: false,
        });
        // The committed snapshot owns selected bits; the app's input owner
        // separately produces VoiceSelect through this canonical reader.
        let voice = crate::app::match_runtime::sound_dispatch::selection_voice_event(
            &mut self.runtime.simulation,
            &self.runtime.resources.rules,
            self.mcv,
            true,
        )
        .expect("stock human AMCV selection acknowledgement");
        let expected = voice.sound_id().to_ascii_uppercase();
        assert!(
            self.process
                .audio_catalog()
                .unwrap()
                .sounds()
                .get(&expected)
                .is_some(),
            "the input producer's registered voice must exist in the process audio catalog"
        );
        let playback = SoundPlaybackInputs {
            assets: &self.assets,
            catalog: self.process.audio_catalog().unwrap(),
            listener: listener(),
        };
        playback.consume(
            &mut self.player,
            &[voice],
            Some(&self.runtime.simulation),
            Some("Americans"),
        );
        assert_eq!(
            self.player.voice_queue.pending_for(self.mcv),
            Some(expected.as_str())
        );
    }

    fn source_event(&mut self) -> EventId {
        assert!(
            self.runtime.simulation.logic_order().contains(&self.mcv),
            "placed AMCV must have joined the actual Logic vector"
        );
        let mut facts = Vec::new();
        for _ in 0..4 {
            let output = self.frame();
            let reached = output.sound_events.iter().any(|event| {
                matches!(event,
                SimSoundEvent::UnitVoiceVisit { owner } if *owner == self.mcv)
            });
            facts.extend(output.sound_events);
            if reached {
                // Do not retry a failed sound admission after a reached head.
                return self.player.arbiter.validate_loop_handle(HandleOwner::UnitVoice(self.mcv))
                    .unwrap_or_else(|| panic!("actual AMCV voice head did not bind its registered event; pending={:?}, playing={:?}, facts={facts:?}",
                        self.player.voice_queue.pending_for(self.mcv), self.player.voice_queue.playing_for(self.mcv)));
            }
        }
        panic!(
            "AMCV did not reach its voice head in four ordinary visits; logic={:?}, pending={:?}, facts={facts:?}",
            self.runtime.simulation.logic_order(),
            self.player.voice_queue.pending_for(self.mcv)
        );
    }

    fn frame(&mut self) -> SimFrameOutput {
        self.runtime
            .simulation
            .prepare_unit_voice_visits(self.player.pending_unit_voice_owners());
        let mut queue = SoundEventQueue::new();
        let mut radar = |_, _, _: &crate::rules::ruleset::RuleSet| false;
        let playback = SoundPlaybackInputs {
            assets: &self.assets,
            catalog: self.process.audio_catalog().unwrap(),
            listener: listener(),
        };
        let boundary = NoDrawBoundary(FrameClientBoundary::new(
            Some((&mut self.player, playback)),
            Some("Americans"),
            &mut queue,
            &mut radar,
        ));
        let output = self
            .runtime
            .advance_frame(
                &[],
                33,
                TickLane::Ordinary,
                FrameEffects::for_sound(&boundary),
            )
            .unwrap();
        // Consume only any suffix not already delivered inline, as the app does.
        boundary.consume(
            &self.runtime.simulation,
            &self.runtime.resources.rules,
            &output.sound_events,
        );
        drop(boundary);
        assert!(queue.is_empty());
        output
    }

    fn service(&mut self, milliseconds: u64) {
        let catalog = self.process.audio_catalog().unwrap();
        let mut draw = super::unit_voice_tests::retained_draws(self.runtime.simulation.main_rng());
        self.player.service_events(
            milliseconds,
            catalog.sounds(),
            catalog.eva(),
            &self.assets,
            catalog.index(),
            &mut draw,
        );
        // Existing unit_voice_lifecycle service boundary: hold actual Players,
        // rather than treating game ticks or silence as sample progression.
        for output in self.player.live.values() {
            output.player.pause();
        }
    }

    fn deploy(&mut self) -> (u64, SimFrameOutput) {
        let actor = self.runtime.simulation.entities().get(self.mcv).unwrap();
        assert_eq!(
            crate::sim::movement::motion_query::is_moving(actor),
            Some(false)
        );
        assert_eq!(
            actor.body_facing_byte(self.runtime.simulation.session().binary_frame()),
            128
        );
        self.command(Command::DeployMcv {
            entity_id: self.mcv,
        });
        for _ in 0..8 {
            let output = self.frame();
            if let Some(yard) = self.runtime.simulation.entities().values().find(|entity| {
                self.runtime
                    .simulation
                    .interner()
                    .resolve(entity.type_ref())
                    == "GACNST"
            }) {
                assert!(self.runtime.simulation.entities().get(self.mcv).is_none());
                return (yard.stable_id(), output);
            }
        }
        panic!("aligned stock AMCV must convert through scheduled Unload");
    }
}

#[test]
fn stock_deploy_keeps_playing_output_and_held_move_voice_then_new_owner_destroys_it() {
    let Some(mut scene) = scene() else { return };
    scene.select();
    let event = scene.source_event();
    scene.service(1000);
    let token = scene.player.arbiter.event_token(event).unwrap();
    let before = scene.player.arbiter.native_event_snapshot(event).unwrap();
    let position = scene.player.live[&event].player.get_pos();
    assert_eq!(scene.player.live[&event].token, Some(token));
    assert_eq!(
        scene.player.voice_queue.playing_for(scene.mcv),
        Some("MCVALLIEDSELECT")
    );

    // Accepted Move then Stop before the next receiver visit leaves a real
    // move acknowledgement behind the live selection line. Input latency is
    // supplied; the stock VoiceMove reader and SFX latch remain production.
    scene.command(Command::Move {
        entity_id: scene.mcv,
        target_rx: 12,
        target_ry: 13,
    });
    let sound = scene
        .runtime
        .simulation
        .order_voice_request(
            &scene.runtime.resources.rules,
            scene.mcv,
            "VoiceMove",
            None,
            true,
        )
        .unwrap()
        .to_owned();
    let request = GameSoundEvent::UnitMoveOrder {
        speaker_id: scene.mcv,
        sound_id: sound,
    };
    // No replacement sound or transfer fact is authored.
    let playback = SoundPlaybackInputs {
        assets: &scene.assets,
        catalog: scene.process.audio_catalog().unwrap(),
        listener: listener(),
    };
    playback.consume(
        &mut scene.player,
        &[request],
        Some(&scene.runtime.simulation),
        Some("Americans"),
    );
    scene.command(Command::Stop {
        entity_id: scene.mcv,
    });
    let (yard, output) = scene.deploy();
    assert_eq!(
        scene
            .player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(yard)),
        Some(event)
    );
    assert_eq!(scene.player.arbiter.event_token(event), Some(token));
    assert_eq!(
        scene.player.arbiter.native_event_snapshot(event),
        Some(before)
    );
    assert_eq!(scene.player.live[&event].token, Some(token));
    assert_eq!(scene.player.live[&event].player.get_pos(), position);
    assert_eq!(
        scene.player.voice_queue.playing_for(yard),
        Some("MCVALLIEDSELECT")
    );
    assert_eq!(
        scene.player.voice_queue.pending_for(yard),
        Some("MCVALLIEDMOVE")
    );
    assert!(scene.player.voice_queue.pending_for(scene.mcv).is_none());
    assert!(scene.player.voice_queue.playing_for(scene.mcv).is_none());
    assert!(output.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::UnitVoiceVisit { owner } if *owner == yard)));
    assert!(output.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::UnitVoiceDestroyed { owner } if *owner == scene.mcv)));
    let report = scene.player.finish_pcm_observation().unwrap();
    assert_eq!(report.outputs.len(), 1);
    let observed = &report.outputs[0];
    assert_eq!(
        (observed.event, observed.event_serial, observed.owner),
        (event.0, token.serial, Some(yard))
    );
    assert_eq!(
        observed.resolved_samples.len(),
        1,
        "no second sample decode"
    );
    assert!(observed.source_sample_count > 0);
    assert!(
        !observed
            .actions
            .iter()
            .any(|action| matches!(action.kind, "stopped" | "completed"))
    );

    // Real replacement UnInit still owns the ordinary hard-stop cleanup.
    scene.runtime.simulation.uninit_with_context(
        yard,
        crate::sim::world::UninitContext::with_rules(&scene.runtime.resources.rules),
    );
    scene.frame();
    assert!(scene.runtime.simulation.entities().get(yard).is_none());
    assert!(!scene.player.arbiter.is_live_token(token));
    assert!(!scene.player.live.contains_key(&event));
    assert!(scene.player.voice_queue.pending_for(yard).is_none());
    scene.player.stop_all();
}

#[test]
fn stock_deploy_preserves_queued_event_until_its_first_physical_sample_service() {
    let Some(mut scene) = scene() else { return };
    scene.select();
    let event = scene.source_event();
    let token = scene.player.arbiter.event_token(event).unwrap();
    assert_eq!(scene.player.pending[&event].token, token);
    assert!(scene.player.pending[&event].loaded.is_none());
    assert!(!scene.player.live.contains_key(&event));
    let (yard, _) = scene.deploy();
    assert_eq!(
        scene
            .player
            .arbiter
            .validate_loop_handle(HandleOwner::UnitVoice(yard)),
        Some(event)
    );
    assert_eq!(scene.player.arbiter.event_token(event), Some(token));
    assert_eq!(scene.player.pending[&event].token, token);
    assert!(scene.player.pending[&event].loaded.is_none());
    scene.service(1000);
    assert_eq!(scene.player.live[&event].token, Some(token));
    let report = scene.player.finish_pcm_observation().unwrap();
    assert_eq!(report.outputs.len(), 1);
    let observed = &report.outputs[0];
    assert_eq!(
        (observed.event, observed.event_serial, observed.owner),
        (event.0, token.serial, Some(yard))
    );
    assert_eq!(observed.resolved_samples.len(), 1);
    assert!(observed.source_sample_count > 0);
    assert!(
        !observed
            .actions
            .iter()
            .any(|action| matches!(action.kind, "stopped" | "completed"))
    );
    scene.player.stop_all();
}
