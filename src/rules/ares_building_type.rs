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
//! - Urban combat and garrisons (`UC.*`, `Bunker.Raidable`,
//!   `CanBeOccupiedBy`, `+0x68..+0x80`, `+0xD0`): `sim::ares_garrison`.
//! - `IsPassable` (`+0x5E`): the Infantry and Unit `Can_Enter_Cell` and
//!   InfantryAI building arms (`sim::world::object_entry`, `object_turn`).
//! - `BuildupTime` and `SellTime` (`+0x208`, `+0x218`): the build-up and
//!   sale rates (`rules::buildup_asset_catalog`, `sim::building_construction`).
//! - RulesExt `EngineerDamage` and `EngineerAlwaysCaptureTech`
//!   ([`BuildingGlobalRules`]): an engineer entering a hostile building.

use crate::rules::ares_warhead::HouseSelector;
use crate::rules::foundation::Foundation;
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

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
/// strength to 0 and the intact strength to -1, both UC rates to 0.0, the
/// UC damage multiplier to 1.0, `Bunker.Raidable` to false, the occupier
/// list to empty, and leaves `EngineerRepairable` unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingTypeExtRules {
    pub(crate) rubble_destroyed: RubbleSwitch,
    pub(crate) rubble_intact: RubbleSwitch,
    /// `EngineerRepairable` (`+0x231`, Nullable<bool> `1000E9B0`).
    pub(crate) engineer_repairable: Option<bool>,
    /// `UC.PassThrough` (`+0x68`): how often a shot passes through the
    /// building to one of its occupants (Valueable<double> `1000EB00`).
    pub(crate) uc_pass_through: NativeF64Bits,
    /// `UC.FatalRate` (`+0x70`): how often that occupant dies outright.
    pub(crate) uc_fatal_rate: NativeF64Bits,
    /// `UC.DamageMultiplier` (`+0x78`): the occupant's share of the damage.
    pub(crate) uc_damage_multiplier: NativeF64Bits,
    /// `Bunker.Raidable` (`+0x80`): an empty building of another house may
    /// be garrisoned, which raids it.
    pub(crate) bunker_raidable: bool,
    /// `CanBeOccupiedBy` (`+0xD0`): the InfantryTypes that may garrison it,
    /// any when empty (Find-only `ValueableVector<InfantryTypeClass*>`
    /// `1000ED90`).
    pub(crate) allowed_occupiers: Vec<String>,
    /// `IsPassable` (`+0x5E`, Valueable<bool>): Foots ignore the building
    /// when they enter its cells and when they idle in one.
    pub(crate) is_passable: bool,
    /// `BuildupTime` (`+0x208`, Nullable<double>, minutes): the build-up's
    /// duration, else `[General] BuildupTime=`.
    pub(crate) buildup_time: Option<NativeF64Bits>,
    /// `SellTime` (`+0x218`, Nullable<double>, minutes): the sale's
    /// pack-up duration, else the build-up's.
    pub(crate) sell_time: Option<NativeF64Bits>,
}

impl Default for BuildingTypeExtRules {
    fn default() -> Self {
        Self {
            rubble_destroyed: RubbleSwitch::new(0),
            rubble_intact: RubbleSwitch::new(-1),
            engineer_repairable: None,
            uc_pass_through: NativeF64Bits::POSITIVE_ZERO,
            uc_fatal_rate: NativeF64Bits::POSITIVE_ZERO,
            uc_damage_multiplier: NativeF64Bits::ONE,
            bunker_raidable: false,
            allowed_occupiers: Vec::new(),
            is_passable: false,
            buildup_time: None,
            sell_time: None,
        }
    }
}

/// The registry's Find-only type lookups of one BuildingType pass: `None`
/// keeps a field (absent or unknown), `Some(None)` clears a pointer for a
/// blank value, and a reached vector read replaces the list.
#[derive(Debug, Default)]
pub(crate) struct BuildingTypeExtReferences {
    /// `Rubble.Destroyed`, `Rubble.Intact` (`1000CA20`).
    pub(crate) rubble_types: [Option<Option<String>>; 2],
    /// `Rubble.Destroyed.Anim`, `Rubble.Intact.Anim` (`1000C9C0`).
    pub(crate) rubble_anims: [Option<Option<String>>; 2],
    /// `CanBeOccupiedBy` (`1000ED90`).
    pub(crate) allowed_occupiers: Option<Vec<String>>,
}

