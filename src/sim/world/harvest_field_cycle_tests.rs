//! War Miner ore-field visits through the production frame (`advance_tick`)
//! on the oracle replay's scene (`harvest_field_oracle_tests::row_scene`):
//! Mission_Harvest state 0 finding ore and driving to it, state 1 cutting a
//! bale per StageClass gate, the hop to the next cell and the return once
//! the field runs dry. The per-step behaviour is pinned row by row against
//! gamemd in `harvest_field_oracle_tests`; these pin how the steps compose
//! over frames.

use super::harvest_field_oracle_tests::{registry, row_scene};
use super::refinery_dock_oracle_tests::Scene;
use crate::sim::miner::{MinerState, ResourceType};
use crate::sim::ore_growth::OreGrowthConfig;
use std::collections::BTreeMap;

/// One miner's observable state after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    cell: (u16, u16),
    state: Option<MinerState>,
    bales: usize,
    harvesting: bool,
    driving: bool,
}

fn frame(s: &mut Scene) -> Sample {
    let grid = s.sim.path_grid_snapshot();
    s.sim.advance_tick(
        &[],
        Some(&s.rules),
        &BTreeMap::new(),
        grid.as_deref(),
        Some(registry()),
        67,
    );
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let state = miner.miner.as_ref().unwrap();
    Sample {
        cell: (miner.position.rx, miner.position.ry),
        state: miner.miner_state(),
        bales: state.cargo.len(),
        harvesting: state.harvesting,
        driving: miner.navigation.nav_com.is_some(),
    }
}

/// A War Miner on Mission_Harvest state 0 at `at` with `ore` cells
/// (`[x, y, tiberium, variant, OverlayData]`), no growth or spread, standing
/// on its cell as a production unit does.
fn field(at: (u16, u16), ore: serde_json::Value) -> Scene {
    let mut s = row_scene(&serde_json::json!({
        "name": "field",
        "mission": "harvest",
        "status": 0,
        "miner_cell": [at.0, at.1],
        "unlimbo_at_cell": true,
        "ore": ore,
    }));
    s.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    s
}

fn bales_at(s: &Scene, cell: (u16, u16)) -> Option<u8> {
    let overlay = s.sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
    overlay.overlay_id.map(|_| overlay.overlay_data)
}

/// On ore at once: the first dispatch arms the stage (state 0's literal rate
/// 2) and each later bale comes one gate (9 steps of `HarvesterLoadRate` 2,
/// plus the dispatch-before-tick frame) after the last. A cell of density 2
/// gives two bales; the third gate clears it for nothing and the miner hops to
/// the neighbour, where it cuts on the frame after arrival: the stage was
/// never re-armed. With the field empty, state 1 sends it home partly full.
#[test]
fn war_miner_cuts_a_bale_per_gate_hops_without_waiting_and_returns_when_dry() {
    let mut s = field(
        (15, 15),
        serde_json::json!([[15, 15, 0, 0, 2], [16, 15, 0, 0, 2]]),
    );
    let mut samples = Vec::new();
    for _ in 0..400 {
        samples.push(frame(&mut s));
    }
    assert_eq!(
        samples[0].state,
        Some(MinerState::Harvest),
        "on ore at once"
    );
    assert!(samples[0].harvesting);
    let bale_frames: Vec<usize> = (1..samples.len())
        .filter(|&n| samples[n].bales > samples[n - 1].bales)
        .collect();
    assert_eq!(
        bale_frames.len(),
        4,
        "two bales from each cell: {bale_frames:?}"
    );
    // The stage armed at frame 0's dispatch counts 9 steps of 2; the dispatch
    // after the ninth cuts: 19 frames later, and 19 between the two cuts.
    assert_eq!(bale_frames[0], 19);
    assert_eq!(bale_frames[1] - bale_frames[0], 19);
    assert_eq!(
        bales_at(&s, (15, 15)),
        None,
        "the third gate cleared the cell"
    );
    // The hop: the third gate (19 frames after the second cut) sends it east.
    let hop = bale_frames[1] + 19;
    assert!(samples[hop].driving && samples[hop].harvesting);
    assert_eq!(
        samples[hop].state,
        Some(MinerState::Harvest),
        "state 1 holds for the hop"
    );
    let arrival = (hop..samples.len())
        .find(|&n| !samples[n].driving)
        .expect("the hop ends");
    assert_eq!(samples[arrival].cell, (16, 15));
    assert!(
        bale_frames[2] <= arrival + 1,
        "the first bale on the new cell comes on arrival, not a gate later \
         (arrival {arrival}, bale {})",
        bale_frames[2]
    );
    assert_eq!(bale_frames[3] - bale_frames[2], 19);
    let end = samples.last().unwrap();
    assert_eq!(end.bales, 4);
    assert_eq!(
        end.state,
        Some(MinerState::ReturnToRefinery),
        "no ore within TiberiumShortScan: home with a part load"
    );
    assert!(!end.harvesting);
}

