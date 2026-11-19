//! Rule choices admitted for scenario construction and live simulation.

/// Selects the native rule path used when building a scenario.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BootstrapRuleProfile {
    #[default]
    Campaign,
    Multiplayer,
}

impl BootstrapRuleProfile {
    pub const fn uses_multiplayer_rules(self) -> bool {
        matches!(self, Self::Multiplayer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AllowWinTagsPolicy {
    RequireClear,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ComputerParanoidPolicy {
    Disabled,
    React,
}

/// Admission of map-authored crate overlays at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AuthoredCratePolicy {
    Keep,
    Skip,
}

/// Whether the AI may replace unavailable or invalidated base-plan nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AiBasePolicy {
    RetainAuthoredNodes,
    ReplaceUnavailableNodes,
}

/// How factory selection interprets `DoubleOwned=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DoubleOwnedPolicy {
    OwnerMask,
    AllCountries,
}

/// The set of live TeamClass instances that count toward a TeamType's `Max=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum TeamLimitScope {
    AllHouses,
    PerHouse,
}

/// Independently selectable rules. Stock admission and legacy decoding choose
/// a preset from the mode and mode class. Live sessions retain the admitted
/// value. Offline Skirmish (GameMode 5) uses multiplayer rules with local
/// execution. Binary choices share one flag word; named selectors describe
/// the behavior at subsystem boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RulePolicy {
    flags: u64,
}

const _: () = assert!(std::mem::size_of::<RulePolicy>() <= 16);

impl Default for RulePolicy {
    fn default() -> Self {
        Self::stock_campaign()
    }
}

impl RulePolicy {
    const MULTIPLAYER_BOOTSTRAP: u64 = 1 << 0;
    const IGNORE_ALLOW_WIN_TAGS: u64 = 1 << 1;
    const PARANOID_REACTIONS: u64 = 1 << 2;
    const SKIP_AUTHORED_CRATES: u64 = 1 << 3;
    const REPLACE_UNAVAILABLE_BASE_NODES: u64 = 1 << 4;
    const DOUBLE_OWNED_ALL_COUNTRIES: u64 = 1 << 5;
    const MOD_MAP_ADDITIONS: u64 = 1 << 6;
    const TEAM_LIMIT_PER_HOUSE: u64 = 1 << 7;
    const DISSOLVE_UNFILLED_TEAMS: u64 = 1 << 8;
    const SHARED_ALLIED_VICTORY: u64 = 1 << 9;
    const SKIP_NETWORK_SCORE_DIALOG: u64 = 1 << 10;
    const PARANOID_IGNORES_ALLIES_ALLOWED: u64 = 1 << 11;
    const NETWORK_DIPLOMACY_BRIEFING: u64 = 1 << 12;
    const STARTING_HOUSE_ROLE_ALLIANCES: u64 = 1 << 13;
    const ARES_PSYCHIC_REVEAL_EXCLUSIONS: u64 = 1 << 14;
    const AI_MCV_DEPLOY_HUNT: u64 = 1 << 15;
    const REQUIRES_AI_BASE_RESERVATION_PROXIMITY: u64 = 1 << 16;
    const USES_AI_PRODUCTION_MODES: u64 = 1 << 17;
    const IGNORES_AUTHORED_AI_SELLABLE: u64 = 1 << 18;
    const ALLOWS_AI_SALE_AFTER_CAPTURE: u64 = 1 << 19;
    const ALLOWS_CONSTRUCTION_YARD_REDEPLOY: u64 = 1 << 20;
    const ALLOWS_AI_VIRTUAL_PURIFIERS: u64 = 1 << 21;
    const AI_SEARCHES_FOR_ENEMY: u64 = 1 << 22;
    const AI_IGNORES_SUPERWEAPON_IQ: u64 = 1 << 23;
    const AI_SELLS_AND_HUNTS_WITHOUT_FACTORY: u64 = 1 << 24;
    const AI_MANAGES_STRATEGY_BUILD_QUEUE: u64 = 1 << 25;
    const ARES_SKIPS_PASSIVE_HOUSE_ANGER: u64 = 1 << 26;
    const AI_TRIGGERS_REQUIRE_MULTIPLAYER_FLAG: u64 = 1 << 27;
    const AI_TRIGGER_USES_HOUSE_DIFFICULTY: u64 = 1 << 28;
    const TEAM_ADMITS_RECRUITS_WITHOUT_FACTORY: u64 = 1 << 29;
    const TARGETING_REQUIRES_PLAYER_DISCOVERY: u64 = 1 << 30;
    const TARGETING_RESPECTS_PASSIVE_HOUSES: u64 = 1 << 31;
    const FORCES_LISTED_AI_TRIGGER_ENABLES: u64 = 1 << 32;
    const USES_DEFAULT_TASK_FORCE_MEMBER_TECH_LEVEL: u64 = 1 << 33;
    const REJECTS_DEFEATED_PLAYER_AT_REINFORCEMENTS: u64 = 1 << 34;
    const AI_BUILDINGS_REPAIRABLE_ON_UNLIMBO: u64 = 1 << 35;
    const AI_VEHICLES_AVOID_CRATES: u64 = 1 << 36;
    const ARES_AI_STORES_TIBERIUM: u64 = 1 << 37;
    const ARES_RETURNS_DEFEATED_STRUCTURES: u64 = 1 << 38;
    const AUTOMATIC_HOUSE_DEFEAT: u64 = 1 << 39;
    const OUTCOMES_FOLLOW_PLAYER_CONTROL: u64 = 1 << 40;
    const ALLOWS_IDLE_OBJECT_SLEEP: u64 = 1 << 41;
    const FORCE_TIBERIUM_FLAGS: u64 = 1 << 42;
    const HONORS_AUTHORED_INERT: u64 = 1 << 43;
    const USES_SESSION_HARVESTER_TRUCE: u64 = 1 << 44;
    const ALLOWS_OWNERLESS_TERRAIN_LOGIC: u64 = 1 << 45;
    const COUNTS_PLAYER_CONTROL_AS_HUMAN: u64 = 1 << 46;
    const BUILDING_SERVICES_USE_DISCOVERY: u64 = 1 << 47;
    const USES_COUNTRY_DIFFICULTY_BIASES: u64 = 1 << 48;
    const ARES_APPLIES_SUPERWEAPON_SHELL_SETTING: u64 = 1 << 49;
    const SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS: u64 = 1 << 50;
    const RECALCULATES_AI_BASE_AFTER_MCV_DEPLOY: u64 = 1 << 51;
    const CRATES_USE_RUNTIME_SLOTS: u64 = 1 << 52;
    const CRATES_ALLOW_PASSIVE_COLLECTORS: u64 = 1 << 53;
    const CRATES_USE_AUTHORED_DEFAULT_PAYLOAD: u64 = 1 << 54;
    const CRATES_VALIDATE_PICKUP_PAYLOAD: u64 = 1 << 55;
    const CRATES_PAY_LOCAL_PLAYER: u64 = 1 << 56;
    const PASSIVE_HOUSES_CAST_DIRECT_SIGHT: u64 = 1 << 57;
    const ARMED_VICTIMS_DEFEND_WITHOUT_BASE_RESPONSE: u64 = 1 << 58;
    const BASE_DEFENSE_REQUIRES_RECRUITABLE_MISSION: u64 = 1 << 59;
    const FIRE_DISCLOSURE_USES_LOCAL_DISCOVERY: u64 = 1 << 60;
    const HUNTER_SEEKER_LIMITS_AI_TO_DESIGNATED_ENEMY: u64 = 1 << 61;
    const HUNTER_SEEKER_HUMANS_PREFER_NONPASSIVE_TARGETS: u64 = 1 << 62;

