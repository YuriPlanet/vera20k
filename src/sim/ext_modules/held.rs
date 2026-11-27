//! What module actions leave held between frames, beside the modifiers:
//! timed conversions, house type restrictions, kept animations and visual overrides.
//! [`ActionHeld`] is the one
//! owner; it is saved with the modules and hashed when it holds anything.
//!
//! - A timed `Convert` (`value2` frames) remembers the object's type before
//!   its first conversion; the module phase of the frame the time is up
//!   converts it back through the same owner (Ares ConvertToType
//!   `0x10043650`). A later timed conversion of the same object restarts the
//!   time and keeps that first type; a permanent one drops the reversion.
//!   The object leaving the game drops it too.
//! - `AllowType` with 0 forbids a TechnoType to one house: CanBuild
//!   (`production::can_build`) refuses it there as it refuses a room ban,
//!   for the sidebar, the production event and the computer's choosers
//!   alike. 1 lifts the module's forbid; it does not grant a type the
//!   house's prerequisites, tech level or build limit refuse.
//! - `AttachAnim` with `value2` other than 0 keeps its animation looping on
//!   the object, as a Phobos AttachEffect keeps its own (`hold_loops`, the
//!   Ares attached-animation byte 255 at Anim+0x195): a positive count
//!   until the module phase of the frame it runs out, a negative one until
//!   `DetachAnim` removes it. Each module phase keeps exactly the wanted
//!   animations alive, as Phobos' AttachEffect `CreateAnim`/`KillAnim` do
//!   ([`Simulation::maintain_ext_module_anims`]): it destroys one whose time
//!   is up or whose object left the game, takes one off while its object is
//!   in limbo (a transport) or fully cloaked, so it gives no hidden object
//!   away, and puts it back once the object shows again or the engine
//!   destroyed it. It follows the object's coordinates.
//! - A `DoorOpen` write holds how a war factory's or gate's door is drawn,
//!   open (1) or shut (0), until -1 lets the door show its own state again.
//!   Only the picture follows it ([`Simulation::ext_module_door`]): the door
//!   itself, production, the exit and a gate's passage keep running as they
//!   would.
//! - `ImageOverride` draws one object as another TechnoType of its
//!   category, building excepted, without changing what it is: the draw
//!   reads the type name ([`Simulation::ext_module_image`]) where it picks
//!   the model or the infantry image. A write of 0 drops it, and so does the
//!   object leaving the game; a conversion keeps it, as a conversion stays
//!   in the category.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::TechnoTypeReference;
use crate::sim::anim_class::AnimId;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// A pending reversion of a timed conversion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Reversion {
    /// The type the object had before its first timed conversion.
    original: TechnoTypeReference,
    /// The frame whose module phase converts it back.
    at: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ActionHeld {
    /// By object creation serial.
    conversions: BTreeMap<u64, Reversion>,
    /// (house array index, TechnoType ID) pairs a module forbade.
    forbidden: BTreeSet<(u32, u32)>,
    /// The animations `AttachAnim` keeps, in attach order.
    #[serde(default)]
    anims: Vec<HeldAnim>,
    /// `DoorOpen` writes: how each building's door is drawn, by creation
    /// serial.
    #[serde(default)]
    doors: BTreeMap<u64, bool>,
    /// `ImageOverride`: the TechnoType each object is drawn as, by
    /// creation serial.
    #[serde(default)]
    images: BTreeMap<u64, String>,
}

/// An animation a module keeps on an object.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct HeldAnim {
    /// The object's creation serial.
    object: u64,
    /// The AnimType array index `AttachAnim` took, which `DetachAnim` names.
    anim_type: i32,
    origin: super::ModuleSource,
    /// The frame whose module phase destroys it; `None` until `DetachAnim`.
    until: Option<u32>,
    /// The live animation; `None` while its object is hidden.
    anim: Option<AnimId>,
}

impl ActionHeld {
    pub(super) fn is_empty(&self) -> bool {
        self.conversions.is_empty()
            && self.forbidden.is_empty()
            && self.doors.is_empty()
            && self.images.is_empty()
            && self.anims.is_empty()
    }


    /// A held animation lives in engine memory across module callbacks. Bound
    /// the total, including attachments waiting for their owner to reappear.
    pub(super) fn can_hold_anim(&self) -> bool {
        self.anims.len() < 65_536
    }

