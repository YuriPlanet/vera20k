//! The Behind indicator Anim a Techno retains (`TechnoClass+0x12C`).

use super::GameEntity;
use crate::sim::anim_class::AnimId;
use std::hash::{Hash, Hasher};

impl GameEntity {
    /// `TechnoClass+0x12C`: the Behind Anim attached while the object stands
    /// on a hiding Cell. The Techno constructor clears it.
    pub fn behind_anim(&self) -> Option<AnimId> {
        self.behind_anim
    }

    /// UpdateBehindIndicator (`0x0070F257`) retains the constructed Anim.
    pub(crate) fn retain_behind_anim(&mut self, anim: AnimId) {
        self.behind_anim = Some(anim);
    }

    /// `TechnoClass::Detach @ 0x00710410` (`0x00710415..0x00710421`) clears
    /// only the matching pointer.
    pub(crate) fn expire_behind_anim(&mut self, anim: AnimId) {
        if self.behind_anim == Some(anim) {
            self.behind_anim = None;
        }
    }

    /// The AI's removal (`0x006FA2F8..0x006FA306`) and the Techno destructor
    /// (`0x006F4667..0x006F4679`) take the Anim for UnInit.
    pub(crate) fn take_behind_anim(&mut self) -> Option<AnimId> {
        self.behind_anim.take()
    }

    /// Folded only while held, so objects without one keep their hash.
    pub(crate) fn hash_behind_anim(&self, hasher: &mut impl Hasher) {
        if let Some(anim) = self.behind_anim {
            b"behind-anim".hash(hasher);
            anim.hash(hasher);
        }
    }
}
