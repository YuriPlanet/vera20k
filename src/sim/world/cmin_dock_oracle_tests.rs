//! Replay of `tools/spatial_oracle/cmin_dock.json`: every original row of
//! the Chrono Miner's refinery return runs against the Rust owners — the Unit
//! setter's Teleporter arm and Foot tail (`Simulation::set_unit_cell_destination`
//! / `set_unit_null_destination`), Teleport Move_To (`teleport_move_to`), the
//! warp in the object turn, Mission_Harvest states 0, 2 and 3, Mission_Enter,
//! Mission_Unload's turn and the Per_Cell DOCK_NOW arm — on the War Miner
//! scene (`refinery_dock_oracle_tests`) with a `Teleporter=` miner: refinery
//! NW (6, 9), pad (9, 10), `ChronoHarvTooFarDistance=10` as in the fixture.
//!
//! Compared per row: NavCom, Techno+0x1F8, the active locomotor and its stash,
//! a Drive created or ended, the Drive's and the armed Teleport's
//! destinations, the current and queued mission, the Harvest status, radio
//! contacts and tethers, the facing destination, the location, the transmit
//! sequence, the dispatch delay (the Rate base plus one Scenario draw), and
//! for a warp its ChronoOut/ChronoIn sounds, WarpOut animations and the
//! warp-in byte frame by frame.
//!
//! Not compared: the Teleport destination's occupy-bit reservation and COM
//! reference counts (residuals on `teleport_move_to`), and the
//! Enter_Idle_Mode a non-harvester's warp-in expiry runs (`0x00719BF0`; a
//! harvester never reaches it). Rows whose prestate or path has no Rust
//! owner are listed in [`UNREPRESENTED`].

use super::refinery_dock_oracle_tests::{
    RULES, Scene, cell, compare_delay, oracle_sends, scene_with, sends,
};
use super::techno_ai::ObjectAiCtx;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::miner::{CargoBale, MinerState, ResourceType};
use crate::sim::movement::teleport_movement::{self, TeleportPhase, TeleportState};
use crate::sim::radio;
use crate::sim::world::SimSoundEvent;
use serde_json::Value;

/// Rows whose native prestate or path VERA does not represent:
/// - `latch_27c`, `lifted_2b0`: Techno+0x27C (the Chronosphere's transit
///   latch) and +0x2B0 (a lifted owner) have no Rust producer and read clear.
/// - `pad_cannot_enter*`: Teleport Move_To's destination resolution
///   (`0x00718B70`) — the Can_Enter_Cell refusal of a cell whose occupy bit
///   is set with no Unit in its list, and the nearby-cell fallback.
/// - `move_to_guard_deploying`: Move_To's `vt+0x37C` (the EMP lock or the
///   Unit death-frame counter +0x6D8), unrepresented as for the Drive.
const UNREPRESENTED: [&str; 5] = [
    "latch_27c",
    "lifted_2b0",
    "pad_cannot_enter",
    "pad_cannot_enter_no_cell",
    "move_to_guard_deploying",
];

const CMIN: &str = "[CMIN]\nStrength=400\nSpeed=4\nROT=5\nHarvester=yes\nTeleporter=yes\n\
    Dock=GAREFN\nStorage=20\nMovementZone=Crusher\nUnloadingClass=CMON\n\
    ChronoInSound=CminIn\nChronoOutSound=CminOut\n\
    Locomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n\
    [CMON]\nStrength=400\nSpeed=4\nLocomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n\
    [CTNK]\nStrength=400\nSpeed=4\nROT=5\nTeleporter=yes\n\
    ChronoInSound=CminIn\nChronoOutSound=CminOut\n\
    Locomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n";

fn corpus() -> Value {
    serde_json::from_str(include_str!("../../../tools/spatial_oracle/cmin_dock.json")).unwrap()
}

fn skipped(row: &Value) -> bool {
    UNREPRESENTED.contains(&row["input"]["name"].as_str().unwrap())
}

