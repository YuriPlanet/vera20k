//! Replay of `tools/spatial_oracle/harvest_field.json`: every original row
//! of the ore field runs against the Rust owners — `CellClass::Reduce_Tiberium`
//! (`tiberium::reduce_tiberium`), the Foot tiberium search
//! (`miner::ore_scan`), `UnitClass::Harvest_Ore_Tick` and Mission_Harvest
//! states 0/1 (`miner::miner_system`) — on the refinery_dock scene (a War
//! Miner and a 4x3 DockUnload refinery at NW (6, 9)) with the row's ore and
//! gem overlays.
//!
//! The Scenario RNG is seeded as the oracle seeds it (the original seeder,
//! `SimRng::new`), so the spread-queue priorities, the Rate epilogue's draw
//! and the RNG cursors compare exactly.
//!
//! Compared per row: the returned value (removed amount, found cell, search
//! answer, tick answer, dispatch delay); each row cell's overlay, density and
//! LandType; the spread queue entries with their priorities; the RNG
//! cursors; the Can_Reach_Zone arguments (`ore_scan::harvest_reach`); and for
//! the miner its NavCom, Harvest cursor, archive target, Unit+0x6D2,
//! StageClass, cargo, the house's +0x242 byte and, for the Chrono Miner rows,
//! the active locomotor.
//!
//! Not compared: the per-probe Can_Reach_Zone call list (the oracle observes
//! it), and the rows listed in `SKIPPED`.

use super::refinery_dock_oracle_tests::{Scene, cell, scene_with};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::miner::{MinerState, ResourceType};
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::rng::SimRng;
use crate::sim::tiberium::test_support;
use serde_json::Value;

/// Rows whose supplied answers the Rust scene does not reproduce.
const SKIPPED: &[&str] = &[
    // A supplied Can_Reach_Zone refusal for one open cell: the scene's zones
    // come from its terrain, where the cell is reachable.
    "scan_unreachable_skipped",
    // A type without `Harvester=` carries no Miner component, so the Harvest
    // dispatch (and its ore tick) never reaches it: the native 450-frame
    // non-harvester hold (`0x0073E62F`) is the dispatcher's gate.
    "tick_not_harvester",
];

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/harvest_field.json"
    ))
    .unwrap()
}

/// The refinery_dock rules with the row's `SpreadPercentage=` and
/// `Harvester=`, the stock overlay list and the row's `[General]` values.
fn rules_for(input: &Value) -> (RuleSet, IniFile) {
    let percentage = input["spread_percentage"].as_f64().unwrap_or(0.1);
    let mut text = super::refinery_dock_oracle_tests::RULES
        .replace(
            "[Riparius]\nImage=1\nValue=25\n",
            &format!("[Riparius]\nImage=1\nValue=25\nSpreadPercentage={percentage}\n"),
        )
        .replace(
            "[Cruentus]\nImage=2\nValue=50\n",
            &format!("[Cruentus]\nImage=2\nValue=50\nSpreadPercentage={percentage}\n"),
        );
    if input["harvester"] == false {
        text = text.replacen("Harvester=yes", "Harvester=no", 1);
    }
    if input["cmin"] == true {
        // cmin_dock's Chrono Miner (a Teleport locomotor) and its twin.
        text = text.replacen("1=MTNK\n", "1=MTNK\n2=CMIN\n3=CTNK\n", 1);
        text.push_str(super::cmin_dock_oracle_tests::CMIN);
    }
    let overlays = test_support::tiberium_rules_text();
    text.push_str(&overlays[overlays.find("[OverlayTypes]").unwrap()..]);
    for land in LandType::ALL.iter().take(9) {
        text.push_str(&format!(
            "[{}]\nFoot=100%\nTrack=100%\nWheel=100%\nBuildable=yes\n",
            land.section_name()
        ));
    }
    let ini = IniFile::from_str(&text);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str(
            "[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n[GAREFX]\nFoundation=4x3\nQueueingCell=4,1\n",
        ),
    ));
    rules.general.tiberium_short_scan = input["short_scan"].as_i64().unwrap_or(6 * 256) as i32;
    rules.general.tiberium_long_scan = input["long_scan"].as_i64().unwrap_or(48 * 256) as i32;
    rules.general.harvester_load_rate = input["load_rate"].as_i64().unwrap_or(2) as i32;
    (rules, ini)
}

/// The overlay registry of the scene's rules: the stock list
/// (`test_support::tiberium_rules_text`) with the scene's land rows, so a
/// recalculated ore cell carries its `[Tiberium]` speed row.
pub(super) fn registry() -> &'static crate::map::overlay_types::OverlayTypeRegistry {
    static REGISTRY: std::sync::OnceLock<crate::map::overlay_types::OverlayTypeRegistry> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        crate::map::overlay_types::OverlayTypeRegistry::from_ini(
            &rules_for(&serde_json::json!({})).1,
            None,
        )
    })
}

