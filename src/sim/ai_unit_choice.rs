//! The computer's Unit, Infantry and Aircraft choices: the type each of its
//! war factories, barracks and airfields makes next.
//!
//! Native owner: `HouseClass`. `+0x5650` (Unit), `+0x5654` (Infantry) and
//! `+0x5658` (Aircraft) hold a type's index in its class array, -1 for none
//! (constructor `0x004F5BC6..0x004F5BD2`). The choosers (`AI_Choose_Unit @
//! 0x004FEA60`; `0x004FEEE0` for infantry and `0x004FF210` for aircraft,
//! whose Ghidra labels are swapped) fill a choice that is -1; the house's
//! chooser dispatch calls them (`sim::ai_base_building`). A factory building
//! makes the choice (`Suggest_New_Object @ 0x004FBD80`,
//! `production::factory_ai`), whose exit clears it again
//! (`BuildingClass::Exit_Object`, `0x00443CCA`, `0x00444119`, `0x00444131`),
//! as does its abandon (`FactoryClass::AbandonProduction`,
//! `production::factory_lifecycle`).
//!
//! A chooser asks what the house's teams still need. Every team of the house
//! still taking members (a `Reinforce=` team until full, any other until it
//! reaches strength) adds its TaskForce less its members
//! (`TeamClass::Get_Needed_Types @ 0x006EF4D0`); each of the house's
//! objects of the class free to join (`FootClass::IsRecruitable @
//! 0x004DA230`) covers one need of its type. Among the types still needed
//! that `CanBuild` admits and the house can pay for, it draws once, always:
//! below `[General] FillEarliestTeamProbability=` percent it takes the type
//! of the earliest team, otherwise it draws again among the most needed
//! types. The Unit chooser first keeps the house's harvesters up (see
//! [`choose`]).
//!
//! Evidence: instruction reading of the three bodies and their helpers;
//! `tools/ai_team_oracle.py` executes the choosers with CanBuild, Cost_Of,
//! Available_Money, IsRecruitable and the draws hooked (the
//! `ai_unit_choice_tests` goldens).
//!
//! RESIDUALS:
//! - Native tallies need in stack arrays of 100 entries: a class with more
//!   than 100 types, or more than 100 candidate types, writes past them.
//!   VERA sizes them by the class. Retail classes hold fewer than 100 types.
//! - A team's `+0x79` (full) and `+0x77` (forced) are written only by
//!   recruitment, which is not ported (`sim::team_script_vm`), so a
//!   `Reinforce=` team counts as needing members forever.

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::production::{CanBuild, can_build};
use crate::sim::world::Simulation;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};

/// `1 / 0x7FFFFFFE` as the choosers scale their draw (`[0x007E3570]`).
const DRAW_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3E00_0000_0040_0000);
/// `0.01` (`[0x007E3808]`).
const PERCENT_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3F84_7AE1_47AE_147B);

/// One of the three choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitChoiceKind {
    Unit,
    Infantry,
    Aircraft,
}

impl UnitChoiceKind {
    /// The class array the choice indexes.
    pub(crate) const fn category(self) -> ObjectCategory {
        match self {
            Self::Unit => ObjectCategory::Vehicle,
            Self::Infantry => ObjectCategory::Infantry,
            Self::Aircraft => ObjectCategory::Aircraft,
        }
    }

    /// The choice a produced object of `category` fills; none for a building.
    pub(crate) const fn of(category: ObjectCategory) -> Option<Self> {
        match category {
            ObjectCategory::Vehicle => Some(Self::Unit),
            ObjectCategory::Infantry => Some(Self::Infantry),
            ObjectCategory::Aircraft => Some(Self::Aircraft),
            ObjectCategory::Building => None,
        }
    }
}

/// `HouseClass+0x5650`, `+0x5654` and `+0x5658`: a type index in its class
/// array, -1 for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HouseAiUnitChoices {
    unit: i32,
    infantry: i32,
    aircraft: i32,
}

impl Default for HouseAiUnitChoices {
    fn default() -> Self {
        Self {
            unit: -1,
            infantry: -1,
            aircraft: -1,
        }
    }
}

impl HouseAiUnitChoices {
    pub(crate) const fn get(&self, kind: UnitChoiceKind) -> i32 {
        match kind {
            UnitChoiceKind::Unit => self.unit,
            UnitChoiceKind::Infantry => self.infantry,
            UnitChoiceKind::Aircraft => self.aircraft,
        }
    }

