//! One retained Type slot (`ProcessedType`): Type families, Ares factory owners,
//! Techno/Anim/VoxelAnim references, weapon slots and find-or-construct.

use super::{
    AresSideState, CountryVeterancyState, NativeTypeConstructionEvent, NativeTypeConstructorFamily,
    abstract_type_stored_id, default_curtain_modifiers,
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
use crate::rules::object_type::WEAPON_SLOT_COUNT;
use crate::rules::prerequisite::Prerequisite;
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::sound_ini::SoundRegistry;
use crate::rules::superweapon_targeting::AresSuperWeaponTargetingState;
use crate::rules::superweapon_type::AresSuperWeaponAvailability;
use crate::rules::superweapon_type::AresSuperWeaponCharge;
use crate::rules::superweapon_type::AresSuperWeaponPresentation;
#[cfg(feature = "phobos")]
use crate::rules::superweapon_type::PhobosSuperWeaponAvailability;
use crate::rules::superweapon_type::SuperWeaponBuildingRequirements;
use crate::rules::superweapon_type::UnitDeliveryRules;
use crate::util::native_x87::NativeF64Bits;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

/// The type arrays the Ares `[Urban]` lists name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UrbanTypeKind {
    Building,
    Infantry,
    Vehicle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) enum RulesTypeFamily {
    Country,
    Side,
    Overlay,
    SuperWeapon,
    Warhead,
    Smudge,
    Terrain,
    Building,
    Vehicle,
    Aircraft,
    Infantry,
    Animation,
    VoxelAnimation,
    Particle,
    ParticleSystem,
    Weapon,
    Projectile,
}

/// Explicit registry order in `RulesClass::Process @ 0x00668BF0`.
///
/// `Sides` is special: `FUN_00672440` passes the entry name to
/// `SideClass::Constructor @ 0x006A4550`; all other rows read a 32-byte value.
pub(super) const EXPLICIT_RULE_TYPE_FAMILIES: &[(&str, RulesTypeFamily)] = &[
    ("Countries", RulesTypeFamily::Country),
    ("Sides", RulesTypeFamily::Side),
    ("OverlayTypes", RulesTypeFamily::Overlay),
    ("SuperWeaponTypes", RulesTypeFamily::SuperWeapon),
    ("Warheads", RulesTypeFamily::Warhead),
    ("SmudgeTypes", RulesTypeFamily::Smudge),
    ("TerrainTypes", RulesTypeFamily::Terrain),
    ("BuildingTypes", RulesTypeFamily::Building),
    ("VehicleTypes", RulesTypeFamily::Vehicle),
    ("AircraftTypes", RulesTypeFamily::Aircraft),
    ("InfantryTypes", RulesTypeFamily::Infantry),
    ("Animations", RulesTypeFamily::Animation),
    ("VoxelAnims", RulesTypeFamily::VoxelAnimation),
    ("Particles", RulesTypeFamily::Particle),
    ("ParticleSystems", RulesTypeFamily::ParticleSystem),
];

/// Families rebuilt into the compatibility INI projection. `Sides` remains
/// an ordinary section there because its values are country membership lists,
/// not an index-to-Type registry.
pub(super) const PROJECTED_RULE_TYPE_FAMILIES: &[(&str, RulesTypeFamily)] = &[
    ("Countries", RulesTypeFamily::Country),
    ("OverlayTypes", RulesTypeFamily::Overlay),
    ("SuperWeaponTypes", RulesTypeFamily::SuperWeapon),
    ("Warheads", RulesTypeFamily::Warhead),
    ("SmudgeTypes", RulesTypeFamily::Smudge),
    ("TerrainTypes", RulesTypeFamily::Terrain),
    ("BuildingTypes", RulesTypeFamily::Building),
    ("VehicleTypes", RulesTypeFamily::Vehicle),
    ("AircraftTypes", RulesTypeFamily::Aircraft),
    ("InfantryTypes", RulesTypeFamily::Infantry),
    ("Animations", RulesTypeFamily::Animation),
    ("VoxelAnims", RulesTypeFamily::VoxelAnimation),
    ("Particles", RulesTypeFamily::Particle),
    ("ParticleSystems", RulesTypeFamily::ParticleSystem),
];

impl RulesTypeFamily {
    pub(super) fn from_object_category(category: ObjectCategory) -> Self {
        match category {
            ObjectCategory::Building => Self::Building,
            ObjectCategory::Aircraft => Self::Aircraft,
            ObjectCategory::Vehicle => Self::Vehicle,
            ObjectCategory::Infantry => Self::Infantry,
        }
    }

    pub(super) fn object_category(self) -> Option<ObjectCategory> {
        match self {
            Self::Building => Some(ObjectCategory::Building),
            Self::Aircraft => Some(ObjectCategory::Aircraft),
            Self::Vehicle => Some(ObjectCategory::Vehicle),
            Self::Infantry => Some(ObjectCategory::Infantry),
            _ => None,
        }
    }

    pub(super) fn native_constructor_family(self) -> Option<NativeTypeConstructorFamily> {
        Some(match self {
            Self::Country => NativeTypeConstructorFamily::HouseType,
            Self::Side => NativeTypeConstructorFamily::Side,
            Self::Overlay => NativeTypeConstructorFamily::OverlayType,
            Self::SuperWeapon => NativeTypeConstructorFamily::SuperWeaponType,
            Self::Warhead => NativeTypeConstructorFamily::WarheadType,
            Self::Smudge => NativeTypeConstructorFamily::SmudgeType,
            Self::Terrain => NativeTypeConstructorFamily::TerrainType,
            Self::Building => NativeTypeConstructorFamily::BuildingType,
            Self::Vehicle => NativeTypeConstructorFamily::UnitType,
            Self::Aircraft => NativeTypeConstructorFamily::AircraftType,
            Self::Infantry => NativeTypeConstructorFamily::InfantryType,
            Self::Animation => NativeTypeConstructorFamily::AnimType,
            Self::VoxelAnimation => NativeTypeConstructorFamily::VoxelAnimType,
            Self::Particle => return None,
            Self::ParticleSystem => NativeTypeConstructorFamily::ParticleSystemType,
            Self::Weapon => NativeTypeConstructorFamily::WeaponType,
            Self::Projectile => NativeTypeConstructorFamily::BulletType,
        })
    }
}

/// Resolved identities preserve native factory selection and vector order;
/// colliding AnimTypes keep distinct runtime keys. These pointer bindings
/// belong to the retained Rules object, not merged INI text.
/// Native reader corpus: tools/rules_oracle/select_anim_inputs.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct SelectAnimRulesState {
    pub(super) lightning_warhead: String,
    pub(super) weather_con_bolt_explosion: String,
    pub(super) weapon_nullify_anim: String,
    pub(super) splash_list: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct WarheadAnimReadState {
    pub(super) conventional: bool,
    pub(super) em_effect: bool,
    /// WarheadCtor75CF77 starts false. ReadINI75D576..75D58A reads the
    /// current byte; Save75E2D1/Load75E0F0 retain it in the 1D0-byte object.
    pub(super) sparky: bool,
    pub(super) anim_list: Vec<String>,
    /// `DebrisTypes=` (+18C, `0x0075DAF5`): ReadString128, then each `strtok`
    /// token's non-null VoxelAnimType replaces the list.
    pub(super) debris_types: Vec<String>,
    /// Firestorm's web keys (`WarheadWeb::read_pass`); `None` until a pass
    /// gives them other values than the constructor's.
    pub(super) web: Option<crate::rules::warhead_type::WarheadWeb>,
}

/// Installed Ares TechnoTypeExt ctor1003CB1D..1003CB41: empty Country
/// vectors+308/+314 and false bytes+320/+321. Common reader1003E973..1003E9DC
/// uses current Rules INI, including both aliases for the same+320 field.
/// Source: installed Ares3.0 country_vector_reader1003FAE0,
/// country_vector_tokens1003C310 and Country Find1001BA90.
/// Bounded qualification execution and reader limits:
/// tools/rules_oracle/ares_factory_owners.md (reader expectations are instructions).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresFactoryOwners {
    pub(super) phase: i32,
    /// TechnoTypeExt+2D8, constructor1003CAC2..1003CACE: stable Building ordinal.
    pub(super) built_at: Vec<i32>,
    pub(super) allowed: Vec<crate::rules::ruleset::CountryIdx>,
    pub(super) forbidden: Vec<crate::rules::ruleset::CountryIdx>,
    permanent: bool,
    has_all_plans: bool,
}

