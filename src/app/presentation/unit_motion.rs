//! Presentation history for visible ground and flying units. A tick supplies
//! the endpoints. A successful present supplies the picker's geometry. SHP
//! infantry keep the committed DoType, stage and facing on each extra draw.

use std::cell::RefCell;

use super::frame::ObjectRecord;
use super::instances::EntityDrawBand;
use super::render_pose::RenderPose;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::util::lookup_hash::LookupMap;

/// The previous interpolation setting treated a step larger than a cell and
/// a half on any world axis as a placement, even without a teleport flag.
const MAX_STEP_LEPTONS: u32 = 384;

/// The hull offset, tilt and ground projection the locomotor's draw
/// resolved at this tick. A hovering hull can rock or climb without
/// changing the ground point beneath it.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct AirPose {
    pub(crate) body_offset_y: f32,
    pub(crate) tilt: [f32; 2],
    /// Projected XY at world Z zero. A climb alone leaves this row still.
    pub(crate) ground_y: f32,
}

fn screen_axis(from: f32, to: f32, fraction: f32, pixel_aligned: bool) -> f32 {
    let at = from + (to - from) * fraction;
    if pixel_aligned {
        (at + 0.5).floor()
    } else {
        at
    }
}

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
        if fraction == 0.0 {
            return self;
        }
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
                screen_axis(self.anchor[axis], to.anchor[axis], fraction, pixel_aligned)
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
pub(crate) struct UnitSample {
    previous: Transform,
    current: Transform,
    air_previous: AirPose,
    air_current: AirPose,
    location: DriveCoord,
    /// Previous confirmed XYZ minus current, in world leptons. Sampling the
    /// same displacement at a shadow's own world point keeps its terrain
    /// and bridge queries in step with the visible vehicle.
    previous_delta: [i32; 3],
    type_id: InternedId,
    locomotor: LocomotorKind,
    /// SHP infantry's current action. A change of action starts a fresh
    /// segment; successive animation stages of its walk use the same pair.
    infantry_doing: Option<i32>,
    shp: bool,
    band: EntityDrawBand,
    on_bridge: bool,
    hover_slope: u8,
    parasite_infested: bool,
    last_fire: i64,
}

impl UnitSample {
    fn continuous_with(&self, next: &Self) -> bool {
        let (a, b) = (self.location, next.location);
        self.type_id == next.type_id
            && self.locomotor == next.locomotor
            && self.infantry_doing == next.infantry_doing
            && self.shp == next.shp
            && self.band == next.band
            && self.on_bridge == next.on_bridge
            // A disabled hover settling onto a ramp takes a different native
            // draw matrix. A squid attaching/releasing its host changes the
            // grapple relation at the confirmed point.
            && self.hover_slope == next.hover_slope
            && self.parasite_infested == next.parasite_infested
            // The flash and projectile start at the authoritative muzzle on
            // this frame. Its vehicle must be at the same endpoint.
            && self.last_fire == next.last_fire
            && a.x.abs_diff(b.x) <= MAX_STEP_LEPTONS
            && a.y.abs_diff(b.y) <= MAX_STEP_LEPTONS
            && a.z.abs_diff(b.z) <= MAX_STEP_LEPTONS
    }

    fn moved(&self) -> bool {
        self.previous != self.current
            || self.air_previous != self.air_current
            // A climb can move the ground shadow even when the airframe's
            // projected anchor and tilt happen to stay on the same pixel.
            || (matches!(self.locomotor, LocomotorKind::Jumpjet | LocomotorKind::Fly | LocomotorKind::Hover)
                && self.previous_delta != [0; 3])
    }

    fn planar_sort_delta(&self) -> i32 {
        self.previous_delta[0] + self.previous_delta[1]
    }
}

#[derive(Debug, Default)]
struct Picture {
    units: LookupMap<u64, UnitSample>,
    frame: u32,
    fraction: Option<f32>,
    pixel_aligned: bool,
    moving: usize,
    /// Even an airborne mover can supply the sort adjustment for a Ground
    /// attachment. Only movement along X+Y changes those native keys.
    planar_moves: usize,
}

