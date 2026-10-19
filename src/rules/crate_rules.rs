//! Scenario-start crate rule authority.
//!
//! `RulesClass__ReadCrateRules @ 0x0066B900` runs once per ordered rules pass
//! after late global references have been allocated. Missing sections and keys
//! retain the already-live fields; native no-type sentinels resolve to null.

use crate::rules::ini_parser::{IniFile, IniSection, is_native_none_type_name};
use crate::util::native_x87::NativeF64Bits;

/// Every `[CrateRules]` field `RulesClass__ReadCrateRules @ 0x0066B900` reads,
/// in its native read order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CrateRules {
    /// `FreeMCV` -> `Rules+0x40`. `CellClass::PickupCrate @ 0x00481A00` does
    /// not read it: its MCV pre-empt (`0x00481BB8..0x00481BFF`) gates on the
    /// lobby Bases byte (`0x00A8B258`), no tracked buildings, more than 1500
    /// available credits and no tracked `BaseUnit=` vehicle. No other reader
    /// of `+0x40` is established here.
    pub free_mcv: bool,
    /// `WoodCrateImg` -> `Rules+0xF8`. The three images and
    /// [`Self::unit_crate_type`] are stored IDs of the Types the ordered Rules
    /// processor's factories answered.
    pub wood_crate_img: Option<String>,
    /// `CrateImg` -> `Rules+0xFC`.
    pub crate_img: Option<String>,
    /// `WaterCrateImg` -> `Rules+0x100`.
    pub water_crate_img: Option<String>,
    /// `HealCrateSound` -> `Rules+0x718`, a Voc index natively.
    ///
    /// Native resolves the name through `VocClass__FindByName` at parse time and
    /// keeps the PREVIOUS index when the lookup fails, so an unresolvable name
    /// is indistinguishable from an absent key. A no-type sentinel is modelled
    /// exactly — it retains the live value instead of clearing it. What cannot
    /// be modelled here is an ordinary name that no sound defines: VERA has no
    /// Voc registry at rules-parse time, so it stores the name and resolves at
    /// use, where native would already have kept the earlier sound. Reaching
    /// that needs two passes, the first naming a real sound and the second an
    /// undefined one; recorded as a deferred DRIFT.
    pub heal_crate_sound: Option<String>,
    /// `CrateMinimum` -> `Rules+0x1470`.
    pub minimum: i32,
    /// `CrateMaximum` -> `Rules+0x1474`.
    pub maximum: i32,
    /// `CrateRadius` -> `Rules+0x172C`, read by `CCINIClass::ReadRange` and
    /// stored in leptons. Every radius crate effect compares a 3-D distance
    /// against this value with a strict `<`.
    pub radius: i32,
    /// `CrateRegen` -> `Rules+0x1678`.
    pub regen: NativeF64Bits,
    /// `UnitCrateType` -> `Rules+0x1148`. A named type overrides the Unit
    /// effect's random `CrateGoodie` selection.
    pub unit_crate_type: Option<String>,
    /// `SoloCrateMoney` -> `Rules+0x1140`, paid flat by the Money effect in
    /// game mode zero instead of drawing a random amount.
    pub solo_crate_money: i32,
    /// `SilverCrate` -> `Rules+0x1464`: the fixed outcome a `CrateImg` crate
    /// yields in game mode zero. Stored as a slot index in the fixed
    /// `[Powerups]` table via `Powerup_From_Name @ 0x0048DE70`.
    pub silver_crate: usize,
    /// `WoodCrate` -> `Rules+0x1468`, the same mapping for `WoodCrateImg`.
    pub wood_crate: usize,
    /// `WaterCrate` -> `Rules+0x146C`, the same mapping for `WaterCrateImg`.
    pub water_crate: usize,
}

impl Default for CrateRules {
    fn default() -> Self {
        // `RulesClass__Constructor @ 0x00665650` immediate stores, each read
        // this session: `+0x40 = BL` with `EBX` zeroed at `0x00665663`;
        // `+0x718 = EBP` with `EBP` set to -1 at `0x0066585F` (no sound);
        // `+0x1140 = ECX` loaded `0x7D0` at `0x00666DD7`; `+0x1148 = EBX`;
        // `+0x1464 = EDX` loaded `2` at `0x006671CD`; `+0x1468`/`+0x146C = EBX`;
        // `+0x1470 = EAX` loaded `1`; `+0x1474 = 0xFF`; `+0x172C = 0x280`.
        // Image pointers begin null; stock rulesmd.ini fills them during Process.
        Self {
            free_mcv: false,
            wood_crate_img: None,
            crate_img: None,
            water_crate_img: None,
            heal_crate_sound: None,
            minimum: 1,
            maximum: 255,
            radius: 0x280,
            regen: NativeF64Bits::from_bits(10.0_f64.to_bits()),
            unit_crate_type: None,
            solo_crate_money: 2000,
            // The constructor's own defaults already agree with the stock INI:
            // silver -> HealBase, wood and water -> Money.
            silver_crate: crate::rules::powerups::POWERUP_HEAL_BASE,
            wood_crate: crate::rules::powerups::POWERUP_MONEY,
            water_crate: crate::rules::powerups::POWERUP_MONEY,
        }
    }
}

