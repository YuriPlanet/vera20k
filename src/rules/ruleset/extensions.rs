//! Ares and Phobos globals a `RuleSet` exposes: Ares general settings and
//! cursors, the Phobos catalogs (insignia, shields, attach effects, digital
//! displays, placement preview) and the overlay registry.

use crate::rules::object_type::ObjectCategory;

use super::RuleSet;

impl RuleSet {
    /// Ares1003DB30/1003DB50. Both group labels and ordinary type IDs share
    /// one namespace; callers compare these keys case-insensitively.
    pub fn type_selection_key<'a>(&'a self, category: ObjectCategory, type_id: &'a str) -> &'a str {
        if self.ares
            && let Some(object) = self.object_in_category(category, type_id)
        {
            return object.type_selection_key();
        }
        type_id
    }

    /// CollectSelectedIDs732D47 -> Ares10041990/10041A3F gates the
    /// DeploysInto/UndeploysInto seeds. A stock game keeps retail behavior.
    pub fn type_select_uses_deploy(&self) -> bool {
        if self.ares {
            self.ares_global_controls.type_select_use_deploy()
        } else {
            true
        }
    }

    /// RulesExt `[General] ChronoInfantryCrush` (constructor true): whether
    /// warping infantry may land on a cell holding anything but infantry.
    pub(crate) fn ares_chrono_infantry_crush(&self) -> bool {
        self.ares_global_controls.chrono_infantry_crush()
    }

    /// RulesExt `[General]`/`[AudioVisual]` keys of the team, starting-unit,
    /// wrench, disk-laser and electric-death hooks.
    pub(crate) fn ares_general(&self) -> &crate::rules::native_processing::AresGeneralRules {
        self.ares_global_controls.general()
    }

    /// RulesExt `[General] EnemyWrench` (constructor true): a player not
    /// allied with a repairing building's owner sees its wrench.
    pub fn ares_enemy_wrench(&self) -> bool {
        self.ares_global_controls.general().enemy_wrench()
    }

    /// RulesExt `[General] EnemyInsignia` (constructor true): a player not
    /// allied with an object's owner sees its rank insignia.
    pub fn ares_enemy_insignia(&self) -> bool {
        self.ares_global_controls.general().enemy_insignia()
    }

    /// RulesExt `[CrateRules] RandomCrateMoney` (constructor 900).
    pub(crate) fn ares_random_crate_money(&self) -> i32 {
        self.ares_global_controls.random_crate_money()
    }

    /// RulesExt `[General] RepairStopOnInsufficientFunds` (constructor true).
    pub(crate) fn ares_repair_stop_on_insufficient_funds(&self) -> bool {
        self.ares_global_controls
            .repair_stop_on_insufficient_funds()
    }

    pub(crate) fn bounty_rules(&self) -> &crate::rules::ares_bounty::BountyGlobalRules {
        self.ares_global_controls.bounty()
    }

    /// RulesExt CloakHeight, dim factors and detection speak delays.
    pub fn ares_cloak_power(&self) -> &crate::rules::ares_cloak_power::CloakPowerGlobalRules {
        self.ares_global_controls.cloak_power()
    }

    /// RulesExt `[AudioVisual] VeteranFlashTimer` (`rules::ares_veterancy`).
    pub(crate) fn ares_veterancy(&self) -> &crate::rules::ares_veterancy::VeterancyGlobalRules {
        self.ares_global_controls.veterancy()
    }

    /// The Ares `[MouseCursors]` list the client draws cursors from
    /// ([`crate::rules::ares_mouse_cursor`]); presentation only.
    pub fn ares_mouse_cursors(&self) -> &crate::rules::ares_mouse_cursor::MouseCursors {
        &self.ares_mouse_cursors
    }

    /// The `[MouseCursors]` cursors `SWTypeExt::ExtData::WhatAction @
    /// 10033EB0` shows over a cell superweapon `id` may or may not fire at.
    /// It answers only for a type a NewSWType handles or whose `Action=` is
    /// `Custom`; the others keep the stock action cursor. VERA's handled types
    /// are its implemented handlers (`SuperWeaponKind::has_ares_handler`).
    pub fn ares_super_weapon_cursors(
        &self,
        id: &str,
    ) -> Option<crate::rules::ares_mouse_cursor::SuperWeaponCursors> {
        let sw = self.super_weapon(id)?;
        (sw.kind.has_ares_handler()
            || sw
                .action
                .as_deref()
                .is_some_and(|action| action.eq_ignore_ascii_case("Custom")))
        .then(|| {
            self.ares_super_weapon_cursors
                .get(&sw.id.to_ascii_uppercase())
                .copied()
                .unwrap_or_default()
        })
    }

