//! Fly movement transaction: legacy horizontal steering followed by native
//! integer height stepping. Jumpjet, Rocket and Parachute have separate owners.
//!
//! The vertical range is compared against original instructions in `fly_height`.
//! Horizontal approach zones, arrival handling and landing callbacks still need
//! their native migration; they are not covered by that height comparison.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState, MovementLayer};
use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SIM_ZERO, SimFixed};

/// Fly interface+84 /4CFE20 reads TYPE Speed, not Foot's adjusted speed.
/// The retained fraction follows the existing SimFixed policy. Its dyadic
/// product with the bounded type integer is exact in i64; truncate toward zero
/// once, before trig. Do not truncate a per-second displacement or multiply
/// two I16F16 values that can overflow at high authored speed/fraction.
pub(crate) fn current_fly_speed(type_speed: i32, fraction: SimFixed) -> i32 {
    (i64::from(type_speed) * i64::from(fraction.to_bits()) / 65536) as i32
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

/// Spawn owns Aircraft initialization. Legacy/headless movers may lack these
/// controllers; initialize them once without replacing an existing live turn.
pub(crate) fn ensure_fly_facings(entity: &mut crate::sim::game_entity::GameEntity) {
    let initial = u16::from(entity.facing) << 8;
    let rot = entity.locomotor.as_ref().map_or(0, |l| l.rot);
    entity
        .body_facing
        .get_or_insert_with(|| super::FacingClass::new(initial, rot));
    entity
        .barrel_facing
        .get_or_insert_with(|| super::FacingClass::new(initial, rot));
}

/// Shared represented MoveTo/BeginTakeoff refusal gates. EMP and Foot+6A0
/// still require their missing native owners; do not substitute deploy state.
pub(crate) fn fly_coordinate_admitted(entity: &crate::sim::game_entity::GameEntity) -> bool {
    !entity.locomotor.as_ref().is_some_and(|l| !l.powered)
        && !super::locomotor_owner::owner_is_warping(entity)
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
    binary_frame: u32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> AirMovementTickStats {
    let mut stats = AirMovementTickStats::default();

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
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap()
            .prepare_process(entity.mission.effective());
        ensure_fly_facings(entity);
        // Fly4CDA62 reads Primary.Current before navigation/phase setters.
        // This byte is a presentation/legacy projection; displacement below
        // reads the full retained direction, not this quantized cache.
        entity.facing = (entity.body_facing.unwrap().current(binary_frame) >> 8) as u8;

        // --- Horizontal movement (facing-based, only when airborne) ---
        let has_movement: bool = entity.movement_target.is_some();

        if has_movement {
            let height = current_fly_height(entity, terrain);
            let can_move: bool = entity
                .locomotor
                .as_ref()
                .is_some_and(|l| height >= l.fly_target_height() / 2);

            if can_move {
                let native_type_speed = rules_context
                    .and_then(|(rules, interner)| rules.object(interner.resolve(entity.type_ref())))
                    .map(|object| {
                        crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed)
                    })
                    // Mapless/headless compatibility only; production rules
                    // are authoritative even when an order cache is stale.
                    .or_else(|| {
                        entity
                            .movement_target
                            .as_ref()
                            .map(|target| (target.speed / SimFixed::from_num(15)).to_num::<i32>())
                    })
                    .unwrap_or(0);
                let speed = current_fly_speed(
                    native_type_speed,
                    entity.locomotor.as_ref().unwrap().fly_current_speed,
                );
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

                // The native step consumes entry speed BEFORE approach
                // slowdown. The remaining policy below is still the legacy
                // adapter, pending the full4CE145/4CEFB0 migration.
                let approach_speed = approach_target_speed(dist_i32);
                if let Some(ref mut loco) = entity.locomotor {
                    // Only lower speed_fraction for approach; missions can set it
                    // higher (e.g., full speed during attack run).
                    loco.speed_fraction = loco.speed_fraction.min(approach_speed);
                }

                // 4. Fine approach deceleration.
                if dist_i32 < FINE_APPROACH_THRESHOLD {
                    if let Some(ref mut loco) = entity.locomotor {
                        loco.fly_current_speed *= RAPID_DECEL_FACTOR;
                        if loco.fly_current_speed < MIN_CREEP_SPEED && dist_i32 > 0 {
                            loco.fly_current_speed = MIN_CREEP_SPEED;
                        }
                    }
                }

                //4CDA3C..4CDB4C: full Primary.Current, integer Fly speed and
                // final world-coordinate truncation using the shared table.
                if speed > 0 {
                    let current = super::ground_pose::position_world_xy(&entity.position);
                    let proposed = crate::util::native_trig::facing_step_world_xy(
                        current,
                        entity.body_facing.unwrap().current(binary_frame),
                        speed,
                    );
                    // Existing placement boundary remains a separate residual:
                    // native4CDB4C..4CDD07 applies map/owner-specific correction.
                    // Keep the bounded adapter until those gates are migrated.
                    entity.position.rx = (proposed[0] / 256).clamp(0, 511) as u16;
                    entity.position.ry = (proposed[1] / 256).clamp(0, 511) as u16;
                    entity.position.sub_x = SimFixed::from_num(
                        (proposed[0] - i32::from(entity.position.rx) * 256).clamp(0, 255),
                    );
                    entity.position.sub_y = SimFixed::from_num(
                        (proposed[1] - i32::from(entity.position.ry) * 256).clamp(0, 255),
                    );
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

        //4CDD75..4CDDD3 samples distance after XY placement and BEFORE vertical
        // motion/landing drift. The later pitch writer consumes this same local.
        let destination = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .destination();
        let xy = super::ground_pose::position_world_xy(&entity.position);
        let approach_distance = crate::sim::cell_kernel::native_xy_distance(
            destination.x.wrapping_sub(xy[0]),
            destination.y.wrapping_sub(xy[1]),
        );

        // Original vertical controller follows the committed XY. Object Z
        // remains authoritative; loco.altitude is a bounded read cache only.
        let phase_before = fly_mission_phase(entity, terrain);
        update_fly_height(entity, terrain, rules_context);
        // Process4CCC15..4CCC49 requests navigation after movement/height and
        // before phase callbacks. Destination choice is still the legacy
        // adapter pending4CEFB0's docking/strafe migration. Its heading request
        // now uses Primary.Set and native phase/readiness suppression.
        let state = entity.locomotor.as_ref().unwrap().fly_runtime().unwrap();
        let navigation = entity.movement_target.is_some()
            && entity.locomotor.as_ref().unwrap().powered
            && entity.health.current > 0
            && !state.has_phase_callback()
            && entity
                .mission_leaf
                .as_aircraft()
                .is_none_or(|leaf| leaf.action_latch() == 0)
            && current_fly_height(entity, terrain) > 0;
        if navigation {
            let destination = state.destination();
            let xy = super::ground_pose::position_world_xy(&entity.position);
            let dx = destination.x.wrapping_sub(xy[0]);
            let dy = destination.y.wrapping_sub(xy[1]);
            if dx != 0 || dy != 0 {
                entity.body_facing.as_mut().unwrap().set(
                    crate::util::direction_tables::facing16_from_delta(dx, dy),
                    binary_frame,
                );
            }
        }
        entity.facing = (entity.body_facing.unwrap().current(binary_frame) >> 8) as u8;
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

        //4CE2E5..4CE3BA approach attitude. Only IsDropship produces Techno+2E8.
        if let Some(object) =
            rules_context.and_then(|(r, i)| r.object(i.resolve(entity.type_ref())))
            && object.is_dropship
            && entity.health.current > 0
            && entity.locomotor.as_ref().is_some_and(|l| {
                l.fly_current_speed > SIM_ZERO && l.fly_runtime().is_some_and(|s| !s.taking_off())
            })
        {
            entity.flight_attitude.approach(
                approach_distance,
                object.slowdown_distance,
                object.pitch_angle,
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
pub(crate) fn current_fly_height(
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
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.locomotor = Some(make_fly_loco());
        sim.substrate.entities.insert(entity);

        let ok = sim.issue_air_cell_destination(1, (20, 15), SimFixed::from_num(10), None);
        assert!(ok);

        // Should have a MovementTarget with final_goal set.
        let e = sim.substrate.entities.get(1).expect("has entity");
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
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        let mut entity = GameEntity::test_default(1, "SHAD", "Americans", 10, 10);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
        sim.substrate.entities.insert(entity);

        assert!(sim.issue_air_cell_destination(1, (20, 15), SimFixed::from_num(10), None,));

        let e = sim.substrate.entities.get(1).expect("has entity");
        assert!(e.movement_target.is_some(), "the order itself is accepted");
        assert_eq!(
            e.locomotor.as_ref().unwrap().air_phase(),
            AirMovePhase::Landed
        );
    }

    #[test]
    fn test_issue_air_move_already_at_target() {
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.locomotor = Some(make_fly_loco());
        sim.substrate.entities.insert(entity);

        let ok = sim.issue_air_cell_destination(1, (10, 10), SimFixed::from_num(10), None);
        assert!(ok);
        // Native MoveTo accepts a nonnull destination even at the owner cell.
        let e = sim.substrate.entities.get(1).expect("has entity");
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

        let live_stats = tick_air_movement(&mut live_entities, &[2], 0, 0, None, None);
        let stable_stats = tick_air_movement(&mut stable_entities, &[], 0, 0, None, None);

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
