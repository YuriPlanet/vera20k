//! Target-preflight port attributed by contributor baixiong443 to their
//! earlier ra2-mono work, submitted under VERA's GPLv3. The donor source was
//! unavailable for independent review; see the documented provenance below.
//!
//! The decision is pure: commands, AI and cursor queries pass observations
//! from their existing world owners. It never spends charge or changes fog/RNG.
//! Source mapping and intentional adaptations: docs/ares-generic-warhead.md.

#[cfg(feature = "ares")]
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TargetMask(u8);

impl TargetMask {
    pub const NONE: Self = Self(0);
    pub const LAND: Self = Self(1);
    pub const WATER: Self = Self(2);
    pub const EMPTY: Self = Self(4);
    pub const INFANTRY: Self = Self(8);
    pub const UNIT: Self = Self(16);
    pub const BUILDING: Self = Self(32);
    pub const ALL_CELLS: Self = Self(3);
    pub const ALL_TECHNOS: Self = Self(56);
    pub const ALL_CONTENTS: Self = Self(60);
    pub const ALL: Self = Self(255);

    #[cfg(feature = "ares")]
    pub(crate) fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[cfg(feature = "ares")]
    pub fn parse(raw: &str) -> Result<Self, String> {
        parse_mask(raw, |token| {
            Some(match token {
                "land" => Self::LAND.0,
                "water" => Self::WATER.0,
                "empty" | "nocontent" | "no-content" | "no_content" => Self::EMPTY.0,
                "infantry" => Self::INFANTRY.0,
                "unit" | "units" | "aircraft" => Self::UNIT.0,
                "building" | "buildings" => Self::BUILDING.0,
                "allcells" | "all-cells" | "all_cells" => Self::ALL_CELLS.0,
                "alltechnos" | "all-technos" | "all_technos" => Self::ALL_TECHNOS.0,
                "allcontents" | "all-contents" | "all_contents" => Self::ALL_CONTENTS.0,
                "all" => Self::ALL.0,
                _ => return None,
            })
        })
        .map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HouseMask(u8);

impl HouseMask {
    pub const NONE: Self = Self(0);
    pub const OWNER: Self = Self(1);
    pub const ALLIES: Self = Self(2);
    pub const ENEMIES: Self = Self(4);
    pub const ALL: Self = Self(7);

    #[cfg(feature = "ares")]
    pub(crate) fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Ares household-mask reader100341D0, tokens1009BCE0: successful
    /// native comma-only lists replace; absent or unknown keeps the prior.
    #[cfg(feature = "ares")]
    pub(crate) fn read_ares(section: &IniSection, key: &str) -> Option<Self> {
        read_native_mask(section, key, house_token).map(|bits| Self(bits as u8))
    }

    #[cfg(feature = "ares")]
    pub fn parse(raw: &str) -> Result<Self, String> {
        parse_mask(raw, |token| {
            Some(match token {
                "owner" | "self" => 1,
                "ally" | "allies" => 2,
                "enemy" | "enemies" => 4,
                "team" => 3,
                "other" | "others" => 6,
                "all" => 7,
                _ => return None,
            })
        })
        .map(Self)
    }
}

#[cfg(feature = "ares")]
fn read_native_mask(
    section: &IniSection,
    key: &str,
    token: fn(&str) -> Option<u32>,
) -> Option<u32> {
    section
        .read_list(key, 2048)?
        .into_iter()
        .try_fold(0, |bits, name| Some(bits | token(name)?))
}

#[cfg(feature = "ares")]
fn house_token(name: &str) -> Option<u32> {
    [
        ("none", 0),
        ("owner", 1),
        ("allies", 2),
        ("enemies", 4),
        ("team", 3),
        ("trump", 5),
        ("others", 6),
        ("all", 7),
    ]
    .iter()
    .find_map(|(key, bits)| key.eq_ignore_ascii_case(name).then_some(*bits))
}

// ra2-mono accepts composable aliases as well as the canonical Ares tokens.
// Preserve those aliases, but never turn an unknown token into no restriction.
#[cfg(feature = "ares")]
fn parse_mask(raw: &str, bit: impl Fn(&str) -> Option<u8>) -> Result<u8, String> {
    let mut mask = 0;
    for token in raw
        .trim()
        .trim_matches(['<', '>'])
        .split(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | ';' | '|' | '+'))
        .filter(|s| !s.is_empty())
    {
        let token_lower = token.to_ascii_lowercase();
        if matches!(token_lower.as_str(), "none" | "no" | "false") {
            continue;
        }
        mask |= bit(&token_lower).ok_or_else(|| token.to_owned())?;
    }
    Ok(mask)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetRequirements {
    pub target: Result<TargetMask, String>,
    pub house: Result<HouseMask, String>,
}

#[cfg(feature = "ares")]
impl TargetRequirements {
    pub fn parse(target: &str, house: &str) -> Self {
        Self {
            target: TargetMask::parse(target),
            house: HouseMask::parse(house),
        }
    }
}

/// Immutable configuration. Only explicitly opted-in extension types read it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SuperWeaponTargeting {
    pub manual: TargetRequirements,
    pub automatic: TargetRequirements,
    pub fire_into_shroud: bool,
    pub range_minimum: NativeF64Bits,
    pub range_maximum: NativeF64Bits,
}

#[cfg(feature = "ares")]
impl SuperWeaponTargeting {
    pub fn range(&self) -> (f64, f64) {
        (
            f64::from_bits(self.range_minimum.bits()),
            f64::from_bits(self.range_maximum.bits()),
        )
    }
}

/// Ares3.0 mode reader10034350, exact table1009BAF8 (21 entries).
/// The table includes modes whose selectors are separate pending mechanisms;
/// recognizing a mode must not substitute a different selector for it.
#[cfg(feature = "ares")]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum AiTargetingMode {
    #[default]
    None,
    Nuke,
    LightningStorm,
    PsychicDominator,
    ParaDrop,
    GeneticMutator,
    ForceShield,
    NoTarget,
    Offensive,
    Stealth,
    SelfTarget,
    Base,
    MultiMissile,
    HunterSeeker,
    EnemyBase,
    IronCurtain,
    Attack,
    LowPower,
    LowPowerAttack,
    DropPod,
    LightningRandom,
}

