//! The MegaMission ports against the retail bodies.
//!
//! `tools/spatial_oracle/foot_attack_move.json` executes Assign_Target_Command
//! (`0x004DF0E0`), Clear_All_TarCom (`0x004DF1A0`), HaveMegaMission
//! (`0x004DF1C0`), MegaMissionIsAttackMove (`0x004DF310`), ContinueMegaMission
//! (`0x004DF320`), RefreshMegaMission (`0x004DF4B0`) and the PointerExpired
//! slice (`0x004D9AC9..0x004D9B43`) for Unit and Infantry, with QueueMission,
//! the scan and the setters as recording stubs. Compared here are the rows
//! whose state a [`MegaMission`] can hold: a mission of NONE or 29, cell or
//! object destinations and targets, every present event tag resolving to a
//! token, and no scripted callback. Each compares the answer, the four
//! fields, the mission the body queues and the destination and target its
//! setters receive.
//!
//! Left out, each for its stated reason:
//! - `acquire` (Arrival_Target_Handler): its rows script the scan and the
//!   target predicate; it is tested through the object AI in `world_tests`
//!   and `unit_fire_turn_tests`;
//! - `convert_target` (`0x004DF280`): no retail caller, so no port;
//! - `saved_coordinate` (GetAttackMoveCoords `0x004DF1F0`, vt+0x4B8): only
//!   the radar's attack-move lines call it (`0x0063BC53`, `0x0063C4A2`);
//! - setup events holding a token a decoded event never carries: an
//!   unsupported tag (1), an invalid tag (255), or an event mission outside
//!   the mission table (-128, 127).

use super::MegaMission;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::world::{FrameEffects, Simulation};
use serde_json::Value;

const OWNER: u64 = 1;
// The corpus `pointers`, and what each stands for here: the two destinations
// are cells, the three targets objects 2..=4, which a destination may name too.
const DESTINATION_A: i64 = 0x2000_3000;
const DESTINATION_B: i64 = 0x2000_3100;
const TARGET_A: i64 = 0x2000_4000;
const TARGET_B: i64 = 0x2000_4100;
const TARGET_C: i64 = 0x2000_4200;
const CELL_A: (u16, u16) = (10, 12);
const CELL_B: (u16, u16) = (20, 22);
/// The object an expiring DESTINATION_A stands for.
const DESTINATION_OBJECT: u64 = 5;
/// An object no fixture holds.
const DELETED_OBJECT: u64 = 999;

/// A cell token's Cell.
fn cell(token: i64) -> Option<(u16, u16)> {
    match token {
        DESTINATION_A => Some(CELL_A),
        DESTINATION_B => Some(CELL_B),
        _ => None,
    }
}

/// A destination token: a Cell, one of the target objects, or none.
fn destination(token: i64) -> Option<Option<TargetKind>> {
    match token {
        0 => Some(None),
        DESTINATION_A | DESTINATION_B => cell(token).map(|(rx, ry)| Some(TargetKind::Cell(rx, ry))),
        TARGET_A | TARGET_B | TARGET_C => target(token),
        _ => None,
    }
}

/// A target token; `DESTINATION_B` is the cell the map lookup answers.
fn target(token: i64) -> Option<Option<TargetKind>> {
    match token {
        0 => Some(None),
        TARGET_A => Some(Some(TargetKind::Entity(2))),
        TARGET_B => Some(Some(TargetKind::Entity(3))),
        TARGET_C => Some(Some(TargetKind::Entity(4))),
        DESTINATION_B => Some(Some(TargetKind::Cell(CELL_B.0, CELL_B.1))),
        _ => None,
    }
}

fn int(value: &Value) -> i64 {
    value.as_i64().unwrap()
}

/// The MegaMission a corpus state stands for. `unread_mission` is the value
/// a body that never reads the saved mission keeps unchanged.
fn mega_mission(state: &Value, unread_mission: Option<i64>) -> Option<MegaMission> {
    let attack_move = match int(&state["saved_mission"]) {
        -1 => false,
        29 => true,
        unread if Some(unread) == unread_mission => false,
        _ => return None,
    };
    Some(MegaMission::for_test(
        attack_move,
        destination(int(&state["saved_destination"]))?,
        target(int(&state["saved_target"]))?,
        int(&state["engaged"]) != 0,
    ))
}

fn mission(raw: i64) -> Option<MissionId> {
    matches!(raw, -1 | 0 | 1 | 2 | 5).then(|| MissionId::from_raw(raw as i32))
}