/// The War scene's rules with the fixture's Chrono Miner, its non-harvester
/// twin and the retail chrono `[General]` values.
pub(super) fn cmin_rules(input: &Value) -> (RuleSet, IniFile) {
    let general = format!(
        "[General]\nChronoDelay=60\nChronoDistanceFactor=48\nChronoTrigger=yes\n\
         ChronoMinimumDelay=16\nChronoRangeMinimum={}\nWarpOut=WARPOUT\n",
        input["range_minimum"].as_i64().unwrap_or(0)
    );
    let mut text = RULES
        .replacen("1=MTNK\n", "1=MTNK\n2=CMIN\n3=CTNK\n", 1)
        .replace("ChronoHarvTooFarDistance=50", "ChronoHarvTooFarDistance=10")
        .replacen("[General]\n", &general, 1);
    text.push_str(CMIN);
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
    let ini = IniFile::from_str(&text);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n[GAREFX]\nFoundation=4x3\nQueueingCell=4,1\n\
         [WARPOUT]\nFlat=yes\nTranslucent=yes\nRate=120\n",
    ));
    art.bind_anim_frame_count_for_test("WARPOUT", 13);
    rules.merge_art_data(&art);
    (rules, ini)
}

/// Move `id` into `to`'s cell list at the cell centre (Unlimbo refuses the
/// refinery's foundation cells).
fn place(s: &mut Scene, id: u64, to: (u16, u16)) {
    let entity = s.sim.substrate.entities.get_mut(id).unwrap();
    let from = (entity.position.rx, entity.position.ry);
    entity.position.rx = to.0;
    entity.position.ry = to.1;
    let layer = entity.occupancy_list_layer().unwrap();
    s.sim.substrate.occupancy.move_entity(
        from.0,
        from.1,
        to.0,
        to.1,
        id,
        layer,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
}

fn coord(v: &Value) -> DriveCoord {
    DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    }
}

/// The War scene dressed with the row's CMIN prestate: the raw NavCom (the
/// oracle writes +0x5A4 directly), a Drive piggybacked over the Teleport, the
/// +0x1F8/+0x6AD bytes, the paralysis timer and the warp bytes.
fn cmin_scene(input: &Value) -> Scene {
    let mut base = input.clone();
    let fields = base.as_object_mut().unwrap();
    fields.remove("nav");
    let miner_type = if input["harvester"] == false {
        "CTNK"
    } else {
        "CMIN"
    };
    fields.insert("miner_type".into(), miner_type.into());
    let (rules, ini) = cmin_rules(input);
    let mut s = scene_with(&base, rules, &ini);
    let frame = s.sim.session.binary_frame;
    if input["pad_unit"] == true {
        // A second Unit listed first in the pad cell (Get_Unit finds it).
        let tank = s
            .sim
            .spawn_object(
                "MTNK",
                "Americans",
                20,
                4,
                0,
                &s.rules,
                &std::collections::BTreeMap::new(),
            )
            .expect("pad unit");
        place(&mut s, tank, (9, 10));
    }
    if let Some(exact) = input.get("miner_coord") {
        let exact = coord(exact);
        let miner = s.miner;
        place(
            &mut s,
            miner,
            ((exact.x >> 8) as u16, (exact.y >> 8) as u16),
        );
        let position = &mut s.sim.substrate.entities.get_mut(miner).unwrap().position;
        position.sub_x = crate::util::fixed_math::SimFixed::from_num(exact.x & 0xFF);
        position.sub_y = crate::util::fixed_math::SimFixed::from_num(exact.y & 0xFF);
    }
    let nav = input.get("nav").filter(|n| !n.is_null()).map(cell);
    let terrain = s.sim.resolved_terrain.clone();
    let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
    if input["loco"] == "drive_piggy" {
        assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(entity, frame));
        if input["moving"] == true {
            let current = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let drive = entity.drive_locomotion.get_or_insert_with(Default::default);
            drive.head_to = Some(current);
            drive.destination =
                nav.map(|(x, y)| crate::sim::movement::target_cell_coord(x, y, terrain.as_ref()));
        }
    }
    entity.navigation.nav_com = nav.map(|(x, y)| NavTargetRef::cell(x, y));
    entity.setter_force_reassign = input["force_reassign"] == true;
    entity.foot_locomotor_swap_active = input["swap_6ad"] == true;
    if input["locked"] == true {
        entity.paralysis_timer.start(frame as i32, 30);
    }
    if input["warp_out"] == true {
        entity.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(s.other);
    }
    if input["warp_in"] == true {
        entity.teleport_state = Some(TeleportState {
            phase: TeleportPhase::ChronoDelay,
            target_rx: 10,
            target_ry: 10,
            being_warped_ticks: 30,
        });
    }
    s
}

fn kind_name(kind: LocomotorKind) -> &'static str {
    match kind {
        LocomotorKind::Teleport => "teleport",
        LocomotorKind::Drive => "drive",
        other => panic!("unexpected locomotor {other:?}"),
    }
}

