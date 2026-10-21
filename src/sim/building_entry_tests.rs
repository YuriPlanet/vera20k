//! Tests for the building-entry seeks. The picks replay native
//! EnterBioReactor's, EnterBattleBunker's and EnterTankBunker's rows
//! (`tools/team_recruit_oracle.py`'s `bio_reactor`, `battle_bunker` and
//! `tank_bunker`, under Unicorn); the production callers below are Rust
//! regression tests.

use serde_json::{Value, json};

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::docking::bunker_install::BunkerState;
use crate::sim::docking::bunker_link::BunkerLink;
use crate::sim::house_state::HouseState;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::PathGrid;
use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};

const RULES: &str = "\
[InfantryTypes]
0=INIT
1=OVERSIZE
2=E1
[VehicleTypes]
0=LTNK
1=TRNS
2=YTNK
3=CAOS
[BuildingTypes]
0=YAPOWR
1=GAPOWR
2=UABSORB
3=NABNKR
4=CAGAS01
5=NATBNK
[Guard]
Rate=.030
[Capture]
Rate=.016
[E1]
Strength=125
Speed=4
Occupier=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[NABNKR]
Strength=500
TechLevel=1
CanBeOccupied=yes
CanOccupyFire=yes
MaxNumberOccupants=5
[CAGAS01]
Strength=400
TechLevel=-1
CanBeOccupied=yes
MaxNumberOccupants=5
[INIT]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[OVERSIZE]
Strength=100
Speed=4
Size=16
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[LTNK]
Strength=300
Speed=6
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[TRNS]
Strength=300
Speed=6
Passengers=2
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[YTNK]
Strength=375
Speed=6
Turret=yes
Primary=AGGattling
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[CAOS]
Strength=200
Speed=6
; Turret= unlike retail's, so that only Bunkerable= refuses it.
Turret=yes
Primary=ChaosAttack
Bunkerable=no
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[NATBNK]
Strength=1000
Bunker=yes
NumberImpassableRows=0
[YAPOWR]
Strength=700
InfantryAbsorb=yes
Passengers=5
SizeLimit=15
Power=50
ExtraPower=100
[GAPOWR]
Strength=750
[UABSORB]
Strength=700
UnitAbsorb=yes
Passengers=5
SizeLimit=15
";

const ART: &str = "[YAPOWR]\nFoundation=2x2\n[GAPOWR]\nFoundation=2x2\n[UABSORB]\nFoundation=2x2\n\
                   [NABNKR]\nFoundation=2x2\n[CAGAS01]\nFoundation=2x2\n[NATBNK]\nFoundation=2x2\n";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str(ART),
    )
    .expect("building entry rules")
}

/// The flat arena with the computer YuriCountry and the human Americans.
fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = Simulation::with_seed(0x4DFB70);
    for (name, side, human) in [("YuriCountry", 2, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// Fills `count` of `building`'s passenger slots.
fn absorb(sim: &mut Simulation, rules: &RuleSet, building: u64, count: usize) {
    for _ in 0..count {
        let id = sim
            .construct_object_limbo_at_height(
                "INIT",
                "YuriCountry",
                1,
                1,
                0,
                0,
                rules,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("absorbed infantry");
        sim.substrate.entities.get_mut(id).unwrap().passenger_role =
            PassengerRole::inside_for_test(building, false);
        let building = sim.substrate.entities.get_mut(building).unwrap();
        building
            .passenger_role
            .cargo_mut()
            .unwrap()
            .board_forced_chain(&[(id, 1.0)]);
    }
}

fn can_load(sim: &mut Simulation, rules: &RuleSet, from: u64, building: u64) -> RadioResponse {
    transmit(
        sim,
        from,
        building,
        RadioMessage::CanEnter,
        RadioPayload::default(),
        Some(rules),
        FrameEffects::default(),
    )
}

fn seek(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    sim.foot_enter_bio_reactor(id, rules, None, FrameEffects::default())
}

fn entity(sim: &Simulation, id: u64) -> &GameEntity {
    sim.substrate.entities.get(id).unwrap()
}

/// Runs the production frame until `done` holds, for at most `frames`
/// frames; answers whether it held.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    frames: usize,
    done: impl Fn(&Simulation) -> bool,
) -> bool {
    for _ in 0..frames {
        if done(sim) {
            return true;
        }
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
    }
    done(sim)
}

/// A team running script action `action_id` (a seek), then 49, of
/// `members`.
fn seek_team(sim: &mut Simulation, action_id: i32, members: Vec<u64>) -> u64 {
    let script = sim.interner.intern(&format!("SEEK {action_id}"));
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: vec![
            TeamScriptAction {
                action_id,
                argument: 0,
            },
            TeamScriptAction {
                action_id: 49,
                argument: 0,
            },
        ],
    });
    let owner = sim.interner.intern("YuriCountry");
    sim.team_script_vm.create_team(owner, script, members, 0)
}

fn garrison_seek(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    sim.foot_enter_battle_bunker(id, rules, None, FrameEffects::default())
}

fn whole_map_garrison_seek(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    sim.foot_enter_nearest_occupiable(id, rules, None, FrameEffects::default())
}

fn tank_bunker_seek(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    sim.foot_enter_tank_bunker(id, rules, None, FrameEffects::default())
}

/// The bunker's native install status and its first radio contact.
fn install_state(sim: &Simulation, bunker: u64) -> (BunkerState, Option<u64>) {
    let building = entity(sim, bunker);
    (
        building.bunker_runtime.unwrap().state(),
        building.radio_contacts.slot(0),
    )
}

fn occupants(sim: &Simulation, building: u64) -> Vec<u64> {
    entity(sim, building)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()
        .to_vec()
}

fn xyz(value: &Value) -> [i32; 3] {
    let axes: Vec<i32> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|axis| i32::try_from(axis.as_i64().unwrap()).unwrap())
        .collect();
    axes.try_into().unwrap()
}

/// Native EnterBioReactor (`0x004DFB70`), its CAN_LOAD answers supplied:
/// each absorber is asked from the list's last, and the pick, its reissue
/// (Queue_Mission(Enter, 0) and Assign_Destination(pick, 1)), the answer
/// and the latch are [`pick_bio_reactor`]'s over [`object_distance`]:
/// strictly nearer wins, ties to the later absorber, distances that differ
/// below a lepton tie, and a pick already the first radio contact or the
/// pending entry is not reissued.
#[test]
fn the_bio_reactor_pick_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json")).unwrap();
    let rows = oracle["bio_reactor"].as_array().unwrap();
    assert_eq!(rows.len(), 129);
    let enter = MissionId::from_known(MissionType::Enter).raw();
    let mut picked_rows = 0;
    for (number, row) in rows.iter().enumerate() {
        let events = row["events"].as_array().unwrap();
        let of = |kinds: &[&str]| -> Vec<Value> {
            events
                .iter()
                .filter(|event| kinds.iter().any(|kind| event[0] == *kind))
                .cloned()
                .collect()
        };
        let absorbers = row["absorbers"].as_array().unwrap();
        let walk: Vec<u64> = (0..absorbers.len() as u64).rev().collect();
        let asked: Vec<Value> = walk
            .iter()
            .map(|&index| json!(["transmit", RadioMessage::CanEnter as u8, index]))
            .collect();
        assert_eq!(of(&["transmit"]), asked, "row {number}: the walk");

        let foot = xyz(&row["foot"]);
        let (best, reissue) = pick_bio_reactor(
            walk.iter().map(|&index| {
                let absorber = &absorbers[index as usize];
                let roger = absorber["answer"] == RadioResponse::Roger.code();
                let distance = if roger {
                    object_distance(foot, xyz(&absorber["xyz"]), None)
                } else {
                    i32::MAX
                };
                (index, roger, distance)
            }),
            row["contact"].as_u64(),
            row["pending"].as_u64(),
        );
        let native_pick = of(&["picked"]).first().and_then(|event| event[1].as_u64());
        assert_eq!(best, native_pick, "row {number}: the pick");
        let reissued: Vec<Value> = best
            .filter(|_| reissue)
            .map(|best| {
                vec![
                    json!(["queue_mission", enter, 0]),
                    json!(["assign_destination", best, 1]),
                ]
            })
            .unwrap_or_default();
        assert_eq!(
            of(&["queue_mission", "assign_destination"]),
            reissued,
            "row {number}: the reissue"
        );
        assert_eq!(
            row["result"] == 1,
            best.is_some(),
            "row {number}: the answer"
        );
        assert_eq!(
            row["latch_after"] == 1,
            best.is_some(),
            "row {number}: the latch"
        );
        picked_rows += usize::from(best.is_some());
    }
    assert_eq!(picked_rows, 89);
}

/// Native EnterBattleBunker (`0x004DFCB0`), its CanBeOccupiedBy answers
/// supplied: the walk runs from the list's last, only a strictly nearer
/// building is asked (with the Foot, or NULL for a vehicle, which refuses),
/// ties keep the later building, and the pick, its reissue
/// (Assign_Destination(pick, 1), then Queue_Mission(Capture, 1)), the answer
/// and the latch are [`pick_nearest_admitted`]'s and
/// [`garrison_seek_reissues`]' over [`object_distance`].
#[test]
fn the_battle_bunker_pick_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json")).unwrap();
    let rows = oracle["battle_bunker"].as_array().unwrap();
    assert_eq!(rows.len(), 128);
    let capture = MissionId::from_known(MissionType::Capture).raw();
    let mut picked_rows = 0;
    for (number, row) in rows.iter().enumerate() {
        let events = row["events"].as_array().unwrap();
        let bunkers = row["bunkers"].as_array().unwrap();
        let foot = xyz(&row["foot"]);
        let infantry = row["infantry"] == true;
        let mut seen = Vec::new();
        let best = pick_nearest_admitted(
            (0..bunkers.len() as u64).rev().map(|index| {
                let to = xyz(&bunkers[index as usize]["xyz"]);
                (index, object_distance(foot, to, None))
            }),
            |index| {
                let occupant = if infantry { json!("foot") } else { Value::Null };
                seen.push(json!(["can_be_occupied_by", index, occupant]));
                infantry && bunkers[index as usize]["answer"] == true
            },
        );
        let nav_com = row["nav"].as_u64().map(NavTargetRef::object);
        let mission = MissionId::from_raw(i32::try_from(row["mission"].as_i64().unwrap()).unwrap());
        if let Some(best) = best
            && garrison_seek_reissues(best, nav_com, mission)
        {
            seen.push(json!(["assign_destination", best, 1]));
            seen.push(json!(["queue_mission", capture, 1]));
        }
        assert_eq!(&seen, events, "row {number}: the calls");
        assert_eq!(
            row["result"] == 1,
            best.is_some(),
            "row {number}: the answer"
        );
        assert_eq!(
            row["latch_after"] == 1,
            best.is_some(),
            "row {number}: the latch"
        );
        picked_rows += usize::from(best.is_some());
    }
    assert_eq!(picked_rows, 86);
}

