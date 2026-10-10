//! The paradrops (`paradrop`, `aircraft::paradrop_mission`,
//! `aircraft::drop_payload`): their native comparisons against
//! `tools/superweapon_oracle.json` (`paradrop_launch`, `send_paradrop_planes`,
//! `paradrop_missions`, `drop_payload`, `spawn_parachuted`; `--check`
//! regenerates them), and drops on retail rules through production frames.

use super::{ParaDropKind, board_passengers, launch, retarget, send_planes};
use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::drop_payload::{OBSERVED, Observed, drop_payload};
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord};
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, retail_rules_binding, step,
};
use crate::sim::superweapon::spy_plane_tests::{
    edge, events, expected_pick, flag, int, oracle, rows, world,
};
use crate::sim::timer::CdTimer;
use crate::sim::world::edge_cell::Edge;
use crate::sim::world::{PlacementEvidence, RevealOutcome, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton;
use serde_json::{Value, json};

const PARADROP: &str = "ParaDropSpecial";
const AMER_PARADROP: &str = "AmericanParaDropSpecial";
/// The oracle's target cell (`PD_TARGET_CELL`).
const TARGET: (u16, u16) = (40, 40);
/// The fixture theater's WaterSet base (`PD_WATER_SET`).
const WATER_SET: i32 = 50;
/// The oracle's fixture InfantryTypes by index.
const INFANTRY: [&str; 3] = ["E1", "E2", "INIT"];
/// What the oracle's Launch finds reading a count list past its end
/// (`PD_PAST_THE_VECTOR`).
const PAST_THE_VECTOR: i32 = 0x5EED;
/// The plane's Location in the drop rows (`PD_LOCATION`): cell (40, 40)'s
/// centre at FlightLevel 1500.
const LOCATION: [i32; 3] = [40 * 256 + 128, 40 * 256 + 128, 1500];
const FLY: &str = "{4A582746-9839-11D1-B709-00A024DDAFD1}";

/// Retail's single list entries (`PD_RETAIL_LISTS`).
fn retail_lists() -> Value {
    json!({"amer": [[0], [8]], "ally": [[0], [6]], "sov": [[1], [9]], "yuri": [[2], [6]]})
}

/// The `[General]` keys of an oracle row's `lists`: each side's types by
/// fixture index and its counts. An empty list is left unauthored.
fn list_keys(lists: &Value) -> String {
    let mut keys = String::new();
    for (side, prefix) in [
        ("amer", "Amer"),
        ("ally", "Ally"),
        ("sov", "Sov"),
        ("yuri", "Yuri"),
    ] {
        let entries = |at: usize| lists[side][at].as_array().unwrap().iter().map(int);
        let types: Vec<&str> = entries(0)
            .map(|index| INFANTRY[usize::try_from(index).unwrap()])
            .collect();
        let nums: Vec<String> = entries(1).map(|num| num.to_string()).collect();
        if !types.is_empty() {
            keys += &format!("{prefix}ParaDropInf={}\n", types.join(","));
        }
        if !nums.is_empty() {
            keys += &format!("{prefix}ParaDropNum={}\n", nums.join(","));
        }
    }
    keys
}

/// Whether a list names an InfantryType whose ArrayIndex (`+0xDF8`) is -1,
/// which Launch skips (e.g. `0x006CD405`). No type has one: its constructor
/// records its slot in the type array (`0x0052387E`), -1 only when that
/// array cannot grow (`0x00523816..0x0052385C`).
fn names_unallocated_type(lists: &Value) -> bool {
    ["amer", "ally", "sov", "yuri"].iter().any(|side| {
        lists[side][0]
            .as_array()
            .unwrap()
            .iter()
            .any(|index| int(index) < 0)
    })
}

/// A replay's rules: PDPLANE as retail authors it (unless `plane` is false),
/// E1, E2 and INIT, both paradrop Supers, the `[General]` list keys and
/// `ParadropRadius=radius`.
fn rules(lists: &str, plane: bool, radius: i32) -> RuleSet {
    let aircraft = if plane { "0=PDPLANE\n" } else { "" };
    let ini = IniFile::from_str(&format!(
        "[General]\n{lists}FlightLevel=1500\nParadropRadius={radius}\n\
         [InfantryTypes]\n0=E1\n1=E2\n2=INIT\n[VehicleTypes]\n[AircraftTypes]\n{aircraft}\
         [BuildingTypes]\n[SuperWeaponTypes]\n0={PARADROP}\n1={AMER_PARADROP}\n\
         [{PARADROP}]\nType=ParaDrop\n[{AMER_PARADROP}]\nType=AmerParaDrop\n\
         [E1]\nStrength=125\n[E2]\nStrength=125\n[INIT]\nStrength=125\n\
         [PDPLANE]\nStrength=400\nSpeed=15\nROT=2\nAmmo=100\nLandable=no\nSelectable=no\n\
         Spawned=yes\nSight=0\nLocomotor={FLY}\n",
    ));
    RuleSet::from_ini(&ini).unwrap()
}

fn retail_list_rules(radius: i32) -> RuleSet {
    rules(&list_keys(&retail_lists()), true, radius)
}

/// `rules` on [`world`]'s map with its theater's WaterSet at [`WATER_SET`],
/// so the test grid's tile 0 is land.
fn paradrop_world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    let (rules, mut sim, americans) = world(rules);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .set_projectile_water_set_base(WATER_SET);
    (rules, sim, americans)
}

/// Gives `cell` the WaterSet base's tile plus `offset`.
fn set_tile(sim: &mut Simulation, cell: (u16, u16), offset: i32) {
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(cell.0, cell.1)
        .unwrap()
        .final_tile_index = WATER_SET + offset;
}

