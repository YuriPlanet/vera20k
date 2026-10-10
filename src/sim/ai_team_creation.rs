//! The computer's team creation: when a house creates teams, and of which
//! TeamTypes.
//!
//! Native owner: `HouseClass`. Every frame each house's update
//! (`HouseClass::Update`, `0x004F8A00..0x004F8B08`) looks at its team timer
//! (`+0x5798`/`+0x57A0`). Once the timer has expired, a house no human
//! controls and not `MultiplayPassive=` picks TeamTypes from the AI triggers
//! (`0x006F0AB0`, [`select_team_types`]), creates a team of each
//! (`TeamTypeClass::Create_Team @ 0x006F09C0`,
//! `TeamScriptVm::construct_team`) and restarts the timer with its
//! difficulty's `[General] TeamDelays=`. A human or passive house leaves its
//! expired timer as it is. `HouseClass::SetDifficulty` seeds the timer,
//! staggered by 175 frames per house (`HouseState::set_difficulty`).
//!
//! A new team is empty and forming: the unit choosers
//! (`sim::ai_unit_choice`) build what the forming teams still need.
//!
//! Evidence: instruction reading of the update block, the selector, the
//! eligibility test `0x0041E720` and its helpers. `tools/ai_team_oracle.py`
//! executes the update block (its timer, gates and restart), the selector
//! (draws, team counts, eviction, weights and cancel pass), the eligibility
//! test's defense gate and its power and money conditions, and the Iron
//! Curtain readiness `0x0041F0D0`; `ai_team_creation_tests` replays them.
//! The other eligibility gates, which counter a condition reads and the
//! Super lookup rest on instruction reading.
//!
//! Scenario draws: `RandomRanged(1, 100)` on every pass that reaches the
//! selector, then `RandomRanged(1, total weight)` when an AI trigger
//! qualifies (no draw when the total is 1). Timer writes: the restart after
//! every such pass. Detach calls: the evicted defense team's destructor
//! (`TeamClass::~TeamClass @ 0x006E8DE0`, `TeamScriptVm::destroy_team`).
//!
//! RESIDUALS:
//! - A campaign (`GameMode == 0`) gates an AI trigger on the scenario's
//!   difficulty (`Scenario+0x60C`), now retained by ScenarioSession, and admits a
//!   TaskForce type without a factory when the house owns one it can recruit
//!   (`0x00509610`'s scan of `0x008B3DC4` with `0x006F1E20`). VERA skips the
//!   difficulty gate and refuses such a trigger. Trigger: campaign AI trigger
//!   selection after startup; effects include missing recruitment and a
//!   different eligible/weighted trigger set. Required live chain: issue1308.
//! - The trigger actions that write RatioAITriggerTeam (`0x006DF364`,
//!   `0x006E330E`) are not ported; the map reader now initializes this owner.
//! - Conditions 5 and 6 compare a super weapon's charge against its type's
//!   `RechargeTime=` ([`super_nearly_ready`]'s residual).

use crate::sim::world::FrameEffects;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::intern::InternedId;
use crate::sim::production::find_factory;
use crate::sim::superweapon::{super_nearly_ready, super_types_with_type};
use crate::sim::team_script_vm::{
    TeamAiTriggerDefinition, TeamAiTriggerOwner, TeamMemberTypeIdentity, TeamTypeDefinition,
};
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::native_x87::X87Chop53;

/// The AI trigger weight that takes precedence (`0x006F0C72`): once one
/// qualifies, only triggers of this weight compete.
const PRIORITY_WEIGHT: i32 = 5000;

/// A house's team creation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HouseTeamCreation {
    /// `HouseClass+0x5798`/`+0x57A0`: the constructor starts it at the
    /// construction frame, 0, for one frame (`0x004F5CCA..0x004F5CDB`);
    /// `HouseState::set_difficulty` restarts it.
    timer: CdTimer,
    /// `HouseClass+0x565C`, the percent chance a pass picks an AI trigger:
    /// constructor 100 (`0x004F5BDD`), followed by the map's exact-key read.
    ratio: i32,
}

impl Default for HouseTeamCreation {
    fn default() -> Self {
        Self {
            timer: CdTimer::started(0, 1),
            ratio: 100,
        }
    }
}