/// Native EnterTankBunker (`0x004DFF40`): an admitted Foot walks from the
/// list's last bunker, only a strictly nearer one has its first radio
/// contact (`0x0065AD30`) and occupant read, and ties keep the later bunker,
/// as [`pick_nearest_admitted`] over [`object_distance`] picks; the pick
/// gets Assign_Destination(pick, 1), HELLO and Queue_Mission(Enter, 1). A
/// refused Foot walks nothing.
#[test]
fn the_tank_bunker_pick_matches_native() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json")).unwrap();
    let rows = oracle["tank_bunker"].as_array().unwrap();
    assert_eq!(rows.len(), 130);
    let enter = MissionId::from_known(MissionType::Enter).raw();
    let mut picked_rows = 0;
    for (number, row) in rows.iter().enumerate() {
        let events = row["events"].as_array().unwrap();
        if row["admitted"] == 0 {
            assert!(events.is_empty(), "row {number}: a refused walk");
            assert_eq!(row["result"], 0, "row {number}: a refused answer");
            continue;
        }
        let bunkers = row["bunkers"].as_array().unwrap();
        let foot = xyz(&row["foot"]);
        let seen = std::cell::RefCell::new(Vec::new());
        let best = pick_nearest_admitted(
            (0..bunkers.len() as u64).rev().map(|index| {
                seen.borrow_mut().push(json!(["coords", index]));
                let to = xyz(&bunkers[index as usize]["xyz"]);
                (index, object_distance(foot, to, None))
            }),
            |index| {
                seen.borrow_mut().push(json!(["contact", index, 0]));
                let bunker = &bunkers[index as usize];
                bunker["contact"] == false && bunker["occupied"] == false
            },
        );
        let mut seen = seen.into_inner();
        if let Some(best) = best {
            seen.push(json!(["assign_destination", best, 1]));
            seen.push(json!(["transmit", RadioMessage::Hello.code(), best]));
            seen.push(json!(["queue_mission", enter, 1]));
        }
        assert_eq!(&seen, events, "row {number}: the calls");
        assert_eq!(
            row["result"] == 1,
            best.is_some(),
            "row {number}: the answer"
        );
        picked_rows += usize::from(best.is_some());
    }
    assert_eq!(picked_rows, 85);
}

/// `BuildingClass::Receive_Radio`'s CAN_LOAD absorber arm
/// (`0x0043C43B..0x0043C4F2`): ROGER while a slot is free, the later arms'
/// STATIC to a sender above `SizeLimit=` or when full, NEGATORY to a class
/// the absorber does not take.
#[test]
fn an_absorber_answers_can_load_by_room_size_and_class() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let infantry = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    let oversize = spawn(&mut sim, &rules, "OVERSIZE", "YuriCountry", 10, 14);

    absorb(&mut sim, &rules, reactor, 4);
    assert_eq!(
        can_load(&mut sim, &rules, infantry, reactor),
        RadioResponse::Roger
    );
    assert_eq!(
        can_load(&mut sim, &rules, tank, reactor),
        RadioResponse::Negatory
    );
    assert_eq!(
        can_load(&mut sim, &rules, oversize, reactor),
        RadioResponse::None
    );
    absorb(&mut sim, &rules, reactor, 1);
    assert_eq!(
        can_load(&mut sim, &rules, infantry, reactor),
        RadioResponse::None
    );
}

/// The CAN_LOAD absorber arm refuses a sender controlling a captive
/// (`0x004722C0`), though a slot is free.
#[test]
fn an_absorber_refuses_a_controller() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let controller = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let captive = spawn(&mut sim, &rules, "INIT", "Americans", 10, 12);
    assert_eq!(
        can_load(&mut sim, &rules, controller, reactor),
        RadioResponse::Roger
    );

    sim.substrate
        .entities
        .get_mut(controller)
        .unwrap()
        .capture_manager = Some(
        crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(
            1,
            false,
            &[captive],
        ),
    );

    assert_eq!(
        can_load(&mut sim, &rules, controller, reactor),
        RadioResponse::Negatory
    );
}

/// EnterBioReactor over its house's absorbers only: a power plant, a full
/// reactor and another house's reactor stand nearer than the reactor with
/// room that takes the Foot. The Foot latches, queues Enter, heads for it
/// and, through the Infantry setter's Enter arm, HELLOs it
/// (`0x0051ACD4`).
#[test]
fn the_nearest_own_reactor_with_room_takes_the_foot() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let foot = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let _plant = spawn(&mut sim, &rules, "GAPOWR", "YuriCountry", 12, 12);
    let _enemy = spawn(&mut sim, &rules, "YAPOWR", "Americans", 7, 10);
    let full = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 13, 8);
    absorb(&mut sim, &rules, full, 5);
    let room = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 18, 10);
    let _far = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 24, 10);

    assert!(seek(&mut sim, &rules, foot));

    let entity = sim.substrate.entities.get(foot).unwrap();
    assert!(entity.building_entry.should_enter_absorber());
    assert_eq!(
        entity.mission.queued(),
        MissionId::from_known(MissionType::Enter)
    );
    assert_eq!(
        entity.navigation.nav_com(),
        Some(NavTargetRef::object(room))
    );
    assert_eq!(entity.radio_contacts.slot(0), Some(room));
}

/// Equidistant absorbers through the production walk: the house's list is
/// walked from its last and only a strictly nearer absorber replaces the
/// pick, so the one built later takes the Foot, whichever side it stands.
/// The seeker is a vehicle, which stands at its cell's centre (an
/// infantryman's sub-cell spot would break the mirror); the Unit setter
/// parks the pick as its pending entry (`0x00741CE5..0x00741D0D`).
#[test]
fn of_equidistant_absorbers_the_later_built_takes_the_foot() {
    let rules = rules();
    for cells in [[(13, 8), (8, 13)], [(8, 13), (13, 8)]] {
        let (mut sim, _) = arena(&rules);
        let foot = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 10);
        let reactors =
            cells.map(|(rx, ry)| spawn(&mut sim, &rules, "UABSORB", "YuriCountry", rx, ry));
        let coords = |id| {
            let coords = object_get_coords(entity(&sim, id), sim.resolved_terrain.as_ref());
            [coords.x, coords.y, coords.z]
        };
        let [first, later] =
            reactors.map(|reactor| object_distance(coords(foot), coords(reactor), None));
        assert_eq!(first, later, "the absorbers stand equally far");

        assert!(seek(&mut sim, &rules, foot));

        assert_eq!(entity(&sim, foot).pending_entry(), Some(reactors[1]));
    }
}

/// All three latches are saved and, while set, hashed.
#[test]
fn the_latches_are_saved_and_hashed() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let _reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let _bunker = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 16, 14);
    let foot = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let occupier = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 14);
    assert!(seek(&mut sim, &rules, foot));
    assert!(garrison_seek(&mut sim, &rules, occupier));
    set_should_enter_nearest_occupiable_for_test(
        sim.substrate.entities.get_mut(occupier).unwrap(),
        true,
    );

    let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "Seek", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();

    assert!(
        entity(&restored, foot)
            .building_entry
            .should_enter_absorber()
    );
    assert!(
        entity(&restored, occupier)
            .building_entry
            .should_enter_occupiable()
    );
    assert!(
        entity(&restored, occupier)
            .building_entry
            .should_enter_nearest_occupiable()
    );
    let latched = restored.state_hash();
    set_should_enter_absorber_for_test(restored.substrate.entities.get_mut(foot).unwrap(), false);
    let absorber_cleared = restored.state_hash();
    assert_ne!(absorber_cleared, latched);
    set_should_enter_occupiable_for_test(
        restored.substrate.entities.get_mut(occupier).unwrap(),
        false,
    );
    let house_garrison_cleared = restored.state_hash();
    assert_ne!(house_garrison_cleared, absorber_cleared);
    set_should_enter_nearest_occupiable_for_test(
        restored.substrate.entities.get_mut(occupier).unwrap(),
        false,
    );
    assert_ne!(restored.state_hash(), house_garrison_cleared);
}

/// Script action 62 (`0x006E9E83`) through the team pass: the member a
/// reactor takes leaves the team, the one none takes stays, and the action
/// is finished.
#[test]
fn script_action_62_sends_the_members_a_reactor_takes() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let _reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 18, 10);
    let infantry = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    let team = seek_team(&mut sim, 62, vec![infantry, tank]);

    sim.run_team_ai_pass(&rules, None, FrameEffects::default());

    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), [tank]);
    assert!(state.advance_pending());
    assert_eq!(sim.team_script_vm.team_for_member(infantry), None);
    let entity = sim.substrate.entities.get(infantry).unwrap();
    assert!(entity.building_entry.seeking());
    assert_eq!(
        entity.mission.queued(),
        MissionId::from_known(MissionType::Enter)
    );
    assert!(
        !sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .building_entry
            .seeking()
    );
}

/// `TeamClass::Can_Add` refuses a Foot seeking a building's entry
/// (`Is_Seeking_Building_Entry`, `0x006EA79B`).
#[test]
fn a_seeking_foot_is_not_recruited() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let infantry = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let team = seek_team(&mut sim, 62, Vec::new());
    assert_eq!(sim.team_can_add(team, infantry, true, &rules), Some(0));

    set_should_enter_absorber_for_test(sim.substrate.entities.get_mut(infantry).unwrap(), true);

    assert_eq!(sim.team_can_add(team, infantry, true, &rules), None);
}

