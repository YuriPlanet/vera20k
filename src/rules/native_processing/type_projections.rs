//! `ProcessedRulesLayers` per-Type projections: the read states of the live
//! Techno, Building, Infantry, Weapon, Warhead, SuperWeapon and Anim registries.

use super::ProcessedRulesLayers;
use super::{
    AnimTypeReferences, AresExplicitFactory, AresFactoryOwners, ProcessedType, RulesTypeFamily,
    TechnoTypeReferences, UrbanTypeKind, VoxelAnimTypeReferences, WeaponSlots, WeaponState,
};
use crate::rules::aircraft_trailer::AircraftTrailer;
use crate::rules::ares_armor::AresWarheadVerses;
use crate::rules::armor::ArmorId;
use crate::rules::gunner_turrets::GunnerTurrets;
use crate::rules::ini_parser::IniSection;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_image::ObjectImageState;
use crate::rules::object_type::InfantryArt;
use crate::rules::object_type::InfantryInfiltration;
use crate::rules::object_type::InfantryPips;
use crate::rules::object_type::ObjectCategory;
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::rules_type_references::RulesTypeReferences;
use crate::rules::superweapon_type::AresSuperWeaponMetadata;
use crate::rules::superweapon_type::AresSuperWeaponPresentation;
#[cfg(feature = "phobos")]
use crate::rules::superweapon_type::PhobosSuperWeaponAvailability;
use crate::rules::superweapon_type::SuperWeaponBuildingRequirements;
use crate::rules::superweapon_type::UnitDeliveryRules;
use crate::util::native_x87::NativeF64Bits;

