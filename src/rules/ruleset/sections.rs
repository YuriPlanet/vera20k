//! The smaller global rules blocks read beside `GeneralRules`: production
//! speeds, paradrop lists, difficulty rows, general building types, detail
//! levels, prism support, garrison, elevation, bridge and radiation rules.

use std::hash::Hash;

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// The `[General]` keys `TechnoClass::Time_To_Build @ 0x006F47A0` reads,
/// stored as the RulesClass fields hold them. The reader (`0x0066D530`) reads
/// each with `INIClass::ReadDouble @ 0x005283D0`, the field's current value as
/// default and no clamp; the values below are the constructor's
/// (`RulesClass @ 0x00665650`).
#[derive(Debug, Clone, Copy)]
pub struct ProductionRules {
    /// `BuildSpeed=`: minutes to build a 1000-credit object (`+0x1748`, double;
    /// 1.0).
    pub build_speed: NativeF64Bits,
    /// `MultipleFactory=`: the time factor per extra factory (`+0x57C`, float;
    /// 1.0).
    pub multiple_factory: NativeF32Bits,
    /// `LowPowerPenaltyModifier=` (`+0x578`, float; 1.0).
    pub low_power_penalty_modifier: NativeF32Bits,
    /// `MinLowPowerProductionSpeed=` (`+0x570`, float; 0.5).
    pub min_low_power_production_speed: NativeF32Bits,
    /// `MaxLowPowerProductionSpeed=` (`+0x574`, float; 0.9).
    pub max_low_power_production_speed: NativeF32Bits,
    /// `WallBuildSpeedCoefficient=`: a wall's time factor (`+0x758`, double;
    /// 0.5).
    pub wall_build_speed_coefficient: NativeF64Bits,
}

impl Default for ProductionRules {
    fn default() -> Self {
        Self {
            build_speed: NativeF64Bits::ONE,
            multiple_factory: NativeF32Bits::ONE,
            low_power_penalty_modifier: NativeF32Bits::ONE,
            min_low_power_production_speed: NativeF32Bits::from_bits(0.5_f32.to_bits()),
            max_low_power_production_speed: NativeF32Bits::from_bits(0x3F66_6666),
            wall_build_speed_coefficient: NativeF64Bits::HALF,
        }
    }
}

impl ProductionRules {
    /// The RulesClass `[General]` reads, in native order: the four floats
    /// (`0x0066EB5B..0x0066EBC9`), then `BuildSpeed=` (`0x00670D23`) and
    /// `WallBuildSpeedCoefficient=` (`0x0067187F`). An absent section skips
    /// them all (the section test `0x00526810` jumps to `0x00671E8E`).
    pub(super) fn from_ini(ini: &IniFile) -> Self {
        let mut rules = Self::default();
        let Some(general) = ini.section("General") else {
            return rules;
        };
        rules.min_low_power_production_speed = general.read_double_to_float(
            "MinLowPowerProductionSpeed",
            rules.min_low_power_production_speed,
        );
        rules.max_low_power_production_speed = general.read_double_to_float(
            "MaxLowPowerProductionSpeed",
            rules.max_low_power_production_speed,
        );
        rules.low_power_penalty_modifier = general
            .read_double_to_float("LowPowerPenaltyModifier", rules.low_power_penalty_modifier);
        rules.multiple_factory =
            general.read_double_to_float("MultipleFactory", rules.multiple_factory);
        rules.build_speed = general.read_double_bits("BuildSpeed", rules.build_speed);
        rules.wall_build_speed_coefficient = general.read_double_bits(
            "WallBuildSpeedCoefficient",
            rules.wall_build_speed_coefficient,
        );
        rules
    }
}