/// A loaded Bio Reactor accepts the player's Unload order and ejects its cargo
/// through the regular building mission and frame, one cargo head per visit.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_bio_reactor_unload_command_reveals_its_passenger() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let owner = sim.interner.get("YuriCountry").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(true);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let passengers: Vec<_> = (0..5)
        .map(|index| spawn(&mut sim, &rules, "INIT", "YuriCountry", 6 + index * 2, 10))
        .collect();
    for &passenger in &passengers {
        assert!(seek(&mut sim, &rules, passenger));
        assert!(
            entity(&sim, passenger)
                .building_entry
                .should_enter_absorber()
        );
        assert!(run_until(&mut sim, &rules, &grid, 300, |sim| {
            entity(sim, passenger).passenger_role.inside_transport_id() == Some(reactor)
        }));
    }
    assert_eq!(sim.houses[&owner].tracking().infantry(), 0);
    assert_eq!(
        sim.houses[&owner].tracking().owned_count(
            EntityCategory::Infantry,
            entity(&sim, passengers[0]).type_ref()
        ),
        5
    );
    for &passenger in &passengers {
        assert!(
            !entity(&sim, passenger)
                .house_tracking_latches()
                .is_counted()
        );
        assert!(
            entity(&sim, passenger)
                .house_tracking_latches()
                .is_absorbed()
        );
    }
    let cargo_lifo = entity(&sim, reactor)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()
        .to_vec();
    assert_eq!(
        cargo_lifo,
        passengers.iter().rev().copied().collect::<Vec<_>>()
    );
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .mission_leaf
        .set_building_ready_latch(1);

    let order = CommandEnvelope::new(
        owner,
        sim.session.tick(),
        Command::UnloadPassengers {
            transport_id: reactor,
        },
    );
    let result = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 33);
    assert_eq!(result.executed_commands, 1, "the Unload order is admitted");

    let mut departed = Vec::new();
    let record_departures = |sim: &Simulation, departed: &mut Vec<u64>| {
        for &passenger in &passengers {
            if !entity(sim, passenger).lifecycle.in_limbo() && !departed.contains(&passenger) {
                departed.push(passenger);
            }
        }
    };
    record_departures(&sim, &mut departed);
    for _ in 0..300 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
        record_departures(&sim, &mut departed);
        if entity(&sim, reactor)
            .passenger_role
            .cargo()
            .is_some_and(|cargo| cargo.is_empty())
            && departed.len() == passengers.len()
        {
            break;
        }
    }
    assert!(
        entity(&sim, reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert_eq!(departed, cargo_lifo, "one cargo head leaves on each visit");
    for passenger_id in passengers {
        let passenger = entity(&sim, passenger_id);
        assert!(passenger.lifecycle.object_alive());
        assert!(passenger.lifecycle.cell_marked());
        assert!(passenger.in_logic_vector.is_member());
        assert!(!passenger.building_entry.should_enter_absorber());
        assert!(passenger.house_tracking_latches().is_counted());
        assert!(!passenger.house_tracking_latches().is_absorbed());
    }
    assert_eq!(sim.houses[&owner].tracking().infantry(), 5);
    assert_eq!(
        sim.houses[&owner].tracking().owned_count(
            EntityCategory::Infantry,
            entity(&sim, departed[0]).type_ref()
        ),
        5
    );
}

/// Mission_Unload pops the cargo head before scanning; when all eight
/// adjacent cells fail it restores that same head, spends only the shared
/// Unload cadence draw, and retries successfully after a legal exit opens.
#[test]
fn a_blocked_bio_reactor_restores_and_retries_its_cargo_head() {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(
        "[AudioVisual]\nEnterBioReactorSound=BioReactorEnter\nLeaveBioReactorSound=BioReactorExit\n",
    ));
    assert_eq!(
        ini.section("AudioVisual")
            .unwrap()
            .read_name("EnterBioReactorSound", 0x80),
        Some("BioReactorEnter")
    );
    assert_eq!(
        ini.section("AudioVisual")
            .unwrap()
            .read_name("LeaveBioReactorSound", 0x80),
        Some("BioReactorExit")
    );
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini.clone())
        .process_with_fixed_art(&IniFile::from_str(ART))
        .unwrap();
    assert_eq!(
        processed
            .ini()
            .section("AudioVisual")
            .unwrap()
            .read_name("LeaveBioReactorSound", 0x80),
        Some("BioReactorExit")
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART))
        .expect("Bio Reactor sound rules parse");
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=BioReactorEnter\n1=BioReactorExit\n\
         [BioReactorEnter]\nSounds=fixture.wav\n\
         [BioReactorExit]\nSounds=fixture.wav\n",
    ));
    rules.bind_type_sound_references(&ini, &sounds);
    assert_eq!(
        rules.general.leave_bio_reactor_sound.as_deref(),
        Some("BioReactorExit")
    );
    let (mut sim, grid) = arena(&rules);
    let owner = sim.interner.get("YuriCountry").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(true);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let passenger = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    assert!(seek(&mut sim, &rules, passenger));
    assert!(run_until(&mut sim, &rules, &grid, 300, |sim| {
        entity(sim, passenger).passenger_role.inside_transport_id() == Some(reactor)
    }));
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
                    if sound_id == "BioReactorEnter"
            ))
            .count(),
        1,
        "successful InfantryAbsorb boarding emits EnterBioReactorSound once"
    );
    let foot_coords = crate::sim::movement::ground_pose::object_get_coords(
        entity(&sim, passenger),
        sim.resolved_terrain.as_ref(),
    );
    let (foot_rx, foot_ry, foot_sub_x, foot_sub_y, _) = crate::sim::anim_class::AnimWorldCoord {
        x: foot_coords.x,
        y: foot_coords.y,
        z: foot_coords.z,
    }
    .to_cell_sub_z();
    assert!(
        sim.sound_events.iter().any(|event| matches!(
            event,
            crate::sim::world::SimSoundEvent::VocAt {
                sound_id,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
                ..
            } if sound_id == "BioReactorEnter"
                && (*rx, *ry) == (foot_rx, foot_ry)
                && *sub_x == foot_sub_x
                && *sub_y == foot_sub_y
                && *world_z_leptons == foot_coords.z
        )),
        "entry sound uses the arriving Foot's GetCoords"
    );
    sim.substrate
        .mission_assign_exact(
            reactor,
            MissionId::from_known(MissionType::Unload),
            sim.session.binary_frame(),
        )
        .unwrap();

    // Mission_Unload's scan uses the building Location (`vt+0x1B8`),
    // distinct from GetCoords' foundation center used by its sound tail.
    let base = (
        entity(&sim, reactor).position.rx(),
        entity(&sim, reactor).position.ry(),
    );
    let directions = crate::util::direction::DIRECTION_DELTAS;
    let neighbor = |from: (u16, u16), direction: (i32, i32)| {
        (
            (i32::from(from.0) + direction.0) as u16,
            (i32::from(from.1) + direction.1) as u16,
        )
    };
    let adjacent: Vec<_> = directions
        .iter()
        .map(|&delta| neighbor(base, delta))
        .collect();
    let block_foot = |sim: &mut Simulation, cell: (u16, u16), blocked: bool| {
        let cost = if blocked { 0 } else { 100 };
        let cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        cell.speed_costs.foot = Some(cost);
        cell.base_speed_costs.foot = Some(cost);
    };
    for &cell in &adjacent {
        block_foot(&mut sim, cell, true);
    }

    let held = serde_json::to_value(entity(&sim, reactor).passenger_role.cargo()).unwrap();
    let health = entity(&sim, passenger).health.current();
    let mut expected_rng = sim.scenario_rng.clone();
    expected_rng.next_range_u32_inclusive(0, 2);
    let rate_only_state = expected_rng.logical_state();
    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        serde_json::to_value(entity(&sim, reactor).passenger_role.cargo()).unwrap(),
        held,
        "the failed search reinserts the popped passenger at the cargo head"
    );
    assert_eq!(
        entity(&sim, passenger).passenger_role.inside_transport_id(),
        Some(reactor)
    );
    assert!(entity(&sim, passenger).lifecycle.in_limbo());
    assert_eq!(entity(&sim, passenger).health.current(), health);
    assert_eq!(sim.scenario_rng.logical_state(), rate_only_state);
    assert!(!sim.sound_events.iter().any(|event| matches!(
        event,
        crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
            if sound_id == "BioReactorExit"
    )));

    // The scan starts behind the facing; a foundation cell refuses the Foot
    // (its NavCom went with its entry, `0x004DF0D0`), so the first exit is
    // the first octant off the foundation.
    let facing = entity(&sim, reactor).body_facing_current(sim.session.binary_frame());
    let start = usize::from(crate::util::direction_tables::dir_from_facing16(
        facing.wrapping_add(0x7FFF),
    ));
    let octant = (0..8)
        .map(|index| (start + index) & 7)
        .find(|&octant| {
            let (x, y) = adjacent[octant];
            sim.substrate.occupancy.first_building_on_layer(
                x,
                y,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            ) != Some(reactor)
        })
        .unwrap();
    let exit = adjacent[octant];
    let beyond = neighbor(exit, directions[octant]);
    block_foot(&mut sim, exit, false);
    block_foot(&mut sim, beyond, false);

    let mut expected_rng = sim.scenario_rng.clone();
    assert!(
        crate::sim::movement::bump_crush::pick_infantry_sublocation(
            sim.resolved_terrain.as_ref(),
            &sim.substrate.raw_cell_occupation,
            &sim.substrate.occupancy,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            &mut expected_rng,
            crate::sim::components::DriveCoord::cell(exit.0, exit.1, 0),
        )
        .is_some()
    );
    expected_rng.next_range_u32_inclusive(0, 2);
    let successful_state = expected_rng.logical_state();

    sim.power_states.map_mut_for_test().insert(
        owner,
        crate::sim::power_system::PowerState::settled_for_test(),
    );
    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(
        entity(&sim, reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert!(!entity(&sim, passenger).lifecycle.in_limbo());
    assert!(entity(&sim, passenger).lifecycle.cell_marked());
    assert_eq!(sim.scenario_rng.logical_state(), successful_state);
    let building_coords = crate::sim::movement::ground_pose::object_get_coords(
        entity(&sim, reactor),
        sim.resolved_terrain.as_ref(),
    );
    let (building_rx, building_ry, building_sub_x, building_sub_y, _) =
        crate::sim::anim_class::AnimWorldCoord {
            x: building_coords.x,
            y: building_coords.y,
            z: building_coords.z,
        }
        .to_cell_sub_z();
    assert!(sim.power_states[&owner].power_recheck_for_test());
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
                    if sound_id == "BioReactorExit"
            ))
            .count(),
        1
    );
    assert!(
        sim.sound_events.iter().any(|event| matches!(
            event,
            crate::sim::world::SimSoundEvent::VocAt {
                sound_id,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
                ..
            } if sound_id == "BioReactorExit"
                && (*rx, *ry) == (building_rx, building_ry)
                && *sub_x == building_sub_x
                && *sub_y == building_sub_y
                && *world_z_leptons == building_coords.z
        )),
        "leave sound uses the Bio Reactor's GetCoords"
    );
}

/// A legal Infantry exit spends the native placement draw before Unlimbo. If
/// reveal then refuses the passenger, Mission_Unload re-adds the cargo head
/// and still takes its shared rate draw; the success-only tail does not run.
#[test]
fn a_bio_reactor_unlimbo_failure_keeps_the_placement_draw_and_restores_cargo() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    absorb(&mut sim, &rules, reactor, 1);
    let passenger = entity(&sim, reactor)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()[0];
    let owner = entity(&sim, reactor).owner();
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .body_facing
        .set(0, sim.session.binary_frame());

    // The first strict pair is north: Location (16,10), exit (16,9),
    // beyond (16,8). MarkSucceeded reaches reveal, whose lifecycle authority
    // rejects an already-marked object after the placement operation ran.
    // This injected state exercises Unlimbo's post-placement failure tail.
    let exit = (16, 9);
    let mut expected_rng = sim.scenario_rng.clone();
    assert!(
        crate::sim::movement::bump_crush::pick_infantry_sublocation(
            sim.resolved_terrain.as_ref(),
            &sim.substrate.raw_cell_occupation,
            &sim.substrate.occupancy,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            &mut expected_rng,
            crate::sim::components::DriveCoord::cell(exit.0, exit.1, 0),
        )
        .is_some()
    );
    expected_rng.next_range_u32_inclusive(0, 2);
    let expected_state = expected_rng.logical_state();

    let cargo_before = serde_json::to_value(entity(&sim, reactor).passenger_role.cargo()).unwrap();
    let health_before = entity(&sim, passenger).health.current();
    sim.substrate
        .entities
        .get_mut(passenger)
        .unwrap()
        .lifecycle
        .set_cell_marked_for_test(true);
    sim.power_states.map_mut_for_test().insert(
        owner,
        crate::sim::power_system::PowerState::settled_for_test(),
    );

    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    let released = entity(&sim, passenger);
    assert_eq!(
        serde_json::to_value(entity(&sim, reactor).passenger_role.cargo()).unwrap(),
        cargo_before
    );
    assert_eq!(released.passenger_role.inside_transport_id(), Some(reactor));
    assert!(released.lifecycle.in_limbo());
    assert_eq!(released.health.current(), health_before);
    assert_eq!(sim.scenario_rng.logical_state(), expected_state);
    assert!(!sim.power_states[&owner].power_recheck_for_test());
}

