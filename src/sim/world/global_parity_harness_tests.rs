//! A two-House skirmish fixture (an Allied war factory, refinery and harvester
//! over a seeded ore patch; tanks and infantry on both sides) and the replay
//! diagnostics the bridge and slice-6 replay harnesses share.
//!
//! The tests check that a cross-House Move and Stop execute and replay without
//! changing the actor's owner, and that a fresh Drive turn publishes on its
//! request frame. The 700-frame global replay that pinned a final hash here was
//! removed on 2026-11-11: its pins were VERA's own values on this synthetic
//! map, and they had drifted unnoticed since 2026-10-24.

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use std::io::Write;

/// Optional observations of bounded replay fixtures, never golden input.
/// The file records existing owners without changing or draining their state.
pub(super) fn replay_diagnostic_file(name: &str) -> Option<std::io::BufWriter<std::fs::File>> {
    let root = std::path::PathBuf::from(std::env::var_os("VERA20K_REPLAY_DIAGNOSTICS")?);
    std::fs::create_dir_all(&root).unwrap();
    Some(std::io::BufWriter::new(
        std::fs::File::create(root.join(format!("{name}.jsonl"))).unwrap(),
    ))
}

pub(super) fn record_replay_diagnostic(
    writer: &mut Option<std::io::BufWriter<std::fs::File>>,
    sim: &Simulation,
    result: Option<&TickResult>,
    commands: &[CommandEnvelope],
    draws: &[serde_json::Value],
) {
    let Some(writer) = writer else { return };
    let actors: Vec<_> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .map(|id| sim.substrate.entities.get(id).unwrap())
        .collect();
    let fires: Vec<_> = sim
        .fire_events
        .iter()
        .map(|fire| {
            serde_json::json!({
                "attacker":fire.attacker_id, "target":fire.target,
                "weapon":sim.resolve(fire.weapon_id),
                "position":[fire.fire_coord.x,fire.fire_coord.y,fire.fire_coord.z],
            })
        })
        .collect();
    let retained_path_inputs = serde_json::json!({
        "navigation_present":sim.zone_grid.is_some(),
        "houses":sim.houses.iter().map(|(owner,house)| serde_json::json!({
            "owner":sim.resolve(*owner),
            "spatial_grid_all_zero":house.spatial_threat_values().iter().all(|&value|value==0),
        })).collect::<Vec<_>>(),
        "actors":actors.iter().map(|actor| serde_json::json!({
            "id":actor.stable_id(),
            "coefficient_bits":actor.navigation.path_threat_coefficient().bits(),
            "cached_contribution":actor.cached_spatial_threat(),
            "team_override":sim.team_script_vm.member_avoids_threats(actor.stable_id()),
        })).collect::<Vec<_>>(),
        "team_count":sim.team_script_vm.teams_in_order().count(),
        // House/Factory receipts observe shared consumers through their existing
        // owners, including private power state; they never mutate the replay.
        "engineer_house_attribution":{
            "house_order":sim.houses.order().iter().map(|owner|sim.resolve(*owner)).collect::<Vec<_>>(),
            "current_house":sim.session.current_house().map(|owner|sim.resolve(owner)),
            "houses":sim.houses.iter().map(|(owner,house)|serde_json::json!({
                "owner":sim.resolve(*owner),"stats":house.stats(),"economy":house.economy(),
                "capture_notified":house.building_capture_notified(),
                "discovered_by_current_house":house.discovered_by_current_house(),
                "building_order":house.base_projection().buildings(),
                "eva_funds_timer":house.eva_funds_timer(),"eva_low_power_guard":house.eva_low_power_guard(),
                "repair_delay_bits":house.repair_delay().to_bits(),
                "repair_start_latch":house.repair_start_latch(),"repair_latch_timer":house.repair_latch_timer(),
            })).collect::<Vec<_>>(),
            "power_states":sim.power_states.iter().map(|(owner,state)|serde_json::json!({
                "owner":sim.resolve(*owner),"state":state,
            })).collect::<Vec<_>>(),
            "factory_count":sim.production.factories().len(),
            "factories":sim.production.factories().holders_insertion_ordered().into_iter()
                .map(|(holder,factory)|serde_json::json!({"holder":holder,"factory":factory})).collect::<Vec<_>>(),
        },
        // Only the global fixture kills a Unit. Preserve the shared cleanup
        // owners around that death without inventing or draining any state.
        "lifecycle_hash_attribution":{
            "raw_occupation":format!("{:?}",sim.substrate.raw_cell_occupation),
            "shared_dummy":format!("{:?}",sim.effective_shared_cell_dummy().snapshot()),
            "shared_dummy_raw_occupation":sim.substrate.raw_cell_occupation.dummy_for_hash(),
            "houses":sim.houses.iter().map(|(owner,house)|serde_json::json!({
                "owner":sim.resolve(*owner),"retained":house,
            })).collect::<Vec<_>>(),
        },
    });
    serde_json::to_writer(
        &mut *writer,
        &serde_json::json!({
            "next_frame":sim.session.binary_frame(),
            "tick_result":result.map(|r| serde_json::json!({"tick":r.tick,
                "committed":r.frame_committed,"executed_commands":r.executed_commands,
                "state_hash":r.state_hash,"terminal_score_finalized":r.terminal_score_finalized})),
            "commands":commands,"entities":actors,"logic_order":sim.logic_order(),
            "planning":sim.planning,
            "fire_events_accumulated":fires,
            "lifecycle_outputs_accumulated":format!("{:?}",sim.lifecycle_outputs),
            "rng":{"scenario":sim.scenario_rng.native_state_hex(),
                "main":sim.main_rng.native_state_hex(),"mapgen":sim.mapgen_rng.native_state_hex()},
            "draws":draws,"retained_path_inputs":retained_path_inputs,
            "source_admissions":sim.fog.sight_admissions_for_test().iter().map(|(&(id,viewer),source)|
                serde_json::json!({"id":id,"viewer":sim.resolve(viewer),"source":source})
            ).collect::<Vec<_>>(),
            "alternate_sight_admissions":sim.fog.alternate_sight_admissions_for_test().iter().map(|(&(id,viewer),source)|
                serde_json::json!({"id":id,"viewer":sim.resolve(viewer),"source":source})
            ).collect::<Vec<_>>(),
            "fog_owners":sim.fog.by_owner().iter().map(|(owner,view)| {
                let mut hash = crate::util::state_hasher::StateHasher::new();
                view.hash_cells(&mut hash);
                serde_json::json!({"owner":sim.resolve(*owner),"digest":std::hash::Hasher::finish(&hash)})
            }).collect::<Vec<_>>(),
            "default_fog_owner_digest":({
                let mut hash = crate::util::state_hasher::StateHasher::new();
                crate::sim::vision::OwnerVisibility::new(sim.fog.width(),sim.fog.height()).hash_cells(&mut hash);
                std::hash::Hasher::finish(&hash)
            }),
        }),
    )
    .unwrap();
    writeln!(writer).unwrap();
    writer.flush().unwrap();
}

