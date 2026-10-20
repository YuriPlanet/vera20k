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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    fn intersects(self, other: Self) -> bool {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HouseMask(u8);

impl HouseMask {
    pub const NONE: Self = Self(0);
    pub const OWNER: Self = Self(1);
    pub const ALLIES: Self = Self(2);
    pub const ENEMIES: Self = Self(4);

    #[cfg(feature = "ares")]
    fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
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
    pub fn from_ini(section: &IniSection) -> Self {
        let range = |key| {
            let value = if section.is_present(key) {
                section.read_double(key, f64::NAN)
            } else {
                -1.0
            };
            NativeF64Bits::from_bits(value.to_bits())
        };
        Self {
            manual: TargetRequirements::parse(
                section.read_name("SW.RequiresTarget", 0x100).unwrap_or(""),
                section.read_name("SW.RequiresHouse", 0x100).unwrap_or(""),
            ),
            // GenericWarhead defaults to the Offensive targeting mode. Its
            // automatic requirements are independent of the manual masks.
            automatic: TargetRequirements::parse(
                section
                    .read_name("SW.AIRequiresTarget", 0x100)
                    .unwrap_or("all-technos"),
                section
                    .read_name("SW.AIRequiresHouse", 0x100)
                    .unwrap_or("enemies"),
            ),
            fire_into_shroud: section.read_bool("SW.FireIntoShroud", true),
            range_minimum: range("SW.RangeMinimum"),
            range_maximum: range("SW.RangeMaximum"),
        }
    }

    pub fn range(&self) -> (f64, f64) {
        (
            f64::from_bits(self.range_minimum.bits()),
            f64::from_bits(self.range_maximum.bits()),
        )
    }
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
