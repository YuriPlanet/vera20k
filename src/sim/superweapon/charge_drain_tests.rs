//! Authored UseChargeDrain histories through retained House instances and the
//! real ForceShield leaf. The flat map, image frame count and time jumps are
//! fixture inputs; this does not claim original whole-match execution.

use super::*;
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::sim::command::Command;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::superweapon::chronosphere_tests::{click, step};
use crate::sim::superweapon::invulnerability::InvulnKind;
use crate::util::save_codec;

const SHIELD: &str = "DrainShield";

fn layers() -> RulesLayerStack {
    RulesLayerStack::new(crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n\
         [BuildingTypes]\n0=PROVIDER\n1=TARGET\n\
         [PROVIDER]\nStrength=1000\nTurret=no\nSight=0\nSuperWeapon=DrainShield\n\
         [TARGET]\nStrength=1000\nTurret=no\nSight=0\n\
         [SuperWeaponTypes]\n0=DrainShield\n\
         [DrainShield]\nType=ForceShield\nAction=ForceShield\nRechargeTime=1\n\
         UseChargeDrain=yes\nIsPowered=no\nShowTimer=yes\nFlashSidebarTabFrames=120\n\
         [General]\nForceShieldRadius=4\nForceShieldDuration=500\n\
         ForceShieldBlackoutDuration=1000\nForceShieldPlayFadeSoundTime=75\n\
         ForceShieldInvokeAnim=INVOKE\n[Animations]\n0=INVOKE\n",
    ))
}

fn art() -> IniFile {
    IniFile::from_str(
        "[PROVIDER]\nFoundation=1x1\n[TARGET]\nFoundation=1x1\n\
         [INVOKE]\nImage=INVOKE\nRate=900\n",
    )
}

fn process(layers: &RulesLayerStack) -> (ProcessedRulesLayers, RuleSet) {
    let processed = layers.process_with_fixed_art(&art()).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    bind_resources(&mut rules, &processed);
    (processed, rules)
}

fn bind_resources(rules: &mut RuleSet, processed: &ProcessedRulesLayers) {
    // Process reads the fixed ART source; runtime image facts use its actual
    // installed registry on both the initial and same-build restored rules.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art()));
    // Explicit fixed SOUND prior, resolved by the same catalog-binding owner
    // as NativeRulesProcessOwner::build_rules; no audio device is modeled.
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=ForceShieldStarting\n",
    ));
    rules.bind_type_sound_references(processed.ini(), &sounds);
    // The renderer normally binds this image fact; no SHP/device is loaded.
    rules.bind_anim_frame_count_for_test("INVOKE", 2000);
}

fn world(rules: &RuleSet) -> (Simulation, InternedId, InternedId, InternedId, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.session.set_binary_frame_for_test(4100);
    sim.session.set_map_name_for_test("DRAIN.MAP".into());
    let local = sim.interner.get("Americans").unwrap();
    let remote = sim.interner.get("Russians").unwrap();
    sim.session.set_current_house_for_test(Some(local));
    sim.houses
        .get_mut(&local)
        .unwrap()
        .set_player_control_for_test(true);
    for owner in [local, remote] {
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_country_for_test(Some(owner));
        construct_house_supers(&mut sim, rules, owner, false);
    }
    let provider = sim
        .spawn_object("PROVIDER", "Americans", 10, 10, 0, rules)
        .unwrap();
    sim.spawn_object("TARGET", "Russians", 20, 20, 0, rules)
        .unwrap();
    sim.assess_power_for_test(rules);
    sim.recheck_house_tech_tree(local, rules);
    let id = sim.interner.get(SHIELD).unwrap();
    (sim, local, remote, id, provider)
}

fn display(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    stage: i32,
    status: SuperWeaponStatus,
    can_fire: bool,
) {
    let view = superweapon_views_for_owner(sim, rules, &owner)
        .pop()
        .unwrap();
    assert_eq!(
        view.display,
        SuperWeaponDisplay {
            stage,
            draw_progress: true,
            status: Some(status)
        }
    );
    assert_eq!(view.can_fire, can_fire);
    assert!(
        !view.should_flash_tab,
        "AI drain readiness retains constructor ReadyFrame=-100"
    );
}

fn invoke_count(sim: &Simulation) -> usize {
    let image = sim.interner.get("INVOKE").unwrap();
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| anim.type_id() == image)
        .count()
}

fn launches(sim: &Simulation) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
        .count()
}

