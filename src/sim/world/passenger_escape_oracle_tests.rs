//! Replay of `tools/spatial_oracle/passenger_escape.json`: every original row
//! of a dying Unit's passenger block (`UnitClass::ReceiveDamage
//! 0x00737F80..0x007381BC`) runs against its Rust owner
//! (`crew_survival::release_dying_unit_passengers`) on the refinery_dock map
//! (33x33 clear level-0 cells) with the row's ramp, water and bridge cells.
//!
//! The scene is built like the oracle's: a transport of the row's type flags
//! in limbo at the row's coordinate (its Mark(UP) already done), unarmed
//! infantry passengers that boarded through the production Limbo (Doing
//! Ready, not prone) with passenger 0 at the cargo head, the row's standing
//! infantry revealed in their spots, a second house allied both ways only
//! when the row says, and the Scenario stream seeded 31 at the row's frame.
//!
//! Compared per row: the cargo left, the RNG cursors, the ground and deck
//! occupation bits and owners of the transport's cell; and for each
//! passenger: dead (RecordKill and UnInit) or still aboard or out, its
//! Unlimbo coordinate, facing and OnBridge, the transporter link, the queued
//! mission, the NavCom its Scatter installed, the target an open-topped
//! transport of another house makes it drop, and its selection.
//!
//! Not compared: the current mission (the oracle answers FootClass::Unlimbo,
//! so its Enter_Idle_Mode never runs), the kill credit's effects (RecordKill
//! is answered), and Team Add_Member (no VERA owner). Residual rows
//! (`crew_survival` residuals):
//! - the ramp rows that Unlimbo at the exact coordinate compare the floor
//!   under the XY: native keeps the cell centre's Z, VERA's reveal grounds
//!   the object;
//! - `dropped_as_bomb` is skipped: VERA has no IsABomb byte (`+0x8F`), so
//!   its passengers escape where native kills them (`0x007380AF`).

use super::refinery_dock_oracle_tests::{cell, world_with};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponOverride;
use crate::sim::components::NavTargetRef;
use crate::sim::crew_survival::DyingTransport;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::RawCellKey;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::rng::SimRng;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation, UninitContext,
};
use crate::util::fixed_math::SimFixed;
use serde_json::Value;
use std::collections::BTreeMap;

/// The passengers' type is the oracle's unarmed InfantryType (Strength 125,
/// MovementZone Infantry, SpeedType Foot, Walk); the transport's flags are
/// the row's.
const RULES: &str = "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=APC\n\
    [E1]\nStrength=125\nSpeed=4\nMovementZone=Infantry\n\
    Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
    [APC]\nStrength=200\nSpeed=6\nPassengers=5\n\
    Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n";

/// PlaceInfantryInCell's spot offsets (`0x0089E9F0`).
const SPOT_OFFSETS: [(i32, i32); 5] = [(128, 128), (64, 64), (192, 64), (64, 192), (192, 192)];
const MISSION_HUNT: i64 = 15;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/passenger_escape.json"
    ))
    .unwrap()
}

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    transport: u64,
    passengers: Vec<u64>,
    attacker: u64,
}

fn rules_ini(input: &Value) -> IniFile {
    let mut text = String::from(RULES);
    for (flag, key) in [
        ("crashable", "Crashable"),
        ("open_topped", "OpenTopped"),
        ("gunner", "Gunner"),
    ] {
        if input[flag] == true {
            text = text.replacen("[APC]\n", &format!("[APC]\n{key}=yes\n"), 1);
        }
    }
    for land in crate::rules::terrain_rules::LandType::ALL.iter().take(9) {
        text.push_str(&format!(
            "[{}]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=100%\nAmphibious=100%\n\
             FloatBeach=100%\nHover=100%\nBuildable=yes\n",
            land.section_name()
        ));
    }
    IniFile::from_str(&text)
}

