//! Fly movement transaction: legacy horizontal steering followed by native
//! integer height stepping and the native speed control (the target speed
//! and its ramp). Jumpjet, Rocket and Parachute have separate owners.
//!
//! The vertical range is compared against original instructions in
//! `fly_height`, the target speed in `fly_target_speed`. Horizontal_Step's
//! arrival arm, the Process landing trigger and the landing callbacks still
//! need their native migration; the legacy arrival below stands in.
//!
//! A dead (crashing) Fly follows Process natively: its fall block and impact
//! run first (`sim::world::crash`), then the paid step only while IsMoving at
//! its frozen speed and heading, then the height step. The whole fall to the
//! impact is compared frame by frame with `tools/spatial_oracle/aircraft_crash`.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::movement::locomotor::{AirMovePhase, LocomotorState, MovementLayer};
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

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

/// The target speed of a Fly crawling in under the 0.1 slowdown floor
/// (`0x004CE27C`).
const FLY_CRAWL_SPEED: SimFixed = SimFixed::lit("0.1");

/// The 0.05 current speed of Process's creep (`0x004CE2D1`) and of
/// Horizontal_Step's landing test (`0x007E8AE8`), which the legacy arrival
/// below and [`fly_landing_arrival`] stand in for.
const MIN_CREEP_SPEED: SimFixed = SimFixed::lit("0.05");

/// `TechnoTypeClass` constructor `SlowdownDistance` (`0x00710BB2`), for a
/// mover without a resolved type.
const DEFAULT_SLOWDOWN_DISTANCE: i32 = 500;

/// Process `0x004CE441..0x004CE495`: the current speed (`+0x48`) chases the
/// target speed (`+0x40`) by 0.1 a frame.
fn ramp_fly_speed(state: &mut super::fly_height::FlyRuntime) {
    let target = state.target_speed;
    let current = state.current_speed;
    if current < target {
        state.current_speed = (current + FLY_SPEED_RAMP_STEP).min(target);
    } else if current > target {
        state.current_speed = (current - FLY_SPEED_RAMP_STEP).max(target);
    }
}

/// What `0x004D0180` reads to decide whether a Fly slows for its destination.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FlySlowFacts {
    /// The owner is an Aircraft: only an Aircraft has the IFlyControl
    /// subobject (`+0x6C0`, found through QueryInterface `0x00414290`) and a
    /// FlyBy type.
    pub aircraft: bool,
    /// IFlyControl `+0x20` (`0x0041B860`): the release latch, Aircraft `+0x6D2`.
    pub locked: bool,
    /// AircraftType `FlyBy=` (`+0xE0B`).
    pub fly_by: bool,
    /// IFlyControl `+0x18` Is_Strafe (`0x0041B7F0`) or `+0x1C` Is_Fighter
    /// (`0x0041B840`).
    pub strafe_or_fighter: bool,
    /// Techno Ammo (`+0x2FC`), signed.
    pub ammo: i32,
}

/// Fly `0x004D0180`: no while an aircraft's release latch holds; yes while
/// landing or out of cruise mode (`+0x5C`); otherwise no for a FlyBy, yes for
/// an aircraft that neither strafes nor fights, and for the rest (strafers,
/// fighters, non-aircraft owners) only at Ammo 0. Horizontal_Step's arrival
/// arm reads the same gate (`0x004CF4DC`).
fn fly_may_slow(landing: bool, cruise: bool, facts: &FlySlowFacts) -> bool {
    if facts.aircraft && facts.locked {
        return false;
    }
    if landing || !cruise {
        return true;
    }
    if facts.aircraft && facts.fly_by {
        return false;
    }
    if facts.aircraft && !facts.strafe_or_fighter {
        return true;
    }
    facts.ammo == 0
}

