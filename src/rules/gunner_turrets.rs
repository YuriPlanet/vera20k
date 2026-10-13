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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GunnerTurrets {
    charge_turret: bool,
    turrets: [i32; WEAPON_SLOT_COUNT],
}

impl Default for GunnerTurrets {
    fn default() -> Self {
        Self {
            charge_turret: false,
            turrets: [-1; WEAPON_SLOT_COUNT],
        }
    }
}

impl GunnerTurrets {
    /// Single raw-pass reader for direct ObjectType fixtures. Production runs
    /// `apply_pass` against the retained native registry before projection.
    pub(crate) fn from_ini_section(
        id: &str,
        section: &IniSection,
        category: ObjectCategory,
    ) -> Self {
        let mut result = Self::default();
        result.apply_pass(id, section, category);
        result
    }

    pub(crate) fn apply_pass(&mut self, id: &str, section: &IniSection, category: ObjectCategory) {
        self.charge_turret = section.read_bool("IsChargeTurret", self.charge_turret);
        if category != ObjectCategory::Vehicle || !id.eq_ignore_ascii_case("FV") {
            return;
        }
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

    /// TechnoType+810 after the last reached rules pass, including FV aliases.
    pub fn is_charge_turret(&self) -> bool {
        self.charge_turret
    }

    /// Native getter7178B0 table entry for a validated gunner weapon number.
    /// Unread entries are -1;
    /// invalid external indices also return -1 rather than indexing native memory.
    pub fn turret_for_weapon(&self, weapon: i32) -> i32 {
        usize::try_from(weapon)
            .ok()
            .and_then(|weapon| self.turrets.get(weapon))
            .copied()
            .unwrap_or(-1)
    }
}

#[cfg(test)]
#[path = "gunner_turret_rules_tests.rs"]
mod tests;
