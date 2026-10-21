//! Replay of `tools/spatial_oracle/cmin_dock.json`: every original row of
//! the Chrono Miner's refinery return runs against the Rust owners — the Unit
//! setter's Teleporter arm and Foot tail (`Simulation::set_unit_destination`
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
//! warp-in byte frame by frame. A setter row that reaches Teleport Move_To
//! also compares the resolver's Marked coordinate (`+0x28`) and the occupy
//! bit 0x20 of every cell; the harness stubs Find_Nearby_Passable_Cell at
//! `0x00719185`, so the replay hands its answer to the Rust search
//! (`fresh_oracle_seam::install_nearby_only`) and compares the arguments.
//!
//! Not compared: COM reference counts, and the
//! Enter_Idle_Mode a non-harvester's warp-in expiry runs (`0x00719BF0`; a
//! harvester never reaches it). The legacy native producer records
//! `enter_idle_mode(0,1)` then returns without executing Unit738970
//! (`refinery_dock::observe_dock`, native_idle=false). Only on those expiry
//! rows, current/queued missions are outside the native comparison boundary;
//! Guard is asserted separately as a Rust integration result. All other
//! observed fields and other rows' mission comparisons remain bounded by
//! their original controls. Rows whose prestate or path has no Rust owner
//! are listed in [`UNREPRESENTED`].

use super::refinery_dock_oracle_tests::{
    RULES, Scene, cell, compare_delay, oracle_sends, scene_with, sends,
};
use super::techno_ai::ObjectAiCtx;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::miner::{CargoBale, MinerState, ResourceType};
use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord};
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::radio;
use crate::sim::world::SimSoundEvent;
use serde_json::Value;

/// Rows whose native prestate or path VERA does not represent:
/// - `lifted_2b0`: +0x2B0 (a lifted owner) has no Rust producer and reads
///   clear.
/// - `move_to_guard_deploying`: Move_To's `vt+0x37C` (the EMP lock or the
///   Unit death-frame counter +0x6D8), unrepresented as for the Drive.
const UNREPRESENTED: [&str; 2] = ["lifted_2b0", "move_to_guard_deploying"];

pub(super) const CMIN: &str = "[CMIN]\nStrength=400\nSpeed=4\nROT=5\nHarvester=yes\nTeleporter=yes\n\
    Dock=GAREFN\nStorage=20\nMovementZone=Crusher\nUnloadingClass=CMON\n\
    ChronoInSound=CminIn\nChronoOutSound=CminOut\n\
    Locomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n\
    [CMON]\nStrength=400\nSpeed=4\nLocomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n\
    [CTNK]\nStrength=400\nSpeed=4\nROT=5\nTeleporter=yes\n\
    ChronoInSound=CminIn\nChronoOutSound=CminOut\n\
    Locomotor={4A582747-9839-11D1-B709-00A024DDAFD1}\n";

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/cmin_dock.json")
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
    let art_ini = IniFile::from_str(
        "[GAREFN]\nFoundation=4x3\nQueueingCell=4,1\n[GAREFX]\nFoundation=4x3\nQueueingCell=4,1\n\
         [GAOREP]\nFoundation=2x2\n\
         [WARPOUT]\nFlat=yes\nTranslucent=yes\nRate=120\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("WARPOUT", 13);
    rules.install_art_data(art);
    (rules, ini)
}