/// The paradrop planes, oldest first.
fn planes(sim: &Simulation) -> Vec<u64> {
    let mut planes: Vec<u64> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "PDPLANE" && !entity.dying)
        .map(|entity| entity.stable_id())
        .collect();
    planes.sort_unstable();
    planes
}

/// The plane's passengers, head first.
fn cargo(sim: &Simulation, plane: u64) -> Vec<u64> {
    sim.substrate
        .entities
        .get(plane)
        .unwrap()
        .passenger_role
        .cargo()
        .map_or_else(Vec::new, |cargo| cargo.passengers.clone())
}

/// A PDPLANE as SendParadropPlanes builds one (mission-only before its
/// Unlimbo) over `at`'s centre, 1500 leptons up, with `passengers` E1
/// aboard.
fn plane_at(sim: &mut Simulation, rules: &RuleSet, at: (u16, u16), passengers: u32) -> u64 {
    let id = sim
        .with_object_placement_scope(|sim| {
            sim.construct_object_limbo_at_height("PDPLANE", "Americans", 0, 0, 0, 0, rules)
        })
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mark_mission_only();
    sim.with_object_placement_scope(|sim| {
        sim.reveal_constructed_object_at_height(
            id,
            at.0,
            at.1,
            0,
            0,
            PlacementEvidence::MarkSucceeded,
            rules,
            crate::sim::world::FrameEffects::default(),
        )
    })
    .unwrap();
    board_passengers(sim, rules, id, "Americans", "E1", passengers);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .position
        .exact_z_leptons = Some(1500);
    id
}

/// Fills `cell`'s three functional infantry spots, so PlaceInfantryInCell
/// finds none free.
fn fill_spots(sim: &mut Simulation, cell: (u16, u16)) {
    for spot in [2, 3, 4] {
        let id = sim.allocate_stable_id();
        let mut blocker = GameEntity::test_default(id, "E1", "Americans", cell.0, cell.1);
        blocker.owner = sim.interner.intern("Americans");
        blocker.type_ref = sim.interner.intern("E1");
        blocker.category = EntityCategory::Infantry;
        blocker.is_voxel = false;
        blocker.sub_cell = Some(spot);
        (blocker.position.sub_x, blocker.position.sub_y) =
            lepton::subcell_lepton_offset(Some(spot));
        sim.substrate.entities.insert(blocker);
        assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    }
}

/// A playfield holding none of the drop rows' cells: only cells whose
/// `x - y` is below -12 (`MapClass::IsCoordInPlayfield @ 0x005785F0`).
const NO_DROP_PLAYFIELD: PlayfieldBounds =
    PlayfieldBounds::from_normalized_local_size(32, 2, 4, 8, 8);

fn coord(value: &Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

fn chute_sounds(sim: &Simulation) -> Vec<(u16, u16)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::ChuteSound { rx, ry } => Some((*rx, *ry)),
            _ => None,
        })
        .collect()
}

/// Runs `drop` with the passenger's Can_Enter_Cell answering `can_enter`,
/// returning what Drop_Payload called and the Can_Enter_Cell calls.
fn observe(can_enter: u8, drop: impl FnOnce()) -> (Vec<Observed>, Vec<FreshCallRecord>) {
    fresh_oracle_seam::install(vec![can_enter], Vec::new());
    OBSERVED.with(|log| *log.borrow_mut() = Some(Vec::new()));
    drop();
    let observed = OBSERVED.with(|log| log.borrow_mut().take()).unwrap();
    let (records, _) = fresh_oracle_seam::finish();
    (observed, records)
}