pub(super) fn print_replay_summary(name: &str, sim: &Simulation) {
    if std::env::var_os("VERA20K_REPLAY_DIAGNOSTICS").is_none() {
        return;
    }
    let actors: Vec<_> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .map(|id| {
            let e = sim.substrate.entities.get(id).unwrap();
            serde_json::json!({"id":id,"type":sim.resolve(e.type_ref()),
            "health":e.health.current(),"position":e.position,"mission":e.mission,
            "target":e.attack_target,"nav":e.navigation.nav_com(),
            "passive_timer":e.passive_scan.timer(),"last_scan":e.passive_scan.last_scan_frame(),
            "rearm":e.rearm_timer.timer()})
        })
        .collect();
    println!(
        "[replay observation {name}] {}",
        serde_json::json!({
            "next_frame":sim.session.binary_frame(),"hash":sim.state_hash(),
            "streams":[sim.scenario_rng.state(),sim.main_rng.state(),sim.mapgen_rng.state()],
            "actors":actors,"fire_count":sim.fire_events.len(),
            "hash_owners":sim.hash_owner_diagnostics_for_test(),
            "source_admissions":sim.fog.sight_admissions_for_test().iter().map(|(&(id,viewer),source)|
                serde_json::json!({"id":id,"viewer":sim.resolve(viewer),"source":source})
            ).collect::<Vec<_>>(),
            "alternate_sight_admissions":sim.fog.alternate_sight_admissions_for_test().iter().map(|(&(id,viewer),source)|
                serde_json::json!({"id":id,"viewer":sim.resolve(viewer),"source":source})
            ).collect::<Vec<_>>(),
        })
    );
}