impl HouseTeamCreation {
    /// House ReadScenarioINI500D0D..500D25 uses the existing ratio as default;
    /// its temporary team timer is then overwritten by outer SetDifficulty.
    pub(crate) fn read_scenario_ratio(&mut self, section: &crate::rules::ini_parser::IniSection) {
        self.ratio = section.read_int("RatioAITriggerTeam", self.ratio);
    }

    /// Restart the team timer at `frame` for `delay` frames.
    pub(crate) fn restart(&mut self, frame: i32, delay: i32) {
        self.timer.start(frame, delay);
    }

    /// Fold the timer and the ratio, each tagged and only off its
    /// constructor value, so a house that never ran team creation hashes as
    /// earlier schemas.
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        let constructed = Self::default();
        if self.timer != constructed.timer {
            b"house-team-timer-v1".hash(hasher);
            self.timer.hash(hasher);
        }
        if self.ratio != constructed.ratio {
            b"house-ai-trigger-ratio-v1".hash(hasher);
            self.ratio.hash(hasher);
        }
    }

    pub(crate) const fn timer(&self) -> CdTimer {
        self.timer
    }

    pub(crate) const fn ratio(&self) -> i32 {
        self.ratio
    }
}

/// The team block of `HouseClass::Update` for house `owner` (see the module
/// doc).
pub(crate) fn update_team_creation(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    frame_effects: FrameEffects<'_>,
) {
    let frame = sim.session.binary_frame as i32;
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if !house.team_creation.timer.expired(frame)
        || house.is_controlled_by_human(game_mode_nonzero)
        || house.multiplay_passive
    {
        return;
    }
    let team_types = select_team_types(sim, rules, owner, frame_effects);
    for team_type in team_types {
        sim.team_script_vm
            .construct_team(team_type, owner, game_mode_nonzero, frame);
    }
    if let Some(house) = sim.houses.get_mut(&owner) {
        let delay = house.difficulty_value(&rules.general.team_delays);
        house.team_creation.restart(frame, delay);
    }
}

/// The selector `0x006F0AB0` for house `owner`: the TeamTypes to create.
///
/// It always draws `RandomRanged(1, 100)`; at or below the house's
/// `RatioAITriggerTeam=` (`+0x565C`), and while its AI triggers are active
/// (`+0x1F2`), it counts the house's teams and its base-defense teams
/// (`IsBaseDefense=`):
/// - below `TotalAITeamCap=`, or with fewer defense teams than half its
///   teams, the defense teams are full when they exceed
///   `MaximumAIDefensiveTeams=`;
/// - otherwise the oldest defense team (the first of the lowest creation
///   frame) is destroyed, and the defense teams are full.
///
/// Still below the cap, every AI trigger in order that qualifies
/// ([`is_eligible`]) joins the draw with its weight (`ftol`); the first one
/// weighing 5000 restarts the list, and from then on only those of 5000
/// join. A non-zero total draws `RandomRanged(1, total)`, and the first
/// trigger whose running sum reaches it (unsigned) gives its TeamTypes, the
/// second after the first.
///
/// When one of the house's teams is still forming and is of a picked type,
/// nothing is created. Otherwise each picked TeamType is marked
/// `Autocreate=` (`+0xA9`).
fn select_team_types(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    frame_effects: FrameEffects<'_>,
) -> Vec<InternedId> {
    select_team_types_with(
        sim,
        rules,
        owner,
        |sim, trigger, enemy, defense_full| {
            is_eligible(sim, rules, trigger, owner, enemy, defense_full)
        },
        |sim, low, high| sim.scenario_rng.next_range_i32_inclusive(low, high),
        frame_effects,
    )
}

