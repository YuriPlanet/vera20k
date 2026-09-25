//! Replay of `tools/spatial_oracle/refinery_dock.json`: every original row of
//! the War Miner refinery dock runs against the Rust owners — the radio
//! receive chain (`radio::receive`), Mission_Enter / Mission_Harvest /
//! Mission_Unload (`miner::refinery_dock`, `miner_system`), the
//! Per_Cell_Process DOCK_NOW sender and the StageClass tick — on a scene
//! built like the oracle's: a War Miner and a 4x3 DockUnload refinery whose
//! NW cell is (6, 9), so the pad is (9, 10).
//!
//! Compared per row: the transmit sequence (sender, message, receiver,
//! reply), the reply or dispatch delay (the Rate base; the Scenario draw is
//! the Rust stream's own, one draw per native draw), the contacts, tethers,
//! NavCom, facing destination, queued and current mission, Unload status,
//! the +0x6D1 latch, the stage counter, cargo and the refinery owner's
//! credits and harvested stat.
//!
//! Not compared: the animation producers and Enter_Idle_Mode, which the
//! oracle observes without executing (their Rust owners are pinned by their
//! own oracles); the Harvest rows' supplied Find_Docking_Bay answers are
//! reproduced by the scene (a busy slot, an offline refinery) rather than
//! supplied, and a Find_Nearby_Passable_Cell miss is wheel-impassable ground.

use super::lifecycle_tests::common_raw_terrain_cell;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::components::NavTargetRef;
use crate::sim::miner::{CargoBale, MinerState, ResourceType};
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::FacingClass;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::world::Simulation;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

const RULES: &str = "[VehicleTypes]\n0=HARV\n1=MTNK\n\
    [HARV]\nStrength=1000\nSpeed=4\nROT=5\nHarvester=yes\nDock=GAREFN\nStorage=40\n\
    MovementZone=Crusher\nUnloadingClass=HORV\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [HORV]\nStrength=1000\nSpeed=4\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [MTNK]\nStrength=300\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
    [BuildingTypes]\n0=GAREFN\n1=GAREFX\n2=GAOREP\n\
    [GAREFN]\nFoundation=4x3\nStrength=900\nRefinery=yes\nDockUnload=yes\nNumberOfDocks=1\n\
    NumberImpassableRows=3\n\
    [GAREFX]\nFoundation=4x3\nStrength=900\nRefinery=yes\nNumberOfDocks=1\nNumberImpassableRows=3\n\
    [GAOREP]\nFoundation=2x2\nStrength=1000\nOrePurifier=yes\n\
    [Tiberiums]\n0=Riparius\n1=Cruentus\n\
    [Riparius]\nImage=1\nValue=25\n[Cruentus]\nImage=2\nValue=50\n\
    [General]\nHarvesterTooFarDistance=5\nChronoHarvTooFarDistance=50\n\
    PurifierBonus=.25\nAIVirtualPurifiers=4,2,0\n\
    [AudioVisual]\nConditionYellow=50%\n\
    [Guard]\nRate=.030\n[Enter]\nRate=.016\n[Harvest]\nRate=.016\n[Unload]\nRate=.016\n";

const NW: (u16, u16) = (6, 9);
const OTHER_NW: (u16, u16) = (20, 20);

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/refinery_dock.json"
    ))
    .unwrap()
}

pub(super) struct Scene {
    pub(super) sim: Simulation,
    pub(super) rules: RuleSet,
    pub(super) miner: u64,
    pub(super) refinery: u64,
    pub(super) other: u64,
}

impl Scene {
    fn name(&self, id: u64) -> Value {
        match id {
            id if id == self.miner => "miner".into(),
            id if id == self.refinery => "refinery".into(),
            id if id == self.other => "other".into(),
            _ => Value::Null,
        }
    }

    fn id(&self, name: &str) -> u64 {
        match name {
            "miner" => self.miner,
            "refinery" => self.refinery,
            "other" => self.other,
            other => panic!("unknown object {other}"),
        }
    }
}

