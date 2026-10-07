//! Paradrop Drop_Payload — V-pattern math + per-tick passenger ejection.
//!
//! Each Drop_Payload call ejects one passenger from the carrier's cargo at
//! a 128-lepton offset perpendicular to flight heading. Drops alternate
//! left/right by post-decrement payload-count parity. With initial count=8
//! the visible drop sequence is L, R, L, R, L, R, L, R (first drop LEFT).
//!
//! The original retains the full facing word, wraps it by +/-0x3FFF and
//! truncates only after adding the retail-table displacement to world XY.
//! `native_trig` owns that shared coordinate math; `ground_pose` owns Location.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/native_trig and movement/ground_pose.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::movement::bump_crush;
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::parachute_descent::begin_parachute_descent;
use crate::sim::passenger::{DepartureFailure, DepartureRoute, PassengerRole, depart_cargo_head};
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
};
use crate::util::lepton;
use crate::util::native_trig::facing_step_world_xy;

/// V-pattern lateral radius. From gamemd constant at 0x7E2808 = 128.0 leptons
/// (= 0.5 cell). Each paratrooper lands half a cell to the left or right of
/// the plane's center.
pub const V_PATTERN_RADIUS_LEPTONS: i32 = 128;

/// Drop interval in native gameplay frames between consecutive drops.
///
/// Hardcoded in gamemd's ParadropOverfly mission (0x00415960): every code path returns
/// 5, meaning the overfly mission re-fires every 5 game frames while in range
/// and drops one passenger per call. This is NOT driven by `[ParaDropWeapon]
/// ROF=` (that weapon is a dummy — its rules.ini comment says so).
///
/// One admitted simulation advance is one native gameplay frame.
pub const PARADROP_DROP_INTERVAL_FRAMES: u16 = 5;

/// Candidate drop Location XY, in whole world leptons.
///
/// `facing`: full body FacingClass::Current word (0=N, 0x4000=E).
/// `payload_count_post_dec`: payload count AFTER decrement (matches gamemd's order).
///
/// DropPayload415CA6..415D78 wraps the word by +/-3FFF, then uses the same
/// signed-angle/table math as Walk/Fly. Rounding a separate offset loses the
/// original final-coordinate truncation and can change the landing cell.
/// Native corpus: tools/spatial_oracle/paradrop_coordinates.{py,json,md}.
fn drop_world_xy(current: [i32; 2], facing: u16, payload_count_post_dec: u8) -> [i32; 2] {
    let drop_facing = if (payload_count_post_dec & 1) == 0 {
        facing.wrapping_add(0x3FFF) // EVEN → RIGHT
    } else {
        facing.wrapping_sub(0x3FFF) // ODD → LEFT
    };
    facing_step_world_xy(current, drop_facing, V_PATTERN_RADIUS_LEPTONS)
}

/// Outcome of a single Drop_Payload attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropResult {
    /// Passenger placed, parachute descent attached. Caller resets cooldown
    /// to the ParadropOverfly 5-frame cadence and
    /// decrements payload_count.
    Success,
    /// Drop cell impassable. Passenger was re-inserted at cargo HEAD; caller
    /// leaves drop_cooldown unchanged so the mission can retry immediately.
    ImpassableRetry,
    /// Reveal/Unlimbo or parachute attachment failed after placement admission.
    /// Same cargo-head retry semantics as ImpassableRetry.
    AttachFailedRetry,
    /// Cargo was empty (caller should have gated on cargo_empty already).
    NoCargo,
}

