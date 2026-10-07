//! Bullet AI 4668D9..467032 and HomingTrack 5B20F0.
//! Velocity is the only direction/magnitude authority. Native represented
//! arithmetic uses the existing deterministic scalar service and retail tables.
//! Executed controls: tools/projectile_oracle/guided_step.py.

use super::native_math::*;
use super::{ProjectileCoord as Coord, ProjectileGuidance, ProjectileVelocity as Velocity};
use crate::map::retail_trig::TrigTable;
use crate::util::native_x87::{NativeF64Bits as D, X87Chop53 as X, X87Ordering, X87Value};

fn square(value: X87Value) -> X87Value {
    X::mul(value, value)
}
fn length2(v: Velocity) -> X87Value {
    sqrt(X::add(square(d(v.x)), square(d(v.y))))
}
fn length3(v: Velocity, order: [usize; 3]) -> X87Value {
    let t = v.native().map(|x| square(d(x)));
    sqrt(X::add(X::add(t[order[0]], t[order[1]]), t[order[2]]))
}
fn zero(value: D) -> bool {
    value.bits() & 0x7fff_ffff_ffff_ffff == 0
}
fn advance(p: Coord, v: Velocity, scale: i32) -> Coord {
    let step = v.integer_projection();
    Coord::new(
        p.x.wrapping_add(step.x.wrapping_mul(scale)),
        p.y.wrapping_add(step.y.wrapping_mul(scale)),
        p.z.wrapping_add(step.z.wrapping_mul(scale)),
    )
}
fn delta(a: Coord, b: Coord) -> Coord {
    Coord::new(
        a.x.wrapping_sub(b.x),
        a.y.wrapping_sub(b.y),
        a.z.wrapping_sub(b.z),
    )
}
fn distance(v: Coord, order: [usize; 3]) -> i32 {
    int(length3(Velocity::new(v.x, v.y, v.z), order))
}

/// 4668D9..466B09: a stored lock byte and signed counter, then normalize
/// the complete double vector. Overspeed uses the original type acceleration.
pub(super) fn ramp(mut v: Velocity, g: &mut ProjectileGuidance, frame: i32) -> Velocity {
    let magnitude = round(length3(v, [0, 1, 2]));
    let maximum = X::load_i32(g.max_speed);
    if g.course_lock_duration == 0 {
        if g.max_speed >= 40
            || !less(
                X::add(magnitude, d(D::from_bits(0x3fe0_0000_0000_0000))),
                maximum,
            )
        {
            g.course_locked = false;
        }
    } else if g.course_frames < g.course_lock_duration {
        g.course_frames = g.course_frames.wrapping_add(1);
        if g.course_frames >= g.course_lock_duration {
            g.course_locked = false;
        }
    }
    let acceleration = if g.course_locked && g.course_lock_duration == 0 {
        i32::from(frame % 2 == 0)
    } else {
        g.acceleration
    };
    let next = match X::compare(magnitude, maximum) {
        X87Ordering::Less => {
            let increased = X::add(X::load_i32(acceleration), magnitude);
            round(if less(increased, maximum) {
                increased
            } else {
                maximum
            })
        }
        X87Ordering::Greater => {
            let reduced = X::sub(magnitude, X::load_i32(g.acceleration / 2));
            round(if less(X::load_i32(0), reduced) {
                reduced
            } else {
                X::load_i32(0)
            })
        }
        X87Ordering::Equal => return v,
    };
    if zero(v.x) && zero(v.y) && zero(v.z) {
        v.x = Velocity::new(100, 0, 0).x;
    }
    let scale = X::div(next, length3(v, [2, 1, 0])).expect("seeded native velocity");
    Velocity::from_native(v.native().map(|value| store(X::mul(scale, d(value)))))
}

