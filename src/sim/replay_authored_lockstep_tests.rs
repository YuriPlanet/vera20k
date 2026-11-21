use super::*;
use crate::headless_scenario::{HeadlessScenario, battle_session, load_with_launch_and_ini};
use crate::map::source::test_support::TestDirectory;
use crate::sim::house_state::HouseOutcomeKind;
use crate::sim::scenario_bootstrap::MatchLaunchDescriptor;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, TickLane, TickResult};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, SkirmishLaunchMode, SkirmishLaunchSession,
};
use crate::util::ini_writer::set_ini_values;
use std::num::{NonZeroU8, NonZeroU64};

const TICK_MS: u32 = 22;
const SEED: u32 = 19;
const BATTLE_MAP: &str = "XMP03T4.MAP";

#[test]
fn map_houses_blueprint_round_trips_difficulty_without_recording_local_source() {
    use crate::map::campaign_manifest::CampaignManifest;
    use crate::sim::scenario_bootstrap::AuthoredCampaignStart;
    use crate::sim::scenario_session::{LocalViewIdentity, ScenarioDescriptor, ScenarioSession};

    let directory = TestDirectory::new("campaign-blueprint");
    directory.write(
        "campaign.ini",
        b"[Stage Opening]\nMap=OPEN.MAP\n[Roles Opening]\nLead=Americans\nWing=Americans\n[Grants Opening]\nWing.Russians=UnitsOnly",
    );
    let manifest = CampaignManifest::load(&directory.path().join("campaign.ini")).unwrap();
    let launch =
        AuthoredCampaignStart::from_stage(&manifest.stages[0], 0, 2, Default::default()).unwrap();
    let mut session = ScenarioSession::from_descriptor(&ScenarioDescriptor::default());
    session.install_admission(ScenarioGameMode::Wol, None, launch.rule_policy());
    session.install_shared_house_sources(vec![0, 0], Some(1));
    session
        .initialize_campaign_startup(
            launch.difficulty(),
            launch.mission_counter(),
            None,
            0,
            crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            None,
        )
        .unwrap();
    let blueprint = MatchStartBlueprint::capture(
        ReplayLaunchInput::MapHouses(&launch),
        &session,
        Default::default(),
    )
    .unwrap();
    let encoded = serde_json::to_value(&blueprint).unwrap();
    assert_eq!(encoded["kind"], "campaign");
    assert_eq!(encoded["map_houses"]["difficulty"], 2);
    let decoded: MatchStartBlueprint = serde_json::from_value(encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.map_houses(), Some(&launch));
    assert_eq!(
        decoded.admitted().local_view_identity(),
        &LocalViewIdentity::default()
    );
}