#[cfg(feature = "ares")]
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum AiTargetPreference {
    #[default]
    None,
    Offensive,
    Defensive,
}

#[cfg(feature = "ares")]
impl AiTargetingMode {
    fn parse(raw: &str) -> Option<Self> {
        const MODES: [(&str, AiTargetingMode); 21] = [
            ("None", AiTargetingMode::None),
            ("Nuke", AiTargetingMode::Nuke),
            ("LightningStorm", AiTargetingMode::LightningStorm),
            ("PsychicDominator", AiTargetingMode::PsychicDominator),
            ("ParaDrop", AiTargetingMode::ParaDrop),
            ("GeneticMutator", AiTargetingMode::GeneticMutator),
            ("ForceShield", AiTargetingMode::ForceShield),
            ("NoTarget", AiTargetingMode::NoTarget),
            ("Offensive", AiTargetingMode::Offensive),
            ("Stealth", AiTargetingMode::Stealth),
            ("Self", AiTargetingMode::SelfTarget),
            ("Base", AiTargetingMode::Base),
            ("MultiMissile", AiTargetingMode::MultiMissile),
            ("HunterSeeker", AiTargetingMode::HunterSeeker),
            ("EnemyBase", AiTargetingMode::EnemyBase),
            ("IronCurtain", AiTargetingMode::IronCurtain),
            ("Attack", AiTargetingMode::Attack),
            ("LowPower", AiTargetingMode::LowPower),
            ("LowPowerAttack", AiTargetingMode::LowPowerAttack),
            ("DropPod", AiTargetingMode::DropPod),
            ("LightningRandom", AiTargetingMode::LightningRandom),
        ];
        MODES
            .iter()
            .find_map(|(name, mode)| name.eq_ignore_ascii_case(raw).then_some(*mode))
    }

    /// Native defaults1009B8A0, stride12: constraints+4, preference+8,
    /// automatic target+9 and house+10. Nullable overrides remain separate.
    fn defaults(self) -> (u32, AiTargetPreference, TargetMask, HouseMask) {
        use AiTargetPreference as P;
        match self {
            Self::Nuke => (4, P::Offensive, TargetMask::ALL_TECHNOS, HouseMask::ENEMIES),
            Self::LightningStorm => (
                12,
                P::Offensive,
                TargetMask::ALL_TECHNOS,
                HouseMask::ENEMIES,
            ),
            Self::PsychicDominator => (21, P::None, TargetMask(24), HouseMask(7)),
            Self::ParaDrop => (0, P::Offensive, TargetMask::NONE, HouseMask::NONE),
            Self::GeneticMutator => (1, P::None, TargetMask::INFANTRY, HouseMask(7)),
            Self::ForceShield => (0, P::Defensive, TargetMask::NONE, HouseMask::NONE),
            Self::Offensive => (4, P::None, TargetMask::ALL_TECHNOS, HouseMask::ENEMIES),
            Self::Stealth => (0, P::None, TargetMask::ALL_TECHNOS, HouseMask::ENEMIES),
            Self::MultiMissile => (4, P::Offensive, TargetMask::BUILDING, HouseMask::ENEMIES),
            Self::HunterSeeker | Self::EnemyBase | Self::DropPod => {
                (4, P::None, TargetMask::NONE, HouseMask::NONE)
            }
            Self::Attack => (32, P::None, TargetMask::NONE, HouseMask::NONE),
            Self::LowPower => (64, P::None, TargetMask::NONE, HouseMask::NONE),
            Self::LowPowerAttack => (96, P::None, TargetMask::NONE, HouseMask::NONE),
            Self::None
            | Self::NoTarget
            | Self::SelfTarget
            | Self::Base
            | Self::IronCurtain
            | Self::LightningRandom => (0, P::None, TargetMask::NONE, HouseMask::NONE),
        }
    }
}