/// Launch cases 5 and 6: a charged Super with a known PDPLANE and a real
/// cell sends one plane per entry of its lists (case 6 the American ones,
/// case 5 those of the house's side: 0 Allied, 2 Yuri, any other Soviet),
/// each with mission 26, the cell and the entry's infantry and count. The
/// Allied, Yuri and American lists send nothing unless both have as many
/// entries; the Soviet one walks its infantry and reads past its counts.
/// The player's tail clears the selected Super and drops a queued
/// `EVA_ReinforcementsReady` after every charged launch; VERA reports each
/// launch and the app does both (`super_selection`, `sound_dispatch`).
#[test]
fn launch_cases_five_and_six_match_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "paradrop_launch");
    assert!(rows.len() > 40);
    let mut replayed = 0;
    for row in rows {
        let sends: Vec<&Value> = events(row, "send").collect();
        let charged = flag(row, "charged");
        if charged {
            assert_eq!(
                events(row, "find_aircraft_type").next(),
                Some(&json!(["find_aircraft_type", "PDPLANE"])),
                "{row}"
            );
            assert_eq!(
                events(row, "map_cell").next(),
                Some(&json!(["map_cell", [40, 40]])),
                "{row}"
            );
            let player = flag(row, "player");
            let eva: Vec<&Value> = events(row, "vox_find").collect();
            let expected = json!(["vox_find", "EVA_ReinforcementsReady"]);
            assert_eq!(eva, vec![&expected; usize::from(player)], "{row}");
            let selected = if player { -1 } else { 9 };
            assert_eq!(int(&row["selected_super"]), selected, "{row}");
        } else {
            // ClickFire admits only a charged Super; the cases check again.
            assert_eq!(row["events"], json!([]), "{row}");
        }
        for send in &sends {
            // One PDPLANE (index 3), mission 26, no destination.
            assert_eq!(
                send.as_array().unwrap()[1..4],
                [json!(3), json!(1), json!(26)],
                "{row}"
            );
            assert_eq!(send[5], 0, "{row}");
        }
        if names_unallocated_type(&row["lists"]) {
            continue;
        }
        let target = &row["cells"][0];
        assert_eq!(target[0], json!([40, 40]));
        // MapClass::operator[] answers the shared dummy off the map; VERA's
        // lookup never answers NULL.
        let (cell, tile) = match &target[1] {
            Value::String(_) => ((200, 200), None),
            tile => (TARGET, Some(int(tile))),
        };
        if tile.is_some_and(|tile| (0..14).contains(&tile)) {
            // A WaterSet target: `a_water_target_moves_as_native_does`.
            assert!(events(row, "nearby").next().is_some(), "{row}");
            continue;
        }
        assert!(events(row, "nearby").next().is_none(), "{row}");
        let (kind, name) = match int(&row["kind"]) {
            5 => (ParaDropKind::Generic, PARADROP),
            6 => (ParaDropKind::American, AMER_PARADROP),
            other => panic!("Type={other}"),
        };
        let plane_known = int(&row["plane_index"]) != -1;
        let (rules, mut sim, americans) =
            paradrop_world(rules(&list_keys(&row["lists"]), plane_known, 1024));
        if let Some(tile) = tile {
            set_tile(&mut sim, TARGET, tile);
        }
        sim.houses.get_mut(&americans).unwrap().side_index = int(&row["side"]) as u8;
        let sw_type = if charged {
            charge_super(&mut sim, americans, name)
        } else {
            sim.interner.intern(name)
        };

        assert_eq!(
            launch(
                &mut sim,
                &rules,
                americans,
                cell.0,
                cell.1,
                kind,
                sw_type,
                crate::sim::world::FrameEffects::default()
            ),
            charged,
            "{row}"
        );

        let reports = sim
            .sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
            .count();
        assert_eq!(reports, usize::from(charged), "{row}");
        let planes = planes(&sim);
        assert_eq!(planes.len(), sends.len(), "{row}");
        for (&plane, send) in planes.iter().zip(&sends) {
            let entity = sim.substrate.entities.get(plane).unwrap();
            let sent_to = TargetKind::Cell(int(&send[4][0]) as u16, int(&send[4][1]) as u16);
            assert_eq!(
                entity.attack_target.as_ref().map(|attack| attack.target),
                Some(sent_to),
                "{row}"
            );
            // The counts RESIDUAL in `paradrop`'s module doc: a count read
            // past the list or a negative one sends the plane empty.
            let num = int(&send[7]);
            let expected = if num == PAST_THE_VECTOR {
                0
            } else {
                num.max(0)
            };
            let aboard = cargo(&sim, plane);
            assert_eq!(aboard.len(), expected as usize, "{row}");
            let infantry = INFANTRY[usize::try_from(int(&send[6])).unwrap()];
            for &passenger in &aboard {
                let passenger = sim.substrate.entities.get(passenger).unwrap();
                assert_eq!(sim.interner.resolve(passenger.type_ref()), infantry);
                assert_eq!(passenger.owner(), americans);
            }
        }
        replayed += 1;
    }
    assert!(replayed > 25, "{replayed}");
}

/// A WaterSet target (`0x00485060`) moves to Find_Nearby_Passable_Cell's
/// answer (Foot, no zone, Normal, no alt, 1x1, no overlay, height or
/// burrowing test, bridges allowed, no target cell), unless that is the
/// empty cell, the dummy or WaterSet again ([`retarget`]). The oracle stubs
/// the search, so its answers replay into `retarget` on a map whose tiles
/// the row names; the search is the shared port (`find_nearby_cell`).
#[test]
fn a_water_target_moves_as_native_does() {
    let oracle = oracle();
    let mut replayed = 0;
    for row in rows(&oracle, "paradrop_launch") {
        let Some(nearby) = events(row, "nearby").next() else {
            continue;
        };
        assert_eq!(
            nearby,
            &json!([
                "nearby",
                [40, 40],
                [0, -1, 0, 0, 1, 1, 0, 0, 0, 1],
                [0, 0],
                0,
                0
            ]),
            "{row}"
        );
        let [send] = events(row, "send").collect::<Vec<_>>()[..] else {
            panic!("one plane: {row}");
        };
        let found = (int(&row["nearby"][0]) as u16, int(&row["nearby"][1]) as u16);
        let (_, mut sim, _) = paradrop_world(retail_list_rules(1024));
        let mut answer = found;
        for cell in row["cells"].as_array().unwrap() {
            let at = (int(&cell[0][0]) as u16, int(&cell[0][1]) as u16);
            match &cell[1] {
                // A NULL or dummy answer: VERA's is the dummy, off the map.
                Value::String(_) => {
                    assert_eq!(at, found);
                    answer = (200, 200);
                }
                tile => set_tile(&mut sim, at, int(tile)),
            }
        }
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cells = NativeCellQuery::canonical(terrain);
        assert!(terrain.native_cell_is_water_set_tile(cells.lookup((40, 40))));

        let moved = retarget(&cells, TARGET, Some(answer));

        assert_eq!(json!([moved.0, moved.1]), send[4], "{row}");
        replayed += 1;
    }
    assert!(replayed > 10, "{replayed}");
}