/// [`select_team_types`] with `eligible` as the eligibility test
/// ([`is_eligible`] in play) and `draw` as `RandomRanged` on the Scenario RNG.
pub(crate) fn select_team_types_with(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    eligible: impl FnMut(&Simulation, &TeamAiTriggerDefinition, Option<InternedId>, bool) -> bool,
    mut draw: impl FnMut(&mut Simulation, i32, i32) -> i32,
    frame_effects: FrameEffects<'_>,
) -> Vec<InternedId> {
    let Some(house) = sim.houses.get(&owner) else {
        return Vec::new();
    };
    let enemy = house.enemy_house;
    let ratio = house.team_creation.ratio;
    let triggers_active = house.ai_activation.ai_triggers_active;
    let team_cap = house.difficulty_value(&rules.general.total_ai_team_cap);
    let max_defensive = house.difficulty_value(&rules.general.maximum_ai_defensive_teams);
    let mut picked = Vec::new();
    let roll = draw(sim, 1, 100);
    if roll <= ratio && triggers_active {
        let (mut teams, defense_teams) = count_teams(sim, owner);
        let mut defense_full = false;
        if teams < team_cap || defense_teams < teams / 2 {
            defense_full = max_defensive < defense_teams;
        } else if let Some(oldest) = oldest_defense_team(sim, owner) {
            teams -= 1;
            defense_full = true;
            sim.destroy_team(oldest, rules, frame_effects);
        }
        if teams < team_cap {
            picked = draw_ai_trigger(sim, enemy, defense_full, eligible, draw);
        }
    }
    let vm = &sim.team_script_vm;
    let cancelled = vm.teams_in_order().any(|team| {
        team.owner() == owner
            && team.is_forming()
            && team
                .team_type_id()
                .is_some_and(|team_type| picked.contains(&team_type))
    });
    if cancelled {
        return Vec::new();
    }
    for &team_type in &picked {
        sim.team_script_vm.mark_autocreate(team_type);
    }
    picked
}

/// The house's teams, and those of an `IsBaseDefense=` TeamType. The latter
/// is also the house's `+0x566C`, which the TeamClass constructor and
/// destructor keep (`0x006E8D14`, `0x006E8E58`).
fn count_teams(sim: &Simulation, owner: InternedId) -> (i32, i32) {
    let vm = &sim.team_script_vm;
    vm.teams_in_order()
        .filter(|team| team.owner() == owner)
        .fold((0, 0), |(teams, defense), team| {
            let is_defense = team
                .team_type_id()
                .and_then(|id| vm.team_type_definition(id))
                .is_some_and(|team_type| team_type.is_base_defense);
            (teams + 1, defense + i32::from(is_defense))
        })
}

/// The house's defense team of the lowest creation frame (`+0x50`, signed
/// `<` from `0x7FFFFFFF`, so the first of equals).
fn oldest_defense_team(sim: &Simulation, owner: InternedId) -> Option<u64> {
    let vm = &sim.team_script_vm;
    let mut oldest: Option<(u64, i32)> = None;
    for team in vm.teams_in_order() {
        let is_defense = team
            .team_type_id()
            .and_then(|id| vm.team_type_definition(id))
            .is_some_and(|team_type| team_type.is_base_defense);
        if team.owner() == owner
            && is_defense
            && team.created_frame() < oldest.map_or(i32::MAX, |(_, frame)| frame)
        {
            oldest = Some((team.id(), team.created_frame()));
        }
    }
    oldest.map(|(id, _)| id)
}

/// The weighted draw among the qualifying AI triggers (`0x006F0C3E..
/// 0x006F0DC2`).
fn draw_ai_trigger(
    sim: &mut Simulation,
    enemy: Option<InternedId>,
    defense_full: bool,
    mut eligible: impl FnMut(&Simulation, &TeamAiTriggerDefinition, Option<InternedId>, bool) -> bool,
    mut draw: impl FnMut(&mut Simulation, i32, i32) -> i32,
) -> Vec<InternedId> {
    let mut entries: Vec<(Option<InternedId>, Option<InternedId>, i32)> = Vec::new();
    let mut priority = false;
    let mut total: i32 = 0;
    for (trigger, record) in sim.team_script_vm.ai_triggers_in_order() {
        if !eligible(sim, trigger, enemy, defense_full) {
            continue;
        }
        let weight = X87Chop53::ftol_i32_low_masked(
            X87Chop53::load_f64(record.weight()).expect("an AI trigger weight is finite"),
        );
        if weight == PRIORITY_WEIGHT {
            if !priority {
                priority = true;
                entries.clear();
                total = 0;
            }
        } else if priority {
            continue;
        }
        entries.push((
            trigger.primary_team_type,
            trigger.secondary_team_type,
            weight,
        ));
        total = total.wrapping_add(weight);
    }
    if total == 0 || entries.is_empty() {
        return Vec::new();
    }
    let roll = draw(sim, 1, total) as u32;
    let mut sum: u32 = 0;
    for (primary, secondary, weight) in entries {
        sum = sum.wrapping_add(weight as u32);
        if roll <= sum {
            return primary.into_iter().chain(secondary).collect();
        }
    }
    Vec::new()
}

