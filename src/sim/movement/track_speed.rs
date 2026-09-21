//! ProcessMovement publishes the class target; TrackProcess consumes it.
//! This fixed-point scaffold still lacks native double precision, raw ramp
//! speed/full-XYZ distance, and the Foot getter's house/crate/CTF inputs.
//! Exact numeric helpers remain in track_speed_native pending owner migration.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed, isqrt_i64};

/// Native Drive4B3DFA..3E21 publishes the request separately from its later
/// TrackProcess prefix. Signed selectors >=64 preserve the class target and
/// send the request to the Foot setter instead. Ship has the same branch.
/// The current fresh caller publishes only after successful selection, so
/// turning/refusal does not execute this prefix. The world fresh continuation
/// must place this write after first CanEnter and before its entering callback.
pub(super) fn publish_fresh_target(
    entity: &mut GameEntity,
    rules: Option<&RuleSet>,
    strength: Option<i32>,
    terrain: Option<&ResolvedTerrainGrid>,
    terrain_speed: &TerrainSpeedConfig,
) {
    let Some(loco) = entity.locomotor.as_ref() else {
        return;
    };
    let kind = loco.kind;
    if !matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship) {
        return;
    }
    let target = entity.movement_target.as_ref();
    let next = target.and_then(|target| target.path.get(target.next_index).copied());
    let below_yellow = rules.zip(strength).is_some_and(|(r, strength)| {
        crate::sim::pathfinding::terrain_speed::is_at_or_below_condition_yellow(
            entity.health.current,
            strength,
            r.general.condition_yellow,
        )
    });
    let requested = match (terrain, next) {
        (Some(terrain), Some(next)) => {
            let xy = super::ground_pose::position_world_xy(&entity.position);
            super::drive_locomotion::compute_drive_target_speed_fraction(
                loco.speed_type,
                kind,
                (xy[0], xy[1]),
                next,
                terrain,
                terrain_speed,
                below_yellow,
            )
        }
        _ => SIM_ONE,
    };
    let (selector, retained) = match kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.get_or_insert_with(Default::default);
            (state.track.turn_index, &mut state.target_speed_fraction)
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.get_or_insert_with(Default::default);
            (state.track.turn_index, &mut state.target_speed_fraction)
        }
        _ => unreachable!(),
    };
    if selector < 64 {
        *retained = requested;
    } else if entity.foot_speed.applied_fraction != requested {
        entity.foot_speed.applied_fraction = requested.clamp(SIM_ZERO, SIM_ONE);
    }
}

