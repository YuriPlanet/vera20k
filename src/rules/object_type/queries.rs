//! `ObjectType` queries: the accessors and derived answers the simulation, AI,
//! render and UI read from a parsed type (weapons, voices and sounds, pips,
//! factory roles, extension keys), plus the projections `RuleSet` stamps in.

use crate::rules::ini_parser::{IniSection, is_native_none_type_name};
use crate::rules::sound_ini::SoundRegistry;
use crate::util::native_x87::NativeF64Bits;

#[cfg(feature = "phobos")]
use super::PhobosOreGathering;
use super::WEAPON_SLOT_PRIMARY;
#[cfg(any(test, feature = "test-support"))]
use super::WEAPON_SLOT_SECONDARY;
use super::{BuildCategory, DOCK_CONTACT_LIMIT, FactoryType, HouseMasks, InfantryArt};
use super::{InfantryInfiltration, InfantryPips, ObjectCategory, ObjectType, PipScale};

impl ObjectType {
    pub(crate) fn threat_coefficients(&self) -> super::threat_coefficients::ThreatCoefficients {
        self.threat_coefficients
    }

    pub(crate) fn project_threat_coefficients(
        &mut self,
        coefficients: super::threat_coefficients::ThreatCoefficients,
    ) {
        self.threat_coefficients = coefficients;
    }

    /// Phobos `AllowAirstrike` as the cursor and `CanFire` tests read it: an
    /// explicit value wins, a building otherwise follows `CanC4`, and every
    /// other kind is targetable by default.
    #[cfg(feature = "phobos")]
    pub fn allows_airstrike(&self, is_building: bool) -> bool {
        self.allow_airstrike
            .unwrap_or(if is_building { self.can_c4 } else { true })
    }

    /// The weapon-choice reading of `AllowAirstrike`. Its vanilla default also
    /// refuses a building that is both a resource destination and a resource
    /// gatherer (`0x6F3477`), which the cursor test above does not apply.
    #[cfg(feature = "phobos")]
    pub fn allows_airstrike_weapon(&self, is_building: bool) -> bool {
        self.allow_airstrike.unwrap_or(if is_building {
            self.can_c4 && (!self.resource_destination || !self.resource_gatherer)
        } else {
            true
        })
    }

    pub(crate) fn bind_weapon_offsets(
        &mut self,
        offsets: &crate::rules::weapon_offsets::WeaponOffsets,
    ) {
        self.weapon_offsets = offsets.clone();
    }

    /// GetFLH6F3B28 after GetWeapon has qualified the base or elite record.
    pub fn weapon_flh(&self, index: i32, use_elite: bool) -> crate::rules::flh::Flh {
        self.weapon_offsets.weapon(index, use_elite)
    }

    /// GetFLH6F3AF5: an open transport lends its zero-based cargo port.
    pub fn alternate_fire_flh(&self, port: usize) -> crate::rules::flh::Flh {
        self.weapon_offsets.alternate(port)
    }

    /// Native TechnoType `PitchSpeed` double used by the normal Fly draw gate.
    pub fn fly_pitch_speed(&self) -> f64 {
        f64::from_bits(self.pitch_speed.bits())
    }

    /// Native TechnoType `RollAngle` in radians for normal Fly presentation.
    pub fn fly_roll_angle_radians(&self) -> f64 {
        f64::from_bits(self.roll_angle.bits())
    }

    /// Fly4CF7C5..4CF801 stores RollAngle to binary32 with native chop before
    /// table trig. Negating this stored finite value also preserves the
    /// negative-turn arm's sign-before-store result.
    pub fn fly_roll_angle_f32(&self) -> f32 {
        use crate::util::native_x87::X87Chop53 as X;
        // `RollAngle=1e999` reads as infinity and `1e41` overflows the
        // binary32 store; native FSTP stores an indefinite value there. A
        // rules file is outside data: the angle is zero instead of a panic.
        X::load_f64(self.roll_angle)
            .ok()
            .and_then(|angle| X::store_f32(angle).ok())
            .map_or(0.0, |stored| f32::from_bits(stored.bits()))
    }

    /// Existing typed PitchAngle value shared with Fly approach and normal
    /// Fly rendering. Rendering converts only at the f32 matrix boundary.
    pub fn fly_pitch_angle_radians(&self) -> f32 {
        self.pitch_angle.to_num::<f32>()
    }

    pub(crate) fn project_image_state(
        &mut self,
        state: &crate::rules::object_image::ObjectImageState,
    ) {
        self.image = state.image().to_string();
        self.shp_image_loads = state.shp_loads().to_vec();
        self.voxel_image_load = state.voxel_load().map(str::to_string);
        self.voxel_model_slots = state.model_slots.clone();
    }

    /// Immutable load request consumed by the client SHP atlas.
    pub fn shp_image_loads(&self) -> &[crate::rules::object_image::ObjectImageLoad] {
        &self.shp_image_loads
    }

    /// The voxel this scenario's `TechnoTypeClass::ReadINI` loaded
    /// (`0x0071609C`), if a pass reached the load.
    pub fn voxel_image_load(&self) -> Option<&str> {
        self.voxel_image_load.as_deref()
    }

    pub(crate) fn additional_super_weapons(&self) -> &[String] {
        &self.additional_super_weapons
    }

    pub(crate) fn project_additional_super_weapons(&mut self, names: &[String]) {
        self.additional_super_weapons = names.to_vec();
    }

