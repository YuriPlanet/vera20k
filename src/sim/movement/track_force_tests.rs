//! Comparisons with original gamemd Force_Track4B0C40 and bunker459190..4591C4.
//! See locomotor_force_track.meta.json for binary identity and supplied state.
//! Full Force calls use allocated flat, no-overlay cells; the separate crate
//! continuations supply callback effects, not an implementation of crate AI.
//! Rust's raw occupation owner retains the native low byte, not the untouched
//! high dword bits (native fixtures deliberately seed ground400/deck800).

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveLocomotionRuntime;
use crate::sim::docking::bunker_install::{BunkerRuntime, BunkerState, tick_bunker_install};
use crate::sim::game_entity::BunkerLink;
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
        serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/locomotor_force_track.json"
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

fn mirrored_state(entity: &GameEntity) -> Value {
    let drive = entity.drive_locomotion.as_ref().unwrap();
    json!({
        "turn": drive.track.turn_index,
        "cursor": drive.track.cursor,
        "reversed": u8::from(drive.track.reversed),
        "residual": drive.track.residual,
        "head": coord_array(drive.head_to),
        "destination": coord_array(drive.destination),
        "track_valid": u8::from(drive.track_valid),
        "target_fraction_bits": format!("{:016x}", drive.target_speed_fraction.to_num::<f64>().to_bits()),
        "applied_fraction_bits": format!("{:016x}", entity.foot_speed.applied_fraction.to_num::<f64>().to_bits()),
        "owner_limbo": u8::from(entity.lifecycle.in_limbo),
        "owner_alive": u8::from(entity.lifecycle.object_alive),
    })
}

fn owner_state(entity: &GameEntity, exclude_bunker_speed_write: bool) -> Value {
    let mut value = serde_json::to_value(entity).unwrap();
    value.as_object_mut().unwrap().remove("drive_locomotion");
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
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("MTNK");
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
        LocomotorKind::Drive,
    ));
    // These are separate native bytes. In particular, dead-but-present owners
    // must not acquire a new success-path gate from health or LogicVector.
    entity.lifecycle.object_alive = integer(&before["owner_alive"]) != 0;
    entity.lifecycle.in_limbo = integer(&before["owner_limbo"]) != 0;
    entity.lifecycle.cell_marked = false;
    entity.foot_occupation_enabled = input["occupation_enabled"].as_bool().unwrap_or(false);
    entity.foot_speed.applied_fraction = fraction(&before["applied_fraction_bits"]);
    entity.foot_speed.cached_current_speed = 73;
    put_coords(
        &mut entity,
        DriveCoord {
            x: 2432,
            y: 2688,
            z: 312,
        },
    );
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        destination: nullable_coord(&before["destination"]),
        head_to: nullable_coord(&before["head"]),
        track: TrackProgress {
            turn_index: integer(&before["turn"]),
            cursor: integer(&before["cursor"]),
            reversed: integer(&before["reversed"]) != 0,
            residual: integer(&before["residual"]),
        },
        track_valid: integer(&before["track_valid"]) != 0,
        target_speed_fraction: fraction(&before["target_fraction_bits"]),
        ..Default::default()
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
            let claims = &sim.substrate.cell_occupation;
            assert_eq!(
                [
                    claims.vehicle_bits(x, y, MovementLayer::Ground),
                    claims.vehicle_bits(x, y, MovementLayer::Bridge),
                ],
                wanted,
                "rebuildable claims ({x},{y}), {}",
                case["input"]
            );
        }
    }
}

#[test]
fn full_force_track_no_overlay_matches_native_publication_and_owner_preservation() {
    let cases = corpus()["force_track"].as_array().unwrap();
    assert_eq!(cases.len(), 192);
    for case in cases {
        let mut sim = fixture(case);
        let before_owner = owner_state(sim.substrate.entities.get(UNIT).unwrap(), false);
        let mut callbacks = 0;
        let admitted = sim.force_drive_track_observed(
            UNIT,
            integer(&case["input"]["turn"]),
            supplied(case),
            &mut |sim, id, received_coord| {
                callbacks += 1;
                assert_eq!(received_coord, supplied(case));
                assert_eq!(
                    mirrored_state(sim.substrate.entities.get(id).unwrap()),
                    stage(case, "head_published_before_map_query").unwrap()["state"],
                    "pre-crate publication: {}",
                    case["input"]
                );
                true
            },
        );
        // Native Force is void: Rust's admission result is compared against
        // whether the captured execution reached occupation/destination.
        assert_eq!(
            admitted,
            stage(case, "occupation_return_before_destination").is_some(),
            "admission path: {}",
            case["input"]
        );
        assert_eq!(
            callbacks,
            usize::from(stage(case, "post_crate_return").is_some())
        );
        let entity = sim.substrate.entities.get(UNIT).unwrap();
        assert_eq!(
            mirrored_state(entity),
            case["output"]["state"],
            "{}",
            case["input"]
        );
        assert_eq!(case["output"]["owner_preserved"], true);
        assert_eq!(
            owner_state(entity, false),
            before_owner,
            "owner: {}",
            case["input"]
        );
        assert_raw_cells(&sim, case);
    }
}

