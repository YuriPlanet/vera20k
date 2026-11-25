//! What module actions leave held between frames, beside the modifiers:
//! the timed conversions `Convert` will undo and the house type list
//! `AllowType` writes (RFC 0001 appendix 5.3). [`ActionHeld`] is the one
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

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::TechnoTypeReference;
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
}

impl ActionHeld {
    pub(super) fn is_empty(&self) -> bool {
        self.conversions.is_empty() && self.forbidden.is_empty()
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

impl Simulation {
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