impl AresFactoryOwners {
    pub(crate) fn built_at(&self) -> &[i32] {
        &self.built_at
    }

    pub(crate) fn allowed(&self) -> &[crate::rules::ruleset::CountryIdx] {
        &self.allowed
    }

    pub(crate) fn forbidden(&self) -> &[crate::rules::ruleset::CountryIdx] {
        &self.forbidden
    }

    pub(crate) fn permanent(&self) -> bool {
        self.permanent
    }

    pub(crate) fn has_all_plans(&self) -> bool {
        self.has_all_plans
    }

    pub(super) fn read_pass(
        &mut self,
        section: &IniSection,
        find_country: impl Fn(&str) -> Option<crate::rules::ruleset::CountryIdx>,
        read_built_at: impl FnOnce() -> Option<Vec<i32>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // Extension dispatcher1003DEC0: only phases0/1/2 enter the common
        // reader. First-phase initialization does not reset these ctor fields.
        if !(0..=2).contains(&self.phase) {
            return;
        }
        if self.phase == 0 {
            self.phase = 1;
        }
        // BuiltAt1003E8D9..1003E8EA precedes FactoryOwners in the same Type
        // phase; shared vector reader10010850 uses current Building Find10007E50.
        if let Some(built_at) = read_built_at() {
            self.built_at = built_at;
        }
        // ReadString2048: missing/empty retains; any nonempty list replaces,
        // including comma-only or all unknown tokens. Find-only is by current
        // stored Country ID, without Name aliases, ParentCountry or allocation.
        for (key, target) in [
            ("FactoryOwners", &mut self.allowed),
            ("FactoryOwners.Forbidden", &mut self.forbidden),
        ] {
            if let Some(tokens) = section.read_list(key, 2048) {
                *target = tokens.into_iter().filter_map(&find_country).collect();
            }
        }
        self.permanent = section.read_ares_bool("FactoryOwners.HaveAllPlans", self.permanent);
        self.permanent = section.read_ares_bool("FactoryOwners.Permanent", self.permanent);
        self.has_all_plans =
            section.read_ares_bool("FactoryOwners.HasAllPlans", self.has_all_plans);
        self.phase = 2;
    }
}

/// BuildingExt+126, ctor1000CC98=false; separate dispatcher1000DCF0 and
/// common reader1000DD40. Its first-phase100105A0 leaves this field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresExplicitFactory {
    pub(super) phase: i32,
    pub(super) explicit_only: bool,
}

impl AresExplicitFactory {
    pub(crate) fn explicit_only(&self) -> bool {
        self.explicit_only
    }

