//! Comparisons with original gamemd Force_Track4B0C40, its Ship twin 6A0310
//! and bunker459190..4591C4.
//! See locomotor_force_track.meta.json for binary identity and supplied state.
//! Full Force calls use allocated flat, no-overlay cells; the separate crate
//! continuations supply callback effects, not an implementation of crate AI.
//! Rust's raw occupation owner retains the native low byte, not the untouched
//! high dword bits (native fixtures deliberately seed ground400/deck800).

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::docking::bunker_install::{BunkerRuntime, BunkerState, update_bunker};
use crate::sim::docking::bunker_link::BunkerLink;
use crate::sim::movement::{DriveLocomotionRuntime, FacingClass, ShipLocomotionRuntime};
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};
use std::sync::OnceLock;

const UNIT: u64 = 1;
const SUPPLIED: DriveCoord = DriveCoord {
    x: 2656,
    y: 2720,
    z: 731,
};

fn corpus() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_force_track.json",
        ))
        .expect("saved native Force_Track corpus")
    })
}

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed integer")).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: integer(&value[0]),
        y: integer(&value[1]),
        z: integer(&value[2]),
    }
}

fn nullable_coord(value: &Value) -> Option<DriveCoord> {
    let value = coord(value);
    (value != (DriveCoord { x: 0, y: 0, z: 0 })).then_some(value)
}

fn fraction(value: &Value) -> SimFixed {
    SimFixed::from_num(f64::from_bits(
        u64::from_str_radix(value.as_str().expect("native binary64 bits"), 16).unwrap(),
    ))
}

fn coord_array(value: Option<DriveCoord>) -> [i32; 3] {
    value.map_or([0, 0, 0], |c| [c.x, c.y, c.z])
}

fn is_ship(case: &Value) -> bool {
    case["input"]["family"] == "ship"
}

fn mirrored_state(entity: &GameEntity) -> Value {
    let locomotor = entity.locomotor.as_ref().unwrap();
    let (track, head, destination, valid, target) =
        match locomotor.selected_ship_runtime().and_then(|r| r.retained()) {
            Some(ship) => (
                ship.track(),
                ship.head_to(),
                ship.destination(),
                ship.track_valid(),
                ship.target_speed_fraction(),
            ),
            None => {
                let drive = locomotor
                    .selected_drive_runtime()
                    .and_then(|r| r.retained())
                    .unwrap();
                (
                    drive.track(),
                    drive.head_to(),
                    drive.destination(),
                    drive.track_valid(),
                    drive.target_speed_fraction(),
                )
            }
        };
    json!({
        "turn": track.turn_index(),
        "cursor": track.cursor(),
        "reversed": u8::from(track.reversed()),
        "residual": track.residual(),
        "head": coord_array(head),
        "destination": coord_array(destination),
        "track_valid": u8::from(valid),
        "target_fraction_bits": format!("{:016x}", target.to_num::<f64>().to_bits()),
        "applied_fraction_bits": format!("{:016x}", entity.foot_speed.applied_fraction().to_num::<f64>().to_bits()),
        "owner_limbo": u8::from(entity.lifecycle.in_limbo()),
        "owner_alive": u8::from(entity.lifecycle.object_alive()),
    })
}

fn owner_state(entity: &GameEntity, exclude_bunker_speed_write: bool) -> Value {
    let mut value = serde_json::to_value(entity).unwrap();
    let payload = &mut value["locomotor"]["runtime_payload"];
    let family = if payload.get("Ship").is_some() {
        "Ship"
    } else {
        "Drive"
    };
    payload[family].as_object_mut().unwrap().remove("retained");
    if exclude_bunker_speed_write {
        value["foot_speed"]
            .as_object_mut()
            .unwrap()
            .remove("applied_fraction");
    }
    value
}

fn stage<'a>(case: &'a Value, name: &str) -> Option<&'a Value> {
    case["output"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["stage"] == name)
}

fn supplied(case: &Value) -> DriveCoord {
    case["input"].get("head").map_or(SUPPLIED, coord)
}

fn fixture(case: &Value) -> Simulation {
    let mut sim = Simulation::new();
    let before = &case["before"];
    let input = &case["input"];
    let mut entity = GameEntity::test_default(UNIT, "MTNK", "Americans", 9, 10);
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_type_ref_for_test(sim.intern("MTNK"));
    entity.set_category_for_test(EntityCategory::Unit);
    entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
        if is_ship(case) {
            LocomotorKind::Ship
        } else {
            LocomotorKind::Drive
        },
    ));
    // These are separate native bytes. In particular, dead-but-present owners
    // must not acquire a new success-path gate from health or LogicVector.
    entity
        .lifecycle
        .set_object_alive_for_test(integer(&before["owner_alive"]) != 0);
    entity
        .lifecycle
        .set_in_limbo_for_test(integer(&before["owner_limbo"]) != 0);
    entity.lifecycle.set_cell_marked_for_test(false);
    entity.set_foot_occupation_enabled(input["occupation_enabled"].as_bool().unwrap_or(false));
    entity
        .foot_speed
        .set_speed_fraction(fraction(&before["applied_fraction_bits"]));
    entity.position.put_location(DriveCoord {
        x: 2432,
        y: 2688,
        z: 312,
    });
    let track = TrackProgress::for_test(
        integer(&before["turn"]),
        integer(&before["cursor"]),
        integer(&before["reversed"]) != 0,
        integer(&before["residual"]),
    );
    let locomotor = entity.locomotor.as_mut().unwrap();
    assert!(if is_ship(case) {
        locomotor.install_ship_state_for_test(Some(
            ShipLocomotionRuntime::default()
                .with_destination_for_test(nullable_coord(&before["destination"]))
                .with_head_to_for_test(nullable_coord(&before["head"]))
                .with_track_for_test(track)
                .with_track_valid_for_test(integer(&before["track_valid"]) != 0)
                .with_target_speed_fraction_for_test(fraction(&before["target_fraction_bits"])),
        ))
    } else {
        locomotor.install_drive_state_for_test(Some(
            DriveLocomotionRuntime::default()
                .with_destination_for_test(nullable_coord(&before["destination"]))
                .with_head_to_for_test(nullable_coord(&before["head"]))
                .with_track_for_test(track)
                .with_track_valid_for_test(integer(&before["track_valid"]) != 0)
                .with_target_speed_fraction_for_test(fraction(&before["target_fraction_bits"])),
        ))
    });
    assert_eq!(mirrored_state(&entity), *before, "fixture: {input}");
    sim.substrate.entities.insert(entity);
    let bridge = input["bridge"].as_bool().unwrap_or(false);
    let cells = (0..32)
        .flat_map(|y| {
            (0..32).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                cell.level = 2;
                cell.bridge_facts.raw_flags = if bridge { 0x100 } else { 0 };
                cell
            })
        })
        .collect();
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(32, 32, cells));
    sim
}

