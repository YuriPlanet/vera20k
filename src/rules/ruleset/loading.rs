//! Building a `RuleSet`: projecting processed rules layers (and INI test
//! fixtures) into the typed registries, binding type sound and EVA
//! references, and projecting the country and side registry.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::rules::combat_damage::CombatDamageDefaults;
use crate::rules::crate_rules::CrateRules;
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::mission_data::MissionControl;
use crate::rules::native_processing::{
    CountryProjection, ProcessedRulesLayers, RulesLayerStack, SideProjection,
};
use crate::rules::object_type::{HouseMasks, ObjectCategory, ObjectType};
use crate::rules::particle_system_type::{ParticleSystemType, ParticleSystemTypeId};
use crate::rules::projectile_type::ProjectileType;
use crate::rules::radar_event_config::RadarEventConfig;
use crate::rules::smudge_type::SmudgeTypeRegistry;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::terrain_rules::TerrainRules;
use crate::rules::tiberium_type::TiberiumTypeRegistry;
use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::util::lookup_hash::{LookupMap, LookupSet};

use super::{
    AI_BASE_SPACING_LIMIT, BridgeRules, CategoryIndex, CountryIdx, CountryRules, ElevationModel,
    GarrisonRules, GeneralRules, ProductionRules, ProjectileHandle, RadiationRules, RuleSet,
    SideIdx, SoundReferenceSource, TYPE_REGISTRIES, TypeHandle, WarheadHandle, WeaponHandle,
    ambient_change_terms, next_rule_set_serial,
};

impl RuleSet {
    /// Build from the active ordered rules sources.
    pub fn from_rules_layers(layers: &RulesLayerStack) -> Result<Self, RulesError> {
        Self::from_processed_rules(&layers.process()?)
    }

    /// Project processed Rules into the game's rules, the Ares way when their
    /// registry reads that way (`ares_switch::ares_way`). The projection's
    /// reader lookups are recorded, so the per-game Ares, Phobos, Kratos and
    /// NPatch switches it stores ([`Self::ares`], [`Self::phobos`],
    /// [`Self::kratos`], [`Self::npatch`]) also count the keys only these
    /// readers read.
    pub fn from_processed_rules(processed: &ProcessedRulesLayers) -> Result<Self, RulesError> {
        let _ares_way = crate::rules::ares_switch::read_ares_way(
            processed.native_registry_state().reads_ares_way(),
        );
        let (rules, reads) = crate::rules::ares_switch::record(|| Self::project(processed));
        let mut rules = rules?;
        rules.ares = processed.ares_inputs().decide(&reads);
        rules.ares_kept_off = processed.ares_inputs().kept_off(&reads);
        rules.phobos = crate::rules::phobos_switch::decide(&reads);
        rules.kratos = processed.kratos_inputs().decide(&reads);
        rules.npatch = processed.npatch_inputs().decide(&reads);
        rules.reads_ares_way = processed.native_registry_state().reads_ares_way();
        rules.apply_ra2(crate::util::ra2_launch::enabled());
        Ok(rules)
    }

    /// Record whether this game plays RA2 Mode, with what each type keeps of
    /// that answer ([`ObjectType::occupants_fire`]). The answer's one writer.
    fn apply_ra2(&mut self, ra2: bool) {
        self.ra2 = ra2;
        for object in &mut self.object_list {
            object.ra2_occupy_fire = ra2;
        }
    }