const HARNESS_SEED: u64 = 0xC0FFEE1234;
const HARNESS_TICK_MS: u32 = 67;
const HARNESS_MAP_SIZE: u16 = 64;
const HARNESS_CELL_SIDE: u16 = 2 * HARNESS_MAP_SIZE + 1;
const HARNESS_COORD_SHIFT: u16 = HARNESS_MAP_SIZE / 2;

fn harness_ini() -> IniFile {
    // Multi-faction vehicles + infantry + buildings (war factory, refinery) plus a
    // real harvester (Harvester/Dock/Storage) and a real refinery (Refinery=yes)
    // so the miner dock path is reachable. Short weapon ranges keep combat to the
    // scripted engagements, keeping the scenario deterministic.
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=HARV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAWEAP\n1=GAREFN\n\n\
         [OverlayTypes]\n0=TIB01\n\n\
         [Tiberiums]\n0=Riparius\n\n\
         [Riparius]\nImage=1\nValue=25\n\n\
         [TIB01]\nTiberium=yes\n\n\
         [Tiberium]\nFoot=100%\nTrack=100%\nWheel=100%\n\n\
         [E1]\nImage=GI\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [HARV]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=600\nArmor=heavy\nSpeed=5\nHarvester=yes\nStorage=28\nDock=GAREFN\n\n\
         [GAWEAP]\nStrength=1000\nArmor=wood\nFoundation=4x3\n\n\
         [GAREFN]\nStrength=1000\nArmor=wood\nRefinery=yes\nDockUnload=yes\nFoundation=3x3\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    );
    crate::rules::tiberium_type::with_tiberium_image_slots(&ini)
}

