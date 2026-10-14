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
        z: input_ground_z.wrapping_add(if bridge {
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        }),
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
    let deck = coord.z >= ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS)
        && (!put || structural);
    let mask = infantry_raw_occupation_mask(
        SimFixed::from_num(coord.x % 256),
        SimFixed::from_num(coord.y % 256),
    );
    let layer = if deck {
        super::locomotor::MovementLayer::Bridge
    } else {
        super::locomotor::MovementLayer::Ground
    };
    raw.write_occupant(key, layer, mask, Some(owner), put);
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
    if entity.lifecycle.object_alive() {
        let current = super::ground_pose::position_world_coord(&entity.position);
        let facing = crate::util::direction_tables::facing16_from_delta(
            head.x.wrapping_sub(current.x),
            head.y.wrapping_sub(current.y),
        );
        super::locomotor_turn::walk_do_turn(entity, facing, native_frame);
        entity
            .foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
        //75BC36's dead-owner exit bypasses75BCB2 and retains the exact byte.
        // Native controls: foot_scold_latch.json dead_fresh_head rows.
        entity.navigation.path_runtime.clear_scold_latch();
    }
    true
}

impl crate::sim::world::Simulation {
    /// Foot4DB260's first Limbo invokes ILocomotion+9C(0). Walk75CA30
    /// queries the retained head (75AC00 falls back to current XYZ) and calls
    /// Infantry+F4. It does not retire the stored head; the Lock (+0xB0,
    /// `LocomotorState::walk_lock`) that follows it does. Repeated Limbo skips
    /// this destructive raw clear, including when another walker reused it.
    pub(crate) fn release_walk_occupation_before_foot_limbo(&mut self, id: u64) {
        let Some((owner, coord)) = self.substrate.entities.get(id).and_then(|e| {
            if e.lifecycle.in_limbo() {
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
        #[cfg(test)]
        self.trace_lifecycle_for_test(crate::sim::world::LifecycleTestEvent::RawOccupationCleared);
    }

    /// Walk FindSubCellDest (`0x0075C240`), the one port for its three
    /// callers: ProcessMovement's fresh head (`0x0075BC1A`) and arrival
    /// (`0x0075BE18`, a null request) and Force_Immediate_Destination
    /// (`0x0075AE5D`). [`select_step_head_at`], then the crate question on a
    /// stored head (`0x0075C565..0x0075C5A6`): a false answer outside limbo
    /// resets the head to the null coordinate, and a dead owner then answers 0
    /// without the Mark tail; then the Mark tail ([`commit_walk_head`]).
    pub(crate) fn walk_find_sub_cell_dest(
        &mut self,
        id: u64,
        request: Option<DriveCoord>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: crate::sim::world::FrameEffects<'_>,
    ) -> Result<bool, String> {
        let grid = self.path_grid_snapshot();
        let selection = select_step_head_at(
            &mut self.substrate.entities,
            id,
            &self.substrate.occupancy,
            &mut self.substrate.raw_cell_occupation,
            self.resolved_terrain.as_ref(),
            grid.as_deref(),
            rules,
            &self.interner,
            &mut self.scenario_rng,
            request,
        );
        if let StepHead::Done(answer) = selection {
            return Ok(answer);
        }
        let head = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.locomotor.as_ref())
            .and_then(|loco| loco.step_head())
            .ok_or("Walk head vanished before its crate question")?;
        let picked = self.pickup_crate_at(
            id,
            super::foot_path::coord_cell(head),
            rules,
            registry,
            frame_effects,
        );
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk head owner")?;
        if !picked && !actor.lifecycle.in_limbo() {
            if let Some(loco) = actor.locomotor.as_mut() {
                loco.set_step_head(None);
            }
            if !actor.lifecycle.object_alive() {
                return Ok(false);
            }
        }
        Ok(commit_walk_head(
            &self.substrate.entities,
            id,
            &mut self.substrate.raw_cell_occupation,
            self.resolved_terrain.as_ref(),
            grid.as_deref(),
        ))
    }

    /// The Foot's Locomotor (`+0x674`) Force_Immediate_Destination
    /// (ILocomotion +0x78). Walk's (`0x0075AE30`) reads its IsMoving byte
    /// (+0x30), runs [`Self::walk_find_sub_cell_dest`] on `coord` (`None` is
    /// the null coordinate) and then, with both its head and its destination
    /// null, clears IsMoving and calls the owner's +0x54C when the byte had
    /// been set (`0x0075AE5B..0x0075AE9A`). Every other VERA locomotor holds
    /// the base slot, `ret 0x10` (`0x0055AC00`); Mech's (`0x005B01A0`) has no
    /// VERA locomotor.
    pub(crate) fn foot_force_immediate_destination(
        &mut self,
        id: u64,
        coord: Option<DriveCoord>,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: crate::sim::world::FrameEffects<'_>,
    ) -> Result<(), String> {
        let Some(was_moving) = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.locomotor.as_ref())
            .and_then(|loco| loco.walk_is_moving())
        else {
            return Ok(());
        };
        self.walk_find_sub_cell_dest(id, coord, rules, registry, frame_effects)?;
        let Some(loco) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.locomotor.as_mut())
        else {
            return Ok(());
        };
        if loco.step_head().is_none() && loco.walk_destination().is_none() {
            // With no head, a null MoveTo writes only IsMoving.
            loco.set_walk_destination(None);
            if was_moving {
                self.infantry_pending_deploy_stop_callback(id, rules, frame_effects)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_fresh_head_preserves_native_scold_byte() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/foot_scold_latch.json",
        ))
        .unwrap();
        let mut checked = 0;
        for row in corpus["paid_tails"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["case"] == "dead_fresh_head")
        {
            let mut actor =
                crate::sim::game_entity::GameEntity::test_default(1, "E1", "Owner", 9, 10);
            actor.lifecycle.set_object_alive_for_test(false);
            actor.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Walk,
            ));
            actor
                .locomotor
                .as_mut()
                .unwrap()
                .set_step_head(Some(DriveCoord::cell(10, 10, 0)));
            actor
                .foot_speed
                .set_speed_fraction(SimFixed::from_num(0.75));
            actor
                .navigation
                .path_runtime
                .set_scold_latch_for_test(row["supplied_byte"].as_u64().unwrap() as u8);
            let body_before = actor.body_facing;
            assert!(finish_fresh_head(&mut actor, 100));
            assert_eq!(
                u64::from(actor.navigation.path_runtime.scold_latch_raw()),
                row["final_byte"].as_u64().unwrap()
            );
            assert_eq!(
                actor.locomotor.as_ref().unwrap().walk_animation_moving(),
                Some(row["motion"] == 1)
            );
            assert_eq!(
                actor.foot_speed.applied_fraction(),
                SimFixed::from_num(row["speed_fraction"].as_f64().unwrap())
            );
            assert_eq!(actor.body_facing, body_before, "a dead owner turns nothing");
            checked += 1;
        }
        assert_eq!(checked, 3);
    }

    #[test]
    fn slave_priority_reserves_head_through_live_master_occupation_without_rng() {
        use super::super::locomotor::{LocomotorState, MovementLayer};
        use crate::map::entities::EntityCategory;
        use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind, ruleset::RuleSet};
        use crate::sim::{
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
        let mut master = GameEntity::test_default_of_category(
            1,
            "MASTER",
            "Owner",
            4,
            4,
            EntityCategory::Structure,
        );
        master.set_owner_for_test(owner);
        master.set_type_ref_for_test(interner.intern("MASTER"));
        let slav = interner.intern("SLAV");
        // An existing manager that does not hold the slave refuses it.
        master.slave_manager = Some(SlaveManager::new(slav, [None; 0], 0, 0, 0));
        entities.insert(master);
        let mut slave = GameEntity::test_default(2, "SLAV", "Owner", 3, 4);
        slave.set_owner_for_test(owner);
        slave.set_type_ref_for_test(interner.intern("SLAV"));
        slave.set_category_for_test(EntityCategory::Infantry);
        slave.position.sub_x = SimFixed::from_num(192);
        slave.position.sub_y = SimFixed::from_num(64);
        slave.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        slave.navigation.path_replay = crate::sim::components::FootPathQueue {
            directions: vec![2],
            cursor: 0,
            reference_cell: Some((3, 4)),
        };
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
        let data: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_head_occupation.json",
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
        let mut ordinary_cell_rows = 0;
        for row in selections {
            let i = &row["input"];
            let out = &row["output"];
            let input = coord(&i["input"]);
            let bridge = i["bridge"].as_bool().unwrap();
            let ground = i["ground"].as_u64().unwrap() as u8;
            let deck = i["deck"].as_u64().unwrap() as u8;
            let mut raw = RawCellOccupationGrid::default();
            raw.mark_ground(10, 10, ground);
            raw.mark_deck(10, 10, deck);
            let mut rng = crate::sim::rng::SimRng::new(i["seed"].as_u64().unwrap());
            let layer = if bridge {
                super::super::locomotor::MovementLayer::Bridge
            } else {
                super::super::locomotor::MovementLayer::Ground
            };
            let slot = super::super::bump_crush::place_infantry_in_native_cell(
                &raw,
                RawCellKey::Real(10, 10),
                layer,
                input,
                i["priority"].as_bool().unwrap(),
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
            let expected = (expected != DriveCoord { x: 0, y: 0, z: 0 }).then_some(expected);
            assert_eq!(actual, expected, "{i}");
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
            if !i["priority"].as_bool().unwrap() && i["gate"] != 2 {
                // Existing crew/slave/unload callers supply no priority or
                // Gate exception. Compare their cell/subcell entry to these
                // same original results, including the selected plane/draw.
                let mut rng = crate::sim::rng::SimRng::new(i["seed"].as_u64().unwrap());
                let slot = super::super::bump_crush::place_infantry_in_cell(
                    &raw,
                    10,
                    10,
                    layer,
                    SimFixed::from_num(input.x & 255),
                    SimFixed::from_num(input.y & 255),
                    &mut rng,
                );
                assert_eq!(
                    slot.map(|s| selected_head(input, s, z, bridge)),
                    expected,
                    "ordinary cell entry {i}"
                );
                assert_eq!(
                    rng.logical_view().index_a,
                    out["random_indices"][0].as_i64().unwrap() as i32,
                    "ordinary cell entry {i}"
                );
                assert_eq!(
                    rng.logical_view().index_b,
                    out["random_indices"][1].as_i64().unwrap() as i32,
                    "ordinary cell entry {i}"
                );
                ordinary_cell_rows += 1;
            }
        }
        assert_eq!(ordinary_cell_rows, 72);
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
            .as_chunks::<2>()
            .0
            .iter()
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
            components::{FootPathQueue, NavTargetRef},
            entity_store::EntityStore,
            game_entity::GameEntity,
            intern::StringInterner,
            mission::{MissionId, state::MissionTestFixture},
            occupancy::OccupancyGrid,
        };
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/walk_first_step.json",
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
            entity.set_category_for_test(crate::map::entities::EntityCategory::Infantry);
            if input["infantry_constructor_facing"]
                .as_bool()
                .unwrap_or(false)
            {
                // The Infantry constructor's rate 127 (`0x00517BC5`).
                entity.set_body_facing_rot(0);
            } else {
                // Retain the older corpus's explicitly supplied rate0 owner.
                entity.body_facing = super::super::FacingClass::new(0, 0);
            }
            entity.set_owner_for_test(InternedId::from_index(41));
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
                serde_json::to_value(entity.body_facing).unwrap(),
                row["facing"]
            );
            assert_eq!(
                entity.foot_speed.applied_fraction(),
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

    /// A Force_Immediate_Destination that cannot be seated drops the held
    /// head: FindSubCellDest stores PlaceInfantryInCell's answer as the head
    /// (`0x0075C541..0x0075C562`), here the empty coordinate for a cell with
    /// a vehicle, and Marks the Foot's own coordinate.
    #[test]
    fn a_force_refused_its_spot_drops_the_held_head() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\nSpeed=4\n\
                 Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
            ))
            .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(16),
        );
        let id = sim
            .spawn_object("E1", "Americans", 6, 5, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .set_step_head(Some(DriveCoord {
                x: 7 * 256 + 64,
                y: 5 * 256 + 64,
                z: 0,
            }));
        sim.substrate.raw_cell_occupation.mark_ground(7, 5, 0x20);
        sim.foot_force_immediate_destination(
            id,
            Some(DriveCoord::cell(7, 5, 0)),
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.locomotor.as_ref().unwrap().step_head(), None);
    }
}

