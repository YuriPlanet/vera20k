//! TechnoType-owned firing offsets in forward/lateral/height leptons.
//! ART supplies values at each admitted type read; changing Image does not
//! reconstruct slots that a later ReadINI did not write.
//!
//! RESIDUAL: WeaponStruct barrel length/thickness and TurretLocked remain
//! outside this FLH owner. Nonzero authored barrel/lock settings require the
//! original mechanism too; the aim/firing-gate risk is documented beside
//! movement/turret.rs's WeaponStruct+18 read. Extending indices does not
//! establish parity for those separate settings.

use super::{flh::Flh, ini_parser::IniSection, object_type::WEAPON_SLOT_COUNT};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(not(feature = "ares"), derive(Copy))]
pub(crate) struct WeaponOffsets {
    // TechnoType710AF0, clears7116E6..71173A: both WeaponStruct arrays.
    base: [[i32; 3]; WEAPON_SLOT_COUNT],
    elite: [[i32; 3]; WEAPON_SLOT_COUNT],
    alternate: [[i32; 3]; 5],
    #[cfg(feature = "ares")]
    extra_base: Vec<[i32; 3]>,
    #[cfg(feature = "ares")]
    extra_elite: Vec<[i32; 3]>,
}

impl WeaponOffsets {
    /// TechnoType::ReadINI715B10..716005, using its retained Image+1F8.
    /// A numbered slot beyond this pass's WeaponCount is left untouched;
    /// every reached elite coordinate read defaults to the updated base.
    pub(crate) fn read_pass(&mut self, art: &IniSection, turret_count: i32, weapon_count: i32) {
        if turret_count > 0 {
            let count = usize::try_from(weapon_count).unwrap_or(0);
            #[cfg(not(feature = "ares"))]
            let count = count.min(WEAPON_SLOT_COUNT);
            #[cfg(feature = "ares")]
            {
                // Ares10040020/resize10040AF0: extension tails are destroyed
                // on shrink; regrown WeaponStruct coordinates start at zero.
                let extra = count.saturating_sub(WEAPON_SLOT_COUNT);
                self.extra_base.resize(extra, [0; 3]);
                self.extra_elite.resize(extra, [0; 3]);
            }
            for slot in 0..count {
                if slot < WEAPON_SLOT_COUNT {
                    Self::read_numbered(art, slot, &mut self.base[slot], &mut self.elite[slot]);
                } else {
                    #[cfg(feature = "ares")]
                    Self::read_numbered(
                        art,
                        slot,
                        &mut self.extra_base[slot - WEAPON_SLOT_COUNT],
                        &mut self.extra_elite[slot - WEAPON_SLOT_COUNT],
                    );
                }
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

    fn read_numbered(art: &IniSection, slot: usize, base: &mut [i32; 3], elite: &mut [i32; 3]) {
        #[cfg(feature = "ares")]
        {
            //10040020 copies base FLH before the optional elite coordinate read.
            *base = art.read_ares_coord3(&format!("Weapon{}FLH", slot + 1), *base);
            *elite = art.read_ares_coord3(&format!("EliteWeapon{}FLH", slot + 1), *base);
        }
        #[cfg(not(feature = "ares"))]
        {
            *base = art.read_coord3(&format!("Weapon{}FLH", slot + 1), *base);
            *elite = art.read_coord3(&format!("EliteWeapon{}FLH", slot + 1), *base);
        }
    }

    /// GetFLH6F3B28's already-qualified base/elite WeaponStruct coordinate.
    /// Count governs reads, not the lifetime or runtime lookup of a slot.
    pub(crate) fn weapon(&self, index: i32, elite: bool) -> Flh {
        let Some(slot) = usize::try_from(index).ok() else {
            return Flh::default();
        };
        let coordinate = if elite {
            self.elite.get(slot)
        } else {
            self.base.get(slot)
        };
        #[cfg(feature = "ares")]
        let coordinate = coordinate.or_else(|| {
            let extra = slot.checked_sub(WEAPON_SLOT_COUNT)?;
            if elite {
                self.extra_elite.get(extra)
            } else {
                self.extra_base.get(extra)
            }
        });
        coordinate.copied().unwrap_or_default().into()
    }

    /// GetFLH6F3AF5..6F3B21: ports beyond the five stored coordinates are zero.
    pub(crate) fn alternate(&self, port: usize) -> Flh {
        self.alternate.get(port).copied().unwrap_or_default().into()
    }
}