#[test]
fn authored_drain_provider_commands_effects_and_saved_phase_follow_native_conversions() {
    let mut sources = layers();
    assert_eq!(
        process(&sources).1.general.charge_to_drain_ratio.bits(),
        3.0_f64.to_bits()
    );
    sources.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[General]\nChargeToDrainRatio=.5\n"),
    );
    assert_eq!(
        process(&sources).1.general.charge_to_drain_ratio.bits(),
        0.5_f64.to_bits()
    );
    sources.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nChargeToDrainRatio=.333\n"),
    );
    sources.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[General]\nChargeToDrainRatio=\nForceShieldRadius=4\n\
             [DrainShield]\nStartSound=ForceShieldStarting\n",
        ),
    );
    let (processed, rules) = process(&sources);
    assert_eq!(
        rules.super_weapon(SHIELD).unwrap().start_sound.as_deref(),
        Some("ForceShieldStarting"),
        "the authored cue must resolve through the supplied fixed SOUND catalog"
    );
    // ReadDouble528558 scans binary32 before widening; later empty/omitted
    // physical keys preserve this retained scalar, not constructor3.0.
    assert_eq!(
        rules.general.charge_to_drain_ratio.bits(),
        0x3fd5_4fdf_4000_0000
    );
    let (mut sim, owner, _, id, provider) = world(&rules);
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(
        (
            instance.charge_drain_state(),
            instance.charge_start_tick(),
            instance.charge_duration()
        ),
        (0, 4100, 900)
    );
    display(&sim, &rules, owner, 0, SuperWeaponStatus::Charging, false);
    sim.sound_events.clear_for_test();
    // House AI and queued commands observe the incoming native frame N;
    // run_late_region commits N+1 only after those visits have completed.
    sim.session.set_binary_frame_for_test(5000);
    step(&mut sim, &rules); // scheduled House AI6CBCA0, no forced charge.
    assert_eq!(sim.session.binary_frame(), 5001);
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::SuperWeaponReady { .. }))
    );
    assert_eq!(sim.super_weapons[&owner][&id].ready_tick(), -100);
    display(&sim, &rules, owner, 53, SuperWeaponStatus::Ready, true);

    // SPECIAL_PLACE at the ordinary frame tail, with no ready/state/timer
    // override. Native6CB9D9 converts (900-0)*q before actual case10.
    struct ActivationBoundary {
        delivered: std::cell::Cell<usize>,
        activations: std::cell::Cell<usize>,
        owner: InternedId,
        id: InternedId,
        provider: u64,
    }
    impl crate::sim::world::SoundBoundary for ActivationBoundary {
        fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]) {
            for event in &events[self.delivered.get()..] {
                if let SimSoundEvent::SuperWeaponActivation { sw_type, world } = event
                    && *sw_type == self.id
                {
                    // Launch6CD16B..6CD176: the StartSound callback sees the
                    // converted phase and armed fade before blackout6CD18B,
                    // curtain6CD28B and the final local selection/EVA fact.
                    assert_eq!(
                        rules.super_weapon(SHIELD).unwrap().start_sound.as_deref(),
                        Some("ForceShieldStarting")
                    );
                    assert_eq!([world.x, world.y, world.z], [2688, 2688, 0]);
                    let instance = &sim.super_weapons[&self.owner][&self.id];
                    assert_eq!(instance.charge_drain_state(), 2);
                    assert_eq!(instance.charge_duration(), 299);
                    assert_eq!(instance.fade(), (425, [2688, 2688, 0]));
                    assert_eq!(
                        sim.power_states[&self.owner]
                            .blackout_remaining(sim.session.binary_frame()),
                        0
                    );
                    assert!(
                        sim.substrate
                            .entities
                            .get(self.provider)
                            .unwrap()
                            .invulnerability
                            .is_none()
                    );
                    assert!(!events.iter().any(|event| matches!(event,
                        SimSoundEvent::SuperWeaponLaunched { owner, sw_type, .. }
                            if *owner == self.owner && *sw_type == self.id)));
                    self.activations.set(self.activations.get() + 1);
                }
            }
            self.delivered.set(events.len());
        }

        fn delivered(&self) -> usize {
            self.delivered.get()
        }
    }
    let boundary = ActivationBoundary {
        delivered: std::cell::Cell::new(0),
        activations: std::cell::Cell::new(0),
        owner,
        id,
        provider,
    };
    sim.queue_command(crate::sim::command::CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::LaunchSuperWeapon {
            sw_type_id: id,
            target_rx: 10,
            target_ry: 10,
        },
    ));
    let commands = sim.take_due_commands();
    let output = sim
        .advance_app_frame(
            &commands,
            &rules,
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::for_sound(&boundary),
        )
        .unwrap();
    assert_eq!(boundary.activations.get(), 1);
    assert_eq!(sim.session.binary_frame(), 5002);
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(
        (
            instance.charge_drain_state(),
            instance.charge_start_tick(),
            instance.charge_duration()
        ),
        (2, 5001, 299)
    );
    assert!(instance.is_ready());
    assert_eq!(instance.fade(), (425, [2688, 2688, 0]));
    // The app transaction transfers the final facts to its output; the
    // callback above consumed only the prefix available at StartSound.
    assert_eq!(
        output
            .sound_events
            .iter()
            .filter(|event| matches!(event,
            SimSoundEvent::SuperWeaponLaunched { owner: actual, sw_type, .. }
                if *actual == owner && *sw_type == id))
            .count(),
        1
    );
    assert_eq!(invoke_count(&sim), 1);
    let curtain = sim
        .substrate
        .entities
        .get(provider)
        .unwrap()
        .invulnerability
        .as_ref()
        .unwrap();
    assert_eq!(curtain.kind(), InvulnKind::ForceShield);
    assert_eq!(curtain.timer().remaining(5001), 500);
    assert_eq!(sim.power_states[&owner].blackout_remaining(5001), 1000);
    display(&sim, &rules, owner, 53, SuperWeaponStatus::Active, true);

    // Same-build resource wire and snapshot preserve the active phase. Image
    // binding and flat terrain are external resource facts rebound on load.
    sim.session.set_binary_frame_for_test(5100);
    let expected = save_codec::serialize(&sim.super_weapons).unwrap();
    let resources = save_codec::serialize(&processed).unwrap();
    let retained: ProcessedRulesLayers = save_codec::deserialize(&resources).unwrap();
    let mut rebuilt = RuleSet::from_processed_rules(&retained).unwrap();
    bind_resources(&mut rebuilt, &retained);
    assert_eq!(
        rebuilt.general.charge_to_drain_ratio.bits(),
        rules.general.charge_to_drain_ratio.bits()
    );
    assert_eq!(
        rebuilt.simulation_config_hash(),
        rules.simulation_config_hash()
    );
    let bytes =
        GameSnapshot::save_validated(&sim, 17, rules.simulation_config_hash(), "draining", 0);
    let mut loaded =
        GameSnapshot::load_validated(&bytes, 17, rebuilt.simulation_config_hash(), "DRAIN.MAP")
            .unwrap()
            .sim;
    loaded.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    crate::sim::arena_fixture::flat_ground(&mut loaded, &rebuilt);
    loaded.resolve_type_handles(&rebuilt);
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(
        save_codec::serialize(&loaded.super_weapons).unwrap(),
        expected
    );
    assert_eq!(
        loaded.super_weapons[&owner][&id].charge_remaining(5100),
        200
    );
    sim = loaded;
    let rules = rebuilt;
    let anims = invoke_count(&sim);
    sim.sound_events.clear_for_test();
    sim.session.set_binary_frame_for_test(5101);
    click(&mut sim, &rules, owner, SHIELD, (10, 10));
    // Native6CB995..6CB9A9: ftol(900 - 199/q), not a cancellation.
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(
        (
            instance.charge_drain_state(),
            instance.charge_start_tick(),
            instance.charge_duration()
        ),
        (1, 5101, 302)
    );
    assert_eq!(launches(&sim), 0);
    assert_eq!(invoke_count(&sim), anims);
    assert_eq!(
        sim.substrate
            .entities
            .get(provider)
            .unwrap()
            .invulnerability
            .as_ref()
            .unwrap()
            .timer()
            .remaining(5101),
        400
    );
    assert_eq!(sim.power_states[&owner].blackout_remaining(5101), 900);
    display(&sim, &rules, owner, 35, SuperWeaponStatus::Ready, true);
    sim.session.set_binary_frame_for_test(5151);
    click(&mut sim, &rules, owner, SHIELD, (10, 10));
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(
        (
            instance.charge_drain_state(),
            instance.charge_start_tick(),
            instance.charge_duration()
        ),
        (2, 5151, 215)
    );
    assert_eq!(launches(&sim), 1);
    assert_eq!(invoke_count(&sim), anims + 1);
    assert_eq!(sim.power_states[&owner].blackout_remaining(5151), 1000);

    sim.sound_events.clear_for_test();
    sim.session.set_binary_frame_for_test(5366);
    step(&mut sim, &rules);
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(
        (
            instance.charge_drain_state(),
            instance.charge_start_tick(),
            instance.charge_duration()
        ),
        (0, 5366, 900)
    );
    assert!(
        instance.is_ready(),
        "AI6CBD86 restarts charge without clearing rawReady"
    );
    display(&sim, &rules, owner, 0, SuperWeaponStatus::Charging, false);
    assert_eq!(
        sim.substrate
            .entities
            .get(provider)
            .unwrap()
            .invulnerability
            .as_ref()
            .unwrap()
            .timer()
            .remaining(5366),
        285
    );
    assert_eq!(sim.power_states[&owner].blackout_remaining(5366), 785);
    sim.session.set_binary_frame_for_test(6266);
    step(&mut sim, &rules);
    let instance = &sim.super_weapons[&owner][&id];
    assert_eq!(instance.charge_drain_state(), 1);
    assert!(instance.is_ready());
    assert_eq!(instance.ready_tick(), -100);
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::SuperWeaponReady { .. }))
    );
    display(&sim, &rules, owner, 53, SuperWeaponStatus::Ready, true);
}

