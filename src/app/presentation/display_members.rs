//! The Display members of one [`PresentationFrame`], each filed once under
//! the store that holds it, and each record's screen span.
//!
//! A draw walks the Display members in Tactical's order twice: the object
//! loop (`render::build_instances`) and the effects loop
//! (`instances::build_display_effect_instances`). Asking every store about
//! every member, and reading every record to cull it, made each draw a walk
//! of the whole map. The frame's world cannot change while the frame lives,
//! so the frame files its members once
//! ([`PresentationFrame::display_members`]) and every draw of it reads the
//! same table. Each effect keeps the point its draw is tested at, so a draw
//! passes an effect off the view by before asking its store.

use crate::app::presentation::frame::{ObjectRecord, PresentationFrame};
use crate::app::presentation::instances::{AnimDrawPoint, anim_screen_point};
use crate::map::entities::EntityCategory;
use crate::sim::particles::DisplayParticlePlace;
use crate::sim::world::Simulation;
use crate::sim::world::display_layers::DisplayLayer;
use crate::util::lookup_hash::LookupMap;

/// Where a record draws and is picked, in world screen pixels: the column
/// from its body down to anything it draws below it. The object loop's
/// pass-by (`instances::PassByWindow`) and the cursor pick's pre-filter
/// (`entity_pick`) read it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ScreenSpan {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

impl ScreenSpan {
    pub(crate) fn of(record: &ObjectRecord) -> Self {
        // `RenderPose::read` took this point from
        // `locomotor_visual::screen_position`, where every visitor culls
        // from. An IonBlast lifts the point infantry are drawn at and every
        // object is picked at by `visual_lift`, which may be negative.
        let [x, y] = record.pose.screen;
        let lifted = y - record.pose.visual_lift;
        let (top, bottom) = (y.min(lifted), y.max(lifted));
        if record.category() != EntityCategory::Aircraft {
            return Self { x, top, bottom };
        }
        // An aircraft's shadow lies on the ground straight below it
        // (`aircraft_shadow::pose`), a rocket's lower again by its height:
        // never past twice the body's height above its Z = 0 point.
        let [world_x, world_y] =
            crate::sim::movement::ground_pose::position_world_xy(&record.position);
        let (_, ground) = crate::util::lepton::absolute_leptons_to_screen(world_x, world_y, 0);
        Self {
            x,
            top: top.min(ground),
            bottom: bottom.max(2.0 * ground - y),
        }
    }
}

/// A Display member the object loop draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ObjectMember {
    pub layer: DisplayLayer,
    pub id: u64,
    pub kind: ObjectKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ObjectKind {
    /// An object with a record: its slot in the frame
    /// ([`PresentationFrame::record_at`]) and its span.
    Record { slot: usize, span: ScreenSpan },
    /// A Bullet.
    Bullet,
    /// A piece of debris (a VoxelAnim).
    Debris,
}

/// A Display member the effects loop draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectMember {
    pub id: u64,
    pub kind: EffectKind,
}

/// An effect's kind and where its draw is tested against the view, in world
/// screen pixels before the camera; `None` where it draws nothing this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectKind {
    /// Where its DrawIt draws; `None` while it is delayed or has no
    /// coordinate.
    Anim(Option<AnimDrawPoint>),
    Spotlight([f32; 2]),
    /// `None` in limbo.
    ParticleSystem(Option<[f32; 2]>),
    Particle(DisplayParticlePlace, Option<[f32; 2]>),
}

impl EffectMember {
    /// The point the effects loop tests against the view
    /// (`instances::projection_admitted`) before it asks any store.
    pub(crate) fn point(&self) -> Option<[f32; 2]> {
        match self.kind {
            EffectKind::Anim(draw) => draw.map(|draw| draw.center),
            EffectKind::Spotlight(point) => Some(point),
            EffectKind::ParticleSystem(point) | EffectKind::Particle(_, point) => point,
        }
    }
}

fn screen_point(x: i32, y: i32, z: i32) -> [f32; 2] {
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
    [x, y]
}

/// One frame's Display members in Tactical's order, split by the loop that
/// draws them. Terrain objects, which neither loop draws, are left out.
#[derive(Clone, Debug, Default)]
pub(crate) struct DisplayMembers {
    objects: Vec<ObjectMember>,
    effects: Vec<EffectMember>,
    /// Where each Display particle is kept, for a reader that asks by id.
    particles: LookupMap<u64, DisplayParticlePlace>,
}

impl DisplayMembers {
    /// `sim`'s Display members, filed. `sim` is the world `frame` was
    /// captured from.
    pub(crate) fn of(frame: &PresentationFrame, sim: &Simulation) -> Self {
        let particles: LookupMap<_, _> = sim.particle_systems().display_particle_places().collect();
        let mut objects = Vec::new();
        let mut effects = Vec::new();
        // Every object store shares one id namespace
        // (`Simulation::allocate_stable_id`), so an id has one store and the
        // order of these tests picks no winner: records, Anims and Terrain
        // objects, the many, are asked first.
        for (layer, id) in sim.display_layers().members_in_order() {
            let object = |kind| ObjectMember { layer, id, kind };
            let kind = if let Some((slot, span)) = frame.slot_and_span(id) {
                objects.push(object(ObjectKind::Record { slot, span }));
                continue;
            } else if let Some(anim) = sim.anim(id) {
                EffectKind::Anim(anim_screen_point(frame, anim, id))
            } else if sim.terrain_objects().objects().contains_key(&id) {
                continue;
            } else if sim.projectiles().get(id).is_some() {
                objects.push(object(ObjectKind::Bullet));
                continue;
            } else if sim.voxel_anims().get(id).is_some() {
                objects.push(object(ObjectKind::Debris));
                continue;
            } else if let Some(light) = frame.scene.spotlights.get(&id) {
                let [x, y, z] = light.coord;
                EffectKind::Spotlight(screen_point(x, y, z))
            } else if let Some(system) = sim.particle_systems().get(id) {
                let at = system.coords();
                EffectKind::ParticleSystem(
                    (!system.in_limbo()).then(|| screen_point(at.x, at.y, at.z)),
                )
            } else if let Some(&place) = particles.get(&id) {
                let point =
                    sim.particle_systems()
                        .display_particle_at(id, place)
                        .map(|(_, particle)| {
                            let at = particle.coords();
                            screen_point(at.x, at.y, at.z)
                        });
                EffectKind::Particle(place, point)
            } else {
                continue;
            };
            effects.push(EffectMember { id, kind });
        }
        Self {
            objects,
            effects,
            particles,
        }
    }

    /// The units, buildings, infantry, aircraft, Bullets and debris.
    pub(crate) fn objects(&self) -> &[ObjectMember] {
        &self.objects
    }

    /// The Anims, spotlights, particle systems and particles.
    pub(crate) fn effects(&self) -> &[EffectMember] {
        &self.effects
    }

    /// Where Display particle `id` is kept, if it is one.
    pub(crate) fn particle_place(&self, id: u64) -> Option<DisplayParticlePlace> {
        self.particles.get(&id).copied()
    }
}

#[cfg(test)]
#[path = "display_members_tests.rs"]
mod tests;
