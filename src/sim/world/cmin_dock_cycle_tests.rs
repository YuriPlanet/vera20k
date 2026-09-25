//! Chrono Miner refinery visits through the production frame (`advance_tick`)
//! on the oracle replay's scene (a single-dock refinery at NW (6, 9), pad
//! (9, 10)): Harvest's return states, Mission_Enter's MOVE_HERE warping the
//! miner onto the pad, the hull turn, the unload and the departure.

use super::cmin_dock_oracle_tests::cmin_rules;
use super::refinery_dock_oracle_tests::{Scene, scene_with};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::{CargoBale, MinerState, ResourceType};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{RadioMessage, RadioResponse, TransmitRecord};
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
    /// The miner sent its refinery OVER_OUT during the frame.
    released: bool,
    /// ChronoTeleport sound cells emitted during the frame.
    chrono_sounds: Vec<(u16, u16)>,
}

/// Advance one production frame; returns its radio transmissions.
fn step(s: &mut Scene) -> Vec<TransmitRecord> {
    crate::sim::radio::take_transmit_log();
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
    crate::sim::radio::take_transmit_log()
}

fn frame(s: &mut Scene) -> Sample {
    let sounds_before = s.sim.sound_events.len();
    let transmits = step(s);
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
        released: transmits.iter().any(|r| {
            r.sender_sid == s.miner
                && r.target_sid == s.refinery
                && r.msg == RadioMessage::Break.code()
        }),
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
/// Teleport locomotor and in contact with the refinery. It tethers, faces
/// east before the first dump, is paid 500 credits for 20 ore bales and
/// leaves the refinery's slot free.
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
    assert_ne!(before, PAD);
    assert_eq!(
        samples[warp].chrono_sounds,
        vec![before, PAD],
        "the pad is reached by a warp"
    );
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
            assert!(!sample.released, "no track end drops the contact");
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

/// Take the scene's second refinery (NW (20, 20)) offline, so CAN_LOAD
/// refuses it and the scene has one dock.
fn one_dock(s: &mut Scene) {
    s.sim.substrate.entities.get_mut(s.other).unwrap().temporal =
        crate::sim::temporal::TemporalState::warped_by_for_test(s.miner);
}

/// A second full Chrono Miner in Mission_Harvest's FINDING_HOME state.
fn spawn_returning_cmin(s: &mut Scene, cell: (u16, u16)) -> u64 {
    let id = s
        .sim
        .spawn_object(
            "CMIN",
            "Americans",
            cell.0,
            cell.1,
            0,
            &s.rules,
            &BTreeMap::new(),
        )
        .expect("second miner");
    let now = s.sim.session.binary_frame;
    s.sim
        .mission_assign_exact(id, MissionId::from_known(MissionType::Harvest), now)
        .unwrap();
    let entity = s.sim.substrate.entities.get_mut(id).unwrap();
    entity
        .mission
        .set_handler_state(MinerState::ReturnToRefinery.cursor());
    entity.miner.as_mut().unwrap().cargo = vec![
        CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        };
        20
    ];
    id
}

fn tethered(s: &Scene, id: u64) -> bool {
    s.sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .dock_entered_with
        .is_some()
}

