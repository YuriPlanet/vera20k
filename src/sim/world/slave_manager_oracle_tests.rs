//! Replay of `tools/spatial_oracle/slave_manager.json`: every original row of
//! the slave manager runs against its Rust owner (`sim::slave_manager`) on
//! the harvest_field scene (the refinery_dock map, House and ore tables) with
//! a 2x2 slave refinery (YAREFN) at NW (12, 12) and its slaves.
//!
//! - `ai_update` rows: `SlaveManagerClass::AI_Update @ 0x006AF6C0`.
//! - `manager` rows: the manager's machine (`0x006AFD60`), DeploySlaves
//!   included.
//! - `deploy` rows: DeploySlaves (`0x006B04C0`) through the original
//!   PlaceInfantryInCell, Unlimbo and Scatter.
//! - `slave_harvest` rows: `InfantryClass::Mission_Harvest @ 0x00522E70`,
//!   with the Guard it queues applied as the dispatcher applies it.
//! - `deposit` rows: the slave's deposit (`0x00522D50`).
//! - `unit_manager`, `unit_helper` and `unit_mission` rows: a Slave Miner
//!   owner (SMIN at (15, 15)): the manager machine's unit states,
//!   ShouldRecallSlaves, the hunt start, the reset, HandleReturnedSlaves and
//!   the Guard/AreaGuard kick. The oracle answers UnitClass::Deploy and
//!   Find_Nearby_Passable_Cell; the scene reproduces the answers (a deploy
//!   that converts, or a structure on the footprint; the supplied deploy
//!   cell is the one VERA's search finds on the scene, whose own evidence is
//!   `find_nearby_cell`'s). A converting deploy moves the manager to the
//!   refinery, which the comparison follows.
//! - `relocation` rows: state 5 with no ore within `SlaveMinerShortScan` (the
//!   refinery's relocation test, the Building's Scan_For_Tiberium run
//!   natively). The queued Selling a relocation records is VERA's started
//!   undeploy (`building_down`); its archive is compared.
//! - `building_helper` rows: the placement hand-off (`0x006B0D60`).
//!
//! The Scenario RNG is seeded as the oracle seeds it. The oracle hands the
//! regrown slave out of a supplied `CreateObject`, so the Rust constructor's
//! own Scenario draw (the TechnoClass constructor word) is added to the
//! expected cursor for every slave created.
//!
//! Compared per row: every node (slave, state, timer), each slave's mission,
//! queued mission, NavCom, archive, Doing, Storage, Strength, SlaveOwner,
//! limbo byte and coordinate, the manager's state and frame, the row's ore
//! cells, the house balance and the RNG cursors, and a harvest row's delay.
//!
//! Not compared: the refinery smoke (`0x00459900`), which the scene's
//! refinery type gives no particle system; the Unlimbo's sight reveal and
//! layer submission, which the oracle answers; and the rows in `SKIPPED`.

use super::harvest_field_oracle_tests::{registry, row_scene_with};
use super::refinery_dock_oracle_tests::{Scene, cell};
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::miner::{CargoBale, ResourceType};
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::rng::SimRng;
use crate::sim::slave_manager::{DeployCellSearch, ManagerState, SlaveLink, SlaveNode, SlaveState};
use crate::sim::timer::CdTimer;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, UninitContext,
};
use crate::util::fixed_math::SimFixed;
use serde_json::Value;
use std::collections::BTreeMap;

/// Rows whose inputs the Rust scene does not reproduce.
const SKIPPED: &[&str] = &[
    // IncomeMult 0.9: VERA's integer credit fold pays 90 where the native
    // float product truncates to 89 (the economy owner's recorded residual).
    "dep_income_mult",
    // A supplied Unlimbo refusal on an open cell: the Rust Unlimbo places it.
    "d_unlimbo_refused",
    // FindDeployCell's supplied Find_Nearby_Passable_Cell miss: the scene's
    // open ground offers a cell, and ground no Track can cross would also
    // strand the Slave Miner and change its scan. The miss is the manager's
    // state 0 at now (and HandleReturnedSlaves' Guard), read at 0x006AFE67
    // and 0x006B0E7B.
    "mu1_no_deploy_cell",
    "hr_nav_no_cell",
    // The same miss for the refinery's relocation test: the scene finds
    // (6, 7) by (0, 0) and (20, 14) beside the field. A miss is D = (0,0),
    // read at 0x006B00F0.
    "r5_no_field_no_cell",
    "r5_no_deploy_cell",
];

