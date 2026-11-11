//! Tiberium type definitions parsed from rules.ini.
//!
//! `[Tiberiums]` defines the native `TiberiumClass` order. Simulation code uses
//! this data as the bridge from overlay cells to per-type growth/spread state.

#[cfg(any(test, feature = "test-support"))]
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::native_processing::ProcessedRulesLayers;

/// `TiberiumClass::Constructor @ 0x007216C0` seeds both percentages with
/// `0x3FB999999999999A` (0.1) at `0x007216DC`/`0x007216E7`.
const CTOR_PERCENTAGE: f64 = 0.1;

/// Index into the parsed `[Tiberiums]` list: the `TiberiumClass+0x98`
/// `ArrayIndex` that `TiberiumClass::Constructor @ 0x007216C0` assigns from
/// the array count and `TiberiumClass::GetArrayIndex @ 0x00723700` (vtable
/// slot `+0x40`) returns.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TiberiumTypeId(pub u8);

/// Supply the numeric Overlay slots an explicitly authored fixture Image
/// reads, without moving or renaming any of the fixture's existing entries.
/// This is opt-in test input construction, never a production read fallback.
#[cfg(any(test, feature = "test-support"))]
pub fn with_tiberium_image_slots(ini: &IniFile) -> IniFile {
    let mut filled = ini.clone();
    let mut overlays = ini
        .section("OverlayTypes")
        .cloned()
        .unwrap_or_else(|| IniSection::new("OverlayTypes".into()));
    let mut names: Vec<String> = overlays
        .registry_ids()
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut suffix = 0;
    while names.len() < 148 {
        let candidate = format!("TESTIMAGE{suffix:03}");
        let key = format!("__test_image_{suffix}");
        suffix += 1;
        if names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&candidate))
            || overlays.keys().any(|existing| existing == key)
        {
            continue;
        }
        overlays.set(&key, &candidate);
        names.push(candidate);
    }
    filled.replace_first_section(overlays);
    filled
}

/// Retained `TiberiumClass+E0/E4/E8/EC`, rather than the last `Image=` text.
///
/// Constructor72173A..72174C clears all four fields. ReadINI721C3F..721CD6
/// binds an existing OverlayType slot and writes the counts; Image2 leaves
/// the slope count alone. Save7220D0/Load721E80 retain all four fields.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TiberiumImageState {
    base_overlay: Option<u32>,
    density_levels: i32,
    primary_count: i32,
    slope_count: i32,
}

impl TiberiumImageState {
    pub fn base_overlay(self) -> Option<u32> {
        self.base_overlay
    }

    pub fn density_levels(self) -> i32 {
        self.density_levels
    }

    pub fn primary_count(self) -> i32 {
        self.primary_count
    }

    pub fn slope_count(self) -> i32 {
        self.slope_count
    }

    /// ReadINI721C49 always passes -1, even on later physical rules layers.
    /// The selected pointer is captured now, without name or Tiberium-flag
    /// lookup. A missing array slot would read invalid memory natively; the
    /// rules failure boundary reports it without inventing a null binding.
    pub(crate) fn read_ini(
        &mut self,
        section: &IniSection,
        overlay_count: usize,
    ) -> Result<(), crate::rules::error::RulesError> {
        let image = section.read_int("Image", -1);
        let base: u32 = match image {
            -1 => return Ok(()),
            2 => 27,
            3 => 127,
            4 => 147,
            _ => 102,
        };
        if base as usize >= overlay_count {
            return Err(crate::rules::error::RulesError::InvalidValue {
                section: section.name.clone(),
                key: "Image".into(),
                expected: format!(
                    "existing OverlayTypes slot {base} (catalog has {overlay_count} slots)"
                ),
                value: image.to_string(),
            });
        }
        self.base_overlay = Some(base);
        self.density_levels = 12;
        self.primary_count = 12;
        if image != 2 {
            self.slope_count = 8;
        }
        Ok(())
    }

    /// What every runtime read of the type needs: a bound Image
    /// (`TiberiumClass+0xE0`), which `OverlayToTiberiumIndex @ 0x005FDD20`
    /// dereferences unchecked at `+0x294`, and the nonzero primary and
    /// density counts (`+0xE8`, `+0xE4`) gamemd divides by
    /// (`DrawOverlay 0x0047F860..0x0047F8FD`, `SpreadCellGerminate
    /// 0x004819CA`).
    fn usable(self) -> bool {
        self.base_overlay.is_some() && self.density_levels > 0 && self.primary_count > 0
    }