    /// Phobos `UnitTypeExt`'s gather-overlay lists. Every list is empty for a
    /// type that authors none, which is the constructor state.
    #[cfg(feature = "phobos")]
    pub fn ore_gathering(&self) -> &PhobosOreGathering {
        &self.ore_gathering
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn project_ore_gathering(&mut self, gathering: &PhobosOreGathering) {
        self.ore_gathering = gathering.clone();
    }
    pub(crate) fn emp_rules(&self) -> Option<&crate::rules::ares_emp::EmpTypeRules> {
        self.emp.as_ref()
    }
    pub(crate) fn promotion_abilities(
        &self,
    ) -> Option<&crate::rules::ares_abilities::PromotionAbilities> {
        self.promotion_abilities.as_ref()
    }
    pub(crate) fn self_attach_effect(
        &self,
    ) -> Option<&crate::rules::ares_warhead::AttachEffectRules> {
        self.self_attach_effect.as_ref()
    }
    pub(crate) fn cloak_allowed(&self) -> bool {
        self.cloak_allowed
    }
    pub(crate) fn bind_ares_spotlight(
        &mut self,
        enabled: bool,
        options: crate::rules::ares_spotlight::SpotlightTypeRules,
    ) {
        self.has_spotlight = enabled;
        self.spotlight = options;
    }

    pub(crate) fn spotlight_rules(&self) -> crate::rules::ares_spotlight::SpotlightTypeRules {
        self.spotlight
    }

    pub(crate) fn driver_rules(&self) -> &crate::rules::ares_driver::DriverTypeRules {
        &self.driver
    }

    pub(crate) fn bounty_rules(&self) -> &crate::rules::ares_bounty::BountyTypeRules {
        &self.bounty
    }

    pub(crate) fn bind_bounty_rules(
        &mut self,
        bounty: &crate::rules::ares_bounty::BountyTypeRules,
    ) {
        self.bounty = bounty.clone();
    }

    /// AircraftType's retained fixed-ART Trailer and signed SpawnDelay.
    pub fn aircraft_trailer(&self) -> &crate::rules::aircraft_trailer::AircraftTrailer {
        &self.aircraft_trailer
    }

    pub(crate) fn project_aircraft_trailer(
        &mut self,
        trailer: &crate::rules::aircraft_trailer::AircraftTrailer,
    ) {
        self.aircraft_trailer.clone_from(trailer);
    }

    pub(crate) fn aircraft_crashable_override(&self) -> Option<bool> {
        self.aircraft_crashable
    }

    /// Ares `IronCurtain.Modifier` and `ForceShield.Modifier`, in that order.
    pub(crate) fn ares_curtain_modifiers(&self) -> [NativeF64Bits; 2] {
        self.curtain_modifiers
    }

    pub(crate) fn crew_rules(&self) -> &crate::rules::ares_crew::CrewTypeRules {
        &self.crew
    }

    pub(crate) fn bind_crew_rules(&mut self, crew: &crate::rules::ares_crew::CrewTypeRules) {
        self.crew = crew.clone();
    }

    /// The type's TechnoTypeExt combat and movement keys.
    pub(crate) fn techno_combat(&self) -> &crate::rules::ares_techno_type::AresTechnoTypeRules {
        &self.techno_combat
    }

    /// Ares TechnoTypeExt `PrerequisiteLists` (+0x40): `Prerequisite=`
    /// (list 0, also `Prerequisite.List0=`), then `Prerequisite.List1=` on.
    /// A human needs any one of them on the map (`PrerequisitesMet @
    /// 10021CA0`); the planner any one among its types (`10021C50`).
    pub(crate) fn ares_prerequisite_lists(
        &self,
    ) -> impl Iterator<Item = &[crate::rules::prerequisite::Prerequisite]> {
        std::iter::once(self.prerequisite.as_slice()).chain(
            self.techno_combat
                .prerequisites()
                .extra_lists()
                .iter()
                .map(Vec::as_slice),
        )
    }

    pub(crate) fn bind_techno_combat_rules(
        &mut self,
        rules: &crate::rules::ares_techno_type::AresTechnoTypeRules,
    ) {
        self.techno_combat = rules.clone();
    }

    pub(crate) fn cloak_power(&self) -> &crate::rules::ares_cloak_power::CloakPowerTypeRules {
        &self.cloak_power
    }

    pub(crate) fn bind_cloak_power_rules(
        &mut self,
        rules: &crate::rules::ares_cloak_power::CloakPowerTypeRules,
    ) {
        self.cloak_power = rules.clone();
    }

    /// The progress stages of this type's cloak: Ares Cloakable.Stages
    /// (StartUncloaking hook7036EB ->10048400, VisualCharacter hook703A79 ->
    /// 100484D0), else `[General]CloakingStages`.
    pub fn cloaking_stages(&self, general: i32) -> i32 {
        self.cloak_power.cloaking_stages(general)
    }

    /// Ares `SensorArray.Warn=` (TypeExt+204, default true): a sensed
    /// enemy of this type raises the radar event (hook70DA95).
    pub fn sensor_array_warn(&self) -> bool {
        self.cloak_power.sensor_array_warn()
    }

    /// The cloak (`false`) or decloak (`true`) sound override, `None` when
    /// unset. Hooks70380A/70375B use it before their Rules fallbacks.
    pub(crate) fn cloak_sound_override(&self, decloak: bool) -> Option<Option<&str>> {
        self.cloak_sounds[usize::from(decloak)]
            .as_ref()
            .map(|cue| cue.as_deref())
    }

    pub(crate) fn bind_driver_rules(
        &mut self,
        driver: &crate::rules::ares_driver::DriverTypeRules,
    ) {
        self.driver = driver.clone();
    }

    pub(crate) fn bind_selection_group(&mut self, group: &str) {
        group.clone_into(&mut self.selection_group);
    }

    /// Installed Ares GetSelectionGroupID1003DB30: an empty override uses
    /// the type's stored ID, not its localized name or another type's group.
    pub(crate) fn type_selection_key(&self) -> &str {
        if self.selection_group.is_empty() {
            &self.id
        } else {
            &self.selection_group
        }
    }

    pub(crate) fn bind_ares_techno_rules(
        &mut self,
        emp: Option<&crate::rules::ares_emp::EmpTypeRules>,
        abilities: Option<&crate::rules::ares_abilities::PromotionAbilities>,
        attachment: Option<&crate::rules::ares_warhead::AttachEffectRules>,
        cloak_allowed: bool,
        turret_rot_override: Option<i32>,
        aircraft_crashable: Option<bool>,
        curtain_modifiers: [NativeF64Bits; 2],
    ) {
        self.emp = emp.cloned();
        if crate::rules::ares_switch::ares_way() {
            self.promotion_abilities = abilities.cloned();
            self.self_attach_effect = attachment.cloned();
            self.cloak_allowed = cloak_allowed;
            self.turret_rot_override = turret_rot_override;
            self.aircraft_crashable = aircraft_crashable;
            self.curtain_modifiers = curtain_modifiers;
        }
    }

    pub(crate) fn script_conversion(
        &self,
    ) -> Option<&crate::rules::superweapon_type::DeliveryType> {
        self.convert_script.as_ref()
    }

    /// Retained nullable input; preserve presence independently of ROT.
    pub(crate) fn turret_rot_override(&self) -> Option<i32> {
        self.turret_rot_override
    }

    /// Ares conversion1004380E..10043839 selects the optional secondary ROT.
    pub(crate) fn secondary_turn_rate(&self) -> i32 {
        self.turret_rot_override.unwrap_or(self.turret_rot)
    }

    pub(crate) fn bind_script_conversion(
        &mut self,
        target: Option<&crate::rules::superweapon_type::DeliveryType>,
    ) {
        self.convert_script = target.cloned();
    }

    pub(crate) fn bind_super_weapon_techno_rules(
        &mut self,
        retained: &crate::rules::superweapon_influence::TechnoSuperWeaponRules,
    ) {
        self.designator_range = retained.designator_range;
        self.inhibitor_range = retained.inhibitor_range;
        self.hunter_seeker_ignore = retained.hunter_seeker_ignore;
        self.chronoshift = retained.chronoshift;
    }

    /// TechnoTypeExt `Chronoshift.Allow`/`.IsVehicle`/`.Crushable`.
    pub(crate) fn ares_chronoshift(&self) -> crate::rules::superweapon_influence::ChronoshiftRules {
        self.chronoshift
    }

    pub(crate) fn hunter_seeker_ignored(&self) -> bool {
        self.hunter_seeker_ignore
    }

    pub(crate) fn designator_range(&self) -> Option<i32> {
        self.designator_range
    }

    pub(crate) fn inhibitor_range(&self) -> Option<i32> {
        self.inhibitor_range
    }

    pub(crate) fn surface_conversion(
        &self,
        water: bool,
    ) -> Option<&crate::rules::superweapon_type::DeliveryType> {
        if water {
            self.convert_water.as_ref()
        } else {
            self.convert_land.as_ref()
        }
    }

    pub(crate) fn bind_surface_conversions(
        &mut self,
        water: Option<&crate::rules::superweapon_type::DeliveryType>,
        land: Option<&crate::rules::superweapon_type::DeliveryType>,
    ) {
        self.convert_water = water.cloned();
        self.convert_land = land.cloned();
    }

    /// `NoSpawnAlt=yes`.
    pub fn no_spawn_alt(&self) -> bool {
        self.no_spawn_alt_model.is_some()
    }

    /// The `<TYPE>WO` art a `NoSpawnAlt=yes` type draws while no spawn is
    /// docked, named after the section.
    pub fn no_spawn_alt_model(&self) -> Option<&str> {
        self.no_spawn_alt_model.as_deref()
    }

    /// `SmallVisceroid=` or `LargeVisceroid=` of a VehicleType (UnitType
    /// `+0xE18`/`+0xE19`); the SHP Unit shadow refuses both
    /// (`0x00705FF3..0x00706007`, and Ares' `UnitClass_DrawSHP_DrawShadowEarlier`).
    pub fn visceroid(&self) -> bool {
        self.small_visceroid || self.large_visceroid
    }

    /// Read-only projection of the retained WaterImage pointer.
    pub fn water_image(&self) -> Option<&str> {
        self.water_image.as_deref()
    }

    pub(crate) fn bind_water_image(&mut self, image: Option<&str>) {
        self.water_image = image.map(str::to_owned);
    }

    /// Retained TechnoType+C9E; building crew admission reads this input
    /// separately from the constructed Infantry's survivor marker.
    pub const fn nominal(&self) -> bool {
        self.nominal
    }

    /// Ares `Saboteur=` ([`Self::saboteur`] field doc).
    pub(crate) const fn saboteur(&self) -> bool {
        self.saboteur
    }

    pub(crate) fn infiltration_rules(
        &self,
    ) -> &crate::rules::ares_infiltration::InfiltrationTypeRules {
        &self.infiltration
    }

    /// Ares `SpyEffect.RevealProduction`, which the status text asks.
    pub fn spy_reveals_production(&self) -> bool {
        self.infiltration.reveal_production
    }

    pub(crate) fn bind_infiltration_rules(
        &mut self,
        infiltration: &crate::rules::ares_infiltration::InfiltrationTypeRules,
    ) {
        self.infiltration = infiltration.clone();
    }

    /// Ares BuildingTypeExt rubble and repair keys (`rules::ares_building_type`).
    pub(crate) fn building_type_ext(
        &self,
    ) -> &crate::rules::ares_building_type::BuildingTypeExtRules {
        &self.building_type_ext
    }

    /// BuildingTypeExt `Fake=`, which DrawExtraInfo (Ares 10015F10, hook
    /// 0x70AA60) labels `TXT_FAKE` in place of its other texts.
    pub fn ares_fake(&self) -> bool {
        self.building_type_ext.fake()
    }

    /// BuildingTypeExt `Firestorm.Wall` (`sim::ares_firestorm`), for the
    /// client's health bar (hook `0x6F64CB`).
    pub fn ares_firestorm_wall(&self) -> bool {
        self.building_type_ext.firestorm_wall
    }

    pub(crate) fn bind_building_type_ext(
        &mut self,
        ext: &crate::rules::ares_building_type::BuildingTypeExtRules,
    ) {
        self.building_type_ext = ext.clone();
    }

    /// Ares CompleteInitialization (`1000D380`, hook 0x679CAF) over this
    /// building's final foundation; `foundation_of` resolves a BuildingType
    /// by name.
    pub(crate) fn complete_building_type_ext<'a>(
        &mut self,
        foundation_of: impl Fn(&str) -> Option<&'a crate::rules::foundation::Foundation>,
    ) {
        self.building_type_ext
            .complete_initialization(&self.id, &self.foundation, foundation_of);
    }