    /// A `DoorOpen` write: draw `object`'s door open or shut, or (`None`)
    /// as it is.
    pub(super) fn hold_door(&mut self, object: u64, open: Option<bool>) {
        match open {
            Some(open) => self.doors.insert(object, open),
            None => self.doors.remove(&object),
        };
    }

    /// `ImageOverride`: draws `object` as `image`, or as itself with `None`.
    pub(super) fn set_image(&mut self, object: u64, image: Option<String>) {
        match image {
            Some(image) => self.images.insert(object, image),
            None => self.images.remove(&object),
        };
    }

    /// After `AttachAnim` attached `anim` to `object`: kept for `frames`
    /// frames from `frame`, or until `DetachAnim` when `frames` is negative.
    pub(super) fn hold_anim(
        &mut self,
        anim: Option<AnimId>,
        object: u64,
        anim_type: i32,
        origin: super::ModuleSource,
        frames: i64,
        frame: u32,
    ) {
        let until = (frames >= 0)
            .then(|| frame.saturating_add(u32::try_from(frames).unwrap_or(u32::MAX)));
        self.anims.push(HeldAnim {
            object,
            anim_type,
            origin,
            until,
            anim,
        });
    }

    /// `DetachAnim`: takes the animations `origin` keeps on `object`, those
    /// of AnimType `anim_type` only unless it is -1; the live ones' IDs.
    pub(super) fn take_anims_of(
        &mut self,
        object: u64,
        anim_type: i32,
        origin: super::ModuleSource,
    ) -> Vec<AnimId> {
        let mut taken = Vec::new();
        self.anims.retain(|held| {
            let take = held.object == object
                && held.origin == origin
                && (anim_type == -1 || held.anim_type == anim_type);
            if take {
                taken.extend(held.anim);
            }
            !take
        });
        taken
    }

    /// After `Convert` turned `object` from `original`: a timed conversion
    /// (`frames` > 0) undoes at `frame + frames`, a permanent one (0) keeps
    /// the new type.
    pub(super) fn note_conversion(
        &mut self,
        object: u64,
        original: TechnoTypeReference,
        frames: u32,
        frame: u32,
    ) {
        if frames == 0 {
            self.conversions.remove(&object);
            return;
        }
        let at = frame.saturating_add(frames);
        self.conversions
            .entry(object)
            .and_modify(|reversion| reversion.at = at)
            .or_insert(Reversion { original, at });
    }

    /// `AllowType`: forbids or allows TechnoType `type_id` to house `house`.
    pub(super) fn allow_type(&mut self, house: u32, type_id: u32, allow: bool) {
        if allow {
            self.forbidden.remove(&(house, type_id));
        } else {
            self.forbidden.insert((house, type_id));
        }
    }

    pub(super) fn forget_object(&mut self, object: u64) {
        self.images.remove(&object);
        self.doors.remove(&object);
        self.conversions.remove(&object);
    }

    /// The reversions due at `frame`, taken out, in object order.
    fn take_due(&mut self, frame: u32) -> Vec<(u64, TechnoTypeReference)> {
        let due: Vec<u64> = self
            .conversions
            .iter()
            .filter(|(_, reversion)| reversion.at <= frame)
            .map(|(object, _)| *object)
            .collect();
        due.into_iter()
            .filter_map(|object| {
                let reversion = self.conversions.remove(&object)?;
                Some((object, reversion.original))
            })
            .collect()
    }
}

pub(super) fn held_anim_hidden(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity.lifecycle.in_limbo()
        || entity.cloak.as_ref().is_some_and(|cloak| cloak.is_fully_cloaked())
}

impl Simulation {
    /// How a module's `DoorOpen` write has object `id`'s door drawn.
    pub fn ext_module_door(&self, id: u64) -> Option<bool> {
        self.ext_modules.held.doors.get(&id).copied()
    }

    /// Every TechnoType a module's `ImageOverride` draws some object as.
    pub fn ext_module_images(&self) -> impl Iterator<Item = &str> {
        self.ext_modules.held.images.values().map(String::as_str)
    }

    /// The TechnoType a module's `ImageOverride` draws object `id` as.
    pub fn ext_module_image(&self, id: u64) -> Option<&str> {
        self.ext_modules.held.images.get(&id).map(String::as_str)
    }