    /// PointerExpired722191..72219C clears E0 while leaving E4/E8/EC.
    pub(crate) fn clear_overlay(&mut self) {
        self.base_overlay = None;
    }

    pub(crate) fn valid_saved_state(self, overlay_count: usize) -> bool {
        self.base_overlay
            .is_none_or(|base| (base as usize) < overlay_count && self.density_levels == 12)
            && matches!(self.density_levels, 0 | 12)
            && self.primary_count == self.density_levels
            && matches!(self.slope_count, 0 | 8)
            && (self.density_levels != 0 || self.slope_count == 0)
    }
}

/// Per-`TiberiumClass` rules data needed by growth/spread and placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiberiumType {
    pub id: TiberiumTypeId,
    pub section: String,
    pub display_name: Option<String>,
    /// TiberiumClass::ReadINI @ 0x00721A50 (721B20), ReadColor474A90:
    /// retained runtime ColorScheme
    /// ordinal, not a name resolved against a later Colors catalog.
    color_scheme: i32,
    /// TiberiumRead721B40..721C1E's ordered, duplicate-preserving nonnull
    /// AnimType428B80 answers, used by AnimStart424DCC.
    debris: Vec<String>,
    image: TiberiumImageState,
    /// Set once by `TiberiumTypeRegistry::from_processed_rules` for an entry
    /// whose retained Image state no runtime read can use: the type keeps its
    /// `[Tiberiums]` position, matches no overlay and is never placed.
    inert: bool,
    /// Credit value per harvested density unit.
    pub value: i32,
    /// `Power=` (`TiberiumClass+0xBC`): constructor 0, ReadInt over the
    /// current value (`0x00721AE1` block). Only Ares reads it in play.
    pub power: i32,
    /// Signed `Growth=` timer reload input (`TiberiumClass+0xA8`).
    pub growth: i32,
    /// `GrowthPercentage=` as the native `TiberiumClass+0xB0` double (IEEE
    /// bits); the growth processor multiplies the heap count by it in x87
    /// arithmetic and compares it against 1e-05.
    pub growth_percentage_bits: u64,
    /// Signed `Spread=` timer reload value (`TiberiumClass+0x9C`).
    pub spread: i32,
    /// `SpreadPercentage=` as the native `TiberiumClass+0xA0` double.
    pub spread_percentage_bits: u64,
    /// Ares TiberiumExt (`rules::ares_tiberium`).
    #[cfg(feature = "ares")]
    ares: crate::rules::ares_tiberium::TiberiumExtRules,
}

/// Ordered tiberium type registry.
///
/// Its types live for the whole match, as gamemd's do: `TiberiumClass::
/// Destructor @ 0x00721880` runs only from the scalar deleting destructor
/// `0x00723710` (its vtable slot and the COM class factory), never from a
/// match, and `PointerExpired @ 0x00722140` only drops a dying AnimType from
/// `Debris` or a dying OverlayType from `Image`, type objects no match frees.
/// The processed type owner preserves these objects across Rules reset,
/// detaching the deleted Overlay/Anim bindings before the next rules pass.
#[derive(Debug, Clone, Default)]
pub struct TiberiumTypeRegistry {
    types: Vec<TiberiumType>,
}