    /// Ares Experience.*/Promote.* (`rules::ares_veterancy`).
    pub(crate) fn ares_veterancy(&self) -> &crate::rules::ares_veterancy::VeterancyTypeRules {
        &self.veterancy
    }

    pub(crate) fn bind_ares_veterancy(
        &mut self,
        veterancy: &crate::rules::ares_veterancy::VeterancyTypeRules,
    ) {
        self.veterancy = veterancy.clone();
    }

    /// The EVA cue a promotion to veteran or elite queues, `None` silent.
    pub(crate) fn promotion_eva(&self, elite: bool) -> Option<&str> {
        self.promotion_eva[usize::from(elite)].as_deref()
    }

    pub(crate) fn bind_promotion_eva(&mut self, eva: [Option<String>; 2]) {
        self.promotion_eva = eva;
    }

    /// Ares BuildingTypeExt `LostEvaEvent`.
    pub(crate) fn lost_eva(&self) -> Option<&str> {
        self.lost_eva.as_deref()
    }

    pub(crate) fn bind_lost_eva(&mut self, eva: Option<String>) {
        self.lost_eva = eva;
    }

    /// Ares TechnoTypeExt `EnemyUIName`, the CSF label a non-allied player
    /// reads for a building of this type (BuildingClass_GetUIName, Ares.dll
    /// 10015DA0, hook 0x459ED0).
    pub fn enemy_ui_name(&self) -> Option<&str> {
        self.techno_combat.presentation().enemy_ui_name()
    }

    /// Ares BuildingTypeExt `SlamSound`: `None` keeps `[AudioVisual]
    /// BuildingSlam`, `Some(None)` silences it.
    pub fn slam_sound(&self) -> Option<Option<&str>> {
        self.building_type_ext.slam_sound()
    }

    /// Ares TechnoTypeExt `EVA.Lost` (`rules::ares_techno_type`).
    pub(crate) fn bind_unit_lost_eva(&mut self, eva: Option<String>) {
        self.techno_combat.bind_unit_lost_eva(eva);
    }

    /// Ares TechnoTypeExt `EVA.Lost`: `Some(None)` is the silent -1, `None`
    /// rules read without the EVA catalog, where Initialize's seed applies
    /// (`EVA_UnitLost` for a non-building, -1 for a building).
    pub(crate) fn unit_lost_eva(&self) -> Option<Option<&str>> {
        self.techno_combat.unit_lost_eva()
    }

    /// Ares BuildingTypeExt Academy.* (`rules::ares_veterancy`).
    pub(crate) fn academy(&self) -> &crate::rules::ares_veterancy::AcademyTypeRules {
        &self.academy
    }

    /// Ares BuildTime.*, FactoryPlant.Multiplier and cloning
    /// (`rules::ares_production`).
    pub(crate) fn ares_production(&self) -> &crate::rules::ares_production::ProductionTypeRules {
        &self.production
    }

    pub(crate) fn bind_ares_production(
        &mut self,
        production: &crate::rules::ares_production::ProductionTypeRules,
    ) {
        self.production = production.clone();
    }

    /// The type's Ares `[MouseCursors]` keys (`Cursor.Move`, `Cursor.Spy`, ...).
    pub fn ares_cursors(&self) -> &crate::rules::ares_mouse_cursor::TechnoCursors {
        &self.ares_cursors
    }

    pub(crate) fn bind_ares_cursors(
        &mut self,
        cursors: crate::rules::ares_mouse_cursor::TechnoCursors,
    ) {
        self.ares_cursors = cursors;
    }

    pub(crate) fn ares_production_mut(
        &mut self,
    ) -> &mut crate::rules::ares_production::ProductionTypeRules {
        &mut self.production
    }

    /// Whether `[General] WallBuildSpeedCoefficient=` scales this type's
    /// build time: a `Wall=` BuildingType (Time_To_Build6F4917,
    /// Select6AB8BB), with Ares only while `BuildTime.Speed` is unset
    /// (10041C6C..10041C87, 10041950).
    pub fn wall_build_speed_applies(&self) -> bool {
        self.category == ObjectCategory::Building
            && self.wall
            && self.production.build_time_speed().is_none()
    }

    pub(crate) fn bind_academy(
        &mut self,
        academy: &crate::rules::ares_veterancy::AcademyTypeRules,
    ) {
        self.academy = academy.clone();
    }

    /// The optional Ares manual-action restriction, shared by cursor and
    /// click through the existing action owner. Only a Rules read the Ares
    /// way sets it.
    pub const fn no_manual_fire(&self) -> bool {
        self.no_manual_fire
    }

    /// Ares TechnoTypeExt+4C5 `NoManualUnload` (`rules::ares_techno_type`);
    /// false unless the Rules are read the Ares way.
    pub fn no_manual_unload(&self) -> bool {
        self.techno_combat.movement().no_manual_unload()
    }

    /// Optional Ares tracking override (reader1003F47C, constructor1003CF8E).
    pub const fn keep_alive(&self) -> Option<bool> {
        self.keep_alive
    }

    /// BuiltAt BuildingTypes ordinals, bound at each reached TechnoType read.
    /// An empty list leaves the source factory unspecified; duplicates are kept.
    pub fn built_at(&self) -> &[i32] {
        self.factory_owners.built_at()
    }

    /// BuildingExt+126: source factory only admits an explicit BuiltAt match.
    pub fn factory_explicit_only(&self) -> bool {
        self.explicit_factory.explicit_only()
    }

    /// Allowed original factory Countries in native list order, duplicates kept.
    pub fn factory_owners(&self) -> &[crate::rules::ruleset::CountryIdx] {
        self.factory_owners.allowed()
    }

    /// Countries excluded for the same candidate that satisfies FactoryOwners.
    pub fn forbidden_factory_owners(&self) -> &[crate::rules::ruleset::CountryIdx] {
        self.factory_owners.forbidden()
    }

    /// TypeExt+320: HaveAllPlans followed by Permanent writes the same field.
    pub fn factory_owners_permanent(&self) -> bool {
        self.factory_owners.permanent()
    }

    /// Independent TypeExt+321: a live building supplies all factory categories.
    pub fn factory_has_all_plans(&self) -> bool {
        self.factory_owners.has_all_plans()
    }

    /// Retained custom palette request; the asset loader remains responsible
    /// for resolving it. Empty ART Palette reads retain this name (717820).
    pub fn custom_palette_name(&self) -> Option<&str> {
        self.custom_palette.as_deref()
    }

    pub(crate) fn project_custom_palette(&mut self, name: Option<&str>) {
        self.custom_palette = name.map(str::to_owned);
    }

    pub(crate) fn project_factory_owners(
        &mut self,
        state: &crate::rules::native_processing::AresFactoryOwners,
    ) {
        self.factory_owners.clone_from(state);
    }

