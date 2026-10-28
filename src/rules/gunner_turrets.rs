//! Retained TechnoType charge-turret flag and the IFV weapon-to-model table.
//!
//! TechnoType constructor71136F..71137B initializes charge=false; the loop at
//! 711781..711790 initializes all eighteen table entries to -1. ReadINI
//! 71287E..712898 reads IsChargeTurret with the current value. UnitTypeReadINI
//! 747BBD..747E90 then reads the fixed key pairs below only for type ID FV
//! (case-insensitive native identity comparison410A40).
//!
//! Each pair passes a literal index default and weapon default -1 on EVERY
//! reached pass; absent keys do not retain their earlier authored values.
//! The unchecked setter717890 writes Type+814+4*weapon. Its weapon=-1 case
//! overwrites Type+810, including the charge flag's low byte, after the generic
//! flag read. The table itself retains every entry that this pass did not write.
//!
//! Custom weapon indices below -1 or above 17 write unrelated native memory.
//! Rust ignores those writes; stock FV maps use 0..16. This safety boundary does
//! not claim those malformed custom configurations reproduce native behavior.
//! Original executable comparisons: tools/spatial_oracle/ifv_turret_switching.md.

use crate::rules::ini_parser::IniSection;
use crate::rules::object_type::{ObjectCategory, WEAPON_SLOT_COUNT};

// Native read order, exact-case keys, and the index reader's literal default.
const IFV_TURRET_READS: [(&str, &str, i32); 17] = [
    ("NormalTurretIndex", "NormalTurretWeapon", 0),
    ("RepairTurretIndex", "RepairTurretWeapon", 1),
    ("MachineGunTurretIndex", "MachineGunTurretWeapon", 2),
    ("FlakTurretIndex", "FlakTurretWeapon", 3),
    ("PistolTurretIndex", "PistolTurretWeapon", 0),
    ("SniperTurretIndex", "SniperTurretWeapon", 0),
    ("ShockTurretIndex", "ShockTurretWeapon", 0),
    ("ExplodeTurretIndex", "ExplodeTurretWeapon", 0),
    ("BrainBlastTurretIndex", "BrainBlastTurretWeapon", 0),
    ("RadCannonTurretIndex", "RadCannonTurretWeapon", 0),
    ("ChronoTurretIndex", "ChronoTurretWeapon", 0),
    (
        "TerroristExplodeTurretIndex",
        "TerroristExplodeTurretWeapon",
        0,
    ),
    ("CowTurretIndex", "CowTurretWeapon", 0),
    ("InitiateTurretIndex", "InitiateTurretWeapon", 0),
    ("VirusTurretIndex", "VirusTurretWeapon", 0),
    ("YuriPrimeTurretIndex", "YuriPrimeTurretWeapon", 0),
    ("GuardianTurretIndex", "GuardianTurretWeapon", 0),
];

/// Immutable runtime projection of the process-resident TechnoType table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(not(feature = "ares"), derive(Copy))]
pub struct GunnerTurrets {
    gunner: bool,
    charge_turret: bool,
    turrets: [i32; WEAPON_SLOT_COUNT],
    #[cfg(feature = "ares")]
    extra_turrets: Vec<i32>,
    #[cfg(feature = "ares")]
    ui_names: Vec<Option<String>>,
}

impl Default for GunnerTurrets {
    fn default() -> Self {
        Self {
            gunner: false,
            charge_turret: false,
            turrets: [-1; WEAPON_SLOT_COUNT],
            #[cfg(feature = "ares")]
            extra_turrets: Vec::new(),
            #[cfg(feature = "ares")]
            ui_names: Vec::new(),
        }
    }
}

impl GunnerTurrets {
    pub(crate) fn apply_pass(
        &mut self,
        id: &str,
        section: &IniSection,
        category: ObjectCategory,
        weapon_count: i32,
    ) {
        // TechnoType::ReadINI714A49..714A5D retains the Gunner byte.
        self.gunner = section.read_bool("Gunner", self.gunner);
        self.charge_turret = section.read_bool("IsChargeTurret", self.charge_turret);
        #[cfg(feature = "ares")]
        {
            let _ = id;
            // UnitRead747BCF ->1004C620 tests Gunner, not the FV identity.
            if category == ObjectCategory::Vehicle && self.gunner {
                self.read_ares_mapping(section, weapon_count);
            }
            return;
        }
        #[cfg(not(feature = "ares"))]
        let _ = weapon_count;
        #[cfg(not(feature = "ares"))]
        if category != ObjectCategory::Vehicle || !id.eq_ignore_ascii_case("FV") {
            return;
        }
        #[cfg(not(feature = "ares"))]
        for (index_key, weapon_key, default_index) in IFV_TURRET_READS {
            let turret = section.read_int(index_key, default_index);
            let weapon = section.read_int(weapon_key, -1);
            if weapon == -1 {
                self.charge_turret = turret as u8 != 0;
            } else if let Some(entry) = usize::try_from(weapon)
                .ok()
                .and_then(|weapon| self.turrets.get_mut(weapon))
            {
                *entry = turret;
            }
        }
    }