/// The CMIN state every row compares (`state` is the oracle's final state).
fn compare_cmin(s: &Scene, state: &Value, context: &str) {
    let entity = s.sim.substrate.entities.get(s.miner).unwrap();
    let nav = match entity.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
        None => Value::Null,
        other => panic!("{context}: non-cell NavCom {other:?}"),
    };
    assert_eq!(nav, state["nav"], "{context}: NavCom");
    assert_eq!(
        u64::from(entity.setter_force_reassign),
        state["force_reassign"].as_u64().unwrap(),
        "{context}: +0x1F8"
    );
    let locomotor = entity.locomotor.as_ref().unwrap();
    let expected = &state["locomotor"];
    assert_eq!(
        kind_name(locomotor.active_kind()),
        expected["kind"].as_str().unwrap(),
        "{context}: active locomotor"
    );
    assert_eq!(
        locomotor
            .piggyback
            .as_ref()
            .map_or(Value::Null, |stash| kind_name(stash.kind).into()),
        expected["stash"].clone(),
        "{context}: stash"
    );
    if locomotor.active_kind() == LocomotorKind::Drive {
        let destination = entity
            .drive_locomotion
            .as_ref()
            .and_then(|drive| drive.destination)
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        assert_eq!(
            destination,
            coord(&expected["destination"]),
            "{context}: Drive destination"
        );
    }
    let teleport = &state["teleport"];
    let armed = entity
        .teleport_state
        .as_ref()
        .filter(|warp| warp.phase == TeleportPhase::Relocate);
    assert_eq!(
        u64::from(armed.is_some()),
        teleport["moving"].as_u64().unwrap(),
        "{context}: Teleport request"
    );
    if let Some(warp) = armed {
        let destination = coord(&teleport["destination"]);
        assert_eq!(
            (warp.target_rx, warp.target_ry),
            ((destination.x >> 8) as u16, (destination.y >> 8) as u16),
            "{context}: Teleport destination"
        );
    }
    assert_eq!(
        entity.mission.current().raw() as i64,
        state["mission"].as_i64().unwrap(),
        "{context}: mission"
    );
    assert_eq!(
        entity.mission.queued().raw() as i64,
        state["queued"].as_i64().unwrap(),
        "{context}: queued mission"
    );
    assert_eq!(
        entity
            .radio_contacts
            .slot(0)
            .map_or(Value::Null, |id| s.name(id)),
        state["miner_contact"],
        "{context}: miner contact"
    );
    assert_eq!(
        u64::from(entity.dock_entered_with.is_some()),
        state["miner_tether"].as_u64().unwrap(),
        "{context}: miner tether"
    );
    let location = coord(&state["location"]);
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        location,
        "{context}: location"
    );
    let desired = entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |body| body.destination());
    assert_eq!(
        u64::from(desired),
        state["facing"]["desired"].as_u64().unwrap(),
        "{context}: facing destination"
    );
}

/// A Drive CoCreated (`cocreate`) or ended (`end_piggyback`) in this call is
/// the active kind changing between Teleport and Drive.
fn compare_swap(before: LocomotorKind, s: &Scene, events: &[Value], context: &str) {
    let after = s
        .sim
        .substrate
        .entities
        .get(s.miner)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .active_kind();
    let created = events.iter().any(|e| e[0] == "cocreate");
    let ended = events.iter().any(|e| e[0] == "end_piggyback");
    match (created, ended) {
        (false, false) => assert_eq!(after, before, "{context}: no swap"),
        (true, false) => assert_eq!(
            (before, after),
            (LocomotorKind::Teleport, LocomotorKind::Drive),
            "{context}: Drive created"
        ),
        (false, true) => assert_eq!(
            (before, after),
            (LocomotorKind::Drive, LocomotorKind::Teleport),
            "{context}: Drive ended"
        ),
        // The Drive ended, then a new one created for the destination.
        (true, true) => assert_eq!(
            (before, after),
            (LocomotorKind::Drive, LocomotorKind::Drive),
            "{context}: Drive replaced"
        ),
    }
}

fn active(s: &Scene) -> LocomotorKind {
    s.sim
        .substrate
        .entities
        .get(s.miner)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .active_kind()
}

