//! Map lighting — parses [Lighting] section and computes per-cell RGB tint.
//!
//! RA2 maps define global lighting parameters (Ambient, Red, Green, Blue, Ground,
//! Level) in their INI [Lighting] section. These determine a per-cell color
//! multiplier that tints terrain tiles and entity sprites. Ordinary map
//! lighting is app/render-derived state; simulation does not own this data.
//!
//! Point light sources (lamp posts, buildings with LightVisibility) add localized
//! brightness using linear falloff: `contribution = ((range - distance) / range) * intensity`.
//! This matches the original engine's point light calculation.
//!
//! ## Dependency rules
//! - Part of map/ — depends on rules/ini_parser for IniFile, map/entities for MapEntity.

use std::collections::HashMap;

#[cfg(test)]
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::ini_parser::IniFile;
#[cfg(test)]
use crate::rules::ruleset::RuleSet;

/// Maximum combined lighting value per channel in current compatibility tint output.
pub const TOTAL_AMBIENT_CAP: f32 = 2.0;

/// Internal light unit scale. `1000 == 1.0`.
pub const LIGHT_UNIT: i32 = 1000;

/// Minimum stored scalar light value.
pub const LIGHT_CLAMP_MIN: i32 = 0;

/// Maximum stored scalar/RGB light value.
pub const LIGHT_CLAMP_MAX: i32 = 2000;

/// Identity 16.16 scale used by the LightConvert normalization path.
pub const LIGHT_SCALE16_IDENTITY: i32 = 0x10000;

/// Scenario Ground/Level: original 0068A92E/0068A968 multiply by double
/// [007E4658] = 1000.0, then add 0.01 and truncate. See the Ground/Level
/// correction in LIGHTCONVERT_ROW_RGB565_ORACLE_2026_09_09.md.
pub const GROUND_LEVEL_UNIT_SCALE: i32 = LIGHT_UNIT;

/// Binary light unit scale for point-light intensity/tint values.
pub const POINT_LIGHT_UNIT_SCALE: i32 = LIGHT_UNIT;

/// Leptons per cell in RA2's coordinate system.
pub const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;

const HALF_CELL_LEPTONS: i32 = LEPTONS_PER_CELL / 2;
const BOTTOM_LEVEL_OFFSET: i32 = 4;
const DETAIL_LOW_RGB_MASK: i32 = !127;
const DETAIL_MEDIUM_RGB_MASK: i32 = !63;
const DETAIL_HIGH_RGB_MASK: i32 = !31;

/// Default white tint (no lighting effect).
pub const DEFAULT_TINT: [f32; 3] = [1.0, 1.0, 1.0];

/// Global lighting parameters from the map's [Lighting] INI section.
#[derive(Debug, Clone)]
pub struct LightingConfig {
    /// Base brightness level (default 1.0).
    pub ambient: f32,
    /// Red channel multiplier (default 1.0).
    pub red: f32,
    /// Green channel multiplier (default 1.0).
    pub green: f32,
    /// Blue channel multiplier (default 1.0).
    pub blue: f32,
    /// Ground-level darkening subtracted from ambient. Default 0.05 (native reset 50).
    pub ground: f32,
    /// Height-based ambient boost per elevation level. Default 0.008 (native reset 8).
    pub level: f32,
    /// Lightning Storm ambient target (default 0.87).
    pub ion_ambient: f32,
    /// Lightning Storm global red profile (default 0.30).
    pub ion_red: f32,
    /// Lightning Storm global green profile (default 0.40).
    pub ion_green: f32,
    /// Lightning Storm global blue profile (default 0.75).
    pub ion_blue: f32,
    /// Lightning Storm ground subtraction (default 0.0).
    pub ion_ground: f32,
    /// Lightning Storm height contribution (default 0.0).
    pub ion_level: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            ambient: 1.0,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            ground: 0.05,
            level: 0.008,
            ion_ambient: 0.87,
            ion_red: 0.30,
            ion_green: 0.40,
            ion_blue: 0.75,
            ion_ground: 0.0,
            ion_level: 0.0,
        }
    }
}

/// Scenario-owned integer lighting profile as stored after map INI parsing.
/// Ambient/RGB use the native 100 scale; Ground/Level use the native 1000 scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightingProfileUnits {
    pub ambient_percent: i32,
    pub red_percent: i32,
    pub green_percent: i32,
    pub blue_percent: i32,
    pub ground_units: i32,
    pub level_units: i32,
}

/// Ordinary and Lightning/Ion profiles parsed from one map `[Lighting]` section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedLightingProfiles {
    pub normal: LightingProfileUnits,
    pub ion: LightingProfileUnits,
}

/// Integer RGB identity used by the light profile cache.
pub type LightRgbKey = [i32; 3];

/// Stable id for a cached light profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightProfileId(pub usize);

/// Cached RGB profile shared by cells with the same channel multipliers.
#[derive(Debug, Clone, PartialEq)]
pub struct LightProfile {
    pub id: LightProfileId,
    pub rgb_key: LightRgbKey,
    pub rgb: [f32; 3],
}

/// LightConvert-style profile identity cache for render-facing cell light.
#[derive(Debug, Clone)]
pub struct LightProfileCache {
    profiles: Vec<LightProfile>,
    by_key: HashMap<LightRgbKey, LightProfileId>,
}

impl LightProfileCache {
    pub fn new() -> Self {
        let mut cache = Self {
            profiles: Vec::new(),
            by_key: HashMap::new(),
        };
        cache.profile_id_for_rgb(DEFAULT_TINT);
        cache
    }

    pub fn default_profile_id(&self) -> LightProfileId {
        LightProfileId(0)
    }

    pub fn profile_id_for_rgb(&mut self, rgb: [f32; 3]) -> LightProfileId {
        let key = rgb_to_key(rgb);
        self.profile_id_for_key(key)
    }

    pub fn profile_id_for_key(&mut self, key: LightRgbKey) -> LightProfileId {
        if let Some(id) = self.by_key.get(&key).copied() {
            return id;
        }
        let id = LightProfileId(self.profiles.len());
        self.profiles.push(LightProfile {
            id,
            rgb_key: key,
            rgb: key_to_rgb(key),
        });
        self.by_key.insert(key, id);
        id
    }

    pub fn get(&self, id: LightProfileId) -> Option<&LightProfile> {
        self.profiles.get(id.0)
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

impl Default for LightProfileCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-cell light state backed by a cached RGB profile.
#[derive(Debug, Clone, PartialEq)]
pub struct CellLight {
    pub profile_id: LightProfileId,
    /// Quantized normalized RGB key used for the profile/cache bridge.
    pub rgb_key: LightRgbKey,
    /// Raw RGB accumulators before normalization.
    pub raw_rgb: LightRgbKey,
    /// 16.16 scale computed by the verified RGB normalization helper.
    pub scale16: i32,
    /// Raw additive source intensity before RGB normalization.
    pub raw_additive_intensity: i32,
    /// Source intensity published to Cell+0x108, independent of the scalar
    /// normalized into Cell+0x10C (004845CB -> 005558E0).
    pub additive_intensity: i32,
    /// Raw top/common scalar before final clamp.
    pub raw_top_scalar: i32,
    /// Raw bottom scalar before `scale16` multiplication and final clamp.
    pub raw_bottom_scalar: i32,
    pub top_scalar: i32,
    pub common_scalar: i32,
    pub bottom_scalar: i32,
}

impl CellLight {
    /// Cell484680 (active-retail gamemd): ambient/profile refresh retains
    /// Cell104 normalization and Cell108 source intensity. Unlike full483E30
    /// sampling, it scales the signed-word top BEFORE the final 0..2000 clamp.
    /// See tools/spatial_oracle/light_retained_scalar.py and its native records.
    pub(crate) fn refresh_retained_scalars(&mut self, profile: LightingProfileUnits, level: u8) {
        let ambient = (profile.ambient_percent.wrapping_mul(1000) / 100) as i16;
        let base = ambient.wrapping_add(self.additive_intensity as i16);
        let level_units = profile.level_units as i16;
        let ground = profile.ground_units as i16;
        let height = i16::from(level as i8);
        let top = base.wrapping_add(level_units.wrapping_mul(height).wrapping_sub(ground));
        let bottom = base.wrapping_add(
            level_units
                .wrapping_mul(height.wrapping_add(4))
                .wrapping_sub(ground),
        );
        let scaled = |value: i16| {
            // Native IMUL retains low32, then takes the signed high word.
            (i32::from(value).wrapping_mul(self.scale16) >> 16) as i16
        };
        self.raw_top_scalar = i32::from(top);
        self.raw_bottom_scalar = i32::from(bottom);
        self.top_scalar = i32::from(top).clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX);
        self.common_scalar = i32::from(scaled(top)).clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX);
        self.bottom_scalar = i32::from(scaled(bottom)).clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX);
    }

    pub fn new(
        profile_id: LightProfileId,
        rgb_key: LightRgbKey,
        raw_rgb: LightRgbKey,
        scale16: i32,
        raw_additive_intensity: i32,
        additive_intensity: i32,
        raw_top_scalar: i32,
        raw_bottom_scalar: i32,
        top_scalar: i32,
        common_scalar: i32,
        bottom_scalar: i32,
    ) -> Self {
        Self {
            profile_id,
            rgb_key,
            raw_rgb,
            scale16,
            raw_additive_intensity,
            additive_intensity,
            raw_top_scalar,
            raw_bottom_scalar,
            top_scalar,
            common_scalar,
            bottom_scalar,
        }
    }

    pub fn compatibility(
        profile_id: LightProfileId,
        rgb_key: LightRgbKey,
        common_scalar: i32,
    ) -> Self {
        let scalar = common_scalar.clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX);
        Self::new(
            profile_id,
            rgb_key,
            rgb_key,
            LIGHT_SCALE16_IDENTITY,
            0,
            0,
            common_scalar,
            common_scalar,
            scalar,
            scalar,
            scalar,
        )
    }
}

/// Map-level cell lighting container with compatibility tint accessors.
#[derive(Debug, Clone)]
pub struct CellLightGrid {
    cells: HashMap<(u16, u16), CellLight>,
    profiles: LightProfileCache,
    detail_level: u32,
    alternate_rgb: Option<LightRgbKey>,
}