    fn slot_mut(&mut self, kind: UnitChoiceKind) -> &mut i32 {
        match kind {
            UnitChoiceKind::Unit => &mut self.unit,
            UnitChoiceKind::Infantry => &mut self.infantry,
            UnitChoiceKind::Aircraft => &mut self.aircraft,
        }
    }

    /// An exit or abandon of `kind`'s object.
    pub(crate) fn clear(&mut self, kind: UnitChoiceKind) {
        *self.slot_mut(kind) = -1;
    }

    /// `AI_Manage_Build_Queue`'s harvester choice (`0x004FE113`).
    pub(crate) fn set(&mut self, kind: UnitChoiceKind, index: i32) {
        *self.slot_mut(kind) = index;
    }
}

/// The chooser of `kind` for house `owner`, when its choice is -1.
///
/// The Unit chooser (`0x004FEA7D..0x004FEBD7`) first looks for the first
/// `[General] HarvesterUnit=` type the house's country owns. With one, it
/// takes that harvester, and nothing else, while the house's IQ reaches
/// `[IQ] Harvester=`, no harvester of it has found no ore (`+0x242`), no
/// human controls it, it has fewer `ResourceGatherer=` objects (`+0x158`)
/// than `HarvestersPerRefinery=` times its `ResourceDestination=` objects
/// (`+0x15C`) — `AISlaveMinerNumber=` when no `[AI] BuildRefinery=` type is
/// buildable for it — and the harvester's TechLevel is within the house's
/// (unsigned). Without one, it takes the unit the first buildable refinery
/// undeploys into while it has fewer gatherers than `AISlaveMinerNumber=`.
/// Otherwise it asks the teams.
pub(crate) fn choose(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: UnitChoiceKind,
) {
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.ai_unit_choices.get(kind) != -1 {
        return;
    }
    let fill_earliest_percent =
        house.difficulty_value(&rules.general.fill_earliest_team_probability);
    let choice = match (kind == UnitChoiceKind::Unit)
        .then(|| harvester_choice(sim, rules, owner))
        .flatten()
    {
        Some(index) => Some(index),
        None => {
            let candidates = candidates(sim, rules, owner, kind);
            pick(&candidates, fill_earliest_percent, |low, high| {
                sim.scenario_rng.next_range_i32_inclusive(low, high)
            })
        }
    };
    if let Some(index) = choice
        && let Some(house) = sim.houses.get_mut(&owner)
    {
        *house.ai_unit_choices.slot_mut(kind) = index;
    }
}

/// The Unit chooser's harvester branch (see [`choose`]): the vehicle index
/// it takes, or `None` to ask the teams.
fn harvester_choice(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> Option<i32> {
    let house = sim.houses.get(&owner)?;
    let country_name = sim.interner.resolve(house.house_type_id());
    let country_bit = crate::sim::ai_buildable::house_country_bit(rules, country_name);
    let unit_index = |id: &str| rules.type_array_index(ObjectCategory::Vehicle, id);
    let refinery = crate::sim::ai_buildable::first_buildable_from_array(
        rules,
        &rules.build_refinery_types,
        country_name,
        house.side_index,
        sim.session.game_options.super_weapons,
    );
    let harvester = crate::sim::ai_buildable::first_owner_compatible_harvester(rules, country_bit);
    harvester_decision(&HarvesterFacts {
        harvester: harvester.and_then(|harvester| {
            Some((unit_index(&harvester.id)?, sim.object_tech_level(harvester)))
        }),
        refinery: refinery.map(|refinery| refinery.undeploys_into.as_deref().and_then(unit_index)),
        gatherers: house.tracking.resource_gatherers(),
        destinations: house.tracking.resource_destinations(),
        harvesters_per_refinery: house.difficulty_value(&rules.general.harvesters_per_refinery),
        slave_miners: house.difficulty_value(&rules.ai_slave_miner_number),
        current_iq: house.current_iq,
        iq_harvester: rules.general.iq_harvester,
        no_ore: house.harvester_no_ore,
        human: house.is_controlled_by_human(sim.session.game_mode_nonzero),
        tech_level: house.tech_level,
    })
}

/// What the Unit chooser's harvester branch reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HarvesterFacts {
    /// The first `[General] HarvesterUnit=` type the house's country owns:
    /// its vehicle index (`+0xDF8`) and TechLevel (`+0x634`).
    harvester: Option<(i32, i32)>,
    /// The first `[AI] BuildRefinery=` type buildable for the house
    /// (`HouseClass::FirstBuildableFromArray @ 0x005051E0`): the vehicle
    /// index of its `UndeploysInto=` (`+0x408`), if any.
    refinery: Option<Option<i32>>,
    /// `+0x158`, its `ResourceGatherer=` objects.
    gatherers: i32,
    /// `+0x15C`, its `ResourceDestination=` objects.
    destinations: i32,
    /// `[General] HarvestersPerRefinery=` of its difficulty.
    harvesters_per_refinery: i32,
    /// `[General] AISlaveMinerNumber=` of its difficulty.
    slave_miners: i32,
    /// `+0x24C`.
    current_iq: i32,
    /// `[IQ] Harvester=` (`Rules+0x1458`).
    iq_harvester: i32,
    /// `+0x242`.
    no_ore: bool,
    /// A human controls the house.
    human: bool,
    /// `+0x1D4`.
    tech_level: i32,
}

