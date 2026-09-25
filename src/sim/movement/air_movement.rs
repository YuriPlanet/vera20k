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
fn ramp_fly_speed(loco: &mut LocomotorState) {
    let target = loco.speed_fraction;
    let current = loco.fly_current_speed;
    if current < target {
        loco.fly_current_speed = (current + FLY_SPEED_RAMP_STEP).min(target);
    } else if current > target {
        loco.fly_current_speed = (current - FLY_SPEED_RAMP_STEP).max(target);
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
    let Some(state) = loco.fly_runtime() else {
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
        loco.speed_fraction = SIM_ONE;
        return;
    }
    if facts.hunter_seeker {
        loco.speed_fraction = if !taking_off && facts.target {
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
        loco.speed_fraction = if slowdown == 0 || distance >= slowdown {
            SIM_ONE
        } else {
            SimFixed::from_bits(((distance << 16) / slowdown) as i32)
        };
    } else if distance > 0x55 {
        loco.speed_fraction = FLY_CRAWL_SPEED;
    } else {
        let bits = loco.fly_current_speed.to_bits();
        loco.fly_current_speed = SimFixed::from_bits(bits / 2 + bits % 2);
        loco.speed_fraction = SIM_ZERO;
    }
    if distance << 16 < i64::from(loco.fly_current_speed.to_bits()) {
        loco.fly_current_speed = SimFixed::from_bits((distance << 16) as i32);
    }
    if loco.speed_fraction == SIM_ZERO && loco.fly_current_speed == SIM_ZERO && distance > 0 {
        loco.fly_current_speed = MIN_CREEP_SPEED;
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
                        entity.veterancy,
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
        && loco.fly_current_speed < MIN_CREEP_SPEED
}

/// Per-tick stats for air movement diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct AirMovementTickStats {
    /// Number of air entities processed.
    pub air_movers: u32,
    /// Number that completed their move this tick.
    pub arrivals: u32,
    /// A dead Fly's fall reached the ground this visit: Process ends in the
    /// impact (`Simulation::fly_crash_impact`), which its caller commits.
    pub impact: bool,
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
            continue;
        }
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

                // Legacy arrival, standing in for Horizontal_Step's arrival
                // arm (4CF4D2) and the Process landing trigger (4CE3C0) until
                // they are ported: close enough AND speed near zero.
                let arrived = !dead
                    && dist_i32 < 128
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

        if speed_control
            && entity.health.current > 0
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
        // Path contains only the destination (no Bresenham).
        assert_eq!(target.path.len(), 1);
        assert_eq!(target.path[0], (20, 15));

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
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        let cells = (0..32)
            .flat_map(|y| {
                (0..32)
                    .map(move |x| crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false))
            })
            .collect();
        sim.resolved_terrain =
            Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(32, 32, cells));
        // A playfield holding cells (10..20, 10..15) for the order's search.
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 16,
            off_fc: 0,
            off_100: 0,
            off_104: 24,
            off_108: 24,
        });
        sim.playfield_size_height = Some(24);
        let mut entity = GameEntity::test_default(1, "SHAD", "Americans", 10, 10);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
        sim.substrate.entities.insert(entity);

        assert!(sim.issue_air_cell_destination(1, (20, 15), SimFixed::from_num(10), None,));

        let e = sim.substrate.entities.get(1).expect("has entity");
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
        assert!(runtime.moving);
        assert_eq!(
            runtime.destination,
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

    /// Every row of `tools/spatial_oracle/fly_target_speed`, the original
    /// Process target-speed writer, through [`write_fly_target_speed`]: the
    /// gate, `0x004D0180`, HunterSeeker and the slowdown law. Target and
    /// current speed land within one Q16 quantum of the native binary64.
    #[test]
    fn native_target_speed_rows() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_target_speed.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 374);
        for row in &rows {
            let input = &row["input"];
            let int = |name: &str, fallback: i64| input[name].as_i64().unwrap_or(fallback) as i32;
            let flag = |name: &str| input[name].as_bool().unwrap_or(false);
            let aircraft = input["kind"].as_str().unwrap_or("aircraft") == "aircraft";
            let class = input["class"].as_str().unwrap_or("neither");
            let mut loco = make_fly_loco();
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
            loco.speed_fraction = SimFixed::from_bits(int("target_speed", 65536));
            loco.fly_current_speed = SimFixed::from_bits(int("current", 32768));
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
            for (actual, native) in [
                (loco.speed_fraction, &row["target_speed"]),
                (loco.fly_current_speed, &row["current"]),
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
