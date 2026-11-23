//! Country and Side registry state: veterancy lists, the Ares SideExt and
//! HouseTypeExt fields, and the Ares nullable/valueable vector readers.

use super::{ProcessedType, RulesTypeFamily};
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::object_type::ObjectCategory;
use crate::rules::sound_ini::EvaRegistry;
use crate::util::native_x87::NativeF64Bits;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

/// HouseType511D16..51208C retains these three pointer vectors in this order.
/// One live Country as [`NativeRulesRegistryState::country_projection`] yields
/// it: ID, body, Color+C0, Side+BC, ParentCountry+98 and its veteran lists.
pub(crate) type CountryProjection<'a> = (
    &'a str,
    &'a IniSection,
    i32,
    i32,
    &'a str,
    &'a CountryVeterancyState,
);

/// The process registry owns resolved stored IDs; RuleSet clones an immutable view.
/// As in its other Type projections, distinct native pointers sharing a truncated
/// stored ID are not distinguished here. This chain covers unique stored IDs.
/// Constructor51147B/511511/511530 starts each vector empty. List membership
/// drives Infantry517CE7, Unit735604/746827 and Aircraft413F98 starting rank.
/// Native evidence: tools/rules_oracle/country_veterancy.md.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CountryVeterancyState {
    pub(super) infantry: Vec<String>,
    pub(super) units: Vec<String>,
    pub(super) aircraft: Vec<String>,
    /// Only a read the Ares way leaves this from its default.
    pub(super) ares: AresCountryState,
}

// A stock-way read leaves `ares` at its default, so a stock game folds the
// three lists alone, as before.
impl Hash for CountryVeterancyState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.infantry.hash(state);
        self.units.hash(state);
        self.aircraft.hash(state);
        if self.ares != AresCountryState::default() {
            self.ares.hash(state);
        }
    }
}

/// Live Side identity plus the immutable selected extension fields. The
/// retained ProcessedType owns initialization and reads; the RuleSet view does
/// not replay either. Rules read the stock way have no extension.
pub(crate) type SideProjection<'a> = (
    &'a str,
    Option<&'a crate::rules::ares_side_extension::AresSideExtension>,
);

/// Ares3.0 SideExt constructor1002D9F0 and dispatcher1002B450 retain the phase
/// with the extension fields. The first ReadTypeData entry initializes defaults
/// by stored ID at1002E190, then reads the current INI at1002E450; later calls
/// retain current fields. Evidence: tools/rules_oracle/ares_side_sidebar.md
/// and tools/spatial_oracle/ares_eva.md.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct AresSideState {
    pub(super) phase: i32,
    pub(super) fields: crate::rules::ares_side_extension::AresSideExtension,
}

impl Default for AresSideState {
    fn default() -> Self {
        // SideExt constructor1002D9F0 leaves the extension unset; its first
        // reader phase supplies ID-based defaults. Executed:
        // spatial_oracle/ares_eva.json.
        Self {
            phase: 0,
            fields: crate::rules::ares_side_extension::AresSideExtension::default(),
        }
    }
}

/// The type identities a Side type reference can resolve against at this
/// point in the pass: the same live registries the native `Find` calls see.
#[derive(Default)]
pub(super) struct SideRegisteredTypes {
    pub(super) infantry: HashMap<String, String>,
    vehicles: HashMap<String, String>,
    pub(super) aircraft: HashMap<String, String>,
    pub(super) buildings: HashMap<String, String>,
    pub(super) animations: HashMap<String, String>,
}

impl SideRegisteredTypes {
    pub(super) fn collect(families: &HashMap<RulesTypeFamily, Vec<ProcessedType>>) -> Self {
        let mut out = Self::default();
        let fill = |family: RulesTypeFamily, target: &mut HashMap<String, String>| {
            for member in families.get(&family).into_iter().flatten() {
                target
                    .entry(member.native_stored_id.to_ascii_uppercase())
                    .or_insert_with(|| member.native_stored_id.clone());
            }
        };
        fill(RulesTypeFamily::Infantry, &mut out.infantry);
        fill(RulesTypeFamily::Vehicle, &mut out.vehicles);
        fill(RulesTypeFamily::Aircraft, &mut out.aircraft);
        fill(RulesTypeFamily::Building, &mut out.buildings);
        fill(RulesTypeFamily::Animation, &mut out.animations);
        out
    }