#[test]
fn remote_house_human_selector_and_reset_keep_drain_one_time_and_leaf_ready_guard() {
    let mut sources = layers();
    sources.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nChargeToDrainRatio=.333\n"),
    );
    let (_, rules) = process(&sources);
    // Actual map action33 gives one-time Grant->ForcedCharge6CC080. The
    // remote owner's human byte, not current-house identity, selects6CB93D.
    for human in [true, false] {
        let (mut sim, local, remote, id, _) = world(&rules);
        sim.houses
            .get_mut(&remote)
            .unwrap()
            .set_is_human_for_test(human);
        sim.houses
            .get_mut(&remote)
            .unwrap()
            .set_player_control_for_test(false);
        assert_eq!(sim.session.current_house(), Some(local));
        let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Triggers]\nGRANT=Russians,<none>,Grant,0,1,1,1,0\n[Events]\nGRANT=1,8,0,0\n[Actions]\nGRANT=1,33,11,0,0,0,0,0,A\n[Tags]\nGRANTTAG=0,Grant,GRANT\n").unwrap();
        sim.initialize_trigger_definitions(
            &map,
            &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
            None,
            None,
        );
        sim.classify_map_triggers(&rules);
        sim.advance_triggers(&rules, None, FrameEffects::default());
        let instance = &sim.super_weapons[&remote][&id];
        assert!(instance.is_active() && instance.is_ready() && instance.one_time);
        assert_eq!(instance.charge_drain_state(), 1);
        let original_timer = (instance.charge_start_tick(), instance.charge_duration());
        sim.sound_events.clear_for_test();
        assert!(sim.apply_command_with_overlays(
            "Russians",
            &Command::LaunchSuperWeapon {
                sw_type_id: id,
                target_rx: 20,
                target_ry: 20
            },
            &rules,
            None,
            FrameEffects::default()
        ));
        let instance = &sim.super_weapons[&remote][&id];
        assert!(instance.is_active() && instance.is_ready() && instance.one_time);
        if human {
            assert_eq!(
                (
                    instance.charge_drain_state(),
                    instance.charge_start_tick(),
                    instance.charge_duration()
                ),
                (2, 4100, 299)
            );
            sim.session.set_binary_frame_for_test(4200);
            assert!(!sim.apply_command_with_overlays(
                "Russians",
                &Command::LaunchSuperWeapon {
                    sw_type_id: id,
                    target_rx: 20,
                    target_ry: 20
                },
                &rules,
                None,
                FrameEffects::default()
            ));
            assert_eq!(sim.super_weapons[&remote][&id].charge_drain_state(), 1);
        } else {
            assert_eq!(instance.charge_drain_state(), 1);
            assert_eq!(
                (instance.charge_start_tick(), instance.charge_duration()),
                original_timer
            );
        }
        assert_eq!(launches(&sim), 1);
        assert_eq!(invoke_count(&sim), 1);
        // Reset6CE0B0 retains state1 while clearing rawReady. A subsequent
        // drain click still converts the timer; Launch case10's6CD077 guard
        // suppresses every effect, including fade/selection/audio output.
        reset_super(
            &sim.interner,
            &sim.session,
            &mut sim.substrate,
            &mut sim.sound_events,
            &mut sim.super_weapons,
            &rules,
            remote,
            SHIELD,
        );
        let before = &sim.super_weapons[&remote][&id];
        assert_eq!(before.charge_drain_state(), 1);
        assert!(!before.is_ready());
        let fade = before.fade();
        let blackout = sim.power_states[&remote].blackout_remaining(sim.session.binary_frame());
        let anims = invoke_count(&sim);
        sim.sound_events.clear_for_test();
        sim.apply_command_with_overlays(
            "Russians",
            &Command::LaunchSuperWeapon {
                sw_type_id: id,
                target_rx: 20,
                target_ry: 20,
            },
            &rules,
            None,
            FrameEffects::default(),
        );
        let instance = &sim.super_weapons[&remote][&id];
        assert_eq!(instance.charge_drain_state(), if human { 2 } else { 1 });
        assert_eq!(instance.charge_duration(), if human { 0 } else { 900 });
        assert!(instance.one_time && instance.is_active() && !instance.is_ready());
        assert_eq!(instance.fade(), fade);
        assert_eq!(
            sim.power_states[&remote].blackout_remaining(sim.session.binary_frame()),
            blackout
        );
        assert_eq!(invoke_count(&sim), anims);
        assert!(sim.sound_events.is_empty());
    }
}
