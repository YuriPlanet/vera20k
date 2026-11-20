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

/// Phobos `Nullable<CoordStruct>::Read` on one ART key: the `INI_EX` string
/// reader (2048) then a complete `%d,%d,%d` scan. Absent, blank, `<default>`
/// and rejected text all leave the nullable unset.
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


/// One `ParseBurstFLHs` key set: the normal and elite rows of one firing
/// offset family.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BurstFlhRows {
    normal: Vec<Vec<[i32; 3]>>,
    elite: Vec<Vec<[i32; 3]>>,
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InfantryFlhState {
    Ordinary,
    Deployed,
    Crouched,
}


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
pub(crate) struct WeaponOffsets {
    // TechnoType710AF0, clears7116E6..71173A: both WeaponStruct arrays.
    base: [WeaponSlotGeometry; WEAPON_SLOT_COUNT],
    elite: [WeaponSlotGeometry; WEAPON_SLOT_COUNT],
    /// GetFLH6F3AF5's `AlternateFLH0..4`, plus Phobos's sixth
    /// `AlternateFLH5` entry (`Ext/TechnoType/Body.cpp:1368-1384`): ports 0..4
    /// default to `Weapon[0].FLH`, and the sixth is appended only when
    /// authored, otherwise the stock negative-index read stays at zero.
    alternate: [[i32; 3]; 6],
    // Ares extension slots past the original eighteen; only a read the Ares
    // way fills them.
    extra_base: Vec<WeaponSlotGeometry>,
    extra_elite: Vec<WeaponSlotGeometry>,
    #[serde(default)]
    alternate_flh_on_turret: Option<bool>,
    #[serde(default)]
    alternate_flh_apply_vehicle: Option<bool>,
    #[serde(default)]
    burst: BurstFlhRows,
    #[serde(default)]
    deployed_burst: BurstFlhRows,
    #[serde(default)]
    prone_burst: BurstFlhRows,
    #[serde(default)]
    deployed_simple: [Option<[i32; 3]>; 2],
    #[serde(default)]
    prone_simple: [Option<[i32; 3]>; 2],
}