/// `Mission_Unload @ 0x0044D92D` scans from the Building's Location cell
/// (`vt+0x1B8`), not the foundation-center `GetCoords` used by the sound.
/// This 2x2 fixture leaves only the Location's north exit and its strict
/// beyond cell open; those cells are outside every strict exit pair around
/// the foundation center.
#[test]
fn a_bio_reactor_scan_uses_location_cell_on_a_multi_cell_foundation() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    absorb(&mut sim, &rules, reactor, 1);
    let passenger = entity(&sim, reactor)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()[0];
    let base = (16u16, 10u16);
    let center = {
        let coords = crate::sim::movement::ground_pose::object_get_coords(
            entity(&sim, reactor),
            sim.resolved_terrain.as_ref(),
        );
        (
            crate::util::lepton::lepton_to_cell(coords.x) as u16,
            crate::util::lepton::lepton_to_cell(coords.y) as u16,
        )
    };
    assert_ne!(
        center, base,
        "the 2x2 foundation has a distinct center cell"
    );
    let directions = crate::util::direction::DIRECTION_DELTAS;
    let set_foot = |sim: &mut Simulation, cell: (u16, u16), cost| {
        let terrain = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        terrain.speed_costs.foot = Some(cost);
        terrain.base_speed_costs.foot = Some(cost);
    };
    let step = |cell: (u16, u16), delta: (i32, i32), scale: i32| {
        (
            (i32::from(cell.0) + delta.0 * scale) as u16,
            (i32::from(cell.1) + delta.1 * scale) as u16,
        )
    };
    for origin in [base, center] {
        for delta in directions {
            set_foot(&mut sim, step(origin, delta, 1), 0);
            set_foot(&mut sim, step(origin, delta, 2), 0);
        }
    }
    let north = (base.0, base.1 - 1);
    let beyond_north = (base.0, base.1 - 2);
    let center_scan = directions
        .into_iter()
        .flat_map(|delta| [step(center, delta, 1), step(center, delta, 2)])
        .collect::<Vec<_>>();
    assert!(!center_scan.contains(&north) && !center_scan.contains(&beyond_north));
    set_foot(&mut sim, north, 100);
    set_foot(&mut sim, beyond_north, 100);

    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .body_facing
        .set(0x7000, sim.session.binary_frame());
    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(
        entity(&sim, reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        (
            entity(&sim, passenger).position.rx(),
            entity(&sim, passenger).position.ry()
        ),
        north,
        "the only eligible exit is the native Location-cell north neighbor"
    );
}

/// The native Infantry +0x1AC receiver gets Building 5F5F00's height. A
/// two-level step blocks the first strict direction even though its foot
/// terrain cost is open, so the next flat strict pair is selected.
#[test]
fn a_bio_reactor_scan_uses_infantry_cell_entry_with_building_height() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    absorb(&mut sim, &rules, reactor, 1);
    let passenger = entity(&sim, reactor)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()[0];
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .body_facing
        .set(0, sim.session.binary_frame());
    for cell in [(16, 11), (16, 12)] {
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap()
            .level = 2;
    }

    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        (
            entity(&sim, passenger).position.rx(),
            entity(&sim, passenger).position.ry()
        ),
        (15, 11),
        "the south pair fails Infantry51BF90's height check; southwest is flat"
    );
}

/// Every strict beyond cell is refused, so the original loop falls through
/// to its relaxed pass at index 1. With facing 0x7000 that wraps from octant 7
/// to north (octant 0), which accepts the adjacent cell despite its blocked
/// beyond cell.
#[test]
fn a_bio_reactor_relaxed_scan_wraps_from_octant_seven() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    absorb(&mut sim, &rules, reactor, 1);
    let passenger = entity(&sim, reactor)
        .passenger_role
        .cargo()
        .unwrap()
        .passengers()[0];
    let base = (16u16, 10u16);
    let directions = crate::util::direction::DIRECTION_DELTAS;
    let step = |delta: (i32, i32), scale: i32| {
        (
            (i32::from(base.0) + delta.0 * scale) as u16,
            (i32::from(base.1) + delta.1 * scale) as u16,
        )
    };
    let set_foot = |sim: &mut Simulation, cell: (u16, u16), cost| {
        let terrain = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(cell.0, cell.1)
            .unwrap();
        terrain.speed_costs.foot = Some(cost);
        terrain.base_speed_costs.foot = Some(cost);
    };
    for delta in directions {
        set_foot(&mut sim, step(delta, 1), 0);
        set_foot(&mut sim, step(delta, 2), 0);
    }
    let north = (16, 9);
    set_foot(&mut sim, north, 100);
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .body_facing
        .set(0x7000, sim.session.binary_frame());
    let mut expected_rng = sim.scenario_rng.clone();
    assert!(
        crate::sim::movement::bump_crush::pick_infantry_sublocation(
            sim.resolved_terrain.as_ref(),
            &sim.substrate.raw_cell_occupation,
            &sim.substrate.occupancy,
            &sim.substrate.entities,
            Some(&rules),
            &sim.interner,
            &mut expected_rng,
            crate::sim::components::DriveCoord::cell(north.0, north.1, 0),
        )
        .is_some()
    );
    expected_rng.next_range_u32_inclusive(0, 2);

    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(
        entity(&sim, reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        (
            entity(&sim, passenger).position.rx(),
            entity(&sim, passenger).position.ry()
        ),
        north
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

/// `IsControlledByHuman` also admits campaign PlayerControl houses, but the
/// native Unload tail tests the distinct House+0x1EC `IsHuman` byte. A
/// PlayerControl-only owner therefore retains the absorbed passenger's
/// `ShouldEnterAbsorber` latch after a successful reveal.
#[test]
fn a_player_control_only_bio_reactor_owner_does_not_clear_the_seek_latch() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let owner = sim.interner.get("YuriCountry").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_is_human_for_test(false);
    house.set_player_control_for_test(true);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    assert!(sim.owner_is_human(owner));
    assert!(!sim.houses[&owner].is_human());

    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let passenger = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    assert!(seek(&mut sim, &rules, passenger));
    assert!(run_until(&mut sim, &rules, &grid, 300, |sim| {
        entity(sim, passenger).passenger_role.inside_transport_id() == Some(reactor)
    }));
    assert!(
        entity(&sim, passenger)
            .building_entry
            .should_enter_absorber()
    );

    crate::sim::transport_unload::building_mission_unload_absorber(
        &mut sim,
        &rules,
        reactor,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(!entity(&sim, passenger).lifecycle.in_limbo());
    assert!(
        entity(&sim, passenger)
            .building_entry
            .should_enter_absorber()
    );
}

/// A real Bio Reactor boarding first removes House+0x2F4 and clears the
/// once-counted latch. Both native successful-exit owners—SpawnSurvivors and
/// Selling—must restore that total exactly once while leaving the per-type
/// tracking count unchanged.
#[test]
fn absorbed_infantry_tracking_is_restored_after_building_death_and_sale() {
    for selling in [false, true] {
        let mut rules = rules();
        if selling {
            rules.set_buildup_control_for_test("YAPOWR", [0, 25, 2]);
        }
        let (mut sim, grid) = arena(&rules);
        let owner = sim.interner.get("YuriCountry").unwrap();
        let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
        let passenger = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
        assert!(seek(&mut sim, &rules, passenger));
        assert!(run_until(&mut sim, &rules, &grid, 300, |sim| {
            entity(sim, passenger).passenger_role.inside_transport_id() == Some(reactor)
        }));
        let infantry_type = entity(&sim, passenger).type_ref();
        assert_eq!(sim.houses[&owner].tracking().infantry(), 0);
        assert_eq!(
            sim.houses[&owner]
                .tracking()
                .owned_count(EntityCategory::Infantry, infantry_type),
            1
        );
        assert!(
            !entity(&sim, passenger)
                .house_tracking_latches()
                .is_counted()
        );
        assert!(
            entity(&sim, passenger)
                .house_tracking_latches()
                .is_absorbed()
        );

        if selling {
            assert!(crate::sim::production::sell_building_now_for_test(
                &mut sim, &rules, reactor
            ));
        } else {
            sim.spawn_building_survivors(
                &rules,
                None,
                reactor,
                |_, _| {},
                crate::sim::world::FrameEffects::default(),
            );
        }

        let released = entity(&sim, passenger);
        assert!(!released.lifecycle.in_limbo());
        assert!(released.lifecycle.cell_marked());
        assert!(released.house_tracking_latches().is_counted());
        assert!(!released.house_tracking_latches().is_absorbed());
        assert_eq!(sim.houses[&owner].tracking().infantry(), 1);
        assert_eq!(
            sim.houses[&owner]
                .tracking()
                .owned_count(EntityCategory::Infantry, infantry_type),
            1
        );
    }
}

/// Native unload dirties power only when the BuildingType's `ExtraPower`
/// field (`+0xEE8`) is positive (`0x0044DC55..0x0044DC75`).
#[test]
fn a_bio_reactor_unload_only_dirties_power_for_positive_extra_power() {
    for extra_power in [0, -1] {
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            &RULES.replace("ExtraPower=100", &format!("ExtraPower={extra_power}")),
        );
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART))
            .expect("Bio Reactor rules parse");
        let (mut sim, _) = arena(&rules);
        let owner = sim.interner.get("YuriCountry").unwrap();
        let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
        absorb(&mut sim, &rules, reactor, 1);
        sim.power_states.map_mut_for_test().insert(
            owner,
            crate::sim::power_system::PowerState::settled_for_test(),
        );

        crate::sim::transport_unload::building_mission_unload_absorber(
            &mut sim,
            &rules,
            reactor,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert!(
            entity(&sim, reactor)
                .passenger_role
                .cargo()
                .unwrap()
                .is_empty(),
            "ExtraPower={extra_power} still unloads its passenger"
        );
        assert!(
            !sim.power_states[&owner].power_recheck_for_test(),
            "ExtraPower={extra_power} does not dirty the house power cache"
        );
    }
}