#[test]
#[ignore = "slow; real campaign maps, player-save restoration and independent replay construction"]
fn map_house_campaign_manifest_loads_carry_and_replays_each_frame() {
    use crate::headless_scenario::{check_campaign_stage, load_campaign_stage};
    use crate::map::campaign_manifest::{CampaignManifest, ManifestError};
    use crate::sim::control_authority::ControlScope;
    use crate::sim::scenario_bootstrap::{AuthoredCampaignLaunch, AuthoredCampaignStart};
    use crate::sim::snapshot::{GameSnapshot, PlayerSaveCells};
    use crate::sim::world::SnapshotProcessState;

    let (retail, assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("retail RA2_DIR");
    let directory = TestDirectory::new("authored-map-house-campaign");
    let map_name = "ALL01UMD.MAP";
    let map = assets.resolve_ref(map_name).unwrap().bytes;
    let map = set_ini_values(
        map,
        "Basic",
        &[("CarryOverMoney", "100%"), ("CarryOverCap", "-1")],
    );
    directory.write(map_name, &map);
    let map_path = directory.path().join(map_name);
    let map_path = map_path.to_str().unwrap();
    let map_digest = crate::util::sha256::sha256_hex(&map);
    let manifest = format!(
        r#"
[Stage Opening]
Map={map_name}
MapSha256={map_digest}
[Roles Opening]
Lead=Americans
Wing=Americans
[Grants Opening]
Lead.Russians=FullControl
Wing.Russians=UnitsOnly
Lead.YuriCountry=UnitsOnly
[Rules Opening]
AllowsManualAlliances=Yes
[CarryOver Opening]
House.Americans=Credits,Difficulty
House.Russians=Credits,Difficulty
[Routes Opening]
Win=Finale
[Stage Finale]
Map={map_name}
[Roles Finale]
Lead=Americans
Wing=Americans
[Grants Finale]
Lead.Russians=FullControl
Wing.Russians=UnitsOnly
Lead.YuriCountry=UnitsOnly
[Rules Finale]
AllowsManualAlliances=Yes
"#
    );
    directory.write("campaign.ini", manifest.as_bytes());
    let manifest_path = directory.path().join("campaign.ini");
    let invalid_manifest = manifest.replace("House.Russians=Credits", "House.Missing=Credits");
    let invalid_path = directory.path().join("invalid.ini");
    directory.write("invalid.ini", invalid_manifest.as_bytes());
    let error = check_campaign_stage(&retail, &invalid_path, "Opening", 3)
        .unwrap_err()
        .downcast::<ManifestError>()
        .unwrap();
    assert_eq!(error.file, invalid_path);
    assert_eq!(error.section, "CarryOver Opening");
    assert_eq!(error.key, "House.Missing");
    assert_eq!(
        error.line,
        invalid_manifest
            .lines()
            .position(|line| line.starts_with("House.Missing="))
            .unwrap()
            + 1
    );
    assert!(!error.hint.is_empty());
    let duplicate = set_ini_values(&map, "Houses", &[("999", "americans")]);
    directory.write("Duplicate.map", &duplicate);
    let duplicate_path = directory.path().join("duplicate.ini");
    directory.write("duplicate.ini", b"[Stage Opening]\nMap=Duplicate.map");
    let error = check_campaign_stage(&retail, &duplicate_path, "Opening", 1)
        .unwrap_err()
        .downcast::<ManifestError>()
        .unwrap();
    assert_eq!((error.key.as_str(), error.line), ("Map", 2));
    assert!(error.problem.contains("repeats the name"));

    let manifest = CampaignManifest::load(&manifest_path).unwrap();
    let report = check_campaign_stage(&retail, &manifest_path, "opening", 3).unwrap();
    assert_eq!(report.checked_frames, 3);
    assert_eq!(report.map_sha256, map_digest);
    assert_eq!(report.manifest_sha256, manifest.digest);

    let stage = manifest.stage("Opening").unwrap();
    let opening = AuthoredCampaignStart::from_stage(stage, 0, 1, Default::default()).unwrap();
    fn client(start: &AuthoredCampaignStart) -> AuthoredCampaignLaunch<'_> {
        AuthoredCampaignLaunch {
            start,
            mode: ScenarioGameMode::Wol,
            local_source: Some(1),
        }
    }
    let mut previous =
        load_campaign_stage(&retail, map_path, SEED, client(&opening), None, None, None).unwrap();
    let american = previous.sim().interner().get("Americans").unwrap();
    let soviet = previous.sim().interner().get("Russians").unwrap();
    let yuri = previous.sim().interner().get("YuriCountry").unwrap();
    let mut balances = Vec::new();
    for (id, credit) in [(american, 123), (soviet, 456)] {
        let economy = previous
            .runtime
            .simulation
            .houses
            .get_mut(&id)
            .unwrap()
            .economy_mut();
        let before = economy.available_money();
        economy.add_credits(credit);
        balances.push((id, before + economy.available_money()));
    }
    let carry = previous
        .sim()
        .save_stage_carry_over(&stage.carry_over)
        .unwrap();
    assert!(!carry.is_empty());
    let finale = stage.routes["win"];
    assert_eq!(manifest.stages[finale].id, "Finale");
    let launch =
        AuthoredCampaignStart::from_stage(&manifest.stages[finale], finale, 2, carry).unwrap();
    let scene = load_campaign_stage(
        &retail,
        map_path,
        SEED,
        client(&launch),
        None,
        Some(4),
        None,
    )
    .unwrap();
    let sim = scene.sim();
    assert!(
        sim.session().uses_campaign_world() && sim.session().execution_topology().is_lockstep()
    );
    assert!(sim.session().rule_policy().allows_manual_alliances());
    assert_eq!(
        sim.session().local_view_identity().shared_house_source,
        Some(1)
    );
    let primary = sim.houses().index_of(american).unwrap();
    assert_eq!(
        sim.session()
            .network_sources(sim.houses().len())
            .collect::<Vec<_>>(),
        [(0, primary), (1, primary)]
    );
    assert_eq!(
        sim.control_scope_for_source(0, soviet),
        ControlScope::FullControl
    );
    assert_eq!(
        sim.control_scope_for_source(1, soviet),
        ControlScope::UnitsOnly
    );
    assert_eq!(
        sim.control_scope_for_source(0, yuri),
        ControlScope::UnitsOnly
    );
    assert_eq!(sim.control_scope_for_source(1, yuri), ControlScope::None);
    for (id, credits) in balances {
        assert_eq!(sim.houses()[&id].economy().available_money(), credits);
    }
    let terrain_template = sim.resolved_terrain().unwrap().clone();

    let mut recording = AuthoredReplay::record_loaded(
        scene,
        ReplayLaunchInput::MapHouses(&launch),
        Default::default(),
    );
    recording.frame(&[]);
    let boundary = recording.scene.sim().session().tick();
    recording.service(ScenarioServiceInput::Pause);
    assert!(recording.scene.sim().session().scenario_paused());
    recording.service(ScenarioServiceInput::Resume);
    assert_eq!(recording.scene.sim().session().tick(), boundary);
    for _ in 1..8 {
        recording.frame(&[]);
    }
    recording.log.finish(recording.scene.sim());
    let scene = recording.scene;
    let mut encoded = Vec::new();
    recording.log.write_json(&mut encoded).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let blueprint = &json["startup"]["blueprint"];
    assert_eq!(blueprint["kind"], "campaign");
    assert_eq!(blueprint["map_houses"]["roles"][1]["name"], "Wing");
    for field in ["endpoints", "socket", "token", "nonce", "queue"] {
        assert!(blueprint.get(field).is_none());
    }
    let replay = ReplayLog::decode_json(&encoded).unwrap();
    let verified = ReplayRunner::verify(&retail, &replay, Some(map_path)).unwrap();
    assert_eq!(verified.checked_hashes, 8);
    assert_eq!(verified.final_state_hash, scene.sim().state_hash());

    let sim = scene.sim();
    assert!(
        sim.entities()
            .values()
            .any(|entity| entity.move_sound.countdown() != 0),
        "snapshot exercises live Foot move-sound state"
    );
    let rules = &scene.runtime.resources.rules;
    let rules_hash = rules.simulation_config_hash();
    let map_hash = scene.map.ini.content_hash();
    let bytes = GameSnapshot::save_validated(sim, map_hash, rules_hash, "lockstep stage", 0);
    let snap = GameSnapshot::load_validated(&bytes, map_hash, rules_hash, map_name).unwrap();
    let land = terrain_template
        .capture_user_saved_template(sim.resolved_terrain().unwrap())
        .unwrap()
        .user_load_land_values()
        .unwrap();
    let (restored, _) = snap
        .restore_player_world(
            SnapshotProcessState::Live(sim),
            rules,
            terrain_template,
            sim.terrain_speed_config().clone(),
            PlayerSaveCells::Saved {
                land,
                prepare_assets: &mut |_, _, _| Ok(()),
            },
        )
        .unwrap();
    assert_eq!(
        restored.state_hash(),
        sim.state_hash(),
        "changed hash owners: {:?}",
        sim.state_hash_owners()
            .into_iter()
            .zip(restored.state_hash_owners())
            .filter(|(live, loaded)| live != loaded)
            .collect::<Vec<_>>(),
    );
}