/// One side's paradrop lists as `RulesClass::ReadGeneral` keeps them
/// (`0x0067062F..0x006707C1`), each emptied by the constructor
/// (`0x00666649..0x006666D8`) and kept by a pass that lacks its key.
///
/// The two are separate vectors: nothing here pairs them or compares their
/// lengths. Their consumers do (`superweapon::paradrop`, which pairs them by
/// index, and `superweapon::spy_plane`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParadropList {
    /// `*ParaDropInf=` as stored InfantryType IDs, through the InfantryType
    /// list reader (`0x0067BB10`: ReadString 0x80, `strtok(",")`, and
    /// `InfantryTypeClass::FindOrAllocate @ 0x00524CB0`, which drops `none`
    /// and `<none>` and allocates a name it does not know). The processed
    /// rules own it (`native_processing`).
    pub infantry: Vec<String>,
    /// `*ParaDropNum=` through the IntVector reader (`0x00475D70`); Ares
    /// 3.0 Buf_General (1005BB00, hook 0x66D55E) reads it through
    /// ParseList<int> (1005B030) instead.
    pub counts: Vec<i32>,
}

impl ParadropList {
    /// The counts as the projected INI holds them; the infantry are the
    /// processed rules' ([`RuleSet::from_processed_rules`]).
    pub(super) fn read_counts(general: &crate::rules::ini_parser::IniSection, key: &str) -> Self {
        let counts = if crate::rules::ares_switch::ares_way() {
            crate::rules::ares_switch::not_ares_read(|| general.read_ares_int_list(key))
        } else {
            general.read_int_list(key)
        };
        Self {
            infantry: Vec::new(),
            counts: counts.unwrap_or_default(),
        }
    }
}

/// `RulesClass::ReadDifficulty @ 0x0066D270`'s `RepairDelay=` default, the
/// double nearest .02 (`0x3F947AE147AE147B`, pushed at `0x0066D317`).
const DIFFICULTY_REPAIR_DELAY_DEFAULT: f64 = 0.02;

/// One original ReadDifficulty66D270 row: its scalars and `DestroyWalls=`.
/// `BuildSlowdown=` (`+0x48`) and `ContentScan=` (`+0x4A`) have no reader yet.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DifficultyRules {
    pub firepower: f64,
    pub ground_speed: f64,
    pub air_speed: f64,
    pub armor: f64,
    pub rof: f64,
    pub cost: f64,
    pub build_time: f64,
    pub repair_delay: f64,
    pub build_delay: f64,
    /// `DestroyWalls=` (`+0x49`, ReadBool `0x0066D374..0x0066D385`, default
    /// yes): a computer house at this difficulty lets its passive scan pick
    /// an enemy wall ([`crate::sim::combat::greatest_threat`]'s wall
    /// fallback, `0x006F8C10`). Retail RULESMD sets `no` on all three rows.
    pub destroy_walls: bool,
}

impl Default for DifficultyRules {
    fn default() -> Self {
        Self {
            firepower: 1.0,
            ground_speed: 1.0,
            air_speed: 1.0,
            armor: 1.0,
            rof: 1.0,
            cost: 1.0,
            build_time: 1.0,
            repair_delay: DIFFICULTY_REPAIR_DELAY_DEFAULT,
            build_delay: 0.03,
            destroy_walls: true,
        }
    }
}

impl DifficultyRules {
    /// Called only for a present row section. Each read uses a literal
    /// default, independent of the preceding Process pass. Native key order:
    /// RepairDelay, BuildDelay, BuildSlowdown, BuildTime, DestroyWalls,
    /// ContentScan.
    pub(crate) fn read_pass(section: &crate::rules::ini_parser::IniSection) -> Self {
        let firepower = section.read_double("FirePower", 1.0);
        let ground_speed = section.read_double("Groundspeed", 1.0);
        let air_speed = section.read_double("Airspeed", 1.0);
        let armor = section.read_double("Armor", 1.0);
        let rof = section.read_double("ROF", 1.0);
        let cost = section.read_double("Cost", 1.0);
        let repair_delay = section.read_double("RepairDelay", DIFFICULTY_REPAIR_DELAY_DEFAULT);
        let build_delay = section.read_double("BuildDelay", 0.03);
        let build_time = section.read_double("BuildTime", 1.0);
        let destroy_walls = section.read_bool("DestroyWalls", true);
        Self {
            firepower,
            ground_speed,
            air_speed,
            armor,
            rof,
            cost,
            build_time,
            repair_delay,
            build_delay,
            destroy_walls,
        }
    }
}