/// The slave type and refinery the oracle builds: SLAV (Strength 125, the
/// row's Storage and HarvestRate, MovementZone Infantry, Walk) and a 2x2
/// building with no slaves of its own; the row's manager is installed after.
fn slave_rules(input: &Value) -> String {
    format!(
        "[InfantryTypes]\n0=SLAV\n\
         [SLAV]\nStrength=125\nStorage={}\nHarvestRate={}\nSpeed=4\nSlaved=yes\n\
         MovementZone=Infantry\nLocomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [YAREFN]\nFoundation=2x2\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=0\n\
         SlaveRegenRate={}\nSlaveReloadRate={}\nDeployFacing=0\nUndeploysInto=SMIN\n\
         [SMIN]\nStrength=2000\nSpeed=3\nROT=5\nEnslaves=SLAV\nSlavesNumber=0\nDeploysInto=YAREFN\n\
         ResourceGatherer=yes\nResourceDestination=yes\nMovementZone=Crusher\n\
         Locomotor={{4A582741-9839-11D1-B709-00A024DDAFD1}}\n\
         [Area Guard]\nRate=.016\n\
         [Warheads]\n0=KILLWH\n[KILLWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        input["slave_storage"].as_i64().unwrap_or(4),
        input["harvest_rate"].as_i64().unwrap_or(150),
        input["regen"].as_i64().unwrap_or(500),
        input["reload"].as_i64().unwrap_or(25),
    )
}

/// The Do_Action sequences the slave machine requests; any non-zero count.
const SLAVE_SEQUENCE: &str = "[SlavSequence]\nReady=0,1,1\nGuard=0,1,1\nWalk=8,6,6\n\
    Shovel=56,4,4\nCheer=100,8,0,E\n";

const YAREFN_NW: (u16, u16) = (12, 12);
/// The oracle's regen spare (`slave_address(7)`).
const SPARE: u64 = 7;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/slave_manager.json"
    ))
    .unwrap()
}

fn mission(name: &str) -> MissionType {
    match name {
        "guard" => MissionType::Guard,
        "move" => MissionType::Move,
        "harvest" => MissionType::Harvest,
        "selling" => MissionType::Selling,
        "area_guard" => MissionType::AreaGuard,
        other => panic!("unmapped mission {other}"),
    }
}

/// The row's scene: the harvest_field world and ore, the refinery with the
/// row's mission, its manager and slaves, and the Scenario RNG seeded last.
pub(super) struct SlaveScene {
    pub(super) scene: Scene,
    pub(super) master: u64,
    /// The scene's YAREFN (the master unless a Slave Miner owns the manager).
    pub(super) refinery: u64,
    /// Oracle slave index -> Rust id.
    pub(super) slaves: BTreeMap<u64, u64>,
}

impl SlaveScene {
    /// The entity holding the row's manager: the master, or the refinery a
    /// Slave Miner's deploy made of it.
    fn holder(&self) -> u64 {
        let sim = &self.scene.sim;
        if sim
            .substrate
            .entities
            .get(self.master)
            .is_some_and(|entity| entity.slave_manager.is_some())
        {
            return self.master;
        }
        sim.substrate
            .entities
            .values()
            .find(|entity| {
                entity.slave_manager.is_some()
                    && entity.stable_id() != self.refinery
                    && entity.stable_id() != self.master
            })
            .expect("the manager moved to the deployed refinery")
            .stable_id()
    }

    fn index_of(&self, id: u64) -> u64 {
        self.slaves
            .iter()
            .find(|(_, rust)| **rust == id)
            .map_or(SPARE, |(index, _)| *index)
    }
}

