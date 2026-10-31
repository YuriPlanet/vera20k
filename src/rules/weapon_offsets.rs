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

/// Phobos `Nullable<CoordStruct>::Read` on one ART key: the `INI_EX` string
/// reader (2048) then a complete `%d,%d,%d` scan. Absent, blank, `<default>`
/// and rejected text all leave the nullable unset.
#[cfg(feature = "phobos")]
fn nullable_flh(art: &IniSection, key: &str) -> Option<[i32; 3]> {
    art.read_int_fields_complete::<3>(key, 2048)
}

/// Phobos `TechnoTypeExt::ParseBurstFLHs` (`src/Ext/TechnoType/Body.cpp`):
/// `pPrefixTag` + `PrimaryFire`/`SecondaryFire` when the type is not a
/// multi-weapon turret, else `pPrefixTag + WeaponN` with the live WeaponCount,
/// each with its `Elite` variant. The run stops at the first index whose two
/// keys are both absent; a missing elite coordinate copies the normal one, and
/// a normal key that is absent while the elite key is present reads as zero
/// (the null `Nullable::Get`). Rows are append-only, as the native owner never
/// clears them between reads.
#[cfg(feature = "phobos")]
fn read_burst_rows(
    art: &IniSection,
    turret_count: i32,
    weapon_count: i32,
    prefix: &str,
    rows: &mut BurstFlhRows,
) {
    let multi_weapon = turret_count > 0 && weapon_count > 0;
    let slots = if multi_weapon {
        usize::try_from(weapon_count).unwrap_or(0)
    } else {
        2
    };
    rows.resize_slots(slots);
    for slot in 0..slots {
        let base_key = if multi_weapon {
            format!("{prefix}Weapon{}FLH", slot + 1)
        } else if slot == 0 {
            format!("{prefix}PrimaryFireFLH")
        } else {
            format!("{prefix}SecondaryFireFLH")
        };
        let elite_key = format!("Elite{base_key}");
        let mut burst = 0usize;
        loop {
            let base = art.read_int_fields_complete::<3>(&format!("{base_key}.Burst{burst}"), 2048);
            let elite =
                art.read_int_fields_complete::<3>(&format!("{elite_key}.Burst{burst}"), 2048);
            if base.is_none() && elite.is_none() {
                break;
            }
            let base = base.unwrap_or([0; 3]);
            rows.normal[slot].push(base);
            rows.elite[slot].push(elite.unwrap_or(base));
            burst += 1;
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "weapon_offsets_tests.rs"]
mod tests;

/// One `ParseBurstFLHs` key set: the normal and elite rows of one firing
/// offset family.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BurstFlhRows {
    normal: Vec<Vec<[i32; 3]>>,
    elite: Vec<Vec<[i32; 3]>>,
}

#[cfg(feature = "phobos")]
impl BurstFlhRows {
    fn is_empty(&self) -> bool {
        self.normal.is_empty()
    }

    fn resize_slots(&mut self, slots: usize) {
        self.normal.resize(slots, Vec::new());
        self.elite.resize(slots, Vec::new());
    }

    /// The rows, for the rules identity fold.
    pub(crate) fn normal(&self) -> &[Vec<[i32; 3]>] {
        &self.normal
    }

    pub(crate) fn elite(&self) -> &[Vec<[i32; 3]>] {
        &self.elite
    }

    /// Phobos `GetBurstFLH`'s indexed read; `None` past the authored run.
    fn offset(&self, slot: usize, elite: bool, burst_index: u8) -> Option<Flh> {
        let rows = if elite { &self.elite } else { &self.normal };
        rows.get(slot)?
            .get(usize::from(burst_index))
            .copied()
            .map(Flh::from)
    }
}

/// Phobos `InfantryClass` state that selects a burst row set: deployed wins
/// over prone, and neither is consulted for a non-infantry TechnoType.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InfantryFlhState {
    Ordinary,
    Deployed,
    Crouched,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct WeaponOffsets {
    // TechnoType710AF0, clears7116E6..71173A: both WeaponStruct arrays.
    base: [[i32; 3]; WEAPON_SLOT_COUNT],
    elite: [[i32; 3]; WEAPON_SLOT_COUNT],
    alternate: [[i32; 3]; 5],
    /// Phobos `TechnoTypeExt::ParseBurstFLHs` rows, one per weapon slot:
    /// `(Elite)(PrimaryFire|SecondaryFire|WeaponN)FLH.BurstN`. Each reached
    /// ART read appends that pass's contiguous run; an authored elite value
    /// defaults to the normal one.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    burst: BurstFlhRows,
    /// Phobos `InfantryTypeExt`'s `Deployed`/`Prone` copies of both the burst
    /// rows and the plain `(Primary|Secondary)FireFLH` overrides.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    deployed_burst: BurstFlhRows,
    #[cfg(feature = "phobos")]
    #[serde(default)]
    prone_burst: BurstFlhRows,
    #[cfg(feature = "phobos")]
    #[serde(default)]
    deployed_simple: [Option<[i32; 3]>; 2],
    #[cfg(feature = "phobos")]
    #[serde(default)]
    prone_simple: [Option<[i32; 3]>; 2],
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
        #[cfg(feature = "phobos")]
        read_burst_rows(art, turret_count, weapon_count, "", &mut self.burst);
    }

    /// Phobos `InfantryTypeExt::LoadFromINIFile`'s ART reads, reached only for
    /// an InfantryType: the two state key sets and their burst rows.
    #[cfg(feature = "phobos")]
    pub(crate) fn read_infantry_state_pass(
        &mut self,
        art: &IniSection,
        turret_count: i32,
        weapon_count: i32,
    ) {
        read_burst_rows(
            art,
            turret_count,
            weapon_count,
            "Deployed",
            &mut self.deployed_burst,
        );
        read_burst_rows(
            art,
            turret_count,
            weapon_count,
            "Prone",
            &mut self.prone_burst,
        );
        for slot in 0..2 {
            let prefix = if slot == 0 {
                "PrimaryFire"
            } else {
                "SecondaryFire"
            };
            self.deployed_simple[slot] = nullable_flh(art, &format!("Deployed{prefix}FLH"));
            self.prone_simple[slot] = nullable_flh(art, &format!("Prone{prefix}FLH"));
        }
    }

    /// Phobos `InfantryExt::GetSimpleFLH`: the deployed or prone override of
    /// the primary/secondary firing offset. Deployed wins; any other slot or
    /// state has no override.
    #[cfg(feature = "phobos")]
    pub(crate) fn state_flh(&self, index: i32, deployed: bool, crawling: bool) -> Option<Flh> {
        let slot = match index {
            0 | 1 => index as usize,
            _ => return None,
        };
        let rows = if deployed {
            &self.deployed_simple
        } else if crawling {
            &self.prone_simple
        } else {
            return None;
        };
        rows[slot].map(Flh::from)
    }

    /// Phobos `GetBurstFLH`: the elite rows and the infantry state pick the
    /// row set, an empty state set falls back to the type's own rows, and the
    /// frozen `CurrentBurstIndex` selects within it. `None` past the authored
    /// run, where `GetFLH` keeps the slot's FLH.
    #[cfg(feature = "phobos")]
    pub(crate) fn burst(
        &self,
        index: i32,
        elite: bool,
        state: InfantryFlhState,
        burst_index: u8,
    ) -> Option<Flh> {
        let slot = usize::try_from(index).ok()?;
        let set: &BurstFlhRows = match state {
            InfantryFlhState::Deployed if !self.deployed_burst.is_empty() => &self.deployed_burst,
            InfantryFlhState::Crouched if !self.prone_burst.is_empty() => &self.prone_burst,
            _ => &self.burst,
        };
        set.offset(slot, elite, burst_index)
    }

    /// Every authored row set, for the rules identity fold.
    #[cfg(feature = "phobos")]
    pub(crate) fn burst_row_sets(&self) -> [&BurstFlhRows; 3] {
        [&self.burst, &self.deployed_burst, &self.prone_burst]
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