/// Run both miners' visits to the one dock until both loads are paid and
/// both have left. Returns each miner's (id, frame) tether and untether
/// edges in order. No HELLO to the refinery is refused on the way: the
/// narrow pass admits only a bay with a free slot (`0x004DEF09`).
fn run_two_visits(s: &mut Scene, second: u64) -> (Vec<(u64, usize)>, Vec<(u64, usize)>) {
    let paid_before = credits(s);
    let miners = [s.miner, second];
    let mut docked = Vec::new();
    let mut left = Vec::new();
    let mut was = [false; 2];
    for n in 0..3000 {
        let transmits = step(s);
        assert!(
            !transmits.iter().any(|r| r.target_sid == s.refinery
                && r.msg == RadioMessage::Hello.code()
                && r.reply == Some(RadioResponse::Negatory.code())),
            "frame {n}: a HELLO was refused"
        );
        for (slot, id) in miners.into_iter().enumerate() {
            let now = tethered(s, id);
            if now && !was[slot] {
                docked.push((id, n));
            }
            if !now && was[slot] {
                left.push((id, n));
            }
            was[slot] = now;
        }
        if credits(s) - paid_before == 1000 && left.len() == 2 {
            break;
        }
    }
    assert_eq!(credits(s) - paid_before, 1000, "both loads pay");
    assert_eq!(docked.len(), 2, "each miner docks once: {docked:?}");
    let (first, first_docked) = docked[0];
    let (next, next_docked) = docked[1];
    assert_ne!(first, next);
    let first_left = left
        .iter()
        .find(|(id, _)| *id == first)
        .map(|&(_, n)| n)
        .expect("the first miner leaves");
    assert!(
        first_docked < first_left && first_left <= next_docked,
        "one miner on the pad at a time: {docked:?} / {left:?}"
    );
    let refinery = s.sim.substrate.entities.get(s.refinery).unwrap();
    assert!(refinery.radio_contacts.is_empty(), "the slot is free again");
    (docked, left)
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
/// (`0x0073EB3A`). The Drive rolls on, and a track end reached before state 3
/// queues Enter sends the refinery OVER_OUT (Per_Cell_Process(2),
/// `0x0073ACD7`): Enter then finds no target, and the Guard override's
/// full-and-moving Teleporter arm (`0x00740810`) brings Harvest back to HELLO
/// from further in. Only an Enter queued before the next track end keeps the
/// contact, and its MOVE_HERE warps the miner onto the pad. How many such
/// cycles the approach takes, and so where the warp starts, is frame timing
/// (the Drive's speed against the dispatch delays) that no oracle row covers;
/// the test pins the sequence's shape, not its frames.
#[test]
fn far_chrono_miner_drives_in_then_warps_onto_the_pad() {
    let mut s = fixture_scene((28, 26));
    // The scene's second refinery (NW (20, 20)) would be the nearer bay.
    one_dock(&mut s);
    let paid_before = credits(&s);
    let samples = assert_whole_visit(&mut s, true);
    let warp = samples.iter().position(|x| x.cell == PAD).unwrap();
    assert!(
        samples[..warp]
            .iter()
            .any(|x| x.active == LocomotorKind::Drive),
        "the far return drives"
    );
    assert!(
        samples[..warp].iter().any(|x| x.released),
        "a track end dropped the contact the moving miner's HELLO made"
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

/// Two full Chrono Miners, one dock. While the first holds the slot the
/// narrow pass finds no bay for the second, so it sends no HELLO and drives
/// toward the queueing cell instead (a Teleporter never waits in place,
/// `0x0073ECD0`). Once the first has left, the second's HELLO lands and the
/// MOVE_HERE warps it onto the pad. Both loads pay.
#[test]
fn a_second_chrono_miner_warps_in_after_the_first_leaves() {
    let mut s = fixture_scene((14, 12));
    one_dock(&mut s);
    let second = spawn_returning_cmin(&mut s, (15, 15));
    let (docked, _) = run_two_visits(&mut s, second);
    assert_eq!(docked[0].0, s.miner, "the nearer miner docks first");
}

/// A player refinery order (`Command::MinerReturn`) for a second Chrono Miner
/// while the first unloads pins the busy refinery. The pin still goes
/// through the narrow pass's free-slot gate, so the order sends no HELLO
/// that the refinery would refuse and does not stop a driving miner; the
/// miner warps in once the first has left.
#[test]
fn a_chrono_miner_ordered_to_a_busy_refinery_warps_in_after_it_frees() {
    let mut s = fixture_scene((14, 12));
    one_dock(&mut s);
    for _ in 0..1500 {
        frame(&mut s);
        if tethered(&s, s.miner) {
            break;
        }
    }
    assert!(tethered(&s, s.miner), "the first miner docks");
    let second = spawn_returning_cmin(&mut s, (15, 15));
    assert!(s.sim.apply_command(
        "Americans",
        &crate::sim::command::Command::MinerReturn {
            entity_id: second,
            target_refinery_id: Some(s.refinery),
        },
        Some(&s.rules),
        None,
        &BTreeMap::new(),
    ));
    let paid_before = credits(&s);
    let mut second_docked = None;
    let mut first_left = None;
    for n in 0..3000 {
        let transmits = step(&mut s);
        assert!(
            !transmits.iter().any(|r| r.sender_sid == second
                && r.msg == RadioMessage::Hello.code()
                && r.reply == Some(RadioResponse::Negatory.code())),
            "frame {n}: the pinned bay refused a HELLO"
        );
        if first_left.is_none() && !tethered(&s, s.miner) {
            first_left = Some(n);
        }
        if second_docked.is_none() && tethered(&s, second) {
            second_docked = Some(n);
        }
        if credits(&s) - paid_before == 1000 && second_docked.is_some() && !tethered(&s, second) {
            break;
        }
    }
    let first_left = first_left.expect("the first miner leaves");
    let second_docked = second_docked.expect("the ordered miner docks");
    assert!(first_left <= second_docked);
    assert_eq!(
        credits(&s) - paid_before,
        1000,
        "the rest of the first load and the whole second load"
    );
}

/// Selling the refinery under an unloading Chrono Miner: the sale's RUN_AWAY
/// (`0x0044AB5A`) drops the latch and hands the miner to Harvest
/// (`0x00737A98`) with its cargo, as for the War Miner. It stays on the pad:
/// the Foot arm's Unit Scatter (`0x00743A50`) refuses the active Teleport.
#[test]
fn selling_the_refinery_mid_unload_hands_the_chrono_miner_to_harvest() {
    let mut s = fixture_scene((14, 12));
    let mut unloading = false;
    for _ in 0..1500 {
        unloading = frame(&mut s).unloading;
        if unloading {
            break;
        }
    }
    assert!(unloading, "the miner starts unloading");
    let before = s
        .sim
        .substrate
        .entities
        .get(s.miner)
        .unwrap()
        .miner
        .as_ref()
        .unwrap()
        .cargo
        .len();
    let refinery = s.refinery;
    assert!(crate::sim::production::sell_building(
        &mut s.sim, &s.rules, refinery
    ));
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let state = miner.miner.as_ref().unwrap();
    assert!(!state.unload_active, "RUN_AWAY dropped the latch");
    assert_eq!(miner.dock_entered_with, None);
    assert_eq!(miner.radio_contacts.slot(0), None);
    assert_eq!(state.cargo.len(), before, "nothing more was dumped");
    assert_eq!(
        miner.mission.current(),
        MissionId::from_known(MissionType::Harvest)
    );
    assert_eq!((miner.position.rx, miner.position.ry), PAD);
    assert_eq!(
        miner.locomotor.as_ref().unwrap().active_kind(),
        LocomotorKind::Teleport
    );
}