/// The overlay name for a row cell's `(tiberium, variant)`.
fn overlay_name(kind: u64, variant: u64) -> String {
    match kind {
        0 => format!("TIB{:02}", variant + 1),
        1 => format!("GEM{:02}", variant + 1),
        other => panic!("tiberium {other}"),
    }
}

/// The scene with the row's prestate: no radio contact unless the row says
/// so, its ore cells (LandType recomputed), the queues sized on the oracle's
/// 16x16 MapSize, the archive, Unit+0x6D2 and the Scenario RNG seed.
pub(super) fn row_scene(input: &Value) -> Scene {
    let mut input = input.clone();
    if input.get("linked").is_none() {
        input["linked"] = false.into();
    }
    // Every row's miner stands off the refinery foundations, so it
    // Unlimbos on its own cell: the scan's Can_Enter_Cell reads raw
    // occupation, which a relocated spawn would leave on the spawn cell.
    if input.get("unlimbo_at_cell").is_none() {
        input["unlimbo_at_cell"] = true.into();
    }
    let (rules, ini) = rules_for(&input);
    let cmin = input["cmin"] == true;
    let mut s = if cmin {
        let mut s = scene_with(
            &super::cmin_dock_oracle_tests::cmin_base_input(&input),
            rules,
            &ini,
        );
        super::cmin_dock_oracle_tests::dress_cmin(&mut s, &input);
        s
    } else {
        scene_with(&input, rules, &ini)
    };
    // Other objects on the row's cells, before the ore lands on them: an
    // allied Unit standing there (Unlimbo lists it and sets its vehicle
    // bit), or only the raw vehicle bit (`+0x124` 0x20), as a Drive holding
    // the cell as its next one.
    let heights = std::collections::BTreeMap::new();
    for at in input["units"].as_array().into_iter().flatten() {
        let (x, y) = cell(at);
        s.sim
            .spawn_object("MTNK", "Americans", x, y, 0, &s.rules, &heights)
            .expect("standing unit");
    }
    for at in input["reserved"].as_array().into_iter().flatten() {
        let (x, y) = cell(at);
        s.sim.substrate.raw_cell_occupation.mark_ground(x, y, 0x20);
    }
    let registry = registry();
    for ore in input["ore"].as_array().unwrap() {
        let at = (
            ore[0].as_u64().unwrap() as u16,
            ore[1].as_u64().unwrap() as u16,
        );
        let id = registry
            .id_for_name(&overlay_name(
                ore[2].as_u64().unwrap(),
                ore[3].as_u64().unwrap(),
            ))
            .unwrap();
        let grid = s.sim.overlay_grid.as_mut().unwrap();
        grid.place_overlay(at.0, at.1, id, ore[4].as_u64().unwrap() as u8);
        grid.recalculate_runtime_cell(
            s.sim.resolved_terrain.as_mut().unwrap(),
            registry,
            at,
            crate::sim::overlay_grid::NavigationPublication::FrameBoundary,
        );
    }
    let frame = s.sim.session.binary_frame;
    s.sim.production.ore_growth_state = OreGrowthState::new(33, 33);
    s.sim
        .production
        .ore_growth_state
        .reset_native_tiberium_classes_for_rect((16, 16), 2, frame);
    for queued in input["spread_queued"].as_array().into_iter().flatten() {
        s.sim
            .production
            .ore_growth_state
            .mark_native_spread_bitmap_for_tests(0, cell(queued));
    }
    s.sim.production.ore_growth_config.spreads = input["spreads"] != false;
    {
        let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
        if let Some(archive) = input.get("archive").filter(|a| !a.is_null()) {
            let (x, y) = cell(archive);
            entity.set_archive_target(Some(TargetKind::Cell(x, y)));
        }
        entity.miner.as_mut().unwrap().harvesting = input["harvesting"] == true;
    }
    s.sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
    s
}

/// One row cell as the oracle records it: overlay ordinal (-1 when clear),
/// density byte and LandType.
fn cell_state(s: &Scene, at: (u16, u16)) -> Value {
    let overlay = s.sim.overlay_grid.as_ref().unwrap().cell(at.0, at.1);
    let land = s
        .sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(at.0, at.1)
        .unwrap()
        .land_type;
    serde_json::json!([
        overlay.overlay_id.map_or(-1, i64::from),
        overlay.overlay_data,
        land
    ])
}

