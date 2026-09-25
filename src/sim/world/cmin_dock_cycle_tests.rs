//! Chrono Miner refinery visits through the production frame (`advance_tick`)
//! on the oracle replay's scene (a single-dock refinery at NW (6, 9), pad
//! (9, 10)): Harvest's return states, Mission_Enter's MOVE_HERE warping the
//! miner onto the pad, the hull turn, the unload and the departure.

use super::cmin_dock_oracle_tests::cmin_rules;
use super::refinery_dock_oracle_tests::{Scene, scene_with};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::ResourceType;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::SimSoundEvent;
use std::collections::BTreeMap;

const PAD: (u16, u16) = (9, 10);

/// One miner's observable state after a frame.
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    cell: (u16, u16),
    mission: MissionId,
    contact: Option<u64>,
    tethered: bool,
    unloading: bool,
    facing: u16,
    ore: usize,
    active: LocomotorKind,
    force_reassign: bool,
    /// ChronoTeleport sound cells emitted during the frame.
    chrono_sounds: Vec<(u16, u16)>,
}

fn frame(s: &mut Scene) -> Sample {
    let sounds_before = s.sim.sound_events.len();
    let overlay = crate::sim::tiberium::test_support::overlay_registry();
    let grid = s.sim.path_grid_snapshot();
    s.sim.advance_tick(
        &[],
        Some(&s.rules),
        &BTreeMap::new(),
        grid.as_deref(),
        Some(overlay),
        67,
    );
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let state = miner.miner.as_ref().unwrap();
    Sample {
        cell: (miner.position.rx, miner.position.ry),
        mission: miner.mission.current(),
        contact: miner.radio_contacts.slot(0),
        tethered: miner.dock_entered_with.is_some(),
        unloading: state.unload_active,
        facing: miner
            .body_facing
            .as_ref()
            .map_or(0, |f| f.current(s.sim.session.binary_frame)),
        ore: state
            .cargo
            .iter()
            .filter(|b| b.resource_type == ResourceType::Ore)
            .count(),
        active: miner.locomotor.as_ref().unwrap().active_kind(),
        force_reassign: miner.setter_force_reassign,
        chrono_sounds: s.sim.sound_events[sounds_before..]
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::ChronoTeleport { rx, ry, .. } => Some((*rx, *ry)),
                _ => None,
            })
            .collect(),
    }
}

fn credits(s: &Scene) -> i32 {
    let owner = s.sim.interner.get("Americans").unwrap();
    s.sim.houses[&owner].economy.credits
}

/// A full Chrono Miner in Mission_Harvest's FINDING_HOME state at `cell`.
fn returning_input(cell: (u16, u16)) -> serde_json::Value {
    serde_json::json!({
        "name": "cmin cycle",
        "miner_type": "CMIN",
        "linked": false,
        "mission": "harvest",
        "status": 2,
        "miner_cell": [cell.0, cell.1],
        "unlimbo_at_cell": true,
        "storage": [20.0, 0.0],
    })
}

/// One whole visit. Before the warp the miner holds still (`drives` false)
/// or drives one cell at a time; the warp lands it on the pad in one frame
/// with ChronoOut at its old cell and ChronoIn on the pad, still on the
/// Teleport locomotor. It tethers, faces east before the first dump, is paid
/// 500 credits for 20 ore bales and leaves the refinery's slot free.
fn assert_whole_visit(s: &mut Scene, drives: bool) -> Vec<Sample> {
    let start = {
        let position = &s.sim.substrate.entities.get(s.miner).unwrap().position;
        (position.rx, position.ry)
    };
    let mut samples = Vec::new();
    for _ in 0..1500 {
        samples.push(frame(s));
    }
    let warp = samples
        .iter()
        .position(|x| x.cell == PAD)
        .expect("the miner reaches the pad");
    let before = if warp == 0 {
        start
    } else {
        samples[warp - 1].cell
    };
    assert!(
        before.0.abs_diff(PAD.0) > 1 || before.1.abs_diff(PAD.1) > 1,
        "the pad is reached by a warp, from {before:?}"
    );
    assert_eq!(samples[warp].chrono_sounds, vec![before, PAD]);
    assert_eq!(samples[warp].active, LocomotorKind::Teleport);
    assert_eq!(samples[warp].contact, Some(s.refinery));
    let mut last = start;
    for sample in &samples[..warp] {
        assert!(sample.chrono_sounds.is_empty(), "one warp only");
        if drives {
            assert!(
                sample.cell.0.abs_diff(last.0) <= 1 && sample.cell.1.abs_diff(last.1) <= 1,
                "the approach drives ({last:?} -> {:?})",
                sample.cell
            );
        } else {
            assert_eq!(
                sample.cell, last,
                "a close return warps from where it stands"
            );
        }
        last = sample.cell;
    }
    let docked = samples
        .iter()
        .find(|x| x.tethered)
        .expect("the miner tethers to the refinery");
    assert_eq!(docked.cell, PAD, "tethered on the pad");
    let first_pay = samples
        .iter()
        .position(|x| x.ore < 20)
        .expect("the unload pays");
    assert!(first_pay > warp);
    assert_eq!(
        samples[first_pay].mission,
        MissionId::from_known(MissionType::Unload)
    );
    assert_eq!(
        samples[first_pay - 1].facing,
        0x4000,
        "the hull faces east before the first dump"
    );
    let end = samples.last().unwrap();
    assert_eq!(end.ore, 0);
    assert!(!end.tethered);
    assert_eq!(end.contact, None);
    assert_ne!(end.mission, MissionId::from_known(MissionType::Unload));
    let refinery = s.sim.substrate.entities.get(s.refinery).unwrap();
    assert!(refinery.radio_contacts.is_empty(), "the slot is free again");
    assert_eq!(refinery.dock_entered_with, None);
    samples
}