/// The production path of a WaterSet click (Rust integration, not a native
/// comparison: the search order is the shared Find_Nearby_Passable_Cell
/// port's own evidence). (40, 40) is water no Foot can enter, among land, so
/// Launch case 5 sends the plane to a land cell beside it.
#[test]
fn a_water_click_sends_the_plane_to_land_beside_it() {
    let (rules, mut sim, americans) = paradrop_world(retail_list_rules(1024));
    set_tile(&mut sim, TARGET, 3);
    let cell = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(TARGET.0, TARGET.1)
        .unwrap();
    // Water: Foot's speed there is 0.
    cell.land_type = 2;
    cell.speed_costs.foot = Some(0);
    cell.base_speed_costs.foot = Some(0);
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let sw_type = charge_super(&mut sim, americans, PARADROP);

    assert!(launch(
        &mut sim,
        &rules,
        americans,
        TARGET.0,
        TARGET.1,
        ParaDropKind::Generic,
        sw_type,
        crate::sim::world::FrameEffects::default()
    ));

    let [plane] = planes(&sim)[..] else {
        panic!("one plane");
    };
    let target = sim
        .substrate
        .entities
        .get(plane)
        .unwrap()
        .attack_target
        .as_ref()
        .map(|attack| attack.target);
    let Some(TargetKind::Cell(x, y)) = target else {
        panic!("a cell target: {target:?}");
    };
    assert_ne!((x, y), TARGET);
    assert!(
        x.abs_diff(TARGET.0) <= 1 && y.abs_diff(TARGET.1) <= 1,
        "{x},{y}"
    );
}

/// SendParadropPlanes with case 5's arguments: the plane is constructed in
/// the ScenarioInit bracket and is mission-only at its edge pick (the
/// house's own edge, `GetEdge`); it queues mission 26, targets the cell,
/// Unlimbos at the picked cell's centre facing North in the bracket again,
/// takes its passengers (each at the cargo's head, so the last built drops
/// first) and starts its mission.
#[test]
fn send_paradrop_planes_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "send_paradrop_planes");
    let mut replayed = 0;
    let mut authored_replayed = 0;
    for row in rows {
        let calls: Vec<&Value> = row["events"].as_array().unwrap().iter().collect();
        assert_eq!(calls[0][0], "create");
        assert_eq!(calls[0][1], 1, "{row}");
        if calls[0][2] == false {
            // No plane, so nothing else runs. CreateObject answers NULL only
            // past the object limits, which VERA does not model.
            assert_eq!(calls.len(), 1, "{row}");
            continue;
        }
        let pick = calls[1];
        assert_eq!(pick[0], "pick_cell_on_edge");
        assert_eq!(
            pick.as_array().unwrap()[2..],
            [
                json!("0xb04c38"),
                json!("0xb04c38"),
                json!(4),
                json!(1),
                json!(0),
                json!(1)
            ],
            "{row}"
        );
        assert_eq!(calls[2], &json!(["queue", 26, 0]), "{row}");
        assert_eq!(calls[3], &json!(["target", "target"]), "{row}");
        let centre = |axis: usize| int(&row["pick"][axis]) * 256 + 128;
        assert_eq!(
            calls[4],
            &json!(["unlimbo", [centre(0), centre(1), 0], 0, 1]),
            "{row}"
        );
        let built = events(row, "create_infantry").count();
        if int(&row["returned"]) != 1 {
            // A refused Unlimbo deletes the plane, as `send_planes`
            // discards it; aircraft Unlimbo refuses nothing VERA can stage.
            assert_eq!(calls[5], &json!(["delete", 1]), "{row}");
            continue;
        }
        if int(&row["what_am_i"]) != 2 || int(&row["inf_index"]) == -1 {
            // PDPLANE is an AircraftType, and no InfantryType's ArrayIndex
            // is -1 (`names_unallocated_type`): the passengers always board.
            assert_eq!(built, 0, "{row}");
            continue;
        }
        if events(row, "create_infantry").any(|call| call[2] == false) {
            // An infantry's CreateObject answers NULL only past the limits.
            continue;
        }
        let house_edge = int(&row["edge"]);
        let (rules, mut sim, americans) = paradrop_world(retail_list_rules(1024));
        if (0..=3).contains(&house_edge) {
            super::super::spy_plane_tests::install_campaign_edge(&mut sim, americans, house_edge);
            authored_replayed += 1;
        }
        let waypoint_edge = u8::try_from(int(&row["waypoint_edge"])).unwrap_or(u8::MAX);
        sim.houses.get_mut(&americans).unwrap().waypoint_edge = waypoint_edge;
        let own = edge(pick);
        if !(0..=3).contains(&house_edge) {
            assert_eq!(own, Edge::own_edge(waypoint_edge), "{row}");
        }
        // The plane's constructor draws one word before the pick.
        let expected = expected_pick(&sim, own, 1);
        // A NULL InfantryType slot builds nothing; so does a name VERA has
        // no type for.
        let infantry = if flag(row, "inf_type") {
            "E1"
        } else {
            "NOSUCH"
        };
        let num = u32::try_from(int(&row["num"])).unwrap();

        assert!(send_planes(
            &mut sim,
            &rules,
            americans,
            "PDPLANE",
            MissionType::ParadropApproach,
            TARGET,
            Some((infantry, num)),
            crate::sim::world::FrameEffects::default(),
        ));

        let [plane] = planes(&sim)[..] else {
            panic!("one plane: {row}");
        };
        let entity = sim.substrate.entities.get(plane).unwrap();
        assert!(entity.is_mission_only());
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::ParadropApproach)
        );
        assert_eq!(entity.mission.queued(), MissionId::NONE);
        assert_eq!(
            entity.attack_target.as_ref().map(|attack| attack.target),
            Some(TargetKind::Cell(TARGET.0, TARGET.1))
        );
        assert_eq!(entity.navigation.nav_com, None);
        assert_eq!((entity.position.rx, entity.position.ry), expected, "{row}");
        assert_eq!(
            (entity.position.sub_x, entity.position.sub_y),
            (SimFixed::from_num(128), SimFixed::from_num(128))
        );
        assert_eq!(entity.body_facing_current(sim.session.binary_frame), 0);
        let mut infantry: Vec<u64> = sim
            .substrate
            .entities
            .values()
            .filter(|entity| entity.category == EntityCategory::Infantry)
            .map(|entity| entity.stable_id())
            .collect();
        infantry.sort_unstable();
        infantry.reverse();
        assert_eq!(cargo(&sim, plane), infantry, "{row}");
        assert_eq!(infantry.len(), int(&row["cargo"][0]) as usize, "{row}");
        for &passenger in &infantry {
            let passenger = sim.substrate.entities.get(passenger).unwrap();
            assert!(passenger.lifecycle.in_limbo);
            assert_eq!(passenger.passenger_role.inside_transport_id(), Some(plane));
        }
        replayed += 1;
    }
    assert!(replayed > 8, "{replayed}");
    assert_eq!(authored_replayed, 1);
}