fn mission(name: &str) -> MissionType {
    match name {
        "guard" => MissionType::Guard,
        "enter" => MissionType::Enter,
        "harvest" => MissionType::Harvest,
        "return" => MissionType::Return,
        "unload" => MissionType::Unload,
        "selling" => MissionType::Selling,
        other => panic!("unmapped mission {other}"),
    }
}

fn cell(v: &Value) -> (u16, u16) {
    (v[0].as_u64().unwrap() as u16, v[1].as_u64().unwrap() as u16)
}

/// The oracle's map: 33x33 clear cells with native zones, bounds and a
/// passable path grid.
fn world(rules: &RuleSet, ini: &IniFile) -> Simulation {
    let mut terrain = ResolvedTerrainGrid::from_cells(
        33,
        33,
        (0..33)
            .flat_map(|y| (0..33).map(move |x| common_raw_terrain_cell(x, y, 0, false)))
            .collect(),
    );
    crate::map::resolved_terrain::install_ordinary_repair_test_catalog(&mut terrain);
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(ini);
    let costs = terrain_rules
        .semantics_for_land_type(0)
        .unwrap()
        .speed_costs;
    for y in 0..33 {
        for x in 0..33 {
            let c = terrain.cell_mut(x, y).unwrap();
            c.speed_costs = costs.clone();
            c.base_speed_costs = costs.clone();
        }
    }
    // Map width 12: every cell the rows use (x + y > 12, y - x < 12) is
    // inside the playfield diamond.
    let bounds =
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(12, 0, 0, 32, 32);
    let bridges =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, (32, 32));
    let path = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_native_map_context(
        &path,
        &BTreeMap::new(),
        &terrain,
        bridges.endpoint_records(),
        Some((32, 32)),
        Some(bounds),
    );
    assert!(zones.hierarchy_for(MovementZone::Normal).is_some());
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.playfield_bounds = Some(bounds);
    sim.playfield_size_height = Some(32);
    sim.session.map_width = 33;
    sim.session.map_height = 33;
    sim.overlay_grid = Some(OverlayGrid::new(33, 33));
    sim.bridge_state = Some(bridges);
    sim.zone_grid = Some(zones);
    sim.path_grid = Some(Arc::new(path));
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.terrain_costs = crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    sim
}