/// Every compared field, in the oracle's names.
fn compare_state(s: &Scene, row: &Value, context: &str) {
    let native = &row["state"];
    for ore in row["input"]["ore"].as_array().unwrap() {
        let at = (
            ore[0].as_u64().unwrap() as u16,
            ore[1].as_u64().unwrap() as u16,
        );
        let key = format!("{},{}", at.0, at.1);
        assert_eq!(
            cell_state(s, at),
            native["cells"][&key],
            "{context}: cell {key}"
        );
    }
    let classes = &s
        .sim
        .production
        .ore_growth_state
        .native_tiberium_state()
        .classes;
    for (index, heap) in native["spread"].as_array().unwrap().iter().enumerate() {
        let mut expected: Vec<(u16, u16, u32)> = heap
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry[0][0].as_u64().unwrap() as u16,
                    entry[0][1].as_u64().unwrap() as u16,
                    (entry[1].as_f64().unwrap() as f32).to_bits(),
                )
            })
            .collect();
        let mut actual: Vec<(u16, u16, u32)> = classes[index]
            .spread
            .iter_heap()
            .map(|entry| (entry.rx, entry.ry, entry.priority_bits))
            .collect();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, expected, "{context}: spread queue {index}");
    }
    let view = s.sim.scenario_rng.logical_view();
    assert_eq!(
        serde_json::json!([view.index_a, view.index_b]),
        native["random_indices"],
        "{context}: Scenario RNG cursors"
    );
    let entity = s.sim.substrate.entities.get(s.miner).unwrap();
    let miner = entity.miner.as_ref().unwrap();
    let nav = match entity.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
        None => Value::Null,
        Some(other) => panic!("{context}: NavCom {other:?}"),
    };
    assert_eq!(nav, native["miner_nav"], "{context}: NavCom");
    let archive = match entity.archive_target() {
        Some(TargetKind::Cell(x, y)) => serde_json::json!([x, y]),
        None => Value::Null,
        Some(other) => panic!("{context}: archive {other:?}"),
    };
    assert_eq!(archive, native["archive"], "{context}: ArchiveTarget");
    assert_eq!(
        u64::from(miner.harvesting),
        native["harvesting"].as_u64().unwrap(),
        "{context}: Unit+0x6D2"
    );
    let start = if miner.stage_timer.is_armed() {
        i64::from(miner.stage_timer.start_frame)
    } else {
        -1
    };
    assert_eq!(
        serde_json::json!([
            miner.stage_value,
            start,
            miner.stage_timer.duration,
            miner.stage_rate
        ]),
        native["stage"],
        "{context}: StageClass"
    );
    let count = |kind| {
        miner
            .cargo
            .iter()
            .filter(|bale| bale.resource_type == kind)
            .count() as f64
    };
    let storage = native["storage"].as_array().unwrap();
    assert_eq!(
        (count(ResourceType::Ore), count(ResourceType::Gem)),
        (storage[0].as_f64().unwrap(), storage[1].as_f64().unwrap()),
        "{context}: cargo"
    );
    let owner = s.sim.interner.get("Americans").unwrap();
    assert_eq!(
        u64::from(s.sim.houses[&owner].harvester_no_ore),
        native["house_no_ore"].as_u64().unwrap(),
        "{context}: House+0x242"
    );
}

fn rows<'a>(corpus: &'a Value, group: &str) -> impl Iterator<Item = (&'a Value, String)> {
    corpus[group]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row, row["input"]["name"].as_str().unwrap().to_string()))
        .filter(|(_, name)| !SKIPPED.contains(&name.as_str()))
}

#[test]
fn reduce_tiberium_matches_the_original_cell_reduction() {
    let corpus = corpus();
    for (row, context) in rows(&corpus, "reduce") {
        let input = &row["input"];
        let mut s = row_scene(input);
        let at = cell(&input["cell"]);
        let outcome = s.sim.reduce_tiberium_at_with_native_context(
            at,
            input["amount"].as_i64().unwrap() as i32,
            Some(&s.rules),
            Some(registry()),
        );
        assert_eq!(
            u64::from(outcome.removed_amount),
            row["removed"].as_u64().unwrap(),
            "{context}: removed"
        );
        compare_state(&s, row, &context);
    }
}

/// The Can_Reach_Zone arguments the oracle records on every probe of a row
/// (source cell, MovementZone, ShouldBeOnBridge, destination bridge, fringe)
/// against the request the Rust scan derives, taken before the row runs.
fn compare_reach(s: &Scene, row: &Value, context: &str) {
    let Some(native) = row["reach_args"].as_array().and_then(|args| args.first()) else {
        return;
    };
    let reach = crate::sim::miner::ore_scan::harvest_reach(&s.sim, &s.rules, s.miner)
        .expect("reach request");
    let rust = serde_json::json!([
        [reach.source.0, reach.source.1],
        reach.movement_zone.map_or(-1, |zone| zone as i32),
        u8::from(reach.source_on_bridge),
        0,
        0
    ]);
    assert_eq!(&rust, native, "{context}: Can_Reach_Zone arguments");
}