impl CellLightGrid {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            profiles: LightProfileCache::new(),
            detail_level: 2,
            alternate_rgb: None,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            cells: HashMap::with_capacity(capacity),
            profiles: LightProfileCache::new(),
            detail_level: 2,
            alternate_rgb: None,
        }
    }

    fn with_detail_level(detail_level: u32) -> Self {
        Self {
            cells: HashMap::new(),
            profiles: LightProfileCache::new(),
            detail_level: detail_level.min(2),
            alternate_rgb: None,
        }
    }

    pub fn profiles(&self) -> &LightProfileCache {
        &self.profiles
    }

    /// LightConvert556090 retains normal RGB/row identity while selecting
    /// Ion alternate table RGB; neither normalization nor Cell104 changes.
    pub(crate) fn set_alternate_rgb(&mut self, rgb: Option<LightRgbKey>) {
        self.alternate_rgb = rgb;
    }

    pub(crate) fn palette_rgb(&self, normal: LightRgbKey) -> LightRgbKey {
        self.alternate_rgb.unwrap_or(normal)
    }

    pub(crate) fn refresh_retained_scalars<I>(&mut self, heights: I, profile: LightingProfileUnits)
    where
        I: IntoIterator<Item = ((u16, u16), u8)>,
    {
        for (cell, height) in heights {
            if let Some(light) = self.cells.get_mut(&cell) {
                light.refresh_retained_scalars(profile, height);
            }
        }
    }

    pub fn insert_light(&mut self, cell: (u16, u16), light: CellLight) {
        self.cells.insert(cell, light);
    }

    pub fn insert_profiled_light(&mut self, cell: (u16, u16), rgb: [f32; 3], common_scalar: f32) {
        let profile_id = self.profiles.profile_id_for_rgb(rgb);
        let rgb_key = rgb_to_key(rgb);
        self.insert_light(
            cell,
            CellLight::compatibility(profile_id, rgb_key, light_float_to_units(common_scalar)),
        );
    }

    #[cfg(test)]
    pub fn set_compat_tint(&mut self, cell: (u16, u16), tint: [f32; 3]) {
        self.insert_profiled_light(cell, tint, 1.0);
    }

    pub fn cells(&self) -> impl Iterator<Item = ((u16, u16), &CellLight)> {
        self.cells.iter().map(|(cell, light)| (*cell, light))
    }

    pub fn cell_light_at(&self, cell: (u16, u16)) -> Option<&CellLight> {
        self.cells.get(&cell)
    }

    pub fn tint_at(&self, cell: (u16, u16)) -> Option<[f32; 3]> {
        let light = self.cells.get(&cell)?;
        Some(self.tint_for_light(light))
    }

    pub fn tint_or_default(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_at(cell).unwrap_or(DEFAULT_TINT)
    }

    pub fn techno_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_or_default(cell)
    }

    /// UnitClass body draw (`0x0073CEC0`) signed-adds ExtraUnitLight to the
    /// cell's top scalar before dispatching either its SHP or VXL body draw.
    pub fn unit_tint_at(&self, cell: (u16, u16), extra_light: i32) -> [f32; 3] {
        self.tint_for_top_scalar_with_extra_or_default(cell, extra_light)
    }

    /// InfantryClass body draw (`0x00518F90`) signed-adds ExtraInfantryLight
    /// to the cell's top scalar before its SHP draw dispatch.
    pub fn infantry_tint_at(&self, cell: (u16, u16), extra_light: i32) -> [f32; 3] {
        self.tint_for_top_scalar_with_extra_or_default(cell, extra_light)
    }

    pub fn aircraft_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.techno_tint_at(cell)
    }

    pub fn building_body_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.techno_tint_at(cell)
    }

    pub fn overlay_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_or_default(cell)
    }

    #[cfg(test)]
    pub fn terrain_object_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_for_common_scalar_or_default(cell)
    }

    pub fn terrain_object_tint_for_type(
        &self,
        cell: (u16, u16),
        spawns_tiberium: bool,
    ) -> [f32; 3] {
        if spawns_tiberium {
            self.tint_for_top_scalar_or_default(cell)
        } else {
            self.tint_for_common_scalar_or_default(cell)
        }
    }

    pub fn terrain_tile_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_for_common_scalar_or_default(cell)
    }

    /// Tint for an animation drawn on `cell`.
    ///
    /// gamemd's animation draw starts the shape's brightness argument at full
    /// (`LIGHT_UNIT`) and, in each of that function's branches, only replaces it
    /// with a scalar read off the animation's cell when the type's
    /// `UseNormalLight=` is clear. A set flag therefore skips the cell entirely,
    /// which is why explosions and fires keep their own brightness on a darkened
    /// or colour-shifted map. `None` means no animation art record was found for
    /// the draw, which leaves the flag at its INI default of false.
    ///
    /// DRIFT, deferred, not a consequence of the flag: gamemd chooses the
    /// palette convert *separately* from the brightness scalar, and an ordinary
    /// animation uses the global animation convert allocated once at init
    /// regardless of which cell it sits on — only the brightness comes from the
    /// cell. This grid folds hue and brightness into one multiplier, so on the
    /// unflagged path it also hue-shifts an animation where gamemd only dims it.
    /// That fires on every muzzle flash, smoke, splash and death animation on
    /// essentially every stock map, since almost all ship a non-unit `[Lighting]`
    /// RGB channel. Deferred rather than fixed because what the native convert
    /// table does internally is UNCHECKED, so the size of the pixel difference is
    /// unknown; and because the correct repair is a per-consumer split of hue
    /// from brightness, not a blanket change to this accessor.
    pub fn anim_tint_at(&self, cell: (u16, u16), art: Option<&AnimTypeRuntimeConfig>) -> [f32; 3] {
        if art.is_some_and(|config| config.use_normal_light) {
            return DEFAULT_TINT;
        }
        self.tint_or_default(cell)
    }

    pub fn bridge_body_tint_at(&self, cell: (u16, u16)) -> [f32; 3] {
        self.tint_or_default(cell)
    }

    fn tint_for_light(&self, light: &CellLight) -> [f32; 3] {
        self.tint_for_light_scalar(light, light.common_scalar)
    }

    fn tint_for_top_scalar_or_default(&self, cell: (u16, u16)) -> [f32; 3] {
        let Some(light) = self.cells.get(&cell) else {
            return DEFAULT_TINT;
        };
        self.tint_for_light_scalar(light, light.top_scalar)
    }

    fn tint_for_top_scalar_with_extra_or_default(
        &self,
        cell: (u16, u16),
        extra_light: i32,
    ) -> [f32; 3] {
        let Some(light) = self.cells.get(&cell) else {
            return self.tint_for_profile_scalar(
                self.profiles.default_profile_id(),
                extra_light_scalar(LIGHT_UNIT, extra_light),
            );
        };
        self.tint_for_light_scalar(light, extra_light_scalar(light.top_scalar, extra_light))
    }

    fn tint_for_common_scalar_or_default(&self, cell: (u16, u16)) -> [f32; 3] {
        let Some(light) = self.cells.get(&cell) else {
            return DEFAULT_TINT;
        };
        self.tint_for_light_scalar(light, light.common_scalar)
    }

    fn tint_for_light_scalar(&self, light: &CellLight, scalar: i32) -> [f32; 3] {
        if let Some(rgb) = self.alternate_rgb {
            return rgb.map(|channel| {
                channel as f32 / LIGHT_UNIT as f32 * scalar as f32 / LIGHT_UNIT as f32
            });
        }
        self.tint_for_profile_scalar(light.profile_id, scalar)
    }

    fn tint_for_profile_scalar(&self, profile_id: LightProfileId, scalar: i32) -> [f32; 3] {
        let profile = self
            .profiles
            .get(profile_id)
            .or_else(|| self.profiles.get(self.profiles.default_profile_id()))
            .expect("default light profile is always present");
        let scalar = scalar as f32 / LIGHT_UNIT as f32;
        [
            profile.rgb[0] * scalar,
            profile.rgb[1] * scalar,
            profile.rgb[2] * scalar,
        ]
    }
}

fn extra_light_scalar(top_scalar: i32, extra_light: i32) -> i32 {
    top_scalar.wrapping_add(extra_light)
}

impl Default for CellLightGrid {
    fn default() -> Self {
        Self::new()
    }
}

fn rgb_to_key(rgb: [f32; 3]) -> LightRgbKey {
    [
        (rgb[0] * LIGHT_UNIT as f32).round() as i32,
        (rgb[1] * LIGHT_UNIT as f32).round() as i32,
        (rgb[2] * LIGHT_UNIT as f32).round() as i32,
    ]
}

fn key_to_rgb(key: LightRgbKey) -> [f32; 3] {
    [
        key[0] as f32 / LIGHT_UNIT as f32,
        key[1] as f32 / LIGHT_UNIT as f32,
        key[2] as f32 / LIGHT_UNIT as f32,
    ]
}

/// Return signed building body draw-depth adjustment from art.ini ExtraLight.
#[cfg(test)]
pub fn building_body_depth_adjustment(extra_light: i32) -> i32 {
    extra_light
}

/// Parse [Lighting] section from a map INI file.
pub fn parse_lighting(ini: &IniFile) -> LightingConfig {
    let section = match ini.section("Lighting") {
        Some(s) => s,
        None => return LightingConfig::default(),
    };
    LightingConfig {
        ambient: section.get_f32("Ambient").unwrap_or(1.0),
        red: section.get_f32("Red").unwrap_or(1.0),
        green: section.get_f32("Green").unwrap_or(1.0),
        blue: section.get_f32("Blue").unwrap_or(1.0),
        ground: section.get_f32("Ground").unwrap_or(0.05),
        level: section.get_f32("Level").unwrap_or(0.008),
        ion_ambient: section.get_f32("IonAmbient").unwrap_or(0.87),
        ion_red: section.get_f32("IonRed").unwrap_or(0.30),
        ion_green: section.get_f32("IonGreen").unwrap_or(0.40),
        ion_blue: section.get_f32("IonBlue").unwrap_or(0.75),
        ion_ground: section.get_f32("IonGround").unwrap_or(0.0),
        ion_level: section.get_f32("IonLevel").unwrap_or(0.0),
    }
}

/// Parse the fixed integer ScenarioClass lighting fields without a second
/// floating-point narrowing step. `ReadDouble` already models the native
/// single-precision scan widened to double; each consumer then applies its
/// own scale, +0.01 bias, and truncate-toward-zero conversion.
pub fn parse_lighting_profiles(ini: &IniFile) -> ParsedLightingProfiles {
    let Some(section) = ini.section("Lighting") else {
        return ParsedLightingProfiles::default();
    };
    let percent =
        |key: &str, default: f64| quantize_scenario_light(section.read_double(key, default), 100);
    // 006838F7/00683901 reset internal Ground/Level to 50/8. Missing keys
    // retain those values; map-authored .20/.032 instead parse to 200/32.
    let ground_level = |key: &str, default: f64| {
        quantize_scenario_light(section.read_double(key, default), GROUND_LEVEL_UNIT_SCALE)
    };
    ParsedLightingProfiles {
        normal: LightingProfileUnits {
            ambient_percent: percent("Ambient", 1.0),
            red_percent: percent("Red", 1.0),
            green_percent: percent("Green", 1.0),
            blue_percent: percent("Blue", 1.0),
            ground_units: ground_level("Ground", 0.05),
            level_units: ground_level("Level", 0.008),
        },
        ion: LightingProfileUnits {
            ambient_percent: percent("IonAmbient", 0.87),
            red_percent: percent("IonRed", 0.30),
            green_percent: percent("IonGreen", 0.40),
            blue_percent: percent("IonBlue", 0.75),
            ground_units: ground_level("IonGround", 0.0),
            level_units: ground_level("IonLevel", 0.0),
        },
    }
}