/// Move `id` into `to`'s cell list at the cell centre (Unlimbo refuses the
/// refinery's foundation cells).
fn place(s: &mut Scene, id: u64, to: (u16, u16)) {
    let entity = s.sim.substrate.entities.get_mut(id).unwrap();
    let from = (entity.position.rx(), entity.position.ry());
    entity.position.set_rx_for_test(to.0);
    entity.position.set_ry_for_test(to.1);
    let layer = s
        .sim
        .substrate
        .occupancy
        .listed_layer(from.0, from.1, id)
        .unwrap();
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

/// The War scene's input for a CMIN row: the Chrono Miner (or its
/// non-harvester twin) as the miner, and no NavCom yet (`dress_cmin` writes
/// it raw, as the oracle writes +0x5A4).
pub(super) fn cmin_base_input(input: &Value) -> Value {
    let mut base = input.clone();
    let fields = base.as_object_mut().unwrap();
    fields.remove("nav");
    let miner_type = if input["harvester"] == false {
        "CTNK"
    } else {
        "CMIN"
    };
    fields.insert("miner_type".into(), miner_type.into());
    base
}

/// The War scene dressed with the row's CMIN prestate: the raw NavCom (the
/// oracle writes +0x5A4 directly), a Drive piggybacked over the Teleport, the
/// +0x1F8/+0x6AD bytes, the paralysis timer and the warp bytes.
fn cmin_scene(input: &Value) -> Scene {
    let (rules, ini) = cmin_rules(input);
    let mut s = scene_with(&cmin_base_input(input), rules, &ini);
    dress_cmin(&mut s, input);
    s
}

/// [`cmin_scene`]'s prestate on a built scene.
pub(super) fn dress_cmin(s: &mut Scene, input: &Value) {
    let frame = s.sim.session.binary_frame();
    // The occupy bit 0x20 the oracle writes on its reserved cells (+0x124).
    for reserved in input["reserved"].as_array().into_iter().flatten() {
        let (x, y) = cell(reserved);
        s.sim.substrate.raw_cell_occupation.mark_ground(x, y, 0x20);
    }
    if input["pad_unit"] == true {
        // A second Unit listed first in the pad cell (Get_Unit finds it).
        let tank = s
            .sim
            .spawn_object("MTNK", "Americans", 20, 4, 0, &s.rules)
            .expect("pad unit");
        place(s, tank, (9, 10));
    }
    if let Some(exact) = input.get("miner_coord") {
        let exact = coord(exact);
        let miner = s.miner;
        place(s, miner, ((exact.x >> 8) as u16, (exact.y >> 8) as u16));
        let position = &mut s.sim.substrate.entities.get_mut(miner).unwrap().position;
        position.set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(exact.x & 0xFF));
        position.set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(exact.y & 0xFF));
    }
    let nav = input.get("nav").filter(|n| !n.is_null()).map(cell);
    let terrain = s.sim.resolved_terrain.clone();
    let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
    if input["loco"] == "drive_piggy" {
        assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(entity, frame));
        if input["moving"] == true {
            let current = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let drive = entity.locomotor.as_mut().unwrap();
            assert!(drive.ensure_installed_track_state());
            assert!(drive.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                Some(current)
            ));
            assert!(drive.store_track_destination(
                crate::sim::movement::track_process::TrackFamily::Drive,
                nav.map(|(x, y)| {
                    crate::sim::movement::target_cell_coord(
                        x,
                        y,
                        terrain
                            .as_ref()
                            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                            .as_ref(),
                    )
                })
            ));
        }
    }
    entity
        .navigation
        .set_nav_com_for_test(nav.map(|(x, y)| NavTargetRef::cell(x, y)));
    entity
        .setter_force_reassign
        .set_for_test(input["force_reassign"] == true);
    entity
        .lift
        .set_swap_active_for_test(input["swap_6ad"] == true);
    if input["locked"] == true {
        entity
            .parasite
            .set_paralysis_for_test(crate::sim::timer::CdTimer::started(frame as i32, 30));
    }
    if input["warp_out"] == true {
        entity.set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
            s.other,
        ));
    }
    if input["latch_27c"] == true {
        // Techno+0x27C, which only the Chronosphere's warp writes in VERA:
        // its payload on the active Teleport.
        entity
            .drop_credit
            .set_warped_by_house_for_test(Some(entity.owner()));
        entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.teleport_runtime_mut())
            .expect("latched miner on its Teleport")
            .arm_chrono(crate::sim::movement::teleport_movement::ChronoWarp::new(
                DriveCoord { x: 0, y: 0, z: 0 },
                frame,
            ));
    }
    if input["warp_in"] == true {
        entity.install_teleport_state_for_test(Some(TeleportState::for_test(
            TeleportPhase::ChronoDelay,
            10,
            10,
            30,
        )));
    }
}

fn kind_name(kind: LocomotorKind) -> &'static str {
    match kind {
        LocomotorKind::Teleport => "teleport",
        LocomotorKind::Drive => "drive",
        other => panic!("unexpected locomotor {other:?}"),
    }
}