    pub const fn stock_campaign() -> Self {
        Self { flags: 0 }
            .with_team_admits_recruits_without_factory(true)
            .with_targeting_requires_player_discovery(true)
            .with_ai_vehicles_avoid_crates(true)
            .with_ares_ai_stores_tiberium(true)
            .with_outcomes_follow_player_control(true)
            .with_honors_authored_inert(true)
            .with_allows_ownerless_terrain_logic(true)
            .with_counts_player_control_as_human(true)
            .with_building_services_use_discovery(true)
            .with_crates_allow_passive_collectors(true)
            .with_crates_use_authored_default_payload(true)
            .with_crates_pay_local_player(true)
            .with_passive_houses_cast_direct_sight(true)
            .with_armed_victims_defend_without_base_response(true)
            .with_base_defense_requires_recruitable_mission(true)
            .with_fire_disclosure_uses_local_discovery(true)
    }

    pub const fn stock_multiplayer() -> Self {
        Self::stock_campaign()
            .with_bootstrap(BootstrapRuleProfile::Multiplayer)
            .with_allow_win_tags(AllowWinTagsPolicy::Ignore)
            .with_computer_paranoid(ComputerParanoidPolicy::React)
            .with_crates(AuthoredCratePolicy::Skip)
            .with_ai_base(AiBasePolicy::ReplaceUnavailableNodes)
            .with_ai_mcv_deploy_hunt(true)
            .with_double_owned(DoubleOwnedPolicy::AllCountries)
            .with_mod_map_additions(true)
            .with_team_creation_limit_scope(TeamLimitScope::PerHouse)
            .with_dissolve_unfilled_teams(true)
            .with_ares_psychic_reveal_exclusions(true)
            .with_requires_ai_base_reservation_proximity(true)
            .with_uses_ai_production_modes(true)
            .with_ignores_authored_ai_sellable(true)
            .with_allows_ai_sale_after_capture(true)
            .with_allows_construction_yard_redeploy(true)
            .with_allows_ai_virtual_purifiers(true)
            .with_ai_searches_for_enemy(true)
            .with_ai_ignores_superweapon_iq(true)
            .with_ai_sells_and_hunts_without_factory(true)
            .with_ai_manages_strategy_build_queue(true)
            .with_ares_skips_passive_house_anger(true)
            .with_ai_triggers_require_multiplayer_flag(true)
            .with_ai_trigger_uses_house_difficulty(true)
            .with_team_admits_recruits_without_factory(false)
            .with_targeting_requires_player_discovery(false)
            .with_targeting_respects_passive_houses(true)
            .with_forces_listed_ai_trigger_enables(true)
            .with_uses_default_task_force_member_tech_level(true)
            .with_rejects_defeated_player_at_reinforcements(true)
            .with_ai_buildings_repairable_on_unlimbo(true)
            .with_ai_vehicles_avoid_crates(false)
            .with_ares_ai_stores_tiberium(false)
            .with_ares_returns_defeated_structures(true)
            .with_automatic_house_defeat(true)
            .with_outcomes_follow_player_control(false)
            .with_allows_idle_object_sleep(true)
            .with_force_tiberium_flags(true)
            .with_honors_authored_inert(false)
            .with_uses_session_harvester_truce(true)
            .with_counts_player_control_as_human(false)
            .with_building_services_use_discovery(false)
            .with_uses_country_difficulty_biases(true)
            .with_ares_applies_superweapon_shell_setting(true)
            .with_skips_local_player_authored_buildings(true)
            .with_recalculates_ai_base_after_mcv_deploy(true)
            .with_crates_use_runtime_slots(true)
            .with_crates_allow_passive_collectors(false)
            .with_crates_use_authored_default_payload(false)
            .with_crates_validate_pickup_payload(true)
            .with_crates_pay_local_player(false)
            .with_passive_houses_cast_direct_sight(false)
            .with_armed_victims_defend_without_base_response(false)
            .with_base_defense_requires_recruitable_mission(false)
            .with_fire_disclosure_uses_local_discovery(false)
            .with_hunter_seeker_limits_ai_to_designated_enemy(true)
            .with_hunter_seeker_humans_prefer_nonpassive_targets(true)
    }

