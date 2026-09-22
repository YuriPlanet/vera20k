//! Paradrop Drop_Payload — V-pattern math + per-tick passenger ejection.
//!
//! Each Drop_Payload call ejects one passenger from the carrier's cargo at
//! a 128-lepton offset perpendicular to flight heading. Drops alternate
//! left/right by post-decrement payload-count parity. With initial count=8
//! the visible drop sequence is L, R, L, R, L, R, L, R (first drop LEFT).
//!
//! The 0x3FFF binary-angle quarter-circle in the original collapses to
//! a 64-step facing offset under our 256-facing convention (0x3FFF/0xFFFF
//! ≈ 0.25, and 64/256 = 0.25). The existing 256-entry SIN_TABLE/COS_TABLE
//! in util/facing_table covers all the trig.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/facing_table, util/fixed_math.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::parachute_descent::begin_parachute_descent;
use crate::sim::passenger::{DepartureFailure, DepartureRoute, PassengerRole, depart_cargo_head};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
};
use crate::util::facing_table::facing_to_movement;
use crate::util::fixed_math::{SIM_ZERO, SimFixed, sim_to_i32};
use crate::util::lepton;

/// V-pattern lateral radius. From gamemd constant at 0x7E2808 = 128.0 leptons
/// (= 0.5 cell). Each paratrooper lands half a cell to the left or right of
/// the plane's center.
pub const V_PATTERN_RADIUS_LEPTONS: i32 = 128;

/// Reset value for the LandingState mutex (gamemd `aircraft+0x6D3`).
/// Decremented per tick as mirrored aircraft state. Standard in-range
/// Mission_Rescue cadence is still controlled by the mission's 5-frame return.
pub const LANDING_STATE_RESET: u8 = 5;

/// Drop interval in native gameplay frames between consecutive drops.
///
/// Hardcoded in gamemd's `Mission_Rescue` (0x00415960): every code path returns
/// 5, meaning the rescue mission re-fires every 5 game frames while in range
/// and drops one passenger per call. This is NOT driven by `[ParaDropWeapon]
/// ROF=` (that weapon is a dummy — its rules.ini comment says so).
///
/// One admitted simulation advance is one native gameplay frame.
pub const PARADROP_DROP_INTERVAL_FRAMES: u16 = 5;

/// Compute the V-pattern lateral offset for the next drop, in leptons.
///
/// `facing`: aircraft body facing 0..=255 (RA2 convention: 0=N, 64=E, 128=S, 192=W).
/// `payload_count_post_dec`: payload count AFTER decrement (matches gamemd's order).
///
/// Returns `(dx, dy)` in leptons. EVEN parity → CW 90° from heading (RIGHT);
/// ODD parity → CCW 90° from heading (LEFT). With initial count=8 the
/// post-decrement sequence 7,6,5,4,3,2,1,0 produces drop sides L,R,L,R,L,R,L,R.
pub fn v_offset(facing: u8, payload_count_post_dec: u8) -> (i32, i32) {
    let drop_facing = if (payload_count_post_dec & 1) == 0 {
        facing.wrapping_add(64) // EVEN → CW 90° (RIGHT of heading)
    } else {
        facing.wrapping_sub(64) // ODD  → CCW 90° (LEFT of heading)
    };
    let radius = SimFixed::from_num(V_PATTERN_RADIUS_LEPTONS);
    let (dx, dy) = facing_to_movement(drop_facing, radius);
    (sim_to_i32(dx), sim_to_i32(dy))
}