    pub(super) fn read_pass(&mut self, section: &IniSection, aircraft_repair: bool) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if !(0..=2).contains(&self.phase) {
            return;
        }
        if self.phase == 0 {
            self.phase = 1;
        }
        // 1000DD79..1000DD89 jumps past common fields for UnitRepair with
        // Factory=AircraftType(3), retaining the flag but still completing phase2.
        if !aircraft_repair {
            self.explicit_only = section.read_ares_bool("Factory.ExplicitOnly", self.explicit_only);
        }
        self.phase = 2;
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ProcessedType {
    /// TechnoType ctor710B6A..710BA0 starts all five at zero. Preserve the
    /// admitted-read history on save; a missing section must not apply defaults.
    pub(super) threat_coefficients: super::threat_coefficients::ThreatCoefficients,
    pub(super) native_stored_id: String,
    pub(super) body: IniSection,
    // Mandatory on load: a missing Arctic latch cannot be reconstructed from
    // a flattened Image key or inferred from whether its name ends in A.
    pub(super) object_image: ObjectImageState,
    /// ObjectTypeCtor5F713A starts +1F0 at -1. Each successful inherited
    /// ObjectTypeRead5F938B..5F93D5 retains a fixed Voc binding on bad input.
    pub(super) object_crush_sound: Option<String>,
    /// AircraftTypeRead41CCEF..41CD4E. Mandatory saved history: final Rules
    /// text cannot recover an empty ART read's previously bound AnimType.
    pub(super) aircraft_trailer: AircraftTrailer,
    /// OverlayCtor5FE265 starts at1; OverlayRead5FE8A6..5FE8C3 retains
    /// the full signed dword through each fixed-ART Image-section read.
    pub(super) overlay_damage_levels: i32,
    /// OverlayCtor5FE2C0 false; ReadINI5FE839..5FE853 current-default bool.
    pub(super) overlay_explodes: bool,
    /// OverlayTypeClass ctor5FE290 starts null; ReadINI5FE867..5FE8A0
    /// retains empty reads and stores AnimTypeFindOrAllocate428B80's result.
    pub(super) overlay_cell_anim: Option<String>,
    /// Constructor711351=0; fixed ART ReadInt71592A..715948 keeps the
    /// current value as default across Image changes and later Rules passes.
    #[serde(default)]
    pub(super) voxel_shadow_index: i32,
    /// TechnoType ctor711082 initializes+634 to255; ReadInt714570 uses
    /// its current value, including a prior TAction106 write, as the default.
    /// Native controls: trigger_object_tech_level.json scalar_layers/lifecycle.
    pub(super) tech_level: i32,
    /// HouseType ctor51141C starts Color+C0 at0; each reached ReadINI
    /// passes the current index to the shared ReadColor474A90.
    pub(super) country_color_scheme: i32,
    /// TiberiumCtor72171F/721731: Color0 and an empty Debris vector;
    /// ReadINI721B20..721C1E retains these on the same live Tiberium object.
    pub(super) tiberium_color_scheme: i32,
    pub(super) tiberium_debris: Vec<String>,
    // Mandatory saved field: flattened Image text loses the retained slope
    // count and cannot distinguish a detached binding after Rules reset.
    pub(super) tiberium_image: crate::rules::tiberium_type::TiberiumImageState,
    /// TiberiumExt (`rules::ares_tiberium`): every reached ReadINI exit
    /// (hooks 721C7B/721CDC/721CE9) runs LoadFromINI100508D0 on this type.
    #[serde(default)]
    pub(super) tiberium_ext: crate::rules::ares_tiberium::TiberiumExtRules,
    /// HouseType ctor511416 writes-1. Side67259A and the late Side reader
    /// 51209E/4756F0 update this binding on the same retained Country.
    pub(super) country_side_index: i32,
    /// Country+98: ctor511404 clears its first byte; reached5118CF reads
    /// ParentCountry with current ID default, capacity25.
    pub(super) country_parent: String,
    pub(super) country_veterancy: CountryVeterancyState,
    pub(super) factory_owners: Option<AresFactoryOwners>,
    pub(super) explicit_factory: Option<AresExplicitFactory>,
    /// Side+9C ordered Country indexes. ReadHouses4767C0 retains the prior
    /// vector on count0 and otherwise replaces it without deduplication.
    /// Signed entries preserve `<random>`=-2 before the full-reader boundary.
    pub(super) side_country_members: Vec<i32>,
    /// SideExt only: constructor values remain uninitialized until the
    /// complete ReadTypeData entry, not Country-only prepasses. Ordinary
    /// Process, failures and saved Rules retain this same owner.
    pub(super) ares_side: Option<AresSideState>,
    /// False until the native AnimType ART-body boundary has been entered.
    /// Kept on the process-resident type so subsequent Rules passes retain it.
    pub(super) anim_art_read: bool,
    /// One owner for Bullet's two-phase Image/ART reads across rules passes.
    pub(super) projectile_art: ProjectileArtState,
    /// Terrain constructor/current fields and ordered admitted reader inputs;
    /// deferred SHP frame colors bind in TerrainAssetCatalog.
    #[serde(default)]
    pub(super) terrain_type_read_state:
        Option<crate::rules::terrain_object_type::TerrainTypeReadState>,
    /// BuildingType45DF13 initializes +EF0 to zero; ReadINI461225..46125D
    /// updates it from fixed ART after ObjectType5F933B reads effective Image.
    pub(super) building_foundation: crate::rules::foundation::Foundation,
    /// Constructor45DEBC..45DEC8 clears BuildingType+EBC/EC0/EC4.
    pub(super) building_target_coord_offset: [i32; 3],
    /// BuildingTypeClass ctor45DD90 clears both triples; reached
    /// ReadINI45FE50 calls Read3Int at45FF13 and45FF3C with current defaults.
    #[serde(default)]
    pub(super) building_half_damage_smoke_location1: [i32; 3],
    #[serde(default)]
    pub(super) building_half_damage_smoke_location2: [i32; 3],
    /// BuildingType ctor45DFDA=false; ReadBool45FEE5..45FF0D, current default.
    pub(super) building_has_spotlight: bool,
    /// BuildingType+EE8/+EEC, retained across reached ReadINI4610A0 passes.
    #[serde(default)]
    pub(super) building_extra_power: crate::rules::building_extra_power::BuildingExtraPower,
    pub(super) building_light: crate::rules::building_light::BuildingLightRules,
    /// BuildingType+E88/+16FC/+14E0, retained by ReadINI460E17/464727.
    pub(super) building_upgrade: crate::rules::building_upgrade::BuildingUpgradeRules,
    pub(super) secret_lab: crate::rules::secret_lab::SecretLabRules,
    /// Unit ctor747167..7471B2 and reached fixed-ART reader7477C1..747AAE.
    pub(super) unit_shp: crate::rules::shp_vehicle_sequence::UnitShpReadState,
    /// Unit7470D7 ctor -1 and7476D3..74770B retained default/read sequence.
    pub(super) unit_speed_type: SpeedType,
    /// TechnoType ctor71136F/711781, generic flag read71287E and FV pair
    /// reads747BBD..747E90, retained across every reached Rules pass.
    pub(super) gunner_turrets: GunnerTurrets,
    /// TechnoType ctor71148A/711490..49C/711765..77F; ReadINI714016..71410F.
    pub(super) is_gattling: bool,
    pub(super) gattling_stages: crate::rules::gattling_type::GattlingStages,
    /// Infantry ctor5236BD/C7 and unconditional ReadPip5240D1..524125.
    /// Missing type sections skip the whole reader; missing keys do not.
    #[serde(default)]
    pub(super) infantry_pips: InfantryPips,
    /// InfantryTypeCtor5237A1 clears+EC1; ReadINI52454A..524564 uses
    /// the current byte as Civilian's default on each reached own-ID body.
    #[serde(default)]
    pub(super) infantry_civilian: bool,
    /// TechnoType+C8F: InfantryType ReadINI clears it on every pass
    /// (`0x005240BD`, skipped under Ares), then a reached body sets it from
    /// the retained `Cyborg` byte (`0x005243DD..0x005243E7`).
    #[serde(default)]
    pub(super) infantry_damage_sparks: bool,
    #[serde(default)]
    pub(super) infantry_infiltration: InfantryInfiltration,
    /// Infantry ctor `0x00523788`/`0x005236D7..0x005236F9` and the fixed-ART
    /// reads of every reached ReadINI pass (`0x005246A1..0x0052472D`).
    #[serde(default)]
    pub(super) infantry_art: InfantryArt,
    /// Ctor52392C initializes the whole bank; each reached InfantryType
    /// ReadINI52473A updates its timing and fixed-catalog sound slots together.
    pub(super) infantry_sequence_bank:
        Option<crate::rules::infantry_sequence::InfantrySequenceBank>,
    pub(super) recoil: crate::rules::recoil::RecoilConfig,
    pub(super) airstrike: crate::rules::airstrike::AirstrikeSettings,
    /// TechnoType constructor711134 and current-default ReadInt7147B4.
    #[serde(default)]
    pub(super) radial_fire_segments: i32,
    /// Type+3DC, constructor710D28 and retained ReadInt714145..714159.
    pub(super) lepton_mind_control_offset: i32,
    /// TechnoType `Prerequisite=` (`+0x638`) and `PrerequisiteOverride=`
    /// (`+0x654`), each read with its current list as the default.
    pub(super) prerequisite: Vec<Prerequisite>,
    pub(super) prerequisite_override: Vec<Prerequisite>,
    pub(super) weapon_slots: WeaponSlots,
    pub(super) techno_references: TechnoTypeReferences,
    pub(super) weapon: WeaponReadState,
    pub(super) warhead_anim: WarheadAnimReadState,
    /// WarheadTypeClass+0x140 `Particle=`. Constructor75CF4D starts null;
    /// ReadINI75D4BE uses ReadString's empty default and then
    /// ParticleSystemTypeClass::FindOrAllocate @0x00644890, so empty keeps
    /// this pointer while `none`/`<none>` stores null.
    #[serde(default)]
    pub(super) warhead_particle: Option<String>,
    /// Warhead ctor75CEC0 starts every +A0 slot at1.0. Each reached
    /// ReadINI75DDCC..75DE5A uses its literal all-100% missing-key default;
    /// no section leaves this live table alone. The retained registry owns
    /// these bits; RuleSet only projects them. Native execution:
    /// tools/rules_oracle/bridge_landing_inputs.{py,json,meta.json}.
    pub(super) warhead_verses: [NativeF64Bits; 11],
    /// Independent Ares10051D80 constructor/10006460 read vector.
    pub(super) ares_verses: Option<AresWarheadVerses>,
    pub(super) ares_warhead: Option<crate::rules::ares_warhead::AresWarheadRules>,
    pub(super) emp_type: Option<crate::rules::ares_emp::EmpTypeRules>,
    #[serde(default)]
    pub(super) promotion_abilities: Option<crate::rules::ares_abilities::PromotionAbilities>,
    pub(super) self_attach_effect: Option<crate::rules::ares_warhead::AttachEffectRules>,
    #[serde(default)]
    pub(super) super_weapon_techno: crate::rules::superweapon_influence::TechnoSuperWeaponRules,
    /// TechnoTypeExt+1FA: ctor1003C9D7 defaults true; read1003EBEF.
    pub(super) cloak_allowed: bool,
    #[serde(default)]
    pub(super) cloak_power: crate::rules::ares_cloak_power::CloakPowerTypeRules,
    /// Ares TypeExt+588/+58C. Constructor1003CED0/6 clears value/presence;
    /// nullable reader10008CC0 at1003F267 retains both on invalid input.
    #[serde(default)]
    pub(super) turret_rot_override: Option<i32>,
    #[serde(default)]
    pub(super) spotlight: crate::rules::ares_spotlight::SpotlightTypeRules,
    #[serde(default)]
    pub(super) driver: crate::rules::ares_driver::DriverTypeRules,
    /// Ares TypeExt+250, empty in constructor1003CA53. Read1003EC3C..7C.
    #[serde(default)]
    pub(super) selection_group: String,
    #[serde(default)]
    pub(super) bounty: crate::rules::ares_bounty::BountyTypeRules,
    #[serde(default)]
    pub(super) crew: crate::rules::ares_crew::CrewTypeRules,
    /// BuildingTypeExt+AA..+CC and +1E8 (`rules::ares_infiltration`).
    #[serde(default)]
    pub(super) infiltration: crate::rules::ares_infiltration::InfiltrationTypeRules,
    /// BuildingTypeExt rubble and repair keys (`rules::ares_building_type`).
    #[serde(default)]
    pub(super) building_type_ext: crate::rules::ares_building_type::BuildingTypeExtRules,
    /// Extension<WarheadTypeClass>::LoadFromINI 10051D10 has left its first
    /// state, which ran the `NUKE` warhead's Initialize.
    #[serde(default)]
    pub(super) ares_warhead_loaded: bool,
    /// TechnoTypeExt Experience.* and Promote.* (`rules::ares_veterancy`).
    #[serde(default)]
    pub(super) veterancy: crate::rules::ares_veterancy::VeterancyTypeRules,
    /// BuildingTypeExt+138..+170 Academy.* (`rules::ares_veterancy`).
    #[serde(default)]
    pub(super) academy: crate::rules::ares_veterancy::AcademyTypeRules,
    /// TechnoTypeExt BuildTime.*, FactoryPlant.Multiplier and cloning, and
    /// BuildingTypeExt CloningFacility (`rules::ares_production`).
    #[serde(default)]
    pub(super) production: crate::rules::ares_production::ProductionTypeRules,
    ///1003CB5B leaves the Nullable<bool> presence byte+324 clear.
    #[serde(default)]
    pub(super) aircraft_crashable: Option<bool>,
    /// TechnoTypeExt combat and movement keys (`rules::ares_techno_type`).
    #[serde(default)]
    pub(super) techno_combat: crate::rules::ares_techno_type::AresTechnoTypeRules,
    /// TechnoTypeExt `Cursor.*` and BuildingTypeExt `Cursor.Spy`
    /// (`rules::ares_mouse_cursor`).
    #[serde(default)]
    pub(super) ares_cursors: crate::rules::ares_mouse_cursor::TechnoCursors,
    /// SWTypeExt `Cursor`/`NoCursor` (`rules::ares_mouse_cursor`).
    #[serde(default)]
    pub(super) ares_super_weapon_cursors: crate::rules::ares_mouse_cursor::SuperWeaponCursors,
    /// Ares TypeExt+150 `IronCurtain.Modifier` and +158
    /// `ForceShield.Modifier`, Valueable<double>, constructor 1.0.
    #[serde(default = "default_curtain_modifiers")]
    pub(super) curtain_modifiers: [NativeF64Bits; 2],
    /// ObjectType4753F0 -> Ares10006E10 binds at each reached body read.
    pub(super) armor_id: ArmorId,
    /// ParticleSystemType+294, the bound ParticleType's index: constructor -1
    /// (`0x006440B3`), rebound by every reached ReadINI (`0x0064430F..0x00644323`).
    pub(super) holds_what: Option<usize>,
    /// ParticleTypeReadINI644F50's per-body reset state and retained +310
    /// NextParticle binding (645315..64535B), owned by the allocated type.
    #[serde(default)]
    pub(super) particle_read: crate::rules::particle_type::ParticleReadState,
    #[serde(default)]
    pub(super) particle_next: Option<usize>,
    /// Constructor6440B9 and reached reader644461 store -1 for no behavior.
    #[serde(default)]
    pub(super) particle_system_behavior:
        crate::rules::particle_system_type::ParticleSystemBehavesLike,
    /// SuperWeaponType+9C `WeaponType=`, the stored ID of its WeaponType:
    /// constructor null (`0x006CE5CB`); ReadINI (`0x006CEA4A..0x006CEA8C`)
    /// keeps it on an empty ReadString128 and otherwise stores
    /// `WeaponTypeClass::FindOrAllocate @ 0x00772FA0`, null for `none`.
    pub(super) super_weapon_weapon: Option<String>,
    /// SuperWeaponType+C8 `AuxBuilding=` (`0x006CED07..0x006CED2E`), read the
    /// same way through BuildingType's factory (`0x004653C0`).
    pub(super) super_weapon_aux_building: Option<String>,
    #[serde(default)]
    pub(super) super_weapon_building_requirements: Option<SuperWeaponBuildingRequirements>,
    pub(super) super_weapon_availability: AresSuperWeaponAvailability,
    #[serde(default)]
    pub(super) super_weapon_influence: crate::rules::superweapon_influence::SuperWeaponInfluence,
    pub(super) super_weapon_presentation: AresSuperWeaponPresentation,
    pub(super) super_weapon_targeting: AresSuperWeaponTargetingState,
    #[serde(default)]
    pub(super) super_weapon_charge: AresSuperWeaponCharge,
    /// SWTypeExt launch fields and the Protect/Battery payloads.
    #[serde(default)]
    pub(super) super_weapon_launch: crate::rules::superweapon_type::AresSuperWeaponLaunch,
    /// Phobos `SW.TechLevel`/`SW.AuxTechnos`/`SW.NegTechnos`, read by the same
    /// ordered Process owner as the Ares gates and keyed the same way.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(super) super_weapon_phobos_availability: PhobosSuperWeaponAvailability,
    pub(super) additional_super_weapons: Vec<String>,
    /// Phobos `UnitTypeExt`'s three `OreGathering.*` vectors.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(super) ore_gathering: crate::rules::object_type::PhobosOreGathering,
    /// Phobos `TechnoTypeExt::LoadFromINIFile`'s insignia block. Its
    /// `InsigniaType=` resolution and direct keys are read per reached pass on
    /// the same live TechnoType, so this accumulates instead of projecting a
    /// final merged section.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    pub(super) phobos_insignia: crate::rules::insignia_type::PhobosInsignia,
    /// Ares `TechnoTypeExt`'s insignia keys, read on every reached pass of
    /// the same live TechnoType like Phobos's block above.
    #[serde(default)]
    pub(super) ares_insignia: crate::rules::insignia_type::AresInsignia,
    /// Ares `SW.Warhead=`, read only for Type=GenericWarhead.
    pub(super) generic_super_weapon_warhead: Option<String>,
    pub(super) unit_delivery: Option<UnitDeliveryRules>,
    #[serde(default)]
    pub(super) hunter_seeker: Option<crate::rules::hunter_seeker::HunterSeekerSettings>,
    /// ParticleType `Warhead=` (`0x00645043..0x00645067`): ReadString128,
    /// then WarheadType's factory (`0x0075E3B0`); an empty read keeps it.
    pub(super) particle_warhead: Option<String>,
    pub(super) voxel_anim_references: VoxelAnimTypeReferences,
    /// Type+2F0 is mutable through Voxel contact74A923 and retained on reread.
    pub(super) voxel_anim_damage: i32,
    ///74B107..167: each reached section supplies literal -1 defaults, unlike
    /// current-default fields that can project from the merged text history.
    pub(super) voxel_anim_angular_velocities: [NativeF64Bits; 2],
    pub(super) voxel_anim_read: bool,
    pub(super) voxel_anim_share_flags: [bool; 3],
    pub(super) anim_references: AnimTypeReferences,
    /// BulletType+2B4 `ShrapnelWeapon=`, read the same way by BulletType
    /// ReadINI (`0x0046C2E1..0x0046C31F`) over the constructor's null
    /// (`0x0046BC82`).
    pub(super) shrapnel_weapon: Option<String>,
    /// Constructor46BC7C clears+2B0. ReadString128 at46C2B9 retains the
    /// existing pointer on empty input, else stores the Weapon factory result.
    #[serde(default)]
    pub(super) airburst_weapon: Option<String>,
}

/// The ObjectType `Image` and TechnoType pointer fields each reached ReadINI
/// pass replaces and later passes keep; `RuleSet` publishes them onto its
/// `ObjectType`. Each pointer is a ReadString into 0x80 bytes with an empty
/// default: an empty read keeps the field, and a value goes through its
/// family's FindOrAllocate, which answers null for `none` and `<none>`
/// (WeaponTypes `0x00772FA0`, BuildingTypes `0x004653C0`, UnitTypes
/// `0x007480D0`, AnimTypes `0x00428B80`, ParticleSystemTypes `0x00644890`).
/// Each list is a ReadString into 0x80 bytes whose nonempty read replaces the
/// list with the non-null answers for its `strtok` tokens. The constructors
/// leave every pointer null and every list empty.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TechnoTypeReferences {
    /// Last nonempty fixed ART Palette request bound by717820. Constructor
    /// store711680 clears DF0;71582B reads DD0 with empty default/capacity32.
    ///717820..71783E keeps DF0 on an empty read, and otherwise replaces it
    /// with6263D0's answer. Asset resolution, not this request, proves load
    /// success (627590 returns null for a missing file). See
    /// tools/rules_oracle/techno_palette_binding.md.
    #[serde(default)]
    pub(super) palette_binding: Option<String>,
    /// `DeathWeapon=` (+D18): TechnoType ReadINI `0x007122D8..0x00712311`
    /// over the constructor's null (`0x007114AE`).
    pub(super) death_weapon: Option<String>,
    /// `DebrisTypes=` (`0x007125F1`), VoxelAnimTypes, and `DebrisAnims=`
    /// (`0x0071274F`), AnimTypes.
    pub(super) debris_types: Vec<String>,
    pub(super) debris_anims: Vec<String>,
    /// `Dock=` (`0x00713180`), BuildingTypes.
    pub(super) dock: Vec<String>,
    /// `DeploysInto=` (+404): `0x00713279..0x00713297`, a BuildingType, over
    /// the constructor's null (`0x00710D61`).
    pub(super) deploys_into: Option<String>,
    /// `UndeploysInto=` (+408, `0x007132B2..0x007132E4`) and `PowersUnit=`
    /// (+40C, `0x007132DF..0x00713309`), UnitTypes.
    pub(super) undeploys_into: Option<String>,
    pub(super) powers_unit: Option<String>,
    /// `Explosion=` (`0x0071399A`) and `DestroyAnim=` (`0x00713A97`),
    /// AnimTypes.
    pub(super) explosion: Vec<String>,
    pub(super) destroy_anim: Vec<String>,
    /// `NaturalParticleSystem=` (+764, `0x00713B9A..0x00713BD5`) and
    /// `RefinerySmokeParticleSystem=` (+774, `0x00713BCD..0x00713C05`), then the
    /// `DamageParticleSystems=` (`0x00713C2C`) and `DestroyParticleSystems=`
    /// (`0x00713D2C`) lists, all ParticleSystemTypes.
    pub(super) natural_particle_system: Option<String>,
    pub(super) refinery_smoke_particle_system: Option<String>,
    pub(super) damage_particle_systems: Vec<String>,
    pub(super) destroy_particle_systems: Vec<String>,
    /// `UnloadingClass=` (+6B8, `0x007146E8..0x0071471A`), a UnitType, and
    /// `DeployingAnim=` (+6BC, `0x00714715..0x0071473F`), an AnimType.
    pub(super) unloading_class: Option<String>,
    /// Ares TechnoTypeExt ctor1003C9E4 clears +1DC. Early reader10041DC0 in
    /// Ext/TechnoType/Hooks.cpp
    /// (hook715320, resumes715326) calls1002EBC0/1002D970 and UnitType
    /// Find1002DCF0, without allocation. Absent/empty/unknown retains the
    /// pointer. The values none/<none> clear it. The draw consumes this pointer.
    #[serde(default)]
    pub(super) water_image: Option<String>,
    pub(super) deploying_anim: Option<String>,
    /// `Enslaves=` (+D40, `0x00714DD7..0x00714DEC`) through the InfantryType
    /// reader `0x0067BAC0`, and `Spawns=` (+D58, `0x00714E9E..0x00714EB3`)
    /// through the AircraftType reader `0x0067BD30`. Both are the same
    /// ReadString128 and factory.
    pub(super) enslaves: Option<String>,
    pub(super) spawns: Option<String>,
    /// `OccupyWeapon=` (+E04) and `EliteOccupyWeapon=` (+E20). Only
    /// InfantryType ReadINI reads them (`0x00524102..0x0052417F`); the
    /// InfantryType constructor nulls both (`0x00523890`, `0x005238B4`).
    pub(super) occupy_weapon: Option<String>,
    pub(super) elite_occupy_weapon: Option<String>,
    /// InfantryType `DeadBodies=` (`0x005241F3`), AnimTypes.
    pub(super) dead_bodies: Vec<String>,
    /// BuildingType `FreeUnit=` (+EA0, `0x00460540..0x0046056D`), a UnitType,
    /// and `ToOverlay=`, which BuildingType ReadINI reads from the fixed ART
    /// section +1F8 names (`0x004611F5..0x0046121F`), an OverlayType.
    pub(super) free_unit: Option<String>,
    pub(super) to_overlay: Option<String>,
    /// Ares TechnoTypeExt+618: ctor1003CF4C clears it; reader1003F3F3
    /// calls1003FA40/1003C650, retaining a common TechnoType pointer.
    #[serde(default)]
    pub(super) convert_script: Option<crate::rules::superweapon_type::DeliveryType>,
    /// Installed Ares TypeExt+610/+614: constructor1003CF40/1003CF46 clears;
    /// Convert.Water1003F3C7 and Convert.Land1003F3DD use lookup1003FA40.
    #[serde(default)]
    pub(super) convert_water: Option<crate::rules::superweapon_type::DeliveryType>,
    #[serde(default)]
    pub(super) convert_land: Option<crate::rules::superweapon_type::DeliveryType>,
}

