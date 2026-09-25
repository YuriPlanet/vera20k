//! Walk75C240 retained head and Infantry5217C0/521850 raw subcell leaves.
//! The raw bits and mark-time house are canonical; no later occupancy rebuild
//! reconstructs the history from the path or a changed terrain layer.
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::{
    components::DriveCoord,
    intern::InternedId,
    occupancy::{RawCellKey, RawCellOccupationGrid, infantry_raw_occupation_mask},
    pathfinding::PathGrid,
};
use crate::util::fixed_math::SimFixed;

/// The observable 481180 selection corridor. Priority skips even blockers
/// and RNG. Ground object occupation is checked regardless of selected plane;
/// its sole exception is a live, stable-open Gate (481298..481313).
pub(crate) fn select_slot(
    input: DriveCoord,
    priority: bool,
    selected_raw: u8,
    ground_raw: u8,
    ground_gate_open: bool,
    rng: &mut crate::sim::rng::SimRng,
) -> Option<u8> {
    use crate::sim::cell_kernel::{
        CellQueryPoint, infantry_preferred_spot, select_infantry_subcell,
    };
    let preferred = infantry_preferred_spot(CellQueryPoint {
        x: input.x,
        y: input.y,
    });
    if priority {
        return Some(preferred);
    }
    if selected_raw & 0x20 != 0 || (ground_raw & 0x40 != 0 && !ground_gate_open) {
        return None;
    }
    // The center path draws even when all three functional slots are full.
    let random_row = (preferred == 0).then(|| rng.next_range_u32(4) as u8);
    select_infantry_subcell(preferred, selected_raw, false, random_row)
}

pub(crate) fn selected_head(
    input: DriveCoord,
    slot: u8,
    input_ground_z: i32,
    bridge: bool,
) -> DriveCoord {
    let (x, y, _) = crate::sim::cell_kernel::infantry_subcell_offset(slot);
    DriveCoord {
        x: (input.x & !255).wrapping_add(x),
        y: (input.y & !255).wrapping_add(y),
        z: input_ground_z.wrapping_add(if bridge { 416 } else { 0 }),
    }
}

pub(crate) fn raw_at(
    raw: &mut RawCellOccupationGrid,
    owner: InternedId,
    coord: DriveCoord,
    put: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    fallback: Option<&PathGrid>,
) {
    let cell = ((coord.x / 256) as u16, (coord.y / 256) as u16);
    // The native leaves select the Cell before ground height and raw plane.
    let identity = terrain.map(|t| t.native_cell_identity((cell.0 as i16, cell.1 as i16)));
    let key = terrain
        .zip(identity)
        .map_or(RawCellKey::Real(cell.0, cell.1), |(t, c)| {
            RawCellKey::from_native(t, c)
        });
    let ground =
        super::ground_pose::ground_surface_z_at([coord.x, coord.y], false, terrain, fallback)
            .unwrap_or(coord.z);
    let structural = terrain.zip(identity).map_or_else(
        || {
            fallback
                .and_then(|g| g.cell(cell.0, cell.1))
                .is_some_and(|c| c.bridge_deck_level_if_any().is_some())
        },
        |(t, c)| t.native_cell_flags(c) & 0x100 != 0,
    );
    let deck = coord.z >= ground.wrapping_add(416) && (!put || structural);
    let mask = infantry_raw_occupation_mask(
        SimFixed::from_num(coord.x % 256),
        SimFixed::from_num(coord.y % 256),
    );
    let layer = if deck {
        super::locomotor::MovementLayer::Bridge
    } else {
        super::locomotor::MovementLayer::Ground
    };
    raw.write_infantry(key, layer, mask, owner, put);
}

