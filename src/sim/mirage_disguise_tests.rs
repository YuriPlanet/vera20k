//! Executed Unit7468C0 comparisons at the live, post-Foot visit boundary.
//! Native state, selected Cell-list priors and complete Random2 bytes come
//! from the independent original executable corpus. This does not execute
//! held locomotion, modal game-inactive Logic or a whole native frame.

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::occupancy::CellListInsertion;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .expect("original Mirage corpus");
    assert_eq!(native["schema_version"], 1);
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed dword")).unwrap()
}

/// Import only the represented identity/timer fields. +1E4 is the poisoned,
/// unread middle timer word, not an invented packed Cell coordinate.
pub(in crate::sim) fn state_from_native(sim: &mut Simulation, native: &Value) -> DisguiseRuntime {
    let type_id = native["type"]
        .as_str()
        .map(|name| sim.interner.intern(name));
    assert_eq!(native["house_pointer"], "0x0", "ordinary Mirage NULL house");
    DisguiseRuntime {
        disguised: native["disguised"] == 1,
        disguise_creation_frame: signed(&native["creation_frame"]) as u32,
        disguise_type: type_id,
        disguised_as_house: None,
        reveal_timer: CdTimer::from_raw(
            signed(&native["reveal_start"]),
            signed(&native["reveal_duration"]),
        ),
    }
}

fn assert_state(sim: &Simulation, native: &Value, context: &str) {
    assert_entity_state(sim, 1, native, context);
}

pub(in crate::sim) fn assert_entity_state(
    sim: &Simulation,
    id: u64,
    native: &Value,
    context: &str,
) {
    let entity = sim.substrate.entities.get(id).unwrap();
    let state = entity.disguise.as_ref().expect("Techno constructor state");
    assert_eq!(
        json!({
            "disguised": u8::from(state.is_disguised()),
            "creation_frame": state.creation_frame() as i32,
            "type": state.type_id().map(|id| sim.interner.resolve(id)),
            "null_house": state.house().is_none(),
            "reveal_start": state.reveal_timer().start_frame(),
            "reveal_duration": state.reveal_timer().duration(),
        }),
        json!({
            "disguised": native["disguised"],
            "creation_frame": native["creation_frame"],
            "type": native["type"],
            "null_house": native["house_pointer"] == "0x0",
            "reveal_start": native["reveal_start"],
            "reveal_duration": native["reveal_duration"],
        }),
        "{context}"
    );
}

fn fixture(row: &Value) -> Simulation {
    let mut sim = Simulation::new();
    assert_eq!(sim.allocate_stable_id(), 1);
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("MGTK");
    let before = &row["before"];
    let [x, y, z] = std::array::from_fn(|i| signed(&before["xyz"][i]));
    let mut entity =
        GameEntity::test_default(1, "MGTK", "Americans", (x / 256) as u16, (y / 256) as u16);
    entity.owner = owner;
    entity.type_ref = type_ref;
    entity.category = EntityCategory::Unit;
    entity.position.sub_x = SimFixed::from_num(x % 256);
    entity.position.sub_y = SimFixed::from_num(y % 256);
    entity.position.exact_z_leptons = Some(z);
    entity.health.current = signed(&before["health"]);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.disguise = Some(state_from_native(&mut sim, before));
    let origin = (entity.position.rx, entity.position.ry);
    sim.substrate.entities.insert(entity);
    let tile = row["input"]["tile_index"].as_i64().unwrap_or(0) as i32;
    let mut grid = test_grid(128, 128, |rx, ry| {
        let mut cell = test_flat_cell(rx, ry);
        if (rx, ry) == origin {
            cell.final_tile_index = tile;
        }
        cell
    });
    let base = row["input"]["bridge_base"].as_i64().unwrap_or(-1);
    grid.test_set_high_bridge_set_starts(u16::try_from(base).ok(), None);
    sim.resolved_terrain = Some(grid);
    sim.session.binary_frame = signed(&row["frame"]) as u32;
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap());
    sim
}

fn add_neighbor(
    sim: &mut Simulation,
    id: u64,
    cell: (u16, u16),
    owner: &str,
    category: EntityCategory,
    layer: MovementLayer,
) {
    assert_eq!(sim.allocate_stable_id(), id);
    let kind = if category == EntityCategory::Infantry {
        "E1"
    } else {
        "MTNK"
    };
    let mut entity = GameEntity::test_default(id, kind, owner, cell.0, cell.1);
    entity.owner = sim.interner.intern(owner);
    entity.type_ref = sim.interner.intern(kind);
    entity.category = category;
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy.add(
        cell.0,
        cell.1,
        id,
        layer,
        (category == EntityCategory::Infantry).then_some(2),
        CellListInsertion::PrependNonBuilding,
    );
}