    pub(super) fn find(map: &HashMap<String, String>, name: &str) -> Option<String> {
        map.get(&name.to_ascii_uppercase()).cloned()
    }

    pub(super) fn techno(&self, name: &str) -> Option<String> {
        Self::find(&self.infantry, name).or_else(|| Self::find(&self.vehicles, name))
    }
}

impl AresSideState {
    /// `SideExt::initialize @ 0x1002E190` phase-0 defaults, then the ordered
    /// reader prefix `0x1002E450..0x1002EA4C`. Every reader keeps its current
    /// field when ReadString returns 0 (absent or empty), exactly as
    /// `Nullable`/`Valueable`; only `none`/`<none>` clears a field.
    /// `color_index` is the shared ColorScheme lookup `0x1001BB30`.
    pub(super) fn read_pass(
        &mut self,
        id: &str,
        array_index: i32,
        pass: &IniFile,
        eva: &EvaRegistry,
        types: &SideRegisteredTypes,
        color_index: impl Fn(&str) -> Option<i32>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if !(0..=2).contains(&self.phase) {
            return;
        }
        if self.phase == 0 {
            self.initialize_defaults(id, array_index, types);
            self.phase = 1;
        }
        if let Some(section) = pass.section(id) {
            let fields = &mut self.fields;
            fields.base_defense_counts = read_nullable_int_vector(
                section,
                "AI.BaseDefenseCounts",
                fields.base_defense_counts.as_ref(),
            );
            fields.base_defenses = read_nullable_type_vector(
                section,
                "AI.BaseDefenses",
                fields.base_defenses.as_ref(),
                |name| SideRegisteredTypes::find(&types.buildings, name),
            );
            fields.crew = read_ares_type_ref(section, "Crew", &fields.crew, |name| {
                SideRegisteredTypes::find(&types.infantry, name)
            });
            fields.engineer = read_ares_type_ref(section, "Engineer", &fields.engineer, |name| {
                SideRegisteredTypes::find(&types.infantry, name)
            });
            fields.technician =
                read_ares_type_ref(section, "Technician", &fields.technician, |name| {
                    SideRegisteredTypes::find(&types.infantry, name)
                });
            fields.default_disguise = read_ares_type_ref(
                section,
                "DefaultDisguise",
                &fields.default_disguise,
                |name| SideRegisteredTypes::find(&types.infantry, name),
            );
            // SideExt1002EC00 reads EVA.Tag before the two sidebar keys.
            // Fixed EVAMD catalog lookup precedes literal none handling;
            // missing/empty/unknown retains the old index and never rebinds Vox.
            if let Some(tag) = section.read_name("EVA.Tag", 0x800) {
                if let Some(index) = eva.voice_index_for_tag(tag) {
                    fields.eva_voice_index = index;
                } else if is_native_none_type_name(tag) {
                    fields.eva_voice_index = -1;
                }
            }
            fields.parachute_anim =
                read_ares_type_ref(section, "Parachute.Anim", &fields.parachute_anim, |name| {
                    SideRegisteredTypes::find(&types.animations, name)
                });
            fields.paradrop_aircraft = read_ares_type_ref(
                section,
                "ParaDrop.Aircraft",
                &fields.paradrop_aircraft,
                |name| SideRegisteredTypes::find(&types.aircraft, name),
            );
            // `remove_non_paradroppables` keeps only InfantryType and UnitType
            // identities after the vector read.
            fields.paradrop_types = read_nullable_type_vector(
                section,
                "ParaDrop.Types",
                fields.paradrop_types.as_ref(),
                |name| types.techno(name),
            );
            fields.paradrop_num =
                read_nullable_int_vector(section, "ParaDrop.Num", fields.paradrop_num.as_ref());
            fields.sidebar_mix_file_index =
                section.read_ares_int("Sidebar.MixFileIndex", fields.sidebar_mix_file_index);
            fields.sidebar_yuri_file_names =
                section.read_ares_bool("Sidebar.YuriFileNames", fields.sidebar_yuri_file_names);
            if let Some(raw) = section.raw_value("ToolTipColor") {
                // `Valueable<ColorStruct>`: present-empty/invalid retains.
                fields.tooltip_color = IniSection::read_color_rgb_value(raw, fields.tooltip_color);
            }
            fields.survivor_divisor =
                section.read_ares_nullable_int("SurvivorDivisor", fields.survivor_divisor);
            // `ValueableIdx<ColorScheme>` 0x1001CC70 over `0x1001BB30(name, 1)`.
            fields.message_text_color_index = section.read_ares_color_scheme_index(
                "MessageTextColor",
                fields.message_text_color_index,
                &color_index,
            );
            fields.hunter_seeker =
                read_ares_type_ref(section, "HunterSeeker", &fields.hunter_seeker, |name| {
                    SideRegisteredTypes::find(&types.vehicles, name)
                });
            for (key, target) in [
                (
                    "CampaignScore.Background",
                    &mut fields.campaign_score_background,
                ),
                (
                    "CampaignScore.Transition",
                    &mut fields.campaign_score_transition,
                ),
                (
                    "CampaignScore.Animation",
                    &mut fields.campaign_score_animation,
                ),
                ("CampaignScore.Palette", &mut fields.campaign_score_palette),
                (
                    "MultiplayerScore.Background",
                    &mut fields.multiplayer_score_background,
                ),
                ("MultiplayerScore.Bars", &mut fields.multiplayer_score_bars),
                (
                    "MultiplayerScore.Palette",
                    &mut fields.multiplayer_score_palette,
                ),
                (
                    "CampaignScore.UnderParTheme",
                    &mut fields.campaign_score_under_par_theme,
                ),
                (
                    "CampaignScore.OverParTheme",
                    &mut fields.campaign_score_over_par_theme,
                ),
                (
                    "MultiplayerScore.WinTheme",
                    &mut fields.multiplayer_score_win_theme,
                ),
                (
                    "MultiplayerScore.LoseTheme",
                    &mut fields.multiplayer_score_lose_theme,
                ),
                ("GraphicalText.Image", &mut fields.graphical_text_image),
                ("GraphicalText.Palette", &mut fields.graphical_text_palette),
                (
                    "DialogBackground.Image",
                    &mut fields.dialog_background_image,
                ),
                (
                    "DialogBackground.Palette",
                    &mut fields.dialog_background_palette,
                ),
            ] {
                // AresFixedString<0x20> (`0x1002E5D0..`): ReadString(0x800)
                // returning 0 retains, `none`/`<none>` clears, anything else
                // copies its first 0x1F bytes.
                if let Some(value) = section.read_name(key, 0x800) {
                    *target = (!is_native_none_type_name(value)).then(|| {
                        crate::rules::ini_value::truncate_native_bytes(value, 0x1f).to_owned()
                    });
                }
            }
        }
        self.phase = 2;
    }

