//! `ProcessedRulesLayers` core: extension catalogs, object-image load restore,
//! saved-state validation and constructor-event hand-off.

use super::{
    AresGlobalControls, CountryVeterancyState, NativeRulesRegistryState,
    NativeTypeConstructionEvent, ProcessedRulesLayers, ProcessedType, RulesTypeFamily,
    anim_type_runtime_keys, find_or_construct_type,
};
use crate::rules::aircraft_trailer::AircraftTrailer;
use crate::rules::ares_armor::ArmorCatalog;
use crate::rules::ares_prerequisites::GenericPrerequisites;
#[cfg(feature = "phobos")]
use crate::rules::attach_effect::AttachEffectTypeCatalog;
use crate::rules::compatibility::RulesCompatibility;
#[cfg(feature = "phobos")]
use crate::rules::digital_display::DigitalDisplayTypeCatalog;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::is_native_none_type_name;
#[cfg(feature = "phobos")]
use crate::rules::insignia_type::InsigniaTypeCatalog;
use crate::rules::object_image::ImageTheater;
use crate::rules::object_image::ObjectImageKind;
use crate::rules::object_image::ObjectImageState;
use crate::rules::object_type::ObjectCategory;
#[cfg(feature = "phobos")]
use crate::rules::phobos_radiation::RadiationTypeCatalog;
#[cfg(feature = "phobos")]
use crate::rules::phobos_shields::ShieldTypeCatalog;
use std::collections::HashSet;

impl ProcessedRulesLayers {
    pub(crate) fn ares_inputs(&self) -> &crate::rules::ares_switch::AresInputs {
        &self.ares_inputs
    }

    pub(crate) fn kratos_inputs(&self) -> &crate::rules::kratos_switch::KratosInputs {
        &self.kratos_inputs
    }

    pub(crate) fn npatch_inputs(&self) -> &crate::rules::npatch_switch::NPatchInputs {
        &self.npatch_inputs
    }

    pub(crate) fn ares_global_controls(&self) -> &AresGlobalControls {
        &self
            .native_type_construction_trace
            .registry_state()
            .ares_global_controls
    }

    /// Ares definitions have the same retained process lifetime as Types.
    pub(crate) fn ares_prerequisites(&self) -> Option<&GenericPrerequisites> {
        self.native_type_construction_trace
            .registry_state
            .ares_prerequisites
            .as_ref()
    }

    pub(crate) fn ares_armors(&self) -> Option<&ArmorCatalog> {
        self.native_type_construction_trace
            .registry_state
            .ares_armors
            .as_ref()
    }

    /// The Ares `[MouseCursors]` list after the last pass.
    pub(crate) fn ares_mouse_cursors(&self) -> &crate::rules::ares_mouse_cursor::MouseCursors {
        &self
            .native_type_construction_trace
            .registry_state
            .ares_mouse_cursors
    }

