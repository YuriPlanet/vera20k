//! War Miner refinery visits through the production frame (`advance_tick`):
//! Harvest's return states, Mission_Enter's docking handshake and pad drive,
//! the hull turn, Mission_Unload's dumps and the departure, on the oracle
//! replay's scene (a single-dock refinery at NW (6, 9), pad (9, 10)).

use super::refinery_dock_oracle_tests::{Scene, scene, scene_with};
use crate::rules::ruleset::RuleSet;
use crate::sim::miner::{CargoBale, MinerState, ResourceType};
use crate::sim::mission::{MissionId, MissionType};
use std::collections::BTreeMap;

/// One miner's observable state after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    cell: (u16, u16),
    mission: MissionId,
    contact: Option<u64>,
    tethered: bool,
    unloading: bool,
    facing: u16,
    ore: usize,
}

fn sample(s: &Scene, id: u64) -> Sample {
    let miner = s.sim.substrate.entities.get(id).unwrap();
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
    }
}

/// Run frames until the miner's unload latch is up (the Unload first pass).
fn run_until_unloading(s: &mut Scene) {
    for _ in 0..1500 {
        frame(s);
        if sample(s, s.miner).unloading {
            return;
        }
    }
    panic!("the miner never started unloading");
}

fn credits(s: &Scene) -> i32 {
    let owner = s.sim.interner.get("Americans").unwrap();
    s.sim.houses[&owner].economy.credits
}

/// Advance one production frame; returns the owner's credits after it.
fn frame(s: &mut Scene) -> i32 {
    let heights = BTreeMap::new();
    let overlay = crate::sim::tiberium::test_support::overlay_registry();
    let grid = s.sim.path_grid_snapshot();
    s.sim.advance_tick(
        &[],
        Some(&s.rules),
        &heights,
        grid.as_deref(),
        Some(overlay),
        67,
    );
    credits(s)
}

/// A second full War Miner in Mission_Harvest's FINDING_HOME state.
fn spawn_returning_miner(s: &mut Scene, cell: (u16, u16)) -> u64 {
    let id = s
        .sim
        .spawn_object(
            "HARV",
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
        40
    ];
    id
}

fn returning_input() -> serde_json::Value {
    serde_json::json!({
        "name": "cycle",
        "linked": false,
        "mission": "harvest",
        "status": 2,
        "miner_cell": [14, 12],
        "storage": [40.0, 0.0],
    })
}

fn returning_scene() -> Scene {
    scene(&returning_input())
}

/// One whole visit: tethered on the pad, east before the first dump, 1000
/// credits for 40 ore, and the refinery's slot free again afterwards.
fn assert_whole_visit(s: &mut Scene) {
    let mut samples = Vec::new();
    for _ in 0..1500 {
        let paid = frame(s);
        samples.push((sample(s, s.miner), paid));
    }
    let (docked, _) = samples
        .iter()
        .find(|(x, _)| x.tethered)
        .expect("the miner tethers to the refinery");
    assert_eq!(docked.cell, (9, 10), "tethered on the pad");
    assert_eq!(docked.contact, Some(s.refinery));
    let latch = samples
        .iter()
        .position(|(x, _)| x.unloading)
        .expect("the Unload first pass raises the latch");
    let first_pay_frame = samples
        .iter()
        .position(|(_, paid)| *paid > 0)
        .expect("the unload pays");
    // The first pass arms the StageClass at rate 1; the stage ticks after each
    // dispatch (0x006FABC4), so the dump dispatch sees Value 15 sixteen frames
    // after the first pass (`stage_tick` oracle rows).
    assert_eq!(
        first_pay_frame - latch,
        16,
        "first dump at the first pass + 16"
    );
    let (first_pay, _) = &samples[first_pay_frame];
    assert_eq!(
        first_pay.mission,
        MissionId::from_known(MissionType::Unload)
    );
    assert_eq!(
        first_pay.facing, 0x4000,
        "the hull faces east before the first dump"
    );
    let (end, paid) = samples.last().unwrap();
    assert_eq!(end.ore, 0);
    assert_eq!(*paid, 1000, "40 ore bales at 25");
    assert!(!end.tethered);
    assert_eq!(end.contact, None);
    assert_ne!(end.mission, MissionId::from_known(MissionType::Unload));
    let refinery = s.sim.substrate.entities.get(s.refinery).unwrap();
    assert!(refinery.radio_contacts.is_empty(), "the slot is free again");
    assert_eq!(refinery.dock_entered_with, None);
}