    fn initialize_defaults(&mut self, id: &str, array_index: i32, types: &SideRegisteredTypes) {
        let nod = id.eq_ignore_ascii_case("Nod");
        let third = id.eq_ignore_ascii_case("ThirdSide");
        let fields = &mut self.fields;
        fields.sidebar_mix_file_index = if nod || third { 2 } else { 1 };
        fields.sidebar_yuri_file_names = third;
        fields.eva_voice_index = if nod {
            1
        } else if third {
            2
        } else {
            0
        };
        fields.tooltip_color = if nod || third {
            [255, 255, 0]
        } else {
            [164, 210, 255]
        };
        fields.message_text_color_index = if nod {
            11
        } else if third {
            25
        } else {
            21
        };
        // `SideExt::initialize` sets ParaDropPlane from
        // `AircraftTypeClass::FindIndex("PDPLANE")`; -1 when unregistered.
        fields.paradrop_aircraft = Some(SideRegisteredTypes::find(&types.aircraft, "PDPLANE"));
        let score = match array_index {
            0 => [
                "ASCRBKMD.SHP",
                "ASCRTMD.SHP",
                "ASCRAMD.SHP",
                "ASCORE.PAL",
                "MPASCRNL.SHP",
                "mpascrnlbar~~.pcx",
                "MPASCRN.PAL",
            ],
            1 => [
                "SSCRBKMD.SHP",
                "SSCRTMD.SHP",
                "SSCRAMD.SHP",
                "SSCORE.PAL",
                "MPSSCRNL.SHP",
                "mpsscrnlbar~~.pcx",
                "MPSSCRN.PAL",
            ],
            _ => [
                "SYCRBKMD.SHP",
                "SYCRTMD.SHP",
                "SYCRAMD.SHP",
                "YSCORE.PAL",
                "MPYSCRNL.SHP",
                "mpyscrnlbar~~.pcx",
                "MPYSCRN.PAL",
            ],
        };
        fields.campaign_score_background = Some(score[0].to_owned());
        fields.campaign_score_transition = Some(score[1].to_owned());
        fields.campaign_score_animation = Some(score[2].to_owned());
        fields.campaign_score_palette = Some(score[3].to_owned());
        fields.multiplayer_score_background = Some(score[4].to_owned());
        fields.multiplayer_score_bars = Some(score[5].to_owned());
        fields.multiplayer_score_palette = Some(score[6].to_owned());
        fields.campaign_score_under_par_theme = Some("SCORE".to_owned());
        fields.campaign_score_over_par_theme = Some("SCORE".to_owned());
        fields.multiplayer_score_win_theme = Some("SCORE".to_owned());
        fields.multiplayer_score_lose_theme = Some("SCORE".to_owned());
    }
}

