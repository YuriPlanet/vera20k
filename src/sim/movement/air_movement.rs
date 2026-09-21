//! Fly movement transaction: legacy horizontal steering followed by native
//! integer height stepping. Jumpjet, Rocket and Parachute have separate owners.
//!
//! The vertical range is compared against original instructions in `fly_height`.
//! Horizontal approach zones, arrival handling and landing callbacks still need
//! their native migration; they are not covered by that height comparison.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::movement::facing_from_delta;
use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState, MovementLayer};
use crate::util::fixed_math::{
    SIM_HALF, SIM_ONE, SIM_ZERO, SimFixed, native_movement_frame_fraction,
};

/// Checked SimFixed multiply — logs a warning and saturates on overflow
/// instead of panicking. Used to diagnose I16F16 overflows in air movement.
#[inline]
fn checked_mul_log(a: SimFixed, b: SimFixed, label: &str, entity_id: u64) -> SimFixed {
    match a.checked_mul(b) {
        Some(v) => v,
        None => {
            log::warn!(
                "air_movement I16F16 overflow at {}: entity={} a={} b={} (saturating)",
                label,
                entity_id,
                a,
                b
            );
            if (a > SIM_ZERO) == (b > SIM_ZERO) {
                SimFixed::MAX
            } else {
                SimFixed::MIN
            }
        }
    }
}

/// Per-tick speed ramp step for Fly aircraft (0.1 per tick).
/// Original: _DAT_007e3860 = 0.1 (verified from binary).
/// Full acceleration 0->1 takes 10 ticks.
const FLY_SPEED_RAMP_STEP: SimFixed = SimFixed::lit("0.1");

/// Fine approach deceleration threshold in leptons (~1/3 cell).
/// Below this distance, speed is halved each tick for smooth landing.
const FINE_APPROACH_THRESHOLD: i32 = 86;

/// Speed halving factor for fine approach deceleration.
const RAPID_DECEL_FACTOR: SimFixed = SimFixed::lit("0.5");

/// Minimum creep speed to prevent zero-speed deadlock during final approach.
const MIN_CREEP_SPEED: SimFixed = SimFixed::lit("0.05");

/// Ramp fly_current_speed toward speed_fraction (target) by +/-FLY_SPEED_RAMP_STEP.
fn ramp_fly_speed(loco: &mut LocomotorState) {
    let target = loco.speed_fraction;
    let current = loco.fly_current_speed;
    if current < target {
        loco.fly_current_speed = (current + FLY_SPEED_RAMP_STEP).min(target);
    } else if current > target {
        loco.fly_current_speed = (current - FLY_SPEED_RAMP_STEP).max(target);
    }
}

/// Legacy distance-based approach zones; native4CE145 uses continuous slowdown.
/// Returns the target speed fraction for the given distance in leptons.
///
/// | Distance          | TargetSpeed |
/// |-------------------|-------------|
/// | >= 768 (3 cells)  | 1.0         |
/// | >= 512 (2 cells)  | 0.75        |
/// | >= 128 (0.5 cell) | 0.5         |
/// | < 128             | 0.0         |
fn approach_target_speed(dist_leptons: i32) -> SimFixed {
    if dist_leptons >= 768 {
        SIM_ONE
    } else if dist_leptons >= 512 {
        SimFixed::lit("0.75")
    } else if dist_leptons >= 128 {
        SIM_HALF
    } else {
        SIM_ZERO
    }
}

/// Turn facing toward desired by at most `rot` steps per tick.
/// Returns the new facing. Handles wrapping around 0/255.
fn turn_facing_toward(current: u8, desired: u8, rot: i32) -> u8 {
    if rot <= 0 || current == desired {
        return desired; // instant turn or already aligned
    }
    let diff = desired.wrapping_sub(current) as i8;
    let abs_diff = (diff as i16).unsigned_abs() as i32;
    if abs_diff <= rot {
        return desired; // close enough, snap
    }
    // Turn by rot in the shorter direction.
    if diff > 0 {
        current.wrapping_add(rot as u8)
    } else {
        current.wrapping_sub(rot as u8)
    }
}