pub(super) fn scene(input: &Value) -> Scene {
    let mut text = String::from(RULES);
    // A supplied Find_Nearby_Passable_Cell miss is ground no wheel can cross.
    let wheel = if input["passable"] == serde_json::json!([null]) {
        0
    } else {
        100
    };
    for land in crate::rules::terrain_rules::LandType::ALL.iter().take(9) {
        text.push_str(&format!(
            "[{}]\nFoot=100%\nTrack=100%\nWheel={wheel}%\nBuildable=yes\n",
            land.section_name()
        ));
    }
    if let Some(mult) = input["income_mult"].as_f64() {
        text.push_str(&format!(
            "[Countries]\n0=Americans\n[Americans]\nIncomeMult={mult}\n"
        ));
    }
    let ini = IniFile::from_str(&text);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n[GAREFX]\nFoundation=4x3\nQueueingCell=4,1\n"),
    ));
    let mut sim = world(&rules, &ini);
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
    house.is_human = input["human"].as_bool().unwrap_or(true);
    house.player_control = house.is_human;
    house.difficulty = match input["difficulty"].as_u64().unwrap_or(0) {
        0 => crate::sim::house_state::HouseDifficulty::Hard,
        1 => crate::sim::house_state::HouseDifficulty::Normal,
        _ => crate::sim::house_state::HouseDifficulty::Easy,
    };
    house.economy.credits = input["balance"].as_i64().unwrap_or(0) as i32;
    if input["income_mult"].is_number() {
        house.country = Some(owner);
    }
    sim.houses.insert(owner, house);
    sim.session.game_mode_nonzero = input["game_mode"].as_u64().unwrap_or(1) != 0;
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(200) as u32;
    let heights = BTreeMap::new();
    let dock_type = if input["dock_unload"] == false {
        "GAREFX"
    } else {
        "GAREFN"
    };
    let refinery = sim
        .spawn_object(dock_type, "Americans", NW.0, NW.1, 0, &rules, &heights)
        .expect("refinery");
    let other = sim
        .spawn_object(
            "GAREFN",
            "Americans",
            OTHER_NW.0,
            OTHER_NW.1,
            0,
            &rules,
            &heights,
        )
        .expect("other refinery");
    for index in 0..input["purifiers"].as_u64().unwrap_or(0) {
        sim.spawn_object(
            "GAOREP",
            "Americans",
            22 + 3 * index as u16,
            3,
            0,
            &rules,
            &heights,
        )
        .expect("purifier");
    }
    // Spawned on a free cell, then placed: the pad is a refinery foundation
    // cell, which Unlimbo refuses.
    let miner = sim
        .spawn_object("HARV", "Americans", 16, 16, 0, &rules, &heights)
        .expect("miner");
    let (x, y) = input.get("miner_cell").map_or((10, 10), cell);
    let entity = sim.substrate.entities.get_mut(miner).unwrap();
    let (old_x, old_y) = (entity.position.rx, entity.position.ry);
    entity.position.rx = x;
    entity.position.ry = y;
    let layer = entity.occupancy_list_layer().unwrap();
    sim.substrate.occupancy.move_entity(
        old_x,
        old_y,
        x,
        y,
        miner,
        layer,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
    let scene = Scene {
        sim,
        rules,
        miner,
        refinery,
        other,
    };
    dress(scene, input)
}