pub(super) fn row_scene(input: &Value) -> SlaveScene {
    let mut input = input.clone();
    if input.get("ore").is_none() {
        input["ore"] = serde_json::json!([]);
    }
    // The oracle parks its War Miner at (2, 2); that cell is outside this
    // scene's playfield, so the miner Unlimbos on a free cell no row uses.
    input["miner_cell"] = serde_json::json!([26, 6]);
    // A Slave Miner row's owner is the oracle's Unit at (15, 15), which the
    // row strips of Harvester=; the Rust scene's War Miner stays a harvester
    // and the Slave Miner is an entity of its own.
    let unit_owner = input["owner"] == "unit";
    if let Some(fields) = input.as_object_mut() {
        fields.remove("harvester");
    }
    let mut scene = row_scene_with(&input, |text| {
        *text = text.replacen("2=GAOREP\n", "2=GAOREP\n3=YAREFN\n", 1);
        *text = text.replacen("1=MTNK\n", "1=MTNK\n2=SMIN\n", 1);
        text.push_str(&slave_rules(&input));
    });
    // The oracle's Rules+0x1780.. (ReadRange leptons), KickFrameDelay and
    // ApproachTargetResetMultiplier.
    let ranges = input["slave_ranges"]
        .as_array()
        .map_or(vec![8, 14, 48, 3], |r| {
            r.iter().map(|v| v.as_i64().unwrap() as i32).collect()
        });
    let general = &mut scene.rules.general;
    general.slave_miner_short_scan = ranges[0] * 256;
    general.slave_miner_slave_scan = ranges[1] * 256;
    general.slave_miner_long_scan = ranges[2] * 256;
    general.slave_miner_scan_correction = ranges[3] * 256;
    general.slave_miner_kick_frame_delay = input["kick_delay"].as_i64().unwrap_or(150) as i32;
    general.approach_target_reset_multiplier = input["approach_reset"].as_i64().unwrap_or(1) as i32;
    let sequences = crate::rules::infantry_sequence::parse_infantry_sequence_registry(
        &IniFile::from_str(SLAVE_SEQUENCE),
    );
    let mut catalog = scene.rules.animation_sequences().clone();
    catalog.insert(
        "SLAV".to_string(),
        crate::rules::infantry_sequence::build_sequence_set(&sequences["SLAVSEQUENCE"]),
    );
    scene.rules.replace_animation_sequences_for_test(catalog);
    let rules = &scene.rules;
    let sim = &mut scene.sim;
    let frame = sim.session.binary_frame;
    let heights = BTreeMap::new();
    let refinery = sim
        .spawn_object(
            "YAREFN",
            "Americans",
            YAREFN_NW.0,
            YAREFN_NW.1,
            0,
            rules,
            &heights,
        )
        .expect("slave refinery");
    let master = if unit_owner {
        // The oracle's Slave Miner holds the manager; its fixture refinery
        // holds none (`+0x2D8` = 0), so it takes no manager turns.
        sim.substrate
            .entities
            .get_mut(refinery)
            .unwrap()
            .slave_manager = None;
        // Facing north, YAREFN's DeployFacing, so UnitClass::Deploy converts.
        let smin = sim
            .spawn_object_with_overlay_registry(
                "SMIN",
                "Americans",
                15,
                15,
                0,
                rules,
                &heights,
                registry(),
            )
            .unwrap_or_else(|| panic!("slave miner for {}", input["name"]));
        let entity = sim.substrate.entities.get_mut(smin).unwrap();
        entity.mcv_deploy_pending = input["deploy_pending"] == true;
        if let Some((nx, ny)) = input.get("owner_nav").map(cell) {
            entity.navigation.nav_com = Some(NavTargetRef::cell(nx, ny));
        }
        smin
    } else {
        refinery
    };
    // A refused UnitClass::Deploy is a structure on the deploy footprint.
    if input["deploys"] == serde_json::json!([0]) {
        sim.spawn_object("GAOREP", "Americans", 16, 16, 0, rules, &heights)
            .expect("footprint blocker");
    }
    // VERA publishes neither the Construction mission nor a BState: a
    // building that builds up carries `building_up`, which stands for both
    // (`GameEntity::constructing_or_selling`, `slave_manager_step`).
    let owner_mission = input["owner_mission"].as_str().unwrap_or("guard");
    if owner_mission == "construction" || input["bstate"].as_i64() == Some(0) {
        sim.substrate.entities.get_mut(master).unwrap().building_up =
            Some(crate::sim::components::BuildingUp {
                elapsed_ticks: 0,
                total_ticks: 30,
            });
    }
    let owner_mission = match owner_mission {
        "construction" => MissionType::Guard,
        other => mission(other),
    };
    // The MissionClass start frame (`+0xC0`) the row supplies.
    let started = (i64::from(frame) + input["mission_start"].as_i64().unwrap_or(0)) as u32;
    sim.mission_assign_exact(master, MissionId::from_known(owner_mission), started)
        .unwrap();
    let mut slaves = BTreeMap::new();
    let mut nodes = Vec::new();
    for (index, node) in input["nodes"].as_array().into_iter().flatten().enumerate() {
        let slave = (node["slave"] != false).then(|| {
            let id = place_slave(sim, rules, node, master);
            slaves.insert(index as u64, id);
            id
        });
        let timer = node["timer"].as_array().map_or((0, 0), |t| {
            (t[0].as_i64().unwrap() as i32, t[1].as_i64().unwrap() as i32)
        });
        nodes.push(SlaveNode {
            slave,
            state: slave_state(node["state"].as_u64().unwrap()),
            timer: CdTimer::from_raw(frame as i32 + timer.0, timer.1),
        });
    }
    let manager_state = manager_state(input["manager_state"].as_u64().unwrap_or(5));
    let manager_frame = match input["manager_frame_ago"].as_i64() {
        Some(ago) => (i64::from(frame) - ago) as i32,
        None => input["manager_frame"].as_i64().unwrap_or(0) as i32,
    };
    sim.substrate
        .entities
        .get_mut(master)
        .unwrap()
        .slave_manager
        .as_mut()
        .expect("Enslaves= builds the manager")
        .set_for_test(
            manager_state,
            manager_frame,
            nodes,
            CdTimer::started(frame as i32, 10),
        );
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
    SlaveScene {
        scene,
        master,
        refinery,
        slaves,
    }
}