    /// The module phase's kept animations, in attach order: one whose time
    /// is up or whose object left the game is destroyed and dropped; one
    /// whose object is in limbo or fully cloaked is destroyed and kept; one
    /// that is gone or no longer the object's while it shows is made again.
    pub(super) fn maintain_ext_module_anims(&mut self, rules: &RuleSet) {
        if self.ext_modules.held.anims.is_empty() {
            return;
        }
        let frame = self.session.binary_frame();
        let mut held = std::mem::take(&mut self.ext_modules.held.anims);
        held.retain_mut(|kept| {
            let entity = self.substrate.entities.get(kept.object);
            let gone = kept.until.is_some_and(|until| until <= frame)
                || entity.is_none_or(|entity| !entity.lifecycle.object_alive());
            let hidden = entity.is_some_and(held_anim_hidden);
            let live = kept.anim.filter(|&anim| {
                self.substrate
                    .anims
                    .get(anim)
                    .is_some_and(|live| live.owner_entity() == Some(kept.object))
            });
            if gone || hidden {
                if let Some(anim) = live {
                    self.destroy_anim_with_context(anim, rules);
                }
                kept.anim = None;
                return !gone;
            }
            if live.is_none() {
                kept.anim = self.spawn_attached_anim(rules, kept.object, kept.anim_type, true);
            }
            true
        });
        self.ext_modules.held.anims = held;
    }

    /// One pass (or held loops) of AnimType `anim_type` (its array index) attached to
    /// `object` as team script action 51 (`0x006EF610`) attaches one: delay
    /// 0, the type's loops, draw flags `0x600`, at the object's GetCoords.
    /// `None` for an object not on the map or an animation that does not
    /// construct.
    pub(super) fn spawn_attached_anim(
        &mut self,
        rules: &RuleSet,
        object: u64,
        anim_type: i32,
        hold: bool,
    ) -> Option<AnimId> {
        let type_name = rules.anim_type_runtime_key_by_index(anim_type)?;
        let placed = self.substrate.entities.get(object).is_some_and(|entity| {
            entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo()
        });
        let world = placed.then(|| self.anim_owner_coords(object)).flatten()?;
        let type_id = self.interner.intern(&type_name);
        let (rx, ry, sub_x, sub_y, level) = world.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            loop_count: 1,
            draw_flags: 0x600,
            owner: Some(object),
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_id, rx, ry, sub_x, sub_y, level,
            )
        };
        let anim = self.spawn_anim_at_world(rules, descriptor, world).ok()?;
        if hold && let Some(live) = self.substrate.anims.get_mut(anim) {
            live.hold_loops();
        }
        Some(anim)
    }

    /// Destroys a module-kept animation the engine has not destroyed yet.
    pub(super) fn destroy_held_anim(&mut self, anim: AnimId, rules: &RuleSet) {
        if self.substrate.anims.get(anim).is_some() {
            self.destroy_anim_with_context(anim, rules);
        }
    }

    /// Whether a module's `AllowType` forbade `obj` to `owner`.
    pub(crate) fn ext_module_forbids(
        &self,
        rules: &RuleSet,
        owner: InternedId,
        obj: &ObjectType,
    ) -> bool {
        let forbidden = &self.ext_modules.held.forbidden;
        if forbidden.is_empty() {
            return false;
        }
        let house = self
            .houses
            .index_of(owner)
            .and_then(|index| u32::try_from(index).ok());
        match (house, rules.type_handle(&obj.id)) {
            (Some(house), Some(handle)) => forbidden.contains(&(house, handle.0)),
            _ => false,
        }
    }

    /// The module phase's reversions: each timed conversion whose time is
    /// up converts back, in object order. One that no longer converts (the
    /// object changed class meanwhile) is dropped.
    pub(super) fn revert_ext_module_conversions(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        if self.ext_modules.held.conversions.is_empty() {
            return;
        }
        let frame = self.session.binary_frame();
        for (object, original) in self.ext_modules.held.take_due(frame) {
            if !self.convert_techno_type(object, &original, rules, frame_effects) {
                log::info!(target: "ext_module", "timed Convert of {object} could not revert");
            }
        }
    }
}