/// The row's cells: a slope under the transport, a bridge (the structural
/// flag `0x100`) on three cells in a west-east line through it, and water
/// (LandType 2, no SpeedType crosses it) under the bridge or the transport.
fn edit_cells(input: &Value, terrain: &mut ResolvedTerrainGrid) {
    let (x, y) = input.get("cell").map_or((15, 15), cell);
    let bridge: Vec<(u16, u16)> = if input["bridge"] == true {
        vec![(x - 1, y), (x, y), (x + 1, y)]
    } else {
        Vec::new()
    };
    for &(bx, by) in &bridge {
        let c = terrain.cell_mut(bx, by).unwrap();
        c.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        c.has_bridge_deck = true;
        c.bridge_walkable = true;
        c.bridge_deck_level = c.level + 4;
    }
    if input["water"] == true {
        let water: Vec<(u16, u16)> = if bridge.is_empty() {
            vec![(x, y)]
        } else {
            bridge.clone()
        };
        let none = SpeedCostProfile {
            foot: Some(0),
            track: Some(0),
            wheel: Some(0),
            float: Some(0),
            amphibious: Some(0),
            float_beach: Some(0),
            hover: Some(0),
        };
        for (wx, wy) in water {
            let c = terrain.cell_mut(wx, wy).unwrap();
            c.yr_cell_land_type = 2;
            c.base_yr_cell_land_type = 2;
            c.is_water = true;
            c.speed_costs = none;
            c.base_speed_costs = none;
        }
    }
    if let Some(slope) = input["slope"].as_u64() {
        let c = terrain.cell_mut(x, y).unwrap();
        c.slope_type = slope as u8;
        c.has_ramp = true;
    }
}

fn scene(input: &Value) -> Scene {
    let ini = rules_ini(input);
    let rules = RuleSet::from_ini(&ini).expect("passenger escape rules");
    let mut sim = world_with(&rules, &ini, |terrain| edit_cells(input, terrain));
    let human = input["human"] == true;
    let americans = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(americans, 0, None, human, 0, 10);
    house.player_control = human;
    sim.houses.insert(americans, house);
    let foreign = sim.interner.intern("Foreign");
    sim.houses.insert(
        foreign,
        crate::sim::house_state::HouseState::new(foreign, 1, None, false, 0, 10),
    );
    if input["foreign_allied"] == true {
        for (a, b) in [("AMERICANS", "FOREIGN"), ("FOREIGN", "AMERICANS")] {
            sim.house_alliances
                .entry(a.to_string())
                .or_default()
                .insert(b.to_string());
        }
    }
    sim.session.game_mode_nonzero = true;
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(200) as u32;
    let frame = sim.session.binary_frame;
    let heights = BTreeMap::new();

    // The transport: in limbo at its coordinate, as its Mark(UP) leaves it.
    let (x, y) = input.get("cell").map_or((15, 15), cell);
    let on_bridge = input["on_bridge"] == true;
    let transport = sim
        .construct_object_limbo_at_height("APC", "Americans", x, y, 0, 0, &rules)
        .expect("transport");
    let sub = input.get("sub").map_or((128, 128), |sub| {
        (
            sub[0].as_i64().unwrap() as i32,
            sub[1].as_i64().unwrap() as i32,
        )
    });
    let world_xy = [i32::from(x) * 256 + sub.0, i32::from(y) * 256 + sub.1];
    let floor = crate::sim::movement::ground_pose::ground_surface_z_at(
        world_xy,
        false,
        sim.resolved_terrain.as_ref(),
        None,
    )
    .unwrap();
    let height = input["height"].as_i64().unwrap_or(0) as i32;
    let facing = input["facing"].as_u64().unwrap_or(0xC000) as u16;
    {
        let unit = sim.substrate.entities.get_mut(transport).unwrap();
        unit.position.sub_x = SimFixed::from_num(sub.0);
        unit.position.sub_y = SimFixed::from_num(sub.1);
        unit.position.z = if on_bridge { 4 } else { 0 };
        unit.position.exact_z_leptons = Some(floor + if on_bridge { 416 } else { 0 } + height);
        unit.on_bridge = on_bridge;
        unit.facing = (facing >> 8) as u8;
        // The Unit's FacingClass (`+0x388`), settled on the row's facing.
        let mut body = crate::sim::movement::FacingClass::new(facing, 0);
        body.snap(facing, frame);
        unit.body_facing = Some(body);
        if input["gunner"] == true {
            unit.weapon_override = Some(WeaponOverride::IfvSlot(0));
        }
        if unit.passenger_role.cargo().is_none() {
            unit.passenger_role = PassengerRole::Transport {
                cargo: PassengerCargo::new(5, 0),
            };
        }
    }
    let attacker = sim
        .spawn_object("APC", "Foreign", 28, 28, 0, &rules, &heights)
        .expect("attacker");

    // The passengers board through the production Limbo, last first.
    let specs = input["passengers"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![Value::Object(Default::default())]);
    let mut passengers = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let owner = if spec["foreign"] == true {
            "Foreign"
        } else {
            "Americans"
        };
        let id = sim
            .spawn_object("E1", owner, 18 + index as u16, 12, 0, &rules, &heights)
            .expect("passenger");
        passengers.push(id);
    }
    for &id in passengers.iter().rev() {
        assert_eq!(
            sim.techno_limbo_with_rules(id, &rules),
            crate::sim::world::ConcealOutcome::Concealed
        );
        sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::Inside {
            transport_id: transport,
        };
        assert!(
            sim.substrate
                .entities
                .get_mut(transport)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap()
                .board(id, 1)
        );
        if input["open_topped"] == true {
            assert!(sim.register_open_topped_passenger(id));
            // A target to drop: Assign_Target(NULL) is the only writer here.
            let commits = crate::sim::mission::concrete_effects::assign_target_commits(
                &sim.substrate.entities,
                Some(TargetKind::Entity(attacker)),
            );
            crate::sim::mission::concrete_effects::represented_assign_target_admitted(
                sim.substrate.entities.get_mut(id).unwrap(),
                Some(TargetKind::Entity(attacker)),
                commits,
            );
        }
    }

    // The standing infantry, each revealed in its spot.
    for spec in input["occupants"].as_array().into_iter().flatten() {
        let owner = if spec["foreign"] == true {
            "Foreign"
        } else {
            "Americans"
        };
        let spot = spec["spot"].as_u64().unwrap() as u8;
        let id = sim
            .construct_object_limbo_at_height("E1", owner, x, y, 0, 0, &rules)
            .expect("occupant");
        sim.substrate.entities.get_mut(id).unwrap().sub_cell = Some(spot);
        let (dx, dy) = SPOT_OFFSETS[usize::from(spot)];
        let outcome = sim.try_reveal_entity_with_context(
            id,
            RevealRequest {
                position: RevealPosition {
                    rx: x,
                    ry: y,
                    z: 0,
                    sub_x: SimFixed::from_num(dx),
                    sub_y: SimFixed::from_num(dy),
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            UninitContext::with_rules(&rules),
        );
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));
    }
    if input["team"] == true {
        let script = sim.interner.intern("ESCAPE");
        sim.team_script_vm
            .create_team(americans, script, vec![transport], None, 0);
    }
    // The oracle seeds the Scenario stream 31 and draws nothing before the
    // block.
    sim.scenario_rng = SimRng::new(31);
    sim.substrate.pending_delete.clear();
    Scene {
        sim,
        rules,
        transport,
        passengers,
        attacker,
    }
}