/// One oracle slave: constructed, then revealed at its cell's centre unless
/// it waits in limbo, with the row's missions, NavCom, Doing, Strength and
/// Storage, and the refinery as its SlaveOwner.
fn place_slave(
    sim: &mut crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    node: &Value,
    master: u64,
) -> u64 {
    let frame = sim.session.binary_frame;
    let (x, y) = node.get("cell").map_or((15, 15), cell);
    let id = sim
        .construct_object_limbo_at_height("SLAV", "Americans", x, y, 0, 0, rules)
        .expect("slave");
    // The oracle supplies the cell's centre as every slave's coordinate.
    {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.sub_x = SimFixed::from_num(128);
        entity.position.sub_y = SimFixed::from_num(128);
    }
    if node["limbo"] != true {
        let outcome = sim.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition {
                    rx: x,
                    ry: y,
                    z: 0,
                    sub_x: SimFixed::from_num(128),
                    sub_y: SimFixed::from_num(128),
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(rules),
        );
        assert!(
            matches!(outcome, RevealOutcome::Revealed { .. }),
            "slave reveal at ({x}, {y})"
        );
    }
    let current = mission(node["mission"].as_str().unwrap_or("guard"));
    sim.mission_assign_exact(id, MissionId::from_known(current), frame)
        .unwrap();
    if let Some(queued) = node["queued"].as_str() {
        sim.mission_queue_exact(
            id,
            MissionId::from_known(mission(queued)),
            0,
            frame,
            &EntityReadyInputProvider,
        )
        .unwrap();
    }
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    // The oracle writes the row's health to both Strength and EstimatedHealth.
    let health = node["health"].as_i64().unwrap_or(125) as i32;
    entity.health.current = health;
    entity.estimated_health.reset(health);
    if let Some(doing) = node["doing"].as_i64() {
        entity
            .mission_leaf
            .set_infantry_doing_verified(doing as i32)
            .unwrap();
    }
    if let Some((nx, ny)) = node.get("nav").map(cell) {
        entity.navigation.nav_com = Some(NavTargetRef::cell(nx, ny));
    }
    let storage = node["storage"].as_array();
    let mut cargo = Vec::new();
    for (slot, kind, value) in [(0, ResourceType::Ore, 25), (1, ResourceType::Gem, 50)] {
        let count = storage.map_or(0, |s| s[slot].as_f64().unwrap() as usize);
        for _ in 0..count {
            cargo.push(CargoBale {
                resource_type: kind,
                value,
            });
        }
    }
    entity.slave = SlaveLink::for_test(Some(master), cargo);
    id
}