fn assert_visit(sim: &mut Simulation, rules: &RuleSet, row: &Value) {
    let context = row["name"].as_str().unwrap();
    assert_state(sim, &row["before"], context);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_before"]["scenario"],
        "{context}: Scenario input"
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        row["rng_before"]["main"],
        "{context}: Main input"
    );
    sim.session.binary_frame = signed(&row["frame"]) as u32;
    sim.update_unit_disguise(1, rules).expect("live Unit visit");
    assert_state(sim, &row["after"], context);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_after"]["scenario"],
        "{context}: entire Scenario continuation"
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        row["rng_after"]["main"],
        "{context}: entire Main continuation"
    );
}

#[test]
fn native_mirage_first_infantry_layer_and_timer_boundaries() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    let mut compared = 0;
    for row in native["adjacency"]["controls"].as_array().unwrap() {
        if row["input"]["game_active"] == 0 {
            // The normal Logic visit cannot run inside the original modal
            // pause/shutdown arm. Keep that executable control in the corpus.
            continue;
        }
        let mut sim = fixture(row);
        if row["input"].get("neighbor_offset").is_some()
            || row["input"].get("enemy_layer").is_some()
        {
            let pos = sim.substrate.entities.get(1).unwrap().position;
            let dx = row["input"]["neighbor_offset"][0].as_i64().unwrap_or(1);
            let dy = row["input"]["neighbor_offset"][1].as_i64().unwrap_or(0);
            let cell = (
                (i64::from(pos.rx) + dx) as u16,
                (i64::from(pos.ry) + dy) as u16,
            );
            let layer = if row["input"]["enemy_layer"] == "bridge" {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            let category = if row["input"]["chain"] == "unit" {
                EntityCategory::Unit
            } else {
                EntityCategory::Infantry
            };
            add_neighbor(&mut sim, 2, cell, "Russians", category, layer);
            if row["input"]["chain"] == "allied_then_enemy" {
                // Native chains are head-first; the production insertion
                // owner prepends this friendly Infantry ahead of the enemy.
                add_neighbor(
                    &mut sim,
                    3,
                    cell,
                    "Americans",
                    EntityCategory::Infantry,
                    layer,
                );
            }
        }
        assert_visit(&mut sim, &retail.rules, row);
        compared += 1;
    }
    assert_eq!(compared, 29);
}

#[test]
fn native_mirage_reveal_refresh_removal_and_reacquisition_history() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    let history = &native["adjacency"]["physical_history"];
    let rows = history["rows"].as_array().unwrap();
    let mut sim = fixture(&rows[0]);
    let cell = (
        history["placement"]["cell"][0].as_u64().unwrap() as u16,
        history["placement"]["cell"][1].as_u64().unwrap() as u16,
    );
    assert_eq!(history["placement"]["returned_al"], 1);
    assert_eq!(history["remove_returned_al"], 1);
    add_neighbor(
        &mut sim,
        2,
        cell,
        "Russians",
        EntityCategory::Infantry,
        MovementLayer::Ground,
    );
    for row in rows {
        if row["name"] == "expiry_minus1" {
            // Original Infantry Limbo removes this same Cell-list member.
            // The comparison begins after its placement/house boundary; this
            // is not a movement or ownership-transfer test.
            sim.substrate.occupancy.remove(cell.0, cell.1, 2);
            sim.substrate.entities.remove(2);
        }
        assert_visit(&mut sim, &retail.rules, row);
    }
    assert_eq!(rows.len(), 6);
}