#[test]
fn supplied_post_crate_continuations_reload_life_head_and_valid_like_native() {
    let cases = corpus()["supplied_post_crate_continuations"]
        .as_array()
        .unwrap();
    assert_eq!(cases.len(), 24);
    for case in cases {
        let mut sim = fixture(case);
        let effect = &case["input"]["supplied_callback"];
        let mut supplied_owner = None;
        let admitted = sim.force_drive_track_observed(
            UNIT,
            integer(&case["input"]["turn"]),
            supplied(case),
            &mut |sim, id, received_coord| {
                assert_eq!(received_coord, supplied(case));
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                assert_eq!(
                    mirrored_state(entity),
                    stage(case, "head_published_before_map_query").unwrap()["state"]
                );
                entity.lifecycle.object_alive = effect["alive"].as_bool().unwrap();
                entity.lifecycle.in_limbo = effect["limbo"].as_bool().unwrap();
                let drive = entity.drive_locomotion.as_mut().unwrap();
                drive.head_to = nullable_coord(&effect["head"]);
                drive.track_valid = effect["track_valid"].as_bool().unwrap();
                supplied_owner = Some(owner_state(entity, false));
                assert_eq!(
                    mirrored_state(entity),
                    stage(case, "post_crate_return").unwrap()["state"],
                    "{}",
                    case["input"]
                );
                integer(&effect["result_al"]) != 0
            },
        );
        assert_eq!(
            admitted,
            stage(case, "occupation_return_before_destination").is_some()
        );
        let entity = sim.substrate.entities.get(UNIT).unwrap();
        assert_eq!(
            mirrored_state(entity),
            case["output"]["state"],
            "{}",
            case["input"]
        );
        assert_eq!(case["output"]["owner_preserved"], true);
        assert_eq!(
            owner_state(entity, false),
            supplied_owner.unwrap(),
            "{}",
            case["input"]
        );
        assert_raw_cells(&sim, case);
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
        assert!(direct.force_drive_track(UNIT, selector, supplied(case)));
        assert_eq!(
            mirrored_state(direct.substrate.entities.get(UNIT).unwrap()),
            stage(case, "bunker_after_force_before_owner_speed").unwrap()["state"]
        );
        assert_raw_cells(&direct, case);

        // The actual building state machine chooses its selector, invokes
        // Force, and explicitly sets applied speed. Diagonal approach fixtures
        // supply the selected quadrant; the native capture starts AFTER that
        // selection/GetCoords, so this is not facing-selection parity.
        let mut sim = fixture(case);
        let (x, y) = match selector {
            67 => (9, 11),
            68 => (9, 9),
            69 => (11, 9),
            70 => (11, 11),
            _ => unreachable!(),
        };
        let entity = sim.substrate.entities.get_mut(UNIT).unwrap();
        put_coords(
            entity,
            DriveCoord {
                x: x * 256 + 128,
                y: y * 256 + 128,
                z: 312,
            },
        );
        entity.bunker_link = BunkerLink::Approaching(2);
        entity.facing_target = None;
        let before_owner = owner_state(entity, true);
        let mut building = GameEntity::test_default(2, "NATBNK", "Americans", 10, 10);
        building.owner = sim.intern("Americans");
        building.type_ref = sim.intern("NATBNK");
        building.category = EntityCategory::Structure;
        put_coords(&mut building, supplied(case));
        building.bunker_runtime = Some(BunkerRuntime {
            state: BunkerState::TurnToBuilding,
            installing_unit: Some(UNIT),
        });
        sim.substrate.entities.insert(building);
        tick_bunker_install(&mut sim, &rules, None);
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
                .state,
            BunkerState::TrackStep
        );
        assert_raw_cells(&sim, case);
    }
}

#[test]
fn special_process_selectors_preserve_or_copy_applied_fraction_like_native_prefix() {
    // Process4B0F20..4B1274 is bounded before GetCurrentSpeed. The shared Rust
    // entry also computes that getter cache, which is outside this comparison.
    // This fixture supplies no live Passive type. Its dedicated comparison
    // remains bounded to the special-selector rows; the broader numeric
    // prefix corpus covers ordinary selectors and Passive separately.
    let mut compared = 0;
    for case in corpus()["process_speed_prefix"].as_array().unwrap() {
        if integer(&case["input"]["turn"]) < 64
            || case["input"]["passive"].as_bool().unwrap_or(false)
        {
            continue;
        }
        let mut sim = fixture(case);
        let entity = sim.substrate.entities.get_mut(UNIT).unwrap();
        entity.drive_accelerates = case["input"]["accelerates"].as_bool().unwrap();
        super::super::track_speed::advance(entity, None, None, None);
        assert_eq!(
            mirrored_state(entity),
            case["output"]["state"],
            "{}",
            case["input"]
        );
        compared += 1;
    }
    assert_eq!(compared, 18);
}