fn slave_state(raw: u64) -> SlaveState {
    [
        SlaveState::Ready,
        SlaveState::Scanning,
        SlaveState::Moving,
        SlaveState::Harvesting,
        SlaveState::Returning,
        SlaveState::Reloading,
        SlaveState::Dead,
    ][raw as usize]
}

fn manager_state(raw: u64) -> ManagerState {
    [
        ManagerState::Ready,
        ManagerState::Scanning,
        ManagerState::Travelling,
        ManagerState::Deploying,
        ManagerState::Deployed,
        ManagerState::Working,
        ManagerState::PackingUp,
    ][raw as usize]
}

/// The Scenario cursors the oracle reached, then `extra` more draws.
fn expected_cursors(seed: u64, native: &Value, extra: usize) -> Value {
    let mut rng = SimRng::new(seed);
    for _ in 0..10_000 {
        let view = rng.logical_view();
        if serde_json::json!([view.index_a, view.index_b]) == *native {
            for _ in 0..extra {
                rng.next_u32();
            }
            let view = rng.logical_view();
            return serde_json::json!([view.index_a, view.index_b]);
        }
        rng.next_u32();
    }
    panic!("native cursors {native} are not on the seed {seed} stream");
}

fn compare_state(s: &SlaveScene, row: &Value, context: &str) {
    let native = &row["state"];
    let sim = &s.scene.sim;
    let holder = s.holder();
    let manager = sim
        .substrate
        .entities
        .get(holder)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap();
    let frame = sim.session.binary_frame as i64;
    let nodes: Vec<Value> = manager
        .nodes()
        .iter()
        .map(|node| {
            serde_json::json!({
                "slave": node.slave.map(|id| s.index_of(id)),
                "state": node.state as u8,
                "timer": [node.timer.start_frame(), node.timer.duration()],
            })
        })
        .collect();
    assert_eq!(Value::from(nodes), native["nodes"], "{context}: nodes");
    assert_eq!(
        manager.state() as i64,
        native["manager_state"].as_i64().unwrap(),
        "{context}: manager state"
    );
    assert_eq!(
        i64::from(manager.frame()),
        native["manager_frame"].as_i64().unwrap(),
        "{context}: manager frame"
    );
    assert_eq!(frame, native["frame"].as_i64().unwrap(), "{context}: frame");
    if !native["ai_timer"].is_null() {
        let timer = manager.ai_timer();
        assert_eq!(
            serde_json::json!([timer.start_frame(), timer.duration()]),
            native["ai_timer"],
            "{context}: manager AI timer"
        );
    }
    // The Slave Miner itself, unless its deploy converted it (the oracle
    // answers UnitClass::Deploy without converting).
    if !native["owner"].is_null() && holder == s.master {
        let owner = sim.substrate.entities.get(s.master).unwrap();
        let expected = &native["owner"];
        assert_eq!(
            owner.mission.current().raw() as i64,
            expected["mission"].as_i64().unwrap(),
            "{context}: owner mission"
        );
        assert_eq!(
            owner.mission.queued().raw() as i64,
            expected["queued"].as_i64().unwrap(),
            "{context}: owner queued mission"
        );
        let nav = match owner.navigation.nav_com {
            Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
            None => Value::Null,
            Some(other) => panic!("{context}: owner NavCom {other:?}"),
        };
        assert_eq!(nav, expected["nav"], "{context}: owner NavCom");
        assert_eq!(
            u64::from(owner.mcv_deploy_pending),
            expected["deploy_pending"].as_u64().unwrap(),
            "{context}: owner deploy pending"
        );
    }
    // The refinery: the Selling a relocation queues (`+0xB4`) is VERA's
    // started undeploy (`building_down`), and its ArchiveTarget. The
    // relocation's `+0x4F8` (the undeploy voice's silencer) has no VERA
    // counterpart (`slave_manager::relocate_refinery`).
    if !native["building"].is_null() && holder == s.refinery {
        let refinery = sim.substrate.entities.get(s.refinery).unwrap();
        let expected = &native["building"];
        let queued = match expected["queued"].as_i64().unwrap() {
            -1 => false,
            19 => true,
            other => panic!("{context}: refinery queued mission {other}"),
        };
        assert_eq!(
            refinery.building_down.is_some(),
            queued,
            "{context}: refinery Selling (undeploy)"
        );
        let archive = match refinery.archive_target() {
            Some(TargetKind::Cell(x, y)) => serde_json::json!([x, y]),
            None => Value::Null,
            Some(other) => panic!("{context}: refinery archive {other:?}"),
        };
        assert_eq!(archive, expected["archive"], "{context}: refinery archive");
    }
    let mut compared: Vec<(u64, u64)> = s.slaves.iter().map(|(i, id)| (*i, *id)).collect();
    // A slave the call created stands for the oracle's supplied spare.
    for node in manager.nodes() {
        if let Some(id) = node.slave
            && !s.slaves.values().any(|known| *known == id)
        {
            compared.push((SPARE, id));
        }
    }
    for (index, id) in compared {
        let context = format!("{context}: slave {index}");
        let expected = &native["slaves"][index.to_string()];
        let entity = sim
            .substrate
            .entities
            .get(id)
            .unwrap_or_else(|| panic!("{context}: gone"));
        // The oracle's spare carries its fixture's Guard; a constructed
        // slave carries the MissionClass constructor's none.
        if index != SPARE {
            assert_eq!(
                entity.mission.current().raw() as i64,
                expected["mission"].as_i64().unwrap(),
                "{context}: mission"
            );
            assert_eq!(
                entity.mission.queued().raw() as i64,
                expected["queued"].as_i64().unwrap(),
                "{context}: queued"
            );
        }
        let nav = match entity.navigation.nav_com {
            Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
            None => Value::Null,
            Some(other) => panic!("{context}: NavCom {other:?}"),
        };
        assert_eq!(nav, expected["nav"], "{context}: NavCom");
        let archive = match entity.archive_target() {
            Some(TargetKind::Cell(x, y)) => serde_json::json!([x, y]),
            None => Value::Null,
            Some(other) => panic!("{context}: archive {other:?}"),
        };
        assert_eq!(archive, expected["archive"], "{context}: archive");
        assert_eq!(
            entity.mission_leaf.as_infantry().unwrap().doing() as i64,
            expected["doing"].as_i64().unwrap(),
            "{context}: Doing"
        );
        let count = |kind| {
            entity
                .slave
                .cargo()
                .iter()
                .filter(|bale| bale.resource_type == kind)
                .count() as f64
        };
        let storage = expected["storage"].as_array().unwrap();
        assert_eq!(
            (count(ResourceType::Ore), count(ResourceType::Gem)),
            (storage[0].as_f64().unwrap(), storage[1].as_f64().unwrap()),
            "{context}: Storage"
        );
        assert_eq!(
            i64::from(entity.health.current),
            expected["health"][0].as_i64().unwrap(),
            "{context}: Strength"
        );
        assert_eq!(
            i64::from(entity.estimated_health.get()),
            expected["health"][1].as_i64().unwrap(),
            "{context}: EstimatedHealth"
        );
        assert_eq!(
            entity.slave.owner() == Some(holder),
            expected["owner"].as_bool().unwrap(),
            "{context}: SlaveOwner"
        );
        assert_eq!(
            u64::from(entity.lifecycle.in_limbo),
            expected["limbo"].as_u64().unwrap(),
            "{context}: limbo"
        );
        if index != SPARE {
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            assert_eq!(
                serde_json::json!([coord.x, coord.y]),
                serde_json::json!([expected["coord"][0], expected["coord"][1]]),
                "{context}: coordinate"
            );
        }
    }
    for ore in row["input"]["ore"].as_array().into_iter().flatten() {
        let at = (
            ore[0].as_u64().unwrap() as u16,
            ore[1].as_u64().unwrap() as u16,
        );
        let key = format!("{},{}", at.0, at.1);
        let overlay = sim.overlay_grid.as_ref().unwrap().cell(at.0, at.1);
        let land = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .unwrap()
            .land_type;
        assert_eq!(
            serde_json::json!([
                overlay.overlay_id.map_or(-1, i64::from),
                overlay.overlay_data,
                land
            ]),
            native["cells"][&key],
            "{context}: cell {key}"
        );
    }
    let owner = sim.interner.get("Americans").unwrap();
    assert_eq!(
        i64::from(sim.houses[&owner].economy.credits),
        native["balance"].as_i64().unwrap(),
        "{context}: balance"
    );
    let created = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] == "create_slave")
        .count();
    // A deploy that converted constructed the refinery (its TechnoClass
    // constructor word), which the oracle's answered Deploy does not.
    let created = created + usize::from(holder != s.master);
    let view = sim.scenario_rng.logical_view();
    assert_eq!(
        serde_json::json!([view.index_a, view.index_b]),
        expected_cursors(
            row["input"]["seed"].as_u64().unwrap_or(1),
            &native["random_indices"],
            created
        ),
        "{context}: Scenario RNG cursors"
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
fn ai_update_matches_the_original_node_machine() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "ai_update") {
        let mut s = row_scene(&row["input"]);
        s.scene
            .sim
            .slave_ai_update(s.master, &s.scene.rules, Some(registry()));
        compare_state(&s, row, &name);
    }
}

