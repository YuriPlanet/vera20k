//! Phobos custom radiation types, retained by the Rules registry.
//!
//! Native identity: Phobos `RadTypeClass` (`src/New/Type/RadTypeClass.{h,cpp}`),
//! an `Enumerable<RadTypeClass>` whose main section is `RadiationTypes` and
//! whose `AddDefaults` seeds the `Radiation` entry (`GameStrings::Radiation`).
//! `RadTypeClass::LoadFromINI` reads the type's own section with `Nullable`
//! fields, so an absent key falls back to the global value: the vanilla
//! `RulesClass` `Rad*` fields for the original keys, and
//! `RulesExt::Global()`'s `[Radiation]` keys for `UseGlobalRadApplicationDelay`,
//! `RadApplicationDelay.Building`, `RadBuildingDamageMaxCount`,
//! `RadSiteWarhead.Detonate`, `RadSiteWarhead.Detonate.Full`, `RadHasOwner`
//! and `RadHasInvoker` (defaults true/0/-1/false/true/false/false).
//! `WeaponTypeExt::RadType` (`RadType=`, default the `Radiation` type,
//! `src/Ext/WeaponType/Body.cpp`) binds a weapon's field;
//! `RadSiteExt::CreateInstance` stores the bound `Type` and clamps the site
//! level with `min(radLevel, GetLevelMax())`; `BulletClass::ApplyRadiation
//! @ 0x46ADE0` merges only a site with matching spread and type; and the Foot
//! AI loop (`0x004DA554`) plus the building loop group each cell's sites by
//! type, cap the summed level at the type's `RadLevelMax` and apply the
//! type's `RadLevelFactor`, `RadSiteWarhead` and `RadApplicationDelay`.
//!
//! This module owns catalog reads and the effective-value projection. The
//! retained `NativeRulesRegistryState` owns the catalog's lifetime and the
//! reached passes; `sim::radiation` owns site and damage consumption. The
//! per-type `RadSiteWarhead.Detonate(.Full)`, `RadHasOwner` and `RadHasInvoker`
//! keys are read and retained here but not consumed: warhead detonation from
//! a radiation site and radiation damage attribution are separate chains.

#[cfg(feature = "phobos")]
use crate::rules::ini_parser::IniFile;
#[cfg(feature = "phobos")]
use crate::rules::ini_value::truncate_native_bytes;
use crate::rules::ruleset::RadiationRules;
use crate::util::fixed_math::SimFixed;

/// Phobos `GameStrings::Radiation`: the default type every weapon binds and
/// the section the vanilla `[Radiation]` keys also name.
pub(crate) const DEFAULT_RADIATION_TYPE: &str = "Radiation";

/// `Enumerable` stores `Name[32]` with `_TRUNCATE`, so the payload is 31
/// bytes (`RadTypeClass` shares `Utilities/TemplateDef.h`'s readers).
#[cfg(feature = "phobos")]
const NAME_PAYLOAD_BYTES: usize = 0x1F;

/// One `[RadiationTypes]` entry. Every field is `Nullable` in native terms:
/// `None` falls back to the global value at effective resolution, so a later
/// Rules pass that omits a key retains the earlier pass's read.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct RadiationType {
    name: String,
    duration_multiple: Option<i32>,
    application_delay: Option<i32>,
    application_delay_building: Option<i32>,
    building_damage_max_count: Option<i32>,
    level_max: Option<i32>,
    level_delay: Option<i32>,
    light_delay: Option<i32>,
    level_factor: Option<f64>,
    light_factor: Option<f64>,
    tint_factor: Option<f64>,
    color: Option<(u8, u8, u8)>,
    site_warhead: Option<String>,
    site_warhead_detonate: Option<bool>,
    site_warhead_detonate_full: Option<bool>,
    has_owner: Option<bool>,
    has_invoker: Option<bool>,
}

impl RadiationType {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Float fields hash by bits so equal entries hash equal, as the retained
/// registry's config hash requires.
impl std::hash::Hash for RadiationType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.duration_multiple.hash(state);
        self.application_delay.hash(state);
        self.application_delay_building.hash(state);
        self.building_damage_max_count.hash(state);
        self.level_max.hash(state);
        self.level_delay.hash(state);
        self.light_delay.hash(state);
        self.level_factor.map(f64::to_bits).hash(state);
        self.light_factor.map(f64::to_bits).hash(state);
        self.tint_factor.map(f64::to_bits).hash(state);
        self.color.hash(state);
        self.site_warhead.hash(state);
        self.site_warhead_detonate.hash(state);
        self.site_warhead_detonate_full.hash(state);
        self.has_owner.hash(state);
        self.has_invoker.hash(state);
    }
}