fn harness_rules() -> RuleSet {
    let ini = harness_ini();
    // Explicit authored GI inputs use the production fixed-ART reader and binder;
    // zero-count constructor records do not admit native Ready/idle actions.
    let mut art = IniFile::from_str(crate::rules::retail_ini_fixture::GI_ART_EXCERPT);
    art.merge(&IniFile::from_str(
        "[GAWEAP]\nFoundation=4x3\n[GAREFN]\nFoundation=3x3\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

fn harness_overlays() -> OverlayTypeRegistry {
    OverlayTypeRegistry::from_ini(&harness_ini(), None)
}

fn unit(owner: &str, type_id: &str, cx: u16, cy: u16, cat: EntityCategory) -> MapEntity {
    MapEntity {
        owner: owner.to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: cx,
        cell_y: cy,
        facing: 64,
        category: cat,
        sub_cell: 0,
        veterancy: Some(0),
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: None,
    }
}

/// Build the recorded scenario into `sim`. Spawn order fixes stable ids
/// 1..=7 (war factory, refinery, harvester, Allied tank, Allied infantry,
/// Soviet tank, Soviet infantry).
fn seed_scenario(sim: &mut Simulation, rules: &RuleSet, overlays: &OverlayTypeRegistry) {
    // Flat map cells and the playfield, as a map load installs them before
    // placing objects: Unlimbo's Unit `Can_Enter_Cell` reads both, and the
    // harvester's ore scan (`FootClass::Is_Cell_Harvestable @ 0x004DCE80`)
    // admits only playfield cells of LandType 5.
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_flat_ground_grid(
        HARNESS_CELL_SIDE,
    ));
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: u32::from(HARNESS_MAP_SIZE),
        height: u32::from(HARNESS_MAP_SIZE),
        local_left: 2,
        local_top: 2,
        local_width: 60,
        local_height: 56,
    });
    sim.bridge_state = Some(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            rules.bridge_rules.strength,
            (i32::from(HARNESS_MAP_SIZE), i32::from(HARNESS_MAP_SIZE)),
        ),
    );
    // The owners' houses exist before a map load places objects, so each
    // object's constructor `Add_Tracking` counts it: the harvester's Dock
    // checks read the house's tracked BuildingType counts (`+0x5500`). They
    // are computer houses, which keep this run's behaviour: the damage
    // response treated a house-less owner as a computer's (a human house's
    // AttackMove tank keeps moving when hit), and the harvester branches that
    // treat a missing house as human are not reached in these 700 ticks.
    // Registered, they run House AI; with no credits it builds nothing and
    // forms no team. A separate eligible sender below supplies the scripted
    // events without changing these actors' AI-control semantics.
    for (owner, side) in [("Americans", 0), ("Soviet", 1)] {
        let id = sim.interner.intern(owner);
        sim.houses.get_or_register_for_test(id, || {
            crate::sim::house_state::HouseState::new(id, side, None, false, 0, 10)
        });
    }
    let mut authored = [
        unit("Americans", "GAWEAP", 3, 3, EntityCategory::Structure), // 1
        unit("Americans", "GAREFN", 3, 10, EntityCategory::Structure), // 2
        unit("Americans", "HARV", 8, 12, EntityCategory::Unit),       // 3
        unit("Americans", "MTNK", 10, 8, EntityCategory::Unit),       // 4
        unit("Americans", "E1", 11, 9, EntityCategory::Infantry),     // 5
        unit("Soviet", "MTNK", 40, 8, EntityCategory::Unit),          // 6
        unit("Soviet", "E1", 41, 9, EntityCategory::Infantry),        // 7
    ];
    // Preserve relative scene geometry inside a valid native Size diamond.
    // The former width-zero rectangle cannot contain both (3,3) and (41,9).
    for entity in &mut authored {
        entity.cell_x += HARNESS_COORD_SHIFT;
        entity.cell_y += HARNESS_COORD_SHIFT;
        assert!(sim.map_cell_in_bounds((entity.cell_x as i16, entity.cell_y as i16)));
        assert!(
            sim.playfield_bounds
                .unwrap()
                .contains_geometry_packed(i32::from(entity.cell_x), i32::from(entity.cell_y),)
        );
    }
    sim.spawn_from_map(&authored, Some(rules));
    // Seed the native CellClass overlay authority near the harvester.
    let tib01 = overlays.id_for_name("TIB01").expect("harness TIB01");
    let mut overlay_grid = OverlayGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    let terrain = sim.resolved_terrain.as_mut().expect("installed above");
    for (rx, ry) in [(12, 13), (13, 13), (12, 14), (13, 14)] {
        let (rx, ry) = (rx + HARNESS_COORD_SHIFT, ry + HARNESS_COORD_SHIFT);
        overlay_grid.place_overlay(rx, ry, tib01, 11);
        // RecalcAttributes: LandType 5 and its [Tiberium] speed row.
        overlay_grid.recalculate_runtime_cell(terrain, overlays, (rx, ry));
    }
    overlay_grid.take_dirty_cells();
    sim.overlay_grid = Some(overlay_grid);
    // Live Foot reachability consumes the same map authorities as a map load.
    // The old ore-query copy silently skipped zones in this direct-spawn scene.
    // This harness passes its PathGrid into each frame; retain that owner
    // rather than installing a second grid while completing the zone fixture.
    let path = PathGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
            &path,
            sim.resolved_terrain.as_ref().unwrap(),
            &[],
            sim.map_size_diamond(),
            sim.playfield_bounds,
        )
        .into(),
    );
    // Native MegaMission and IDLE select the actor from their payload, without
    // comparing its House to the Event header. Keep the fixture's two AI actor
    // Houses unchanged; this human House has no actors and only issues orders.
    // Add it after object seeding to preserve prior actor/type identities.
    let issuer = sim.interner.intern("ScriptIssuer");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            issuer, 2, None, true, 0, 10,
        ));
}