impl ProcessedRulesLayers {
    /// The live global TechnoType array, not a concatenation of the four
    /// subtype arrays. Constructors71181D/711ABD append each new identity.
    /// TAction106's active inline body6DF5D1 scans this order backwards.
    pub(crate) fn techno_type_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .techno_type_states()
    }

    /// Allocated AnimTypes and whether their fixed-ART body has actually been
    /// read, in native registry order. A present ART section alone is not
    /// evidence of a read: later Type readers can allocate after the Anim sweep.
    pub(crate) fn anim_type_art_read_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .anim_type_art_read_states()
    }

    /// Retained Image and reached resource requests of the real AnimTypes,
    /// including ctor-only types and the saved Load428800 boundary.
    pub(crate) fn anim_type_image_states(&self) -> impl Iterator<Item = (&str, &ObjectImageState)> {
        self.member_states(RulesTypeFamily::Animation, |member| &member.object_image)
    }

    /// Retained Bullet/ObjectType Image and ART fields after each native pass.
    pub(crate) fn projectile_art_states(
        &self,
    ) -> impl Iterator<Item = (&str, &ProjectileArtState, &ObjectImageState)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Projectile)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    &member.projectile_art,
                    &member.object_image,
                )
            })
    }

    /// BuildingType+16B0/EA4/EA8/EAC retained by its original reached body.
    pub(crate) fn secret_lab_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::secret_lab::SecretLabRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.secret_lab))
    }

    pub(crate) fn secret_lab_choices(&self) -> &[Vec<String>; 3] {
        &self.general_type_lists().secret_lab_choices
    }

    /// BuildingType +EF0 after each reached ReadINI body, retained with its
    /// native process registry rather than reconstructed from merged Rules keys.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn building_foundation_states(&self) -> impl Iterator<Item = (&str, u8)> {
        self.building_foundation_geometries()
            .map(|(name, foundation)| (name, foundation.selector()))
    }

    pub(crate) fn building_foundation_geometries(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::foundation::Foundation)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    &member.building_foundation,
                )
            })
    }

    /// BuildingType+154B after each reached Rules body; current-default ReadBool.
    pub(crate) fn building_spotlight_flags(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_has_spotlight,
                )
            })
    }

    /// BuildingType+E30..E40 after each admitted Rules section, including
    /// missing-key integer-default conversions460C8C..460DCA.
    pub(crate) fn building_light_states(
        &self,
    ) -> impl Iterator<Item = (&str, crate::rules::building_light::BuildingLightRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.building_light))
    }

    /// BuildingType's retained upgrade inputs after the last reached Rules body.
    pub(crate) fn building_upgrade_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::building_upgrade::BuildingUpgradeRules)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.building_upgrade))
    }

    /// BuildingType+EE8/+EEC after the last reached Rules body.
    pub(crate) fn building_extra_power_states(
        &self,
    ) -> impl Iterator<Item = (&str, crate::rules::building_extra_power::BuildingExtraPower)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_extra_power,
                )
            })
    }

    /// BuildingType+EBC after each reached Rules body. Fixed ART never reads it.
    pub(crate) fn building_target_coord_offsets(&self) -> impl Iterator<Item = (&str, [i32; 3])> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_target_coord_offset,
                )
            })
    }

    /// BuildingType's retained HalfDamageSmokeLocation1/2 after the last
    /// admitted Rules body, projected from the same native type owner.
    pub(crate) fn building_half_damage_smoke_locations(
        &self,
    ) -> impl Iterator<Item = (&str, [i32; 3], [i32; 3])> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.building_half_damage_smoke_location1,
                    member.building_half_damage_smoke_location2,
                )
            })
    }

    /// Unit747620's retained frame fields after the last reached Rules body.
    /// Missing Rules sections skip ART; the fixed ART itself is never layered.
    pub(crate) fn unit_shp_read_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::shp_vehicle_sequence::UnitShpReadState)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Vehicle)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), &member.unit_shp))
    }

    /// UnitType+67C after the last admitted Rules body; an unread type keeps
    /// constructor -1. Missing sections and missing SpeedType keys differ.
    pub(crate) fn unit_speed_type_states(&self) -> impl Iterator<Item = (&str, SpeedType)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Vehicle)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.unit_speed_type))
    }

    /// TechnoType +810/+814 state after the reached generic and FV readers.
    /// The FV pairs have literal defaults and overlapping writes, so merged
    /// source keys cannot reconstruct this process-resident result.
    pub(crate) fn gunner_turret_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &GunnerTurrets)> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        [
            RulesTypeFamily::Building,
            RulesTypeFamily::Aircraft,
            RulesTypeFamily::Vehicle,
            RulesTypeFamily::Infantry,
        ]
        .into_iter()
        .flat_map(move |family| {
            let category = family.object_category().expect("TechnoType family");
            families
                .get(&family)
                .into_iter()
                .flatten()
                .map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.gunner_turrets,
                    )
                })
        })
    }

    /// TechnoType+CD5/CD8..D10 after the last admitted ReadINI712170 body.
    pub(crate) fn gattling_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            bool,
            &crate::rules::gattling_type::GattlingStages,
        ),
    > {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        [
            RulesTypeFamily::Building,
            RulesTypeFamily::Aircraft,
            RulesTypeFamily::Vehicle,
            RulesTypeFamily::Infantry,
        ]
        .into_iter()
        .flat_map(move |family| {
            let category = family.object_category().expect("TechnoType family");
            families
                .get(&family)
                .into_iter()
                .flatten()
                .map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.is_gattling,
                        &member.gattling_stages,
                    )
                })
        })
    }

    /// Final retained InfantryType state; the merged present-key history cannot
    /// represent admitted missing-Pip calls. Projection performs no new read.
    pub(crate) fn infantry_pip_states(&self) -> impl Iterator<Item = (&str, InfantryPips)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.infantry_pips))
    }

    /// InfantryType+EC1 after every admitted own-ID ReadINI52455F call.
    pub(crate) fn infantry_civilian_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.infantry_civilian))
    }

    /// TechnoType+C8F of each InfantryType after the last pass.
    pub(crate) fn infantry_damage_spark_states(&self) -> impl Iterator<Item = (&str, bool)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.infantry_damage_sparks,
                )
            })
    }

    /// Each InfantryType's fixed-ART reads after every reached pass.
    pub(crate) fn infantry_art_states(&self) -> impl Iterator<Item = (&str, InfantryArt)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| (member.native_stored_id.as_str(), member.infantry_art))
    }

    /// Each native InfantryType's retained ReadSequenceData523D00 bank.
    pub(crate) fn infantry_sequence_banks(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::infantry_sequence::InfantrySequenceBank)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .filter_map(|member| {
                Some((
                    member.native_stored_id.as_str(),
                    member.infantry_sequence_bank.as_ref()?,
                ))
            })
    }

    /// Infantry52466E's postprocessed flags, including a prior role's latch
    /// when a later pass omits Infiltrate while clearing the role itself.
    pub(crate) fn infantry_infiltration_states(
        &self,
    ) -> impl Iterator<Item = (&str, InfantryInfiltration)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Infantry)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.infantry_infiltration,
                )
            })
    }

    /// Native715320 recopies the turret controls on every reached rules pass.
    pub(crate) fn recoil_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, crate::rules::recoil::RecoilConfig)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members
                    .iter()
                    .map(move |member| (category, member.native_stored_id.as_str(), member.recoil))
            })
    }

    /// Retained six-field TechnoType71458A Airstrike reader state.
    pub(crate) fn airstrike_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::airstrike::AirstrikeSettings,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.airstrike,
                    )
                })
            })
    }

    /// TechnoType+6A4 after every reached ReadINI7147B4 pass.
    pub(crate) fn radial_fire_segments_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.radial_fire_segments,
                    )
                })
            })
    }

    /// TechnoType+3DC after each reached ReadInt714145..714159.
    pub(crate) fn lepton_mind_control_offset_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.lepton_mind_control_offset,
                    )
                })
            })
    }

    /// TechnoType's TurretCount, WeaponCount and base/elite weapon arrays
    /// after every reached ReadINI pass.
    pub(crate) fn weapon_slot_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &WeaponSlots)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.weapon_slots,
                    )
                })
            })
    }

    /// Ares insignia state after every reached TechnoType ReadINI pass.
    pub(crate) fn techno_ares_insignia_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::insignia_type::AresInsignia,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.ares_insignia,
                    )
                })
            })
    }

    /// Phobos insignia state after every reached TechnoType ReadINI pass.
    #[cfg(feature = "phobos")]
    pub(crate) fn techno_insignia_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::insignia_type::PhobosInsignia,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.phobos_insignia,
                    )
                })
            })
    }

    /// Project every actual allocation in native array order, including empty
    /// IDs and constructor-only types. References remain array indices; a
    /// truncated duplicate name must not collapse either allocated object.
    pub(crate) fn particle_type_projections(
        &self,
    ) -> impl Iterator<Item = crate::rules::particle_type::ParticleType> {
        use crate::rules::particle_type::{ParticleType, ParticleTypeId};
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Particle)
            .into_iter()
            .flatten()
            .map(|member| {
                ParticleType::from_read_state(
                    &member.native_stored_id,
                    &member.body,
                    &member.particle_read,
                    member
                        .particle_next
                        .map(|index| ParticleTypeId(index as u32)),
                    member.particle_warhead.clone(),
                )
            })
    }

    pub(crate) fn particle_system_type_projections(
        &self,
    ) -> impl Iterator<Item = crate::rules::particle_system_type::ParticleSystemType> {
        use crate::rules::{
            particle_system_type::ParticleSystemType, particle_type::ParticleTypeId,
        };
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::ParticleSystem)
            .into_iter()
            .flatten()
            .map(|member| {
                ParticleSystemType::from_read_state(
                    &member.native_stored_id,
                    &member.body,
                    member.particle_system_behavior,
                    member.holds_what.map(|index| ParticleTypeId(index as u32)),
                )
            })
    }

    /// Each WeaponType's retained reads after the last Weapon sweep, in the
    /// actual WeaponType array order. References remain native family indices
    /// until this read-only projection.
    pub(crate) fn weapon_states(&self) -> impl Iterator<Item = WeaponState<'_>> {
        let families = &self
            .native_type_construction_trace
            .registry_state()
            .families;
        families
            .get(&RulesTypeFamily::Weapon)
            .into_iter()
            .flatten()
            .map(move |member| {
                let name = |family: RulesTypeFamily, index: usize| {
                    families[&family][index].native_stored_id.as_str()
                };
                WeaponState {
                    id: member.native_stored_id.as_str(),
                    damage: member.weapon.damage,
                    speed: member.weapon.speed,
                    range_leptons: member.weapon.range_leptons,
                    projectile: member
                        .weapon
                        .projectile
                        .map(|index| name(RulesTypeFamily::Projectile, index)),
                    warhead: member
                        .weapon
                        .warhead
                        .map(|index| name(RulesTypeFamily::Warhead, index)),
                    anim: &member.weapon.anim,
                    assault_anim: member.weapon.assault_anim.as_deref(),
                    occupant_anim: member.weapon.occupant_anim.as_deref(),
                    open_topped_anim: member.weapon.open_topped_anim.as_deref(),
                    attached_particle_system: member.weapon.attached_particle_system.as_deref(),
                    ares_bolt_particle_system: &member.weapon.ares_bolt_particle_system,
                    ares_ivan_bomb_warhead: &member.weapon.ares_ivan_bomb_warhead,
                    ares_abductor_anim: member.weapon.ares_abductor_anim.as_deref(),
                    ares_cursors: member.weapon.ares_cursors,
                }
            })
    }

    pub(crate) fn voxel_model_plan(&self) -> &super::voxel_model_plan::VoxelModelPlan {
        &self.native_registry_state().model_plan
    }

    pub(crate) fn voxel_anim_type_projections(
        &self,
    ) -> impl Iterator<Item = crate::rules::voxel_anim_type::VoxelAnimType> {
        self.native_registry_state()
            .families
            .get(&RulesTypeFamily::VoxelAnimation)
            .into_iter()
            .flatten()
            .map(|member| {
                let mut kind = if member.voxel_anim_read {
                    crate::rules::voxel_anim_type::VoxelAnimType::from_ini_section(
                        &member.native_stored_id,
                        &member.body,
                    )
                } else {
                    crate::rules::voxel_anim_type::VoxelAnimType::with_defaults(
                        &member.native_stored_id,
                    )
                };
                kind.image = member.object_image.image().to_owned();
                [
                    kind.share_body_data,
                    kind.share_turret_data,
                    kind.share_barrel_data,
                ] = member.voxel_anim_share_flags;
                [kind.min_angular_velocity, kind.max_angular_velocity] =
                    member.voxel_anim_angular_velocities;
                kind.model_slots = member.object_image.model_slots.clone();
                kind
            })
    }

    /// Each TechnoType's Image and pointer fields after every reached
    /// ReadINI pass.
    pub(crate) fn techno_reference_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &TechnoTypeReferences,
            &ObjectImageState,
            Option<&str>,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        &member.techno_references,
                        &member.object_image,
                        member.object_crush_sound.as_deref(),
                    )
                })
            })
    }

    /// AircraftType+E00/+E04 retained by each admitted fixed-ART read.
    pub(crate) fn aircraft_trailer_states(&self) -> impl Iterator<Item = (&str, &AircraftTrailer)> {
        self.member_states(RulesTypeFamily::Aircraft, |member| &member.aircraft_trailer)
    }

    /// TechnoRead712170 and ReadTypeData679A10 visit each concrete object
    /// independently. Preserve its own admitted passes even when another
    /// category has the same stored ID in the compatibility INI.
    pub(crate) fn techno_type_read_section(
        &self,
        category: ObjectCategory,
        index: i32,
    ) -> Option<&IniSection> {
        self.native_registry_state()
            .families
            .get(&RulesTypeFamily::from_object_category(category))?
            .get(usize::try_from(index).ok()?)
            .map(|member| &member.body)
    }

    /// Already bound Country identities from each reached TechnoTypeExt read.
    /// Only Rules read the Ares way have them.
    pub(crate) fn factory_owner_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &AresFactoryOwners)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().filter_map(move |member| {
                    member
                        .factory_owners
                        .as_ref()
                        .map(|state| (category, member.native_stored_id.as_str(), state))
                })
            })
    }

    /// Each BuildingType's Ares factory keys; only Rules read the Ares way
    /// have them.
    pub(crate) fn explicit_factory_states(
        &self,
    ) -> impl Iterator<Item = (&str, &AresExplicitFactory)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Building)
            .into_iter()
            .flatten()
            .filter_map(|member| {
                member
                    .explicit_factory
                    .as_ref()
                    .map(|state| (member.native_stored_id.as_str(), state))
            })
    }

    /// TechnoType+7FC after each reached fixed-ART ShadowIndex read.
    pub(crate) fn voxel_shadow_states(&self) -> impl Iterator<Item = (ObjectCategory, &str, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.voxel_shadow_index,
                    )
                })
            })
    }

    /// Each SuperWeaponType's `WeaponType=` (+9C) and `AuxBuilding=` (+C8)
    /// after every reached pass.
    pub(crate) fn super_weapon_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, (Option<&str>, Option<&str>))> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            (
                member.super_weapon_weapon.as_deref(),
                member.super_weapon_aux_building.as_deref(),
            )
        })
    }

    pub(crate) fn super_weapon_building_requirements(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&SuperWeaponBuildingRequirements>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.super_weapon_building_requirements.as_ref()
        })
    }

    pub(crate) fn super_weapon_availability_states(
        &self,
    ) -> impl Iterator<Item = (&str, AresSuperWeaponMetadata)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            AresSuperWeaponMetadata::project(
                &member.super_weapon_availability,
                member.super_weapon_aux_building.as_deref(),
                member.super_weapon_building_requirements.as_ref(),
                &member.super_weapon_charge,
                &member.super_weapon_influence,
                member.super_weapon_targeting,
                &member.super_weapon_launch,
            )
        })
    }

    pub(crate) fn super_weapon_presentation_states(
        &self,
    ) -> impl Iterator<Item = (&str, &AresSuperWeaponPresentation)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            &member.super_weapon_presentation
        })
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn phobos_super_weapon_availability_states(
        &self,
    ) -> impl Iterator<Item = (&str, PhobosSuperWeaponAvailability)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.super_weapon_phobos_availability.clone()
        })
    }

    pub(crate) fn additional_super_weapon_states(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.member_states(RulesTypeFamily::Building, |member| {
            member.additional_super_weapons.as_slice()
        })
    }

    /// Phobos `UnitTypeExt`'s `OreGathering.*` vectors, one per VehicleType.
    #[cfg(feature = "phobos")]
    pub(crate) fn ore_gathering_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::object_type::PhobosOreGathering)> {
        self.member_states(RulesTypeFamily::Vehicle, |member| &member.ore_gathering)
    }

    /// Ares GenericWarhead references, resolved by the same ordered type
    /// allocator as native references. Kept separate from the retail fields.
    pub(crate) fn generic_super_weapon_warhead_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.generic_super_weapon_warhead.as_deref()
        })
    }

    pub(crate) fn hunter_seeker_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            Option<&crate::rules::hunter_seeker::HunterSeekerSettings>,
        ),
    > {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.hunter_seeker.as_ref()
        })
    }

    pub(crate) fn unit_delivery_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&UnitDeliveryRules>)> {
        self.member_states(RulesTypeFamily::SuperWeapon, |member| {
            member.unit_delivery.as_ref()
        })
    }

    /// Each VoxelAnimType's references after every reached pass.
    pub(crate) fn voxel_anim_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, &VoxelAnimTypeReferences, i32)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::VoxelAnimation)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    &member.voxel_anim_references,
                    member.voxel_anim_damage,
                )
            })
    }

    /// Each AnimType's fixed-ART references, in AnimType registry order; a
    /// type whose ART body was never read keeps the constructor's nulls.
    pub(crate) fn anim_type_reference_states(
        &self,
    ) -> impl Iterator<Item = (&str, &AnimTypeReferences)> {
        self.member_states(RulesTypeFamily::Animation, |member| &member.anim_references)
    }

    /// Each BulletType's retained child weapons (+2B0/+2B4), in native read order.
    pub(crate) fn projectile_weapon_states(
        &self,
    ) -> impl Iterator<Item = (&str, (Option<&str>, Option<&str>))> {
        self.member_states(RulesTypeFamily::Projectile, |member| {
            (
                member.airburst_weapon.as_deref(),
                member.shrapnel_weapon.as_deref(),
            )
        })
    }

    pub(super) fn member_states<'a, T>(
        &'a self,
        family: RulesTypeFamily,
        state: impl Fn(&'a ProcessedType) -> T + 'a,
    ) -> impl Iterator<Item = (&'a str, T)> + 'a {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&family)
            .into_iter()
            .flatten()
            .map(move |member| (member.native_stored_id.as_str(), state(member)))
    }

    /// `[CombatDamage] AmmoCrateDamage=` (ReadCombatDamage 66BBDE) after the
    /// last pass.
    pub(crate) fn ammo_crate_damage(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_ammo_crate_damage
    }

    /// `[CombatDamage] TiberiumExplosionDamage=` (Rules+568, ReadCombatDamage
    /// 66BC37) after the last pass.
    pub(crate) fn tiberium_explosion_damage(&self) -> i32 {
        self.native_type_construction_trace
            .registry_state()
            .rules_tiberium_explosion_damage
    }

    /// `TiberiumClass::ReadINI_All @ 0x00721D10` / `ReadINI @ 0x00721A50`:
    /// retained ordinal, admitted body, Color and Debris.
    /// The Debris factory answers belong to this read, not a final text lookup.
    pub(crate) fn tiberium_read_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            &IniSection,
            i32,
            &[String],
            crate::rules::tiberium_type::TiberiumImageState,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .tiberiums
            .iter()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    &member.body,
                    member.tiberium_color_scheme,
                    member.tiberium_debris.as_slice(),
                    member.tiberium_image,
                )
            })
    }

    /// The retained TiberiumExt of each `[Tiberiums]` type, in order.
    pub(crate) fn tiberium_ext_states(
        &self,
    ) -> impl Iterator<Item = &crate::rules::ares_tiberium::TiberiumExtRules> {
        self.native_type_construction_trace
            .registry_state()
            .tiberiums
            .iter()
            .map(|member| &member.tiberium_ext)
    }

    /// `[CombatDamage] DeathWeapon=` (Rules+FDC) after the last pass.
    pub(crate) fn combat_damage_death_weapon(&self) -> Option<&str> {
        self.native_type_construction_trace
            .registry_state()
            .rules_death_weapon
            .as_deref()
    }

    /// The RulesClass Type pointers and lists of [`RulesTypeReferences`]
    /// after the last pass.
    pub(crate) fn rules_type_references(&self) -> &RulesTypeReferences {
        &self
            .native_type_construction_trace
            .registry_state()
            .rules_type_references
    }

    /// `[AI] NeutralTechBuildings=` (Rules+ADC) after the last pass, as
    /// stored BuildingType IDs.
    pub fn neutral_tech_buildings(&self) -> &[String] {
        self.rules_type_references().list("NeutralTechBuildings")
    }

    /// Whether `name` is a registered BuildingType, InfantryType or
    /// VehicleType: the case-insensitive ID scans of
    /// `BuildingTypeClass::FindIndex @ 0x0045E7B0`, `InfantryTypeClass::Find`
    /// and `UnitTypeClass::Find`, as Ares' `[Urban]` lists use them
    /// (Ares.dll 1006AAD0, hook 0x5A66B0).
    pub(crate) fn registers_urban_type(&self, kind: UrbanTypeKind, name: &str) -> bool {
        let family = match kind {
            UrbanTypeKind::Building => RulesTypeFamily::Building,
            UrbanTypeKind::Infantry => RulesTypeFamily::Infantry,
            UrbanTypeKind::Vehicle => RulesTypeFamily::Vehicle,
        };
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&family)
            .is_some_and(|members| {
                members
                    .iter()
                    .any(|member| member.native_stored_id.eq_ignore_ascii_case(name))
            })
    }

    /// Retained Warhead ReadINI fields, including+0xA0 after its last reached
    /// ReadINI75DDCC..75DE5A. The compatibility INI cannot represent the
    /// table's literal per-pass default.
    pub(crate) fn warhead_read_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            bool,
            bool,
            bool,
            &[String],
            &[String],
            &[NativeF64Bits; 11],
            Option<&AresWarheadVerses>,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Warhead)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    &*member.native_stored_id,
                    member.warhead_anim.conventional,
                    member.warhead_anim.em_effect,
                    member.warhead_anim.sparky,
                    member.warhead_anim.anim_list.as_slice(),
                    member.warhead_anim.debris_types.as_slice(),
                    &member.warhead_verses,
                    member.ares_verses.as_ref(),
                )
            })
    }

    /// WarheadTypeClass+0x140 `Particle=` after every reached
    /// ReadINI75D4BE..75D4D4.
    pub(crate) fn warhead_particle_states(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Warhead)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.warhead_particle.as_deref(),
                )
            })
    }

    pub(crate) fn ares_spotlight_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            bool,
            crate::rules::ares_spotlight::SpotlightTypeRules,
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.building_has_spotlight,
                        member.spotlight,
                    )
                })
            })
    }

    pub(crate) fn selection_group_states(
        &self,
    ) -> impl Iterator<Item = (ObjectCategory, &str, &str)> {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                member.selection_group.as_str(),
            )
        })
    }

    pub(crate) fn driver_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_driver::DriverTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.driver,
            )
        })
    }

    pub(crate) fn bounty_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_bounty::BountyTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.bounty,
            )
        })
    }

    pub(crate) fn cloak_power_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_cloak_power::CloakPowerTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.cloak_power,
            )
        })
    }

    pub(crate) fn crew_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_crew::CrewTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.crew,
            )
        })
    }

    /// The retained TechnoTypeExt combat and movement keys.
    pub(crate) fn techno_combat_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_techno_type::AresTechnoTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.techno_combat,
            )
        })
    }

    /// The retained TechnoTypeExt Experience.*/Promote.* reads.
    pub(crate) fn veterancy_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_veterancy::VeterancyTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.veterancy,
            )
        })
    }

    /// The retained TechnoTypeExt BuildTime.*/FactoryPlant/cloning reads.
    pub(crate) fn production_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::ares_production::ProductionTypeRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.production,
            )
        })
    }

    /// The retained BuildingTypeExt Academy.* reads, by BuildingType.
    pub(crate) fn academy_states(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::rules::ares_veterancy::AcademyTypeRules)> {
        self.member_states(RulesTypeFamily::Building, |member| &member.academy)
    }

    /// The retained BuildingTypeExt infiltration reads, by BuildingType.
    pub(crate) fn infiltration_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            &crate::rules::ares_infiltration::InfiltrationTypeRules,
        ),
    > {
        self.member_states(RulesTypeFamily::Building, |member| &member.infiltration)
    }

    /// The retained BuildingTypeExt rubble and repair reads, by BuildingType.
    pub(crate) fn building_type_ext_states(
        &self,
    ) -> impl Iterator<
        Item = (
            &str,
            &crate::rules::ares_building_type::BuildingTypeExtRules,
        ),
    > {
        self.member_states(RulesTypeFamily::Building, |member| {
            &member.building_type_ext
        })
    }

    pub(crate) fn ares_techno_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            Option<&crate::rules::ares_emp::EmpTypeRules>,
            Option<&crate::rules::ares_abilities::PromotionAbilities>,
            Option<&crate::rules::ares_warhead::AttachEffectRules>,
            bool,
            Option<i32>,
            Option<bool>,
            [NativeF64Bits; 2],
        ),
    > {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter_map(|(family, members)| {
                family.object_category().map(|category| (category, members))
            })
            .flat_map(|(category, members)| {
                members.iter().map(move |member| {
                    (
                        category,
                        member.native_stored_id.as_str(),
                        member.emp_type.as_ref(),
                        // Only a read the Ares way fills these; the stock way
                        // leaves None, None, None and 1.0 twice.
                        member.promotion_abilities.as_ref(),
                        member.self_attach_effect.as_ref(),
                        member.cloak_allowed,
                        member.turret_rot_override,
                        member.aircraft_crashable,
                        member.curtain_modifiers,
                    )
                })
            })
    }

    pub(crate) fn super_weapon_techno_states(
        &self,
    ) -> impl Iterator<
        Item = (
            ObjectCategory,
            &str,
            &crate::rules::superweapon_influence::TechnoSuperWeaponRules,
        ),
    > {
        let registry = self.native_type_construction_trace.registry_state();
        registry.techno_type_order.iter().map(|&(family, index)| {
            let member = &registry.families[&family][index];
            (
                family.object_category().expect("TechnoType category"),
                member.native_stored_id.as_str(),
                &member.super_weapon_techno,
            )
        })
    }

    pub(crate) fn ares_warhead_states(
        &self,
    ) -> impl Iterator<Item = (&str, Option<&crate::rules::ares_warhead::AresWarheadRules>)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .get(&RulesTypeFamily::Warhead)
            .into_iter()
            .flatten()
            .map(|member| {
                (
                    member.native_stored_id.as_str(),
                    member.ares_warhead.as_ref(),
                )
            })
    }

    pub(crate) fn object_armor_states(
        &self,
    ) -> impl Iterator<Item = (Option<ObjectCategory>, &str, ArmorId)> {
        self.native_type_construction_trace
            .registry_state()
            .families
            .iter()
            .filter(|(family, _)| {
                family.object_category().is_some() || **family == RulesTypeFamily::Terrain
            })
            .flat_map(|(&family, members)| {
                members.iter().map(move |member| {
                    (
                        family.object_category(),
                        member.native_stored_id.as_str(),
                        member.armor_id,
                    )
                })
            })
    }
}