/// Apply the row's prestate to the spawned objects.
fn dress(mut s: Scene, input: &Value) -> Scene {
    let frame = s.sim.session.binary_frame;
    let (miner, refinery, other) = (s.miner, s.refinery, s.other);
    // Missions, as the oracle writes +AC/+B4.
    let current = mission(input["mission"].as_str().unwrap_or("enter"));
    s.sim
        .mission_assign_exact(miner, MissionId::from_known(current), frame)
        .unwrap();
    if let Some(queued) = input["queued"].as_str() {
        s.sim
            .mission_queue_exact(
                miner,
                MissionId::from_known(mission(queued)),
                0,
                frame,
                &EntityReadyInputProvider,
            )
            .unwrap();
    }
    let status = input["status"].as_u64().unwrap_or(0) as u32;
    let building_mission = mission(input["refinery_mission"].as_str().unwrap_or("guard"));
    s.sim
        .mission_assign_exact(refinery, MissionId::from_known(building_mission), frame)
        .unwrap();
    // Contacts: linked both ways unless the row names the holders.
    let linked = input["linked"].as_bool().unwrap_or(true);
    let (miner_contact, refinery_contact) = if linked {
        (Some(refinery), Some(miner))
    } else {
        (
            input["miner_contact"].as_str().map(|n| s.id(n)),
            input["refinery_contact"].as_str().map(|n| s.id(n)),
        )
    };
    let other_contact = input["other_contact"].as_str().map(|n| s.id(n));
    for (id, contact) in [
        (miner, miner_contact),
        (refinery, refinery_contact),
        (other, other_contact),
    ] {
        let entity = s.sim.substrate.entities.get_mut(id).unwrap();
        entity.radio_contacts.clear_all();
        if let Some(contact) = contact {
            entity.radio_contacts.set_slot(0, contact);
        }
    }
    // Refinery state.
    {
        let building = s.sim.substrate.entities.get_mut(refinery).unwrap();
        building.health.current = input["refinery_health"].as_i64().unwrap_or(900) as i32;
        if input["online"] == false {
            building.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(other);
        }
        if input["refinery_tether"] == true {
            building.dock_entered_with = Some(miner);
        }
        // A live animation in the slot (the oracle's nonzero pointer); no
        // stock refinery art defines these, so the handles are placeholders.
        if input["production_anim"].as_u64().is_some_and(|a| a != 0) {
            building.building_anim_slots[8] = Some(u64::MAX - 8);
        }
        if input["special_anim"].as_u64().is_some_and(|a| a != 0) {
            building.building_anim_slots[10] = Some(u64::MAX - 10);
        }
    }
    if input["west_building"] == false {
        for y in NW.1..NW.1 + 3 {
            for x in NW.0..NW.0 + 4 {
                s.sim.substrate.occupancy.remove(x, y, refinery);
            }
        }
    }
    // Miner state.
    {
        let entity = s.sim.substrate.entities.get_mut(miner).unwrap();
        entity.mission.set_handler_state(status);
        if input["ready"]
            .as_array()
            .is_some_and(|r| r.first() == Some(&0.into()))
        {
            // The oracle answers Ready_To_Commence 0: a raised tracker byte
            // is a Unit readiness refusal with no other effect here.
            entity.mission_leaf.set_unit_tracker_byte_18(1);
        }
        if input["miner_tether"] == true {
            entity.dock_entered_with = Some(refinery);
        }
        entity.turret_rotation_latch = input["turret_latch"] == true;
        let raw = input["facing"].as_u64().unwrap_or(0xC000) as u16;
        let mut body = FacingClass::new(raw, 5);
        body.set(raw, frame);
        entity.body_facing = Some(body);
        entity.facing = (raw >> 8) as u8;
        let miner_state = entity.miner.as_mut().expect("War Miner");
        miner_state.unload_active = input["unloading"] == true;
        if let Some(stage) = input["stage"].as_array() {
            miner_state.unload_accumulator = stage[0].as_i64().unwrap() as i32;
            let start = stage[2].as_i64().unwrap();
            let left = stage[3].as_u64().unwrap() as u32;
            miner_state.unload_cluster_repeat = stage[4].as_u64().unwrap() as u32;
            if start >= 0 {
                miner_state.unload_cluster_timer.arm(start as u32, left);
            } else {
                miner_state.unload_cluster_timer.clear();
            }
        }
        if let Some(storage) = input["storage"].as_array() {
            let ore = storage[0].as_f64().unwrap() as usize;
            let gems = storage[1].as_f64().unwrap() as usize;
            miner_state.cargo = std::iter::repeat_n(
                CargoBale {
                    resource_type: ResourceType::Ore,
                    value: 25,
                },
                ore,
            )
            .chain(std::iter::repeat_n(
                CargoBale {
                    resource_type: ResourceType::Gem,
                    value: 50,
                },
                gems,
            ))
            .collect();
        }
    }
    if let Some(nav) = input.get("nav").filter(|n| !n.is_null()) {
        let nav = cell(nav);
        assert!(s.sim.set_unit_cell_destination(miner, nav, &s.rules));
        if input["moving"] != true {
            let entity = s.sim.substrate.entities.get_mut(miner).unwrap();
            crate::sim::movement::track_stop_moving(entity);
        }
    }
    s
}

/// The Rust transmit log in the oracle's event form.
fn sends(s: &Scene) -> Vec<Value> {
    radio::take_transmit_log()
        .into_iter()
        .map(|r| {
            serde_json::json!([
                "send",
                s.name(r.sender_sid),
                r.msg,
                s.name(r.target_sid),
                r.reply
            ])
        })
        .collect()
}

fn oracle_sends(row: &Value) -> Vec<Value> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "send")
        .cloned()
        .collect()
}