/// Ares `Nullable<T*>`/`Valueable<T*>`/`ValueableIdx<T>` reads
/// (`0x1002D910`, `0x1002D970`, `0x1000C9C0`, `0x1001CC10`): ReadString(0x800)
/// returning 0 (absent or empty) retains; a registered identity resolves;
/// otherwise `none`/`<none>` (`0x10003950`) stores null (`Some(None)`, the -1
/// index) and any other name retains.
pub(super) fn read_ares_type_ref(
    section: &IniSection,
    key: &str,
    current: &Option<Option<String>>,
    resolve: impl Fn(&str) -> Option<String>,
) -> Option<Option<String>> {
    let _ares = crate::rules::ares_switch::ares_reads();
    let Some(name) = section.read_name(key, 0x800) else {
        return current.clone();
    };
    match resolve(name) {
        Some(id) => Some(Some(id)),
        None if is_native_none_type_name(name) => Some(None),
        None => current.clone(),
    }
}

/// Ares `NullableVector<T>::Read` (`0x1002EB60`), which Phobos
/// `NullableVector<T>::Read` (`Utilities/TemplateDef.h`) repeats: ReadString(0x800)
/// returning 0 (absent or empty) retains; `<default>` returns to the getter
/// fallback; any other value replaces the vector with its comma-strtok tokens,
/// skipping names that do not resolve.
pub(crate) fn read_nullable_type_vector<T: Clone>(
    section: &IniSection,
    key: &str,
    current: Option<&Vec<T>>,
    resolve: impl Fn(&str) -> Option<T>,
) -> Option<Vec<T>> {
    let _ares = crate::rules::ares_switch::ares_reads();
    let Some(value) = section.read_name(key, 0x800) else {
        return current.cloned();
    };
    if value.eq_ignore_ascii_case("<default>") {
        return None;
    }
    Some(
        section
            .read_list(key, 0x800)
            .unwrap_or_default()
            .into_iter()
            .filter_map(resolve)
            .collect(),
    )
}

/// Ares `NullableVector<int>::Read` (`0x1002EB00`) over `parse_values<int>`,
/// with the same retain/`<default>` rules as [`read_nullable_type_vector`].
fn read_nullable_int_vector(
    section: &IniSection,
    key: &str,
    current: Option<&Vec<i32>>,
) -> Option<Vec<i32>> {
    let _ares = crate::rules::ares_switch::ares_reads();
    let Some(value) = section.read_name(key, 0x800) else {
        return current.cloned();
    };
    if value.eq_ignore_ascii_case("<default>") {
        return None;
    }
    Some(section.read_ares_int_list(key).unwrap_or_default())
}