impl Default for ParsedLightingProfiles {
    fn default() -> Self {
        Self {
            normal: LightingProfileUnits {
                ambient_percent: 100,
                red_percent: 100,
                green_percent: 100,
                blue_percent: 100,
                ground_units: 50,
                level_units: 8,
            },
            ion: LightingProfileUnits {
                ambient_percent: 87,
                red_percent: 30,
                green_percent: 40,
                blue_percent: 75,
                ground_units: 0,
                level_units: 0,
            },
        }
    }
}

fn quantize_scenario_light(value: f64, scale: i32) -> i32 {
    (value * f64::from(scale) + 0.01) as i32
}

/// Compute the RGB tint for a single cell given its elevation.
pub fn cell_tint(config: &LightingConfig, z: u8) -> [f32; 3] {
    let grid = build_cell_light_grid_from_heights([((1, 1), z)], config);
    grid.tint_or_default((1, 1))
}

/// Compute the uniform terrain tint for the map.
///
/// Terrain uses the ground-level lighting value across all cells so repeating
/// tile textures do not expose the map grid through per-cell tint boundaries.
#[cfg(test)]
pub fn terrain_tint(config: &LightingConfig) -> [f32; 3] {
    cell_tint(config, 0)
}

/// Build the profile-backed base cell light grid from known cell heights.
pub fn build_cell_light_grid_from_heights<I>(heights: I, config: &LightingConfig) -> CellLightGrid
where
    I: IntoIterator<Item = ((u16, u16), u8)>,
{
    build_cell_light_grid_from_heights_and_units(heights, scenario_profile_units(config))
}

/// Build a cell-light grid from exact ScenarioClass integer fields.
pub fn build_cell_light_grid_from_heights_and_units<I>(
    heights: I,
    profile: LightingProfileUnits,
) -> CellLightGrid
where
    I: IntoIterator<Item = ((u16, u16), u8)>,
{
    build_cell_light_grid_from_heights_and_units_with_detail(heights, profile, 2)
}

/// Exact initial `CellClass +0x10A` ground Z-adjust for one elevation level.
/// This is the value `OverlayClass::Mark` copies to a newly constructed
/// `CellAnim` after its delay-zero constructor has completed.
pub fn cell_ground_z_adjust(profile: LightingProfileUnits, level: u8) -> i32 {
    let units = scenario_units_from_profile(profile);
    units
        .ambient
        .wrapping_add(units.level.wrapping_mul(i32::from(level)))
        .wrapping_sub(units.ground)
        .clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX)
}

/// Build a cell-light grid using the native Options detail-level RGB cache mask.
pub fn build_cell_light_grid_from_heights_and_units_with_detail<I>(
    heights: I,
    profile: LightingProfileUnits,
    detail_level: u32,
) -> CellLightGrid
where
    I: IntoIterator<Item = ((u16, u16), u8)>,
{
    let mut grid = CellLightGrid::with_detail_level(detail_level);
    let units = scenario_units_from_profile(profile);
    for (cell, z) in heights {
        if cell == (0, 0) {
            let light = neutral_cell_light(&mut grid.profiles);
            grid.insert_light(cell, light);
            continue;
        }
        let raw_top = units.ambient + units.level * i32::from(z) - units.ground;
        let raw_bottom =
            units.ambient + units.level * (i32::from(z) + BOTTOM_LEVEL_OFFSET) - units.ground;
        let light = build_cell_light_from_raw(
            &mut grid.profiles,
            [units.red, units.green, units.blue],
            0,
            raw_top,
            raw_bottom,
            grid.detail_level,
        );
        grid.insert_light(cell, light);
    }
    grid
}

/// A point light source placed on the map (lamp post, lit building, etc.).
///
/// Created from map entity data during map load. Each light contributes
/// localized brightness to nearby cells using linear distance falloff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointLight {
    /// Cell position of the light source.
    pub rx: u16,
    pub ry: u16,
    /// Light center X in leptons.
    pub center_x: i32,
    /// Light center Y in leptons.
    pub center_y: i32,
    /// Inclusive visibility radius in leptons.
    pub radius_leptons: i32,
    /// Brightness intensity in `1000 == 1.0` units. Can be negative.
    pub intensity: i32,
    /// RGB tint components in `1000 == 1.0` units.
    pub tint: [i32; 3],
    /// Whether the light source currently contributes.
    pub active: bool,
    /// Detail-level gate placeholder for ordinary light sources.
    pub detail: bool,
}

/// Renderer-local counterpart of the pending records consumed by YR
/// `LightSourceClass::UpdateLightConverts`. Sampling is deliberately separate
/// from replacing the visible grid: no partially gathered light area is shown.
#[derive(Debug, Clone)]
pub struct DeferredCellLightRefresh {
    cells: Vec<((u16, u16), u8)>,
    profile: LightingProfileUnits,
    detail_level: u32,
    lights: Vec<PointLight>,
    samples: Vec<Option<CellLightSample>>,
    gathered: usize,
}

#[derive(Debug, Clone)]
struct CellLightSample {
    cell: (u16, u16),
    neutral: bool,
    raw_rgb: LightRgbKey,
    raw_additive_intensity: i32,
    raw_top_scalar: i32,
    raw_bottom_scalar: i32,
}

impl DeferredCellLightRefresh {
    pub fn new<I>(heights: I, config: &LightingConfig, lights: Vec<PointLight>) -> Self
    where
        I: IntoIterator<Item = ((u16, u16), u8)>,
    {
        Self::new_with_profile(heights, normal_profile_units(config), 2, lights)
    }

    pub fn new_with_profile<I>(
        heights: I,
        profile: LightingProfileUnits,
        detail_level: u32,
        lights: Vec<PointLight>,
    ) -> Self
    where
        I: IntoIterator<Item = ((u16, u16), u8)>,
    {
        let cells: Vec<((u16, u16), u8)> = heights.into_iter().collect();
        Self {
            samples: vec![None; cells.len()],
            cells,
            profile,
            detail_level: detail_level.min(2),
            lights,
            gathered: 0,
        }
    }

    /// Gather up to `budget` records in reverse order. The currently displayed
    /// grid remains untouched until every record has a future sample.
    pub fn gather(&mut self, budget: usize) -> bool {
        let remaining = self.cells.len().saturating_sub(self.gathered);
        let count = budget.min(remaining);
        for _ in 0..count {
            let index = self.cells.len() - 1 - self.gathered;
            let (cell, height) = self.cells[index];
            self.samples[index] = Some(sample_cell_light(cell, height, self.profile, &self.lights));
            self.gathered += 1;
        }
        self.is_gathered()
    }

    pub fn gather_all(&mut self) {
        self.gather(self.cells.len());
    }

    pub fn is_gathered(&self) -> bool {
        self.gathered == self.cells.len()
    }

    /// Build the replacement forward only after all samples are gathered.
    /// Returning `None` before then enforces the native atomic-commit boundary.
    pub fn commit(self) -> Option<CellLightGrid> {
        if !self.is_gathered() {
            return None;
        }
        let mut grid = CellLightGrid::with_detail_level(self.detail_level);
        for sample in self.samples.into_iter().flatten() {
            let light = if sample.neutral {
                neutral_cell_light(&mut grid.profiles)
            } else {
                build_cell_light_from_raw(
                    &mut grid.profiles,
                    sample.raw_rgb,
                    sample.raw_additive_intensity,
                    sample.raw_top_scalar,
                    sample.raw_bottom_scalar,
                    self.detail_level,
                )
            };
            grid.insert_light(sample.cell, light);
        }
        Some(grid)
    }

    /// Commit a gathered source-area delta into the existing visible grid.
    /// Samples are applied forward, matching native pending-vector commit order.
    pub fn commit_into(self, grid: &mut CellLightGrid) -> bool {
        if !self.is_gathered() {
            return false;
        }
        grid.detail_level = self.detail_level;
        for sample in self.samples.into_iter().flatten() {
            let light = if sample.neutral {
                neutral_cell_light(&mut grid.profiles)
            } else {
                build_cell_light_from_raw(
                    &mut grid.profiles,
                    sample.raw_rgb,
                    sample.raw_additive_intensity,
                    sample.raw_top_scalar,
                    sample.raw_bottom_scalar,
                    self.detail_level,
                )
            };
            grid.insert_light(sample.cell, light);
        }
        true
    }
}

/// Native `LightSourceClass::ApplyAreaLightConvert @ 0x00554AF0` area walk.
/// The inclusive square is outer-Y/inner-X and the final gate uses truncated
/// center-to-source XY distance `<= radius`.
pub fn point_light_area_cells(
    light: &PointLight,
    width: u16,
    height: u16,
    mut height_at: impl FnMut(u16, u16) -> Option<u8>,
) -> Vec<((u16, u16), u8)> {
    if light.radius_leptons <= 0 || width == 0 || height == 0 {
        return Vec::new();
    }
    let radius_cells = light.radius_leptons / LEPTONS_PER_CELL + 1;
    let source_rx = light.center_x.div_euclid(LEPTONS_PER_CELL);
    let source_ry = light.center_y.div_euclid(LEPTONS_PER_CELL);
    let mut cells = Vec::new();
    for ry in source_ry - radius_cells..=source_ry + radius_cells {
        for rx in source_rx - radius_cells..=source_rx + radius_cells {
            if rx < 0 || ry < 0 || rx >= i32::from(width) || ry >= i32::from(height) {
                continue;
            }
            let rx = rx as u16;
            let ry = ry as u16;
            let Some(cell_height) = height_at(rx, ry) else {
                continue;
            };
            let center_x = i32::from(rx) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
            let center_y = i32::from(ry) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
            let dx = i64::from(center_x - light.center_x);
            let dy = i64::from(center_y - light.center_y);
            if integer_sqrt(dx * dx + dy * dy) <= i64::from(light.radius_leptons) {
                cells.push(((rx, ry), cell_height));
            }
        }
    }
    cells
}