/// A reactor destroyed while the Foot walks to it: its contact goes with
/// the reactor, Mission_Enter's no-target arm idles it (`0x004D9425..
/// 0x004D9466`), and its Guard head seeks again, finds no taker and clears
/// the latch, so a team may recruit it again.
#[test]
fn a_seeker_whose_reactor_dies_gives_up_and_can_be_recruited() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 24, 10);
    let infantry = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 10);
    let _team = seek_team(&mut sim, 62, vec![infantry]);
    assert!(run_until(&mut sim, &rules, &grid, 30, |sim| {
        entity(sim, infantry).radio_contacts.slot(0) == Some(reactor)
    }));

    sim.despawn_entity(reactor);

    assert!(run_until(&mut sim, &rules, &grid, 300, |sim| {
        !entity(sim, infantry).building_entry.seeking()
    }));
    let foot = entity(&sim, infantry);
    assert!(foot.passenger_role.is_none());
    assert_ne!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Enter)
    );
    let team = seek_team(&mut sim, 62, Vec::new());
    assert!(sim.team_can_add(team, infantry, true, &rules).is_some());
}

/// Two seekers for a reactor's last slot: the first HELLOs it and holds its
/// one radio slot (`max(NumberOfDocks, 1)`), so the Infantry setter parks
/// the other as its pending entry (`0x0051AC6D..0x0051ACC5`). Once the
/// first is in, the full reactor answers the other's CAN_LOAD NEGATORY
/// (`TryEnterTransport @ 0x0070D825`), which drops the entry, and its Guard
/// head finds no taker and clears the latch.
#[test]
fn a_seeker_beaten_to_the_last_slot_gives_up() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    absorb(&mut sim, &rules, reactor, 4);
    let near = spawn(&mut sim, &rules, "INIT", "YuriCountry", 12, 10);
    let far = spawn(&mut sim, &rules, "INIT", "YuriCountry", 8, 10);
    let _team = seek_team(&mut sim, 62, vec![near, far]);
    assert!(run_until(&mut sim, &rules, &grid, 30, |sim| {
        [near, far]
            .iter()
            .all(|&id| entity(sim, id).building_entry.seeking())
    }));

    assert!(run_until(&mut sim, &rules, &grid, 900, |sim| {
        entity(sim, near).passenger_role.inside_transport_id() == Some(reactor)
            && !entity(sim, far).building_entry.seeking()
    }));
    assert_eq!(
        entity(&sim, reactor)
            .passenger_role
            .cargo()
            .unwrap()
            .count(),
        5
    );
    let foot = entity(&sim, far);
    assert!(foot.passenger_role.is_none());
    assert_ne!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Enter)
    );
}

/// The cell coordinate centre of `(rx, ry)`.
fn cell_centre(rx: u16, ry: u16) -> crate::sim::components::DriveCoord {
    crate::sim::components::DriveCoord {
        x: i32::from(rx) * 256 + 128,
        y: i32::from(ry) * 256 + 128,
        z: 0,
    }
}

/// A Bio Reactor answers DOCK_NOW ROGER whatever its room
/// (`0x0043C710..0x0043C72C`), and Infantry PerCellProcess's building arm
/// asks nothing more (`0x0051A298..0x0051A2A7`): a contact ordered in while
/// it had room goes in on arrival, past `Passengers=`.
#[test]
fn a_full_bio_reactor_takes_an_arriving_contact() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let foot = spawn(&mut sim, &rules, "INIT", "YuriCountry", 13, 10);
    sim.substrate
        .mission_assign_exact(foot, MissionId::from_known(MissionType::Enter), 0)
        .unwrap();
    let _ = sim.assign_destination_represented(
        foot,
        Some(NavTargetRef::object(reactor)),
        Some(&rules),
        None,
        FrameEffects::default(),
    );
    assert_eq!(entity(&sim, foot).radio_contacts.slot(0), Some(reactor));
    absorb(&mut sim, &rules, reactor, 5);
    sim.foot_set_location_marked(
        foot,
        cell_centre(16, 10),
        Some(&rules),
        None,
        FrameEffects::default(),
    );

    sim.per_cell_process(
        foot,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();

    assert_eq!(
        entity(&sim, foot).passenger_role.inside_transport_id(),
        Some(reactor)
    );
    assert_eq!(occupants(&sim, reactor).len(), 6);
}

/// A unit's way into a `UnitAbsorb=` building: the Unit setter parks it as
/// the unit's pending entry (`0x00741CE5..0x00741D0D`); Foot's AI HELLOs it,
/// asks CAN_LOAD and takes it as destination (`TryEnterTransport @
/// 0x0070D7E0`); on its foundation, UnitClass::PerCellProcess's absorber
/// arm asks CAN_LOAD again and ROGER takes it in (`0x0073A237..
/// 0x0073A31C`).
#[test]
fn a_unit_absorber_takes_a_unit_arriving_on_its_foundation() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let absorber = spawn(&mut sim, &rules, "UABSORB", "YuriCountry", 16, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 12, 10);
    assert!(seek(&mut sim, &rules, tank));
    assert_eq!(entity(&sim, tank).pending_entry(), Some(absorber));

    crate::sim::docking::building_dock::try_pending_entry(
        &mut sim,
        &rules,
        tank,
        FrameEffects::default(),
    );
    assert_eq!(entity(&sim, tank).pending_entry(), None);
    assert_eq!(entity(&sim, tank).radio_contacts.slot(0), Some(absorber));
    assert_eq!(
        entity(&sim, tank).navigation.nav_com(),
        Some(NavTargetRef::object(absorber))
    );
    // Queue_Mission(Enter, 1) commenced it (`0x0070D83C`).
    assert_eq!(
        entity(&sim, tank).mission.effective(),
        MissionId::from_known(MissionType::Enter)
    );
    sim.foot_set_location_marked(
        tank,
        cell_centre(16, 10),
        Some(&rules),
        None,
        FrameEffects::default(),
    );

    sim.per_cell_process(
        tank,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();

    assert_eq!(
        entity(&sim, tank).passenger_role.inside_transport_id(),
        Some(absorber)
    );
    assert_eq!(occupants(&sim, absorber), [tank]);
}

/// #1045: an infantryman ordered into a Bio Reactor and retasked to Move
/// while his step onto its foundation is paid finishes that step, but
/// InfantryClass::PerCellProcess enters only on Enter (`0x0051A21F`), so he
/// walks on to the Move goal and the reactor keeps no claim on him.
#[test]
fn a_foot_retasked_to_move_stays_out_of_the_old_absorber() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 16, 10);
    let foot = spawn(&mut sim, &rules, "INIT", "Americans", 10, 10);
    let on_foundation = |rx: i32, ry: i32| (16..18).contains(&rx) && (10..12).contains(&ry);
    let cell = |sim: &Simulation| {
        let foot = entity(sim, foot);
        (i32::from(foot.position.rx()), i32::from(foot.position.ry()))
    };
    assert!(sim.apply_command(
        "Americans",
        &Command::clicked_mission(
            foot,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(reactor)
        ),
        Some(&rules),
    ));
    let stepping_on = |sim: &Simulation| {
        let (rx, ry) = cell(sim);
        !on_foundation(rx, ry)
            && entity(sim, foot)
                .locomotor
                .as_ref()
                .and_then(|loco| loco.step_head())
                .is_some_and(|head| on_foundation(head.x / 256, head.y / 256))
    };
    assert!(
        run_until(&mut sim, &rules, &grid, 300, stepping_on),
        "his step onto the foundation"
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: foot,
            target_rx: 4,
            target_ry: 10,
        },
        Some(&rules),
    ));
    let mut crossed = false;
    for _ in 0..300 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
        assert!(
            !entity(&sim, foot).passenger_role.is_inside_transport(),
            "he went into the old absorber"
        );
        let (rx, ry) = cell(&sim);
        crossed |= on_foundation(rx, ry);
    }
    assert!(crossed, "the paid step carried him onto the foundation");
    assert!(occupants(&sim, reactor).is_empty());
    assert!(cell(&sim).0 < 16, "he walked on toward the Move goal");
}

/// An order into an absorber is admitted by the CAN_LOAD the click sends
/// (`InfantryClass::What_Action_OnObject`, `0x0051EE1A`): the building
/// (`0x0043C2D0`) refuses a class it does not absorb (`0x0043C43B..
/// 0x0043C4F2`) and asks alliance, not ownership.
#[test]
fn an_order_into_an_absorber_is_admitted_by_its_can_load() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "YuriCountry", 16, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    let guest = spawn(&mut sim, &rules, "INIT", "Americans", 10, 10);
    let admits =
        |sim: &Simulation, id| crate::sim::passenger::can_enter_transport(sim, &rules, id, reactor);
    assert!(!admits(&sim, tank), "a vehicle into an infantry absorber");
    assert!(!admits(&sim, guest), "an enemy house's infantryman");
    for (house, ally) in [("YURICOUNTRY", "AMERICANS"), ("AMERICANS", "YURICOUNTRY")] {
        sim.house_alliances_mut_for_test()
            .entry(house.to_string())
            .or_default()
            .insert(ally.to_string());
    }
    assert!(admits(&sim, guest), "an allied house's infantryman");
}

/// Retail rules (every layer, gamemd-exact): the Bio Reactor takes a Yuri
/// Initiate (`Size=1` within `SizeLimit=15`) and refuses a Lasher (no
/// `UnitAbsorb=`).
#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_retail_bio_reactor_takes_infantry_only() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let reactor = spawn(&mut sim, rules, "YAPOWR", "YuriCountry", 16, 10);
    let initiate = spawn(&mut sim, rules, "INIT", "YuriCountry", 10, 10);
    let lasher = spawn(&mut sim, rules, "LTNK", "YuriCountry", 10, 12);

    assert_eq!(
        can_load(&mut sim, rules, initiate, reactor),
        RadioResponse::Roger
    );
    assert_eq!(
        can_load(&mut sim, rules, lasher, reactor),
        RadioResponse::Negatory
    );
}

/// House+0xE0 holds only a buildable occupiable (`TechLevel=` above -1); of
/// two the nearest takes the infantryman, which leaves on Capture toward it
/// with the latch set. A vehicle (NULL to CanBeOccupiedBy), a full bunker
/// and a red bunker are refused, which clears the latch.
#[test]
fn the_nearest_own_bunker_with_room_takes_an_occupier() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let owner = sim.interner.intern("YuriCountry");
    let far = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 22, 10);
    let near = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 16, 10);
    let civilian = spawn(&mut sim, &rules, "CAGAS01", "YuriCountry", 12, 14);
    assert_eq!(sim.house_occupiables(owner, &rules), [far, near]);
    let _ = civilian;
    let infantry = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    set_should_enter_occupiable_for_test(sim.substrate.entities.get_mut(tank).unwrap(), true);

    assert!(garrison_seek(&mut sim, &rules, infantry));
    assert!(!garrison_seek(&mut sim, &rules, tank));

    let foot = entity(&sim, infantry);
    assert!(foot.building_entry.should_enter_occupiable());
    assert_eq!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Capture)
    );
    assert!(matches!(
        foot.navigation.nav_com(),
        Some(NavTargetRef::Object { id }) if id == near
    ));
    assert!(!entity(&sim, tank).building_entry.seeking());

    // Full, then red: neither admits, and the latch clears.
    for id in 1000..1005 {
        sim.substrate
            .entities
            .get_mut(near)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap()
            .add_occupant(id, 1.0);
    }
    sim.substrate
        .entities
        .get_mut(far)
        .unwrap()
        .health
        .set_for_test(100);
    assert!(!garrison_seek(&mut sim, &rules, infantry));
    assert!(!entity(&sim, infantry).building_entry.seeking());
}

