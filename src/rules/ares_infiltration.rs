//! Optional Ares building infiltration and sabotage configuration: the
//! `SpyEffect.*` keys and `ImmuneToSaboteurs=` of
//! `BuildingTypeExt::ExtData::LoadFromINIFile` (installed Ares `1000DD40`,
//! reads at `1000DD40+0x201..0x284`). The native processing registry owns
//! reached reads; `ObjectType` projects them. `sim::spy_infiltration` and
//! the infantry infiltration owners consume them.
//!
//! `Cursor.Spy` (`+0x240`) is not read here: it belongs to Ares' cursor
//! type table (`[MouseCursors]`, `rules::ares_mouse_cursor`).

use crate::{
    rules::{ini_parser::IniSection, object_type::ObjectType},
    util::native_x87::NativeF32Bits,
};

/// BuildingTypeExt `+0xAA..+0xCC` and `+0x1E8`. The constructor
/// (`BuildingTypeExt::ExtData::ExtData @ 1000CAE0`) clears every flag, the
/// Super pointer, the amounts and the delay, and leaves the
/// `ImmuneToSaboteurs` Nullable unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfiltrationTypeRules {
    /// `SpyEffect.Custom` (`+0xAA`): infiltration runs the Ares effects
    /// below instead of the type's original arm.
    pub(crate) custom: bool,
    /// `SpyEffect.RevealProduction` (`+0xAB`).
    pub(crate) reveal_production: bool,
    /// `SpyEffect.ResetSuperweapons` (`+0xAC`).
    pub(crate) reset_super_weapons: bool,
    /// `SpyEffect.ResetRadar` (`+0xAD`).
    pub(crate) reset_radar: bool,
    /// `SpyEffect.RevealRadar` (`+0xAE`).
    pub(crate) reveal_radar: bool,
    /// `SpyEffect.KeepRadar` (`+0xAF`, Ares `RevealRadarPersist`).
    pub(crate) keep_radar: bool,
    /// `SpyEffect.UnitVeterancy` (`+0xB0`, Ares `GainVeterancy`): the
    /// factory-type veterancy of the original arm.
    pub(crate) unit_veterancy: bool,
    /// `SpyEffect.InfantryVeterancy` (`+0xB1`).
    pub(crate) infantry_veterancy: bool,
    /// `SpyEffect.VehicleVeterancy` (`+0xB2`).
    pub(crate) vehicle_veterancy: bool,
    /// `SpyEffect.NavalVeterancy` (`+0xB3`).
    pub(crate) naval_veterancy: bool,
    /// `SpyEffect.AircraftVeterancy` (`+0xB4`).
    pub(crate) aircraft_veterancy: bool,
    /// `SpyEffect.BuildingVeterancy` (`+0xB5`).
    pub(crate) building_veterancy: bool,
    /// `SpyEffect.UndoReverseEngineer` (`+0xB6`).
    pub(crate) undo_reverse_engineer: bool,
    /// `SpyEffect.SuperWeaponPermanent` (`+0xB7`).
    pub(crate) super_weapon_permanent: bool,
    /// `SpyEffect.SuperWeapon` (`+0xB8`), a `[SuperWeaponTypes]` entry.
    pub(crate) super_weapon: Option<String>,
    /// `SpyEffect.StolenTechIndex` (`+0xBC`, a 32-bit set, constructor 0):
    /// the StolenTech bits the spy's house gains.
    pub(crate) stolen_tech: u32,
    /// `SpyEffect.StolenMoneyAmount` (`+0xC0`).
    pub(crate) stolen_money_amount: i32,
    /// `SpyEffect.StolenMoneyPercentage` (`+0xC4`), a float in 3.0.
    pub(crate) stolen_money_percentage: NativeF32Bits,
    /// `SpyEffect.PowerOutageDuration` (`+0xC8`), frames.
    pub(crate) power_outage_duration: i32,
    /// `SpyEffect.SabotageDelay` (`+0xCC`), frames; negative uses
    /// `[General] C4Delay=`.
    pub(crate) sabotage_delay: i32,
    /// `ImmuneToSaboteurs` (`+0x1E8`, Nullable).
    pub(crate) immune_to_saboteurs: Option<bool>,
}