/// From each side of a density-1 target with ore behind it: state 0 drives
/// onto the target (the nearer ring), cuts only on arrival, and the bale and
/// the cleared overlay are the target's; the cell behind keeps its ore.
#[test]
fn war_miner_cuts_the_cell_it_drove_onto_from_every_side() {
    let target = (16u16, 16u16);
    for (label, start, behind) in [
        ("west", (15u16, 16u16), (17u16, 16u16)),
        ("east", (17, 16), (15, 16)),
        ("north", (16, 15), (16, 17)),
        ("south", (16, 17), (16, 15)),
    ] {
        let mut s = field(
            start,
            serde_json::json!([[target.0, target.1, 0, 0, 1], [behind.0, behind.1, 0, 0, 1]]),
        );
        let mut cut = None;
        for n in 0..200 {
            let sample = frame(&mut s);
            if sample.bales > 0 {
                cut = Some((n, sample));
                break;
            }
        }
        let (_, sample) = cut.unwrap_or_else(|| panic!("{label}: never cut"));
        assert_eq!(sample.cell, target, "{label}: cut where it stands");
        let miner = s.sim.substrate.entities.get(s.miner).unwrap();
        assert_eq!(
            (
                miner.position.sub_x.to_num::<i32>(),
                miner.position.sub_y.to_num::<i32>()
            ),
            (128, 128),
            "{label}: at the cell centre"
        );
        assert_eq!(
            bales_at(&s, target),
            Some(0),
            "{label}: the target lost its level"
        );
        assert_eq!(
            bales_at(&s, behind),
            Some(1),
            "{label}: the cell behind is untouched"
        );
        assert_eq!(
            miner.miner.as_ref().unwrap().cargo[0].resource_type,
            ResourceType::Ore
        );
    }
}

/// Retail RULESMD.INI through the production readers: `TiberiumShortScan=6`
/// and `TiberiumLongScan=48` become 1536 and 12288 leptons (`ReadRange`),
/// `HarvesterLoadRate` stays the constructor's 2, and a retail War Miner cuts
/// on the same 19-frame gate.
#[test]
fn war_miner_gate_on_retail_rules() {
    let Some((rules_ini, _art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let retail = crate::rules::ruleset::RuleSet::from_ini(&rules_ini).unwrap();
    assert_eq!(retail.general.tiberium_short_scan, 1536);
    assert_eq!(retail.general.tiberium_long_scan, 12288);
    assert_eq!(retail.general.harvester_load_rate, 2);
    let mut s = field((15, 15), serde_json::json!([[15, 15, 0, 0, 3]]));
    s.rules.general.tiberium_short_scan = retail.general.tiberium_short_scan;
    s.rules.general.tiberium_long_scan = retail.general.tiberium_long_scan;
    s.rules.general.harvester_load_rate = retail.general.harvester_load_rate;
    let mut last = 0;
    let mut gaps = Vec::new();
    let mut bales = 0;
    for n in 0..120 {
        let sample = frame(&mut s);
        if sample.bales > bales {
            gaps.push(n - last);
            last = n;
            bales = sample.bales;
        }
    }
    assert_eq!(gaps, vec![19, 19, 19], "three bales, one per gate");
}

/// Frames until a War Miner on `field` has cut its first bale, still cutting.
fn until_first_bale(s: &mut Scene) {
    for _ in 0..60 {
        let sample = frame(s);
        if sample.bales > 0 {
            assert!(sample.harvesting && sample.state == Some(MinerState::Harvest));
            return;
        }
    }
    panic!("no bale");
}

fn order(s: &mut Scene, command: crate::sim::command::Command) {
    let grid = s.sim.path_grid_snapshot();
    assert!(s.sim.apply_command_with_overlays(
        "Americans",
        &command,
        Some(&s.rules),
        grid.as_deref(),
        &BTreeMap::new(),
        Some(registry()),
    ));
}

/// A player return order on a cutting miner: its mission becomes the
/// return's (VERA's ForcedReturn cursor for the native Enter), and
/// `UnitClass::AI` (`0x007365BB..0x007365D8`) clears Unit+0x6D2 on the next
/// frame, so OREGATH is gone while the miner waits at rest for its dock.
#[test]
fn a_return_order_ends_the_harvesting_byte_and_oregath() {
    let mut s = field((15, 15), serde_json::json!([[15, 15, 0, 0, 5]]));
    until_first_bale(&mut s);
    let miner = s.miner;
    order(
        &mut s,
        crate::sim::command::Command::MinerReturn {
            entity_id: miner,
            target_refinery_id: None,
        },
    );
    let sample = frame(&mut s);
    assert!(!sample.harvesting, "Unit+0x6D2 cleared by the unit AI");
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    assert!(
        miner.harvest_overlay.as_ref().is_none_or(|ho| !ho.visible),
        "OREGATH hidden with the byte"
    );
}

/// A harvest order on a miner already cutting (`Queue_Mission` keeps the
/// same mission, `0x005B3601..0x005B3612`): state 1 carries on, treats the
/// drive to the clicked cell as a hop and cuts on arrival; the gate it had
/// already counted is not re-armed.
#[test]
fn a_harvest_order_on_a_cutting_miner_keeps_state_one_and_cuts_on_arrival() {
    let mut s = field(
        (15, 15),
        serde_json::json!([[15, 15, 0, 0, 5], [19, 15, 0, 0, 5]]),
    );
    until_first_bale(&mut s);
    // Four cells away: the drive outlasts the gate armed by the first cut.
    let miner = s.miner;
    order(
        &mut s,
        crate::sim::command::Command::HarvestCell {
            entity_id: miner,
            target_rx: 19,
            target_ry: 15,
        },
    );
    let mut samples = Vec::new();
    for _ in 0..200 {
        samples.push(frame(&mut s));
    }
    assert!(
        samples
            .iter()
            .all(|sample| sample.state == Some(MinerState::Harvest)),
        "state 1 throughout: {samples:?}"
    );
    let arrival = samples
        .iter()
        .position(|sample| sample.cell == (19, 15) && !sample.driving)
        .expect("reaches the clicked cell");
    let cut = samples
        .iter()
        .position(|sample| sample.cell == (19, 15) && sample.bales > samples[0].bales)
        .expect("cuts there");
    assert!(
        cut <= arrival + 1,
        "cuts on arrival (arrival {arrival}, cut {cut})"
    );
    assert_eq!(
        bales_at(&s, (15, 15)),
        Some(4),
        "the first cell kept its ore"
    );
}