/// The mission rows' Target, inside [`world`]'s playfield (cells whose
/// coordinates sum to 41..=86), and one outside it.
const PLANE_TARGET: (u16, u16) = (36, 40);
const OUTSIDE_TARGET: (u16, u16) = (58, 34);
/// The NavCom a mission row with one starts with.
const NAV_CELL: (u16, u16) = (20, 40);

/// The mission row's plane: PDPLANE east of `target`'s centre where
/// `ObjectClass::Distance_To` (`combat::object_distance_to`) answers the
/// row's distance, carrying one E1 when the row has passengers, with its
/// passes (`+0x6D3`) and latch (`+0x6D2`), in its mission with the timer
/// due.
fn mission_plane(sim: &mut Simulation, rules: &RuleSet, row: &Value, target: (u16, u16)) -> u64 {
    let id = plane_at(sim, rules, target, u32::from(flag(row, "passengers")));
    if flag(row, "nav_com") {
        let nav = NavTargetRef::cell(NAV_CELL.0, NAV_CELL.1);
        sim.assign_aircraft_destination(
            id,
            Some(nav),
            rules,
            crate::sim::world::FrameEffects::default(),
        );
    }
    let distance = int(&row["distance"]);
    let goal = TargetKind::Cell(target.0, target.1);
    let centre = |cell: u16| i32::from(cell) * 256 + 128;
    let mut placed = false;
    'search: for dy in 0..64 {
        for dx in (distance - 4).max(0)..distance + 8 {
            let (x, y) = (centre(target.0) + dx, centre(target.1) + dy);
            let plane = sim.substrate.entities.get_mut(id).unwrap();
            plane.position.rx = (x / 256) as u16;
            plane.position.ry = (y / 256) as u16;
            plane.position.sub_x = SimFixed::from_num(x % 256);
            plane.position.sub_y = SimFixed::from_num(y % 256);
            let entities = &sim.substrate.entities;
            let plane = entities.get(id).unwrap();
            if crate::sim::combat::object_distance_to(plane, &goal, entities) == Some(distance) {
                placed = true;
                break 'search;
            }
        }
    }
    assert!(placed, "no spot {distance} leptons out");
    let mission = if row["mission"] == "approach" {
        MissionType::ParadropApproach
    } else {
        MissionType::ParadropOverfly
    };
    let plane = sim.substrate.entities.get_mut(id).unwrap();
    plane.attack_target = flag(row, "target").then(|| AttackTarget::for_cell(target.0, target.1));
    if !flag(row, "nav_com") {
        plane.navigation.nav_com = None;
    }
    let passes = i8::try_from(int(&row["passes"])).unwrap();
    plane.mission_leaf.set_paradrop_passes_for_test(passes);
    plane
        .mission_leaf
        .set_aircraft_action_latch(int(&row["latch"]) != 0);
    plane.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(mission),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    id
}

/// One dispatch of Mission_ParadropApproach or Mission_ParadropOverfly per
/// row: the queued mission, the passes, the latch, the destination, the
/// Target, the drop and the frames the mission timer takes. The oracle
/// stubs Drop_Payload; here the passenger's Can_Enter_Cell refuses it, so
/// the drop leaves the plane as it was.
#[test]
fn paradrop_missions_match_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "paradrop_missions");
    let mut replayed = 0;
    for row in rows {
        if int(&row["distance"]) < 0 {
            // Distance_To answers a length (`0x005F6440`); the stub's -1
            // tests the signed compare.
            continue;
        }
        let target = if flag(row, "in_playfield") {
            PLANE_TARGET
        } else {
            OUTSIDE_TARGET
        };
        let (rules, mut sim, _) = paradrop_world(retail_list_rules(int(&row["radius"])));
        let id = mission_plane(&mut sim, &rules, row, target);
        let now = sim.session.binary_frame;

        let (observed, records) = observe(7, || {
            crate::sim::aircraft::dispatch_mission(&mut sim, id, &rules, Default::default());
        });

        let dropped = events(row, "drop_payload").next().is_some();
        assert_eq!(!observed.is_empty(), dropped, "{row}");
        assert_eq!(records.len(), usize::from(dropped), "{row}");
        let plane = sim.substrate.entities.get(id).unwrap();
        let queued = events(row, "queue")
            .last()
            .map_or(MissionId::NONE, |call| MissionId::from_raw(int(&call[1])));
        assert_eq!(plane.mission.queued(), queued, "{row}");
        let leaf = plane.mission_leaf.as_aircraft().unwrap();
        assert_eq!(
            i32::from(leaf.paradrop_passes()),
            int(&row["passes_after"]),
            "{row}"
        );
        assert_eq!(
            i32::from(leaf.action_latch()),
            int(&row["latch_after"]),
            "{row}"
        );
        let nav = match events(row, "destination").last() {
            None => flag(row, "nav_com").then(|| NavTargetRef::cell(NAV_CELL.0, NAV_CELL.1)),
            Some(call) if call[1] == "null" => None,
            Some(_) => Some(NavTargetRef::cell(target.0, target.1)),
        };
        assert_eq!(plane.navigation.nav_com, nav, "{row}");
        let targeted = flag(row, "target") && events(row, "target").all(|call| call[1] != "null");
        assert_eq!(
            plane.attack_target.as_ref().map(|attack| attack.target),
            targeted.then_some(TargetKind::Cell(target.0, target.1)),
            "{row}"
        );
        let timer = plane.mission.dispatch_timer();
        assert_eq!(
            (timer.start_frame(), timer.delay()),
            (now as i32, int(&row["returned"])),
            "{row}"
        );
        replayed += 1;
    }
    assert!(replayed > 50, "{replayed}");
}

