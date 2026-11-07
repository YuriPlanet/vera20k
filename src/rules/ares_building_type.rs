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
//! - `LightningRod.Modifier` (`+0x60`) with TechnoType `LightningRod=`: the
//!   Ares Lightning Storm's strikes (`sim::superweapon::lightning_storm`).
//! - `SolidHeight` and `SolidLevel` (`+0x8`, `+0xC`) and RulesExt
//!   `AlliedSolidTransparency`: the building on a `SubjectToBuildings=`
//!   shot's line of fire (`sim::combat::line_of_fire`).
//! - `Firestorm.Wall` (`+0x5D`): a firestorm wall section
//!   (`sim::ares_firestorm`).

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
    /// `MassSelectable` (`+0x22C`, Nullable<bool>): whether a band box may
    /// select the building, else the stock IsUndeployable answer.
    mass_selectable: Option<bool>,
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
    /// `Returnable` (`+0xDC`, Nullable<bool>): a captured tech building goes
    /// back to its first owner when its holder is destroyed, else
    /// `[General] ReturnStructures=`.
    pub(crate) returnable: Option<bool>,
    /// `AIBuildCounts` (`+0x1EC`, ValueableVector<int>): per AI difficulty,
    /// how many of this type a computer base plans (at least one).
    pub(crate) ai_build_counts: Vec<i32>,
    /// `AIExtraCounts` (`+0x1F8`, ValueableVector<int>): per AI difficulty,
    /// up to how many more it plans at random.
    pub(crate) ai_extra_counts: Vec<i32>,
    /// Art `DockUnloadCell` (`+0x248`, Valueable<Point2D>): the pad offset
    /// from the dock's north-west cell.
    pub(crate) dock_unload_cell: [i32; 2],
    /// Art `DockUnloadFacing` (`+0x250`, Valueable<Facing32>, 0..31): the
    /// facing a unit docks and unloads at.
    pub(crate) dock_unload_facing: i32,
    /// `Degrade.Amount` (`+0x1D0`, Nullable<int>): the low-power damage per
    /// `DamageDelay`, else the RulesExt consumer or normal amount.
    pub(crate) degrade_amount: Option<i32>,
    /// `Degrade.Percentage` (`+0x1D8`, Nullable<double>): the health ratio
    /// the degrading stops at, else the RulesExt one.
    pub(crate) degrade_percentage: Option<NativeF64Bits>,
    /// `Message.Capture` (`+0x188`, CSF label): printed for the player who
    /// captures this tech building.
    pub(crate) message_capture: String,
    /// `Message.Lost` (`+0x1AC`, CSF label): printed for the player who loses
    /// it.
    pub(crate) message_lost: String,
    /// `AIBaseNormal` (`+0x235`, Nullable<bool>): whether the building counts
    /// for the AI base reservation, else the stock BaseNormal test.
    pub(crate) ai_base_normal: Option<bool>,
    /// `PrismForwarding.*` (`+0xE0`, `rules::ares_prism_forwarding`), read
    /// first by `1000DD40` through the registry.
    #[serde(default)]
    prism_forwarding: crate::rules::ares_prism_forwarding::PrismForwardingRules,
    /// `SolidHeight` (`+0x8`, Valueable<int>, constructor 0): how many levels
    /// of 256 leptons over its cell's floor the building stops shots; 0 never,
    /// negative its `Height=`. Read from art, then rules.
    #[serde(default)]
    pub(crate) solid_height: i32,
    /// `SolidLevel` (`+0xC`, Valueable<int>, constructor 1): the projectile
    /// `SolidLevel=` that shoots through it.
    #[serde(default = "default_solid_level")]
    pub(crate) solid_level: i32,
    /// TechnoType `LightningRod=` (`+0xD26`, ctor false), which
    /// `TechnoTypeClass::ReadINI @ 0x00712170` reads with ReadBool for every
    /// type. Only the Ares storm reads it (of a BuildingType), so only Ares
    /// builds keep it.
    #[serde(default)]
    pub(crate) lightning_rod: bool,
    /// `LightningRod.Modifier` (`+0x60`, Valueable<double>, ctor 1.0): the
    /// Ares storm's damage factor on a struck lightning rod.
    #[serde(default = "default_lightning_rod_modifier")]
    pub(crate) lightning_rod_modifier: NativeF64Bits,
    /// `Firestorm.Wall` (`+0x5D`, Valueable<bool>, ctor false): a section of
    /// its owner's firestorm wall, in place of the stock `FirestormWall=`
    /// at every hook of `Ext/Building/Hooks.Firewall.cpp`; also the
    /// `IsLinkable` test of placement line building (`1000D770`).
    #[serde(default)]
    pub(crate) firestorm_wall: bool,
}