/// The single BuildingTypes `RulesClass::ReadGeneral` names in `[General]`,
/// as stored IDs. The ordered Process owner (`native_processing`) reads them
/// pass by pass; `RuleSet` publishes its result.
///
/// The gates and WallTower (`Rules+0x86C..+0x87C`, read at
/// `0x0066F450..0x0066F583`) are the types `CellClass::Is_Clear_To_Build` lets
/// stand on their own house's wall overlay (`sim::build_site`): WallTower and
/// the two GDI gates over GASAND/GAWALL (`0x0047C8DD..0x0047C8F7`), the two
/// Nod gates over NAWALL (`0x0047C92F..0x0047C943`). Retail names GADUMY for
/// all five. The four power plants (`Rules+0x89C..+0x8A8`, read at
/// `0x0066F692..0x0066F781`) are the plant the computer's building choice
/// splices into its BasePlan (`sim::ai_base_building`); retail GAPOWR, NAPOWR,
/// NANRCT and YAPOWR.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GeneralBuildingTypes {
    pub gdi_gate_one: Option<String>,
    pub gdi_gate_two: Option<String>,
    pub nod_gate_one: Option<String>,
    pub nod_gate_two: Option<String>,
    pub wall_tower: Option<String>,
    /// `GDIPowerPlant=` (`Rules+0x89C`).
    pub gdi_power_plant: Option<String>,
    /// `NodRegularPower=` (`Rules+0x8A0`).
    pub nod_regular_power: Option<String>,
    /// `NodAdvancedPower=` (`Rules+0x8A4`).
    pub nod_advanced_power: Option<String>,
    /// `ThirdPowerPlant=` (`Rules+0x8A8`).
    pub third_power_plant: Option<String>,
}

impl GeneralBuildingTypes {
    /// `type == Rules+0x87C || +0x86C || +0x870`.
    pub fn stands_on_gdi_wall(&self, type_id: &str) -> bool {
        [&self.wall_tower, &self.gdi_gate_one, &self.gdi_gate_two]
            .into_iter()
            .any(|name| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(type_id))
            })
    }

    /// `type == Rules+0x874 || +0x878`.
    pub fn stands_on_nod_wall(&self, type_id: &str) -> bool {
        [&self.nod_gate_one, &self.nod_gate_two]
            .into_iter()
            .any(|name| {
                name.as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(type_id))
            })
    }
}

/// Retained `[AudioVisual]` frame-rate thresholds. The RulesClass constructor
/// writes 15/20/5 at +0/+4/+8 (665665..665672). ReadAudioVisual66920D..669258
/// reads signed integers in this order with current defaults and no clamps.
/// Native constructor/full-reader controls: rules_oracle/weapon_laser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DetailRules {
    pub min_frame_rate_normal: i32,
    pub min_frame_rate_movie: i32,
    pub buffer_zone_width: i32,
}

impl Default for DetailRules {
    fn default() -> Self {
        Self {
            min_frame_rate_normal: 15,
            min_frame_rate_movie: 20,
            buffer_zone_width: 5,
        }
    }
}

impl DetailRules {
    pub(crate) fn read_pass(self, audio_visual: &IniSection) -> Self {
        Self {
            min_frame_rate_normal: audio_visual
                .read_int("DetailMinFrameRateNormal", self.min_frame_rate_normal),
            min_frame_rate_movie: audio_visual
                .read_int("DetailMinFrameRateMovie", self.min_frame_rate_movie),
            buffer_zone_width: audio_visual
                .read_int("DetailBufferZoneWidth", self.buffer_zone_width),
        }
    }
}