/// `AITriggerTypeClass::Is_Eligible`-like test `0x0041E720` of `trigger`
/// for house `owner`, whose enemy (`+0x5600`) is `enemy`.
///
/// In order:
/// - a trigger without its first TeamType fails;
/// - with no enemy, or with `UseMinDefenseRule=` and fewer defense teams
///   than `MinimumAIDefensiveTeams=`, only a trigger with a base-defense
///   TeamType passes, and only while the defense teams are not full;
///   otherwise a base-defense trigger fails while they are full;
/// - a global (`AIMD.INI`) trigger fails in a map with
///   `IgnoreGlobalAITriggers=` (`Scenario+0x34B4`);
/// - a disabled trigger fails;
/// - in a multiplayer game a trigger without `IsForMultiplayer` fails, as
///   does one not enabled for the house's difficulty (module residual for
///   campaigns);
/// - its owner, side (1 Allied, 2 Soviet, 3 Yuri) and TechLevel (`+0xB0`)
///   must admit the house;
/// - its condition ([`condition_holds`]);
/// - each TeamType's base zones ([`zones_admit`]), factories
///   ([`factories_admit`]) and `Max=` (the house's teams of the type,
///   `0x005095D0`).
fn is_eligible(
    sim: &Simulation,
    rules: &RuleSet,
    trigger: &TeamAiTriggerDefinition,
    owner: InternedId,
    enemy: Option<InternedId>,
    defense_full: bool,
) -> bool {
    let vm = &sim.team_script_vm;
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let team_type = |id: Option<InternedId>| id.and_then(|id| vm.team_type_definition(id));
    let primary = team_type(trigger.primary_team_type);
    let secondary = team_type(trigger.secondary_team_type);
    let Some(primary) = primary else {
        return false;
    };
    let base_defense = primary.is_base_defense || secondary.is_some_and(|tt| tt.is_base_defense);
    let (_, defense_teams) = count_teams(sim, owner);
    let defense_admits = defense_gate(
        enemy.is_some(),
        rules.general.use_min_defense_rule,
        defense_teams,
        house.difficulty_value(&rules.general.minimum_ai_defensive_teams),
        base_defense,
        defense_full,
    );
    if !defense_admits {
        return false;
    }
    let global = trigger.source == TeamAiDefinitionSource::FixedAimd;
    if global && sim.session.ignore_global_ai_triggers() {
        return false;
    }
    if !trigger.enabled {
        return false;
    }
    if sim.session.game_mode_nonzero {
        if !trigger.multiplayer {
            return false;
        }
        let enabled = match house.difficulty.table_index() {
            0 => trigger.difficulty_enabled[2],
            1 => trigger.difficulty_enabled[1],
            2 => trigger.difficulty_enabled[0],
            _ => true,
        };
        if !enabled {
            return false;
        }
    }
    match trigger.owner {
        None => return false,
        Some(TeamAiTriggerOwner::All) => {}
        Some(TeamAiTriggerOwner::Country(country)) => {
            let house_country = rules.country_index(sim.interner.resolve(house.house_type_id()));
            if house_country != Some(country) {
                return false;
            }
        }
    }
    let side_admits = match trigger.side {
        1 => house.side_index == 0,
        2 => house.side_index == 1,
        3 => house.side_index == 2,
        _ => true,
    };
    if !side_admits || house.tech_level < trigger.threshold {
        return false;
    }
    if !condition_holds(sim, rules, trigger, owner, enemy) {
        return false;
    }
    let team_types = std::iter::once(primary).chain(secondary);
    team_types
        .clone()
        .all(|tt| zones_admit(sim, tt, owner, enemy))
        && team_types
            .clone()
            .all(|tt| factories_admit(sim, rules, tt, owner))
        && team_types
            .clone()
            .all(|tt| !vm.at_max_teams(tt.id, owner, false))
}