/// Successful fresh-head tail75BC2A..75BCBD. Facing+4C is75AE00 ->
/// FacingClass4C9300; owner+544 is4D3710. These owners are independent from
/// the physical movement adapter and the later Infantry Doing consumer.
pub(super) fn finish_fresh_head(
    entity: &mut crate::sim::game_entity::GameEntity,
    native_frame: u32,
) -> bool {
    let Some(loco) = entity.locomotor.as_mut() else {
        return false;
    };
    if loco.kind != crate::rules::locomotor_type::LocomotorKind::Walk {
        return false;
    }
    let Some(head) = loco.step_head() else {
        return false;
    };
    loco.begin_walk_motion();
    if entity.lifecycle.object_alive {
        let current = super::ground_pose::position_world_coord(&entity.position);
        let facing = crate::util::direction_tables::facing16_from_delta(
            head.x.wrapping_sub(current.x),
            head.y.wrapping_sub(current.y),
        );
        // Class constructor owns ROT, even for a Unit using the Walk GUID.
        let rot = if entity.category == crate::map::entities::EntityCategory::Infantry {
            127 // Infantry ctor517BBD; Unit ctor735570 uses Type ROT.
        } else {
            loco.rot
        };
        entity
            .body_facing
            .get_or_insert_with(|| super::FacingClass::new(0, rot))
            .snap(facing, native_frame);
        entity.facing = (facing >> 8) as u8;
        entity.facing_target = None;
        entity.foot_speed.applied_fraction = crate::util::fixed_math::SIM_ONE;
    }
    true
}