#[test]
fn cross_house_move_and_stop_execute_and_replay_without_changing_actor_owner() {
    let rules = harness_rules();
    let overlays = harness_overlays();
    let grid = PathGrid::new(HARNESS_CELL_SIDE, HARNESS_CELL_SIDE);
    let mut recorded = Simulation::with_seed(HARNESS_SEED);
    recorded.hash_state_every(std::num::NonZeroU64::MIN);
    seed_scenario(&mut recorded, &rules, &overlays);
    let actor_owner = recorded.substrate.entities.get(6).unwrap().owner();
    let issuer = recorded.interner.get("ScriptIssuer").unwrap();
    assert_ne!(actor_owner, issuer);
    let destination = (28 + HARNESS_COORD_SHIFT, 10 + HARNESS_COORD_SHIFT);
    let command = CommandEnvelope::new(
        issuer,
        1,
        Command::Move {
            entity_id: 6,
            target_rx: destination.0,
            target_ry: destination.1,
        },
    );
    let mut replay = ReplayLog::new(ReplayHeader {
        version: 1,
        pixel_conversion_bounds: recorded.session.pixel_conversion_bounds(),
        tick_hz: 15,
        seed: HARNESS_SEED,
        map_name: "cross_house_move".into(),
        rules_hash: 0,
    });
    let result = recorded.advance_tick(
        std::slice::from_ref(&command),
        Some(&rules),
        Some(&grid),
        Some(&overlays),
        HARNESS_TICK_MS,
    );
    assert_eq!(
        recorded
            .substrate
            .entities
            .get(6)
            .unwrap()
            .navigation
            .nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(
            destination.0,
            destination.1
        ))
    );
    replay.record_tick(0, vec![command], result.state_hash);
    // Event IDLE 0x4C74CB replaces its Event receiver with the decoded actor
    // at 0x4C74D3. It has no sender/actor ownership comparison.
    let stop = CommandEnvelope::new(issuer, 2, Command::Stop { entity_id: 6 });
    let stopped = recorded.advance_tick(
        std::slice::from_ref(&stop),
        Some(&rules),
        Some(&grid),
        Some(&overlays),
        HARNESS_TICK_MS,
    );
    assert!(
        recorded
            .substrate
            .entities
            .get(6)
            .unwrap()
            .navigation
            .nav_com()
            .is_none()
    );
    replay.record_tick(1, vec![stop], stopped.state_hash);
    let replay: ReplayLog = serde_json::from_slice(&serde_json::to_vec(&replay).unwrap()).unwrap();
    let mut restored = Simulation::with_seed(HARNESS_SEED);
    seed_scenario(&mut restored, &rules, &overlays);
    let hashes = ReplayRunner::run_fixture_with_overlay_registry(
        &mut restored,
        &replay,
        Some(&rules),
        Some(&grid),
        Some(&overlays),
        HARNESS_TICK_MS,
    );
    assert_eq!(hashes, vec![result.state_hash, stopped.state_hash]);
    assert_eq!(restored.state_hash(), recorded.state_hash());
    let actor = restored.substrate.entities.get(6).unwrap();
    assert_eq!(actor.owner(), actor_owner);
    assert!(actor.navigation.nav_com().is_none());
}

