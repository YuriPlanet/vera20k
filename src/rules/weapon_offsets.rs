//! TechnoType-owned firing offsets and retained barrel geometry.
//! ART supplies values at each admitted type read; changing Image does not
//! reconstruct slots that a later ReadINI did not write.
//!
//! Native TechnoSave716DC0/Load7162F0 retain the complete WeaponStruct arrays.
//! Barrel length/thickness have no read in the checked firing/drawing paths;
//! their saved values must still survive each partial ART pass and rebind.
//! RESIDUAL: the separate WeaponStruct+18 TurretLocked firing gate remains
//! documented beside movement/turret.rs; geometry does not implement it.

use super::{flh::Flh, ini_parser::IniSection, object_type::WEAPON_SLOT_COUNT};

/// TechnoCtor7116E6..71173A zeros both arrays, including these signed fields.
/// The pointer is retained by WeaponSlots; this record owns the ART geometry.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
struct WeaponSlotGeometry {
    flh: [i32; 3],
    barrel_length: i32,
    barrel_thickness: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[cfg_attr(not(feature = "ares"), derive(Copy))]
pub(crate) struct WeaponOffsets {
    // TechnoType710AF0, clears7116E6..71173A: both WeaponStruct arrays.
    base: [WeaponSlotGeometry; WEAPON_SLOT_COUNT],
    elite: [WeaponSlotGeometry; WEAPON_SLOT_COUNT],
    alternate: [[i32; 3]; 5],
    #[cfg(feature = "ares")]
    extra_base: Vec<WeaponSlotGeometry>,
    #[cfg(feature = "ares")]
    extra_elite: Vec<WeaponSlotGeometry>,
}

impl WeaponOffsets {
    /// TechnoType::ReadINI715B10..716005, using its retained Image+1F8.
    /// A numbered slot beyond this pass's WeaponCount is left untouched;
    /// every reached elite geometry read defaults to the updated base.
    pub(crate) fn read_pass(&mut self, art: &IniSection, turret_count: i32, weapon_count: i32) {
        if turret_count > 0 {
            let count = usize::try_from(weapon_count).unwrap_or(0);
            #[cfg(not(feature = "ares"))]
            let count = count.min(WEAPON_SLOT_COUNT);
            #[cfg(feature = "ares")]
            {
                // Ares10040020/resize10040AF0: extension tails are destroyed
                // on shrink; regrown WeaponStruct geometry starts at zero.
                // LoadWeapons reads this art block itself, so
                // TechnoTypeClass_LoadFromINI_Weapons2 (10042590, hook
                // 0x715B1F) skips the stock one.
                let extra = count.saturating_sub(WEAPON_SLOT_COUNT);
                self.extra_base.resize(extra, WeaponSlotGeometry::default());
                self.extra_elite
                    .resize(extra, WeaponSlotGeometry::default());
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
            //715D94..715E94 reads both base records before715E94..715F98
            // reads the elites. ClearAllWeapons changes only the pointers.
            for (slot, flh, length, thickness) in [
                (0, "PrimaryFireFLH", "PBarrelLength", "PBarrelThickness"),
                (1, "SecondaryFireFLH", "SBarrelLength", "SBarrelThickness"),
            ] {
                let record = &mut self.base[slot];
                record.flh = art.read_coord3(flh, record.flh);
                record.barrel_length = art.read_int(length, record.barrel_length);
                record.barrel_thickness = art.read_int(thickness, record.barrel_thickness);
            }
            for (slot, flh, length, thickness) in [
                (
                    0,
                    "ElitePrimaryFireFLH",
                    "ElitePBarrelLength",
                    "ElitePBarrelThickness",
                ),
                (
                    1,
                    "EliteSecondaryFireFLH",
                    "EliteSBarrelLength",
                    "EliteSBarrelThickness",
                ),
            ] {
                let record = &mut self.elite[slot];
                let base = self.base[slot];
                record.flh = art.read_coord3(flh, base.flh);
                record.barrel_length = art.read_int(length, base.barrel_length);
                record.barrel_thickness = art.read_int(thickness, base.barrel_thickness);
            }
        }
        //715F9E..716005 always reads all five ports after either arm, with
        // the current base Weapon[0] coordinate as each default.
        for (port, offset) in self.alternate.iter_mut().enumerate() {
            *offset = art.read_coord3(&format!("AlternateFLH{port}"), self.base[0].flh);
        }
    }

    fn read_numbered(
        art: &IniSection,
        slot: usize,
        base: &mut WeaponSlotGeometry,
        elite: &mut WeaponSlotGeometry,
    ) {
        //715B3B..715D6E reads each base/elite record in order. The installed
        // Ares10040020 hook replaces that path with its own integer parser.
        #[cfg(feature = "ares")]
        let read_int = |key: &str, current| {
            crate::rules::ares_switch::not_ares_read(|| art.read_ares_int(key, current))
        };
        #[cfg(not(feature = "ares"))]
        let read_int = |key: &str, current| art.read_int(key, current);
        #[cfg(feature = "ares")]
        {
            base.flh = crate::rules::ares_switch::not_ares_read(|| {
                art.read_ares_coord3(&format!("Weapon{}FLH", slot + 1), base.flh)
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            base.flh = art.read_coord3(&format!("Weapon{}FLH", slot + 1), base.flh);
        }
        base.barrel_length = read_int(
            &format!("Weapon{}BarrelLength", slot + 1),
            base.barrel_length,
        );
        base.barrel_thickness = read_int(
            &format!("Weapon{}BarrelThickness", slot + 1),
            base.barrel_thickness,
        );
        #[cfg(feature = "ares")]
        {
            elite.flh = crate::rules::ares_switch::not_ares_read(|| {
                art.read_ares_coord3(&format!("EliteWeapon{}FLH", slot + 1), base.flh)
            });
        }
        #[cfg(not(feature = "ares"))]
        {
            elite.flh = art.read_coord3(&format!("EliteWeapon{}FLH", slot + 1), base.flh);
        }
        elite.barrel_length = read_int(
            &format!("EliteWeapon{}BarrelLength", slot + 1),
            base.barrel_length,
        );
        elite.barrel_thickness = read_int(
            &format!("EliteWeapon{}BarrelThickness", slot + 1),
            base.barrel_thickness,
        );
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
        coordinate
            .map(|record| record.flh)
            .unwrap_or_default()
            .into()
    }

    /// Ares Prism SupportWeapon reader `100103F0`: `slot` takes AlternateFLH0
    /// (base) or AlternateFLH1 (elite), else that array's slot-0 coordinate.
    #[cfg(feature = "ares")]
    pub(crate) fn copy_support_flh(&mut self, slot: usize, elite: bool) {
        let alternate = self.alternate[usize::from(elite)];
        let slots = if elite {
            &mut self.elite
        } else {
            &mut self.base
        };
        let flh = if alternate == [0; 3] {
            slots[0].flh
        } else {
            alternate
        };
        if let Some(entry) = slots.get_mut(slot) {
            entry.flh = flh;
        }
    }

    /// GetFLH6F3AF5..6F3B21: ports beyond the five stored coordinates are zero.
    pub(crate) fn alternate(&self, port: usize) -> Flh {
        self.alternate.get(port).copied().unwrap_or_default().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
    use crate::util::save_codec;

    fn offsets<'a>(processed: &'a ProcessedRulesLayers, name: &str) -> &'a WeaponOffsets {
        processed
            .weapon_slot_states()
            .find(|(_, id, _)| *id == name)
            .unwrap()
            .2
            .offsets()
    }

    fn pair(record: WeaponSlotGeometry) -> (i32, i32) {
        (record.barrel_length, record.barrel_thickness)
    }

    #[test]
    fn retail_barrels_survive_saved_type_state() {
        let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
            return;
        };
        let processed = RulesLayerStack::new(root)
            .process_with_fixed_art(&art)
            .unwrap();
        let restored: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&processed).unwrap()).unwrap();
        // Physical ARTMD inputs reach the native legacy-slot reader, even
        // for a civilian truck without an active WeaponType pointer.
        for state in [&processed, &restored] {
            for (name, expected) in [
                ("NALASR", (200, 48)),
                ("TRUCKA", (192, 0)),
                ("TRUCKB", (192, 0)),
            ] {
                let slots = state
                    .weapon_slot_states()
                    .find(|(_, id, _)| *id == name)
                    .unwrap()
                    .2;
                assert_eq!(slots.turret_count(), 0);
                assert_eq!(pair(slots.offsets().base[0]), expected, "{name} base");
                assert_eq!(pair(slots.offsets().elite[0]), expected, "{name} elite");
            }
        }
    }