/// Attempt to drop one passenger from the carrier aircraft's cargo.
///
/// Pre-conditions (caller-enforced):
///   - aircraft entity exists and has PassengerRole::Transport with non-empty cargo
///   - ParadropOverfly mission cadence is ready for another Drop_Payload call
///
/// Drop-cell passability reads the canonical path grid at the drop; a
/// headless fixture without one or terrain defaults to "always passable".
pub fn try_drop(
    sim: &mut Simulation,
    rules: &RuleSet,
    aircraft_id: u64,
    payload_count_pre_dec: u8,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> DropResult {
    // 1. Snapshot aircraft state (release borrow before mutating).
    // Capture the aircraft's full lepton position (cell + sub-cell) so the
    // V-pattern offset can apply at lepton precision. With cell-only math the
    // ±128 lateral offset truncates to 0 and every drop lands on the same cell.
    //
    // The drop coordinate is the plane's GetCoords (`0x00415C93`), and only
    // its XY moves to the landing spot (`0x00415DD7..0x00415DDF`): each
    // passenger starts falling at the plane's Z.
    let (facing, drop_z, aircraft_position) = match sim.substrate.entities.get(aircraft_id) {
        Some(a) => {
            let drop_z = ground_pose::object_world_z_leptons(a, sim.resolved_terrain.as_ref());
            (
                a.body_facing_current(sim.session.binary_frame),
                drop_z,
                a.position,
            )
        }
        None => return DropResult::NoCargo,
    };

    // Remove the native cargo HEAD (last boarded), and complete any retry here.
    let result = depart_cargo_head(
        sim,
        rules,
        registry,
        aircraft_id,
        DepartureRoute::Paradrop,
        |sim, passenger_id| {
            let (
                passenger_category,
                passenger_ready_for_reveal,
                passenger_speed_type,
                passenger_movement_zone,
                prior_movement_layer,
            ) = match sim.substrate.entities.get(passenger_id) {
                Some(passenger) => {
                    let object_type = sim.object_type(passenger.type_ref(), rules);
                    (
                        passenger.category,
                        passenger.lifecycle.object_alive && passenger.lifecycle.in_limbo,
                        passenger
                            .locomotor
                            .as_ref()
                            .map(|locomotor| locomotor.speed_type)
                            .or_else(|| object_type.map(|object_type| object_type.speed_type))
                            .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot),
                        passenger
                            .locomotor
                            .as_ref()
                            .map(|locomotor| locomotor.movement_zone)
                            .or_else(|| object_type.map(|object_type| object_type.movement_zone))
                            .unwrap_or(crate::rules::locomotor_type::MovementZone::Normal),
                        passenger
                            .locomotor
                            .as_ref()
                            .map(|locomotor| locomotor.layer)
                            .unwrap_or(MovementLayer::Ground),
                    )
                }
                None => {
                    return Err(DepartureFailure::MissingPassenger);
                }
            };
            if !passenger_ready_for_reveal {
                return Err(DepartureFailure::NotReady);
            }

            // 3. Compute the final native coordinate before splitting it through
            // the Location owner. Signed off-map remainders must survive too.
            let payload_count_post = payload_count_pre_dec.saturating_sub(1);
            let xy = drop_world_xy(
                ground_pose::position_world_xy(&aircraft_position),
                facing,
                payload_count_post,
            );
            let mut drop_position = aircraft_position;
            ground_pose::set_position_world_xy(&mut drop_position, xy);
            let (drop_rx, drop_ry) = (drop_position.rx, drop_position.ry);
            let (drop_sub_x, drop_sub_y) = (drop_position.sub_x, drop_position.sub_y);

            // Paradrop refuses a structural bridge cell (`+0x140 & 0x100`)
            // without the `0x200` flag (`0x005F597B..0x005F5996`).
            if sim
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(drop_rx, drop_ry))
                .is_some_and(|cell| {
                    cell.bridge_facts.has_structural_bridge()
                        && !cell.bridge_facts.has_transition_flag()
                })
            {
                return Err(DepartureFailure::Placement);
            }

            // Native: ObjectClass::SpawnParachuted computes the landing plane and then
            // calls CellClass::IsClearToMove before virtual Unlimbo. Zone identity is
            // not threaded into this Rust caller, so only that unavailable comparison
            // remains omitted; terrain, bridge plane, and raw occupation are live.
            let path_grid = sim.path_grid_snapshot();
            let path_grid = path_grid.as_deref();
            let land_passable = sim
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(drop_rx, drop_ry))
                .map(|cell| {
                    cell.speed_costs
                        .cost_for_speed_type(passenger_speed_type)
                        .is_none_or(|cost| cost > 0)
                })
                .unwrap_or_else(|| path_grid.is_none_or(|grid| grid.is_walkable(drop_rx, drop_ry)));
            let passability = if path_grid.is_none() && sim.resolved_terrain.is_none() {
                // Headless construction tests have no Cell substrate. Keep their
                // established admission explicit; live calls always thread map data.
                IsClearToMoveResult::Clear {
                    selected_layer: MovementLayer::Ground,
                }
            } else {
                evaluate_live_cell_passability(LiveCellPassabilityQuery {
                    target: (drop_rx, drop_ry),
                    speed_type: passenger_speed_type,
                    movement_zone: passenger_movement_zone,
                    requested_zone: None,
                    actual_zone: 0,
                    requested_layer: None,
                    ignore_infantry: false,
                    ignore_vehicles: false,
                    land_passable,
                    path_grid,
                    resolved_terrain: sim.resolved_terrain.as_ref(),
                    raw_occupation: Some(&sim.substrate.raw_cell_occupation),
                })
            };
            let landing_layer = match passability {
                IsClearToMoveResult::Clear { selected_layer } => selected_layer,
                IsClearToMoveResult::ClearWinged => MovementLayer::Ground,
                _ => {
                    return Err(DepartureFailure::Placement);
                }
            };
            if !matches!(landing_layer, MovementLayer::Ground | MovementLayer::Bridge) {
                return Err(DepartureFailure::Placement);
            }

            let selected_sub_cell = if passenger_category == EntityCategory::Infantry {
                match bump_crush::place_infantry_in_cell(
                    &sim.substrate.raw_cell_occupation,
                    drop_rx,
                    drop_ry,
                    landing_layer,
                    drop_sub_x,
                    drop_sub_y,
                    &mut sim.scenario_rng,
                ) {
                    Some(sub_cell) => Some(sub_cell),
                    None => {
                        return Err(DepartureFailure::Placement);
                    }
                }
            } else {
                None
            };
            let (final_sub_x, final_sub_y) = selected_sub_cell
                .map(|sub_cell| lepton::subcell_lepton_offset(Some(sub_cell)))
                .unwrap_or((drop_sub_x, drop_sub_y));

            // 5. Supply caller-owned subcell/role state while the passenger is still
            // limbo. Parachute attachment follows; Reveal is the success boundary.
            // Do NOT touch `loco.altitude` here: normal paradropped infantry keep
            // their base locomotor identity, and the fall's height is the
            // Location Z. Paradrop sets OnBridge on the `0x100` flag
            // (`0x005F5986`), the flag that makes IsClearToMove select the deck,
            // before Unlimbo's Mark, so GetHeight measures the fall to the deck.
            // RESIDUAL: native sets it before its admission checks, never
            // clears it, and restores nothing when the drop fails; this writes
            // it once admitted. They differ only when a passenger's drop over a
            // bridge cell failed and a later drop lands off the bridge: native
            // keeps OnBridge set unless something clears it in between (not
            // traced), so its fall would stop at deck height. Rare; risk: one
            // paratrooper's height.
            if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                passenger.sub_cell = selected_sub_cell;
                passenger.passenger_role = PassengerRole::None;
                passenger.on_bridge = landing_layer == MovementLayer::Bridge;
                if let Some(locomotor) = passenger.locomotor.as_mut() {
                    locomotor.layer = landing_layer;
                }
            }
            // 6. Attach parachute descent while the passenger is still limbo. Reveal
            // is the local success boundary; an attach retry is not Techno Limbo.
            if !begin_parachute_descent(&mut sim.substrate.entities, passenger_id, drop_z) {
                return Err(DepartureFailure::ParachuteAttach(prior_movement_layer));
            }

            let landing_z = sim
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(drop_rx, drop_ry))
                .map_or(0, |cell| {
                    cell.level
                        .wrapping_add(u8::from(landing_layer == MovementLayer::Bridge) * 4)
                });
            let reveal_outcome = sim.try_reveal_entity_with_context(
                passenger_id,
                RevealRequest {
                    position: RevealPosition {
                        exact_z_leptons: None,
                        rx: drop_rx,
                        ry: drop_ry,
                        z: landing_z,
                        sub_x: final_sub_x,
                        sub_y: final_sub_y,
                    },
                    placement: PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                },
                crate::sim::world::UninitContext::with_rules(rules),
            );
            if !matches!(reveal_outcome, RevealOutcome::Revealed { .. }) {
                return Err(DepartureFailure::ParachuteReveal(
                    reveal_outcome,
                    prior_movement_layer,
                ));
            }

            // `ObjectClass::Paradrop @ 0x005F5940` builds the canopy once it has
            // placed the object and set its coordinate.
            sim.attach_parachute_anim(rules, passenger_id);
            sim.foot_neighbors_after_payload_drop(passenger_id, (drop_rx as i16, drop_ry as i16));

            // 7. ChuteSound at drop cell.
            sim.sound_events.push(SimSoundEvent::ChuteSound {
                rx: drop_rx,
                ry: drop_ry,
            });
            // `0x00415E74..0x00415E83`: the passenger leaves the carrier's team,
            // if it is in that one.
            if let Some((team, _)) = sim.team_script_vm.team_for_member(aircraft_id) {
                sim.team_remove_member(team, passenger_id, false, Some(rules));
            }

            Ok(())
        },
    );
    match result {
        Ok(()) => DropResult::Success,
        Err(DepartureFailure::NoCargo) => DropResult::NoCargo,
        Err(DepartureFailure::Placement) => DropResult::ImpassableRetry,
        Err(
            DepartureFailure::MissingPassenger
            | DepartureFailure::NotReady
            | DepartureFailure::ParachuteAttach(_)
            | DepartureFailure::ParachuteReveal(..),
        ) => DropResult::AttachFailedRetry,
        Err(DepartureFailure::GroundReveal(_)) => unreachable!("ground reveal in paradrop"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::PassengerCargo;
    use crate::util::fixed_math::SimFixed;

    fn drop_test_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             0=PDPLANE\n\
             [BuildingTypes]\n\
             [E1]\n\
             Name=GI\n\
             Strength=100\n\
             Size=1\n\
             [PDPLANE]\n\
             Name=Paradrop Plane\n\
             Strength=400\n",
        );
        RuleSet::from_ini(&ini).expect("drop test rules should parse")
    }

    fn insert_loaded_paradrop_pair(sim: &mut Simulation, aircraft_id: u64, passenger_id: u64) {
        let mut aircraft = GameEntity::test_default(aircraft_id, "PDPLANE", "Americans", 50, 20);
        aircraft.owner = sim.interner.intern("Americans");
        aircraft.type_ref = sim.interner.intern("PDPLANE");
        aircraft.category = EntityCategory::Aircraft;
        aircraft.body_facing.snap(0x8000, 0);
        let mut cargo = PassengerCargo::new(8, 0);
        cargo.board_forced(passenger_id, 1);
        aircraft.passenger_role = PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(aircraft);

        let mut passenger = GameEntity::new_at_frame_zero_for_test(
            passenger_id,
            50,
            20,
            0,
            0,
            sim.interner.intern("Americans"),
            crate::sim::components::Health { current: 100 },
            sim.interner.intern("E1"),
            EntityCategory::Infantry,
            0,
            0,
            false,
        );
        passenger.is_voxel = false;
        passenger.sub_cell = Some(2);
        passenger.passenger_role = PassengerRole::Inside {
            transport_id: aircraft_id,
            open_topped: false,
        };
        sim.substrate.entities.insert(passenger);
    }

    #[test]
    fn paradrop_landing_cell_matches_original_coordinate_prefix() {
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let rules = drop_test_rules();
        let mut checked = 0;
        // All selected headings for both parities at (12928,5248). These are
        // original coordinate outputs, before native admission/subcell choice.
        for sweep in &oracle["sweeps"].as_array().unwrap()[..2] {
            assert_eq!(sweep["origin"], serde_json::json!([12928, 5248]));
            for row in sweep["samples"].as_array().unwrap() {
                let mut sim = Simulation::new();
                insert_loaded_paradrop_pair(&mut sim, 1, 2);
                let plane = sim.substrate.entities.get_mut(1).unwrap();
                crate::sim::movement::ground_pose::set_position_world_xy(
                    &mut plane.position,
                    [12928, 5248],
                );
                plane
                    .body_facing
                    .snap(row["facing"].as_u64().unwrap() as u16, 0);
                let expected_cell = (
                    lepton::lepton_to_cell(row["world_xy"][0].as_i64().unwrap() as i32) as u16,
                    lepton::lepton_to_cell(row["world_xy"][1].as_i64().unwrap() as i32) as u16,
                );
                assert_eq!(
                    try_drop(
                        &mut sim,
                        &rules,
                        1,
                        sweep["post_count"].as_u64().unwrap() as u8 + 1,
                        None
                    ),
                    DropResult::Success,
                );
                let passenger = sim.substrate.entities.get(2).unwrap();
                assert_eq!(
                    (passenger.position.rx, passenger.position.ry),
                    expected_cell,
                    "native row={row}, post_count={}",
                    sweep["post_count"],
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 52);
    }

    #[test]
    fn paradrop_world_coordinates_match_all_native_headings() {
        use crate::util::sha256::{Sha256, digest_hex};
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let sweeps = oracle["sweeps"].as_array().unwrap();
        assert_eq!(sweeps.len(), 6);
        for (index, sweep) in sweeps.iter().enumerate() {
            let origin = [[12928, 5248], [0, 0], [131071, 130816]][index / 2];
            let post_count = (index % 2) as u8;
            assert_eq!(sweep["origin"], serde_json::json!(origin));
            assert_eq!(sweep["post_count"], post_count);
            assert_eq!(sweep["facing_count"], 65536);
            for sample in sweep["samples"].as_array().unwrap() {
                assert_eq!(
                    serde_json::json!(drop_world_xy(
                        origin,
                        sample["facing"].as_u64().unwrap() as u16,
                        post_count
                    )),
                    sample["world_xy"],
                    "origin={origin:?}, post={post_count}, sample={sample}",
                );
            }
            let mut digest = Sha256::new();
            for facing in 0..=u16::MAX {
                for coordinate in drop_world_xy(origin, facing, post_count) {
                    digest.update(&coordinate.to_le_bytes());
                }
            }
            assert_eq!(digest_hex(digest.finalize()), sweep["world_xy_sha256"]);
        }
    }

    #[test]
    fn paradrop_uses_native_cell_for_occupancy_admission() {
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let row = oracle["sweeps"][0]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["facing"] == 0x7F)
            .unwrap();
        let native_cell = (
            lepton::lepton_to_cell(row["world_xy"][0].as_i64().unwrap() as i32) as u16,
            lepton::lepton_to_cell(row["world_xy"][1].as_i64().unwrap() as i32) as u16,
        );
        let rules = drop_test_rules();
        for blocked in [false, true] {
            let mut sim = Simulation::new();
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .body_facing
                .snap(0x7F, 0);
            // Opposite admission outcomes on the two adjacent cells: the old
            // byte-facing/offset path chose (51,20), the native row (50,20).
            let blocked_cell = if blocked { native_cell } else { (51, 20) };
            for (id, subcell) in [(90, 2), (91, 3), (92, 4)] {
                let mut blocker =
                    GameEntity::test_default(id, "E1", "Americans", blocked_cell.0, blocked_cell.1);
                blocker.owner = sim.interner.intern("Americans");
                blocker.type_ref = sim.interner.intern("E1");
                blocker.category = EntityCategory::Infantry;
                blocker.is_voxel = false;
                blocker.sub_cell = Some(subcell);
                (blocker.position.sub_x, blocker.position.sub_y) =
                    lepton::subcell_lepton_offset(Some(subcell));
                sim.substrate.entities.insert(blocker);
                assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
            }
            assert_eq!(
                try_drop(&mut sim, &rules, 1, 1, None),
                if blocked {
                    DropResult::ImpassableRetry
                } else {
                    DropResult::Success
                },
            );
            let cargo = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .passenger_role
                .cargo()
                .unwrap();
            let passenger = sim.substrate.entities.get(2).unwrap();
            if blocked {
                assert_eq!(cargo.passengers, vec![2]);
                assert!(passenger.lifecycle.in_limbo);
                assert!(passenger.parachute_state.is_none());
            } else {
                assert!(cargo.passengers.is_empty());
                assert_eq!((passenger.position.rx, passenger.position.ry), native_cell);
                assert!(!passenger.lifecycle.in_limbo);
            }
        }
    }

    /// The production path end to end: the drop attaches the canopy, the
    /// object turn winds it down on the landing edge, and the store plays it
    /// out to its last frame and removes it (`ObjectClass::Paradrop
    /// 0x005F5A9D`, `ObjectClass::AI 0x005F3F9D`, `AnimClass::AI 0x0042475A`).
    #[test]
    fn a_drop_attaches_a_canopy_that_winds_down_at_landing_and_plays_out() {
        use crate::rules::art_data::ArtRegistry;
        let mut sim = Simulation::new();
        let mut rules = drop_test_rules();
        rules.general.parachute_shp = Some("PARACH".to_string());
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PARACH]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n",
        ));
        art.bind_anim_frame_count_for_test("PARACH", 9);
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);
        // The fixture places ids 1 and 2 by hand; keep the counter clear of them.
        let (aircraft_id, passenger_id) = (sim.allocate_stable_id(), sim.allocate_stable_id());
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);

        assert_eq!(
            try_drop(&mut sim, &rules, aircraft_id, 4, None),
            DropResult::Success
        );
        let parachute = sim.interner.get("PARACH").expect("type interned");
        let canopy = |sim: &Simulation| {
            sim.substrate
                .anims
                .iter()
                .find(|(_, anim)| anim.type_id == parachute)
                .map(|(id, anim)| (*id, anim.owner_entity, anim.runtime.loop_remaining))
        };
        let (canopy_id, owner, loops) = canopy(&sim).expect("the drop attached a canopy");
        assert_eq!((owner, loops), (Some(passenger_id), u8::MAX));

        let mut wound_down = false;
        let mut played_out = false;
        for _ in 0..60 {
            sim.advance_tick(&[], Some(&rules), None, None, 100);
            let falling = sim
                .substrate
                .entities
                .get(passenger_id)
                .is_some_and(|passenger| passenger.parachute_state.is_some());
            match canopy(&sim) {
                Some((id, owner, loops)) => {
                    assert_eq!((id, owner), (canopy_id, Some(passenger_id)));
                    assert_eq!(loops == 0, !falling, "zeroed exactly at the landing");
                    wound_down |= loops == 0;
                }
                None => {
                    played_out = true;
                    break;
                }
            }
        }
        assert!(wound_down, "the landing zeroed the remaining loops");
        assert!(played_out, "the canopy left at its last frame");
        assert!(sim.substrate.entities.get(passenger_id).is_some());
    }

    /// The drop coordinate is the plane's GetCoords (`0x00415C93`) with only
    /// its XY moved to the landing spot (`0x00415DD7..0x00415DDF`), and
    /// Paradrop places the passenger there (`0x005F5A50`). Dropped from a
    /// plane over higher ground than its landing spot, a paratrooper starts at
    /// the plane's Z, not at the plane's altitude above the landing ground.
    #[test]
    fn a_paratrooper_starts_falling_at_the_planes_z() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;

        let mut sim = Simulation::new();
        let cells = (0..64u16)
            .flat_map(|y| {
                (0..64u16).map(move |x| {
                    let mut cell = test_flat_cell(x, y);
                    if (x, y) == (50, 20) {
                        cell.level = 2;
                    }
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(64, 64, cells));
        let rules = drop_test_rules();
        let (aircraft_id, passenger_id) = (1, 2);
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
        // In flight 1500 leptons over its level-2 cell, as the Fly host keeps it.
        let plane_z = 2 * 104 + 1500;
        let plane = sim.substrate.entities.get_mut(aircraft_id).unwrap();
        plane.position.exact_z_leptons = Some(plane_z);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Fly);
        locomotor.altitude = SimFixed::from_num(1500);
        plane.locomotor = Some(locomotor);

        assert_eq!(
            try_drop(&mut sim, &rules, aircraft_id, 4, None),
            DropResult::Success
        );
        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        assert_eq!(
            (passenger.position.rx, passenger.position.ry),
            (51, 20),
            "the landing cell is at level 0"
        );
        assert!(passenger.is_falling_down());
        assert_eq!(passenger.position.exact_z_leptons, Some(plane_z));
    }

    /// Over a structural bridge cell (`+0x140 & 0x100`) Paradrop sets OnBridge
    /// (`0x005F5986`) and refuses the cell unless it also has the `0x200` flag
    /// (`0x005F598D..0x005F5996`).
    #[test]
    fn a_drop_over_a_bridge_cell_needs_its_0x200_flag() {
        use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION};
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};

        let rules = drop_test_rules();
        let drop = |flags: u32| {
            let mut sim = Simulation::new();
            let cells = (0..64u16)
                .flat_map(|y| {
                    (0..64u16).map(move |x| {
                        let mut cell = test_flat_cell(x, y);
                        if (x, y) == (51, 20) {
                            cell.bridge_facts.raw_flags = flags;
                            cell.has_bridge_deck = true;
                            cell.bridge_deck_level = 4;
                        }
                        cell
                    })
                })
                .collect();
            sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
                64, 64, cells,
            ));
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            let result = try_drop(&mut sim, &rules, 1, 4, None);
            let passenger = sim.substrate.entities.get(2).unwrap();
            (
                result,
                matches!(passenger.passenger_role, PassengerRole::Inside { .. }),
                passenger.on_bridge,
            )
        };

        let (result, inside, _) = drop(BRIDGE_FLAG_STRUCTURAL);
        assert_eq!((result, inside), (DropResult::ImpassableRetry, true));
        assert_eq!(
            drop(BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_TRANSITION),
            (DropResult::Success, false, true)
        );
    }

    #[test]
    fn paradrop_infantry_uses_valid_subcell_instead_of_raw_v_coordinate() {
        let mut sim = Simulation::new();
        let rules = drop_test_rules();
        let aircraft_id = 1;
        let passenger_id = 2;
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);

        let result = try_drop(&mut sim, &rules, aircraft_id, 4, None);

        assert_eq!(result, DropResult::Success);
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|event| matches!(event, SimSoundEvent::ChuteSound { .. }))
                .count(),
            1,
            "successful passenger drop should emit exactly one ChuteSound"
        );
        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger exists");
        assert!(passenger.lifecycle.object_alive);
        assert!(!passenger.lifecycle.in_limbo);
        assert!(passenger.lifecycle.cell_marked);
        assert!(passenger.in_logic_vector);
        assert_eq!((passenger.position.rx, passenger.position.ry), (51, 20));
        let sub_cell = passenger
            .sub_cell
            .expect("placed infantry should have a subcell");
        assert!(
            bump_crush::FUNCTIONAL_SUB_CELLS.contains(&sub_cell),
            "drop should pick a functional infantry subcell, got {}",
            sub_cell
        );
        assert_ne!(
            (
                passenger.position.sub_x.to_num::<i32>(),
                passenger.position.sub_y.to_num::<i32>()
            ),
            (0, 128),
            "raw V-pattern half-cell coordinate must not be the final infantry XY"
        );
        assert_eq!(
            (passenger.position.sub_x, passenger.position.sub_y),
            lepton::subcell_lepton_offset(Some(sub_cell))
        );
        let occupied_subcells: Vec<(u64, u8)> = sim
            .substrate
            .occupancy
            .get(51, 20)
            .expect("drop cell occupied")
            .infantry(MovementLayer::Ground)
            .collect();
        assert_eq!(occupied_subcells, vec![(passenger_id, sub_cell)]);
    }

    #[test]
    fn paradrop_full_infantry_subcells_retry_and_restore_cargo_head() {
        let mut sim = Simulation::new();
        let rules = drop_test_rules();
        let aircraft_id = 1;
        let passenger_id = 2;
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
        for (id, sub_cell) in [(90, 2), (91, 3), (92, 4)] {
            let mut blocker = GameEntity::test_default(id, "E1", "Americans", 51, 20);
            blocker.owner = sim.interner.intern("Americans");
            blocker.type_ref = sim.interner.intern("E1");
            blocker.category = EntityCategory::Infantry;
            blocker.is_voxel = false;
            blocker.sub_cell = Some(sub_cell);
            (blocker.position.sub_x, blocker.position.sub_y) =
                lepton::subcell_lepton_offset(Some(sub_cell));
            sim.substrate.entities.insert(blocker);
            assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
        }

        let result = try_drop(&mut sim, &rules, aircraft_id, 4, None);

        assert_eq!(result, DropResult::ImpassableRetry);
        assert!(
            sim.sound_events
                .iter()
                .all(|event| !matches!(event, SimSoundEvent::ChuteSound { .. })),
            "failed placement retry must not emit ChuteSound"
        );
        let cargo = sim
            .substrate
            .entities
            .get(aircraft_id)
            .and_then(|a| a.passenger_role.cargo())
            .expect("aircraft cargo restored");
        assert_eq!(cargo.passengers, vec![passenger_id]);
        assert_eq!(cargo.total_size, 1);
        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger exists");
        assert!(matches!(
            passenger.passenger_role,
            PassengerRole::Inside { transport_id, .. } if transport_id == aircraft_id
        ));
        assert!(passenger.parachute_state.is_none());
        assert!(passenger.lifecycle.object_alive);
        assert!(passenger.lifecycle.in_limbo);
        assert!(!passenger.lifecycle.cell_marked);
        assert!(!passenger.in_logic_vector);
        assert!(
            !sim.substrate
                .occupancy
                .contains_entity(51, 20, passenger_id),
            "failed placement must not unlimbo the passenger into occupancy"
        );
    }

    #[test]
    fn attach_failed_retry_clears_peer_radio_contact_to_passenger() {
        let mut sim = Simulation::new();
        let rules = drop_test_rules();
        let aircraft_id = 1;
        let missing_passenger_id = 7;
        let peer_id = 9;

        let mut aircraft = GameEntity::test_default(aircraft_id, "PDPLANE", "Americans", 10, 10);
        aircraft.owner = sim.interner.intern("Americans");
        aircraft.type_ref = sim.interner.intern("PDPLANE");
        let mut cargo = PassengerCargo::new(8, 0);
        cargo.board_forced(missing_passenger_id, 1);
        aircraft.passenger_role = PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(aircraft);

        let mut peer = GameEntity::test_default(peer_id, "E1", "Americans", 11, 10);
        peer.owner = sim.interner.intern("Americans");
        peer.type_ref = sim.interner.intern("E1");
        peer.mark_live_contact_with(missing_passenger_id);
        sim.substrate.entities.insert(peer);

        let result = try_drop(&mut sim, &rules, aircraft_id, 1, None);

        assert_eq!(result, DropResult::AttachFailedRetry);
        assert!(
            sim.sound_events
                .iter()
                .all(|event| !matches!(event, SimSoundEvent::ChuteSound { .. })),
            "attach-failed retry must not emit ChuteSound"
        );
        let cargo = sim
            .substrate
            .entities
            .get(aircraft_id)
            .and_then(|a| a.passenger_role.cargo())
            .expect("aircraft cargo restored");
        assert_eq!(cargo.passengers, vec![missing_passenger_id]);
        assert!(
            !sim.substrate
                .entities
                .get(peer_id)
                .unwrap()
                .has_live_contact_with(missing_passenger_id),
            "attach-failed retry should clear stale peer radio contacts"
        );
    }
    #[test]
    fn cargo_departure_paradrop_preserves_early_state_and_unwinds_reveal_failure() {
        use crate::sim::movement::locomotor::LocomotorState;
        for post_attach in [false, true] {
            let mut sim = Simulation::new();
            let rules = drop_test_rules();
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            let mut loco = LocomotorState::from_object_type(rules.object("E1").unwrap(), 0);
            loco.layer = MovementLayer::Bridge;
            {
                let passenger = sim.substrate.entities.get_mut(2).unwrap();
                passenger.locomotor = Some(loco);
                passenger.lifecycle.object_alive = post_attach;
                passenger.lifecycle.cell_marked = post_attach;
            }
            let mut peer = GameEntity::test_default(9, "E1", "Americans", 30, 30);
            peer.mark_live_contact_with(2);
            sim.substrate.entities.insert(peer);
            {
                let aircraft = sim.substrate.entities.get_mut(1).unwrap();
                aircraft.set_gunner_selection_for_test(99, -1);
                let cargo = aircraft.passenger_role.cargo_mut().unwrap();
                cargo.passenger_sizes[0] = 7;
                cargo.total_size = 7;
                cargo.board_forced(1234, 3);
                // Put the real passenger back at the head, preserving mixed sizes.
                cargo.passengers.swap(0, 1);
                cargo.passenger_sizes.swap(0, 1);
            }
            let held = serde_json::to_value(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role
                    .cargo(),
            )
            .unwrap();
            let rng_before = sim.scenario_rng.state();
            assert_eq!(
                try_drop(&mut sim, &rules, 1, 4, None),
                DropResult::AttachFailedRetry
            );
            let aircraft = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                serde_json::to_value(aircraft.passenger_role.cargo()).unwrap(),
                held
            );
            assert_eq!(aircraft.current_weapon_number(), 99);
            let passenger = sim.substrate.entities.get(2).unwrap();
            assert_eq!(passenger.passenger_role.inside_transport_id(), Some(1));
            assert!(passenger.lifecycle.in_limbo);
            assert_eq!(passenger.lifecycle.cell_marked, post_attach);
            assert!(!passenger.in_logic_vector);
            assert!(passenger.parachute_state.is_none());
            assert_eq!(
                passenger.locomotor.as_ref().unwrap().layer,
                MovementLayer::Bridge
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(9)
                    .unwrap()
                    .has_live_contact_with(2),
                !post_attach
            );
            assert!(sim.sound_events.is_empty());
            if !post_attach {
                assert_eq!(sim.scenario_rng.state(), rng_before);
            }
            println!(
                "CARGO_TRACE paradrop {post_attach} {:?} {:?} {:?}",
                sim.scenario_rng.state(),
                held,
                passenger.position
            );
            if post_attach {
                sim.substrate
                    .entities
                    .get_mut(2)
                    .unwrap()
                    .lifecycle
                    .cell_marked = false;
                assert_eq!(try_drop(&mut sim, &rules, 1, 4, None), DropResult::Success);
                let cargo = sim
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role
                    .cargo()
                    .unwrap();
                assert_eq!(cargo.passengers, vec![1234]);
                assert_eq!(cargo.passenger_sizes, vec![3]);
                assert_eq!(cargo.total_size, 3);
                let passenger = sim.substrate.entities.get(2).unwrap();
                assert!(passenger.parachute_state.is_some());
                assert!(passenger.lifecycle.cell_marked && passenger.in_logic_vector);
                assert!(!passenger.passenger_role.is_inside_transport());
                assert_eq!(
                    sim.sound_events
                        .iter()
                        .filter(|event| matches!(event, SimSoundEvent::ChuteSound { .. }))
                        .count(),
                    1
                );
            }
        }
    }
}