/// Test adapter over [`prepare_step_head_at`]: the prospective coordinate is
/// the Foot+5E0 head word's neighbour (Walk 0x0075B5A7), as the admission
/// request reads it. Pre-head CanEnter75B690 remains the caller's
/// responsibility.
#[cfg(test)]
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
    use crate::rules::locomotor_type::LocomotorKind;
    let Some(entity) = entities.get(id) else {
        return false;
    };
    let Some(loco) = entity.locomotor.as_ref() else {
        return true;
    };
    if loco.kind != LocomotorKind::Walk || loco.step_head().is_some() {
        return true;
    }
    let Some(&direction) = entity.navigation.path_replay.remaining_directions().first() else {
        return true;
    };
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
    let current = super::ground_pose::position_world_coord(&entity.position);
    let input = DriveCoord {
        x: current.x.wrapping_add(dx * 256),
        y: current.y.wrapping_add(dy * 256),
        z: current.z,
    };
    prepare_step_head_at(
        entities, id, occupancy, raw, terrain, grid, rules, interner, rng, input,
    )
}

/// The chooser's answer up to its crate question (`0x0075C56C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepHead {
    /// A Walk head is stored (`+0x28`) and awaits the crate question and
    /// [`commit_walk_head`].
    Pending,
    /// The chooser already answered: a non-Walk locomotor, a null request, or
    /// a failed subcell selection that restored the Foot's own raw occupation.
    Done(bool),
}