#[test]
fn manager_machine_matches_the_original_building_owner_states() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "manager") {
        let mut s = row_scene(&row["input"]);
        s.scene
            .sim
            .slave_manager_step(s.master, &s.scene.rules, Some(registry()));
        compare_state(&s, row, &name);
    }
}

/// State 5 with no ore within `SlaveMinerShortScan`: the relocation test
/// and, when it passes, the archive, the undeploy (Selling) and state 6.
#[test]
fn manager_machine_matches_the_original_slave_refinery_relocation() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "relocation") {
        let mut s = row_scene(&row["input"]);
        s.scene
            .sim
            .slave_manager_step(s.master, &s.scene.rules, Some(registry()));
        compare_state(&s, row, &name);
    }
}

/// The hand-off a refinery placed from production takes (`0x006B0D60`).
#[test]
fn placed_hand_off_matches_the_original() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "building_helper") {
        let mut s = row_scene(&row["input"]);
        assert_eq!(row["input"]["helper"], "placed_hand_off", "{name}");
        s.scene.sim.slave_manager_hand_off(s.master, &s.scene.rules);
        compare_state(&s, row, &name);
    }
}

#[test]
fn deploy_slaves_matches_the_original_unlimbo_and_scatter() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "deploy") {
        let mut s = row_scene(&row["input"]);
        s.scene
            .sim
            .deploy_slaves(s.master, &s.scene.rules, Some(registry()));
        compare_state(&s, row, &name);
    }
}

