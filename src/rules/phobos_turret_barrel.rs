//! Phobos RA1-style multi-turret / multi-barrel offsets.
//!
//! `UnitTypeExt::LoadFromINI` reads these from the **art** section
//! (`Ext/UnitType/Body.cpp:100-134`):
//!
//! ```ini
//! BarrelOverTurret=          ; Nullable<bool>, unset               :100
//! BarrelOffset=              ; Valueable<int>, 0                  :101
//! ExtraBarrelCount=          ; Valueable<int>, 0                   :102
//! ExtraBarrelOffset<n>=      ; Valueable<int>, 0, 0..ExtraBarrelCount :109
//! ExtraTurretCount=          ; Valueable<int>, 0                   :119
//! ExtraTurretOffset<n>=      ; Valueable<CoordStruct>, 0,0,0, 0..Count :126
//! BurstPerTurret=            ; Valueable<int>, 0, only if Count > 0 :130
//! ```
//!
//! Two clamps are load-bearing: a non-positive `ExtraBarrelCount=` /
//! `ExtraTurretCount=` clamps itself to zero and reads no offsets
//! (`:116`, `:134`), and `BurstPerTurret=` is read **only** inside the positive
//! `ExtraTurretCount=` arm (`:121-131`).
//!
//! Declarations `Ext/UnitType/Body.h:76-82`, constructor defaults `Body.h:135-141`.
//!
//! The keys live in the art INI, not the type's rule section.
//! `ArtEntry` reads the block from its own section; `RuleSet::project_art_data`
//! copies it onto the Vehicle type through the native `ImageFile` section
//! selection (`obj.image` else `obj.id`), so a rule-section key never reaches
//! this parser.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The barrel/turret block of `UnitTypeExt`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhobosTurretBarrel {
    /// `BarrelOverTurret=` (`:100`), `Nullable`: an extra barrel is drawn over
    /// the turret rather than under the body.
    pub barrel_over_turret: Option<bool>,
    /// `BarrelOffset=` (`:101`), the barrel axis shift in leptons that the
    /// extra barrels inherit (`Hooks.MatrixOp.cpp:174`, `:885`).
    pub barrel_offset: i32,
    /// `ExtraBarrelCount=` (`:102`), clamped to zero when not positive.
    pub extra_barrel_count: i32,
    /// `ExtraBarrelOffset0=`, `ExtraBarrelOffset1=`, … in index order.
    pub extra_barrel_offsets: Vec<i32>,
    /// `ExtraTurretCount=` (`:119`), clamped to zero when not positive.
    pub extra_turret_count: i32,
    /// `ExtraTurretOffset0=`, … as `CoordStruct` triples in index order.
    pub extra_turret_offsets: Vec<[i32; 3]>,
    /// `BurstPerTurret=` (`:130`), read only when `ExtraTurretCount=` is
    /// positive.
    pub burst_per_turret: i32,
}

impl PhobosTurretBarrel {
    /// Reads every key in `LoadFromINI` order, including the indexed loops.
    pub fn from_section(section: &IniSection) -> Self {
        let extra_barrel_count = section.read_int("ExtraBarrelCount", 0).max(0);
        let extra_barrel_offsets = (0..extra_barrel_count)
            .map(|index| section.read_int(&format!("ExtraBarrelOffset{index}"), 0))
            .collect();
        let extra_turret_count = section.read_int("ExtraTurretCount", 0).max(0);
        let extra_turret_offsets = (0..extra_turret_count)
            .map(|index| {
                section.read_phobos_int_triple(&format!("ExtraTurretOffset{index}"), [0; 3])
            })
            .collect();
        Self {
            barrel_over_turret: section.read_phobos_nullable_bool("BarrelOverTurret", None),
            barrel_offset: section.read_int("BarrelOffset", 0),
            extra_barrel_count,
            extra_barrel_offsets,
            extra_turret_count,
            extra_turret_offsets,
            // `Body.cpp:130`: inside the positive-count arm only.
            burst_per_turret: if extra_turret_count > 0 {
                section.read_int("BurstPerTurret", 0)
            } else {
                0
            },
        }
    }

    /// How many barrels the type draws in total: the stock one plus the extra
    /// count.
    pub fn barrel_total(&self) -> i32 {
        1 + self.extra_barrel_count
    }

    /// How many turrets the type draws in total: the stock one plus the extra
    /// count.
    pub fn turret_total(&self) -> i32 {
        1 + self.extra_turret_count
    }
}

#[cfg(test)]
#[path = "phobos_turret_barrel_tests.rs"]
mod tests;