/// Collect point light sources from map-placed buildings with nonzero LightIntensity.
///
/// Iterates all structure entities on the map and checks their ObjectType
/// for light emission properties parsed from rules.ini.
#[cfg(test)]
pub fn collect_building_lights(entities: &[MapEntity], rules: Option<&RuleSet>) -> Vec<PointLight> {
    let Some(rules) = rules else {
        return Vec::new();
    };
    let mut lights = Vec::new();
    for ent in entities {
        if ent.category != EntityCategory::Structure {
            continue;
        }
        let Some(obj) = rules.object(&ent.type_id) else {
            continue;
        };
        if let Some(light) = point_light_from_object(
            ent.cell_x,
            ent.cell_y,
            obj.light_visibility,
            obj.light_intensity,
            [
                obj.light_red_tint,
                obj.light_green_tint,
                obj.light_blue_tint,
            ],
        ) {
            lights.push(light);
        }
    }
    lights
}

/// Build a point light from object light fields at a cell origin.
pub fn point_light_from_object(
    rx: u16,
    ry: u16,
    visibility_leptons: i32,
    intensity: f32,
    tint: [f32; 3],
) -> Option<PointLight> {
    let intensity = light_value_to_units(intensity);
    if intensity == 0 {
        return None;
    }
    let radius_leptons = visibility_leptons.max(0);
    if radius_leptons == 0 {
        return None;
    }
    Some(PointLight {
        rx,
        ry,
        center_x: i32::from(rx) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
        center_y: i32::from(ry) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
        radius_leptons,
        intensity,
        tint: [
            light_value_to_units(tint[0]),
            light_value_to_units(tint[1]),
            light_value_to_units(tint[2]),
        ],
        active: true,
        detail: true,
    })
}

/// Build a radiation-glow point light at a cell center.
///
/// `intensity` and `tint` are already in `1000 == 1.0` units (the radiation
/// light math is done by the caller). Detail is forced on: radiation is never
/// culled by the detail level, unlike ordinary lamps.
pub fn radiation_point_light(
    rx: u16,
    ry: u16,
    radius_leptons: i32,
    intensity: i32,
    tint: [i32; 3],
) -> PointLight {
    PointLight {
        rx,
        ry,
        center_x: i32::from(rx) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
        center_y: i32::from(ry) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
        radius_leptons: radius_leptons.max(0),
        intensity,
        tint,
        active: true,
        detail: true,
    }
}

/// Accumulate point light contributions into an existing CellLightGrid.
///
/// Contributions are signed and summed before one final clamp per channel.
pub fn accumulate_point_lights(grid: &mut CellLightGrid, lights: &[PointLight]) {
    if lights.is_empty() {
        return;
    }
    let cells: Vec<((u16, u16), CellLight)> = grid
        .cells()
        .map(|(cell, light)| (cell, light.clone()))
        .collect();
    for (cell, base_light) in cells {
        let cell_center_x = i32::from(cell.0) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
        let cell_center_y = i32::from(cell.1) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
        let mut raw_rgb = base_light.raw_rgb;
        let mut raw_additive = base_light.raw_additive_intensity;
        let mut raw_top = base_light.raw_top_scalar;
        let mut raw_bottom = base_light.raw_bottom_scalar;
        for light in lights {
            if !light.active || !light.detail || light.radius_leptons <= 0 {
                continue;
            }
            let dx = i64::from(cell_center_x - light.center_x);
            let dy = i64::from(cell_center_y - light.center_y);
            let distance_sq = dx * dx + dy * dy;
            let radius = i64::from(light.radius_leptons);
            if distance_sq > radius * radius {
                continue;
            }
            let distance = integer_sqrt(distance_sq);
            let falloff = radius - distance;
            let factor = (falloff * i64::from(LIGHT_UNIT)) / radius;
            let intensity = signed_div_1000(i64::from(light.intensity) * factor);
            raw_additive += intensity;
            raw_top += intensity;
            raw_bottom += intensity;
            for (channel, raw) in raw_rgb.iter_mut().enumerate() {
                *raw += signed_div_1000(i64::from(light.tint[channel]) * factor);
            }
        }
        let updated = build_cell_light_from_raw(
            &mut grid.profiles,
            raw_rgb,
            raw_additive,
            raw_top,
            raw_bottom,
            grid.detail_level,
        );
        grid.insert_light(cell, updated);
    }
}

fn sample_cell_light(
    cell: (u16, u16),
    height: u8,
    profile: LightingProfileUnits,
    lights: &[PointLight],
) -> CellLightSample {
    if cell == (0, 0) {
        return CellLightSample {
            cell,
            neutral: true,
            raw_rgb: [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT],
            raw_additive_intensity: 0,
            raw_top_scalar: LIGHT_UNIT,
            raw_bottom_scalar: LIGHT_UNIT,
        };
    }
    let units = scenario_units_from_profile(profile);
    let mut raw_rgb = [units.red, units.green, units.blue];
    let mut raw_additive_intensity = 0;
    let mut raw_top_scalar = units.ambient + units.level * i32::from(height) - units.ground;
    let mut raw_bottom_scalar =
        units.ambient + units.level * (i32::from(height) + BOTTOM_LEVEL_OFFSET) - units.ground;
    accumulate_light_sample(
        cell,
        lights,
        &mut raw_rgb,
        &mut raw_additive_intensity,
        &mut raw_top_scalar,
        &mut raw_bottom_scalar,
    );
    CellLightSample {
        cell,
        neutral: false,
        raw_rgb,
        raw_additive_intensity,
        raw_top_scalar,
        raw_bottom_scalar,
    }
}

fn accumulate_light_sample(
    cell: (u16, u16),
    lights: &[PointLight],
    raw_rgb: &mut LightRgbKey,
    raw_additive: &mut i32,
    raw_top: &mut i32,
    raw_bottom: &mut i32,
) {
    let cell_center_x = i32::from(cell.0) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
    let cell_center_y = i32::from(cell.1) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS;
    for light in lights {
        if !light.active || !light.detail || light.radius_leptons <= 0 {
            continue;
        }
        let dx = i64::from(cell_center_x - light.center_x);
        let dy = i64::from(cell_center_y - light.center_y);
        let distance_sq = dx * dx + dy * dy;
        let radius = i64::from(light.radius_leptons);
        if distance_sq > radius * radius {
            continue;
        }
        let distance = integer_sqrt(distance_sq);
        let factor = ((radius - distance) * i64::from(LIGHT_UNIT)) / radius;
        let intensity = signed_div_1000(i64::from(light.intensity) * factor);
        *raw_additive += intensity;
        *raw_top += intensity;
        *raw_bottom += intensity;
        for (channel, raw) in raw_rgb.iter_mut().enumerate() {
            *raw += signed_div_1000(i64::from(light.tint[channel]) * factor);
        }
    }
}

/// Convert INI light values using the verified `value * 1000 + 0.1` shape.
pub fn light_value_to_units(value: f32) -> i32 {
    (value * POINT_LIGHT_UNIT_SCALE as f32 + 0.1) as i32
}

fn light_float_to_units(value: f32) -> i32 {
    (value * LIGHT_UNIT as f32).round() as i32
}

fn integer_sqrt(value: i64) -> i64 {
    (value as f64).sqrt() as i64
}

#[derive(Debug, Clone, Copy)]
struct ScenarioLightUnits {
    ambient: i32,
    red: i32,
    green: i32,
    blue: i32,
    ground: i32,
    level: i32,
}

#[derive(Debug, Clone, Copy)]
struct NormalizedLight {
    scale16: i32,
    common_scalar: i32,
    rgb_key: LightRgbKey,
}

fn scenario_profile_units(config: &LightingConfig) -> LightingProfileUnits {
    LightingProfileUnits {
        ambient_percent: (f64::from(config.ambient) * 100.0 + 0.01) as i32,
        red_percent: (f64::from(config.red) * 100.0 + 0.01) as i32,
        green_percent: (f64::from(config.green) * 100.0 + 0.01) as i32,
        blue_percent: (f64::from(config.blue) * 100.0 + 0.01) as i32,
        ground_units: quantize_scenario_light(f64::from(config.ground), GROUND_LEVEL_UNIT_SCALE),
        level_units: quantize_scenario_light(f64::from(config.level), GROUND_LEVEL_UNIT_SCALE),
    }
}

/// Convert the compatibility map config into the exact ordinary profile
/// shape used by the integer cell-light builder.
pub fn normal_profile_units(config: &LightingConfig) -> LightingProfileUnits {
    scenario_profile_units(config)
}

fn scenario_units_from_profile(profile: LightingProfileUnits) -> ScenarioLightUnits {
    ScenarioLightUnits {
        ambient: profile.ambient_percent.wrapping_mul(10),
        red: profile.red_percent.wrapping_mul(10),
        green: profile.green_percent.wrapping_mul(10),
        blue: profile.blue_percent.wrapping_mul(10),
        ground: profile.ground_units,
        level: profile.level_units,
    }
}

fn build_cell_light_from_raw(
    profiles: &mut LightProfileCache,
    raw_rgb: LightRgbKey,
    raw_additive_intensity: i32,
    raw_top_scalar: i32,
    raw_bottom_scalar: i32,
    detail_level: u32,
) -> CellLight {
    let top_high = raw_top_scalar.min(LIGHT_CLAMP_MAX);
    // CellClass 004845C0/C2 seeds both fields from capped top, then passes
    // the common pointer in EDX to 005558E0. The helper scales common (and
    // zeros it for near-black RGB); it does not normalize source additive.
    // Original 00483F7A..00483FD8 publishes those distinct outputs. See
    // LIGHTCONVERT_ROW_RGB565_ORACLE_2026_09_09.md and palette_oracle fixtures.
    let normalized = normalize_light_at_detail(raw_rgb, top_high, detail_level);
    let common_high = normalized.common_scalar;
    let scaled_bottom = ((i64::from(raw_bottom_scalar) * i64::from(normalized.scale16)) >> 16)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let bottom_high = scaled_bottom.min(LIGHT_CLAMP_MAX);
    let top_scalar = clamp_light_scalar(top_high);
    let common_scalar = clamp_light_scalar(common_high);
    let bottom_scalar = clamp_light_scalar(bottom_high);
    let profile_id = profiles.profile_id_for_key(normalized.rgb_key);
    CellLight::new(
        profile_id,
        normalized.rgb_key,
        raw_rgb,
        normalized.scale16,
        raw_additive_intensity,
        raw_additive_intensity,
        raw_top_scalar,
        raw_bottom_scalar,
        top_scalar,
        common_scalar,
        bottom_scalar,
    )
}