/// The retained common SWTypeExt policy, read by ProcessedType in pass order.
/// Other definitions project it; no payload keeps its own competing copy.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresTargetPolicy {
    initialized: bool,
    pub(crate) use_ai_targeting: bool,
    pub(crate) auto_fire: bool,
    pub(crate) manual_fire: bool,
    pub(crate) ai_mode: AiTargetingMode,
    constraints: Option<u32>,
    preference: Option<AiTargetPreference>,
    pub(crate) affects_target: TargetMask,
    pub(crate) affects_house: HouseMask,
    manual_target: TargetMask,
    manual_house: HouseMask,
    ai_target: Option<TargetMask>,
    ai_house: Option<HouseMask>,
    fire_into_shroud: bool,
    range_minimum: NativeF64Bits,
    range_maximum: NativeF64Bits,
}

#[cfg(feature = "ares")]
impl Default for AresTargetPolicy {
    fn default() -> Self {
        // SWTypeExt ctor10031460: packed01010007 at10031733;
        // mode/nullable flags1003176A..10031783; masks100317AE/B8.
        // Both ranges take binary64 -1 at1003180E/4E/59 (1009C338).
        Self {
            initialized: false,
            use_ai_targeting: false,
            auto_fire: false,
            manual_fire: true,
            ai_mode: AiTargetingMode::None,
            constraints: None,
            preference: None,
            affects_target: TargetMask::ALL,
            affects_house: HouseMask(7),
            manual_target: TargetMask::NONE,
            manual_house: HouseMask::NONE,
            ai_target: None,
            ai_house: None,
            fire_into_shroud: true,
            range_minimum: NativeF64Bits::from_bits((-1.0_f64).to_bits()),
            range_maximum: NativeF64Bits::from_bits((-1.0_f64).to_bits()),
        }
    }
}

#[cfg(feature = "ares")]
impl AresTargetPolicy {
    pub(crate) fn read_pass(&mut self, section: &IniSection, kind: Option<&str>) {
        if !self.initialized {
            // NewSWType Initialize is called once by10033250, before the
            // common reads. These are the two extension types in this public chain.
            self.ai_mode = match kind {
                Some(kind) if kind.eq_ignore_ascii_case("GenericWarhead") => {
                    AiTargetingMode::Offensive
                }
                Some(kind) if kind.eq_ignore_ascii_case("UnitDelivery") => {
                    AiTargetingMode::ParaDrop
                }
                _ => AiTargetingMode::None,
            };
            self.initialized = true;
        }
        // SWTypeExt ReadINI10033370. Unimplemented intervening keys do not
        // alias these scalar fields. Native readers retain on unsuccessful input.
        self.fire_into_shroud = section.read_ares_bool("SW.FireIntoShroud", self.fire_into_shroud);
        self.auto_fire = section.read_ares_bool("SW.AutoFire", self.auto_fire);
        self.manual_fire = section.read_ares_bool("SW.ManualFire", self.manual_fire);
        if let Some(bits) = read_native_mask(section, "SW.AffectsHouse", house_token) {
            self.affects_house = HouseMask(bits as u8);
        }
        self.use_ai_targeting = section.read_ares_bool("SW.UseAITargeting", self.use_ai_targeting);
        self.ai_mode =
            section.read_string_with("SW.AITargeting", 2048, self.ai_mode, |previous, raw| {
                AiTargetingMode::parse(raw).unwrap_or(previous)
            });
        if let Some(bits) =
            read_native_mask(section, "SW.AITargeting.Constraints", constraint_token)
        {
            self.constraints = Some(bits);
        }
        self.preference = section.read_string_with(
            "SW.AITargeting.Preference",
            2048,
            self.preference,
            |previous, raw| {
                [
                    ("none", AiTargetPreference::None),
                    ("offensive", AiTargetPreference::Offensive),
                    ("defensive", AiTargetPreference::Defensive),
                ]
                .iter()
                .find_map(|(name, preference)| {
                    name.eq_ignore_ascii_case(raw).then_some(Some(*preference))
                })
                .unwrap_or(previous)
            },
        );
        if let Some(bits) = read_native_mask(section, "SW.AffectsTarget", target_token) {
            self.affects_target = TargetMask(bits as u8);
        }
        if let Some(bits) = read_native_mask(section, "SW.RequiresTarget", target_token) {
            self.manual_target = TargetMask(bits as u8);
        }
        if let Some(bits) = read_native_mask(section, "SW.RequiresHouse", house_token) {
            self.manual_house = HouseMask(bits as u8);
        }
        if let Some(bits) = read_native_mask(section, "SW.AIRequiresTarget", target_token) {
            self.ai_target = Some(TargetMask(bits as u8));
        }
        if let Some(bits) = read_native_mask(section, "SW.AIRequiresHouse", house_token) {
            self.ai_house = Some(HouseMask(bits as u8));
        }
        self.range_minimum = section.read_ares_double_bits("SW.RangeMinimum", self.range_minimum);
        self.range_maximum = section.read_ares_double_bits("SW.RangeMaximum", self.range_maximum);
    }