/// One TrackProcess invocation consumes its retained class target, even when
/// callbacks have changed the path, terrain, health, or destination request.
pub(super) fn advance(
    entity: &mut GameEntity,
    object: Option<&ObjectType>,
    rules: Option<&RuleSet>,
    grid: Option<&PathGrid>,
) -> i32 {
    let Some(loco) = entity.locomotor.as_ref() else {
        return 0;
    };
    let kind = loco.kind;
    if !matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship) {
        return 0;
    }
    let speed = super::foot_speed::adjusted_speed(
        entity,
        object,
        rules.map_or(1.0, |r| r.general.veteran_speed),
    );
    let target = entity.movement_target.as_ref();
    let class_goal = if kind == LocomotorKind::Drive {
        entity
            .drive_locomotion
            .as_ref()
            .and_then(|d| d.destination.or(d.head_to))
    } else {
        entity
            .ship_locomotion
            .as_ref()
            .and_then(|d| d.destination.or(d.head_to))
    };
    let goal = target.and_then(|t| t.final_goal.or_else(|| t.path.last().copied()));
    let current = super::ground_pose::position_world_xy(&entity.position);
    let goal_xy = goal
        .map(|(x, y)| [i32::from(x) * 256 + 128, i32::from(y) * 256 + 128])
        .or_else(|| class_goal.map(|c| [c.x, c.y]))
        .unwrap_or(current);
    let dx = i64::from(goal_xy[0]) - i64::from(current[0]);
    let dy = i64::from(goal_xy[1]) - i64::from(current[1]);
    let mut distance = SimFixed::from_num(isqrt_i64(dx * dx + dy * dy) as i32);
    if loco.movement_zone.is_water_mover()
        && grid
            .and_then(|g| g.cell(entity.position.rx, entity.position.ry))
            .is_some_and(|cell| cell.bridge_deck_level_if_any().is_some())
    {
        distance += super::movement_bridge::BRIDGE_Z_OFFSET;
    }
    let accel = object
        .map(|o| o.accel_factor)
        .or_else(|| target.map(|t| t.accel_factor))
        .unwrap_or(SIM_ZERO);
    let decel = object
        .map(|o| o.decel_factor)
        .or_else(|| target.map(|t| t.decel_factor))
        .unwrap_or(SIM_ZERO);
    let slowdown = object
        .map(|o| SimFixed::from_num(o.slowdown_distance))
        .or_else(|| target.map(|t| t.slowdown_distance))
        .unwrap_or(SIM_ZERO);
    let unit_passive = entity.category == crate::map::entities::EntityCategory::Unit
        && object.is_some_and(|object| object.passive);
    match kind {
        LocomotorKind::Drive => {
            if let Some(drive) = entity.drive_locomotion.as_ref() {
                super::drive_locomotion::update_drive_speed_fraction(
                    drive,
                    &mut entity.foot_speed,
                    entity.drive_accelerates,
                    unit_passive,
                    speed / SimFixed::from_num(15),
                    accel,
                    decel,
                    slowdown,
                    distance,
                );
            }
        }
        LocomotorKind::Ship => {
            if let Some(ship) = entity.ship_locomotion.as_ref() {
                super::drive_locomotion::update_ship_speed_fraction(
                    ship,
                    &mut entity.foot_speed,
                    entity.drive_accelerates,
                    unit_passive,
                    speed / SimFixed::from_num(15),
                    accel,
                    decel,
                    slowdown,
                    distance,
                );
            }
        }
        _ => unreachable!(),
    }
    entity.foot_speed.cached_current_speed = super::foot_speed::owner_current_speed_from_fraction(
        speed,
        entity.foot_speed.applied_fraction,
    );
    if let Some(target) = entity.movement_target.as_mut() {
        target.current_speed = speed * entity.foot_speed.applied_fraction;
    }
    entity.foot_speed.cached_current_speed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::{DriveLocomotionRuntime, ShipLocomotionRuntime};
    use crate::sim::movement::locomotor::LocomotorState;

    fn entity(kind: LocomotorKind, selector: i32, target: SimFixed) -> GameEntity {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
        entity.locomotor = Some(LocomotorState::for_test_kind(kind));
        match kind {
            LocomotorKind::Drive => {
                let mut state = DriveLocomotionRuntime::default();
                state.track.turn_index = selector;
                state.track_valid = true;
                state.target_speed_fraction = target;
                entity.drive_locomotion = Some(state);
            }
            LocomotorKind::Ship => {
                let mut state = ShipLocomotionRuntime::default();
                state.track.turn_index = selector;
                state.track_valid = true;
                state.target_speed_fraction = target;
                entity.ship_locomotion = Some(state);
            }
            _ => unreachable!(),
        }
        entity
    }

    fn retained(entity: &GameEntity) -> SimFixed {
        match entity.locomotor.as_ref().unwrap().kind {
            LocomotorKind::Drive => {
                entity
                    .drive_locomotion
                    .as_ref()
                    .unwrap()
                    .target_speed_fraction
            }
            LocomotorKind::Ship => {
                entity
                    .ship_locomotion
                    .as_ref()
                    .unwrap()
                    .target_speed_fraction
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn fresh_publication_uses_signed_selector_and_preserves_the_other_speed_owner() {
        for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            for selector in [-1, 63, 64, 71] {
                let mut entity = entity(kind, selector, SimFixed::lit("0.5"));
                entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
                publish_fresh_target(
                    &mut entity,
                    None,
                    None,
                    None,
                    &TerrainSpeedConfig::default(),
                );
                let expected = if selector < 64 {
                    (SIM_ONE, SimFixed::lit("0.25"))
                } else {
                    (SimFixed::lit("0.5"), SIM_ONE)
                };
                assert_eq!(
                    (retained(&entity), entity.foot_speed.applied_fraction),
                    expected,
                    "{kind:?} selector {selector}"
                );
            }
        }
    }

    #[test]
    fn rules_backed_mixed_formation_caps_actual_drive_and_ship_motion() {
        use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
        use crate::sim::world::Simulation;
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=SLOW\n1=FAST\n[SLOW]\nStrength=100\nSpeed=4\nAccelerates=no\n[FAST]\nStrength=100\nSpeed=8\nAccelerates=no\n",
        )).unwrap();
        // This preserves the existing VERA group-Move policy. It is a real
        // rules-backed Process regression, not a claim about native formations.
        for fast_kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            let mut sim = Simulation::with_seed(0);
            let slow_kind = if fast_kind == LocomotorKind::Drive {
                LocomotorKind::Ship
            } else {
                LocomotorKind::Drive
            };
            let mut before = Vec::new();
            for (id, name, kind, raw_speed, x) in
                [(1, "SLOW", slow_kind, 4, 8), (2, "FAST", fast_kind, 8, 12)]
            {
                let mut mover = GameEntity::test_default(id, name, "Americans", x, 8);
                mover.type_ref = sim.intern(name);
                mover.owner = sim.intern("Americans");
                mover.lifecycle.object_alive = true;
                mover.lifecycle.in_limbo = false;
                mover.lifecycle.cell_marked = true;
                mover.position.sub_x = SimFixed::from_num(128);
                mover.position.sub_y = SimFixed::from_num(128);
                mover.locomotor = Some(LocomotorState::for_test_kind(kind));
                mover.drive_accelerates = false;
                let head = DriveCoord::cell(x, 7, 0);
                match kind {
                    LocomotorKind::Drive => {
                        mover.drive_locomotion = Some(DriveLocomotionRuntime {
                            head_to: Some(head),
                            destination: Some(head),
                            track_valid: true,
                            track: crate::sim::components::TrackProgress {
                                turn_index: 0,
                                cursor: 0,
                                ..Default::default()
                            },
                            target_speed_fraction: SIM_ONE,
                            ..Default::default()
                        })
                    }
                    LocomotorKind::Ship => {
                        mover.ship_locomotion = Some(ShipLocomotionRuntime {
                            head_to: Some(head),
                            destination: Some(head),
                            track_valid: true,
                            track: crate::sim::components::TrackProgress {
                                turn_index: 0,
                                cursor: 0,
                                ..Default::default()
                            },
                            target_speed_fraction: SIM_ONE,
                            ..Default::default()
                        })
                    }
                    _ => unreachable!(),
                }
                mover.navigation.nav_com = Some(NavTargetRef::cell(x, 7));
                mover.movement_target = Some(MovementTarget {
                    path: vec![(x, 8), (x, 7)],
                    next_index: 1,
                    final_goal: Some((x, 7)),
                    speed: ra2_speed_to_leptons_per_second(raw_speed),
                    group_id: Some(7),
                    ..Default::default()
                });
                before.push(super::super::ground_pose::position_world_xy(
                    &mover.position,
                ));
                sim.substrate.entities.insert(mover);
            }
            sim.substrate.occupancy =
                crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
            super::super::sync_formation_speeds_after_live_pass(&mut sim.substrate.entities);
            let cap = ra2_speed_to_leptons_per_second(4);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(2)
                    .unwrap()
                    .movement_target
                    .as_ref()
                    .unwrap()
                    .speed,
                cap
            );
            for frame in 0..4 {
                sim.session.binary_frame = frame;
                for id in [1, 2] {
                    sim.process_ground_locomotor_for_test(id, Some(&rules), None, None)
                        .unwrap();
                    let mover = sim.substrate.entities.get(id).unwrap();
                    assert_eq!(
                        mover.foot_speed.cached_current_speed, 10,
                        "{fast_kind:?}, mover {id}"
                    );
                    assert_eq!(mover.movement_target.as_ref().unwrap().current_speed, cap);
                }
            }
            let after: Vec<_> = [1, 2]
                .into_iter()
                .map(|id| {
                    super::super::ground_pose::position_world_xy(
                        &sim.substrate.entities.get(id).unwrap().position,
                    )
                })
                .collect();
            let slow_delta = [after[0][0] - before[0][0], after[0][1] - before[0][1]];
            let fast_delta = [after[1][0] - before[1][0], after[1][1] - before[1][1]];
            assert_ne!(slow_delta, [0, 0]);
            assert_eq!(
                fast_delta, slow_delta,
                "mixed formation with fast {fast_kind:?}"
            );
        }
    }

    #[test]
    fn live_type_speed_wins_without_group_and_caps_cannot_increase_it() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=100\nSpeed=6\nAccelerates=no\n",
        ))
        .unwrap();
        for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            for (group_id, cached_speed) in [(None, 15), (Some(7), 1500)] {
                let mut mover = entity(kind, 0, SIM_ONE);
                mover.drive_accelerates = false;
                mover.movement_target = Some(crate::sim::components::MovementTarget {
                    speed: SimFixed::from_num(cached_speed),
                    group_id,
                    ..Default::default()
                });
                assert_eq!(
                    advance(&mut mover, rules.object("MTNK"), Some(&rules), None),
                    15
                );
                assert_eq!(
                    mover.movement_target.as_ref().unwrap().current_speed,
                    SimFixed::from_num(225)
                );
            }
        }
    }

    #[test]
    fn production_prefix_passive_special_and_nonaccelerating_gates_match_original() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/track_speed_native.json"
        ))
        .unwrap();
        let fixed = |value: &serde_json::Value| {
            let bits = u64::from_str_radix(value.as_str().unwrap(), 16).unwrap();
            let native = f64::from_bits(bits);
            let fixed = SimFixed::from_num(native);
            assert_eq!(
                fixed.to_num::<f64>(),
                native,
                "gate case must be exactly representable"
            );
            fixed
        };
        let mut compared = [0; 2];
        for case in corpus["prefixes"].as_array().unwrap() {
            let input = &case["input"];
            let accelerates = input["accelerates"].as_bool().unwrap_or(true);
            let passive = input["passive"].as_bool().unwrap_or(false);
            let selector = input["selector"].as_i64().unwrap_or(1) as i32;
            if accelerates && !passive && selector < 64 {
                continue;
            }
            let kind = if input["family"] == "drive" {
                LocomotorKind::Drive
            } else {
                LocomotorKind::Ship
            };
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=100\nSpeed=6\nPassive={}\n",
                if passive { "yes" } else { "no" },
            )))
            .unwrap();
            let mut entity = entity(kind, selector, fixed(&input["target_bits"]));
            entity.drive_accelerates = accelerates;
            entity.foot_speed.applied_fraction = fixed(&input["applied_bits"]);
            advance(&mut entity, rules.object("MTNK"), Some(&rules), None);
            assert_eq!(
                retained(&entity),
                fixed(&case["output"]["target_bits"]),
                "{input}"
            );
            assert_eq!(
                entity.foot_speed.applied_fraction,
                fixed(&case["output"]["applied_bits"]),
                "{input}"
            );
            compared[usize::from(kind == LocomotorKind::Ship)] += 1;
        }
        assert_eq!(compared, [14, 14]);
    }
}