impl CrateRules {
    /// One pass of `RulesClass__ReadCrateRules` into the live fields, which
    /// RulesClass keeps across passes and Process calls. `resolve(section,
    /// key)` is the ordered Rules processor's ReadString128 and factory for
    /// the Type keys: `None` for an empty read, otherwise the factory's
    /// answer.
    pub(crate) fn apply_pass(
        &mut self,
        ini: &IniFile,
        mut resolve: impl FnMut(&IniSection, &str) -> Option<Option<String>>,
    ) {
        let Some(section) = ini.section("CrateRules") else {
            return;
        };

        // `RulesClass__ReadCrateRules @ 0x0066B900` reads its keys in exactly
        // this order. Every reader is handed the live value as its default, so
        // a missing key keeps the field. The string keys allocate OverlayType
        // and UnitType objects as they are read.
        self.free_mcv = section.read_bool("FreeMCV", self.free_mcv);
        // ReadString128, then `OverlayTypeClass__FindOrAllocate @ 0x005FEC70`
        // only for a nonempty read; its null for `<none>` and `none` is
        // stored, so unlike the sound below these keys null on a sentinel.
        for (key, target) in [
            ("WoodCrateImg", &mut self.wood_crate_img),
            ("CrateImg", &mut self.crate_img),
            ("WaterCrateImg", &mut self.water_crate_img),
        ] {
            if let Some(overlay) = resolve(section, key) {
                *target = overlay;
            }
        }
        if section.is_present("HealCrateSound") {
            // `if ((read == 0) || (index = VocClass__FindByName(), index == -1))
            //  { index = previous; }` — a failed lookup RETAINS the live index
            // rather than clearing it, so a no-type sentinel must not null the
            // field the way the image keys above do.
            let value = section.read_string("HealCrateSound", "", 0x80);
            if !is_native_none_type_name(&value) {
                self.heal_crate_sound = Some(value.to_ascii_uppercase());
            }
        }
        if section.is_present("CrateMinimum") {
            self.minimum = section.read_int("CrateMinimum", self.minimum);
        }
        if section.is_present("CrateMaximum") {
            self.maximum = section.read_int("CrateMaximum", self.maximum);
        }
        // `CrateRadius` is stored in leptons; the stock `3.0` is three cells.
        // ReadRange owns the absent-key and `-1` sentinel cases itself, so this
        // needs no presence guard of its own.
        self.radius = section.read_range("CrateRadius", self.radius);
        if section.is_present("CrateRegen") {
            self.regen = NativeF64Bits::from_bits(
                section
                    .read_double("CrateRegen", f64::from_bits(self.regen.bits()))
                    .to_bits(),
            );
        }
        // The same read through `UnitTypeClass__FindOrAllocate @ 0x007480D0`.
        if let Some(unit) = resolve(section, "UnitCrateType") {
            self.unit_crate_type = unit;
        }
        if section.is_present("SoloCrateMoney") {
            self.solo_crate_money = section.read_int("SoloCrateMoney", self.solo_crate_money);
        }
        for (key, target) in [
            ("SilverCrate", &mut self.silver_crate),
            ("WoodCrate", &mut self.wood_crate),
            ("WaterCrate", &mut self.water_crate),
        ] {
            if !section.is_present(key) {
                continue;
            }
            // `FUN_004759F0` reads the string, then `Powerup_From_Name` maps it
            // to a slot; an unmatched name resolves to Money rather than failing.
            let value = section.read_string(key, "", 0x80);
            *target = crate::rules::powerups::powerup_from_name(&value);
        }
    }