#[test]
fn native_mirage_acquisition_gates_and_retained_identity() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    let rows = native["acquisition"].as_array().unwrap();
    for row in rows {
        let mut sim = fixture(row);
        let input = &row["input"];
        let mut overrides = String::from("[MGTK]\n");
        for (input_key, ini_key) in [
            ("can_disguise", "CanDisguise"),
            ("perma_disguise", "PermaDisguise"),
            ("disguise_when_still", "DisguiseWhenStill"),
        ] {
            if let Some(value) = input[input_key].as_bool() {
                overrides.push_str(&format!("{ini_key}={}\n", if value { "yes" } else { "no" }));
            }
        }
        let mut layers =
            crate::rules::native_processing::RulesLayerStack::new(retail.processed_rules.clone());
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            crate::rules::ini_parser::IniFile::from_str(&overrides),
        );
        let rules = RuleSet::from_processed_rules(
            &layers.process_with_fixed_art(&retail.fixed_art).unwrap(),
        )
        .unwrap();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        if let Some(slot) = input["radio_contact_slot"].as_u64() {
            entity.radio_contacts.set_capacity(slot as usize + 1);
            entity.radio_contacts.set_slot(slot as usize, 99);
        }
        if input["drive_destination_prior"] == true {
            // Original DriveIsMoving reads an explicit nonzero Destination
            // prior; no locomotor Process or movement order is executed.
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_drive_state_for_test(Some(
                        crate::sim::movement::DriveLocomotionRuntime::default()
                            .with_destination_for_test(Some(
                                crate::sim::components::DriveCoord::cell(90, 51, 0)
                            ))
                    ))
            );
        }
        if row["entry"] == "0x7360c0" {
            // Full native UnitAI also visits Foot's independent timer/RNG
            // work. Compare only the absent disguise visit here; do not
            // mistake its whole-AI stream for this caller gate's stream.
            assert!(
                row["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|call| call["name"] != "update_disguise")
            );
            let rng = sim.rng_state();
            sim.update_unit_disguise(1, &rules).unwrap();
            assert_state(&sim, &row["after"], row["name"].as_str().unwrap());
            assert_eq!(sim.rng_state(), rng);
        } else {
            assert_visit(&mut sim, &rules, row);
        }
    }
    assert_eq!(rows.len(), 10);
}

#[test]
fn native_mirage_object_load_preserves_reveal_and_retained_continuations() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    for row in native["load"].as_array().unwrap() {
        let visits = row["continuation"].as_array().unwrap();
        let mut sim = fixture(&visits[0]);
        let bytes = serde_json::to_vec(sim.substrate.entities.get(1).unwrap()).unwrap();
        let loaded: GameEntity = serde_json::from_slice(&bytes).unwrap();
        sim.substrate.entities.insert(loaded);
        assert_state(&sim, &row["after_load"], row["name"].as_str().unwrap());
        for visit in visits {
            assert_visit(&mut sim, &retail.rules, visit);
        }
    }
}

#[test]
fn native_mirage_constructor_initializes_identity_and_timer_frame() {
    let native = corpus();
    let expected = &native["initialization"]["actor_constructor"];
    assert_eq!(expected["reveal_middle_raw"], 0xa5a5a5a5_u32);
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let kind = sim.interner.intern("MGTK");
    let entity = GameEntity::new_at_frame_for_test(
        1,
        0,
        0,
        0,
        0,
        owner,
        crate::sim::components::Health {
            current: signed(&expected["health"]),
        },
        kind,
        EntityCategory::Unit,
        0,
        0,
        true,
        signed(&expected["reveal_start"]) as u32,
    );
    sim.substrate.entities.insert(entity);
    assert_state(&sim, expected, "full poisoned Unit constructor");
}

/// Rust production persistence regression in addition to the narrower
/// original object-load comparisons above. Full Scenario load deliberately
/// resets Scenario Random2 to seed zero, so it cannot promise the unsaved
/// stream's next tree. Identity, signed timer and the no-draw wait survive.
#[test]
fn mirage_full_snapshot_preserves_wait_and_uses_the_restored_scenario_owner() {
    use crate::sim::snapshot::GameSnapshot;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    let row = native["load"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "revealed_timer")
        .unwrap();
    let first = &row["continuation"][0];
    let mut sim = fixture(first);
    sim.session.map_name = "MIRAGE.MAP".into();
    let config_hash = retail.rules.simulation_config_hash();
    let bytes = GameSnapshot::save_validated(&sim, config_hash, 0, "Mirage timer", 0);
    let mut loaded = GameSnapshot::load_validated(&bytes, config_hash, 0, "MIRAGE.MAP")
        .unwrap()
        .sim;
    loaded
        .restore_after_snapshot_load()
        .expect("restored Unit graph");
    loaded.resolve_type_handles(&retail.rules);
    // Resolved terrain is an immutable app-provided map input, not snapshot
    // data. Bind that same map before the resumed Unit visit.
    loaded.resolved_terrain = sim.resolved_terrain.take();
    assert_state(&loaded, &row["after_load"], "full restore");
    assert_eq!(
        loaded.scenario_rng.logical_state(),
        SimRng::new(0).logical_state()
    );
    let rng = loaded.rng_state();
    loaded.update_unit_disguise(1, &retail.rules).unwrap();
    assert_state(&loaded, &first["after"], "restored unexpired visit");
    assert_eq!(loaded.rng_state(), rng);
}