impl Picture {
    fn begin(&mut self, frame: u32, fraction: Option<f32>, zoom: f32) {
        self.units.clear();
        self.moving = 0;
        self.planar_moves = 0;
        self.frame = frame;
        self.pixel_aligned = (zoom - 1.0).abs() < 0.001;
        self.fraction = fraction
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(0.0, 1.0));
    }

    fn transform(&self, sample: UnitSample) -> Transform {
        sample.previous.between(
            sample.current,
            self.fraction.unwrap_or(1.0),
            self.pixel_aligned,
        )
    }

    /// Aircraft's doubled Fly Draw_Point is added before the sprite rounds
    /// to a screen pixel. Its hit test uses that same final row.
    fn fly_body_y(&self, sample: UnitSample) -> f32 {
        let from = sample.previous.anchor[1] + sample.air_previous.body_offset_y;
        let to = sample.current.anchor[1] + sample.air_current.body_offset_y;
        let fraction = self.fraction.unwrap_or(1.);
        if fraction == 0. {
            from
        } else if fraction == 1. || from == to {
            to
        } else {
            screen_axis(from, to, fraction, self.pixel_aligned)
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UnitMotion {
    presented: Picture,
    /// Object visitors share the match while constructing a candidate draw.
    /// Readers outside that draw only ever see the last published picture.
    drawing: RefCell<Picture>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct IntermediateKinds {
    pub(crate) jumpjet: bool,
    pub(crate) fly: bool,
    pub(crate) infantry: bool,
    pub(crate) ship: bool,
    pub(crate) hover: bool,
    pub(crate) shp_drive: bool,
}

impl UnitMotion {
    pub(crate) fn clear(&mut self) {
        self.presented.begin(0, None, 1.0);
        self.drawing.get_mut().begin(0, None, 1.0);
    }

    pub(crate) fn begin_draw(&mut self, frame: u32, fraction: Option<f32>, zoom: f32) {
        self.drawing.get_mut().begin(frame, fraction, zoom);
    }

    pub(super) fn sampling(&self) -> bool {
        self.drawing.borrow().fraction.is_some()
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

    /// Visible units with distinct endpoints on the last intermediate
    /// present, including independent voxel hull and turret turns.
    pub(crate) fn intermediate_units(&self) -> usize {
        if self.presented.fraction.is_some_and(|f| f > 0.0 && f < 1.0) {
            self.presented.moving
        } else {
            0
        }
    }

    /// For the spawn benchmark. Translation witnesses require a displayed
    /// anchor distinct from both endpoints: a 1 px step at native zoom
    /// often leaves no in-between pixel.
    pub(crate) fn intermediate_kinds(&self) -> IntermediateKinds {
        let mut kinds = IntermediateKinds::default();
        if self.intermediate_units() != 0 {
            for row in self.presented.units.values() {
                let translation = match (row.infantry_doing, row.locomotor) {
                    (Some(_), _) => Some(&mut kinds.infantry),
                    (None, LocomotorKind::Ship) => Some(&mut kinds.ship),
                    (None, LocomotorKind::Hover) => Some(&mut kinds.hover),
                    (None, LocomotorKind::Drive) if row.shp => Some(&mut kinds.shp_drive),
                    (None, LocomotorKind::Jumpjet) => {
                        kinds.jumpjet |= row.moved();
                        None
                    }
                    (None, LocomotorKind::Fly) => {
                        kinds.fly |= row.moved();
                        None
                    }
                    _ => None,
                };
                if let Some(seen) = translation
                    && !*seen
                    && row.previous.anchor != row.current.anchor
                {
                    let shown = self.presented.transform(*row).anchor;
                    *seen = shown != row.previous.anchor && shown != row.current.anchor;
                }
                if kinds.jumpjet
                    && kinds.fly
                    && kinds.infantry
                    && kinds.ship
                    && kinds.hover
                    && kinds.shp_drive
                {
                    break;
                }
            }
        }
        kinds
    }

    /// Drive, Ship, Hover and Walk belong to Ground. Flyers follow their registered
    /// Display band. A new layer or infantry action starts a fresh pair.
    /// Teleport infantry stay at their confirmed placement, even for a warp
    /// shorter than a cell. Carry, crash, paradrop and weapon lifts have
    /// their own draw paths.
    pub(crate) fn sample(
        &self,
        entity: &ObjectRecord,
        band: EntityDrawBand,
        air: AirPose,
    ) -> Option<UnitSample> {
        let drawing = self.drawing.borrow();
        drawing.fraction?;
        let locomotor = entity.locomotor.as_ref()?.active_kind();
        let infantry_doing = match (entity.category(), entity.is_voxel(), locomotor, band) {
            (EntityCategory::Infantry, false, LocomotorKind::Walk, EntityDrawBand::Ground)
            | (EntityCategory::Infantry, false, LocomotorKind::Jumpjet, _) => {
                Some(entity.infantry_sprite_pose()?.0)
            }
            (
                EntityCategory::Unit,
                _,
                LocomotorKind::Drive | LocomotorKind::Ship,
                EntityDrawBand::Ground,
            )
            | (EntityCategory::Unit, true, LocomotorKind::Hover, EntityDrawBand::Ground)
            | (EntityCategory::Unit, true, LocomotorKind::Jumpjet, _)
            | (EntityCategory::Aircraft, true, LocomotorKind::Fly, _) => None,
            _ => return None,
        };
        if !entity.is_object_alive()
            // A corpse can still be displayed as IsAlive at zero health.
            || entity.health.current() <= 0
            || entity.passenger_role.is_inside_transport()
            || entity.in_limbo()
            || entity.in_tube
            || entity.on_carryall
            || entity.is_falling_down()
            || entity.sinking.is_active()
            || entity.crash.is_crashing()
            || entity.is_warped_out()
            || entity.is_warping_in()
            || entity.pose.visual_lift != 0.0
        {
            return None;
        }
        let current = Transform::of(entity.pose);
        let mut next = UnitSample {
            previous: current,
            current,
            air_previous: air,
            air_current: air,
            location: entity.hud.location,
            previous_delta: [0; 3],
            type_id: entity.type_ref(),
            locomotor,
            infantry_doing,
            shp: !entity.is_voxel(),
            band,
            on_bridge: entity.on_bridge(),
            hover_slope: if locomotor == LocomotorKind::Hover {
                entity.place.hover_draw_slope
            } else {
                0
            },
            parasite_infested: entity.parasite_infested,
            last_fire: entity.last_fire_frame.frame(),
        };
        if let Some(old) = self.presented.units.get(&entity.stable_id())
            && old.continuous_with(&next)
        {
            if self.presented.frame.wrapping_add(1) == drawing.frame {
                next.previous = old.current;
                next.air_previous = old.air_current;
                next.previous_delta = [
                    old.location.x - next.location.x,
                    old.location.y - next.location.y,
                    old.location.z - next.location.z,
                ];
            } else if self.presented.frame == drawing.frame
                && old.current == current
                && old.air_current == air
                && old.location == next.location
            {
                next.previous = old.previous;
                next.air_previous = old.air_previous;
                next.previous_delta = old.previous_delta;
            }
        }
        if next.shp {
            // Infantry's 0x00518D80 and Unit's SHP selectors choose the
            // current authored facing and animation stage. Only move the
            // origin; a stationary SHP turn uses the new image at once.
            next.previous = Transform {
                anchor: next.previous.anchor,
                ..current
            };
        }
        Some(next)
    }

    pub(super) fn pose(&self, sample: UnitSample, committed: RenderPose) -> RenderPose {
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
    pub(super) fn depth_height(&self, sample: UnitSample, current: i32) -> i32 {
        current.saturating_add(
            (sample.previous_delta[2] as f32
                * (1.0 - self.drawing.borrow().fraction.unwrap_or(1.0)))
            .round() as i32,
        )
    }

    pub(super) fn air_pose(&self, sample: UnitSample) -> AirPose {
        let drawing = self.drawing.borrow();
        let fraction = drawing.fraction.unwrap_or(1.);
        let (from, to) = (sample.air_previous, sample.air_current);
        if fraction == 0. {
            return from;
        }
        if fraction >= 1.0 || (from == to && sample.previous == sample.current) {
            return to;
        }
        AirPose {
            body_offset_y: if sample.locomotor == LocomotorKind::Fly {
                drawing.fly_body_y(sample) - drawing.transform(sample).anchor[1]
            } else {
                0.
            },
            tilt: std::array::from_fn(|i| from.tilt[i] + (to.tilt[i] - from.tilt[i]) * fraction),
            ground_y: screen_axis(from.ground_y, to.ground_y, fraction, drawing.pixel_aligned),
        }
    }

    pub(super) fn world_coord(&self, sample: UnitSample, committed: [i32; 3]) -> [i32; 3] {
        let remaining = 1.0 - self.drawing.borrow().fraction.unwrap_or(1.0);
        std::array::from_fn(|axis| {
            committed[axis]
                .saturating_add((sample.previous_delta[axis] as f32 * remaining).round() as i32)
        })
    }

    /// Native Ground order uses world X+Y. The native key of this draw is
    /// already known; move it by the two endpoints' planar displacement.
    pub(super) fn sort_delta(&self, id: u64) -> i32 {
        let drawing = self.drawing.borrow();
        drawing.units.get(&id).map_or(0, |row| {
            (row.planar_sort_delta() as f32 * (1.0 - drawing.fraction.unwrap_or(1.0))).round()
                as i32
        })
    }

    pub(super) fn ground_order_changed(&self) -> bool {
        let drawing = self.drawing.borrow();
        drawing.planar_moves != 0 && drawing.fraction.is_some_and(|phase| phase < 1.0)
    }

    /// Record a body only once the plan has admitted visible pieces. Removing
    /// a body later (for fog) also removes its reason for an extra redraw.
    pub(crate) fn drawn(&self, id: u64, sample: UnitSample) {
        let mut drawing = self.drawing.borrow_mut();
        let old = drawing.units.insert(id, sample);
        drawing.moving += usize::from(sample.moved());
        drawing.moving -= usize::from(old.is_some_and(|row| row.moved()));
        drawing.planar_moves += usize::from(sample.planar_sort_delta() != 0);
        drawing.planar_moves -= usize::from(old.is_some_and(|row| row.planar_sort_delta() != 0));
    }

    pub(super) fn conceal(&self, id: u64) {
        let mut drawing = self.drawing.borrow_mut();
        if let Some(old) = drawing.units.remove(&id) {
            drawing.moving -= usize::from(old.moved());
            drawing.planar_moves -= usize::from(old.planar_sort_delta() != 0);
        }
    }

    /// Added to an attached graphic's native screen point in the current draw.
    pub(crate) fn offset(&self, id: u64) -> [f32; 2] {
        let drawing = self.drawing.borrow();
        if drawing.moving == 0 {
            return [0.0; 2];
        }
        drawing.units.get(&id).map_or([0.; 2], |&row| {
            let anchor = drawing.transform(row).anchor;
            [
                anchor[0] - row.current.anchor[0],
                anchor[1] - row.current.anchor[1],
            ]
        })
    }

    pub(crate) fn painted_anchor(&self, id: u64) -> Option<[f32; 2]> {
        let row = *self.presented.units.get(&id)?;
        let mut anchor = self.presented.transform(row).anchor;
        if row.locomotor == LocomotorKind::Fly {
            anchor[1] = self.presented.fly_body_y(row);
        }
        Some(anchor)
    }
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    fn placed_unit(
        name: &str,
        category: EntityCategory,
        kind: LocomotorKind,
        z: i32,
    ) -> GameEntity {
        let mut vehicle =
            GameEntity::test_default_of_category(7, name, "Americans", 10, 10, category);
        vehicle.lifecycle.set_object_alive_for_test(true);
        vehicle.lifecycle.set_in_limbo_for_test(false);
        vehicle.locomotor = Some(LocomotorState::for_test_kind(kind));
        vehicle.position.put_location(DriveCoord::cell(10, 10, z));
        vehicle
    }

    #[test]
    fn climbing_fly_uses_one_body_row_and_the_same_ground_point() {
        let mut jet = placed_unit("ORCA", EntityCategory::Aircraft, LocomotorKind::Fly, 128);
        let mut motion = UnitMotion::default();
        let lower = ObjectRecord::of_test(&jet);
        let level = AirPose {
            body_offset_y: 4.,
            ground_y: 200.,
            ..Default::default()
        };
        let band = EntityDrawBand::Top;
        motion.begin_draw(4, Some(1.), 1.);
        motion.drawn(jet.stable_id(), motion.sample(&lower, band, level).unwrap());
        motion.commit_presented();
        let [x, y] = lower.pose.screen;
        assert_eq!(motion.painted_anchor(jet.stable_id()), Some([x, y + 4.]));

        jet.position.put_location(DriveCoord::cell(10, 10, 256));
        let upper = ObjectRecord::of_test(&jet);
        let climb = AirPose {
            body_offset_y: 6.,
            tilt: [0.5, -1.],
            ..level
        };
        motion.begin_draw(5, Some(0.25), 1.);
        let sample = motion.sample(&upper, band, climb).unwrap();
        let physical_y = motion.pose(sample, upper.pose).screen[1];
        let air = motion.air_pose(sample);
        assert!(upper.pose.screen[1] < physical_y && physical_y < lower.pose.screen[1]);
        let body_y = (3. * (y + 4.) + upper.pose.screen[1] + 6.) / 4.;
        assert_eq!(physical_y + air.body_offset_y, (body_y + 0.5).floor());
        assert_eq!(air.ground_y, level.ground_y);
        assert_eq!(air.tilt, [0.125, -0.25]);
        assert_eq!(motion.world_coord(sample, [12, 15, 256]), [12, 15, 160]);
        motion.drawn(jet.stable_id(), sample);
        assert!(!motion.ground_order_changed());
        assert_eq!(
            upper.pose.screen[1] + motion.offset(jet.stable_id())[1],
            physical_y
        );
        motion.commit_presented();
        assert_ne!(air.body_offset_y, 0.);
        assert_eq!(
            motion.painted_anchor(jet.stable_id()),
            Some([x, physical_y + air.body_offset_y])
        );
        assert_eq!(
            motion.intermediate_kinds(),
            IntermediateKinds {
                fly: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn descending_jumpjet_samples_its_height_until_the_ground_band_takes_it() {
        let mut heli = placed_unit("SHAD", EntityCategory::Unit, LocomotorKind::Jumpjet, 232);
        let mut motion = UnitMotion::default();
        let air = AirPose::default();
        let id = heli.stable_id();
        let top = ObjectRecord::of_test(&heli);
        motion.begin_draw(6, Some(1.), 1.);
        motion.drawn(id, motion.sample(&top, EntityDrawBand::Air, air).unwrap());
        motion.commit_presented();

        let mut descending = DriveCoord::cell(10, 10, 216);
        descending.x += 8;
        heli.position.put_location(descending);
        let low = ObjectRecord::of_test(&heli);
        motion.begin_draw(7, Some(0.5), 1.);
        let halfway = motion.sample(&low, EntityDrawBand::Air, air).unwrap();
        assert_eq!(motion.depth_height(halfway, 216), 224);
        motion.drawn(id, halfway);
        // A Ground attachment can inherit its airborne parent's sort key.
        assert!(motion.ground_order_changed());
        motion.commit_presented();
        assert_eq!(
            motion.intermediate_kinds(),
            IntermediateKinds {
                jumpjet: true,
                ..Default::default()
            }
        );

        // Jumpjet In_Which_Layer (0x0054B8D0) returns Ground below two
        // levels while this helicopter is still descending.
        heli.position.put_location(DriveCoord::cell(10, 10, 200));
        let landing = ObjectRecord::of_test(&heli);
        motion.begin_draw(8, Some(0.25), 1.);
        let ground = motion
            .sample(&landing, EntityDrawBand::Ground, air)
            .unwrap();
        assert_eq!(motion.pose(ground, landing.pose), landing.pose);
        assert_eq!(motion.depth_height(ground, 200), 200);
        motion.drawn(id, ground);
        assert!(!motion.ground_order_changed());
        motion.commit_presented();
        assert!(!motion.needs_redraw(8, Some(0.75)));
    }

    #[test]
    fn walking_infantry_uses_the_current_facing_and_stage_at_its_sampled_position() {
        let mut gi = placed_unit("E1", EntityCategory::Infantry, LocomotorKind::Walk, 0);
        gi.set_is_voxel_for_test(false);
        gi.mission_leaf.set_infantry_doing_verified(3).unwrap(); // Walk
        let id = gi.stable_id();
        let band = EntityDrawBand::Ground;
        let air = AirPose::default();
        let mut motion = UnitMotion::default();
        let old = ObjectRecord::of_test(&gi);
        motion.begin_draw(2, Some(1.), 1.);
        motion.drawn(id, motion.sample(&old, band, air).unwrap());
        motion.commit_presented();

        let mut to = old.hud.location;
        to.x += 128;
        gi.position.put_location(to);
        gi.body_facing.snap(0x4000, 3);
        gi.set_native_stage_value(2);
        let next = ObjectRecord::of_test(&gi);
        motion.begin_draw(3, Some(0.5), 1.);
        let sample = motion.sample(&next, band, air).unwrap();
        let pose = motion.pose(sample, next.pose);
        assert!(old.pose.screen[0] < pose.screen[0] && pose.screen[0] < next.pose.screen[0]);
        assert_eq!(pose.body, 0x4000);
        assert_eq!(next.infantry_sprite_pose(), Some((3, 2)));
        motion.drawn(id, sample);
        motion.commit_presented();
        assert_eq!(motion.painted_anchor(id), Some(pose.screen));
        assert!(motion.intermediate_kinds().infantry);

        to.x += 32;
        gi.position.put_location(to);
        gi.mission_leaf.set_infantry_doing_verified(4).unwrap(); // FireUp
        let shooting = ObjectRecord::of_test(&gi);
        motion.begin_draw(4, Some(0.), 1.);
        let sample = motion.sample(&shooting, band, air).unwrap();
        assert_eq!(motion.pose(sample, shooting.pose), shooting.pose);
    }

    #[test]
    fn a_rocketeer_counts_only_when_its_translation_has_an_in_between_pixel() {
        let mut trooper = placed_unit(
            "JUMPJET",
            EntityCategory::Infantry,
            LocomotorKind::Jumpjet,
            500,
        );
        trooper.set_is_voxel_for_test(false);
        let band = EntityDrawBand::Top;
        let air = AirPose::default();
        let mut motion = UnitMotion::default();
        let id = trooper.stable_id();
        let old = ObjectRecord::of_test(&trooper);
        motion.begin_draw(9, Some(1.), 1.);
        motion.drawn(id, motion.sample(&old, band, air).unwrap());
        motion.commit_presented();

        let mut step = old.hud.location;
        step.x += 9;
        trooper.position.put_location(step);
        let new = ObjectRecord::of_test(&trooper);
        assert_eq!(
            new.pose.screen,
            [old.pose.screen[0] + 1., old.pose.screen[1]]
        );
        motion.begin_draw(10, Some(0.5), 1.);
        motion.drawn(id, motion.sample(&new, band, air).unwrap());
        motion.commit_presented();
        assert_eq!(motion.intermediate_kinds(), IntermediateKinds::default());

        // At wheel zoom the sampler submits a fractional world position. The
        // world zoom pass can still round it while composing the native texture.
        motion.begin_draw(10, Some(0.5), 2.);
        motion.drawn(id, motion.sample(&new, band, air).unwrap());
        motion.commit_presented();
        assert_eq!(
            motion.intermediate_kinds(),
            IntermediateKinds {
                infantry: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn repeated_draws_move_the_body_and_its_attachments_together_only_on_present() {
        let mut tank = GameEntity::test_default(8, "MTNK", "Americans", 10, 10);
        tank.lifecycle.set_object_alive_for_test(true);
        tank.lifecycle.set_in_limbo_for_test(false);
        tank.position.put_location(DriveCoord::cell(10, 10, 0));
        tank.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        tank.body_facing.snap(0xf000, 0);
        let motion = &mut UnitMotion::default();
        let old = ObjectRecord::of_test(&tank);
        let id = tank.stable_id();
        motion.begin_draw(10, Some(1.0), 1.0);
        motion.drawn(
            id,
            motion
                .sample(&old, EntityDrawBand::Ground, AirPose::default())
                .unwrap(),
        );
        motion.commit_presented();

        let mut to = old.hud.location;
        to.x += 256;
        tank.position.put_location(to);
        tank.body_facing.snap(0x1000, 1);
        let new = ObjectRecord::of_test(&tank);
        motion.begin_draw(11, Some(0.5), 1.0);
        let half = motion
            .sample(&new, EntityDrawBand::Ground, AirPose::default())
            .unwrap();
        let pose = motion.pose(half, new.pose);
        let [x, y] = old.pose.screen;
        assert_eq!(pose.screen, [x + 15.0, y + 8.0]);
        assert_eq!(pose.body, 0);
        motion.drawn(id, half);
        assert!(motion.ground_order_changed());
        motion.drawn(id, half);
        motion.conceal(id);
        assert!(!motion.ground_order_changed());
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
        let end = motion
            .sample(&new, EntityDrawBand::Ground, AirPose::default())
            .unwrap();
        assert_eq!(motion.pose(end, new.pose), new.pose);
        motion.drawn(id, end);
        motion.commit_presented();
        assert!(!motion.needs_redraw(11, None));
    }
}