    /// Ares10040020 resizes both extension vectors before numbered weapons;
    ///1003FE10 subsequently grows labels to full WeaponCount for Gunner Units.
    #[cfg(feature = "ares")]
    pub(crate) fn prepare_numbered_read(&mut self, weapon_count: i32) {
        let extra = usize::try_from(weapon_count)
            .unwrap_or(0)
            .saturating_sub(WEAPON_SLOT_COUNT);
        self.extra_turrets.resize(extra, -1);
        self.ui_names.resize(extra, None);
    }

    #[cfg(feature = "ares")]
    fn read_ares_mapping(&mut self, section: &IniSection, weapon_count: i32) {
        let count = usize::try_from(weapon_count).unwrap_or(0);
        self.extra_turrets
            .resize(count.saturating_sub(WEAPON_SLOT_COUNT), -1);
        self.ui_names.resize(count, None);
        //1003FE80..1003FF20: named aliases precede numbered overrides.
        // Negative weapon/turret values are skipped, never charge-byte writes.
        for (index_key, weapon_key, default_index) in IFV_TURRET_READS {
            let weapon = section.read_ares_int(weapon_key, -1);
            if weapon < 0 {
                continue;
            }
            let turret = section.read_ares_int(index_key, default_index);
            if turret >= 0 {
                self.set_turret(weapon as usize, turret);
            }
        }
        for slot in 0..count {
            //1003FF4F..1003FFC2: invalid/unset values keep the current map;
            // an out-of-range positive model is warned about but not clamped.
            if let Some(turret) =
                section.read_ares_nullable_int(&format!("WeaponTurretIndex{}", slot + 1), None)
                && turret >= 0
            {
                self.set_turret(slot, turret);
            }
            // Cached label reader10076510 first reads2048 then copies31 bytes.
            // The immutable session CSF owner resolves this label at display.
            if let Some(label) = section.read_name(&format!("WeaponUIName{}", slot + 1), 2048) {
                self.ui_names[slot] =
                    Some(crate::rules::ini_value::truncate_native_bytes(label, 31).to_owned());
            }
        }
    }

    #[cfg(feature = "ares")]
    fn set_turret(&mut self, slot: usize, turret: i32) {
        let entry = if slot < WEAPON_SLOT_COUNT {
            self.turrets.get_mut(slot)
        } else {
            self.extra_turrets.get_mut(slot - WEAPON_SLOT_COUNT)
        };
        if let Some(entry) = entry {
            *entry = turret;
        }
    }

    pub fn is_gunner(&self) -> bool {
        self.gunner
    }

    /// UnitGetUIName746B89 ->1004BF20 selects the current weapon's full name.
    #[cfg(feature = "ares")]
    pub fn weapon_ui_name(&self, weapon: i32) -> Option<&str> {
        self.ui_names
            .get(usize::try_from(weapon).ok()?)
            .and_then(Option::as_deref)
    }

    /// TechnoType+810 after the last reached rules pass, including FV aliases.
    pub fn is_charge_turret(&self) -> bool {
        self.charge_turret
    }

    /// Native getter7178B0 table entry for a validated gunner weapon number.
    /// Unread entries are -1;
    /// invalid external indices also return -1 rather than indexing native memory.
    pub fn turret_for_weapon(&self, weapon: i32) -> i32 {
        let Some(slot) = usize::try_from(weapon).ok() else {
            return -1;
        };
        let turret = self.turrets.get(slot);
        #[cfg(feature = "ares")]
        let turret =
            turret.or_else(|| self.extra_turrets.get(slot.checked_sub(WEAPON_SLOT_COUNT)?));
        turret.copied().unwrap_or(-1)
    }
}

// Mode labels are presentation inputs; retained gameplay state alone enters CRC.
impl std::hash::Hash for GunnerTurrets {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.gunner, state);
        std::hash::Hash::hash(&self.charge_turret, state);
        std::hash::Hash::hash(&self.turrets, state);
        #[cfg(feature = "ares")]
        std::hash::Hash::hash(&self.extra_turrets, state);
    }
}

#[cfg(test)]
#[path = "gunner_turret_rules_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ares_gunner_tests.rs"]
mod extended_tests;