/// Script action 63 (`0x006E9EC8`) through the team pass: the occupier a
/// bunker admits leaves the team, the vehicle stays, and the action is
/// finished.
#[test]
fn script_action_63_sends_the_members_a_bunker_admits() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let _bunker = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 18, 10);
    let infantry = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    let tank = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    let team = seek_team(&mut sim, 63, vec![infantry, tank]);

    sim.run_team_ai_pass(&rules, None, FrameEffects::default());

    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), [tank]);
    assert!(state.advance_pending());
    assert_eq!(sim.team_script_vm.team_for_member(infantry), None);
    assert!(
        entity(&sim, infantry)
            .building_entry
            .should_enter_occupiable()
    );
    assert!(!entity(&sim, tank).building_entry.seeking());
}

/// Occupants are kept in admission order (`Occupants.AddItem`,
/// `0x00522970..0x0052297C`); the first one in plays the garrison cues at
/// its own cell, and a human house's occupant drops its occupy seek
/// (`0x005229F4..0x005229FA`) where a computer's keeps it.
#[test]
fn occupants_are_admitted_in_order_and_a_human_drops_its_seek() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "NABNKR", "Americans", 16, 10);
    let first = spawn(&mut sim, &rules, "E1", "Americans", 14, 10);
    let second = spawn(&mut sim, &rules, "E1", "YuriCountry", 14, 12);
    for id in [first, second] {
        set_should_enter_occupiable_for_test(sim.substrate.entities.get_mut(id).unwrap(), true);
        set_should_enter_nearest_occupiable_for_test(
            sim.substrate.entities.get_mut(id).unwrap(),
            true,
        );
    }
    sim.sound_events.clear_for_test();

    for id in [first, second] {
        assert!(crate::sim::passenger::add_garrison_occupant(
            &mut sim,
            &rules,
            id,
            bunker,
            None,
            FrameEffects::default(),
        ));
    }

    assert_eq!(occupants(&sim, bunker), [first, second]);
    assert!(!entity(&sim, first).building_entry.seeking());
    assert!(
        entity(&sim, second)
            .building_entry
            .should_enter_occupiable()
    );
    assert!(
        entity(&sim, second)
            .building_entry
            .should_enter_nearest_occupiable()
    );
    let americans = sim.interner.intern("Americans");
    let cues: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match *event {
            crate::sim::world::SimSoundEvent::StructureGarrisoned { owner } => Some((owner, None)),
            crate::sim::world::SimSoundEvent::BuildingGarrisonedSfx { owner, rx, ry } => {
                Some((owner, Some((rx, ry))))
            }
            _ => None,
        })
        .collect();
    assert_eq!(cues, [(americans, None), (americans, Some((14, 10)))]);
}

/// Two occupiers of one team garrison in arrival order (`Occupants.AddItem`,
/// `0x00522970..0x0052297C`), so the bunker fires first through the first
/// one in (`+0x688[+0x69C]`, `0x00452742..0x00452759`), then the second.
#[test]
fn two_occupiers_garrison_in_arrival_order_and_fire_in_that_order() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 16, 10);
    let near = spawn(&mut sim, &rules, "E1", "YuriCountry", 13, 10);
    let far = spawn(&mut sim, &rules, "E1", "YuriCountry", 8, 12);
    let _team = seek_team(&mut sim, 63, vec![far, near]);

    assert!(run_until(&mut sim, &rules, &grid, 900, |sim| {
        occupants(sim, bunker).len() == 2
    }));

    assert_eq!(occupants(&sim, bunker), [near, far]);
    let object = rules.object("NABNKR").unwrap();
    for (index, expected) in [(0, near), (1, far)] {
        let building = sim.substrate.entities.get_mut(bunker).unwrap();
        building
            .passenger_role
            .cargo_mut()
            .unwrap()
            .set_garrison_fire_index_for_test(index);
        assert_eq!(
            crate::sim::combat::combat_weapon::firing_occupant_id(entity(&sim, bunker), object),
            Some(expected)
        );
    }
}

/// A bunker filled while the occupier walks to it: the occupier arm asks
/// CanBeOccupiedBy again on arrival, drops the destination and scatters
/// (`0x005196DD..0x00519700`); Mission_Capture then idles it, and its Guard
/// head finds no bunker with room and clears the latch.
#[test]
fn an_occupier_turned_away_at_a_full_bunker_gives_up() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 16, 10);
    let infantry = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    let _team = seek_team(&mut sim, 63, vec![infantry]);
    assert!(run_until(&mut sim, &rules, &grid, 30, |sim| {
        entity(sim, infantry)
            .building_entry
            .should_enter_occupiable()
    }));
    for id in 1000..1005 {
        sim.substrate
            .entities
            .get_mut(bunker)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap()
            .add_occupant(id, 1.0);
    }

    assert!(run_until(&mut sim, &rules, &grid, 900, |sim| {
        !entity(sim, infantry).building_entry.seeking()
    }));
    let foot = entity(&sim, infantry);
    assert!(!foot.lifecycle.in_limbo());
    assert!(foot.passenger_role.is_none());
    assert_ne!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Capture)
    );
}

/// SellBuilding's eject drops a human house's occupant's occupy seek once
/// it is back on the map (`0x004580CB..0x004580E2`); a computer's occupant
/// keeps it.
#[test]
fn an_ejected_human_occupant_drops_its_seek() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let mut ejected = Vec::new();
    for (owner, ry) in [("Americans", 10), ("YuriCountry", 16)] {
        let bunker = spawn(&mut sim, &rules, "NABNKR", owner, 16, ry);
        let occupant = spawn(&mut sim, &rules, "E1", owner, 12, ry);
        assert!(crate::sim::passenger::add_garrison_occupant(
            &mut sim,
            &rules,
            occupant,
            bunker,
            None,
            FrameEffects::default(),
        ));
        set_should_enter_occupiable_for_test(
            sim.substrate.entities.get_mut(occupant).unwrap(),
            true,
        );
        set_should_enter_nearest_occupiable_for_test(
            sim.substrate.entities.get_mut(occupant).unwrap(),
            true,
        );

        let out = crate::sim::passenger::garrison_eject::sell_building_occupants(
            &mut sim,
            &rules,
            None,
            bunker,
            crate::sim::passenger::garrison_eject::GarrisonEjectMode::Release,
            FrameEffects::default(),
        );

        assert_eq!(out, 1);
        ejected.push(occupant);
    }

    assert!(!entity(&sim, ejected[0]).building_entry.seeking());
    assert!(
        entity(&sim, ejected[1])
            .building_entry
            .should_enter_occupiable()
    );
    assert!(
        entity(&sim, ejected[1])
            .building_entry
            .should_enter_nearest_occupiable()
    );
}

/// The Guard head with `+0x690` set (`0x004D508A..0x004D509E`): the seek
/// commences Capture, and the handler answers Capture's `ftol(Rate × 900)`
/// (`0x005B3A00` reads the current mission), not Guard's, with no draw.
#[test]
fn the_guard_head_garrison_seek_answers_captures_rate() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let _bunker = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 16, 10);
    let infantry = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    set_should_enter_occupiable_for_test(sim.substrate.entities.get_mut(infantry).unwrap(), true);
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(infantry, MissionId::from_known(MissionType::Guard), now)
        .unwrap();
    let before_rng = sim.scenario_rng.logical_state();

    sim.object_ai_visit_one(
        infantry,
        Some(&rules),
        crate::sim::world::ObjectAiCtx::default(),
    );

    let foot = entity(&sim, infantry);
    assert_eq!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Capture)
    );
    assert_eq!(
        foot.mission.dispatch_timer(),
        crate::sim::mission::timer::MissionDispatchTimer::from_raw(now as i32, 14)
    );
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);
}

/// Retail rules (every layer, gamemd-exact): the Battle Bunker is the
/// house's occupiable building and admits a Conscript; a civilian building
/// the house owns (`TechLevel=-1`) is not on the list, and a Rhino
/// (NULL to CanBeOccupiedBy) is refused.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_retail_battle_bunker_admits_a_conscript() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let owner = sim.interner.intern("YuriCountry");
    let bunker = spawn(&mut sim, rules, "NABNKR", "YuriCountry", 16, 10);
    let _civilian = spawn(&mut sim, rules, "CAGAS01", "YuriCountry", 22, 16);
    let conscript = spawn(&mut sim, rules, "E2", "YuriCountry", 10, 10);
    let rhino = spawn(&mut sim, rules, "HTNK", "YuriCountry", 10, 13);

    assert_eq!(sim.house_occupiables(owner, rules), [bunker]);
    assert!(garrison_seek(&mut sim, rules, conscript));
    assert!(!garrison_seek(&mut sim, rules, rhino));
}

/// EnterTankBunker over its house's tank bunkers (House+0xC8): one held by
/// its occupant (`+0x2E4`), one held by an approaching unit's contact, one with
/// another radio contact and another house's bunker all stand nearer than
/// the free bunker that takes the tank. The tank commences Enter, the
/// bunker holds it as its first radio contact, and its object destination
/// selects the native direction-dependent docking point.
#[test]
fn the_nearest_free_own_tank_bunker_takes_the_tank() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let owner = sim.interner.intern("YuriCountry");
    let far = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 24, 10);
    let free = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 18, 10);
    let occupied = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 13, 6);
    let claimed = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 13, 14);
    let contacted = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 6, 13);
    let _enemy = spawn(&mut sim, &rules, "NATBNK", "Americans", 6, 7);
    assert_eq!(
        sim.house_bunkers(owner, &rules),
        [far, free, occupied, claimed, contacted]
    );
    let occupant = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 2, 2);
    sim.substrate
        .entities
        .get_mut(occupied)
        .unwrap()
        .bunker
        .set_bunker_occupant_for_test(Some(occupant));
    let claimer = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 14, 18);
    assert!(tank_bunker_seek(&mut sim, &rules, claimer));
    assert_eq!(
        install_state(&sim, claimed),
        (BunkerState::Idle, Some(claimer))
    );
    let caller = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 4, 16);
    assert_eq!(
        transmit(
            &mut sim,
            caller,
            contacted,
            RadioMessage::Hello,
            RadioPayload::default(),
            Some(&rules),
            FrameEffects::default(),
        ),
        RadioResponse::Roger
    );
    let tank = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 10, 10);

    assert!(tank_bunker_seek(&mut sim, &rules, tank));

    let foot = entity(&sim, tank);
    assert_eq!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Enter)
    );
    assert_eq!(foot.radio_contacts.slot(0), Some(free));
    assert_eq!(foot.navigation.nav_com(), Some(NavTargetRef::object(free)));
    assert_eq!(install_state(&sim, free), (BunkerState::Idle, Some(tank)));
    assert_eq!(install_state(&sim, far), (BunkerState::Idle, None));
}

