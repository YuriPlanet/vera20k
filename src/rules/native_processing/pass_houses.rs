//! `RulesPassProcessor` Country and Side reads (with the Ares country extension)
//! and the SuperWeaponType family.

use super::RulesPassProcessor;
use super::{
    RulesTypeFamily, SideRegisteredTypes, ares_country_index_of_name, house_type_mask_from_indices,
    read_member, read_nullable_type_vector, read_valueable_int_vector, read_valueable_type,
    read_valueable_type_vector,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::sound_ini::EvaRegistry;
use crate::rules::sound_ini::SoundRegistry;
use crate::rules::superweapon_type::DeliveryOwner;
use crate::rules::superweapon_type::DeliveryType;

impl RulesPassProcessor {
    pub(super) fn process_house_family(&mut self, pass: &IniFile, _primary_rules: bool) {
        let ares_way = crate::rules::ares_switch::ares_way();
        // Process668BFE calls Colors66D3A0 before Country allocation668CD1
        // and this first ReadTypeData679A2D loop. Use only names registered
        // now; a later pass cannot resolve an earlier unknown retroactively.
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Country) {
            if let Some(raw) = self.begin_rules_member_read(RulesTypeFamily::Country, index, pass) {
                let member = &mut self
                    .families
                    .get_mut(&RulesTypeFamily::Country)
                    .expect("the live Country exists")[index];
                // ReadINI5118CF..511928 always reads with the current ID
                // default, not the previous parent. Whole-section failure
                // skips this body and retains the prior Country+98.
                member.country_parent =
                    raw.read_string("ParentCountry", &member.native_stored_id, 25);
                member.country_color_scheme = raw.read_color_scheme(
                    "Color",
                    member.country_color_scheme,
                    self.colors.iter().map(|(name, _)| name.as_str()),
                );
                // Original511D16/511E20/511F56: ReadString128 followed by
                // FindOrAllocate. Ares replaces511D16 with1005BE70: the
                // same three lists use2048 and Find-only, in the same order.
                // Missing/cached-empty input retains either reader's list.
                for (key, family) in [
                    ("VeteranInfantry", RulesTypeFamily::Infantry),
                    ("VeteranUnits", RulesTypeFamily::Vehicle),
                    ("VeteranAircraft", RulesTypeFamily::Aircraft),
                ] {
                    let resolved = if ares_way {
                        self.resolve_existing_list_from(&raw, key, family, 0x800)
                    } else {
                        self.resolve_list_from(&raw, key, family, 0x80)
                    };
                    if let Some(resolved) = resolved {
                        let veteran =
                            &mut self.family_mut(RulesTypeFamily::Country)[index].country_veterancy;
                        match family {
                            RulesTypeFamily::Infantry => veteran.infantry = resolved,
                            RulesTypeFamily::Vehicle => veteran.units = resolved,
                            RulesTypeFamily::Aircraft => veteran.aircraft = resolved,
                            _ => unreachable!("Country starting-veterancy family"),
                        }
                    }
                }
                let current = self.families[&RulesTypeFamily::Country][index].country_side_index;
                let side = self.read_side_index(&raw, "Side", current);
                if let Ok(side) = usize::try_from(side) {
                    self.change_country_side(index, side);
                }
            }
            // Both original success51214F and failed-section51215A enter
            // Ares1001D110 ->1001C530. Do not put this suffix inside the
            // original section-admission branch or use final merged text.
            if ares_way {
                self.process_ares_country(index, pass, _primary_rules);
            }
            index += 1;
        }
    }

    /// CountryExt1001C530/1001C5D0: first-attempt parent copy precedes this
    /// pass's VeteranBuildings read1001C6DE/10010850. Actual parent Find
    ///1001BA90 uses stored ID, not the original Country Name alias. The
    /// native controls live in tools/rules_oracle/ares_country_veterancy.*.
    pub(super) fn process_ares_country(
        &mut self,
        index: usize,
        pass: &IniFile,
        primary_rules: bool,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let country = &self.families[&RulesTypeFamily::Country][index];
        let phase = country.country_veterancy.ares.phase;
        if !(0..=2).contains(&phase) {
            return;
        }
        let id = country.native_stored_id.clone();
        let parent = country.country_parent.clone();
        let inherit = !country.country_veterancy.ares.parent_initialized
            && !parent.is_empty()
            && !parent.eq_ignore_ascii_case(&id);
        if phase == 0 {
            //1001C549..1001C59A initializes the color from the current Side.
            // It does not reset any GUI field, list or parent attempt.
            let mut loading = country.country_veterancy.ares.loading.clone();
            loading.initialize_color(country.country_side_index, |name| {
                self.loading_color_index(name)
            });
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .loading = loading;
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .phase = 1;
        }
        // GUI1001C770 runs only for the selected primary Rules object and
        // BEFORE common1001C5D0's one-shot parent copy1001BDC0, which
        // overwrites these loading and presentation fields.
        if primary_rules && let Some(section) = pass.section(&id) {
            let current = &mut self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares;
            current.loading.read_primary(section);
            current.presentation.read_primary(section);
        }
        if inherit {
            let inherited = ares_country_index_of_name(
                &parent,
                self.families[&RulesTypeFamily::Country]
                    .iter()
                    .map(|member| member.native_stored_id.as_str()),
            )
            .map(|index| {
                self.families[&RulesTypeFamily::Country][usize::from(index.0)]
                    .country_veterancy
                    .ares
                    .clone()
            });
            if let Some(inherited) = inherited {
                let current = &mut self.family_mut(RulesTypeFamily::Country)[index]
                    .country_veterancy
                    .ares;
                current.buildings = inherited.buildings;
                current.loading = inherited.loading;
                current.presentation.inherit_from(&inherited.presentation);
                current.paradrop.inherit_from(&inherited.paradrop);
                current.power_plants = inherited.power_plants;
                current.degrades = inherited.degrades;
            }
            //1001C0CB consumes the attempt even when Find failed. Empty
            // and case-insensitive self references above do not consume it.
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .parent_initialized = true;
        }
        // Common1001C5D0's first read after its one-shot parent copy1001BDC0,
        // on every pass: `AI.PowerPlants`1001C60C, through the same Building
        // vector reader10010850 as `VeteranBuildings`. An empty list is what
        // GetPowerplants1001BD20 answers with the side's `[General]` plants.
        if let Some(resolved) = pass.section(&id).and_then(|section| {
            self.resolve_existing_list_from(
                section,
                "AI.PowerPlants",
                RulesTypeFamily::Building,
                0x800,
            )
        }) {
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .power_plants = resolved;
        }
        // Common1001C5D0 reads the paradrop block on every pass, after its
        // one-shot parent copy1001BDC0 and before the color: `Parachute.Anim`
        //1001C620, `ParaDrop.Aircraft`1001C636, `ParaDrop.Types`1001C64C
        // (then remove_non_paradroppables100768C0 keeps InfantryType and
        // UnitType identities) and `ParaDrop.Num`1001C670.
        if let Some(section) = pass.section(&id) {
            let types = SideRegisteredTypes::collect(&self.families);
            let paradrop = &mut self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .paradrop;
            paradrop.parachute_anim = read_valueable_type(
                section,
                "Parachute.Anim",
                &paradrop.parachute_anim,
                |name| SideRegisteredTypes::find(&types.animations, name),
            );
            paradrop.aircraft =
                read_valueable_type(section, "ParaDrop.Aircraft", &paradrop.aircraft, |name| {
                    SideRegisteredTypes::find(&types.aircraft, name)
                });
            paradrop.types =
                read_valueable_type_vector(section, "ParaDrop.Types", &paradrop.types, |name| {
                    types.techno(name)
                });
            paradrop.counts = read_valueable_int_vector(section, "ParaDrop.Num", &paradrop.counts);
        }
        // Color1001C686 is common to every pass and follows inheritance.
        if let Some(section) = pass.section(&id) {
            let mut loading = self.families[&RulesTypeFamily::Country][index]
                .country_veterancy
                .ares
                .loading
                .clone();
            loading.read_color(section, |name| self.loading_color_index(name));
            // Degrades1001C5D0 precedes CanBeDriven.
            let degrades = self.families[&RulesTypeFamily::Country][index]
                .country_veterancy
                .ares
                .degrades;
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .degrades = section.read_ares_nullable_bool("Degrades", degrades);
            let current = self.families[&RulesTypeFamily::Country][index]
                .country_veterancy
                .ares
                .can_be_driven;
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .can_be_driven = section.read_ares_nullable_bool("CanBeDriven", current);
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .loading = loading;
        }
        // Common weights and ListIndex1001C69C..1001C6D9 follow the color and
        // override what the parent copy wrote.
        if let Some(section) = pass.section(&id) {
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .presentation
                .read_common(section);
        }
        if let Some(resolved) = pass.section(&id).and_then(|section| {
            self.resolve_existing_list_from(
                section,
                "VeteranBuildings",
                RulesTypeFamily::Building,
                0x800,
            )
        }) {
            self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares
                .buildings = resolved;
        }
        if let Some(section) = pass.section(&id) {
            // `StartInMultiplayer.Types` (NullableVector<TechnoTypeClass*>
            // reader 1000EA30 over Find 10007EA0, unknown names skipped)
            // precedes WithConst.
            let types = read_nullable_type_vector(
                section,
                "StartInMultiplayer.Types",
                self.families[&RulesTypeFamily::Country][index]
                    .country_veterancy
                    .ares
                    .start_in_multiplayer_types
                    .as_ref(),
                |name| self.find_existing_techno_type(name),
            );
            let current = &mut self.family_mut(RulesTypeFamily::Country)[index]
                .country_veterancy
                .ares;
            current.start_in_multiplayer_types = types;
            current.start_in_multiplayer_with_const = section.read_ares_bool(
                "StartInMultiplayer.WithConst",
                current.start_in_multiplayer_with_const,
            );
            current.gives_bounty = section.read_ares_bool("GivesBounty", current.gives_bounty);
        }
        self.family_mut(RulesTypeFamily::Country)[index]
            .country_veterancy
            .ares
            .phase = 2;
    }

    /// CountryExt1001BB30 finds the current kind1 runtime scheme. Each
    /// retained [Colors] entry contributes kind1 then kind53, in native order.
    /// Phobos's `ColorScheme::FindIndex(name, 1)` makes the same match.
    pub(super) fn loading_color_index(&self, name: &str) -> Option<i32> {
        self.colors
            .iter()
            .position(|(id, _)| id.eq_ignore_ascii_case(name))
            .map(|index| (index as i32).wrapping_mul(2))
    }

    /// HouseType ReadINI51208C..512162 stores BC, then updates Side vectors
    /// only when it changed. The old vector deletes POSITION Country+B8,
    /// not the first matching value; an out-of-count position leaves stale
    /// membership. The new vector always appends that Country index.
    /// Executed histories: tools/input_oracle/campaign_start.py --houses,
    /// campaign_start_houses.json side_controls (+meta).
    fn change_country_side(&mut self, country_index: usize, side_index: usize) {
        let side = i32::try_from(side_index).expect("native Side index exceeds i32");
        let country = &mut self
            .families
            .get_mut(&RulesTypeFamily::Country)
            .expect("the live Country exists")[country_index];
        let prior = country.country_side_index;
        country.country_side_index = side;
        if prior == side {
            return;
        }
        let sides = self
            .families
            .get_mut(&RulesTypeFamily::Side)
            .expect("the new Side exists");
        if prior != -1 {
            let prior = usize::try_from(prior).expect("a bound Country Side is nonnegative");
            let members = &mut sides[prior].side_country_members;
            if country_index < members.len() {
                members.remove(country_index);
            }
        }
        sides[side_index]
            .side_country_members
            .push(i32::try_from(country_index).expect("native Country index exceeds i32"));
    }

    pub(super) fn process_super_weapon_family(
        &mut self,
        pass: &IniFile,
        _fixed_eva: &EvaRegistry,
        _fixed_sounds: &SoundRegistry,
    ) {
        let ares_way = crate::rules::ares_switch::ares_way();
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::SuperWeapon) {
            if let Some(raw) =
                self.begin_rules_member_read(RulesTypeFamily::SuperWeapon, index, pass)
            {
                // `Type=` of every pass so far, as Ares reads it: an Ares
                // read, so the key stays among the Ares inputs saved with the
                // rules. An owned copy: the reads below allocate types into
                // the families the body is read from.
                let kind = ares_way
                    .then(|| {
                        crate::rules::ares_switch::ares_read(|| {
                            self.families[&RulesTypeFamily::SuperWeapon][index]
                                .body
                                .read_name("Type", 0x28)
                                .map(str::to_owned)
                        })
                    })
                    .flatten();
                if let Some(weapon) =
                    self.resolve_scalar_from(&raw, "WeaponType", RulesTypeFamily::Weapon, 0x80)
                {
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index].super_weapon_weapon =
                        weapon;
                }
                if let Some(building) =
                    self.resolve_scalar_from(&raw, "AuxBuilding", RulesTypeFamily::Building, 0x80)
                {
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_aux_building = building;
                }
                #[cfg(feature = "phobos")]
                {
                    // Phobos SWTypeExt reads its three gates in the same
                    // ordered body as the Ares keys: level first, then the two
                    // techno vectors. An absent key retains, a reached empty
                    // list replaces.
                    let mut availability = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_phobos_availability
                        .clone();
                    availability
                        .set_tech_level(raw.read_int("SW.TechLevel", availability.tech_level()));
                    if let Some(aux) =
                        self.resolve_existing_techno_list_from(&raw, "SW.AuxTechnos", 0x800)
                    {
                        availability.set_aux_technos(aux);
                    }
                    if let Some(neg) =
                        self.resolve_existing_techno_list_from(&raw, "SW.NegTechnos", 0x800)
                    {
                        availability.set_neg_technos(neg);
                    }
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_phobos_availability = availability;
                }
                if ares_way {
                    let _ares = crate::rules::ares_switch::ares_reads();
                    // LoadFromINIFile10033250: the first call runs the Type
                    // handler's Initialize, then LoadFromINI10033370 reads
                    // EVA.Ready/Activated/Detected before the other fields.
                    let kind = kind.as_deref();
                    let initialized = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_targeting
                        .read_pass(&raw, kind);
                    let presentation = &mut self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_presentation;
                    if initialized {
                        presentation.eva.initialize_for_kind(kind, _fixed_eva);
                        presentation.texts.initialize_for_kind(kind);
                        // SW_Protect::Initialize10074B60: SW_AnimHeight=5.
                        if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("Protect")) {
                            presentation.height = 5;
                        }
                        presentation.messages.initialize_for_kind(kind);
                        // SW_GeneticMutator::Initialize10072090: AnimHeight 5.
                        if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("GeneticConverter")) {
                            presentation.height = 5;
                        }
                        // SW_ChronoSphere::Initialize1006EF10: the anims show
                        // to the owner's team (3), 5 leptons up.
                        if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("ChronoSphere")) {
                            presentation.height = 5;
                            presentation.visibility =
                                crate::rules::superweapon_targeting::HouseMask::TEAM;
                        }
                    }
                    presentation.eva.read_pass(&raw, _fixed_eva);
                    // The handler's Initialize names its cursor; 10033370
                    // reads Cursor/NoCursor after SW.ChargeToDrainRatio.
                    let cursors =
                        &mut read_member(&mut self.families, RulesTypeFamily::SuperWeapon, index)
                            .ares_super_weapon_cursors;
                    if initialized {
                        cursors.initialize_for_kind(kind);
                    }
                    cursors.read_pass(&raw, &self.ares_mouse_cursors);
                    let animation = self.resolve_existing_type_from(
                        &raw,
                        "SW.Animation",
                        RulesTypeFamily::Animation,
                    );
                    // 10033824 -> 1001CC70.
                    let message_color = raw.read_ares_color_scheme_index(
                        "Message.Color",
                        self.families[&RulesTypeFamily::SuperWeapon][index]
                            .super_weapon_presentation
                            .messages
                            .color_scheme(),
                        |name| self.loading_color_index(name),
                    );
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_presentation
                        .read_pass(&raw, animation, message_color);
                    let generic_warhead =
                        kind.is_some_and(|kind| kind.eq_ignore_ascii_case("GenericWarhead"));
                    // Same SWTypeExt owner and native read order: controller
                    // flags, Country masks, building vectors, then shot limit.
                    let mut availability = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_availability
                        .clone();
                    availability.read_allowance_pass(&raw);
                    let designators =
                        self.resolve_existing_techno_type_list_from(&raw, "SW.Designators");
                    if let Some(designators) = designators {
                        self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .super_weapon_influence
                            .designators = designators;
                    }
                    let influence = &mut self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_influence;
                    influence.any_designator =
                        raw.read_ares_bool("SW.AnyDesignator", influence.any_designator);
                    let inhibitors =
                        self.resolve_existing_techno_type_list_from(&raw, "SW.Inhibitors");
                    if let Some(inhibitors) = inhibitors {
                        self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .super_weapon_influence
                            .inhibitors = inhibitors;
                    }
                    let influence = &mut self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_influence;
                    influence.any_inhibitor =
                        raw.read_ares_bool("SW.AnyInhibitor", influence.any_inhibitor);
                    availability.read_house_pass(|key, prior| {
                        raw.read_list(key, 128).map_or(prior, |tokens| {
                            house_type_mask_from_indices(
                                tokens
                                    .into_iter()
                                    .map(|token| self.country_index_of_name(token)),
                            )
                        })
                    });
                    // Ares ValueableVector<BuildingTypeClass*> uses Find, not
                    // FindOrAllocate (Utilities/TemplateDef.h::parse_values and
                    // Utilities/Parser.h::Parser<T,1>, Ares 4f1d9299). Unknown
                    // tokens are skipped; each nonempty read replaces the list.
                    let auxiliary = self.resolve_existing_list_from(
                        &raw,
                        "SW.AuxBuildings",
                        RulesTypeFamily::Building,
                        0x800,
                    );
                    let negative = self.resolve_existing_list_from(
                        &raw,
                        "SW.NegBuildings",
                        RulesTypeFamily::Building,
                        0x800,
                    );
                    if auxiliary.is_some() || negative.is_some() {
                        let requirements = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .super_weapon_building_requirements
                            .get_or_insert_default();
                        if let Some(auxiliary) = auxiliary {
                            requirements.auxiliary = auxiliary;
                        }
                        if let Some(negative) = negative {
                            requirements.negative = negative;
                        }
                    }
                    // SW_HunterSeeker::LoadFromINI10072A30 reads HunterSeeker.Type,
                    // .RandomOnly and .Buildings; the common SWTypeExt reader
                    // reads SW.MaxCount at10033682. Scalar unknowns retain; a
                    // reached vector replaces.
                    if kind.is_some_and(|kind| kind.eq_ignore_ascii_case("HunterSeeker")) {
                        let unit = self.resolve_existing_type_from(
                            &raw,
                            "HunterSeeker.Type",
                            RulesTypeFamily::Vehicle,
                        );
                        let buildings = self.resolve_existing_list_from(
                            &raw,
                            "HunterSeeker.Buildings",
                            RulesTypeFamily::Building,
                            2048,
                        );
                        let settings = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .hunter_seeker
                            .get_or_insert_default();
                        settings.max_count = raw.read_ares_int("SW.MaxCount", settings.max_count);
                        if let Some(unit) = unit {
                            settings.unit_type = unit;
                        }
                        settings.random_only =
                            raw.read_ares_bool("HunterSeeker.RandomOnly", settings.random_only);
                        if let Some(buildings) = buildings {
                            settings.buildings = buildings;
                        }
                    }
                    availability.read_shots_pass(&raw);
                    // SW.CreateRadarEvent, SW.Range and SW.Group, then the
                    // Protect/Battery LoadFromINI (10074C20/1006ECC0) after
                    // the common reads; Battery's vectors use Find only.
                    let is = |name: &str| kind.is_some_and(|kind| kind.eq_ignore_ascii_case(name));
                    let mut reads = crate::rules::superweapon_type::AresHandlerReads::default();
                    if is("Battery") {
                        reads.battery_keep_online = self.resolve_existing_list_from(
                            &raw,
                            "Battery.KeepOnline",
                            RulesTypeFamily::Building,
                            0x800,
                        );
                        reads.battery_overpower = self.resolve_existing_list_from(
                            &raw,
                            "Battery.Overpower",
                            RulesTypeFamily::Building,
                            0x800,
                        );
                    } else if is("ChronoSphere") {
                        // 1006EF90: Nullable<AnimTypeClass*>, Find only.
                        reads.chrono_blasts = ["Chronosphere.BlastSrc", "Chronosphere.BlastDest"]
                            .map(|key| {
                                self.resolve_existing_type_from(
                                    &raw,
                                    key,
                                    RulesTypeFamily::Animation,
                                )
                            });
                    } else if is("PsychicDominator") {
                        // 10070A40: Nullable<AnimTypeClass*>, Find only.
                        reads.dominator_anims = [
                            "Dominator.FirstAnim",
                            "Dominator.SecondAnim",
                            "Dominator.ControlAnim",
                        ]
                        .map(|key| {
                            self.resolve_existing_type_from(&raw, key, RulesTypeFamily::Animation)
                        });
                    } else if is("MultiMissile") {
                        // SW_NuclearMissile::Initialize10073360 allocates its
                        // payload weapon and finds PSIWARN; LoadFromINI
                        // 10073410's reads look the types up only (1000A370).
                        if initialized {
                            let payload = self
                                .find_or_allocate(RulesTypeFamily::Weapon, "NukePayload")
                                .map(|index| {
                                    self.resolved_type_key(RulesTypeFamily::Weapon, index)
                                });
                            let psi_warning = self
                                .find_existing(
                                    RulesTypeFamily::Animation,
                                    crate::rules::effect_asset_catalog::PSI_WARNING_ANIM,
                                )
                                .map(|index| {
                                    self.resolved_type_key(RulesTypeFamily::Animation, index)
                                });
                            reads.nuke_defaults = Some((payload, psi_warning));
                        }
                        reads.nuke = [
                            ("Nuke.Payload", RulesTypeFamily::Weapon),
                            ("Nuke.TakeOff", RulesTypeFamily::Animation),
                            ("Nuke.PsiWarning", RulesTypeFamily::Animation),
                        ]
                        .map(|(key, family)| self.resolve_existing_type_from(&raw, key, family));
                    } else if is("SpyPlane") {
                        // SW_SpyPlane::Initialize10075800 and LoadFromINI
                        // 10075880: AircraftTypeClass::FindIndex, never an
                        // allocation.
                        if initialized {
                            reads.spy_plane_default =
                                Some(self.find_existing(RulesTypeFamily::Aircraft, "SPYP").map(
                                    |index| {
                                        self.resolved_type_key(RulesTypeFamily::Aircraft, index)
                                    },
                                ));
                        }
                        reads.spy_plane_type = self.resolve_existing_type_from(
                            &raw,
                            "SpyPlane.Type",
                            RulesTypeFamily::Aircraft,
                        );
                    } else if is("LightningStorm") {
                        // SW_LightningStorm::LoadFromINI10072CF0: Nullable
                        // and NullableVector<AnimTypeClass*> reads use Find
                        // (1002B4D0, 10072F30), the sounds VocClass::FindIndex
                        // (10072E70); none allocates.
                        reads.lightning.bolt_explosion = self.resolve_existing_type_from(
                            &raw,
                            "Lightning.BoltExplosion",
                            RulesTypeFamily::Animation,
                        );
                        let current = self.families[&RulesTypeFamily::SuperWeapon][index]
                            .super_weapon_launch
                            .lightning()
                            .clone();
                        let anims = |key, current: &Option<Vec<String>>| {
                            read_nullable_type_vector(&raw, key, current.as_ref(), |name| {
                                self.find_existing(RulesTypeFamily::Animation, name)
                                    .map(|index| {
                                        self.resolved_type_key(RulesTypeFamily::Animation, index)
                                    })
                            })
                        };
                        reads.lightning.lists = Some([
                            anims("Lightning.Clouds", &current.clouds),
                            anims("Lightning.Bolts", &current.bolts),
                            anims("Lightning.Debris", &current.debris),
                            read_nullable_type_vector(
                                &raw,
                                "Lightning.Sounds",
                                current.sounds.as_ref(),
                                |name| _fixed_sounds.get(name).map(|entry| entry.id.clone()),
                            ),
                        ]);
                    } else if is("EMPulse") {
                        // SW_EMPulse::LoadFromINI10071510: Nullable<AnimTypeClass*>
                        // and ValueableVector<BuildingTypeClass*>, both Find.
                        reads.emp_pulse_ball = self.resolve_existing_type_from(
                            &raw,
                            "EMPulse.PulseBall",
                            RulesTypeFamily::Animation,
                        );
                        reads.emp_pulse_cannons = self.resolve_existing_list_from(
                            &raw,
                            "EMPulse.Cannons",
                            RulesTypeFamily::Building,
                            0x800,
                        );
                    } else if is("DropPod") {
                        // SW_DropPod::LoadFromINI10071140:
                        // ValueableVector<TechnoTypeClass*>, Find.
                        reads.drop_pod_types =
                            self.resolve_existing_techno_type_list_from(&raw, "DropPod.Types");
                    } else if is("ParaDrop") || is("AmerParaDrop") {
                        // SW_ParaDrop::Initialize10073DC0 gives an AmerParaDrop
                        // a default plane of Rules' AmerParaDropInf/Num as
                        // this pass's ReadGeneral left them. LoadFromINI
                        // 10073F70 reads Find-only: the plane through
                        // detail::read (AircraftTypeClass::Find), the types
                        // through ValueableVector<TechnoTypeClass*> 1000EDD0.
                        let mut paradrop = self.families[&RulesTypeFamily::SuperWeapon][index]
                            .super_weapon_launch
                            .paradrop()
                            .clone();
                        if initialized && is("AmerParaDrop") {
                            let types = self.general_type_lists.paradrop_infantry[0]
                                .iter()
                                .map(|id| crate::rules::superweapon_type::TechnoTypeReference {
                                    category: ObjectCategory::Infantry,
                                    id: id.clone(),
                                })
                                .collect();
                            let counts = self
                                .ordinary
                                .as_ref()
                                .and_then(|ini| ini.section("General"))
                                .and_then(|general| {
                                    crate::rules::ares_switch::not_ares_read(|| {
                                        general.read_ares_int_list("AmerParaDropNum")
                                    })
                                })
                                .unwrap_or_default();
                            paradrop.initialize_american(types, counts);
                        }
                        let sides: Vec<String> = self
                            .families
                            .get(&RulesTypeFamily::Side)
                            .into_iter()
                            .flatten()
                            .map(|side| side.native_stored_id.clone())
                            .collect();
                        let countries: Vec<(String, i32)> = self
                            .families
                            .get(&RulesTypeFamily::Country)
                            .into_iter()
                            .flatten()
                            .map(|country| {
                                (country.native_stored_id.clone(), country.country_side_index)
                            })
                            .collect();
                        paradrop.read_pass(
                            &raw,
                            &sides,
                            &countries,
                            |key| {
                                self.resolve_existing_type_from(
                                    &raw,
                                    key,
                                    RulesTypeFamily::Aircraft,
                                )
                            },
                            |key| self.resolve_existing_techno_type_list_from(&raw, key),
                        );
                        reads.paradrop = Some(paradrop);
                    }
                    // 10033670: Nullable<WarheadTypeClass*> through the
                    // Warhead factory, for every type.
                    reads.warhead = self.resolve_scalar_from(
                        &raw,
                        "SW.Warhead",
                        RulesTypeFamily::Warhead,
                        0x80,
                    );
                    let generic_warhead_read = reads.warhead.clone();
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_launch
                        .read_pass(&raw, kind, initialized, reads);
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_charge
                        .read_pass(&raw);
                    self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                        .super_weapon_availability = availability;
                    if generic_warhead && let Some(warhead) = generic_warhead_read {
                        self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .generic_super_weapon_warhead = warhead;
                    }
                    if raw
                        .read_name("Type", 0x28)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("UnitDelivery"))
                        || self.families[&RulesTypeFamily::SuperWeapon][index]
                            .unit_delivery
                            .is_some()
                    {
                        let types = raw.read_list("Deliver.Types", 2048).map(|tokens| {
                            tokens
                                .into_iter()
                                .filter_map(|token| {
                                    let matches: Vec<_> = [
                                        (RulesTypeFamily::Infantry, ObjectCategory::Infantry),
                                        (RulesTypeFamily::Vehicle, ObjectCategory::Vehicle),
                                        (RulesTypeFamily::Aircraft, ObjectCategory::Aircraft),
                                        (RulesTypeFamily::Building, ObjectCategory::Building),
                                    ]
                                    .into_iter()
                                    .filter_map(|(family, category)| {
                                        self.find_existing(family, token).map(|index| {
                                            DeliveryType {
                                                category,
                                                id: self.families[&family][index]
                                                    .native_stored_id
                                                    .clone(),
                                            }
                                        })
                                    })
                                    .collect();
                                    // Do not silently bind an ambiguous cross-category ID
                                    // to an unrelated type. Unknown/none tokens add none.
                                    (matches.len() == 1)
                                        .then(|| matches.into_iter().next().unwrap())
                                })
                                .collect()
                        });
                        let state = self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                            .unit_delivery
                            .get_or_insert_default();
                        if let Some(types) = types {
                            state.types = types;
                        }
                        state.base_normal = raw.read_bool("Deliver.BaseNormal", state.base_normal);
                        if let Some(owner) = raw.read_name("Deliver.Owner", 2048) {
                            state.owner = match owner.to_ascii_lowercase().as_str() {
                                "invoker" => DeliveryOwner::Invoker,
                                "neutral" => DeliveryOwner::Neutral,
                                "special" => DeliveryOwner::Special,
                                "civilian" => DeliveryOwner::Civilian,
                                _ => state.owner,
                            };
                        }
                        // Activation sound resolves against the fixed SOUNDMD catalog
                        // in RuleSet::bind_type_sound_references, using this type's
                        // retained INI read history rather than an unresolved name.
                    }
                }
            } else if ares_way {
                self.family_mut(RulesTypeFamily::SuperWeapon)[index]
                    .super_weapon_targeting
                    .skip_pass();
            }
            index += 1;
        }
    }
}