    /// RulesExt BerserkROFMultiplier, AutoRepel and PlayerAutoRepel.
    pub(crate) fn ares_techno_combat(
        &self,
    ) -> &crate::rules::ares_techno_type::TechnoCombatGlobalRules {
        self.ares_global_controls.techno_combat()
    }

    /// RulesExt EngineerDamage and EngineerAlwaysCaptureTech.
    pub(crate) fn ares_building_globals(
        &self,
    ) -> &crate::rules::ares_building_type::BuildingGlobalRules {
        self.ares_global_controls.building()
    }

    /// RulesExt DamageAirConsiderBridges and IronCurtainFlash.
    pub(crate) fn ares_warhead_globals(&self) -> &crate::rules::ares_warhead::WarheadGlobalRules {
        self.ares_global_controls.warhead()
    }

    /// RulesExt TiberiumDamageEnabled, TiberiumHealEnabled, the explosive
    /// warhead and anim, and OverlayExplodeThreshold.
    pub(crate) fn ares_tiberium_globals(
        &self,
    ) -> &crate::rules::ares_tiberium::TiberiumGlobalRules {
        self.ares_global_controls.tiberium()
    }

    /// RulesExt's firestorm anims, warhead and damage coefficient
    /// (`+0xF0..+0x108`, `sim::ares_firestorm`).
    pub(crate) fn ares_firestorm_globals(
        &self,
    ) -> &crate::rules::ares_firestorm::FirestormGlobalRules {
        self.ares_global_controls.firestorm()
    }

    /// Ares RulesExt+70, used by SW_HunterSeeker::Activate100728EB.
    pub(crate) fn hunter_seeker_buildings(&self) -> &[String] {
        self.ares_global_controls.hunter_seeker_buildings()
    }

    /// Ares RulesExt's DropPod globals with the stock locomotor keys, used by
    /// SW_DropPod::Activate (10070E40) and the DropPod locomotor.
    pub(crate) fn ares_drop_pod(&self) -> &crate::rules::superweapon_type::AresDropPodGlobalRules {
        self.ares_global_controls.drop_pod()
    }

    /// `TreeFire=` and `TreeFlammability=`, read by the Ares terrain fire.
    pub(crate) fn forest_fire(&self) -> &crate::rules::terrain_object_type::ForestFireRules {
        self.ares_global_controls.forest_fire()
    }

    /// RulesExt+14A indexed by native House difficulty0/1/2 (hardest first).
    /// Only the Ares CanBuild owner consumes this retained policy.
    pub fn ares_ai_bypasses_build_limit(&self, difficulty_index: usize) -> bool {
        if self.ares {
            self.ares_global_controls
                .ai_bypasses_build_limit(difficulty_index)
        } else {
            false
        }
    }

    /// RulesExt+160/+164 retained nullable named mission. A stock game
    /// neither reads this extension key nor supplies an AI EMP recovery producer.
    pub(crate) fn ares_emp_ai_recover_mission(
        &self,
    ) -> Option<crate::rules::mission_data::MissionType> {
        if self.ares {
            self.ares_global_controls.emp_ai_recover_mission()
        } else {
            None
        }
    }

    pub(crate) fn bind_emp_sparkles(&mut self, name: Option<String>) {
        self.emp_sparkles = name;
    }
    pub(crate) fn emp_sparkles(&self) -> Option<&str> {
        self.emp_sparkles.as_deref()
    }

    pub(crate) fn reveal_trigger_radius(&self) -> i32 {
        self.reveal_trigger_radius
    }

    /// The immutable Overlay projection from this same processed Rules owner.
    pub fn overlay_registry(&self) -> &crate::rules::overlay_types::OverlayTypeRegistry {
        &self.overlay_registry
    }

    /// Keep this projection across authored constructors that bind other Rules
    /// resources, or after the match gives its overlay view back to the shell.
    pub fn retain_overlay_registry(
        &self,
    ) -> std::sync::Arc<crate::rules::overlay_types::OverlayTypeRegistry> {
        std::sync::Arc::clone(&self.overlay_registry)
    }
}

