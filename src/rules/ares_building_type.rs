//! Optional Ares BuildingTypeExt keys of the building-type chain, read by
//! `BuildingTypeExt::ExtData::LoadFromINIFile` (installed Ares `1000DD40`)
//! and checked by its `CompleteInitialization` (`1000D380`, hook
//! `0x00679CAF`). The native processing registry owns the reached reads and
//! the Find-only type lookups; `ObjectType` projects them.
//!
//! - Advanced rubble (`Rubble.*`, `+0x88..+0xA9`): `sim::ares_rubble`
//!   switches a destroyed building to its rubble and a rebuilt rubble back.
//! - `EngineerRepairable` (`+0x231`, Nullable over `Repairable=`): the
//!   engineer's repair action and arrival (`sim::world::world_orders`).

use crate::rules::ares_warhead::HouseSelector;
use crate::rules::foundation::Foundation;
use crate::rules::ini_parser::IniSection;

/// One direction of the advanced rubble switch: `Rubble.Destroyed*` on a
/// normal building (the rubble it turns into) or `Rubble.Intact*` on a
/// rubble (the building an engineer rebuilds), the arguments
/// `BuildingTypeExt::SwitchRubble @ 1000F2B0` takes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct RubbleSwitch {
    /// `Rubble.Destroyed` (`+0x8C`) / `Rubble.Intact` (`+0x88`): a
    /// BuildingType, through the Find-only `detail::read<BuildingTypeClass*>`
    /// (`1000CA20`).
    pub(crate) building_type: Option<String>,
    /// `Rubble.*.Anim` (`+0x90`/`+0x94`), Find-only
    /// `detail::read<AnimTypeClass*>` (`1000C9C0`).
    pub(crate) anim: Option<String>,
    /// `Rubble.*.Owner` (`+0x98`/`+0x9C`), resolved by
    /// `HouseExt::GetHouseKind` with random allowed.
    pub(crate) owner: HouseSelector,
    /// `Rubble.*.Strength` (`+0xA0`/`+0xA4`): -100..-1 is a percentage of
    /// the new type's Strength (at least 1), a positive value a capped
    /// health, anything else full Strength.
    pub(crate) strength: i32,
    /// `Rubble.*.Remove` (`+0xA8`/`+0xA9`): the building leaves the map
    /// without a successor.
    pub(crate) remove: bool,
}

impl RubbleSwitch {
    fn new(strength: i32) -> Self {
        Self {
            building_type: None,
            anim: None,
            owner: HouseSelector::Default,
            strength,
            remove: false,
        }
    }

    /// The hooks' gate: a successor type is set or the building is removed
    /// (`10017160`, `10017130`, `100257A0`, `10025420`).
    pub(crate) fn applies(&self) -> bool {
        self.building_type.is_some() || self.remove
    }
}

/// The building chain's BuildingTypeExt fields. The constructor
/// (`BuildingTypeExt::ExtData::ExtData @ 1000CAE0`) clears both rubble
/// types, anims and removes, sets both owners to `Default`, the destroyed
/// strength to 0 and the intact strength to -1, and leaves
/// `EngineerRepairable` unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingTypeExtRules {
    pub(crate) rubble_destroyed: RubbleSwitch,
    pub(crate) rubble_intact: RubbleSwitch,
    /// `EngineerRepairable` (`+0x231`, Nullable<bool> `1000E9B0`).
    pub(crate) engineer_repairable: Option<bool>,
}

impl Default for BuildingTypeExtRules {
    fn default() -> Self {
        Self {
            rubble_destroyed: RubbleSwitch::new(0),
            rubble_intact: RubbleSwitch::new(-1),
            engineer_repairable: None,
        }
    }
}

