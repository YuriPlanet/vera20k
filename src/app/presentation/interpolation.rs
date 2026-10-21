//! Experimental, presentation-only previous/current transform sampling.
//! This intentionally adds one logical-frame of visual latency. It neither
//! predicts nor writes simulation, commands, RNG, hashes or snapshot fields.

#[cfg(test)]
use std::collections::HashMap;

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::object_location;
use crate::sim::world::Simulation;

#[cfg(test)]
#[path = "interpolation/aircraft_reuse_tests.rs"]
mod aircraft_reuse_tests;
mod history;
#[cfg(test)]
#[path = "interpolation/perf_tests.rs"]
mod perf_tests;
mod scene;
#[cfg(test)]
#[path = "interpolation/shp_reuse_tests.rs"]
mod shp_reuse_tests;
#[cfg(test)]
#[path = "interpolation/zero_projection_tests.rs"]
mod zero_projection_tests;
pub(crate) use scene::{PointKind, PointPose, PointSample};

const TELEPORT_LEPTONS: i64 = 384; // Prototype guard: 1.5 cells, like ra2-mono.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPose {
    pub screen: [f32; 2],
    pub body: u16,
    pub turret: u16,
    pub barrel: u16,
    pub angles: [f32; 2],
    pub rocket_pitch: f32,
    pub visual_lift: f32,
    pub lift: f32,
    pub recoil: ([f32; 2], bool),
    pub slope: Option<crate::sim::movement::slope_transition::SlopeRenderPhase>,
    location: [i32; 3],
    type_id: u32,
    on_bridge: bool,
}

/// Fly DrawMatrix4CF632/4CF659 reads SecondaryFacing for both its cache key
/// and rotation, like its separate ShadowMatrix. PrimaryFacing may already
/// point elsewhere while the plane turns. Other locomotors keep their owner.
pub(crate) fn body_draw_facing(entity: &GameEntity, binary_frame: u32) -> u16 {
    if entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.active_kind() == crate::rules::locomotor_type::LocomotorKind::Fly)
    {
        entity.barrel_facing.as_ref().map_or_else(
            || entity.body_facing_current(binary_frame),
            |facing| facing.current(binary_frame),
        )
    } else {
        entity.body_facing_current(binary_frame)
    }
}

impl RenderPose {
    fn read(entity: &GameEntity, frame: u32) -> Self {
        let location = object_location(entity, None);
        let ((x, y), lift) = crate::render::locomotor_visual::screen_position_and_lift(entity);
        let body = body_draw_facing(entity, frame);
        Self {
            screen: [x, y],
            body,
            barrel: entity.barrel_elevation().current(frame),
            angles: entity
                .rocking
                .as_ref()
                .map_or([0.; 2], |r| r.angles_native()),
            rocket_pitch: entity
                .locomotor
                .as_ref()
                .and_then(|l| l.rocket_runtime())
                .map_or(0., |r| r.draw_pitch()),
            visual_lift: f32::from(entity.ion_blast_lift_pixels()),
            lift: lift as f32,
            recoil: entity.voxel_recoil(),
            slope: crate::sim::movement::slope_transition::state_for_entity(entity)
                .map(|s| s.render_phase(frame.wrapping_sub(1))),
            turret: entity
                .barrel_facing
                .as_ref()
                .map_or(body, |f| f.current(frame)),
            location: [location.x, location.y, location.z],
            type_id: entity.type_ref().index(),
            on_bridge: entity.on_bridge(),
        }
    }

    fn continuous_with(self, other: Self) -> bool {
        self.type_id == other.type_id
            && self.on_bridge == other.on_bridge
            && self
                .location
                .iter()
                .zip(other.location)
                .all(|(&a, b)| (i64::from(a) - i64::from(b)).abs() <= TELEPORT_LEPTONS)
    }

    fn between(self, current: Self, alpha: f32) -> Self {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Self {
            screen: [
                self.screen[0] + (current.screen[0] - self.screen[0]) * alpha,
                self.screen[1] + (current.screen[1] - self.screen[1]) * alpha,
            ],
            body: facing_between(self.body, current.body, alpha),
            turret: facing_between(self.turret, current.turret, alpha),
            barrel: facing_between(self.barrel, current.barrel, alpha),
            angles: std::array::from_fn(|i| {
                angle_between(self.angles[i], current.angles[i], alpha)
            }),
            rocket_pitch: angle_between(self.rocket_pitch, current.rocket_pitch, alpha),
            visual_lift: self.visual_lift + (current.visual_lift - self.visual_lift) * alpha,
            lift: self.lift + (current.lift - self.lift) * alpha,
            recoil: (
                std::array::from_fn(|i| {
                    self.recoil.0[i] + (current.recoil.0[i] - self.recoil.0[i]) * alpha
                }),
                if alpha >= 1. {
                    current.recoil.1
                } else {
                    self.recoil.1 || current.recoil.1
                },
            ),
            slope: slope_between(self.slope, current.slope, alpha),
            ..current
        }
    }
}