/// One radiation type's effective values: the type's own reads over the
/// globals, exactly as `RadTypeClass`'s `Get*` accessors resolve them.
/// `site_warhead` borrows the owning rules so hot paths allocate nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectiveRadiation<'a> {
    pub(crate) duration_multiple: i32,
    /// 0 disables damage to feet when `UseGlobalRadApplicationDelay` is off
    /// (native `delay == 0` skips the site); the global clamp to >= 1 keeps
    /// the shared modulo well-defined.
    pub(crate) application_delay: i32,
    pub(crate) application_delay_building: i32,
    pub(crate) building_damage_max_count: i32,
    pub(crate) level_max: i32,
    pub(crate) level_delay: i32,
    pub(crate) light_delay: i32,
    pub(crate) level_factor: f64,
    pub(crate) light_factor: SimFixed,
    pub(crate) tint_factor: SimFixed,
    pub(crate) color: (u8, u8, u8),
    /// Empty when no warhead is bound: the damage path skips the type
    /// entirely, as native `!GetWarhead()` does.
    pub(crate) site_warhead: &'a str,
    pub(crate) site_warhead_detonate: bool,
    pub(crate) site_warhead_detonate_full: bool,
    pub(crate) has_owner: bool,
    pub(crate) has_invoker: bool,
}

impl EffectiveRadiation<'_> {
    /// Every value from the globals: what a build without the Phobos
    /// capability, or a catalog without the bound entry, resolves to.
    pub(crate) fn from_globals(rules: &RadiationRules) -> EffectiveRadiation<'_> {
        EffectiveRadiation {
            duration_multiple: rules.duration_multiple,
            application_delay: rules.application_delay,
            #[cfg(feature = "phobos")]
            application_delay_building: rules.application_delay_building,
            #[cfg(not(feature = "phobos"))]
            application_delay_building: 0,
            #[cfg(feature = "phobos")]
            building_damage_max_count: rules.building_damage_max_count,
            #[cfg(not(feature = "phobos"))]
            building_damage_max_count: -1,
            level_max: rules.level_max,
            level_delay: rules.level_delay,
            light_delay: rules.light_delay,
            level_factor: rules.level_factor,
            light_factor: rules.light_factor,
            tint_factor: rules.tint_factor,
            color: rules.color,
            site_warhead: &rules.site_warhead,
            #[cfg(feature = "phobos")]
            site_warhead_detonate: rules.site_warhead_detonate,
            #[cfg(not(feature = "phobos"))]
            site_warhead_detonate: false,
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: rules.site_warhead_detonate_full,
            #[cfg(not(feature = "phobos"))]
            site_warhead_detonate_full: true,
            #[cfg(feature = "phobos")]
            has_owner: rules.has_owner,
            #[cfg(not(feature = "phobos"))]
            has_owner: false,
            #[cfg(feature = "phobos")]
            has_invoker: rules.has_invoker,
            #[cfg(not(feature = "phobos"))]
            has_invoker: false,
        }
    }
}

/// Ordered Phobos `Enumerable<RadTypeClass>::Array`. The retained Rules
/// registry owns this value; `RuleSet` borrows its projection and the
/// simulation resolves sites against it.
#[derive(Debug, Clone, PartialEq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct RadiationTypeCatalog {
    types: Vec<RadiationType>,
}

impl RadiationTypeCatalog {
    /// `AddDefaults` seeds the `Radiation` entry before any INI read.
    pub(crate) fn with_default() -> Self {
        Self {
            types: vec![RadiationType::named(DEFAULT_RADIATION_TYPE)],
        }
    }

    #[cfg(all(test, feature = "phobos"))]
    pub(crate) fn entries(&self) -> &[RadiationType] {
        &self.types
    }