/// Drop_Payload per row. The oracle stubs the passenger's Can_Enter_Cell
/// (the seam answers it here), PlaceInfantryInCell (a full cell stands for
/// its refusal; VERA's spot within the landing cell replaces the stub's
/// coordinate) and SpawnParachuted (a playfield without the landing cell
/// stands for its refusal); the rest runs: the head's departure, Ammo, the
/// landing point, ChuteSound at the plane, the neighbour cell, the team,
/// the passes and the rearm timer, or the failure's re-boarding.
#[test]
fn drop_payload_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "drop_payload");
    let mut replayed = 0;
    for row in rows {
        if coord(&row["location"]) != LOCATION {
            // The stub's SpawnParachuted lands a passenger past the map's
            // corner, where the real one's playfield test refuses it.
            continue;
        }
        let (rules, mut sim, _) = paradrop_world(retail_list_rules(1024));
        let passengers = u32::try_from(int(&row["passengers"])).unwrap();
        let plane = plane_at(&mut sim, &rules, TARGET, passengers);
        let entity = sim.substrate.entities.get_mut(plane).unwrap();
        entity
            .body_facing
            .snap(u16::try_from(int(&row["facing"])).unwrap(), 0);
        entity.aircraft_ammo.as_mut().unwrap().current = int(&row["ammo"]);
        let passes = i8::try_from(int(&row["passes"])).unwrap();
        entity.mission_leaf.set_paradrop_passes_for_test(passes);
        let rearm = entity.rearm_timer;
        if flag(row, "team") {
            crate::sim::team_script_vm::join_team_for_test(&mut sim, &[plane], false, false);
        }
        let ids = cargo(&sim, plane);
        let landing = events(row, "cell_at")
            .next()
            .map(|call| coord(&call[1]))
            .map(|[x, y, _]| ((x / 256) as u16, (y / 256) as u16));
        if !flag(row, "spot") {
            fill_spots(&mut sim, landing.unwrap());
        }
        if !flag(row, "spawned") {
            sim.playfield_bounds = Some(NO_DROP_PLAYFIELD);
        }
        let neighbours: Vec<(i16, i16)> = ids
            .iter()
            .map(|&id| {
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .navigation
                    .neighbor_state
                    .cell()
            })
            .collect();
        sim.session.binary_frame = 4321;
        sim.sound_events.clear();

        let can_enter = int(&row["can_enter"]) as u8;
        let (observed, records) = observe(can_enter, || {
            drop_payload(
                &mut sim,
                plane,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
        });

        let observed: Vec<Observed> = observed
            .into_iter()
            .filter(|call| !matches!(call, Observed::Queue(..) | Observed::DoAction(..)))
            .collect();
        let spawned_at = observed.iter().find_map(|call| match call {
            Observed::SpawnParachuted(_, at) => Some(*at),
            _ => None,
        });
        let calls = row["events"].as_array().unwrap();
        let expected: Vec<Observed> = calls
            .iter()
            .enumerate()
            .map(|(n, call)| {
                let passenger = |at: usize| ids[usize::try_from(int(&call[at])).unwrap()];
                match call[0].as_str().unwrap() {
                    "cell_at" if n == 0 => Observed::CellAt(coord(&call[1])),
                    // Map[] at the spot SpawnParachuted took.
                    "cell_at" => Observed::CellAt(spawned_at.unwrap()),
                    "can_enter" => {
                        // Direction -1, height -1, no previous cell, mode 1.
                        assert_eq!(
                            call.as_array().unwrap()[2..4],
                            [json!(true), json!([-1, -1, 0, 1])]
                        );
                        Observed::CanEnter(passenger(1), int(&call[4]))
                    }
                    "subposition" => {
                        // No priority, no alternative, no occupancy test.
                        assert_eq!(call[2], json!([0, 0, 0]));
                        Observed::Subposition(coord(&call[1]))
                    }
                    "spawn_parachuted" => {
                        // The spot's XY at the plane's Z.
                        let at = spawned_at.unwrap();
                        assert_eq!(at[2], coord(&call[2])[2], "{row}");
                        let cell = ((at[0] / 256) as u16, (at[1] / 256) as u16);
                        assert_eq!(Some(cell), landing, "{row}");
                        Observed::SpawnParachuted(passenger(1), at)
                    }
                    "play_at" => {
                        // ChuteSound (`Rules+0x71C`) at the plane's Location.
                        assert_eq!((int(&call[1]), int(&call[3])), (9, 0));
                        Observed::PlayAt(coord(&call[2]))
                    }
                    "remove_member" => {
                        assert_eq!(call.as_array().unwrap()[3..], [json!(-1), json!(0)]);
                        assert_eq!(call[1], true);
                        Observed::RemoveMember(passenger(2))
                    }
                    "limbo" => Observed::Limbo(passenger(1)),
                    "conceal" => Observed::Conceal(passenger(1)),
                    other => panic!("{other}"),
                }
            })
            .collect();
        assert_eq!(observed, expected, "{row}");
        let asked = events(row, "can_enter").count();
        assert_eq!(records.len(), asked, "{row}");
        if let (
            Some(FreshCallRecord::CanEnter {
                cell,
                direction,
                height,
                ..
            }),
            Some(landing),
        ) = (records.first(), landing)
        {
            assert_eq!(*cell, (landing.0 as i16, landing.1 as i16));
            assert_eq!((*direction, *height), (-1, -1));
        }

        let entity = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(
            entity.aircraft_ammo.as_ref().unwrap().current,
            int(&row["ammo_after"]),
            "{row}"
        );
        let leaf = entity.mission_leaf.as_aircraft().unwrap();
        assert_eq!(
            i32::from(leaf.paradrop_passes()),
            int(&row["passes_after"]),
            "{row}"
        );
        let rearm = if row["rearm_timer"] == json!([4321, 0]) {
            CdTimer::started(4321, 0)
        } else {
            rearm
        };
        assert_eq!(entity.rearm_timer, rearm, "{row}");
        let chain: Vec<u64> = row["cargo"][1]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| ids[usize::try_from(int(index)).unwrap()])
            .collect();
        assert_eq!(chain.len(), int(&row["cargo"][0]) as usize);
        assert_eq!(cargo(&sim, plane), chain, "{row}");
        let sounds = events(row, "play_at").count();
        assert_eq!(chute_sounds(&sim), vec![TARGET; sounds], "{row}");
        // The dropped passenger's neighbour cell (`+0x55C`) is the spot's
        // cell; the stub's cell answers (40, 41) for any coordinate.
        for (n, &id) in ids.iter().enumerate() {
            let landed = n == 0 && sounds == 1;
            let expected = match spawned_at {
                Some([x, y, _]) if landed => ((x / 256) as i16, (y / 256) as i16),
                _ => neighbours[n],
            };
            let cell = sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .neighbor_state
                .cell();
            assert_eq!(cell, expected, "{row}");
        }
        replayed += 1;
    }
    assert!(replayed > 25, "{replayed}");
}

