//! TechnoType-owned firing offsets in forward/lateral/height leptons.
//! ART supplies values at each admitted type read; changing Image does not
//! reconstruct slots that a later ReadINI did not write.

use super::{flh::Flh, ini_parser::IniSection, object_type::WEAPON_SLOT_COUNT};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct WeaponOffsets {
    // TechnoType710AF0, clears7116E6..71173A: both WeaponStruct arrays.
    base: [[i32; 3]; WEAPON_SLOT_COUNT],
    elite: [[i32; 3]; WEAPON_SLOT_COUNT],
    alternate: [[i32; 3]; 5],
}

impl WeaponOffsets {
    /// TechnoType::ReadINI715B10..716005, using its retained Image+1F8.
    /// A numbered slot beyond this pass's WeaponCount is left untouched;
    /// every reached elite coordinate read defaults to the updated base.
    pub(crate) fn read_pass(&mut self, art: &IniSection, turret_count: i32, weapon_count: i32) {
        if turret_count > 0 {
            // Native does not bound malformed counts. Retain the existing
            // retail-only safety boundary rather than overwriting memory.
            let count = usize::try_from(weapon_count)
                .unwrap_or(0)
                .min(WEAPON_SLOT_COUNT);
            for slot in 0..count {
                self.base[slot] =
                    art.read_coord3(&format!("Weapon{}FLH", slot + 1), self.base[slot]);
                self.elite[slot] =
                    art.read_coord3(&format!("EliteWeapon{}FLH", slot + 1), self.base[slot]);
            }
        } else {
            for (slot, key, elite_key) in [
                (0, "PrimaryFireFLH", "ElitePrimaryFireFLH"),
                (1, "SecondaryFireFLH", "EliteSecondaryFireFLH"),
            ] {
                self.base[slot] = art.read_coord3(key, self.base[slot]);
                self.elite[slot] = art.read_coord3(elite_key, self.base[slot]);
            }
        }
        //715F9E..716005 always reads all five ports after either arm, with
        // the current base Weapon[0] coordinate as each default.
        for (port, offset) in self.alternate.iter_mut().enumerate() {
            *offset = art.read_coord3(&format!("AlternateFLH{port}"), self.base[0]);
        }
    }

    /// GetFLH6F3B28's already-qualified base/elite WeaponStruct coordinate.
    /// Count governs reads, not the lifetime or runtime lookup of a slot.
    pub(crate) fn weapon(&self, index: i32, elite: bool) -> Flh {
        usize::try_from(index)
            .ok()
            .and_then(|slot| {
                if elite {
                    self.elite.get(slot)
                } else {
                    self.base.get(slot)
                }
            })
            .copied()
            .unwrap_or_default()
            .into()
    }

    /// GetFLH6F3AF5..6F3B21: ports beyond the five stored coordinates are zero.
    pub(crate) fn alternate(&self, port: usize) -> Flh {
        self.alternate.get(port).copied().unwrap_or_default().into()
    }
}
