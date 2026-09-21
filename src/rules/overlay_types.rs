//! Rules-owned overlay type registry — overlay ID to name, flags, and
//! tiberium/wall/crate semantics parsed from rules.ini `[OverlayTypes]` and
//! per-overlay sections (with art.ini image overrides).
//!
//! Declaration order is authoritative: `[OverlayTypes]` list position IS the
//! overlay ID, and every name/first-match lookup preserves it. Bridge-ID
//! geometry helpers stay in `map::overlay_types`; render-only SHP name
//! resolution lives in `render::overlay_assets`.
//!
//! ## Dependency rules
//! - Part of rules/ and depends only on rules siblings plus std.

use crate::rules::ini_parser::IniFile;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainRules};
use crate::rules::tiberium_type::{TiberiumType, TiberiumTypeId, TiberiumTypeRegistry};
use std::collections::HashSet;

#[cfg(test)]
const STOCK_FLAT_RIPARIUS_VARIANT_COUNT: usize = 12;
const TIBERIUM_FLAT_VARIANT_COUNT: usize = 12;
const NATIVE_TIBERIUM_PRIMARY_IMAGE_COUNT: usize = 12;
const NATIVE_TIBERIUM_EXTRA_IMAGE_COUNT: usize = 8;
const NATIVE_CRUENTUS_OVERLAY_BASE: usize = 27;
const NATIVE_RIPARIUS_OVERLAY_BASE: usize = 102;
const NATIVE_VINIFERA_OVERLAY_BASE: usize = 127;
const NATIVE_ABOREUS_OVERLAY_BASE: usize = 147;

/// Per-overlay-type vertical draw bias, in screen pixels.
///
/// Applied by the native overlay draw-offset helper to Tiberium, Wall and Crate
/// overlays; every other overlay type gets 0.
const OVERLAY_TYPE_Y_DRAW_BIAS_PX: f32 = -12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeTiberiumOverlayRange {
    base: usize,
    primary_count: usize,
    extra_count: usize,
}

impl NativeTiberiumOverlayRange {
    const fn new(base: usize, primary_count: usize, extra_count: usize) -> Self {
        Self {
            base,
            primary_count,
            extra_count,
        }
    }

    fn contains(self, overlay_id: usize) -> bool {
        let primary_end = self.base + self.primary_count;
        (self.base..primary_end).contains(&overlay_id)
            || (primary_end..primary_end + self.extra_count).contains(&overlay_id)
    }
}

fn native_tiberium_overlay_range(image: u8) -> NativeTiberiumOverlayRange {
    match image {
        2 => NativeTiberiumOverlayRange::new(
            NATIVE_CRUENTUS_OVERLAY_BASE,
            NATIVE_TIBERIUM_PRIMARY_IMAGE_COUNT,
            0,
        ),
        3 => NativeTiberiumOverlayRange::new(
            NATIVE_VINIFERA_OVERLAY_BASE,
            NATIVE_TIBERIUM_PRIMARY_IMAGE_COUNT,
            NATIVE_TIBERIUM_EXTRA_IMAGE_COUNT,
        ),
        4 => NativeTiberiumOverlayRange::new(
            NATIVE_ABOREUS_OVERLAY_BASE,
            NATIVE_TIBERIUM_PRIMARY_IMAGE_COUNT,
            NATIVE_TIBERIUM_EXTRA_IMAGE_COUNT,
        ),
        _ => NativeTiberiumOverlayRange::new(
            NATIVE_RIPARIUS_OVERLAY_BASE,
            NATIVE_TIBERIUM_PRIMARY_IMAGE_COUNT,
            NATIVE_TIBERIUM_EXTRA_IMAGE_COUNT,
        ),
    }
}

/// Check if an overlay index is a bridge overlay. The original engine identifies
/// bridges by hardcoded index position in `[OverlayTypes]`, not by INI flags.
/// These indices must not be reordered without breaking bridge logic.
pub fn is_bridge_overlay_index(id: u8) -> bool {
    matches!(
        id,
        24 | 25             // BRIDGE1, BRIDGE2 (high concrete)
        | 237 | 238         // BRIDGEB1, BRIDGEB2 (high wood)
        | 74..=101          // LOBRDG01-28 (low wood)
        | 122..=125         // LOBRDGE1-4 (low wood ends, TS)
        | 205..=232         // LOBRDB01-28 (low urban)
        | 233..=236         // LOBRDGB1-4 (low urban ends)
    )
}

/// Check if a bridge overlay index is a HIGH bridge (elevated, 3-cell-wide).
pub fn is_high_bridge_index(id: u8) -> bool {
    matches!(id, 24 | 25 | 237 | 238)
}