impl BuildingTypeExtRules {
    /// One reached BuildingType pass in the DLL's order (`1000DD40`): the
    /// UC block and `Bunker.Raidable`, then Destroyed before Intact for the
    /// rubble types, anims, owners, strengths and removes, then
    /// `CanBeOccupiedBy` and `EngineerRepairable`. `references` holds the
    /// registry's lookups; the rest are Valueable/Nullable reads that keep
    /// their value on an absent, empty or invalid read.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        references: BuildingTypeExtReferences,
    ) {
        self.uc_pass_through =
            section.read_ares_double_bits("UC.PassThrough", self.uc_pass_through);
        self.uc_fatal_rate = section.read_ares_double_bits("UC.FatalRate", self.uc_fatal_rate);
        self.uc_damage_multiplier =
            section.read_ares_double_bits("UC.DamageMultiplier", self.uc_damage_multiplier);
        self.bunker_raidable = section.read_ares_bool("Bunker.Raidable", self.bunker_raidable);
        let [destroyed_type, intact_type] = references.rubble_types;
        let [destroyed_anim, intact_anim] = references.rubble_anims;
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
        if let Some(occupiers) = references.allowed_occupiers {
            self.allowed_occupiers = occupiers;
        }
        self.is_passable = section.read_ares_bool("IsPassable", self.is_passable);
        self.buildup_time =
            section.read_ares_nullable_double_bits("BuildupTime", self.buildup_time);
        self.sell_time = section.read_ares_nullable_double_bits("SellTime", self.sell_time);
        self.engineer_repairable =
            section.read_ares_nullable_bool("EngineerRepairable", self.engineer_repairable);
    }

    /// `BuildingTypeExt::ExtData::UpdateBuildupFrames @ 1000F450`'s two
    /// durations in minutes: `BuildupTime.Get(Rules BuildupTime)` and
    /// `SellTime.Get(that)`.
    pub(crate) fn buildup_and_sell_time(
        &self,
        rules_buildup_time: NativeF64Bits,
    ) -> (NativeF64Bits, NativeF64Bits) {
        let buildup = self.buildup_time.unwrap_or(rules_buildup_time);
        (buildup, self.sell_time.unwrap_or(buildup))
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

    /// `BuildingTypeExt::ExtData::CanBeOccupiedBy`: any InfantryType while
    /// the list is empty, else one it names.
    pub(crate) fn can_be_occupied_by(&self, infantry_type: &str) -> bool {
        self.allowed_occupiers.is_empty()
            || self
                .allowed_occupiers
                .iter()
                .any(|id| id.eq_ignore_ascii_case(infantry_type))
    }
}

/// The building chain's RulesExt globals, read by `LoadAfterTypeData`
/// (`1002A800`). The constructor (`1002A360`) sets `EngineerDamage` to 0.0
/// and `EngineerAlwaysCaptureTech` to true.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingGlobalRules {
    /// `[General] EngineerDamage` (RulesExt `+0x10`, Valueable<double>): the
    /// share of a hostile building's Strength an engineer's entry destroys
    /// once it is above `EngineerCaptureLevel`; 0 bars his entry instead.
    pub(crate) engineer_damage: NativeF64Bits,
    /// `[General] EngineerAlwaysCaptureTech` (`+0x18`, Valueable<bool>): a
    /// building first owned by a `MultiplayPassive=` house is always captured.
    pub(crate) engineer_always_capture_tech: bool,
}

impl Default for BuildingGlobalRules {
    fn default() -> Self {
        Self {
            engineer_damage: NativeF64Bits::POSITIVE_ZERO,
            engineer_always_capture_tech: true,
        }
    }
}

impl BuildingGlobalRules {
    /// `1002A800`'s two `[General]` reads, ahead of `HSBuilding`.
    pub(crate) fn read_after_type_data(&mut self, pass: &crate::rules::ini_parser::IniFile) {
        if let Some(section) = pass.section("General") {
            self.engineer_damage =
                section.read_ares_double_bits("EngineerDamage", self.engineer_damage);
            self.engineer_always_capture_tech = section.read_ares_bool(
                "EngineerAlwaysCaptureTech",
                self.engineer_always_capture_tech,
            );
        }
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
        rules.read_pass(empty.section("B").unwrap(), Default::default());
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
            BuildingTypeExtReferences {
                rubble_types: [Some(Some("RUBBLE".into())), None],
                rubble_anims: [None, Some(Some("FIRE".into()))],
                allowed_occupiers: None,
            },
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
            BuildingTypeExtReferences {
                rubble_types: [Some(None), None],
                ..Default::default()
            },
        );
        assert_eq!(rules.rubble_destroyed.building_type, None);
    }

    /// `UpdateBuildupFrames` (`1000F450`): `SellTime` falls back to the
    /// type's `BuildupTime`, which falls back to the Rules one; `none` is
    /// no number, so it keeps the earlier value.
    #[test]
    fn sell_time_falls_back_to_the_buildup_time() {
        let general = NativeF64Bits::from_bits(0.06f64.to_bits());
        let mut rules = BuildingTypeExtRules::default();
        assert_eq!(rules.buildup_and_sell_time(general), (general, general));
        let ini = IniFile::from_str("[B]\nBuildupTime=0.5\n");
        rules.read_pass(ini.section("B").unwrap(), Default::default());
        let half = NativeF64Bits::from_bits(0.5f64.to_bits());
        assert_eq!(rules.buildup_and_sell_time(general), (half, half));
        let ini = IniFile::from_str("[B]\nSellTime=2\nBuildupTime=none\n");
        rules.read_pass(ini.section("B").unwrap(), Default::default());
        let two = NativeF64Bits::from_bits(2.0f64.to_bits());
        assert_eq!(rules.buildup_and_sell_time(general), (half, two));
    }
}