/// The eligibility test's defense gate (`0x0041E740..0x0041E7AD`): with no
/// enemy, or with `UseMinDefenseRule=` and fewer defense teams (`+0x566C`)
/// than `MinimumAIDefensiveTeams=`, only a base-defense trigger passes, and
/// only while the defense teams are not full; otherwise a base-defense
/// trigger fails while they are full.
pub(crate) fn defense_gate(
    has_enemy: bool,
    use_min_defense_rule: bool,
    defense_teams: i32,
    min_defensive: i32,
    base_defense: bool,
    defense_full: bool,
) -> bool {
    let needs_defense = !has_enemy || (use_min_defense_rule && defense_teams < min_defensive);
    if needs_defense {
        base_defense && !defense_full
    } else {
        !(base_defense && defense_full)
    }
}

/// The trigger's condition (`+0x98`, switch `0x0041E908`). With no enemy,
/// only -1 (always) and 1 (the house owns) are asked; otherwise:
/// - -1: always;
/// - 0: the enemy owns ([`owned_count_holds`], `0x0041EAF0`);
/// - 1: the house owns (`0x0041EE90`);
/// - 2: the enemy's power output less its drain (IHouse `+0x20`, `+0x24`)
///   is below 100; 3: below 0;
/// - 4: the enemy's available money compares (`0x0041F230`);
/// - 5: the house's Iron Curtain is nearly ready (`0x0041F0D0`); 6: its
///   Chronosphere (`0x0041F180`), see [`super_weapon_nearly_ready`];
/// - 7: the first house of the Civilian side owns (`0x0041EC90`);
/// - any other fails.
fn condition_holds(
    sim: &Simulation,
    rules: &RuleSet,
    trigger: &TeamAiTriggerDefinition,
    owner: InternedId,
    enemy: Option<InternedId>,
) -> bool {
    let Some(enemy) = enemy else {
        return match trigger.condition {
            -1 => true,
            1 => owned_count_holds(sim, trigger, owner),
            _ => false,
        };
    };
    let power = || {
        sim.power_states
            .get(&enemy)
            .map_or((0, 0), |power| (power.total_output, power.total_drain))
    };
    match trigger.condition {
        -1 => true,
        0 => owned_count_holds(sim, trigger, enemy),
        1 => owned_count_holds(sim, trigger, owner),
        2 | 3 => {
            let (output, drain) = power();
            power_condition_holds(trigger.condition, output, drain)
        }
        4 => compare(
            &trigger.comparison_mask,
            crate::sim::credit_income::available_money(sim, enemy),
        ),
        5 => super_weapon_nearly_ready(sim, rules, owner, SuperWeaponKind::IronCurtain),
        6 => super_weapon_nearly_ready(sim, rules, owner, SuperWeaponKind::ChronoSphere),
        7 => civilian_house(sim, rules).is_some_and(|house| owned_count_holds(sim, trigger, house)),
        _ => false,
    }
}

/// Conditions 2 and 3 (`0x0041E92B..0x0041E999`): the enemy's power output
/// less its drain (IHouse `+0x20` less `+0x24`, a wrapping `SUB`, then
/// `FILD`) is below 100.0 (2) or 0.0 (3).
pub(crate) fn power_condition_holds(condition: i32, output: i32, drain: i32) -> bool {
    let surplus = output.wrapping_sub(drain);
    if condition == 2 {
        surplus < 100
    } else {
        surplus < 0
    }
}

/// A condition's comparison (`+0xE8`, the second word of `mask`) of `value`
/// with its amount (`+0xE4`, the first): 0 `<`, 1 `<=`, 2 `==`, 3 `>=`,
/// 4 `>`, 5 `!=`; any other fails.
pub(crate) fn compare(mask: &[u8; 32], value: i32) -> bool {
    let word = |at: usize| i32::from_le_bytes(mask[at..at + 4].try_into().expect("four bytes"));
    let amount = word(0);
    match word(4) {
        0 => value < amount,
        1 => value <= amount,
        2 => value == amount,
        3 => value >= amount,
        4 => value > amount,
        5 => value != amount,
        _ => false,
    }
}