    const fn has_flag(self, flag: u64) -> bool {
        self.flags & flag != 0
    }

    const fn with_flag(mut self, flag: u64, enabled: bool) -> Self {
        if enabled {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
        self
    }

    /// HouseClass::IsCenterOfBase (0x0050B760) uses base-node proximity.
    pub const fn requires_ai_base_reservation_proximity(self) -> bool {
        self.has_flag(Self::REQUIRES_AI_BASE_RESERVATION_PROXIMITY)
    }

    pub const fn with_requires_ai_base_reservation_proximity(self, enabled: bool) -> Self {
        self.with_flag(Self::REQUIRES_AI_BASE_RESERVATION_PROXIMITY, enabled)
    }

    /// Use the production economy paths at 0x00509700 and 0x004F9099.
    pub const fn uses_ai_production_modes(self) -> bool {
        self.has_flag(Self::USES_AI_PRODUCTION_MODES)
    }

    pub const fn with_uses_ai_production_modes(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_AI_PRODUCTION_MODES, enabled)
    }

    /// Allow a low-credit AI House to sell beyond the map's Sellable list (0x00450781).
    pub const fn ignores_authored_ai_sellable(self) -> bool {
        self.has_flag(Self::IGNORES_AUTHORED_AI_SELLABLE)
    }

    pub const fn with_ignores_authored_ai_sellable(self, enabled: bool) -> Self {
        self.with_flag(Self::IGNORES_AUTHORED_AI_SELLABLE, enabled)
    }

    /// Allow the AI to sell certain newly captured buildings (0x004492D7).
    pub const fn allows_ai_sale_after_capture(self) -> bool {
        self.has_flag(Self::ALLOWS_AI_SALE_AFTER_CAPTURE)
    }