/// The CMIN state every row compares (`state` is the oracle's final state).
fn compare_cmin(s: &Scene, state: &Value, context: &str, mission_body_executed: bool) {
    let entity = s.sim.substrate.entities.get(s.miner).unwrap();
    let nav = match entity.navigation.nav_com() {
        Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
        None => Value::Null,
        other => panic!("{context}: non-cell NavCom {other:?}"),
    };
    assert_eq!(nav, state["nav"], "{context}: NavCom");
    assert_eq!(
        u64::from(entity.setter_force_reassign.is_raised()),
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
            .piggyback()
            .as_ref()
            .map_or(Value::Null, |stash| kind_name(stash.active_kind()).into()),
        expected["stash"].clone(),
        "{context}: stash"
    );
    if locomotor.active_kind() == LocomotorKind::Drive {
        let destination = entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .and_then(|drive| drive.destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        assert_eq!(
            destination,
            coord(&expected["destination"]),
            "{context}: Drive destination"
        );
    }
    let teleport = &state["teleport"];
    let armed = entity
        .teleport_state()
        .filter(|warp| warp.phase() == TeleportPhase::Relocate);
    assert_eq!(
        u64::from(armed.is_some()),
        teleport["moving"].as_u64().unwrap(),
        "{context}: Teleport request"
    );
    if let Some(warp) = armed {
        let destination = coord(&teleport["destination"]);
        assert_eq!(
            warp.target_cell().unwrap(),
            ((destination.x >> 8) as u16, (destination.y >> 8) as u16),
            "{context}: Teleport destination"
        );
    }
    if mission_body_executed {
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
    }
    assert_eq!(
        entity
            .radio_contacts
            .slot(0)
            .map_or(Value::Null, |id| s.name(id)),
        state["miner_contact"],
        "{context}: miner contact"
    );
    assert_eq!(
        u64::from(entity.tether.partner().is_some()),
        state["miner_tether"].as_u64().unwrap(),
        "{context}: miner tether"
    );
    let location = coord(&state["location"]);
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&entity.position),
        location,
        "{context}: location"
    );
    let desired = entity.body_facing.destination();
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

/// The occupy bit 0x20 (ground, deck) of every cell of the oracle's 32x32
/// map, y-major as its `state` lists them.
fn occupy_bits(s: &Scene) -> Vec<(bool, bool)> {
    let raw = &s.sim.substrate.raw_cell_occupation;
    (0..32u16)
        .flat_map(|y| (0..32u16).map(move |x| (x, y)))
        .map(|(x, y)| {
            (
                raw.ground_bits(x, y) & 0x20 != 0,
                raw.deck_bits(x, y) & 0x20 != 0,
            )
        })
        .collect()
}

/// Teleport Move_To's resolver (`0x00718B70`) against the oracle: Marked
/// (`+0x28`, which a Teleport a Drive suspends keeps) and the occupy bit 0x20
/// of every cell. A cell the oracle's raw receivers named or left reserved
/// takes its final bit; every other cell keeps the bit it had (the miner's
/// spawn cell holds a Rust-only one).
fn compare_resolver(s: &Scene, row: &Value, before: &[(bool, bool)], context: &str) {
    let state = &row["state"];
    let locomotor = s
        .sim
        .substrate
        .entities
        .get(s.miner)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap();
    let teleport = locomotor
        .teleport_runtime()
        .or_else(|| locomotor.piggyback().as_deref()?.teleport_runtime())
        .expect("the miner's Teleport");
    assert_eq!(
        teleport
            .resolved_destination()
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 }),
        coord(&state["teleport"]["resolved"]),
        "{context}: Marked"
    );
    let listed = |key: &str| -> Vec<(u16, u16)> {
        state[key].as_array().unwrap().iter().map(cell).collect()
    };
    let (ground, deck) = (listed("reserved"), listed("reserved_deck"));
    let touched: Vec<(u16, u16)> = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "reserve" || e[0] == "unreserve")
        .map(|e| cell(&e[1]))
        .chain(ground.iter().copied())
        .collect();
    for (index, bits) in occupy_bits(s).into_iter().enumerate() {
        let at = ((index % 32) as u16, (index / 32) as u16);
        let expected = if touched.contains(&at) {
            (ground.contains(&at), deck.contains(&at))
        } else {
            before[index]
        };
        assert_eq!(bits, expected, "{context}: occupy bit 0x20 at {at:?}");
    }
}