fn rules() -> RuleSet {
    let mut ini = String::from("[VehicleTypes]\n");
    let mut sections = String::new();
    let mut infantry = String::from("[InfantryTypes]\n");
    for (index, (primary, prevent)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
        for (list, letter) in [(&mut ini, 'U'), (&mut infantry, 'I')] {
            let name = format!("{letter}{primary}{prevent}");
            list.push_str(&format!("{index}={name}\n"));
            sections.push_str(&format!("[{name}]\nStrength=100\n"));
            if primary == 1 {
                sections.push_str("Primary=Gun\n");
            }
            if prevent == 1 {
                sections.push_str("PreventAttackMove=yes\n");
            }
        }
    }
    ini.push_str(&infantry);
    ini.push_str(&sections);
    ini.push_str("[Gun]\nDamage=1\nROF=10\nRange=4\n");
    RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap()
}

/// The receiver as the row's state supplies it, with objects 2..=4 standing
/// for the target tokens. `None` when the state is not representable.
fn fixture(row: &Value, owner_type: &str, unread_mission: Option<i64>) -> Option<Simulation> {
    let input = &row["input"];
    let state = &input["state"];
    let category = match input["family"].as_str().unwrap() {
        "unit" => EntityCategory::Unit,
        "infantry" => EntityCategory::Infantry,
        other => panic!("family {other}"),
    };
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Owner");
    let enemy = sim.interner.intern("Enemy");
    let type_ref = sim.interner.intern(owner_type);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        OWNER,
        5,
        5,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        category,
        0,
        0,
        category != EntityCategory::Infantry,
    );
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: mission(int(&state["mission"]))?,
        suspended: MissionId::NONE,
        queued: mission(int(&state["queued_mission"]))?,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    // Queue_Mission's readiness reads the Locomotor's moving state.
    let mut locomotor = LocomotorState::for_test_kind(match category {
        EntityCategory::Unit => LocomotorKind::Drive,
        _ => LocomotorKind::Walk,
    });
    if category == EntityCategory::Unit {
        assert!(locomotor.install_drive_state_for_test(Some(Default::default())));
    }
    entity.locomotor = Some(locomotor);
    entity.attack_target =
        target(int(&state["current_target"]))?.map(|target| AttackTarget { target });
    entity.mega_mission = mega_mission(state, unread_mission)?;
    sim.substrate.entities.insert(entity);
    let target_type = sim.interner.intern("U00");
    for id in 2..=4 {
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                30 + id as u16,
                30,
                0,
                0,
                enemy,
                Health::new(100),
                target_type,
                EntityCategory::Unit,
                0,
                0,
                true,
            ));
    }
    Some(sim)
}

/// The mission the body handed QueueMission, if it did.
fn queued_mission(row: &Value) -> Option<MissionType> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["name"] == "queue_mission")
        .map(|event| {
            assert_eq!(event["args"][1], 1, "commence_now");
            MissionId::from_raw(int(&event["args"][0]) as i32)
                .known()
                .unwrap()
        })
}

