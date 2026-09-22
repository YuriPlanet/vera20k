use super::*;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::occupancy::CellListInsertion;

#[test]
fn unit_entry_preserves_original_numeric_results_and_repair_projection() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/unit_entry.json"
    ))
    .unwrap();
    let mut mismatches = Vec::new();
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let mut extra = format!(
            "[VehicleTypes]\n0=MOVER\n1=BLOCKER\n[BuildingTypes]\n1=BUILDING\n[MOVER]\nSpeedType=Track\nCrusher={}\nOmniCrusher={}\nMovementZone={}\n{}[BLOCKER]\nSpeedType=Track\n[BUILDING]\nFoundation=1x1\nGate={}\n[TESTGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJECTILE\nWarhead=SA\n[TESTPROJECTILE]\nAG={}\n[SA]\nWall={}\n",
            flag("crusher"),
            flag("omni"),
            if flag("crusher_all") {
                "CrusherAll"
            } else {
                "Normal"
            },
            if flag("armed") {
                "Primary=TESTGUN\n"
            } else {
                ""
            },
            input["objects"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|node| node["gate"].as_bool().unwrap_or(false)),
            input["ag"].as_bool().unwrap_or(true),
            flag("wall_weapon"),
        );
        if input.get("wall").is_some() {
            extra.push_str(&format!("[O0]\nWall=yes\nCrushable={}\n", flag("wall")));
        }
        let (mut sim, rules, registry) = super::super::super::tests::fixture_with_rules(&extra);
        sim.session.binary_frame = 100;
        let ours = sim.intern("Americans");
        let enemy = sim.intern("Russians");
        let mut mover = GameEntity::test_default(90, "MOVER", "Americans", 10, 10);
        mover.owner = ours;
        mover.type_ref = sim.intern("MOVER");
        mover.category = EntityCategory::Unit;
        mover.in_playfield = false;
        mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(mover);
        sim.mission_assign_exact(
            90,
            crate::sim::mission::MissionId::from_raw(input["mission"].as_i64().unwrap_or(5) as i32),
            100,
        )
        .unwrap();
        let mut ids = Vec::new();
        for (index, node) in input["objects"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let id = if node["self"].as_bool().unwrap_or(false) {
                90
            } else {
                let id = 91 + index as u64;
                let building = node["building"].as_bool().unwrap_or(false);
                let name = if building { "BUILDING" } else { "BLOCKER" };
                let mut blocker = GameEntity::test_default(id, name, "Americans", 11, 10);
                blocker.type_ref = sim.intern(name);
                blocker.owner = if node["enemy"].as_bool().unwrap_or(false) {
                    enemy
                } else {
                    ours
                };
                blocker.category = if building {
                    EntityCategory::Structure
                } else {
                    EntityCategory::Unit
                };
                blocker.crushable = node["crushable"].as_bool().unwrap_or(false);
                blocker.omni_crush_resistant = node["resistant"].as_bool().unwrap_or(false);
                blocker.foot_occupation_enabled = node["occupation"].as_bool().unwrap_or(true);
                if !building {
                    blocker.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
                }
                if node["nav"].as_bool().unwrap_or(false) {
                    blocker.navigation.nav_com = Some(NavTargetRef::cell(11, 10));
                }
                if node["turn"].as_bool().unwrap_or(false) {
                    let mut facing = crate::sim::movement::FacingClass::new(0, 5);
                    facing.set(0x4000, 100);
                    blocker.body_facing = Some(facing);
                }
                if node["gate"].as_bool().unwrap_or(false) {
                    let opened = node["open"].as_bool().unwrap_or(false);
                    blocker.building_gate = Some(crate::sim::game_entity::BuildingGateRuntime {
                        mission_18_active: opened,
                        phase: if opened {
                            crate::sim::game_entity::BuildingGatePhase::OpenStable
                        } else {
                            crate::sim::game_entity::BuildingGatePhase::ClosedStable
                        },
                        ..Default::default()
                    });
                }
                if node["cloaked"].as_bool().unwrap_or(false) {
                    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(100, 9);
                    cloak.establish_unlimbo_fully_cloaked();
                    blocker.cloak = Some(cloak);
                }
                sim.substrate.entities.insert(blocker);
                id
            };
            ids.push(id);
        }
        if let Some(index) = input["nav_target"].as_u64() {
            sim.substrate
                .entities
                .get_mut(90)
                .unwrap()
                .navigation
                .nav_com = Some(NavTargetRef::Entity {
                id: ids[index as usize],
            });
        }
        for id in ids {
            sim.substrate.occupancy.add(
                11,
                10,
                id,
                if flag("deck") {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                },
                None,
                // Supplied native list order, independent of Unlimbo's
                // prepend/append lifecycle producers (outside this corpus).
                CellListInsertion::AppendBuilding,
            );
        }
        if flag("deck") {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .raw_flags = 0x100;
        }
        if input.get("wall").is_some() {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(11, 10)
                .unwrap()
                .bridge_facts
                .overlay_id = Some(0);
            if let Some(owner) = match input["wall_owner"].as_i64().unwrap_or(-1) {
                0 => Some(ours),
                1 => Some(enemy),
                _ => None,
            } {
                sim.overlay_grid
                    .as_mut()
                    .unwrap()
                    .set_wall_owner(11, 10, owner);
            }
        }
        for (bits_key, owner_key, layer) in [
            ("bits", "owner", MovementLayer::Ground),
            ("deck_bits", "deck_owner", MovementLayer::Bridge),
        ] {
            let bits = input[bits_key].as_u64().unwrap_or(0) as u8;
            let owner = match input[owner_key].as_i64().unwrap_or(-1) {
                -1 => None,
                0 => Some(ours),
                1 => Some(enemy),
                _ => unreachable!(),
            };
            let raw = &mut sim.substrate.raw_cell_occupation;
            match (layer, owner) {
                (MovementLayer::Ground, None) => raw.mark_ground(11, 10, bits),
                (MovementLayer::Ground, Some(owner)) => {
                    raw.mark_ground_infantry(11, 10, bits, owner)
                }
                (MovementLayer::Bridge, None) => raw.mark_deck(11, 10, bits),
                (MovementLayer::Bridge, Some(owner)) => raw.mark_deck_infantry(11, 10, bits, owner),
                _ => unreachable!(),
            }
        }
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((11, 10));
        let mut live = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        let expected = row["result"].as_u64().unwrap() as u8;
        let actual = foot_entry(
            &mut live,
            CellObjectMember::Entity(90),
            cell,
            InfantryEntryArgs::REPAIR,
        );
        let repair = impassable(&mut live, CellObjectMember::Entity(90), cell);
        if actual != Ok(expected) || repair != Ok(expected == 7) {
            mismatches.push(format!(
                "{input}: expected{expected}, actual{actual:?}, repair{repair:?}"
            ));
        }
    }
    assert_eq!(rows.as_array().unwrap().len(), 150);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