impl RuleSet {
    /// Phobos `[InsigniaTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn insignia_types(&self) -> &crate::rules::insignia_type::InsigniaTypeCatalog {
        &self.insignia_types
    }

    /// Phobos `[General]`/`[AudioVisual]` insignia settings.
    #[cfg(feature = "phobos")]
    pub fn phobos_draw_globals(&self) -> &crate::rules::insignia_type::PhobosDrawGlobals {
        &self.phobos_draw_globals
    }

    /// Phobos `[ShieldTypes]` entry by name (case-insensitive, first spelling
    /// retained). No entry in a build without the capability.
    #[cfg(feature = "phobos")]
    pub fn shield_type(&self, name: &str) -> Option<&crate::rules::phobos_shields::ShieldType> {
        self.shield_types.entry(name)
    }

    /// Phobos shield globals (`[CombatDamage]`/`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn shield_globals(&self) -> &crate::rules::phobos_shields::ShieldGlobals {
        &self.shield_globals
    }

    /// Phobos Iron Curtain / Force Shield organics, tint and deploy settings.
    #[cfg(feature = "phobos")]
    pub fn invulnerability_globals(
        &self,
    ) -> &crate::rules::invulnerability_rules::InvulnerabilityGlobals {
        &self.invulnerability_globals
    }

    /// Phobos crit defaults (`[CombatDamage]`/`[General]`).
    #[cfg(feature = "phobos")]
    pub fn crit_globals(&self) -> &crate::rules::phobos_crit::CritGlobals {
        &self.crit_globals
    }

    /// Phobos `[DigitalDisplayTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn digital_display_types(
        &self,
    ) -> &crate::rules::digital_display::DigitalDisplayTypeCatalog {
        &self.digital_display_types
    }

    /// Phobos `[AttachEffectTypes]` catalog.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_types(&self) -> &crate::rules::attach_effect::AttachEffectTypeCatalog {
        &self.attach_effect_types
    }

    /// Phobos AttachEffect `[General]` switches.
    #[cfg(feature = "phobos")]
    pub fn attach_effect_globals(&self) -> &crate::rules::attach_effect::AttachEffectGlobals {
        &self.attach_effect_globals
    }

    /// Phobos `AttachEffect.AttachTypes` authored on a country/house type.
    #[cfg(feature = "phobos")]
    pub fn country_attach_effect_types(&self, id: &str) -> &[String] {
        self.country_rules(id)
            .map_or(&[], |country| country.attach_effect_attach_types.as_slice())
    }

    /// Phobos DigitalDisplay globals (`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn digital_display_globals(&self) -> &crate::rules::digital_display::DigitalDisplayGlobals {
        &self.digital_display_globals
    }

    /// `TechnoExt::ProcessDigitalDisplays`'s list resolution: an explicit
    /// `DigitalDisplayTypes=` wins; unknown names are skipped; otherwise the
    /// category's `[AudioVisual]` default list applies. Yields catalog
    /// indices in list order without allocating.
    #[cfg(feature = "phobos")]
    pub fn digital_display_indices<'a>(
        &'a self,
        object: &'a crate::rules::object_type::ObjectType,
    ) -> impl Iterator<Item = usize> + 'a {
        let explicit = object.digital_display_types();
        let names: &[String] = if !explicit.is_empty() {
            explicit
        } else {
            match object.category {
                crate::rules::object_type::ObjectCategory::Building => {
                    &self.digital_display_globals.buildings_default
                }
                crate::rules::object_type::ObjectCategory::Infantry => {
                    &self.digital_display_globals.infantry_default
                }
                crate::rules::object_type::ObjectCategory::Vehicle => {
                    &self.digital_display_globals.vehicles_default
                }
                crate::rules::object_type::ObjectCategory::Aircraft => {
                    &self.digital_display_globals.aircraft_default
                }
            }
        };
        names
            .iter()
            .filter_map(|name| self.digital_display_types.index_of(name))
    }

    /// Phobos placement preview globals (`[AudioVisual]`).
    #[cfg(feature = "phobos")]
    pub fn placement_preview_globals(
        &self,
    ) -> &crate::rules::phobos_placement_preview::PlacementPreviewGlobals {
        &self.placement_preview_globals
    }

    /// The player-colour conversion this rules load selects and the colours
    /// its lobby lists.
    ///
    /// A build without the `phobos` feature reads no
    /// `[General] SkirmishUnlimitedColors=`, so callers need no `cfg`.
    pub fn skirmish_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        self.skirmish_color_mode
    }
}