/// Whether `house`'s count of the trigger's type (`+0xD8`) on the map
/// (`HouseTracking::active_count`: `+0x5564`, `+0x5578`, `+0x558C`,
/// `+0x5550` by class) compares; a trigger without a type counts 0.
fn owned_count_holds(
    sim: &Simulation,
    trigger: &TeamAiTriggerDefinition,
    house: InternedId,
) -> bool {
    let count = match (trigger.object_type, sim.houses.get(&house)) {
        (Some(TeamMemberTypeIdentity { category, id }), Some(house)) => house
            .tracking
            .active_count(EntityCategory::from(category), id),
        _ => 0,
    };
    compare(&trigger.comparison_mask, count)
}

/// The first house, in HouseClass::Array order, whose HouseType's side
/// (`+0xBC`) is `Civilian` (`SideClass::Find_Index @ 0x006A46D0`).
fn civilian_house(sim: &Simulation, rules: &RuleSet) -> Option<InternedId> {
    let civilian = rules.side_index("Civilian")?;
    sim.session.house_order.iter().copied().find(|owner| {
        sim.houses.get(owner).is_some_and(|house| {
            rules.country_side_index(sim.interner.resolve(house.house_type_id())) == Some(civilian)
        })
    })
}

/// `0x0041F0D0` (Iron Curtain) and `0x0041F180` (Chronosphere): the house's
/// first super weapon of `kind`, in `[SuperWeaponTypes]` order, is
/// [`super_nearly_ready`].
fn super_weapon_nearly_ready(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: SuperWeaponKind,
) -> bool {
    super_types_with_type(rules, kind.native_index())
        .next()
        .is_some_and(|type_name| super_nearly_ready(sim, rules, owner, type_name))
}

/// `0x0041FEE0` for TeamType `team_type`: with `+0xF0` and an enemy, the
/// house's and the enemy's base cells (`HouseClass+0x5494`, else `+0x5490`,
/// `0x0050DEF0`) share a zone under the TeamType's movement zone (`+0xEC`);
/// with `+0xF1` they must not, but must share one as Amphibious.
fn zones_admit(
    sim: &Simulation,
    team_type: &TeamTypeDefinition,
    owner: InternedId,
    enemy: Option<InternedId>,
) -> bool {
    let Some(enemy) = enemy else {
        return true;
    };
    if !team_type.base_zone_relation_enforced {
        return true;
    }
    let base_cell = |house: InternedId| {
        let (x, y) = sim
            .houses
            .get(&house)
            .map_or((0, 0), |house| house.base_origin());
        (i32::from(x), i32::from(y))
    };
    let (own, theirs) = (base_cell(owner), base_cell(enemy));
    let zone = |cell: (i32, i32), movement_zone: MovementZone| {
        sim.zone_grid
            .as_ref()
            .zip(sim.resolved_terrain.as_ref())
            .and_then(|(zones, terrain)| {
                zones.get_zone_id_native(
                    terrain,
                    (cell.0 as u16, cell.1 as u16),
                    movement_zone,
                    false,
                )
            })
    };
    let movement_zone = team_type.combined_movement_zone;
    let same_zone = zone(own, movement_zone) == zone(theirs, movement_zone);
    if !team_type.transport_crossing_required {
        return same_zone;
    }
    !same_zone && zone(own, MovementZone::Amphibious) == zone(theirs, MovementZone::Amphibious)
}

/// `TeamTypeClass` `0x00509610` for `team_type`: in a multiplayer game, every
/// TaskForce type has a factory of the house (`FindFactory(1, 1, 0,
/// house)`: online, CanBuild not asked). A TeamType without a TaskForce
/// fails.
fn factories_admit(
    sim: &Simulation,
    rules: &RuleSet,
    team_type: &TeamTypeDefinition,
    owner: InternedId,
) -> bool {
    let Some(task_force) = sim.team_script_vm.task_force_of(team_type.id) else {
        return false;
    };
    // Campaigns: module residual.
    sim.session.game_mode_nonzero
        && task_force.entries.iter().all(|entry| {
            rules
                .object_in_category(
                    entry.member_type.category,
                    sim.interner.resolve(entry.member_type.id),
                )
                .is_some_and(|obj| {
                    find_factory(sim, rules, owner, obj, true, true, false).is_some()
                })
        })
}

#[cfg(test)]
#[path = "ai_team_creation_tests.rs"]
pub(crate) mod tests;