struct AuthoredReplay {
    scene: HeadlessScenario,
    log: ReplayLog,
    network: ReplayNetworkBoundary,
}

impl AuthoredReplay {
    fn begin(
        root: &Path,
        map_path: &str,
        mut descriptor: MatchLaunchDescriptor,
        mode: ScenarioGameMode,
        options: ReplayStartOptions,
    ) -> Self {
        let launch = descriptor.session().clone();
        descriptor.set_personal_rules(options.personal_rules.clone());
        let scene = load_with_launch_and_ini(
            root,
            map_path,
            SEED,
            descriptor,
            &Default::default(),
            mode,
            None,
            None,
        )
        .unwrap();
        Self::record_loaded(scene, ReplayLaunchInput::Slots(&launch), options)
    }

    fn record_loaded(
        mut scene: HeadlessScenario,
        launch: ReplayLaunchInput<'_>,
        mut options: ReplayStartOptions,
    ) -> Self {
        let sim = &mut scene.runtime.simulation;
        sim.batch_path_searches();
        sim.batch_target_scans();
        sim.present_effect_anims();
        sim.sleep_idle_objects_in_match();
        if let Some(record) = &options.campaign_carry_over {
            sim.restore_campaign_carry_over(record, &scene.runtime.resources.rules);
        }
        sim.hash_state_every(NonZeroU64::MIN);
        let lockstep = sim.session().execution_topology().is_lockstep();
        if lockstep {
            let sources: Vec<_> = sim
                .session()
                .network_sources(sim.houses().len())
                .filter_map(|(source, house)| {
                    let owner = sim.houses().order()[house];
                    sim.houses()[&owner].is_human().then_some(source)
                })
                .collect();
            sim.install_network_timing(10);
            sim.network_command_queues()
                .install_timing(0, NonZeroU8::MIN, sources);
        }
        options.every_frame_network = lockstep;
        let startup = ReplayStartup::capture(
            &scene.runtime,
            launch,
            &scene.source,
            scene.map.ini.content_hash(),
            TICK_MS,
            options,
        )
        .unwrap();
        let mut log = ReplayLog::new(ReplayHeader {
            version: REPLAY_VERSION,
            tick_hz: 45,
            seed: u64::from(SEED),
            map_name: scene.sim().session().map_name().to_owned(),
            rules_hash: scene.runtime.resources.rules.simulation_config_hash(),
            pixel_conversion_bounds: scene.sim().session().pixel_conversion_bounds(),
        });
        log.startup = Some(Box::new(startup));
        Self {
            scene,
            log,
            network: ReplayNetworkBoundary {
                every_frame_network: lockstep,
                checksum: Default::default(),
            },
        }
    }