#[test]
fn war_miner_docks_unloads_and_leaves_through_the_production_frame() {
    assert_whole_visit(&mut returning_scene());
}

/// The same visit on retail RULESMD.INI and ARTMD.INI through the production
/// readers: HARV (`Dock=NAREFN,GAREFN`, `Storage=40`, `Turret=yes`), GAREFN
/// (`DockUnload=yes`, `NumberImpassableRows=3`, art `QueueingCell=4,1`) and
/// the constructor `HarvesterDumpRate` the retail rules leave unset.
#[test]
fn war_miner_visit_on_retail_rules() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&rules_ini).unwrap();
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let refinery = rules.object("GAREFN").unwrap();
    assert!(refinery.dock_unload && refinery.refinery);
    assert_eq!(refinery.queueing_cell, [4, 1]);
    assert_eq!(refinery.number_impassable_rows, 3);
    assert_eq!(rules.general.harvester_dump_frames, 15);
    assert_eq!(rules.general.harvester_too_far_distance, 5);
    assert_whole_visit(&mut scene_with(&returning_input(), rules, &rules_ini));
}

/// Two full miners, one dock: the second's HELLO is refused while the first
/// holds the slot, and it docks once the first has left. Both loads pay.
#[test]
fn a_second_war_miner_docks_after_the_first_leaves() {
    let mut s = returning_scene();
    let second = spawn_returning_miner(&mut s, (15, 15));
    let mut tethered: Vec<(u64, usize)> = Vec::new();
    let mut untethered: Vec<(u64, usize)> = Vec::new();
    let mut was = [false, false];
    let mut paid = 0;
    for n in 0..3000 {
        paid = frame(&mut s);
        for (slot, id) in [s.miner, second].into_iter().enumerate() {
            let now = sample(&s, id).tethered;
            if now && !was[slot] {
                tethered.push((id, n));
            }
            if !now && was[slot] {
                untethered.push((id, n));
            }
            was[slot] = now;
        }
        if paid == 2000 && untethered.len() == 2 {
            break;
        }
    }
    assert_eq!(paid, 2000, "both loads pay");
    assert_eq!(tethered.len(), 2, "each miner docks once: {tethered:?}");
    let (first, first_docked) = tethered[0];
    let (next, next_docked) = tethered[1];
    assert_ne!(first, next);
    let first_left = untethered
        .iter()
        .find(|(id, _)| *id == first)
        .map(|&(_, n)| n)
        .expect("the first miner leaves");
    assert!(
        first_docked < first_left && first_left <= next_docked,
        "one miner on the pad at a time: {tethered:?} / {untethered:?}"
    );
}

/// Mixed cargo: one slot per dump, ore first, the second dump fifteen frames
/// after the first (the stage restarts at 0 on each dump).
#[test]
fn mixed_cargo_dumps_ore_then_gems_fifteen_frames_apart() {
    let mut input = returning_input();
    input["storage"] = serde_json::json!([5.0, 3.0]);
    let mut s = scene(&input);
    let mut paid = Vec::new();
    for n in 0..1500 {
        let credits = frame(&mut s);
        if paid
            .last()
            .map_or(credits > 0, |&(_, last)| credits != last)
        {
            paid.push((n, credits));
        }
    }
    assert_eq!(
        paid.iter().map(|&(_, c)| c).collect::<Vec<_>>(),
        vec![125, 125 + 150],
        "ore slot (5 x 25), then the gem slot (3 x 50)"
    );
    assert_eq!(paid[1].0 - paid[0].0, 15);
}

/// A player refinery order in the middle of an unload (`Command::MinerReturn`)
/// leaves the dock and the unload latch; the miner docks again and pays the
/// load it still carries, and the refinery's slot ends free.
#[test]
fn a_refinery_order_mid_unload_redocks_and_pays() {
    let mut s = returning_scene();
    run_until_unloading(&mut s);
    assert!(s.sim.apply_command(
        "Americans",
        &crate::sim::command::Command::MinerReturn {
            entity_id: s.miner,
            target_refinery_id: Some(s.refinery),
        },
        Some(&s.rules),
        None,
        &BTreeMap::new(),
    ));
    let after_order = sample(&s, s.miner);
    assert!(!after_order.unloading && !after_order.tethered);
    let mut paid = 0;
    for _ in 0..1500 {
        paid = frame(&mut s);
        if paid == 1000 && !sample(&s, s.miner).tethered {
            break;
        }
    }
    assert_eq!(paid, 1000, "the whole load is paid after the re-dock");
    let refinery = s.sim.substrate.entities.get(s.refinery).unwrap();
    assert!(refinery.radio_contacts.is_empty());
    assert_eq!(refinery.dock_entered_with, None);
}