/// Compare the receiver's fields and queue after the body ran.
fn compare_after(sim: &Simulation, row: &Value, unread_mission: Option<i64>, context: &str) {
    let expected = mega_mission(&row["after"], unread_mission)
        .unwrap_or_else(|| panic!("{context}: after state"));
    assert_eq!(sim.mega_mission(OWNER), expected, "{context}: fields");
    let state = &row["input"]["state"];
    let entity = sim.substrate.entities.get(OWNER).unwrap();
    let before = [
        mission(int(&state["mission"])).unwrap(),
        mission(int(&state["queued_mission"])).unwrap(),
    ];
    match queued_mission(row) {
        // Queue_Mission(m, 1) leaves m queued or, ready, current.
        Some(queued) => {
            let queued = MissionId::from_known(queued);
            if !before.contains(&queued) {
                assert!(
                    entity.mission.queued() == queued || entity.mission.current() == queued,
                    "{context}: queued {queued:?}"
                );
            }
        }
        None => assert_eq!(
            [entity.mission.current(), entity.mission.queued()],
            before,
            "{context}: no queue"
        ),
    }
    // ContinueMegaMission's setters: Set_Destination(MegaDestination, 1) and
    // Assign_Target(MegaTarget).
    for event in row["events"].as_array().unwrap() {
        match event["name"].as_str().unwrap() {
            "assign_destination" => {
                let destination = destination(int(&event["args"][0]))
                    .unwrap_or_else(|| panic!("{context}: destination token"));
                assert_eq!(
                    entity.navigation.nav_com,
                    destination.map(NavTargetRef::from),
                    "{context}: Set_Destination"
                );
            }
            "assign_target" => {
                let target = target(int(&event["args"][0]))
                    .unwrap_or_else(|| panic!("{context}: target token"));
                assert_eq!(
                    entity.attack_target.as_ref().map(|attack| attack.target),
                    target,
                    "{context}: Assign_Target"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn mega_mission_bodies_match_the_original_rows() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_attack_move.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(corpus["row_count"], 638);
    let rules = rules();
    let mut compared = std::collections::BTreeMap::<String, usize>::new();
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        if input["callbacks"] != serde_json::json!({}) {
            continue;
        }
        let entry = input["entry"].as_str().unwrap();
        let context = input["name"].as_str().unwrap();
        let letter = if input["family"] == "unit" { 'U' } else { 'I' };
        let answer = int(&row["returned_al"]);
        match entry {
            "clear" | "has_saved_mission" | "is_attack_move" | "resume" | "idle_resume" => {
                let Some(mut sim) = fixture(row, &format!("{letter}00"), None) else {
                    continue;
                };
                if mega_mission(&row["after"], None).is_none() {
                    continue;
                }
                match entry {
                    "clear" => sim.foot_clear_all_tarcom(OWNER),
                    "has_saved_mission" | "is_attack_move" => assert_eq!(
                        sim.mega_mission(OWNER).is_attack_move(),
                        answer != 0,
                        "{context}"
                    ),
                    "resume" => assert_eq!(
                        sim.foot_continue_mega_mission(
                            OWNER,
                            Some(&rules),
                            None,
                            FrameEffects::default()
                        ),
                        answer != 0,
                        "{context}"
                    ),
                    _ => assert_eq!(
                        sim.foot_refresh_mega_mission(
                            OWNER,
                            Some(&rules),
                            None,
                            FrameEffects::default()
                        ),
                        answer != 0,
                        "{context}"
                    ),
                }
                compare_after(&sim, row, None, context);
            }
            "expiry_slice" => {
                // The map lookup answers its configured cell: DESTINATION_B
                // unless the row names another.
                let Some(cell) =
                    input
                        .get("map_cell")
                        .map_or(Some(Some(CELL_B)), |token| match int(token) {
                            0 => Some(None),
                            token => cell(token).map(Some),
                        })
                else {
                    continue;
                };
                let Some(mut sim) = fixture(row, &format!("{letter}00"), None) else {
                    continue;
                };
                let mega = &mut sim.substrate.entities.get_mut(OWNER).unwrap().mega_mission;
                let expired = match int(&input["expired_pointer"]) {
                    TARGET_A => 2,
                    TARGET_B => 3,
                    TARGET_C => 4,
                    // No Cell expires here, so an expiring DESTINATION_A
                    // stands for an object MegaDestination, which the slice
                    // compares by pointer (`0x004D9B35..0x004D9B3D`).
                    DESTINATION_A => {
                        if mega.destination == Some(TargetKind::Cell(CELL_A.0, CELL_A.1)) {
                            mega.destination = Some(TargetKind::Entity(DESTINATION_OBJECT));
                        }
                        DESTINATION_OBJECT
                    }
                    _ => continue,
                };
                mega.pointer_expired(expired, cell);
                compare_after(&sim, row, None, context);
            }
            "setup" => {
                let event = &input["event"];
                let event_mission = match int(&event["event_mission"]) {
                    29 => MissionType::AttackMove,
                    raw => match mission(raw).and_then(MissionId::known) {
                        Some(known) => known,
                        None => continue,
                    },
                };
                // The tags a decoded event carries: a cell (tag 11, the map's
                // configured cell), the indexed object 101, or index 999,
                // which no object holds: one deleted before the event ran.
                let token = |tag: &str, id: &str| match (int(&event[tag]), int(&event[id])) {
                    (0, _) => Some(None),
                    (11, _) => Some(Some(TargetKind::Cell(CELL_B.0, CELL_B.1))),
                    (52, 101) => Some(Some(TargetKind::Entity(3))),
                    (52, 999) => Some(Some(TargetKind::Entity(DELETED_OBJECT))),
                    _ => None,
                };
                let (Some(event_target), Some(event_destination)) = (
                    token("event_target_tag", "event_target_id"),
                    token("event_destination_tag", "event_destination_id"),
                ) else {
                    continue;
                };
                // Setup never reads the saved mission, which the corpus seeds 5.
                let unread = Some(int(&input["state"]["saved_mission"]));
                let owner_type = format!(
                    "{letter}{}{}",
                    int(&input["primary"]),
                    int(&input["prevent_attack_move"])
                );
                let Some(mut sim) = fixture(row, &owner_type, unread) else {
                    continue;
                };
                let answered = sim.foot_assign_target_command(
                    OWNER,
                    event_mission,
                    event_target,
                    event_destination,
                    Some(&rules),
                );
                assert_eq!(i64::from(answered.id()), answer, "{context}: answer");
                compare_after(&sim, row, unread, context);
            }
            // See the module documentation.
            "acquire" | "convert_target" | "saved_coordinate" => continue,
            other => panic!("{context}: unknown entry {other}"),
        }
        *compared.entry(entry.to_owned()).or_default() += 1;
    }
    let expected = [
        ("clear", 4),
        ("expiry_slice", 24),
        ("has_saved_mission", 4),
        ("idle_resume", 112),
        ("is_attack_move", 4),
        ("resume", 48),
        ("setup", 104),
    ]
    .map(|(entry, count)| (entry.to_owned(), count));
    assert_eq!(compared, std::collections::BTreeMap::from(expected));
}