#[test]
fn scan_for_tiberium_matches_the_original_ring_scan() {
    let corpus = corpus();
    for (row, context) in rows(&corpus, "scan") {
        let input = &row["input"];
        let mut s = row_scene(input);
        compare_reach(&s, row, &context);
        let found = crate::sim::miner::ore_scan::scan_for_tiberium(
            &mut s.sim,
            &s.rules,
            Some(registry()),
            s.miner,
            input["range"].as_i64().unwrap() as i32,
        );
        let found = found.map_or(serde_json::json!([0, 0]), |(x, y)| {
            serde_json::json!([x, y])
        });
        assert_eq!(found, row["found"], "{context}: found cell");
        compare_state(&s, row, &context);
    }
}

#[test]
fn search_for_tiberium_matches_the_original_search_and_move() {
    let corpus = corpus();
    for (row, context) in rows(&corpus, "search") {
        let input = &row["input"];
        let mut s = row_scene(input);
        compare_reach(&s, row, &context);
        let grid = s.sim.path_grid.clone();
        let ok = crate::sim::miner::ore_scan::search_for_tiberium_and_move(
            &mut s.sim,
            &s.rules,
            grid.as_deref(),
            Some(registry()),
            s.miner,
            input["range"].as_i64().unwrap() as i32,
        );
        assert_eq!(
            u64::from(ok),
            row["ok"].as_u64().unwrap(),
            "{context}: answer"
        );
        compare_state(&s, row, &context);
    }
}

#[test]
fn harvest_ore_tick_matches_the_original_tick() {
    let corpus = corpus();
    for (row, context) in rows(&corpus, "ore_tick") {
        let input = &row["input"];
        let mut s = row_scene(input);
        let ok = crate::sim::miner::miner_system::harvest_ore_tick_for_test(
            &mut s.sim,
            &s.rules,
            Some(registry()),
            s.miner,
        );
        assert_eq!(
            u64::from(ok),
            row["ok"].as_u64().unwrap(),
            "{context}: answer"
        );
        compare_state(&s, row, &context);
    }
}

#[test]
fn mission_harvest_states_zero_and_one_match_the_original_dispatch() {
    let corpus = corpus();
    for (row, context) in rows(&corpus, "harvest") {
        let input = &row["input"];
        let mut s = row_scene(input);
        compare_reach(&s, row, &context);
        let config = crate::sim::miner::MinerConfig::from_rules(&s.rules);
        let grid = s.sim.path_grid.clone();
        let frame = s.sim.session.binary_frame;
        crate::sim::miner::dispatch_harvest_for_object(
            &mut s.sim,
            &s.rules,
            &config,
            grid.as_deref(),
            Some(registry()),
            s.miner,
        );
        let entity = s.sim.substrate.entities.get(s.miner).unwrap();
        let timer = entity.mission.dispatch_timer();
        assert_eq!(
            timer.start_frame(),
            frame as i32,
            "{context}: epilogue frame"
        );
        assert_eq!(
            i64::from(timer.delay()),
            row["delay"].as_i64().unwrap(),
            "{context}: delay"
        );
        let status = entity.miner_state().map(MinerState::cursor);
        assert_eq!(
            status.map(u64::from),
            row["state"]["miner_status"].as_u64(),
            "{context}: Harvest cursor"
        );
        if input["cmin"] == true {
            // The Unit setter's NULL destination on a Teleporter piggybacks
            // a Drive over the Teleport (the oracle's `begin_piggyback`).
            let piggybacked = input["loco"] == "drive_piggy"
                || row["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event[0] == "begin_piggyback");
            assert_eq!(
                entity.locomotor.as_ref().unwrap().active_kind()
                    == crate::rules::locomotor_type::LocomotorKind::Drive,
                piggybacked,
                "{context}: active locomotor"
            );
        }
        compare_state(&s, row, &context);
    }
}

#[test]
fn replay_covers_every_row() {
    let corpus = corpus();
    let count = |group: &str| corpus[group].as_array().unwrap().len();
    assert_eq!(count("reduce"), 16);
    assert_eq!(count("scan"), 19);
    assert_eq!(count("search"), 4);
    assert_eq!(count("ore_tick"), 11);
    assert_eq!(count("harvest"), 23);
}