/// 466BC5..466CF9: signed wrap/remainder, table sine, two separate ftol
/// conversions (the second only inside 256 leptons), then the low byte.
pub(super) fn turn_word(
    g: &ProjectileGuidance,
    identity: i32,
    frame: i32,
    old: Coord,
    target: Coord,
    trig: &TrigTable,
) -> u16 {
    let phase = identity.wrapping_add(frame) % 15;
    let angle = store(X::mul(
        X::mul(X::load_i32(phase), d(D::from_bits(0x3fb1_1111_1111_1111))),
        d(D::from_bits(0x4019_21fb_5444_2d18)),
    ));
    let variation = d(g.missile_rot_var);
    let varied = X::add(
        X::mul(sin(trig, angle), variation),
        X::add(variation, X::load_i32(1)),
    );
    let mut rate = int(X::mul(varied, X::load_i32(g.rot)));
    if distance(delta(old, target), [1, 2, 0]) < 256 {
        rate = int(X::mul(
            X::load_i32(rate),
            d(D::from_bits(0x3ff8_0000_0000_0000)),
        ));
    }
    if g.course_locked {
        0
    } else {
        (rate as u16 & 0xff) << 8
    }
}

/// DirStruct helpers 5B2950/5B2990/5B29C0 use SIGNED 16-bit differences
/// and signed turn magnitudes. This also preserves negative authored ROT.
fn clamp(current: u16, desired: u16, turn: u16) -> u16 {
    let change = desired.wrapping_sub(current) as i16;
    if i32::from(change).abs() <= i32::from(turn as i16).abs() {
        desired
    } else if change < 0 {
        current.wrapping_sub(turn)
    } else {
        current.wrapping_add(turn)
    }
}
fn pitch(v: Velocity) -> u16 {
    angle_word(atan(d(v.z), round(length2(v))))
}
fn apply_pitch(mut v: Velocity, desired: u16, order: [usize; 3], trig: &TrigTable) -> Velocity {
    let current = radians(pitch(v));
    let magnitude = round(length3(v, order));
    if !zero(current) {
        let divisor = cos(trig, current);
        v.x = store(X::div(d(v.x), divisor).expect("finite native pitch cosine"));
        v.y = store(X::div(d(v.y), divisor).expect("finite native pitch cosine"));
    }
    let angle = radians(desired);
    v.x = store(X::mul(cos(trig, angle), d(v.x)));
    v.y = store(X::mul(cos(trig, angle), d(v.y)));
    v.z = store(X::mul(sin(trig, angle), magnitude));
    v
}

pub(super) struct TrackResult {
    pub candidate: Coord,
    pub velocity: Velocity,
    pub reached_distance: i32,
}