/// Per-overlay-type rendering flags parsed from each type's rules.ini section.
///
/// These flags select the correct palette and Y-offset for rendering.
#[derive(Debug, Clone)]
pub struct OverlayTypeFlags {
    /// Tiberium=yes — rendered with unit palette, gets -12px Y offset.
    pub tiberium: bool,
    /// ChainReaction=yes permits Apply_area_damage resource reduction.
    pub chain_reaction: bool,
    /// Wall=yes — rendered with unit palette, gets -12px Y offset.
    pub wall: bool,
    /// `CrushSound=` -> `OverlayTypeClass+0x1F0`: the Voc `UnitClass::PerCellProcess
    /// @ 0x0073B045..B04D` plays at the crusher's coordinates when it flattens
    /// the overlay. `None` when the key is absent (an invalid Voc is silence).
    pub crush_sound: Option<String>,
    /// `DrawFlat=` -> OverlayTypeClass+0x2B3. The ordinary non-wall
    /// `CellClass::DrawOverlay_Body @ 0x0047F6A0` selects gradient 0 when
    /// true and gradient 2 when false. Native walls force gradient 2.
    pub draw_flat: bool,
    /// Overlay `Armor=wood`, used by the Wood warhead wall-damage route.
    pub armor_is_wood: bool,
    /// IsVeins=yes — rendered with unit palette, gets -12px Y offset.
    pub is_veins: bool,
    /// IsVeinholeMonster=yes — rendered with unit palette.
    pub is_veinhole_monster: bool,
    /// Gate=yes — retained for gate-specific consumers; RecalcZoneType does not read it.
    pub is_gate: bool,
    /// Crushable=yes inherited from ObjectTypeClass; RecalcZoneType column 1.
    pub crushable: bool,
    /// Crate=yes — gets -12px Y offset.
    pub crate_type: bool,
    /// `CrateTrigger=` -> `OverlayTypeClass+0x2AB`, read beside `Crate=` at
    /// `OverlayTypeClass::Read_INI @ 0x005FE82E`. A set flag makes
    /// `CellClass__PickupCrate` spring trigger event `0x31` and latch
    /// `ScenarioClass+0x34BE`, which `LogicClass__PerTickUpdate` consumes and
    /// clears. Both stock crate overlays set it.
    pub crate_trigger: bool,
    /// `Overrides=yes` protects an existing overlay from ordinary runtime placement.
    pub overrides: bool,
    /// Parsed `CellAnim=` AnimType identity (`OverlayTypeClass+0x29C`).
    ///
    /// `OverlayTypeClass::ReadINI @ 0x005FE770` resolves this name through
    /// `AnimTypeClass::FindByName`; it is not the overlay's own SHP image.
    pub cell_anim: Option<String>,
    /// IsRubble=yes — explicitly returns reduced ZoneType 0 after earlier overlay checks.
    pub is_rubble: bool,
    /// IsARock=yes — reduced ZoneType 6.
    pub is_a_rock: bool,
    /// True when the overlay Land= section has Wheel speed exactly 0%.
    pub land_wheel_speed_zero: bool,
    /// Overlay name identifies a bridge deck/high-bridge overlay.
    pub bridge_deck: bool,
    /// `RadarColor=R,G,B` from the type's rules section.
    ///
    /// The engine prefers the growth stage's colour out of the overlay SHP and
    /// falls back to this when that comes back essentially black, so an overlay
    /// whose art is missing still paints the right colour on radar and previews.
    pub radar_color: Option<[u8; 3]>,
    /// Railroad track overlay (TRACKS01..TRACKS16). FA2 renders these +15px lower.
    pub track: bool,
    /// Canonical Land= value. Constructor default is Clear.
    pub land: LandType,
    /// Whether CellClass retains the overlay's land instead of restoring tile land.
    /// Constructor default is true and ReadINI uses the existing field as default.
    pub no_use_tile_land_type: bool,
    /// Final Land row's speed profile, when that canonical rules section exists.
    pub land_speed_costs: Option<SpeedCostProfile>,
    /// Whether the final Land row blocks ground movement on its own.
    ///
    /// Taken from the same `terrain_rules` row as `land_speed_costs`, because a
    /// cell whose LandType the overlay replaces must derive *every* land
    /// attribute from the replacement. `CellClass__RecalcAttributes` @
    /// `0x0047D2B0` stores exactly one land attribute — `Cell->LandType` — and
    /// its early overlay branch writes `OverlayTypeClass+0x298` (`Land=`) and
    /// returns before the tile's own subtile land type is ever read, whenever
    /// `Land` is Wall/Railroad or `+0x2AC` (`NoUseTileLandType`) is set. gamemd
    /// has no cached per-cell "blocked" bit to go stale; ground passability is
    /// re-derived from that stored LandType. VERA caches the derivation, so the
    /// cache has to move with the LandType that produced it.
    pub land_ground_blocked: bool,
    /// Whether the final `Land=` rules row rejects building placement.
    /// This is cached beside movement semantics for the same reason: native
    /// stores one LandType and derives both consumers from that live value.
    pub land_build_blocked: bool,
    /// Strength= from rules.ini — hit points for destructible overlays.
    /// Only meaningful when wall=true. Default 1.
    pub strength: u16,
    /// DamageLevels= from art.ini — number of damage stages for walls.
    /// Only meaningful when wall=true. Default 1.
    pub damage_levels: u16,
}

impl Default for OverlayTypeFlags {
    fn default() -> Self {
        Self {
            tiberium: false,
            chain_reaction: false,
            wall: false,
            crush_sound: None,
            draw_flat: true,
            armor_is_wood: false,
            is_veins: false,
            is_veinhole_monster: false,
            is_gate: false,
            crushable: false,
            crate_type: false,
            crate_trigger: false,
            overrides: false,
            cell_anim: None,
            is_rubble: false,
            is_a_rock: false,
            land_wheel_speed_zero: false,
            bridge_deck: false,
            track: false,
            land: LandType::Clear,
            no_use_tile_land_type: true,
            land_speed_costs: None,
            land_ground_blocked: false,
            land_build_blocked: false,
            radar_color: None,
            strength: 1,
            damage_levels: 1,
        }
    }
}

impl OverlayTypeFlags {
    /// Y pixel offset applied before the sprite is centred on its cell.
    ///
    /// The active YR overlay draw path biases Tiberium, Wall and Crate overlays
    /// by -12px and leaves everything else at 0. It is NOT the 15px half-cell
    /// height: the half-cell shift is already carried by the cell-centre term
    /// the caller adds, and both engines apply it identically, so this constant
    /// is the whole remaining vertical delta. `IsVeins=` is deliberately absent
    /// — it is not in the binary's predicate (and is TS-legacy dead data in YR).
    ///
    /// Not modelled: the `-1` the binary adds for `Land=Railroad` overlays and
    /// for one specific overlay slot. The railroad case is entangled with a
    /// separate FA2-sourced track offset in the instance builder; see the
    /// GSI-13.05 report.
    pub fn y_draw_offset(&self) -> f32 {
        if self.tiberium || self.wall || self.crate_type {
            OVERLAY_TYPE_Y_DRAW_BIAS_PX
        } else {
            0.0
        }
    }
}