/// The `[General]` Prism support keys, read by `RulesClass::ReadGeneral`
/// (`0x0066D530`) on every rules pass that has a `[General]` section, each
/// with its current value as the default (`0x0067114F..0x006711B8`). The
/// beam's `PrismSupportDuration=` (`Rules+0x4A8`) feeds the support laser;
/// `PrismSupportHeight=` (`Rules+0x4AC`) has no
/// reader outside the constructor and ReadGeneral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PrismSupportRules {
    /// `PrismSupportModifier=` (`Rules+0x49C`), the percent each supporter
    /// adds to the shot. Original gamemd uses the scaled integer as the
    /// ReadDouble default; Ares converts that default back to a fraction.
    pub modifier: i32,
    /// `PrismSupportMax=` (`Rules+0x4A0`), the supporters one shot recruits.
    pub max: i32,
    /// `PrismSupportDelay=` (`Rules+0x4A4`), a supporter's downtime in frames.
    pub delay: i32,
    /// `PrismSupportDuration=` (`Rules+0x4A8`), signed support-laser lifetime
    /// in frames. ReadInt retains the current value; no clamp or byte cast.
    pub duration: i32,
}

impl Default for PrismSupportRules {
    /// The RulesClass constructor's values (`0x00665CF3`, `0x00665CF9`,
    /// `0x00665D03`).
    fn default() -> Self {
        Self {
            modifier: 100,
            max: 8,
            delay: 100,
            duration: 15,
        }
    }
}

impl PrismSupportRules {
    /// One ReadGeneral pass over a `[General]` section. The modifier is
    /// `fmul qword 100.0` (`0x0067116E`) on the value ReadDouble returns, then
    /// ftol (`0x007C5F00`), under the game's masked chop control word, which
    /// also chops ReadDouble's own percent product: a modded `35%` reads 34.
    /// Ares fixes the default's units before that read (see below).
    pub(crate) fn read_pass(self, general: &crate::rules::ini_parser::IniSection) -> Self {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        let current = f64::from(self.modifier);
        // Ares RulesClass_Addition_General_PrismSupportModifier @ 0x1005D040,
        // hook0x00671152: FILD Rules+49C; FDIV 100.0; pass that fraction to
        // ReadDouble. Without this, each keyless map/mode multiplies the
        // retained percentage by another 100 (original ReadGeneral67114F).
        let current = if crate::rules::ares_switch::ares_way() {
            f64::from_bits(
                X87::store_f64_masked_chop(X87::div(
                    X87::load_f64(NativeF64Bits::from_bits(current.to_bits())),
                    X87::load_f64(NativeF64Bits::from_bits(100.0_f64.to_bits())),
                ))
                .bits(),
            )
        } else {
            current
        };
        let percent = general.read_double("PrismSupportModifier", current);
        let modifier = X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(percent.to_bits())),
            X87::load_f64(NativeF64Bits::from_bits(100.0_f64.to_bits())),
        ));
        Self {
            modifier,
            max: general.read_int("PrismSupportMax", self.max),
            delay: general.read_int("PrismSupportDelay", self.delay),
            duration: general.read_int("PrismSupportDuration", self.duration),
        }
    }
}