/// Everything Process's target-speed writer reads besides the Fly state.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FlySpeedFacts {
    /// Owner Health (`+0x6C`).
    pub health: i32,
    /// Owner GetHeight after the vertical step; read only while taking off.
    pub height: i32,
    /// The `0x004CDDD3` local: XY leptons from the owner to the retained
    /// destination after this frame's paid step.
    pub distance: i32,
    pub slow: FlySlowFacts,
    /// TechnoType `HunterSeeker=` (`+0xD27`).
    pub hunter_seeker: bool,
    /// Techno Target (`+0x2B4`) is set.
    pub target: bool,
    /// TechnoType `SlowdownDistance=` (`+0x2F8`).
    pub slowdown_distance: i32,
}

/// Fly Process `0x004CE145..0x004CE2DB`: the target speed (`+0x40`) that the
/// ramp chases, rewritten on every frame the Fly is moving and its owner
/// lives, is not landing, has climbed to half its takeoff height and holds a
/// destination:
/// - `0x004D0180` refusing: full speed. A locked aircraft, a cruising FlyBy
///   and a cruising strafer or fighter with ammo never slow for their
///   destination.
/// - HunterSeeker: full speed while it has a Target and is not taking off,
///   else a stop.
/// - Otherwise `distance / SlowdownDistance`, capped at 1. Under 0.1 it is a
///   0.1 crawl beyond 85 leptons and, within, a stop that halves the current
///   speed. A current speed above the integer distance drops to it (only at
///   distance 0). A Fly already at a standstill short of its destination gets
///   a 0.05 current speed, which only the IsDropship attitude reads before the
///   ramp (`0x004CE46F`) takes it back to the zero target: it moves nothing.
///
/// Native keeps binary64 and VERA SimFixed, each result within one quantum of
/// native: the ratio truncates to Q16, and the halving rounds a dropped half
/// away from zero, so it is zero exactly when native's is (a speed of one
/// quantum truncated to zero would take the creep's branch). The floor test
/// is the exact `10 * distance <= SlowdownDistance`, assuming gamemd's chop
/// control word `0x0E7F`, under which the oracle runs: the chop quotient of
/// an exact tenth falls below the stored 0.1. Rounded to nearest it would
/// equal 0.1 and take the ratio arm (a 0.1 target for that frame, no
/// halving). The rows of `tools/spatial_oracle/fly_target_speed` hold the
/// native outputs.
pub(crate) fn write_fly_target_speed(loco: &mut LocomotorState, facts: &FlySpeedFacts) {
    let Some(state) = loco.fly_runtime_mut() else {
        return;
    };
    let taking_off = state.taking_off();
    if facts.health <= 0
        || state.landing()
        || (taking_off && facts.height < state.target_height() / 2)
        || state.destination() == (DriveCoord { x: 0, y: 0, z: 0 })
    {
        return;
    }
    if !fly_may_slow(state.landing(), state.cruise_mode(), &facts.slow) {
        state.target_speed = SIM_ONE;
        return;
    }
    if facts.hunter_seeker {
        state.target_speed = if !taking_off && facts.target {
            SIM_ONE
        } else {
            SIM_ZERO
        };
        return;
    }
    let distance = i64::from(facts.distance);
    let slowdown = i64::from(facts.slowdown_distance);
    // A zero SlowdownDistance divides to +inf (NaN at distance 0), which the
    // cap turns into full speed; a negative one is always under the floor.
    if slowdown == 0 || (slowdown > 0 && distance * 10 > slowdown) {
        state.target_speed = if slowdown == 0 || distance >= slowdown {
            SIM_ONE
        } else {
            SimFixed::from_bits(((distance << 16) / slowdown) as i32)
        };
    } else if distance > 0x55 {
        state.target_speed = FLY_CRAWL_SPEED;
    } else {
        let bits = state.current_speed.to_bits();
        state.current_speed = SimFixed::from_bits(bits / 2 + bits % 2);
        state.target_speed = SIM_ZERO;
    }
    if distance << 16 < i64::from(state.current_speed.to_bits()) {
        state.current_speed = SimFixed::from_bits((distance << 16) as i32);
    }
    if state.target_speed == SIM_ZERO && state.current_speed == SIM_ZERO && distance > 0 {
        state.current_speed = MIN_CREEP_SPEED;
    }
}