    pub const fn with_allows_ai_sale_after_capture(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_AI_SALE_AFTER_CAPTURE, enabled)
    }

    /// Admit eligible player yard pack-up orders (0x0044A8DF, 0x0044F602, 0x0044375E).
    pub const fn allows_construction_yard_redeploy(self) -> bool {
        self.has_flag(Self::ALLOWS_CONSTRUCTION_YARD_REDEPLOY)
    }

    pub const fn with_allows_construction_yard_redeploy(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_CONSTRUCTION_YARD_REDEPLOY, enabled)
    }

    /// Let computer Houses calculate virtual ore purifiers (0x0073E3D5).
    pub const fn allows_ai_virtual_purifiers(self) -> bool {
        self.has_flag(Self::ALLOWS_AI_VIRTUAL_PURIFIERS)
    }

    pub const fn with_allows_ai_virtual_purifiers(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_AI_VIRTUAL_PURIFIERS, enabled)
    }

    /// Initialize and service the AI enemy search timer (0x006869EA, 0x004FD538).
    pub const fn ai_searches_for_enemy(self) -> bool {
        self.has_flag(Self::AI_SEARCHES_FOR_ENEMY)
    }

    pub const fn with_ai_searches_for_enemy(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_SEARCHES_FOR_ENEMY, enabled)
    }

    /// Permit AI superweapon use before meeting its IQ threshold (0x004FD77C).
    pub const fn ai_ignores_superweapon_iq(self) -> bool {
        self.has_flag(Self::AI_IGNORES_SUPERWEAPON_IQ)
    }

    pub const fn with_ai_ignores_superweapon_iq(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_IGNORES_SUPERWEAPON_IQ, enabled)
    }

    /// Let the AI give up its factory-less base (0x004FD848).
    pub const fn ai_sells_and_hunts_without_factory(self) -> bool {
        self.has_flag(Self::AI_SELLS_AND_HUNTS_WITHOUT_FACTORY)
    }

    pub const fn with_ai_sells_and_hunts_without_factory(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_SELLS_AND_HUNTS_WITHOUT_FACTORY, enabled)
    }

    /// Let the AI strategy loop ask for replacement factories (0x004FD848).
    pub const fn ai_manages_strategy_build_queue(self) -> bool {
        self.has_flag(Self::AI_MANAGES_STRATEGY_BUILD_QUEUE)
    }

    pub const fn with_ai_manages_strategy_build_queue(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_MANAGES_STRATEGY_BUILD_QUEUE, enabled)
    }

    /// Ares anger recalculation skips passive Houses (HouseClass at 0x00504796).
    pub const fn ares_skips_passive_house_anger(self) -> bool {
        self.has_flag(Self::ARES_SKIPS_PASSIVE_HOUSE_ANGER)
    }

    pub const fn with_ares_skips_passive_house_anger(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_SKIPS_PASSIVE_HOUSE_ANGER, enabled)
    }

    /// Filter authored AI triggers by their multiplayer flag (0x0041E7D7).
    pub const fn ai_triggers_require_multiplayer_flag(self) -> bool {
        self.has_flag(Self::AI_TRIGGERS_REQUIRE_MULTIPLAYER_FLAG)
    }

    pub const fn with_ai_triggers_require_multiplayer_flag(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_TRIGGERS_REQUIRE_MULTIPLAYER_FLAG, enabled)
    }

    /// Match AI trigger difficulty against its owning House (0x0041E7D7).
    pub const fn ai_trigger_uses_house_difficulty(self) -> bool {
        self.has_flag(Self::AI_TRIGGER_USES_HOUSE_DIFFICULTY)
    }

    pub const fn with_ai_trigger_uses_house_difficulty(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_TRIGGER_USES_HOUSE_DIFFICULTY, enabled)
    }

    /// Allow TeamClass to select a fallback recruit without a factory (0x0050965E).
    pub const fn team_admits_recruits_without_factory(self) -> bool {
        self.has_flag(Self::TEAM_ADMITS_RECRUITS_WITHOUT_FACTORY)
    }

    pub const fn with_team_admits_recruits_without_factory(self, enabled: bool) -> Self {
        self.with_flag(Self::TEAM_ADMITS_RECRUITS_WITHOUT_FACTORY, enabled)
    }

    /// Use local discovery in greatest-threat selection where the topology permits it (0x006F81BE).
    pub const fn targeting_requires_player_discovery(self) -> bool {
        self.has_flag(Self::TARGETING_REQUIRES_PLAYER_DISCOVERY)
    }

    pub const fn with_targeting_requires_player_discovery(self, enabled: bool) -> Self {
        self.with_flag(Self::TARGETING_REQUIRES_PLAYER_DISCOVERY, enabled)
    }

    /// Target selection applies the passive-House tests (0x006F826E).
    pub const fn targeting_respects_passive_houses(self) -> bool {
        self.has_flag(Self::TARGETING_RESPECTS_PASSIVE_HOUSES)
    }

    pub const fn with_targeting_respects_passive_houses(self, enabled: bool) -> Self {
        self.with_flag(Self::TARGETING_RESPECTS_PASSIVE_HOUSES, enabled)
    }

    /// Force listed AITriggerTypesEnable entries on at registry load (0x0041F451).
    pub const fn forces_listed_ai_trigger_enables(self) -> bool {
        self.has_flag(Self::FORCES_LISTED_AI_TRIGGER_ENABLES)
    }

    pub const fn with_forces_listed_ai_trigger_enables(self, enabled: bool) -> Self {
        self.with_flag(Self::FORCES_LISTED_AI_TRIGGER_ENABLES, enabled)
    }

    /// Include the default member tech level in the ordered TaskForce fold (0x006E8780).
    pub const fn uses_default_task_force_member_tech_level(self) -> bool {
        self.has_flag(Self::USES_DEFAULT_TASK_FORCE_MEMBER_TECH_LEVEL)
    }

    pub const fn with_uses_default_task_force_member_tech_level(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_DEFAULT_TASK_FORCE_MEMBER_TECH_LEVEL, enabled)
    }

    /// Ares validates defeated PlayerAt Houses for reinforcements (0x0065D8FB).
    pub const fn rejects_defeated_player_at_reinforcements(self) -> bool {
        self.has_flag(Self::REJECTS_DEFEATED_PLAYER_AT_REINFORCEMENTS)
    }

    pub const fn with_rejects_defeated_player_at_reinforcements(self, enabled: bool) -> Self {
        self.with_flag(Self::REJECTS_DEFEATED_PLAYER_AT_REINFORCEMENTS, enabled)
    }

    /// Grant AI repair on placement (0x00440B4F).
    pub const fn ai_buildings_repairable_on_unlimbo(self) -> bool {
        self.has_flag(Self::AI_BUILDINGS_REPAIRABLE_ON_UNLIMBO)
    }

    pub const fn with_ai_buildings_repairable_on_unlimbo(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_BUILDINGS_REPAIRABLE_ON_UNLIMBO, enabled)
    }

    /// Make AI vehicles avoid crate cells (0x0073F405).
    pub const fn ai_vehicles_avoid_crates(self) -> bool {
        self.has_flag(Self::AI_VEHICLES_AVOID_CRATES)
    }

    pub const fn with_ai_vehicles_avoid_crates(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_VEHICLES_AVOID_CRATES, enabled)
    }

    /// Ares may store AI ore instead of paying immediate credits (0x10024290).
    pub const fn ares_ai_stores_tiberium(self) -> bool {
        self.has_flag(Self::ARES_AI_STORES_TIBERIUM)
    }

    pub const fn with_ares_ai_stores_tiberium(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_AI_STORES_TIBERIUM, enabled)
    }

    /// Ares may return defeated structures to their original owners (0x10024150).
    pub const fn ares_returns_defeated_structures(self) -> bool {
        self.has_flag(Self::ARES_RETURNS_DEFEATED_STRUCTURES)
    }

    pub const fn with_ares_returns_defeated_structures(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_RETURNS_DEFEATED_STRUCTURES, enabled)
    }

    /// Run automatic defeat checks each House turn (0x004F8E86).
    pub const fn automatic_house_defeat(self) -> bool {
        self.has_flag(Self::AUTOMATIC_HOUSE_DEFEAT)
    }

    pub const fn with_automatic_house_defeat(self, enabled: bool) -> Self {
        self.with_flag(Self::AUTOMATIC_HOUSE_DEFEAT, enabled)
    }

    /// Route the authored scenario outcome through its controlled Houses.
    pub const fn outcomes_follow_player_control(self) -> bool {
        self.has_flag(Self::OUTCOMES_FOLLOW_PLAYER_CONTROL)
    }

    pub const fn with_outcomes_follow_player_control(self, enabled: bool) -> Self {
        self.with_flag(Self::OUTCOMES_FOLLOW_PLAYER_CONTROL, enabled)
    }

    /// Use VERA's idle-object sleep under its trigger and repair guards.
    pub const fn allows_idle_object_sleep(self) -> bool {
        self.has_flag(Self::ALLOWS_IDLE_OBJECT_SLEEP)
    }

    pub const fn with_allows_idle_object_sleep(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_IDLE_OBJECT_SLEEP, enabled)
    }

    /// PregameSetup forces the paired ore growth and spread flags (0x005E74CD).
    pub const fn force_tiberium_flags(self) -> bool {
        self.has_flag(Self::FORCE_TIBERIUM_FLAGS)
    }

    pub const fn with_force_tiberium_flags(self, enabled: bool) -> Self {
        self.with_flag(Self::FORCE_TIBERIUM_FLAGS, enabled)
    }

    /// Apply the map's Inert setting at startup (0x006B8EB1).
    pub const fn honors_authored_inert(self) -> bool {
        self.has_flag(Self::HONORS_AUTHORED_INERT)
    }

    pub const fn with_honors_authored_inert(self, enabled: bool) -> Self {
        self.with_flag(Self::HONORS_AUTHORED_INERT, enabled)
    }

    /// Take HarvesterImmune from the accepted room option (0x005E7495).
    pub const fn uses_session_harvester_truce(self) -> bool {
        self.has_flag(Self::USES_SESSION_HARVESTER_TRUCE)
    }

    pub const fn with_uses_session_harvester_truce(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_SESSION_HARVESTER_TRUCE, enabled)
    }

    /// Admit ownerless terrain to logic without the identity test (0x005F501B).
    pub const fn allows_ownerless_terrain_logic(self) -> bool {
        self.has_flag(Self::ALLOWS_OWNERLESS_TERRAIN_LOGIC)
    }

    pub const fn with_allows_ownerless_terrain_logic(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_OWNERLESS_TERRAIN_LOGIC, enabled)
    }

    /// HouseClass::IsControlledByHuman also honors PlayerControl (0x0050B730).
    pub const fn counts_player_control_as_human(self) -> bool {
        self.has_flag(Self::COUNTS_PLAYER_CONTROL_AS_HUMAN)
    }

    pub const fn with_counts_player_control_as_human(self, enabled: bool) -> Self {
        self.with_flag(Self::COUNTS_PLAYER_CONTROL_AS_HUMAN, enabled)
    }

    /// Power, radar and SpySat check discovery for a controlled House (0x00508CBD).
    pub const fn building_services_use_discovery(self) -> bool {
        self.has_flag(Self::BUILDING_SERVICES_USE_DISCOVERY)
    }

    pub const fn with_building_services_use_discovery(self, enabled: bool) -> Self {
        self.with_flag(Self::BUILDING_SERVICES_USE_DISCOVERY, enabled)
    }

    /// Multiply House difficulty biases by the selected country's values (0x004F6EC0).
    pub const fn uses_country_difficulty_biases(self) -> bool {
        self.has_flag(Self::USES_COUNTRY_DIFFICULTY_BIASES)
    }

    pub const fn with_uses_country_difficulty_biases(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_COUNTRY_DIFFICULTY_BIASES, enabled)
    }

    /// Ares applies the lobby Super Weapons switch to factories (0x10021A00).
    pub const fn ares_applies_superweapon_shell_setting(self) -> bool {
        self.has_flag(Self::ARES_APPLIES_SUPERWEAPON_SHELL_SETTING)
    }

    pub const fn with_ares_applies_superweapon_shell_setting(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_APPLIES_SUPERWEAPON_SHELL_SETTING, enabled)
    }

    /// Skip the local player's authored structures in local multiplayer setup (0x0044F8CB).
    pub const fn skips_local_player_authored_buildings(self) -> bool {
        self.has_flag(Self::SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS)
    }

    pub const fn with_skips_local_player_authored_buildings(self, enabled: bool) -> Self {
        self.with_flag(Self::SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS, enabled)
    }

    /// Start a new AI base plan after MCV deployment (0x0073987C).
    pub const fn recalculates_ai_base_after_mcv_deploy(self) -> bool {
        self.has_flag(Self::RECALCULATES_AI_BASE_AFTER_MCV_DEPLOY)
    }

    pub const fn with_recalculates_ai_base_after_mcv_deploy(self, enabled: bool) -> Self {
        self.with_flag(Self::RECALCULATES_AI_BASE_AFTER_MCV_DEPLOY, enabled)
    }

    /// Enable live crate-slot placement and regeneration (0x0056C020).
    pub const fn crates_use_runtime_slots(self) -> bool {
        self.has_flag(Self::CRATES_USE_RUNTIME_SLOTS)
    }

    pub const fn with_crates_use_runtime_slots(self, enabled: bool) -> Self {
        self.with_flag(Self::CRATES_USE_RUNTIME_SLOTS, enabled)
    }

    /// Allow a passive House to collect a crate (0x00481A3F).
    pub const fn crates_allow_passive_collectors(self) -> bool {
        self.has_flag(Self::CRATES_ALLOW_PASSIVE_COLLECTORS)
    }

    pub const fn with_crates_allow_passive_collectors(self, enabled: bool) -> Self {
        self.with_flag(Self::CRATES_ALLOW_PASSIVE_COLLECTORS, enabled)
    }

    /// Allow a crate overlay to supply the default payload (0x00481B3D).
    pub const fn crates_use_authored_default_payload(self) -> bool {
        self.has_flag(Self::CRATES_USE_AUTHORED_DEFAULT_PAYLOAD)
    }

    pub const fn with_crates_use_authored_default_payload(self, enabled: bool) -> Self {
        self.with_flag(Self::CRATES_USE_AUTHORED_DEFAULT_PAYLOAD, enabled)
    }

    /// Validate and normalize crate payloads on collection (0x00481B99).
    pub const fn crates_validate_pickup_payload(self) -> bool {
        self.has_flag(Self::CRATES_VALIDATE_PICKUP_PAYLOAD)
    }

    pub const fn with_crates_validate_pickup_payload(self, enabled: bool) -> Self {
        self.with_flag(Self::CRATES_VALIDATE_PICKUP_PAYLOAD, enabled)
    }

    /// Credit the local player when the campaign money crate qualifies (0x004824A7).
    pub const fn crates_pay_local_player(self) -> bool {
        self.has_flag(Self::CRATES_PAY_LOCAL_PLAYER)
    }

    pub const fn with_crates_pay_local_player(self, enabled: bool) -> Self {
        self.with_flag(Self::CRATES_PAY_LOCAL_PLAYER, enabled)
    }

    /// Allow a passive House to cast direct sight (0x0070ADDE).
    pub const fn passive_houses_cast_direct_sight(self) -> bool {
        self.has_flag(Self::PASSIVE_HOUSES_CAST_DIRECT_SIGHT)
    }

    pub const fn with_passive_houses_cast_direct_sight(self, enabled: bool) -> Self {
        self.with_flag(Self::PASSIVE_HOUSES_CAST_DIRECT_SIGHT, enabled)
    }

    /// Skip base response when an armed victim can defend itself (0x00708114).
    pub const fn armed_victims_defend_without_base_response(self) -> bool {
        self.has_flag(Self::ARMED_VICTIMS_DEFEND_WITHOUT_BASE_RESPONSE)
    }

    pub const fn with_armed_victims_defend_without_base_response(self, enabled: bool) -> Self {
        self.with_flag(Self::ARMED_VICTIMS_DEFEND_WITHOUT_BASE_RESPONSE, enabled)
    }

    /// Apply the recruitable-mission table to base defenders (0x007081D4).
    pub const fn base_defense_requires_recruitable_mission(self) -> bool {
        self.has_flag(Self::BASE_DEFENSE_REQUIRES_RECRUITABLE_MISSION)
    }

    pub const fn with_base_defense_requires_recruitable_mission(self, enabled: bool) -> Self {
        self.with_flag(Self::BASE_DEFENSE_REQUIRES_RECRUITABLE_MISSION, enabled)
    }

    /// Use local discovery when deciding to reveal an attacker (0x006FDE73).
    pub const fn fire_disclosure_uses_local_discovery(self) -> bool {
        self.has_flag(Self::FIRE_DISCLOSURE_USES_LOCAL_DISCOVERY)
    }

    pub const fn with_fire_disclosure_uses_local_discovery(self, enabled: bool) -> Self {
        self.with_flag(Self::FIRE_DISCLOSURE_USES_LOCAL_DISCOVERY, enabled)
    }

    /// Prefer the AI's designated enemy for Ares Hunter Seeker (0x10042E40).
    pub const fn hunter_seeker_limits_ai_to_designated_enemy(self) -> bool {
        self.has_flag(Self::HUNTER_SEEKER_LIMITS_AI_TO_DESIGNATED_ENEMY)
    }

    pub const fn with_hunter_seeker_limits_ai_to_designated_enemy(self, enabled: bool) -> Self {
        self.with_flag(Self::HUNTER_SEEKER_LIMITS_AI_TO_DESIGNATED_ENEMY, enabled)
    }

    /// Prefer nonpassive targets for human Ares Hunter Seekers (0x10042E40).
    pub const fn hunter_seeker_humans_prefer_nonpassive_targets(self) -> bool {
        self.has_flag(Self::HUNTER_SEEKER_HUMANS_PREFER_NONPASSIVE_TARGETS)
    }

    pub const fn with_hunter_seeker_humans_prefer_nonpassive_targets(self, enabled: bool) -> Self {
        self.with_flag(
            Self::HUNTER_SEEKER_HUMANS_PREFER_NONPASSIVE_TARGETS,
            enabled,
        )
    }

    pub const fn bootstrap(self) -> BootstrapRuleProfile {
        if self.has_flag(Self::MULTIPLAYER_BOOTSTRAP) {
            BootstrapRuleProfile::Multiplayer
        } else {
            BootstrapRuleProfile::Campaign
        }
    }

    pub const fn with_bootstrap(self, profile: BootstrapRuleProfile) -> Self {
        self.with_flag(
            Self::MULTIPLAYER_BOOTSTRAP,
            profile.uses_multiplayer_rules(),
        )
    }

    pub const fn allow_win_tags(self) -> AllowWinTagsPolicy {
        if self.has_flag(Self::IGNORE_ALLOW_WIN_TAGS) {
            AllowWinTagsPolicy::Ignore
        } else {
            AllowWinTagsPolicy::RequireClear
        }
    }

    pub const fn with_allow_win_tags(self, policy: AllowWinTagsPolicy) -> Self {
        self.with_flag(
            Self::IGNORE_ALLOW_WIN_TAGS,
            matches!(policy, AllowWinTagsPolicy::Ignore),
        )
    }

    pub const fn computer_paranoid(self) -> ComputerParanoidPolicy {
        if self.has_flag(Self::PARANOID_REACTIONS) {
            ComputerParanoidPolicy::React
        } else {
            ComputerParanoidPolicy::Disabled
        }
    }

    pub const fn with_computer_paranoid(self, policy: ComputerParanoidPolicy) -> Self {
        self.with_flag(
            Self::PARANOID_REACTIONS,
            matches!(policy, ComputerParanoidPolicy::React),
        )
    }

    pub const fn crates(self) -> AuthoredCratePolicy {
        if self.has_flag(Self::SKIP_AUTHORED_CRATES) {
            AuthoredCratePolicy::Skip
        } else {
            AuthoredCratePolicy::Keep
        }
    }

    pub const fn with_crates(self, policy: AuthoredCratePolicy) -> Self {
        self.with_flag(
            Self::SKIP_AUTHORED_CRATES,
            matches!(policy, AuthoredCratePolicy::Skip),
        )
    }

    pub const fn ai_base(self) -> AiBasePolicy {
        if self.has_flag(Self::REPLACE_UNAVAILABLE_BASE_NODES) {
            AiBasePolicy::ReplaceUnavailableNodes
        } else {
            AiBasePolicy::RetainAuthoredNodes
        }
    }

    pub const fn with_ai_base(self, policy: AiBasePolicy) -> Self {
        self.with_flag(
            Self::REPLACE_UNAVAILABLE_BASE_NODES,
            matches!(policy, AiBasePolicy::ReplaceUnavailableNodes),
        )
    }

    /// A computer's construction-yard maker can hunt to deploy, both from
    /// UnitClass::AI (0x007363DE..0x0073645B) and after an initial Unload
    /// attempt without a pending deploy (0x0073DDCB..0x0073DE1E).
    pub(crate) const fn allows_ai_mcv_deploy_hunt(self) -> bool {
        self.has_flag(Self::AI_MCV_DEPLOY_HUNT)
    }

    pub const fn with_ai_mcv_deploy_hunt(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_MCV_DEPLOY_HUNT, enabled)
    }

    pub const fn double_owned(self) -> DoubleOwnedPolicy {
        if self.has_flag(Self::DOUBLE_OWNED_ALL_COUNTRIES) {
            DoubleOwnedPolicy::AllCountries
        } else {
            DoubleOwnedPolicy::OwnerMask
        }
    }

    pub const fn with_double_owned(self, policy: DoubleOwnedPolicy) -> Self {
        self.with_flag(
            Self::DOUBLE_OWNED_ALL_COUNTRIES,
            matches!(policy, DoubleOwnedPolicy::AllCountries),
        )
    }

    pub const fn mod_map_additions(self) -> bool {
        self.has_flag(Self::MOD_MAP_ADDITIONS)
    }

    pub const fn with_mod_map_additions(self, enabled: bool) -> Self {
        self.with_flag(Self::MOD_MAP_ADDITIONS, enabled)
    }

    pub const fn shared_allied_victory(self) -> bool {
        self.has_flag(Self::SHARED_ALLIED_VICTORY)
    }

    pub const fn with_shared_allied_victory(self, enabled: bool) -> Self {
        self.with_flag(Self::SHARED_ALLIED_VICTORY, enabled)
    }

    pub const fn skips_network_score_dialog(self) -> bool {
        self.has_flag(Self::SKIP_NETWORK_SCORE_DIALOG)
    }

    pub const fn with_skips_network_score_dialog(self, enabled: bool) -> Self {
        self.with_flag(Self::SKIP_NETWORK_SCORE_DIALOG, enabled)
    }

    /// `Computer_Paranoid @ 0x00501640` can pass the AlliesAllowed gate
    /// when the stock mode's virtual at +4 returns true (0x00501659).
    pub(crate) const fn paranoid_ignores_allies_allowed(self) -> bool {
        self.has_flag(Self::PARANOID_IGNORES_ALLIES_ALLOWED)
    }

    pub const fn with_paranoid_ignores_allies_allowed(self, enabled: bool) -> Self {
        self.with_flag(Self::PARANOID_IGNORES_ALLIES_ALLOWED, enabled)
    }

    /// The LAN/WOL sidebar request chooses the briefing when both mode
    /// virtuals answer true (0x00653918..0x00653938).
    pub const fn restates_mission_for_network_diplomacy(self) -> bool {
        self.has_flag(Self::NETWORK_DIPLOMACY_BRIEFING)
    }

    pub const fn with_network_diplomacy_briefing(self, enabled: bool) -> Self {
        self.with_flag(Self::NETWORK_DIPLOMACY_BRIEFING, enabled)
    }

    /// Add mutual alliances among starting Houses with matching human roles.
    pub(crate) const fn allies_starting_houses_by_role(self) -> bool {
        self.has_flag(Self::STARTING_HOUSE_ROLE_ALLIANCES)
    }

    pub const fn with_starting_house_role_alliances(self, enabled: bool) -> Self {
        self.with_flag(Self::STARTING_HOUSE_ROLE_ALLIANCES, enabled)
    }

    /// Ares RevealHelper (0x100692D5..0x10069315 and 0x10069480)
    /// excludes three map-relative cells in competitive LAN/WOL games.
    pub(crate) const fn ares_psychic_reveal_exclusions(self) -> bool {
        self.has_flag(Self::ARES_PSYCHIC_REVEAL_EXCLUSIONS)
    }

    pub const fn with_ares_psychic_reveal_exclusions(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_PSYCHIC_REVEAL_EXCLUSIONS, enabled)
    }

    /// `HouseClass::Update @ 0x004F8440` checks the AllowWin count only
    /// in the mode-zero victory arm (`0x004F85F7..0x004F8605`).
    pub(crate) const fn victory_requires_cleared_allow_win_tags(self) -> bool {
        matches!(self.allow_win_tags(), AllowWinTagsPolicy::RequireClear)
    }

    /// Admission of diplomacy and base requests into `Computer_Paranoid @
    /// 0x00501640`. Each caller applies its own House and Rules checks.
    pub(crate) const fn permits_paranoid_reactions(self) -> bool {
        matches!(self.computer_paranoid(), ComputerParanoidPolicy::React)
    }

    /// `TeamTypeClass::Create_Team @ 0x006F09C0` checks mode at 0x006F0A16.
    pub(crate) const fn team_creation_limit_scope(self) -> TeamLimitScope {
        if self.has_flag(Self::TEAM_LIMIT_PER_HOUSE) {
            TeamLimitScope::PerHouse
        } else {
            TeamLimitScope::AllHouses
        }
    }

    pub(crate) const fn with_team_creation_limit_scope(self, scope: TeamLimitScope) -> Self {
        self.with_flag(
            Self::TEAM_LIMIT_PER_HOUSE,
            matches!(scope, TeamLimitScope::PerHouse),
        )
    }

    pub const fn dissolves_unfilled_teams(self) -> bool {
        self.has_flag(Self::DISSOLVE_UNFILLED_TEAMS)
    }

    pub const fn with_dissolve_unfilled_teams(self, enabled: bool) -> Self {
        self.with_flag(Self::DISSOLVE_UNFILLED_TEAMS, enabled)
    }

    /// `TeamClass::AI @ 0x006E9140` uses `DissolveUnfilledTeamDelay=` for
    /// never-full empty teams in the nonzero mode arm at `0x006E92AD`.
    pub(crate) const fn unfilled_team_timeout(self, delay: i32) -> Option<i32> {
        if self.dissolves_unfilled_teams() {
            Some(delay)
        } else {
            None
        }
    }
}