impl crate::sim::world::Simulation {
    /// Foot4DB260's first Limbo invokes ILocomotion+9C(0). Walk75CA30
    /// queries the retained head (75AC00 falls back to current XYZ) and calls
    /// Infantry+F4. It does not retire the stored head. Repeated Limbo skips
    /// this destructive raw clear, including when another walker reused it.
    pub(crate) fn release_walk_occupation_before_foot_limbo(&mut self, id: u64) {
        let Some((owner, coord)) = self.substrate.entities.get(id).and_then(|e| {
            if e.lifecycle.in_limbo {
                return None;
            }
            let loco = e.locomotor.as_ref()?;
            if loco.kind != crate::rules::locomotor_type::LocomotorKind::Walk {
                return None;
            }
            Some((
                e.owner(),
                loco.step_head()
                    .unwrap_or_else(|| super::ground_pose::position_world_coord(&e.position)),
            ))
        }) else {
            return;
        };
        raw_at(
            &mut self.substrate.raw_cell_occupation,
            owner,
            coord,
            false,
            self.resolved_terrain.as_ref(),
            self.path_grid.as_deref(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slave_priority_reserves_head_through_live_master_occupation_without_rng() {
        use super::super::locomotor::{LocomotorState, MovementLayer};
        use crate::map::entities::EntityCategory;
        use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind, ruleset::RuleSet};
        use crate::sim::{
            components::MovementTarget,
            entity_store::EntityStore,
            game_entity::GameEntity,
            intern::StringInterner,
            occupancy::{CellListInsertion, OccupancyGrid},
            slave_manager::SlaveManager,
        };
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=MASTER\n[MASTER]\nFoundation=1x1\n[InfantryTypes]\n0=SLAV\n",
        ))
        .unwrap();
        let mut interner = StringInterner::new();
        let owner = interner.intern("Owner");
        let mut entities = EntityStore::new();
        let mut master = GameEntity::test_default(1, "MASTER", "Owner", 4, 4);
        master.owner = owner;
        master.type_ref = interner.intern("MASTER");
        master.category = EntityCategory::Structure;
        let slav = interner.intern("SLAV");
        // An existing manager that does not hold the slave refuses it.
        master.slave_manager = Some(SlaveManager::new(slav, [None; 0], 0, 0, 0));
        entities.insert(master);
        let mut slave = GameEntity::test_default(2, "SLAV", "Owner", 3, 4);
        slave.owner = owner;
        slave.type_ref = interner.intern("SLAV");
        slave.category = EntityCategory::Infantry;
        slave.position.sub_x = SimFixed::from_num(192);
        slave.position.sub_y = SimFixed::from_num(64);
        slave.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        slave.movement_target = Some(MovementTarget {
            path: vec![(3, 4), (4, 4)],
            next_index: 1,
            ..Default::default()
        });
        slave.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(1), Vec::new());
        let current = super::super::ground_pose::position_world_coord(&slave.position);
        entities.insert(slave);
        let terrain = ResolvedTerrainGrid::from_cells(
            8,
            8,
            (0..8)
                .flat_map(|y| {
                    (0..8).map(move |x| {
                        crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false)
                    })
                })
                .collect(),
        );
        let mut occupancy = OccupancyGrid::default();
        occupancy.add(
            4,
            4,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let mut raw = RawCellOccupationGrid::default();
        raw.mark_ground(4, 4, 0x40);
        raw_at(&mut raw, owner, current, true, Some(&terrain), None);
        let mut rng = crate::sim::rng::SimRng::new(31);
        let rng_before = rng.logical_state();
        assert!(!prepare_step_head(
            &mut entities,
            2,
            &occupancy,
            &mut raw,
            Some(&terrain),
            None,
            Some(&rules),
            &interner,
            &mut rng
        ));
        assert_eq!(
            raw.ground_bits(3, 4) & 0x1c,
            4,
            "failed selection restores the cleared current slot"
        );
        entities.get_mut(1).unwrap().slave_manager =
            Some(SlaveManager::new(slav, [Some(2)], 0, 0, 0));
        assert!(prepare_step_head(
            &mut entities,
            2,
            &occupancy,
            &mut raw,
            Some(&terrain),
            None,
            Some(&rules),
            &interner,
            &mut rng
        ));
        assert_eq!(
            entities
                .get(2)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .step_head(),
            Some(DriveCoord {
                x: 4 * 256 + 192,
                y: 4 * 256 + 64,
                z: 0
            })
        );
        assert_eq!(raw.ground_bits(3, 4) & 0x1c, 0);
        assert_eq!(raw.ground_infantry_owner(3, 4), None);
        assert_eq!(raw.ground_bits(4, 4), 0x44);
        assert_eq!(raw.ground_infantry_owner(4, 4), Some(owner));
        assert_eq!(
            rng.logical_state(),
            rng_before,
            "priority skips placement RNG"
        );
    }

    fn coord(v: &serde_json::Value) -> DriveCoord {
        DriveCoord {
            x: v[0].as_i64().unwrap() as i32,
            y: v[1].as_i64().unwrap() as i32,
            z: v[2].as_i64().unwrap() as i32,
        }
    }

    #[test]
    fn placement_and_raw_history_match_original_infantry_bodies() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/walk_head_occupation.json"
        ))
        .unwrap();
        let selections = data["selection"].as_array().unwrap();
        assert_eq!(selections.len(), 176);
        let mut terrain = ResolvedTerrainGrid::from_cells(
            11,
            11,
            (0..11)
                .flat_map(|y| {
                    (0..11).map(move |x| {
                        crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false)
                    })
                })
                .collect(),
        );
        let c = terrain.cell_mut(10, 10).unwrap();
        c.level = 2;
        c.slope_type = 1;
        for row in selections {
            let i = &row["input"];
            let out = &row["output"];
            let input = coord(&i["input"]);
            let bridge = i["bridge"].as_bool().unwrap();
            let ground = i["ground"].as_u64().unwrap() as u8;
            let deck = i["deck"].as_u64().unwrap() as u8;
            let mut rng = crate::sim::rng::SimRng::new(i["seed"].as_u64().unwrap());
            let slot = select_slot(
                input,
                i["priority"].as_bool().unwrap(),
                if bridge { deck } else { ground },
                ground,
                i["gate"] == 2,
                &mut rng,
            );
            let z = super::super::ground_pose::ground_surface_z_at(
                [input.x, input.y],
                false,
                Some(&terrain),
                None,
            )
            .unwrap();
            let actual = slot.map(|s| selected_head(input, s, z, bridge));
            let expected = coord(&out["head"]);
            assert_eq!(
                actual,
                (expected != DriveCoord { x: 0, y: 0, z: 0 }).then_some(expected),
                "{i}"
            );
            assert_eq!(
                rng.logical_view().index_a,
                out["random_indices"][0].as_i64().unwrap() as i32,
                "{i}"
            );
            assert_eq!(
                rng.logical_view().index_b,
                out["random_indices"][1].as_i64().unwrap() as i32,
                "{i}"
            );
        }
        let rows = data["raw"].as_array().unwrap();
        assert_eq!(rows.len(), 160);
        for row in rows {
            let i = &row["input"];
            let out = &row["output"];
            terrain.cell_mut(10, 10).unwrap().bridge_facts.raw_flags =
                if i["structural"].as_bool().unwrap() {
                    0x100
                } else {
                    0
                };
            let mut raw = RawCellOccupationGrid::default();
            raw.mark_ground_infantry(
                10,
                10,
                i["ground"].as_u64().unwrap() as u8,
                InternedId::from_index(41),
            );
            raw.mark_deck_infantry(
                10,
                10,
                i["deck"].as_u64().unwrap() as u8,
                InternedId::from_index(42),
            );
            raw_at(
                &mut raw,
                InternedId::from_index(i["owner"].as_u64().unwrap() as u32),
                coord(&i["input"]),
                i["put"].as_bool().unwrap(),
                Some(&terrain),
                None,
            );
            assert_eq!(
                raw.ground_bits(10, 10),
                out["ground"].as_u64().unwrap() as u8,
                "{i}"
            );
            assert_eq!(
                raw.deck_bits(10, 10),
                out["deck"].as_u64().unwrap() as u8,
                "{i}"
            );
            for (index, layer) in [
                crate::sim::movement::locomotor::MovementLayer::Ground,
                crate::sim::movement::locomotor::MovementLayer::Bridge,
            ]
            .into_iter()
            .enumerate()
            {
                let expected = out["owners"][index].as_u64().unwrap() as u32;
                assert_eq!(
                    raw.infantry_owner(10, 10, layer),
                    (expected != u32::MAX).then(|| InternedId::from_index(expected)),
                    "{i}"
                );
            }
        }
    }

    fn assert_native_rng(rng: &crate::sim::rng::SimRng, hex: &str) {
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let view = rng.logical_view();
        assert_eq!(view.disabled, bytes[0]);
        // Native padding bytes1..3 have no Rust state authority.
        assert_eq!(view.index_a, word(4) as i32);
        assert_eq!(view.index_b, word(8) as i32);
        let expected: Vec<u32> = (0..250).map(|i| word(12 + i * 4)).collect();
        assert_eq!(view.words, expected.as_slice());
    }

    #[test]
    fn fresh_walk_head_effects_and_rng_match_original_process() {
        use crate::sim::{
            components::{FootPathQueue, MovementTarget, NavTargetRef},
            entity_store::EntityStore,
            game_entity::GameEntity,
            intern::StringInterner,
            mission::{MissionId, state::MissionTestFixture},
            occupancy::OccupancyGrid,
        };
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/walk_first_step.json"
        ))
        .unwrap();
        let terrain = ResolvedTerrainGrid::from_cells(
            11,
            11,
            (0..11)
                .flat_map(|y| {
                    (0..11).map(move |x| {
                        let mut cell =
                            crate::sim::world::common_raw_test_terrain_cell(x, y, 2, false);
                        cell.slope_type = 1;
                        cell
                    })
                })
                .collect(),
        );
        for row in corpus["ready_path"].as_array().unwrap() {
            let input = &row["input"];
            let mut entity = GameEntity::test_default(1, "E1", "Owner", 9, 10);
            entity.category = crate::map::entities::EntityCategory::Infantry;
            if !input["infantry_constructor_facing"]
                .as_bool()
                .unwrap_or(false)
            {
                // Retain the older corpus's explicitly supplied rate0 owner;
                // the new constructor rows exercise lazy Infantry rate127.
                entity.body_facing = Some(super::super::FacingClass::new(0, 0));
            }
            entity.owner = InternedId::from_index(41);
            entity.position.sub_x = SimFixed::from_num(input["sub"][0].as_i64().unwrap());
            entity.position.sub_y = SimFixed::from_num(input["sub"][1].as_i64().unwrap());
            entity.position.exact_z_leptons = Some(260);
            entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ));
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .set_walk_destination(Some(DriveCoord {
                    x: 2688,
                    y: 2688,
                    z: 260,
                }));
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(input["mission"].as_i64().unwrap_or(1) as i32),
                queued: MissionId::NONE,
                suspended: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
            entity.navigation.path_replay = FootPathQueue {
                directions: vec![2, 3, 4, 5],
                cursor: 0,
                reference_cell: Some((9, 8)),
            };
            if input["nav"].as_bool().unwrap_or(true) {
                entity.navigation.nav_com = Some(NavTargetRef::Cell { rx: 10, ry: 10 });
            }
            entity.movement_target = Some(MovementTarget {
                path: vec![(9, 10), (10, 10)],
                next_index: 1,
                ..Default::default()
            });
            let current = super::super::ground_pose::position_world_coord(&entity.position);
            let mut raw = RawCellOccupationGrid::default();
            raw_at(
                &mut raw,
                entity.owner(),
                current,
                true,
                Some(&terrain),
                None,
            );
            assert_eq!(
                raw.ground_bits(9, 10),
                row["initial_raw"].as_u64().unwrap() as u8
            );
            let mut entities = EntityStore::new();
            entities.insert(entity);
            let mut rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap_or(31));
            assert_native_rng(&rng, row["rng_before"].as_str().unwrap());
            assert!(prepare_step_head(
                &mut entities,
                1,
                &OccupancyGrid::default(),
                &mut raw,
                Some(&terrain),
                None,
                None,
                &StringInterner::new(),
                &mut rng
            ));
            let entity = entities.get_mut(1).unwrap();
            assert!(finish_fresh_head(entity, 100));
            assert_eq!(
                super::super::ground_pose::position_world_coord(&entity.position),
                coord(&row["current"])
            );
            assert_eq!(
                entity.locomotor.as_ref().unwrap().step_head(),
                Some(coord(&row["head"]))
            );
            assert_eq!(
                entity.locomotor.as_ref().unwrap().walk_animation_moving(),
                Some(row["motion"] == 1)
            );
            assert_eq!(
                serde_json::to_value(entity.body_facing.unwrap()).unwrap(),
                row["facing"]
            );
            assert_eq!(
                entity.foot_speed.applied_fraction,
                SimFixed::from_num(row["speed_fraction"].as_f64().unwrap())
            );
            assert_eq!(entity.navigation.path_replay.directions, vec![2, 3, 4, 5]);
            assert_eq!(entity.navigation.path_replay.reference_cell, Some((9, 8)));
            assert_eq!(
                raw.ground_bits(9, 10),
                row["current_raw"].as_u64().unwrap() as u8
            );
            assert_eq!(
                raw.ground_bits(10, 10),
                row["head_raw"].as_u64().unwrap() as u8
            );
            assert_native_rng(&rng, row["rng_after"].as_str().unwrap());
        }
        for row in corpus["completed_queue"].as_array().unwrap() {
            let mut queue = FootPathQueue {
                directions: row["before"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap() as u8)
                    .collect(),
                cursor: 0,
                reference_cell: Some((9, 8)),
            };
            super::super::path_markers::consume_walk_path_replay(&mut queue);
            let expected: Vec<u8> = row["after"]
                .as_array()
                .unwrap()
                .iter()
                .take(23)
                .map(|v| v.as_i64().unwrap() as u8)
                .collect();
            assert_eq!(
                &queue.directions[usize::from(queue.cursor)..],
                expected.as_slice()
            );
            assert_eq!(queue.reference_cell, Some((9, 8)));
            assert_eq!(
                queue.remaining_directions().is_empty(),
                row["invalidated"].as_bool().unwrap()
            );
        }
    }

    #[test]
    fn releasing_last_walk_slot_clears_only_its_plane_owner() {
        let mut raw = RawCellOccupationGrid::default();
        let owner = InternedId::from_index(1);
        let other = InternedId::from_index(2);
        let coord = DriveCoord {
            x: 3 * 256 + 192,
            y: 4 * 256 + 64,
            z: 0,
        };
        raw_at(&mut raw, owner, coord, true, None, None);
        raw.mark_ground_infantry(3, 4, 8, other);
        raw.mark_deck_infantry(3, 4, 4, owner);
        raw_at(&mut raw, owner, coord, false, None, None);
        assert_eq!(raw.ground_bits(3, 4), 8);
        assert_eq!(raw.ground_infantry_owner(3, 4), Some(other));
        raw_at(
            &mut raw,
            other,
            DriveCoord {
                x: 3 * 256 + 64,
                y: 4 * 256 + 192,
                z: 0,
            },
            false,
            None,
            None,
        );
        assert_eq!(raw.ground_infantry_owner(3, 4), None);
        assert_eq!(raw.deck_bits(3, 4), 4);
    }
}