/// Cell overlay-data byte written by the successful ordinary Mark tail.
///
/// `OverlayClass__Mark @ 0x005FC570` writes zero at `0x005FD0CC`, replaces it
/// with one for `Land=Road` at `0x005FD0E5`, then replaces either value with
/// `0xFF` when `OverlayType+0x2AA` (`Crate=yes`) at `0x005FD0FB..0x005FD105`.
pub(crate) fn native_mark_overlay_data(flags: &OverlayTypeFlags) -> u8 {
    if flags.crate_type {
        u8::MAX
    } else if flags.land == LandType::Road {
        1
    } else {
        0
    }
}

/// Registry of overlay type names indexed by overlay ID.
///
/// Built from the [OverlayTypes] section of rules.ini.
/// Overlay IDs 0..N map to the names listed in order.
#[derive(Clone)]
pub struct OverlayTypeRegistry {
    /// Overlay ID -> name (indexed by preserved internal [OverlayTypes] IDs).
    names: Vec<String>,
    /// Per-type rendering flags (same indexing as names).
    flags: Vec<OverlayTypeFlags>,
}

impl OverlayTypeRegistry {
    /// Parse [OverlayTypes] from rules.ini into an indexed registry.
    ///
    /// The section lists types with numeric keys: `0=GASAND\n1=GEM01\n...`.
    /// RA2/YR may skip some numeric keys, but internal overlay ids still follow
    /// the ordered list rather than reserving holes for every missing raw key.
    /// Returns an empty registry if the section is missing.
    pub fn from_ini(ini: &IniFile, art_ini: Option<&IniFile>) -> Self {
        let section = match ini.section("OverlayTypes") {
            Some(s) => s,
            None => {
                log::warn!("[OverlayTypes] section not found in rules.ini");
                return OverlayTypeRegistry {
                    names: Vec::new(),
                    flags: Vec::new(),
                };
            }
        };

        let names: Vec<String> = section
            .get_values()
            .into_iter()
            .map(str::to_string)
            .collect();
        if names.is_empty() {
            log::warn!("[OverlayTypes] present but empty");
            return OverlayTypeRegistry {
                names: Vec::new(),
                flags: Vec::new(),
            };
        }

        // Native registry allocation follows declaration order; numeric key
        // text is not a sorting authority.
        let terrain_rules = TerrainRules::from_ini(ini);
        let clear_semantics = terrain_rules.semantics_for_land_type(LandType::Clear.as_index());
        let clear_speed_costs = clear_semantics.map(|semantics| semantics.speed_costs);
        let clear_ground_blocked =
            clear_semantics.is_some_and(|semantics| semantics.ground_blocked);
        let clear_build_blocked = clear_semantics.is_some_and(|semantics| !semantics.buildable);
        // `OverlayTypeClass::ReadINI @ 0x005FE770` uses
        // `AnimTypeClass::FindByName`: an unknown CellAnim name leaves +0x29C
        // null rather than becoming an unresolved filename.
        let registered_anim_types: HashSet<String> = ini
            .section("Animations")
            .map(|section| {
                section
                    .get_values()
                    .into_iter()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_ascii_uppercase)
                    .collect()
            })
            .unwrap_or_default();
        let mut flags: Vec<OverlayTypeFlags> = Vec::with_capacity(names.len());
        for (idx, name) in names.iter().enumerate() {
            let upper_name = name.to_ascii_uppercase();
            // Bridge overlays are identified by hardcoded index position in
            // [OverlayTypes], matching the original engine's direct index checks.
            let bridge_deck = is_bridge_overlay_index(idx as u8);
            let track = upper_name.starts_with("TRACKS");
            if let Some(type_section) = ini.section(name) {
                let tiberium = type_section.get_bool("Tiberium").unwrap_or(false);
                let mut land = type_section
                    .get("Land")
                    .and_then(parse_land_type)
                    .unwrap_or(LandType::Clear);
                if tiberium && land == LandType::Clear {
                    land = LandType::Tiberium;
                }
                let land_semantics = terrain_rules.semantics_for_land_type(land.as_index());
                let land_speed_costs = land_semantics.map(|semantics| semantics.speed_costs);
                let land_ground_blocked =
                    land_semantics.is_some_and(|semantics| semantics.ground_blocked);
                let land_build_blocked =
                    land_semantics.is_some_and(|semantics| !semantics.buildable);
                let land_wheel_speed_zero =
                    land_speed_costs.is_some_and(|speed_costs| speed_costs.wheel == Some(0));
                // Strength from rules section (e.g., [GAWALL] Strength=300).
                let strength = type_section
                    .get("Strength")
                    .and_then(|v| v.parse::<u16>().ok())
                    .unwrap_or(1);
                let radar_color = type_section.get("RadarColor").and_then(parse_radar_color);
                // DamageLevels from art section (e.g., [GASAND] DamageLevels=2 in art.ini).
                let damage_levels = art_ini
                    .and_then(|art| art.section(name))
                    .and_then(|s| s.get("DamageLevels"))
                    .and_then(|v| v.parse::<u16>().ok())
                    .unwrap_or(1);
                flags.push(OverlayTypeFlags {
                    tiberium,
                    chain_reaction: type_section.get_bool("ChainReaction").unwrap_or(false),
                    wall: type_section.get_bool("Wall").unwrap_or(false),
                    crush_sound: type_section
                        .get("CrushSound")
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string),
                    draw_flat: type_section.get_bool("DrawFlat").unwrap_or(true),
                    armor_is_wood: type_section
                        .get("Armor")
                        .is_some_and(|armor| armor.eq_ignore_ascii_case("wood")),
                    is_veins: type_section.get_bool("IsVeins").unwrap_or(false),
                    is_veinhole_monster: type_section
                        .get_bool("IsVeinholeMonster")
                        .unwrap_or(false),
                    is_gate: type_section.get_bool("Gate").unwrap_or(false),
                    crushable: type_section.get_bool("Crushable").unwrap_or(false),
                    crate_type: type_section.get_bool("Crate").unwrap_or(false),
                    crate_trigger: type_section.get_bool("CrateTrigger").unwrap_or(false),
                    overrides: type_section.get_bool("Overrides").unwrap_or(false),
                    cell_anim: type_section
                        .get("CellAnim")
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_ascii_uppercase)
                        .filter(|value| registered_anim_types.contains(value)),
                    is_rubble: type_section.get_bool("IsRubble").unwrap_or(false),
                    is_a_rock: type_section.get_bool("IsARock").unwrap_or(false),
                    land_wheel_speed_zero,
                    bridge_deck,
                    track,
                    land,
                    no_use_tile_land_type: type_section
                        .get_bool("NoUseTileLandType")
                        .unwrap_or(true),
                    land_speed_costs,
                    land_ground_blocked,
                    land_build_blocked,
                    radar_color,
                    strength,
                    damage_levels,
                });
            } else {
                flags.push(OverlayTypeFlags {
                    bridge_deck,
                    track,
                    land_speed_costs: clear_speed_costs,
                    land_ground_blocked: clear_ground_blocked,
                    land_build_blocked: clear_build_blocked,
                    land_wheel_speed_zero: clear_speed_costs
                        .is_some_and(|speed_costs| speed_costs.wheel == Some(0)),
                    ..OverlayTypeFlags::default()
                });
            }
        }
        let max_index: usize = names.len().saturating_sub(1);

        log::info!(
            "OverlayTypeRegistry: {} types loaded (max_id={})",
            names.len(),
            max_index,
        );
        OverlayTypeRegistry { names, flags }
    }

    /// Create an empty registry (used as fallback when map loading fails).
    pub fn empty() -> Self {
        OverlayTypeRegistry {
            names: Vec::new(),
            flags: Vec::new(),
        }
    }

    /// Look up the name for an overlay ID. Returns None if out of range.
    pub fn name(&self, overlay_id: u8) -> Option<&str> {
        self.names
            .get(overlay_id as usize)
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
    }

    /// Look up the rendering flags for an overlay ID.
    pub fn flags(&self, overlay_id: u8) -> Option<&OverlayTypeFlags> {
        self.flags.get(overlay_id as usize)
    }

    /// Look up flags by overlay name (case-sensitive).
    pub fn flags_by_name(&self, name: &str) -> Option<&OverlayTypeFlags> {
        self.names
            .iter()
            .position(|n| n == name)
            .and_then(|i| self.flags.get(i))
    }

    /// Look up overlay_id by name (case-insensitive). Returns None if not found.
    pub fn id_for_name(&self, name: &str) -> Option<u8> {
        self.names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Stock flat Riparius overlay variants (`TIB01..TIB12`) in registry order.
    ///
    /// gamemd's no-overlay TIBTRE placement picks one of these 12 entries using
    /// the Riparius image index plus `RandomRanged(0, 0xB)`. The returned IDs
    /// come from the parsed registry positions and require `Tiberium=yes`, so no
    /// internal overlay IDs are baked into Rust.
    #[cfg(test)]
    pub fn stock_flat_riparius_variant_ids(&self) -> Option<[u8; 12]> {
        let mut ids: Vec<u8> = Vec::with_capacity(STOCK_FLAT_RIPARIUS_VARIANT_COUNT);
        let mut found = [false; STOCK_FLAT_RIPARIUS_VARIANT_COUNT];

        for (idx, name) in self.names.iter().enumerate() {
            let Some(variant) = stock_flat_riparius_variant_index(name) else {
                continue;
            };
            if found[variant] || !self.flags.get(idx).is_some_and(|flags| flags.tiberium) {
                return None;
            }
            found[variant] = true;
            ids.push(u8::try_from(idx).ok()?);
        }

        if !found.iter().all(|present| *present) {
            return None;
        }

        ids.try_into().ok()
    }

    /// Flat overlay variants for one parsed tiberium type image family.
    pub fn flat_tiberium_variant_ids(&self, ty: &TiberiumType) -> Option<[u8; 12]> {
        let mut ids: Vec<u8> = Vec::with_capacity(TIBERIUM_FLAT_VARIANT_COUNT);
        for variant in 1..=TIBERIUM_FLAT_VARIANT_COUNT {
            let name = tiberium_flat_overlay_name(ty.image, variant as u8)?;
            let id = self.id_for_name(&name)?;
            if !self.flags(id).is_some_and(|flags| flags.tiberium) {
                return None;
            }
            ids.push(id);
        }
        ids.try_into().ok()
    }

    /// Reproduce the native overlay flag gate and ordered tiberium range lookup.
    pub fn tiberium_type_for_overlay(
        &self,
        tiberium_types: &TiberiumTypeRegistry,
        overlay_id: u8,
    ) -> Option<TiberiumTypeId> {
        if !self.flags(overlay_id).is_some_and(|flags| flags.tiberium) {
            return None;
        }

        for ty in tiberium_types.types() {
            if native_tiberium_overlay_range(ty.image).contains(usize::from(overlay_id)) {
                return Some(ty.id);
            }
        }

        tiberium_types.types().first().map(|ty| ty.id)
    }

    /// Resolve the flat resource image selected for one rendered cell.
    ///
    /// The stored overlay identity remains authoritative for resource type and
    /// density. This returns only the display identity; callers keep the Cell
    /// overlay-data byte as the SHP frame.
    ///
    /// Active YR `CellClass__DrawOverlay_Body @ 0x0047F6A0` selects the parsed
    /// TiberiumClass from the stored overlay, then indexes that type's 12 flat
    /// images with `((short)rx * (short)ry) % 12`. Preserve the signed native
    /// intermediates: add the possibly-negative remainder to the signed family
    /// base ordinal before validating the final registered overlay identity.
    pub fn flat_tiberium_display_overlay_id(
        &self,
        tiberium_types: &TiberiumTypeRegistry,
        overlay_id: u8,
        rx: u16,
        ry: u16,
    ) -> Option<u8> {
        let ty = self
            .tiberium_type_for_overlay(tiberium_types, overlay_id)
            .and_then(|id| tiberium_types.get(id))?;
        let variants = self.flat_tiberium_variant_ids(ty)?;
        let signed_rx = i32::from(rx as i16);
        let signed_ry = i32::from(ry as i16);
        let variant = signed_rx.wrapping_mul(signed_ry) % TIBERIUM_FLAT_VARIANT_COUNT as i32;
        let display_ordinal = i32::from(variants[0]).wrapping_add(variant);
        let display_id = u8::try_from(display_ordinal).ok()?;
        self.name(display_id).map(|_| display_id)
    }

    /// Total number of registered overlay types.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
fn stock_flat_riparius_variant_index(name: &str) -> Option<usize> {
    let prefix = name.get(..3)?;
    if !prefix.eq_ignore_ascii_case("TIB") {
        return None;
    }
    let suffix = name.get(3..)?;
    if suffix.len() != 2 {
        return None;
    }
    let variant = suffix.parse::<usize>().ok()?;
    (1..=STOCK_FLAT_RIPARIUS_VARIANT_COUNT)
        .contains(&variant)
        .then_some(variant - 1)
}

fn tiberium_flat_overlay_name(image: u8, variant: u8) -> Option<String> {
    if variant == 0 || variant as usize > TIBERIUM_FLAT_VARIANT_COUNT {
        return None;
    }
    match image {
        1 => Some(format!("TIB{:02}", variant)),
        2 => Some(format!("GEM{:02}", variant)),
        image => image
            .checked_sub(1)
            .map(|suffix| format!("TIB{}_{:02}", suffix, variant)),
    }
}

fn parse_land_type(value: &str) -> Option<LandType> {
    LandType::ALL
        .into_iter()
        .find(|land_type| value.eq_ignore_ascii_case(land_type.section_name()))
}

/// Exact predicate for RecalcAttributes' early overlay-Land branch.
pub(crate) fn uses_early_recalc_land_branch(flags: &OverlayTypeFlags) -> bool {
    flags.land == LandType::Wall || flags.land == LandType::Railroad || flags.no_use_tile_land_type
}

/// Whether RecalcAttributes removes this resource overlay object/index/data.
pub(crate) fn clears_tiberium_on_slope(flags: &OverlayTypeFlags, slope_type: u8) -> bool {
    flags.tiberium && slope_type != 0 && (uses_early_recalc_land_branch(flags) || slope_type >= 5)
}

/// Cell land retained by the current RecalcAttributes invocation.
///
/// The early branch copies overlay Land before it removes a sloped resource,
/// so that invocation keeps the copied land even though the overlay pointer is
/// gone. A normal/nonclaiming resource removed on slope 5+ restores tile land.
pub(crate) fn retained_overlay_land(flags: &OverlayTypeFlags, slope_type: u8) -> Option<LandType> {
    let uses_early_branch = uses_early_recalc_land_branch(flags);
    if clears_tiberium_on_slope(flags, slope_type) && !uses_early_branch {
        return None;
    }
    (uses_early_branch || flags.tiberium).then_some(flags.land)
}

/// Parse a `RadarColor=R,G,B` value. Anything malformed yields `None` so the
/// caller falls back to the overlay's art rather than to a wrong colour.
fn parse_radar_color(value: &str) -> Option<[u8; 3]> {
    let mut channels = value.split(',').map(|part| part.trim().parse::<u8>());
    let red = channels.next()?.ok()?;
    let green = channels.next()?.ok()?;
    let blue = channels.next()?.ok()?;
    if channels.next().is_some() {
        return None;
    }
    Some([red, green, blue])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_overlay_types() {
        let text: &str = "\
[OverlayTypes]
0=GASAND
1=INTREE01
2=GAWALL
3=GEM01
";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);
        assert_eq!(reg.len(), 4);
        assert_eq!(reg.name(0), Some("GASAND"));
        assert_eq!(reg.name(1), Some("INTREE01"));
        assert_eq!(reg.name(3), Some("GEM01"));
        assert_eq!(reg.name(255), None);
    }

    #[test]
    fn test_sparse_overlay_types() {
        let text: &str = "\
[OverlayTypes]
0=GASAND
1=GEM01
5=BRIDGE
10=BIGFENCE
";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);
        // Keys sorted by numeric value, compacted to 0-based sequential indices.
        assert_eq!(reg.len(), 4);
        assert_eq!(reg.name(0), Some("GASAND"));
        assert_eq!(reg.name(1), Some("GEM01"));
        assert_eq!(reg.name(2), Some("BRIDGE"));
        assert_eq!(reg.name(3), Some("BIGFENCE"));
        assert_eq!(reg.name(4), None);
    }

    #[test]
    fn test_empty_registry() {
        let text: &str = "[General]\nKey=Value\n";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);
        assert!(reg.is_empty());
        assert_eq!(reg.name(0), None);
    }

    #[test]
    fn test_parse_reduced_zone_overlay_flags() {
        let text: &str = "\
[OverlayTypes]
0=SANDBAG
1=ROCKOVL
2=RUBBLE
[Clear]
Wheel=100%
[Rock]
Wheel=0%
[SANDBAG]
Crushable=yes
Wall=yes
Land=Clear
[ROCKOVL]
Land=Rock
IsARock=yes
[RUBBLE]
IsRubble=yes
";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);

        let sandbag = reg.flags(0).expect("sandbag flags");
        assert!(sandbag.crushable);
        assert!(sandbag.wall);
        assert!(!sandbag.land_wheel_speed_zero);

        let rock = reg.flags(1).expect("rock flags");
        assert!(rock.is_a_rock);
        assert!(rock.land_wheel_speed_zero);

        let rubble = reg.flags(2).expect("rubble flags");
        assert!(rubble.is_rubble);
    }

    #[test]
    fn gsi_04_04_overlay_land_defaults_and_exact_names() {
        let mut text = String::from("[OverlayTypes]\n0=MISSING_SECTION\n");
        for (index, _land) in LandType::ALL.into_iter().enumerate() {
            text.push_str(&format!("{}=LAND_{index}\n", index + 1));
        }
        text.push_str("13=LOWERCASE\n");
        for (index, land) in LandType::ALL.into_iter().enumerate() {
            text.push_str(&format!(
                "[LAND_{index}]\nLand={}\nNoUseTileLandType=no\n",
                land.section_name()
            ));
        }
        text.push_str("[LOWERCASE]\nLand=road\n");

        let ini = IniFile::from_str(&text);
        let reg = OverlayTypeRegistry::from_ini(&ini, None);

        let missing = reg.flags(0).expect("missing-section defaults");
        assert_eq!(missing.land, LandType::Clear);
        assert!(missing.no_use_tile_land_type);
        for (index, expected) in LandType::ALL.into_iter().enumerate() {
            let flags = reg.flags((index + 1) as u8).expect("canonical land flags");
            assert_eq!(flags.land, expected, "{}", expected.section_name());
            assert!(!flags.no_use_tile_land_type);
        }
        let lowercase = reg.flags(13).expect("lowercase land flags");
        assert_eq!(lowercase.land, LandType::Road);
        assert!(lowercase.no_use_tile_land_type);
    }

    #[test]
    fn gsi_04_04_tiberium_forces_land_before_final_wheel_lookup() {
        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=ORE
[Clear]
Wheel=100%
[Tiberium]
Wheel=0%
[ORE]
Tiberium=yes
Land=Clear
NoUseTileLandType=no
",
        );
        let reg = OverlayTypeRegistry::from_ini(&ini, None);
        let flags = reg.flags(0).expect("ore flags");

        assert!(flags.tiberium);
        assert_eq!(flags.land, LandType::Tiberium);
        assert!(!flags.no_use_tile_land_type);
        assert!(flags.land_wheel_speed_zero);
        assert_eq!(
            flags.land_speed_costs.expect("Tiberium speed row").wheel,
            Some(0)
        );
    }

    #[test]
    fn gsi_04_11_chain_reaction_ctor_default_is_false_and_ini_key_overrides() {
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=GREEN\n1=CHAIN\n2=BLUE\n\
             [GREEN]\nTiberium=yes\n\
             [CHAIN]\nTiberium=yes\nChainReaction=yes\n\
             [BLUE]\nTiberium=yes\nChainReaction=no\n",
        );
        let reg = OverlayTypeRegistry::from_ini(&ini, None);

        assert!(!reg.flags(0).unwrap().chain_reaction);
        assert!(reg.flags(1).unwrap().chain_reaction);
        assert!(!reg.flags(2).unwrap().chain_reaction);
    }

    #[test]
    fn gsi_04_04_overlay_land_retention_matches_recalc_attributes() {
        let mut ordinary = OverlayTypeFlags {
            land: LandType::Road,
            no_use_tile_land_type: false,
            ..OverlayTypeFlags::default()
        };
        assert_eq!(retained_overlay_land(&ordinary, 0), None);

        ordinary.no_use_tile_land_type = true;
        assert!(uses_early_recalc_land_branch(&ordinary));
        assert_eq!(retained_overlay_land(&ordinary, 0), Some(LandType::Road));

        for land in [LandType::Wall, LandType::Railroad] {
            let flags = OverlayTypeFlags {
                land,
                no_use_tile_land_type: false,
                ..OverlayTypeFlags::default()
            };
            assert!(uses_early_recalc_land_branch(&flags));
            assert_eq!(retained_overlay_land(&flags, 0), Some(land));
        }

        let resource = OverlayTypeFlags {
            tiberium: true,
            land: LandType::Tiberium,
            no_use_tile_land_type: false,
            ..OverlayTypeFlags::default()
        };
        assert_eq!(
            retained_overlay_land(&resource, 4),
            Some(LandType::Tiberium)
        );
        assert_eq!(
            retained_overlay_land(&resource, 1),
            Some(LandType::Tiberium)
        );
        assert_eq!(retained_overlay_land(&resource, 5), None);

        let stock_default = OverlayTypeFlags {
            tiberium: true,
            land: LandType::Tiberium,
            ..OverlayTypeFlags::default()
        };
        assert_eq!(
            [0, 1, 4, 5].map(|slope| retained_overlay_land(&stock_default, slope)),
            [Some(LandType::Tiberium); 4]
        );

        for land in [LandType::Wall, LandType::Railroad] {
            let authoritative = OverlayTypeFlags {
                tiberium: true,
                land,
                no_use_tile_land_type: false,
                ..OverlayTypeFlags::default()
            };
            assert_eq!(retained_overlay_land(&authoritative, 0), Some(land));
            assert_eq!(retained_overlay_land(&authoritative, 1), Some(land));
        }
    }

    #[test]
    fn test_one_based_overlay_types_compacted() {
        let text: &str = "\
[OverlayTypes]
1=GASAND
2=GEM01
";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);
        // Keys sorted and compacted to 0-based — key numbers are ordering hints only.
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.name(0), Some("GASAND"));
        assert_eq!(reg.name(1), Some("GEM01"));
    }

    #[test]
    fn test_sparse_keys_compacted() {
        let text: &str = "\
[OverlayTypes]
1=GASAND
2=GEM01
6=BRIDGE
11=BIGFENCE
";
        let ini: IniFile = IniFile::from_str(text);
        let reg: OverlayTypeRegistry = OverlayTypeRegistry::from_ini(&ini, None);
        assert_eq!(reg.len(), 4);
        assert_eq!(reg.name(0), Some("GASAND"));
        assert_eq!(reg.name(1), Some("GEM01"));
        assert_eq!(reg.name(2), Some("BRIDGE"));
        assert_eq!(reg.name(3), Some("BIGFENCE"));
    }

    #[test]
    fn stock_flat_riparius_variant_ids_use_registry_order() {
        let text = "\
[OverlayTypes]
1=GASAND
2=TIB01
3=TIB02
4=TIB03
5=TIB04
6=TIB05
7=TIB06
8=TIB07
9=TIB08
10=TIB09
11=TIB10
12=TIB11
13=TIB12
";
        let mut sections = String::from(text);
        for i in 1..=12 {
            sections.push_str(&format!("[TIB{:02}]\nTiberium=yes\n", i));
        }
        let ini = IniFile::from_str(&sections);
        let reg = OverlayTypeRegistry::from_ini(&ini, None);

        assert_eq!(
            reg.stock_flat_riparius_variant_ids(),
            Some([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
        );
    }

    #[test]
    fn stock_flat_riparius_variant_ids_require_tiberium_flag() {
        let mut text = String::from("[OverlayTypes]\n");
        for i in 1..=12 {
            text.push_str(&format!("{}=TIB{:02}\n", i, i));
        }
        for i in 1..=11 {
            text.push_str(&format!("[TIB{:02}]\nTiberium=yes\n", i));
        }
        text.push_str("[TIB12]\nTiberium=no\n");

        let ini = IniFile::from_str(&text);
        let reg = OverlayTypeRegistry::from_ini(&ini, None);

        assert_eq!(reg.stock_flat_riparius_variant_ids(), None);
    }

    fn stock_shaped_tiberium_ini(false_flag_name: Option<&str>) -> IniFile {
        let mut text = String::from(
            "\
[Tiberiums]
0=Riparius
1=Cruentus
2=Vinifera
3=Aboreus

[Riparius]
Image=1
[Cruentus]
Image=2
[Vinifera]
Image=3
[Aboreus]
Image=4

[OverlayTypes]
",
        );

        let mut names = Vec::new();
        for raw_key in (1..=170).filter(|key| *key != 40 && *key != 41) {
            let name = match raw_key {
                28..=39 => format!("GEM{:02}", raw_key - 27),
                105..=124 => format!("TIB{:02}", raw_key - 104),
                130..=149 => format!("TIB2_{:02}", raw_key - 129),
                150..=169 => format!("TIB3_{:02}", raw_key - 149),
                170 => "STRAY".to_owned(),
                _ => format!("FILL{raw_key:03}"),
            };
            text.push_str(&format!("{raw_key}={name}\n"));
            names.push(name);
        }
        for name in names {
            let enabled = false_flag_name != Some(name.as_str());
            text.push_str(&format!(
                "[{name}]\nTiberium={}\n",
                if enabled { "yes" } else { "no" }
            ));
        }

        IniFile::from_str(&text)
    }

    fn stock_shaped_tiberium_registries(
        false_flag_name: Option<&str>,
    ) -> (OverlayTypeRegistry, TiberiumTypeRegistry) {
        let ini = stock_shaped_tiberium_ini(false_flag_name);
        (
            OverlayTypeRegistry::from_ini(&ini, None),
            TiberiumTypeRegistry::from_ini(&ini),
        )
    }

    #[test]
    fn native_tiberium_overlay_range_covers_every_u8_selector() {
        let cruentus = NativeTiberiumOverlayRange::new(27, 12, 0);
        let riparius = NativeTiberiumOverlayRange::new(102, 12, 8);
        let vinifera = NativeTiberiumOverlayRange::new(127, 12, 8);
        let aboreus = NativeTiberiumOverlayRange::new(147, 12, 8);

        for image in u8::MIN..=u8::MAX {
            let expected = match image {
                2 => cruentus,
                3 => vinifera,
                4 => aboreus,
                _ => riparius,
            };
            assert_eq!(native_tiberium_overlay_range(image), expected);
        }
        for image in [0, 1, 5, 255] {
            assert_eq!(native_tiberium_overlay_range(image), riparius);
        }
    }

    #[test]
    fn overlay_to_tiberium_index_covers_stock_primary_and_extra_boundaries() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        let cases = [
            ("GEM01", 27, 1),
            ("GEM12", 38, 1),
            ("TIB01", 102, 0),
            ("TIB12", 113, 0),
            ("TIB13", 114, 0),
            ("TIB20", 121, 0),
            ("TIB2_01", 127, 2),
            ("TIB2_12", 138, 2),
            ("TIB2_13", 139, 2),
            ("TIB2_20", 146, 2),
            ("TIB3_01", 147, 3),
            ("TIB3_12", 158, 3),
            ("TIB3_13", 159, 3),
            ("TIB3_20", 166, 3),
        ];

        for (name, expected_overlay_id, expected_type_id) in cases {
            let overlay_id = overlays
                .id_for_name(name)
                .unwrap_or_else(|| panic!("{name} id"));
            assert_eq!(overlay_id, expected_overlay_id, "{name} compact id");
            assert_eq!(
                overlays.tiberium_type_for_overlay(&tiberiums, overlay_id),
                Some(TiberiumTypeId(expected_type_id)),
                "{name} type"
            );
        }

        let after_gem = overlays.id_for_name("FILL042").expect("flagged slot 39");
        assert_eq!(after_gem, 39);
        assert_eq!(
            overlays.tiberium_type_for_overlay(&tiberiums, after_gem),
            Some(TiberiumTypeId(0)),
            "Cruentus has no extra range; a flagged miss falls back to type zero"
        );
    }

    #[test]
    fn overlay_to_tiberium_index_uses_compact_slots_across_numeric_key_gaps() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        for (name, compact_id, type_id) in
            [("GEM01", 27, 1), ("TIB2_01", 127, 2), ("TIB3_01", 147, 3)]
        {
            let overlay_id = overlays
                .id_for_name(name)
                .unwrap_or_else(|| panic!("{name} id"));
            assert_eq!(overlay_id, compact_id);
            assert_eq!(
                overlays.tiberium_type_for_overlay(&tiberiums, overlay_id),
                Some(TiberiumTypeId(type_id))
            );
        }
    }

    #[test]
    fn overlay_to_tiberium_index_rejects_false_flag_before_range_lookup() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(Some("TIB2_01"));
        let overlay_id = overlays.id_for_name("TIB2_01").expect("TIB2_01 id");
        assert_eq!(
            overlays.tiberium_type_for_overlay(&tiberiums, overlay_id),
            None
        );
    }

    #[test]
    fn overlay_to_tiberium_index_flagged_range_miss_falls_back_to_type_zero() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        let stray_id = overlays.id_for_name("STRAY").expect("STRAY id");
        assert_eq!(stray_id, 167);
        assert_eq!(
            overlays.tiberium_type_for_overlay(&tiberiums, stray_id),
            Some(TiberiumTypeId(0))
        );

        let (unflagged, tiberiums) = stock_shaped_tiberium_registries(Some("STRAY"));
        let stray_id = unflagged.id_for_name("STRAY").expect("STRAY id");
        assert_eq!(
            unflagged.tiberium_type_for_overlay(&tiberiums, stray_id),
            None
        );
    }

    #[test]
    fn overlay_to_tiberium_index_overlapping_ranges_keep_first_tiberium_order() {
        let mut text = String::from(
            "\
[Tiberiums]
0=Nonmatching
1=FirstDefault
2=SecondDefault

[Nonmatching]
Image=2
[FirstDefault]
Image=1
[SecondDefault]
Image=5

[OverlayTypes]
",
        );
        for key in 0..=102 {
            let name = if key == 102 {
                "TARGET".to_owned()
            } else {
                format!("FILL{key:03}")
            };
            text.push_str(&format!("{key}={name}\n"));
        }
        text.push_str("[TARGET]\nTiberium=yes\n");
        let ini = IniFile::from_str(&text);
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberiums = TiberiumTypeRegistry::from_ini(&ini);
        let target = overlays.id_for_name("TARGET").expect("TARGET id");

        assert_eq!(target, 102);
        assert_eq!(
            overlays.tiberium_type_for_overlay(&tiberiums, target),
            Some(TiberiumTypeId(1))
        );
    }

    #[test]
    fn overlay_to_tiberium_index_handles_unknown_and_empty_registries() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        assert_eq!(
            overlays.tiberium_type_for_overlay(&tiberiums, u8::MAX),
            None
        );

        let empty_types = TiberiumTypeRegistry::from_ini(&IniFile::from_str(""));
        let stray_id = overlays.id_for_name("STRAY").expect("STRAY id");
        assert_eq!(
            overlays.tiberium_type_for_overlay(&empty_types, stray_id),
            None
        );
    }

    #[test]
    fn flat_tiberium_variants_remain_twelve_primary_images() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        let expected = [
            ("TIB01", "TIB12", "TIB13"),
            ("GEM01", "GEM12", "FILL042"),
            ("TIB2_01", "TIB2_12", "TIB2_13"),
            ("TIB3_01", "TIB3_12", "TIB3_13"),
        ];

        for (ty, (first, last, first_extra)) in tiberiums.types().iter().zip(expected) {
            let variants = overlays
                .flat_tiberium_variant_ids(ty)
                .unwrap_or_else(|| panic!("flat variants for {}", ty.section));
            assert_eq!(variants.len(), 12);
            assert_eq!(
                variants[0],
                overlays.id_for_name(first).expect("first primary")
            );
            assert_eq!(
                variants[11],
                overlays.id_for_name(last).expect("last primary")
            );
            assert!(
                !variants.contains(
                    &overlays
                        .id_for_name(first_extra)
                        .expect("first extra or post-family slot")
                )
            );
        }
    }

    #[test]
    fn gsi_13_05_flat_tiberium_display_uses_signed_cell_product_and_parsed_family() {
        let (overlays, tiberiums) = stock_shaped_tiberium_registries(None);
        let tib12 = overlays.id_for_name("TIB12").expect("TIB12");
        let tib01 = overlays.id_for_name("TIB01").expect("TIB01");
        let tib05 = overlays.id_for_name("TIB05").expect("TIB05");
        let gem12 = overlays.id_for_name("GEM12").expect("GEM12");
        let gem01 = overlays.id_for_name("GEM01").expect("GEM01");

        assert_eq!(
            overlays.flat_tiberium_display_overlay_id(&tiberiums, tib12, 4, 7),
            Some(tib05),
            "4 * 7 % 12 selects the fifth Riparius flat image"
        );
        assert_eq!(
            overlays.flat_tiberium_display_overlay_id(&tiberiums, gem12, 0, 7),
            Some(gem01),
            "the stored Cruentus identity selects the GEM flat-image family"
        );
        assert_eq!(
            overlays.flat_tiberium_display_overlay_id(&tiberiums, tib12, u16::MAX, 1),
            tib01.checked_sub(1),
            "(short)0xffff is -1, so the signed family-base addition selects base - 1"
        );
    }

    #[test]
    fn overlay_y_draw_bias_matches_the_native_predicate() {
        // The binary biases Tiberium / Wall / Crate overlays by -12 and nothing
        // else. -15 (half a cell height) is the caller's cell-centre term, not
        // this one, so folding it in here double-counts by 3px on every ore
        // cell, wall and crate on screen.
        for flags in [
            OverlayTypeFlags {
                tiberium: true,
                ..OverlayTypeFlags::default()
            },
            OverlayTypeFlags {
                wall: true,
                ..OverlayTypeFlags::default()
            },
            OverlayTypeFlags {
                crate_type: true,
                ..OverlayTypeFlags::default()
            },
        ] {
            assert_eq!(flags.y_draw_offset(), -12.0);
        }

        // Bridges, tracks, rocks and the rest sit flat on the cell.
        assert_eq!(OverlayTypeFlags::default().y_draw_offset(), 0.0);
        assert_eq!(
            OverlayTypeFlags {
                bridge_deck: true,
                ..OverlayTypeFlags::default()
            }
            .y_draw_offset(),
            0.0
        );
        // IsVeins= is not part of the native predicate.
        assert_eq!(
            OverlayTypeFlags {
                is_veins: true,
                ..OverlayTypeFlags::default()
            }
            .y_draw_offset(),
            0.0
        );
    }

    #[test]
    fn radar_color_parses_only_a_well_formed_triple() {
        assert_eq!(parse_radar_color("220,200,0"), Some([220, 200, 0]));
        assert_eq!(parse_radar_color(" 220 , 200 , 0 "), Some([220, 200, 0]));
        // Malformed values yield None so the caller falls back to the art
        // rather than painting a confidently wrong colour.
        assert_eq!(parse_radar_color("220,200"), None);
        assert_eq!(parse_radar_color("220,200,0,5"), None);
        assert_eq!(parse_radar_color("220,200,300"), None);
        assert_eq!(parse_radar_color(""), None);
    }
}