    /// Each SuperWeaponType's retained Ares `Cursor`/`NoCursor`.
    pub(crate) fn ares_super_weapon_cursor_states(
        &self,
    ) -> impl Iterator<Item = (&str, crate::rules::ares_mouse_cursor::SuperWeaponCursors)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.ares_super_weapon_cursors
        })
    }

    /// Each TechnoType's retained Ares cursor keys.
    pub(crate) fn ares_cursor_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            crate::rules::ares_mouse_cursor::TechnoCursors,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                member.ares_cursors,
            )
        })
    }

    /// Phobos `[RadiationTypes]` catalog with the same retained process
    /// lifetime as Types.
    #[cfg(feature = "phobos")]
    pub(crate) fn radiation_types(&self) -> Option<&RadiationTypeCatalog> {
        self.native_type_construction_trace
            .registry_state
            .phobos_radiation_types
            .as_ref()
    }

    /// Phobos `[InsigniaTypes]` catalog with the same retained process
    /// lifetime as Types.
    #[cfg(feature = "phobos")]
    pub(crate) fn insignia_types(&self) -> Option<&InsigniaTypeCatalog> {
        self.native_type_construction_trace
            .registry_state
            .phobos_insignia_types
            .as_ref()
    }

    /// Phobos `[ShieldTypes]` catalog with the same retained process lifetime
    /// as Types.
    #[cfg(feature = "phobos")]
    pub(crate) fn shield_types(&self) -> Option<&ShieldTypeCatalog> {
        self.native_type_construction_trace
            .registry_state
            .phobos_shield_types
            .as_ref()
    }

    /// Phobos `[DigitalDisplayTypes]` catalog with the same retained process
    /// lifetime as Types.
    #[cfg(feature = "phobos")]
    pub(crate) fn digital_display_types(&self) -> Option<&DigitalDisplayTypeCatalog> {
        self.native_type_construction_trace
            .registry_state
            .phobos_digital_display_types
            .as_ref()
    }

    /// Phobos `[AttachEffectTypes]` catalog with the same retained process
    /// lifetime as Types.
    #[cfg(feature = "phobos")]
    pub(crate) fn attach_effect_types(&self) -> Option<&AttachEffectTypeCatalog> {
        self.native_type_construction_trace
            .registry_state
            .phobos_attach_effect_types
            .as_ref()
    }

    /// Read-only projection of the retained registry's build capability.
    pub(crate) fn compatibility(&self) -> RulesCompatibility {
        self.native_type_construction_trace
            .registry_state
            .compatibility
    }

    pub(crate) fn native_registry_state(&self) -> &NativeRulesRegistryState {
        self.native_type_construction_trace.registry_state()
    }

    /// Restore the inherited image calls of Terrain, Bullet and TechnoTypes
    /// from their retained fields. RulesLoad675210 never replays ReadINI.
    /// Call once on the incoming owned payload, after its saved digest passes.
    pub(crate) fn restore_object_image_loads(
        &mut self,
        theater_name: &str,
    ) -> Result<(), RulesError> {
        self.validate_saved_state()?;
        let _ares_way =
            crate::rules::ares_switch::read_ares_way(self.native_registry_state().reads_ares_way());
        let theater = ImageTheater::from_name(theater_name);
        let registry = &mut self.native_type_construction_trace.registry_state;
        // Type Load receives fresh resource pointers. Saved borrowed addresses
        // are never swizzled; VoxelType74B8BB reloads its own image instead.
        registry.model_plan = Default::default();
        // Interned predicate order contributes to the retained plan identity.
        // Deserialization creates a new HashMap seed, so rebuild in the same
        // family order every time, before canonicalizing its live roots.
        let mut families: Vec<_> = registry.families.iter_mut().collect();
        families.sort_unstable_by_key(|(family, _)| **family as u8);
        for (&family, members) in families {
            if family.object_category().is_none()
                && !matches!(
                    family,
                    RulesTypeFamily::Terrain
                        | RulesTypeFamily::Projectile
                        | RulesTypeFamily::Overlay
                        | RulesTypeFamily::Animation
                        | RulesTypeFamily::VoxelAnimation
                )
            {
                continue;
            }
            for member in members {
                if let Some(terrain) = member.terrain_type_read_state.as_mut() {
                    terrain.restore_occupy_list();
                }
                let image = &mut member.object_image;
                image.model_slots = Default::default();
                image.clear_shp();
                if family != RulesTypeFamily::Building {
                    image.load_voxel();
                    if family.object_category().is_some()
                        || family == RulesTypeFamily::VoxelAnimation
                    {
                        let input = super::voxel_model_plan::GenericLoadInput {
                            image: image.visual_image().to_owned(),
                            unit: family == RulesTypeFamily::Vehicle,
                            apc: member.native_stored_id == "APC",
                            turret: member.body.read_bool("Turret", false),
                            turret_count: member.weapon_slots.turret_count,
                            gattling: member.is_gattling,
                            no_spawn_alt: member.body.read_bool("NoSpawnAlt", false),
                        };
                        registry
                            .model_plan
                            .load_generic(&mut image.model_slots, &input);
                    }
                }
                // VoxelTypeLoad74B810 has no generic SHP call. Keep its saved
                // +296 flag while its resources follow generic5F8110.
                if family == RulesTypeFamily::VoxelAnimation {
                    continue;
                }
                let kind = match family {
                    RulesTypeFamily::Terrain => ObjectImageKind::Terrain,
                    RulesTypeFamily::Animation => ObjectImageKind::Animation,
                    _ => ObjectImageKind::Ordinary,
                };
                image.load_unowned_shp(kind, theater, None);
                if family == RulesTypeFamily::Building {
                    if let Some((turret, barrel)) =
                        super::voxel_frame_catalog::building_voxel_read_names(&member.body)
                    {
                        registry.model_plan.load_building(
                            &mut image.model_slots,
                            turret.as_deref(),
                            Some(&barrel),
                        );
                    }
                }
                if family == RulesTypeFamily::Animation {
                    if image.theater() {
                        image.load_saved_anim_theater_shp(&member.native_stored_id);
                    } else if image.new_theater() {
                        image.load_anim_shp(
                            &member.native_stored_id,
                            theater,
                            crate::rules::object_image::AnimImagePhase::Theater,
                        );
                    }
                    image.init_anim_theater(&member.native_stored_id, theater);
                }
                if family == RulesTypeFamily::Overlay {
                    image.append_saved_overlay_shp();
                    image.init_overlay_theater();
                }
            }
        }
        let mut families: Vec<_> = registry.families.iter_mut().collect();
        families.sort_unstable_by_key(|(family, _)| **family as u8);
        registry
            .model_plan
            .compact(families.into_iter().flat_map(|(_, members)| {
                members
                    .iter_mut()
                    .map(|member| &mut member.object_image.model_slots)
            }));
        Ok(())
    }

    /// The client atlas regression exercises this same resource operation;
    /// connected saved-resource admission is tested by the persistence owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn restore_object_image_loads_for_test(&mut self, theater: &str) -> Result<(), RulesError> {
        self.restore_object_image_loads(theater)
    }

    /// Project each actual Terrain member, including constructor-only Types.
    /// A same-named type in another family owns a different body and image.
    pub(crate) fn terrain_type_projections(
        &self,
    ) -> impl Iterator<Item = crate::rules::terrain_object_type::TerrainObjectType> + '_ {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Terrain)
            .into_iter()
            .flatten()
            .filter_map(ProcessedType::terrain_projection)
    }

    /// The actual family's admitted Rules history, including empty bodies of
    /// late constructor-only Types; another family's equal ID is independent.
    pub(crate) fn terrain_type_read_sections(&self) -> impl Iterator<Item = (&str, &IniSection)> {
        self.member_states(RulesTypeFamily::Terrain, |member| &member.body)
    }

    /// TerrainReadMap71CAD2 calls FindOrAllocate71E2A0 immediately before
    /// this row's instance constructor. No Rules/ART pass is replayed here.
    pub(crate) fn find_or_allocate_map_terrain_type(
        &mut self,
        incoming: &str,
    ) -> Option<(
        crate::rules::terrain_object_type::TerrainObjectType,
        Option<NativeTypeConstructionEvent>,
    )> {
        if incoming.is_empty() || is_native_none_type_name(incoming) {
            return None;
        }
        let state = &mut self.native_type_construction_trace.registry_state;
        let mut constructors = Vec::new();
        let index = find_or_construct_type(
            &mut state.families,
            &mut state.techno_type_order,
            &mut constructors,
            RulesTypeFamily::Terrain,
            incoming,
        );
        let members = &state.families[&RulesTypeFamily::Terrain];
        let terrain = members[index]
            .terrain_projection()
            .expect("Terrain factory initializes its retained subclass state");
        if !constructors.is_empty() {
            let mut registry = IniSection::new("TerrainTypes".into());
            for (index, member) in members.iter().enumerate() {
                registry.set(&index.to_string(), &member.native_stored_id);
            }
            self.ini.replace_first_section(registry);
            // Leave shared compatibility text intact: e.g. SMOKEY already
            // belongs to Anim. Terrain projects its own empty body above.
        }
        Some((terrain, constructors.pop()))
    }

    /// Ordered immutable Overlay projection. ProcessedType retains the input
    /// fields and resource requests; registry construction performs no reads.
    pub(crate) fn overlay_type_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            i32,
            bool,
            Option<&str>,
            &ObjectImageState,
            Option<&str>,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Overlay)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.overlay_damage_levels,
                    member.overlay_explodes,
                    member.overlay_cell_anim.as_deref(),
                    &member.object_image,
                    member.object_crush_sound.as_deref(),
                )
            })
    }

    pub fn ini(&self) -> &IniFile {
        &self.ini
    }

    /// Transfer constructor chronology once, retaining the same processed
    /// Rules/type authority for save and later process transitions.
    pub(crate) fn take_constructor_events(&mut self) -> (Vec<NativeTypeConstructionEvent>, usize) {
        (
            std::mem::take(&mut self.native_type_construction_trace.events),
            self.native_type_construction_trace
                .allocated_super_weapon_type_count,
        )
    }

    /// RulesLoad675210 and the type hierarchy restore live fields, rather than
    /// executing fresh Full_Init/Process. VERA persists this existing processed
    /// state and rebuilds its RuleSet projection; no constructors or IDs replay.
    /// Reject indexes before any projection can dereference a saved type slot.
    pub(crate) fn validate_saved_state(&self) -> Result<(), RulesError> {
        let registry = &self.native_type_construction_trace.registry_state;
        if !registry.compatibility.matches_build() {
            return Err(RulesError::InvalidSavedState(
                "saved Rules require a different compiled extension capability",
            ));
        }
        // Only a read the Ares way fills these SuperWeaponType fields.
        if !registry.reads_ares_way
            && registry.families.values().flatten().any(|member| {
                member.super_weapon_building_requirements.is_some()
                    || member.generic_super_weapon_warhead.is_some()
                    || member.unit_delivery.is_some()
            })
        {
            return Err(RulesError::InvalidSavedState(
                "saved Rules contain Ares extension state the stock way never reads",
            ));
        }
        if !registry.model_plan.validate() {
            return Err(RulesError::InvalidSavedState("invalid voxel model plan"));
        }
        let families = &registry.families;
        if registry.ares_armors.is_some() != registry.reads_ares_way {
            return Err(RulesError::InvalidSavedState(
                "saved armor catalog differs from the saved Ares mode",
            ));
        }
        if let Some(armors) = &registry.ares_armors {
            if !armors.valid_saved_state()
                || families
                    .values()
                    .flatten()
                    .any(|member| !armors.contains(member.armor_id))
                || families.iter().any(|(&family, members)| {
                    members.iter().any(|member| {
                        if family == RulesTypeFamily::Warhead {
                            member
                                .ares_verses
                                .as_ref()
                                .is_none_or(|verses| !verses.valid_saved_state(armors))
                        } else {
                            member.ares_verses.is_some()
                        }
                    })
                })
            {
                return Err(RulesError::InvalidSavedState("invalid Ares armor registry"));
            }
        } else if families
            .values()
            .flatten()
            .any(|member| member.armor_id.index() >= 11 || member.ares_verses.is_some())
        {
            return Err(RulesError::InvalidSavedState(
                "Ares armor state in retail rules",
            ));
        }
        if families.iter().any(|(&family, members)| {
            members.iter().any(|member| {
                member.country_parent.chars().count() > 24
                    || (family != RulesTypeFamily::Country && !member.country_parent.is_empty())
            })
        }) {
            return Err(RulesError::InvalidSavedState(
                "invalid retained Country parent",
            ));
        }
        let animations = families
            .get(&RulesTypeFamily::Animation)
            .map_or(&[][..], Vec::as_slice);
        let animation_keys: HashSet<_> = anim_type_runtime_keys(
            animations
                .iter()
                .map(|member| member.native_stored_id.as_str()),
        )
        .map(|key| key.to_ascii_uppercase())
        .collect();
        for (&family, members) in &registry.families {
            for member in members {
                if (family != RulesTypeFamily::Aircraft
                    && member.aircraft_trailer != AircraftTrailer::default())
                    || member
                        .aircraft_trailer
                        .animation()
                        .is_some_and(|name| !animation_keys.contains(&name.to_ascii_uppercase()))
                {
                    return Err(RulesError::InvalidSavedState(
                        "Aircraft trailer is outside its type family or AnimType registry",
                    ));
                }
                if (family == RulesTypeFamily::Infantry) != member.infantry_sequence_bank.is_some()
                    || member
                        .infantry_sequence_bank
                        .as_ref()
                        .is_some_and(|bank| !bank.valid_saved_state())
                {
                    return Err(RulesError::InvalidSavedState(
                        "invalid InfantryType sequence bank",
                    ));
                }
                member.object_image.validate_saved_state()?;
                if !self
                    .native_registry_state()
                    .model_plan
                    .valid_slots(&member.object_image.model_slots)
                {
                    return Err(RulesError::InvalidSavedState("invalid voxel model plan"));
                }
                if let Some(terrain) = &member.terrain_type_read_state {
                    terrain.validate_saved_state()?;
                }
                // The Ares way allocates these extensions with their types; the
                // stock way never does.
                if member.factory_owners.is_some()
                    != (registry.reads_ares_way && family.object_category().is_some())
                {
                    return Err(RulesError::InvalidSavedState(
                        "FactoryOwners extension belongs to the wrong type family",
                    ));
                }
                if member.factory_owners.as_ref().is_some_and(|state| {
                    state.allowed.iter().chain(&state.forbidden).any(|country| {
                        usize::from(country.0)
                            >= families.get(&RulesTypeFamily::Country).map_or(0, Vec::len)
                    }) || state.built_at.iter().any(|&building| {
                        usize::try_from(building).map_or(true, |index| {
                            index >= families.get(&RulesTypeFamily::Building).map_or(0, Vec::len)
                        })
                    })
                }) {
                    return Err(RulesError::InvalidSavedState(
                        "factory Country or BuiltAt reference is outside its registry",
                    ));
                }
                if member.explicit_factory.is_some()
                    != (registry.reads_ares_way && family == RulesTypeFamily::Building)
                {
                    return Err(RulesError::InvalidSavedState(
                        "explicit factory extension belongs to the wrong type family",
                    ));
                }
                if member.ares_side.is_some()
                    != (registry.reads_ares_way && family == RulesTypeFamily::Side)
                {
                    return Err(RulesError::InvalidSavedState(
                        "retained Side sidebar extension belongs to the wrong family",
                    ));
                }
                let veteran = &member.country_veterancy;
                if family != RulesTypeFamily::Country
                    && *veteran != CountryVeterancyState::default()
                {
                    return Err(RulesError::InvalidSavedState(
                        "country veterancy stored on another type family",
                    ));
                }
                for (family, references) in [
                    (RulesTypeFamily::Infantry, &veteran.infantry),
                    (RulesTypeFamily::Vehicle, &veteran.units),
                    (RulesTypeFamily::Aircraft, &veteran.aircraft),
                ] {
                    if references.iter().any(|id| {
                        !registry.families.get(&family).is_some_and(|types| {
                            types
                                .iter()
                                .any(|entry| entry.native_stored_id.eq_ignore_ascii_case(id))
                        })
                    }) {
                        return Err(RulesError::InvalidSavedState(
                            "country veteran reference is outside its type registry",
                        ));
                    }
                }
                if veteran.ares.buildings.iter().any(|id| {
                    !registry
                        .families
                        .get(&RulesTypeFamily::Building)
                        .is_some_and(|types| {
                            types
                                .iter()
                                .any(|entry| entry.native_stored_id.eq_ignore_ascii_case(id))
                        })
                }) {
                    return Err(RulesError::InvalidSavedState(
                        "country veteran building is outside its type registry",
                    ));
                }
            }
        }
        if !registry.rules_land.valid_saved_state() {
            return Err(RulesError::InvalidSavedState(
                "land row has invalid derived ground blocking",
            ));
        }
        let overlay_count = families.get(&RulesTypeFamily::Overlay).map_or(0, Vec::len);
        if registry
            .tiberiums
            .iter()
            .any(|member| !member.tiberium_image.valid_saved_state(overlay_count))
        {
            return Err(RulesError::InvalidSavedState(
                "invalid retained Tiberium image binding or counts",
            ));
        }
        registry.validate_techno_type_order()?;
        registry.validate_prerequisites()?;
        registry.validate_radiation_types()?;
        registry.validate_shield_types()?;
        #[cfg(feature = "phobos")]
        registry.validate_digital_display_types()?;
        #[cfg(feature = "phobos")]
        registry.validate_attach_effect_types()?;
        let families = &registry.families;
        let projectile_count = families
            .get(&RulesTypeFamily::Projectile)
            .map_or(0, Vec::len);
        let warhead_count = families.get(&RulesTypeFamily::Warhead).map_or(0, Vec::len);
        let particle_count = families.get(&RulesTypeFamily::Particle).map_or(0, Vec::len);
        if let Some(member) = families
            .values()
            .flatten()
            .find(|member| !member.building_foundation.valid())
        {
            return Err(RulesError::InvalidFoundation {
                type_id: member.native_stored_id.clone(),
            });
        }
        if families.values().flatten().any(|member| {
            // Only a read the Ares way installs a custom foundation or
            // weapon slots past the eighteen.
            (!registry.reads_ares_way
                && (matches!(
                    member.building_foundation,
                    crate::rules::foundation::Foundation::Custom(_)
                        | crate::rules::foundation::Foundation::InvalidCustom
                ) || member.weapon_slots.has_extension_slots()))
                || member
                    .weapon
                    .projectile
                    .is_some_and(|index| index >= projectile_count)
                || member
                    .weapon
                    .warhead
                    .is_some_and(|index| index >= warhead_count)
                || member
                    .holds_what
                    .is_some_and(|index| index >= particle_count)
                || member
                    .particle_next
                    .is_some_and(|index| index >= particle_count)
        }) {
            return Err(RulesError::InvalidSavedState(
                "type reference is outside its registry",
            ));
        }
        Ok(())
    }
}