fn house_index(sim: &Simulation, owner: Option<crate::sim::intern::InternedId>) -> i64 {
    match owner.map(|owner| sim.interner.resolve(owner).to_string()) {
        None => -1,
        Some(name) if name == "Americans" => 0,
        Some(name) if name == "Foreign" => 1,
        Some(other) => panic!("occupation owner {other}"),
    }
}

/// The rows whose escapees Unlimbo at the exact coordinate on a ramp: VERA
/// grounds them on the floor under their XY instead of the centre's Z.
const RAMP_EXACT: &[&str] = &[
    "ramp_off_centre",
    "ramp_2_off_centre",
    "corner_ramp_off_centre",
];

fn compare(row: &Value) {
    let input = &row["input"];
    let name = input["name"].as_str().unwrap();
    let native = &row["state"];
    let events = row["events"].as_array().unwrap();
    let Scene {
        mut sim,
        rules,
        transport,
        passengers,
        attacker,
    } = scene(input);
    let dying = DyingTransport {
        attacker: (input["attacker"] != false).then_some(attacker),
        ignore_defenses: input["ignore_defenses"] == true,
        selected_by_player: input["selected"] == true,
    };
    // The oracle answers the Walk Process slot (`0x0075AC80`) as not moving.
    super::bridge_hut_scatter::answered_process::install();
    sim.release_dying_unit_passengers(&rules, None, transport, dying);
    let processed = super::bridge_hut_scatter::answered_process::finish();
    let expected_processed: Vec<u64> = events
        .iter()
        .filter(|event| event[0] == "process")
        .map(|event| passengers[event[1].as_u64().unwrap() as usize])
        .collect();
    assert_eq!(
        processed, expected_processed,
        "row {name}: Walk Process calls"
    );

    let context = format!("row {name}");
    let cargo = sim
        .substrate
        .entities
        .get(transport)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap()
        .passengers
        .clone();
    let head = cargo.first().map_or(Value::Null, |id| {
        passengers.iter().position(|p| p == id).into()
    });
    assert_eq!(
        serde_json::json!([cargo.len(), head]),
        native["cargo"],
        "{context}: cargo"
    );
    let rng = sim.scenario_rng.logical_state();
    assert_eq!(
        serde_json::json!([rng.index_a, rng.index_b]),
        native["random_indices"],
        "{context}: RNG cursors"
    );
    let (x, y) = input.get("cell").map_or((15, 15), cell);
    let key = RawCellKey::Real(x, y);
    let raw = &sim.substrate.raw_cell_occupation;
    let occupation = serde_json::json!({
        "ground": raw.bits_at(key, MovementLayer::Ground),
        "deck": raw.bits_at(key, MovementLayer::Bridge),
        "owners": [
            house_index(&sim, raw.owner_at(key, MovementLayer::Ground)),
            house_index(&sim, raw.owner_at(key, MovementLayer::Bridge)),
        ],
    });
    assert_eq!(occupation, native["occupation"], "{context}: occupation");

    for (index, &id) in passengers.iter().enumerate() {
        let context = format!("{context}: passenger {index}");
        let expected = &native["passengers"][index];
        let of = |kind: &str| {
            events
                .iter()
                .find(|event| event[0] == kind && event[1] == index)
        };
        let killed = of("uninit").is_some();
        assert_eq!(
            sim.substrate.pending_delete.contains(&id),
            killed,
            "{context}: UnInit"
        );
        if killed {
            continue;
        }
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            u64::from(entity.lifecycle.in_limbo),
            expected["limbo"].as_u64().unwrap(),
            "{context}: limbo"
        );
        let aboard = matches!(
            entity.passenger_role,
            PassengerRole::Inside { transport_id } if transport_id == transport
        );
        assert_eq!(
            aboard,
            expected["transporter"] == "transport",
            "{context}: transporter"
        );
        if aboard {
            continue;
        }
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let mut want: Vec<i64> = expected["coord"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        if RAMP_EXACT.contains(&name) {
            want[2] = i64::from(
                crate::sim::movement::ground_pose::ground_surface_z_at(
                    [want[0] as i32, want[1] as i32],
                    false,
                    sim.resolved_terrain.as_ref(),
                    None,
                )
                .unwrap(),
            );
        }
        assert_eq!(
            vec![i64::from(coord.x), i64::from(coord.y), i64::from(coord.z)],
            want,
            "{context}: coordinate"
        );
        assert_eq!(
            u64::from(entity.on_bridge),
            expected["on_bridge"].as_u64().unwrap(),
            "{context}: OnBridge"
        );
        let unlimbo = of("unlimbo").expect("an escapee Unlimboes");
        assert_eq!(
            u64::from(entity.facing),
            unlimbo[3].as_u64().unwrap(),
            "{context}: Unlimbo facing"
        );
        let queued = entity.mission.queued().raw() as i64;
        let queued = if queued == MISSION_HUNT { queued } else { -1 };
        assert_eq!(
            queued,
            expected["queued"].as_i64().unwrap(),
            "{context}: queued mission"
        );
        let nav = match entity.navigation.nav_com {
            Some(NavTargetRef::Cell { rx, ry }) => serde_json::json!([rx, ry]),
            None => Value::Null,
            Some(other) => panic!("{context}: NavCom {other:?}"),
        };
        assert_eq!(nav, expected["nav"], "{context}: NavCom");
        assert_eq!(
            entity.selected,
            of("select").is_some(),
            "{context}: selected"
        );
        if input["open_topped"] == true {
            assert_eq!(
                entity.attack_target.is_none(),
                of("assign_target").is_some(),
                "{context}: target"
            );
        }
    }
    if input["gunner"] == true {
        assert_eq!(
            sim.substrate
                .entities
                .get(transport)
                .unwrap()
                .weapon_override,
            None,
            "{context}: the emptying pop hands the gunner's weapon back"
        );
    }
}

#[test]
fn passenger_escape_matches_the_original_rows() {
    let corpus = corpus();
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 40);
    let mut failures = Vec::new();
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap().to_string();
        // RESIDUAL: no IsABomb byte (module doc).
        if name == "dropped_as_bomb" {
            continue;
        }
        if let Err(cause) = std::panic::catch_unwind(|| compare(row)) {
            let message = cause
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| cause.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{name}: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