/// Walk FindSubCellDest (`0x0075C240`) up to its crate question. The retained
/// head's raw bits, else the Foot's own, are cleared first (`0x0075C2A0`). A
/// null request stores no head and takes the Mark tail at once
/// (`0x0075C2C6..0x0075C2DF` -> `0x0075C5C5`); any other request is placed
/// through the priority corridor and PlaceInfantryInCell and stored. The
/// crate question (`CellClass::PickupCrate @ 0x00481A00` at `0x0075C56C`) and
/// the Mark tail ([`commit_walk_head`]) follow in
/// [`crate::sim::world::Simulation::walk_find_sub_cell_dest`].
pub(super) fn select_step_head_at(
    entities: &mut crate::sim::entity_store::EntityStore,
    id: u64,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    raw: &mut RawCellOccupationGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    grid: Option<&PathGrid>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
    input: Option<DriveCoord>,
) -> StepHead {
    use super::locomotor::MovementLayer;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    let Some(entity) = entities.get(id) else {
        return StepHead::Done(false);
    };
    let Some(loco) = entity.locomotor.as_ref() else {
        return StepHead::Done(true);
    };
    if loco.kind != LocomotorKind::Walk {
        return StepHead::Done(true);
    }
    let owner = entity.owner();
    let current = super::ground_pose::position_world_coord(&entity.position);
    //75C25A..75C2A0 clears the retained head, else current; even a failed
    //chooser restores current raw at75C62A.
    raw_at(
        raw,
        owner,
        loco.step_head().unwrap_or(current),
        false,
        terrain,
        grid,
    );
    let Some(input) = input else {
        if let Some(loco) = entities.get_mut(id).and_then(|e| e.locomotor.as_mut()) {
            loco.set_step_head(None);
        }
        return StepHead::Done(commit_walk_head(entities, id, raw, terrain, grid));
    };
    let next = ((input.x / 256) as u16, (input.y / 256) as u16);
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
            priority = match target.category() {
                EntityCategory::Unit | EntityCategory::Aircraft => {
                    let c = super::ground_pose::position_world_coord(&target.position);
                    ((c.x / 256) as i16, (c.y / 256) as i16)
                        == ((input.x / 256) as i16, (input.y / 256) as i16)
                }
                EntityCategory::Structure => {
                    let cell = terrain.map_or(next, |t| {
                        let cell = t
                            .native_cell_identity(((input.x / 256) as i16, (input.y / 256) as i16));
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
            let selected = t.native_cell_identity(((input.x / 256) as i16, (input.y / 256) as i16));
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
        super::bump_crush::ground_gate_is_open(occupancy, entities, rules, interner, cell)
    } else {
        false
    };
    let layer = if bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    //75C541..75C562 stores PlaceInfantryInCell's answer as the head; a
    //failed placement answers the empty coordinate, so the head is dropped
    //and 75C5C5 Marks current.
    let Some(slot) = super::bump_crush::place_infantry_in_native_cell(
        raw, raw_key, layer, input, priority, gate_open, rng,
    ) else {
        if let Some(loco) = entities.get_mut(id).and_then(|e| e.locomotor.as_mut()) {
            loco.set_step_head(None);
        }
        raw_at(raw, owner, current, true, terrain, grid);
        return StepHead::Done(false);
    };
    let ground = super::ground_pose::ground_surface_z_at([input.x, input.y], false, terrain, grid)
        .unwrap_or(current.z);
    let head = selected_head(input, slot, ground, bridge);
    let Some(loco) = entities.get_mut(id).and_then(|e| e.locomotor.as_mut()) else {
        return StepHead::Done(false);
    };
    loco.set_step_head(Some(head));
    StepHead::Pending
}

/// `0x0075C5C5..0x0075C638`: the chooser's tail after the crate question. A
/// retained head is Marked (`vt+0xF0`, raw PUT) and the chooser answers 1; a
/// head reset to the dummy coordinate Marks the Foot's own XYZ instead and
/// answers 0, which the caller treats as a failed subcell selection.
pub(super) fn commit_walk_head(
    entities: &crate::sim::entity_store::EntityStore,
    id: u64,
    raw: &mut RawCellOccupationGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    grid: Option<&PathGrid>,
) -> bool {
    let Some(entity) = entities.get(id) else {
        return false;
    };
    let owner = entity.owner();
    match entity.locomotor.as_ref().and_then(|loco| loco.step_head()) {
        Some(head) => {
            raw_at(raw, owner, head, true, terrain, grid);
            true
        }
        None => {
            let current = super::ground_pose::position_world_coord(&entity.position);
            raw_at(raw, owner, current, true, terrain, grid);
            false
        }
    }
}

/// [`select_step_head_at`] then [`commit_walk_head`] with no crate on the
/// head: the test adapters' complete chooser. Production asks the crate
/// question between the two ([`Simulation::walk_find_sub_cell_dest`]).
#[cfg(test)]
pub(super) fn prepare_step_head_at(
    entities: &mut crate::sim::entity_store::EntityStore,
    id: u64,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    raw: &mut RawCellOccupationGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    grid: Option<&PathGrid>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
    input: DriveCoord,
) -> bool {
    match select_step_head_at(
        entities,
        id,
        occupancy,
        raw,
        terrain,
        grid,
        rules,
        interner,
        rng,
        Some(input),
    ) {
        StepHead::Pending => commit_walk_head(entities, id, raw, terrain, grid),
        StepHead::Done(answer) => answer,
    }
}
