//! Original51BF90 counter0/1/2 and actual mode0/1 controls. The native
//! fixture supplies the same clear/rim cell and independently retained object
//! list/raw occupation priors; it does not certify whole Unlimbo or movement.

use super::*;
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::rng::SimRng;
use crate::sim::world::entry_test_fixture;
use crate::util::fixed_math::SimFixed;

#[test]
fn escape_counter_matches_native_cell_list_and_raw_owner_gates() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_scatter_entry_priority.json",
    ))
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_scatter_entry_priority.meta.json",
    ))
    .unwrap();
    assert_eq!(
        metadata["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 192);
    let structure_cases = corpus["structure_cases"].as_array().unwrap();
    assert_eq!(structure_cases.len(), 30);
    for row in cases.iter().chain(structure_cases) {
        let code_sections = row["native_executable_sections_unchanged"]
            .as_array()
            .unwrap();
        assert_eq!(code_sections.len(), 1);
        assert_eq!(
            code_sections[0]["sha256"],
            "4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc"
        );
        let input = &row["input"];
        let armed = input["armed"].as_bool().unwrap();
        let extra = format!(
            "[InfantryTypes]\n2=MOVER\n3=I_BLOCKER\n[VehicleTypes]\n0=U_BLOCKER\n\
             [MOVER]\nSpeedType=Foot\n{}\
             [BuildingTypes]\n1=B_BLOCKER\n[AircraftTypes]\n1=A_BLOCKER\n\
             [I_BLOCKER]\nSpeedType=Foot\n[U_BLOCKER]\nSpeedType=Track\n\
             [B_BLOCKER]\nGate={}\n[A_BLOCKER]\nSpeedType=Winged\n\
             [TESTGUN]\nDamage=1\nROF=20\nRange=5\nProjectile=TESTPROJECTILE\n\
             Warhead=SA\n[TESTPROJECTILE]\nAG=yes\n",
            if armed { "Primary=TESTGUN\n" } else { "" },
            input["gate"].as_bool().unwrap_or(false),
        );
        let (mut sim, rules, _) = entry_test_fixture::fixture_with_rules(&extra);
        let ours = sim.intern("Americans");
        let enemy = sim.intern("Russians");
        for house in [ours, enemy] {
            sim.houses
                .insert(house, HouseState::new(house, 0, None, false, 0, 10));
            sim.session.house_order.push(house);
        }
        sim.session.game_mode_nonzero = input["mode"].as_u64().unwrap() != 0;
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 16,
            off_fc: if input["rim"].as_bool().unwrap() {
                0
            } else {
                -16
            },
            off_100: if input["rim"].as_bool().unwrap() {
                0
            } else {
                -16
            },
            off_104: if input["rim"].as_bool().unwrap() {
                1
            } else {
                64
            },
            off_108: if input["rim"].as_bool().unwrap() {
                1
            } else {
                64
            },
        });
        let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        mover.owner = ours;
        mover.type_ref = sim.intern("MOVER");
        mover.category = EntityCategory::Infantry;
        mover.in_playfield = true;
        mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        sim.substrate.entities.insert(mover);
        sim.mission_assign_exact(90, crate::sim::mission::MissionId::from_raw(5), 0)
            .unwrap();
        if input["occupant"] != "none" {
            let kind = input["occupant_kind"].as_str().unwrap();
            let (name, category) = match kind {
                "infantry" => ("I_BLOCKER", EntityCategory::Infantry),
                "unit" => ("U_BLOCKER", EntityCategory::Unit),
                "building" => ("B_BLOCKER", EntityCategory::Structure),
                "aircraft" => ("A_BLOCKER", EntityCategory::Aircraft),
                _ => panic!("unexpected native blocker {kind}"),
            };
            let infantry = category == EntityCategory::Infantry;
            let mut blocker = GameEntity::test_default(91, name, "Russians", 11, 10);
            blocker.owner = enemy;
            blocker.type_ref = sim.intern(name);
            blocker.category = category;
            blocker.position.sub_x = SimFixed::from_num(128);
            blocker.position.sub_y = SimFixed::from_num(128);
            blocker.foot_occupation_enabled = true;
            let moving = input["occupant"] == "moving";
            if infantry {
                let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Walk);
                let LocomotorRuntimePayload::Walk(walk) = &mut locomotor.runtime_payload else {
                    unreachable!()
                };
                walk.moving = moving;
                blocker.locomotor = Some(locomotor);
            } else if category == EntityCategory::Unit {
                blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
                assert!(
                    blocker
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_drive_state_for_test(Some(
                            DriveLocomotionRuntime::default().with_destination_for_test(
                                moving.then_some(DriveCoord {
                                    x: 3200,
                                    y: 2688,
                                    z: 0,
                                })
                            )
                        ))
                );
            }
            let category = blocker.category;
            sim.substrate.entities.insert(blocker);
            sim.substrate.occupancy.add(
                11,
                10,
                91,
                MovementLayer::Ground,
                None,
                CellListInsertion::from_category(category),
            );
        }
        let bits = input["raw_bits"].as_u64().unwrap() as u8;
        if input["raw_owner"] == 1 {
            sim.substrate
                .raw_cell_occupation
                .mark_ground_infantry(11, 10, bits, enemy);
        } else {
            sim.substrate.raw_cell_occupation.mark_ground(11, 10, bits);
        }
        sim.scenario_rng = SimRng::new(31);
        sim.main_rng = SimRng::new(37);
        sim.mapgen_rng = SimRng::new(41);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((11, 10));
        let probe = |sim: &mut Simulation| {
            sim.foot_entry_receiver(90, &rules, None)
                .unwrap()
                .can_enter(cell, InfantryEntryArgs::REPAIR)
                .unwrap()
        };
        for (name, rng) in [
            ("scenario", &sim.scenario_rng),
            ("main", &sim.main_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert!(
                rng.native_state_hex() == row["rng_before"][name].as_str().unwrap(),
                "{input}: full {name} native prior"
            );
        }
        let result = match input["priority"].as_u64().unwrap() {
            0 => probe(&mut sim),
            1 => sim.with_object_placement_scope(probe),
            2 => sim.with_object_placement_scope(|sim| sim.with_object_placement_scope(probe)),
            _ => unreachable!(),
        };
        assert_eq!(
            u64::from(result),
            row["result"].as_u64().unwrap(),
            "{input}"
        );
        assert!(
            !sim.object_placement_scope_active(),
            "{input}: bracket cleanup"
        );
        for (name, rng) in [
            ("scenario", &sim.scenario_rng),
            ("main", &sim.main_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert!(
                rng.native_state_hex() == row["rng_after"][name].as_str().unwrap(),
                "{input}: full {name} after entry"
            );
        }
    }
}