/// SpawnParachuted: ObjectClass::Paradrop places the passenger, then Guard
/// for a house controlled by a human (`HouseClass::IsControlledByHuman`:
/// human, or PlayerControl outside a multiplayer game mode), else Hunt, and
/// a forced Paradrop action; a refused Paradrop does neither.
#[test]
fn spawn_parachuted_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "spawn_parachuted");
    assert!(rows.len() > 10);
    for row in rows {
        let (rules, mut sim, americans) = paradrop_world(retail_list_rules(1024));
        let plane = plane_at(&mut sim, &rules, TARGET, 1);
        let [passenger] = cargo(&sim, plane)[..] else {
            panic!("one passenger");
        };
        let house = sim.houses.get_mut(&americans).unwrap();
        house.is_human = flag(row, "human");
        house.player_control = flag(row, "player_control");
        sim.session.game_mode_nonzero = int(&row["game_mode"]) != 0;
        if !flag(row, "paradropped") {
            sim.playfield_bounds = Some(NO_DROP_PLAYFIELD);
        }

        let (observed, _) = observe(0, || {
            drop_payload(
                &mut sim,
                plane,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
        });

        let spawned = observed
            .iter()
            .position(|call| matches!(call, Observed::SpawnParachuted(..)))
            .expect("SpawnParachuted ran");
        let after: Vec<Observed> = observed[spawned + 1..]
            .iter()
            .filter(|call| matches!(call, Observed::Queue(..) | Observed::DoAction(..)))
            .cloned()
            .collect();
        let expected: Vec<Observed> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|call| match call[0].as_str().unwrap() {
                "paradrop" => None,
                "queue" => {
                    assert_eq!(call[2], 0);
                    Some(Observed::Queue(
                        passenger,
                        MissionId::from_raw(int(&call[1])),
                    ))
                }
                "do_action" => {
                    // The random-frame argument is 0, as VERA's Do_Action.
                    assert_eq!(call[3], 0);
                    Some(Observed::DoAction(passenger, int(&call[1]), call[2] == 1))
                }
                other => panic!("{other}"),
            })
            .collect();
        assert_eq!(after, expected, "{row}");
        if let Some(Observed::Queue(_, mission)) = expected.first() {
            let entity = sim.substrate.entities.get(passenger).unwrap();
            assert_eq!(entity.mission.queued(), *mission, "{row}");
        }
    }
}

/// Retail rules (the lib suite loads no SHP; the canopy draws nothing).
fn retail_rules() -> Option<RuleSet> {
    retail_rules_binding(&[])
}

