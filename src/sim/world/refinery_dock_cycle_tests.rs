//! War Miner refinery visits through the production frame (`advance_tick`):
//! Harvest's return states, Mission_Enter's docking handshake and pad drive,
//! the hull turn, Mission_Unload's dumps and the departure, on the oracle
//! replay's scene (a single-dock refinery at NW (6, 9), pad (9, 10)).

use super::refinery_dock_oracle_tests::{Scene, scene};
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
    facing: u16,
    ore: usize,
}

fn sample(s: &Scene, id: u64) -> Sample {
    let miner = s.sim.substrate.entities.get(id).unwrap();
    Sample {
        cell: (miner.position.rx, miner.position.ry),
        mission: miner.mission.current(),
        contact: miner.radio_contacts.slot(0),
        tethered: miner.dock_entered_with.is_some(),
        facing: miner
            .body_facing
            .as_ref()
            .map_or(0, |f| f.current(s.sim.session.binary_frame)),
        ore: miner
            .miner
            .as_ref()
            .unwrap()
            .cargo
            .iter()
            .filter(|b| b.resource_type == ResourceType::Ore)
            .count(),
    }
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

fn returning_scene() -> Scene {
    scene(&serde_json::json!({
        "name": "cycle",
        "linked": false,
        "mission": "harvest",
        "status": 2,
        "miner_cell": [14, 12],
        "storage": [40.0, 0.0],
    }))
}

#[test]
fn war_miner_docks_unloads_and_leaves_through_the_production_frame() {
    let mut s = returning_scene();
    let mut samples = Vec::new();
    for _ in 0..1500 {
        let paid = frame(&mut s);
        samples.push((sample(&s, s.miner), paid));
    }
    let (docked, _) = samples
        .iter()
        .find(|(x, _)| x.tethered)
        .expect("the miner tethers to the refinery");
    assert_eq!(docked.cell, (9, 10), "tethered on the pad");
    assert_eq!(docked.contact, Some(s.refinery));
    let (first_pay, _) = samples
        .iter()
        .find(|(_, paid)| *paid > 0)
        .expect("the unload pays");
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