    fn frame(&mut self, commands: &[CommandEnvelope]) -> TickResult {
        let mut output = self
            .scene
            .runtime
            .advance_frame(
                commands,
                TICK_MS,
                TickLane::Ordinary,
                &mut FrameEffects::empty().with_network_commands(&mut self.network),
            )
            .unwrap();
        let admitted = output.take_admitted_commands();
        assert!(!output.network_waiting && !output.outcome_waiting);
        assert!(output.tick.state_hash.is_some());
        self.log
            .record_tick(output.tick.tick, admitted, output.tick.state_hash);
        output.tick
    }

    fn service(&mut self, service: ScenarioServiceInput) {
        self.log
            .apply_scenario_service(&mut self.scene.runtime.simulation, service);
    }

    fn roundtrip(&mut self) -> ReplayLog {
        self.log.finish(self.scene.sim());
        let mut bytes = Vec::new();
        self.log.write_json(&mut bytes).unwrap();
        let json = std::str::from_utf8(&bytes).unwrap();
        for forbidden in [
            "LobbyOnly",
            "\"queue\"",
            "\"host_rules\"",
            "\"selected_map_file\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "unexpected replay field {forbidden}"
            );
        }
        ReplayLog::decode_json(&bytes).unwrap()
    }

    fn verify(&self, root: &Path, map_path: &str, replay: &ReplayLog) {
        let verified = ReplayRunner::verify(root, replay, Some(map_path)).unwrap();
        assert_eq!(verified.checked_hashes, self.log.ticks.len());
        assert_eq!(verified.final_tick, self.scene.sim().session().tick());
        assert_eq!(verified.final_state_hash, self.scene.sim().state_hash());
        assert_eq!(
            verified.terminal_score_snapshot.as_ref(),
            self.scene.sim().terminal_score_snapshot(),
        );
    }
}