/// Selling the refinery under an unloading miner: the sale's stage-0 visit,
/// the frame after the order's, broadcasts RUN_AWAY (`0x0044AB5A`), which
/// drops the latch and hands the miner to Harvest (`0x00737A98`) with its
/// cargo and its contact; the next visit's OVER_OUT (stage 1) releases it
/// from the pad.
#[test]
fn selling_the_refinery_mid_unload_hands_the_miner_to_harvest() {
    let mut s = returning_scene();
    run_until_unloading(&mut s);
    let refinery = s.refinery;
    let type_id = s
        .sim
        .interner
        .resolve(s.sim.substrate.entities.get(refinery).unwrap().type_ref())
        .to_string();
    s.rules.set_buildup_control_for_test(&type_id, [0, 25, 2]);
    assert!(crate::sim::production::sell_back(
        &mut s.sim,
        &s.rules,
        refinery,
        crate::sim::production::SellOrder::Player
    ));
    // The order stands for this frame's event (`EventClass::Execute`); the
    // frame's own Selling mission does not visit yet.
    frame(&mut s);
    assert!(
        sample(&s, s.miner).unloading,
        "the order's frame changes nothing"
    );
    frame(&mut s);
    let after = sample(&s, s.miner);
    assert!(!after.unloading, "RUN_AWAY dropped the latch");
    assert!(after.tethered);
    assert_eq!(after.contact, Some(refinery), "RUN_AWAY keeps the contact");
    assert_eq!(after.ore, 40, "nothing was dumped");
    assert_eq!(
        after.mission,
        MissionId::from_known(MissionType::Harvest),
        "Harvest queued and commenced"
    );
    assert_eq!(
        s.sim
            .substrate
            .entities
            .get(s.miner)
            .unwrap()
            .display_type_override,
        None
    );
    frame(&mut s);
    let after = sample(&s, s.miner);
    assert!(!after.tethered, "OVER_OUT released the miner");
    assert_eq!(after.contact, None);
    assert_eq!(after.ore, 40);
}

/// A refinery destroyed under an unloading miner: the NowDead contact loop
/// (`0x00442511`) sends it RUN_AWAY, so it leaves Unload for Harvest with its
/// cargo instead of waiting on the rubble.
#[test]
fn a_refinery_destroyed_mid_unload_hands_the_miner_to_harvest() {
    let mut s = returning_scene();
    run_until_unloading(&mut s);
    let refinery = s.refinery;
    {
        let building = s.sim.substrate.entities.get_mut(refinery).unwrap();
        building.health.current = 0;
    }
    s.sim.object_destroy_callback(
        refinery,
        crate::sim::world::UninitContext::with_rules(&s.rules),
    );
    let after = sample(&s, s.miner);
    assert!(!after.unloading);
    assert!(!after.tethered);
    assert_eq!(after.ore, 40);
    assert_eq!(after.mission, MissionId::from_known(MissionType::Harvest));
}

/// Stop while the miner drives onto the pad (Enter, not yet tethered): the
/// IDLE event breaks the link and clears the NavCom without writing a
/// mission (`0x004C74CB..0x004C76BB`); the next Mission_Enter finds no target
/// and parks the human player's miner on Guard. It does not dock again.
#[test]
fn stop_on_the_pad_approach_parks_the_miner() {
    let mut s = returning_scene();
    for _ in 0..1500 {
        frame(&mut s);
        let now = sample(&s, s.miner);
        if now.mission == MissionId::from_known(MissionType::Enter) && !now.tethered {
            break;
        }
    }
    assert_eq!(
        sample(&s, s.miner).mission,
        MissionId::from_known(MissionType::Enter)
    );
    assert!(s.sim.apply_command(
        "Americans",
        &crate::sim::command::Command::Stop { entity_id: s.miner },
        Some(&s.rules),
        None,
        &BTreeMap::new(),
    ));
    for _ in 0..200 {
        frame(&mut s);
    }
    let end = sample(&s, s.miner);
    assert_eq!(end.mission, MissionId::from_known(MissionType::Guard));
    assert!(!end.tethered && end.contact.is_none());
    assert_eq!(end.ore, 40, "it did not dock and unload");
}
