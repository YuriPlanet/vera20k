//! A Yuri slave refinery's harvest cycle through the production frame
//! (`advance_tick`) on the oracle replay's scene
//! (`slave_manager_oracle_tests::row_scene`): the manager leaving state 0,
//! DeploySlaves, the slaves walking to ore, digging one level per
//! `HarvestRate`, carrying the load home, paying it and going back out. The
//! per-step behaviour is pinned row by row against gamemd in
//! `slave_manager_oracle_tests`; these pin how the steps compose over frames.

use super::harvest_field_oracle_tests::registry;
use super::slave_manager_oracle_tests::{SlaveScene, row_scene};
use crate::sim::animation::SequenceKind;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::mission::MissionType;
use crate::sim::ore_growth::OreGrowthConfig;
use crate::sim::slave_manager::{ManagerState, SlaveState};
use crate::sim::world::SimSoundEvent;
use std::collections::BTreeMap;

fn frame(s: &mut SlaveScene) {
    let grid = s.scene.sim.path_grid_snapshot();
    s.scene.sim.advance_tick(
        &[],
        Some(&s.scene.rules),
        &BTreeMap::new(),
        grid.as_deref(),
        Some(registry()),
        67,
    );
}

/// A built refinery (manager state 0) holding `count` slaves inside, with a
/// density-5 ore patch east of it and no growth.
fn refinery_with_slaves(count: usize) -> SlaveScene {
    let slaves: Vec<_> = (0..count)
        .map(|_| serde_json::json!({"state": 0, "cell": [13, 13], "limbo": true}))
        .collect();
    let mut s = row_scene(&serde_json::json!({
        "name": "cycle",
        "manager_state": 0,
        "nodes": slaves,
        "ore": [
            [17, 13, 0, 0, 5], [18, 13, 0, 0, 5], [17, 14, 0, 0, 5],
            [18, 14, 0, 0, 5], [17, 12, 0, 0, 5], [18, 12, 0, 0, 5],
        ],
    }));
    s.scene.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    s
}

fn node_states(s: &SlaveScene) -> Vec<SlaveState> {
    s.scene
        .sim
        .substrate
        .entities
        .get(s.master)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .map(|node| node.state)
        .collect()
}

fn credits(s: &SlaveScene) -> i32 {
    let owner = s.scene.sim.interner.get("Americans").unwrap();
    s.scene.sim.houses[&owner].economy.credits
}

/// The first visit (10 frames) moves the built refinery to state 5; the next
/// lets every slave out onto the drop cell's spots and scatters them; the
/// third sends each to ore. Each digs one level per 150 frames, walks its
/// four levels home, pays them (100 credits of ore) inside the refinery,
/// reloads for 25 frames and goes out again.
#[test]
fn slave_refinery_deploys_digs_carries_home_pays_and_goes_out_again() {
    let mut s = refinery_with_slaves(3);
    let slaves: Vec<u64> = s.slaves.values().copied().collect();
    let manager_state = |s: &SlaveScene| {
        s.scene
            .sim
            .substrate
            .entities
            .get(s.master)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap()
            .state()
    };
    for _ in 0..11 {
        frame(&mut s);
    }
    assert_eq!(manager_state(&s), ManagerState::Working);
    for _ in 0..10 {
        frame(&mut s);
    }
    assert_eq!(
        node_states(&s),
        vec![SlaveState::Scanning; 3],
        "all three out"
    );
    for &slave in &slaves {
        let entity = s.scene.sim.substrate.entities.get(slave).unwrap();
        assert!(!entity.lifecycle.in_limbo);
        assert!(
            entity.navigation.nav_com.is_some(),
            "scattered off the drop cell"
        );
    }

    let mut dug = false;
    let mut shovel_shown = false;
    let mut paid_at = None;
    let start = credits(&s);
    for n in 0..3000 {
        frame(&mut s);
        for &slave in &slaves {
            let entity = s.scene.sim.substrate.entities.get(slave).unwrap();
            dug |= !entity.slave_cargo.is_empty();
            shovel_shown |= entity
                .animation
                .as_ref()
                .is_some_and(|animation| animation.sequence == SequenceKind::Shovel);
        }
        if paid_at.is_none() && credits(&s) > start {
            paid_at = Some(n);
        }
        if paid_at.is_some_and(|paid| n > paid + 60) {
            break;
        }
    }
    assert!(dug, "the slaves reach the ore and dig");
    assert!(shovel_shown, "a digging slave shows its Shovel sequence");
    let paid_at = paid_at.expect("a full slave carries its load home and pays");
    assert_eq!(
        (credits(&s) - start) % 100,
        0,
        "each visit pays four levels of ore"
    );
    // A slave that paid reloaded inside and went out again.
    assert!(
        node_states(&s)
            .iter()
            .any(|state| matches!(state, SlaveState::Scanning | SlaveState::Moving)),
        "back out after the reload (paid at {paid_at}): {:?}",
        node_states(&s)
    );
}