    /// Test-only: play this game in RA2 Mode, or not, as a load would decide.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_ra2_for_test(&mut self, ra2: bool) {
        self.apply_ra2(ra2);
    }

    /// The handle of the object of `category` whose stored ID is exactly `id`.
    ///
    /// Registry construction keeps one slot per category and ASCII
    /// case-insensitive ID. Projection binds exact-case state only when its
    /// spelling matches the object stored in that slot.
    fn object_handle_by_id(&self, category: ObjectCategory, id: &str) -> Option<TypeHandle> {
        self.type_handle_in_category(category, id)
            .filter(|&handle| self.object_by_handle(handle).id == id)
    }

    fn project(processed: &ProcessedRulesLayers) -> Result<Self, RulesError> {
        processed.validate_saved_state()?;
        let mut rules = Self::from_projected_ini(
            processed,
            project_country_side_registry(
                processed.country_projection(),
                processed.side_projection(),
            ),
        )?;
        let mut native_identity_counts = HashMap::new();
        for (category, id, _) in processed.techno_type_states() {
            *native_identity_counts
                .entry((category, id.to_ascii_uppercase()))
                .or_insert(0usize) += 1;
        }
        for (category, id, tech_level) in processed.techno_type_states() {
            let key = (category, id.to_ascii_uppercase());
            let object = rules.object_handle_by_id(category, id);
            if let Some(object) = object {
                // TechnoType+634 is retained typed state, not a merged INI
                // default: reached ReadINI714570 and outgoing106 handoff.
                // Equal truncated stored IDs share one projected object, which
                // the last such member's body built; its value is written last.
                rules.object_list[object.0 as usize].tech_level = tech_level;
            }
            let handle = object.filter(|_| native_identity_counts[&key] == 1);
            rules
                .native_techno_type_order
                .push((category, id.to_owned(), handle));
        }
        for (slot, (_, id, _)) in rules.native_techno_type_order.iter().enumerate() {
            rules
                .ares_techno_type_first
                .entry(id.to_ascii_uppercase())
                .or_insert(slot);
        }
        for (category, id, coefficients) in processed.techno_threat_coefficients() {
            if let Some(handle) = rules.object_handle_by_id(category, id) {
                rules.object_list[handle.0 as usize].project_threat_coefficients(coefficients);
            }
        }
        for (category, name, slots) in processed.weapon_slot_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.turret_count = slots.turret_count();
                object.weapon_count = slots.weapon_count();
                object.weapon_list = slots.base();
                object.elite_weapon_list = slots.elite();
                object.bind_weapon_offsets(slots.offsets());
            }
        }
        // Phobos TechnoTypeExt reads insignia keys on every reached pass, so
        // the processor's accumulated state is the owner, not a projection of
        // the final merged section.
        #[cfg(feature = "phobos")]
        for (category, name, insignia) in processed.techno_insignia_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.phobos_insignia = insignia.clone();
            }
        }
        for (category, name, insignia) in processed.techno_ares_insignia_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.ares_insignia = insignia.clone();
            }
        }
        rules.crate_rules = processed.crate_rules().clone();
        rules.powerups = processed.powerups().clone();
        rules.general.metallic_debris = processed.metallic_debris().to_vec();
        rules.general.weather_con_clouds = processed.weather_con_clouds().to_vec();
        rules.general.weather_con_bolts = processed.weather_con_bolts().to_vec();
        let [amer, ally, sov, yuri] = processed.paradrop_infantry().clone();
        rules.general.amer_paradrop.infantry = amer;
        rules.general.ally_paradrop.infantry = ally;
        rules.general.sov_paradrop.infantry = sov;
        rules.general.yuri_paradrop.infantry = yuri;
        rules.general.anim_to_infantry = processed.anim_to_infantry().to_vec();
        rules.general.secret_lab_choices = processed.secret_lab_choices().clone();
        rules.general.default_mirage_disguises = processed.default_mirage_disguises().to_vec();
        rules.general.infantry_disguises = processed.infantry_disguises().clone();
        rules.bridge_rules.explosions = processed.bridge_explosions().to_vec();
        rules.general.gravity = processed.gravity();
        rules.general.wind_direction = processed.wind_direction();
        rules.general.difficulty_rows = *processed.difficulty_rows();
        rules.general.detail = processed.detail();
        rules.general.talk_bubble_time = processed.talk_bubble_time();
        rules.general.spotlight = processed.spotlight();
        let (damage, blast, beam, ion_cannon) = processed.ion_blast_inputs();
        rules.bridge_warheads.ion_cannon_name = ion_cannon.to_owned();
        rules.general.ion_cannon_damage = damage;
        rules.general.ion_blast_anim = blast.to_string();
        rules.general.ion_beam_anim = beam.to_string();
        let (ambient_rate, ambient_step) = processed.ambient_change_inputs();
        (
            rules.general.ambient_change_rate_nonzero,
            rules.general.ambient_change_interval_frames,
            rules.general.ambient_change_step,
        ) = ambient_change_terms(
            f64::from_bits(ambient_rate.bits()),
            f64::from_bits(ambient_step.bits()),
        );
        rules.general.prism_support = processed.prism_support();
        rules.general.patrol_scan_minutes = processed.patrol_scan_minutes();
        rules.general.ai_hate_delays = processed.ai_hate_delays().to_vec();
        (rules.general.shroud_grow, rules.general.shroud_rate) = processed.shroud_growth_inputs();
        rules.general.fog_rate = processed.fog_rate();
        rules.general.prism_type = processed.prism_type().map(str::to_owned);
        #[cfg(feature = "phobos")]
        {
            rules.general.timer_blink_color_scheme = processed.timer_blink_color_scheme();
            rules.general.missing_cameo = processed.missing_cameo().to_owned();
        }
        let (drop_zone_anim, drop_zone_radius) = processed.drop_zone_controls();
        rules.general.drop_zone_anim = drop_zone_anim.map(str::to_owned);
        rules.general.drop_zone_radius = drop_zone_radius;
        rules.general.behind = processed.behind().map(str::to_owned);
        rules.general.prerequisite_proc_alternate =
            processed.prerequisite_proc_alternate().map(str::to_owned);
        rules.prerequisite_lists = processed.prerequisite_lists().clone();
        rules.ares_prerequisites = processed.ares_prerequisites().cloned();
        rules.ares_prerequisite_alternates = rules
            .ares_prerequisites
            .as_ref()
            .into_iter()
            .flat_map(|catalog| catalog.groups())
            .flat_map(|group| group.alternates())
            .filter_map(|alternate| {
                rules
                    .object_category_index
                    .get(alternate.category(), alternate.id())
            })
            .collect();
        rules.general.building_types = processed.building_types().clone();
        let (lightning, weather_anim, nullify_anim, splash) = processed.select_anim_rules();
        rules.general.lightning_warhead = lightning.to_owned();
        rules.general.weather_con_bolt_explosion = weather_anim.to_owned();
        rules.general.weapon_nullify_anim = nullify_anim.to_owned();
        rules.combat_damage.splash_list = splash.to_vec();
        rules.combat_damage.ammo_crate_damage = processed.ammo_crate_damage();
        rules.combat_damage.tiberium_explosion_damage = processed.tiberium_explosion_damage();
        rules.combat_damage.death_weapon =
            processed.combat_damage_death_weapon().map(str::to_owned);
        for (
            name,
            conventional,
            em_effect,
            sparky,
            anim_list,
            debris_types,
            verses,
            ares_verses,
            web,
        ) in processed.warhead_read_states()
        {
            if let Some(warhead) = rules.warhead_mut(name) {
                warhead.conventional = conventional;
                warhead.em_effect = em_effect;
                warhead.web = web;
                warhead.sparky = sparky;
                warhead.anim_list = anim_list.to_vec();
                warhead.debris_types = debris_types.to_vec();
                warhead.verses_f64 = verses.map(|value| f64::from_bits(value.bits()));
                warhead.install_ares_verses(ares_verses);
            }
        }
        for (name, particle_system) in processed.warhead_particle_states() {
            if let Some(warhead) = rules.warhead_mut(name) {
                warhead.particle_system = particle_system.map(str::to_owned);
            }
        }
        for (
            category,
            name,
            emp,
            abilities,
            attachment,
            cloak_allowed,
            turret_rot,
            aircraft_crashable,
            curtain_modifiers,
        ) in processed.ares_techno_states()
        {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.bind_ares_techno_rules(
                    emp,
                    abilities,
                    attachment,
                    cloak_allowed,
                    turret_rot,
                    aircraft_crashable,
                    curtain_modifiers,
                );
            }
        }
        // The retained canopy key can enable Ares in a stock-way game too.
        for (category, name, retained) in processed.techno_combat_states() {
            if let Some(handle) = rules.object_category_index.get(category, name) {
                rules.object_list[handle.0 as usize].bind_techno_combat_rules(retained);
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (category, name, enabled, options) in processed.ares_spotlight_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_spotlight(enabled, options);
                }
            }
            for (category, name, retained) in processed.super_weapon_techno_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_super_weapon_techno_rules(retained);
                }
            }
            for (category, name, group) in processed.selection_group_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_selection_group(group);
                }
            }
            for (category, name, driver) in processed.driver_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_driver_rules(driver);
                }
            }
            for (category, name, bounty) in processed.bounty_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_bounty_rules(bounty);
                }
            }
            for (category, name, crew) in processed.crew_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_crew_rules(crew);
                }
            }
            for (name, infiltration) in processed.infiltration_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    rules.object_list[handle.0 as usize].bind_infiltration_rules(infiltration);
                }
            }
            for (category, name, veterancy) in processed.veterancy_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_veterancy(veterancy);
                }
            }
            for (category, name, production) in processed.production_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_production(production);
                }
            }
            for (category, name, cursors) in processed.ares_cursor_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_ares_cursors(cursors);
                }
            }
            // Ares ValidateThings100677B0 (hook687C16), after every INI
            // read: a Factory= building cannot be a ClonedAt source nor a
            // CloningFacility.
            let factories: HashSet<String> = rules
                .object_list
                .iter()
                .filter(|object| {
                    object.category == ObjectCategory::Building && object.factory_type().is_some()
                })
                .map(|object| object.id.to_ascii_uppercase())
                .collect();
            for object in &mut rules.object_list {
                let factory =
                    object.category == ObjectCategory::Building && object.factory_type().is_some();
                object.ares_production_mut().validate(factory, |source| {
                    factories.contains(&source.to_ascii_uppercase())
                });
            }
            for (name, academy) in processed.academy_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    rules.object_list[handle.0 as usize].bind_academy(academy);
                }
            }
            for (category, name, retained) in processed.cloak_power_states() {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_cloak_power_rules(retained);
                }
            }
            // Ares ValidateThings100677B0 (hook687C16), after every INI read:
            // PoweredBy replaces PoweredUnit, and a PowersUnit naming a
            // PoweredBy type is cleared. Both write the live Type.
            let powered_by_types: HashSet<String> = rules
                .object_list
                .iter()
                .filter(|object| !object.cloak_power().powered_by().is_empty())
                .map(|object| object.id.to_ascii_uppercase())
                .collect();
            for object in &mut rules.object_list {
                if !object.cloak_power().powered_by().is_empty() {
                    object.powered_unit = false;
                }
                if object
                    .powers_unit
                    .as_deref()
                    .is_some_and(|unit| powered_by_types.contains(&unit.to_ascii_uppercase()))
                {
                    object.powers_unit = None;
                }
            }
        }
        for (name, ares) in processed.ares_warhead_states() {
            if let Some(warhead) = rules.warhead_mut(name) {
                warhead.bind_ares_rules(ares);
            }
        }
        for (category, name, armor) in processed.object_armor_states() {
            if let Some(category) = category {
                if let Some(handle) = rules.object_category_index.get(category, name) {
                    rules.object_list[handle.0 as usize].bind_armor(armor);
                }
            } else if let Some(terrain) = rules
                .terrain_object_types
                .get_mut(&name.to_ascii_uppercase())
            {
                terrain.bind_armor(armor);
            }
        }
        (
            rules.general.missile_rot_var,
            rules.general.safety_altitude,
            rules.general.line_trail_color_override,
        ) = processed.projectile_rule_controls();
        rules.general.chrono_beam_color = processed.chrono_beam_color();
        (
            rules.general.parachute_max_fall_rate,
            rules.general.no_parachute_max_fall_rate,
        ) = processed.fall_rate_controls();
        [
            rules.general.allied_survivor_divisor,
            rules.general.soviet_survivor_divisor,
            rules.general.third_survivor_divisor,
        ] = processed.survivor_divisors();
        rules.general.maximum_cheer_rate = processed.maximum_cheer_rate();
        rules.general.comp_easy_bonus = processed.comp_easy_bonus();
        (
            rules.general.infantry_repair_step,
            rules.general.unit_repair_rate,
            rules.general.infantry_repair_rate,
        ) = processed.repair_service_inputs();
        [
            rules.general.extra_unit_light,
            rules.general.extra_infantry_light,
            rules.general.extra_aircraft_light,
        ] = processed.class_extra_lights();
        rules.general.charge_to_drain_ratio = processed.charge_to_drain_ratio();
        let mut weapon_name_counts = HashMap::<String, usize>::new();
        for weapon in processed.weapon_states() {
            *weapon_name_counts
                .entry(weapon.id.to_ascii_uppercase())
                .or_default() += 1;
        }
        let mut warhead_name_counts = HashMap::<String, usize>::new();
        for (name, ..) in processed.warhead_read_states() {
            *warhead_name_counts
                .entry(name.to_ascii_uppercase())
                .or_default() += 1;
        }
        for weapon in processed.weapon_states() {
            // Abstract410800 truncates stored IDs to24 bytes, but native
            // FindOrAllocate compares the full incoming name. Do not alias two
            // such Types through Action42's ordinal lookup. The wider existing
            // name-projection limitation remains issue1784, without rejecting
            // unrelated custom scenarios at load.
            let unambiguous = weapon_name_counts[&weapon.id.to_ascii_uppercase()] == 1
                && weapon.warhead.is_none_or(|name| {
                    warhead_name_counts.get(&name.to_ascii_uppercase()) == Some(&1)
                });
            rules.weapon_array.push((weapon.id.to_owned(), unambiguous));
        }
        for (name, speed_type) in processed.unit_speed_type_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Vehicle, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.speed_type = speed_type;
            }
        }
        for (category, name, airstrike) in processed.airstrike_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.airstrike = airstrike.clone();
            }
        }
        for (category, name, recoil) in processed.recoil_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.recoil = recoil;
            }
        }
        for (category, name, segments) in processed.radial_fire_segments_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.radial_fire_segments = segments;
            }
        }
        for (category, name, offset) in processed.lepton_mind_control_offset_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.lepton_mind_control_offset = offset;
            }
        }
        for (category, name, prerequisite, prerequisite_override) in processed.prerequisite_states()
        {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.prerequisite = prerequisite.to_vec();
                object.prerequisite_override = prerequisite_override.to_vec();
            }
        }
        for (category, name, gunner_turrets) in processed.gunner_turret_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.gunner = gunner_turrets.is_gunner();
                object.gunner_turrets = gunner_turrets.clone();
            }
        }
        for (category, name, is_gattling, stages) in processed.gattling_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.is_gattling = is_gattling;
                object.gattling_stages = *stages;
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (category, name, state) in processed.factory_owner_states() {
                if let Some(handle) = rules.object_handle_by_id(category, name) {
                    let object = &mut rules.object_list[handle.0 as usize];
                    object.project_factory_owners(state);
                }
            }
            for (name, state) in processed.explicit_factory_states() {
                if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                    let object = &mut rules.object_list[handle.0 as usize];
                    object.project_explicit_factory(state);
                }
            }
        }
        for (category, name, references, image, crush_sound) in processed.techno_reference_states()
        {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_image_state(image);
                object.crush_sound = crush_sound.map(str::to_owned);
                references.apply_to(object);
            }
        }
        for (name, trailer) in processed.aircraft_trailer_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Aircraft, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_aircraft_trailer(trailer);
            }
        }
        for (category, name, shadow_index) in processed.voxel_shadow_states() {
            if let Some(handle) = rules.object_handle_by_id(category, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.set_voxel_shadow_index(shadow_index);
            }
        }
        for (name, pips) in processed.infantry_pip_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_infantry_pips(pips);
            }
        }
        for (name, civilian) in processed.infantry_civilian_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.civilian = civilian;
            }
        }
        for (name, sparks) in processed.infantry_damage_spark_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.damage_sparks = sparks;
            }
        }
        for (name, art) in processed.infantry_art_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_infantry_art(art);
            }
        }
        for (name, bank) in processed.infantry_sequence_banks() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_infantry_sequence_bank(bank);
            }
        }
        for (name, flags) in processed.infantry_infiltration_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Infantry, name) {
                let object = &mut rules.object_list[handle.0 as usize];
                object.project_infantry_infiltration(flags);
            }
        }
        rules.anim_type_art_read_states = processed
            .anim_type_art_read_states()
            .map(|(name, read)| (name.to_owned(), read))
            .collect();
        rules.anim_type_names.extend(
            crate::rules::native_processing::anim_type_runtime_keys(
                rules
                    .anim_type_art_read_states
                    .iter()
                    .map(|(name, _)| name.as_str()),
            )
            .map(|key| key.to_ascii_uppercase()),
        );
        rules.anim_type_images = processed
            .anim_type_image_states()
            .map(|(name, image)| (name.to_owned(), image.clone()))
            .collect();
        rules.anim_type_references = processed
            .anim_type_reference_states()
            .map(|(name, references)| (name.to_owned(), references.clone()))
            .collect();
        rules.unit_shp_read_states = processed
            .unit_shp_read_states()
            .map(|(name, state)| (name.to_owned(), *state))
            .collect();
        // Building ctor45DF13 / ReadINI461225..46125D: the process-resident
        // owner retained +EF0 across reached rules passes and exact ART reads.
        // Projection receives its result; RULES Foundation is not an input.
        for (name, config) in processed.secret_lab_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.set_secret_lab(config.clone());
            }
        }
        for (name, foundation_id) in processed.building_foundation_geometries() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.foundation = foundation_id.clone();
                building.base_reservation_spacing = building
                    .base_reservation_writer_eligible()
                    .then_some(rules.ai_base_spacing);
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (name, ext) in processed.building_type_ext_states() {
                if let Some(handle) = rules
                    .object_category_index
                    .get(ObjectCategory::Building, name)
                {
                    let building = &mut rules.object_list[handle.0 as usize];
                    building.bind_building_type_ext(ext);
                    // AIBaseNormal joins the base-reservation gate above.
                    building.base_reservation_spacing = building
                        .base_reservation_writer_eligible()
                        .then_some(rules.ai_base_spacing);
                }
            }
            // Ares CompleteInitialization 1000D380 (hook 0x679CAF), once the
            // foundations above are final: a rubble pair shares one foundation.
            let foundations: HashMap<String, crate::rules::foundation::Foundation> = rules
                .object_list
                .iter()
                .filter(|object| object.category == ObjectCategory::Building)
                .map(|object| (object.id.to_ascii_uppercase(), object.foundation.clone()))
                .collect();
            for object in rules
                .object_list
                .iter_mut()
                .filter(|object| object.category == ObjectCategory::Building)
            {
                object
                    .complete_building_type_ext(|name| foundations.get(&name.to_ascii_uppercase()));
            }
        }
        for (name, flag) in processed.building_spotlight_flags() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.has_spotlight = flag;
            }
        }
        for (name, light) in processed.building_light_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.project_building_light(light);
            }
        }
        for (name, upgrade) in processed.building_upgrade_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.project_building_upgrade(upgrade.clone());
            }
        }
        for (name, extra_power) in processed.building_extra_power_states() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.extra_power = extra_power;
            }
        }
        for (name, offset) in processed.building_target_coord_offsets() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.set_target_coord_offset(offset);
            }
        }
        for (name, location1, location2) in processed.building_half_damage_smoke_locations() {
            if let Some(handle) = rules.object_handle_by_id(ObjectCategory::Building, name) {
                let building = &mut rules.object_list[handle.0 as usize];
                building.set_half_damage_smoke_locations(location1, location2);
            }
        }
        // The registry processor owns native read timing and retained values;
        // the runtime definition receives that result, not another ART read.
        for (name, art, image) in processed.projectile_art_states() {
            if let Some(projectile) = rules.projectile_mut(name) {
                art.apply_to(image, projectile);
            }
        }
        for (name, (airburst, shrapnel)) in processed.projectile_weapon_states() {
            if let Some(projectile) = rules.projectile_mut(name) {
                projectile.airburst_weapon = airburst.map(str::to_owned);
                projectile.shrapnel_weapon = shrapnel.map(str::to_owned);
            }
        }
        for (name, (weapon, aux_building)) in processed.super_weapon_reference_states() {
            if let Some(super_weapon) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
            {
                super_weapon.weapon_type = weapon.map(str::to_owned);
                super_weapon.aux_building = aux_building.map(str::to_owned);
            }
        }
        for (name, requirements) in processed.super_weapon_building_requirements() {
            if let Some(super_weapon) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
            {
                super_weapon.building_requirements = requirements.cloned();
            }
        }
        #[cfg(feature = "phobos")]
        {
            rules.phobos_super_weapon_metadata.clear();
            for (name, availability) in processed.phobos_super_weapon_availability_states() {
                rules
                    .phobos_super_weapon_metadata
                    .insert(name.to_ascii_uppercase(), availability);
            }
            for (name, gathering) in processed.ore_gathering_states() {
                if let Some(handle) = rules.type_handle_in_category(ObjectCategory::Vehicle, name) {
                    let unit = &mut rules.object_list[handle.0 as usize];
                    unit.project_ore_gathering(gathering);
                }
            }
        }
        if crate::rules::ares_switch::ares_way() {
            for (name, presentation) in processed.super_weapon_presentation_states() {
                if let Some(sw) = rules
                    .super_weapons
                    .values_mut()
                    .find(|sw| sw.id.eq_ignore_ascii_case(name))
                {
                    sw.presentation = presentation.clone();
                }
            }
            for (name, supers) in processed.additional_super_weapon_states() {
                if let Some(handle) = rules.type_handle_in_category(ObjectCategory::Building, name)
                {
                    let building = &mut rules.object_list[handle.0 as usize];
                    building.project_additional_super_weapons(supers);
                }
            }
        }
        for (name, settings) in processed.hunter_seeker_states() {
            if let Some(sw) = rules
                .super_weapons
                .values_mut()
                .find(|sw| sw.id.eq_ignore_ascii_case(name))
            {
                sw.hunter_seeker = settings.cloned();
            }
        }
        for (name, delivery) in processed.unit_delivery_states() {
            if let Some(sw) = rules
                .super_weapons
                .values_mut()
                .find(|sw| sw.id.eq_ignore_ascii_case(name))
            {
                sw.unit_delivery = delivery.cloned();
            }
        }
        // Equal truncated stored IDs share one projected type, which the last
        // such member's state is written to last.
        for (name, warhead) in processed.generic_super_weapon_warhead_states() {
            if let Some(settings) = rules
                .super_weapons
                .values_mut()
                .find(|super_weapon| super_weapon.id.eq_ignore_ascii_case(name))
                .and_then(|super_weapon| super_weapon.generic_warhead.as_mut())
            {
                settings.warhead = warhead.map(str::to_owned);
            }
        }
        for (name, references, damage) in processed.voxel_anim_reference_states() {
            if let Some(voxel_anim) = rules
                .voxel_anim_types
                .iter_mut()
                .find(|voxel_anim| voxel_anim.name.eq_ignore_ascii_case(name))
            {
                references.apply_to(voxel_anim);
                voxel_anim.damage = damage;
            }
        }
        processed.rules_type_references().apply_to(&mut rules);
        // Ares 3.0 InfantryClass_ReceiveDamage_ElectricDeath (1002C930, hook
        // 0x518744) replaces the InfDeath=5 anim, AnimTypes[1]: the RulesExt
        // InfantryElectrocuted, else Find("ELECTRO") over the live registry,
        // else AnimTypes[1]. The first death caches it; no later read
        // changes the registry, so it is resolved here.
        if crate::rules::ares_switch::ares_way() {
            let anims = processed
                .anim_type_art_read_states()
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            let key = |index| {
                crate::rules::native_processing::anim_type_runtime_key(anims.iter().copied(), index)
                    .map(|key| key.into_owned())
            };
            let electric = rules
                .ares_general()
                .infantry_electrocuted()
                .map(str::to_owned)
                .or_else(|| {
                    anims
                        .iter()
                        .position(|id| id.eq_ignore_ascii_case("ELECTRO"))
                        .and_then(key)
                })
                .or_else(|| key(1));
            if electric.is_some() {
                rules.general.infantry_death_anims[5] = electric;
            }
        }
        rules.project_house_masks();
        rules.rebuild_animation_sequences();
        rules.resolve_handles();
        rules.rebuild_sound_names();
        Ok(rules)
    }

    /// Derive every type's [`HouseMasks`] against the current Country
    /// registry. The Rules build and [`Self::refresh_country_side_projection`]
    /// are the registry's only writers, and both end here.
    fn project_house_masks(&mut self) {
        let masks: Vec<HouseMasks> = self
            .object_list
            .iter()
            .map(|object| HouseMasks::derive(object, self))
            .collect();
        for (object, masks) in self.object_list.iter_mut().zip(masks) {
            object.project_house_masks(masks);
        }
    }

    /// The `ids` whose Type has an object of `category`. The ordered Rules
    /// processor allocates a Type for each token its factory misses, and
    /// native keeps it in the list; VERA's projection builds no object for
    /// some of them (a token with surrounding spaces) and drops them here, an
    /// approved exclusion that retail never reaches.
    pub(crate) fn ids_with_objects(&self, ids: &[String], category: ObjectCategory) -> Vec<String> {
        ids.iter()
            .filter(|id| self.object_category_index.contains(category, id))
            .cloned()
            .collect()
    }

    /// Resolved construction-yard types in the order read from `[AI] BuildConst=`.
    pub fn build_const_types(&self) -> &[String] {
        &self.build_const_types
    }

    /// Set the ordered list and its per-building membership together.
    pub(in crate::rules) fn set_build_const_types(&mut self, ids: &[String]) {
        self.build_const_types = self.ids_with_objects(ids, ObjectCategory::Building);
        for object in &mut self.object_list {
            object.build_const_eligible = false;
        }
        for id in &self.build_const_types {
            // `ids_with_objects` kept only ids this index holds.
            if let Some(handle) = self.object_category_index.get(ObjectCategory::Building, id) {
                self.object_list[handle.0 as usize].build_const_eligible = true;
            }
        }
    }

    /// Resolve type sound readers against the startup-selected
    /// SOUNDMD registry. The processed projection retains only passes where
    /// each type existed, including the native current-ID retention order.
    /// Type defaults and Rules+208 start at -1; [AudioVisual] owns the latter.
    pub fn bind_type_sound_references<'a>(
        &mut self,
        source: impl Into<SoundReferenceSource<'a>>,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        let source = source.into();
        let ini = source.ini();
        // Original669423..6694B6, in native open/close read order. The
        // processed section already owns the reached Rules pass history.
        self.general.gui_open_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUIOpenSound", None));
        self.general.gui_close_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GUICloseSound", None));
        self.general.disk_laser_charge_up = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "DiskLaserChargeUp", None));
        self.general.create_unit_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateUnitSound", None));
        self.general.create_infantry_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateInfantrySound", None));
        self.general.create_aircraft_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CreateAircraftSound", None));
        // ReadAudioVisual 0x66b550..0x66b596 clears Rules+0x648, then reads
        // IceCrackSounds with ReadSoundList525430 over the emptied list.
        // Residual: the processed section keeps the last pass that set the
        // key, where a later pass without it empties gamemd's list.
        self.general.ice.crack_sounds =
            ini.section("AudioVisual").map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "IceCrackSounds")
            });
        // Original66A763..66A973, in native read order for these bound fields. Use the same fixed
        // registry and history-aware reader as every other bound Voc ID.
        self.general.game_closed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameClosed", None));
        self.general.incoming_message_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "IncomingMessage", None));
        self.general.message_char_typed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MessageCharTyped", None));
        self.general.system_error_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SystemError", None));
        self.general.options_changed_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "OptionsChanged", None));
        self.general.game_forming_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GameForming", None));
        self.general.player_left_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerLeft", None));
        self.general.player_joined_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlayerJoined", None));
        self.general.airstrike_abort_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAbortSound", None));
        self.general.airstrike_attack_voice = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "AirstrikeAttackVoice", None));
        if crate::rules::ares_switch::ares_way() {
            self.general.gate_sounds = ["GateDown", "GateUp"].map(|key| {
                ini.section("AudioVisual")
                    .and_then(|section| sounds.read_rules_reference(section, key, None))
            });
        }
        self.general.leave_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "LeaveBioReactorSound", None));
        self.general.enter_bio_reactor_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterBioReactorSound", None));
        self.general.enter_grinder_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EnterGrinderSound", None));
        self.general.sinking_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SinkingSound", None));
        self.general.score_anim_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoreAnimSound", None));
        self.general.construction_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "Construction", None));
        self.general.building_abandoned_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "BuildingAbandonedSound", None)
        });
        self.general.building_damage_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "BuildingDamageSound", None));
        self.general.building_repaired_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "BuildingRepairedSound", None)
        });
        self.general.spy_plane_camera = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "SpyPlaneCamera", None));
        if crate::rules::ares_switch::ares_way() {
            self.general.ares_decloak_sound = ini
                .section("AudioVisual")
                .and_then(|section| sounds.read_ares_reference(section, "DecloakSound"));
        }
        self.general.stop_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "StopSound", None));
        self.general.guard_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GuardSound", None));
        // Original66AAC0..66AB53: fixed process catalog, exact keys and
        // current-ID retention across the existing processed read history.
        self.general.radar_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOn", None));
        self.general.radar_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "RadarOff", None));
        self.general.movie_on = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOn", None));
        self.general.movie_off = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "MovieOff", None));
        // Original66A4EF..66A582 shares the retained fixed-catalog reader.
        self.general.spy_sat_activation_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "SpySatActivationSound", None)
        });
        self.general.spy_sat_deactivation_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "SpySatDeactivationSound", None)
        });
        self.general.voice_ifv_repair = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "VoiceIFVRepair", None));
        // Original ReadAudioVisual order: Start6696C0, End66970A, Add669B25.
        // The processed section owns pass history; the fixed catalog owns IDs.
        self.general.start_planning_mode_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "StartPlanningModeSound", None)
        });
        self.general.end_planning_mode_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "EndPlanningModeSound", None));
        self.general.add_planning_mode_command_sound =
            ini.section("AudioVisual").and_then(|section| {
                sounds.read_rules_reference(section, "AddPlanningModeCommandSound", None)
            });
        // Original669BA8, after Add669B25 in the same AudioVisual pass.
        self.general.place_beacon_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "PlaceBeaconSound", None));
        self.general.time_warp_out_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "LetsDoTheTimeWarpOutAgain", None)
        });
        self.general.time_warp_in_sound = ini.section("AudioVisual").and_then(|section| {
            sounds.read_rules_reference(section, "LetsDoTheTimeWarpInAgain", None)
        });
        self.general.cheer_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "CheerSound", None));
        self.general.scatter_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScatterSound", None));
        self.general.scold_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "ScoldSound", None));
        // RulesClass::ReadAudioVisual66AD24..66AD56: ReadString128 then
        // VocFind7514D0; unknown later passes retain the previous resolved ID.
        self.general.generic_click_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "GenericClick", None));
        // Original RulesClass::ReadAudioVisual669CE2..669D68 resolves
        // +228/+22C through ReadString128/VocFind7514D0, independently of
        // ReadGeneral. Reuse the fixed-catalog/pass-history owner, including
        // invalid later reads retaining their previous IDs (ctor66596A/70=-1).
        self.general.upgrade_veteran_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeVeteranSound", None));
        self.general.upgrade_elite_sound = ini
            .section("AudioVisual")
            .and_then(|section| sounds.read_rules_reference(section, "UpgradeEliteSound", None));
        // WeaponTypeClass::ReadINI @ `0x00772080` calls
        // CCINIClass::ReadSoundList @ `0x00525430` for Report/DownReport at
        // `0x00772394`/`0x0077241A`; use its shared catalog/list owner.
        for weapon in &mut self.weapon_types {
            let section = ini.section(&weapon.id);
            weapon.report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "Report")
            });
            weapon.down_report = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "DownReport")
            });
            // WeaponTypeExt LoadFromINIFile10055520: NullableIdx<Voc>
            // 1000E9D0 reads of the custom Ivan bomb's two cues.
            if crate::rules::ares_switch::ares_way() {
                weapon.ares_ivan_bomb.ticking_sound = section.and_then(|section| {
                    sounds.read_ares_reference(section, "IvanBomb.TickingSound")
                });
                weapon.ares_ivan_bomb.attach_sound = section.and_then(|section| {
                    sounds.read_ares_reference(section, "IvanBomb.AttachSound")
                });
            }
        }
        for sw in self.super_weapons.values_mut() {
            let section = ini.section(&sw.id);
            if crate::rules::ares_switch::ares_way() {
                // SWTypeExt::LoadFromINI10033522 -> NullableIdx<Voc>
                // 1000E9D0: successful names/none set presence; unknown
                // retains. Read against fixed SOUNDMD, in rules-pass order.
                sw.presentation.sound =
                    section.and_then(|section| sounds.read_ares_reference(section, "SW.Sound"));
                // 10033538: SW.ActivationSound, the same NullableIdx<Voc> read.
                sw.presentation.activation_sound = section
                    .and_then(|section| sounds.read_ares_reference(section, "SW.ActivationSound"));
            }
            // Original6CEB74..6CEBD8 keeps each current Voc ID when
            // ReadString128 or FindIndex7514D0 finds no replacement.
            sw.special_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "SpecialSound", None));
            sw.start_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "StartSound", None));
            if crate::rules::ares_switch::ares_way()
                && let Some(delivery) = sw.unit_delivery.as_mut()
            {
                // Ares NullableIdx<VocClass>: unknown retains the previous
                // resolved cue; an explicit none sentinel clears it. The
                // retained type INI owns the ordered read history.
                delivery.activation_sound = section
                    .and_then(|section| sounds.read_ares_reference(section, "SW.ActivationSound"))
                    .flatten();
            }
        }
        // WarheadTypeExt ReadINI10051D80: DieSound.Override/VoiceDie.Override
        // are NullableIdx<Voc> 1000E9D0 reads against the fixed SOUNDMD.
        for warhead in &mut self.warhead_types {
            let section = ini.section(&warhead.id);
            if let Some(ares) = warhead.ares_rules_mut() {
                ares.combat_mut().bind_sound_overrides(section, sounds);
            }
        }
        let type_array_indices = &self.type_array_indices;
        for object in &mut self.object_list {
            let section = match source {
                SoundReferenceSource::Processed(processed) => type_array_indices
                    .get(object.category, &object.id)
                    .and_then(|index| processed.techno_type_read_section(object.category, index)),
                #[cfg(any(test, feature = "test-support"))]
                SoundReferenceSource::Fixture(ini) => ini.section(&object.id),
            };
            object.bind_scalar_sound_references(section, sounds);
            object.bind_deploy_voice_references(section, sounds);
            object.bind_order_voice_references(section, sounds);
            object.voice_select = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelect")
            });
            object.voice_select_enslaved = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelectEnslaved")
            });
            object.voice_select_deactivated = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSelectDeactivated")
            });
            object.move_sound = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "MoveSound")
            });
            object.voice_special_attack = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceSpecialAttack")
            });
            object.voice_feedback = section.map_or_else(Vec::new, |section| {
                sounds.read_rules_sound_list(section, "VoiceFeedback")
            });
            if object.category == crate::rules::object_type::ObjectCategory::Building {
                object.buildup_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "BuildupSound", None));
                // BuildingType46088F/4608DD read WorkingSound (+0xE80) and
                // NotWorkingSound (+0xE84) the same way.
                object.working_sound = section
                    .and_then(|section| sounds.read_rules_reference(section, "WorkingSound", None));
                object.not_working_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "NotWorkingSound", None)
                });
            }
            object.sinking_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "SinkingSound", None));
            object.voice_sinking = section
                .and_then(|section| sounds.read_rules_reference(section, "VoiceSinking", None));
            // TechnoType712FEC reads VoiceFalling (+0x54C) the same way.
            object.voice_falling = section
                .and_then(|section| sounds.read_rules_reference(section, "VoiceFalling", None));
            // InfantryType524193 reads VoiceComment (+0xE88) through
            // ReadSoundList525430; no other type class reads the key.
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                object.voice_comment = section.map_or_else(Vec::new, |section| {
                    sounds.read_rules_sound_list(section, "VoiceComment")
                });
            }
            // TechnoType7133BA reads TurretRotateSound (+0x560) through
            // ReadString128 and Voc7514D0 for every techno type; the key is not
            // category-gated even though only buildings consume it.
            object.turret_rotate_sound = section.and_then(|section| {
                sounds.read_rules_reference(section, "TurretRotateSound", None)
            });
            // TechnoType7138D1/71390D read ActivateSound (+0x5A8) and
            // DeactivateSound (+0x5AC) the same way, for every techno type.
            object.activate_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "ActivateSound", None));
            object.deactivate_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "DeactivateSound", None));
            // ObjectType5F93C1 reads AmbientSound (+0x1F4) the same way, for
            // every object type.
            object.ambient_sound = section
                .and_then(|section| sounds.read_rules_reference(section, "AmbientSound", None));
            if object.category == crate::rules::object_type::ObjectCategory::Infantry {
                // Infantry52440B then524447: missing/empty/unregistered names
                // keep the prior signed ID across the existing processed passes.
                object.enter_water_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "EnterWaterSound", None)
                });
                object.leave_water_sound = section.and_then(|section| {
                    sounds.read_rules_reference(section, "LeaveWaterSound", None)
                });
                // ReadSequenceData (`0x0052403F`) resolves each pair through
                // VocClass::FindByName (`0x007514D0`, first _strcmpi match).
                object.bind_sequence_sounds(sounds);
            }
        }
        self.rebuild_sound_names();
    }

    pub(crate) fn bind_terrain_type_sound_references(
        &mut self,
        processed: &ProcessedRulesLayers,
        sounds: &crate::rules::sound_ini::SoundRegistry,
    ) {
        for (name, section) in processed.terrain_type_read_sections() {
            if let Some(terrain) = self
                .terrain_object_types
                .get_mut(&name.to_ascii_uppercase())
            {
                terrain.bind_sound_references(section, sounds);
            }
        }
    }

    /// Resolve Ares refusal cues through the same fixed EVAMD catalog that
    /// owns process playback. The retained INI body supplies each read pass.
    /// Only Rules read the Ares way (`ares_switch::ares_way`) have them.
    pub fn bind_type_eva_references(
        &mut self,
        ini: &IniFile,
        eva: &crate::rules::sound_ini::EvaRegistry,
    ) {
        if crate::rules::ares_switch::ares_way() {
            for sw in self.super_weapons.values_mut() {
                // LoadFromINI10033370 reads these ValueableIdx<VoxClass> in this
                // order after Ready/Activated/Detected; neither has a handler
                // Initialize write. Ctor10031460: Impatient -1, SelectTarget
                // and InsufficientFunds FindIndex of their literal names.
                sw.impatient_eva =
                    eva.read_rules_reference(ini.section(&sw.id), "EVA.Impatient", "");
                sw.insufficient_funds_eva = eva.read_rules_reference(
                    ini.section(&sw.id),
                    "EVA.InsufficientFunds",
                    "EVA_InsufficientFunds",
                );
                sw.select_target_eva = eva.read_rules_reference(
                    ini.section(&sw.id),
                    "EVA.SelectTarget",
                    "EVA_SelectTarget",
                );
            }
            for object in &mut self.object_list {
                // TypeExt LoadFromINI1003DFB0 reads both ValueableIdx<VoxClass>
                // after the promotion sounds; Initialize1003DEC0 seeds each with
                // FindIndex("EVA_UnitPromoted").
                let section = ini.section(&object.id);
                object.bind_promotion_eva([
                    eva.read_rules_reference(section, "EVA.VeteranPromoted", "EVA_UnitPromoted"),
                    eva.read_rules_reference(section, "EVA.ElitePromoted", "EVA_UnitPromoted"),
                ]);
                // EVA.Lost, read before those; Initialize1003DEC0 seeds
                // FindIndex("EVA_UnitLost") for every type but a building.
                let building =
                    object.category == crate::rules::object_type::ObjectCategory::Building;
                object.bind_unit_lost_eva(eva.read_rules_reference(
                    section,
                    "EVA.Lost",
                    if building { "" } else { "EVA_UnitLost" },
                ));
                // BuildingTypeExt LoadFromINIFile 1000DD40 reads LostEvaEvent;
                // the Initialize of 1000DCF0 seeds FindIndex("EVA_TechBuildingLost").
                if object.category == crate::rules::object_type::ObjectCategory::Building {
                    object.bind_lost_eva(eva.read_rules_reference(
                        section,
                        "LostEvaEvent",
                        "EVA_TechBuildingLost",
                    ));
                }
            }
        }
    }

    /// Parse a complete RuleSet from a rules.ini IniFile.
    ///
    /// Loads all type registries, individual object sections, and any
    /// weapons/warheads referenced by those objects. Missing sections
    /// are logged as warnings but don't cause errors — RA2's rules.ini
    /// sometimes references sections that don't exist.
    pub fn from_ini(ini: &IniFile) -> Result<Self, RulesError> {
        Self::from_rules_layers(&RulesLayerStack::new(ini.clone()))
    }

    /// The rules an empty `RULESMD.INI` reads to: every key at its reader
    /// default and no types. Fixtures that exercise no rules data pass it
    /// where the game passes the scenario's rules.
    #[cfg(any(test, feature = "test-support"))]
    pub fn empty_for_test() -> &'static RuleSet {
        // One for each Rules-reading mode (`ares_switch::ares_way`).
        static EMPTY: [std::sync::OnceLock<RuleSet>; 2] =
            [std::sync::OnceLock::new(), std::sync::OnceLock::new()];
        EMPTY[usize::from(crate::rules::ares_switch::ares_way())].get_or_init(|| {
            RuleSet::from_ini(&IniFile::from_str("")).expect("an empty rules INI reads")
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_fixed_art_and_sounds_for_test(
        ini: &IniFile,
        art: &IniFile,
        sound_names: &[&str],
    ) -> Result<Self, RulesError> {
        let mut catalog = String::from("[SoundList]\n");
        for (index, name) in sound_names.iter().enumerate() {
            catalog.push_str(&format!("{index}={name}\n"));
        }
        let mut owner =
            crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                ini.clone(),
                None,
                art.clone(),
                std::sync::Arc::new(crate::rules::sound_ini::SoundRegistry::from_ini(
                    &IniFile::from_str(&catalog),
                )),
                std::sync::Arc::default(),
            )?;
        owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::Campaign(None),
                &IniFile::empty(),
            )
            .map(|loaded| loaded.into_parts().0)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_fixed_art_for_test(
        ini: &IniFile,
        art: &IniFile,
    ) -> Result<Self, RulesError> {
        Self::from_processed_rules(&RulesLayerStack::new(ini.clone()).process_with_fixed_art(art)?)
    }

    /// [`Self::from_ini`] with the type sounds bound, as production binds
    /// them, against a SOUNDMD `[SoundList]` of `sound_names`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_ini_with_sounds_for_test(
        ini: &IniFile,
        sound_names: &[&str],
    ) -> Result<Self, RulesError> {
        Self::from_ini_with_fixed_art_and_sounds_for_test(ini, &IniFile::empty(), sound_names)
    }

    fn from_projected_ini(
        processed: &ProcessedRulesLayers,
        country_side_registry: ProjectedCountrySideRegistry,
    ) -> Result<Self, RulesError> {
        let ini = processed.ini();
        let mut object_list: Vec<ObjectType> = Vec::new();
        let mut object_index: LookupMap<String, TypeHandle> = LookupMap::default();
        let mut object_category_index: CategoryIndex<TypeHandle> = CategoryIndex::default();
        let mut type_array_indices: CategoryIndex<i32> = CategoryIndex::default();
        let mut infantry_ids: Vec<String> = Vec::new();
        let mut vehicle_ids: Vec<String> = Vec::new();
        let mut aircraft_ids: Vec<String> = Vec::new();
        let mut building_ids: Vec<String> = Vec::new();
        let production: ProductionRules = ProductionRules::from_ini(ini);
        let mut general: GeneralRules = GeneralRules::from_ini(ini);
        (general.drain_money_frame_delay, general.drain_money_amount) =
            processed.drain_money_controls();
        general.attack_cursor_on_disguise = processed.attack_cursor_on_disguise();
        general.allow_shrouded_subterannean_moves = processed.allow_shrouded_subterannean_moves();
        general.tree_strength = processed.tree_strength();
        general.threat_coefficient_defaults = processed.threat_coefficient_defaults();
        general.bomb_ticking_sound = processed.bomb_ticking_sound().map(str::to_owned);
        general.bomb_attach_sound = processed.bomb_attach_sound().map(str::to_owned);
        general.psychic_reveal_activate_sound =
            processed.psychic_reveal_activate_sound().map(str::to_owned);
        let [gui_build, gui_tab, building_slam] = processed.ui_sounds();
        general.gui_build_sound = gui_build.map(str::to_owned);
        general.gui_tab_sound = gui_tab.map(str::to_owned);
        general.building_slam = building_slam.map(str::to_owned);
        let (_, color_indexes) = processed.color_add_state();
        general.laser_target_color = color_indexes.laser_target_color();
        general.iron_curtain_color = color_indexes.iron_curtain_color();
        general.berserk_color = color_indexes.berserk_color();
        general.force_shield_color = color_indexes.force_shield_color();
        // `RulesClass::ReadAI`'s ReadInt (`0x00673D7E`, Rules+0x1460), within
        // VERA's bound.
        let ai_base_spacing = ini
            .section_or_empty("AI")
            .read_int("AIBaseSpacing", 1)
            .clamp(-AI_BASE_SPACING_LIMIT, AI_BASE_SPACING_LIMIT);
        // Original-RA2 `[AI]` ore-scan radii (game.exe 0x0064D005/0x0064D025,
        // ReadInt into RulesClass+0x13E0/+0x13E4; constructor default 0). YR
        // has no such keys, so it reads the 0 default with no behavior change.
        let tiberium_near_scan = ini.section_or_empty("AI").read_int("TiberiumNearScan", 0);
        let tiberium_far_scan = ini.section_or_empty("AI").read_int("TiberiumFarScan", 0);
        // `DifficultyClass::ReadINI_IntVector @ 0x00475D70`, reached by
        // `RulesClass::ReadGeneral` for AISlaveMinerNumber
        // `0x00670585..0x006705B7`, AIExtraRefineries `0x006705F9..0x0067062A`
        // and the three BaseDefenseCounts vectors `0x00670013..0x006700BE`.
        let parse_difficulty_vector = |key: &str| {
            ini.section_or_empty("General")
                .read_int_list(key)
                .unwrap_or_default()
        };
        let ai_slave_miner_number = parse_difficulty_vector("AISlaveMinerNumber");
        let ai_extra_refineries = parse_difficulty_vector("AIExtraRefineries");
        let allied_base_defense_counts = parse_difficulty_vector("AlliedBaseDefenseCounts");
        let soviet_base_defense_counts = parse_difficulty_vector("SovietBaseDefenseCounts");
        let third_base_defense_counts = parse_difficulty_vector("ThirdBaseDefenseCounts");
        // Original RA2 hardcodes these three difficulty vectors in game.exe and
        // does not print them into rules.ini (YR moved them to rulesmd.ini with
        // the "formerly hardcoded" comment). With no INI value VERA would build
        // empty vectors -> BasePlan preflight fails -> MCV deploy is refused and
        // the AI never leaves its opening lull. Only the RA2 launch profile
        // substitutes the native hardcoded values; YR data always carries the
        // keys, so the YR path is untouched.
        let (ai_slave_miner_number, ai_extra_refineries, third_base_defense_counts) =
            if crate::util::ra2_launch::enabled() {
                (
                    if ai_slave_miner_number.is_empty() {
                        vec![4, 3, 2]
                    } else {
                        ai_slave_miner_number
                    },
                    if ai_extra_refineries.is_empty() {
                        vec![2, 1, 0]
                    } else {
                        ai_extra_refineries
                    },
                    if third_base_defense_counts.is_empty() {
                        vec![25, 22, 6]
                    } else {
                        third_base_defense_counts
                    },
                )
            } else {
                (
                    ai_slave_miner_number,
                    ai_extra_refineries,
                    third_base_defense_counts,
                )
            };
        // gamemd-derived: `RulesClass::Constructor @ 0x00666922` stores 20 at
        // +0xE0C; `RulesClass::ReadGeneral @ 0x006701D9..0x006701FE` reads the
        // signed `AINavalYardAdjacency=` override.
        let ai_naval_yard_adjacency = ini
            .section_or_empty("General")
            .read_int("AINavalYardAdjacency", 20);
        let initial_veteran = ini
            .section_or_empty("SpecialFlags")
            .read_bool("InitialVeteran", false);
        let terrain_rules: TerrainRules = processed.terrain_rules().clone();
        let tiberium_types = TiberiumTypeRegistry::from_processed_rules(processed);
        let bridge_rules: BridgeRules = BridgeRules::from_ini(ini);
        let crate_rules = CrateRules::default();
        // Overwritten from the processed layers by the two callers above; the
        // projection path has no `[Powerups]` accumulator of its own.
        let powerups = crate::rules::powerups::PowerupTable::default();
        let garrison_rules: GarrisonRules = GarrisonRules::from_ini(ini);
        // `RulesClass::ReadWallModel` 0x0066D1F0 (body 0x0066D1F0-0x0066D262)
        // leaves the constructor value (0, from 0x00667825) in place when
        // `[WallModel]` is absent: 0x0066D20E passes the current
        // `[ESI+0x1850]` as the default and 0x0066D22E stores the answer back.
        let allied_wall_transparency: bool = ini
            .section_or_empty("WallModel")
            .read_bool("AlliedWallTransparency", false);
        let elevation_model = ElevationModel::from_ini(ini);
        let radiation: RadiationRules = RadiationRules::from_ini(ini);
        #[cfg(feature = "phobos")]
        let insignia_types = processed.insignia_types().cloned().unwrap_or_default();
        #[cfg(feature = "phobos")]
        let phobos_draw_globals = crate::rules::insignia_type::PhobosDrawGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let (shield_types, shield_globals) = {
            let shield_types = processed.shield_types().cloned().unwrap_or_default();
            log::info!(
                "Phobos shield types: [{}]",
                shield_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                shield_types,
                crate::rules::phobos_shields::ShieldGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let (digital_display_types, digital_display_globals) = {
            let digital_display_types = processed
                .digital_display_types()
                .cloned()
                .unwrap_or_default();
            log::info!(
                "Phobos digital display types: [{}]",
                digital_display_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                digital_display_types,
                crate::rules::digital_display::DigitalDisplayGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let (attach_effect_types, attach_effect_globals) = {
            let attach_effect_types = processed.attach_effect_types().cloned().unwrap_or_default();
            log::info!(
                "Phobos attach effect types: [{}]",
                attach_effect_types.names().collect::<Vec<_>>().join(", ")
            );
            (
                attach_effect_types,
                crate::rules::attach_effect::AttachEffectGlobals::from_ini(ini),
            )
        };
        #[cfg(feature = "phobos")]
        let placement_preview_globals =
            crate::rules::phobos_placement_preview::PlacementPreviewGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let radiation = {
            let mut radiation = radiation;
            radiation.radiation_types = processed.radiation_types().cloned().unwrap_or_default();
            // The catalog is the only rules state a mod names by list, so
            // report it once per load: a missing entry then shows up here
            // instead of as an unchanged radiation field.
            log::info!(
                "Phobos radiation types: [{}]",
                radiation
                    .radiation_types
                    .names()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            radiation
        };
        let radar_event_config: RadarEventConfig = RadarEventConfig::from_ini(ini);
        let countries = country_side_registry.rules;
        #[cfg(feature = "phobos")]
        let side_gdi_positions: Vec<bool> = country_side_registry
            .side_ids
            .iter()
            .enumerate()
            .map(|(index, name)| {
                ini.section(name).map_or(index == 0, |section| {
                    section.read_bool("Sidebar.GDIPositions", index == 0)
                })
            })
            .collect();
        #[cfg(feature = "phobos")]
        let sw_sidebar = crate::rules::phobos_sw_sidebar::SuperWeaponSidebarGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let selection_flash =
            crate::rules::phobos_selection_flash::SelectionFlashGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let fly_no_wobbles = ini
            .section("AudioVisual")
            .and_then(|section| section.read_phobos_nullable_bool("FlyNoWobbles", None));
        #[cfg(feature = "phobos")]
        let power_plant_enhancer =
            crate::rules::phobos_power_plant_enhancer::PowerPlantEnhancerGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let phobos_airburst = crate::rules::phobos_airburst::PhobosAirburstGlobals::from_ini(ini);
        #[cfg(feature = "phobos")]
        let side_sw_sidebar_art: Vec<
            crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt,
        > = country_side_registry
            .side_ids
            .iter()
            .map(|name| {
                ini.section(name)
                    .map(crate::rules::phobos_sw_sidebar::SuperWeaponSidebarSideArt::from_section)
                    .unwrap_or_default()
            })
            .collect();
        let mut color_schemes = crate::rules::color_scheme::parse_color_schemes(ini);
        // Product extension: the lobby's colours beyond YR's eight. The mode
        // appends the built-in ones `[Colors]` leaves out, so the ramps below
        // and every other scheme consumer hold them.
        let skirmish_color_mode =
            crate::rules::skirmish_colors::SkirmishColorMode::from_ini(ini, &mut color_schemes);
        let house_color_ramps =
            crate::rules::house_colors::HouseColorRamps::from_schemes(&color_schemes);
        // VERA's team-colour view (Alt+T) draws enemies by team; the palette
        // comes from the same lobby colours, so a mod's [Colors] decides it.
        let enemy_team_palette = crate::rules::team_colors::EnemyTeamPalette::from_rules(
            skirmish_color_mode,
            &color_schemes,
        );
        log::info!(
            "VERA team colours: {} warm colour(s) for enemy teams",
            enemy_team_palette.entries().len()
        );
        let color_add = processed.color_add_state().0.clone();

        // Step 1: Parse each type registry and load object sections.
        for &(registry_name, category) in TYPE_REGISTRIES {
            let ids: Vec<String> = parse_registry(ini, registry_name);
            log::info!("Registry [{}]: {} entries", registry_name, ids.len());

            for (registry_index, id) in ids.iter().enumerate() {
                let array_index =
                    type_array_indices.get_or_insert(category, id, registry_index as i32);
                if let Some(section) = ini.section(id) {
                    let mut obj: ObjectType = ObjectType::from_ini_section(id, section, category);
                    if category == ObjectCategory::Building {
                        obj.base_plan_type_index = array_index;
                    }
                    if obj.base_reservation_writer_eligible() {
                        obj.base_reservation_spacing = Some(ai_base_spacing);
                    }
                    let key = id.to_ascii_uppercase();
                    // Find-or-allocate is per native type registry. A duplicate
                    // within one registry reuses its slot; the same ID in a
                    // different registry remains a distinct native type.
                    let handle = match object_category_index.get(category, id) {
                        Some(TypeHandle(idx)) => {
                            log::warn!(
                                "Object '{}' merges onto an existing case-duplicate {:?} type",
                                id,
                                category,
                            );
                            object_list[idx as usize] = obj;
                            TypeHandle(idx)
                        }
                        None => {
                            let handle = TypeHandle(object_list.len() as u32);
                            object_list.push(obj);
                            object_category_index.insert(category, id, handle);
                            handle
                        }
                    };
                    if object_index.get(&key).is_some_and(|prior| *prior != handle) {
                        log::warn!(
                            "Object '{}' is registered in multiple native type categories",
                            id
                        );
                    }
                    // Preserve the pre-existing broad lookup's later-registry
                    // winner for callers that do not own a category order.
                    object_index.insert(key, handle);
                } else {
                    log::trace!(
                        "Object '{}' listed in [{}] but has no section",
                        id,
                        registry_name
                    );
                }
            }

            // Store ID lists per category.
            match category {
                ObjectCategory::Infantry => infantry_ids = ids,
                ObjectCategory::Vehicle => vehicle_ids = ids,
                ObjectCategory::Aircraft => aircraft_ids = ids,
                ObjectCategory::Building => building_ids = ids,
            }
        }

        // Step 2: WeaponTypes. The ordered Rules processor allocated every
        // weapon a reached reader names (TechnoType slots, DeathWeapon,
        // OccupyWeapon, SuperWeapon WeaponType, [CombatDamage] DeathWeapon,
        // Projectile AirburstWeapon/ShrapnelWeapon, ...) and keeps each one
        // even when nothing current references it: Action42 indexes this
        // family directly (6E2390). Its retained Damage, Speed, Range,
        // Projectile, Warhead, Anim, OccupantAnim, OpenToppedAnim and
        // AttachedParticleSystem are retained here; AssaultAnim is retained by
        // this pass too, and Report lists bind below through the SOUNDMD catalog.
        let mut weapons: LookupMap<String, WeaponHandle> = LookupMap::default();
        let mut weapon_types: Vec<WeaponType> = Vec::new();
        for state in processed.weapon_states() {
            let Some(section) = ini.section(state.id) else {
                log::trace!("Weapon '{}' allocated but has no section", state.id);
                continue;
            };
            let mut weapon = WeaponType::from_ini_section(&section.name, section);
            weapon.speed = state.speed;
            weapon.range_leptons = state.range_leptons;
            weapon.projectile = state.projectile.map(str::to_owned);
            weapon.damage = state.damage;
            weapon.warhead = state.warhead.map(str::to_owned);
            weapon.anim = state.anim.to_vec();
            weapon.assault_anim = state.assault_anim.map(str::to_owned);
            weapon.occupant_anim = state.occupant_anim.map(str::to_owned);
            weapon.open_topped_anim = state.open_topped_anim.map(str::to_owned);
            weapon.attached_particle_system = state.attached_particle_system.map(str::to_owned);
            if crate::rules::ares_switch::ares_way() {
                weapon.ares_visuals.bolt_particle_system = state.ares_bolt_particle_system.clone();
                weapon.ares_ivan_bomb.warhead = state.ares_ivan_bomb_warhead.clone();
                weapon.ares_abductor.anim = state.ares_abductor_anim.map(str::to_owned);
                weapon.ares_cursors = state.ares_cursors;
            }
            // A repeated ID replaces the earlier weapon under its key. The
            // reserve is `insert`'s, so the keys iterate in the order they
            // did when the map held the weapons themselves.
            weapons.reserve(1);
            match weapons.entry(section.name.clone()) {
                std::collections::hash_map::Entry::Occupied(slot) => {
                    weapon_types[slot.get().0 as usize] = weapon;
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(WeaponHandle(weapon_types.len() as u32));
                    weapon_types.push(weapon);
                }
            }
        }
        // Step 3: Parse warhead sections. The projected [Warheads] registry is
        // the processor's whole Warhead family: the [Warheads] values and
        // every reference-driven allocation, weapons' warheads and the
        // [Radiation] RadSiteWarhead included.
        // Assign handles in projected registry order. Iterating a RandomState
        // HashSet here could give two loads of the same rules different handles.
        let warhead_ids = parse_registry(ini, "Warheads");
        let mut warheads: LookupMap<String, WarheadHandle> = LookupMap::default();
        let mut warhead_types: Vec<WarheadType> = Vec::new();
        for warhead_id in &warhead_ids {
            if let Some(section) = ini.section(warhead_id) {
                // Find-or-allocate by the section's canonical header name, so
                // two references differing only in case resolve to one type
                // (the original engine matches case-insensitively).
                let canonical = section.name.clone();
                warheads.entry(canonical.clone()).or_insert_with(|| {
                    warhead_types.push(WarheadType::from_ini_section(&canonical, section));
                    WarheadHandle(warhead_types.len() as u32 - 1)
                });
            } else {
                log::trace!("Warhead '{}' referenced but has no section", warhead_id);
            }
        }

        // Step 4: Collect projectile IDs referenced by weapons and parse them.
        let mut projectiles: LookupMap<String, ProjectileHandle> = LookupMap::default();
        let mut projectile_types: Vec<ProjectileType> = Vec::new();
        // Assign handles in first weapon-reference order. The set only drops
        // repeated references and never determines the iteration order.
        let mut projectile_ids = Vec::new();
        let mut seen_projectile_ids = LookupSet::default();
        for weapon in &weapon_types {
            if let Some(proj_id) = weapon.projectile.as_deref()
                && seen_projectile_ids.insert(proj_id)
            {
                projectile_ids.push(proj_id);
            }
        }
        for proj_id in projectile_ids {
            if let Some(section) = ini.section(proj_id) {
                // Find-or-allocate by canonical section name (see warheads above).
                // Stock rulesmd references [InvisibleLow] as both "InvisibleLow"
                // and "Invisiblelow"; this collapses them to one entry, as the
                // original engine does.
                let canonical = section.name.clone();
                projectiles.entry(canonical.clone()).or_insert_with(|| {
                    projectile_types
                        .push(ProjectileType::from_ini_section(&canonical, section, None));
                    ProjectileHandle(projectile_types.len() as u32 - 1)
                });
            } else {
                log::trace!("Projectile '{}' referenced but has no section", proj_id);
            }
        }

        // Step 5: Parse superweapon type registry.
        let ares_super_weapon_metadata: BTreeMap<_, _> = if crate::rules::ares_switch::ares_way() {
            processed
                .super_weapon_availability_states()
                .map(|(name, metadata)| (name.to_ascii_uppercase(), metadata))
                .collect()
        } else {
            BTreeMap::new()
        };
        let mut super_weapons: HashMap<String, SuperWeaponType> = HashMap::new();
        let sw_ids: Vec<String> = parse_registry(ini, "SuperWeaponTypes");
        let super_weapon_order: Vec<String> = sw_ids.clone();
        for sw_id in &sw_ids {
            if let Some(section) = ini.section(sw_id) {
                let sw = SuperWeaponType::from_ini_section_with_range(
                    sw_id,
                    section,
                    ares_super_weapon_metadata
                        .get(&sw_id.to_ascii_uppercase())
                        .map(|ext| ext.launch().range()),
                    general.psychic_reveal_radius,
                );
                super_weapons.insert(sw_id.clone(), sw);
            } else {
                log::trace!(
                    "SuperWeapon '{}' listed in [SuperWeaponTypes] but has no section",
                    sw_id
                );
            }
        }
        log::info!("SuperWeaponTypes: {} loaded", super_weapons.len());

        // Parse [CombatDamage] defaults (particle-system fallbacks).
        let combat_damage: CombatDamageDefaults = ini
            .section("CombatDamage")
            .map(CombatDamageDefaults::from_ini_section)
            .unwrap_or_default();

        // [CombatDamage] IonCannonWarhead, C4Warhead and CrushWarhead: the
        // ordered Process owner's, published by from_processed_rules.
        let bridge_warheads = crate::rules::bridge_warheads::BridgeWarheads::default();

        let mut mind_control = crate::rules::mind_control_rules::MindControlRules::from_ini(
            ini,
            processed.mind_control_attack_line_frames(),
        );
        let [capture, release, overload] = processed.mind_control_sounds();
        mind_control.mind_control_sound = capture.map(str::to_owned);
        mind_control.mind_cleared_sound = release.map(str::to_owned);
        mind_control.overload_sound = overload.map(str::to_owned);

        // [General] rocket blocks + [CombatDamage] missile warheads, retained
        // pass by pass by the ordered Process owner.
        let missile_spawn = processed.missile_spawn().clone();

        let c4_delay = ini
            .section_or_empty("CombatDamage")
            .read_double("C4Delay", 0.03);

        // Per-mission behaviour table from the [<MissionName>] sections.
        let mission_control = MissionControl::from_ini(ini);

        // Parse [TerrainTypes] registry → per-type sections (TIBTRE01, TREE01, etc.).
        let terrain_object_types: HashMap<String, TerrainObjectType> = processed
            .terrain_type_projections()
            .map(|terrain| (terrain.name.to_ascii_uppercase(), terrain))
            .collect();
        log::info!(
            "TerrainTypes: {} loaded ({} with SpawnsTiberium=yes)",
            terrain_object_types.len(),
            terrain_object_types
                .values()
                .filter(|t| t.spawns_tiberium)
                .count(),
        );

        // Preserve actual allocations and per-pass bindings, including
        // constructor-only particles reached through HoldsWhat/NextParticle.
        let particle_types = processed.particle_type_projections().collect::<Vec<_>>();
        let (particle_system_types, particle_system_types_by_name) =
            parse_particle_system_types(processed);
        let (voxel_anim_types, voxel_anim_types_by_name) = parse_voxel_anim_types(processed);

        log::info!(
            "RuleSet loaded: {} objects ({} inf, {} veh, {} air, {} bld), \
             {} weapons, {} warheads, {} projectiles, \
             {} particle types, {} particle system types",
            object_list.len(),
            infantry_ids.len(),
            vehicle_ids.len(),
            aircraft_ids.len(),
            building_ids.len(),
            weapons.len(),
            warheads.len(),
            projectiles.len(),
            particle_types.len(),
            particle_system_types.len()
        );

        // Lockstep invariant: `lookup_ci`'s case-insensitive scan is deterministic
        // only if no two stored type names are equal ignoring case. The original
        // engine's case-insensitive find-or-allocate merges case-duplicate names,
        // guaranteeing this for valid data; assert it in debug so a malformed INI
        // surfaces loudly instead of desyncing silently in lockstep. Compiled out
        // of release, so normal play pays nothing.
        #[cfg(debug_assertions)]
        {
            let check_unique_ci = |label: &str, keys: Vec<&String>| {
                let mut lowered: Vec<String> =
                    keys.iter().map(|k| k.to_ascii_lowercase()).collect();
                lowered.sort();
                for pair in lowered.windows(2) {
                    debug_assert_ne!(
                        pair[0], pair[1],
                        "{label}: type names collide ignoring case ({:?}) — breaks deterministic lookup",
                        pair[0]
                    );
                }
            };
            // Objects merge case-duplicates at insert (find-or-allocate), so the
            // uppercase-keyed index can't hold a case-collision by construction.
            check_unique_ci("weapons", weapons.keys().collect());
            check_unique_ci("warheads", warheads.keys().collect());
            check_unique_ci("projectiles", projectiles.keys().collect());
            check_unique_ci("super_weapons", super_weapons.keys().collect());
        }

        // Product extension: commander personalities ([AIPersonalities]). The
        // registry walk stays here so the stored-ID projection has one owner;
        // without the section the catalog is empty and nothing else changes.
        //
        // `Country=` is the only filter a personality is written with, at
        // `rules.ini`'s `Owner=` granularity: exact `[Countries]` keys, matched
        // against the house's own country name. Nothing here needs the country
        // or side tables, which is what lets a mod's own country take part
        // without registering anything.
        let personalities = crate::rules::personality::parse_personalities(
            ini,
            &parse_registry(ini, "AIPersonalities"),
        );

        let mut rules = RuleSet {
            object_list,
            object_index,
            serial: next_rule_set_serial(),
            sound_names: Default::default(),
            object_category_index,
            native_techno_type_order: Vec::new(),
            ares_techno_type_first: LookupMap::default(),
            type_array_indices,
            weapons,
            weapon_types,
            weapon_array: Vec::new(),
            warheads,
            warhead_types,
            projectiles,
            projectile_types,
            countries,
            country_ids: country_side_registry.country_ids,
            country_indices: country_side_registry.country_indices,
            country_name_indices: country_side_registry.country_name_indices,
            side_ids: country_side_registry.side_ids,
            personalities,
            commander_feedback: crate::rules::personality::parse_commander_feedback(ini),
            end_game_screens: crate::rules::end_game::parse_end_game_screens(
                ini,
                &parse_registry(ini, "EndGameScreens"),
            ),
            #[cfg(feature = "phobos")]
            side_gdi_positions,
            #[cfg(feature = "phobos")]
            sw_sidebar,
            #[cfg(feature = "phobos")]
            selection_flash,
            #[cfg(feature = "phobos")]
            fly_no_wobbles,
            #[cfg(feature = "phobos")]
            power_plant_enhancer,
            #[cfg(feature = "phobos")]
            phobos_airburst,
            #[cfg(feature = "phobos")]
            side_sw_sidebar_art,
            side_extensions: country_side_registry.side_extensions,
            side_indices: country_side_registry.side_indices,
            country_sides: country_side_registry.country_sides,
            color_schemes,
            enemy_team_palette,
            house_color_ramps,
            color_add,
            production,
            general,
            reveal_trigger_radius: ini
                .section_or_empty("General")
                .read_int("RevealTriggerRadius", 5),
            ai_base_spacing,
            // The ordered Rules processor publishes these lists
            // (`rules_type_references`).
            shipyard_types: Vec::new(),
            repair_bay_types: Vec::new(),
            build_const_types: Vec::new(),
            build_power_types: Vec::new(),
            build_refinery_types: Vec::new(),
            build_barracks_types: Vec::new(),
            build_tech_types: Vec::new(),
            build_weapons_types: Vec::new(),
            build_radar_types: Vec::new(),
            allied_base_defense_types: Vec::new(),
            soviet_base_defense_types: Vec::new(),
            third_base_defense_types: Vec::new(),
            concrete_wall_types: Vec::new(),
            ns_gate_types: Vec::new(),
            ew_gate_types: Vec::new(),
            harvester_unit_types: Vec::new(),
            ai_slave_miner_number,
            ai_extra_refineries,
            allied_base_defense_counts,
            soviet_base_defense_counts,
            third_base_defense_counts,
            tiberium_near_scan,
            tiberium_far_scan,
            ai_naval_yard_adjacency,
            initial_veteran,
            infantry_ids,
            vehicle_ids,
            aircraft_ids,
            building_ids,
            prerequisite_lists: Default::default(),
            ares_prerequisites: None,
            ares_armors: processed.ares_armors().cloned(),
            emp_sparkles: None,
            // Decided by `from_processed_rules` once its readers ran.
            ares: false,
            ares_kept_off: false,
            phobos: false,
            kratos: false,
            npatch: false,
            ra2: false,
            reads_ares_way: false,
            ares_global_controls: processed.ares_global_controls().clone(),
            ares_mouse_cursors: processed.ares_mouse_cursors().clone(),
            ares_super_weapon_cursors: processed
                .ares_super_weapon_cursor_states()
                .map(|(name, cursors)| (name.to_ascii_uppercase(), cursors))
                .collect(),
            ares_super_weapon_metadata,
            #[cfg(feature = "phobos")]
            phobos_super_weapon_metadata: BTreeMap::new(),
            parallel_ai_queues:
                crate::rules::phobos_parallel_ai_queues::ParallelAiQueueRules::from_ini(ini),
            ares_prerequisite_alternates: HashSet::new(),
            terrain_rules,
            tiberium_types,
            terrain_object_types,
            overlay_registry: std::sync::Arc::new(
                crate::rules::overlay_types::OverlayTypeRegistry::from_processed_rules(processed),
            ),
            bridge_rules,
            crate_rules,
            powerups,
            garrison_rules,
            allied_wall_transparency,
            elevation_model,
            radiation,
            radiation_site_warhead: None,
            #[cfg(feature = "phobos")]
            shield_types,
            #[cfg(feature = "phobos")]
            insignia_types,
            #[cfg(feature = "phobos")]
            phobos_draw_globals,
            #[cfg(feature = "phobos")]
            shield_globals,
            #[cfg(feature = "phobos")]
            crit_globals: crate::rules::phobos_crit::CritGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            invulnerability_globals:
                crate::rules::invulnerability_rules::InvulnerabilityGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            phobos_warhead_anim:
                crate::rules::phobos_warhead_anim::PhobosWarheadAnimGlobals::from_ini(ini),
            #[cfg(feature = "phobos")]
            digital_display_types,
            #[cfg(feature = "phobos")]
            digital_display_globals,
            #[cfg(feature = "phobos")]
            attach_effect_types,
            #[cfg(feature = "phobos")]
            attach_effect_globals,
            #[cfg(feature = "phobos")]
            placement_preview_globals,
            skirmish_color_mode,
            radar_event_config,
            super_weapons,
            super_weapon_order,
            combat_damage,
            bridge_warheads,
            mind_control,
            missile_spawn,
            c4_delay,
            particle_types,
            particle_system_types,
            particle_system_types_by_name,
            voxel_anim_types,
            voxel_anim_types_by_name,
            smudge_types: SmudgeTypeRegistry::from_rules_ini(ini),
            art_registry: crate::rules::art_data::ArtRegistry::empty(),
            anim_type_art_read_states: Vec::new(),
            anim_type_images: Vec::new(),
            anim_type_references: Vec::new(),
            anim_type_names: ini
                .section("Animations")
                .into_iter()
                .flat_map(|section| section.registry_ids())
                .map(|name| name.to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect(),
            effect_assets: crate::rules::effect_asset_catalog::EffectAssetCatalog::default(),
            voxel_assets: crate::rules::voxel_asset_catalog::VoxelAssetCatalog::default(),
            voxel_model_plan: processed.voxel_model_plan().clone(),
            buildup_assets: crate::rules::buildup_asset_catalog::BuildupAssetCatalog::default(),
            rubble_assets: crate::rules::building_rubble::RubbleAssetCatalog::default(),
            terrain_assets: crate::rules::terrain_asset_catalog::TerrainAssetCatalog::default(),
            alpha_images: crate::rules::alpha_image_assets::AlphaImageCatalog::default(),
            animation_sequences: BTreeMap::new(),
            animation_sequence_by_handle: Vec::new(),
            unit_shp_read_states: BTreeMap::new(),
            mission_control,
            // The boundary-sensitive RulesLayerStack hash.
            source_ini_hash: processed.content_hash(),
        };
        rules.rebuild_animation_sequences();
        Ok(rules)
    }
}

/// Parse a type registry section (e.g., [InfantryTypes]) into a list of IDs.
///
/// Registry sections use numbered keys: `0=E1`, `1=E2`, ...
/// Returns empty Vec if the section doesn't exist.
fn parse_registry(ini: &IniFile, section_name: &str) -> Vec<String> {
    match ini.section(section_name) {
        Some(section) => {
            // Every caller consumes NativeRulesRegistryState::finish's
            // projected stored IDs. The original registry ReadString32 and
            // constructor410800 truncation already ran there. Re-reading
            // them would trim valid list-created IDs such as " TWO" and lose
            // the Type pointer identity retained by Country veteran lists.
            let raw: Vec<String> = section
                .registry_ids()
                .into_iter()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
            // TypeClass identity is case-insensitive find-or-allocate. Preserve
            // declaration order while keeping the first spelling of an identity.
            let mut seen: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(raw.len());
            let before = raw.len();
            let deduped: Vec<String> = raw
                .into_iter()
                .filter(|id| seen.insert(id.to_ascii_uppercase()))
                .collect();
            let removed = before - deduped.len();
            if removed > 0 {
                log::info!(
                    "Registry [{}]: removed {} duplicate entries",
                    section_name,
                    removed,
                );
            }
            deduped
        }
        None => {
            log::warn!("Registry section [{}] not found in rules.ini", section_name);
            Vec::new()
        }
    }
}

struct ProjectedCountrySideRegistry {
    rules: HashMap<String, CountryRules>,
    country_ids: Vec<String>,
    country_indices: LookupMap<String, CountryIdx>,
    country_name_indices: LookupMap<String, CountryIdx>,
    side_ids: Vec<String>,
    side_extensions: Vec<crate::rules::ares_side_extension::AresSideExtension>,
    side_indices: HashMap<String, SideIdx>,
    country_sides: Vec<Option<SideIdx>>,
}

/// A gameplay projection of the one live native registry. ReadSides672440
/// and Country511850 already made every allocation and side assignment in
/// reached-pass order; reconstructing those decisions from merged text would
/// invent Countries, lose prior Side vectors and change allocation chronology.
fn project_country_side_registry<'a>(
    countries: impl Iterator<Item = CountryProjection<'a>>,
    sides: impl Iterator<Item = SideProjection<'a>>,
) -> ProjectedCountrySideRegistry {
    let sides = sides.collect::<Vec<_>>();
    let side_ids: Vec<String> = sides.iter().map(|(id, _)| (*id).to_owned()).collect();
    // Every allocated Side owns its Ares extension when the Rules were read
    // the Ares way, and none does when they were read the stock way.
    let side_extensions = sides
        .iter()
        .map(|(_, extension)| extension.cloned())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    let mut side_indices = HashMap::with_capacity(side_ids.len());
    for (index, id) in side_ids.iter().enumerate() {
        side_indices
            .entry(id.to_ascii_uppercase())
            .or_insert_with(|| {
                SideIdx(u8::try_from(index).expect("SIDE_LIMIT keeps the Side registry within u8"))
            });
    }
    let mut country_ids = Vec::new();
    let mut country_indices = LookupMap::default();
    let mut country_name_indices = LookupMap::default();
    let mut country_sides = Vec::new();
    let mut rules = HashMap::new();
    for (id, section, color, side, parent, veteran) in countries {
        let index = CountryIdx(
            u16::try_from(country_ids.len())
                .expect("COUNTRY_LIMIT keeps the Country registry within u16"),
        );
        country_ids.push(id.to_owned());
        country_indices
            .entry(id.to_ascii_uppercase())
            .or_insert(index);
        country_sides.push((side >= 0).then(|| {
            let index = usize::try_from(side).expect("nonnegative Side index");
            assert!(index < side_ids.len(), "retained Country Side exists");
            SideIdx(u8::try_from(side).expect("SIDE_LIMIT keeps the Side registry within u8"))
        }));
        let fields = rules.entry(id.to_owned()).or_insert_with(|| {
            let mut fields = CountryRules::from_ini_section(section);
            fields.color_scheme = color;
            fields.parent_country = parent.to_owned();
            fields.veteran_types = veteran.clone();
            if crate::rules::ares_switch::ares_way() {
                fields.presentation = veteran.presentation().clone();
                fields.paradrop = veteran.paradrop().clone();
                fields.power_plants = veteran.power_plants().to_vec();
            }
            fields
        });
        // FindIndexOfName's scan stops at the first Country whose alias or
        // identity matches, so an earlier Country keeps a name a later one
        // repeats. The alias is the one the scan reads for this identity
        // (`RuleSet::country_lookup_members`).
        for name in fields.name.as_deref().into_iter().chain([id]) {
            country_name_indices
                .entry(name.to_ascii_uppercase())
                .or_insert(index);
        }
    }
    ProjectedCountrySideRegistry {
        rules,
        country_ids,
        country_indices,
        country_name_indices,
        side_ids,
        side_extensions,
        side_indices,
        country_sides,
    }
}

/// `[VoxelAnims]` registry: the flying-debris types a death throws.
///
/// Registry order is the id order, exactly as for particles and particle
/// systems, because `DebrisTypes=` and `Spawns=` resolve by name and the ids
/// are hashed.
///
/// Native ReadVoxelAnims672920 allocates every listed identity, including
/// constructor-only entries; the retained processor supplies that exact order.
fn parse_voxel_anim_types(
    processed: &ProcessedRulesLayers,
) -> (Vec<VoxelAnimType>, HashMap<String, VoxelAnimTypeId>) {
    let types: Vec<_> = processed.voxel_anim_type_projections().collect();
    let mut by_name = HashMap::new();
    for (index, kind) in types.iter().enumerate() {
        by_name
            .entry(kind.name.to_ascii_uppercase())
            .or_insert(VoxelAnimTypeId(index as u32));
    }
    (types, by_name)
}

/// System allocations retain their own body and direct HoldsWhat index.
fn parse_particle_system_types(
    processed: &ProcessedRulesLayers,
) -> (
    Vec<ParticleSystemType>,
    HashMap<String, ParticleSystemTypeId>,
) {
    let types: Vec<_> = processed.particle_system_type_projections().collect();
    let mut by_name = HashMap::with_capacity(types.len());
    for (index, system) in types.iter().enumerate() {
        by_name
            .entry(system.name.to_ascii_uppercase())
            .or_insert(ParticleSystemTypeId(index as u32));
    }
    (types, by_name)
}

impl RuleSet {
    /// HouseType Multiplay eligibility from the same retained Country body
    /// as gameplay.4E3A00 also requires a supported index and nonempty
    /// localized UIName; the app catalog owns that presentation check.
    pub(crate) fn refresh_country_side_projection<'a>(
        &mut self,
        countries: impl Iterator<Item = CountryProjection<'a>>,
        sides: impl Iterator<Item = SideProjection<'a>>,
    ) {
        let projection = project_country_side_registry(countries, sides);
        self.countries = projection.rules;
        self.country_ids = projection.country_ids;
        self.country_indices = projection.country_indices;
        self.country_name_indices = projection.country_name_indices;
        self.side_ids = projection.side_ids;
        self.side_extensions = projection.side_extensions;
        self.side_indices = projection.side_indices;
        self.country_sides = projection.country_sides;
        self.project_house_masks();
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::foundation::Foundation;

    #[test]
    fn a_shared_name_keeps_building_foundation_in_its_own_category() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=Shared\n1=SHARED\n[BuildingTypes]\n0=Shared\n\
             [Shared]\nStrength=100\n",
        );
        let art = IniFile::from_str("[Shared]\nFoundation=2x2\n");
        let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("the rules read");
        let vehicle = rules
            .object_in_category(ObjectCategory::Vehicle, "Shared")
            .expect("the vehicle reads");
        let building = rules
            .object_in_category(ObjectCategory::Building, "Shared")
            .expect("the building reads");
        assert_eq!(rules.object_count(), 2);
        assert_eq!(vehicle.foundation, Foundation::default());
        assert_eq!(building.foundation, Foundation::from("2x2"));
    }

    #[test]
    fn warhead_handles_follow_the_registry_order() {
        const COUNT: usize = 12;
        let mut text = String::from("[Warheads]\n");
        for index in 0..COUNT {
            text.push_str(&format!("{index}=Head{}\n", COUNT - index));
        }
        for number in 1..=COUNT {
            text.push_str(&format!("[Head{number}]\nVerses=100%\n"));
        }
        let ini = IniFile::from_str(&text);
        let first = RuleSet::from_ini(&ini).expect("the rules read");
        let second = RuleSet::from_ini(&ini).expect("the rules read");
        for index in 0..COUNT {
            let name = format!("Head{}", COUNT - index);
            let first_handle = first.warhead_handle(&name).expect("the warhead reads");
            let second_handle = second.warhead_handle(&name).expect("the warhead reads");
            assert_eq!(first_handle, second_handle, "{name} across loads");
            assert_eq!(first_handle.0 as usize, index, "{name} in registry order");
        }
    }

    #[test]
    fn projectile_handles_follow_the_weapons_that_name_them() {
        const COUNT: usize = 12;
        let mut text = String::from("[VehicleTypes]\n");
        for index in 0..=COUNT {
            text.push_str(&format!("{index}=Unit{index}\n"));
        }
        for index in 0..COUNT {
            let number = COUNT - index;
            text.push_str(&format!(
                "[Unit{index}]\nPrimary=Weapon{index}\n\
                 [Weapon{index}]\nDamage=10\nProjectile=Proj{number}\n\
                 [Proj{number}]\nAA=yes\n",
            ));
        }
        text.push_str(&format!(
            "[Unit{COUNT}]\nPrimary=Repeat\n[Repeat]\nDamage=10\nProjectile=Proj{COUNT}\n",
        ));
        let ini = IniFile::from_str(&text);
        let first = RuleSet::from_ini(&ini).expect("the rules read");
        let second = RuleSet::from_ini(&ini).expect("the rules read");
        for index in 0..COUNT {
            let name = format!("Proj{}", COUNT - index);
            let first_handle = first
                .projectile_handle(&name)
                .expect("the projectile reads");
            let second_handle = second
                .projectile_handle(&name)
                .expect("the projectile reads");
            assert_eq!(first_handle, second_handle, "{name} across loads");
            assert_eq!(first_handle.0 as usize, index, "{name} in weapon order");
        }
        assert_eq!(first.projectile_types.len(), COUNT);
    }
}
