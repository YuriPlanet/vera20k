//! Presentation history for Drive vehicles whose voxel body made it onto the
//! battlefield. A tick supplies the endpoints. A successful present supplies
//! the picker's geometry. An extra draw changes neither the world nor the
//! native voxel art available for a facing.

use std::cell::RefCell;

use super::frame::ObjectRecord;
use super::render_pose::RenderPose;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::util::lookup_hash::LookupMap;

/// The previous interpolation setting treated a step larger than a cell and
/// a half on any world axis as a placement, even without a teleport flag.
const MAX_STEP_LEPTONS: u32 = 384;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Transform {
    anchor: [f32; 2],
    body: u16,
    turret: u16,
    barrel: u16,
}

impl Transform {
    fn of(pose: RenderPose) -> Self {
        Self {
            anchor: pose.screen,
            body: pose.body,
            turret: pose.turret,
            barrel: pose.barrel,
        }
    }

    fn between(self, to: Self, fraction: f32, pixel_aligned: bool) -> Self {
        if fraction >= 1.0 || self == to {
            return to;
        }
        let facing = |from: u16, to: u16| {
            let delta = to.wrapping_sub(from) as i16;
            (i32::from(from) + (f32::from(delta) * fraction).round() as i32) as u16
        };
        Self {
            // At native zoom the voxel shader places geometry on whole pixels.
            // Round the common anchor once so all its pieces share the depth
            // row. Other zooms draw the fractional world point.
            anchor: std::array::from_fn(|axis| {
                let at = self.anchor[axis] + (to.anchor[axis] - self.anchor[axis]) * fraction;
                if pixel_aligned {
                    (at + 0.5).floor()
                } else {
                    at
                }
            }),
            body: facing(self.body, to.body),
            turret: facing(self.turret, to.turret),
            barrel: facing(self.barrel, to.barrel),
        }
    }
}

/// Only the transform and its continuity guards survive a picture. A full
/// ObjectRecord includes status, drawing and sidebar facts we never need here.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TankSample {
    previous: Transform,
    current: Transform,
    location: DriveCoord,
    sort_delta: i32,
    z_delta: i32,
    type_id: InternedId,
    on_bridge: bool,
    last_fire: i64,
}

impl TankSample {
    fn continuous_with(&self, next: &Self) -> bool {
        let (a, b) = (self.location, next.location);
        self.type_id == next.type_id
            && self.on_bridge == next.on_bridge
            // The flash and projectile start at the authoritative muzzle on
            // this frame. Its vehicle must be at the same endpoint.
            && self.last_fire == next.last_fire
            && a.x.abs_diff(b.x) <= MAX_STEP_LEPTONS
            && a.y.abs_diff(b.y) <= MAX_STEP_LEPTONS
            && a.z.abs_diff(b.z) <= MAX_STEP_LEPTONS
    }

    fn moved(&self) -> bool {
        self.previous != self.current
    }
}

#[derive(Debug, Default)]
struct Picture {
    tanks: LookupMap<u64, TankSample>,
    frame: u32,
    fraction: Option<f32>,
    pixel_aligned: bool,
    moving: usize,
}