impl TechnoTypeReferences {
    pub(super) fn new() -> Self {
        Self {
            palette_binding: None,
            death_weapon: None,
            debris_types: Vec::new(),
            debris_anims: Vec::new(),
            dock: Vec::new(),
            deploys_into: None,
            undeploys_into: None,
            powers_unit: None,
            explosion: Vec::new(),
            destroy_anim: Vec::new(),
            natural_particle_system: None,
            refinery_smoke_particle_system: None,
            damage_particle_systems: Vec::new(),
            destroy_particle_systems: Vec::new(),
            unloading_class: None,
            water_image: None,
            deploying_anim: None,
            enslaves: None,
            spawns: None,
            occupy_weapon: None,
            elite_occupy_weapon: None,
            dead_bodies: Vec::new(),
            free_unit: None,
            to_overlay: None,
            convert_script: None,
            convert_water: None,
            convert_land: None,
        }
    }

    /// Publish every retained reference onto the type's `ObjectType`, as
    /// stored IDs.
    pub(crate) fn apply_to(&self, object: &mut crate::rules::object_type::ObjectType) {
        object.project_custom_palette(self.palette_binding.as_deref());
        object.death_weapon.clone_from(&self.death_weapon);
        object.debris_types.clone_from(&self.debris_types);
        object.debris_anims.clone_from(&self.debris_anims);
        object.dock.clone_from(&self.dock);
        object.deploys_into.clone_from(&self.deploys_into);
        object.undeploys_into.clone_from(&self.undeploys_into);
        object.powers_unit.clone_from(&self.powers_unit);
        object.explosion_anims.clone_from(&self.explosion);
        object.destroy_anims.clone_from(&self.destroy_anim);
        object
            .natural_particle_system
            .clone_from(&self.natural_particle_system);
        object
            .refinery_smoke_particle_system
            .clone_from(&self.refinery_smoke_particle_system);
        object
            .damage_particle_systems
            .clone_from(&self.damage_particle_systems);
        object
            .destroy_particle_systems
            .clone_from(&self.destroy_particle_systems);
        object.unloading_class.clone_from(&self.unloading_class);
        object.bind_water_image(self.water_image.as_deref());
        object.deploying_anim.clone_from(&self.deploying_anim);
        object.enslaves.clone_from(&self.enslaves);
        object.spawns.clone_from(&self.spawns);
        object.occupy_weapon.clone_from(&self.occupy_weapon);
        object
            .elite_occupy_weapon
            .clone_from(&self.elite_occupy_weapon);
        object.dead_bodies.clone_from(&self.dead_bodies);
        object.free_unit.clone_from(&self.free_unit);
        object.to_overlay.clone_from(&self.to_overlay);
        // Ares' Convert.* targets; only a read the Ares way sets them.
        object.bind_script_conversion(self.convert_script.as_ref());
        object.bind_surface_conversions(self.convert_water.as_ref(), self.convert_land.as_ref());
    }
}