impl BuildingTypeExtRules {
    /// One reached BuildingType pass in the DLL's order (`1000DD40`):
    /// Destroyed before Intact for the types, the anims, the owners, the
    /// strengths and the removes, then `EngineerRepairable` near the end.
    /// `types` and `anims` are the registry's Find-only lookups in that
    /// order (`None` keeps the field, `Some(None)` clears it for a blank
    /// value); the rest are Valueable reads that keep their value on an
    /// absent, empty or invalid read.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        types: [Option<Option<String>>; 2],
        anims: [Option<Option<String>>; 2],
    ) {
        let [destroyed_type, intact_type] = types;
        let [destroyed_anim, intact_anim] = anims;
        if let Some(name) = destroyed_type {
            self.rubble_destroyed.building_type = name;
        }
        if let Some(name) = intact_type {
            self.rubble_intact.building_type = name;
        }
        if let Some(name) = destroyed_anim {
            self.rubble_destroyed.anim = name;
        }
        if let Some(name) = intact_anim {
            self.rubble_intact.anim = name;
        }
        // Valueable<OwnerHouseKind>::Read 1000A5B0.
        self.rubble_destroyed.owner = self
            .rubble_destroyed
            .owner
            .read(section, "Rubble.Destroyed.Owner");
        self.rubble_intact.owner = self
            .rubble_intact
            .owner
            .read(section, "Rubble.Intact.Owner");
        self.rubble_destroyed.strength =
            section.read_ares_int("Rubble.Destroyed.Strength", self.rubble_destroyed.strength);
        self.rubble_intact.strength =
            section.read_ares_int("Rubble.Intact.Strength", self.rubble_intact.strength);
        self.rubble_destroyed.remove =
            section.read_ares_bool("Rubble.Destroyed.Remove", self.rubble_destroyed.remove);
        self.rubble_intact.remove =
            section.read_ares_bool("Rubble.Intact.Remove", self.rubble_intact.remove);
        self.engineer_repairable =
            section.read_ares_nullable_bool("EngineerRepairable", self.engineer_repairable);
    }

    /// `CompleteInitialization @ 1000D380` (lambda `1000D1C0`): a rubble
    /// pair must share the building's foundation, else Ares stops the game
    /// with a fatal error. VERA cannot exit there; it reports the pair and
    /// drops the link, so no switch places a building over another
    /// foundation. `foundation_of` resolves a BuildingType by name.
    pub(crate) fn complete_initialization<'a>(
        &mut self,
        id: &str,
        foundation: &Foundation,
        foundation_of: impl Fn(&str) -> Option<&'a Foundation>,
    ) {
        for (key, switch) in [
            ("Rubble.Destroyed", &mut self.rubble_destroyed),
            ("Rubble.Intact", &mut self.rubble_intact),
        ] {
            let Some(other) = switch.building_type.as_deref() else {
                continue;
            };
            if !foundation_of(other).is_some_and(|other| foundation_equal(foundation, other)) {
                log::error!(
                    "BuildingType {id} and its {key} {other} don't have the same foundation \
                     (Ares stops the game here); the {key} link is dropped"
                );
                switch.building_type = None;
            }
        }
    }

    /// `EngineerRepairable.Get(Repairable)`.
    pub(crate) fn engineer_repairable(&self, repairable: bool) -> bool {
        self.engineer_repairable.unwrap_or(repairable)
    }
}

/// `BuildingTypeExt::IsFoundationEqual`, inlined into `1000D1C0`: the same
/// foundation selector, and for a custom one the same width, height and
/// cell set (both lists are sorted and deduplicated by the reader, so a
/// permutation test is an equality test).
fn foundation_equal(left: &Foundation, right: &Foundation) -> bool {
    if left.selector() != right.selector() {
        return false;
    }
    match (left, right) {
        (Foundation::Custom(_), Foundation::Custom(_)) => {
            left.dimensions() == right.dimensions() && left.cell_offsets() == right.cell_offsets()
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// The constructor's asymmetric strengths survive an empty pass, and
    /// a blank Find-only lookup clears a type while an invalid owner or
    /// strength keeps the current value.
    #[test]
    fn rubble_reads_keep_the_constructor_strengths_and_clear_on_blank() {
        let mut rules = BuildingTypeExtRules::default();
        let empty = IniFile::from_str("[B]\nStrength=1\n");
        rules.read_pass(empty.section("B").unwrap(), [None, None], [None, None]);
        assert_eq!(
            [
                rules.rubble_destroyed.strength,
                rules.rubble_intact.strength
            ],
            [0, -1]
        );
        let ini = IniFile::from_str(
            "[B]\nRubble.Destroyed.Owner=Neutral\nRubble.Intact.Owner=bogus\n\
             Rubble.Destroyed.Strength=-50\nRubble.Intact.Strength=x\n\
             Rubble.Intact.Remove=yes\nEngineerRepairable=no\n",
        );
        rules.read_pass(
            ini.section("B").unwrap(),
            [Some(Some("RUBBLE".into())), None],
            [None, Some(Some("FIRE".into()))],
        );
        assert_eq!(
            rules.rubble_destroyed.building_type.as_deref(),
            Some("RUBBLE")
        );
        assert_eq!(rules.rubble_destroyed.owner, HouseSelector::Neutral);
        assert_eq!(rules.rubble_intact.owner, HouseSelector::Default);
        assert_eq!(rules.rubble_destroyed.strength, -50);
        assert_eq!(rules.rubble_intact.strength, -1);
        assert!(rules.rubble_intact.applies() && rules.rubble_destroyed.applies());
        assert!(!rules.engineer_repairable(true));
        rules.read_pass(
            empty.section("B").unwrap(),
            [Some(None), None],
            [None, None],
        );
        assert_eq!(rules.rubble_destroyed.building_type, None);
    }
}
