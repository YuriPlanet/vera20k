//! Process-lifetime diagnostic for sim-bench. Run once without --prior-map in
//! a fresh process, then with prior maps in another process. Compare `target`
//! exactly; `history` and process_id explain which lifetime was exercised.
//! Uses the production headless loader, Rules owner and frame transaction.
//! App input/session teardown and rendering are outside this diagnostic.

use super::*;
use serde_json::{Value, json};
use vera20k_core::rules::process_owner::NativeRulesProcessOwner;
use vera20k_core::skirmish_launch::{LaunchCountry, LaunchStartPosition};

pub(super) fn run(args: &Args) -> Result<(), String> {
    let output = args
        .isolation_report
        .as_ref()
        .ok_or("missing report path")?;
    if args.isolation_humans == 0 || args.isolation_humans > 2 {
        return Err("state-isolation diagnostic supports one or two humans".into());
    }
    let mode = args.isolation_mode.unwrap_or(ScenarioGameMode::Wol);
    let computers = args.computers.unwrap_or(1);
    if computers > 8 - args.isolation_humans {
        return Err("state-isolation roster exceeds eight players".into());
    }
    let assets = AssetManager::new(&args.ra2_dir, MediaArchiveMode::STOCK_DIGITAL)
        .map_err(|error| error.to_string())?;
    let audio = vera20k_core::rules::audio_sources::AudioDefinitions::select(&assets);
    let (_, _, mut owner) = vera20k_core::rules::retail_sources::load_startup_rules(
        &assets,
        std::sync::Arc::clone(audio.sounds()),
        std::sync::Arc::clone(audio.eva()),
    )
    .ok_or("retail startup Rules unavailable")?
    .into_parts();
    let mut history = Vec::new();
    for map in &args.prior_maps {
        let mut scene = load(
            args,
            map,
            args.seed.wrapping_add(1),
            mode,
            computers,
            &mut owner,
        )?;
        let report = exercise(&mut scene, 90, args.tick_ms())?;
        history.push(json!({"map":map,"final":report}));
        // Same owner transfer as app::process_assets::transfer_outgoing_simulation_rules.
        // The app cannot be a dependency of this core-side diagnostic.
        let sim = &mut scene.runtime.simulation;
        sim.retire_scenario_started();
        owner
            .consume_outgoing_scenario_rules(
                sim.retire_object_type_tech_levels(),
                sim.retire_ambient_change_inputs(),
                sim.retire_building_upgrade_art_names(),
                sim.retire_voxel_type_damage(),
            )
            .map_err(|error| error.to_string())?;
    }
    let mut target = load(args, &args.map, args.seed, mode, computers, &mut owner)?;
    let initial = snapshot(&target);
    let ticks = args.ticks.map_or(300, NonZeroU64::get);
    let played = exercise(&mut target, ticks, args.tick_ms())?;
    let report = json!({
        "schema":"vera20k.process-state-isolation.v1",
        "process_id":std::process::id(),"history":history,
        "target":{"map":args.map,"seed":args.seed,"mode":mode.as_str(),
            "humans":args.isolation_humans,"computers":computers,
            "initial":initial,"played":played},
        "scope":"headless production Rules/map/Simulation loading and frame execution; app input and transport are tested separately"
    });
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("{}: {error}", output.display()))?;
    serde_json::to_writer_pretty(file, &report).map_err(|error| error.to_string())?;
    println!("state-isolation report: {}", output.display());
    Ok(())
}

fn load(
    args: &Args,
    map: &str,
    seed: u32,
    mode: ScenarioGameMode,
    computers: usize,
    owner: &mut NativeRulesProcessOwner,
) -> Result<HeadlessScenario, String> {
    let mut session = battle_session(map, &sim_bench_match::COMPUTER_ROSTER[..computers]);
    session.humans[0].slot.start_position = LaunchStartPosition::Position(0);
    if args.isolation_humans == 2 {
        let mut second = session.humans[0].clone();
        second.name = "Remote".into();
        second.raw_slot_index = Some(1);
        second.slot.country = LaunchCountry::Russia;
        second.slot.color_index = 1;
        second.slot.start_position = LaunchStartPosition::Position(1);
        session.humans.push(second);
    }
    for (index, ai) in session.opponents.iter_mut().enumerate() {
        ai.raw_slot_index = (args.isolation_humans + index) as u8;
        ai.color_index = (args.isolation_humans + index) as u8;
        ai.start_position = LaunchStartPosition::Position((args.isolation_humans + index) as u8);
    }
    let prefix =
        vera20k_core::sim::native_identity::network_native_id_prefix(mode, session.humans.len());
    let launch =
        MatchLaunchDescriptor::from_resolved(session).map_err(|error| error.to_string())?;
    let mut scene = headless_scenario::load_with_launch_and_ini(
        &args.ra2_dir,
        map,
        seed,
        launch,
        &Default::default(),
        mode,
        prefix,
        Some(owner),
    )?;
    scene
        .runtime
        .simulation
        .install_network_timing(LAN_MAX_AHEAD);
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    scene.runtime.simulation.present_effect_anims();
    scene.runtime.simulation.separate_turn_streams(true);
    Ok(scene)
}

fn snapshot(scene: &HeadlessScenario) -> Value {
    let sim = scene.sim();
    json!({"hash":sim.state_hash(),"owners":sim.state_hash_owners().as_slice(),
        "rules_hash":scene.runtime.resources.rules.simulation_config_hash(),
        "tiberium_types":scene.runtime.resources.rules.tiberium_types.len(),
        "native_prefix":sim.native_identity_prefix_checkpoint(),
        "native_cursor":sim.native_identity_cursor(),"entities":sim.entities().len()})
}

fn exercise(scene: &mut HeadlessScenario, ticks: u64, tick_ms: u32) -> Result<Value, String> {
    let owner = scene
        .sim()
        .session()
        .current_house()
        .ok_or("missing local House")?;
    let actor = scene
        .sim()
        .entities()
        .values_sorted()
        .find(|entity| entity.owner() == owner && scene.sim().resolve(entity.type_ref()) == "MTNK")
        .ok_or("state-isolation fixture needs a starting MTNK")?;
    let id = actor.stable_id();
    let origin = (actor.position.rx(), actor.position.ry());
    let commands = [
        CommandEnvelope::new(
            owner,
            10,
            Command::Move {
                entity_id: id,
                target_rx: origin.0.saturating_add(3),
                target_ry: origin.1,
            },
        ),
        CommandEnvelope::new(owner, 40, Command::Stop { entity_id: id }),
    ];
    let mut frames = Vec::new();
    for frame in 1..=ticks {
        let due: Vec<_> = commands
            .iter()
            .filter(|command| command.execute_tick == frame)
            .cloned()
            .collect();
        let checksum = scene
            .runtime
            .advance_frame_for_tooling(&due, tick_ms)
            .map_err(|error| format!("frame {frame}: {error}"))?;
        frames.push(json!([frame, scene.sim().state_hash(), checksum]));
    }
    let actor = scene
        .sim()
        .entities()
        .get(id)
        .ok_or("fixture MTNK disappeared")?;
    Ok(
        json!({"commands":commands,"frames":frames,"final":snapshot(scene),
        "actor":{"id":id,"origin":origin,"position":actor.position,"mission":actor.mission.current().raw()}}),
    )
}