fn default_lightning_rod_modifier() -> NativeF64Bits {
    NativeF64Bits::ONE
}

fn default_solid_level() -> i32 {
    1
}

impl Default for BuildingTypeExtRules {
    fn default() -> Self {
        Self {
            rubble_destroyed: RubbleSwitch::new(0),
            rubble_intact: RubbleSwitch::new(-1),
            engineer_repairable: None,
            mass_selectable: None,
            uc_pass_through: NativeF64Bits::POSITIVE_ZERO,
            uc_fatal_rate: NativeF64Bits::POSITIVE_ZERO,
            uc_damage_multiplier: NativeF64Bits::ONE,
            bunker_raidable: false,
            allowed_occupiers: Vec::new(),
            is_passable: false,
            buildup_time: None,
            sell_time: None,
            returnable: None,
            ai_build_counts: Vec::new(),
            ai_extra_counts: Vec::new(),
            dock_unload_cell: [3, 1],
            dock_unload_facing: 8,
            degrade_amount: None,
            degrade_percentage: None,
            message_capture: String::new(),
            message_lost: String::new(),
            ai_base_normal: None,
            prism_forwarding: Default::default(),
            solid_height: 0,
            solid_level: default_solid_level(),
            lightning_rod: false,
            lightning_rod_modifier: NativeF64Bits::ONE,
            firestorm_wall: false,
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
        art: &IniSection,
        references: BuildingTypeExtReferences,
    ) {
        // The art keys come first, from the type's Image= section: the
        // Point2D reader 1000FAE0 and the Facing32 reader 1000FBF0.
        self.dock_unload_cell = read_point(art, "DockUnloadCell", self.dock_unload_cell);
        self.dock_unload_facing = art.read_string_with(
            "DockUnloadFacing",
            2048,
            self.dock_unload_facing,
            |current, value| {
                crate::rules::ini_value::parse_ares_int(value)
                    .filter(|facing| (0..32).contains(facing))
                    .unwrap_or(current)
            },
        );
        // The Ares <= 0.9 art key, then the rules key (1000DD40 after the
        // dock keys).
        self.solid_height = art.read_ares_int("SolidHeight", self.solid_height);
        self.solid_height = section.read_ares_int("SolidHeight", self.solid_height);
        self.solid_level = section.read_ares_int("SolidLevel", self.solid_level);
        self.uc_pass_through =
            section.read_ares_double_bits("UC.PassThrough", self.uc_pass_through);
        self.uc_fatal_rate = section.read_ares_double_bits("UC.FatalRate", self.uc_fatal_rate);
        self.uc_damage_multiplier =
            section.read_ares_double_bits("UC.DamageMultiplier", self.uc_damage_multiplier);
        self.bunker_raidable = section.read_ares_bool("Bunker.Raidable", self.bunker_raidable);
        self.lightning_rod = section.read_bool("LightningRod", self.lightning_rod);
        self.lightning_rod_modifier =
            section.read_ares_double_bits("LightningRod.Modifier", self.lightning_rod_modifier);
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
        self.returnable = section.read_ares_nullable_bool("Returnable", self.returnable);
        self.message_capture =
            section.read_ares_csf_label("Message.Capture", &self.message_capture);
        self.message_lost = section.read_ares_csf_label("Message.Lost", &self.message_lost);
        self.degrade_amount = section.read_ares_nullable_int("Degrade.Amount", self.degrade_amount);
        self.degrade_percentage =
            section.read_ares_nullable_double_bits("Degrade.Percentage", self.degrade_percentage);
        self.is_passable = section.read_ares_bool("IsPassable", self.is_passable);
        self.firestorm_wall = section.read_ares_bool("Firestorm.Wall", self.firestorm_wall);
        if let Some(counts) = section.read_ares_int_list("AIBuildCounts") {
            self.ai_build_counts = counts;
        }
        if let Some(counts) = section.read_ares_int_list("AIExtraCounts") {
            self.ai_extra_counts = counts;
        }
        self.buildup_time =
            section.read_ares_nullable_double_bits("BuildupTime", self.buildup_time);
        self.sell_time = section.read_ares_nullable_double_bits("SellTime", self.sell_time);
        self.mass_selectable =
            section.read_ares_nullable_bool("MassSelectable", self.mass_selectable);
        self.engineer_repairable =
            section.read_ares_nullable_bool("EngineerRepairable", self.engineer_repairable);
        self.ai_base_normal = section.read_ares_nullable_bool("AIBaseNormal", self.ai_base_normal);
    }

    /// The tower's `PrismForwarding.*` keys.
    pub(crate) fn prism_forwarding(
        &self,
    ) -> &crate::rules::ares_prism_forwarding::PrismForwardingRules {
        &self.prism_forwarding
    }

    pub(crate) fn prism_forwarding_mut(
        &mut self,
    ) -> &mut crate::rules::ares_prism_forwarding::PrismForwardingRules {
        &mut self.prism_forwarding
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

    /// `Returnable.Get(RulesExt ReturnStructures)`.
    pub(crate) fn returnable(&self, return_structures: bool) -> bool {
        self.returnable.unwrap_or(return_structures)
    }

    /// The extra copies of this type a computer base plan takes at the
    /// difficulty `index` (`HouseClass_GenerateAIBuildList_CountExtra @
    /// 10023940`): none without an `AIBuildCounts` entry; else that count
    /// (at least one, the planned node itself) plus `RandomRanged(0,
    /// max(extra, 0))` when an `AIExtraCounts` entry exists, minus one.
    pub(crate) fn ai_extra_copies(
        &self,
        index: usize,
        mut random_ranged: impl FnMut(i32, i32) -> i32,
    ) -> i32 {
        let Some(&count) = self.ai_build_counts.get(index) else {
            return 0;
        };
        let mut count = count.max(1);
        if let Some(&extra) = self.ai_extra_counts.get(index) {
            count = count.wrapping_add(random_ranged(0, extra.max(0)));
        }
        // `1 < count` (`10023A0D`) guards the insertion loop.
        if count > 1 { count - 1 } else { 0 }
    }

    /// The dock's unload facing as a 16-bit direction (`DockUnloadFacing
    /// << 11`, `TechnoExt::GetDockUnloadFacing @ 10044C00`).
    pub(crate) fn dock_unload_direction(&self) -> u16 {
        (self.dock_unload_facing << 11) as u16
    }

    /// `EngineerRepairable.Get(Repairable)`.
    pub(crate) fn engineer_repairable(&self, repairable: bool) -> bool {
        self.engineer_repairable.unwrap_or(repairable)
    }

    /// `MassSelectable.Get(vanilla)`: Ares BuildingClass_CanBeSelectedNow_
    /// MassSelectable 10013CF0 (hook 0x459C03) over the IsUndeployable
    /// (`0x00465D40`) answer.
    pub(crate) fn mass_selectable(&self, vanilla: bool) -> bool {
        self.mass_selectable.unwrap_or(vanilla)
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

/// The building chain's RulesExt globals, read by `LoadBeforeTypeData`
/// (`1002AB60`) and `LoadAfterTypeData` (`1002A800`). The constructor
/// (`1002A360`) sets `EngineerDamage` to 0.0, `EngineerAlwaysCaptureTech`
/// to true, `ReturnStructures` and `Degrade.Enabled` to false,
/// `Degrade.AmountNormal` to 0 and `Degrade.AmountConsumer` to 1, and leaves
/// `Degrade.Percentage` unset, `TogglePowerAllowed` false, `TogglePowerDelay`
/// 45 and `[IQ] TogglePower` -1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingGlobalRules {
    /// `[General] EngineerDamage` (RulesExt `+0x10`, Valueable<double>): the
    /// share of a hostile building's Strength an engineer's entry destroys
    /// once it is above `EngineerCaptureLevel`; 0 bars his entry instead.
    pub(crate) engineer_damage: NativeF64Bits,
    /// `[General] EngineerAlwaysCaptureTech` (`+0x18`, Valueable<bool>): a
    /// building first owned by a `MultiplayPassive=` house is always captured.
    pub(crate) engineer_always_capture_tech: bool,
    /// `[General] ReturnStructures` (`+0x4A`, Valueable<bool>): the
    /// `Returnable=` default.
    pub(crate) return_structures: bool,
    /// `[General] Degrade.Enabled` (`+0xD4`): low power wears buildings down.
    pub(crate) degrade_enabled: bool,
    /// `[General] Degrade.Percentage` (`+0xD8`, Nullable<double>): the
    /// `Degrade.Percentage=` default, else `ConditionYellow=`.
    pub(crate) degrade_percentage: Option<NativeF64Bits>,
    /// `[General] Degrade.AmountNormal` (`+0xE8`): the amount for a
    /// building without `Power=` drain.
    pub(crate) degrade_amount_normal: i32,
    /// `[General] Degrade.AmountConsumer` (`+0xEC`): the amount for a
    /// power consumer.
    pub(crate) degrade_amount_consumer: i32,
    /// `[General] TogglePowerAllowed` (`+0x1C`): computer houses switch
    /// their buildings off and on to balance their power.
    pub(crate) toggle_power_allowed: bool,
    /// `[General] TogglePowerDelay` (`+0x20`, constructor 45): the frames
    /// between two balancing passes.
    pub(crate) toggle_power_delay: i32,
    /// `[IQ] TogglePower` (`+0x24`, constructor -1): the IQ a house needs.
    pub(crate) toggle_power_iq: i32,
    /// `[CombatDamage] AlliedSolidTransparency` (`+0x110`, Valueable<bool>,
    /// constructor false): a building lets its owner's allies' shots
    /// through.
    #[serde(default)]
    pub(crate) allied_solid_transparency: bool,
}

impl Default for BuildingGlobalRules {
    fn default() -> Self {
        Self {
            engineer_damage: NativeF64Bits::POSITIVE_ZERO,
            engineer_always_capture_tech: true,
            return_structures: false,
            degrade_enabled: false,
            degrade_percentage: None,
            degrade_amount_normal: 0,
            degrade_amount_consumer: 1,
            toggle_power_allowed: false,
            toggle_power_delay: 45,
            toggle_power_iq: -1,
            allied_solid_transparency: false,
        }
    }
}

impl BuildingGlobalRules {
    /// `1002AB60`'s `[General] ReturnStructures` and `Degrade.*` reads and
    /// `[CombatDamage] AlliedSolidTransparency`, before the types.
    pub(crate) fn read_before_type_data(&mut self, pass: &crate::rules::ini_parser::IniFile) {
        if let Some(section) = pass.section("General") {
            self.return_structures =
                section.read_ares_bool("ReturnStructures", self.return_structures);
            self.degrade_enabled = section.read_ares_bool("Degrade.Enabled", self.degrade_enabled);
            self.degrade_percentage = section
                .read_ares_nullable_double_bits("Degrade.Percentage", self.degrade_percentage);
            self.degrade_amount_normal =
                section.read_ares_int("Degrade.AmountNormal", self.degrade_amount_normal);
            self.degrade_amount_consumer =
                section.read_ares_int("Degrade.AmountConsumer", self.degrade_amount_consumer);
        }
        // After the Degrade.* keys.
        if let Some(section) = pass.section("CombatDamage") {
            self.allied_solid_transparency =
                section.read_ares_bool("AlliedSolidTransparency", self.allied_solid_transparency);
        }
    }

    /// `1002A800`'s `[General]` engineer reads, ahead of `HSBuilding`, and
    /// its later TogglePower keys.
    pub(crate) fn read_after_type_data(&mut self, pass: &crate::rules::ini_parser::IniFile) {
        if let Some(section) = pass.section("General") {
            self.engineer_damage =
                section.read_ares_double_bits("EngineerDamage", self.engineer_damage);
            self.engineer_always_capture_tech = section.read_ares_bool(
                "EngineerAlwaysCaptureTech",
                self.engineer_always_capture_tech,
            );
            // Later in 1002A800, after the drop pod keys.
            self.toggle_power_allowed =
                section.read_ares_bool("TogglePowerAllowed", self.toggle_power_allowed);
            self.toggle_power_delay =
                section.read_ares_int("TogglePowerDelay", self.toggle_power_delay);
        }
        if let Some(section) = pass.section("IQ") {
            self.toggle_power_iq = section.read_ares_int("TogglePower", self.toggle_power_iq);
        }
    }
}

/// `detail::read<Point2D> @ 1000FAE0`: ReadString(2048), then
/// `detail::parse<int>(value, 2)`; any parsed value stores both words. A
/// one-value read stores an uninitialized second word natively; VERA keeps
/// the previous point there, as it does for the coordinate reader.
fn read_point(section: &IniSection, key: &str, current: [i32; 2]) -> [i32; 2] {
    let pair = section.read_ares_partial_int_pair(
        key,
        crate::rules::ini_value::PartialIntPair {
            low: current[0],
            high: current[1],
            values: 2,
        },
    );
    if pair.values == 2 {
        [pair.low, pair.high]
    } else {
        current
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
        let art = IniFile::empty();
        let art = art.section_or_empty("B");
        rules.read_pass(empty.section("B").unwrap(), art, Default::default());
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
            art,
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
            art,
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
        let art = IniFile::empty();
        let art = art.section_or_empty("B");
        let mut rules = BuildingTypeExtRules::default();
        assert_eq!(rules.buildup_and_sell_time(general), (general, general));
        let ini = IniFile::from_str("[B]\nBuildupTime=0.5\n");
        rules.read_pass(ini.section("B").unwrap(), art, Default::default());
        let half = NativeF64Bits::from_bits(0.5f64.to_bits());
        assert_eq!(rules.buildup_and_sell_time(general), (half, half));
        let ini = IniFile::from_str("[B]\nSellTime=2\nBuildupTime=none\n");
        rules.read_pass(ini.section("B").unwrap(), art, Default::default());
        let two = NativeF64Bits::from_bits(2.0f64.to_bits());
        assert_eq!(rules.buildup_and_sell_time(general), (half, two));
    }

    /// `Message.Capture`/`Message.Lost` keep 31 bytes of their CSF label
    /// (`1000DD40`), and `LostEvaEvent` binds against EVAMD: the
    /// Initialize's `EVA_TechBuildingLost` unless set, `none` silences it.
    #[test]
    fn capture_lines_read_labels_and_bind_the_lost_eva() {
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=OIL\n1=LAB\n\
             [OIL]\nMessage.Capture=MSG:0123456789012345678901234567890123\nLostEvaEvent=none\n\
             [LAB]\nMessage.Lost=MSG:Lost\n",
        );
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        let eva = crate::rules::sound_ini::EvaRegistry::from_ini(&IniFile::from_str(
            "[DialogList]\n0=EVA_TechBuildingLost\n[EVA_TechBuildingLost]\nAllied=LOST\n",
        ));
        rules.bind_type_eva_references(&ini, &eva);
        let oil = rules.object("OIL").unwrap();
        assert_eq!(
            oil.building_type_ext().message_capture,
            "MSG:012345678901234567890123456"
        );
        assert_eq!(oil.lost_eva(), None);
        let lab = rules.object("LAB").unwrap();
        assert_eq!(lab.building_type_ext().message_lost, "MSG:Lost");
        assert_eq!(lab.building_type_ext().message_capture, "");
        assert_eq!(lab.lost_eva(), Some("EVA_TechBuildingLost"));
    }

    /// `AIBaseNormal` (`10011810`) replaces the stock BaseNormal test of
    /// the AI base reservation both ways.
    #[test]
    fn ai_base_normal_overrides_the_base_reservation_gate() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nAIBaseSpacing=3\n[VehicleTypes]\n0=MCV\n[BuildingTypes]\n0=POD\n1=PLAIN\n2=WALLS\n\
             [POD]\nUndeploysInto=MCV\nAIBaseNormal=yes\n[PLAIN]\nAIBaseNormal=no\n\
             [WALLS]\nUndeploysInto=MCV\n",
        ))
        .unwrap();
        let spacing = |id: &str| rules.object(id).unwrap().base_reservation_spacing;
        assert_eq!(spacing("POD"), Some(3));
        assert_eq!(spacing("PLAIN"), None);
        assert_eq!(
            spacing("WALLS"),
            None,
            "a 1x1 UndeploysInto type is strange"
        );
    }

    /// `MassSelectable` (10013CF0, hook 0x459C03) overrides the stock band
    /// test both ways; unset keeps it (a 1x1 `UndeploysInto=` type).
    #[test]
    fn mass_selectable_overrides_the_band_selection_test() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MCV\n[BuildingTypes]\n0=POD\n1=PLAIN\n2=SHY\n3=BOX\n\
             [POD]\nUndeploysInto=MCV\n[PLAIN]\n\
             [SHY]\nUndeploysInto=MCV\nMassSelectable=no\n[BOX]\nMassSelectable=yes\n",
        ))
        .unwrap();
        let band = |id: &str| rules.object(id).unwrap().band_selectable();
        assert!(band("POD"));
        assert!(!band("PLAIN"));
        assert!(!band("SHY"));
        assert!(band("BOX"));
    }

    /// The art `DockUnloadCell` (`1000FAE0`) takes two values, and
    /// `DockUnloadFacing` (`1000FBF0`) one in 0..31; anything else keeps
    /// the current value (the constructor's 3,1 and 8, east).
    #[test]
    fn dock_unload_art_keys_take_two_values_and_a_32_step_facing() {
        let mut rules = BuildingTypeExtRules::default();
        assert_eq!(rules.dock_unload_cell, [3, 1]);
        assert_eq!(rules.dock_unload_direction(), 0x4000);
        let ini = IniFile::from_str("[B]\nDockUnloadCell=9,9\n");
        for (text, cell, direction) in [
            (
                "DockUnloadCell=2,-1\nDockUnloadFacing=16\n",
                [2, -1],
                0x8000,
            ),
            ("DockUnloadCell=5\nDockUnloadFacing=32\n", [2, -1], 0x8000),
            ("DockUnloadFacing=31\n", [2, -1], 0xF800),
        ] {
            let art = IniFile::from_str(&format!("[B]\n{text}"));
            rules.read_pass(
                ini.section("B").unwrap(),
                art.section_or_empty("B"),
                Default::default(),
            );
            assert_eq!(rules.dock_unload_cell, cell, "{text:?}");
            assert_eq!(rules.dock_unload_direction(), direction);
        }
    }
}