const DENSE_SEED: u64 = 0x00BA771E5EED;

#[test]
fn fresh_drive_turn_publishes_on_request_frame_and_restores_before_admission() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/drive_fresh_turn.json",
    ))
    .unwrap();
    for rot in [0, 5] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\nROT={rot}\n\
             Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n"
        )))
        .unwrap();
        let mut sim = Simulation::with_seed(DENSE_SEED);
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        // Source and loaded graphs both use the retained map/bounds inputs.
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let grid = (*sim.path_grid_snapshot().unwrap()).clone();
        sim.spawn_from_map(
            &[unit("Americans", "MTNK", 40, 5, EntityCategory::Unit)],
            Some(&rules),
        );
        let owner = sim.interner.get("Americans").unwrap();
        let native = rows
            .iter()
            .find(|row| {
                row["input"]["initial"] == 0x4000
                    && row["input"]["direction"] == 6
                    && row["input"]["rate"] == rot * 256
            })
            .unwrap();
        for tick in 1..=3 {
            let due = if tick == 2 {
                vec![CommandEnvelope::new(
                    owner,
                    tick,
                    Command::Move {
                        entity_id: 1,
                        target_rx: 25,
                        target_ry: 5,
                    },
                )]
            } else {
                Vec::new()
            };
            sim.advance_tick(&due, Some(&rules), Some(&grid), None, HARNESS_TICK_MS);
        }
        let entity = sim.substrate.entities.get(1).unwrap();
        let call = &native["calls"][0];
        assert_eq!(
            u64::from(entity.body_facing_byte(sim.session.binary_frame() - 1)),
            call["sampled_after"].as_u64().unwrap() >> 8,
            "ROT={rot}: same-frame native sample"
        );
        let drive = entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap();
        assert_eq!(
            drive.track().turn_index(),
            -1,
            "turn must return before admission"
        );
        assert!(drive.head_to().is_none());
        if rot > 0 {
            assert_eq!(
                u64::from(entity.body_facing.current(sim.session.binary_frame() - 1)),
                call["sampled_after"].as_u64().unwrap(),
                "full16-bit native sample, before the next binary frame"
            );
            assert_eq!(
                entity.body_facing.timer_start_frame(),
                Some(call["timer_start"].as_u64().unwrap() as u32)
            );
        }
        // Native load resets Scenario to Seed0 and clears planning tokens
        // (ClearScene6370B0, TechnoLoad70BF79), retaining process streams.
        // Equalize those load effects before comparing the movement state
        // and its continued full-world hashes.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        sim.reset_planning_after_player_load();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 0, "Fresh turn", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
        restored.restore_after_snapshot_load().unwrap();
        assert!(
            restored
                .zone_grid
                .as_deref()
                .unwrap()
                .is_native_load_pending()
        );
        // Bind scenario terrain before original LoadContent's full hierarchy
        // publication. A decoded saved base is not an active hierarchy.
        restored.rebuild_caches_after_load(
            sim.resolved_terrain.as_ref().unwrap().clone(),
            sim.terrain_speed_config.clone(),
            &rules,
        );
        assert!(restored.rebuild_dynamic_navigation(&rules));
        assert!(
            !restored
                .zone_grid
                .as_deref()
                .unwrap()
                .is_native_load_pending()
        );
        for tick in 4..=35 {
            for world in [&mut sim, &mut restored] {
                world.advance_tick(&[], Some(&rules), Some(&grid), None, HARNESS_TICK_MS);
            }
            assert_eq!(
                sim.state_hash(),
                restored.state_hash(),
                "ROT={rot}, tick={tick}"
            );
            if tick == 4 {
                let entity = sim.substrate.entities.get(1).unwrap();
                assert_eq!(
                    entity
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .unwrap()
                        .head_to()
                        .is_some(),
                    rot == 0
                );
            }
        }
    }
}