    pub(crate) fn factory_owners_hash_state(&self) -> impl std::hash::Hash + '_ {
        (&self.factory_owners, &self.explicit_factory)
    }

    pub(crate) fn project_explicit_factory(
        &mut self,
        state: &crate::rules::native_processing::AresExplicitFactory,
    ) {
        self.explicit_factory.clone_from(state);
    }

    pub fn armor_id(&self) -> crate::rules::armor::ArmorId {
        self.armor_id
    }

    pub(crate) fn bind_armor(&mut self, armor: crate::rules::armor::ArmorId) {
        self.armor_id = armor;
    }

    /// Phobos `ShieldType=`: the bound `[ShieldTypes]` entry name.
    #[cfg(feature = "phobos")]
    pub(crate) fn shield_type(&self) -> Option<&str> {
        self.shield_type.as_deref()
    }

    /// Phobos `TechnoTypeExt::ImmuneToCrit`.
    #[cfg(feature = "phobos")]
    pub fn immune_to_crit(&self) -> bool {
        self.immune_to_crit
    }

    /// Phobos `TechnoTypeExt::VeteranCritChance`; `None` inherits
    /// `[General] VeteranCritChance`.
    #[cfg(feature = "phobos")]
    pub fn veteran_crit_chance(&self) -> Option<f64> {
        self.veteran_crit_chance
    }

    /// Phobos extension rank abilities (`ReadAdditionalAbilities`).
    #[cfg(feature = "phobos")]
    pub fn phobos_abilities(&self) -> &crate::rules::phobos_crit::PhobosAbilityFlags {
        &self.phobos_abilities
    }

    /// Phobos `DigitalDisplayTypes=`, in authored list order.
    #[cfg(feature = "phobos")]
    pub fn digital_display_types(&self) -> &[String] {
        &self.digital_display_types
    }

    /// Phobos `AttachEffect.AttachTypes`, in authored list order.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_attach_types(&self) -> &[String] {
        &self.attach_effect_attach_types
    }

    /// Phobos `AttachEffect.DurationOverrides`, paired by index with
    /// [`Self::attach_effect_attach_types`]; the last value covers the rest.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_duration_overrides(&self) -> &[i32] {
        &self.attach_effect_duration_overrides
    }

    /// Phobos `AttachEffect.Delays`, paired by index with
    /// [`Self::attach_effect_attach_types`].
    #[cfg(feature = "phobos")]
    pub fn attach_effect_delays(&self) -> &[i32] {
        &self.attach_effect_delays
    }

    /// Phobos `AttachEffect.InitialDelays`, paired by index with
    /// [`Self::attach_effect_attach_types`].
    #[cfg(feature = "phobos")]
    pub fn attach_effect_initial_delays(&self) -> &[i32] {
        &self.attach_effect_initial_delays
    }

    /// Phobos `AttachEffect.RecreationDelays`, paired by index with
    /// [`Self::attach_effect_attach_types`].
    #[cfg(feature = "phobos")]
    pub fn attach_effect_recreation_delays(&self) -> &[i32] {
        &self.attach_effect_recreation_delays
    }

    /// Phobos `DigitalDisplay.Disable=`.
    #[cfg(feature = "phobos")]
    pub fn digital_display_disable(&self) -> bool {
        self.digital_display_disable
    }

    /// Phobos `TechnoTypeExt::OpenTopped_UseTransportRangeModifiers`;
    /// `None` inherits `[General] OpenTopped.UseTransportRangeModifiers`.
    #[cfg(feature = "phobos")]
    pub fn open_topped_use_transport_range_modifiers(&self) -> Option<bool> {
        self.open_topped_use_transport_range_modifiers
    }

    /// Phobos `DigitalDisplay.Health.FakeAtDisguise=`, `None` when inheriting.
    #[cfg(feature = "phobos")]
    pub fn digital_display_health_fake_at_disguise(&self) -> Option<bool> {
        self.digital_display_health_fake_at_disguise
    }

    /// Phobos `PlacementPreview=` (per building type).
    #[cfg(feature = "phobos")]
    pub fn placement_preview(&self) -> bool {
        self.placement_preview
    }

    /// Phobos `PlacementPreview.Shape=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_shape(&self) -> Option<&str> {
        self.placement_preview_shape.as_deref()
    }

    /// Phobos `PlacementPreview.ShapeFrame=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_shape_frame(&self) -> Option<i32> {
        self.placement_preview_shape_frame
    }

    /// Phobos `PlacementPreview.Offset=` in leptons.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_offset(&self) -> [i32; 3] {
        self.placement_preview_offset
    }

    /// Phobos `PlacementPreview.Remap=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_remap(&self) -> bool {
        self.placement_preview_remap
    }

    /// Phobos `PlacementPreview.Palette=`.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_palette(&self) -> Option<&str> {
        self.placement_preview_palette.as_deref()
    }

    /// Phobos `PlacementPreview.Translucency=`; `None` inherits the global.
    #[cfg(feature = "phobos")]
    pub fn placement_preview_translucency(
        &self,
    ) -> Option<crate::rules::phobos_placement_preview::Translucency> {
        self.placement_preview_translucency
    }

    /// Native fixed-ART ShadowIndex, retained by the ordered Type reader.
    pub const fn voxel_shadow_index(&self) -> i32 {
        self.voxel_shadow_index
    }

    pub(crate) fn set_voxel_shadow_index(&mut self, index: i32) {
        self.voxel_shadow_index = index;
    }

    /// Type+C95 also selects Fly's flat shadow-matrix arm (4CFB4B).
    pub const fn is_dropship(&self) -> bool {
        self.is_dropship
    }

    /// Phobos `InfantryTypeExt::InfantryAutoDeploy`, unset when the key is
    /// absent.
    #[cfg(feature = "phobos")]
    pub(crate) const fn infantry_auto_deploy(&self) -> Option<bool> {
        self.infantry_auto_deploy
    }

    /// Phobos `InfantryTypeExt::ProneSpeed`, unset when the key is absent.
    #[cfg(feature = "phobos")]
    pub(crate) const fn prone_speed(&self) -> Option<f64> {
        self.prone_speed
    }

    /// Phobos `AircraftTypeExt::FlyNoWobbles`, unset when the key is absent.
    #[cfg(feature = "phobos")]
    pub(crate) const fn fly_no_wobbles(&self) -> Option<bool> {
        self.fly_no_wobbles
    }

    /// Phobos `TechnoTypeExt::LaserTargetColor`, unset when the key is absent.
    #[cfg(feature = "phobos")]
    pub(crate) const fn laser_target_color(&self) -> Option<i32> {
        self.laser_target_color
    }

    /// Phobos `TechnoTypeExt::AirstrikeLineColor`, unset when the key is absent.
    #[cfg(feature = "phobos")]
    pub(crate) const fn airstrike_line_color(&self) -> Option<[u8; 3]> {
        self.airstrike_line_color
    }

    pub const fn building_light(&self) -> crate::rules::building_light::BuildingLightRules {
        self.building_light
    }

    pub(crate) fn project_building_light(
        &mut self,
        light: crate::rules::building_light::BuildingLightRules,
    ) {
        self.building_light = light;
    }

    pub fn building_upgrade(&self) -> &crate::rules::building_upgrade::BuildingUpgradeRules {
        &self.building_upgrade
    }

    pub(crate) fn project_building_upgrade(
        &mut self,
        upgrade: crate::rules::building_upgrade::BuildingUpgradeRules,
    ) {
        self.building_upgrade = upgrade;
    }

    pub(crate) const fn carries_crate(&self) -> bool {
        self.carries_crate
    }

    pub const fn has_radial_indicator(&self) -> bool {
        self.has_radial_indicator
    }

    pub const fn concentric_radial_indicator(&self) -> bool {
        self.concentric_radial_indicator
    }

    /// Phobos registers a building as a power plant enhancer only when its
    /// `PowerPlantEnhancer.PowerPlants` list is non-empty and its `Amount` or
    /// `Factor` actually changes the output (`src/Ext/Building/Hooks.cpp`:
    /// Unlimbo `0x441553`, SetOwningHouse `0x449197`).
    #[cfg(feature = "phobos")]
    pub fn is_power_plant_enhancer(&self) -> bool {
        !self.power_plant_enhancer_power_plants.is_empty()
            && (self.power_plant_enhancer_amount != 0 || self.power_plant_enhancer_factor != 1.0)
    }

    /// Whether this enhancer's `PowerPlantEnhancer.PowerPlants` list names
    /// `building_type` (case-insensitive, like `BuildingTypeClass::Find`).
    #[cfg(feature = "phobos")]
    pub fn power_plant_enhancer_enhances(&self, building_type: &str) -> bool {
        self.power_plant_enhancer_power_plants
            .iter()
            .any(|name| name.eq_ignore_ascii_case(building_type))
    }

    /// `PowerPlantEnhancer.Range` in leptons (`Valueable<Leptons>`); 0
    /// disables the ring.
    #[cfg(feature = "phobos")]
    pub const fn power_plant_enhancer_range(&self) -> i32 {
        self.power_plant_enhancer_range
    }

    #[cfg(feature = "phobos")]
    pub(crate) const fn power_plant_enhancer_amount(&self) -> i32 {
        self.power_plant_enhancer_amount
    }

    #[cfg(feature = "phobos")]
    pub(crate) const fn power_plant_enhancer_factor(&self) -> f32 {
        self.power_plant_enhancer_factor
    }

    /// Signed `PowerPlantEnhancer.MaxCount`; a non-positive value means no
    /// per-type limit.
    #[cfg(feature = "phobos")]
    pub(crate) const fn power_plant_enhancer_max_count(&self) -> i32 {
        self.power_plant_enhancer_max_count
    }

    /// `PowerPlant.DamageFactor`, the multiplier on the health-driven output
    /// loss; 1.0 is the stock curve.
    #[cfg(feature = "phobos")]
    pub(crate) const fn power_plant_damage_factor(&self) -> f64 {
        self.power_plant_damage_factor
    }

    /// Building GetSensorRange4566B0. The caller supplies retained+268 and
    /// GetCurrentWeapon70E1A0's resolved range; Option already represents the
    /// null WeaponStruct/WeaponType test70E240. No General.GapRadius fallback.
    /// `ares` is [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
    pub fn building_sensor_range(
        &self,
        gap_supercharged: bool,
        weapon_range: Option<i32>,
        ares: bool,
    ) -> i32 {
        // Ares BuildingClass_GetRangeOfRadial_Radius (10048E60, hook
        // 0x4566B0) answers a set RadialIndicatorRadius at the entry.
        if ares && let Some(radius) = self.techno_combat.presentation().radial_indicator_radius() {
            return radius;
        }
        if self.psychic_detection_radius > 0 {
            self.psychic_detection_radius
        } else if self.gap_generator {
            if gap_supercharged {
                self.super_gap_radius_in_cells
            } else {
                self.gap_radius_in_cells
            }
        } else if ares && self.sensor_array {
            // Ares GetRangeOfRadial_SensorArray10016D30 (hook4566F9) shows
            // the SensorsSight the array deposits.
            i32::from(self.sensors_sight)
        } else if self.sensor_array || self.cloak_generator {
            i32::from(self.cloak_radius_in_cells)
        } else {
            weapon_range
                .filter(|range| *range > 0)
                .map_or(0, |range| range / 256)
        }
    }

    /// Ares ObjectClass_DrawRadialIndicator (10049A70, hook 0x0041BE80): the
    /// ring radius in cells of a Techno that is no Building. A set
    /// RadialIndicatorRadius wins, a GapGenerator shows GapRadiusInCells, and
    /// any other type shows its current weapon's range (vt+0x3F4) in whole
    /// cells (`Range >> 8`). Zero or less draws nothing.
    pub fn ares_techno_radial_radius(&self, weapon_range: Option<i32>) -> i32 {
        if let Some(radius) = self.techno_combat.presentation().radial_indicator_radius() {
            radius
        } else if self.gap_generator {
            self.gap_radius_in_cells
        } else {
            weapon_range
                .filter(|range| *range > 0)
                .map_or(0, |range| range >> 8)
        }
    }

    pub const fn attacks_friendlies(&self) -> bool {
        self.attack_friendlies
    }

    /// Signed PipWrap configuration, before the draw caller's division.
    pub const fn pip_wrap(&self) -> i32 {
        self.pip_wrap
    }

    /// Building4598A0's current-type query: UnitAbsorb OR InfantryAbsorb.
    /// Class-specific entry and lifecycle admission remain with each caller.
    /// Original instructions/readers: procedural_drawing_oracle/absorber_pips.
    pub const fn absorbs_passengers(&self) -> bool {
        self.unit_absorb || self.infantry_absorb
    }

    /// Ares `Passengers.BySize=`, which the passenger pips also read
    /// (TechnoExt::GetPassengerPips 10044C90).
    pub const fn ares_passengers_by_size(&self) -> bool {
        self.ares_passengers_by_size
    }

    pub const fn pips_draw_for_all(&self) -> bool {
        self.pips_draw_for_all
    }

    /// InfantryType+DFC: passenger frame in Building PIPS or mobile PIPS2.
    pub const fn pip(&self) -> u32 {
        self.infantry_pips.pip
    }

    /// InfantryType+E00: raw PIPS frame for this infantry in a garrison.
    /// Admitted missing-key passes can produce1..5 as well as person frames.
    pub const fn occupy_pip(&self) -> u32 {
        self.infantry_pips.occupy_pip
    }

    pub(crate) fn project_infantry_pips(&mut self, pips: InfantryPips) {
        self.infantry_pips = pips;
    }

    pub(crate) fn project_infantry_art(&mut self, art: InfantryArt) {
        self.crawls = art.crawls;
        [
            self.fire_up_frame,
            self.fire_prone_frame,
            self.secondary_fire_frame,
            self.secondary_prone_frame,
        ] = art.discharge_frames;
    }

    pub(crate) fn project_infantry_sequence_bank(
        &mut self,
        bank: &crate::rules::infantry_sequence::InfantrySequenceBank,
    ) {
        self.infantry_sequence_bank = Some(bank.clone());
    }

    pub(crate) fn infantry_sequence_bank(
        &self,
    ) -> Option<&crate::rules::infantry_sequence::InfantrySequenceBank> {
        self.infantry_sequence_bank.as_ref()
    }

    /// Derive presentation names from saved Voc ordinals, without rereading ART.
    pub(crate) fn bind_sequence_sounds(&mut self, sounds: &crate::rules::sound_ini::SoundRegistry) {
        self.sequence_sounds = self
            .infantry_sequence_bank
            .as_ref()
            .map(|bank| bank.bound_sounds(sounds))
            .unwrap_or_default();
    }

    /// The sounds of DoType `action`'s record, in slot order.
    pub fn sequence_sounds(
        &self,
        action: i32,
    ) -> impl Iterator<Item = &crate::rules::infantry_sequence::SequenceSound> {
        self.sequence_sounds
            .iter()
            .filter(move |sound| i32::from(sound.action) == action)
    }

    pub(crate) fn project_infantry_infiltration(&mut self, flags: InfantryInfiltration) {
        self.infiltrate = flags.infiltrate;
        self.c4 = flags.c4;
        self.engineer = flags.engineer;
        self.agent = flags.agent;
        // Older serialized processing states lack these two members. Keep
        // their merged-INI projection until a reached read supplies a byte.
        self.thief = flags.thief.unwrap_or(self.thief);
        self.vehicle_thief = flags.vehicle_thief.unwrap_or(self.vehicle_thief);
    }

    /// GetPipMax, virtual+74: BuildingType's override (0x0045ECE0) for a
    /// Building, else TechnoType's (0x00716290). Signed configuration
    /// capacity; callers own allocation/fill bounds. Uses the live bound base
    /// weapon0, not a capture manager's retained admission capacity or elite
    /// weapon.
    pub fn pip_max(&self, rules: &crate::rules::ruleset::RuleSet) -> i32 {
        // BuildingTypeClass::GetPipMax 0x0045ECE0: the foundation width
        // (table 0x008192B8) times 60, divided by 8 toward zero
        // (0x0045ECED..0x0045ED02), caps Tiberium pips at `Storage=`, or a
        // Weeder's at `[General] WeedCapacity=`, a negative one answering 0;
        // Power answers the cap, every other scale TechnoType's. Ares
        // BuildingTypeClass_GetMaxPips (Ares.dll 10010150, hook 0x45ECE0)
        // only loads a custom foundation's `Foundation.X` as that width and
        // resumes at 0x0045ECED. Stock buildings draw no Tiberium pips
        // (DrawPipScalePips 0x00709B61), so the Tiberium arm shows only with
        // Ares' storage pips (docs/ares-tiberium.md).
        if self.category == ObjectCategory::Building {
            let cap = i32::from(self.foundation.dimensions().0) * 60 / 8;
            match self.pip_scale {
                PipScale::Tiberium => {
                    let held = if self.weeder {
                        rules.general.weed_capacity
                    } else {
                        self.storage
                    };
                    return if held < 0 { 0 } else { held.min(cap) };
                }
                PipScale::Power => return cap,
                _ => {}
            }
        }
        // TechnoTypeClass::GetPipMax 0x00716290.
        match self.pip_scale {
            PipScale::None => 0,
            PipScale::Ammo => self.ammo,
            PipScale::Tiberium => {
                if self.category == ObjectCategory::Infantry {
                    3
                } else {
                    5
                }
            }
            PipScale::Passengers => self.passengers,
            PipScale::Power => 10,
            // Ares TechnoTypeClass_GetPipMax_MindControl (1004B910, hook
            // 0x7162B0): the first of the two base weapons whose warhead is
            // `MindControl=` with a positive Damage, else the second's
            // MindControl Damage, else 0.
            PipScale::MindControl if rules.ares() => {
                let mind_damage = |slot| {
                    self.weapon_at(slot)
                        .and_then(|name| rules.weapon(name))
                        .filter(|weapon| {
                            weapon
                                .warhead
                                .as_deref()
                                .and_then(|warhead| rules.warhead(warhead))
                                .is_some_and(|warhead| warhead.mind_control)
                        })
                        .map_or(0, |weapon| weapon.damage)
                };
                let primary = mind_damage(0);
                if primary > 0 { primary } else { mind_damage(1) }
            }
            // Native7162B0 dereferences an admitted primary. Preserve the
            // existing host's safe0 for an invalid missing binding; stock
            // MindControl has a bound weapon and follows the original read.
            PipScale::MindControl => self
                .weapon_at(0)
                .and_then(|name| rules.weapon(name))
                .map_or(0, |weapon| weapon.damage),
        }
    }

    /// The authoritative BuildingType+EB8 class, including non-production
    /// RTTI names. Consumers must keep their own native comparisons.
    pub const fn factory_rtti(&self) -> i32 {
        self.factory_rtti
    }

    /// Native nonzero-Factory gates (HasOccurred17, AI, quarry and exit).
    pub const fn has_factory(&self) -> bool {
        self.factory_rtti != 0
    }

    /// Exact Type comparisons used by FindFactory, exit arms and rally.
    /// Object RTTIs (Unit1/Infantry15/...) are deliberately not type RTTIs.
    pub const fn factory_type(&self) -> Option<FactoryType> {
        FactoryType::from_rtti(self.factory_rtti)
    }

    pub(crate) fn target_coord_offset(&self) -> [i32; 3] {
        self.target_coord_offset
    }

    /// Publish the value retained by the native type-processing owner.
    pub(crate) fn set_target_coord_offset(&mut self, offset: [i32; 3]) {
        debug_assert_eq!(self.category, ObjectCategory::Building);
        self.target_coord_offset = offset;
    }

    /// Publish the Building-family ReadINI owner's retained smoke locations.
    pub(crate) fn set_half_damage_smoke_locations(
        &mut self,
        location1: [i32; 3],
        location2: [i32; 3],
    ) {
        debug_assert_eq!(self.category, ObjectCategory::Building);
        self.half_damage_smoke_location1 = location1;
        self.half_damage_smoke_location2 = location2;
    }

    pub fn voice_harvest(&self) -> Option<&str> {
        self.voice_harvest.as_deref()
    }

    pub fn voice_move(&self) -> &[String] {
        &self.voice_move
    }

    pub fn voice_enter(&self) -> Option<&str> {
        self.voice_enter.as_deref()
    }

    pub fn voice_capture(&self) -> Option<&str> {
        self.voice_capture.as_deref()
    }

    pub fn voice_attack(&self) -> &[String] {
        &self.voice_attack
    }

    /// VoiceAttack7090F8..70914A reads primary for index0, otherwise
    /// secondary; its caller supplies the raw750010 elite predicate.
    pub fn voice_attack_override(&self, weapon_index: i32, elite: bool) -> Option<&str> {
        self.voice_attack_overrides[usize::from(weapon_index != 0) * 2 + usize::from(elite)]
            .as_deref()
    }

    /// Bind the existing native type reader's list and scalar slots against
    /// the one fixed catalog and its reached processed-section history.
    pub(crate) fn bind_order_voice_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.voice_move = section.map_or_else(Vec::new, |section| {
            sounds.read_rules_sound_list(section, "VoiceMove")
        });
        self.voice_attack = section.map_or_else(Vec::new, |section| {
            sounds.read_rules_sound_list(section, "VoiceAttack")
        });
        self.voice_enter =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceEnter", None));
        self.voice_capture =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceCapture", None));
        self.voice_attack_overrides = [
            "VoicePrimaryWeaponAttack",
            "VoicePrimaryEliteWeaponAttack",
            "VoiceSecondaryWeaponAttack",
            "VoiceSecondaryEliteWeaponAttack",
        ]
        .map(|key| section.and_then(|section| sounds.read_rules_reference(section, key, None)));
        if crate::rules::ares_switch::ares_way() {
            self.ares_voice_repair = section
                .and_then(|section| sounds.read_rules_reference(section, "VoiceRepair", None));
        }
    }

    /// Ares `VoiceRepair`, see [`Self::ares_voice_repair`]'s field.
    pub(crate) fn ares_voice_repair(&self) -> Option<&str> {
        self.ares_voice_repair.as_deref()
    }

    pub fn voice_deploy(&self) -> Option<&str> {
        self.voice_deploy.as_deref()
    }

    pub fn voice_undeploy(&self) -> Option<&str> {
        self.voice_undeploy.as_deref()
    }

    /// Original scalar readers71363D/713769/7137A5; constructor -1 is
    /// represented by None. Reuse the processed pass/current-ID owner.
    pub(crate) fn bind_deploy_voice_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.voice_harvest =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceHarvest", None));
        self.voice_deploy =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceDeploy", None));
        self.voice_undeploy =
            section.and_then(|section| sounds.read_rules_reference(section, "VoiceUndeploy", None));
    }

    pub fn damage_sound(&self) -> Option<&str> {
        self.damage_sound.as_deref()
    }

    pub fn secret_lab(&self) -> &crate::rules::secret_lab::SecretLabRules {
        &self.secret_lab
    }

    pub(crate) fn set_secret_lab(&mut self, rules: crate::rules::secret_lab::SecretLabRules) {
        self.secret_lab = rules;
    }

    pub fn create_sound(&self) -> Option<&str> {
        self.create_sound.as_deref()
    }

    /// Bind TechnoType+534/+538 in original712E75..712F07 read order,
    /// using only passes where this type existed in the native processing order.
    pub(crate) fn bind_scalar_sound_references(
        &mut self,
        section: Option<&IniSection>,
        sounds: &SoundRegistry,
    ) {
        self.create_sound =
            section.and_then(|section| sounds.read_rules_reference(section, "CreateSound", None));
        self.damage_sound =
            section.and_then(|section| sounds.read_rules_reference(section, "DamageSound", None));
        // TechnoType ReadINI713949..713985: current fixed Voc ID, with
        // ctor710FA5=-1. Unknown/empty later reads retain a valid override.
        self.mind_cleared_sound = section
            .and_then(|section| sounds.read_rules_reference(section, "MindClearedSound", None));
        // The Ares TypeExt sound keys bind only for Rules read the Ares way.
        if !crate::rules::ares_switch::ares_way() {
            return;
        }
        for (slot, key) in ["Promote.VeteranSound", "Promote.EliteSound"]
            .into_iter()
            .enumerate()
        {
            // TypeExt LoadFromINI1003F0C5/1003F0DB reuses NullableIdx<Voc>
            // 1000E9D0:2048-byte read, fixed-catalog lookup, unknown retains.
            self.promotion_sounds[slot] =
                section.and_then(|section| sounds.read_ares_reference(section, key));
        }
        for (slot, key) in ["CloakSound", "DecloakSound"].into_iter().enumerate() {
            // TypeExt LoadFromINI1003DFB0: NullableIdx<Voc>1000E9D0.
            self.cloak_sounds[slot] =
                section.and_then(|section| sounds.read_ares_reference(section, key));
        }
        for (slot, key) in ["VehicleThief.EnterSound", "VehicleThief.LeaveSound"]
            .into_iter()
            .enumerate()
        {
            // Nonnullable Voc reader1003FA80 at1003E72D/1003E743 uses the
            // same 2048-byte lookup/none rules; unset and -1 both mean silence.
            self.vehicle_thief_sounds[slot] = section
                .and_then(|section| sounds.read_ares_reference(section, key))
                .flatten();
        }
        self.techno_combat.bind_sound_references(section, sounds);
        if self.category == ObjectCategory::Building {
            self.building_type_ext
                .bind_sound_references(section, sounds);
        }
    }

    pub(crate) fn promotion_sound_override(&self, elite: bool) -> Option<Option<&str>> {
        self.promotion_sounds[usize::from(elite)]
            .as_ref()
            .map(|cue| cue.as_deref())
    }

    pub(crate) fn vehicle_thief_sound(&self, leaving: bool) -> Option<&str> {
        self.vehicle_thief_sounds[usize::from(leaving)].as_deref()
    }

    /// Every sound name this type's own fields hold, for the set's
    /// sound-name table (`RuleSet::rebuild_sound_names`).
    pub(crate) fn sound_names(&self) -> impl Iterator<Item = &str> {
        let lists = [
            &self.voice_select,
            &self.voice_select_enslaved,
            &self.voice_select_deactivated,
            &self.voice_move,
            &self.voice_attack,
            &self.voice_die,
            &self.die_sounds,
            &self.move_sound,
            &self.voice_comment,
            &self.voice_feedback,
            &self.voice_special_attack,
        ];
        let scalars = [
            &self.voice_harvest,
            &self.voice_deploy,
            &self.voice_undeploy,
            &self.voice_enter,
            &self.voice_capture,
            &self.ares_voice_repair,
            &self.create_sound,
            &self.damage_sound,
            &self.crashing_sound,
            &self.voice_crashing,
            &self.sinking_sound,
            &self.voice_sinking,
            &self.voice_falling,
            &self.turret_rotate_sound,
            &self.activate_sound,
            &self.deactivate_sound,
            &self.ambient_sound,
            &self.impact_water_sound,
            &self.impact_land_sound,
            &self.crush_sound,
            &self.deploy_sound,
            &self.buildup_sound,
            &self.working_sound,
            &self.not_working_sound,
            &self.undeploy_sound,
            &self.enter_water_sound,
            &self.leave_water_sound,
            &self.packup_sound,
            &self.enter_transport_sound,
            &self.leave_transport_sound,
            &self.chrono_in_sound,
            &self.chrono_out_sound,
            &self.mind_cleared_sound,
            &self.aux_sound1,
            &self.aux_sound2,
        ];
        let overrides = self
            .promotion_sounds
            .iter()
            .chain(&self.cloak_sounds)
            .flatten();
        let airstrike_voices = [false, true]
            .into_iter()
            .filter_map(|abort| self.techno_combat().presentation().airstrike_voice(abort))
            .flatten();
        lists
            .into_iter()
            .flatten()
            .map(String::as_str)
            .chain(
                scalars
                    .into_iter()
                    .chain(&self.voice_attack_overrides)
                    .chain(&self.vehicle_thief_sounds)
                    .chain(overrides)
                    .filter_map(|name| name.as_deref()),
            )
            .chain(airstrike_voices)
            .chain(
                self.sequence_sounds
                    .iter()
                    .map(|sound| sound.sound_id.as_str()),
            )
    }

    /// Storage reached by GetWeapon7177C0/7177E0; Ares1003DBC0 adds
    /// extension records, while a stock-way read keeps the fixed bound of
    /// `WEAPON_SLOT_COUNT`. Ares TechnoTypeClass_LoadFromINI_Weapons1
    /// (10042570, hook 0x7128C0) replaces the stock WeaponN block with
    /// LoadWeapons 10040020, which `native_processing::WeaponSlots` ports.
    pub fn weapon_record_count(&self) -> usize {
        self.weapon_list.len()
    }

    /// `Weapon[index]` (`TechnoTypeClass+0x898 + index*0x1C`): the weapon in
    /// base slot `index`, or `None` for an empty slot or an index past the
    /// array. `TechnoTypeClass::ReadINI @ 0x007128B2` writes `Weapon1=` and
    /// `Primary=` to the same `+0x898`, so the `TurretCount` branch is decided
    /// while Rules are processed (`native_processing::WeaponSlots`); no reader
    /// has to know which INI keys a type authored.
    pub fn weapon_at(&self, index: usize) -> Option<&str> {
        self.weapon_list.get(index).and_then(|slot| slot.as_deref())
    }

    /// `EliteWeapon[index]` (`TechnoTypeClass+0xA94 + index*0x1C`).
    pub fn elite_weapon_at(&self, index: usize) -> Option<&str> {
        self.elite_weapon_list
            .get(index)
            .and_then(|slot| slot.as_deref())
    }

    /// Base weapon-array slot 0 (`TechnoTypeClass+0x898`), the field gamemd
    /// calls `Primary`. Filled from `Primary=` for an ordinary type and from
    /// `Weapon1=` for a `TurretCount>0` type — the same storage either way, see
    /// `native_processing::WeaponSlots`. Read it as the native field, not as
    /// "the `Primary=` key": for `[SREF]` and `[YAGGUN]` it holds the
    /// `Weapon1=` weapon even though neither section authors a live `Primary=`.
    pub fn primary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Base weapon-array slot 1 (`TechnoTypeClass+0x8B4`), gamemd's `Secondary`
    /// field. `Secondary=` or `Weapon2=`, same storage.
    #[cfg(any(test, feature = "test-support"))]
    pub fn secondary(&self) -> Option<&str> {
        self.weapon_at(WEAPON_SLOT_SECONDARY)
    }

    /// Whether the type accepts an attack-move order (type vtable `+0xA4`).
    /// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90`, which Infantry
    /// (`0x005247C0`) and Unit (`0x007473D0`) types jump to, answers
    /// `Primary (+0x898) != null && !PreventAttackMove (+0x6C8)`; Aircraft
    /// (`0x0041CB60`) and Building (`0x00465920`) types answer false.
    ///
    /// The raw slot-0 field, not `GetWeapon`: the elite tier does not apply,
    /// and a `TurretCount>0` type passes on its `Weapon1=` ([`Self::primary`]),
    /// so a Prism Tank attack-moves although its `Primary=` is commented out.
    /// The weapon factory answers null for `none` and `<none>`. `Secondary` is
    /// never read, so a secondary-only type is refused. The class follows list
    /// membership: the `[VehicleTypes]` helicopters (`SHAD`, `HIND`, `SCHP`,
    /// `SCHD`) are units here, refused by their `PreventAttackMove=yes`.
    pub fn can_attack_move(&self) -> bool {
        matches!(
            self.category,
            ObjectCategory::Infantry | ObjectCategory::Vehicle
        ) && !self.prevent_attack_move
            && self
                .primary()
                .is_some_and(|primary| !is_native_none_type_name(primary))
    }

    /// Elite weapon-array slot 0 (`TechnoTypeClass+0xA94`), gamemd's
    /// `ElitePrimary` field: `ElitePrimary=` or `EliteWeapon1=`. Replaces
    /// [`Self::primary`] when the unit is at Elite tier (veterancy >= 200) and
    /// this slot names a weapon; Veteran tier (100..199) does NOT swap.
    #[cfg(any(test, feature = "test-support"))]
    pub fn elite_primary(&self) -> Option<&str> {
        self.elite_weapon_at(WEAPON_SLOT_PRIMARY)
    }

    /// Elite weapon-array slot 1 (`TechnoTypeClass+0xAB0`), gamemd's
    /// `EliteSecondary` field: `EliteSecondary=` or `EliteWeapon2=`. Replaces
    /// [`Self::secondary`] at Elite tier under the same rule.
    #[cfg(test)]
    pub fn elite_secondary(&self) -> Option<&str> {
        self.elite_weapon_at(WEAPON_SLOT_SECONDARY)
    }

    /// BuildingType+ED8 is initialized to zero at45DEE6 and has no active-YR
    /// reader/write override. Construction449AFE and Selling449DAA/449DE9
    /// use this raw byte, shifted8, to reset FacingClass's two direction words.
    /// DeployFacing lives separately at+EDC. Keep this invariant with the type
    /// metadata instead of adding a configurable or per-instance copy.
    pub const BUILDING_FACING: u8 = 0;

    /// Native barracks classification read by GetDockCell44EFB0 and
    /// ExitObject443C60; independent from Factory=InfantryType.
    pub fn gdi_barracks(&self) -> bool {
        self.gdi_barracks
    }

    pub fn armory(&self) -> bool {
        self.armory
    }

    /// Native BuildingType+16E5 classification, independent from Factory=.
    pub fn nod_barracks(&self) -> bool {
        self.nod_barracks
    }

    /// Native BuildingType+16E6 classification, independent from Factory=.
    pub fn yuri_barracks(&self) -> bool {
        self.yuri_barracks
    }

    /// Native TechnoType virtual+BC (717800), used by Fly takeoff4CF9F2
    /// and Aircraft Unlimbo414383. Zero and other negatives are literal.
    pub fn flight_level(&self, general_flight_level: i32) -> i32 {
        if self.flight_level == -1 {
            general_flight_level
        } else {
            self.flight_level
        }
    }

    /// `Owner=` (`+0x6CC`) as a House-type mask: `Get_Ownable @ 0x00711EC0`
    /// answers it outside `DoubleOwned=`.
    pub(crate) fn owner_mask(&self, rules: &crate::rules::ruleset::RuleSet) -> u32 {
        self.house_masks(rules).owner
    }

    /// ReadOwners4750D0 / Type reader714531: absent/empty ReadString keeps
    /// the constructor's -1 (711633), but nonempty comma-only input writes0.
    /// CanBuild4F79E4 and FirstBuildable505230 both test this raw mask.
    pub(crate) fn required_houses_mask(&self, rules: &crate::rules::ruleset::RuleSet) -> u32 {
        self.house_masks(rules).required
    }

    /// `ForbiddenHouses=` as a House-type mask (CanBuild4F7A7D).
    pub(crate) fn forbidden_houses_mask(&self, rules: &crate::rules::ruleset::RuleSet) -> u32 {
        self.house_masks(rules).forbidden
    }

    /// The masks [`crate::rules::ruleset::RuleSet`] projected; debug builds
    /// compare them with a fresh read of the lists against `rules`.
    fn house_masks(&self, rules: &crate::rules::ruleset::RuleSet) -> HouseMasks {
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || self.house_masks == HouseMasks::derive(self, rules),
            "{}: house masks projected against another Country registry",
            self.id
        );
        self.house_masks
    }

    /// Store masks derived against the current Country registry.
    pub(crate) fn project_house_masks(&mut self, masks: HouseMasks) {
        self.house_masks = masks;
    }

    /// The type's weapon records as its RuleSet resolved them; reads go
    /// through `RuleSet::resolved_weapon`, which checks the resolving set.
    #[inline]
    pub(crate) fn resolved_weapons(&self) -> &crate::rules::ruleset::TypeWeapons {
        &self.resolved_weapons
    }

    /// Store the weapon records resolved against the current weapons.
    pub(crate) fn project_resolved_weapons(&mut self, weapons: crate::rules::ruleset::TypeWeapons) {
        self.resolved_weapons = weapons;
    }

    /// `debris_types` as its RuleSet resolved it; reads go through
    /// `RuleSet::object_debris_types`, which checks the resolving set.
    pub(crate) fn resolved_debris(&self) -> &crate::rules::ruleset::ResolvedDebris {
        &self.resolved_debris
    }

    /// Store `debris_types` resolved against the current `[VoxelAnims]`.
    pub(crate) fn project_resolved_debris(
        &mut self,
        debris: crate::rules::ruleset::ResolvedDebris,
    ) {
        self.resolved_debris = debris;
    }

    /// The ART `SiloDamage=` [`crate::rules::ruleset::RuleSet`] projected;
    /// debug builds compare it with a fresh resolution against `rules`' ART.
    pub(crate) fn silo_damage(&self, rules: &crate::rules::ruleset::RuleSet) -> bool {
        debug_assert!(
            !crate::util::debug_checks::live_read_check_enabled()
                || self.silo_damage == self.art_silo_damage(rules.art()),
            "{}: SiloDamage projected from another ART",
            self.id
        );
        self.silo_damage
    }

    /// Whether occupants fire from this building: `CanOccupyFire=`
    /// (`+0x157C`), or in RA2 Mode, which has no such key, always
    /// ([`Self::ra2_occupy_fire`]). `IsOccupied` and GetFireError's B1 read it.
    pub(crate) fn occupants_fire(&self) -> bool {
        self.can_occupy_fire || self.ra2_occupy_fire
    }

    /// Store the type's `SiloDamage=` resolved in `art`.
    pub(crate) fn project_silo_damage(&mut self, art: &crate::rules::art_data::ArtRegistry) {
        self.silo_damage = self.art_silo_damage(art);
    }

    fn art_silo_damage(&self, art: &crate::rules::art_data::ArtRegistry) -> bool {
        art.resolve_metadata_entry(&self.id, &self.image)
            .is_some_and(|entry| entry.silo_damage)
    }

    /// The AI base planning side filter: `AIBasePlanningSide=` (`+0x6D0`) is
    /// -1 or the house's side (`HouseTypeClass+0xBC`), as
    /// `FirstBuildableFromArray @ 0x005051E0` and `AI_BuildWalls @ 0x0050C340`
    /// test it.
    pub fn planned_for_side(&self, side_index: i32) -> bool {
        self.ai_base_planning_side == -1 || self.ai_base_planning_side == side_index
    }

    /// Building43BCBD..43BCD0 allocates at least one radio contact even when
    /// the signed type count is nonpositive. This is not the coordinate-query count.
    /// VERA clamps it to [`DOCK_CONTACT_LIMIT`].
    pub fn dock_contact_capacity(&self) -> u32 {
        self.number_of_docks.clamp(1, DOCK_CONTACT_LIMIT) as u32
    }

    /// BuildingType virtual used by the Building receive-damage prelude and
    /// the native base-reservation writer.
    ///
    /// gamemd-derived: `BuildingClass` vtable `+0x80` resolves through
    /// `0x00457620` to `BuildingTypeClass__Is1x1WithUndeploy @ 0x00465D40`.
    /// Ares 3.0 BuildingTypeClass_IsUndeployable 100101D0 (hook 0x465D4A)
    /// tests a custom foundation's own width and height, which
    /// `Foundation::dimensions` already answers.
    pub fn is_1x1_with_undeploy(&self) -> bool {
        self.undeploys_into.is_some() && self.foundation.dimensions() == (1, 1)
    }

    /// YRpp `BuildingTypeClass::CanTogglePower`, inlined in DetermineAction's
    /// power arm (`0x0069286F..0x00692891`) and Ares 3.0's (`1005C790`):
    /// `TogglePower=` (`+0x154A`) with a positive drain (`+0xEE4`) or
    /// `Powered=` (`+0x1573`).
    pub fn can_toggle_power(&self) -> bool {
        self.toggle_power
            && (crate::sim::power_system::native_building_power_drain(self.power) > 0
                || self.powered)
    }

    /// BuildingClass::CanBeSelectedNow's type test (`0x00459C03`): whether a
    /// band box may select the building, [`Self::is_1x1_with_undeploy`]; Ares
    /// 3.0 BuildingClass_CanBeSelectedNow_MassSelectable 10013CF0 lets
    /// `MassSelectable=` override it.
    pub fn band_selectable(&self) -> bool {
        self.building_type_ext
            .mass_selectable(self.is_1x1_with_undeploy())
    }

    /// The slot the per-class House and HouseType factor getters select by the
    /// type's RTTI: InfantryType 0, UnitType 1, AircraftType 2, BuildingType 3,
    /// or 4 for a `BuildCat=Combat` BuildingType (`+0xE08 == 5`)
    /// (`HouseClass::GetCostBonus @ 0x0050BDF0`,
    /// `HouseClass::GetAccumulatedBonus @ 0x0050BEB0`, the build-time factor
    /// `0x0050C0A0`).
    pub fn factor_slot(&self) -> usize {
        match self.category {
            ObjectCategory::Infantry => 0,
            ObjectCategory::Vehicle => 1,
            ObjectCategory::Aircraft => 2,
            ObjectCategory::Building if self.build_cat == Some(BuildCategory::Combat) => 4,
            ObjectCategory::Building => 3,
        }
    }

    /// Membership in the ruleset's ordered `[AI] BuildConst=` list.
    pub fn build_const_eligible(&self) -> bool {
        self.build_const_eligible
    }

    /// BuildingType gate used by the native base-reservation setter. With
    /// Ares, `BuildingExt::ExtData::IsBaseNormal @ 10011810` (hooks
    /// `0x00440C08` Unlimbo, `0x00445A72` Limbo, `0x00456370` the neighbour
    /// repair) answers `AIBaseNormal=` when set (only a read the Ares way sets
    /// it); a delivery's per-building `NoBaseNormal` belongs to the
    /// superweapon chain.
    pub fn base_reservation_writer_eligible(&self) -> bool {
        if let Some(normal) = self.building_type_ext.ai_base_normal {
            return self.category == ObjectCategory::Building && normal;
        }
        self.category == ObjectCategory::Building
            && (self.undeploys_into.is_none() || !self.resource_gatherer)
            && !self.is_1x1_with_undeploy()
    }

    /// gamemd's TechnoTypeClass `+0xC8F`: whether `AI_Update` runs the per-tick
    /// damage-Spark roll for this type (`0x006FACD9`). Only
    /// `InfantryTypeClass::ReadINI` writes it: every pass clears it
    /// (`0x005240BD`) and a pass that reads the type's section sets it when
    /// `Cyborg` holds (`0x005243DD..0x005243E7`). So the last Rules pass
    /// (usually the map) must have the section; other leaves keep the ctor 0.
    /// Ares skips the clear, so there any pass that sets it keeps it set.
    /// Dormant in stock YR (no `Cyborg=yes` types).
    pub fn emits_damage_spark(&self) -> bool {
        self.damage_sparks
    }

    /// `BuildingClass` vt+0x284 (`0x00455DA0`, HasRallyPoint): a
    /// Factory=UnitType or InfantryType, UnitRepair or Cloning building. The
    /// rally click archives only on these, and their rally lines draw.
    pub fn has_rally_line(&self) -> bool {
        matches!(
            self.factory_type(),
            Some(FactoryType::InfantryType | FactoryType::UnitType)
        ) || self.unit_repair
            || self.cloning
            || self.ares_cloning_facility()
    }

    /// Ares BuildingClass_IsFactory_CloningFacility (hook `0x00455DA0` ->
    /// `10041560`): a `CloningFacility=` building answers true.
    fn ares_cloning_facility(&self) -> bool {
        self.production.cloning_facility()
    }
}