    pub(crate) fn constraints(&self) -> u32 {
        self.constraints.unwrap_or(self.ai_mode.defaults().0)
    }
    pub(crate) fn preference(&self) -> AiTargetPreference {
        self.preference.unwrap_or(self.ai_mode.defaults().1)
    }
    pub(crate) fn targeting(&self) -> SuperWeaponTargeting {
        let defaults = self.ai_mode.defaults();
        SuperWeaponTargeting {
            manual: TargetRequirements {
                target: Ok(self.manual_target),
                house: Ok(self.manual_house),
            },
            automatic: TargetRequirements {
                target: Ok(self.ai_target.unwrap_or(defaults.2)),
                house: Ok(self.ai_house.unwrap_or(defaults.3)),
            },
            fire_into_shroud: self.fire_into_shroud,
            range_minimum: self.range_minimum,
            range_maximum: self.range_maximum,
        }
    }
}

// Ares mask readers100341D0/10034290/10034430: comma-only strtok10007920,
// then _strcmpi7C8D20. Do not trim inner tokens or accept donor-only aliases.
// No field is written until every token matched; invalid later passes retain.

#[cfg(feature = "ares")]
fn target_token(name: &str) -> Option<u32> {
    [
        ("none", 0),
        ("land", 1),
        ("water", 2),
        ("empty", 4),
        ("infantry", 8),
        ("units", 16),
        ("buildings", 32),
        ("all", 255),
    ]
    .iter()
    .find_map(|(key, bits)| key.eq_ignore_ascii_case(name).then_some(*bits))
}