/// Outcome of a single Drop_Payload attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropResult {
    /// Passenger placed, parachute descent attached. Caller resets cooldown
    /// to the Mission_Rescue 5-frame cadence, mirrors landing_state=5, and
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
///   - Rescue-equivalent mission cadence is ready for another Drop_Payload call
///
/// `path_grid`: Some when threaded from advance_tick; None in headless tests
/// (passability defaults to "always passable" in that case).
pub fn try_drop(
    sim: &mut Simulation,
    rules: &RuleSet,
    aircraft_id: u64,
    payload_count_pre_dec: u8,
    path_grid: Option<&PathGrid>,
) -> DropResult {
    // 1. Snapshot aircraft state (release borrow before mutating).
    // Capture the aircraft's full lepton position (cell + sub-cell) so the
    // V-pattern offset can apply at lepton precision. With cell-only math the
    // ±128 lateral offset truncates to 0 and every drop lands on the same cell.
    let (facing, altitude, aircraft_x_lep, aircraft_y_lep) =
        match sim.substrate.entities.get(aircraft_id) {
            Some(a) => {
                let alt = a.locomotor.as_ref().map(|l| l.altitude).unwrap_or(SIM_ZERO);
                let x_lep = a.position.rx as i32 * 256 + sim_to_i32(a.position.sub_x);
                let y_lep = a.position.ry as i32 * 256 + sim_to_i32(a.position.sub_y);
                (a.facing, alt, x_lep, y_lep)
            }
            None => return DropResult::NoCargo,
        };

    // Remove the native cargo HEAD (last boarded), and complete any retry here.
    let result = depart_cargo_head(
        sim,
        rules,
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

            // 3. Compute V-offset in leptons, then split into (cell, sub-cell).
            // Using `div_euclid`/`rem_euclid` so negative offsets cross cell
            // boundaries correctly (left-side drops walk one cell west when the
            // aircraft is in the western half of its cell).
            let payload_count_post = payload_count_pre_dec.saturating_sub(1);
            let (dx, dy) = v_offset(facing, payload_count_post);
            let drop_x_lep = aircraft_x_lep + dx;
            let drop_y_lep = aircraft_y_lep + dy;
            let drop_rx = drop_x_lep.div_euclid(256).clamp(0, u16::MAX as i32) as u16;
            let drop_ry = drop_y_lep.div_euclid(256).clamp(0, u16::MAX as i32) as u16;
            let drop_sub_x = SimFixed::from_num(drop_x_lep.rem_euclid(256));
            let drop_sub_y = SimFixed::from_num(drop_y_lep.rem_euclid(256));

            // Native: ObjectClass::SpawnParachuted computes the landing plane and then
            // calls CellClass::IsClearToMove before virtual Unlimbo. Zone identity is
            // not threaded into this Rust caller, so only that unavailable comparison
            // remains omitted; terrain, bridge plane, and raw occupation are live.
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
                let occ = sim.substrate.occupancy.get(drop_rx, drop_ry);
                match bump_crush::allocate_sub_cell_with_preference(
                    occ,
                    landing_layer,
                    None,
                    drop_sub_x,
                    drop_sub_y,
                    // sub-cell placement — scenario stream. Direct field: `occ` may borrow
                    // &sim.substrate.occupancy, so the subcell_rng() accessor could conflict.
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
            // Do NOT touch
            // `loco.altitude` here: normal paradropped infantry keep their base
            // locomotor identity, while descent altitude lives in ParachuteDescentState.
            if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                passenger.sub_cell = selected_sub_cell;
                passenger.passenger_role = PassengerRole::None;
                if let Some(locomotor) = passenger.locomotor.as_mut() {
                    locomotor.layer = landing_layer;
                }
            }
            // 6. Attach parachute descent while the passenger is still limbo. Reveal
            // is the local success boundary; an attach retry is not Techno Limbo.
            if !begin_parachute_descent(&mut sim.substrate.entities, passenger_id, altitude) {
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
            let reveal_outcome = sim.try_reveal_entity(
                passenger_id,
                RevealRequest {
                    position: RevealPosition {
                        rx: drop_rx,
                        ry: drop_ry,
                        z: landing_z,
                        sub_x: final_sub_x,
                        sub_y: final_sub_y,
                    },
                    placement: PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                },
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

    fn magnitude_sq(dx: i32, dy: i32) -> i64 {
        (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64)
    }

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
        aircraft.facing = 128;
        let mut cargo = PassengerCargo::new(8, 0);
        cargo.board_forced(passenger_id, 1);
        aircraft.passenger_role = PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(aircraft);

        let mut passenger = GameEntity::test_default(passenger_id, "E1", "Americans", 50, 20);
        passenger.owner = sim.interner.intern("Americans");
        passenger.type_ref = sim.interner.intern("E1");
        passenger.category = EntityCategory::Infantry;
        passenger.is_voxel = false;
        passenger.sub_cell = Some(2);
        passenger.passenger_role = PassengerRole::Inside {
            transport_id: aircraft_id,
        };
        sim.substrate.entities.insert(passenger);
    }

    #[test]
    fn test_v_pattern_radius_is_128_for_all_facings() {
        // Magnitude of (dx, dy) should be ~128 leptons regardless of facing.
        // sin/cos LUT is exact at multiples of 64 (cardinal facings) and accurate
        // to <1 lepton elsewhere.
        for facing in 0..=255u8 {
            let (dx, dy) = v_offset(facing, 0); // EVEN parity (RIGHT)
            let mag_sq = magnitude_sq(dx, dy);
            let expected_sq = (V_PATTERN_RADIUS_LEPTONS as i64).pow(2);
            // Allow ±2 leptons of error (LUT discretization at 256 facings).
            let tolerance: i64 = 2 * (V_PATTERN_RADIUS_LEPTONS as i64) * 2 + 4;
            assert!(
                (mag_sq - expected_sq).abs() < tolerance,
                "facing={} produced offset ({},{}), mag²={}, expected ~{}",
                facing,
                dx,
                dy,
                mag_sq,
                expected_sq,
            );
        }
    }

    #[test]
    fn test_v_pattern_alternates_starting_left() {
        // gamemd: with initial count=8, post-decrement sequence is 7,6,5,4,3,2,1,0.
        // Parity: 7→ODD→LEFT, 6→EVEN→RIGHT, 5→ODD→LEFT, ...
        // Visible drop sequence = L, R, L, R, L, R, L, R (first drop LEFT).
        let facing = 0u8; // North → LEFT = -X (west), RIGHT = +X (east)
        let (dx_first, _) = v_offset(facing, 7); // first drop, payload_post=7 ODD
        let (dx_second, _) = v_offset(facing, 6); // second drop, payload_post=6 EVEN
        assert!(
            dx_first < 0,
            "first drop (count=7, ODD) should be LEFT (-X), got dx={}",
            dx_first,
        );
        assert!(
            dx_second > 0,
            "second drop (count=6, EVEN) should be RIGHT (+X), got dx={}",
            dx_second,
        );
    }

    #[test]
    fn test_v_pattern_facing_north_right_is_east() {
        // Facing 0 (North): RIGHT 90° → facing 64 (East) → +X direction.
        let (dx, dy) = v_offset(0, 0); // EVEN → RIGHT
        assert!(dx > 100, "North-RIGHT should give +X, got dx={}", dx);
        assert!(
            dy.abs() < 30,
            "North-RIGHT should have ~zero Y, got dy={}",
            dy,
        );
    }

    #[test]
    fn test_v_pattern_facing_east_right_is_south() {
        // Facing 64 (East): RIGHT 90° → facing 128 (South) → +Y direction.
        let (dx, dy) = v_offset(64, 0); // EVEN → RIGHT
        assert!(dy > 100, "East-RIGHT should give +Y, got dy={}", dy);
        assert!(
            dx.abs() < 30,
            "East-RIGHT should have ~zero X, got dx={}",
            dx,
        );
    }

    #[test]
    fn test_v_pattern_facing_north_left_is_west() {
        // Facing 0 (North): LEFT 90° → facing 192 (West) → -X direction.
        let (dx, dy) = v_offset(0, 1); // ODD → LEFT
        assert!(dx < -100, "North-LEFT should give -X, got dx={}", dx);
        assert!(
            dy.abs() < 30,
            "North-LEFT should have ~zero Y, got dy={}",
            dy,
        );
    }

    #[test]
    fn test_v_pattern_facing_south_alternates_correctly() {
        // Facing 128 (South): LEFT = facing 64 (East, +X), RIGHT = facing 192 (West, -X).
        let (dx_left, _) = v_offset(128, 1); // ODD → LEFT
        let (dx_right, _) = v_offset(128, 0); // EVEN → RIGHT
        assert!(
            dx_left > 100,
            "South-LEFT should be +X (East), got {}",
            dx_left
        );
        assert!(
            dx_right < -100,
            "South-RIGHT should be -X (West), got {}",
            dx_right
        );
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
        rules.merge_art_data(&art);
        rules.art_registry = art;
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

        let height_map = std::collections::BTreeMap::new();
        let mut wound_down = false;
        let mut played_out = false;
        for _ in 0..60 {
            sim.advance_tick(&[], Some(&rules), &height_map, None, None, 100);
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
                sim_to_i32(passenger.position.sub_x),
                sim_to_i32(passenger.position.sub_y)
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
            PassengerRole::Inside { transport_id } if transport_id == aircraft_id
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
        use crate::sim::combat::combat_weapon::WeaponOverride;
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
                aircraft.weapon_override = Some(WeaponOverride::IfvSlot(99));
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
            assert_eq!(aircraft.weapon_override, Some(WeaponOverride::IfvSlot(99)));
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