    /// The fields Full_Init's Type reset leaves dangling, cleared; the
    /// scalars stay (`NativeRulesRegistryState::destructive_reset`).
    pub(crate) fn without_type_references(self) -> Self {
        Self {
            wood_crate_img: None,
            crate_img: None,
            water_crate_img: None,
            unit_crate_type: None,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::powerups::{POWERUP_HEAL_BASE, POWERUP_MONEY, POWERUP_VETERAN};

    /// The production reader: rules passes in order, the crate Types read by
    /// the ordered Rules processor's factories.
    fn parse_layers(layers: &[&str]) -> CrateRules {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        let mut stack = RulesLayerStack::new(IniFile::from_str(layers[0]));
        for layer in &layers[1..] {
            stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
        }
        stack.process().unwrap().crate_rules().clone()
    }

    fn parse(section: &str) -> CrateRules {
        parse_layers(&[section])
    }

    /// The stock section, read exactly. `CrateRadius` is stored in leptons and
    /// the three solo mappings become fixed `[Powerups]` slot indices.
    #[test]
    fn stock_crate_rules_section_parses_every_native_field() {
        let rules = parse(
            "[CrateRules]\n\
             CrateMaximum=255\n\
             CrateMinimum=1\n\
             CrateRadius=3.0\n\
             CrateRegen=3\n\
             SilverCrate=HealBase\n\
             SoloCrateMoney=5000\n\
             UnitCrateType=none\n\
             WoodCrate=Money\n\
             WaterCrate=Money\n\
             HealCrateSound=HealCrate\n\
             WoodCrateImg=CRATE\n\
             CrateImg=CRATE\n\
             WaterCrateImg=WCRATE\n\
             FreeMCV=yes\n",
        );

        assert!(rules.free_mcv);
        assert_eq!(rules.minimum, 1);
        assert_eq!(rules.maximum, 255);
        assert_eq!(rules.radius, 768, "3.0 cells is 768 leptons");
        assert_eq!(f64::from_bits(rules.regen.bits()), 3.0);
        assert_eq!(rules.solo_crate_money, 5000);
        assert_eq!(rules.heal_crate_sound.as_deref(), Some("HEALCRATE"));
        assert_eq!(
            rules.unit_crate_type, None,
            "`none` is the native no-type sentinel"
        );
        assert_eq!(rules.silver_crate, POWERUP_HEAL_BASE);
        assert_eq!(rules.wood_crate, POWERUP_MONEY);
        assert_eq!(rules.water_crate, POWERUP_MONEY);
        // Retail names the same overlay for two of the three image slots.
        assert_eq!(rules.wood_crate_img.as_deref(), Some("CRATE"));
        assert_eq!(rules.crate_img.as_deref(), Some("CRATE"));
        assert_eq!(rules.water_crate_img.as_deref(), Some("WCRATE"));
    }

    /// A missing section leaves every constructor default in place.
    #[test]
    fn missing_section_retains_the_constructor_defaults() {
        let rules = parse("[General]\n");
        assert_eq!(rules, CrateRules::default());
        assert!(!rules.free_mcv, "the constructor stores BL with EBX zeroed");
        assert_eq!(rules.radius, 0x280, "2.5 cells");
        assert_eq!(rules.solo_crate_money, 2000);
        assert_eq!(rules.heal_crate_sound, None, "EBP is -1: no sound");
        assert_eq!(rules.silver_crate, POWERUP_HEAL_BASE);
        assert_eq!(rules.wood_crate, POWERUP_MONEY);
    }

    /// Each key is read with the live value as its default, so an absent key in
    /// a later pass never resets an earlier one.
    #[test]
    fn later_pass_without_a_key_retains_the_live_value() {
        let rules = parse_layers(&[
            "[CrateRules]\nFreeMCV=yes\nSoloCrateMoney=5000\nCrateRadius=3.0\nSilverCrate=Veteran\n",
            "[CrateRules]\nCrateMinimum=4\n",
        ]);

        assert!(rules.free_mcv);
        assert_eq!(rules.solo_crate_money, 5000);
        assert_eq!(rules.radius, 768);
        assert_eq!(rules.silver_crate, POWERUP_VETERAN);
        assert_eq!(rules.minimum, 4);
    }

    /// A failed `VocClass__FindByName` retains the live index, so a no-type
    /// sentinel must not clear the sound the way the three image keys are.
    #[test]
    fn heal_crate_sound_sentinel_retains_the_live_value() {
        let rules = parse_layers(&[
            "[CrateRules]\nHealCrateSound=HealCrate\nCrateImg=CRATE\n",
            "[CrateRules]\nHealCrateSound=<none>\nCrateImg=<none>\n",
        ]);

        assert_eq!(
            rules.heal_crate_sound.as_deref(),
            Some("HEALCRATE"),
            "a sentinel keeps the previously resolved sound"
        );
        assert_eq!(
            rules.crate_img, None,
            "the image keys DO null on a sentinel — FindOrCreate has no retain branch"
        );
    }

    /// The crate Types publish the factory's stored ID, the spelling of the
    /// first reference that allocated the Type, and a later pass that omits
    /// a key keeps the earlier Type.
    #[test]
    fn crate_types_publish_the_allocated_type() {
        let rules = parse_layers(&[
            "[OverlayTypes]\n0=Crate\n[VehicleTypes]\n0=Tank\n\
             [CrateRules]\nCrateImg=CRATE\nUnitCrateType=TANK\n",
            "[CrateRules]\nWaterCrateImg=WCrate\n",
        ]);
        assert_eq!(rules.crate_img.as_deref(), Some("Crate"));
        assert_eq!(rules.unit_crate_type.as_deref(), Some("Tank"));
        assert_eq!(rules.water_crate_img.as_deref(), Some("WCrate"));
    }

    /// An unmatched solo-crate mapping resolves to Money rather than failing —
    /// `Powerup_From_Name` has no error path.
    #[test]
    fn unmatched_solo_mapping_falls_back_to_money() {
        let rules = parse("[CrateRules]\nSilverCrate=NotAPowerup\n");
        assert_eq!(rules.silver_crate, POWERUP_MONEY);
    }
}