impl Default for InfiltrationTypeRules {
    fn default() -> Self {
        Self {
            custom: false,
            reveal_production: false,
            reset_super_weapons: false,
            reset_radar: false,
            reveal_radar: false,
            keep_radar: false,
            unit_veterancy: false,
            infantry_veterancy: false,
            vehicle_veterancy: false,
            naval_veterancy: false,
            aircraft_veterancy: false,
            building_veterancy: false,
            undo_reverse_engineer: false,
            super_weapon_permanent: false,
            super_weapon: None,
            stolen_tech: 0,
            stolen_money_amount: 0,
            stolen_money_percentage: NativeF32Bits::POSITIVE_ZERO,
            power_outage_duration: 0,
            sabotage_delay: 0,
            immune_to_saboteurs: None,
        }
    }
}

impl InfiltrationTypeRules {
    /// One reached BuildingType pass, in the installed DLL's read order
    /// (`1000DD40`, which reads the three amounts and the delay before
    /// `UndoReverseEngineer` and the Super). Every key is a Valueable that
    /// keeps its value on an absent, empty or invalid read.
    /// `super_weapon` is the registry's Find-only `SpyEffect.SuperWeapon`
    /// lookup (`detail::read<SuperWeaponTypeClass*> @ 1000CA80`): `None`
    /// keeps the field, `Some(None)` clears it for a blank value.
    pub(crate) fn read_pass(&mut self, section: &IniSection, super_weapon: Option<Option<String>>) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.custom = section.read_ares_bool("SpyEffect.Custom", self.custom);
        self.reveal_production =
            section.read_ares_bool("SpyEffect.RevealProduction", self.reveal_production);
        self.reset_super_weapons =
            section.read_ares_bool("SpyEffect.ResetSuperweapons", self.reset_super_weapons);
        self.reset_radar = section.read_ares_bool("SpyEffect.ResetRadar", self.reset_radar);
        self.reveal_radar = section.read_ares_bool("SpyEffect.RevealRadar", self.reveal_radar);
        self.keep_radar = section.read_ares_bool("SpyEffect.KeepRadar", self.keep_radar);
        self.unit_veterancy =
            section.read_ares_bool("SpyEffect.UnitVeterancy", self.unit_veterancy);
        self.infantry_veterancy =
            section.read_ares_bool("SpyEffect.InfantryVeterancy", self.infantry_veterancy);
        self.vehicle_veterancy =
            section.read_ares_bool("SpyEffect.VehicleVeterancy", self.vehicle_veterancy);
        self.naval_veterancy =
            section.read_ares_bool("SpyEffect.NavalVeterancy", self.naval_veterancy);
        self.aircraft_veterancy =
            section.read_ares_bool("SpyEffect.AircraftVeterancy", self.aircraft_veterancy);
        self.building_veterancy =
            section.read_ares_bool("SpyEffect.BuildingVeterancy", self.building_veterancy);
        self.power_outage_duration =
            section.read_ares_int("SpyEffect.PowerOutageDuration", self.power_outage_duration);
        self.sabotage_delay = section.read_ares_int("SpyEffect.SabotageDelay", self.sabotage_delay);
        self.stolen_money_amount =
            section.read_ares_int("SpyEffect.StolenMoneyAmount", self.stolen_money_amount);
        // Valueable<float>::Read 1000EAE0 -> detail::read<float> 1000FA90.
        self.stolen_money_percentage = section.read_ares_float_bits(
            "SpyEffect.StolenMoneyPercentage",
            self.stolen_money_percentage,
        );
        self.undo_reverse_engineer =
            section.read_ares_bool("SpyEffect.UndoReverseEngineer", self.undo_reverse_engineer);
        if let Some(super_weapon) = super_weapon {
            self.super_weapon = super_weapon;
        }
        self.super_weapon_permanent = section.read_ares_bool(
            "SpyEffect.SuperWeaponPermanent",
            self.super_weapon_permanent,
        );
        // 1000DD40, after SuperWeaponPermanent: ValueableVector<int> into an
        // empty temporary; a nonempty result replaces the set with the bits
        // of its indexes below 32 (unsigned compare: negatives are skipped
        // silently, larger ones logged).
        if let Some(indexes) = section.read_ares_int_list("SpyEffect.StolenTechIndex")
            && !indexes.is_empty()
        {
            self.stolen_tech = indexes
                .into_iter()
                .filter_map(|index| u32::try_from(index).ok().filter(|&index| index < 32))
                .fold(0, |bits, index| bits | 1 << index);
        }
        // 1000DD40+0x271, after the Degrade keys.
        self.immune_to_saboteurs =
            section.read_ares_nullable_bool("ImmuneToSaboteurs", self.immune_to_saboteurs);
    }

    /// `BuildingTypeExt::IsSabotagable @ 1000D790`: whether a `Saboteur=`
    /// may blow `building` up. Unset `ImmuneToSaboteurs` makes a civilian
    /// occupiable building (`CanBeOccupied=` with `TechLevel=-1`, `+0x157B`
    /// and `+0x634`) and one that is not `CanC4=` (`+0x1577`) immune.
    pub(crate) fn sabotagable(&self, building: &ObjectType) -> bool {
        let civilian_occupiable = building.can_be_occupied && building.tech_level == -1;
        let default_immune = civilian_occupiable || !building.can_c4;
        !self.immune_to_saboteurs.unwrap_or(default_immune)
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};

    /// The reader keeps the DLL's Valueable semantics: a percent scales the
    /// float, the Super is a Find-only reference (an unknown name keeps the
    /// earlier one), and an unset `ImmuneToSaboteurs` follows `CanC4=` and
    /// civilian occupation (`IsSabotagable @ 1000D790`).
    #[test]
    fn spy_effect_keys_read_as_the_dll_reads_them() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[SuperWeaponTypes]\n0=SW1\n[SW1]\nType=MultiMissile\n\
             [BuildingTypes]\n0=BANK\n1=HUT\n2=BARREL\n3=SAFE\n\
             [BANK]\nSpyEffect.Custom=yes\nSpyEffect.StolenMoneyPercentage=25%\n\
             SpyEffect.SuperWeapon=SW1\nSpyEffect.SabotageDelay=$90\n\
             SpyEffect.PowerOutageDuration=junk\n\
             [HUT]\nCanBeOccupied=yes\nTechLevel=-1\nSpyEffect.SuperWeapon=NOPE\n\
             [BARREL]\nCanC4=no\n[SAFE]\nCanC4=no\nImmuneToSaboteurs=no\n",
        ))
        .unwrap();
        let bank = rules.object("BANK").unwrap();
        let spy = bank.infiltration_rules();
        assert!(spy.custom);
        assert_eq!(f32::from_bits(spy.stolen_money_percentage.bits()), 0.25);
        assert_eq!(spy.super_weapon.as_deref(), Some("SW1"));
        assert_eq!(spy.sabotage_delay, 90);
        assert_eq!(spy.power_outage_duration, 0, "invalid keeps the default");
        assert!(spy.sabotagable(bank));
        let hut = rules.object("HUT").unwrap();
        assert_eq!(hut.infiltration_rules().super_weapon, None);
        assert!(
            !hut.infiltration_rules().sabotagable(hut),
            "civilian occupiable"
        );
        let barrel = rules.object("BARREL").unwrap();
        assert!(!barrel.infiltration_rules().sabotagable(barrel));
        let safe = rules.object("SAFE").unwrap();
        assert!(
            safe.infiltration_rules().sabotagable(safe),
            "explicitly not immune"
        );
    }
}