/// The Unit arm's Find_Nearby_Passable_Cell calls (`0x0071913C..0x00719185`)
/// against the oracle's record of their arguments: the seed, SpeedType
/// (`+0x67C`), MovementZone (`+0x5B4`), the deck byte, a 1x1 footprint, no
/// overlay, height or obstacle gate, and bridge cells allowed. The harness
/// answers GetZoneID (`0x00719137`) itself, so the zone passed must be the
/// scene's answer for the cell, MovementZone and deck byte it was asked.
fn compare_nearby(s: &Scene, events: &[Value], records: &[FreshCallRecord], context: &str) {
    let native: Vec<&Value> = events
        .iter()
        .filter(|e| e[0] == "teleport_fnpc_args")
        .collect();
    let asked: Vec<&Value> = events
        .iter()
        .filter(|e| e[0] == "movement_zone_type")
        .collect();
    let calls: Vec<&FreshCallRecord> = records
        .iter()
        .filter(|r| matches!(r, FreshCallRecord::NearbyCell { .. }))
        .collect();
    assert_eq!(calls.len(), native.len(), "{context}: FNPC calls");
    assert_eq!(asked.len(), native.len(), "{context}: GetZoneID calls");
    for ((call, native), asked) in calls.into_iter().zip(native).zip(asked) {
        let &FreshCallRecord::NearbyCell {
            seed,
            speed_type,
            zone,
            movement_zone,
            bridge_aware_zone,
            footprint,
            allow_bridge_cells,
            check_height,
            check_occupancy,
            target_cell,
            reject_any_overlay,
        } = call
        else {
            unreachable!()
        };
        let (x, y) = cell(&native[1]);
        assert_eq!(seed, (i32::from(x), i32::from(y)), "{context}: FNPC seed");
        let arg = |index: usize| native[2][index].as_i64().unwrap();
        assert_eq!(
            [
                speed_type as i64,
                i64::from(movement_zone as i8),
                i64::from(bridge_aware_zone),
                i64::from(footprint.width),
                i64::from(footprint.height),
                i64::from(reject_any_overlay),
                i64::from(check_height),
                // The obstacle gate, which VERA's search has no argument for.
                0,
                i64::from(allow_bridge_cells),
            ],
            [0, 2, 3, 4, 5, 6, 7, 8, 9].map(arg),
            "{context}: FNPC SpeedType, MovementZone, deck, footprint and gates"
        );
        // Outside the oracle's record: the zero target cell and no final
        // occupancy test (`0x00719141..0x00719151`).
        assert_eq!((target_cell, check_occupancy), (None, false), "{context}");
        let zone_cell = cell(&asked[1]);
        assert_eq!(
            (i32::from(zone_cell.0), i32::from(zone_cell.1)),
            seed,
            "{context}: GetZoneID cell"
        );
        assert_eq!(
            [asked[2].as_i64().unwrap(), asked[3].as_i64().unwrap()],
            [i64::from(movement_zone as i8), i64::from(bridge_aware_zone)],
            "{context}: GetZoneID MovementZone and deck"
        );
        let scene_zone = s.sim.zone_grid.as_ref().unwrap().get_zone_id_native(
            s.sim.resolved_terrain.as_ref().unwrap(),
            zone_cell,
            movement_zone,
            bridge_aware_zone,
        );
        assert_eq!(zone, scene_zone, "{context}: GetZoneID answer passed on");
    }
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
        let events = row["events"].as_array().unwrap();
        // A supplied Find_Nearby_Passable_Cell answer goes to the Rust search
        // itself, so the scene keeps the fixture's land rows.
        let mut scene_input = input.clone();
        let nearby = scene_input.as_object_mut().unwrap().remove("passable");
        let mut s = cmin_scene(&scene_input);
        let before = active(&s);
        let bits_before = occupy_bits(&s);
        if let Some(answers) = &nearby {
            fresh_oracle_seam::install_nearby_only(
                answers
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|answer| (!answer.is_null()).then(|| cell(answer)))
                    .collect(),
            );
        }
        match input.get("dest").filter(|d| !d.is_null()) {
            Some(dest) => {
                s.sim.set_unit_destination(
                    s.miner,
                    {
                        let (rx, ry) = cell(dest);
                        crate::sim::components::NavTargetRef::cell(rx, ry)
                    },
                    &s.rules,
                    true,
                    crate::sim::world::FrameEffects::default(),
                );
            }
            None => {
                s.sim.set_unit_null_destination(
                    s.miner,
                    &s.rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
            }
        }
        let (records, unused) = if nearby.is_some() {
            fresh_oracle_seam::finish()
        } else {
            (Vec::new(), 0)
        };
        compare_swap(before, &s, events, &context);
        compare_cmin(&s, &row["state"], &context, true);
        if events
            .iter()
            .any(|e| e[0] == "move_to" && e[1] == "teleport")
        {
            compare_resolver(&s, row, &bits_before, &context);
        }
        compare_nearby(&s, events, &records, &context);
        assert_eq!(unused, 0, "{context}: every supplied FNPC answer used");
    }
}