fn neutral_cell_light(profiles: &mut LightProfileCache) -> CellLight {
    let rgb_key = [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT];
    let profile_id = profiles.profile_id_for_key(rgb_key);
    CellLight::new(
        profile_id,
        rgb_key,
        rgb_key,
        LIGHT_SCALE16_IDENTITY,
        0,
        0,
        LIGHT_UNIT,
        LIGHT_UNIT,
        LIGHT_UNIT,
        LIGHT_UNIT,
        LIGHT_UNIT,
    )
}

#[cfg(test)]
fn normalize_light(raw_rgb: LightRgbKey, common_scalar: i32) -> NormalizedLight {
    normalize_light_at_detail(raw_rgb, common_scalar, 2)
}

fn normalize_light_at_detail(
    raw_rgb: LightRgbKey,
    common_scalar: i32,
    detail_level: u32,
) -> NormalizedLight {
    let mut rgb = [
        raw_rgb[0].clamp(0, LIGHT_CLAMP_MAX),
        raw_rgb[1].clamp(0, LIGHT_CLAMP_MAX),
        raw_rgb[2].clamp(0, LIGHT_CLAMP_MAX),
    ];
    let mut common = common_scalar;
    let mut scale16 = LIGHT_SCALE16_IDENTITY;

    if rgb != [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT] {
        let max_channel = rgb[0].max(rgb[1]).max(rgb[2]);
        scale16 = ((i64::from(max_channel) * i64::from(LIGHT_SCALE16_IDENTITY))
            / i64::from(LIGHT_UNIT)) as i32;
        if scale16 < 66 {
            scale16 = LIGHT_SCALE16_IDENTITY;
            rgb = [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT];
            common = 0;
        } else {
            if rgb[0] >= rgb[1] && rgb[0] >= rgb[2] {
                let max = rgb[0].max(1);
                rgb[1] = normalize_channel(rgb[1], max);
                rgb[2] = normalize_channel(rgb[2], max);
                rgb[0] = LIGHT_UNIT;
            } else if rgb[1] >= rgb[2] {
                let scale = scale16.max(1);
                rgb[0] = normalize_channel_by_scale(rgb[0], scale);
                rgb[2] = normalize_channel_by_scale(rgb[2], scale);
                rgb[1] = LIGHT_UNIT;
            } else {
                let scale = scale16.max(1);
                rgb[0] = normalize_channel_by_scale(rgb[0], scale);
                rgb[1] = normalize_channel_by_scale(rgb[1], scale);
                rgb[2] = LIGHT_UNIT;
            }
            common = ((i64::from(scale16) * i64::from(common)) >> 16)
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        }
    }
    if common > LIGHT_CLAMP_MAX {
        common = LIGHT_CLAMP_MAX;
    }

    NormalizedLight {
        scale16,
        common_scalar: common,
        rgb_key: quantize_rgb_key(rgb, detail_level),
    }
}

fn normalize_channel(channel: i32, max_channel: i32) -> i32 {
    ((i64::from(channel) * i64::from(LIGHT_UNIT)) / i64::from(max_channel)) as i32
}

fn normalize_channel_by_scale(channel: i32, scale16: i32) -> i32 {
    ((i64::from(channel) * i64::from(LIGHT_SCALE16_IDENTITY)) / i64::from(scale16)) as i32
}

fn quantize_rgb_key(rgb: LightRgbKey, detail_level: u32) -> LightRgbKey {
    if rgb == [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT] {
        return rgb;
    }
    let mask = match detail_level {
        0 => DETAIL_LOW_RGB_MASK,
        1 => DETAIL_MEDIUM_RGB_MASK,
        _ => DETAIL_HIGH_RGB_MASK,
    };
    [
        (rgb[0].clamp(0, LIGHT_UNIT)) & mask,
        (rgb[1].clamp(0, LIGHT_UNIT)) & mask,
        (rgb[2].clamp(0, LIGHT_UNIT)) & mask,
    ]
}

fn clamp_light_scalar(value: i32) -> i32 {
    value.clamp(LIGHT_CLAMP_MIN, LIGHT_CLAMP_MAX)
}

