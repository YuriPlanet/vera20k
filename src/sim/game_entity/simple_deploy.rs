//! Techno deployment attachment and Unit state access. Native identities and
//! executable controls: tools/spatial_oracle/unit_simple_deploy.{py,json,md}.

use super::GameEntity;
use crate::sim::anim_class::AnimId;
use std::hash::{Hash, Hasher};

/// The deployment attachment: the deploy animation a Unit retains
/// (`TechnoClass+0x130`) and its Jumpjet landing request (`+0x134`). The
/// Techno constructor clears both, and both survive independently of the
/// Unit's deploy bytes (`+0x6E0..+0x6E2`). Only this module's operations
/// write it: the animation is retained (`0x00739BDC`, `0x00739DC0`), expired
/// (`0x00710443..0x0071044E`, `0x0070794C`) and taken for UnInit
/// (`0x006F467F`); the landing is requested (`0x00739B2C`) and cleared by
/// the Jumpjet (`0x0054CA75`, `0x0054B46D`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeployAttachment {
    deploy_anim: Option<AnimId>,
    landing_for_deploy: bool,
}

impl GameEntity {
    /// Unit746DB0: either transition byte, independent of deployed6E0.
    pub fn unit_deploying(&self) -> bool {
        self.mission_leaf.as_unit().is_some_and(|leaf| {
            leaf.deploy_begin_active() != 0 || leaf.deploy_reverse_active() != 0
        })
    }

    pub fn deploy_anim(&self) -> Option<AnimId> {
        self.deploy_attachment.deploy_anim
    }

    ///739BDC/739DC0 retain the constructed Anim until pointer expiry.
    pub(crate) fn retain_deploy_anim(&mut self, anim: AnimId) {
        self.deploy_attachment.deploy_anim = Some(anim);
    }

    /// Techno710410 (710443..71044E) /70794C clear only the matching pointer.
    pub(crate) fn expire_deploy_anim(&mut self, anim: AnimId) {
        if self.deploy_attachment.deploy_anim == Some(anim) {
            self.deploy_attachment.deploy_anim = None;
        }
    }

    /// Techno destructor6F467F takes the retained animation for UnInit.
    pub(crate) fn take_deploy_anim(&mut self) -> Option<AnimId> {
        self.deploy_attachment.deploy_anim.take()
    }

    pub fn landing_for_deploy(&self) -> bool {
        self.deploy_attachment.landing_for_deploy
    }

    /// The deploy's landing request (`0x00739B2C`).
    pub(crate) fn request_landing_for_deploy(&mut self) {
        self.deploy_attachment.landing_for_deploy = true;
    }

    /// The Jumpjet's clear of the landing request: its touchdown
    /// (`0x0054CA75`) and `Move_To`'s lift (`0x0054B46D`).
    pub(crate) fn clear_landing_for_deploy(&mut self) {
        self.deploy_attachment.landing_for_deploy = false;
    }

    /// A fixture's landing request.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_landing_for_deploy(&mut self, landing: bool) {
        self.deploy_attachment.landing_for_deploy = landing;
    }

    pub(crate) fn hash_deploy_attachment(&self, hasher: &mut impl Hasher) {
        self.deploy_attachment.deploy_anim.hash(hasher);
        self.deploy_attachment.landing_for_deploy.hash(hasher);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_unit_simple_deploy_for_test(&mut self, deployed: bool, begin: bool, reverse: bool) {
        self.mission_leaf.set_unit_deployed(u8::from(deployed));
        self.mission_leaf
            .set_unit_deploy_begin_active(u8::from(begin));
        self.mission_leaf
            .set_unit_deploy_reverse_active(u8::from(reverse));
    }
}