/// A retarget while the Unit holds Marked (`+0x28`), as every Chrono Miner
/// trip after its first does: Marked survives Stop and the Drive stash. No
/// corpus row starts with a Marked, so this follows the instructions.
/// ResolveDestination REMOVEs at the old Marked and, for a cell it can take,
/// again at `0x007191B5` before PUTting the new one. When the search finds
/// none, Marked stays (`0x00719190..0x007191A9`): the tail PUTs it and
/// Move_To arms it (`0x00719249..0x0071927F`), so the miner warps to its
/// previous destination.
#[test]
fn a_teleport_unit_retarget_releases_or_rearms_its_prior_marked() {
    let marked = |s: &Scene| {
        let entity = s.sim.substrate.entities.get(s.miner).unwrap();
        let teleport = entity.locomotor.as_ref().unwrap().teleport_runtime();
        teleport.unwrap().resolved_destination()
    };
    let armed = |s: &Scene| {
        let entity = s.sim.substrate.entities.get(s.miner).unwrap();
        let warp = entity.teleport_state();
        warp.filter(|warp| warp.phase() == TeleportPhase::Relocate)
            .and_then(|warp| warp.target_cell())
    };
    let reserved = |s: &Scene, (x, y): (u16, u16)| {
        s.sim.substrate.raw_cell_occupation.ground_bits(x, y) & 0x20 != 0
    };
    let order = |s: &mut Scene, (x, y): (u16, u16)| {
        s.sim.set_unit_destination(
            s.miner,
            NavTargetRef::cell(x, y),
            &s.rules,
            true,
            crate::sim::world::FrameEffects::default(),
        );
    };
    let (pad, other) = ((9, 10), (14, 12));

    let mut s = cmin_scene(&serde_json::json!({ "name": "prior_marked" }));
    order(&mut s, pad);
    assert_eq!(marked(&s), Some(DriveCoord::cell(9, 10, 0)));
    order(&mut s, other);
    assert_eq!(marked(&s), Some(DriveCoord::cell(14, 12, 0)));
    assert!(!reserved(&s, pad), "the old Marked is released");
    assert!(reserved(&s, other));
    assert_eq!(armed(&s), Some(other));

    // Another unit holds the pad and the search finds no cell.
    let mut s = cmin_scene(&serde_json::json!({ "name": "prior_marked_no_cell" }));
    order(&mut s, other);
    s.sim
        .substrate
        .raw_cell_occupation
        .mark_ground(pad.0, pad.1, 0x20);
    fresh_oracle_seam::install_nearby_only(vec![None]);
    order(&mut s, pad);
    let (_, unused) = fresh_oracle_seam::finish();
    assert_eq!(unused, 0, "the refused pad searched");
    assert_eq!(marked(&s), Some(DriveCoord::cell(14, 12, 0)));
    assert!(reserved(&s, other), "the kept Marked is PUT again");
    assert_eq!(armed(&s), Some(other), "the miner warps to its old Marked");
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
        assert!(
            !s.sim
                .teleport_move_to(
                    s.miner,
                    crate::sim::movement::target_cell_coord(
                        cell(&input["dest"]).0,
                        cell(&input["dest"]).1,
                        None
                    ),
                    &s.rules,
                    true,
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap(),
            "{context}: refused"
        );
        // The warp-in row's prestate is a running delay, not a request.
        if input["warp_in"] == true {
            s.sim
                .substrate
                .entities
                .get_mut(s.miner)
                .unwrap()
                .install_teleport_state_for_test(None);
        }
        compare_cmin(&s, &row["state"], &context, true);
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
        let frame = s.sim.session.binary_frame();
        let dest = cell(&input["dest"]);
        assert!(
            s.sim
                .teleport_move_to(
                    s.miner,
                    crate::sim::movement::target_cell_coord(dest.0, dest.1, None),
                    &s.rules,
                    harvester,
                    None,
                    crate::sim::world::FrameEffects::default()
                )
                .unwrap()
        );
        {
            let entity = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            // Armed by a direct Move_To, as the oracle does; the mission waits.
            entity
                .mission
                .write_dispatch_epilogue(frame as i32, 100_000);
        }
        let origin = {
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            (entity.position.rx(), entity.position.ry())
        };
        let warp_out = s.sim.interner.intern("WARPOUT");
        let ticks = row["ticks"].as_array().unwrap();
        let last = ticks.last().unwrap()["frame"].as_u64().unwrap() as u32;
        let mut checked = ticks.iter().peekable();
        for now in frame..=last {
            s.sim.session.set_binary_frame_for_test(now);
            let sounds_before = s.sim.sound_events.len();
            s.sim
                .advance_live_object_turn(s.miner, &s.rules, ObjectAiCtx::default())
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
                    .filter(|anim| anim.type_id() == warp_out)
                    .map(|anim| {
                        let (rx, ry, _, _, _) = anim.world_coord().to_cell_sub_z();
                        (rx, ry)
                    })
                    .collect();
                anims.sort_unstable();
                let mut expected_anims = expected_anims;
                expected_anims.sort_unstable();
                assert_eq!(anims, expected_anims, "{context}: WarpOut animations");
                if expected_anims.is_empty() {
                    let entity = s.sim.substrate.entities.get(s.miner).unwrap();
                    assert_eq!((entity.position.rx(), entity.position.ry()), origin);
                }
            }
            let state = &tick["state"];
            let mission_body_executed = !events.iter().any(|event| {
                event[0] == "enter_idle_mode"
                    && event[1].as_u64() == Some(0)
                    && event[2].as_u64() == Some(1)
            });
            compare_cmin(&s, state, &context, mission_body_executed);
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            if !mission_body_executed {
                // Rust integration, not a native golden: the legacy native
                // callback is a recorded no-effect seam, while production
                // executes the existing Unit idle owner after TimerCheck.
                assert_eq!(
                    entity.mission.current(),
                    crate::sim::mission::MissionId::from_known(
                        crate::sim::mission::MissionType::Guard,
                    ),
                    "{context}: Rust idle enters Guard"
                );
            }
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
        miner.set_cargo_for_test(vec![
            CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            };
            usize::from(miner.capacity_bales())
        ]);
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
            s.sim
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
                    s.miner,
                ));
        }
        let status = input["status"].as_u64().unwrap();
        fill(&mut s, status);
        radio::take_transmit_log();
        let before = active(&s);
        let mut stream = s.sim.scenario_rng.clone();
        let frame = s.sim.session.binary_frame();
        crate::sim::world::dispatch_foot_mission(
            &mut s.sim,
            s.miner,
            &s.rules,
            crate::sim::world::ObjectAiCtx {
                overlay_registry: Some(crate::sim::tiberium::test_support::overlay_registry()),
                ..Default::default()
            },
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
        assert_eq!(
            super::refinery_dock_oracle_tests::legacy_harvest_sends(&s),
            oracle_sends(row),
            "{context}: legacy transmit suffix"
        );
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_swap(before, &s, row["events"].as_array().unwrap(), &context);
        compare_cmin(&s, &row["state"], &context, true);
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
            entity.navigation.set_nav_queue_for_test(
                queue
                    .iter()
                    .map(|c| {
                        let (x, y) = cell(c);
                        NavTargetRef::cell(x, y)
                    })
                    .collect(),
            );
        }
        radio::take_transmit_log();
        let before = active(&s);
        let mut stream = s.sim.scenario_rng.clone();
        let delay = crate::sim::mission::enter::mission_enter(
            &mut s.sim,
            &s.rules,
            None,
            s.miner,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_swap(before, &s, row["events"].as_array().unwrap(), &context);
        compare_cmin(&s, &row["state"], &context, true);
        if let Some(queue) = input["nav_queue"].as_array() {
            let entity = s.sim.substrate.entities.get(s.miner).unwrap();
            assert_eq!(
                entity.navigation.nav_queue().len(),
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
        let delay = crate::sim::miner::mission_unload(
            &mut s.sim,
            &s.rules,
            s.miner,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_delay(&s, row, delay, &mut stream, &context);
        compare_cmin(&s, &row["state"], &context, true);
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
        s.sim.unit_dock_now(
            s.miner,
            &s.rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sends(&s), oracle_sends(row), "{context}: transmit sequence");
        compare_cmin(&s, &row["state"], &context, true);
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

#[path = "cmin_instance_tests.rs"]
mod instance_tests;
