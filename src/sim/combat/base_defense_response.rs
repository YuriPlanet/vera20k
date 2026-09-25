//! Exact arithmetic and six-slot selection for the House base-defence response.
//!
//! The receiver integration owns entity, House, Team and mission mutations.
//! This module keeps the native signed arithmetic and ranking pathology small
//! enough to prove independently before those authorities are borrowed.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::authority::queue_entity_mission_deferred;
use crate::sim::mission::concrete_effects::{
    assign_target_commits, represented_assign_target_admitted,
};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::rng::SimRng;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, distance_3d_leptons};

use super::TargetKind;

const RESPONSE_LIST_CAPACITY: usize = 6;
const FRAMES_PER_MINUTE: i32 = 900;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponderClass {
    Infantry,
    Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExistingTargetDisposition {
    NoneOrUnarmed,
    RequestedAttacker,
    OtherArmedTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ThreatFacts {
    pub(crate) cost: i32,
    pub(crate) speed_leptons_per_frame: i32,
    pub(crate) current_coord: [i32; 3],
    pub(crate) attacker_coord: [i32; 3],
    pub(crate) primary_range_leptons: i32,
    pub(crate) existing_target: ExistingTargetDisposition,
    pub(crate) in_non_base_defense_team: bool,
    pub(crate) mission_is_harvest: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RankedResponder {
    pub(crate) entity_id: u64,
    pub(crate) score: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResponseSelection {
    remaining_budget: i32,
    minimum_score: i32,
    responders: Vec<RankedResponder>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseMission {
    Rescue,
    AreaGuard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
#[allow(dead_code)]
pub(crate) enum ResponderPeekFireError {
    Clear = 0,
    Ammo = 1,
    Busy = 3,
    Illegal = 5,
    Cant = 6,
    MustDeploy = 8,
    Cloaked = 9,
}

pub(crate) struct BaseDefenseResponseContext<'a> {
    pub(crate) entities: &'a mut EntityStore,
    pub(crate) rules: &'a RuleSet,
    pub(crate) interner: &'a StringInterner,
    pub(crate) houses: &'a BTreeMap<InternedId, HouseState>,
    pub(crate) alliances: &'a HouseAllianceMap,
    pub(crate) scenario_rng: &'a mut SimRng,
    pub(crate) teams: &'a mut TeamScriptVm,
    pub(crate) zone_grid: Option<&'a ZoneGrid>,
    pub(crate) terrain: Option<&'a ResolvedTerrainGrid>,
    pub(crate) playfield_bounds: Option<PlayfieldBounds>,
    pub(crate) map_size_width: i32,
    pub(crate) map_size_height: i32,
    pub(crate) current_frame: i32,
    pub(crate) game_mode_nonzero: bool,
}

/// gamemd-derived: `TechnoClass__RespondToBaseAttack @ 0x00708080` checks the
/// attacker-owned `TechnoClass+0x650/+0x658` timer with signed wrapping frame
/// subtraction. Start `-1` is the inactive constructor sentinel.
pub(crate) fn cooldown_remaining(start_frame: i32, duration_frames: i32, now: i32) -> i32 {
    if start_frame == -1 {
        return 0;
    }
    let elapsed = now.wrapping_sub(start_frame);
    if elapsed < duration_frames {
        duration_frames.wrapping_sub(elapsed)
    } else {
        0
    }
}

/// Convert the Rules double through the native x87 `ftol` path. Invalid or
/// out-of-range values retain the x87 signed-indefinite low dword.
///
/// gamemd-derived: `TechnoClass__RespondToBaseAttack @ 0x0070878D` multiplies
/// `[General] BaseDefenseDelay` by the literal `900.0` before `Math__ftol`.
pub(crate) fn response_delay_frames(delay_minutes: f64) -> i32 {
    let loaded = X87Chop53::load_f64(NativeF64Bits::from_bits(delay_minutes.to_bits()));
    let Ok(delay) = loaded else {
        return i32::MIN;
    };
    X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
        delay,
        X87Chop53::load_i32(FRAMES_PER_MINUTE),
    ))
}

/// Exact signed threat score used only by the base-defence responder.
///
/// gamemd-derived: `FootClass__Evaluate_Target_Threat @ 0x004D97A0` uses the
/// current 3-D lepton coordinates, `CoordStruct__Distance3D`'s
/// `Sqrt_Approx/ftol`, signed wrapping `cost << 10`, and integer travel time.
pub(crate) fn evaluate_target_threat(facts: ThreatFacts) -> i32 {
    match facts.existing_target {
        ExistingTargetDisposition::RequestedAttacker => return facts.cost.wrapping_neg(),
        ExistingTargetDisposition::OtherArmedTarget => return 0,
        ExistingTargetDisposition::NoneOrUnarmed => {}
    }
    if facts.in_non_base_defense_team || facts.mission_is_harvest || facts.cost == 0 {
        return 0;
    }

    let distance = distance_3d_leptons(facts.current_coord, facts.attacker_coord);
    let base = facts.cost.wrapping_shl(10);
    if distance <= facts.primary_range_leptons {
        return base;
    }

    let speed = facts.speed_leptons_per_frame.max(1);
    let travel_frames = distance
        .wrapping_sub(facts.primary_range_leptons)
        .wrapping_div(speed)
        .max(1);
    base.wrapping_div(travel_frames).max(1)
}

fn class_adjusted_score(score: i32, class: ResponderClass, victim_is_self_anchor: bool) -> i32 {
    if !victim_is_self_anchor || score == 0 {
        return score;
    }
    match class {
        // The Infantry loop applies its self-anchor multiplier before the
        // negative-score budget branch.
        ResponderClass::Infantry => score.wrapping_mul(100),
        // The Unit loop reaches its multiplier only on the positive arm.
        ResponderClass::Unit if score > 0 => score.wrapping_mul(10),
        ResponderClass::Unit => score,
    }
}

impl ResponseSelection {
    pub(crate) fn new(budget: i32) -> Self {
        Self {
            remaining_budget: budget,
            minimum_score: 0,
            responders: Vec::with_capacity(RESPONSE_LIST_CAPACITY),
        }
    }

    #[cfg(test)]
    pub(crate) fn remaining_budget(&self) -> i32 {
        self.remaining_budget
    }

    pub(crate) fn can_scan(&self) -> bool {
        self.remaining_budget > 0
    }

    /// Apply the Infantry/Unit self-anchor multiplier, debit already-engaged
    /// defenders, and reproduce the native six-slot minimum bug.
    ///
    /// gamemd-derived: `TechnoClass__RespondToBaseAttack @
    /// 0x00708340..0x0070868F`. While the list grows, `minimum_score` remains
    /// zero. The first later positive candidate therefore only exposes the real
    /// minimum (it replaces no slot); subsequent replacements overwrite every
    /// equal-minimum slot with the same candidate, preserving duplicates.
    pub(crate) fn consider(
        &mut self,
        entity_id: u64,
        raw_score: i32,
        class: ResponderClass,
        victim_is_self_anchor: bool,
    ) {
        let score = class_adjusted_score(raw_score, class, victim_is_self_anchor);
        if score < 0 {
            self.remaining_budget = self.remaining_budget.wrapping_add(score);
            return;
        }
        if score == 0 {
            return;
        }
        if self.responders.len() < RESPONSE_LIST_CAPACITY {
            self.responders.push(RankedResponder { entity_id, score });
            return;
        }
        if score <= self.minimum_score {
            return;
        }

        let old_minimum = self.minimum_score;
        let mut replaced = [false; RESPONSE_LIST_CAPACITY];
        for (index, responder) in self.responders.iter_mut().enumerate() {
            if responder.score == old_minimum {
                *responder = RankedResponder { entity_id, score };
                replaced[index] = true;
            }
        }

        self.minimum_score = score;
        for (index, responder) in self.responders.iter().enumerate() {
            if !replaced[index] && responder.score < self.minimum_score {
                self.minimum_score = responder.score;
            }
        }
    }

    /// Stable signed descending sort: equal scores never exchange positions.
    pub(crate) fn into_ranked(mut self) -> (i32, Vec<RankedResponder>) {
        for upper in (1..self.responders.len()).rev() {
            for index in 0..upper {
                if self.responders[index].score < self.responders[index + 1].score {
                    self.responders.swap(index, index + 1);
                }
            }
        }
        (self.remaining_budget, self.responders)
    }
}

/// Every selected occurrence consumes the draw. Team membership changes only
/// the interpretation, never whether the draw occurs.
///
/// gamemd-derived: `TechnoClass__RespondToBaseAttack @
/// 0x007086DB..0x00708718` queues Rescue for `0..=65`, else Area Guard; a
/// base-defence Team member is forced to Area Guard after the draw.
pub(crate) fn response_mission(draw_0_to_99: u32, in_base_defense_team: bool) -> ResponseMission {
    debug_assert!(draw_0_to_99 <= 99);
    if !in_base_defense_team && draw_0_to_99 <= 65 {
        ResponseMission::Rescue
    } else {
        ResponseMission::AreaGuard
    }
}

/// The assignment loop stops only after its signed wrapping cost sum strictly
/// exceeds the post-scan budget; equality deliberately continues.
pub(crate) fn add_assigned_cost(accumulated: i32, cost: i32, budget: i32) -> (i32, bool) {
    let accumulated = accumulated.wrapping_add(cost);
    (accumulated, accumulated > budget)
}

mod admission;

use super::combat_weapon::is_armed;
use admission::{
    candidate_admitted, current_target_disposition, destination_cell, entity_coord,
    primary_range_leptons, should_be_on_bridge_for_response,
};

/// Execute one complete native response transaction after the receiver owner
/// has selected the exact Building or protected-Techno call site.
///
/// gamemd-derived: `TechnoClass__RespondToBaseAttack @ 0x00708080`.
pub(crate) fn respond_to_base_attack(
    victim_id: u64,
    attacker_id: u64,
    context: &mut BaseDefenseResponseContext<'_>,
) {
    let Some(victim) = context.entities.get(victim_id) else {
        return;
    };
    let Some(attacker) = context.entities.get(attacker_id) else {
        return;
    };
    let Some(victim_object) = context
        .rules
        .object(context.interner.resolve(victim.type_ref()))
    else {
        return;
    };
    let Some(attacker_object) = context
        .rules
        .object(context.interner.resolve(attacker.type_ref()))
    else {
        return;
    };
    let victim_owner = victim.owner();
    let attacker_owner = attacker.owner();
    let budget = attacker_object
        .cost
        .wrapping_mul(context.rules.general.computer_base_defense_response);

    if is_allied_with(
        context.alliances,
        context.interner.resolve(victim_owner),
        context.interner.resolve(attacker_owner),
    ) || context
        .houses
        .get(&victim_owner)
        .is_some_and(|house| house.is_human)
        || attacker.lifecycle.in_limbo
        // `0x00708114`: `[g_GameMode 0x00A8B238] == 0 && this->Is_Armed
        // (vt+0x2AC)` — an armed victim in that game mode defends itself
        // instead of calling for help.
        || (!context.game_mode_nonzero && is_armed(victim, victim_object))
        || !matches!(
            attacker.category,
            EntityCategory::Unit | EntityCategory::Infantry
        )
        || victim_object.insignificant
        || cooldown_remaining(
            attacker.base_defense_response.cooldown_start_frame,
            attacker.base_defense_response.cooldown_duration_frames,
            context.current_frame,
        ) != 0
    {
        return;
    }

    // Rust's map caches are derived rather than native globals. A live positive
    // scan cannot be exact without both authorities; fail atomically before the
    // native Team suspension point instead of partially mutating the response.
    if budget > 0 && (context.zone_grid.is_none() || context.terrain.is_none()) {
        return;
    }

    context.teams.suspend_teams_for_base_defense(
        victim_owner,
        context.rules.general.suspend_priority,
        context.current_frame,
        response_delay_frames(context.rules.general.suspend_delay_minutes),
    );
    let mut selection = ResponseSelection::new(budget);
    if !selection.can_scan() {
        return;
    }

    let attacker_coord = entity_coord(
        context
            .entities
            .get(attacker_id)
            .expect("entry retained attacker"),
        context.terrain,
    );
    let victim_is_self_anchor = context
        .entities
        .get(victim_id)
        .and_then(|victim| victim.archive_target())
        == Some(TargetKind::Entity(victim_id));
    let candidate_ids = context.entities.keys_sorted();

    for class in [ResponderClass::Infantry, ResponderClass::Unit] {
        for &candidate_id in &candidate_ids {
            if !selection.can_scan() {
                break;
            }
            let expected_category = match class {
                ResponderClass::Infantry => EntityCategory::Infantry,
                ResponderClass::Unit => EntityCategory::Unit,
            };
            let Some(candidate) = context.entities.get(candidate_id) else {
                continue;
            };
            if candidate.category != expected_category {
                continue;
            }
            let Some(candidate_object) = context
                .rules
                .object(context.interner.resolve(candidate.type_ref()))
            else {
                continue;
            };
            let victim = context
                .entities
                .get(victim_id)
                .expect("entry retained victim");
            if !candidate_admitted(
                candidate,
                candidate_object,
                victim,
                victim_owner,
                attacker_id,
                context,
            ) {
                continue;
            }

            let destination = destination_cell(candidate, context.entities, context.terrain);
            let victim_destination = destination_cell(victim, context.entities, context.terrain);
            let terrain = context.terrain.expect("positive scan validated terrain");
            let Some(source_should_be_on_bridge) =
                should_be_on_bridge_for_response(candidate, context.entities, terrain)
            else {
                continue;
            };
            let source_in_playfield = cell_is_in_playfield_height_aware(
                destination,
                context.playfield_bounds,
                Some(terrain),
            );
            let movement_zone = (candidate_object.movement_zone != MovementZone::Invalid)
                .then_some(candidate_object.movement_zone);
            if !context
                .zone_grid
                .expect("positive scan validated zone grid")
                .can_reach_base_defense_response(
                    movement_zone,
                    destination,
                    victim_destination,
                    source_should_be_on_bridge,
                    source_in_playfield,
                    context.map_size_width,
                    context.map_size_height,
                )
            {
                continue;
            }

            let raw_score = evaluate_target_threat(ThreatFacts {
                cost: candidate_object.cost,
                speed_leptons_per_frame: candidate_object.speed,
                current_coord: entity_coord(candidate, context.terrain),
                attacker_coord,
                primary_range_leptons: primary_range_leptons(
                    candidate,
                    candidate_object,
                    context.rules,
                ),
                existing_target: current_target_disposition(
                    candidate,
                    attacker_id,
                    context.entities,
                    context.rules,
                    context.interner,
                ),
                in_non_base_defense_team: context
                    .teams
                    .team_for_member(candidate_id)
                    .is_some_and(|(_, is_base_defense)| !is_base_defense),
                mission_is_harvest: candidate.mission.current().known()
                    == Some(MissionType::Harvest),
            });
            selection.consider(candidate_id, raw_score, class, victim_is_self_anchor);
        }
    }

    let (budget, responders) = selection.into_ranked();
    if budget <= 0 {
        return;
    }

    let mut accumulated = 0;
    for responder in responders {
        let in_base_defense_team = context
            .teams
            .team_for_member(responder.entity_id)
            .is_some_and(|(_, is_base_defense)| is_base_defense);
        let draw = context.scenario_rng.next_range_u32_inclusive(0, 99);
        let mission = match response_mission(draw, in_base_defense_team) {
            ResponseMission::Rescue => MissionType::Rescue,
            ResponseMission::AreaGuard => MissionType::AreaGuard,
        };
        let attacker_commits =
            assign_target_commits(context.entities, Some(TargetKind::Entity(attacker_id)));
        let Some(responder_entity) = context.entities.get_mut(responder.entity_id) else {
            continue;
        };
        let Some(responder_object) = context
            .rules
            .object(context.interner.resolve(responder_entity.type_ref()))
        else {
            continue;
        };
        queue_entity_mission_deferred(responder_entity, MissionId::from_known(mission));
        responder_entity.set_archive_target(Some(TargetKind::Entity(victim_id)));
        represented_assign_target_admitted(
            responder_entity,
            Some(TargetKind::Entity(attacker_id)),
            attacker_commits,
        );
        let (next, overshot) = add_assigned_cost(accumulated, responder_object.cost, budget);
        accumulated = next;
        if overshot {
            if let Some(attacker) = context.entities.get_mut(attacker_id) {
                attacker.base_defense_response.cooldown_start_frame = context.current_frame;
                attacker.base_defense_response.cooldown_duration_frames =
                    response_delay_frames(context.rules.general.base_defense_delay_minutes);
            }
            break;
        }
    }
}

#[cfg(test)]
#[path = "base_defense_response_tests.rs"]
mod tests;