/// The Type references `AnimTypeClass::ReadINI` reads from the AnimType's
/// fixed ART section (`0x00428045..0x00428735`), retained on the live type:
/// `RuleSet` hands them to its ART registry. Each is a ReadString into 0x80
/// bytes whose empty read keeps the field; `Next=` (+2C8, `0x00428045`) and
/// `Spawns=` (+2F0, `0x004281DE..0x00428216`) go through `AnimTypeClass::FindOrCreate @
/// 0x00428F70`, `BounceAnim=` (`0x0042840D`), `ExpireAnim=` (`0x004284C4`)
/// and `TrailerAnim=` (`0x00428588`) through the inlined AnimType factory,
/// `TiberiumSpawnType=` (+338, `0x0042828A`) through OverlayType's
/// (`0x005FEC70`) and `Warhead=` (`0x0042867A`) through WarheadType's
/// (`0x0075E3B0`); every one of them answers null for `none` and `<none>`.
/// `SpawnsParticle=` (+2CC, `0x00428716..0x00428735`) reads into 0x20 bytes
/// and stores ParticleType FindOrCreate's answer (`0x00645430`), which is
/// null only for `<none>`. The constructor leaves each one null.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AnimTypeReferences {
    pub(super) next: Option<String>,
    pub(super) spawns: Option<String>,
    pub(super) tiberium_spawn_type: Option<String>,
    pub(super) bounce_anim: Option<String>,
    pub(super) expire_anim: Option<String>,
    pub(super) trailer_anim: Option<String>,
    pub(super) warhead: Option<String>,
    pub(super) spawns_particle: Option<String>,
    /// Ares AnimTypeExt `Weapon` (1000A590 -> 1000A370: WeaponType Find,
    /// no allocation), read last by the LoadFromINI hook (0x4287DC/0x4287E9).
    /// `none` clears it; an unknown name keeps it. Only a read the Ares way
    /// sets it.
    #[serde(default)]
    pub(super) weapon: Option<String>,
    /// The one sound slot (+2F8, constructor -1) `AnimTypeClass::Start @
    /// 0x00424CE0` plays: `StartSound=` resolved through VocClass::FindIndex
    /// (`0x007514D0`), else `Report=` (`0x00428344..0x004283BC`).
    pub(super) start_sound: Option<String>,
    /// The sound slot `AnimClass::UnInit @ 0x004255B0` plays (+2FC,
    /// constructor -1): `StopSound=` read the same way, without a fallback
    /// (`0x004283C2..0x00428415`).
    pub(super) stop_sound: Option<String>,
    /// Tiberian Sun's `BounceSound=` and `ExpireSound=`, which gamemd no
    /// longer reads: Firestorm 2.0's AnimTypeClass::Read_INI reads them after
    /// `StartSound=` the way StopSound= is read here (`0x00419181..
    /// 0x00419216`, AnimType +180/+184, constructor -1). See
    /// docs/ts-anim-sounds.md.
    #[serde(default)]
    pub(super) bounce_sound: Option<String>,
    #[serde(default)]
    pub(super) expire_sound: Option<String>,
}