/// The facts [`write_fly_target_speed`] reads from a live mover. Ammo is
/// represented on Aircraft only; no stock non-aircraft owner flies.
fn fly_speed_facts(
    entity: &crate::sim::game_entity::GameEntity,
    distance: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> FlySpeedFacts {
    let aircraft = entity.category == EntityCategory::Aircraft;
    let object = rules_context
        .and_then(|(rules, interner)| rules.object(interner.resolve(entity.type_ref())));
    let strafe_or_fighter = aircraft
        && rules_context
            .zip(object)
            .is_some_and(|((rules, _), object)| {
                object.fighter
                    || crate::sim::combat::combat_weapon::aircraft_strafes(
                        rules,
                        object,
                        entity.veterancy(),
                    )
            });
    FlySpeedFacts {
        health: entity.health.current,
        height: current_fly_height(entity, terrain),
        distance,
        slow: FlySlowFacts {
            aircraft,
            locked: entity
                .mission_leaf
                .as_aircraft()
                .is_some_and(|leaf| leaf.action_latch() != 0),
            fly_by: aircraft && object.is_some_and(|o| o.fly_by),
            strafe_or_fighter,
            ammo: entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current),
        },
        hunter_seeker: object.is_some_and(|o| o.hunter_seeker),
        target: entity.attack_target.is_some(),
        slowdown_distance: object.map_or(DEFAULT_SLOWDOWN_DISTANCE, |o| o.slowdown_distance),
    }
}

/// Spawn owns Aircraft initialization. Legacy/headless movers may lack the
/// Secondary controller; initialize it once from the body's heading without
/// replacing an existing live turn. The body's rate stands in for `ROT=`:
/// the Aircraft constructor (`0x00413FD2..0x00414015`) gives both
/// controllers that one value.
pub(crate) fn ensure_fly_secondary_facing(entity: &mut crate::sim::game_entity::GameEntity) {
    let initial = entity.body_facing.destination();
    let body = &entity.body_facing;
    entity
        .barrel_facing
        .get_or_insert_with(|| super::FacingClass::with_rate_of(initial, body));
}

/// Shared represented MoveTo/BeginTakeoff refusal gates. EMP and Foot+6A0
/// still require their missing native owners; do not substitute deploy state.
pub(crate) fn fly_coordinate_admitted(entity: &crate::sim::game_entity::GameEntity) -> bool {
    !entity.locomotor.as_ref().is_some_and(|l| !l.powered)
        && !super::locomotor_owner::owner_is_warping(entity)
}

/// Horizontal_Step4CF520's landing arm: within 0x80 leptons (XY) of the
/// retained destination with speed+48 below 0.05, read from the retained
/// destination rather than `movement_target`. Its cruise+5C, type+D27 and
/// 4D0180 gates and its own BeginLanding call belong to the pending Fly
/// navigation migration; the legacy dock drivers call BeginLanding on it.
pub(crate) fn fly_landing_arrival(entity: &crate::sim::game_entity::GameEntity) -> bool {
    let Some(loco) = entity.locomotor.as_ref() else {
        return false;
    };
    let Some(state) = loco.fly_runtime() else {
        return false;
    };
    let destination = state.destination();
    let xy = super::ground_pose::position_world_xy(&entity.position);
    crate::sim::cell_kernel::native_xy_distance(
        destination.x.wrapping_sub(xy[0]),
        destination.y.wrapping_sub(xy[1]),
    ) < 0x80
        && state.current_speed < MIN_CREEP_SPEED
}

/// Per-tick stats for air movement diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct AirMovementTickStats {
    /// Number that completed their move this tick.
    pub arrivals: u32,
    /// A dead Fly's fall reached the ground this visit: Process ends in the
    /// impact (`Simulation::fly_crash_impact`), which its caller commits.
    pub impact: bool,
    /// A Jumpjet touched down54C8F0. The native Unit host has already run
    /// PerCell2 and class NULL synchronously; the Infantry caller continues
    /// its still separate class path.
    pub touched_down: bool,
}