/// The fields every row compares against the oracle's final state.
fn compare_state(s: &Scene, row: &Value, context: &str) {
    let expected = &row["state"];
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let refinery = s.sim.substrate.entities.get(s.refinery).unwrap();
    let other = s.sim.substrate.entities.get(s.other).unwrap();
    let idle = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e[0] == "enter_idle_mode");
    if !idle {
        // The oracle observes Enter_Idle_Mode without running it; its
        // mission choice is pinned by `foot_enter_idle`.
        assert_eq!(
            miner.mission.current().raw() as i64,
            expected["miner_mission"].as_i64().unwrap(),
            "{context}: current mission"
        );
        assert_eq!(
            miner.mission.queued().raw() as i64,
            expected["miner_queued"].as_i64().unwrap(),
            "{context}: queued mission"
        );
    }
    let nav = match miner.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
        None => Value::Null,
        other => panic!("{context}: non-cell NavCom {other:?}"),
    };
    assert_eq!(nav, expected["miner_nav"], "{context}: NavCom");
    assert_eq!(
        miner
            .radio_contacts
            .slot(0)
            .map_or(Value::Null, |id| s.name(id)),
        expected["miner_contact"],
        "{context}: miner contact"
    );
    assert_eq!(
        refinery
            .radio_contacts
            .slot(0)
            .map_or(Value::Null, |id| s.name(id)),
        expected["refinery_contact"],
        "{context}: refinery contact"
    );
    assert_eq!(
        other
            .radio_contacts
            .slot(0)
            .map_or(Value::Null, |id| s.name(id)),
        expected["other_contact"],
        "{context}: other contact"
    );
    assert_eq!(
        u64::from(miner.dock_entered_with.is_some()),
        expected["miner_tether"].as_u64().unwrap(),
        "{context}: miner tether"
    );
    assert_eq!(
        u64::from(refinery.dock_entered_with.is_some()),
        expected["refinery_tether"].as_u64().unwrap(),
        "{context}: refinery tether"
    );
    let desired = miner
        .body_facing
        .as_ref()
        .map_or(u16::from(miner.facing) << 8, |body| body.destination());
    assert_eq!(
        u64::from(desired),
        expected["facing"]["desired"].as_u64().unwrap(),
        "{context}: facing destination"
    );
}

fn compare_unload(s: &Scene, row: &Value, context: &str) {
    let expected = &row["state"];
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let state = miner.miner.as_ref().unwrap();
    if miner.mission.current() == MissionId::from_known(MissionType::Unload) {
        assert_eq!(
            miner.mission.handler_state() as i64,
            expected["miner_status"].as_i64().unwrap(),
            "{context}: Unload status"
        );
    }
    assert_eq!(
        u64::from(state.unload_active),
        expected["unloading"].as_u64().unwrap(),
        "{context}: +0x6D1"
    );
    assert_eq!(
        i64::from(state.unload_accumulator),
        expected["stage"][0].as_i64().unwrap(),
        "{context}: stage value"
    );
    let ore = state
        .cargo
        .iter()
        .filter(|b| b.resource_type == ResourceType::Ore)
        .count() as f64;
    let gems = state
        .cargo
        .iter()
        .filter(|b| b.resource_type == ResourceType::Gem)
        .count() as f64;
    assert_eq!(
        ore,
        expected["storage"][0].as_f64().unwrap(),
        "{context}: ore"
    );
    assert_eq!(
        gems,
        expected["storage"][1].as_f64().unwrap(),
        "{context}: gems"
    );
    let house = s
        .sim
        .houses
        .get(&s.sim.interner.get("Americans").unwrap())
        .unwrap();
    let (balance, score) = (
        i64::from(house.economy.credits),
        i64::from(house.economy.harvested_credits),
    );
    if row["input"]["name"] == "unload_gate_income_mult" {
        // RESIDUAL (documented on `refinery_dock`): native 0.9f pays
        // ftol(25 * 0.9f * 40) = 899, VERA's integer economy 900.
        assert_eq!((balance, expected["balance"].as_i64().unwrap()), (900, 899));
    } else if row["input"]["name"] == "unload_gate_income_mult_bonus" {
        // Native 877 + ftol(243.37.. + 877) = 1096; VERA 877 + 219 = 1096.
        assert_eq!(
            balance,
            expected["balance"].as_i64().unwrap(),
            "{context}: balance"
        );
    } else {
        assert_eq!(
            balance,
            expected["balance"].as_i64().unwrap(),
            "{context}: balance"
        );
    }
    assert_eq!(
        score,
        expected["score"].as_i64().unwrap(),
        "{context}: score"
    );
}