impl WeaponOffsets {
    /// TechnoType::ReadINI715B10..716005, using its retained Image+1F8.
    /// A numbered slot beyond this pass's WeaponCount is left untouched;
    /// every reached elite geometry read defaults to the updated base.
    pub(crate) fn read_pass(&mut self, art: &IniSection, turret_count: i32, weapon_count: i32) {
        if turret_count > 0 {
            let count = usize::try_from(weapon_count).unwrap_or(0);
            let count = if crate::rules::ares_switch::ares_way() {
                // Ares10040020/resize10040AF0: extension tails are destroyed
                // on shrink; regrown WeaponStruct geometry starts at zero.
                // LoadWeapons reads this art block itself, so
                // TechnoTypeClass_LoadFromINI_Weapons2 (10042590, hook
                // 0x715B1F) skips the stock one.
                let extra = count.saturating_sub(WEAPON_SLOT_COUNT);
                self.extra_base.resize(extra, WeaponSlotGeometry::default());
                self.extra_elite
                    .resize(extra, WeaponSlotGeometry::default());
                count
            } else {
                count.min(WEAPON_SLOT_COUNT)
            };
            for slot in 0..count {
                if slot < WEAPON_SLOT_COUNT {
                    Self::read_numbered(art, slot, &mut self.base[slot], &mut self.elite[slot]);
                } else {
                    // Only a read the Ares way gets past the eighteen slots.
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
        // Phobos's loop appends a sixth entry only when `AlternateFLH5` is
        // present and valid; an absent or rejected key leaves no entry, which
        // the negative-index read answers as the zero FLH
        // (`Ext/TechnoType/Body.cpp:1368-1384`).
        if let Some(fifth) = nullable_flh(art, "AlternateFLH5") {
            self.alternate[5] = fifth;
        }
        read_burst_rows(art, turret_count, weapon_count, "", &mut self.burst);
        // Phobos `TechnoTypeExt::LoadFromINIFile`
        // (`Ext/TechnoType/Body.cpp:1365-1366`): two ART `Nullable<bool>`
        // reads; absent or invalid keeps the retained value.
        {
            self.alternate_flh_on_turret = art
                .read_phobos_nullable_bool("AlternateFLH.OnTurret", self.alternate_flh_on_turret);
            self.alternate_flh_apply_vehicle = art.read_phobos_nullable_bool(
                "AlternateFLH.ApplyVehicle",
                self.alternate_flh_apply_vehicle,
            );
        }
    }

    /// ART `AlternateFLH.OnTurret=`; `None` asks the `[General]` fallback.
    pub(crate) fn alternate_flh_on_turret(&self) -> Option<bool> {
        self.alternate_flh_on_turret
    }

    /// ART `AlternateFLH.ApplyVehicle=`; `None` asks the `[General]` fallback.
    pub(crate) fn alternate_flh_apply_vehicle(&self) -> Option<bool> {
        self.alternate_flh_apply_vehicle
    }

    /// Phobos `InfantryTypeExt::LoadFromINIFile`'s ART reads, reached only for
    /// an InfantryType: the two state key sets and their burst rows.
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
        // `Ext/InfantryType/Body.cpp:34-37` reads these four keys directly;
        // `InfantryExt::GetSimpleFLH` (`Ext/Infantry/Body.cpp:8-41`) selects
        // one by weapon index and state. Each key stays literal at its reader
        // call so the v3 scanner attributes the read.
        self.prone_simple[0] = art.read_int_fields_complete::<3>("PronePrimaryFireFLH", 2048);
        self.prone_simple[1] = art.read_int_fields_complete::<3>("ProneSecondaryFireFLH", 2048);
        self.deployed_simple[0] = art.read_int_fields_complete::<3>("DeployedPrimaryFireFLH", 2048);
        self.deployed_simple[1] =
            art.read_int_fields_complete::<3>("DeployedSecondaryFireFLH", 2048);
    }

    /// Phobos `InfantryExt::GetSimpleFLH`: the deployed or prone override of
    /// the primary/secondary firing offset. Deployed wins; any other slot or
    /// state has no override.
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
    pub(crate) fn burst_row_sets(&self) -> [&BurstFlhRows; 3] {
        [&self.burst, &self.deployed_burst, &self.prone_burst]
    }

    fn read_numbered(
        art: &IniSection,
        slot: usize,
        base: &mut WeaponSlotGeometry,
        elite: &mut WeaponSlotGeometry,
    ) {
        //715B3B..715D6E reads each base/elite record in order. The installed
        // Ares10040020 hook replaces that path with its own integer parser
        // when the Rules are read the Ares way.
        let ares = crate::rules::ares_switch::ares_way();
        let read_int = |key: &str, current| {
            if ares {
                crate::rules::ares_switch::not_ares_read(|| art.read_ares_int(key, current))
            } else {
                art.read_int(key, current)
            }
        };
        let read_coord3 = |key: &str, current| {
            if ares {
                crate::rules::ares_switch::not_ares_read(|| art.read_ares_coord3(key, current))
            } else {
                art.read_coord3(key, current)
            }
        };
        base.flh = read_coord3(&format!("Weapon{}FLH", slot + 1), base.flh);
        base.barrel_length = read_int(
            &format!("Weapon{}BarrelLength", slot + 1),
            base.barrel_length,
        );
        base.barrel_thickness = read_int(
            &format!("Weapon{}BarrelThickness", slot + 1),
            base.barrel_thickness,
        );
        elite.flh = read_coord3(&format!("EliteWeapon{}FLH", slot + 1), base.flh);
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

#[cfg(all(test, tests_rules_o_z))]
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

    #[test]
    fn ares_extra_barrels_use_the_extension_reader_and_regrow_zeroed() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
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