impl Picture {
    fn begin(&mut self, frame: u32, fraction: Option<f32>, zoom: f32) {
        self.tanks.clear();
        self.moving = 0;
        self.frame = frame;
        self.pixel_aligned = (zoom - 1.0).abs() < 0.001;
        self.fraction = fraction
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(0.0, 1.0));
    }

    fn transform(&self, sample: TankSample) -> Transform {
        sample.previous.between(
            sample.current,
            self.fraction.unwrap_or(1.0),
            self.pixel_aligned,
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct TankMotion {
    presented: Picture,
    /// Object visitors share the match while constructing a candidate draw.
    /// Readers outside that draw only ever see the last published picture.
    drawing: RefCell<Picture>,
}

impl TankMotion {
    pub(crate) fn clear(&mut self) {
        self.presented.begin(0, None, 1.0);
        self.drawing.get_mut().begin(0, None, 1.0);
    }

    pub(crate) fn begin_draw(&mut self, frame: u32, fraction: Option<f32>, zoom: f32) {
        self.drawing.get_mut().begin(frame, fraction, zoom);
    }

    pub(crate) fn commit_presented(&mut self) {
        std::mem::swap(&mut self.presented, self.drawing.get_mut());
    }

    pub(crate) fn needs_redraw(&self, frame: u32, fraction: Option<f32>) -> bool {
        let fraction = fraction
            .filter(|f| f.is_finite())
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        self.presented.frame == frame
            && self.presented.moving != 0
            && self
                .presented
                .fraction
                .is_some_and(|shown| shown != fraction)
    }

    /// Visible vehicles with distinct endpoints on the last intermediate
    /// present. Includes independent hull and turret turns.
    pub(crate) fn intermediate_tanks(&self) -> usize {
        if self.presented.fraction.is_some_and(|f| f > 0.0 && f < 1.0) {
            self.presented.moving
        } else {
            0
        }
    }

    /// Only eligible on the Ground voxel draw. A body being carried,
    /// converted, entering a tube or raised by a weapon has its own points.
    pub(crate) fn sample(&self, entity: &ObjectRecord) -> Option<TankSample> {
        let drawing = self.drawing.borrow();
        drawing.fraction?;
        if entity.category() != EntityCategory::Unit
            || !entity.is_voxel()
            || entity.locomotor.as_ref().map(|l| l.active_kind()) != Some(LocomotorKind::Drive)
            || !entity.is_object_alive()
            || entity.in_limbo()
            || entity.in_tube
            || entity.on_carryall
            || entity.is_falling_down()
            || entity.sinking.is_active()
            || entity.is_warped_out()
            || entity.is_warping_in()
            || entity.pose.visual_lift != 0.0
        {
            return None;
        }
        let current = Transform::of(entity.pose);
        let mut next = TankSample {
            previous: current,
            current,
            location: entity.hud.location,
            sort_delta: 0,
            z_delta: 0,
            type_id: entity.type_ref(),
            on_bridge: entity.on_bridge(),
            last_fire: entity.last_fire_frame.frame(),
        };
        if let Some(old) = self.presented.tanks.get(&entity.stable_id())
            && old.continuous_with(&next)
        {
            if self.presented.frame.wrapping_add(1) == drawing.frame {
                next.previous = old.current;
                next.sort_delta =
                    (old.location.x - next.location.x) + (old.location.y - next.location.y);
                next.z_delta = old.location.z - next.location.z;
            } else if self.presented.frame == drawing.frame
                && old.current == current
                && old.location == next.location
            {
                next.previous = old.previous;
                next.sort_delta = old.sort_delta;
                next.z_delta = old.z_delta;
            }
        }
        Some(next)
    }

    pub(super) fn pose(&self, sample: TankSample, committed: RenderPose) -> RenderPose {
        let pose = self.drawing.borrow().transform(sample);
        RenderPose {
            screen: pose.anchor,
            body: pose.body,
            turret: pose.turret,
            barrel: pose.barrel,
            ..committed
        }
    }

    /// Height for Foot's depth seed, using the same fraction as the body.
    pub(super) fn depth_height(&self, sample: TankSample, current: i32) -> i32 {
        current.saturating_add(
            (sample.z_delta as f32 * (1.0 - self.drawing.borrow().fraction.unwrap_or(1.0))).round()
                as i32,
        )
    }

    /// Native Ground order uses world X+Y. The native key of this draw is
    /// already known; move it by the two endpoints' planar displacement.
    pub(super) fn sort_delta(&self, id: u64) -> i32 {
        let drawing = self.drawing.borrow();
        drawing.tanks.get(&id).map_or(0, |row| {
            (row.sort_delta as f32 * (1.0 - drawing.fraction.unwrap_or(1.0))).round() as i32
        })
    }

    pub(super) fn drawing_motion(&self) -> bool {
        let drawing = self.drawing.borrow();
        drawing.moving != 0 && drawing.fraction.is_some_and(|phase| phase < 1.0)
    }

    /// Record a body only once the plan has admitted visible pieces. Removing
    /// a body later (for fog) also removes its reason for an extra redraw.
    pub(crate) fn drawn(&self, id: u64, sample: TankSample) {
        let mut drawing = self.drawing.borrow_mut();
        let old = drawing.tanks.insert(id, sample);
        drawing.moving += usize::from(sample.moved());
        drawing.moving -= usize::from(old.is_some_and(|row| row.moved()));
    }

    pub(super) fn conceal(&self, id: u64) {
        let mut drawing = self.drawing.borrow_mut();
        if let Some(old) = drawing.tanks.remove(&id) {
            drawing.moving -= usize::from(old.moved());
        }
    }

    /// Added to an attached graphic's native screen point in the current draw.
    pub(crate) fn offset(&self, id: u64) -> [f32; 2] {
        let drawing = self.drawing.borrow();
        if drawing.moving == 0 {
            return [0.0; 2];
        }
        drawing.tanks.get(&id).map_or([0.; 2], |&row| {
            let anchor = drawing.transform(row).anchor;
            [
                anchor[0] - row.current.anchor[0],
                anchor[1] - row.current.anchor[1],
            ]
        })
    }

    pub(crate) fn painted_anchor(&self, id: u64) -> Option<[f32; 2]> {
        self.presented
            .tanks
            .get(&id)
            .map(|&row| self.presented.transform(row).anchor)
    }
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    #[test]
    fn repeated_draws_move_the_body_and_its_attachments_together_only_on_present() {
        let mut tank = GameEntity::test_default(8, "MTNK", "Americans", 10, 10);
        tank.lifecycle.set_object_alive_for_test(true);
        tank.lifecycle.set_in_limbo_for_test(false);
        tank.position.put_location(DriveCoord::cell(10, 10, 0));
        tank.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        tank.body_facing.snap(0xf000, 0);
        let motion = &mut TankMotion::default();
        let old = ObjectRecord::of_test(&tank);
        let id = tank.stable_id();
        motion.begin_draw(10, Some(1.0), 1.0);
        motion.drawn(id, motion.sample(&old).unwrap());
        motion.commit_presented();

        let mut to = old.hud.location;
        to.x += 256;
        tank.position.put_location(to);
        tank.body_facing.snap(0x1000, 1);
        let new = ObjectRecord::of_test(&tank);
        motion.begin_draw(11, Some(0.5), 1.0);
        let half = motion.sample(&new).unwrap();
        let pose = motion.pose(half, new.pose);
        let [x, y] = old.pose.screen;
        assert_eq!(pose.screen, [x + 15.0, y + 8.0]);
        assert_eq!(pose.body, 0);
        motion.drawn(id, half);
        let [dx, dy] = motion.offset(id);
        assert_eq!(
            [new.pose.screen[0] + dx, new.pose.screen[1] + dy],
            pose.screen
        );
        assert_eq!(motion.painted_anchor(id), Some(old.pose.screen));
        motion.commit_presented();
        assert_eq!(motion.painted_anchor(id), Some(pose.screen));
        assert!(motion.needs_redraw(11, Some(1.0)));

        motion.begin_draw(11, Some(1.0), 1.0);
        let end = motion.sample(&new).unwrap();
        assert_eq!(motion.pose(end, new.pose), new.pose);
        motion.drawn(id, end);
        motion.commit_presented();
        assert!(!motion.needs_redraw(11, None));
    }
}