#[test]
fn unit_setter_teleporter_arm_matches_the_original_assign_destination() {
    let corpus = corpus();
    for row in corpus["assign_destination"].as_array().unwrap() {
        if skipped(row) {
            continue;
        }
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        let before = active(&s);
        match input.get("dest").filter(|d| !d.is_null()) {
            Some(dest) => {
                s.sim
                    .set_unit_cell_destination(s.miner, cell(dest), &s.rules);
            }
            None => {
                s.sim.set_unit_null_destination(s.miner, Some(&s.rules));
            }
        }
        compare_swap(before, &s, row["events"].as_array().unwrap(), &context);
        compare_cmin(&s, &row["state"], &context);
    }
}

#[test]
fn teleport_move_to_guards_match_the_original_refusals() {
    let corpus = corpus();
    for row in corpus["teleport_move_to"].as_array().unwrap() {
        if skipped(row) {
            continue;
        }
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        let frame = s.sim.session.binary_frame;
        let general = s.rules.general.clone();
        let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
        assert!(
            !teleport_movement::teleport_move_to(
                entity,
                cell(&input["dest"]),
                &general,
                true,
                frame
            ),
            "{context}: refused"
        );
        // The warp-in row's prestate is a running delay, not a request.
        if input["warp_in"] == true {
            entity.teleport_state = None;
        }
        compare_cmin(&s, &row["state"], &context);
    }
}