    #[test]
    fn barrel_reads_follow_image_count_and_elite_defaults_across_layers() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=T\n[T]\nImage=A\nWeaponCount=2\nPrimary=GUN\n",
        ));
        let art = IniFile::from_str(
            "[A]\nPBarrelLength=-7\nPBarrelThickness=11\nElitePBarrelLength=-9\n\
             SBarrelLength=13\nSBarrelThickness=17\nEliteSBarrelThickness=19\n\
             Weapon1BarrelLength=999\n\
             [B]\nWeapon1BarrelLength=29\nWeapon1BarrelThickness=31\nEliteWeapon1BarrelLength=37\n\
             Weapon2BarrelLength=41\nWeapon2BarrelThickness=43\nPBarrelLength=999\n\
             [C]\nWeapon1BarrelLength=-5\nweapon1barrelthickness=999\n",
        );
        let first = layers.process_with_fixed_art(&art).unwrap();
        let legacy = offsets(&first, "T");
        assert_eq!(pair(legacy.base[0]), (-7, 11));
        assert_eq!(pair(legacy.elite[0]), (-9, 11));
        assert_eq!(pair(legacy.base[1]), (13, 17));
        assert_eq!(pair(legacy.elite[1]), (13, 19));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[T]\nImage=B\nTurretCount=1\n"),
        );
        let second = layers.process_with_fixed_art(&art).unwrap();
        assert_eq!(pair(offsets(&second, "T").elite[0]), (37, 31));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[T]\nImage=C\nWeaponCount=1\nClearAllWeapons=yes\n"),
        );
        let final_state = layers.process_with_fixed_art(&art).unwrap();
        let restored: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&final_state).unwrap()).unwrap();
        for state in [&final_state, &restored] {
            let slots = state
                .weapon_slot_states()
                .find(|(_, id, _)| *id == "T")
                .unwrap()
                .2;
            assert!(slots.base().iter().all(Option::is_none));
            assert_eq!(pair(slots.offsets().base[0]), (-5, 31));
            assert_eq!(pair(slots.offsets().elite[0]), (-5, 31));
            assert_eq!(pair(slots.offsets().base[1]), (41, 43));
            assert_eq!(pair(slots.offsets().elite[1]), (41, 43));
            assert_eq!(pair(slots.offsets().base[2]), (0, 0));
        }
    }

    #[cfg(feature = "ares")]
    #[test]
    fn ares_extra_barrels_use_the_extension_reader_and_regrow_zeroed() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=T\n[T]\nImage=A\nTurretCount=1\nWeaponCount=19\n",
        ));
        let art = IniFile::from_str(
            "[A]\nWeapon19BarrelLength=$17\nWeapon19BarrelThickness=1Ah\n\
             [B]\nWeapon19BarrelLength=999\n",
        );
        let first = layers.process_with_fixed_art(&art).unwrap();
        let extra = offsets(&first, "T");
        assert_eq!(pair(extra.extra_base[0]), (17, 26));
        assert_eq!(pair(extra.extra_elite[0]), (17, 26));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[T]\nImage=B\nTurretCount=0\n"),
        );
        let legacy = layers.process_with_fixed_art(&art).unwrap();
        assert_eq!(pair(offsets(&legacy, "T").extra_base[0]), (17, 26));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[T]\nTurretCount=1\nWeaponCount=18\n"),
        );
        let shrunk = layers.process_with_fixed_art(&art).unwrap();
        assert!(offsets(&shrunk, "T").extra_base.is_empty());
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[T]\nImage=MISSING\nWeaponCount=19\n"),
        );
        let regrown = layers.process_with_fixed_art(&art).unwrap();
        let restored: ProcessedRulesLayers =
            save_codec::deserialize(&save_codec::serialize(&regrown).unwrap()).unwrap();
        assert_eq!(pair(offsets(&restored, "T").extra_base[0]), (0, 0));
        assert_eq!(pair(offsets(&restored, "T").extra_elite[0]), (0, 0));
    }
}