#[cfg(feature = "ares")]
fn constraint_token(name: &str) -> Option<u32> {
    [
        ("none", 0),
        ("offensive_cell_clear", 1),
        ("defensive_cell_clear", 2),
        ("enemy", 4),
        ("lightningstorm_inactive", 8),
        ("dominator_inactive", 16),
        ("attacked", 32),
        ("lowpower", 64),
        ("offensive_cell_set", 128),
        ("defensive_cell_set", 256),
    ]
    .iter()
    .find_map(|(key, bits)| key.eq_ignore_ascii_case(name).then_some(*bits))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetReject {
    InstanceNotFound,
    InvalidPayload,
    TargetRequired,
    TargetOutOfBounds,
    UnsupportedTargetRequirement(String),
    TargetRequiresLand,
    TargetRequiresWater,
    TargetContentMismatch,
    TargetHouseMismatch,
    TargetShrouded,
    InvalidRangeConfiguration,
    NoEligibleLaunchSite,
    NoEligibleDesignator,
    InhibitorInRange,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetContent {
    pub kind: TargetMask,
    pub relation: HouseMask,
}

#[derive(Debug, Clone, Copy)]
pub struct LaunchSite {
    pub center: (i32, i32),
    pub eligible: bool,
    pub range_minimum_override: Option<f64>,
    pub range_maximum_override: Option<f64>,
}

#[cfg(feature = "ares")]
impl LaunchSite {
    pub fn admits(self, target: (i32, i32), range: (f64, f64)) -> bool {
        let minimum = self.range_minimum_override.unwrap_or(range.0);
        let maximum = self.range_maximum_override.unwrap_or(range.1);
        if !self.eligible || !valid_range((minimum, maximum)) {
            return false;
        }
        let dx = i64::from(target.0) - i64::from(self.center.0);
        let dy = i64::from(target.1) - i64::from(self.center.1);
        // Coordinates supplied by the world are 16-bit cells. f64 exactly
        // represents their squared distance; no sqrt, FMA or RNG is involved.
        let distance_squared = (dx as f64) * (dx as f64) + (dy as f64) * (dy as f64);
        (minimum < 0.0 || distance_squared >= minimum * minimum)
            && (maximum < 0.0 || distance_squared <= maximum * maximum)
    }
}

#[cfg(feature = "ares")]
fn valid_range((minimum, maximum): (f64, f64)) -> bool {
    minimum.is_finite()
        && maximum.is_finite()
        && (minimum < 0.0 || maximum < 0.0 || minimum <= maximum)
}

pub struct TargetPreflightInput<'a> {
    pub target_cell: Option<(i32, i32)>,
    pub requires_target_cell: bool,
    pub requirements: &'a TargetRequirements,
    pub contents: &'a [TargetContent],
    pub target_in_bounds: bool,
    pub terrain_is_water: Option<bool>,
    pub fire_into_shroud: bool,
    pub shrouded: Option<bool>,
    pub range: (f64, f64),
    pub requires_launch_site: bool,
    pub launch_sites: &'a [LaunchSite],
}

/// Port of Evaluate: same ordered gates and rejection categories. A missing
/// terrain/visibility observation is not invented as water or shroud.
#[cfg(feature = "ares")]
pub fn evaluate_target(input: &TargetPreflightInput<'_>) -> Result<(), TargetReject> {
    let target_mask = input
        .requirements
        .target
        .as_ref()
        .map_err(|token| TargetReject::UnsupportedTargetRequirement(token.clone()))?;
    let house_mask = input
        .requirements
        .house
        .as_ref()
        .map_err(|token| TargetReject::UnsupportedTargetRequirement(token.clone()))?;
    let constrained_range = input.range.0 >= 0.0
        || input.range.1 >= 0.0
        || input.launch_sites.iter().any(|site| {
            site.range_minimum_override.is_some_and(|v| v >= 0.0)
                || site.range_maximum_override.is_some_and(|v| v >= 0.0)
        });
    let Some(target) = input.target_cell else {
        return if input.requires_target_cell
            || *target_mask != TargetMask::NONE
            || !input.fire_into_shroud
            || constrained_range
        {
            Err(TargetReject::TargetRequired)
        } else {
            Ok(())
        };
    };
    if !input.target_in_bounds {
        return Err(TargetReject::TargetOutOfBounds);
    }
    if target_mask.intersects(TargetMask::ALL_CELLS)
        && let Some(water) = input.terrain_is_water
    {
        if water && !target_mask.intersects(TargetMask::WATER) {
            return Err(TargetReject::TargetRequiresLand);
        }
        if !water && !target_mask.intersects(TargetMask::LAND) {
            return Err(TargetReject::TargetRequiresWater);
        }
    }
    let content_mask = TargetMask(target_mask.0 & TargetMask::ALL_CONTENTS.0);
    let constrains_content = content_mask != TargetMask::NONE;
    if constrains_content || *house_mask != HouseMask::NONE {
        let kind_matches =
            |content: &TargetContent| !constrains_content || content_mask.intersects(content.kind);
        let accepted = if input.contents.is_empty() {
            !constrains_content || content_mask.intersects(TargetMask::EMPTY)
        } else {
            input.contents.iter().any(|content| {
                kind_matches(content)
                    && (*house_mask == HouseMask::NONE || house_mask.intersects(content.relation))
            })
        };
        if !accepted {
            return Err(
                if !constrains_content || input.contents.iter().any(kind_matches) {
                    TargetReject::TargetHouseMismatch
                } else {
                    TargetReject::TargetContentMismatch
                },
            );
        }
    }
    if !input.fire_into_shroud && input.shrouded == Some(true) {
        return Err(TargetReject::TargetShrouded);
    }
    if !valid_range(input.range) {
        return Err(TargetReject::InvalidRangeConfiguration);
    }
    if input.requires_launch_site && !input.launch_sites.iter().any(|site| site.eligible) {
        return Err(TargetReject::NoEligibleLaunchSite);
    }
    if constrained_range
        && !input
            .launch_sites
            .iter()
            .any(|site| site.admits(target, input.range))
    {
        return Err(TargetReject::NoEligibleLaunchSite);
    }
    Ok(())
}

#[cfg(all(test, feature = "ares"))]
#[path = "superweapon_targeting_tests.rs"]
mod tests;