/// Garrison/occupation combat rules parsed from `[CombatDamage]` in `rules(md).ini`.
/// These global multipliers govern how garrisoned infantry fire from buildings.
#[derive(Debug, Clone)]
pub struct GarrisonRules {
    /// Damage multiplier applied to garrison fire: the f32 at `Rules+0xF40`
    /// that FireAt multiplies on the x87 (`0x006FE3F1`).
    pub occupy_damage_multiplier: f32,
    /// `[CombatDamage] OccupyROFMultiplier=`, the single at `Rules+0xF44`
    /// (ReadDouble stored with `FSTP dword`, `0x0066C6A9..0x0066C6B4`;
    /// constructor 1.0f, `0x00666AB6`). GetROF divides a garrison's reload by
    /// it on the x87 when it is greater than zero (`0x006FD19C`).
    pub occupy_rof_multiplier: f32,
    /// Fixed weapon range in cells for garrisoned fire, replaces weapon's own range.
    pub occupy_weapon_range: i32,
    /// Damage multiplier for bunker passengers.
    pub bunker_damage_multiplier: f32,
    /// `[CombatDamage] BunkerROFMultiplier=`, the single at `Rules+0xF50`
    /// (`0x0066C712..0x0066C71D`; constructor 1.0f, `0x00666ACD`). GetROF
    /// divides a bunkered unit's reload by it when it is non-zero
    /// (`0x006FD1B1..0x006FD1EF`).
    pub bunker_rof_multiplier: f32,
    /// `[CombatDamage] BunkerWeaponRangeBonus=` cells, Rules+F54.
    /// Constructor666AC8..666AD3 stores2; reader66C723..66C73D uses the
    /// current signed value as its ReadInteger default, without a clamp.
    /// Executed constructor/reader controls: walk_cell_range_bunker.json.
    pub bunker_weapon_range_bonus: i32,
    /// `[CombatDamage] OpenToppedDamageMultiplier=`, the single at
    /// `Rules+0xF58` (constructor 1.0f `0x00666AD9`; ReadDouble stored with
    /// `FSTP dword` at `0x0066C743..`). FireAt multiplies an open-topped
    /// passenger's damage by it (`0x006FE43B`).
    pub open_topped_damage_multiplier: f32,
    /// `[CombatDamage] OpenToppedRangeBonus=` in cells, `Rules+0xF5C`
    /// (constructor 2, `0x00666AD9..`; ReadInt at `0x0066C743..0x0066C7AC`).
    /// InRange adds it, shifted to leptons, for an open-topped passenger
    /// (`0x006F72C8`).
    pub open_topped_range_bonus: i32,
}

impl Default for GarrisonRules {
    fn default() -> Self {
        Self {
            occupy_damage_multiplier: 1.0,
            occupy_rof_multiplier: 1.0,
            occupy_weapon_range: 5,
            bunker_damage_multiplier: 1.0,
            bunker_rof_multiplier: 1.0,
            bunker_weapon_range_bonus: 2,
            open_topped_damage_multiplier: 1.0,
            open_topped_range_bonus: 2,
        }
    }
}

/// `[ElevationModel]`: the range bonus `TechnoClass::InRange` gives a shot
/// fired down at a lower target (`0x006F6F60` on the direct arm, `0x006F70E0`
/// on the arcing arm). `RulesClass::ReadElevationModel @ 0x0066D150`, called
/// by `RulesClass::Process` at `0x00668F46`, reads the three keys only when the
/// pass has the section (`0x00526810`), each defaulting to its current value.
#[derive(Debug, Clone, Copy)]
pub struct ElevationModel {
    /// `ElevationIncrement=` (`Rules+0x1838`, ReadInt `0x0066D183`): levels of
    /// height per bonus step. InRange divides by it unchecked (`IDIV` at
    /// `0x006F705A` and `0x006F71D6`).
    ///
    /// RESIDUAL: at 0, the constructor's value, that division faults natively
    /// on the first `SubjectToElevation=` shot that passes both vt+0x50 checks
    /// (a grounded firer at a grounded object or a dry cell), even with no
    /// drop; VERA adds no bonus. Dormant: retail `rulesmd.ini` sets 4, so only
    /// a map or mode that sets 0 reaches it.
    pub increment: i32,
    /// `ElevationIncrementBonus=` (`Rules+0x1840`, ReadDouble `0x0066D1AA`):
    /// cells of range per step.
    pub increment_bonus: NativeF64Bits,
    /// `ElevationBonusCap=` (`Rules+0x1848`, ReadDouble `0x0066D1D1`): the most
    /// cells the steps add.
    pub bonus_cap: NativeF64Bits,
}

impl Default for ElevationModel {
    /// The constructor's stores (`0x00667807..0x0066781F`): `EBX` is 0 and
    /// `EBP` holds `0x3FF00000`, the high dword of 1.0.
    fn default() -> Self {
        Self {
            increment: 0,
            increment_bonus: NativeF64Bits::ONE,
            bonus_cap: NativeF64Bits::POSITIVE_ZERO,
        }
    }
}