#[test]
fn slave_mission_harvest_matches_the_original_dig() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "slave_harvest") {
        let mut s = row_scene(&row["input"]);
        let slave = s.slaves[&0];
        let (delay, guard) =
            s.scene
                .sim
                .infantry_mission_harvest(slave, &s.scene.rules, Some(registry()));
        if guard {
            let frame = s.scene.sim.session.binary_frame;
            s.scene
                .sim
                .mission_queue_exact(
                    slave,
                    MissionId::from_known(MissionType::Guard),
                    0,
                    frame,
                    &EntityReadyInputProvider,
                )
                .unwrap();
        }
        assert_eq!(
            i64::from(delay),
            row["delay"].as_i64().unwrap(),
            "{name}: delay"
        );
        compare_state(&s, row, &name);
    }
}

#[test]
fn slave_deposit_matches_the_original_payment() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "deposit") {
        let mut s = row_scene(&row["input"]);
        let slave = s.slaves[&0];
        s.scene.sim.slave_deposit(slave, s.master, &s.scene.rules);
        compare_state(&s, row, &name);
    }
}

#[test]
fn manager_machine_matches_the_original_slave_miner_states() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "unit_manager") {
        let mut s = row_scene(&row["input"]);
        s.scene
            .sim
            .slave_manager_step(s.master, &s.scene.rules, Some(registry()));
        compare_state(&s, row, &name);
    }
}