// A stock-way read never sets `weapon`, and no retail ART sets the Tiberian
// Sun sounds, so a stock game's hash folds the stock references alone.
impl Hash for AnimTypeReferences {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.next.hash(state);
        self.spawns.hash(state);
        self.tiberium_spawn_type.hash(state);
        self.bounce_anim.hash(state);
        self.expire_anim.hash(state);
        self.trailer_anim.hash(state);
        self.warhead.hash(state);
        self.spawns_particle.hash(state);
        self.start_sound.hash(state);
        self.stop_sound.hash(state);
        if self.weapon.is_some() {
            self.weapon.hash(state);
        }
        if self.bounce_sound.is_some() || self.expire_sound.is_some() {
            self.bounce_sound.hash(state);
            self.expire_sound.hash(state);
        }
    }
}

impl AnimTypeReferences {
    /// Publish onto the ART registry's runtime record, whose lookups key
    /// every name in upper case.
    pub(crate) fn apply_to(&self, config: &mut crate::rules::art_data::AnimTypeRuntimeConfig) {
        let upper = |name: &Option<String>| name.as_deref().map(str::to_ascii_uppercase);
        config.next = upper(&self.next);
        config.spawns = upper(&self.spawns);
        config.tiberium_spawn_type = upper(&self.tiberium_spawn_type);
        config.bounce_anim = upper(&self.bounce_anim);
        config.expire_anim = upper(&self.expire_anim);
        config.trailer_anim = upper(&self.trailer_anim);
        config.warhead = upper(&self.warhead);
        config.spawns_particle = upper(&self.spawns_particle);
        // Ares AnimTypeExt `Weapon`; None unless read the Ares way.
        config.ares_weapon = self.weapon.clone();
        config.start_sound = upper(&self.start_sound);
        config.stop_sound = upper(&self.stop_sound);
        config.bounce_sound = upper(&self.bounce_sound);
        config.expire_sound = upper(&self.expire_sound);
    }
}

/// The Type references `VoxelAnimTypeClass::ReadINI` retains, each a
/// ReadString into 0x80 bytes whose empty read keeps the field: `Spawns=`
/// (+2D4, `0x0074B1FF..0x0074B224`) through `VoxelAnimTypeClass::FindOrCreate
/// @ 0x0074BB20`, `BounceAnim=` (+2E4, `0x0074B330`), `ExpireAnim=` (+2E8,
/// `0x0074B36F`) and `TrailerAnim=` (+2EC, `0x0074B3AC`) through AnimType's
/// factory (`0x00428B80`), `Warhead=` (+2F8, `0x0074B421`) through
/// WarheadType's and `AttachedSystem=` (+2FC, `0x0074B45E..0x0074B484`)
/// through ParticleSystemType's (`0x00644890`). Each factory answers null for
/// `none` and `<none>`; the constructor leaves each pointer null.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct VoxelAnimTypeReferences {
    pub(super) spawns: Option<String>,
    pub(super) bounce_anim: Option<String>,
    pub(super) expire_anim: Option<String>,
    pub(super) trailer_anim: Option<String>,
    pub(super) warhead: Option<String>,
    pub(super) attached_system: Option<String>,
}

impl VoxelAnimTypeReferences {
    pub(crate) fn apply_to(&self, voxel_anim: &mut crate::rules::voxel_anim_type::VoxelAnimType) {
        voxel_anim.spawns.clone_from(&self.spawns);
        voxel_anim.bounce_anim.clone_from(&self.bounce_anim);
        voxel_anim.expire_anim.clone_from(&self.expire_anim);
        voxel_anim.trailer_anim.clone_from(&self.trailer_anim);
        voxel_anim.warhead.clone_from(&self.warhead);
        voxel_anim.attached_system.clone_from(&self.attached_system);
    }
}

/// The weapon block of `TechnoTypeClass::ReadINI @ 0x00712170`
/// (`0x0071284A..0x00712AAD`), retained on the live Type. Each reached pass
/// reads TurretCount (+808, `0x0071285E`), WeaponCount (+80C, `0x00712878`)
/// and ClearAllWeapons (+A90, `0x007128AC`) with the live values as defaults,
/// then picks the slot keys from those live values:
///
/// - TurretCount > 0 (`0x007128B2`): `Weapon%d` (key `0x00844310`) and
///   `EliteWeapon%d` (`0x00844300`) for 1..=WeaponCount, stored at `+898` and
///   `+A94` plus `(n-1)*0x1C` (`0x0071294A`, `0x00712981`). `Weapon1` and
///   `Primary` are the same storage.
/// - Otherwise, unless ClearAllWeapons (`0x007129A3`): `Primary`, `Secondary`,
///   `ElitePrimary` and `EliteSecondary` into slots 0 and 1 of each array
///   (`0x007129F2`, `0x00712A17`, `0x00712A64`, `0x00712A89`).
/// - Then ClearAllWeapons nulls both arrays' slots 0 and 1
///   (`0x00712A8F..0x00712AAD`), after either branch.
///
/// Each slot read is a ReadString into 0x80 bytes; an empty read keeps the
/// slot, so a pass that omits a key keeps what an earlier pass wrote. A value
/// goes through `WeaponTypeClass::FindOrAllocate @ 0x00772FA0`, which answers
/// null for `none` and `<none>` (stock `[CMIN]` `Primary=none` is unarmed).
/// The TechnoType constructor zeroes TurretCount (`0x0071136F`),
/// ClearAllWeapons (`0x0071137B`) and both 18-entry arrays
/// (`0x007116E6..0x0071173A`); see `ObjectType::weapon_count` for
/// WeaponCount's.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct WeaponSlots {
    pub(super) turret_count: i32,
    pub(super) weapon_count: i32,
    pub(super) clear_all_weapons: bool,
    /// Base (+898) and elite (+A94) arrays, as stored WeaponType IDs.
    pub(super) base: [Option<String>; WEAPON_SLOT_COUNT],
    pub(super) elite: [Option<String>; WEAPON_SLOT_COUNT],
    // Installed Ares loader10040020 and GetWeapon1003DBC0 retain the
    // original eighteen records and own separate resized extension vectors,
    // which stay empty unless read the Ares way.
    #[serde(default)]
    pub(super) extra_base: Vec<Option<String>>,
    #[serde(default)]
    pub(super) extra_elite: Vec<Option<String>>,
    #[serde(default)]
    pub(super) offsets: crate::rules::weapon_offsets::WeaponOffsets,
}