fn facing_between(previous: u16, current: u16, alpha: f32) -> u16 {
    let delta = current.wrapping_sub(previous) as i16;
    (i32::from(previous) + (f32::from(delta) * alpha).round() as i32) as u16
}

fn angle_between(previous: f32, current: f32, alpha: f32) -> f32 {
    if alpha >= 1. || !previous.is_finite() || !current.is_finite() {
        return current;
    }
    let delta = (current - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    previous + delta * alpha
}

fn slope_between(
    previous: Option<crate::sim::movement::slope_transition::SlopeRenderPhase>,
    current: Option<crate::sim::movement::slope_transition::SlopeRenderPhase>,
    alpha: f32,
) -> Option<crate::sim::movement::slope_transition::SlopeRenderPhase> {
    use crate::sim::movement::slope_transition::SlopeRenderPhase::{self, Stable, Transition};
    if alpha >= 1. || previous == current {
        return current;
    }
    let (previous, current) = (previous?, current?);
    let (from, to, start, end) = match (previous, current) {
        (Stable(a), Stable(b)) => (a, b, 0., 1.),
        (
            Stable(a),
            Transition {
                from_slope,
                to_slope,
                phase_num,
                phase_den,
            },
        ) if a == from_slope => (
            a,
            to_slope,
            0.,
            phase_num as f32 / f32::from(phase_den.max(1)),
        ),
        (
            Transition {
                from_slope,
                to_slope,
                phase_num,
                phase_den,
            },
            Stable(b),
        ) if b == to_slope => (
            from_slope,
            b,
            phase_num as f32 / f32::from(phase_den.max(1)),
            1.,
        ),
        (
            Transition {
                from_slope: a,
                to_slope: b,
                phase_num: n,
                phase_den: d,
            },
            Transition {
                from_slope: c,
                to_slope: e,
                phase_num: m,
                phase_den: f,
            },
        ) if (a, b) == (c, e) => (
            a,
            b,
            n as f32 / f32::from(d.max(1)),
            m as f32 / f32::from(f.max(1)),
        ),
        (_, current) => return Some(current),
    };
    Some(SlopeRenderPhase::Transition {
        from_slope: from,
        to_slope: to,
        phase_num: ((start + (end - start) * alpha) * 255.).round() as i32,
        phase_den: 255,
    })
}

fn eligible(entity: &GameEntity) -> bool {
    matches!(
        entity.category(),
        EntityCategory::Unit
            | EntityCategory::Infantry
            | EntityCategory::Aircraft
            | EntityCategory::Structure
    ) && !entity.in_limbo()
        && !entity.is_warped_out()
        && !entity.is_warping_in()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Endpoints {
    previous: RenderPose,
    current: RenderPose,
    frame: u32,
}

impl Endpoints {
    fn next(old: Option<Self>, current: RenderPose, frame: u32, reset: bool) -> Self {
        let previous = old
            .filter(|old| {
                !reset && old.frame.wrapping_add(1) == frame && old.current.continuous_with(current)
            })
            .map_or(current, |old| old.current);
        Self {
            previous,
            current,
            frame,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SimulationReadout {
    pub tps: f32,
    pub interpolation: bool,
    pub sampling: bool,
}

pub(crate) struct RenderInterpolation {
    enabled: bool,
    poses: history::PoseHistory,
    scene: scene::SceneInterpolation,
    last_frame: Option<u32>,
    last_commit_ms: u32,
    interval_ms: Option<f32>,
    alpha: f32,
    held: bool,
    rate_start_ms: u32,
    rate_frames: u32,
    tps: f32,
}

impl RenderInterpolation {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            poses: Default::default(),
            scene: Default::default(),
            last_frame: None,
            last_commit_ms: 0,
            interval_ms: None,
            alpha: 1.0,
            held: true,
            rate_start_ms: 0,
            rate_frames: 0,
            tps: 0.0,
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new(self.enabled);
    }

    pub(crate) fn toggle(&mut self) -> bool {
        self.set_enabled(!self.enabled);
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        // Toggling on never replays a stale interval from before the toggle.
        self.poses.clear();
        self.scene.clear();
        self.held = true;
    }

    pub(crate) fn observe(
        &mut self,
        sim: &Simulation,
        now_ms: u32,
        interval: Option<f32>,
        held: bool,
    ) {
        let frame = sim.session().binary_frame();
        let changed = self.last_frame != Some(frame);
        let consecutive = self
            .last_frame
            .is_some_and(|last| last.wrapping_add(1) == frame);
        let reset = self.held || held || self.interval_ms != interval || !consecutive;
        if self.held || held || self.interval_ms != interval {
            self.poses.clear();
            self.scene.clear();
        }
        if changed {
            let delta = self.last_frame.map_or(0, |last| frame.wrapping_sub(last));
            if self.last_frame.is_none() || delta > 300 {
                self.rate_start_ms = now_ms;
                self.rate_frames = 0;
                self.tps = 0.0;
            } else {
                self.rate_frames = self.rate_frames.saturating_add(delta);
            }
            if self.enabled && !held && interval.is_some() {
                // EntityStore::values is in stable-ID order. History updates
                // follow that order without a per-entity hash lookup/write.
                self.poses.capture(
                    sim.entities()
                        .values()
                        .filter(|entity| eligible(entity))
                        .map(|entity| (entity.stable_id(), RenderPose::read(entity, frame))),
                    frame,
                    reset,
                );
                self.scene.observe(sim, reset);
            } else {
                self.poses.clear();
                self.scene.clear();
            }
            self.last_commit_ms = now_ms;
            self.last_frame = Some(frame);
        }
        let rate_elapsed = now_ms.wrapping_sub(self.rate_start_ms);
        if held {
            self.rate_frames = 0;
            self.rate_start_ms = now_ms;
            self.tps = 0.0;
        } else if rate_elapsed >= 500 {
            self.tps = self.rate_frames as f32 * 1000.0 / rate_elapsed as f32;
            self.rate_frames = 0;
            self.rate_start_ms = now_ms;
        }
        self.alpha = interpolation_alpha(
            now_ms.wrapping_sub(self.last_commit_ms),
            interval,
            held || !self.enabled,
        );
        self.interval_ms = interval;
        self.held = held;
    }

    #[cfg(test)]
    fn record(&mut self, id: u64, current: RenderPose, frame: u32, reset: bool) {
        let next = Endpoints::next(self.poses.get(&id).copied(), current, frame, reset);
        self.poses.insert_for_test(id, next);
    }

    fn sample(&self, id: u64, current: RenderPose, frame: u32) -> RenderPose {
        if !self.enabled || self.held {
            return current;
        }
        self.poses
            .get(&id)
            // An out-of-band mutation must snap the WHOLE transform, not mix
            // a stale position with current facing (same guard as ra2-mono).
            .filter(|endpoints| endpoints.frame == frame && endpoints.current == current)
            .map_or(current, |endpoints| {
                endpoints.previous.between(current, self.alpha)
            })
    }

    pub(crate) fn readout(&self) -> SimulationReadout {
        SimulationReadout {
            tps: self.tps,
            interpolation: self.enabled,
            sampling: self.enabled && !self.held && self.interval_ms.is_some(),
        }
    }

    pub(crate) fn sample_entity(&self, entity: &GameEntity, frame: u32) -> RenderPose {
        let current = RenderPose::read(entity, frame);
        if !eligible(entity) {
            return current;
        }
        self.sample(entity.stable_id(), current, frame)
    }

    /// One fresh read for a consumer needing both a draw pose and an adjusted
    /// GetCoords point. Only share inside that read-only draw, never by frame.
    /// Keep coordinate interpolation in f64 before rounding: the pose's screen
    /// displacement (or its integer location) cannot reconstruct this result.
    pub(crate) fn sample_entity_and_coord(
        &self,
        entity: &GameEntity,
        frame: u32,
        coord: [i32; 3],
    ) -> (RenderPose, [i32; 3]) {
        let current = RenderPose::read(entity, frame);
        if !self.enabled || self.held {
            return (current, coord);
        }
        let endpoints = self
            .poses
            .get(&entity.stable_id())
            .filter(|e| e.frame == frame && e.current == current);
        let sampled = if eligible(entity) {
            endpoints.map_or(current, |e| e.previous.between(current, self.alpha))
        } else {
            current
        };
        // entity_coord has its own sampling gate, independent of eligibility.
        // Preserve it exactly, including malformed/stale history fallbacks.
        let coord = endpoints
            .filter(|_| self.interval_ms.is_some())
            .map_or(coord, |e| {
                std::array::from_fn(|i| {
                    (coord[i] as f64
                        + (f64::from(e.previous.location[i]) - f64::from(e.current.location[i]))
                            * f64::from(1. - self.alpha))
                    .round() as i32
                })
            });
        (sampled, coord)
    }

    pub(crate) fn sample_point(
        &self,
        kind: PointKind,
        id: u64,
        current: PointPose,
        frame: u32,
    ) -> PointSample {
        self.scene.sample(
            kind,
            id,
            current,
            frame,
            if self.readout().sampling {
                self.alpha
            } else {
                1.
            },
        )
    }

    pub(crate) fn sample_draw_point(
        &self,
        kind: PointKind,
        id: u64,
        current: PointPose,
        frame: u32,
    ) -> PointSample {
        if !self.readout().sampling {
            return current.current();
        }
        self.scene
            .sample_extra(kind, id, current, frame, self.alpha)
    }

    /// Apply the sampled *displacement*, retaining a caller's own GetCoords/FLH
    /// offset. This is shared by pickers, command lines and owner-following FX.
    pub(crate) fn entity_offset(&self, entity: &GameEntity, frame: u32) -> [f32; 3] {
        if !self.enabled || self.held || self.poses.is_empty() {
            return [0.; 3];
        }
        let current = RenderPose::read(entity, frame);
        let sampled = if eligible(entity) {
            self.sample(entity.stable_id(), current, frame)
        } else {
            current
        };
        [
            sampled.screen[0] - current.screen[0],
            sampled.screen[1] - current.screen[1],
            sampled.lift - current.lift,
        ]
    }

    pub(crate) fn entity_coord(
        &self,
        entity: &GameEntity,
        frame: u32,
        coord: [i32; 3],
    ) -> [i32; 3] {
        if !self.readout().sampling {
            return coord;
        }
        let current = RenderPose::read(entity, frame);
        let Some(e) = self
            .poses
            .get(&entity.stable_id())
            .filter(|e| e.frame == frame && e.current == current)
        else {
            return coord;
        };
        std::array::from_fn(|i| {
            (coord[i] as f64
                + (f64::from(e.previous.location[i]) - f64::from(e.current.location[i]))
                    * f64::from(1. - self.alpha))
            .round() as i32
        })
    }
}

/// The saved preference owns the default; an explicit diagnostic override
/// selects the effective startup choice shown by the ordinary Options page.
pub(crate) fn startup_enabled(saved: bool, environment: Option<&str>) -> bool {
    match environment {
        Some("0") => false,
        Some("1") => true,
        _ => saved,
    }
}

fn interpolation_alpha(elapsed_ms: u32, interval_ms: Option<f32>, held: bool) -> f32 {
    if held {
        return 1.0;
    }
    interval_ms
        .filter(|interval| interval.is_finite() && *interval > 0.0)
        .map_or(1.0, |interval| {
            (elapsed_ms as f32 / interval).clamp(0.0, 1.0)
        })
}

pub(crate) fn pose(state: &AppState, entity: &GameEntity, frame: u32) -> RenderPose {
    state
        .match_state
        .match_presentation
        .interpolation
        .sample_entity(entity, frame)
}

pub(crate) fn screen_position(state: &AppState, entity: &GameEntity) -> (f32, f32) {
    let frame = state
        .match_state
        .sim_runtime
        .as_ref()
        .map_or(0, |rt| rt.simulation.session().binary_frame());
    let [x, y] = pose(state, entity, frame).screen;
    (x, y)
}

/// Once per presented frame; snapshots are rebuilt only when Logic advances.
/// Hidden-window skipped intervals and map loads snap instead of long lerps.
pub(crate) fn prepare(state: &mut AppState, exact_capture: bool) {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame {
        state.renderer.fps_overlay.set_simulation_readout(None);
        return;
    }
    let held = exact_capture
        || state
            .match_state
            .presentation_paused(&state.frontend.screen)
        || state.platform.focus_freeze_active();
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32;
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let speed = runtime
        .simulation
        .session()
        .game_options()
        .game_speed
        .clamp(0, 6) as u8;
    let interval = state.platform.frame_pacer.budget_ms(speed);
    let phase = state
        .platform
        .frame_pacer
        .interpolation_elapsed_ms(u64::from(now_ms));
    let interpolation = &mut state.match_state.match_presentation.interpolation;
    interpolation.observe(&runtime.simulation, now_ms, interval, held);
    if let Some(elapsed) = phase {
        interpolation.alpha =
            interpolation_alpha(elapsed, interval, held || !interpolation.enabled);
    }
    state
        .renderer
        .fps_overlay
        .set_simulation_readout(Some(interpolation.readout()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, facing: u16) -> RenderPose {
        RenderPose {
            screen: [x as f32, 10.0],
            location: [x, 0, 0],
            body: facing,
            turret: facing,
            barrel: 0,
            angles: [0.; 2],
            rocket_pitch: 0.,
            visual_lift: 0.,
            lift: 0.,
            recoil: ([0.; 2], false),
            slope: None,
            type_id: 1,
            on_bridge: false,
        }
    }

    fn pair() -> RenderInterpolation {
        let mut cache = RenderInterpolation::new(true);
        cache.record(7, at(0, 0), 1, true);
        cache.record(7, at(32, 0x2000), 2, false);
        cache.held = false;
        cache
    }

    #[test]
    fn render_interpolation_saved_choice_and_explicit_startup_override_are_distinct() {
        for saved in [false, true] {
            assert_eq!(startup_enabled(saved, None), saved);
            assert_eq!(startup_enabled(saved, Some("unknown")), saved);
            assert!(!startup_enabled(saved, Some("0")));
            assert!(startup_enabled(saved, Some("1")));
        }
    }

    #[test]
    fn render_interpolation_explicit_switch_is_idempotent_and_clears_disabled_history() {
        let mut cache = pair();
        cache.set_enabled(true);
        assert!(
            cache.poses.contains_key(&7),
            "same preference must not discard history"
        );
        cache.set_enabled(false);
        assert!(cache.poses.is_empty());
        assert!(!cache.readout().interpolation);
        cache.set_enabled(true);
        assert!(cache.readout().interpolation);
        assert!(
            !cache.readout().sampling,
            "reenabling waits for fresh history"
        );
    }

    fn live_world() -> Simulation {
        let mut sim = Simulation::new();
        sim.entities_mut_for_test()
            .insert(GameEntity::test_default(7, "LTNK", "Americans", 4, 4));
        *sim.interner_mut_for_test() = crate::sim::intern::test_interner();
        assert!(matches!(
            sim.reveal(7),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        assert!(eligible(sim.entities().get(7).unwrap()));
        sim
    }

    #[test]
    fn render_interpolation_creates_distinct_presented_positions_between_ticks() {
        let mut cache = pair();
        for (elapsed, expected) in [
            (0, 0.0),
            (8, 8.0),
            (16, 16.0),
            (24, 24.0),
            (32, 32.0),
            (64, 32.0),
        ] {
            cache.alpha = interpolation_alpha(elapsed, Some(32.0), false);
            let painted = cache.sample(7, at(32, 0x2000), 2);
            assert_eq!(painted.screen[0], expected);
            assert_eq!(painted.body, (expected * 256.0) as u16);
        }
    }

    #[test]
    fn render_interpolation_facing_wrap_uses_shortest_arc_for_hull_and_turret() {
        let painted = at(0, 0xF000).between(at(0, 0x1000), 0.5);
        assert_eq!((painted.body, painted.turret), (0, 0));
        assert_eq!(facing_between(0x1000, 0xF000, 0.5), 0);
    }

    #[test]
    fn render_interpolation_snaps_birth_teleport_conversion_bridge_and_missed_frames() {
        for (new_pose, frame) in [
            (at(1000, 3), 3),
            (
                RenderPose {
                    type_id: 2,
                    ..at(40, 3)
                },
                3,
            ),
            (
                RenderPose {
                    on_bridge: true,
                    ..at(40, 3)
                },
                3,
            ),
            (at(40, 3), 5),
        ] {
            let mut cache = pair();
            cache.record(7, new_pose, frame, false);
            cache.alpha = 0.0;
            assert_eq!(cache.sample(7, new_pose, frame), new_pose);
        }
        let mut cache = pair();
        cache.record(8, at(2, 3), 2, false);
        cache.alpha = 0.0;
        assert_eq!(cache.sample(8, at(2, 3), 2), at(2, 3));
    }

    #[test]
    fn render_interpolation_never_samples_stale_authority_and_toggle_clears_history() {
        let mut cache = pair();
        cache.alpha = 0.5;
        assert_eq!(cache.sample(7, at(33, 0x2000), 2), at(33, 0x2000));
        assert_eq!(cache.sample(7, at(32, 0x2000), 9), at(32, 0x2000));
        assert!(!cache.toggle());
        assert_eq!(cache.sample(7, at(32, 0x2000), 2), at(32, 0x2000));
        assert!(cache.toggle());
        assert!(cache.poses.is_empty());
        cache.clear();
        assert!(cache.last_frame.is_none());
    }

    #[test]
    fn render_interpolation_pause_unlimited_and_bad_intervals_never_extrapolate() {
        for interval in [None, Some(0.0), Some(f32::NAN), Some(f32::INFINITY)] {
            assert_eq!(interpolation_alpha(8, interval, false), 1.0);
        }
        assert_eq!(interpolation_alpha(0, Some(32.0), true), 1.0);
        assert_eq!(interpolation_alpha(1000, Some(32.0), false), 1.0);
        let mut cache = pair();
        cache.held = true;
        cache.alpha = 0.0;
        assert_eq!(cache.sample(7, at(32, 0x2000), 2), at(32, 0x2000));
    }

    #[test]
    fn render_interpolation_observation_is_read_only_and_rate_counts_logic_not_renders() {
        let mut sim = live_world();
        let before = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "prototype", 0);
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 100, Some(32.0), false);
        for now in 101..500 {
            cache.observe(&sim, now, Some(32.0), false);
        }
        assert_eq!(
            before,
            crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "prototype", 0)
        );
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        cache.observe(&sim, 600, Some(32.0), false);
        assert_eq!(cache.readout().tps, 2.0); // One Logic frame / 500 ms.
        cache.observe(&sim, 601, Some(32.0), true);
        assert_eq!(cache.readout().tps, 0.0);
    }

    #[test]
    fn render_interpolation_real_entity_sampling_and_pause_resume_do_not_replay_old_motion() {
        let mut sim = live_world();
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.0), false);
        let old = RenderPose::read(sim.entities().get(7).unwrap(), sim.session().binary_frame());
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        sim.entities_mut_for_test()
            .get_mut(7)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: 4 * 256 + 160,
                y: 4 * 256 + 128,
                z: 0,
            });
        cache.observe(&sim, 32, Some(32.0), false);
        cache.observe(&sim, 48, Some(32.0), false);
        let current =
            RenderPose::read(sim.entities().get(7).unwrap(), sim.session().binary_frame());
        let painted = cache.sample(7, current, sim.session().binary_frame());
        assert_eq!(painted.screen, old.between(current, 0.5).screen);
        assert_ne!(painted.screen, current.screen);
        cache.observe(&sim, 49, Some(32.0), true);
        cache.observe(&sim, 50, Some(32.0), false);
        assert_eq!(
            cache.sample(7, current, sim.session().binary_frame()),
            current
        );
        // A same-frame out-of-band relocation cannot reuse old endpoints.
        let mut relocated = current;
        relocated.location[0] += 1;
        assert_eq!(
            cache.sample(7, relocated, sim.session().binary_frame()),
            relocated
        );
    }

    #[test]
    fn render_interpolation_limbo_objects_and_retired_ids_leave_no_cached_ghost() {
        let unplaced = GameEntity::test_default(8, "LTNK", "Americans", 4, 4);
        assert!(!eligible(&unplaced));
        let mut sim = live_world();
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.0), false);
        assert!(cache.poses.contains_key(&7));
        sim.entities_mut_for_test()
            .get_mut(7)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        cache.observe(&sim, 32, Some(32.0), false);
        assert!(!cache.poses.contains_key(&7));
    }

    #[test]
    fn render_interpolation_readout_distinguishes_requested_from_bypassed() {
        let mut sim = live_world();
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, None, false);
        assert!(cache.readout().interpolation);
        assert!(!cache.readout().sampling);
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        cache.observe(&sim, 32, Some(32.0), false);
        assert!(cache.readout().sampling);
        cache.observe(&sim, 40, Some(32.0), true);
        assert!(!cache.readout().sampling);
        cache.toggle();
        assert!(!cache.readout().interpolation);
    }

    #[test]
    fn render_interpolation_status_pieces_follow_one_shared_body_offset() {
        use crate::app::presentation::ui_overlays::{StatusTextOverlayInstances, TechnoExtras};
        use crate::render::batch::SpriteInstance;
        let sprite = || SpriteInstance {
            position: [10.0, 20.0],
            ..Default::default()
        };
        let mut status = TechnoExtras {
            bomb_clock: Some(sprite()),
            wrench: Some(sprite()),
            rank: Some(sprite()),
            background: Some(sprite()),
            fills: vec![sprite()],
            scale: vec![sprite()],
            self_heal: Some(sprite()),
            talk_bubble: Some(sprite()),
            group: StatusTextOverlayInstances {
                rectangles: vec![sprite()],
                text: vec![sprite()],
            },
        };
        status.translate_for_interpolation([1.5, -2.0]);
        let positions: Vec<_> = status
            .bomb_clock
            .iter()
            .chain(status.wrench.iter())
            .chain(status.rank.iter())
            .chain(status.background.iter())
            .chain(status.fills.iter())
            .chain(status.scale.iter())
            .chain(status.self_heal.iter())
            .chain(status.group.rectangles.iter())
            .chain(status.group.text.iter())
            .chain(status.talk_bubble.iter())
            .map(|instance| instance.position)
            .collect();
        assert_eq!(positions.len(), 10);
        assert!(positions.iter().all(|position| *position == [11.5, 18.0]));
    }

    #[test]
    fn render_interpolation_retail_movement_supplies_extra_poses_without_changing_the_world() {
        use crate::sim::command::{Command, CommandEnvelope};
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x20261020);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        let tank = sim
            .spawn_object("LTNK", "Americans", 8, 8, 0, &rules)
            .unwrap();
        let owner = sim.interner().get("Americans").unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session().tick() + 1,
            Command::Move {
                entity_id: tank,
                target_rx: 16,
                target_ry: 12,
            },
        ));
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.0), false);
        let mut additional_poses = 0;
        for step in 1..=120 {
            let previous = RenderPose::read(
                sim.entities().get(tank).unwrap(),
                sim.session().binary_frame(),
            );
            let commands = sim.take_due_commands();
            let grid = sim.path_grid_snapshot();
            sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let current = RenderPose::read(
                sim.entities().get(tank).unwrap(),
                sim.session().binary_frame(),
            );
            cache.observe(&sim, step * 32, Some(32.0), false);
            let hash = sim.state_hash();
            for subframe in [8, 16, 24] {
                cache.observe(&sim, step * 32 + subframe, Some(32.0), false);
                let painted = cache.sample_entity(
                    sim.entities().get(tank).unwrap(),
                    sim.session().binary_frame(),
                );
                if previous.screen != current.screen {
                    assert_eq!(
                        painted.screen,
                        previous.between(current, subframe as f32 / 32.0).screen
                    );
                    assert_ne!(painted.screen, current.screen);
                    additional_poses += 1;
                }
            }
            assert_eq!(
                sim.state_hash(),
                hash,
                "presentation changed authority at step {step}"
            );
        }
        assert!(
            additional_poses > 30,
            "ordinary retail movement produced no meaningful interpolated samples"
        );
    }

    #[test]
    fn render_interpolation_attitude_slope_recoil_and_building_turrets_are_continuous() {
        use crate::sim::movement::slope_transition::SlopeRenderPhase::{Stable, Transition};
        let a = RenderPose {
            angles: [0.2, -0.4],
            rocket_pitch: 0.2,
            visual_lift: 2.,
            lift: 10.,
            recoil: ([2., 4.], true),
            slope: Some(Stable(0)),
            ..at(0, 0xF000)
        };
        let b = RenderPose {
            angles: [0., 0.],
            rocket_pitch: 0.,
            visual_lift: 0.,
            lift: 20.,
            recoil: ([0., 0.], false),
            slope: Some(Transition {
                from_slope: 0,
                to_slope: 3,
                phase_num: 1,
                phase_den: 3,
            }),
            barrel: 0x2000,
            ..at(32, 0x1000)
        };
        let mid = a.between(b, 0.5);
        assert!((mid.angles[0] - 0.1).abs() < 1e-6 && (mid.angles[1] + 0.2).abs() < 1e-6);
        assert_eq!(mid.recoil, ([1., 2.], true));
        assert_eq!((mid.body, mid.turret, mid.barrel), (0, 0, 0x1000));
        assert_eq!((mid.visual_lift, mid.lift), (1., 15.));
        let Some(Transition {
            phase_num,
            phase_den,
            ..
        }) = mid.slope
        else {
            panic!("slope must still transition");
        };
        assert!((phase_num as f32 / f32::from(phase_den) - 1. / 6.).abs() < 1. / 255.);
        assert_eq!(a.between(b, 1.), b);
        let mut sim = live_world();
        let entity = sim.entities_mut_for_test().get_mut(7).unwrap();
        entity.set_category_for_test(EntityCategory::Structure);
        assert!(
            eligible(entity),
            "stationary building guns also retain facing endpoints"
        );
    }

    #[test]
    fn render_interpolation_click_hover_and_band_box_use_the_painted_object() {
        use crate::app::input::entity_pick::{
            PickSource, PresentedWorld, band_rect_contains_drawn_object, pick_entity_at_point,
        };
        let mut sim = live_world();
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.), false);
        let old = RenderPose::read(sim.entities().get(7).unwrap(), sim.session().binary_frame());
        let e = sim.entities_mut_for_test().get_mut(7).unwrap();
        let c = object_location(e, None);
        e.position.put_location(crate::sim::components::DriveCoord {
            x: c.x + 256,
            y: c.y,
            z: c.z,
        });
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        cache.observe(&sim, 32, Some(32.), false);
        let source = PresentedWorld {
            sim: &sim,
            interpolation: &cache,
        };
        assert_eq!(
            source.screen_position(sim.entities().get(7).unwrap()),
            (old.screen[0], old.screen[1])
        );
        assert_eq!(
            pick_entity_at_point(
                source,
                &[7],
                None,
                None,
                old.screen[0],
                old.screen[1],
                Some(sim.interner()),
                None
            ),
            Some(7)
        );
        assert_eq!(
            pick_entity_at_point(
                sim.entities(),
                &[7],
                None,
                None,
                old.screen[0],
                old.screen[1],
                Some(sim.interner()),
                None
            ),
            None
        );
        assert!(band_rect_contains_drawn_object(
            source,
            &[7],
            None,
            None,
            old.screen[0] - 1.,
            old.screen[1] - 1.,
            old.screen[0] + 1.,
            old.screen[1] + 1.,
            Some(sim.interner())
        ));
        assert!(!band_rect_contains_drawn_object(
            sim.entities(),
            &[7],
            None,
            None,
            old.screen[0] - 1.,
            old.screen[1] - 1.,
            old.screen[0] + 1.,
            old.screen[1] + 1.,
            Some(sim.interner())
        ));
    }

    #[test]
    fn render_interpolation_real_torpedo_motion_and_trailer_sampling_leave_authority_untouched() {
        let Some((rules, _, mut sim, id)) =
            crate::app::presentation::instances::launched_stock_submarine_torpedo(false)
        else {
            return;
        };
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.), false);
        let before = PointPose::bullet(sim.projectiles().get(id).unwrap()).current();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let bullet = sim
            .projectiles()
            .get(id)
            .expect("torpedo remains in flight");
        let pose = PointPose::bullet(bullet);
        let current = pose.current();
        assert_ne!(before.coord, current.coord);
        cache.observe(&sim, 32, Some(32.), false);
        cache.observe(&sim, 48, Some(32.), false);
        let hash = sim.state_hash();
        let snapshot = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scene", 0);
        let painted = cache.sample_point(PointKind::Bullet, id, pose, sim.session().binary_frame());
        assert_eq!(
            painted.screen,
            std::array::from_fn(|i| (before.screen[i] + current.screen[i]) * 0.5)
        );
        assert_ne!(painted.screen, current.screen);
        assert!(painted.voxel_matrix().unwrap().is_finite());
        assert_eq!(
            painted.projectile_coord().x,
            painted.coord[0],
            "trail samples the same endpoint"
        );
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(
            crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scene", 0),
            snapshot
        );
    }

    #[test]
    fn render_interpolation_retail_owner_attached_anim_tracks_the_same_motion() {
        use crate::sim::{anim_class::AnimWorldCoord, components::AnimClassSpawnDescriptor};
        let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        rules
            .bind_authored_load_anim("SMOKEY", &assets, "TEM", "TEMPERATE")
            .unwrap();
        let mut sim = live_world();
        let ty = sim.intern("SMOKEY");
        let mut descriptor = AnimClassSpawnDescriptor::new(
            ty,
            4,
            4,
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ZERO,
            0,
        );
        descriptor.owner = Some(7);
        let c = object_location(sim.entities().get(7).unwrap(), None);
        let id = sim
            .spawn_anim_at_world(
                &rules,
                descriptor,
                AnimWorldCoord {
                    x: c.x,
                    y: c.y,
                    z: c.z + 104,
                },
            )
            .unwrap();
        let mut cache = RenderInterpolation::new(true);
        cache.observe(&sim, 0, Some(32.), false);
        let before = sim.anim_absolute_coord(id).unwrap();
        sim.entities_mut_for_test()
            .get_mut(7)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: c.x + 256,
                y: c.y,
                z: c.z,
            });
        let next_frame = sim.session().binary_frame() + 1;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(next_frame);
        cache.observe(&sim, 32, Some(32.), false);
        cache.observe(&sim, 48, Some(32.), false);
        let anim = sim.anim(id).unwrap();
        let c = sim.anim_absolute_coord(id).unwrap();
        assert_ne!(
            c.x, before.x,
            "AnimClass absolute coordinates follow their owner"
        );
        let pose = PointPose::new([c.x, c.y, c.z], anim.type_id().index(), anim.owner_entity());
        let hash = sim.state_hash();
        let sampled = cache.sample_point(PointKind::Anim, id, pose, sim.session().binary_frame());
        assert_eq!(sampled.coord[0], (before.x + c.x) / 2);
        assert_eq!(sampled.coord[2], c.z);
        assert_eq!(sim.state_hash(), hash);
    }

    #[test]
    fn render_interpolation_retail_smoke_children_keep_committed_motion_and_cleanup() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let mut sim = Simulation::with_seed(17);
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(32),
        );
        let ty = rules.ps_type_id_by_name("SmallGreySSys").unwrap();
        sim.spawn_particle_system(
            ty,
            glam::IVec3::new(3200, 3200, 1040),
            None,
            None,
            glam::IVec3::ZERO,
            None,
            &rules,
        )
        .unwrap();
        let mut cache = RenderInterpolation::new(true);
        let mut sampled_motion = 0;
        for step in 0..80 {
            cache.observe(&sim, step * 32, Some(32.), false);
            let previous: HashMap<_, _> = sim
                .particle_systems()
                .display_particles()
                .map(|(_, p)| {
                    (
                        p.stable_id(),
                        PointPose::new(p.coords().to_array(), p.type_id().0, None).current(),
                    )
                })
                .collect();
            sim.advance_tick(&[], Some(&rules), None, None, 33);
            cache.observe(&sim, (step + 1) * 32, Some(32.), false);
            cache.observe(&sim, (step + 1) * 32 + 16, Some(32.), false);
            let hash = sim.state_hash();
            for (_, p) in sim.particle_systems().display_particles() {
                let current = PointPose::new(p.coords().to_array(), p.type_id().0, None);
                if let Some(previous) = previous.get(&p.stable_id()) {
                    let painted = cache.sample_point(
                        PointKind::Particle,
                        p.stable_id(),
                        current,
                        sim.session().binary_frame(),
                    );
                    let current = current.current();
                    assert_eq!(
                        painted.screen,
                        std::array::from_fn(|i| (previous.screen[i] + current.screen[i]) * 0.5)
                    );
                    sampled_motion += usize::from(previous.screen != current.screen);
                }
            }
            assert_eq!(hash, sim.state_hash());
        }
        assert!(
            sampled_motion > 10,
            "production Smoke AI must yield live moving children"
        );
    }
}
