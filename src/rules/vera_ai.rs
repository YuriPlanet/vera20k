//! VERA's own computer-player policy: the `[VeraAI]` rules section and the
//! numbers it holds.
//!
//! The retail AI in `sim::house_strategy` and `sim::ai_base_building` is a
//! faithful port of gamemd's computer player, and it plays the way gamemd
//! plays: a crude build queue, a unit chooser that draws from the house's
//! teams and an anger-driven hunt. It is the rule of the game, not a bug, so
//! it stays the default. What a modder or a map wants is a *different*
//! computer player, and what a trainer wants is a computer player whose
//! behaviour is a small number of numbers it may change.
//!
//! This module is that number set. A game's `[VeraAI]` section switches VERA's
//! own player on and holds its knobs; with retail data the section is absent,
//! every knob keeps its default and [`VeraAiPolicy::enabled`] is false, so the
//! game plays as gamemd.
//!
//! The section is read by the rules projection, like every other rules
//! section, so a mod or a map may carry it and every peer of a network game
//! reads the same numbers from the same rules.
//!
//! Values come from the reader defaults here, never from retail values: no
//! retail INI holds a `[VeraAI]` section. A key outside its range is clamped,
//! never refused, so a bad value cannot stop the game.

use crate::rules::ini_parser::IniSection;

/// The section a game names VERA's own computer-player policy in.
pub const SECTION: &str = "VeraAI";

/// Upper bound for a count of objects or a period in frames. Outside data
/// never stops the game, so every count and period is clamped into it.
const LIMIT: i32 = 10_000;

/// Upper bound for a weight.
const WEIGHT_LIMIT: f32 = 100.0;

/// The squads a house's army is split into (`sim::vera_ai`). How many of them
/// may be on the attack at once is the policy's `AttackWaves`.
pub const SQUAD_COUNT: u32 = 8;

/// VERA's own computer-player policy: the switches and the numbers
/// `sim::vera_ai` plays by.
///
/// The defaults are the policy's own, not retail's: retail holds no
/// `[VeraAI]` section, so a retail game keeps the stock AI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VeraAiPolicy {
    /// Whether VERA's own computer player plays the computer houses.
    ///
    /// Off by default: a game without the section plays as gamemd.
    pub enabled: bool,
    /// How many computer houses VERA's own player takes, in house order.
    ///
    /// Zero takes every computer house, which is what a game wants. One
    /// takes the first, which is what a match between a VERA house and a
    /// stock house wants.
    pub max_houses: i32,
    /// How often the player decides, in frames.
    pub decision_period: i32,
    /// How many combat objects the house must hold before its army leaves.
    pub attack_threshold: i32,
    /// How many combat objects stay at the base and defend it.
    pub home_guard: i32,
    /// How many squads may be on the attack at the same time.
    pub attack_waves: i32,
    /// The composition weights: how much of the army each production
    /// category takes. A category with no weight is never built.
    pub infantry_weight: f32,
    pub vehicle_weight: f32,
    pub aircraft_weight: f32,
    /// The production cadence, in frames: how long the player waits before it
    /// queues the next object of that category.
    pub infantry_period: i32,
    pub vehicle_period: i32,
    pub aircraft_period: i32,
    /// Whether the player queues production of its own, over the native
    /// player's own build queue.
    pub target_production: bool,
    /// How many frames the player waits for the first attack, so that the
    /// base is standing before the army leaves.
    pub opening_delay: i32,
}

impl Default for VeraAiPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_houses: 0,
            decision_period: 30,
            attack_threshold: 6,
            home_guard: 2,
            attack_waves: 4,
            infantry_weight: 1.0,
            vehicle_weight: 1.0,
            aircraft_weight: 0.25,
            infantry_period: 90,
            vehicle_period: 150,
            aircraft_period: 300,
            target_production: true,
            opening_delay: 300,
        }
    }
}