/// EnterTankBunker answers false and orders nothing when `0x0070FB50`
/// refuses the Foot (a `Bunkerable=no` Chaos Drone, this fixture's
/// turretless and unarmed `LTNK`, an infantryman) or when every bunker of
/// its house is held.
#[test]
fn a_refused_foot_or_a_held_tank_bunker_gets_no_order() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 16, 10);
    let drone = spawn(&mut sim, &rules, "CAOS", "YuriCountry", 10, 10);
    let unarmed = spawn(&mut sim, &rules, "LTNK", "YuriCountry", 10, 12);
    let infantry = spawn(&mut sim, &rules, "INIT", "YuriCountry", 10, 14);
    for id in [drone, unarmed, infantry] {
        let mission = entity(&sim, id).mission;
        assert!(!tank_bunker_seek(&mut sim, &rules, id));
        assert_eq!(entity(&sim, id).mission, mission);
        assert_eq!(entity(&sim, id).bunker.link(), BunkerLink::None);
        assert!(entity(&sim, id).radio_contacts.is_empty());
    }
    assert_eq!(install_state(&sim, bunker), (BunkerState::Idle, None));

    let occupant = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 2, 2);
    sim.substrate
        .entities
        .get_mut(bunker)
        .unwrap()
        .bunker
        .set_bunker_occupant_for_test(Some(occupant));
    let tank = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 12, 16);
    let mission = entity(&sim, tank).mission;

    assert!(!tank_bunker_seek(&mut sim, &rules, tank));

    let foot = entity(&sim, tank);
    assert_eq!(foot.mission, mission);
    assert_eq!(foot.navigation.nav_com(), None);
    assert_eq!(foot.bunker.link(), BunkerLink::None);
    assert!(foot.radio_contacts.is_empty());
}

/// Equidistant tank bunkers through the production walk: the house's list
/// is walked from its last and only a strictly nearer bunker replaces the
/// pick (`0x004E0009`, `JGE`), so the one built later takes the tank,
/// whichever side it stands.
#[test]
fn of_equidistant_tank_bunkers_the_later_built_takes_the_tank() {
    let rules = rules();
    for cells in [[(13, 8), (8, 13)], [(8, 13), (13, 8)]] {
        let (mut sim, _) = arena(&rules);
        let tank = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 10, 10);
        let bunkers =
            cells.map(|(rx, ry)| spawn(&mut sim, &rules, "NATBNK", "YuriCountry", rx, ry));
        let coords = |id| {
            let coords = object_get_coords(entity(&sim, id), sim.resolved_terrain.as_ref());
            [coords.x, coords.y, coords.z]
        };
        let [first, later] =
            bunkers.map(|bunker| object_distance(coords(tank), coords(bunker), None));
        assert_eq!(first, later, "the bunkers stand equally far");

        assert!(tank_bunker_seek(&mut sim, &rules, tank));

        assert_eq!(entity(&sim, tank).radio_contacts.slot(0), Some(bunkers[1]));
        assert_eq!(
            entity(&sim, tank).navigation.nav_com(),
            Some(NavTargetRef::object(bunkers[1]))
        );
    }
}

/// Script action 61 (`0x006E9E3E`) through the team pass: of two Gattling
/// Tanks and a Chaos Drone with one free bunker, the first tank in the
/// list takes it and leaves the team; the second, though nearer, finds it
/// held and the drone is refused, so both stay. The action is finished.
#[test]
fn script_action_61_sends_the_members_a_tank_bunker_takes() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let bunker = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 18, 10);
    let first = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 10, 10);
    let drone = spawn(&mut sim, &rules, "CAOS", "YuriCountry", 10, 12);
    let second = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 14, 12);
    let team = seek_team(&mut sim, 61, vec![first, drone, second]);

    sim.run_team_ai_pass(&rules, None, FrameEffects::default());

    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), [drone, second]);
    assert!(state.advance_pending());
    assert_eq!(sim.team_script_vm.team_for_member(first), None);
    let foot = entity(&sim, first);
    assert_eq!(foot.radio_contacts.slot(0), Some(bunker));
    assert_eq!(
        foot.navigation.nav_com(),
        Some(NavTargetRef::object(bunker))
    );
    assert_eq!(
        foot.mission.current(),
        MissionId::from_known(MissionType::Enter)
    );
    assert_eq!(entity(&sim, second).bunker.link(), BunkerLink::None);
    assert!(entity(&sim, second).radio_contacts.is_empty());
}

#[test]
fn cancelling_a_manual_bunker_entry_frees_it_for_another_tank() {
    let rules = rules();
    for stop in [true, false] {
        let (mut sim, grid) = arena(&rules);
        let bunker = spawn(&mut sim, &rules, "NATBNK", "Americans", 16, 10);
        let first = spawn(&mut sim, &rules, "YTNK", "Americans", 10, 10);
        let second = spawn(&mut sim, &rules, "YTNK", "Americans", 12, 14);
        assert!(sim.apply_command(
            "Americans",
            &Command::clicked_mission(
                first,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(bunker)
            ),
            Some(&rules)
        ));
        assert!(!sim.apply_command(
            "YuriCountry",
            &Command::Stop { entity_id: first },
            Some(&rules)
        ));
        assert_eq!(
            install_state(&sim, bunker),
            (BunkerState::Idle, Some(first))
        );
        assert!(run_until(&mut sim, &rules, &grid, 30, |sim| {
            entity(sim, first).mission.current().known() == Some(MissionType::Enter)
        }));
        let cancel = if stop {
            Command::Stop { entity_id: first }
        } else {
            Command::Move {
                entity_id: first,
                target_rx: 10,
                target_ry: 18,
            }
        };
        assert!(sim.apply_command("Americans", &cancel, Some(&rules)));
        assert!(entity(&sim, first).radio_contacts.is_empty());
        assert_eq!(install_state(&sim, bunker), (BunkerState::Idle, None));
        assert!(
            sim.apply_command(
                "Americans",
                &Command::clicked_mission(
                    second,
                    crate::sim::mission::MissionType::Enter,
                    crate::sim::combat::TargetKind::Entity(bunker)
                ),
                Some(&rules)
            ),
            "after {cancel:?}, the empty bunker must accept the second tank"
        );
        assert_eq!(entity(&sim, first).bunker.link(), BunkerLink::None);
        assert!(run_until(&mut sim, &rules, &grid, 900, |sim| {
            entity(sim, bunker).bunker.occupant() == Some(second)
        }));
        assert_eq!(
            entity(&sim, second).bunker.link(),
            BunkerLink::Installed(bunker)
        );

        // Once installed, normal order cancellation must not eject the tank.
        let installed_order = if stop {
            Command::Stop { entity_id: second }
        } else {
            Command::Move {
                entity_id: second,
                target_rx: 12,
                target_ry: 18,
            }
        };
        assert!(sim.apply_command("Americans", &installed_order, Some(&rules)));
        assert_eq!(entity(&sim, bunker).bunker.occupant(), Some(second));
        assert_eq!(
            entity(&sim, second).bunker.link(),
            BunkerLink::Installed(bunker)
        );
    }
}

#[test]
fn replacing_a_bunker_entry_releases_only_the_previous_bunker() {
    let rules = rules();
    for ai in [false, true] {
        let (mut sim, grid) = arena(&rules);
        let previous = spawn(&mut sim, &rules, "NATBNK", "Americans", 16, 10);
        let next = spawn(&mut sim, &rules, "NATBNK", "Americans", 24, 18);
        let first = spawn(&mut sim, &rules, "YTNK", "Americans", 10, 10);
        let second = spawn(&mut sim, &rules, "YTNK", "Americans", 12, 14);
        assert!(sim.apply_command(
            "Americans",
            &Command::clicked_mission(
                first,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(previous)
            ),
            Some(&rules)
        ));
        if ai {
            // The AI seek skips the held previous bunker and queues directly.
            assert!(tank_bunker_seek(&mut sim, &rules, first));
        } else {
            assert!(sim.apply_command(
                "Americans",
                &Command::clicked_mission(
                    first,
                    crate::sim::mission::MissionType::Enter,
                    crate::sim::combat::TargetKind::Entity(next)
                ),
                Some(&rules)
            ));
        }
        assert_eq!(entity(&sim, first).radio_contacts.slot(0), Some(next));
        assert_eq!(
            entity(&sim, first).navigation.nav_com(),
            Some(NavTargetRef::object(next))
        );
        assert_eq!(install_state(&sim, previous), (BunkerState::Idle, None));
        assert_eq!(install_state(&sim, next), (BunkerState::Idle, Some(first)));
        assert!(sim.apply_command(
            "Americans",
            &Command::clicked_mission(
                second,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(previous)
            ),
            Some(&rules)
        ));
        assert!(
            run_until(&mut sim, &rules, &grid, 900, |sim| {
                entity(sim, previous).bunker.occupant() == Some(second)
                    && entity(sim, next).bunker.occupant() == Some(first)
            }),
            "replacement via AI seek: {ai}"
        );
    }
}

#[test]
#[ignore = "slow; run when changing tank-bunker entry"]
fn retail_manual_tank_bunker_entry_installs_from_each_side() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    for kind in ["LTNK", "YTNK", "MTNK", "HTNK"] {
        for (rx, ry) in [(10, 16), (24, 16), (16, 10), (16, 24)] {
            let (mut sim, grid) = arena(rules);
            let bunker = spawn(&mut sim, rules, "NATBNK", "Americans", 16, 16);
            let tank = spawn(&mut sim, rules, kind, "Americans", rx, ry);
            assert!(
                sim.apply_command(
                    "Americans",
                    &Command::clicked_mission(
                        tank,
                        crate::sim::mission::MissionType::Enter,
                        crate::sim::combat::TargetKind::Entity(bunker)
                    ),
                    Some(rules),
                ),
                "{kind} at ({rx}, {ry}) admitted"
            );
            let installed = run_until(&mut sim, rules, &grid, 900, |sim| {
                entity(sim, bunker).bunker.occupant() == Some(tank)
            });
            let unit = entity(&sim, tank);
            assert!(
                installed,
                "{kind} from ({rx}, {ry}): cell {:?}, nav {:?}, mission {:?}, link {:?}, bunker {:?}",
                (unit.position.rx(), unit.position.ry()),
                unit.navigation.nav_com(),
                unit.mission,
                unit.bunker.link(),
                install_state(&sim, bunker)
            );
            assert_eq!(unit.bunker.link(), BunkerLink::Installed(bunker));
        }
    }
}