/// The harvester branch (`0x004FEA7D..0x004FEBD7`) on `facts`. With a
/// harvester: the limit is `HarvestersPerRefinery * destinations`
/// (wrapping `IMUL`), or `AISlaveMinerNumber` when no refinery is
/// buildable; the harvester is taken while the IQ reaches `[IQ]
/// Harvester=`, no harvester found no ore, no human controls the house, its
/// gatherers are below the limit and the harvester's TechLevel is within the
/// house's (unsigned). Without one: the refinery's `UndeploysInto=` while
/// the gatherers are below `AISlaveMinerNumber`.
fn harvester_decision(facts: &HarvesterFacts) -> Option<i32> {
    let Some((harvester, harvester_tech)) = facts.harvester else {
        return (facts.gatherers < facts.slave_miners)
            .then_some(facts.refinery)
            .flatten()
            .flatten();
    };
    let limit = if facts.refinery.is_some() {
        facts
            .harvesters_per_refinery
            .wrapping_mul(facts.destinations)
    } else {
        facts.slave_miners
    };
    let admitted = facts.current_iq >= facts.iq_harvester
        && !facts.no_ore
        && !facts.human
        && facts.gatherers < limit
        && (harvester_tech as u32) <= (facts.tech_level as u32);
    admitted.then_some(harvester)
}

/// A type the house's teams need and the house may buy now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    /// The type's index in its class array.
    index: i32,
    /// Its outstanding need.
    need: i32,
    /// The creation frame (`TeamClass+0x50`) of the earliest team needing it.
    earliest: i32,
}

/// The house's candidates of `kind`'s class ([`tally`] over its teams and
/// objects).
fn candidates(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: UnitChoiceKind,
) -> Vec<Candidate> {
    let category = kind.category();
    let type_count = rules.type_array_ids(category).len();
    let type_index = |id: InternedId| {
        rules
            .type_array_index(category, sim.interner.resolve(id))
            .and_then(|index| usize::try_from(index).ok())
            .filter(|&index| index < type_count)
    };
    let vm = &sim.team_script_vm;
    let teams = vm
        .teams_in_order()
        .filter(|team| {
            team.owner() == owner
                && team
                    .team_type_id()
                    .is_some_and(|team_type| team.wants_members(vm.reinforce(team_type)))
        })
        .map(|team| {
            let needed = vm
                .needed_types(team, |id| {
                    sim.substrate
                        .entities
                        .get(id)
                        .map(crate::sim::team_script_vm::member_type_identity)
                })
                .into_iter()
                .filter(|member_type| member_type.category == category)
                .filter_map(|member_type| type_index(member_type.id))
                .collect();
            (team.created_frame(), needed)
        });
    let class = EntityCategory::from(category);
    // IsRecruitable refuses other houses' objects, so only `owner`'s are
    // tallied.
    let objects = sim
        .substrate
        .entities
        .values()
        .filter(|entity| entity.category() == class && entity.owner() == owner)
        .filter_map(|entity| Some((type_index(entity.type_ref())?, entity)));
    let money = crate::sim::credit_income::available_money(sim, owner);
    let class_type = |index: usize| rules.type_array_at(category, index as i32);
    tally(
        type_count,
        teams,
        objects,
        |entity| is_recruitable(sim, rules, entity, owner),
        money,
        |index| {
            class_type(index).map_or(CanBuild::No, |obj| {
                can_build(sim, rules, owner, obj, false, false)
            })
        },
        |index| class_type(index).map_or(i32::MAX, |obj| sim.cost_of(owner, obj, rules)),
    )
}