impl WeaponSlots {
    pub(crate) fn offsets(&self) -> &crate::rules::weapon_offsets::WeaponOffsets {
        &self.offsets
    }

    pub(crate) fn turret_count(&self) -> i32 {
        self.turret_count
    }

    pub(crate) fn weapon_count(&self) -> i32 {
        self.weapon_count
    }

    pub(crate) fn base(&self) -> Vec<Option<String>> {
        self.base.iter().chain(&self.extra_base).cloned().collect()
    }

    /// Whether the Ares extension slots past the eighteen hold anything.
    pub(super) fn has_extension_slots(&self) -> bool {
        !self.extra_base.is_empty() || !self.extra_elite.is_empty()
    }

    pub(crate) fn elite(&self) -> Vec<Option<String>> {
        self.elite
            .iter()
            .chain(&self.extra_elite)
            .cloned()
            .collect()
    }

    /// Ares Prism SupportWeapon reader `100103F0`: GetUnusedWeaponSlot
    /// (inlined) answers the first empty slot of 2..17 of the base or elite
    /// array.
    pub(super) fn unused_prism_slot(&self, elite: bool) -> Option<usize> {
        let slots = if elite { &self.elite } else { &self.base };
        (2..WEAPON_SLOT_COUNT).find(|&slot| slots[slot].is_none())
    }

    /// `100103F0` stores the support weapon in `slot` of the base or elite
    /// array, with AlternateFLH0 (base) or AlternateFLH1 (elite) as its FLH,
    /// else the slot-0 coordinate of the same array.
    pub(super) fn install_prism_support_weapon(
        &mut self,
        slot: usize,
        elite: bool,
        weapon: String,
    ) {
        let slots = if elite {
            &mut self.elite
        } else {
            &mut self.base
        };
        if let Some(entry) = slots.get_mut(slot) {
            *entry = Some(weapon);
            self.offsets.copy_support_flh(slot, elite);
        }
    }
}

/// One WeaponType's retained reads ([`WeaponReadState`]) with its references
/// as stored IDs, for `RuleSet`.
pub(crate) struct WeaponState<'a> {
    pub(crate) id: &'a str,
    pub(crate) damage: i32,
    pub(crate) speed: i32,
    pub(crate) range_leptons: i32,
    pub(crate) projectile: Option<&'a str>,
    pub(crate) warhead: Option<&'a str>,
    pub(crate) anim: &'a [String],
    pub(crate) assault_anim: Option<&'a str>,
    pub(crate) occupant_anim: Option<&'a str>,
    pub(crate) open_topped_anim: Option<&'a str>,
    pub(crate) attached_particle_system: Option<&'a str>,
    pub(crate) ares_bolt_particle_system: &'a Option<Option<String>>,
    pub(crate) ares_ivan_bomb_warhead: &'a Option<Option<String>>,
    pub(crate) ares_abductor_anim: Option<&'a str>,
    pub(crate) ares_cursors: crate::rules::ares_mouse_cursor::WeaponCursors,
}

/// Fields needed by Weapon7729F0, retained on the same live Weapon object.
/// The projectile handle is an allocation index, not a relookup of a truncated
/// name: repeated names longer than 24 bytes can allocate distinct native types.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct WeaponReadState {
    pub(super) speed: i32,
    /// +B4, ReadRange (`0x00474620`) at `0x00772331` with the retained value
    /// as its default, over the constructor's zero (`0x00771CB6`): leptons,
    /// `-1` keeps the field.
    pub(super) range_leptons: i32,
    pub(super) projectile: Option<usize>,
    /// Weapon771C70 initializes +A4/+AC to zero/null. ReadINI7722D4..7722E8
    /// reads Damage with its current signed value;77294C..772981 only replaces
    /// the Warhead pointer after a nonempty ReadString128. Executed controls:
    /// spatial_oracle/trigger_do_explosion.json (Steam gamemd).
    pub(super) damage: i32,
    pub(super) warhead: Option<usize>,
    /// `Anim=` (`0x00772471`): ReadString128, then each `strtok` token's
    /// non-null AnimType (`0x00428B80`) replaces the list.
    pub(super) anim: Vec<String>,
    /// `WeaponTypeClass::ReadINI @ 0x00772080` reads `AssaultAnim=` with
    /// ReadString128 (`0x00528A10`) and uses AnimTypeClass::FindOrAllocate
    /// (`0x00428B80`) for nonempty names; empty retains the current reference.
    pub(super) assault_anim: Option<String>,
    /// `OccupantAnim=` (+110, `0x007725A1..0x007725CB`) and `OpenToppedAnim=`
    /// (+118, `0x007725E6`): ReadString128, then the AnimType factory, which
    /// answers null for `none` and `<none>`; an empty read keeps the field.
    pub(super) occupant_anim: Option<String>,
    pub(super) open_topped_anim: Option<String>,
    /// `AttachedParticleSystem=` (+11C, `0x0077290B..0x0077293C`): ReadString
    /// into 0x14 bytes over an empty default, then
    /// `ParticleSystemTypeClass::FindOrCreate @ 0x00644630`. Its -1 for an
    /// empty name or `<none>` keeps the field; `none`, like any other name,
    /// finds or constructs a ParticleSystemType of that name.
    pub(super) attached_particle_system: Option<String>,
    /// Ares `Bolt.ParticleSystem` (WeaponTypeExt +0x30), a
    /// `Nullable<ParticleSystemTypeClass*>` whose Find never allocates;
    /// `Some(None)` is an explicit `none`.
    #[serde(default)]
    pub(super) ares_bolt_particle_system: Option<Option<String>>,
    /// Ares `IvanBomb.Warhead` (WeaponTypeExt +0x8C), the same
    /// non-allocating Nullable Find over the Warhead family.
    #[serde(default)]
    pub(super) ares_ivan_bomb_warhead: Option<Option<String>>,
    /// Ares `Abductor.Anim` (WeaponTypeExt +0xC4), a Valueable whose Find
    /// never allocates: `none` stores null, an unknown name retains.
    #[serde(default)]
    pub(super) ares_abductor_anim: Option<String>,
    /// Ares `Cursor.Attack`/`Cursor.AttackOutOfRange` (WeaponTypeExt
    /// +0xD4/+0xD8), the last reads of 10055520.
    #[serde(default)]
    pub(super) ares_cursors: crate::rules::ares_mouse_cursor::WeaponCursors,
}

impl ProcessedType {
    pub(super) fn read_object_sound(&mut self, section: &IniSection, sounds: &SoundRegistry) {
        // ObjectTypeClass::ReadINI5F92D0: ReadString128 then VocFind7514D0.
        // All subclasses share this field, their own admitted section and
        // lifetime. A missing/empty/unknown read keeps the previous signed ID.
        self.object_crush_sound =
            sounds.read_rules_reference(section, "CrushSound", self.object_crush_sound.take());
    }

    pub(super) fn terrain_projection(
        &self,
    ) -> Option<crate::rules::terrain_object_type::TerrainObjectType> {
        let mut terrain = crate::rules::terrain_object_type::TerrainObjectType::from_retained(
            &self.native_stored_id,
            &self.body,
            self.terrain_type_read_state.as_ref()?,
            &self.object_image,
        );
        terrain.bind_armor(self.armor_id);
        Some(terrain)
    }