/// Issue a move command for an air unit.
///
/// Returns whether the legacy execution adapter changed. Native MoveTo is void;
/// this bool is not the enclosing Foot AssignDestination/NavCom outcome.
/// Aircraft ignore ordinary terrain blocking
/// (no terrain blocking check needed — they fly over everything).
///
/// Fly retains the resolved Cell coordinate once; its horizontal adapter reads
/// that owner instead of rebuilding a coordinate from the path cache each tick.
/// Native Aircraft/Foot AssignDestination and Fly null/Stop are still separate
/// pending migrations; this is not their complete navigation transaction.
pub fn issue_air_move_command(
    entities: &mut EntityStore,
    entity_id: u64,
    target: (u16, u16),
    speed: SimFixed,
    timing: super::DestinationTiming,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    let is_fly = entities.get(entity_id).is_some_and(|entity| {
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.fly_runtime())
            .is_some()
    });
    let coordinate = if is_fly {
        super::navcom::target_cell_coord(target.0, target.1, terrain)
    } else {
        DriveCoord::cell(target.0, target.1, 0)
    };
    issue_air_coordinate_move_command(
        entities,
        entity_id,
        coordinate,
        speed,
        timing,
        terrain,
        rules_context,
    )
}

/// The non-null coordinate boundary used by the air order adapter. It does not
/// replace the owner NavCom reference or implement requester-specific +4C calls.
pub(crate) fn issue_air_coordinate_move_command(
    entities: &mut EntityStore,
    entity_id: u64,
    request: DriveCoord,
    speed: SimFixed,
    timing: super::DestinationTiming,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    let Some(entity) = entities.get(entity_id) else {
        return false;
    };
    if entity
        .locomotor
        .as_ref()
        .and_then(|l| l.fly_runtime())
        .is_some_and(|state| state.ignores_destination(request))
    {
        return false;
    }
    // Empty coordinates require the separate native null/landing transaction.
    if request == (DriveCoord { x: 0, y: 0, z: 0 }) {
        return false;
    }
    // MoveTo4CCCEE..4CCD3A admits through owner warp/disable and power gates.
    // EMP+504 and the Foot+6A0 timer still need their missing native producers;
    // see world/techno_ai_cloak.rs. Do not substitute deploy_state for either.
    if entity.locomotor.as_ref().is_some_and(|l| !l.powered)
        || entity
            .teleport_state
            .as_ref()
            .is_some_and(|t| t.warp_in_active() || t.warp_out_active())
    {
        return false;
    }

    let flight_level = rules_context.map_or(500, |(rules, interner)| {
        rules
            .object(interner.resolve(entity.type_ref()))
            .map_or(rules.general.flight_level, |object| {
                object.flight_level(rules.general.flight_level)
            })
    });
    let armed_flight_level = (entity.attack_target.is_some()
        && entity
            .aircraft_ammo
            .as_ref()
            .is_some_and(|ammo| ammo.current != 0))
    .then_some(flight_level);
    // Native stores/substitutes the destination BEFORE querying landing base
    // and owner height. The ground query can stamp the shared Cell Dummy.
    if let Some(state) = entities
        .get_mut(entity_id)
        .and_then(|entity| entity.locomotor.as_mut().and_then(|l| l.fly_runtime_mut()))
    {
        state.retain_destination(request, armed_flight_level, || {
            super::ground_pose::ground_surface_z_at([request.x, request.y], false, terrain, None)
                .unwrap_or(0)
        });
    }
    let entity = entities.get(entity_id).expect("selected air mover");
    let begin_takeoff = entity
        .locomotor
        .as_ref()
        .and_then(|loco| loco.fly_runtime())
        .is_some_and(|state| {
            let base =
                crate::sim::aircraft::landing_base::landing_base(entity, entities, rules_context);
            state.should_begin_takeoff(
                entity.health.current,
                || current_fly_height(entity, terrain),
                base,
            )
        });
    // Derived cell projection for remaining mission/path consumers; Fly's XYZ
    // is authoritative. Legacy execution lifetime and speed remain here until
    // the complete native Process/Stop and mission consumers are migrated.
    let target = (
        (request.x / 256) as i16 as u16,
        (request.y / 256) as i16 as u16,
    );
    let movement = MovementTarget {
        path: vec![target],
        path_layers: vec![MovementLayer::Air],
        next_index: 0,
        speed,
        final_goal: Some(target),
        ..Default::default()
    };

    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };
    timing.accept(entity);
    entity.movement_target = Some(movement);

    if begin_takeoff {
        entity
            .locomotor
            .as_mut()
            .expect("selected Fly locomotor")
            .begin_fly_takeoff(flight_level);
    }
    true
}

