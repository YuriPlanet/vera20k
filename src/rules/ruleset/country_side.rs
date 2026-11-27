//! Country (HouseType) and side queries on a `RuleSet`, the Ares side
//! extensions, and the per-type Phobos rule switches.

use crate::rules::native_processing::house_type_index_of_name;
use crate::rules::object_type::{BuildCategory, ObjectCategory, ObjectType};
use crate::rules::warhead_type::WarheadType;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

use super::{
    CountryDifficultyBiases, CountryIdx, CountryRules, RuleSet, STOCK_UNKNOWN_INSIGNIA, SideIdx,
};

impl RuleSet {
    /// Native Country veteran-list membership uses the current HouseType,
    /// not ParentCountry, and is independent of Trainable/Naval/spy state.
    /// Starting-rank callers: Infantry517CE7, Unit735604/746827,
    /// Aircraft413F98. With Ares, Building Init_Managers442D1B reaches
    ///100236D0 and its retained VeteranBuildings list with the same gates.
    pub fn country_starts_veteran(&self, country: &str, object: &ObjectType) -> bool {
        self.country_rules(country)
            .is_some_and(|rules| rules.veteran_types.contains(object.category, &object.id))
    }

    pub fn country_multiplay(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.multiplay)
    }

    /// Whether a country/house type has `MultiplayPassive=true`.
    pub fn country_multiplay_passive(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.multiplay_passive)
    }

    /// Ares `File.Flag=` (stock/unknown defaults included). None in a stock
    /// game, where the shell keeps its fixed flag table.
    pub fn country_flag(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.flag.as_deref())
        } else {
            None
        }
    }

    /// The insignia surface a Country selection draws, as `Game_GetFlagSurface
    /// @ 0x1001DCE0` (Ares patch site `0x4E3562`, 5 bytes) answers it: the
    /// country's own `File.Flag`, or the stock `rani.pcx` branch at `0x4E3567`
    /// when that file is cleared. `None` in a stock game, where each shell
    /// keeps its fixed country table.
    pub fn country_flag_surface(&self, id: &str) -> Option<String> {
        if self.ares {
            self.country_rules(id).map(|country| {
                country
                    .presentation
                    .flag
                    .clone()
                    .unwrap_or_else(|| STOCK_UNKNOWN_INSIGNIA.to_owned())
            })
        } else {
            None
        }
    }

    /// `Game_GetFlagSurface @ 0x1001DCE0` for a HouseType country index. An
    /// index outside the allocated array returns `None`, where the stock
    /// selection chain from `0x4E3579` still decides.
    pub fn country_insignia_pcx(&self, index: u16) -> Option<String> {
        let id = self.country_name(CountryIdx(index))?;
        self.country_flag_surface(id)
    }

    /// Ares `MenuText.Status=` CSF key (stock/unknown defaults included).
    pub fn country_status_key(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.status.as_deref())
        } else {
            None
        }
    }

    /// Ares `File.Taunt=` pattern; `~~` or the stock `%02i` receives the
    /// taunt number elsewhere. None in a stock game.
    pub fn country_taunt_pattern(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)
                .and_then(|country| country.presentation.taunt.as_deref())
        } else {
            None
        }
    }

    /// `File.Taunt=` for a HouseType country index (the taunt byte's field).
    pub fn country_taunt_pattern_by_index(&self, index: u16) -> Option<&str> {
        let id = self.country_name(CountryIdx(index))?;
        self.country_taunt_pattern(id)
    }

    /// Ares `ListIndex=`; negative hides the country. A stock game returns
    /// the common 100, preserving registry order.
    pub fn country_list_index(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(100, |country| country.presentation.list_index)
        } else {
            100
        }
    }

    /// Ares `RandomSelectionWeight=`, default 1.
    pub fn country_random_weight(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(1, |country| country.presentation.random_weight)
        } else {
            1
        }
    }

    /// Ares `AIRandomSelectionWeight=` once set, else the human weight
    /// (`0x1001CA11..0x1001CA1A`).
    pub fn country_ai_random_weight(&self, id: &str) -> i32 {
        if self.ares {
            self.country_rules(id)
                .map_or(1, |country| country.presentation.ai_weight())
        } else {
            self.country_random_weight(id)
        }
    }

    /// Ares `HouseTypeExt::GetPowerplants @ 0x1001BD20`, as BuildingType
    /// indices: country `id`'s `AI.PowerPlants=` (`+0x130`) while it is
    /// non-empty, else by the country's side (`HouseType+0xBC`): 0
    /// `[GDIPowerPlant]`, 1 `[NodRegularPower, NodAdvancedPower]`
    /// (`0x1001BD6C..0x1001BD8C`), 2 `[ThirdPowerPlant]`, each without its
    /// unset slots, and any other side none.
    pub fn ares_power_plants(&self, id: &str, side: i32) -> Vec<i32> {
        let index = |name: &Option<String>| {
            name.as_deref()
                .and_then(|name| self.building_type_index(name))
        };
        let country = self
            .country_rules(id)
            .map_or(&[][..], |country| country.power_plants.as_slice());
        if !country.is_empty() {
            return country
                .iter()
                .filter_map(|name| self.building_type_index(name))
                .collect();
        }
        let plants = &self.general.building_types;
        let slots = match side {
            0 => [&plants.gdi_power_plant, &None],
            1 => [&plants.nod_regular_power, &plants.nod_advanced_power],
            2 => [&plants.third_power_plant, &None],
            _ => [&None, &None],
        };
        slots.into_iter().filter_map(index).collect()
    }

    /// `HouseTypeExt::GetParadropContent @ 0x1001BC20` for a country: its own
    /// `ParaDrop.Types`/`.Num` while the type list is non-empty, else `None`,
    /// which defers to the side layer (also the answer in a stock game).
    pub fn country_paradrop_lists(&self, id: &str) -> Option<(&[String], &[i32])> {
        if self.ares {
            self.country_rules(id)?.paradrop.lists()
        } else {
            None
        }
    }

    /// Ares `[Country] ParaDrop.Aircraft=` (`HouseTypeExt +0x154`), which
    /// `GetParadropPlane @ 0x1001BCB0` prefers over the side's plane. A
    /// never-set or `none` index is `-1` there, so this answers `None` and the
    /// caller keeps the side layer.
    pub fn country_paradrop_aircraft(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)?.paradrop.aircraft.as_deref()
        } else {
            None
        }
    }

    /// Ares `[Country] Parachute.Anim=` (`HouseTypeExt +0x158`), which
    /// `GetParachuteAnim @ 0x1001BBC0` prefers over the side's canopy and the
    /// rules default. The canopy hook reaches it only when the type names no
    /// parachute of its own.
    pub fn country_parachute_anim(&self, id: &str) -> Option<&str> {
        if self.ares {
            self.country_rules(id)?.paradrop.parachute_anim.as_deref()
        } else {
            None
        }
    }

    /// Ares `File.ObserverBackground=` (PCX or SHP), when retained.
    pub fn country_observer_background(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_background
            .as_deref()
    }

    /// Ares `File.ObserverFlag=` (PCX or SHP), when retained.
    pub fn country_observer_flag(&self, id: &str) -> Option<&str> {
        self.country_rules(id)?
            .presentation
            .observer_flag
            .as_deref()
    }

    /// Ares `File.ObserverFlagAltPalette=`.
    pub fn country_observer_flag_alt_palette(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.presentation.observer_flag_alt_palette)
    }

    /// Ares capture1004524F..10045275 reads CountryExt+1BC when present,
    /// otherwise the live HouseType+1A6 MultiplayPassive byte.
    pub(crate) fn country_can_be_driven(&self, id: &str) -> bool {
        self.country_rules(id).is_some_and(|country| {
            country
                .veteran_types
                .can_be_driven_override()
                .unwrap_or(country.multiplay_passive)
        })
    }

    /// Ares `HouseClass_Update_DamageDelay @ 100247A0`: CountryExt+1BA
    /// `Degrades=` when set; `None` leaves the HouseType's
    /// `!MultiplayPassive` default to the caller.
    pub(crate) fn country_degrades(&self, id: &str) -> Option<bool> {
        self.country_rules(id)
            .and_then(|country| country.veteran_types.degrades_override())
    }

    /// CountryExt+1C0 `StartInMultiplayer.Types=` of the country itself
    /// (MPGameMode_SpawnStartingUnits_Types 1001F220 reads House+34's ext);
    /// `None` when unset.
    pub(crate) fn country_start_in_multiplayer_types(
        &self,
        id: &str,
    ) -> Option<&[crate::rules::superweapon_type::TechnoTypeReference]> {
        self.country_rules(id)
            .and_then(|country| country.veteran_types.start_in_multiplayer_types())
    }

    /// The country's `StartInMultiplayer.WithConst=` (CountryExt+1D0), which
    /// MPGameMode_SpawnBaseUnit_BuildConst (1001F0B0) tests.
    pub(crate) fn country_start_in_multiplayer_with_const(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.veteran_types.start_in_multiplayer_with_const())
    }

    /// Victim CountryExt+1D1 gate in Ares bounty payout10043279..10043289.
    pub(crate) fn country_gives_bounty(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_some_and(|country| country.veteran_types.gives_bounty())
    }

    /// HouseType511850 scalar fields used by SetDifficulty4F6EC0. Unknown
    /// countries retain its constructor's neutral doubles. Category cost,
    /// speed and armor getters use their own independent fields.
    pub(crate) fn country_difficulty_biases(&self, id: &str) -> CountryDifficultyBiases {
        self.country_rules(id)
            .map_or_else(CountryDifficultyBiases::default, |country| {
                let bits = |value: f64| NativeF64Bits::from_bits(value.to_bits());
                CountryDifficultyBiases {
                    firepower: bits(country.firepower),
                    ground_speed: bits(country.ground_speed),
                    air_speed: bits(country.air_speed),
                    armor: bits(country.armor),
                    rof: bits(country.rof),
                    cost: bits(country.cost),
                    build_time: bits(country.build_time),
                }
            })
    }

    /// Whether a country/house type may claim nearby map walls. Native default is true.
    pub fn country_wall_owner(&self, id: &str) -> bool {
        self.country_rules(id)
            .is_none_or(|country| country.wall_owner)
    }

    /// HouseType+148 income float; an unknown country has the neutral default.
    pub fn country_income_mult(&self, id: &str) -> NativeF32Bits {
        self.country_rules(id)
            .map_or(NativeF32Bits::ONE, |country| country.income_mult)
    }

    /// A country's `Cost*Mult=` floats (`HouseType+0x114..+0x124`), the
    /// constructor's 1.0 for an unknown country.
    pub(crate) fn country_cost_mults(&self, id: &str) -> [NativeF32Bits; 5] {
        self.country_rules(id)
            .map_or([NativeF32Bits::ONE; 5], |country| country.cost_mults)
    }

    /// A country's `Speed*Mult=` floats (`HouseType+0x128..+0x130`), the
    /// constructor's 1.0 for an unknown country.
    pub fn country_speed_mults(&self, id: &str) -> [NativeF32Bits; 3] {
        self.country_rules(id)
            .map_or([NativeF32Bits::ONE; 3], |country| country.speed_mults)
    }

    /// `HouseClass::GetArmorMultForType @ 0x0050BD30` for a house of country
    /// `id`: its per-category float for `object` (a `BuildCat=Combat`
    /// building takes `ArmorDefensesMult=`), 1.0 for an unknown country.
    pub(crate) fn country_armor_mult_for_type(&self, id: &str, object: &ObjectType) -> f32 {
        let Some(country) = self.country_rules(id) else {
            return 1.0;
        };
        match object.category {
            ObjectCategory::Infantry => country.armor_infantry_mult,
            ObjectCategory::Vehicle => country.armor_units_mult,
            ObjectCategory::Aircraft => country.armor_aircraft_mult,
            ObjectCategory::Building if object.build_cat == Some(BuildCategory::Combat) => {
                country.armor_defenses_mult
            }
            ObjectCategory::Building => country.armor_buildings_mult,
        }
    }

    /// `HouseClass @ 0x0050C0A0` for a house of country `id`: its
    /// `BuildTime*Mult=` for `object`'s class (a `BuildCat=Combat` building
    /// takes the defenses one), 1.0 for an unknown country.
    pub(crate) fn country_build_time_mult_for_type(
        &self,
        id: &str,
        object: &ObjectType,
    ) -> NativeF32Bits {
        let Some(country) = self.country_rules(id) else {
            return NativeF32Bits::ONE;
        };
        country.build_time_mults[object.factor_slot()]
    }

    /// Native Country+98 as retained by511850. An unread registered Country
    /// has an empty parent; a missing key in a reached section defaults to its ID.
    pub fn country_parent_name(&self, country: &str) -> Option<&str> {
        self.country_rules(country)
            .map(|fields| fields.parent_country.as_str())
    }

    /// Owner-side bit used by FirstOwnable505316, FirstBuildable5051E9,
    /// Recalc5054E2 and the AI economy/defense choosers. Reuse the native
    /// alias-before-ID5117D0 scan: <random> is -2/bit30, unknown is -1/bit31.
    /// RequiredHouses/ForbiddenHouses instead use the actual Country index.
    pub fn country_owner_mask(&self, country: &str) -> u32 {
        let parent = self.country_parent_name(country).unwrap_or(country);
        let index = house_type_index_of_name(parent, self.country_lookup_members());
        1u32.wrapping_shl(index as u32)
    }

    /// The token/OR tail of INI ReadOwners4750D0 through48DEB0. Only -1
    /// is rejected; <random> (-2) contributes bit30. Trigger's <none> ->0
    /// override does not apply. The production list reader owns capacity
    /// and tokenization; the retained Country registry owns name lookup.
    /// Native controls: tools/rules_oracle/session_country_starting_ownership.json
    /// and tools/rules_oracle/session_country_parent.json.
    pub fn house_type_mask(&self, tokens: &[String]) -> u32 {
        crate::rules::native_processing::house_type_mask_from_indices(
            tokens
                .iter()
                .map(|token| house_type_index_of_name(token, self.country_lookup_members())),
        )
    }

    /// Resolve a country name to its stable `[Countries]` registration index.
    pub fn country_index(&self, id: &str) -> Option<CountryIdx> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.country_indices.get(key).copied()
        })
    }

    /// Installed Ares1001BA90 shared Find: first stored-ID match, sentinel
    /// rejection and no alias/ParentCountry expansion. Callers own self fallback.
    /// Native controls: rules_oracle/ares_country_veterancy.json parent_find.
    pub fn ares_find_country(&self, token: &str) -> Option<CountryIdx> {
        crate::rules::native_processing::ares_country_index_of_name(
            token,
            self.country_ids.iter().map(String::as_str),
        )
    }

    /// HouseRead500A05 calls ReadHouseType475540: an absent Country maps
    /// caller-1 to entry0; known tokens use the shared alias-before-ID5117D0
    /// lookup. Unknown nonempty tokens need the native factory branch and
    /// return None here, never an invented entry0 binding.
    pub(crate) fn scenario_country_name(&self, country: Option<&str>) -> Option<&str> {
        let index = match country {
            Some(name) => self.country_index_of_name(name)?,
            None => CountryIdx(0),
        };
        self.country_name(index)
    }

    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0` as a registered Country.
    /// The source-order scan checks each HouseType's `Name=` alias (`+0x64`)
    /// before its registry ID (`+0x24`), ignoring case. The shared owner
    /// retains `<random>`=-2 and unknown=-1; this typed projection returns
    /// only registered Country identities.
    ///
    /// VERA-only: the answer comes from `country_name_indices`, which holds
    /// the first Country of the scan for every alias and identity, so a
    /// lookup compares no names. A debug build under the live-read check runs
    /// the scan beside it.
    pub(crate) fn country_index_of_name(&self, name: &str) -> Option<CountryIdx> {
        // The shared owner answers `<random>` with -2 before its scan.
        let found = (!name.eq_ignore_ascii_case("<random>"))
            .then(|| {
                crate::util::native_string::with_ascii_uppercase(name, |key| {
                    self.country_name_indices.get(key).copied()
                })
            })
            .flatten();
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || found.map_or(-1, |index| i32::from(index.0)).max(-1)
                    == house_type_index_of_name(name, self.country_lookup_members()).max(-1),
            "the Country name index answers {found:?} for {name:?}, unlike the scan"
        );
        found
    }

    /// Cooperative5C34F0/5C35F0 draw over the complete retained HouseType
    /// registry, then49D9F0 matches ID/Name. Passive and non-playable entries
    /// remain rejection candidates and therefore affect Scenario RNG order.
    pub fn cooperative_country_candidates(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.country_lookup_members()
    }

    fn country_lookup_members(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.country_ids.iter().map(|id| {
            (
                id.as_str(),
                self.countries
                    .get(id)
                    .and_then(|country| country.name.as_deref()),
            )
        })
    }

    /// House ctor4F5E5A copies Country+C0 to its current color. A registered
    /// Country whose entire section was absent retains the ctor's0.
    pub fn country_color_scheme(&self, country: &str) -> i32 {
        self.country_rules(country)
            .map_or(0, |fields| fields.color_scheme)
    }

    /// Resolve a trigger owner token to the canonical HouseType registration.
    ///
    /// gamemd-derived: `TriggerTypeClass::Read` resolves token 1 through
    /// `HouseTypeClass__FindIndexOfName @ 0x005117D0`
    /// ([`Self::country_index_of_name`]). Before that scan,
    /// ReadINI727292..7272AA compares literal817474 (`<none>`) and selects
    /// Country zero. This is a TriggerType caller rule, not lookup behavior.
    /// Original caller controls: tools/input_oracle/campaign_start.py
    /// --houses, side_controls.trigger_type_reader_rows (+meta).
    pub fn trigger_house_type_index(&self, owner: &str) -> Option<CountryIdx> {
        if owner.eq_ignore_ascii_case("<none>") {
            return self.country_name(CountryIdx(0)).map(|_| CountryIdx(0));
        }
        self.country_index_of_name(owner)
    }

    /// Resolve a country index back to its source spelling.
    pub fn country_name(&self, index: CountryIdx) -> Option<&str> {
        self.country_ids.get(index.0 as usize).map(String::as_str)
    }

    /// Resolve a side name to its stable `[Sides]` registration index.
    pub fn side_index(&self, id: &str) -> Option<SideIdx> {
        crate::util::native_string::with_ascii_uppercase(id, |key| {
            self.side_indices.get(key).copied()
        })
    }

    /// Commander personalities from `[AIPersonalities]`. Empty for retail data.
    pub fn personalities(&self) -> &crate::rules::personality::PersonalityCatalog {
        &self.personalities
    }

    /// `[CommanderFeedback]`: whether the talking-head layer runs at all, and
    /// how easily a house is considered to be in trouble.
    pub fn commander_feedback(&self) -> crate::rules::personality::CommanderFeedbackRules {
        self.commander_feedback.clone()
    }

    /// `[EndGameScreens]`: the per-country score-page themes and backgrounds.
    /// Empty for retail data, which is what keeps the retail page untouched.
    pub fn end_game_screens(&self) -> &crate::rules::end_game::EndGameCatalog {
        &self.end_game_screens
    }

    /// Phobos `WarheadTypeExt::AllowDamageOnSelf.Get([General] ...)`: the
    /// warhead's own nullable value, then the global, defaulting to false.
    #[cfg(feature = "phobos")]
    pub fn allow_damage_on_self_for(&self, warhead: &WarheadType) -> bool {
        warhead
            .allow_damage_on_self()
            .unwrap_or(self.general.allow_damage_on_self)
    }

    /// Phobos is compiled out: the native `DamageSelf` gate stays final.
    #[cfg(not(feature = "phobos"))]
    pub fn allow_damage_on_self_for(&self, _warhead: &WarheadType) -> bool {
        false
    }

    /// Phobos `WarheadTypeExt::DecloakDamagedTargets.Get([General] ...)`: the
    /// warhead's own nullable value, then the global, defaulting to true.
    #[cfg(feature = "phobos")]
    pub fn decloak_damaged_targets_for(&self, warhead: &WarheadType) -> bool {
        warhead
            .decloak_damaged_targets()
            .unwrap_or(self.general.decloak_damaged_targets)
    }

    /// Phobos `InfantryAutoDeploy`, resolved the way `InfantryClass::
    /// Approach_Target`'s hook resolves it: the TechnoType's own value, then
    /// `[General]`, defaulting to `false` (the retail YR behaviour). `true`
    /// re-enables the deploy arm at original `0x00522434`.
    #[cfg(feature = "phobos")]
    pub fn infantry_auto_deploy_for(&self, ty: &ObjectType) -> bool {
        ty.infantry_auto_deploy()
            .unwrap_or(self.general.infantry_auto_deploy)
    }

    /// Phobos `ProneSpeed`, resolved the way the hook at original `0x00521D94`
    /// resolves it: the InfantryType's own value, then the global that the
    /// type's `Crawls=` selects.
    #[cfg(feature = "phobos")]
    pub fn prone_speed_for(&self, ty: &ObjectType) -> f64 {
        ty.prone_speed().unwrap_or(if ty.crawls {
            self.general.prone_speed_crawls
        } else {
            self.general.prone_speed_no_crawls
        })
    }

    /// Phobos `MindControl.IgnoreSize`: the type's own nullable, then
    /// `[General]`, constructor default true.
    #[cfg(feature = "phobos")]
    pub fn mind_control_ignore_size_for(&self, ty: &ObjectType) -> bool {
        ty.mind_control_ignore_size
            .unwrap_or(self.mind_control.mind_control_ignore_size)
    }

    /// Phobos `MultiMindControl.ReleaseVictim`: the type's own nullable, then
    /// `[General]`, constructor default false.
    #[cfg(feature = "phobos")]
    pub fn multi_mind_control_release_victim_for(&self, ty: &ObjectType) -> bool {
        ty.multi_mind_control_release_victim
            .unwrap_or(self.mind_control.multi_mind_control_release_victim)
    }

    /// Phobos `MindControlLink.VisibleToHouse`: the type's own nullable, then
    /// `[General]`, constructor default All.
    #[cfg(feature = "phobos")]
    pub fn mind_control_link_visible_to_house_for(
        &self,
        ty: &ObjectType,
    ) -> crate::rules::phobos_shields::AffectedHouse {
        ty.mind_control_link_visible_to_house
            .unwrap_or(self.mind_control.mind_control_link_visible_to_house)
    }

    /// Type-select admission for a structure.
    ///
    /// The native ID match (`0x00732C30`) admits a building of a listed type
    /// when CanBeSelectedNow (vt+0x13C) answers true or its type has
    /// `UndeploysInto=` (`0x00732C61..0x00732C83`). A building's
    /// CanBeSelectedNow (`0x00459C00`) first asks `0x00465D40`, true only for
    /// a 1x1 `UndeploysInto=` type, so retail admits exactly the
    /// `UndeploysInto=` buildings. Phobos's hook makes `0x00465D40` answer true
    /// during the ID match under `BuildingTypeSelectable`, with Ares loaded.
    /// RESIDUAL: with the switch on, native still applies CanBeSelectedNow's
    /// TechnoClass checks (`0x006FC030`); this admits every building of the
    /// type.
    pub fn structure_type_selectable(&self, id: &str) -> bool {
        #[cfg(feature = "phobos")]
        {
            if self.ares && self.general.building_type_selectable {
                return true;
            }
        }
        self.object(id)
            .is_some_and(|object| object.undeploys_into.is_some())
    }

    /// Phobos `[AudioVisual] MissingCameo=`: the file a cameo with no art
    /// falls back to.
    #[cfg(feature = "phobos")]
    pub fn missing_cameo(&self) -> &str {
        &self.general.missing_cameo
    }

    /// Phobos `Sidebar.GDIPositions=` for a native side index. A missing or
    /// out-of-range side falls back to the first-side default (index 0 true).
    #[cfg(feature = "phobos")]
    pub fn side_gdi_positions(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.side_gdi_positions.get(index).copied())
            .unwrap_or(index == 0)
    }

    /// Phobos `FlyNoWobbles`, resolved the way the hook at original `0x4CF8B1`
    /// resolves it: the TechnoType's own value, then `[AudioVisual]`, then the
    /// retail rule that branch reads in an unmodified game (`IsDropship`).
    /// `true` skips the `FlyLocomotionClass::Draw_Point` wobble.
    #[cfg(feature = "phobos")]
    pub fn fly_no_wobbles_for(&self, ty: &ObjectType) -> bool {
        crate::rules::phobos_fly_wobble::resolve(
            ty.fly_no_wobbles(),
            self.fly_no_wobbles,
            ty.is_dropship(),
        )
    }

    /// Phobos `[SIDE] SuperWeaponSidebar.*PCX=` for a native side index.
    /// Missing sides have no artwork, which is valid.
    #[cfg(feature = "phobos")]
    pub fn side_sw_sidebar_art(
        &self,
        index: i32,
    ) -> Option<&crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.side_sw_sidebar_art.get(index))
    }

    /// Resolve a side index back to its source spelling.
    pub fn side_name(&self, index: SideIdx) -> Option<&str> {
        self.side_ids.get(index.0 as usize).map(String::as_str)
    }

    /// The already-allocated Side array, including constructor-only entries;
    /// querying it never rereads an INI or creates a missing side.
    pub fn allocated_sides(&self) -> impl Iterator<Item = (SideIdx, &str)> {
        self.side_ids.iter().enumerate().map(|(index, id)| {
            (
                SideIdx(u8::try_from(index).expect("SIDE_LIMIT keeps the Side registry within u8")),
                id.as_str(),
            )
        })
    }

    /// Signed SideExt+88. Zero and negative values are intentional native
    /// selector inputs, not missing values or an instruction to clamp to one.
    /// A stock game has no SideExt configuration.
    pub fn side_sidebar_mix_file_index(&self, index: SideIdx) -> Option<i32> {
        if self.ares {
            self.side_extensions
                .get(usize::from(index.0))
                .map(|extension| extension.sidebar_mix_file_index)
        } else {
            None
        }
    }

    /// SideExt+8C selects Yuri-specific sidebar filenames independently of
    /// the Side array index and Sidebar.MixFileIndex.
    pub fn side_sidebar_yuri_file_names(&self, index: SideIdx) -> Option<bool> {
        if self.ares {
            self.side_extensions
                .get(usize::from(index.0))
                .map(|extension| extension.sidebar_yuri_file_names)
        } else {
            None
        }
    }

    /// Signed SideExt+90 after reached EVA.Tag reads. Binding uses saved
    /// Scenario.Side and preserves an out-of-catalog value; filename selection
    /// decides whether that value has a column. Native: Ares1002FB00.
    pub fn side_eva_voice_index(&self, side: i32) -> Option<i32> {
        self.side_extension(side)
            .map(|extension| extension.eva_voice_index)
    }

    fn side_extension(
        &self,
        side: i32,
    ) -> Option<&crate::rules::ares_side_extension::AresSideExtension> {
        self.side_extensions.get(usize::try_from(side).ok()?)
    }

    /// Ares Side default fallbacks use the side's array index; a fourth or
    /// later side takes the Allied branch (3.0 getters at 0x1002DE60,
    /// 0x1002DEA0, 0x1002E130, 0x1002DDD0, 0x1002DD40, 0x1002DFB0,
    /// 0x1002E060). `None` means the game has no SideExt for this index, so
    /// the caller keeps its stock path.
    fn ares_default_side_index(side: i32) -> usize {
        match side {
            1 => 1,
            2 => 2,
            _ => 0,
        }
    }

    /// `[Side] Crew=` else `AlliedCrew`/`SovietCrew`/`ThirdCrew` by array
    /// index. `Some(None)` is an explicit `none` (no crew), distinct from an
    /// unset key (caller falls back as native `SideExt::GetCrew @ 0x1002DE60`).
    pub(crate) fn side_crew(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.crew {
                Some(value) => value.as_deref(),
                None => match side {
                    1 => self.general.soviet_crew.as_deref(),
                    2 => self.general.third_crew.as_deref(),
                    _ => self.general.allied_crew.as_deref(),
                },
            })
        } else {
            None
        }
    }

    /// `[Side] Engineer=` else global `[General] Engineer=`
    /// (`SideExt::GetEngineer @ 0x1002DEE0`).
    pub(crate) fn side_engineer(&self, side: i32) -> Option<Option<&str>> {
        let extension = self.side_extension(side)?;
        Some(match &extension.engineer {
            Some(value) => value.as_deref(),
            None => self.general.engineer_infantry.as_deref(),
        })
    }

    /// `[Side] Technician=` else global `[General] Technician=`
    /// (`SideExt::GetTechnician @ 0x1002E170`).
    pub(crate) fn side_technician(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.technician {
                Some(value) => value.as_deref(),
                None => self.general.technician.as_deref(),
            })
        } else {
            None
        }
    }

    /// `[Side] DefaultDisguise=` else the stock side disguise by array index
    /// (`SideExt::GetDisguise @ 0x1002DEA0`).
    pub(crate) fn side_disguise(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(match &extension.default_disguise {
                Some(value) => value.as_deref(),
                None => {
                    self.general.infantry_disguises[Self::ares_default_side_index(side)].as_deref()
                }
            })
        } else {
            None
        }
    }

    /// `[Side] SurvivorDivisor=` else `Allied/Soviet/ThirdSurvivorDivisor`
    /// (`SideExt::GetSurvivorDivisor @ 0x1002E130`).
    pub(crate) fn side_survivor_divisor(&self, side: i32) -> Option<i32> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(extension.survivor_divisor.unwrap_or(match side {
                1 => self.general.soviet_survivor_divisor,
                2 => self.general.third_survivor_divisor,
                _ => self.general.allied_survivor_divisor,
            }))
        } else {
            None
        }
    }

    /// `[Side] AI.BaseDefenses=` else the side-index list; a fourth or later
    /// side takes the Allied list (`SideExt::GetBaseDefenses @ 0x1002DDD0`).
    pub(crate) fn side_base_defenses(&self, side: i32) -> Option<&[String]> {
        let extension = self.side_extension(side)?;
        Some(
            extension
                .base_defenses
                .as_deref()
                .unwrap_or_else(|| match side {
                    1 => &self.soviet_base_defense_types,
                    2 => &self.third_base_defense_types,
                    _ => &self.allied_base_defense_types,
                }),
        )
    }

    /// `[Side] AI.BaseDefenseCounts=` else the side-index difficulty vector;
    /// a fourth or later side takes the Allied vector
    /// (`SideExt::GetBaseDefenseCounts @ 0x1002DD40`).
    pub(crate) fn side_base_defense_counts(&self, side: i32) -> Option<&[i32]> {
        if self.ares {
            let extension = self.side_extension(side)?;
            Some(
                extension
                    .base_defense_counts
                    .as_deref()
                    .unwrap_or_else(|| match side {
                        1 => &self.soviet_base_defense_counts,
                        2 => &self.third_base_defense_counts,
                        _ => &self.allied_base_defense_counts,
                    }),
            )
        } else {
            None
        }
    }

    /// `[Side] ParaDrop.Types=`/`ParaDrop.Num=` only when both are set;
    /// otherwise the side-index lists, with a fourth or later side taking the
    /// Allied pair (`SideExt::GetParaDropTypes @ 0x1002E060`,
    /// `GetParaDropNum @ 0x1002DFB0`).
    pub(crate) fn side_paradrop(&self, side: i32) -> Option<(&[String], &[i32])> {
        if self.ares {
            let extension = self.side_extension(side)?;
            if let (Some(types), Some(counts)) =
                (&extension.paradrop_types, &extension.paradrop_num)
            {
                return Some((types, counts));
            }
            let lists = match side {
                1 => &self.general.sov_paradrop,
                2 => &self.general.yuri_paradrop,
                _ => &self.general.ally_paradrop,
            };
            Some((lists.infantry.as_slice(), lists.counts.as_slice()))
        } else {
            None
        }
    }

    /// `[Side] ParaDrop.Aircraft=` else the plane the SideExt initializer
    /// resolved from `PDPLANE` (`+0x74`). `None` for no SideExt or the -1
    /// index (`none`, or PDPLANE unregistered at initialization), where
    /// `HouseTypeExt::GetParadropPlane @ 0x1001BCB0` falls back to
    /// `FindIndex("PDPLANE")` rather than cancelling the drop.
    pub(crate) fn side_paradrop_aircraft(&self, side: i32) -> Option<&str> {
        if self.ares {
            self.side_extension(side)?
                .paradrop_aircraft
                .as_ref()?
                .as_deref()
        } else {
            None
        }
    }

    /// `[Side] Parachute.Anim=` only; an unset key leaves the existing
    /// `[General] Parachute=`/NPSE `VehicleParachute=` owner in charge.
    /// `Some(None)` is an explicit `none`, which the canopy consumer resolves
    /// to `PARACH` (`HouseTypeExt::GetParachuteAnim @ 0x1001BBC0`).
    pub(crate) fn side_parachute_override(&self, side: i32) -> Option<Option<&str>> {
        if self.ares {
            let extension = self.side_extension(side)?;
            extension
                .parachute_anim
                .as_ref()
                .map(|name| name.as_deref())
        } else {
            None
        }
    }

    /// `[Side] ToolTipColor=` as RGB (initialized 255,255,0 for Nod/ThirdSide,
    /// 164,210,255 otherwise; `SideExt+80`).
    pub fn side_tooltip_color(&self, side: i32) -> Option<[u8; 3]> {
        if self.ares {
            self.side_extension(side)
                .map(|extension| extension.tooltip_color)
        } else {
            None
        }
    }

    /// `[Side] MessageTextColor=` as the runtime ColorScheme index
    /// (initialized 11 for Nod, 25 for ThirdSide, 21 otherwise) or -1 for an
    /// explicit `none` (`SideExt+84`).
    pub fn side_message_text_color_index(&self, side: i32) -> Option<i32> {
        if self.ares {
            self.side_extension(side)
                .map(|extension| extension.message_text_color_index)
        } else {
            None
        }
    }

    /// `[Side] HunterSeeker=` (SideExt+0x94), the fallback UnitType of
    /// SW_HunterSeeker::Activate10072885.
    pub(crate) fn side_hunter_seeker(&self, side: i32) -> Option<&str> {
        self.side_extension(side)?
            .hunter_seeker
            .as_ref()?
            .as_deref()
    }

    /// Configured Type, Country and Side canopy identities for the shared
    /// animation asset closure (Ares ObjectClass hook5F5ADD/10047C60).
    pub fn parachute_animations(&self) -> impl Iterator<Item = &str> {
        self.side_extensions
            .iter()
            .filter_map(|extension| extension.parachute_anim.as_ref()?.as_deref())
            .chain(
                self.countries
                    .values()
                    .filter_map(|country| country.paradrop.parachute_anim.as_deref()),
            )
            .chain(
                self.all_objects()
                    .filter_map(|object| object.techno_combat().presentation().parachute_anim()),
            )
    }

    /// `[Side] GraphicalText.Image=` (SideExt `+0x1F8`) and
    /// `[Side] GraphicalText.Palette=` (`+0x218`).
    /// `Game_LoadUI_LoadSideData` (`0x1002F4A0`) loads each into its own
    /// tactical-overlay slot, and `TacticalClass_DrawOverlay_GraphicalText`
    /// (`0x1002FAD0`, patch site `0x6D4E79`) reads them through `0x1002DF10` /
    /// `0x1002DF00`, which fall back to InitGame's literal `GRFXTXT` files per
    /// slot. `None` when the game has no SideExt for this index.
    pub fn side_graphical_text(&self, side: i32) -> Option<(Option<&str>, Option<&str>)> {
        let extension = self.side_extension(side)?;
        Some((
            extension.graphical_text_image.as_deref(),
            extension.graphical_text_palette.as_deref(),
        ))
    }

    /// Return the country's effective side after its optional `Side=`
    /// override has superseded `[Sides]` membership.
    pub fn country_side_index(&self, id: &str) -> Option<SideIdx> {
        let country = self.country_index(id)?;
        self.country_sides
            .get(country.0 as usize)
            .copied()
            .flatten()
    }

    /// Country+BC is signed: an allocated constructor retains-1 until a
    /// reached Sides/Country reader writes it (511416,672597,51208C).
    /// None means unknown Country; Some(-1) is a known unbound Country.
    pub fn country_side_index_raw(&self, id: &str) -> Option<i32> {
        let country = self.country_index(id)?;
        self.country_sides
            .get(usize::from(country.0))
            .map(|side| side.map_or(-1, |index| i32::from(index.0)))
    }

    /// The country's `UIName=` string-table key, then its plain `Name=`.
    /// Callers resolve the key through the CSF table; the plain name is the
    /// fallback when there is no key or the key is missing from the table.
    pub fn country_display_name_sources(&self, id: &str) -> (Option<&str>, Option<&str>) {
        match self.country_rules(id) {
            Some(rules) => (rules.ui_name.as_deref(), rules.name.as_deref()),
            None => (None, None),
        }
    }

    /// Case-insensitive country lookup (gamemd parity), exact key first.
    pub(super) fn country_rules(&self, id: &str) -> Option<&CountryRules> {
        let index = self.country_index(id)?;
        self.countries.get(self.country_name(index)?)
    }
}