/// Ares `Valueable<T*>`/`ValueableIdx<T>` reads (`0x1000C9C0` through
/// `0x1000EB20`, `0x1001CC10`): the [`read_ares_type_ref`] rule without a
/// separate unset state, so a `none` field and a never-set one are both
/// `None` (the null pointer or -1 index).
pub(super) fn read_valueable_type(
    section: &IniSection,
    key: &str,
    current: &Option<String>,
    resolve: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let _ares = crate::rules::ares_switch::ares_reads();
    read_ares_type_ref(section, key, &current.clone().map(Some), resolve).flatten()
}

/// Ares `ValueableVector<TechnoTypeClass*>::Read` (`0x1000EDD0`): ReadString
/// returning 0 (absent or empty) retains; any other value replaces the
/// vector with the tokens that resolve. There is no `<default>` form.
pub(super) fn read_valueable_type_vector(
    section: &IniSection,
    key: &str,
    current: &[String],
    resolve: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let _ares = crate::rules::ares_switch::ares_reads();
    match section.read_list(key, 0x800) {
        Some(tokens) => tokens.into_iter().filter_map(resolve).collect(),
        None => current.to_vec(),
    }
}

/// Ares `ValueableVector<int>::Read` (`0x1000ED00`), with the same retain
/// rule as [`read_valueable_type_vector`].
pub(super) fn read_valueable_int_vector(
    section: &IniSection,
    key: &str,
    current: &[i32],
) -> Vec<i32> {
    let _ares = crate::rules::ares_switch::ares_reads();
    section
        .read_ares_int_list(key)
        .unwrap_or_else(|| current.to_vec())
}

/// The reached CountryExt reader state needed by the starting-rank chain.
/// Constructor1001B85F/1001B87C/1001B8D0 starts phase0, an empty building
/// list and an unconsumed parent attempt. This state shares the existing
/// Country owner, persistence and immutable RuleSet projection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct AresCountryState {
    pub(super) phase: i32,
    pub(super) parent_initialized: bool,
    pub(super) buildings: Vec<String>,
    pub(super) loading: crate::rules::country_loading::AresCountryLoading,
    pub(super) presentation: crate::rules::ares_country_presentation::AresCountryPresentation,
    pub(super) paradrop: crate::rules::ares_country_paradrop::AresCountryParadrop,
    pub(super) power_plants: Vec<String>,
    /// CountryExt+1BA/+1BB `Degrades`: the constructor leaves it unset; the
    /// common reader 1001C5D0 reads it before CanBeDriven, and the parent
    /// copy 1001BDC0 copies the pair.
    #[serde(default)]
    pub(super) degrades: Option<bool>,
    /// CountryExt+1BC/+1BD: ctor1001B8DA leaves it unset;
    /// nullable CanBeDriven reader1001C70A..1001C71B. Parent copy
    /// 1001BDC0..1001C0D6 deliberately does not copy this pair.
    #[serde(default)]
    pub(super) can_be_driven: Option<bool>,
    /// CountryExt+1D1: ctor1001B908 stores the pair+1D0 as0x0100;
    /// common reader1001C74C..1001C75D retains GivesBounty on missing keys.
    /// Parent copy1001BDC0..1001C0D6 stops before these appended flags.
    #[serde(default = "default_gives_bounty")]
    pub(super) gives_bounty: bool,
    /// CountryExt+1D0 `StartInMultiplayer.WithConst` (the pair's low byte,
    /// false), read right before GivesBounty. The house starts with a
    /// construction yard instead of its base unit
    /// (MPGameMode_SpawnBaseUnit_BuildConst 1001F0B0, hook 0x5D7048,
    /// `sim::scenario_bootstrap`).
    #[serde(default)]
    pub(super) start_in_multiplayer_with_const: bool,
    /// CountryExt+1C0 `StartInMultiplayer.Types`
    /// (NullableVector<TechnoTypeClass*>, unset; HasValue +0x1CC), read
    /// right before WithConst. The starting units' candidates and budget
    /// (`sim::scenario_bootstrap`) take it in place of the
    /// AllowedToStartInMultiplayer scan. Not copied from the parent.
    #[serde(default)]
    pub(super) start_in_multiplayer_types:
        Option<Vec<crate::rules::superweapon_type::TechnoTypeReference>>,
}