/// Assert the dispatch delay: the native delay minus its draw is the Rate
/// base, and Rust adds its own single draw from the Scenario stream as it
/// stood before the dispatch. After an Enter_Idle_Mode the oracle did not
/// run, the base is the Rate of the mission Rust's idle mode commenced.
fn compare_delay(
    s: &Scene,
    row: &Value,
    delay: i32,
    stream: &mut crate::sim::rng::SimRng,
    context: &str,
) {
    let idle = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e[0] == "enter_idle_mode");
    let draws: Vec<&Value> = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "random")
        .collect();
    let native = row["delay"].as_i64().unwrap() as i32;
    match draws.as_slice() {
        [] => assert_eq!(delay, native, "{context}: delay without a draw"),
        [draw] => {
            assert_eq!((draw[1].as_i64(), draw[2].as_i64()), (Some(0), Some(2)));
            let base = if idle {
                let current = s
                    .sim
                    .substrate
                    .entities
                    .get(s.miner)
                    .unwrap()
                    .mission
                    .current();
                s.rules
                    .mission_control
                    .rate_frames(current.known().unwrap()) as i32
            } else {
                native - draw[3].as_i64().unwrap() as i32
            };
            let rust_draw = stream.next_range_u32_inclusive(0, 2) as i32;
            assert_eq!(delay, base + rust_draw, "{context}: Rate base + one draw");
        }
        _ => panic!("{context}: more than one draw"),
    }
}