#[test]
fn slave_miner_helpers_match_the_original() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "unit_helper") {
        let mut s = row_scene(&row["input"]);
        let (master, rules) = (s.master, &s.scene.rules);
        let sim = &mut s.scene.sim;
        match row["input"]["helper"].as_str().unwrap() {
            "should_recall" => {
                let answer = sim.should_recall_slaves(master, rules, Some(registry()));
                assert_eq!(
                    u64::from(answer),
                    row["answer"].as_u64().unwrap(),
                    "{name}: ShouldRecallSlaves"
                );
            }
            "begin_hunt" => sim.begin_slave_hunt(master, rules),
            "reset" => sim.reset_slave_manager(master, rules),
            "handle_returned" => sim.handle_returned_slaves(master, rules),
            other => panic!("{name}: helper {other}"),
        }
        compare_state(&s, row, &name);
    }
}

#[test]
fn slave_miner_kick_matches_the_original_guard_and_area_guard() {
    let corpus = corpus();
    for (row, name) in rows(&corpus, "unit_mission") {
        let mut s = row_scene(&row["input"]);
        let mission = mission(row["input"]["owner_mission"].as_str().unwrap());
        let kicked = s.scene.sim.slave_master_mission_kick(
            s.master,
            mission,
            &s.scene.rules,
            Some(registry()),
        );
        assert_eq!(
            kicked.is_some(),
            row["kicked"].as_bool().unwrap(),
            "{name}: kicked"
        );
        if let Some(delay) = kicked {
            assert_eq!(
                i64::from(delay),
                row["delay"].as_i64().unwrap(),
                "{name}: delay"
            );
        }
        compare_state(&s, row, &name);
    }
}

/// FindDeployCell's Find_Nearby_Passable_Cell and GetZoneID arguments as the
/// oracle recorded them at the calls (`0x006B0417`, `0x006B0400`), against
/// the request VERA builds. The zone value itself is the oracle's answer;
/// the obstacle gate and quadrant skip, which VERA's search does not carry,
/// are pushed as 0.
#[test]
fn deploy_cell_search_matches_the_recorded_native_arguments() {
    let corpus = corpus();
    let mut compared = 0;
    for group in ["unit_manager", "unit_helper", "relocation"] {
        // The row's Slave Miner stands at (15, 15); the refinery's own cell
        // (`vt+0x1B8`) is its north-west one. YAREFN is 2x2 either way.
        let owner = if group == "relocation" {
            (12, 12)
        } else {
            (15, 15)
        };
        for row in corpus[group].as_array().unwrap() {
            let name = row["input"]["name"].as_str().unwrap();
            let events = row["events"].as_array().unwrap();
            let Some(query) = events.iter().find(|event| event[0] == "deploy_cell_query") else {
                continue;
            };
            let zone = events
                .iter()
                .find(|event| event[0] == "zone")
                .unwrap_or_else(|| panic!("{name}: GetZoneID precedes the search"));
            let seed = (
                query[1][0].as_u64().unwrap() as u16,
                query[1][1].as_u64().unwrap() as u16,
            );
            let search = DeployCellSearch::new(owner, seed, (2, 2));
            assert_eq!(
                *query,
                serde_json::json!([
                    "deploy_cell_query",
                    [search.seed.0, search.seed.1],
                    search.speed_type as i64,
                    query[3],
                    search.movement_zone as i64,
                    i64::from(search.bridge_aware),
                    search.footprint.0,
                    search.footprint.1,
                    i64::from(search.reject_any_overlay),
                    i64::from(search.check_height),
                    0,
                    i64::from(search.allow_bridge_cells),
                    [search.target.0, search.target.1],
                    0,
                    i64::from(search.check_occupancy),
                ]),
                "{name}: Find_Nearby_Passable_Cell arguments"
            );
            assert_eq!(
                *zone,
                serde_json::json!([
                    "zone",
                    [search.zone_cell.0, search.zone_cell.1],
                    search.zone_movement_zone as i64,
                    i64::from(search.zone_check_bridge),
                ]),
                "{name}: GetZoneID arguments"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 11, "every recorded search");
}

#[test]
fn replay_covers_every_row() {
    let corpus = corpus();
    let total: usize = [
        "ai_update",
        "manager",
        "deploy",
        "slave_harvest",
        "deposit",
        "unit_manager",
        "unit_helper",
        "unit_mission",
        "relocation",
        "building_helper",
    ]
    .iter()
    .map(|group| corpus[*group].as_array().unwrap().len())
    .sum();
    assert_eq!(total, 104);
    assert_eq!(SKIPPED.len(), 6);
}