    /// Every retained entry's canonical name, in allocation order: the
    /// default `Radiation` entry first, then each pass's `[RadiationTypes]`
    /// append. The load-time report names them so a mod's catalog can be
    /// traced without deploying anything.
    #[cfg(feature = "phobos")]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|entry| entry.name.as_str())
    }

    /// Native case-insensitive lookup retaining the first spelling.
    #[cfg(feature = "phobos")]
    pub(crate) fn entry(&self, name: &str) -> Option<&RadiationType> {
        self.types
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// The entry a weapon binds: its `RadType=` name, or the default
    /// `Radiation` entry when the name is absent or unknown
    /// (`Valueable<RadTypeClass*>::Read<true>` keeps the constructor's
    /// `FindOrAllocate(GameStrings::Radiation)`).
    #[cfg(feature = "phobos")]
    pub(crate) fn bound_entry(&self, name: Option<&str>) -> Option<&RadiationType> {
        match name {
            Some(name) => self
                .entry(name)
                .or_else(|| self.entry(DEFAULT_RADIATION_TYPE)),
            None => self.entry(DEFAULT_RADIATION_TYPE),
        }
    }

    /// Effective values for a bound type name. An empty catalog, or a bound
    /// name the catalog does not hold, resolves entirely from the globals.
    #[cfg(feature = "phobos")]
    pub(crate) fn effective<'a>(
        &'a self,
        rules: &'a RadiationRules,
        name: Option<&str>,
    ) -> EffectiveRadiation<'a> {
        let Some(entry) = self.bound_entry(name) else {
            return EffectiveRadiation::from_globals(rules);
        };
        EffectiveRadiation {
            duration_multiple: entry.duration_multiple.unwrap_or(rules.duration_multiple),
            application_delay: entry.application_delay.unwrap_or(rules.application_delay),
            #[cfg(feature = "phobos")]
            application_delay_building: entry
                .application_delay_building
                .unwrap_or(rules.application_delay_building),
            #[cfg(not(feature = "phobos"))]
            application_delay_building: entry.application_delay_building.unwrap_or(0),
            #[cfg(feature = "phobos")]
            building_damage_max_count: entry
                .building_damage_max_count
                .unwrap_or(rules.building_damage_max_count),
            #[cfg(not(feature = "phobos"))]
            building_damage_max_count: entry.building_damage_max_count.unwrap_or(-1),
            level_max: entry.level_max.unwrap_or(rules.level_max),
            level_delay: entry.level_delay.unwrap_or(rules.level_delay),
            light_delay: entry.light_delay.unwrap_or(rules.light_delay),
            level_factor: entry.level_factor.unwrap_or(rules.level_factor),
            // The global readers widen the scanned double through f32 into
            // the stored fixed value; a type read takes the same path.
            light_factor: SimFixed::from_num(
                entry
                    .light_factor
                    .unwrap_or(rules.light_factor.to_num::<f64>()) as f32,
            ),
            tint_factor: SimFixed::from_num(
                entry
                    .tint_factor
                    .unwrap_or(rules.tint_factor.to_num::<f64>()) as f32,
            ),
            color: entry.color.unwrap_or(rules.color),
            site_warhead: entry.site_warhead.as_deref().unwrap_or(&rules.site_warhead),
            #[cfg(feature = "phobos")]
            site_warhead_detonate: entry
                .site_warhead_detonate
                .unwrap_or(rules.site_warhead_detonate),
            #[cfg(not(feature = "phobos"))]
            site_warhead_detonate: entry.site_warhead_detonate.unwrap_or(false),
            #[cfg(feature = "phobos")]
            site_warhead_detonate_full: entry
                .site_warhead_detonate_full
                .unwrap_or(rules.site_warhead_detonate_full),
            #[cfg(not(feature = "phobos"))]
            site_warhead_detonate_full: entry.site_warhead_detonate_full.unwrap_or(true),
            #[cfg(feature = "phobos")]
            has_owner: entry.has_owner.unwrap_or(rules.has_owner),
            #[cfg(not(feature = "phobos"))]
            has_owner: entry.has_owner.unwrap_or(false),
            #[cfg(feature = "phobos")]
            has_invoker: entry.has_invoker.unwrap_or(rules.has_invoker),
            #[cfg(not(feature = "phobos"))]
            has_invoker: entry.has_invoker.unwrap_or(false),
        }
    }

    /// Read one reached Rules pass. `LoadFromINIList` allocates the pass's
    /// `[RadiationTypes]` list and then calls every retained type's
    /// `LoadFromINI` on the same pass, so the per-type section reads run
    /// whether or not this pass carries the list. `Nullable` reads keep
    /// prior values when the key is absent, so a nonempty read replaces and
    /// an omitted one retains.
    #[cfg(feature = "phobos")]
    pub(crate) fn apply_pass(&mut self, pass: &IniFile) {
        if let Some(list) = pass.section("RadiationTypes") {
            // Phobos Enumerable::LoadFromINIList reads each physical key
            // through ReadString with Phobos::readBuffer[2048] before lookup.
            // Source: Phobos b90ab4bc, src/Utilities/Enumerable.h:49-60,
            // src/Phobos.h:23-24; CCINIClass::ReadString @0x00528A10.
            for key in list.physical_keys() {
                let Some(name) = list.read_name(key, 2048) else {
                    continue;
                };
                // FindOrAllocate @ the shared Enumerable helper compares the
                // full incoming key before storing Name[32] with _TRUNCATE:
                // distinct long keys can allocate repeated truncated names, so
                // never merge them by truncating before the lookup.
                if self.entry(name).is_none() {
                    self.types.push(RadiationType::named(&truncate_native_bytes(
                        name,
                        NAME_PAYLOAD_BYTES,
                    )));
                }
            }
        }
        // Every retained type re-reads its own section from this pass,
        // including the default `Radiation` entry, whose section is the
        // vanilla `[Radiation]` the globals also name.
        for index in 0..self.types.len() {
            let name = self.types[index].name.clone();
            let Some(section) = pass.section(&name) else {
                continue;
            };
            let entry = &mut self.types[index];
            if section.is_present("RadDurationMultiple") {
                entry.duration_multiple = Some(section.read_int("RadDurationMultiple", 0));
            }
            if section.is_present("RadApplicationDelay") {
                entry.application_delay = Some(section.read_int("RadApplicationDelay", 0));
            }
            if section.is_present("RadApplicationDelay.Building") {
                entry.application_delay_building =
                    Some(section.read_int("RadApplicationDelay.Building", 0));
            }
            if section.is_present("RadBuildingDamageMaxCount") {
                entry.building_damage_max_count =
                    Some(section.read_int("RadBuildingDamageMaxCount", 0));
            }
            if section.is_present("RadLevelMax") {
                entry.level_max = Some(section.read_int("RadLevelMax", 0));
            }
            if section.is_present("RadLevelDelay") {
                entry.level_delay = Some(section.read_int("RadLevelDelay", 0));
            }
            if section.is_present("RadLightDelay") {
                entry.light_delay = Some(section.read_int("RadLightDelay", 0));
            }
            if section.is_present("RadLevelFactor") {
                entry.level_factor = Some(section.read_double("RadLevelFactor", 0.0));
            }
            if section.is_present("RadLightFactor") {
                entry.light_factor = Some(section.read_double("RadLightFactor", 0.0));
            }
            if section.is_present("RadTintFactor") {
                entry.tint_factor = Some(section.read_double("RadTintFactor", 0.0));
            }
            if section.is_present("RadColor") {
                // An empty raw value keeps the parsed zero triple, where
                // native's ColorStruct read leaves the field null; the
                // global fallback covers absent keys.
                let [red, green, blue] = section.read_color_rgb("RadColor", [0, 0, 0], None);
                entry.color = Some((red, green, blue));
            }
            if section.is_present("RadSiteWarhead") {
                // ReadString with the shared capacity; an empty or `none`
                // name binds no warhead, which the damage path skips.
                entry.site_warhead = Some(
                    section
                        .read_name("RadSiteWarhead", 0x80)
                        .unwrap_or("")
                        .to_owned(),
                );
            }
            if section.is_present("RadSiteWarhead.Detonate") {
                entry.site_warhead_detonate =
                    Some(section.read_bool("RadSiteWarhead.Detonate", false));
            }
            if section.is_present("RadSiteWarhead.Detonate.Full") {
                entry.site_warhead_detonate_full =
                    Some(section.read_bool("RadSiteWarhead.Detonate.Full", true));
            }
            if section.is_present("RadHasOwner") {
                entry.has_owner = Some(section.read_bool("RadHasOwner", false));
            }
            if section.is_present("RadHasInvoker") {
                entry.has_invoker = Some(section.read_bool("RadHasInvoker", false));
            }
        }
    }
}

/// Resolve one site's effective radiation values. A Phobos build resolves
/// the bound catalog entry; every other build, and a Phobos build whose
/// catalog is absent, resolves the globals.
pub(crate) fn effective_radiation<'a>(
    rules: &'a crate::rules::ruleset::RuleSet,
    rad_type: Option<&str>,
) -> EffectiveRadiation<'a> {
    #[cfg(feature = "phobos")]
    {
        rules
            .radiation
            .radiation_types
            .effective(&rules.radiation, rad_type)
    }
    #[cfg(not(feature = "phobos"))]
    {
        let _ = rad_type;
        EffectiveRadiation::from_globals(&rules.radiation)
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "phobos_radiation_tests.rs"]
mod tests;