    pub(super) fn new(native_stored_id: String) -> Self {
        Self {
            threat_coefficients: Default::default(),
            body: IniSection::new(native_stored_id.clone()),
            object_image: ObjectImageState::new(&native_stored_id),
            object_crush_sound: None,
            aircraft_trailer: AircraftTrailer::default(),
            overlay_damage_levels: 1,
            overlay_explodes: false,
            overlay_cell_anim: None,
            voxel_shadow_index: 0,
            tech_level: crate::rules::object_type::DEFAULT_TECH_LEVEL,
            projectile_art: ProjectileArtState::new(),
            techno_references: TechnoTypeReferences::new(),
            native_stored_id,
            country_color_scheme: 0,
            tiberium_color_scheme: 0,
            tiberium_debris: Vec::new(),
            tiberium_image: Default::default(),
            tiberium_ext: Default::default(),
            country_side_index: -1,
            country_parent: String::new(),
            country_veterancy: CountryVeterancyState::default(),
            factory_owners: None,
            explicit_factory: None,
            side_country_members: Vec::new(),
            ares_side: None,
            anim_art_read: false,
            terrain_type_read_state: None,
            building_foundation: crate::rules::foundation::Foundation::default(),
            building_target_coord_offset: [0; 3],
            building_half_damage_smoke_location1: [0; 3],
            building_half_damage_smoke_location2: [0; 3],
            building_has_spotlight: false,
            building_extra_power: Default::default(),
            building_light: Default::default(),
            building_upgrade: Default::default(),
            secret_lab: Default::default(),
            unit_shp: Default::default(),
            unit_speed_type: SpeedType::Invalid,
            gunner_turrets: GunnerTurrets::default(),
            is_gattling: false,
            gattling_stages: Default::default(),
            infantry_pips: InfantryPips::default(),
            infantry_civilian: false,
            infantry_damage_sparks: false,
            infantry_infiltration: InfantryInfiltration::default(),
            infantry_art: InfantryArt::default(),
            infantry_sequence_bank: None,
            recoil: crate::rules::recoil::RecoilConfig::default(),
            airstrike: crate::rules::airstrike::AirstrikeSettings::default(),
            radial_fire_segments: 0,
            lepton_mind_control_offset:
                crate::rules::object_type::DEFAULT_LEPTON_MIND_CONTROL_OFFSET,
            prerequisite: Vec::new(),
            prerequisite_override: Vec::new(),
            weapon_slots: WeaponSlots::default(),
            weapon: WeaponReadState::default(),
            warhead_anim: WarheadAnimReadState::default(),
            warhead_particle: None,
            warhead_verses: [NativeF64Bits::ONE; 11],
            ares_verses: None,
            ares_warhead: None,
            emp_type: None,
            promotion_abilities: None,
            self_attach_effect: None,
            super_weapon_techno: Default::default(),
            cloak_allowed: true,
            cloak_power: Default::default(),
            turret_rot_override: None,
            spotlight: Default::default(),
            driver: Default::default(),
            selection_group: String::new(),
            bounty: Default::default(),
            crew: Default::default(),
            infiltration: Default::default(),
            building_type_ext: Default::default(),
            ares_warhead_loaded: false,
            veterancy: Default::default(),
            academy: Default::default(),
            production: Default::default(),
            aircraft_crashable: None,
            techno_combat: Default::default(),
            ares_cursors: Default::default(),
            ares_super_weapon_cursors: Default::default(),
            curtain_modifiers: default_curtain_modifiers(),
            armor_id: ArmorId::NONE,
            holds_what: None,
            particle_read: Default::default(),
            particle_next: None,
            particle_system_behavior: Default::default(),
            super_weapon_weapon: None,
            super_weapon_aux_building: None,
            unit_delivery: None,
            hunter_seeker: None,
            super_weapon_building_requirements: None,
            super_weapon_availability: AresSuperWeaponAvailability::default(),
            super_weapon_influence: Default::default(),
            super_weapon_presentation: AresSuperWeaponPresentation::default(),
            super_weapon_targeting: AresSuperWeaponTargetingState::default(),
            super_weapon_charge: AresSuperWeaponCharge::default(),
            super_weapon_launch: Default::default(),
            #[cfg(feature = "phobos")]
            super_weapon_phobos_availability: PhobosSuperWeaponAvailability::default(),
            additional_super_weapons: Vec::new(),
            #[cfg(feature = "phobos")]
            ore_gathering: crate::rules::object_type::PhobosOreGathering::default(),
            #[cfg(feature = "phobos")]
            phobos_insignia: Default::default(),
            ares_insignia: Default::default(),
            generic_super_weapon_warhead: None,
            particle_warhead: None,
            voxel_anim_references: VoxelAnimTypeReferences::default(),
            voxel_anim_damage: 0,
            voxel_anim_angular_velocities:
                crate::rules::voxel_anim_type::DEFAULT_ANGULAR_VELOCITIES,
            voxel_anim_read: false,
            voxel_anim_share_flags: [false; 3],
            anim_references: AnimTypeReferences::default(),
            shrapnel_weapon: None,
            airburst_weapon: None,
        }
    }
}

/// One family lookup/constructor for Process and late native factories.
pub(super) fn find_existing_type(
    families: &HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    family: RulesTypeFamily,
    incoming: &str,
) -> Option<usize> {
    families
        .get(&family)?
        .iter()
        .position(|member| member.native_stored_id.eq_ignore_ascii_case(incoming))
}

/// The member at `index` of `family`, for the pass reading it. Every index a
/// pass holds came from that family's own list, so the family is present.
/// Takes the map, not the processor, so the caller can borrow its other
/// fields beside it.
pub(super) fn read_member(
    families: &mut HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    family: RulesTypeFamily,
    index: usize,
) -> &mut ProcessedType {
    &mut families
        .get_mut(&family)
        .expect("a pass's member index comes from its family's list")[index]
}

pub(super) fn find_or_construct_type(
    families: &mut HashMap<RulesTypeFamily, Vec<ProcessedType>>,
    techno_type_order: &mut Vec<(RulesTypeFamily, usize)>,
    events: &mut Vec<NativeTypeConstructionEvent>,
    family: RulesTypeFamily,
    incoming: &str,
) -> usize {
    if let Some(index) = find_existing_type(families, family, incoming) {
        return index;
    }
    let members = families.entry(family).or_default();
    // AbstractTypeClass::Constructor @ 0x00410800 stores only 0x18 bytes.
    // Lookup above compares that stored ID against the full input, so a
    // repeated >24-byte spelling can construct another equal stored ID.
    let native_stored_id = abstract_type_stored_id(incoming);
    let index = members.len();
    members.push(ProcessedType::new(native_stored_id.clone()));
    if family == RulesTypeFamily::Infantry {
        members[index].infantry_sequence_bank = Some(Default::default());
    }
    // Ares' type extensions are constructed with their types when the Rules
    // are read the Ares way.
    let ares_way = crate::rules::ares_switch::ares_way();
    if ares_way && family == RulesTypeFamily::Side {
        members[index].ares_side = Some(AresSideState::default());
    }
    if ares_way && family == RulesTypeFamily::Country {
        let ares = &mut members[index].country_veterancy.ares;
        ares.loading =
            crate::rules::country_loading::AresCountryLoading::for_country_id(&native_stored_id);
        ares.presentation =
            crate::rules::ares_country_presentation::AresCountryPresentation::for_country_id(
                &native_stored_id,
            );
    }
    if ares_way && family == RulesTypeFamily::Building {
        members[index].explicit_factory = Some(AresExplicitFactory::default());
    }
    if family == RulesTypeFamily::Terrain {
        members[index].armor_id = ArmorId::WOOD;
        members[index].terrain_type_read_state =
            Some(crate::rules::terrain_object_type::TerrainTypeReadState::new());
    }
    if ares_way && family == RulesTypeFamily::Warhead {
        members[index].ares_verses = Some(AresWarheadVerses::default());
        members[index].ares_warhead = Some(Default::default());
    }
    if family.object_category().is_some() {
        if ares_way {
            members[index].emp_type = Some(Default::default());
            members[index].promotion_abilities = Some(Default::default());
            members[index].self_attach_effect = Some(Default::default());
            members[index].factory_owners = Some(AresFactoryOwners::default());
        }
        // TechnoType constructors71181D..711832 /711ABD..711ACB append
        // to the shared vector at allocation time, including lazy Types.
        techno_type_order.push((family, index));
    }
    if let Some(family) = family.native_constructor_family() {
        events.push(NativeTypeConstructionEvent {
            family,
            native_stored_id,
        });
    }
    index
}