fn fixture_scene(cell: (u16, u16)) -> Scene {
    let input = returning_input(cell);
    let (rules, ini) = cmin_rules(&input);
    scene_with(&input, rules, &ini)
}

/// Within `ChronoHarvTooFarDistance` (10 cells here) the HELLO goes out from
/// the harvest cell; the refinery's MOVE_HERE then warps the miner onto the
/// pad.
#[test]
fn chrono_miner_warps_onto_the_pad_and_unloads_through_the_production_frame() {
    let mut s = fixture_scene((14, 12));
    let paid_before = credits(&s);
    assert_whole_visit(&mut s, false);
    assert_eq!(credits(&s) - paid_before, 500, "20 ore bales at 25");
}

/// Beyond `ChronoHarvTooFarDistance` the miner drives toward the staging cell
/// (NW + `QueueingCell=`) on a Drive piggyback. Once a free bay is within the
/// distance, Harvest's Teleporter branch drops NavCom mid-drive and HELLOs
/// (`0x0073EB3A`); the MOVE_HERE that follows finds the Drive still moving,
/// so the Teleporter arm stops it, raises Techno+0x1F8 and queues Enter
/// (`0x0074258C`). The stopped Drive ends at the FootClass::AI tail and the
/// Enter redispatch warps onto the pad.
#[test]
fn far_chrono_miner_drives_in_then_warps_onto_the_pad() {
    let mut s = fixture_scene((28, 26));
    // The scene's second refinery (NW (20, 20)) would be the nearer bay:
    // offline, CAN_LOAD refuses it.
    s.sim.substrate.entities.get_mut(s.other).unwrap().temporal =
        crate::sim::temporal::TemporalState::warped_by_for_test(s.miner);
    let paid_before = credits(&s);
    let samples = assert_whole_visit(&mut s, true);
    assert!(
        samples.iter().any(|x| x.active == LocomotorKind::Drive),
        "the far return drives"
    );
    assert!(
        samples.iter().any(|x| x.force_reassign),
        "MOVE_HERE met a moving Drive"
    );
    assert_eq!(credits(&s) - paid_before, 500, "20 ore bales at 25");
}

/// The close return on retail RULESMD.INI and ARTMD.INI through the production
/// readers: CMIN (`Teleporter=yes`, the Teleport locomotor, `Storage=20`,
/// `ChronoInSound=`/`ChronoOutSound=ChronoMinerTeleport`, `Dock=NAREFN,GAREFN`)
/// and `[General] ChronoHarvTooFarDistance=50`.
#[test]
fn chrono_miner_visit_on_retail_rules() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&rules_ini).unwrap();
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let cmin = rules.object("CMIN").unwrap();
    assert!(cmin.teleporter && cmin.harvester);
    assert_eq!(cmin.locomotor, LocomotorKind::Teleport);
    assert_eq!(cmin.storage, 20);
    assert_eq!(cmin.chrono_in_sound.as_deref(), Some("ChronoMinerTeleport"));
    assert_eq!(rules.general.chrono_harv_too_far_distance, 50);
    let mut s = scene_with(&returning_input((14, 12)), rules, &rules_ini);
    let paid_before = credits(&s);
    assert_whole_visit(&mut s, false);
    assert_eq!(credits(&s) - paid_before, 500, "20 ore bales at 25");
}
