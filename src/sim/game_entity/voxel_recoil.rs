//! One owner for Techno's turret/barrel recoil (+3D8/+3F8), copied from type
//! controls by InitManagers6F4277..6F42E3, armed after successful FireAt6FF0B7,
//! and advanced before ChargeTurret by Techno AI6FA4D1..6FA4FB.
//!
//! Recoil changes drawing only: no RNG, clock timer, detach, targeting or shot
//! coordinate consumer. It is saved for visual continuity and excluded from
//! simulation hashes, as is the neighboring turret animation frame. Use f32
//! presentation arithmetic; stock 8/3 travel differs from native x87 chop by
//! at most 0.00002 model units in the pinned histories (recoil.json). State and
//! countdown transitions match exactly. No gameplay math reads these values.
use super::GameEntity;
use crate::rules::recoil::{RecoilConfig, RecoilControl};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Recoil {
    control: RecoilControl,
    step: f32,
    travel: f32,
    state: i32,
    left: i32,
}

impl Recoil {
    fn new(control: RecoilControl) -> Self {
        Self {
            control,
            step: 0.0,
            travel: 0.0,
            state: 0,
            left: 0,
        }
    }

    /// RecoilData70ECE0; FireAt6FF0DD..6FF15B inlines the same state change.
    /// A repeated shot preserves current displacement, even during recovery.
    fn fire(&mut self) {
        let [travel, compress, _, _] = self.control.parameters();
        if travel != 0 {
            self.state = 1;
            self.left = compress.max(1);
            self.step = (f64::from(travel) / f64::from(self.left)) as f32;
        }
    }

    /// RecoilData70ED10. A one-frame hold transitions to recovery immediately.
    fn update(&mut self) {
        if self.state == 0 {
            return;
        }
        self.left = self.left.wrapping_sub(1);
        self.travel += self.step;
        if self.left > 0 {
            return;
        }
        let [travel, _, recover, hold] = self.control.parameters();
        match self.state {
            1 => {
                self.state = 2;
                self.left = hold;
                self.step = 0.0;
                if hold > 1 {
                    return;
                }
            }
            2 => {}
            3 => {
                self.state = 0;
                self.travel = 0.0;
                return;
            }
            _ => return,
        }
        self.state = 3;
        self.left = recover.max(1);
        self.step = (-f64::from(travel) / f64::from(self.left)) as f32;
    }
}

/// Allocated only for types enabling TurretRecoil. The two native components
/// have one entity-owned lifetime; despawning the entity drops them together.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct VoxelRecoil {
    parts: [Recoil; 2],
}

impl GameEntity {
    pub(crate) fn initialize_voxel_recoil(&mut self, config: RecoilConfig) {
        self.voxel_recoil = config.enabled().then(|| {
            Box::new(VoxelRecoil {
                parts: config.controls().map(Recoil::new),
            })
        });
    }

    pub(crate) fn fire_voxel_recoil(&mut self, has_turret: bool) {
        if has_turret && let Some(recoil) = self.voxel_recoil.as_mut() {
            for part in &mut recoil.parts {
                part.fire();
            }
        }
    }

    pub(crate) fn update_voxel_recoil(&mut self) {
        if let Some(recoil) = self.voxel_recoil.as_mut() {
            for part in &mut recoil.parts {
                part.update();
            }
        }
    }

    /// Immutable drawing inputs: turret/barrel displacement and cache bypass.
    pub fn voxel_recoil(&self) -> ([f32; 2], bool) {
        self.voxel_recoil
            .as_ref()
            .map_or(([0.0; 2], false), |recoil| {
                (
                    recoil.parts.each_ref().map(|part| part.travel),
                    recoil.parts.iter().any(|part| part.state != 0),
                )
            })
    }
}

#[cfg(test)]
#[path = "voxel_recoil_tests.rs"]
mod tests;