/// Steps until `plane` has left the map and none of `passengers` is aboard
/// or falling. Returns the ChuteSounds heard and, per passenger, the
/// missions (queued, current) on the frame it left the plane.
fn fly_out(
    sim: &mut Simulation,
    rules: &RuleSet,
    plane: u64,
    passengers: &[u64],
) -> (usize, Vec<Option<(MissionId, MissionId)>>) {
    let mut chutes = 0;
    let mut missions = vec![None; passengers.len()];
    let busy = |sim: &Simulation| {
        passengers.iter().any(|&id| {
            sim.substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.in_limbo || entity.parachute_state.is_some())
        })
    };
    let mut frames = 0;
    while sim.substrate.entities.contains(plane) || busy(sim) {
        sim.sound_events.clear();
        step(sim, rules);
        frames += 1;
        chutes += chute_sounds(sim).len();
        for (n, &id) in passengers.iter().enumerate() {
            if let Some(entity) = sim.substrate.entities.get(id)
                && !entity.lifecycle.in_limbo
                && missions[n].is_none()
            {
                missions[n] = Some((entity.mission.queued(), entity.mission.current()));
            }
        }
        assert!(frames < 4000, "the plane never left");
    }
    (chutes, missions)
}

/// Each of `passengers` stands on the ground within `cells` of `around`,
/// having left the plane in `mission`.
fn assert_landed(
    sim: &Simulation,
    passengers: &[u64],
    missions: &[Option<(MissionId, MissionId)>],
    around: (u16, u16),
    mission: MissionType,
) {
    let mission = MissionId::from_known(mission);
    for (&id, missions) in passengers.iter().zip(missions) {
        let entity = sim.substrate.entities.get(id).expect("landed");
        assert!(!entity.lifecycle.in_limbo && entity.parachute_state.is_none());
        let off = |at: u16, centre: u16| (i32::from(at) - i32::from(centre)).abs();
        assert!(
            off(entity.position.rx, around.0) <= 5 && off(entity.position.ry, around.1) <= 5,
            "{id} landed at ({}, {})",
            entity.position.rx,
            entity.position.ry
        );
        let (queued, current) = missions.expect("left the plane");
        assert!(
            queued == mission || current == mission,
            "{queued:?} {current:?}"
        );
    }
}

/// The player's paradrops on retail rules, from the click through
/// production frames. One PDPLANE enters at the Americans' North edge (its
/// cells sum to 36 on this map) in Mission_ParadropApproach with the
/// clicked cell as its Target, carrying AllyParaDrop's six E1 (case 5, the
/// Allied side) or AmerParaDrop's eight (case 6). It drops them one an
/// Overfly visit within `ParadropRadius=`, each with a ChuteSound, then
/// retreats and leaves the map; they land around the target in Guard. The
/// Super recharges.
#[test]
fn retail_paradrops_land_their_lists_around_the_target() {
    for (name, count) in [(PARADROP, 6), (AMER_PARADROP, 8)] {
        let Some(rules) = retail_rules() else {
            return;
        };
        let (rules, mut sim, americans) = paradrop_world(rules);
        let sw_type = charge_super(&mut sim, americans, name);

        click(&mut sim, &rules, americans, name, TARGET);

        assert!(!sim.super_weapons[&americans][&sw_type].is_ready);
        let [plane] = planes(&sim)[..] else {
            panic!("one plane");
        };
        let entity = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(entity.owner(), americans);
        assert!(entity.is_mission_only());
        assert_eq!(
            entity.mission.current(),
            MissionId::from_known(MissionType::ParadropApproach)
        );
        assert_eq!(
            entity.attack_target.as_ref().map(|attack| attack.target),
            Some(TargetKind::Cell(TARGET.0, TARGET.1))
        );
        assert_eq!(entity.position.rx + entity.position.ry, 36);
        let passengers = cargo(&sim, plane);
        assert_eq!(passengers.len(), count, "{name}");
        for &id in &passengers {
            let passenger = sim.substrate.entities.get(id).unwrap();
            assert_eq!(sim.interner.resolve(passenger.type_ref()), "E1");
        }

        let (chutes, missions) = fly_out(&mut sim, &rules, plane, &passengers);

        assert_eq!(chutes, count, "{name}");
        assert_landed(&sim, &passengers, &missions, TARGET, MissionType::Guard);
    }
}

/// The computer's paradrop on retail rules, through AI_TryFireSW's
/// GroundRallyPoint arm (`0x00509CD0`): its enemy's base cell is passable,
/// so the plane's Target is two cells past it on both axes. The Russians
/// (Soviet side) drop SovParaDrop's nine E2, who land in Hunt.
#[test]
fn retail_computer_paradrops_conscripts_past_the_enemy_base() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = paradrop_world(rules);
    let russians: InternedId = sim.interner.intern("Russians");
    sim.houses.get_mut(&russians).unwrap().enemy_house = Some(americans);
    sim.houses.get_mut(&americans).unwrap().base_center = Some(TARGET);
    let sw_type = charge_super(&mut sim, russians, PARADROP);

    super::super::ai_fire::try_fire(
        &mut sim,
        &rules,
        russians,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(!sim.super_weapons[&russians][&sw_type].is_ready);
    let [plane] = planes(&sim)[..] else {
        panic!("one plane");
    };
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.owner(), russians);
    let target = (TARGET.0 + 2, TARGET.1 + 2);
    assert_eq!(
        entity.attack_target.as_ref().map(|attack| attack.target),
        Some(TargetKind::Cell(target.0, target.1))
    );
    let passengers = cargo(&sim, plane);
    assert_eq!(passengers.len(), 9);
    for &id in &passengers {
        let passenger = sim.substrate.entities.get(id).unwrap();
        assert_eq!(sim.interner.resolve(passenger.type_ref()), "E2");
        assert_eq!(passenger.owner(), russians);
    }

    let (chutes, missions) = fly_out(&mut sim, &rules, plane, &passengers);

    assert_eq!(chutes, 9);
    assert_landed(&sim, &passengers, &missions, target, MissionType::Hunt);
}