fn signed_div_1000(value: i64) -> i32 {
    (value / i64::from(LIGHT_UNIT)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;

    #[test]
    fn native_cell_light_finalization_matches_original_caller_fields() {
        // Original 483EB0 pointer setup, 4845A2 -> complete5558E0, literal
        // 483F7A Cell stores, and 555AC0 key quantization. The fixture spans
        // every clamped maximum in each dominant-channel branch, detail0..2,
        // top/common caps, negative values and the near-black reset.
        let bytes =
            include_bytes!("../../tools/palette_oracle/fixtures/cell-light-finalization.bin");
        assert_eq!(bytes.len() % 60, 0);
        for record in bytes.chunks_exact(60) {
            let row: Vec<i32> = record
                .chunks_exact(4)
                .map(|v| i32::from_le_bytes(v.try_into().unwrap()))
                .collect();
            let mut profiles = LightProfileCache::new();
            let light = build_cell_light_from_raw(
                &mut profiles,
                row[..3].try_into().unwrap(),
                row[3],
                row[4],
                row[5],
                row[6] as u32,
            );
            assert_eq!(
                [
                    light.scale16,
                    light.additive_intensity,
                    light.top_scalar,
                    light.common_scalar,
                    light.bottom_scalar,
                    light.rgb_key[0],
                    light.rgb_key[1],
                    light.rgb_key[2],
                ],
                row[7..],
                "native post-gather inputs {:?}",
                &row[..7]
            );
        }
    }

    #[test]
    fn native_ground_level_ini_quantization_matches_all_four_original_sites() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/palette_oracle/fixtures/ground-level.json"
        ))
        .expect("original instruction fixture");
        for record in fixture["authored"].as_array().unwrap() {
            let token = record["token"].as_str().unwrap();
            let expected = record["units"].as_i64().unwrap() as i32;
            let ini = IniFile::from_str(&format!(
                "[Lighting]\nGround={token}\nLevel={token}\nIonGround={token}\nIonLevel={token}\n"
            ));
            let parsed = parse_lighting_profiles(&ini);
            assert_eq!(
                [
                    parsed.normal.ground_units,
                    parsed.normal.level_units,
                    parsed.ion.ground_units,
                    parsed.ion.level_units
                ],
                [expected; 4],
                "original 68A92E/68A968/68AA8A/68AAC4 token {token}"
            );
            let config_units = normal_profile_units(&parse_lighting(&ini));
            assert_eq!(config_units, parsed.normal, "config path token {token}");
        }
    }

    #[test]
    fn native_ground_level_defaults_and_authored_values_reach_cell_grid() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/palette_oracle/fixtures/ground-level.json"
        ))
        .expect("original instruction fixture");
        // Distinguish absent section, empty section, either key absent, explicit
        // editor values, Ground fixture, and the actual flat8 Level=.032 scene.
        for (text, height, record) in [
            ("", 0, 0),
            ("[Lighting]\n", 0, 0),
            ("[Lighting]\nGround=.05\n", 4, 1),
            ("[Lighting]\nLevel=.008\n", 4, 1),
            ("[Lighting]\nGround=.20\nLevel=.032\n", 0, 2),
            ("[Lighting]\nGround=.20\nLevel=.032\n", 4, 3),
            ("[Lighting]\nGround=.40\nLevel=0\n", 0, 4),
            ("[Lighting]\nGround=0\nLevel=.032\n", 0, 5),
        ] {
            let ini = IniFile::from_str(text);
            let units = parse_lighting_profiles(&ini).normal;
            let config = parse_lighting(&ini);
            let expected: Vec<i32> = fixture["cell_finalizations"][record]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_i64().unwrap() as i32)
                .collect();
            for grid in [
                build_cell_light_grid_from_heights_and_units([((48, 48), height)], units),
                build_cell_light_grid_from_heights([((48, 48), height)], &config),
            ] {
                let light = grid.cell_light_at((48, 48)).unwrap();
                assert_eq!(
                    [
                        light.scale16,
                        light.additive_intensity,
                        light.top_scalar,
                        light.common_scalar,
                        light.bottom_scalar,
                        light.rgb_key[0],
                        light.rgb_key[1],
                        light.rgb_key[2]
                    ],
                    expected[7..],
                    "INI {text:?} height {height}"
                );
            }
        }
    }

    #[test]
    fn test_default_lighting_uses_yr_ground_subtraction() {
        let config: LightingConfig = LightingConfig::default();
        assert!((config.ground - 0.05).abs() < 0.001);
        let tint: [f32; 3] = cell_tint(&config, 0);
        assert!((tint[0] - 0.95).abs() < 0.001);
        assert!((tint[1] - 0.95).abs() < 0.001);
        assert!((tint[2] - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_parse_lighting_uses_yr_defaults() {
        let ini = IniFile::from_str("[Lighting]\nFixtureOnly=1\n");
        let config = parse_lighting(&ini);
        assert!((config.ambient - 1.0).abs() < 0.001);
        assert!((config.red - 1.0).abs() < 0.001);
        assert!((config.green - 1.0).abs() < 0.001);
        assert!((config.blue - 1.0).abs() < 0.001);
        assert!((config.ground - 0.05).abs() < 0.001);
        assert!((config.level - 0.008).abs() < 0.001);
        assert!((config.ion_ambient - 0.87).abs() < 0.001);
        assert!((config.ion_red - 0.30).abs() < 0.001);
        assert!((config.ion_green - 0.40).abs() < 0.001);
        assert!((config.ion_blue - 0.75).abs() < 0.001);
        assert_eq!(
            parse_lighting_profiles(&ini),
            ParsedLightingProfiles::default()
        );
    }

    #[test]
    fn gsi_04_20_lighting_profiles_use_native_integer_scales_and_truncation() {
        let ini = IniFile::from_str(
            "[Lighting]\n\
             Ambient=.8798\nRed=.3198\nGreen=.4198\nBlue=.7598\n\
             Ground=.0039\nLevel=.0319\n\
             IonAmbient=.8698\nIonRed=.2998\nIonGreen=.3998\nIonBlue=.7498\n\
             IonGround=.0079\nIonLevel=.0119\n",
        );
        let parsed = parse_lighting_profiles(&ini);
        assert_eq!(
            parsed.normal,
            LightingProfileUnits {
                ambient_percent: 87,
                red_percent: 31,
                green_percent: 41,
                blue_percent: 75,
                ground_units: 3,
                level_units: 31,
            }
        );
        assert_eq!(
            parsed.ion,
            LightingProfileUnits {
                ambient_percent: 86,
                red_percent: 29,
                green_percent: 39,
                blue_percent: 74,
                ground_units: 7,
                level_units: 11,
            }
        );

        let grid = build_cell_light_grid_from_heights_and_units([((3, 4), 2)], parsed.ion);
        let light = grid.cell_light_at((3, 4)).expect("light");
        // Original INI conversion plus 4845A2 -> 5558E0 fixture: 875/647.
        assert_eq!(light.top_scalar, 875);
        assert_eq!(light.common_scalar, 647);
        assert_eq!(light.raw_rgb, [290, 390, 740]);
    }

    #[test]
    fn gsi_04_20_detail_level_requantizes_lightconvert_rgb() {
        let profile = LightingProfileUnits {
            ambient_percent: 100,
            red_percent: 100,
            green_percent: 88,
            blue_percent: 88,
            ground_units: 0,
            level_units: 0,
        };
        let key_at = |detail_level| {
            build_cell_light_grid_from_heights_and_units_with_detail(
                [((3, 4), 0)],
                profile,
                detail_level,
            )
            .cell_light_at((3, 4))
            .expect("light")
            .rgb_key
        };

        let low = key_at(0);
        let medium = key_at(1);
        let high = key_at(2);
        assert_eq!(low, [896, 768, 768]);
        assert_eq!(medium, [960, 832, 832]);
        assert_eq!(high, [992, 864, 864]);
    }

    #[test]
    fn default_flat_no_lamps_ground_scalar_is_950() {
        let grid = build_cell_light_grid_from_heights([((3, 4), 0)], &LightingConfig::default());
        let light = grid.cell_light_at((3, 4)).expect("light");

        assert_eq!(light.common_scalar, 950);
        assert_eq!(light.top_scalar, 950);
        assert_eq!(light.bottom_scalar, 982);
        assert_eq!(grid.terrain_tile_tint_at((3, 4)), [0.95, 0.95, 0.95]);
    }

    #[test]
    fn sentinel_cell_zero_zero_uses_neutral_light() {
        let grid = build_cell_light_grid_from_heights([((0, 0), 0)], &LightingConfig::default());
        let light = grid.cell_light_at((0, 0)).expect("light");

        assert_eq!(light.scale16, LIGHT_SCALE16_IDENTITY);
        assert_eq!(light.additive_intensity, 0);
        assert_eq!(light.rgb_key, [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT]);
        assert_eq!(light.common_scalar, LIGHT_UNIT);
        assert_eq!(light.top_scalar, LIGHT_UNIT);
        assert_eq!(light.bottom_scalar, LIGHT_UNIT);
        assert_eq!(grid.tint_or_default((0, 0)), DEFAULT_TINT);
    }

    #[test]
    fn default_raised_no_lamps_common_and_bottom_scalars_match_gamemd() {
        let grid = build_cell_light_grid_from_heights([((10, 11), 4)], &LightingConfig::default());
        let light = grid.cell_light_at((10, 11)).expect("light");

        assert_eq!(light.common_scalar, 982);
        assert_eq!(light.top_scalar, 982);
        assert_eq!(light.bottom_scalar, 1014);
        assert_eq!(
            cell_ground_z_adjust(parse_lighting_profiles(&IniFile::from_str("")).normal, 4),
            982
        );
    }

    #[test]
    fn test_elevation_boost() {
        let config: LightingConfig = LightingConfig::default();
        let tint_z0: [f32; 3] = cell_tint(&config, 0);
        let tint_z4: [f32; 3] = cell_tint(&config, 4);
        // Default YR units: 1000 + Level(8) * z(4) - Ground(50) = 982.
        assert!(tint_z4[0] > tint_z0[0]);
        assert!((tint_z4[0] - 0.982).abs() < 0.01);
        let grid = build_cell_light_grid_from_heights([((10, 11), 4)], &config);
        let light = grid.cell_light_at((10, 11)).expect("light");
        assert_eq!(light.common_scalar, 982);
        assert_eq!(light.bottom_scalar, 1014);
    }

    #[test]
    fn test_dark_map() {
        let config: LightingConfig = LightingConfig {
            ambient: 0.5,
            red: 1.0,
            green: 0.8,
            blue: 0.6,
            ground: 0.0,
            level: 0.0,
            ..LightingConfig::default()
        };
        let grid = build_cell_light_grid_from_heights([((1, 1), 0)], &config);
        let light = grid.cell_light_at((1, 1)).expect("light");
        assert_eq!(light.common_scalar, 500);
        assert_eq!(light.raw_rgb, [1000, 800, 600]);
    }

    #[test]
    fn test_cap_at_two() {
        let config: LightingConfig = LightingConfig {
            ambient: 3.0,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            ground: 0.0,
            level: 0.0,
            ..LightingConfig::default()
        };
        let tint: [f32; 3] = cell_tint(&config, 0);
        // 3.0 > 2.0 cap → scaled to 2.0
        assert!((tint[0] - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_ground_darkening() {
        let config: LightingConfig = LightingConfig {
            ambient: 1.0,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            ground: 0.5,
            level: 0.0,
            ..LightingConfig::default()
        };
        let grid = build_cell_light_grid_from_heights([((1, 1), 0)], &config);
        let light = grid.cell_light_at((1, 1)).expect("light");
        // Original 0068A92E converts authored Ground=.5 to 500.
        assert_eq!(light.common_scalar, 500);
    }

    #[test]
    fn test_terrain_tint_matches_ground_level_cell_tint() {
        let config: LightingConfig = LightingConfig {
            ambient: 1.0,
            red: 1.0,
            green: 0.88,
            blue: 0.88,
            ground: 0.0,
            level: 0.039,
            ..LightingConfig::default()
        };
        let terrain: [f32; 3] = terrain_tint(&config);
        let ground_cell: [f32; 3] = cell_tint(&config, 0);
        assert_eq!(terrain, ground_cell);
    }

    #[test]
    fn test_light_profile_cache_reuses_identical_rgb() {
        let mut cache = LightProfileCache::new();
        let neutral = cache.default_profile_id();
        let first = cache.profile_id_for_rgb([1.0, 1.0, 1.0]);
        let second = cache.profile_id_for_rgb([1.0, 1.0, 1.0]);

        assert_eq!(neutral, first);
        assert_eq!(first, second);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cell_light_grid_default_cell_uses_neutral_tint() {
        let grid = CellLightGrid::new();
        assert_eq!(grid.tint_or_default((99, 99)), DEFAULT_TINT);
        assert_eq!(grid.profiles().default_profile_id(), LightProfileId(0));
    }

    #[test]
    fn test_radiation_point_light_center_and_flags() {
        // spread 10 -> radius 10*256+128 = 2688; center cell (100,100).
        let light = radiation_point_light(100, 100, 2688, 50, [0, 1000, 0]);
        assert_eq!(light.center_x, 100 * 256 + 128);
        assert_eq!(light.center_y, 100 * 256 + 128);
        assert_eq!(light.radius_leptons, 2688);
        assert_eq!(light.intensity, 50);
        assert_eq!(light.tint, [0, 1000, 0]);
        assert!(
            light.active && light.detail,
            "radiation light is active and detail-forced"
        );
    }

    #[test]
    fn test_cell_light_grid_compatibility_tint() {
        let mut grid = CellLightGrid::new();
        grid.insert_profiled_light((1, 2), [0.8, 0.7, 0.6], 0.5);

        let tint = grid.tint_or_default((1, 2));
        assert!((tint[0] - 0.4).abs() < 0.001);
        assert!((tint[1] - 0.35).abs() < 0.001);
        assert!((tint[2] - 0.3).abs() < 0.001);
    }

    #[test]
    fn gsi_13_10_unit_and_infantry_extra_light_precedes_colour_profile() {
        let mut grid = CellLightGrid::new();
        grid.insert_profiled_light((1, 2), [1.0, 0.88, 0.88], 1.0);

        let unit = grid.unit_tint_at((1, 2), 200);
        let infantry = grid.infantry_tint_at((1, 2), -125);
        for (actual, expected) in unit.into_iter().zip([1.2, 1.056, 1.056]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        for (actual, expected) in infantry.into_iter().zip([0.875, 0.77, 0.77]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert!(
            (unit[1] - 1.08).abs() > 0.001,
            "extra is not an RGB post-add"
        );
    }

    #[test]
    fn gsi_13_10_extra_light_is_signed_unclamped_wrapping_scalar_math() {
        let mut grid = CellLightGrid::new();
        grid.insert_profiled_light((1, 1), DEFAULT_TINT, 1.9);
        grid.insert_profiled_light((2, 2), DEFAULT_TINT, 0.1);

        let over = grid.unit_tint_at((1, 1), 200);
        let negative = grid.infantry_tint_at((2, 2), -200);
        for channel in over {
            assert!((channel - 2.1).abs() < 0.0001);
        }
        for channel in negative {
            assert!((channel + 0.1).abs() < 0.0001);
        }
        assert_eq!(extra_light_scalar(i32::MAX, 1), i32::MIN);
        assert_eq!(extra_light_scalar(i32::MIN, -1), i32::MAX);
    }

    #[test]
    fn gsi_13_10_missing_cell_uses_neutral_top_scalar_before_extra_light() {
        let grid = CellLightGrid::new();
        let tint = grid.unit_tint_at((99, 99), 200);
        for channel in tint {
            assert!((channel - 1.2).abs() < 0.0001);
        }
        assert_eq!(grid.infantry_tint_at((99, 99), 0), DEFAULT_TINT);
    }

    #[test]
    fn test_consumer_accessors_return_compatibility_tint() {
        let mut grid = CellLightGrid::new();
        grid.set_compat_tint((4, 5), [0.7, 0.8, 0.9]);

        assert_eq!(grid.techno_tint_at((4, 5)), [0.7, 0.8, 0.9]);
        assert_eq!(grid.unit_tint_at((4, 5), 0), [0.7, 0.8, 0.9]);
        assert_eq!(grid.infantry_tint_at((4, 5), 0), [0.7, 0.8, 0.9]);
        assert_eq!(grid.aircraft_tint_at((4, 5)), [0.7, 0.8, 0.9]);
        assert_eq!(grid.overlay_tint_at((4, 5)), [0.7, 0.8, 0.9]);
        assert_eq!(grid.terrain_object_tint_at((4, 5)), [0.7, 0.8, 0.9]);
        assert_eq!(grid.anim_tint_at((4, 5), None), [0.7, 0.8, 0.9]);
        assert_eq!(grid.bridge_body_tint_at((4, 5)), [0.7, 0.8, 0.9]);
    }

    #[test]
    fn use_normal_light_anim_ignores_a_colour_shifted_cell() {
        // [Lighting] read verbatim out of the retail Dustbowl map: unit ambient
        // with green and blue pulled down. That colour shift is exactly what
        // reaches an explosion when the animation is drawn through cell light.
        let config = LightingConfig {
            ambient: 1.0,
            red: 1.0,
            green: 0.88,
            blue: 0.88,
            ground: 0.0,
            level: 0.039,
            ..LightingConfig::default()
        };
        let grid = build_cell_light_grid_from_heights([((7, 9), 0)], &config);

        // TWLT070 is one of the 43 stock art sections that set the key.
        // SMOKEY2 is an ordinary animation that does not.
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[TWLT070]\nUseNormalLight=yes\n[SMOKEY2]\nRate=600\n",
        ));
        let flagged = art.anim_runtime_config("TWLT070").expect("TWLT070 art");
        let plain = art.anim_runtime_config("SMOKEY2").expect("SMOKEY2 art");
        assert!(flagged.use_normal_light);
        assert!(!plain.use_normal_light);

        // Unflagged: the cell's normalized profile tints the animation, so the
        // green and blue channels come out below red.
        let lit = grid.anim_tint_at((7, 9), Some(plain));
        assert_eq!(lit, [0.992, 0.864, 0.864]);
        assert!(lit[1] < lit[0] && lit[2] < lit[0]);

        // Flagged: full brightness through the unmodified animation palette, so
        // none of the cell's colour or darkening reaches it.
        assert_eq!(grid.anim_tint_at((7, 9), Some(flagged)), DEFAULT_TINT);
        assert_ne!(grid.anim_tint_at((7, 9), Some(flagged)), lit);

        // No art record found for the draw leaves the flag at its INI default
        // of false, so the lit path still applies.
        assert_eq!(grid.anim_tint_at((7, 9), None), lit);
    }

    #[test]
    fn use_normal_light_anim_ignores_a_darkened_cell() {
        // Ambient below 1.0 is the other half of the symptom: on a dark map an
        // explosion drawn through cell light comes out dimmer than retail.
        let config = LightingConfig {
            ambient: 0.68,
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            ground: 0.0,
            level: 0.032,
            ..LightingConfig::default()
        };
        let grid = build_cell_light_grid_from_heights([((7, 9), 0)], &config);
        let art = ArtRegistry::from_ini(&IniFile::from_str(
            "[S_BANG24]\nUseNormalLight=yes\n[SMOKEY2]\nRate=600\n",
        ));
        let flagged = art.anim_runtime_config("S_BANG24").expect("S_BANG24 art");
        let plain = art.anim_runtime_config("SMOKEY2").expect("SMOKEY2 art");

        assert_eq!(grid.anim_tint_at((7, 9), Some(plain)), [0.68, 0.68, 0.68]);
        assert_eq!(grid.anim_tint_at((7, 9), Some(flagged)), DEFAULT_TINT);
    }

    #[test]
    fn terrain_object_instances_choose_branch_specific_cell_light() {
        let mut grid = CellLightGrid::new();
        let profile_id = grid
            .profiles
            .profile_id_for_key([LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT]);
        grid.insert_light(
            (4, 5),
            CellLight::new(
                profile_id,
                [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT],
                [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT],
                LIGHT_SCALE16_IDENTITY,
                0,
                0,
                1200,
                900,
                1200,
                900,
                900,
            ),
        );

        assert_eq!(
            grid.terrain_object_tint_for_type((4, 5), false),
            [0.9, 0.9, 0.9]
        );
        assert_eq!(
            grid.terrain_object_tint_for_type((4, 5), true),
            [1.2, 1.2, 1.2]
        );
    }

    #[test]
    fn test_building_body_depth_adjustment_keeps_signed_extra_light() {
        assert_eq!(building_body_depth_adjustment(350), 350);
        assert_eq!(building_body_depth_adjustment(-100), -100);
    }

    #[test]
    fn test_base_cell_light_grid_matches_cell_tint() {
        let config = LightingConfig {
            ambient: 1.0,
            red: 0.8,
            green: 0.9,
            blue: 1.0,
            ground: 0.20,
            level: 0.032,
            ..LightingConfig::default()
        };
        let grid = build_cell_light_grid_from_heights([((10, 10), 0), ((10, 11), 4)], &config);

        for (cell, z) in [((10, 10), 0), ((10, 11), 4)] {
            let expected = cell_tint(&config, z);
            let actual = grid.tint_or_default(cell);
            assert!((actual[0] - expected[0]).abs() < 0.001);
            assert!((actual[1] - expected[1]).abs() < 0.001);
            assert!((actual[2] - expected[2]).abs() < 0.001);
        }
    }

    #[test]
    fn test_base_cell_light_grid_reuses_neutral_profile() {
        let config = LightingConfig::default();
        let grid = build_cell_light_grid_from_heights([((1, 1), 0), ((1, 0), 3)], &config);

        assert_eq!(grid.profiles().len(), 1);
        assert_eq!(
            grid.cell_light_at((1, 1)).map(|light| light.profile_id),
            Some(LightProfileId(0))
        );
        assert_eq!(
            grid.cell_light_at((1, 0)).map(|light| light.profile_id),
            Some(LightProfileId(0))
        );
    }

    /// Helper: build a small grid with uniform tint for testing point lights.
    fn test_grid(size: u16, base_tint: [f32; 3]) -> CellLightGrid {
        let mut grid = CellLightGrid::with_capacity(usize::from(size) * usize::from(size));
        let base_scalar = light_float_to_units(base_tint[0]);
        for y in 0..size {
            for x in 0..size {
                let light = build_cell_light_from_raw(
                    &mut grid.profiles,
                    [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT],
                    0,
                    base_scalar,
                    base_scalar,
                    2,
                );
                grid.insert_light((x, y), light);
            }
        }
        grid
    }

    fn test_point_light(
        rx: u16,
        ry: u16,
        radius_leptons: i32,
        intensity: f32,
        tint: [f32; 3],
    ) -> PointLight {
        PointLight {
            rx,
            ry,
            center_x: i32::from(rx) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
            center_y: i32::from(ry) * LEPTONS_PER_CELL + HALF_CELL_LEPTONS,
            radius_leptons,
            intensity: light_value_to_units(intensity),
            tint: [
                light_value_to_units(tint[0]),
                light_value_to_units(tint[1]),
                light_value_to_units(tint[2]),
            ],
            active: true,
            detail: true,
        }
    }

    #[test]
    fn test_point_light_linear_falloff() {
        let mut grid = test_grid(20, [0.5, 0.5, 0.5]);
        // Zero RGB contribution isolates intensity falloff from the separately
        // tested native common-scalar color normalization.
        let light = test_point_light(10, 10, 5 * LEPTONS_PER_CELL, 1.0, [0.0; 3]);
        accumulate_point_lights(&mut grid, &[light]);

        // Center cell gets full intensity: 0.5 + 1.0 = 1.5
        let center = grid.tint_or_default((10, 10));
        assert!((center[0] - 1.5).abs() < 0.01, "center={:.3}", center[0]);

        // Cell at distance 2.5 gets (5-2.5)/5 = 0.5 intensity: 0.5 + 0.5 = 1.0
        // (2, 1.5 diagonal ≈ 2.5 cells — use (12, 10) which is distance 2.0)
        let d2 = grid.tint_or_default((12, 10));
        let expected_d2 = 0.5 + (5.0 - 2.0) / 5.0;
        assert!(
            (d2[0] - expected_d2).abs() < 0.01,
            "d2={:.3} expected={:.3}",
            d2[0],
            expected_d2
        );

        // Cell at distance 5+ is unchanged: still 0.5
        let far = grid.tint_or_default((15, 10));
        assert!((far[0] - 0.5).abs() < 0.01, "far={:.3}", far[0]);
    }

    #[test]
    fn test_point_light_boundary_is_inclusive_with_zero_edge_contribution() {
        let mut grid = test_grid(8, [0.5, 0.5, 0.5]);
        let light = test_point_light(2, 2, 2 * LEPTONS_PER_CELL, 1.0, [0.0; 3]);
        accumulate_point_lights(&mut grid, &[light]);

        let edge = grid.tint_or_default((4, 2));
        assert_eq!(edge, [0.5, 0.5, 0.5]);
        let inside = grid.tint_or_default((3, 2));
        assert!((inside[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_negative_point_light_darkens_cell() {
        let mut grid = test_grid(5, [0.8, 0.8, 0.8]);
        let light = test_point_light(2, 2, 2 * LEPTONS_PER_CELL, -0.2, [0.0; 3]);
        accumulate_point_lights(&mut grid, &[light]);

        let center = grid.tint_or_default((2, 2));
        assert!((center[0] - 0.6).abs() < 0.001);
        assert!((center[1] - 0.6).abs() < 0.001);
        assert!((center[2] - 0.6).abs() < 0.001);
    }

    #[test]
    fn test_point_lights_sum_before_clamp() {
        let mut grid = test_grid(5, [1.9, 1.9, 1.9]);
        let brighten = test_point_light(2, 2, 2 * LEPTONS_PER_CELL, 0.3, [0.0; 3]);
        let darken = test_point_light(2, 2, 2 * LEPTONS_PER_CELL, -0.3002, [0.0; 3]);
        accumulate_point_lights(&mut grid, &[brighten, darken]);

        let center = grid.tint_or_default((2, 2));
        assert!((center[0] - 1.9).abs() < 0.001);
    }

    #[test]
    fn test_point_light_division_truncates_toward_zero() {
        let mut grid = test_grid(1, [0.0, 0.0, 0.0]);
        let mut light = test_point_light(0, 0, 3, 1.0, [0.0; 3]);
        light.center_x -= 1;
        accumulate_point_lights(&mut grid, &[light]);

        let center = grid.tint_or_default((0, 0));
        assert!((center[0] - 0.666).abs() < 0.001);
    }

    #[test]
    fn test_point_light_colored_tint() {
        let mut grid = test_grid(10, [0.3, 0.3, 0.3]);
        let light = test_point_light(5, 5, 3 * LEPTONS_PER_CELL, 0.6, [1.0, 0.0, 0.5]);
        accumulate_point_lights(&mut grid, &[light]);

        let light = grid.cell_light_at((5, 5)).expect("light");
        assert_eq!(light.raw_additive_intensity, 600);
        assert_eq!(light.raw_rgb, [2000, 1000, 1500]);
        // Native finalization fixture: color scale131072 affects common only.
        assert_eq!(light.top_scalar, 900);
        assert_eq!(light.common_scalar, 1800);
        assert_eq!(light.rgb_key, [992, 480, 736]);
    }

    #[test]
    fn test_accumulation_clamps_at_cap() {
        let mut grid = test_grid(5, [1.8, 1.8, 1.8]);
        let light = test_point_light(2, 2, 3 * LEPTONS_PER_CELL, 1.0, [0.0; 3]);
        accumulate_point_lights(&mut grid, &[light]);

        let center = grid.tint_or_default((2, 2));
        // 1.8 + 1.0 = 2.8 → clamped to 2.0
        assert!((center[0] - TOTAL_AMBIENT_CAP).abs() < 0.001);
    }

    #[test]
    fn test_light_value_to_units_uses_verified_bias() {
        assert_eq!(light_value_to_units(0.0), 0);
        assert_eq!(light_value_to_units(0.01), 10);
        assert_eq!(light_value_to_units(0.5), 500);
        assert_eq!(light_value_to_units(-0.5), -499);
    }

    #[test]
    fn lightconvert_normalization_preserves_scale16_and_rgb_key() {
        let mut profiles = LightProfileCache::new();
        let light =
            build_cell_light_from_raw(&mut profiles, [1050, 1050, 1010], 200, 1150, 1182, 2);

        assert_eq!(light.raw_rgb, [1050, 1050, 1010]);
        assert_eq!(light.raw_additive_intensity, 200);
        assert_eq!(light.scale16, 68812);
        assert_eq!(light.additive_intensity, 200);
        assert_eq!(light.rgb_key, [992, 992, 960]);
        assert_eq!(light.common_scalar, 1207);
        assert_eq!(light.bottom_scalar, 1241);
    }

    #[test]
    fn light_normalize_max_one_resets_to_neutral() {
        let normalized = normalize_light([1, 1, 1], 123);

        assert_eq!(normalized.scale16, LIGHT_SCALE16_IDENTITY);
        assert_eq!(normalized.common_scalar, 0);
        assert_eq!(normalized.rgb_key, [LIGHT_UNIT, LIGHT_UNIT, LIGHT_UNIT]);
    }

    #[test]
    fn light_normalize_max_two_does_not_reset_to_neutral() {
        let normalized = normalize_light([2, 1, 1], 100);

        assert_eq!(normalized.scale16, 131);
        assert_eq!(normalized.common_scalar, 0);
        assert_eq!(normalized.rgb_key, [992, 480, 480]);
    }

    #[test]
    fn light_normalize_negative_common_uses_arithmetic_shift() {
        let normalized = normalize_light([2, 1, 1], -1);

        assert_eq!(normalized.scale16, 131);
        assert_eq!(normalized.common_scalar, -1);
    }

    #[test]
    fn light_normalize_green_max_uses_scale_denominator_for_quantization() {
        let normalized = normalize_light([19, 22, 0], 0);

        assert_eq!(normalized.scale16, 1441);
        assert_eq!(normalized.rgb_key, [864, 992, 0]);
    }

    #[test]
    fn galite_source_fields_match_rulesmd_units() {
        let light = point_light_from_object(10, 10, 5000, 0.2, [0.05, 0.05, 0.01]).expect("light");

        assert_eq!(light.radius_leptons, 5000);
        assert_eq!(light.intensity, 200);
        assert_eq!(light.tint, [50, 50, 10]);
    }

    #[test]
    fn galite_point_light_contribution_separates_intensity_and_rgb() {
        let mut grid =
            build_cell_light_grid_from_heights([((10, 10), 0)], &LightingConfig::default());
        let light = point_light_from_object(10, 10, 5000, 0.2, [0.05, 0.05, 0.01]).expect("light");
        accumulate_point_lights(&mut grid, &[light]);

        let center = grid.cell_light_at((10, 10)).expect("light");
        assert_eq!(center.raw_additive_intensity, 200);
        assert_eq!(center.raw_rgb, [1050, 1050, 1010]);
        assert_eq!(center.common_scalar, 1207);
        assert_ne!(center.raw_rgb, [10, 10, 2]);
    }

    #[test]
    fn light_intensity_zero_allocates_no_static_source() {
        assert!(point_light_from_object(1, 2, 4096, 0.0, [1.0, 1.0, 1.0]).is_none());
    }

    #[test]
    fn test_collect_building_lights_uses_intensity_gate_and_default_radius() {
        let ini = IniFile::from_str(
            "[BuildingTypes]\n1=LAMP\n2=DARK\n3=ZERO\n\
             [LAMP]\nLightIntensity=0.5\n\
             [DARK]\nLightVisibility=2048\nLightIntensity=-0.25\n\
             [ZERO]\nLightVisibility=4096\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("rules");
        let entities = vec![
            test_map_structure("LAMP", 3, 4),
            test_map_structure("DARK", 5, 6),
            test_map_structure("ZERO", 7, 8),
        ];

        let lights = collect_building_lights(&entities, Some(&rules));
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].radius_leptons, 5000);
        assert_eq!(lights[0].intensity, 500);
        assert_eq!(lights[0].center_x, 3 * LEPTONS_PER_CELL + HALF_CELL_LEPTONS);
        assert_eq!(lights[1].radius_leptons, 2048);
        assert_eq!(lights[1].intensity, -249);
    }

    fn test_map_structure(type_id: &str, cell_x: u16, cell_y: u16) -> MapEntity {
        MapEntity {
            owner: "Neutral".to_string(),
            type_id: type_id.to_string(),
            health: 256,
            cell_x,
            cell_y,
            facing: 0,
            category: EntityCategory::Structure,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
        }
    }

    #[test]
    fn test_extra_light_is_not_rgb_cell_light() {
        let grid = test_grid(5, [0.4, 0.4, 0.4]);
        let key = (2u16, 2u16);
        // art.ini ExtraLight affects building draw brightness, not cell state.
        let result = grid.tint_or_default(key);
        assert_eq!(result, [0.4, 0.4, 0.4]);
    }

    #[test]
    fn deferred_refresh_commits_only_after_reverse_gather() {
        let config = LightingConfig::default();
        let heights = [((4, 4), 0), ((5, 4), 2), ((6, 4), 4)];
        let light = point_light_from_object(5, 4, 512, 0.2, [0.1, 0.0, 0.0]).expect("light");
        let immediate = {
            let mut grid = build_cell_light_grid_from_heights(heights, &config);
            accumulate_point_lights(&mut grid, &[light.clone()]);
            grid
        };

        let mut pending = DeferredCellLightRefresh::new(heights, &config, vec![light]);
        assert!(!pending.gather(1));
        assert!(
            pending.commit().is_none(),
            "partial gathers never become visible"
        );

        let mut pending = DeferredCellLightRefresh::new(heights, &config, vec![]);
        pending.gather_all();
        let no_lamp = pending.commit().expect("complete gather commits");
        assert_ne!(
            immediate
                .cell_light_at((5, 4))
                .map(|light| light.raw_additive_intensity),
            no_lamp
                .cell_light_at((5, 4))
                .map(|light| light.raw_additive_intensity),
        );

        let light = point_light_from_object(5, 4, 512, 0.2, [0.1, 0.0, 0.0]).expect("light");
        let mut pending = DeferredCellLightRefresh::new(heights, &config, vec![light]);
        assert!(pending.gather(heights.len()));
        let deferred = pending.commit().expect("complete gather commits");
        for cell in [(4, 4), (5, 4), (6, 4)] {
            let mut deferred_light = deferred
                .cell_light_at(cell)
                .cloned()
                .expect("deferred cell");
            let mut immediate_light = immediate
                .cell_light_at(cell)
                .cloned()
                .expect("immediate cell");
            // Profile IDs are local cache slots; equivalent RGB profiles can
            // be interned in a different order by full-grid and deferred paths.
            deferred_light.profile_id = LightProfileId(0);
            immediate_light.profile_id = LightProfileId(0);
            assert_eq!(deferred_light, immediate_light);
        }
    }

    #[test]
    fn point_light_area_walk_is_outer_y_inner_x_and_radius_gated() {
        let light = point_light_from_object(2, 2, 256, 0.2, [0.1, 0.0, 0.0]).expect("light");
        let cells = point_light_area_cells(&light, 5, 5, |_, _| Some(0));

        assert_eq!(
            cells,
            vec![
                ((2, 1), 0),
                ((1, 2), 0),
                ((2, 2), 0),
                ((3, 2), 0),
                ((2, 3), 0),
            ]
        );
    }

    #[test]
    fn deferred_source_area_commit_preserves_unaffected_cells() {
        let config = LightingConfig::default();
        let mut grid = build_cell_light_grid_from_heights([((1, 1), 0), ((9, 9), 4)], &config);
        let unaffected = grid.cell_light_at((9, 9)).cloned();
        let light = point_light_from_object(1, 1, 256, 0.2, [0.1, 0.0, 0.0]).expect("light");
        let mut pending = DeferredCellLightRefresh::new([((1, 1), 0)], &config, vec![light]);

        pending.gather_all();
        assert!(pending.commit_into(&mut grid));
        assert_eq!(grid.cell_light_at((9, 9)), unaffected.as_ref());
        assert_ne!(grid.cell_light_at((1, 1)), unaffected.as_ref());
    }

    #[test]
    fn retained_scalar_refresh_matches_original_cell_484680() {
        let native: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/light_retained_scalar.json"
        ))
        .unwrap();
        let cases = native["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 16);
        for case in cases {
            let record: Vec<i32> = serde_json::from_value(case["initial_record"].clone()).unwrap();
            let mut light = CellLight::new(
                LightProfileId(0),
                [record[12], record[13], record[14]],
                [record[0], record[1], record[2]],
                record[7],
                record[3],
                record[8],
                record[4],
                record[5],
                record[9],
                record[10],
                record[11],
            );
            let original = light.clone();
            let ion = case["ion"].as_bool().unwrap();
            let field = |name: &str| case[name].as_i64().unwrap() as i32;
            light.refresh_retained_scalars(
                LightingProfileUnits {
                    ambient_percent: field("ambient_percent"),
                    ground_units: field(if ion { "ion_ground" } else { "normal_ground" }),
                    level_units: field(if ion { "ion_level" } else { "normal_level" }),
                    red_percent: 100,
                    green_percent: 100,
                    blue_percent: 100,
                },
                field("level") as u8,
            );
            let expected: Vec<i32> = serde_json::from_value(case["fields"].clone()).unwrap();
            assert_eq!(
                [
                    light.scale16,
                    light.additive_intensity,
                    light.top_scalar,
                    light.common_scalar,
                    light.bottom_scalar
                ]
                .as_slice(),
                expected.as_slice(),
                "native case {case}",
            );
            assert_eq!(light.rgb_key, original.rgb_key);
            assert_eq!(light.raw_rgb, original.raw_rgb);
            assert_eq!(
                light.raw_additive_intensity,
                original.raw_additive_intensity
            );
            if field("finalization_index") == 7 {
                assert_eq!(original.common_scalar, 0);
                assert!(
                    light.common_scalar > 0,
                    "retained near-black state differs from resampling"
                );
            }
        }
    }
}