/// A bunker destroyed while a seeker drives to it, or a seeker destroyed on
/// its way: the despawn teardown drops the radio contact on the other
/// side, so once the object is deleted a save names nothing gone and loads.
#[test]
fn a_tank_bunker_approach_ends_with_either_side() {
    let rules = rules();
    for bunker_goes in [true, false] {
        let (mut sim, grid) = arena(&rules);
        let bunker = spawn(&mut sim, &rules, "NATBNK", "YuriCountry", 18, 10);
        let tank = spawn(&mut sim, &rules, "YTNK", "YuriCountry", 10, 10);
        assert!(tank_bunker_seek(&mut sim, &rules, tank));
        let gone = if bunker_goes { bunker } else { tank };

        sim.despawn_entity(gone);

        if bunker_goes {
            assert_eq!(entity(&sim, tank).bunker.link(), BunkerLink::None);
            assert!(entity(&sim, tank).radio_contacts.is_empty());
        } else {
            assert_eq!(install_state(&sim, bunker), (BunkerState::Idle, None));
        }
        assert!(run_until(&mut sim, &rules, &grid, 30, |sim| {
            !sim.substrate.entities.contains(gone)
        }));
        let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "Approach", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
    }
}

/// Retail rules (every layer, gamemd-exact): of the members the nine "Yuri
/// Unit Hunter" TeamTypes field, the Tank Bunker admits only the Lasher and
/// the Gattling Tank. The Mastermind has no `Turret=`, the Magnetron and
/// the Chaos Drone are `Bunkerable=no`, and infantry are not `Bunkerable=`
/// by default. The Lasher's seek takes the bunker.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_retail_tank_bunker_admits_the_lasher_and_the_gattling_tank() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, _) = arena(rules);
    let owner = sim.interner.intern("YuriCountry");
    let bunker = spawn(&mut sim, rules, "NATBNK", "YuriCountry", 20, 10);
    assert_eq!(sim.house_bunkers(owner, rules), [bunker]);
    let members = [
        "LTNK", "YTNK", "MIND", "TELE", "CAOS", "INIT", "YURI", "VIRUS",
    ];
    let mut admitted = Vec::new();
    for (column, kind) in (4..).step_by(2).zip(members) {
        let id = spawn(&mut sim, rules, kind, "YuriCountry", column, 16);
        if can_auto_deploy_here(&sim, id, rules) {
            admitted.push((kind, id));
        }
    }
    assert_eq!(
        admitted.iter().map(|&(kind, _)| kind).collect::<Vec<_>>(),
        ["LTNK", "YTNK"]
    );

    assert!(tank_bunker_seek(&mut sim, rules, admitted[0].1));
    assert_eq!(
        entity(&sim, admitted[0].1).radio_contacts.slot(0),
        Some(bunker)
    );
    assert_eq!(
        entity(&sim, admitted[0].1).navigation.nav_com(),
        Some(NavTargetRef::object(bunker))
    );
}

/// Nine original4DFE00 controls through the production seek. Ordinary
/// types supply the admission: CAGAS01 takes an occupier, GAPOWR refuses it.
#[test]
fn nearest_occupiable_production_matches_native_controls() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/team_nearest_occupiable.json",
    ))
    .unwrap();
    let rules = rules();
    for (number, row) in native.as_array().unwrap().iter().enumerate() {
        let (mut sim, _) = arena(&rules);
        let foot = spawn(
            &mut sim,
            &rules,
            if row["infantry"] == true {
                "E1"
            } else {
                "LTNK"
            },
            "YuriCountry",
            4,
            4,
        );
        let coord = xyz(&row["foot"]);
        {
            let entity = sim.substrate.entities.get_mut(foot).unwrap();
            entity.position.put_world_xy([coord[0], coord[1]]);
            entity.position.set_exact_z_leptons_for_test(Some(coord[2]));
        }
        let buildings: Vec<_> = row["bunkers"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let building = spawn(
                    &mut sim,
                    &rules,
                    if entry["answer"] == true {
                        "CAGAS01"
                    } else {
                        "GAPOWR"
                    },
                    "YuriCountry",
                    8 + 4 * index as u16,
                    20,
                );
                let coord = xyz(&entry["xyz"]);
                let entity = sim.substrate.entities.get_mut(building).unwrap();
                //1x1 makes Location the native fixture's substituted GetCoords.
                entity.type_facts.set_foundation_for_test("1x1".to_owned());
                entity.position.put_world_xy([coord[0], coord[1]]);
                entity.position.set_exact_z_leptons_for_test(Some(coord[2]));
                building
            })
            .collect();
        let now = sim.session.binary_frame();
        sim.substrate
            .mission_assign_exact(
                foot,
                MissionId::from_raw(row["mission"].as_i64().unwrap() as i32),
                now,
            )
            .unwrap();
        let nav = row["nav"].as_u64().map(|index| buildings[index as usize]);
        sim.substrate
            .entities
            .get_mut(foot)
            .unwrap()
            .navigation
            .set_nav_com_for_test(nav.map(NavTargetRef::object));
        set_should_enter_nearest_occupiable_for_test(
            sim.substrate.entities.get_mut(foot).unwrap(),
            row["latch"] == true,
        );

        let returned = whole_map_garrison_seek(&mut sim, &rules, foot);

        assert_eq!(returned, row["result"] == 1, "row {number}: result");
        let actor = entity(&sim, foot);
        assert_eq!(
            actor.building_entry.should_enter_nearest_occupiable(),
            row["latch_after"] == 1,
            "row {number}: retained691"
        );
        let requested = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "assign_destination")
            .map(|event| buildings[event[1].as_u64().unwrap() as usize]);
        assert_eq!(
            actor.navigation.nav_com(),
            requested.or(nav).map(NavTargetRef::object),
            "row {number}: destination"
        );
        if returned {
            assert_eq!(
                actor.mission.current(),
                MissionId::from_known(MissionType::Capture),
                "row {number}: mission"
            );
        }
    }
}

#[test]
fn script64_enters_a_neutral_civilian_building_outside_the_own_house_list() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&format!(
            "{RULES}\n[Countries]\n0=YuriCountry\n1=Americans\n2=Neutral\n\
             [Neutral]\nMultiplayPassive=yes\n"
        )),
        &IniFile::from_str(ART),
    )
    .unwrap();
    let (mut sim, grid) = arena(&rules);
    let neutral = sim.interner.intern("Neutral");
    let mut house = HouseState::new(neutral, 2, Some(neutral), false, 0, 10);
    house.set_multiplay_passive_for_test(crate::sim::house_state::resolve_multiplay_passive(
        Some(&rules),
        Some("Neutral"),
    ));
    assert!(house.multiplay_passive(), "production Country reader");
    sim.houses.register(house);
    let own = spawn(&mut sim, &rules, "NABNKR", "YuriCountry", 24, 10);
    let civilian = spawn(&mut sim, &rules, "CAGAS01", "Neutral", 16, 10);
    let occupier = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    let owner = entity(&sim, occupier).owner();
    assert_eq!(sim.house_occupiables(owner, &rules), [own]);
    let team = seek_team(&mut sim, 64, vec![occupier]);

    sim.run_team_ai_pass(&rules, None, FrameEffects::default());

    assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
    assert_eq!(sim.team_script_vm.team_for_member(occupier), None);
    assert_eq!(
        entity(&sim, occupier).navigation.nav_com(),
        Some(NavTargetRef::object(civilian))
    );
    assert!(
        entity(&sim, occupier)
            .building_entry
            .should_enter_nearest_occupiable()
    );
    assert!(run_until(&mut sim, &rules, &grid, 600, |sim| {
        entity(sim, occupier).passenger_role.inside_transport_id() == Some(civilian)
    }));
    assert!(
        entity(&sim, occupier)
            .building_entry
            .should_enter_nearest_occupiable(),
        "a computer occupant retains691 for ejection/retry"
    );
}

#[test]
fn nearest_occupiable_uses_native_playfield_instead_of_navigation_rectangle() {
    let rules = rules();
    let (mut sim, grid) = arena(&rules);
    let building = spawn(&mut sim, &rules, "CAGAS01", "YuriCountry", 16, 10);
    let occupier = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
    assert!(entity(&sim, building).position.rx() < grid.width());
    assert!(whole_map_garrison_seek(&mut sim, &rules, occupier));
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 1,
        off_fc: 0,
        off_100: 0,
        off_104: 1,
        off_108: 1,
    }));

    assert!(!whole_map_garrison_seek(&mut sim, &rules, occupier));
    assert!(
        !entity(&sim, occupier)
            .building_entry
            .should_enter_nearest_occupiable()
    );
}

#[test]
fn guard_and_area_guard_retry_script64_without_a_scenario_draw() {
    let rules = rules();
    for mission in [MissionType::Guard, MissionType::AreaGuard] {
        let (mut sim, _) = arena(&rules);
        let building = spawn(&mut sim, &rules, "CAGAS01", "YuriCountry", 16, 10);
        let occupier = spawn(&mut sim, &rules, "E1", "YuriCountry", 10, 10);
        set_should_enter_nearest_occupiable_for_test(
            sim.substrate.entities.get_mut(occupier).unwrap(),
            true,
        );
        let now = sim.session.binary_frame();
        sim.substrate
            .mission_assign_exact(occupier, MissionId::from_known(mission), now)
            .unwrap();
        let rng = sim.scenario_rng.logical_state();

        sim.object_ai_visit_one(
            occupier,
            Some(&rules),
            crate::sim::world::ObjectAiCtx::default(),
        );

        assert_eq!(
            entity(&sim, occupier).navigation.nav_com(),
            Some(NavTargetRef::object(building))
        );
        assert_eq!(
            entity(&sim, occupier).mission.current(),
            MissionId::from_known(MissionType::Capture)
        );
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

/// The selected campaign passes, not Battle mode or hand-written tuning:
/// RULESMD/LANGRULE, optional ALL01UMD.INI, then the map through the existing
/// production reader, and fixed ART for the actual infantry admission.
#[test]
fn retail_all01_script64_conscript_admits_a_map_civilian_garrison() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(
        assets.get_ref("ALL01UMD.MAP").expect("retail ALL01"),
    )
    .unwrap();
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .expect("present campaign INI parses")
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, _, fixed_art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    assert!(rules.object("E2").expect("retail Conscript").occupier);
    let civilian = map
        .entities
        .iter()
        .find(|entity| {
            entity.category == crate::map::entities::EntityCategory::Structure
                && entity.owner == "Neutral"
                && rules
                    .object(&entity.type_id)
                    .is_some_and(|object| object.can_be_occupied && object.tech_level == -1)
        })
        .expect("ALL01 places neutral civilian garrisons");
    let (mut sim, _) = arena(&rules);
    let neutral = sim.interner.intern("Neutral");
    let mut house = HouseState::new(neutral, 2, Some(neutral), false, 0, 10);
    house.set_multiplay_passive_for_test(crate::sim::house_state::resolve_multiplay_passive(
        Some(&rules),
        Some("Neutral"),
    ));
    assert!(
        house.multiplay_passive(),
        "retained campaign Country reader"
    );
    sim.houses.register(house);
    let building = spawn(&mut sim, &rules, &civilian.type_id, "Neutral", 16, 10);
    let conscript = spawn(&mut sim, &rules, "E2", "YuriCountry", 10, 10);

    assert!(whole_map_garrison_seek(&mut sim, &rules, conscript));
    assert_eq!(
        entity(&sim, conscript).navigation.nav_com(),
        Some(NavTargetRef::object(building))
    );
}