/// The callback resolves floor then structural bridge at the exact lookahead
/// coordinate. Native probes six integer velocity steps ahead of candidate.
pub(super) fn track(
    old: Coord,
    mut velocity: Velocity,
    target: Coord,
    turn: u16,
    g: &ProjectileGuidance,
    target_is_aircraft: bool,
    mut floor_with_bridge: impl FnMut(Coord) -> i32,
    trig: &TrigTable,
) -> TrackResult {
    if target == Coord::new(0, 0, 0) {
        let desired = clamp(pitch(velocity), 0x2000, turn);
        velocity = apply_pitch(velocity, desired, [1, 0, 2], trig);
        return TrackResult {
            candidate: advance(old, velocity, 1),
            velocity,
            reached_distance: int(length3(velocity, [2, 0, 1])),
        };
    }
    let mut candidate = advance(old, velocity, 1);
    let mut difference = delta(target, candidate);
    let distance3 = distance(difference, [0, 2, 1]);
    let horizontal = int(sqrt(X::add(
        square(X::load_i32(difference.x)),
        square(X::load_i32(difference.y)),
    )));
    let current_yaw = angle_word(atan(X::neg(d(velocity.y)), d(velocity.x)));
    let desired_yaw = angle_word(atan(
        X::neg(X::load_i32(difference.y)),
        X::load_i32(difference.x),
    ));
    let yaw = radians(clamp(current_yaw, desired_yaw, turn));
    if zero(velocity.x) && zero(velocity.y) {
        velocity.x = Velocity::new(100, 0, 0).x;
    }
    let horizontal_speed = round(length2(velocity));
    velocity.x = store(X::mul(cos(trig, yaw), horizontal_speed));
    velocity.y = store(X::neg(X::mul(sin(trig, yaw), horizontal_speed)));
    let current_pitch = pitch(velocity);
    let mut new_pitch = current_pitch;
    let quantum = (((u32::from(turn) >> 7) + 1) >> 1) & 0xff;
    if !target_is_aircraft
        && (g.airburst || horizontal > if g.very_high { 1536 } else { 768 })
        && quantum > 1
    {
        let floor = floor_with_bridge(advance(candidate, velocity, 6));
        let levels = if g.airburst || g.very_high {
            10
        } else {
            (distance3 / 256).min(5)
        };
        if !g.level {
            let error = candidate
                .z
                .wrapping_sub(levels.wrapping_mul(104))
                .wrapping_sub(floor);
            if error < -20 {
                candidate.z = candidate.z.wrapping_add(18);
            } else if error > 20 {
                candidate.z = candidate.z.wrapping_sub(18);
            }
            let desired = if error < -52 {
                0x2000
            } else if error > 52 {
                0x4800
            } else {
                0x4000
            };
            new_pitch = clamp(current_pitch, desired, ((turn as i16) / 2) as u16);
        }
    } else if !g.level {
        let desired = pitch(Velocity::new(difference.x, difference.y, difference.z));
        new_pitch = clamp(
            current_pitch,
            desired,
            ((turn.wrapping_add(0x100) as i16) / 2) as u16,
        );
    }
    velocity = apply_pitch(velocity, new_pitch, [2, 0, 1], trig);
    // The pre-clearance target delta is retained for HomingTrack's return.
    difference.z = if g.airburst { 0 } else { difference.z / 4 };
    TrackResult {
        candidate,
        velocity,
        reached_distance: distance(difference, [0, 2, 1]),
    }
}

/// 466EB6..467030. Compare the represented intermediate before its qword
/// store; preserve the separate signed warmup counter and lock latch.
pub(super) fn closing(
    g: &mut ProjectileGuidance,
    old: Coord,
    candidate: Coord,
    target: Coord,
) -> bool {
    if g.course_locked {
        return false;
    }
    let delta = distance(delta(old, target), [2, 1, 0])
        .wrapping_sub(distance(delta(candidate, target), [2, 1, 0]));
    let accumulator = d(D::from_bits(g.closing_accumulator_bits));
    if g.closing_frames < 60 {
        g.closing_frames = g.closing_frames.wrapping_add(1);
        g.closing_accumulator_bits = store(X::add(X::load_i32(delta), accumulator)).bits();
        false
    } else {
        let value = X::add(
            X::mul(accumulator, d(D::from_bits(0x3fef_7777_7777_7777))),
            X::load_i32(delta),
        );
        g.closing_accumulator_bits = store(value).bits();
        !less(value, X::load_i32(0)) && less(value, X::load_i32(60)) && !g.airburst && !g.very_high
    }
}

pub(super) struct StepResult {
    pub candidate: Coord,
    pub impact: bool,
    pub reason: super::ProjectileDetonationReason,
}