fn default_gives_bounty() -> bool {
    true // CountryExt constructor1001B908's high byte.
}

pub(super) fn default_curtain_modifiers() -> [NativeF64Bits; 2] {
    [NativeF64Bits::ONE; 2]
}

impl Default for AresCountryState {
    fn default() -> Self {
        Self {
            phase: 0,
            parent_initialized: false,
            buildings: Vec::new(),
            loading: Default::default(),
            presentation: Default::default(),
            paradrop: Default::default(),
            power_plants: Vec::new(),
            degrades: None,
            can_be_driven: None,
            gives_bounty: default_gives_bounty(),
            start_in_multiplayer_with_const: false,
            start_in_multiplayer_types: None,
        }
    }
}

// GUI fields are process-retained and persisted, but do not enter simulation CRC.
impl Hash for AresCountryState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.phase.hash(state);
        self.parent_initialized.hash(state);
        self.buildings.hash(state);
        self.paradrop.hash(state);
        self.power_plants.hash(state);
        if let Some(value) = self.degrades {
            b"ares-country-degrades-v1".hash(state);
            value.hash(state);
        }
        if let Some(value) = self.can_be_driven {
            b"ares-country-driving-v1".hash(state);
            value.hash(state);
        }
        self.gives_bounty.hash(state);
        if self.start_in_multiplayer_with_const {
            b"ares-country-start-with-const-v1".hash(state);
        }
        if let Some(types) = &self.start_in_multiplayer_types {
            b"ares-country-start-types-v1".hash(state);
            types.hash(state);
        }
    }
}

impl CountryVeterancyState {
    /// The country's own `StartInMultiplayer.Types=`; `None` when unset.
    pub(crate) fn start_in_multiplayer_types(
        &self,
    ) -> Option<&[crate::rules::superweapon_type::TechnoTypeReference]> {
        self.ares.start_in_multiplayer_types.as_deref()
    }

    pub(crate) fn gives_bounty(&self) -> bool {
        self.ares.gives_bounty
    }

    /// The country's own `StartInMultiplayer.WithConst=`.
    pub(crate) fn start_in_multiplayer_with_const(&self) -> bool {
        self.ares.start_in_multiplayer_with_const
    }

    pub(crate) fn can_be_driven_override(&self) -> Option<bool> {
        self.ares.can_be_driven
    }

    pub(crate) fn degrades_override(&self) -> Option<bool> {
        self.ares.degrades
    }

    pub(crate) fn loading(&self) -> &crate::rules::country_loading::AresCountryLoading {
        &self.ares.loading
    }

    pub(crate) fn presentation(
        &self,
    ) -> &crate::rules::ares_country_presentation::AresCountryPresentation {
        &self.ares.presentation
    }

    /// The country's own `AI.PowerPlants=` list.
    pub(crate) fn power_plants(&self) -> &[String] {
        &self.ares.power_plants
    }

    /// The country's own Ares paradrop overrides.
    pub(crate) fn paradrop(&self) -> &crate::rules::ares_country_paradrop::AresCountryParadrop {
        &self.ares.paradrop
    }

    pub(crate) fn contains(&self, category: ObjectCategory, type_id: &str) -> bool {
        let members = match category {
            ObjectCategory::Infantry => &self.infantry,
            ObjectCategory::Vehicle => &self.units,
            ObjectCategory::Aircraft => &self.aircraft,
            // Ares' `VeteranBuildings=`; empty unless read the Ares way.
            ObjectCategory::Building => &self.ares.buildings,
        };
        members.iter().any(|id| id.eq_ignore_ascii_case(type_id))
    }
}
