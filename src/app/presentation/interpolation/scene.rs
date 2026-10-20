//! Committed positions of non-Techno Display objects. Lifetimes, membership,
//! animation frames and all native physics remain with their existing owners.
use crate::sim::projectile::{Projectile, ProjectileCoord, ProjectileVelocity};
use crate::sim::world::Simulation;
use glam::{IVec3, Mat4, Quat};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PointKind {
    Bullet,
    Anim,
    Particle,
    System,
    Debris,
    Spotlight(u8),
    Wave(u8),
    BoltSource,
    ControlSource,
    ControlTarget,
    PlanningNode,
    PlanningEdge(u16),
    PlanningActor(u8),
    Airstrike(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointPose {
    pub coord: [i32; 3],
    identity: u32,
    owner: Option<u64>,
    bridge: bool,
    rotation: Option<Quat>,
    matrix: Option<Mat4>,
    velocity: Option<ProjectileVelocity>,
    max_step: i64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PointSample {
    pub coord: [i32; 3],
    pub screen: [f32; 2],
    pub lift: f32,
    pub rotation: Option<Mat4>,
    velocity: Option<(ProjectileVelocity, ProjectileVelocity, f32)>,
}

impl PointPose {
    pub(crate) fn new(coord: [i32; 3], identity: u32, owner: Option<u64>) -> Self {
        Self {
            coord,
            identity,
            owner,
            bridge: false,
            rotation: None,
            matrix: None,
            velocity: None,
            max_step: super::TELEPORT_LEPTONS,
        }
    }

    pub(crate) fn bullet(bullet: &Projectile) -> Self {
        let mut pose = Self::new(
            [
                bullet.position().x,
                bullet.position().y,
                bullet.position().z,
            ],
            bullet
                .payload()
                .projectile_type()
                .map_or(u32::MAX, |id| id.index()),
            Some(bullet.source_id()),
        );
        pose.bridge = bullet.on_bridge();
        // Fast, legitimate bullets may cross more than 1.5 cells in one tick.
        pose.max_step = bullet
            .velocity()
            .native()
            .into_iter()
            .map(|v| f64::from_bits(v.bits()).abs().ceil() as i64)
            .max()
            .unwrap_or(0)
            .saturating_add(super::TELEPORT_LEPTONS);
        pose.velocity = Some(bullet.velocity());
        pose
    }

    pub(crate) fn debris(piece: &crate::sim::voxel_anim::VoxelAnimObject) -> Self {
        let matrix = piece.bounce().drawing_matrix().ok().map(|m| {
            let m = m.map(|v| f32::from_bits(v.bits()));
            crate::render::vxl_raster::hva_to_mat4(&m, 1.)
        });
        Self::new(piece.world_coord().to_array(), piece.type_id().0, None).with_matrix(matrix)
    }

    fn with_matrix(mut self, matrix: Option<Mat4>) -> Self {
        self.matrix = matrix;
        self.rotation = matrix
            .filter(|m| m.is_finite())
            .map(|m| Quat::from_mat4(&m).normalize())
            .filter(|q| q.is_finite());
        self
    }

    fn continuous(self, current: Self) -> bool {
        self.identity == current.identity
            && self.owner == current.owner
            && self.bridge == current.bridge
            && self.coord.into_iter().zip(current.coord).all(|(a, b)| {
                (i64::from(a) - i64::from(b)).abs() <= self.max_step.max(current.max_step)
            })
    }

    fn sample(self, current: Self, alpha: f32) -> PointSample {
        let project = |c: [i32; 3]| {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(c[0], c[1], c[2]);
            [x, y]
        };
        let a = project(self.coord);
        let b = project(current.coord);
        let lift = |z| crate::util::native_x87::adjust_for_z_standard(z) as f32;
        PointSample {
            coord: std::array::from_fn(|i| {
                (f64::from(self.coord[i])
                    + (f64::from(current.coord[i]) - f64::from(self.coord[i])) * f64::from(alpha))
                .round() as i32
            }),
            screen: std::array::from_fn(|i| a[i] + (b[i] - a[i]) * alpha),
            lift: lift(self.coord[2]) + (lift(current.coord[2]) - lift(self.coord[2])) * alpha,
            rotation: if alpha >= 1. {
                current.matrix
            } else if alpha <= 0. {
                self.matrix
            } else {
                match (self.rotation, current.rotation) {
                    (Some(a), Some(b)) => Some(Mat4::from_quat(a.slerp(b, alpha))),
                    _ => current.matrix,
                }
            },
            velocity: self
                .velocity
                .zip(current.velocity)
                .map(|(a, b)| (a, b, alpha)),
        }
    }

    pub(crate) fn current(self) -> PointSample {
        self.sample(self, 1.)
    }
}

impl PointSample {
    pub(crate) fn voxel_matrix(self) -> Option<Mat4> {
        let Some((a, b, alpha)) = self.velocity else {
            return self.rotation;
        };
        let matrix = crate::render::vxl_raster::projectile_voxel_matrix;
        if alpha >= 1. || a == b {
            return matrix(b);
        }
        if alpha <= 0. {
            return matrix(a);
        }
        let (a, b) = (
            Quat::from_mat4(&matrix(a)?).normalize(),
            Quat::from_mat4(&matrix(b)?).normalize(),
        );
        Some(Mat4::from_quat(a.slerp(b, alpha)))
    }

    pub(crate) fn bullet_velocity(self, fallback: ProjectileVelocity) -> ProjectileVelocity {
        let Some((a, b, alpha)) = self.velocity else {
            return fallback;
        };
        if alpha >= 1. || a == b {
            return b;
        }
        if alpha <= 0. {
            return a;
        }
        let a = a.native();
        let b = b.native();
        let v = std::array::from_fn(|i| {
            let a = f64::from_bits(a[i].bits());
            let b = f64::from_bits(b[i].bits());
            crate::util::native_x87::NativeF64Bits::from_bits(
                (a + (b - a) * f64::from(alpha)).to_bits(),
            )
        });
        ProjectileVelocity::from_native(v)
    }
    pub(crate) fn projectile_coord(self) -> ProjectileCoord {
        ProjectileCoord::new(self.coord[0], self.coord[1], self.coord[2])
    }
    pub(crate) fn ivec3(self) -> IVec3 {
        IVec3::from_array(self.coord)
    }
}

struct Endpoints {
    previous: PointPose,
    current: PointPose,
    frame: u32,
}

#[derive(Default)]
pub(super) struct SceneInterpolation {
    points: HashMap<(PointKind, u64), Endpoints>,
    // Geometry available only at its existing draw owner (FLH and wave edges)
    // is observed once per committed frame on demand, never per-frame simulated.
    extra: std::cell::RefCell<HashMap<(PointKind, u64), Endpoints>>,
}

impl SceneInterpolation {
    pub(super) fn clear(&mut self) {
        self.points.clear();
        self.extra.get_mut().clear();
    }

    fn record(&mut self, kind: PointKind, id: u64, current: PointPose, frame: u32, reset: bool) {
        let previous = self
            .points
            .get(&(kind, id))
            .filter(|e| !reset && e.frame.wrapping_add(1) == frame && e.current.continuous(current))
            .map_or(current, |e| e.current);
        self.points.insert(
            (kind, id),
            Endpoints {
                previous,
                current,
                frame,
            },
        );
    }

    pub(super) fn observe(&mut self, sim: &Simulation, reset: bool) {
        let frame = sim.session().binary_frame();
        for (&id, bullet) in sim.projectiles().iter() {
            self.record(
                PointKind::Bullet,
                id,
                PointPose::bullet(bullet),
                frame,
                reset,
            );
        }
        for (&id, anim) in sim.anims() {
            if anim
                .owner_entity()
                .and_then(|id| sim.entities().get(id))
                .is_some_and(|owner| !super::eligible(owner))
            {
                continue;
            }
            if let Some(c) = sim.anim_absolute_coord(id) {
                self.record(
                    PointKind::Anim,
                    id,
                    PointPose::new([c.x, c.y, c.z], anim.type_id().index(), anim.owner_entity()),
                    frame,
                    reset,
                );
            }
        }
        for (&id, piece) in sim.voxel_anims().iter() {
            self.record(
                PointKind::Debris,
                id,
                PointPose::debris(piece),
                frame,
                reset,
            );
        }
        for (&id, system) in sim.particle_systems().iter().filter(|(_, s)| !s.in_limbo()) {
            self.record(
                PointKind::System,
                id,
                PointPose::new(
                    system.coords().to_array(),
                    system.type_id().0,
                    system.owner_entity(),
                ),
                frame,
                reset,
            );
        }
        for (_, particle) in sim.particle_systems().display_particles() {
            self.record(
                PointKind::Particle,
                particle.stable_id(),
                PointPose::new(particle.coords().to_array(), particle.type_id().0, None),
                frame,
                reset,
            );
        }
        self.points.retain(|_, e| e.frame == frame);
        self.extra
            .get_mut()
            .retain(|_, e| e.frame == frame || e.frame.wrapping_add(1) == frame);
    }

    pub(super) fn sample(
        &self,
        kind: PointKind,
        id: u64,
        current: PointPose,
        frame: u32,
        alpha: f32,
    ) -> PointSample {
        let previous = self
            .points
            .get(&(kind, id))
            .filter(|e| e.frame == frame && e.current == current)
            .map_or(current, |e| e.previous);
        previous.sample(current, alpha)
    }

    pub(super) fn sample_extra(
        &self,
        kind: PointKind,
        id: u64,
        current: PointPose,
        frame: u32,
        alpha: f32,
    ) -> PointSample {
        let mut points = self.extra.borrow_mut();
        let e = points.entry((kind, id)).or_insert(Endpoints {
            previous: current,
            current,
            frame,
        });
        if e.frame != frame {
            e.previous = if e.frame.wrapping_add(1) == frame && e.current.continuous(current) {
                e.current
            } else {
                current
            };
            e.current = current;
            e.frame = frame;
        }
        // A same-frame change is an out-of-band edit, not a new endpoint.
        if e.current != current {
            return current.current();
        }
        e.previous.sample(current, alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_interpolation_all_scene_namespaces_share_phase_without_aliasing() {
        let mut cache = SceneInterpolation::default();
        let a = PointPose::new([2048, 2048, 0], 1, None);
        let b = PointPose::new([2304, 2048, 104], 1, None);
        for kind in [
            PointKind::Bullet,
            PointKind::Anim,
            PointKind::Particle,
            PointKind::System,
            PointKind::Debris,
        ] {
            cache.record(kind, 7, a, 1, true);
            cache.record(kind, 7, b, 2, false);
            let mid = cache.sample(kind, 7, b, 2, 0.5);
            assert_eq!(mid.coord, [2176, 2048, 52]);
            let (a, b) = (a.current(), b.current());
            assert_eq!(
                mid.screen,
                std::array::from_fn(|i| (a.screen[i] + b.screen[i]) * 0.5)
            );
            assert_eq!(mid.lift, (a.lift + b.lift) * 0.5);
        }
        assert_eq!(
            cache.points.len(),
            5,
            "object kinds cannot alias equal numeric handles"
        );
        for kind in [
            PointKind::Spotlight(0),
            PointKind::Wave(2),
            PointKind::BoltSource,
            PointKind::ControlSource,
            PointKind::ControlTarget,
            PointKind::PlanningNode,
            PointKind::PlanningEdge(1),
            PointKind::PlanningActor(0),
            PointKind::Airstrike(0),
        ] {
            assert_eq!(cache.sample_extra(kind, 7, a, 1, 0.5).coord, a.coord);
            assert_eq!(
                cache.sample_extra(kind, 7, b, 2, 0.25).coord,
                [2112, 2048, 26]
            );
            assert_eq!(
                cache.sample_extra(kind, 7, b, 2, 0.75).coord,
                [2240, 2048, 78]
            );
        }
    }

    #[test]
    fn render_interpolation_scene_discontinuities_prune_and_never_replay() {
        let a = PointPose::new([2000, 2000, 0], 1, Some(3));
        for b in [
            PointPose::new([3000, 2000, 0], 1, Some(3)),
            PointPose::new([2100, 2000, 0], 2, Some(3)),
            PointPose::new([2100, 2000, 0], 1, Some(4)),
            PointPose { bridge: true, ..a },
        ] {
            let mut cache = SceneInterpolation::default();
            cache.record(PointKind::Anim, 7, a, 1, true);
            cache.record(PointKind::Anim, 7, b, 2, false);
            assert_eq!(cache.sample(PointKind::Anim, 7, b, 2, 0.).coord, b.coord);
        }
        let mut cache = SceneInterpolation::default();
        cache.record(PointKind::Anim, 7, a, 1, true);
        let b = PointPose::new([2100, 2000, 0], 1, Some(3));
        cache.record(PointKind::Anim, 7, b, 5, false);
        assert_eq!(cache.sample(PointKind::Anim, 7, b, 5, 0.).coord, b.coord);
        let c = PointPose::new([2150, 2000, 0], 1, Some(3));
        assert_eq!(
            cache.sample(PointKind::Anim, 7, c, 5, 0.).coord,
            c.coord,
            "same-frame edits snap"
        );
        let mut sim = Simulation::new();
        sim.session_mut_for_test().set_binary_frame_for_test(6);
        cache.observe(&sim, false);
        assert!(
            cache.points.is_empty(),
            "retired storage cannot leave a displayed ghost"
        );
        cache.sample_extra(PointKind::Wave(0), 7, a, 6, 0.5);
        sim.session_mut_for_test().set_binary_frame_for_test(8);
        cache.observe(&sim, false);
        assert!(cache.extra.borrow().is_empty());
        cache.clear();
    }

    #[test]
    fn render_interpolation_tumbling_rotation_is_orthonormal_and_shortest_arc() {
        let a = PointPose::new([2048, 2048, 100], 1, None)
            .with_matrix(Some(Mat4::from_rotation_z(170_f32.to_radians())));
        let b = PointPose::new([2048, 2048, 100], 1, None)
            .with_matrix(Some(Mat4::from_rotation_z((-170_f32).to_radians())));
        let matrix = a.sample(b, 0.5).rotation.unwrap();
        let x = matrix.transform_vector3(glam::Vec3::X);
        assert!((x.x + 1.).abs() < 1e-5 && x.y.abs() < 1e-5);
        assert!((matrix.determinant() - 1.).abs() < 1e-5);
        assert_eq!(
            a.sample(b, 1.).rotation,
            b.matrix,
            "disabled/exact mode keeps native bits"
        );
    }
}