impl VeraAiPolicy {
    /// Read the policy from the `[VeraAI]` section of a rules projection.
    pub fn from_ini(section: &IniSection) -> Self {
        let default = Self::default();
        Self {
            enabled: section.read_bool("Enabled", default.enabled),
            max_houses: section
                .read_int("MaxHouses", default.max_houses)
                .clamp(0, LIMIT),
            decision_period: section
                .read_int("DecisionPeriod", default.decision_period)
                .clamp(1, LIMIT),
            attack_threshold: section
                .read_int("AttackThreshold", default.attack_threshold)
                .clamp(0, LIMIT),
            home_guard: section
                .read_int("HomeGuard", default.home_guard)
                .clamp(0, LIMIT),
            attack_waves: section
                .read_int("AttackWaves", default.attack_waves)
                .clamp(1, SQUAD_COUNT as i32),
            infantry_weight: section
                .read_float("InfantryWeight", default.infantry_weight)
                .clamp(0.0, WEIGHT_LIMIT),
            vehicle_weight: section
                .read_float("VehicleWeight", default.vehicle_weight)
                .clamp(0.0, WEIGHT_LIMIT),
            aircraft_weight: section
                .read_float("AircraftWeight", default.aircraft_weight)
                .clamp(0.0, WEIGHT_LIMIT),
            infantry_period: section
                .read_int("InfantryPeriod", default.infantry_period)
                .clamp(1, LIMIT),
            vehicle_period: section
                .read_int("VehiclePeriod", default.vehicle_period)
                .clamp(1, LIMIT),
            aircraft_period: section
                .read_int("AircraftPeriod", default.aircraft_period)
                .clamp(1, LIMIT),
            target_production: section.read_bool("TargetProduction", default.target_production),
            opening_delay: section
                .read_int("OpeningDelay", default.opening_delay)
                .clamp(0, LIMIT),
        }
    }

    /// The policy a rules projection without a `[VeraAI]` section holds: the
    /// stock AI plays.
    pub fn disabled() -> Self {
        Self::default()
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use crate::rules::ini_parser::IniFile;

    fn read(text: &str) -> super::VeraAiPolicy {
        let file = IniFile::from_str(text);
        let section = file.section(super::SECTION);
        match section {
            Some(section) => super::VeraAiPolicy::from_ini(section),
            None => super::VeraAiPolicy::disabled(),
        }
    }

    #[test]
    fn a_section_that_is_absent_leaves_the_stock_ai_in_charge() {
        let policy = read("[General]\nX=1\n");
        assert!(!policy.enabled);
        assert_eq!(policy, super::VeraAiPolicy::default());
    }

    #[test]
    fn the_section_switches_the_player_on_and_holds_its_numbers() {
        let policy = read("[VeraAI]\nEnabled=1\nAttackThreshold=9\nVehicleWeight=0.5\n");
        assert!(policy.enabled);
        assert_eq!(policy.attack_threshold, 9);
        assert_eq!(policy.vehicle_weight, 0.5);
        // A key the section does not name keeps its default.
        assert_eq!(policy.home_guard, super::VeraAiPolicy::default().home_guard);
    }

    #[test]
    fn a_count_outside_its_range_is_clamped_rather_than_refused() {
        let policy = read(
            "[VeraAI]\nEnabled=1\nAttackThreshold=999999\nHomeGuard=-4\nDecisionPeriod=0\nAttackWaves=99\n",
        );
        assert_eq!(policy.attack_threshold, super::LIMIT);
        assert_eq!(policy.home_guard, 0);
        assert_eq!(policy.decision_period, 1);
        assert_eq!(policy.attack_waves, super::SQUAD_COUNT as i32);
    }

    #[test]
    fn a_weight_outside_its_range_is_clamped() {
        let policy = read("[VeraAI]\nEnabled=1\nInfantryWeight=-2\nAircraftWeight=1000\n");
        assert_eq!(policy.infantry_weight, 0.0);
        assert_eq!(policy.aircraft_weight, super::WEIGHT_LIMIT);
    }
}