#[test]
fn teleport_warp_matches_the_original_process() {
    let corpus = corpus();
    for row in corpus["teleport_process"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        let harvester = input["harvester"] != false;
        let frame = s.sim.session.binary_frame;
        let general = s.rules.general.clone();
        let dest = cell(&input["dest"]);
        {
            let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            // Armed by a direct Move_To, as the oracle does; the mission waits.
            assert!(teleport_movement::teleport_move_to(
                entity, dest, &general, harvester, frame
            ));
            entity
                .mission
                .write_dispatch_epilogue(frame as i32, 100_000);
        }
        let origin = {
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            (entity.position.rx, entity.position.ry)
        };
        let warp_out = s.sim.interner.intern("WARPOUT");
        let ticks = row["ticks"].as_array().unwrap();
        let last = ticks.last().unwrap()["frame"].as_u64().unwrap() as u32;
        let mut checked = ticks.iter().peekable();
        for now in frame..=last {
            s.sim.session.binary_frame = now;
            let sounds_before = s.sim.sound_events.len();
            s.sim
                .advance_live_object_turn(s.miner, Some(&s.rules), ObjectAiCtx::default())
                .unwrap();
            let Some(tick) = checked.next_if(|t| t["frame"].as_u64() == Some(u64::from(now)))
            else {
                continue;
            };
            let events = tick["events"].as_array().unwrap();
            let context = format!("{context} frame {now}");
            // ChronoOut at the old location, then ChronoIn at the new one.
            let expected_sounds: Vec<(String, (u16, u16))> = events
                .iter()
                .filter(|e| e[0] == "sound")
                .map(|e| {
                    let name = match e[1].as_u64().unwrap() {
                        0x41 => "CminIn",
                        0x42 => "CminOut",
                        other => panic!("sound {other}"),
                    };
                    let at = coord(&e[2]);
                    (name.to_string(), ((at.x >> 8) as u16, (at.y >> 8) as u16))
                })
                .collect();
            let sounds: Vec<(String, (u16, u16))> = s.sim.sound_events[sounds_before..]
                .iter()
                .filter_map(|event| match event {
                    SimSoundEvent::ChronoTeleport { sound_id, rx, ry } => {
                        Some((s.sim.interner.resolve(*sound_id).to_string(), (*rx, *ry)))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(sounds, expected_sounds, "{context}: warp sounds");
            let expected_anims: Vec<(u16, u16)> = events
                .iter()
                .filter(|e| e[0] == "anim")
                .map(|e| {
                    let at = coord(&e[2]);
                    ((at.x >> 8) as u16, (at.y >> 8) as u16)
                })
                .collect();
            if now == frame {
                let mut anims: Vec<(u16, u16)> = s
                    .sim
                    .substrate
                    .anims
                    .iter()
                    .map(|(_, anim)| anim)
                    .filter(|anim| anim.type_id == warp_out)
                    .map(|anim| {
                        let (rx, ry, _, _, _) = anim.world_coord.to_cell_sub_z();
                        (rx, ry)
                    })
                    .collect();
                anims.sort_unstable();
                let mut expected_anims = expected_anims;
                expected_anims.sort_unstable();
                assert_eq!(anims, expected_anims, "{context}: WarpOut animations");
                if expected_anims.is_empty() {
                    let entity = s.sim.substrate.entities.get(s.miner).unwrap();
                    assert_eq!((entity.position.rx, entity.position.ry), origin);
                }
            }
            let state = &tick["state"];
            compare_cmin(&s, state, &context);
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            assert_eq!(
                u64::from(entity.is_warping_in()),
                state["warp"][1].as_u64().unwrap(),
                "{context}: +0x271"
            );
        }
        assert!(checked.next().is_none(), "{context}: every tick replayed");
    }
}

/// Full cargo for the return states; state 0 starts empty, as the fixture.
fn fill(s: &mut Scene, status: u64) {
    let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
    let miner = entity.miner.as_mut().unwrap();
    if status != 0 {
        miner.cargo = vec![
            CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            };
            usize::from(miner.capacity_bales)
        ];
    }
    entity.mission.set_handler_state(match status {
        0 => MinerState::SearchOre.cursor(),
        3 => MinerState::Dock.cursor(),
        _ => MinerState::ReturnToRefinery.cursor(),
    });
}

#[test]
fn mission_harvest_teleporter_arms_match_the_original_dispatch() {
    let corpus = corpus();
    for row in corpus["mission_harvest"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        // The supplied Find_Docking_Bay answers, reproduced as in the War
        // replay: the second refinery is offline, and no bay at all takes the
        // dock refinery offline too.
        let offline = if input["bays"][0].is_null() && input["bays"][1].is_null() {
            vec![s.refinery, s.other]
        } else {
            vec![s.other]
        };
        for id in offline {
            s.sim.substrate.entities.get_mut(id).unwrap().temporal =
                crate::sim::temporal::TemporalState::warped_by_for_test(s.miner);
        }
        let status = input["status"].as_u64().unwrap();
        fill(&mut s, status);
        radio::take_transmit_log();
        let before = active(&s);
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
        assert_eq!(
            timer.start_frame(),
            frame as i32,
            "{context}: epilogue frame"
        );
        let delay = timer.delay();
        let cursor = match entity.miner_state() {
            Some(MinerState::SearchOre) => 0,
            Some(MinerState::Dock) => 3,
            Some(MinerState::WaitNoOre) => 4,
            _ => 2,
        };
        assert_eq!(
            cursor,
            row["state"]["status"].as_i64().unwrap(),
            "{context}: Harvest status"
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_swap(before, &s, row["events"].as_array().unwrap(), &context);
        compare_cmin(&s, &row["state"], &context);
    }
}

#[test]
fn mission_enter_teleporter_arms_match_the_original_dispatch() {
    let corpus = corpus();
    for row in corpus["mission_enter"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
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
        let before = active(&s);
        let mut stream = s.sim.scenario_rng.clone();
        let delay = crate::sim::miner::mission_enter(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_swap(before, &s, row["events"].as_array().unwrap(), &context);
        compare_cmin(&s, &row["state"], &context);
        if let Some(queue) = input["nav_queue"].as_array() {
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            assert_eq!(
                entity.navigation.nav_queue.len(),
                queue.len() - 1,
                "{context}: NavQueue[0] taken"
            );
        }
    }
}

#[test]
fn mission_unload_turns_a_teleporter_like_the_original() {
    let corpus = corpus();
    for row in corpus["mission_unload"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        radio::take_transmit_log();
        let mut stream = s.sim.scenario_rng.clone();
        let delay = crate::sim::miner::mission_unload(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_cmin(&s, &row["state"], &context);
    }
}

#[test]
fn per_cell_dock_now_waits_for_the_tether_after_a_warp() {
    let corpus = corpus();
    for row in corpus["per_cell"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap().to_string();
        let mut s = cmin_scene(input);
        radio::take_transmit_log();
        crate::sim::miner::per_cell_dock_now(&mut s.sim, &s.rules, s.miner);
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_cmin(&s, &row["state"], &context);
    }
}

#[test]
fn replay_covers_every_row() {
    let corpus = corpus();
    let count = |key: &str| corpus[key].as_array().unwrap().len();
    assert_eq!(count("assign_destination"), 17);
    assert_eq!(count("teleport_move_to"), 4);
    assert_eq!(count("teleport_process"), 6);
    assert_eq!(count("mission_harvest"), 15);
    assert_eq!(count("mission_enter"), 5);
    assert_eq!(count("mission_unload"), 1);
    assert_eq!(count("per_cell"), 2);
    let names: Vec<&str> = ["assign_destination", "teleport_move_to"]
        .iter()
        .flat_map(|key| corpus[*key].as_array().unwrap())
        .filter(|row| skipped(row))
        .map(|row| row["input"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), UNREPRESENTED.len(), "{names:?}");
}