/// Hits on `target` from `attacker` through the receiver until it dies
/// (`MaxDamage` caps each one).
fn kill(s: &mut SlaveScene, target: u64, attacker: Option<u64>) {
    let warhead = s.scene.sim.interner.intern("KILLWH");
    let house = attacker
        .and_then(|id| s.scene.sim.substrate.entities.get(id))
        .map(|entity| entity.owner());
    for _ in 0..10 {
        if !s
            .scene
            .sim
            .substrate
            .entities
            .get(target)
            .is_some_and(|entity| entity.health.current > 0)
        {
            return;
        }
        let hit = EntityDamageEvent::direct_receiver(
            target,
            100_000,
            0,
            attacker.unwrap_or(RAD_NO_ATTACKER),
            house,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        s.scene
            .sim
            .commit_noncombat_aoe_hits(&s.scene.rules, Some(registry()), &[hit]);
    }
    panic!("{target} survived ten hits");
}

/// The death arm frees the refinery's slaves (`0x006B0AE0`): the two outside
/// pass to the killer's house, reset to Guard and cheer, and `SlavesFreeSound`
/// plays at the first; the one reloading inside dies with the refinery.
#[test]
fn a_destroyed_refinery_frees_its_slaves_to_the_killer() {
    let mut s = row_scene(&serde_json::json!({
        "name": "free",
        "nodes": [
            {"state": 3, "cell": [17, 12], "mission": "harvest"},
            {"state": 1, "cell": [16, 14]},
            {"state": 5, "cell": [13, 13], "limbo": true, "timer": [-5, 25]},
        ],
        "ore": [[17, 12, 0, 0, 5]],
    }));
    s.scene.rules.general.slaves_free_sound = Some("SlavesFreed".to_string());
    let russians = s.scene.sim.interner.intern("Russians");
    s.scene.sim.houses.insert(
        russians,
        crate::sim::house_state::HouseState::new(russians, 1, None, true, 0, 10),
    );
    let killer = s
        .scene
        .sim
        .spawn_object(
            "MTNK",
            "Russians",
            10,
            20,
            0,
            &s.scene.rules,
            &BTreeMap::new(),
        )
        .expect("killer");
    s.scene.sim.sound_events.clear();
    let master = s.master;
    kill(&mut s, master, Some(killer));

    let sim = &s.scene.sim;
    assert!(
        sim.substrate
            .entities
            .get(s.master)
            .unwrap()
            .slave_manager
            .is_none()
    );
    for index in [0, 1] {
        let slave = sim.substrate.entities.get(s.slaves[&index]).unwrap();
        assert_eq!(slave.owner(), russians, "slave {index} joins the killer");
        assert_eq!(slave.slave_owner, None);
        assert_eq!(slave.mission.current().known(), Some(MissionType::Guard));
        assert_eq!(
            slave.mission_leaf.as_infantry().unwrap().doing(),
            32,
            "it cheers"
        );
    }
    let inside = sim.substrate.entities.get(s.slaves[&2]).unwrap();
    assert!(
        !inside.lifecycle.object_alive,
        "the slave inside dies with it"
    );
    // The first freed is the last node's live slave: node 1.
    let freed = sim.substrate.entities.get(s.slaves[&1]).unwrap();
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::VocAt { sound_id, rx, ry, .. }
            if sound_id == "SlavesFreed" && (*rx, *ry) == (freed.position.rx, freed.position.ry)
    )));
}

/// A slave that dies leaves its node (RemoveSlave `0x006B0A20`): lost for
/// `SlaveRegenRate` frames, then regrown inside and let out again.
#[test]
fn a_dead_slave_is_regrown_after_the_regen_rate_and_goes_out_again() {
    let mut s = row_scene(&serde_json::json!({
        "name": "regen",
        "nodes": [{"state": 3, "cell": [17, 12], "mission": "harvest"}],
        "ore": [[17, 12, 0, 0, 5]],
    }));
    s.scene.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    let slave = s.slaves[&0];
    let died_at = s.scene.sim.session.binary_frame as i32;
    kill(&mut s, slave, None);
    let node = |s: &SlaveScene| {
        s.scene
            .sim
            .substrate
            .entities
            .get(s.master)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap()
            .nodes()[0]
    };
    let lost = node(&s);
    assert_eq!((lost.slave, lost.state), (None, SlaveState::Dead));
    assert_eq!(
        (lost.timer.start_frame(), lost.timer.duration()),
        (died_at, 500)
    );
    let mut regrown = None;
    for _ in 0..560 {
        frame(&mut s);
        if let Some(id) = node(&s).slave {
            regrown.get_or_insert(id);
        }
    }
    let regrown = regrown.expect("regrown after SlaveRegenRate");
    assert_ne!(regrown, slave);
    assert_eq!(
        s.scene
            .sim
            .substrate
            .entities
            .get(regrown)
            .unwrap()
            .slave_owner,
        Some(s.master)
    );
    assert!(
        matches!(node(&s).state, SlaveState::Scanning | SlaveState::Moving),
        "let out again: {:?}",
        node(&s).state
    );
}