/// Per-tick stats for air movement diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct AirMovementTickStats {
    /// Number of air entities processed.
    pub air_movers: u32,
    /// Number that completed their move this tick.
    pub arrivals: u32,
}

/// Advance live Fly entities one tick.
///
/// Handles altitude changes and horizontal movement. Air units move in
/// straight lines at their speed, ignoring terrain and ground occupancy.
pub fn tick_air_movement(
    entities: &mut EntityStore,
    live_order: &[u64],
    sim_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> AirMovementTickStats {
    let mut stats = AirMovementTickStats::default();
    let dt = native_movement_frame_fraction();

    // Collect air entity IDs that need processing.
    let air_entity_ids: Vec<u64> = {
        let fallback_order;
        let entity_order: &[u64] = if live_order.is_empty() {
            fallback_order = entities.keys_sorted();
            &fallback_order
        } else {
            live_order
        };
        entity_order
            .iter()
            .copied()
            .filter(|&id| {
                entities.get(id).is_some_and(|e| {
                    e.locomotor.as_ref().is_some_and(|loco| {
                        // Jumpjets are driven by their own locomotor:
                        // `world::jumpjet_cruise` runs `Process 0x0054AEC0`,
                        // which owns their state, altitude, facing and speed.
                        // This adapter must not touch them, or the two
                        // authorities fight (an idle one used to cycle takeoff
                        // and landing every 102 frames).
                        loco.layer == MovementLayer::Air && loco.kind == LocomotorKind::Fly
                    })
                })
            })
            .collect()
    };

    let mut finished: Vec<u64> = Vec::new();

    for &entity_id in &air_entity_ids {
        stats.air_movers = stats.air_movers.saturating_add(1);

        let Some(entity) = entities.get_mut(entity_id) else {
            continue;
        };

        // --- Horizontal movement (facing-based, only when airborne) ---
        let has_movement: bool = entity.movement_target.is_some();

        if has_movement {
            let height = current_fly_height(entity, terrain);
            let can_move: bool = entity
                .locomotor
                .as_ref()
                .is_some_and(|l| height >= l.fly_target_height() / 2);

            if can_move {
                let destination = entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .fly_runtime()
                    .expect("selected Fly mover")
                    .destination();

                // Compute distance to goal in leptons.
                use fixed::types::I48F16;
                let lep256 = I48F16::from_num(256);
                let goal_lx = I48F16::from_num(destination.x);
                let goal_ly = I48F16::from_num(destination.y);
                let cur_lx = I48F16::from_num(entity.position.rx) * lep256
                    + I48F16::from(entity.position.sub_x);
                let cur_ly = I48F16::from_num(entity.position.ry) * lep256
                    + I48F16::from(entity.position.sub_y);
                let dlx = goal_lx - cur_lx;
                let dly = goal_ly - cur_ly;
                let dist_sq = dlx * dlx + dly * dly;
                let dist = if dist_sq <= I48F16::ZERO {
                    I48F16::ZERO
                } else {
                    let two = I48F16::from_num(2);
                    let mut g = dist_sq / two;
                    for _ in 0..20 {
                        if g <= I48F16::ZERO {
                            break;
                        }
                        g = (g + dist_sq / g) / two;
                    }
                    g
                };
                let dist_i32: i32 = dist.to_num::<i32>();

                // 1. Compute desired facing toward goal.
                let face_dx = dlx.to_num::<i32>();
                let face_dy = dly.to_num::<i32>();
                let desired_facing = if face_dx != 0 || face_dy != 0 {
                    facing_from_delta(face_dx, face_dy)
                } else {
                    entity.facing
                };

                // 2. Gradually turn toward desired facing (ROT per tick).
                let rot = entity.locomotor.as_ref().map_or(0, |l| l.rot);
                entity.facing = turn_facing_toward(entity.facing, desired_facing, rot);

                // 3. Set approach target speed based on distance.
                let approach_speed = approach_target_speed(dist_i32);
                if let Some(ref mut loco) = entity.locomotor {
                    // Only lower speed_fraction for approach; missions can set it
                    // higher (e.g., full speed during attack run).
                    loco.speed_fraction = loco.speed_fraction.min(approach_speed);
                }

                // 4. Fine approach deceleration.
                if dist_i32 < FINE_APPROACH_THRESHOLD {
                    if let Some(ref mut loco) = entity.locomotor {
                        loco.fly_current_speed = checked_mul_log(
                            loco.fly_current_speed,
                            RAPID_DECEL_FACTOR,
                            "fly_speed*rapid_decel",
                            entity_id,
                        );
                        if loco.fly_current_speed < MIN_CREEP_SPEED && dist_i32 > 0 {
                            loco.fly_current_speed = MIN_CREEP_SPEED;
                        }
                    }
                }

                // 5. Move in FACING direction (not toward goal).
                let fly_speed = entity
                    .locomotor
                    .as_ref()
                    .map_or(SIM_ZERO, |l| l.fly_current_speed);
                if let Some(ref target) = entity.movement_target {
                    log::debug!(
                        "air_move entity={} type={} speed={} fly_speed={} dt={} alt={} dist_lep={} facing={}",
                        entity_id,
                        entity.type_ref(),
                        target.speed,
                        fly_speed,
                        dt,
                        entity
                            .locomotor
                            .as_ref()
                            .map(|l| l.altitude)
                            .unwrap_or(SIM_ZERO),
                        dist_i32,
                        entity.facing
                    );
                    let sp_fly =
                        checked_mul_log(target.speed, fly_speed, "speed*fly_speed", entity_id);
                    let move_lep = checked_mul_log(sp_fly, dt, "(speed*fly_speed)*dt", entity_id);
                    if move_lep > SIM_ZERO {
                        let (step_x, step_y) =
                            crate::util::facing_table::facing_to_movement(entity.facing, move_lep);
                        let new_lx = cur_lx + I48F16::from(step_x);
                        let new_ly = cur_ly + I48F16::from(step_y);
                        let new_rx = (new_lx / lep256).to_num::<i32>();
                        let new_ry = (new_ly / lep256).to_num::<i32>();
                        entity.position.rx = (new_rx.max(0) as u16).min(511);
                        entity.position.ry = (new_ry.max(0) as u16).min(511);
                        let sub_x = new_lx - I48F16::from_num(entity.position.rx) * lep256;
                        let sub_y = new_ly - I48F16::from_num(entity.position.ry) * lep256;
                        entity.position.sub_x =
                            SimFixed::from_num(sub_x.to_num::<i32>().max(0).min(255));
                        entity.position.sub_y =
                            SimFixed::from_num(sub_y.to_num::<i32>().max(0).min(255));
                    }
                }

                // 6. Arrival detection: close enough AND speed near zero.
                let arrived = dist_i32 < 128
                    && entity
                        .locomotor
                        .as_ref()
                        .is_some_and(|l| l.fly_current_speed < MIN_CREEP_SPEED);
                if arrived {
                    entity.position.rx = destination.x.div_euclid(256) as u16;
                    entity.position.ry = destination.y.div_euclid(256) as u16;
                    entity.position.sub_x = SimFixed::from_num(destination.x.rem_euclid(256));
                    entity.position.sub_y = SimFixed::from_num(destination.y.rem_euclid(256));
                    finished.push(entity_id);
                    stats.arrivals = stats.arrivals.saturating_add(1);
                }
            }
        }

        // Original vertical controller follows the committed XY. Object Z
        // remains authoritative; loco.altitude is a bounded read cache only.
        let phase_before = fly_mission_phase(entity, terrain);
        update_fly_height(entity, terrain, rules_context);
        let phase_after = fly_mission_phase(entity, terrain);
        if phase_before != phase_after {
            entity.push_debug_event(
                sim_tick as u32,
                DebugEventKind::PhaseChange {
                    from: format!("{phase_before:?}"),
                    to: format!("{phase_after:?}"),
                    reason: "height target projection".into(),
                },
            );
        }

        // Speed ramping for Fly aircraft (after altitude and movement).
        if entity.health.current > 0
            && let Some(ref mut loco) = entity.locomotor
        {
            ramp_fly_speed(loco);
        }
    }

    for id in finished {
        if let Some(entity) = entities.get_mut(id) {
            entity.movement_target = None;
        }
    }
    stats
}

/// Object5F5F40 evaluated from retained Z and the current ground/bridge surface.
/// Only pre-migration fixtures without an exact coordinate read the cache.
fn current_fly_height(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    let Some(z) = entity.position.exact_z_leptons else {
        return entity
            .locomotor
            .as_ref()
            .map_or(0, |l| l.altitude.to_num::<i32>());
    };
    let xy = super::ground_pose::position_world_xy(&entity.position);
    let ground = super::ground_pose::ground_surface_z_at(xy, false, terrain, None).unwrap_or(0);
    z.wrapping_sub(ground)
        .wrapping_sub(if entity.on_bridge { 416 } else { 0 })
}

/// Derived compatibility view for aircraft missions; the integer physical
/// height avoids a false never-cruising state above the altitude cache's range.
pub(crate) fn fly_mission_phase(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<AirMovePhase> {
    let state = entity.locomotor.as_ref()?.fly_runtime()?;
    Some(state.mission_phase(current_fly_height(entity, terrain)))
}

fn update_fly_height(
    entity: &mut crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) {
    let xy = super::ground_pose::position_world_xy(&entity.position);
    let ground_z = super::ground_pose::ground_surface_z_at(xy, false, terrain, None).unwrap_or(0);
    let structural_bridge = terrain.is_some_and(|grid| {
        let cell = grid.native_cell_identity(((xy[0] / 256) as i16, (xy[1] / 256) as i16));
        grid.native_cell_flags(cell) & 0x100 != 0
    });
    let object = rules_context
        .and_then(|(rules, interner)| rules.object(interner.resolve(entity.type_ref())));
    let flight_level = rules_context.map_or(500, |(rules, _)| {
        object.map_or(rules.general.flight_level, |o| {
            o.flight_level(rules.general.flight_level)
        })
    });
    let has_passenger = entity.category == crate::map::entities::EntityCategory::Aircraft
        && entity
            .passenger_role
            .cargo()
            .is_some_and(|cargo| cargo.count() != 0);
    let loco = entity
        .locomotor
        .as_mut()
        .expect("Fly process owns a locomotor");
    let world_z = entity.position.exact_z_leptons.unwrap_or_else(|| {
        ground_z
            .wrapping_add(if entity.on_bridge { 416 } else { 0 })
            .wrapping_add(loco.altitude.to_num::<i32>())
    });
    let output = loco
        .fly_runtime()
        .expect("Fly process owns Fly state")
        .step_height(super::fly_height::HeightInput {
            world_z,
            ground_z,
            on_bridge: entity.on_bridge,
            structural_bridge,
            health: entity.health.current,
            has_passenger,
            is_dropship: object.is_some_and(|o| o.is_dropship),
            flight_level,
        });
    entity.position.exact_z_leptons = Some(output.world_z);
    entity.on_bridge = output.on_bridge;
    loco.altitude = SimFixed::saturating_from_num(output.height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::LocomotorSlot;

    #[test]
    fn test_issue_air_move_command() {
        let mut entities = EntityStore::new();
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.locomotor = Some(make_fly_loco());
        entities.insert(entity);

        let ok = issue_air_move_command(
            &mut entities,
            1,
            (20, 15),
            SimFixed::from_num(10),
            crate::sim::movement::DestinationTiming::new(0, 60),
            None,
            None,
        );
        assert!(ok);

        // Should have a MovementTarget with final_goal set.
        let e = entities.get(1).expect("has entity");
        let target = e.movement_target.as_ref().expect("has target");
        assert_eq!(target.final_goal, Some((20, 15)));
        // Path contains only the destination (no Bresenham).
        assert_eq!(target.path.len(), 1);
        assert_eq!(target.path[0], (20, 15));

        // Should trigger ascending.
        let loco = e.locomotor.as_ref().expect("has loco");
        assert_eq!(loco.air_phase(), AirMovePhase::Ascending);
    }

    /// A vehicle Jumpjet's order is accepted through this function too, but
    /// its phase is its own state field: `AirMovePhase` stays untouched.
    #[test]
    fn a_jumpjet_order_does_not_write_the_fly_phase() {
        let mut entities = EntityStore::new();
        let mut entity = GameEntity::test_default(1, "SHAD", "Americans", 10, 10);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
        entities.insert(entity);

        assert!(issue_air_move_command(
            &mut entities,
            1,
            (20, 15),
            SimFixed::from_num(10),
            crate::sim::movement::DestinationTiming::new(0, 60),
            None,
            None,
        ));

        let e = entities.get(1).expect("has entity");
        assert!(e.movement_target.is_some(), "the order itself is accepted");
        assert_eq!(
            e.locomotor.as_ref().unwrap().air_phase(),
            AirMovePhase::Landed
        );
    }

    #[test]
    fn test_issue_air_move_already_at_target() {
        let mut entities = EntityStore::new();
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.locomotor = Some(make_fly_loco());
        entities.insert(entity);

        let ok = issue_air_move_command(
            &mut entities,
            1,
            (10, 10),
            SimFixed::from_num(10),
            crate::sim::movement::DestinationTiming::new(0, 60),
            None,
            None,
        );
        assert!(ok);
        // Native MoveTo accepts a nonnull destination even at the owner cell.
        let e = entities.get(1).expect("has entity");
        assert!(e.movement_target.is_some());
    }

    #[test]
    fn air_movement_uses_live_object_order_not_stable_id_scan() {
        fn build_entities() -> EntityStore {
            let mut entities = EntityStore::new();
            for id in [1, 2] {
                let mut entity = GameEntity::test_default(id, "ORCA", "Americans", 10, 10);
                let mut loco = make_fly_loco();

                loco.set_fly_target_height(600);

                entity.locomotor = Some(loco);
                entities.insert(entity);
            }
            entities
        }

        let mut live_entities = build_entities();
        let mut stable_entities = build_entities();

        let live_stats = tick_air_movement(&mut live_entities, &[2], 0, None, None);
        let stable_stats = tick_air_movement(&mut stable_entities, &[], 0, None, None);

        assert_eq!(
            live_stats.air_movers, 1,
            "nonempty live order limits air movement to live-vector members"
        );
        assert_eq!(
            live_entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .altitude,
            SIM_ZERO,
            "stable-id-first air mover absent from live order must not tick"
        );
        assert!(
            live_entities
                .get(2)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .altitude
                > SIM_ZERO,
            "live-order member must tick"
        );
        assert_eq!(
            stable_stats.air_movers, 2,
            "empty live order preserves direct-test stable-id fallback"
        );
        assert!(
            stable_entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .altitude
                > SIM_ZERO,
            "stable-id fallback still ticks directly inserted air movers"
        );
    }

    fn make_fly_loco() -> LocomotorState {
        LocomotorState {
            kind: crate::rules::locomotor_type::LocomotorKind::Fly,
            slot: LocomotorSlot::from_kind(LocomotorKind::Fly),
            powered: true,
            piggyback: None,
            runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
                LocomotorKind::Fly,
                0,
            ),
            layer: MovementLayer::Air,
            phase: crate::sim::movement::locomotor::GroundMovePhase::Idle,

            speed_multiplier: SIM_ONE,
            speed_fraction: SIM_ONE,
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,

            jumpjet_speed: SIM_ZERO,
            jumpjet_accel: SIM_ZERO,
            jumpjet_current_speed: SIM_ZERO,
            jumpjet_deviation: 0,
            jumpjet_crash_speed: SIM_ZERO,
            jumpjet_turn_rate: 4,
            balloon_hover: false,
            hover_attack: false,
            speed_type: crate::rules::locomotor_type::SpeedType::Track,
            movement_zone: crate::rules::locomotor_type::MovementZone::Normal,
            rot: 0,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: crate::util::fixed_math::SIM_ZERO,
            hover_speed_request: crate::util::fixed_math::SIM_ZERO,
            hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
        }
    }

    #[test]
    fn test_fly_speed_ramp() {
        let mut loco = make_fly_loco();
        loco.speed_fraction = SIM_ONE; // target = 1.0
        loco.fly_current_speed = SIM_ZERO; // start at 0
        // After 5 ramps: should be ~0.5 (fixed-point 0.1 is approximate).
        for _ in 0..5 {
            ramp_fly_speed(&mut loco);
        }
        let half_diff = (loco.fly_current_speed - SIM_HALF).abs();
        assert!(
            half_diff < SimFixed::lit("0.001"),
            "Expected ~0.5, got {:?}",
            loco.fly_current_speed
        );
        // After 5 more: should reach exactly 1.0 (clamped by min(target)).
        for _ in 0..5 {
            ramp_fly_speed(&mut loco);
        }
        assert_eq!(loco.fly_current_speed, SIM_ONE);
    }

    #[test]
    fn test_fly_speed_ramp_decel() {
        let mut loco = make_fly_loco();
        loco.speed_fraction = SIM_ZERO; // target = 0.0
        loco.fly_current_speed = SIM_ONE; // start at 1.0
        for _ in 0..10 {
            ramp_fly_speed(&mut loco);
        }
        assert_eq!(loco.fly_current_speed, SIM_ZERO);
    }

    #[test]
    fn test_turn_facing_toward() {
        // Turn from 0 toward 10 with rot=3: should go 0 -> 3
        assert_eq!(turn_facing_toward(0, 10, 3), 3);
        // Turn from 0 toward 2 with rot=3: snap to 2
        assert_eq!(turn_facing_toward(0, 2, 3), 2);
        // Turn from 0 toward 250 (shorter path is clockwise-negative, wrapping):
        // diff = 250u8.wrapping_sub(0) = 250, as i8 = -6.
        // abs_diff = 6, > rot=3. diff < 0 so subtract: 0.wrapping_sub(3) = 253
        assert_eq!(turn_facing_toward(0, 250, 3), 253);
        // rot=0: instant snap
        assert_eq!(turn_facing_toward(50, 200, 0), 200);
        // Already aligned
        assert_eq!(turn_facing_toward(128, 128, 5), 128);
    }

    #[test]
    fn test_approach_speed_zones() {
        assert_eq!(approach_target_speed(1000), SIM_ONE);
        assert_eq!(approach_target_speed(768), SIM_ONE);
        assert_eq!(approach_target_speed(600), SimFixed::lit("0.75"));
        assert_eq!(approach_target_speed(512), SimFixed::lit("0.75"));
        assert_eq!(approach_target_speed(300), SIM_HALF);
        assert_eq!(approach_target_speed(128), SIM_HALF);
        assert_eq!(approach_target_speed(100), SIM_ZERO);
        assert_eq!(approach_target_speed(0), SIM_ZERO);
    }
}