impl ElevationModel {
    pub(super) fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("ElevationModel");
        let d = Self::default();
        Self {
            increment: section.read_int("ElevationIncrement", d.increment),
            increment_bonus: section.read_double_bits("ElevationIncrementBonus", d.increment_bonus),
            bonus_cap: section.read_double_bits("ElevationBonusCap", d.bonus_cap),
        }
    }
}

impl GarrisonRules {
    pub(super) fn from_ini(ini: &IniFile) -> Self {
        // The multipliers are float fields (`FSTP dword` after each ReadDouble,
        // `0x0066C68A..0x0066C75C`).
        let section = ini.section_or_empty("CombatDamage");
        let d = Self::default();
        Self {
            occupy_damage_multiplier: section
                .read_float("OccupyDamageMultiplier", d.occupy_damage_multiplier),
            occupy_rof_multiplier: section
                .read_float("OccupyROFMultiplier", d.occupy_rof_multiplier),
            occupy_weapon_range: section.read_int("OccupyWeaponRange", d.occupy_weapon_range),
            bunker_damage_multiplier: section
                .read_float("BunkerDamageMultiplier", d.bunker_damage_multiplier),
            bunker_rof_multiplier: section
                .read_float("BunkerROFMultiplier", d.bunker_rof_multiplier),
            bunker_weapon_range_bonus: section
                .read_int("BunkerWeaponRangeBonus", d.bunker_weapon_range_bonus),
            open_topped_damage_multiplier: section.read_float(
                "OpenToppedDamageMultiplier",
                d.open_topped_damage_multiplier,
            ),
            open_topped_range_bonus: section
                .read_int("OpenToppedRangeBonus", d.open_topped_range_bonus),
        }
    }
}

/// Bridge damage/destruction rules parsed from `rules(md).ini`.
#[derive(Debug, Clone)]
pub struct BridgeRules {
    /// Signed upper bound of the per-hit bridge damage admission draw.
    pub strength: i32,
    /// Reset/default value for `SpecialFlags::DestroyableBridges`.
    ///
    /// `[CombatDamage] DestroyableBridges=` exists in retail INI text but is
    /// not read by gamemd as the gameplay gate.
    #[cfg(test)]
    pub destroyable_by_default: bool,
    /// SHP animation names to spawn when a bridge group is destroyed
    /// (e.g., TWLT026, TWLT036, TWLT050, TWLT070). Picked randomly per cell.
    pub explosions: Vec<String>,
    /// Maximum metallic-debris voxels spawned per destroyed bridge cell.
    /// Parsed from `[General] BridgeVoxelMax=` in rules.ini (default 3).
    /// Consumed by the damage state machine in a later tier.
    #[cfg(test)]
    pub voxel_max: u8,
    /// Sound ID played when a bridge segment is repaired by an
    /// Engineer entering a `BridgeRepairHut=yes` building.
    /// Parsed from `[AudioVisual] RepairBridgeSound=` in rules.ini
    /// (stock default `BridgeRepaired`). Stored uppercased.
    /// `None` means the consumer applies its own default.
    pub repair_sound: Option<String>,
}

impl Default for BridgeRules {
    fn default() -> Self {
        Self {
            strength: 1000,
            #[cfg(test)]
            destroyable_by_default: true,
            explosions: Vec::new(),
            #[cfg(test)]
            voxel_max: 3,
            repair_sound: None,
        }
    }
}

impl BridgeRules {
    pub(super) fn from_ini(ini: &IniFile) -> Self {
        // Rules ctor6675DA sets1000; ReadCombatDamage66CD66..66CD86 retains
        // the current signed dword as ReadInteger's default, with no clamp.
        let strength = ini
            .section("CombatDamage")
            .map_or(1000, |section| section.read_int("BridgeStrength", 1000));
        // Published from the ordered RulesClass reader by from_processed_rules.
        let explosions = Vec::new();
        #[cfg(test)]
        let voxel_max = ini
            .section_or_empty("General")
            .read_int("BridgeVoxelMax", 3)
            .clamp(0, 255) as u8;
        let repair_sound = ini
            .section_or_empty("AudioVisual")
            .read_name("RepairBridgeSound", 0x80)
            .map(str::to_uppercase);
        Self {
            strength,
            #[cfg(test)]
            destroyable_by_default: true,
            explosions,
            #[cfg(test)]
            voxel_max,
            repair_sound,
        }
    }
}