fn add_partner(launch: &mut SkirmishLaunchSession) {
    let mut partner = launch.humans[0].clone();
    partner.name = "Wing".into();
    partner.raw_slot_index = Some(1);
    partner.slot.color_index = 1;
    launch.humans.push(partner);
    for opponent in &mut launch.opponents {
        opponent.raw_slot_index += 1;
        opponent.color_index += 1;
    }
}

/// Add a one-shot elapsed-time map trigger through the ordinary authored
/// reader, with its own Tag. No replay-only command writes the world state.
fn add_action(map: &[u8], trigger: &str, tag: &str, seconds: u8, action: &str) -> Vec<u8> {
    let definition = format!("Neutral,<none>,{trigger},0,1,1,1,0");
    let event = format!("1,13,0,{seconds}");
    let tag_value = format!("0,{trigger},{trigger}");
    let map = set_ini_values(map, "Triggers", &[(trigger, &definition)]);
    let map = set_ini_values(&map, "Events", &[(trigger, &event)]);
    let map = set_ini_values(&map, "Actions", &[(trigger, action)]);
    set_ini_values(&map, "Tags", &[(tag, &tag_value)])
}

#[test]
#[ignore = "slow; retail Skirmish diagnostic replay with an authored cutscene and terminal score"]
fn authored_stock_skirmish_replays_an_in_place_cutscene_and_terminal_score() {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets().expect("retail RA2_DIR");
    let directory = TestDirectory::new("authored-skirmish-replay");
    let map = assets.resolve_ref(BATTLE_MAP).unwrap().bytes;
    // Chrono Screen (127) begins in Logic. Main_Tick services its paused
    // midpoint without advancing a frame. Announce Win (67) ends later.
    let map = add_action(map, "7F800000", "7F810000", 0, "1,127,0,1,0,0,0,0,Z");
    let map = add_action(&map, "7F800001", "7F810001", 4, "1,67,0,0,0,0,0,0,Z");
    directory.write(BATTLE_MAP, &map);
    let map_path = directory.path().join(BATTLE_MAP);
    let map_path = map_path.to_str().unwrap();
    let launch = battle_session(BATTLE_MAP, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
    let descriptor = MatchLaunchDescriptor::from_resolved(launch).unwrap();
    let mut recording = AuthoredReplay::begin(
        &root,
        map_path,
        descriptor,
        ScenarioGameMode::Skirmish,
        Default::default(),
    );
    assert!(recording.scene.sim().session().has_stock_rule_policy());
    for _ in 0..64 {
        recording.frame(&[]);
        if recording.scene.sim().session().scenario_paused() {
            break;
        }
    }
    assert!(recording.scene.sim().session().scenario_paused());
    let held_tick = recording.scene.sim().session().tick();
    for _ in 0..8 {
        recording.service(ScenarioServiceInput::StepChronoScreen);
        if !recording.scene.sim().session().scenario_paused() {
            break;
        }
    }
    let session = recording.scene.sim().session();
    assert!(!session.scenario_paused());
    assert_eq!(session.tick(), held_tick);
    assert!(!session.chrono_screen().active());

    for _ in 0..64 {
        if recording.frame(&[]).terminal_score_finalized {
            break;
        }
    }
    let sim = recording.scene.sim();
    assert_eq!(
        sim.session().outcome_request(),
        Some(HouseOutcomeKind::Victory)
    );
    assert!(!sim.terminal_score_snapshot().unwrap().rows.is_empty());
    let mut decoded = recording.roundtrip();
    assert!(matches!(
        decoded.standalone_startup().unwrap().blueprint().launch(),
        ReplayLaunch::Skirmish(_)
    ));
    recording.verify(&root, map_path, &decoded);
    // Alter only the reported score. The original endpoint and every
    // checkpoint still match, so the score must be compared independently.
    let ending = decoded.ending.as_mut().unwrap();
    ending.terminal_score_snapshot.as_mut().unwrap().rows[0].score ^= 1;
    let error = ReplayRunner::verify(&root, &decoded, Some(map_path)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("terminal score snapshot differs")
    );
}

#[test]
#[ignore = "slow; retail Cooperative stage startup and independent lockstep verification"]
fn authored_stock_cooperative_restores_stage_and_keeps_the_native_no_score_outcome() {
    // CoopCampMD.ini Allied stage 1, also the stock WOL Quick Co-op stage.
    const MAP: &str = "C1A01MD.MAP";
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets().expect("retail RA2_DIR");
    let directory = TestDirectory::new("authored-cooperative-replay");
    let map = assets.resolve_ref(MAP).unwrap().bytes;
    let map = add_action(map, "7F800001", "7F810001", 1, "1,67,0,0,0,0,0,0,Z");
    directory.write(MAP, &map);
    let map_path = directory.path().join(MAP);
    let map_path = map_path.to_str().unwrap();
    let mut launch = battle_session(
        MAP,
        &[
            (LaunchCountry::Russia, AiDifficulty::Normal),
            (LaunchCountry::Libya, AiDifficulty::Normal),
        ],
    );
    add_partner(&mut launch);
    launch.mode = SkirmishLaunchMode::stock(3).unwrap();
    let descriptor = MatchLaunchDescriptor::from_resolved(launch)
        .unwrap()
        .with_quick_coop_starts();
    let mut previous = Simulation::new();
    previous.set_trigger_variable_for_test(true, 3, true);
    let mut recording = AuthoredReplay::begin(
        &root,
        map_path,
        descriptor,
        ScenarioGameMode::Wol,
        ReplayStartOptions {
            quick_coop_starts: true,
            campaign_carry_over: Some(previous.save_campaign_carry_over()),
            ..Default::default()
        },
    );
    let sim = recording.scene.sim();
    assert!(sim.global_variables()[3]);
    assert!(sim.session().has_stock_rule_policy());
    assert!(sim.session().rule_policy().skips_network_score_dialog());
    assert_eq!(
        sim.session()
            .network_sources(sim.houses().len())
            .take(2)
            .collect::<Vec<_>>(),
        [(0, 0), (1, 1)],
    );
    for _ in 0..64 {
        assert!(!recording.frame(&[]).terminal_score_finalized);
        if recording.scene.sim().session().outcome_request().is_some() {
            break;
        }
    }
    let sim = recording.scene.sim();
    assert_eq!(
        sim.session().outcome_request(),
        Some(HouseOutcomeKind::Victory)
    );
    assert!(sim.terminal_score_snapshot().is_none());
    let decoded = recording.roundtrip();
    assert!(matches!(
        decoded.standalone_startup().unwrap().blueprint().launch(),
        ReplayLaunch::Cooperative(_)
    ));
    recording.verify(&root, map_path, &decoded);
}

#[test]
#[ignore = "slow; authored retail lockstep replay with shared source authority"]
fn authored_lockstep_replay_preserves_custom_rules_and_source_grants() {
    use crate::rules::rule_policy::BootstrapRuleProfile;
    use crate::sim::combat::TargetKind;
    use crate::sim::command::{ArchiveRecord, MegaMissionTarget};
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};

    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets().expect("retail RA2_DIR");
    let directory = TestDirectory::new("authored-custom-replay");
    let map = assets.resolve_ref(BATTLE_MAP).unwrap().bytes;
    let map = set_ini_values(
        map,
        "SpecialFlags",
        &[
            ("Inert", "yes"),
            ("HarvesterImmune", "yes"),
            ("TiberiumGrows", "no"),
            ("TiberiumSpreads", "no"),
        ],
    );
    let map = add_action(&map, "7F800002", "7F810002", 1, "1,67,0,0,0,0,0,0,Z");
    directory.write(BATTLE_MAP, &map);
    let map_path = directory.path().join(BATTLE_MAP);
    let map_path = map_path.to_str().unwrap();
    let rival = (LaunchCountry::Russia, AiDifficulty::Normal);
    let mut launch = battle_session(BATTLE_MAP, &[rival, rival]);
    launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
    add_partner(&mut launch);
    launch.options.queue.push("LobbyOnly".into());

    let mode = ScenarioGameMode::Lan;
    let policy = mode
        .stock_rule_policy(Some(launch.mode.class))
        .with_bootstrap(BootstrapRuleProfile::Campaign)
        .with_uses_room_options(false)
        .with_starting_house_role_alliances(true);
    let grant = |source, house, level| ControlGrant {
        source,
        house,
        level,
    };
    let grants = [
        grant(0, 1, ControlGrantLevel::FullControl),
        grant(0, 2, ControlGrantLevel::UnitsOnly),
        grant(1, 1, ControlGrantLevel::UnitsOnly),
    ];
    let descriptor = MatchLaunchDescriptor::from_resolved(launch)
        .unwrap()
        .with_rule_policy(policy)
        .with_control_grants(grants.to_vec());
    let mut personal_rules = crate::rules::personal_rules::PersonalRules::default();
    personal_rules.set_terrain_deformation(true);
    let mut recording = AuthoredReplay::begin(
        &root,
        map_path,
        descriptor,
        mode,
        ReplayStartOptions {
            personal_rules,
            ..Default::default()
        },
    );
    assert!(
        recording
            .scene
            .runtime
            .resources
            .rules
            .combat_damage
            .terrain_deformation
    );
    let sim = recording.scene.sim();
    assert!(sim.session().no_damage());
    assert!(sim.session().harvester_immune());
    assert!(!sim.session().tiberium_grows_flag());
    assert!(!sim.session().tiberium_spreads_flag());
    assert_eq!(sim.session().rule_policy(), policy);
    assert_eq!(
        sim.session()
            .network_sources(sim.houses().len())
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0)],
    );
    let actor = |index| {
        let owner = sim.houses().order()[index];
        sim.entities()
            .values_sorted()
            .find(|entity| entity.owner() == owner && entity.locomotor.is_some())
            .unwrap()
            .stable_id()
    };
    let (first, second) = (actor(1), actor(2));

    // A Radar-style scenario hold changes the next lockstep checkpoint
    // without advancing the world. Resume at the following boundary.
    recording.service(ScenarioServiceInput::Pause);
    recording.frame(&[]);
    assert!(recording.scene.sim().session().scenario_paused());
    recording.service(ScenarioServiceInput::Resume);
    assert!(!recording.scene.sim().session().scenario_paused());
    let mut commands = Vec::new();
    for (source, actor, x) in [
        (0, first, 40),
        (1, first, 41),
        (0, second, 42),
        (1, second, 43),
    ] {
        let mut record = CommandRecord::encode(0, 0, 1, &[]).unwrap();
        ArchiveRecord {
            house_id: source,
            frame: 1,
            entity_id: i32::try_from(actor).unwrap(),
            target: MegaMissionTarget::Cell { x, y: 40 },
        }
        .write_into(&mut record)
        .unwrap();
        commands.push(CommandEnvelope::synchronized(
            record,
            recording.scene.sim().session().tick(),
        ));
    }
    recording.frame(&commands);
    for (actor, x) in [(first, 41), (second, 42)] {
        assert_eq!(
            recording
                .scene
                .sim()
                .entities()
                .get(actor)
                .unwrap()
                .archive_target(),
            Some(TargetKind::Cell(x, 40)),
        );
    }
    for _ in 0..64 {
        if recording.frame(&[]).terminal_score_finalized {
            break;
        }
    }
    let sim = recording.scene.sim();
    assert_eq!(
        sim.session().outcome_request(),
        Some(HouseOutcomeKind::Victory)
    );
    assert!(!sim.terminal_score_snapshot().unwrap().rows.is_empty());
    // A last service changes the final shared hash without a new frame.
    let last_checkpoint = recording.log.ticks.last().unwrap().state_hash;
    recording.service(ScenarioServiceInput::Pause);
    assert_ne!(last_checkpoint, Some(recording.scene.sim().state_hash()));
    let decoded = recording.roundtrip();
    let startup = decoded.standalone_startup().unwrap();
    assert!(matches!(
        startup.blueprint().launch(),
        ReplayLaunch::Campaign(_)
    ));
    assert_eq!(startup.rule_policy(), policy);
    assert_eq!(startup.control_grants(), &grants);
    recording.verify(&root, map_path, &decoded);
}