fn assert_raw_cells(sim: &Simulation, case: &Value) {
    let mut expected = [[[0_u8; 2]; 32]; 32];
    for cell in case["output"]["marked_cells"].as_array().unwrap() {
        let x = integer(&cell["cell"][0]) as usize;
        let y = integer(&cell["cell"][1]) as usize;
        // The high bits are retained by native memory, outside Rust's raw-u8
        // representation. Do not silently compare only the vehicle bit.
        expected[y][x] = [
            (cell["raw"][0].as_u64().unwrap() & 0xff) as u8,
            (cell["raw"][1].as_u64().unwrap() & 0xff) as u8,
        ];
    }
    for y in 0..32_u16 {
        for x in 0..32_u16 {
            let raw = &sim.substrate.raw_cell_occupation;
            let wanted = expected[usize::from(y)][usize::from(x)];
            assert_eq!(
                [raw.ground_bits(x, y), raw.deck_bits(x, y)],
                wanted,
                "raw cell ({x},{y}), {}",
                case["input"]
            );
        }
    }
}

#[test]
fn bunker_dispatch_matches_separate_native_force_then_owner_speed_write() {
    let cases = corpus()["bunker_call_tail"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=NATBNK\n\
         [MTNK]\nStrength=400\nSpeed=6\nBunkerable=yes\n\
         [NATBNK]\nStrength=1000\nBunker=yes\n",
    ))
    .unwrap();
    for case in cases {
        let selector = integer(&case["input"]["turn"]);
        // First compare the production generic call against the captured
        // return boundary. No test-side setter stands in for the caller.
        let mut direct = fixture(case);
        assert!(direct.force_track(
            UNIT,
            selector,
            supplied(case),
            None,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        assert_eq!(
            mirrored_state(direct.substrate.entities.get(UNIT).unwrap()),
            stage(case, "bunker_after_force_before_owner_speed").unwrap()["state"]
        );
        assert_raw_cells(&direct, case);

        // The actual building state machine chooses its selector, invokes
        // Force, and explicitly sets applied speed. Supply Current16 for the
        // requested selector: the native capture starts AFTER facing selection
        // and GetCoords, so this comparison does not establish those producers.
        // Their separate bounded caller corpus covers the full selection.
        let mut sim = fixture(case);
        let (x, y) = match selector {
            67 => (9, 11),
            68 => (9, 9),
            69 => (11, 9),
            70 => (11, 11),
            _ => unreachable!(),
        };
        let entity = sim.substrate.entities.get_mut(UNIT).unwrap();
        entity.position.put_location(DriveCoord {
            x: x * 256 + 128,
            y: y * 256 + 128,
            z: 312,
        });
        entity
            .bunker
            .set_bunker_link_for_test(BunkerLink::Approaching(2));
        let current = match selector {
            67 => 0x2000,
            68 => 0x6000,
            69 => 0xA000,
            70 => 0xE000,
            _ => unreachable!(),
        };
        entity.body_facing = FacingClass::new(current, 5);
        let before_owner = owner_state(entity, true);
        let mut building = GameEntity::test_default_of_category(
            2,
            "NATBNK",
            "Americans",
            10,
            10,
            EntityCategory::Structure,
        );
        building.set_owner_for_test(sim.intern("Americans"));
        building.set_type_ref_for_test(sim.intern("NATBNK"));
        building.position.put_location(supplied(case));
        building.bunker_runtime = Some({
            let mut runtime = BunkerRuntime::idle();
            runtime.set_state_for_test(BunkerState::TurnToBuilding);
            runtime.set_installing_unit_for_test(Some(UNIT));
            runtime
        });
        sim.substrate.entities.insert(building);
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
            crate::sim::world::FrameEffects::default(),
        );
        let entity = sim.substrate.entities.get(UNIT).unwrap();
        assert_eq!(
            mirrored_state(entity),
            case["output"]["state"],
            "{}",
            case["input"]
        );
        assert_eq!(owner_state(entity, true), before_owner, "{}", case["input"]);
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .bunker_runtime
                .unwrap()
                .state(),
            BunkerState::TrackStep
        );
        assert_raw_cells(&sim, case);
    }
}