impl TiberiumTypeRegistry {
    /// Fixture compatibility through the same ordered reader/factory owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini(ini: &IniFile) -> Self {
        let processed = crate::rules::native_processing::RulesLayerStack::new(ini.clone())
            .process()
            .expect("fixture Tiberium rules must process");
        Self::from_processed_rules(&processed)
    }

    pub(crate) fn from_processed_rules(processed: &ProcessedRulesLayers) -> Self {
        let mut types = Vec::new();
        for (index, (name, section, color, debris, image)) in
            processed.tiberium_read_states().enumerate()
        {
            let Some(id) = u8::try_from(index).ok().map(TiberiumTypeId) else {
                break;
            };
            let mut ty = TiberiumType::from_ini_section(id, name, section);
            ty.color_scheme = color;
            ty.debris = debris.to_vec();
            ty.image = image;
            // `TiberiumClass::ReadINI @ 0x00721C3F..0x00721C55` reads `Image=`
            // with default -1 and then leaves `+0xE0..+0xEC` at the
            // constructor's zeros (`0x0072173A..0x0072174C`). gamemd faults on
            // that null when the walk in `OverlayToTiberiumIndex @ 0x005FDD20`
            // reaches the entry (it returns at the first match); here the
            // entry stays in place (later indices do not move) and is inert.
            ty.inert = !image.usable();
            if ty.inert {
                log::warn!(
                    "[Tiberiums] entry {index} ({name}) has no usable Image=; \
                     it is kept in place but never matches or places an overlay"
                );
            }
            types.push(ty);
        }
        #[cfg(feature = "ares")]
        for (ty, ext) in types.iter_mut().zip(processed.tiberium_ext_states()) {
            ty.ares = ext.clone();
        }
        Self { types }
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn types(&self) -> &[TiberiumType] {
        &self.types
    }

    pub fn get(&self, id: TiberiumTypeId) -> Option<&TiberiumType> {
        self.types.get(id.0 as usize)
    }

    /// The type runtime code may read an image or a divisor from: `None` for
    /// an absent index and for an inert entry (see `TiberiumType::is_inert`).
    pub fn live(&self, id: TiberiumTypeId) -> Option<&TiberiumType> {
        self.get(id).filter(|ty| !ty.inert)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn id_by_name(&self, name: &str) -> Option<TiberiumTypeId> {
        self.types
            .iter()
            .find(|ty| ty.section.eq_ignore_ascii_case(name))
            .map(|ty| ty.id)
    }
}

impl TiberiumType {
    /// The retained native runtime scheme ordinal. Palette entries pair the
    /// shade1 and shade53 schemes, so their shared entry index is this / 2.
    pub fn color_scheme(&self) -> i32 {
        self.color_scheme
    }

    pub fn debris(&self) -> &[String] {
        &self.debris
    }

    pub fn image(&self) -> TiberiumImageState {
        self.image
    }

    /// True for an entry without a usable `Image=`: it holds its array index
    /// but no overlay belongs to it and nothing places it.
    pub fn is_inert(&self) -> bool {
        self.inert
    }

    /// The Ares TiberiumExt keys of this type.
    #[cfg(feature = "ares")]
    pub(crate) fn ares(&self) -> &crate::rules::ares_tiberium::TiberiumExtRules {
        &self.ares
    }

    /// `TiberiumClass::ReadINI @ 0x00721A50`: every read passes the current
    /// field, which starts at the constructor's value.
    fn from_ini_section(id: TiberiumTypeId, section_name: &str, section: &IniSection) -> Self {
        Self {
            id,
            section: section_name.to_string(),
            // AbstractTypeClass `Name=`, ReadString 0x31 (`0x00410AA0`).
            display_name: section.read_name("Name", 0x31).map(str::to_string),
            // Filled from the admitted, retained reader above.
            color_scheme: 0,
            debris: Vec::new(),
            image: TiberiumImageState::default(),
            inert: true,
            value: section.read_int("Value", 0),
            power: section.read_int("Power", 0),
            // ReadInt stores its signed result directly, without a clamp
            // (`0x00721AE1`); original execution: spatial_oracle/ore_queue.md.
            growth: section.read_int("Growth", 0),
            // ReadDouble -> `FSTP qword [ESI+0xB0]` (`0x00721AF4`).
            growth_percentage_bits: section
                .read_double("GrowthPercentage", CTOR_PERCENTAGE)
                .to_bits(),
            // The same raw signed store at `0x00721AA6`.
            spread: section.read_int("Spread", 0),
            // ReadDouble -> `FSTP qword [ESI+0xA0]` (`0x00721AB9`).
            spread_percentage_bits: section
                .read_double("SpreadPercentage", CTOR_PERCENTAGE)
                .to_bits(),
            #[cfg(feature = "ares")]
            ares: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Selected original constructor/ReadINI instructions and physical RULESMD
    // inputs; fixture bounds: ore_queue.md.
    #[test]
    fn timer_and_percentage_readers_match_original_constructor_and_read_ini() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/ore_queue.json",
        ))
        .unwrap();
        let rows = corpus["timer_reader_cases"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let retail = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini");
        for row in rows {
            let name = row["name"].as_str().unwrap();
            // Image is unrelated to the timer corpus. Leave its native cold
            // null state rather than referencing an absent Overlay catalog.
            let mut text = String::from("[Tiberiums]\n0=Riparius\n[Riparius]\n");
            for (field, key) in [
                ("growth", "Growth"),
                ("spread", "Spread"),
                ("growth_percentage", "GrowthPercentage"),
                ("spread_percentage", "SpreadPercentage"),
            ] {
                if let Some(value) = row["input"][field].as_str() {
                    text.push_str(&format!("{key}={value}\n"));
                }
            }
            let registry = TiberiumTypeRegistry::from_ini(&IniFile::from_str(&text));
            assert_native_timer_inputs(registry.get(TiberiumTypeId(0)).unwrap(), row, name);
            if let (Some(retail), Some(section)) = (&retail, row["physical"]["section"].as_str()) {
                let registry = TiberiumTypeRegistry::from_ini(retail);
                let ty = registry.get(registry.id_by_name(section).unwrap()).unwrap();
                assert_native_timer_inputs(ty, row, name);
            }
        }
    }

    fn assert_native_timer_inputs(ty: &TiberiumType, row: &serde_json::Value, name: &str) {
        for (field, actual) in [("growth", ty.growth), ("spread", ty.spread)] {
            assert_eq!(
                i64::from(actual),
                row["output"][field].as_i64().unwrap(),
                "{name}: {field}"
            );
        }
        for (field, actual) in [
            ("growth_percentage_bits", ty.growth_percentage_bits),
            ("spread_percentage_bits", ty.spread_percentage_bits),
        ] {
            let expected = u64::from_str_radix(row["output"][field].as_str().unwrap(), 16).unwrap();
            assert_eq!(actual, expected, "{name}: {field}");
        }
    }

    #[test]
    fn parses_tiberiums_order_and_per_type_growth_spread_fields() {
        let mut text = String::from(
            "\
[Colors]
NeonGreen=104,241,195

[Tiberiums]
0=Riparius
1=Cruentus
2=Vinifera
3=Aboreus

[Riparius]
Name=Tiberium Riparius
Color=NeonGreen
Image=1
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[Cruentus]
Name=Tiberium Cruentus
Image=2
Value=50
Growth=10000
GrowthPercentage=0
Spread=10000
SpreadPercentage=0

[Vinifera]
Image=3
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[Aboreus]
Image=4
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06
",
        );

        // Physical slots, independent of these fixture names, must exist
        // when the four native Image reads bind their OverlayType pointers.
        text.push_str("\n[OverlayTypes]\n");
        for slot in 0..148 {
            text.push_str(&format!("{slot}=FILL{slot}\n"));
        }
        let registry = TiberiumTypeRegistry::from_ini(&IniFile::from_str(&text));

        assert_eq!(registry.len(), 4);
        let riparius = registry.get(TiberiumTypeId(0)).expect("Riparius");
        assert_eq!(riparius.section, "Riparius");
        assert_eq!(riparius.color_scheme(), 1);
        assert_eq!(riparius.image().base_overlay(), Some(102));
        assert_eq!(riparius.value, 25);
        assert_eq!(riparius.growth, 2200);
        // ReadDouble scans a float: `.06` is `0.06f32` widened.
        let widened = f64::from(0.06_f32).to_bits();
        assert_eq!(riparius.growth_percentage_bits, widened);
        assert_eq!(riparius.spread, 2200);
        assert_eq!(riparius.spread_percentage_bits, widened);
        assert_eq!(riparius.image().density_levels(), 12);

        let cruentus = registry
            .get(registry.id_by_name("cruentus").expect("Cruentus id"))
            .expect("Cruentus");
        assert_eq!(cruentus.image().base_overlay(), Some(27));
        assert_eq!(cruentus.image().slope_count(), 0);
        assert_eq!(cruentus.value, 50);
        assert_eq!(cruentus.growth, 10000);
        assert_eq!(cruentus.growth_percentage_bits, 0.0_f64.to_bits());
        assert_eq!(cruentus.spread, 10000);
        assert_eq!(cruentus.spread_percentage_bits, 0.0_f64.to_bits());
    }

    #[test]
    fn image_reader_retains_pointer_and_counts_across_physical_layers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

        let Some(root) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let mut layers = RulesLayerStack::new(root.clone());
        let cold = TiberiumTypeRegistry::from_processed_rules(&layers.process().unwrap());
        assert_eq!(
            cold.get(TiberiumTypeId(0)).unwrap().image().base_overlay(),
            Some(102)
        );
        assert_eq!(
            cold.get(TiberiumTypeId(1)).unwrap().image().slope_count(),
            0
        );
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=2\n"),
        );
        // A body without this physical pass's list is not read at all.
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Riparius]\nImage=3\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=-1\n"),
        );
        layers.push(
            RulesLayerKind::Tmcj4f,
            IniFile::from_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=$ZZ\n"),
        );
        let retained = TiberiumTypeRegistry::from_processed_rules(&layers.process().unwrap());
        let image = retained.get(TiberiumTypeId(0)).unwrap().image();
        assert_eq!(image.base_overlay(), Some(27));
        assert_eq!(
            (
                image.density_levels(),
                image.primary_count(),
                image.slope_count()
            ),
            (12, 12, 8)
        );

        // Identical incoming text can still have different retained EC.
        // The process owner must hash that state as well as the source stack.
        let (_, trace) = layers
            .process()
            .unwrap()
            .into_ini_and_native_type_construction_trace();
        let mut image2_root = root;
        image2_root.merge(&IniFile::from_str("[Riparius]\nImage=2\n"));
        let same_source = RulesLayerStack::new(image2_root);
        let fresh =
            crate::rules::ruleset::RuleSet::from_processed_rules(&same_source.process().unwrap())
                .unwrap();
        let continued = same_source
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let continued = crate::rules::ruleset::RuleSet::from_processed_rules(&continued).unwrap();
        assert_eq!(fresh.source_ini_hash(), continued.source_ini_hash());
        assert_eq!(
            fresh
                .tiberium_types
                .get(TiberiumTypeId(0))
                .unwrap()
                .image()
                .slope_count(),
            0
        );
        assert_eq!(
            continued
                .tiberium_types
                .get(TiberiumTypeId(0))
                .unwrap()
                .image()
                .slope_count(),
            8
        );
        assert_ne!(
            fresh.simulation_config_hash(),
            continued.simulation_config_hash()
        );

        // A nonnumeric decimal is atoi(0), unlike the failed hex above.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=oops\n"),
        );
        let decimal = TiberiumTypeRegistry::from_processed_rules(&layers.process().unwrap());
        assert_eq!(
            decimal
                .get(TiberiumTypeId(0))
                .unwrap()
                .image()
                .base_overlay(),
            Some(102)
        );
        let missing_catalog = RulesLayerStack::new(IniFile::from_str(
            "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n",
        ))
        .process();
        assert!(
            matches!(missing_catalog, Err(crate::rules::error::RulesError::InvalidValue { key, .. }) if key == "Image")
        );
    }

    #[test]
    fn scenario_reset_and_save_keep_tiberium_objects_but_detach_image() {
        use crate::rules::native_processing::{
            ProcessedRulesLayers, RulesLayerKind, RulesLayerStack, RulesProcessContext,
        };
        use crate::rules::object_image::ImageTheater;
        use crate::rules::ruleset::RuleSet;
        use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};

        let Some(root) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let mut layers = RulesLayerStack::new(root);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=3\nDebris=FIRE01\n"),
        );
        let (_, trace) = layers
            .process()
            .unwrap()
            .into_ini_and_native_type_construction_trace();
        let reset = RulesLayerStack::new(IniFile::from_str(
            "[Tiberiums]\n0=DIFFERENT\n[Riparius]\nImage=-1\n",
        ))
        .process_with_fixed_art_and_registry_state_recovering(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
            RulesProcessContext::ScenarioReset {
                theater: ImageTheater::Temperate,
                skip_root_color_add_and_mode: false,
            },
            &EvaRegistry::default(),
            &SoundRegistry::default(),
        )
        .unwrap();
        let rules = RuleSet::from_processed_rules(&reset).unwrap();
        let ty = rules.tiberium_types.get(TiberiumTypeId(0)).unwrap();
        assert_eq!(ty.section, "Riparius");
        assert_eq!(ty.value, 25);
        assert_eq!(ty.image().base_overlay(), None);
        assert_eq!(
            (
                ty.image().density_levels(),
                ty.image().primary_count(),
                ty.image().slope_count()
            ),
            (12, 12, 8)
        );
        assert!(ty.debris().is_empty());
        let bytes = crate::util::save_codec::serialize(&reset).unwrap();
        let restored: ProcessedRulesLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
        let restored_rules = RuleSet::from_processed_rules(&restored).unwrap();
        assert_eq!(
            restored_rules
                .tiberium_types
                .get(TiberiumTypeId(0))
                .unwrap(),
            ty
        );
        assert_eq!(
            restored_rules.simulation_config_hash(),
            rules.simulation_config_hash()
        );

        let mut new_root = String::from("[OverlayTypes]\n");
        for slot in 0..128 {
            new_root.push_str(&format!("{slot}=NEW{slot}\n"));
        }
        new_root.push_str("[Tiberiums]\n0=IGNORED\n[Riparius]\nImage=3\n");
        let (_, trace) = restored.into_ini_and_native_type_construction_trace();
        let rebound = RulesLayerStack::new(IniFile::from_str(&new_root))
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rebound = TiberiumTypeRegistry::from_processed_rules(&rebound);
        assert_eq!(
            rebound
                .get(TiberiumTypeId(0))
                .unwrap()
                .image()
                .base_overlay(),
            Some(127)
        );
        assert_eq!(
            rebound
                .get(TiberiumTypeId(1))
                .unwrap()
                .image()
                .base_overlay(),
            None
        );
    }
}