/// Accepted-head producer with short entity borrows. Raw leaves and map
/// lookups can run in their native order while querying other live objects.
/// Pre-head CanEnter75B690 remains the caller's responsibility.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_step_head(
    entities: &mut crate::sim::entity_store::EntityStore,
    id: u64,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    raw: &mut RawCellOccupationGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    grid: Option<&PathGrid>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
) -> bool {
    use super::locomotor::MovementLayer;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    let Some(entity) = entities.get(id) else {
        return false;
    };
    let Some(loco) = entity.locomotor.as_ref() else {
        return true;
    };
    if !matches!(loco.kind, LocomotorKind::Walk | LocomotorKind::Hover)
        || loco.step_head().is_some()
    {
        return true;
    }
    let Some(next) = entity
        .movement_target
        .as_ref()
        .and_then(|t| t.path.get(t.next_index))
        .copied()
    else {
        return true;
    };
    let is_walk = loco.kind == LocomotorKind::Walk;
    let owner = entity.owner();
    let current = super::ground_pose::position_world_coord(&entity.position);
    let input = DriveCoord {
        x: i32::from(next.0) * 256 + if is_walk { current.x % 256 } else { 128 },
        y: i32::from(next.1) * 256 + if is_walk { current.y % 256 } else { 128 },
        z: current.z,
    };
    let (head, sub) = if is_walk {
        //75C2A0 first; even a failed chooser must restore current raw at75C62A.
        raw_at(raw, owner, current, false, terrain, grid);
        let mut priority = false;
        if [7, 8, 9, 11, 25].contains(&entity.mission.current().raw()) {
            let target = match entity.navigation.nav_com {
                Some(
                    crate::sim::components::NavTargetRef::Entity { id }
                    | crate::sim::components::NavTargetRef::Object { id }
                    | crate::sim::components::NavTargetRef::Building { id },
                ) => entities.get(id),
                _ => None,
            };
            if let Some(target) = target {
                priority = match target.category {
                    EntityCategory::Unit | EntityCategory::Aircraft => {
                        let c = super::ground_pose::position_world_coord(&target.position);
                        ((c.x / 256) as i16, (c.y / 256) as i16)
                            == ((input.x / 256) as i16, (input.y / 256) as i16)
                    }
                    EntityCategory::Structure => {
                        let cell = terrain.map_or(next, |t| {
                            let cell = t.native_cell_identity((
                                (input.x / 256) as i16,
                                (input.y / 256) as i16,
                            ));
                            let c = t.native_cell_coord(cell);
                            (c.0 as u16, c.1 as u16)
                        });
                        occupancy.first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
                            == Some(target.stable_id())
                    }
                    EntityCategory::Infantry => false,
                };
            }
        }
        //75C434 still executes when the ordinary target already granted
        //priority. Its map lookups cannot be elided by boolean short-circuit.
        if let (Some(terrain), Some(rules)) = (terrain, rules) {
            let query = crate::sim::slave_deposit::SlaveDepositQuery {
                entities,
                occupancy,
                terrain,
                rules,
                interner,
            };
            let slave_priority = query.walk_priority(id, input);
            priority |= slave_priority;
        }
        //75C4C8 reloads input Cell after the complete priority corridor.
        let (cell, structural, raw_key) = terrain.map_or_else(
            || {
                (
                    next,
                    grid.and_then(|g| g.cell(next.0, next.1))
                        .is_some_and(|c| c.bridge_structural),
                    RawCellKey::Real(next.0, next.1),
                )
            },
            |t| {
                let selected =
                    t.native_cell_identity(((input.x / 256) as i16, (input.y / 256) as i16));
                let c = t.native_cell_coord(selected);
                (
                    (c.0 as u16, c.1 as u16),
                    t.native_cell_flags(selected) & 0x100 != 0,
                    RawCellKey::from_native(t, selected),
                )
            },
        );
        let bridge = structural
            && current.z
                > super::ground_pose::ground_surface_z_at([input.x, input.y], false, terrain, grid)
                    .unwrap_or(current.z)
                    .wrapping_add(312);
        let ground_raw = raw.bits_at(raw_key, MovementLayer::Ground);
        let selected_raw = if bridge {
            raw.bits_at(raw_key, MovementLayer::Bridge)
        } else {
            ground_raw
        };
        let gate_open = if !priority && selected_raw & 0x20 == 0 && ground_raw & 0x40 != 0 {
            occupancy
                .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
                .and_then(|id| entities.get(id))
                .is_some_and(|b| {
                    rules
                        .and_then(|r| r.object(interner.resolve(b.type_ref())))
                        .is_some_and(|t| t.gate)
                        && b.building_gate.is_some_and(|g| g.can_garrison_passable())
                })
        } else {
            false
        };
        let Some(slot) = select_slot(input, priority, selected_raw, ground_raw, gate_open, rng)
        else {
            raw_at(raw, owner, current, true, terrain, grid);
            return false;
        };
        let ground =
            super::ground_pose::ground_surface_z_at([input.x, input.y], false, terrain, grid)
                .unwrap_or(current.z);
        (
            selected_head(input, slot, ground, bridge),
            Some(crate::util::lepton::subcell_lepton_offset(Some(slot))),
        )
    } else {
        //Hover515380..5153D5 retains head Z; the threshold has no extra
        //structural-cell gate and uses the existing GetHeight projection.
        let ground =
            super::ground_pose::ground_surface_z_at([input.x, input.y], false, terrain, grid)
                .unwrap_or(current.z);
        let bridge =
            current.z.wrapping_add(loco.altitude.to_num::<i32>()) >= ground.wrapping_add(312);
        (
            DriveCoord {
                x: input.x,
                y: input.y,
                z: ground.wrapping_add(if bridge { 416 } else { 0 }),
            },
            None,
        )
    };
    let Some(loco) = entities.get_mut(id).and_then(|e| e.locomotor.as_mut()) else {
        return false;
    };
    if let Some(sub) = sub {
        loco.subcell_dest = Some(sub);
    }
    loco.set_step_head(Some(head));
    if is_walk {
        raw_at(raw, owner, head, true, terrain, grid);
    }
    true
}