/// Advance one Fly object's Process visit.
///
/// Handles altitude changes and horizontal movement. Air units move in
/// straight lines at their speed, ignoring terrain and ground occupancy.
pub fn tick_air_movement(
    entities: &mut EntityStore,
    entity_id: u64,
    sim_tick: u64,
    binary_frame: u32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> AirMovementTickStats {
    let mut stats = AirMovementTickStats::default();

    // Jumpjets are driven by their own locomotor: `world::jumpjet_cruise`
    // runs `Process 0x0054AEC0`, which owns their state, altitude, facing and
    // speed. This adapter must not touch them, or the two authorities fight
    // (an idle one used to cycle takeoff and landing every 102 frames).
    let Some(entity) = entities.get_mut(entity_id).filter(|e| {
        e.locomotor
            .as_ref()
            .is_some_and(|loco| loco.layer == MovementLayer::Air && loco.kind == LocomotorKind::Fly)
    }) else {
        return stats;
    };
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap()
        .prepare_process(entity.mission.effective());
    ensure_fly_secondary_facing(entity);
    // Process reaches its speed control (the target speed and the ramp)
    // on every frame the Fly is moving (4CDA0B, IsMoving 4CCA90), from
    // its ordinary path (4CD67F..4CD6A8) and its airborne crash path
    // (4CD7A4) alike; only a crash's impact frame (unported) returns
    // first. The writer and the ramp skip a dead owner themselves.
    let speed_control = super::motion_query::is_moving(entity) == Some(true);

    // --- Horizontal movement (facing-based, only when airborne) ---
    // A dead (crashing) Fly takes Process's paid step exactly when IsMoving
    // (`0x004CDA0B`) at its frozen speed and heading: Horizontal_Step, the
    // landing trigger and the speed writer and ramp all need Health > 0
    // (`0x004CCBE9`, `0x004CE3CA`, `0x004CE148`, `0x004CE444`). Evidence:
    // `tools/spatial_oracle/aircraft_crash` `fall` rows.
    let dead = entity.health.current == 0;
    // Process returns at `0x004CDA10` when IsMoving is false, before the
    // paid step and the height step. RESIDUAL: the gate is Process's for
    // every Fly; a living one still reaches the legacy height step below
    // while it stands still (its landing descent is not yet migrated to
    // Process_Landing's callbacks), so only a dead Fly takes it here.
    if dead && !speed_control {
        return stats;
    }
    // The arrival retires the order adapter after the rest of Process, whose
    // navigation request still reads it this frame.
    let mut arrived_at_destination = false;
    let has_movement: bool = if dead {
        speed_control
    } else {
        entity.movement_target.is_some()
    };

    if has_movement {
        let height = current_fly_height(entity, terrain);
        let can_move: bool = dead
            || entity
                .locomotor
                .as_ref()
                .is_some_and(|l| height >= l.fly_target_height() / 2);

        if can_move {
            let native_type_speed = rules_context
                .and_then(|(rules, interner)| rules.object(interner.resolve(entity.type_ref())))
                .map(|object| crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed))
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
                entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .fly_runtime()
                    .unwrap()
                    .current_speed,
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
            let cur_lx =
                I48F16::from_num(entity.position.rx) * lep256 + I48F16::from(entity.position.sub_x);
            let cur_ly =
                I48F16::from_num(entity.position.ry) * lep256 + I48F16::from(entity.position.sub_y);
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

            //4CDA3C..4CDB4C: full Primary.Current, integer Fly speed and
            // final world-coordinate truncation using the shared table.
            if speed > 0 {
                let current = super::ground_pose::position_world_xy(&entity.position);
                let proposed = crate::util::native_trig::facing_step_world_xy(
                    current,
                    entity.body_facing.current(binary_frame),
                    speed,
                );
                // Existing placement boundary remains a separate residual:
                // native4CDB4C..4CDD07 applies map/owner-specific correction.
                // Keep the bounded adapter until those gates are migrated.
                // RESIDUAL: native places the result through
                // FootClass::SetLocation (vt+0x1B4 at 0x004CDD07); this
                // adapter writes XY only and skips its OpenTopped rider
                // tail. Dormant: retail rulesmd.ini makes only the Drive
                // BFRT OpenTopped=yes.
                const MAX_WORLD: i32 = 511 * 256 + 255;
                super::ground_pose::set_position_world_xy(
                    &mut entity.position,
                    [
                        proposed[0].clamp(0, MAX_WORLD),
                        proposed[1].clamp(0, MAX_WORLD),
                    ],
                );
            }

            // Legacy arrival, standing in for Horizontal_Step's arrival
            // arm (4CF4D2) and the Process landing trigger (4CE3C0) until
            // they are ported: close enough AND speed near zero.
            let arrived = !dead
                && dist_i32 < 128
                && entity
                    .locomotor
                    .as_ref()
                    .and_then(LocomotorState::fly_runtime)
                    .is_some_and(|fly| fly.current_speed < MIN_CREEP_SPEED);
            if arrived {
                super::ground_pose::set_position_world_xy(
                    &mut entity.position,
                    [destination.x, destination.y],
                );
                arrived_at_destination = true;
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
    if speed_control {
        let facts = fly_speed_facts(entity, approach_distance, terrain, rules_context);
        write_fly_target_speed(entity.locomotor.as_mut().unwrap(), &facts);
    }
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
            entity.body_facing.set(
                crate::util::direction_tables::facing16_from_delta(dx, dy),
                binary_frame,
            );
        }
    }
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
    if let Some(object) = rules_context.and_then(|(r, i)| r.object(i.resolve(entity.type_ref())))
        && object.is_dropship
        && entity.health.current > 0
        && entity
            .locomotor
            .as_ref()
            .and_then(LocomotorState::fly_runtime)
            .is_some_and(|fly| fly.current_speed > SIM_ZERO && !fly.taking_off())
    {
        entity.flight_attitude.approach(
            approach_distance,
            object.slowdown_distance,
            object.pitch_angle,
        );
    }

    if speed_control
        && entity.health.current > 0
        && let Some(state) = entity
            .locomotor
            .as_mut()
            .and_then(LocomotorState::fly_runtime_mut)
    {
        ramp_fly_speed(state);
    }
    if arrived_at_destination {
        entity.movement_target = None;
    }
    stats
}

/// `ObjectClass::GetHeight @ 0x005F5F40`, shared by every object vtable
/// (`+0x1C8`): the Location's Z less the ground under it and, on a bridge, the
/// deck. An exact coordinate is the Location's Z. An object without one keeps
/// its height apart from its ground ([`object_world_z_leptons`]), so its
/// height is that part ([`object_altitude_leptons`]): an Air-layer
/// locomotor's or an active Hover's altitude.
///
/// [`object_world_z_leptons`]: super::ground_pose::object_world_z_leptons
/// [`object_altitude_leptons`]: super::ground_pose::object_altitude_leptons
pub(crate) fn current_fly_height(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    queried_flight_height(entity, cells.as_ref())
}

fn queried_flight_height(
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> i32 {
    let Some(z) = entity.position.exact_z_leptons else {
        return super::ground_pose::object_altitude_leptons(entity);
    };
    let ground = cells
        .and_then(|cells| {
            super::ground_pose::query_ground_height(
                cells,
                super::ground_pose::position_world_coord(&entity.position),
            )
            .ok()
        })
        .unwrap_or(0);
    super::ground_pose::height_at_z(z, ground, entity.on_bridge)
}

/// Object virtual+50 /5F6B60. A marked grounded object is low; an unmarked
/// object is neither low nor high. This is not the complement of +54.
pub(crate) fn is_low_flying(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    query_flight_predicate(entity, cells.as_ref(), rules_context, false)
}

pub(crate) fn is_low_flying_in_query(
    entity: &crate::sim::game_entity::GameEntity,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    query_flight_predicate(entity, Some(cells), rules_context, false)
}

/// Object virtual+54 /5F6B90 and Aircraft41B920. The two Rules-designated
/// missile aircraft ask their Rocket interface+80 instead of Mark/GetHeight.
/// Existing native comparisons: tools/spatial_oracle/foot_neighbors and
/// object_flight_height. Missing Rules context cannot dispatch named types.
pub(crate) fn is_high_flying(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    query_flight_predicate(entity, cells.as_ref(), rules_context, true)
}

pub(crate) fn is_high_flying_in_query(
    entity: &crate::sim::game_entity::GameEntity,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> bool {
    query_flight_predicate(entity, Some(cells), rules_context, true)
}

fn query_flight_predicate(
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
    high: bool,
) -> bool {
    if let Some(moving) = missile_flight_override(entity, rules_context) {
        // Aircraft41B920 returns moving; Aircraft41B980 returns !moving.
        return moving == high;
    }
    entity.lifecycle.cell_marked
        && (queried_flight_height(entity, cells)
            >= crate::util::lepton::HIGH_FLIGHT_THRESHOLD_LEPTONS as i32)
            == high
}

fn missile_flight_override(
    entity: &crate::sim::game_entity::GameEntity,
    rules_context: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> Option<bool> {
    if entity.category == EntityCategory::Aircraft
        && rules_context.is_some_and(|(rules, interner)| {
            let name = interner.resolve(entity.type_ref());
            name.eq_ignore_ascii_case(&rules.missile_spawn.v3.type_name)
                || name.eq_ignore_ascii_case(&rules.missile_spawn.dmisl.type_name)
        })
    {
        // Native asks the installed locomotor's `Is_Moving_Now` (`0x0041B965`,
        // `0x0041B9C5`). This reads the Rocket body directly: both types run
        // Rocket in retail, and `motion_query::is_moving_now` needs the frame
        // for its Drive and Ship turn arm, which the flight-level callers do
        // not carry.
        return Some(entity.locomotor.as_ref().is_some_and(|locomotor| {
            locomotor
                .rocket_runtime()
                .is_some_and(super::rocket_movement::RocketRuntime::is_moving_now)
        }));
    }
    None
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
    let world_z = super::ground_pose::object_world_z_leptons(entity, terrain);
    let loco = entity
        .locomotor
        .as_mut()
        .expect("Fly process owns a locomotor");
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
    super::ground_pose::mirror_height(entity, output.height);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::util::fixed_math::SIM_HALF;

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
        // The goal is the only route state (no Bresenham, no Foot+5E0 words).
        assert!(e.navigation.path_replay.remaining_directions().is_empty());

        // Should trigger ascending.
        let loco = e.locomotor.as_ref().expect("has loco");
        assert_eq!(loco.air_phase(), AirMovePhase::Ascending);
    }

    /// A vehicle Jumpjet's order takes its own `Move_To`: Foot's NavCom, then
    /// the locomotor's destination at the ordered cell's floor, which the
    /// movement adapter publishes as the goal. Its phase is its own state
    /// field: `AirMovePhase` stays untouched.
    #[test]
    fn a_jumpjet_order_does_not_write_the_fly_phase() {
        // Unit orders now enter the class destination owner, whose type/speed
        // inputs come from the production rules and spawn path.
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
                 [VehicleTypes]\n0=SHAD\n\
                 [SHAD]\nStrength=300\nSpeed=12\nROT=5\n\
                 Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\
                 SpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\n",
            ))
            .expect("Jumpjet Unit rules");
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let id = sim
            .spawn_object("SHAD", "Americans", 10, 10, 0, &rules)
            .expect("Jumpjet Unit");

        assert!(
            sim.issue_air_cell_destination(id, (20, 15), SimFixed::from_num(10), Some(&rules),)
        );

        let e = sim.substrate.entities.get(id).expect("has entity");
        assert_eq!(
            e.movement_target.as_ref().and_then(|t| t.final_goal),
            Some((20, 15)),
            "the order itself is accepted"
        );
        assert_eq!(
            e.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::cell(20, 15))
        );
        let runtime = e
            .locomotor
            .as_ref()
            .and_then(|l| l.jumpjet_runtime())
            .expect("jumpjet runtime");
        assert!(runtime.moving());
        assert_eq!(
            runtime.destination(),
            crate::sim::components::DriveCoord {
                x: 20 * 256 + 128,
                y: 15 * 256 + 128,
                z: 0
            }
        );
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

    fn make_fly_loco() -> LocomotorState {
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Fly);
        loco.fly_runtime_mut().unwrap().target_speed = SIM_ONE;
        loco
    }

    #[test]
    fn test_fly_speed_ramp() {
        let mut fly = crate::sim::movement::fly_height::FlyRuntime::default();
        fly.target_speed = SIM_ONE;
        // After 5 ramps: should be ~0.5 (fixed-point 0.1 is approximate).
        for _ in 0..5 {
            ramp_fly_speed(&mut fly);
        }
        let half_diff = (fly.current_speed - SIM_HALF).abs();
        assert!(
            half_diff < SimFixed::lit("0.001"),
            "Expected ~0.5, got {:?}",
            fly.current_speed
        );
        // After 5 more: should reach exactly 1.0 (clamped by min(target)).
        for _ in 0..5 {
            ramp_fly_speed(&mut fly);
        }
        assert_eq!(fly.current_speed, SIM_ONE);
    }

    #[test]
    fn test_fly_speed_ramp_decel() {
        let mut fly = crate::sim::movement::fly_height::FlyRuntime::default();
        fly.current_speed = SIM_ONE;
        for _ in 0..10 {
            ramp_fly_speed(&mut fly);
        }
        assert_eq!(fly.current_speed, SIM_ZERO);
    }

    /// Every row of `tools/spatial_oracle/fly_target_speed`, the original
    /// Process target-speed writer, through [`write_fly_target_speed`]: the
    /// gate, `0x004D0180`, HunterSeeker and the slowdown law. Target and
    /// current speed land within one Q16 quantum of the native binary64.
    #[test]
    fn native_target_speed_rows() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fly_target_speed.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 374);
        for row in &rows {
            let input = &row["input"];
            let int = |name: &str, fallback: i64| input[name].as_i64().unwrap_or(fallback) as i32;
            let flag = |name: &str| input[name].as_bool().unwrap_or(false);
            let aircraft = input["kind"].as_str().unwrap_or("aircraft") == "aircraft";
            let class = input["class"].as_str().unwrap_or("neither");
            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Fly);
            let destination = input["destination"]
                .as_array()
                .map_or([2944, 2688, 0], |xyz| {
                    [0, 1, 2].map(|n| xyz[n].as_i64().unwrap() as i32)
                });
            *loco.fly_runtime_mut().unwrap() = serde_json::from_value(serde_json::json!({
                "target_height": int("target_height", 1500),
                "taking_off": flag("taking_off"),
                "landing": flag("landing"),
                "destination": destination,
                "cruise_mode": flag("cruise"),
                "moving": true,
            }))
            .unwrap();
            let fly = loco.fly_runtime_mut().unwrap();
            fly.target_speed = SimFixed::from_bits(int("target_speed", 65536));
            fly.current_speed = SimFixed::from_bits(int("current", 32768));
            let facts = FlySpeedFacts {
                health: int("health", 100),
                // The takeoff rows stand over one level-0 cell.
                height: int("z", 1500),
                distance: int("distance", 0),
                slow: FlySlowFacts {
                    aircraft,
                    locked: flag("locked"),
                    fly_by: flag("fly_by"),
                    strafe_or_fighter: aircraft && matches!(class, "strafe" | "fighter"),
                    ammo: int("ammo", 1),
                },
                hunter_seeker: flag("hunter_seeker"),
                target: flag("target"),
                slowdown_distance: int("slowdown", 500),
            };
            write_fly_target_speed(&mut loco, &facts);
            let fly = loco.fly_runtime().unwrap();
            for (actual, native) in [
                (fly.target_speed, &row["target_speed"]),
                (fly.current_speed, &row["current"]),
            ] {
                let native = native.as_f64().unwrap() * 65536.0;
                assert!(
                    (f64::from(actual.to_bits()) - native).abs() < 1.0,
                    "{}: {} vs native {native}",
                    input["name"],
                    actual.to_bits()
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "object_flight_tests.rs"]
mod object_flight_tests;