/// The tally and the candidate pass (`0x004FEBDC..0x004FEE3D`) over a class
/// of `type_count` types: each team, as its creation frame and the class
/// indexes of the types it still needs (`Get_Needed_Types`, in order), adds
/// to their need and lowers their earliest frame; each object, as its class
/// index, of a type still needed covers one need of it when `recruitable`
/// (`IsRecruitable`, asked only then: `0x004FED39..0x004FED4B`); then each
/// type in
/// index order with need left is asked `can_build` (`HouseClass::CanBuild`
/// with neither flag) and, unless it answers No, `cost_of` (`Cost_Of`): at
/// most `money` (`Available_Money`, signed), it is a candidate.
fn tally<T>(
    type_count: usize,
    teams: impl IntoIterator<Item = (i32, Vec<usize>)>,
    objects: impl IntoIterator<Item = (usize, T)>,
    mut recruitable: impl FnMut(T) -> bool,
    money: i32,
    mut can_build: impl FnMut(usize) -> CanBuild,
    mut cost_of: impl FnMut(usize) -> i32,
) -> Vec<Candidate> {
    let mut need = vec![0i32; type_count];
    let mut earliest = vec![i32::MAX; type_count];
    for (created, needed) in teams {
        for index in needed {
            need[index] = need[index].wrapping_add(1);
            earliest[index] = earliest[index].min(created);
        }
    }
    for (index, object) in objects {
        if need[index] > 0 && recruitable(object) {
            need[index] -= 1;
        }
    }
    (0..type_count)
        .filter(|&index| {
            need[index] > 0 && can_build(index) != CanBuild::No && cost_of(index) <= money
        })
        .map(|index| Candidate {
            index: index as i32,
            need: need[index],
            earliest: earliest[index],
        })
        .collect()
}

/// `FootClass::IsRecruitable @ 0x004DA230` for house `owner`: owned by it
/// (`+0x21C`), out of limbo (`+0x81`; VERA's dead await their removal, so
/// they count as in limbo, see `GameEntity::is_ai_alive`), in no team
/// (`+0x5D4`), on a mission (`Get_Mission`) MissionControl lets recruit
/// (`0x005B36E0`: none, -1, is recruitable), and its first scenario
/// recruit byte (`+0x421`) set.
fn is_recruitable(
    sim: &Simulation,
    rules: &RuleSet,
    entity: &GameEntity,
    owner: InternedId,
) -> bool {
    entity.owner() == owner
        && entity.is_ai_alive()
        && !entity.lifecycle.in_limbo()
        && sim
            .team_script_vm
            .team_for_member(entity.stable_id())
            .is_none()
        && crate::sim::mission::control::mission_recruitable(rules, entity.mission.effective())
        && entity.base_defense_response.recruitable_a()
}

/// The choice among `candidates` (`0x004FED6E..0x004FEECB`), with `draw` as
/// `RandomRanged` on the Scenario RNG.
///
/// A candidate whose need exceeds every earlier one's restarts the list; a
/// later one of lower need still joins it. The earliest team's type is the
/// first candidate of the lowest creation frame. The first draw,
/// `RandomRanged(0, 0x7FFFFFFE)`, always happens; in PC53/chop, `fill% *
/// 0.01 > r * (1 / 0x7FFFFFFE)` (`FCOMPP`, `TEST AH,0x41`) takes the earliest
/// team's type (none without candidates) with no second draw; otherwise a
/// non-empty list takes `list[RandomRanged(0, len - 1)]`.
fn pick(
    candidates: &[Candidate],
    fill_earliest_percent: i32,
    mut draw: impl FnMut(i32, i32) -> i32,
) -> Option<i32> {
    type X = X87Chop53;
    let mut most_needed: Vec<i32> = Vec::new();
    let mut best_need = -1;
    let mut earliest: Option<Candidate> = None;
    for &candidate in candidates {
        if best_need == -1 || best_need < candidate.need {
            best_need = candidate.need;
            most_needed.clear();
        }
        most_needed.push(candidate.index);
        if earliest.is_none_or(|first| candidate.earliest < first.earliest) {
            earliest = Some(candidate);
        }
    }
    let load = |bits| X::load_f64(bits).expect("the chooser scales are finite");
    let roll = X::mul(X::load_i32(draw(0, 0x7FFF_FFFE)), load(DRAW_SCALE));
    let fill = X::mul(X::load_i32(fill_earliest_percent), load(PERCENT_SCALE));
    if X::compare(fill, roll) == X87Ordering::Greater {
        return earliest.map(|candidate| candidate.index);
    }
    if most_needed.is_empty() {
        return None;
    }
    let slot = draw(0, most_needed.len() as i32 - 1);
    most_needed.get(usize::try_from(slot).ok()?).copied()
}

#[cfg(test)]
#[path = "ai_unit_choice_tests.rs"]
mod tests;