#[test]
fn docking_handshake_matches_the_original_receivers() {
    let corpus = corpus();
    for row in corpus["can_dock"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        radio::take_transmit_log();
        let reply = radio::transmit(
            &mut s.sim,
            s.miner,
            s.refinery,
            RadioMessage::CanDock,
            RadioPayload::default(),
            Some(&s.rules),
        );
        assert_eq!(
            u64::from(reply.code()),
            row["reply"].as_u64().unwrap(),
            "{context}: reply"
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_state(&s, row, &context);
    }
}

#[test]
fn radio_core_matches_the_original_transmits() {
    let corpus = corpus();
    for row in corpus["radio"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        let (sender, target) = match input["from"].as_str().unwrap() {
            "miner" => (s.miner, s.refinery),
            _ => (s.refinery, s.miner),
        };
        let msg = match input["msg"].as_u64().unwrap() {
            2 => RadioMessage::Hello,
            3 => RadioMessage::Break,
            0x15 => RadioMessage::DockNow,
            0x18 => RadioMessage::Tether,
            0x19 => RadioMessage::Untether,
            other => panic!("unmapped message {other}"),
        };
        radio::take_transmit_log();
        let reply = radio::transmit(
            &mut s.sim,
            sender,
            target,
            msg,
            RadioPayload::default(),
            Some(&s.rules),
        );
        assert_eq!(
            u64::from(reply.code()),
            row["reply"].as_u64().unwrap(),
            "{context}: reply"
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_state(&s, row, &context);
    }
}

#[test]
fn mission_enter_matches_the_original_dispatch() {
    let corpus = corpus();
    for row in corpus["mission_enter"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        if let Some(queue) = input["nav_queue"].as_array() {
            let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            entity.navigation.nav_queue = queue
                .iter()
                .map(|c| {
                    let (x, y) = cell(c);
                    NavTargetRef::cell(x, y)
                })
                .collect();
        }
        radio::take_transmit_log();
        let mut stream = s.sim.scenario_rng.clone();
        let delay = crate::sim::miner::mission_enter(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_state(&s, row, &context);
    }
}

#[test]
fn mission_harvest_states_two_and_three_match_the_original_dispatch() {
    let corpus = corpus();
    for row in corpus["mission_harvest"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        // The supplied Find_Docking_Bay answers, reproduced by the scene: the
        // second refinery is offline (CAN_LOAD refuses it), and no bay at all
        // takes the dock refinery offline too.
        let offline = if input["bays"][0].is_null() && input["bays"][1].is_null() {
            vec![s.refinery, s.other]
        } else {
            vec![s.other]
        };
        for id in offline {
            s.sim.substrate.entities.get_mut(id).unwrap().temporal =
                crate::sim::temporal::TemporalState::warped_by_for_test(s.miner);
        }
        // Full cargo: the Harvest handler is on its return states.
        {
            let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            let miner = entity.miner.as_mut().unwrap();
            miner.cargo = vec![
                CargoBale {
                    resource_type: ResourceType::Ore,
                    value: 25,
                };
                usize::from(miner.capacity_bales)
            ];
            entity
                .mission
                .set_handler_state(match input["status"].as_u64() {
                    Some(3) => MinerState::Dock.cursor(),
                    _ => MinerState::ReturnToRefinery.cursor(),
                });
        }
        radio::take_transmit_log();
        let mut stream = s.sim.scenario_rng.clone();
        let config = crate::sim::miner::MinerConfig::from_rules(&s.rules);
        let frame = s.sim.session.binary_frame;
        crate::sim::miner::dispatch_harvest_for_object(
            &mut s.sim,
            &s.rules,
            &config,
            None,
            Some(crate::sim::tiberium::test_support::overlay_registry()),
            s.miner,
        );
        let entity = s.sim.substrate.entities.get(s.miner).unwrap();
        let timer = entity.mission.dispatch_timer();
        let delay = timer.delay();
        assert_eq!(
            timer.start_frame(),
            frame as i32,
            "{context}: epilogue frame"
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        let status = if entity.miner_state() == Some(MinerState::Dock) {
            3
        } else {
            2
        };
        assert_eq!(
            status,
            row["state"]["miner_status"].as_i64().unwrap(),
            "{context}: Harvest status"
        );
        compare_state(&s, row, &context);
    }
}

#[test]
fn mission_unload_matches_the_original_harvester_branch() {
    let corpus = corpus();
    for row in corpus["mission_unload"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        radio::take_transmit_log();
        let mut stream = s.sim.scenario_rng.clone();
        let delay = crate::sim::miner::mission_unload(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_state(&s, row, &context);
        compare_unload(&s, row, &context);
    }
}

#[test]
fn per_cell_dock_now_matches_the_original_track_end_arm() {
    let corpus = corpus();
    for row in corpus["per_cell"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        radio::take_transmit_log();
        crate::sim::miner::per_cell_dock_now(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_state(&s, row, &context);
    }
}

#[test]
fn stage_tick_matches_the_original_stageclass_step() {
    let corpus = corpus();
    for row in corpus["stage_tick"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = scene(input);
        for sample in row["values"].as_array().unwrap() {
            s.sim.session.binary_frame = sample[0].as_u64().unwrap() as u32;
            crate::sim::miner::tick_unload_stage(&mut s.sim, s.miner);
            let value = s
                .sim
                .substrate
                .entities
                .get(s.miner)
                .unwrap()
                .miner
                .as_ref()
                .unwrap()
                .unload_accumulator;
            assert_eq!(
                i64::from(value),
                sample[1].as_i64().unwrap(),
                "{context}: frame {}",
                sample[0]
            );
        }
    }
}

#[test]
fn replay_covers_every_row() {
    let corpus = corpus();
    let count = |key: &str| corpus[key].as_array().unwrap().len();
    assert_eq!(count("can_dock"), 19);
    assert_eq!(count("radio"), 18);
    assert_eq!(count("mission_enter"), 9);
    assert_eq!(count("mission_harvest"), 9);
    assert_eq!(count("mission_unload"), 30);
    assert_eq!(count("per_cell"), 6);
    assert_eq!(count("stage_tick"), 3);
}