/// Complete guided arm through 467B7A, before coordinate commit/shared probe.
#[allow(clippy::too_many_arguments)]
pub(super) fn step(
    projectile: &mut super::Projectile,
    target_position: Coord,
    binary_frame: i32,
    target_is_aircraft: bool,
    safety_altitude: i32,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    shared_cell_dummy: &crate::map::resolved_terrain::SharedCellDummy,
) -> StepResult {
    let mut guidance = projectile.guidance.expect("guided branch");
    let previous_position = projectile.position;
    let mut impact_flag = false;
    let mut impact_reason = super::ProjectileDetonationReason::Collision;
    let (trig, _) = crate::map::retail_trig::required_math_tables();
    let ramped = ramp(projectile.velocity, &mut guidance, binary_frame);
    let turn = turn_word(
        &guidance,
        projectile.native_unique_id,
        binary_frame,
        previous_position,
        target_position,
        trig,
    );
    let tracked = track(
        previous_position,
        ramped,
        target_position,
        turn,
        &guidance,
        target_is_aircraft,
        |coord| {
            let floor = super::projectile_ground_z(terrain, shared_cell_dummy, coord);
            floor.wrapping_add(
                if super::structural_bridge_at(terrain, shared_cell_dummy, coord) {
                    crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
                } else {
                    0
                },
            )
        },
        trig,
    );
    let mut candidate = tracked.candidate;
    // 466D44 resolves candidate Cell before copying the steered velocity.
    let candidate_structural = super::structural_bridge_at(terrain, shared_cell_dummy, candidate);
    projectile.velocity = tracked.velocity;
    projectile.speed_leptons_per_frame = super::projectile_velocity_magnitude(projectile.velocity)
        .clamp(0.0, f64::from(u16::MAX)) as u16;
    let old_height = previous_position
        .z
        .wrapping_sub(super::projectile_ground_z(
            terrain,
            shared_cell_dummy,
            previous_position,
        ))
        .wrapping_sub(if projectile.on_bridge {
            crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        });
    let (admit, snap) = super::homing_impact_admission(
        tracked.reached_distance,
        projectile.velocity,
        old_height,
        guidance.airburst,
        target_position != Coord::new(0, 0, 0),
    );
    if admit {
        impact_flag = true;
        impact_reason = if old_height <= 0 {
            super::ProjectileDetonationReason::Collision
        } else {
            super::ProjectileDetonationReason::ReachedTarget
        };
        if snap {
            candidate = target_position;
        }
    }
    // 466E70: a targetless missile above Rules.MissileSafetyAltitude detonates.
    if target_position == Coord::new(0, 0, 0) && old_height >= safety_altitude {
        impact_flag = true;
    }
    if closing(&mut guidance, previous_position, candidate, target_position) {
        impact_flag = true;
        impact_reason = super::ProjectileDetonationReason::ReachedTarget;
    }
    // Guided bridge contact 467032 probes Cell before floor and uses
    // strict endpoint inequalities, unlike Vertical's contact helper.
    if !impact_flag
        && (candidate_structural
            || super::structural_bridge_at(terrain, shared_cell_dummy, previous_position))
    {
        let deck = super::projectile_ground_z(terrain, shared_cell_dummy, candidate)
            .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        if (previous_position.z < deck && candidate.z > deck)
            || (previous_position.z > deck && candidate.z < deck)
        {
            candidate.z = deck;
            impact_flag = true;
        }
    }
    projectile.guidance = Some(guidance);
    StepResult {
        candidate,
        impact: impact_flag,
        reason: impact_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn coord(v: &Value) -> Coord {
        Coord::new(
            v[0].as_i64().unwrap() as i32,
            v[1].as_i64().unwrap() as i32,
            v[2].as_i64().unwrap() as i32,
        )
    }
    fn bits(v: &Value) -> [u64; 3] {
        std::array::from_fn(|i| u64::from_str_radix(v[i].as_str().unwrap(), 16).unwrap())
    }

    #[test]
    fn original_38_cardinal_ramp_visits_keep_fractional_velocity() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/weapon_speed.json",
        ))
        .unwrap();
        let mut guidance = ProjectileGuidance {
            rot: 60,
            missile_rot_var: D::from_bits(0.25f64.to_bits()),
            course_lock_duration: 0,
            course_frames: 0,
            course_locked: true,
            airburst: false,
            inaccurate: false,
            very_high: false,
            level: false,
            max_speed: 102,
            acceleration: 3,
            fuse_reference: Coord::new(0, 0, 0),
            closing_frames: 0,
            closing_accumulator_bits: 0,
        };
        let mut v = Velocity::new(1, 0, 0);
        let rows = corpus["physical_ifv"]["cardinal_acceleration_only"]
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 38);
        for row in rows {
            let frame = row["frame"].as_i64().unwrap() as i32;
            v = ramp(v, &mut guidance, frame);
            assert_eq!(
                v.native().map(D::bits),
                bits(&row["velocity_bits"]),
                "native frame{frame}"
            );
            assert_eq!(
                guidance.course_locked,
                row["course_locked"].as_bool().unwrap()
            );
            assert_eq!(
                i64::from(guidance.course_frames),
                row["course_frames"].as_i64().unwrap()
            );
        }
    }

    #[test]
    #[ignore = "requires verified gamemd.exe math tables"]
    fn original_guided_boundaries_preserve_null_aircraft_and_signed_controls() {
        use super::super::{ProjectileStore, ProjectileTarget};
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_guided_controls.json",
        ))
        .unwrap();
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
        let mapped: Vec<_> = (16..25)
            .flat_map(|y| (6..25).map(move |x| (x, y)))
            .collect();
        terrain.test_set_native_allocated_cells(&mapped);
        for &(x, y) in &mapped {
            terrain.cell_mut(x, y).unwrap().level = 6;
        }
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 31);
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            assert!(row["failure"].is_null(), "native fixture failed: {name}");
            let prepared = &row["prepared"];
            let old = coord(&prepared["old"]);
            let velocity = Velocity::from_native(std::array::from_fn(|i| {
                D::from_bits(prepared["velocity"][i].as_f64().unwrap().to_bits())
            }));
            let target = coord(&row["target_coord"]);
            let mut shot = super::super::tests::spawn(ProjectileTarget::Entity(42));
            shot.native_unique_id = prepared["unique_id"].as_i64().unwrap() as i32;
            shot.origin = old;
            shot.velocity = velocity;
            shot.initial_target_position = target;
            shot.guidance = Some(ProjectileGuidance {
                rot: 60,
                missile_rot_var: D::from_bits(
                    u64::from_str_radix(prepared["missile_rot_var_bits"].as_str().unwrap(), 16)
                        .unwrap()
                        .swap_bytes(),
                ),
                course_lock_duration: prepared["course_lock_duration"].as_i64().unwrap() as i32,
                course_frames: input["counter"].as_i64().unwrap_or(0) as i32,
                course_locked: input["locked"].as_bool().unwrap_or(false),
                airburst: prepared["airburst"].as_bool().unwrap(),
                inaccurate: false,
                very_high: prepared["very_high"].as_bool().unwrap(),
                level: prepared["level_flight"].as_bool().unwrap(),
                max_speed: prepared["maximum_speed"].as_i64().unwrap() as i32,
                acceleration: prepared["acceleration"].as_i64().unwrap() as i32,
                fuse_reference: target,
                closing_frames: input["closing_count"].as_i64().unwrap_or(0) as i32,
                closing_accumulator_bits: input["closing_accum"].as_f64().unwrap_or(0.0).to_bits(),
            });
            let mut store = ProjectileStore::new();
            store.spawn_at(1, 0, shot);
            let bullet = store.get_mut(1).unwrap();
            let result = step(
                bullet,
                target,
                prepared["binary_frame"].as_i64().unwrap() as i32,
                prepared["target_kind"].as_str() == Some("aircraft"),
                prepared["safety_altitude"].as_i64().unwrap() as i32,
                Some(&terrain),
                &terrain.shared_cell_dummy(),
            );
            assert_eq!(
                result.candidate,
                coord(&row["candidate"]),
                "{name} candidate"
            );
            assert_eq!(
                result.impact,
                row["impact"].as_u64().unwrap() != 0,
                "{name} impact"
            );
            assert_eq!(
                bullet.velocity.native().map(D::bits),
                bits(&row["velocity"]["bits"]),
                "{name} velocity"
            );
            let g = bullet.guidance.unwrap();
            assert_eq!(
                g.course_locked,
                row["locked"].as_bool().unwrap(),
                "{name} lock"
            );
            assert_eq!(
                i64::from(g.course_frames),
                row["counter"].as_i64().unwrap(),
                "{name} course counter"
            );
            assert_eq!(
                i64::from(g.closing_frames),
                row["closing_count"].as_i64().unwrap(),
                "{name} closing counter"
            );
            // This fixture saves the original little-endian qword bytes.
            let native_closing = u64::from_str_radix(row["closing_bits"].as_str().unwrap(), 16)
                .unwrap()
                .swap_bytes();
            assert_eq!(
                g.closing_accumulator_bits, native_closing,
                "{name} closing bits"
            );
        }
    }

    #[test]
    #[ignore = "requires retail INIs and verified gamemd.exe math tables"]
    fn original_ifv_launch_and_125_flight_visits_match_bits_and_live_bridge_transitions() {
        use super::super::{ProjectileStore, ProjectileTarget, ProjectileTrajectory, launch};
        let Some((ini, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let rules =
            crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let weapon = rules.weapon("HoverMissile").unwrap();
        let kind = rules
            .projectile(weapon.projectile.as_deref().unwrap())
            .unwrap();
        assert_eq!(weapon.speed, 102);
        assert_eq!(kind.rot, 60);
        let (trig, _) = crate::map::retail_trig::required_math_tables();
        assert!(trig.matches_retail());
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ifv_launch.json",
        ))
        .unwrap();
        let mut compared = 0;
        for (case_index, case) in corpus["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["supplied"];
            let native = &case["launch"];
            let origin = coord(&native["position"]);
            let target = coord(&native["target"]);
            let launched = launch::fireat_launch(launch::FireAtLaunch {
                delta: delta(target, origin),
                speed: weapon.speed,
                homing: true,
                vertical: false,
                heading: Some(input["source_heading"].as_u64().unwrap() as u16),
                arcing: kind.arcing,
                gravity: super::super::projectile_gravity(rules.general.gravity, kind.floater),
                high_root: false,
                voxel_downward: None,
                building_pitch_height: None,
            })
            .unwrap();
            assert_eq!(
                launched.velocity.native().map(D::bits),
                bits(&native["velocity"]["bits"]),
                "case{case_index} launch"
            );
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            let mapped: Vec<_> = (16..25)
                .flat_map(|y| (6..25).map(move |x| (x, y)))
                .collect();
            terrain.test_set_native_allocated_cells(&mapped);
            for &(x, y) in &mapped {
                terrain.cell_mut(x, y).unwrap().level = 6;
            }
            let bridges: Vec<(u16, u16)> = if let Some(band) = input["bridge_band"].as_array() {
                band.iter()
                    .map(|v| (v[0].as_u64().unwrap() as u16, v[1].as_u64().unwrap() as u16))
                    .collect()
            } else if let Some(v) = input["live_bridge_cell"].as_array() {
                vec![(v[0].as_u64().unwrap() as u16, v[1].as_u64().unwrap() as u16)]
            } else {
                Vec::new()
            };
            for &(x, y) in &bridges {
                terrain.cell_mut(x, y).unwrap().bridge_facts.raw_flags = 0x100;
            }
            let mut shot = super::super::tests::spawn(ProjectileTarget::Cell { rx: 16, ry: 20 });
            shot.native_unique_id = native["unique_id"].as_i64().unwrap() as i32;
            shot.origin = origin;
            shot.initial_target_position = target;
            shot.velocity = launched.velocity;
            shot.speed_leptons_per_frame = launched.speed as u16;
            shot.trajectory = ProjectileTrajectory::Straight;
            shot.tracks_target = true;
            shot.ranged_fuse = true;
            shot.arm_frames = kind.arm;
            shot.guidance = Some(ProjectileGuidance {
                rot: kind.rot,
                missile_rot_var: D::from_bits(rules.general.missile_rot_var.to_bits()),
                course_lock_duration: kind.course_lock_duration,
                course_frames: 0,
                course_locked: true,
                airburst: kind.airburst,
                inaccurate: kind.inaccurate,
                very_high: kind.very_high,
                level: kind.level,
                max_speed: weapon.speed,
                acceleration: kind.acceleration,
                fuse_reference: target,
                closing_frames: 0,
                closing_accumulator_bits: 0,
            });
            let mut store = ProjectileStore::new();
            store.spawn_at(1, 0, shot);
            for row in case["frames"].as_array().unwrap() {
                let frame = row["frame"].as_u64().unwrap() as u32;
                for change in input["between_frame_flag_changes"].as_array().unwrap() {
                    if change["frame"].as_u64().unwrap() as u32 == frame {
                        for &(x, y) in &bridges {
                            terrain.cell_mut(x, y).unwrap().bridge_facts.raw_flags =
                                if change["live"].as_bool().unwrap() {
                                    0x100
                                } else {
                                    0
                                };
                        }
                    }
                }
                // The native fixture's supplied flat cells contain no receivers;
                // common collision receiver fidelity has its own executed suite.
                let result = store
                    .advance_one(
                        1,
                        frame,
                        |_| None,
                        Some(&terrain),
                        &terrain.shared_cell_dummy(),
                        rules.general.gravity,
                        false,
                        false,
                        rules.general.safety_altitude,
                        |_, _, _| None,
                    )
                    .unwrap();
                let bullet = store.get(1).unwrap();
                assert_eq!(
                    bullet.position,
                    coord(&row["position"]),
                    "case{case_index} frame{frame} position"
                );
                assert_eq!(
                    bullet.velocity.native().map(D::bits),
                    bits(&row["velocity"]["bits"]),
                    "case{case_index} frame{frame} velocity"
                );
                let g = bullet.guidance.unwrap();
                assert_eq!(
                    g.closing_accumulator_bits,
                    row["closing_accum"].as_f64().unwrap().to_bits(),
                    "case{case_index} frame{frame} closing"
                );
                assert_eq!(
                    i64::from(g.closing_frames),
                    row["closing_count"].as_i64().unwrap()
                );
                assert_eq!(g.course_locked, row["course_locked"].as_bool().unwrap());
                let native_impact = row["stopped_at"].as_str() == Some("0x489280");
                assert_eq!(
                    !result.detonations.is_empty(),
                    native_impact,
                    "case{case_index} frame{frame} detonation"
                );
                if native_impact {
                    assert_eq!(
                        result.detonations[0].impact,
                        coord(&case["area_damage_handoff"]["position"])
                    );
                }
                compared += 1;
            }
        }
        assert_eq!(compared, 125);
    }

    #[test]
    #[ignore = "requires RA2_DIR with verified gamemd.exe math tables"]
    fn original_guided_steps_preserve_velocity_phase_and_bridge_clearance() {
        let (trig, _) = crate::map::retail_trig::required_math_tables();
        assert!(trig.matches_retail());
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/guided_step.json",
        ))
        .unwrap();
        for row in corpus["rows"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            let input = &row["supplied"];
            let old = coord(&input["old"]);
            let v = Velocity::from_native(std::array::from_fn(|i| {
                D::from_bits(input["velocity"][i].as_f64().unwrap().to_bits())
            }));
            let bridge = |x: i32, y: i32| {
                input["bridge_cells"].as_array().unwrap().iter().any(|c| {
                    c[0].as_i64() == Some(i64::from(x / 256))
                        && c[1].as_i64() == Some(i64::from(y / 256))
                })
            };
            let x = input["target_cell"][0].as_i64().unwrap() as i32 * 256 + 128;
            let y = input["target_cell"][1].as_i64().unwrap() as i32 * 256 + 128;
            let target = Coord::new(x, y, 624 + if bridge(x, y) { 416 } else { 0 });
            let mut g = ProjectileGuidance {
                rot: 60,
                missile_rot_var: D::from_bits(0.25f64.to_bits()),
                course_lock_duration: 0,
                course_frames: 0,
                course_locked: true,
                airburst: false,
                inaccurate: false,
                very_high: false,
                level: false,
                max_speed: 102,
                acceleration: 3,
                fuse_reference: target,
                closing_frames: 0,
                closing_accumulator_bits: 0,
            };
            let frame = input["binary_frame"].as_i64().unwrap() as i32;
            let id = input["unique_id"].as_i64().unwrap() as i32;
            let initial_guidance = g;
            let ramped = ramp(v, &mut g, frame);
            assert_eq!(
                ramped.native().map(D::bits),
                bits(&row["snapshots"][0]["velocity"]["bits"]),
                "{name} ramp"
            );
            let turn = turn_word(&g, id, frame, old, target, trig);
            assert_eq!(
                u64::from(turn),
                row["turns"][0]["word"].as_u64().unwrap(),
                "{name} turn"
            );
            let tracked = track(
                old,
                ramped,
                target,
                turn,
                &g,
                false,
                |p| 624 + if bridge(p.x, p.y) { 416 } else { 0 },
                trig,
            );
            assert_eq!(
                tracked.candidate,
                coord(&row["snapshots"][1]["candidate"]),
                "{name} candidate before bridge contact"
            );
            assert_eq!(
                tracked.velocity.native().map(D::bits),
                bits(&row["velocity"]["bits"]),
                "{name} steered velocity"
            );
            closing(&mut g, old, tracked.candidate, target);
            assert_eq!(
                g.closing_accumulator_bits,
                row["closing_accum"].as_f64().unwrap().to_bits(),
                "{name} closing"
            );
            assert_eq!(
                g.course_locked,
                row["course_locked"].as_bool().unwrap(),
                "{name} lock"
            );
            assert_eq!(
                i64::from(g.course_frames),
                row["course_frames"].as_i64().unwrap(),
                "{name} lock counter"
            );

            // Run the complete production guided branch against retained cells,
            // including old-only/new-only live flags and strict deck equality.
            let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
            for y in 16..25 {
                for x in 6..25 {
                    let cell = terrain.cell_mut(x, y).unwrap();
                    cell.level = 6;
                    cell.bridge_facts.raw_flags = if bridge(i32::from(x) * 256, i32::from(y) * 256)
                    {
                        0x100
                    } else {
                        0
                    };
                }
            }
            let mut shot = super::super::tests::spawn(super::super::ProjectileTarget::Cell {
                rx: input["target_cell"][0].as_u64().unwrap() as u16,
                ry: input["target_cell"][1].as_u64().unwrap() as u16,
            });
            shot.native_unique_id = id;
            shot.origin = old;
            shot.velocity = v;
            shot.guidance = Some(initial_guidance);
            let mut store = super::super::ProjectileStore::new();
            store.spawn(1, shot);
            let bullet = store.projectiles.get_mut(&1).unwrap();
            let result = step(
                bullet,
                target,
                frame,
                false,
                750,
                Some(&terrain),
                &terrain.shared_cell_dummy(),
            );
            assert_eq!(
                result.candidate,
                coord(&row["candidate"]),
                "{name} complete candidate"
            );
            assert_eq!(
                result.impact,
                row["impact"].as_u64().unwrap() != 0,
                "{name} impact bypasses Arm"
            );
            assert_eq!(
                bullet.velocity.native().map(D::bits),
                bits(&row["velocity"]["bits"]),
                "{name} complete velocity"
            );
            assert_eq!(
                bullet.guidance.unwrap().closing_accumulator_bits,
                row["closing_accum"].as_f64().unwrap().to_bits(),
                "{name} complete closing"
            );
        }
    }
}