/// Global radiation-field constants parsed from the `[Radiation]` section.
/// Consumed by the per-cell radiation service (`sim::radiation`) and the
/// per-foot-unit damage step. Render-only keys (light/tint/color) are parsed
/// here so the render layer can pick them up later.
#[derive(Debug, Clone)]
pub struct RadiationRules {
    /// Frames a site lasts per point of radiation level (`RadDurationMultiple`).
    /// Site duration = level × this.
    pub duration_multiple: i32,
    /// Frames between radiation damage applications to units (`RadApplicationDelay`).
    pub application_delay: i32,
    /// Cap on the level a cell damages as, not on storage (`RadLevelMax`).
    pub level_max: i32,
    /// Frames between per-cell level decrements (`RadLevelDelay`).
    pub level_delay: i32,
    /// Frames between light intensity decrements (`RadLightDelay`). Render-only.
    pub light_delay: i32,
    /// Damage per point of (clamped) cell level (`RadLevelFactor`).
    /// Carried as f64 — the damage step truncates `level × factor` toward
    /// zero under the original PC53/chop arithmetic. The shared ReadDouble
    /// reader scans binary32 and widens it to this stored double.
    pub level_factor: f64,
    /// Light intensity per level point (`RadLightFactor`). Render-only.
    pub light_factor: SimFixed,
    /// Tint scale for the radiation glow (`RadTintFactor`). Render-only.
    pub tint_factor: SimFixed,
    /// Glow color (`RadColor=R,G,B`). Render-only.
    pub color: (u8, u8, u8),
    /// Warhead used for radiation damage (`RadSiteWarhead=`, Rules+1834), as
    /// the ordered Rules processor resolved it: empty for the constructor's
    /// null (`0x00667801`) or the factory's null for `none`. An empty read
    /// keeps it (`0x0066D0EA` reloads the field, `0x0066D127` stores it back).
    pub site_warhead: String,
    /// Phobos `[Radiation] UseGlobalRadApplicationDelay` (default true): when
    /// true every radiation type shares the global `RadApplicationDelay`
    /// gate, and the per-type delay is not consulted.
    #[cfg(feature = "phobos")]
    pub use_global_rad_application_delay: bool,
    /// Phobos `[Radiation] RadApplicationDelay.Building` (default 0). Above 0
    /// it enables radiation damage to buildings; the building damage chain is
    /// not implemented, so the value is retained only.
    #[cfg(feature = "phobos")]
    pub application_delay_building: i32,
    /// Phobos `[Radiation] RadBuildingDamageMaxCount` (default -1: no limit).
    /// Retained only, like the building delay above.
    #[cfg(feature = "phobos")]
    pub building_damage_max_count: i32,
    /// Phobos `[Radiation] RadSiteWarhead.Detonate` (default false): makes
    /// `RadSiteWarhead` detonate on affected objects. Retained only.
    #[cfg(feature = "phobos")]
    pub site_warhead_detonate: bool,
    /// Phobos `[Radiation] RadSiteWarhead.Detonate.Full` (default true): full
    /// warhead detonation instead of area damage when detonating. Retained
    /// only.
    #[cfg(feature = "phobos")]
    pub site_warhead_detonate_full: bool,
    /// Phobos `[Radiation] RadHasOwner` (default false): radiation damage
    /// counts as dealt by the house that fired the projectile. Retained only.
    #[cfg(feature = "phobos")]
    pub has_owner: bool,
    /// Phobos `[Radiation] RadHasInvoker` (default false): radiation damage
    /// counts as dealt by the firing TechnoType and grants it experience.
    /// Retained only.
    #[cfg(feature = "phobos")]
    pub has_invoker: bool,
    /// Phobos `[RadiationTypes]` catalog with the default `Radiation` entry,
    /// read per Rules pass by the ordered Process owner. Empty in builds
    /// without the capability. Reached through
    /// [`crate::rules::phobos_radiation::effective_radiation`], never directly.
    #[cfg(feature = "phobos")]
    pub(crate) radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog,
}

impl Default for RadiationRules {
    fn default() -> Self {
        Self {
            duration_multiple: 1,
            application_delay: 16,
            level_max: 500,
            level_delay: 90,
            light_delay: 90,
            level_factor: 0.2,
            light_factor: SimFixed::from_num(0.1_f32),
            tint_factor: SimFixed::from_num(1.0_f32),
            // Rules665663 zeroes EBX;6677EF/F5/FB seed RGB from BL.
            //66CF70 retains these bytes when RadColor is absent.
            color: (0, 0, 0),
            site_warhead: String::new(),
            // Phobos [Radiation] globals (RulesExt::Global() defaults).
            #[cfg(feature = "phobos")]
            use_global_rad_application_delay: true,
            #[cfg(feature = "phobos")]
            application_delay_building: 0,
            #[cfg(feature = "phobos")]
            building_damage_max_count: -1,
            #[cfg(feature = "phobos")]
            site_warhead_detonate: false,
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: true,
            #[cfg(feature = "phobos")]
            has_owner: false,
            #[cfg(feature = "phobos")]
            has_invoker: false,
            #[cfg(feature = "phobos")]
            radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog::default(),
        }
    }
}

impl RadiationRules {
    pub(super) fn from_ini(ini: &IniFile) -> Self {
        let d = Self::default();
        let Some(section) = ini.section("Radiation") else {
            return d;
        };
        let [red, green, blue] =
            section.read_color_rgb("RadColor", [d.color.0, d.color.1, d.color.2], None);
        Self {
            duration_multiple: section.read_int("RadDurationMultiple", d.duration_multiple),
            // Delays are used as divisors/modulo periods — clamp to >= 1 so a
            // degenerate INI value cannot divide by zero.
            application_delay: section
                .read_int("RadApplicationDelay", d.application_delay)
                .max(1),
            level_max: section.read_int("RadLevelMax", d.level_max),
            level_delay: section.read_int("RadLevelDelay", d.level_delay).max(1),
            light_delay: section.read_int("RadLightDelay", d.light_delay).max(1),
            // Double fields (`0x0066D057..0x0066D0A5`).
            level_factor: section.read_double("RadLevelFactor", d.level_factor),
            // Saturating: an out-of-range literal scans as infinity.
            light_factor: SimFixed::saturating_from_num(
                section.read_double("RadLightFactor", d.light_factor.to_num()) as f32,
            ),
            tint_factor: SimFixed::saturating_from_num(
                section.read_double("RadTintFactor", d.tint_factor.to_num()) as f32,
            ),
            color: (red, green, blue),
            // Only the ordered Process owner reads it (`RulesTypeReferences`).
            site_warhead: d.site_warhead,
            #[cfg(feature = "phobos")]
            use_global_rad_application_delay: section
                .read_bool("UseGlobalRadApplicationDelay", true),
            #[cfg(feature = "phobos")]
            application_delay_building: section.read_int("RadApplicationDelay.Building", 0),
            #[cfg(feature = "phobos")]
            building_damage_max_count: section.read_int("RadBuildingDamageMaxCount", -1),
            #[cfg(feature = "phobos")]
            site_warhead_detonate: section.read_bool("RadSiteWarhead.Detonate", false),
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: section.read_bool("RadSiteWarhead.Detonate.Full", true),
            #[cfg(feature = "phobos")]
            has_owner: section.read_bool("RadHasOwner", false),
            #[cfg(feature = "phobos")]
            has_invoker: section.read_bool("RadHasInvoker", false),
            #[cfg(feature = "phobos")]
            radiation_types: crate::rules::phobos_radiation::RadiationTypeCatalog::default(),
        }
    }
}
