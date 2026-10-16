//! The House base-defence response (`TechnoClass::RespondToBaseAttack @
//! 0x00708080`): its exact signed arithmetic, six-slot ranking pathology and
//! the transaction that recruits, queues and debits in native order.
//!
//! Its admission asks each candidate's GetFireError through the one owner
//! ([`super::fire_error_world::FireSubject`]), so the scan reads the whole
//! world while the transaction's mutations come before and after it.
//! Coordinate/layer/zone source: tools/spatial_oracle/foot_bridge_layer.{json,md},
//! original70829C..70833C and7084F4..708594. Shared movement owners implement
//! Foot4DBDF0/4DDC40 and Map56D100; the response never selects NavCom instead.

use crate::map::entities::EntityCategory;
use crate::map::houses::is_allied_with;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::queue_entity_mission_deferred;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose;
use crate::sim::pathfinding::zone_map::ZoneQueryCell;
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::fixed_math::ra2_speed_to_leptons_per_frame;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, object_distance};

use super::TargetKind;
use super::threat_posed::live_threat_posed;

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
    pub(crate) threat_posed: i32,
    pub(crate) speed_leptons_per_frame: i32,
    pub(crate) current_coord: [i32; 3],
    pub(crate) attacker_coord: Option<[i32; 3]>,
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
/// current 3-D ObjectDistance5F6360's `Sqrt_Approx/ftol`, wrapping signed
/// distance-range and ThreatPosed<<10, then signed integer travel time.
/// Executed control rows: spatial_oracle/base_defense_response.json scoring.
pub(crate) fn evaluate_target_threat(facts: ThreatFacts) -> i32 {
    let Some(attacker_coord) = facts.attacker_coord else {
        return 0;
    };
    match facts.existing_target {
        ExistingTargetDisposition::RequestedAttacker => return facts.threat_posed.wrapping_neg(),
        ExistingTargetDisposition::OtherArmedTarget => return 0,
        ExistingTargetDisposition::NoneOrUnarmed => {}
    }
    if facts.in_non_base_defense_team || facts.mission_is_harvest || facts.threat_posed == 0 {
        return 0;
    }

    // The attacker is Infantry/Unit, so ObjectDistance has no Building
    // foundation discount on this caller's reachable path.
    let distance = object_distance(facts.current_coord, attacker_coord, None)
        .wrapping_sub(facts.primary_range_leptons);
    let base = facts.threat_posed.wrapping_shl(10);
    if distance <= 0 {
        return base;
    }

    // Original4D9868 is UNSIGNED JBE. The signed nonpositive branch follows
    // the second getter. Ordinary type readers yield0..255; preserve the
    // supplied negative-speed control too rather than treating it as speed1.
    let travel_frames = if facts.speed_leptons_per_frame as u32 <= 1 {
        distance
    } else if facts.speed_leptons_per_frame > 0 {
        distance.wrapping_div(facts.speed_leptons_per_frame).max(1)
    } else {
        1
    };
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

    #[cfg(any(test, feature = "test-support"))]
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
        //70834D/7085A9 reject the raw zero BEFORE anchor multiplication.
        // A nonzero Infantry score can wrap to zero at708359..70835F and
        // still enter a slot;708364 branches on signed nonnegative.
        if raw_score == 0 {
            return;
        }
        let score = class_adjusted_score(raw_score, class, victim_is_self_anchor);
        if score < 0 {
            self.remaining_budget = self.remaining_budget.wrapping_add(score);
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

    /// Native708647..7086AF compares each slot with every later slot and
    /// exchanges on signed less-than. A higher score displaces earlier ties.
    pub(crate) fn into_ranked(mut self) -> (i32, Vec<RankedResponder>) {
        for index in 0..self.responders.len() {
            for later in index + 1..self.responders.len() {
                if self.responders[index].score < self.responders[later].score {
                    self.responders.swap(index, later);
                }
            }
        }
        (self.remaining_budget, self.responders)
    }
    /// Original708647..7087A8: sort the selected occurrences, then queue,
    /// archive, assign and reread live ThreatPosed in that order. This is the
    /// production transaction, also replayed by the native dispatch corpus.
    /// The returned local sum is diagnostic; it adds no retained state.
    fn dispatch(
        self,
        world: &mut Simulation,
        rules: &RuleSet,
        victim_id: u64,
        attacker_id: u64,
    ) -> i32 {
        let current_frame = world.session.binary_frame as i32;
        let (budget, responders) = self.into_ranked();
        if budget <= 0 {
            return 0;
        }

        let mut accumulated = 0;
        for responder in responders {
            let in_base_defense_team = world
                .team_script_vm
                .team_for_member(responder.entity_id)
                .is_some_and(|(_, is_base_defense)| is_base_defense);
            let draw = world.scenario_rng.next_range_u32_inclusive(0, 99);
            let mission = match response_mission(draw, in_base_defense_team) {
                ResponseMission::Rescue => MissionType::Rescue,
                ResponseMission::AreaGuard => MissionType::AreaGuard,
            };
            let Some(responder_entity) = world.substrate.entities.get_mut(responder.entity_id)
            else {
                continue;
            };
            queue_entity_mission_deferred(responder_entity, MissionId::from_known(mission));
            responder_entity.set_archive_target(Some(TargetKind::Entity(victim_id)));
            world
                .assign_target_represented(
                    responder.entity_id,
                    Some(TargetKind::Entity(attacker_id)),
                    Some(rules),
                    None,
                    FrameEffects::default(),
                )
                .expect("selected responder remains present during synchronous target assignment");
            let Some(responder_entity) = world.substrate.entities.get(responder.entity_id) else {
                continue;
            };
            let threat = live_threat_posed(
                responder_entity,
                world
                    .interner
                    .object_type(responder_entity.type_ref(), rules),
                &world.substrate.entities,
                rules,
                &world.interner,
            );
            let (next, overshot) = add_assigned_threat(accumulated, threat, budget);
            accumulated = next;
            if overshot {
                if let Some(attacker) = world.substrate.entities.get_mut(attacker_id) {
                    attacker.base_defense_response.cooldown.start(
                        current_frame,
                        response_delay_frames(rules.general.base_defense_delay_minutes),
                    );
                }
                break;
            }
        }
        accumulated
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

/// The assignment loop stops only after its signed wrapping ThreatPosed sum strictly
/// exceeds the post-scan budget; equality deliberately continues.
pub(crate) fn add_assigned_threat(accumulated: i32, threat: i32, budget: i32) -> (i32, bool) {
    let accumulated = accumulated.wrapping_add(threat);
    (accumulated, accumulated > budget)
}

mod admission;

use super::combat_weapon::is_armed;
use admission::{
    candidate_admitted, current_target_disposition, destination_cell, primary_range_leptons,
};

/// Persistent TechnoClass state used by the active House base-defence
/// responder. The two recruitment admission bytes are constructor-true; an
/// authored object's scenario line ([`Self::author_recruitable`]) and
/// `TeamClass::Add_Member` ([`Self::set_team_recruitable`]) write them. Cooldown
/// writes occur only after a responder assignment or strict budget overshoot. The
/// ArchiveTarget (`+0x218`) stored here is the general Techno field, which
/// responders, miners, the slave manager and the rally click all write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct BaseDefenseResponseState {
    /// `Techno+0x421`, RecruitableA.
    recruitable_a: bool,
    /// `Techno+0x422`, RecruitableB.
    recruitable_b: bool,
    archive_target: Option<TargetKind>,
    /// `TechnoClass+0x650`/`+0x658`: the attacker's call-for-help cooldown
    /// (`TechnoClass::RespondToBaseAttack @ 0x00708171..0x00708192`, written
    /// at `0x0070879E..0x007087A9`); the constructor leaves it at `-1`, 0.
    cooldown: CdTimer,
}

impl Default for BaseDefenseResponseState {
    fn default() -> Self {
        Self {
            recruitable_a: true,
            recruitable_b: true,
            archive_target: None,
            cooldown: CdTimer::from_raw(-1, 0),
        }
    }
}

impl BaseDefenseResponseState {
    /// `Techno+0x421`, RecruitableA.
    pub(crate) fn recruitable_a(&self) -> bool {
        self.recruitable_a
    }

    /// `Techno+0x422`, RecruitableB.
    pub(crate) fn recruitable_b(&self) -> bool {
        self.recruitable_b
    }

    /// The scenario line's two recruitment bytes (Unit and Infantry trailing
    /// fields 12 and 13, Aircraft 10 and 11), stored by the authored object's
    /// construction.
    pub(crate) fn author_recruitable(&mut self, recruitable_a: bool, recruitable_b: bool) {
        self.recruitable_a = recruitable_a;
        self.recruitable_b = recruitable_b;
    }

    /// `TeamClass::Add_Member @ 0x006EA500`'s store of the team type's
    /// `AreTeamMembersRecruitable` into `+0x422`.
    pub(crate) fn set_team_recruitable(&mut self, recruitable: bool) {
        self.recruitable_b = recruitable;
    }

    /// The call-for-help cooldown; outside tests only
    /// `respond_to_base_attack` reads and starts it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn cooldown(&self) -> CdTimer {
        self.cooldown
    }

    /// ArchiveTarget (`+0x218`), read through `GameEntity::archive_target`.
    pub(crate) fn archive_target(&self) -> Option<TargetKind> {
        self.archive_target
    }

    /// `TechnoClass::Set_ArchiveTarget @ 0x0070C610`'s store, through
    /// `GameEntity::set_archive_target`.
    pub(crate) fn set_archive_target(&mut self, target: Option<TargetKind>) {
        self.archive_target = target;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_recruitable_a_for_test(&mut self, recruitable: bool) {
        self.recruitable_a = recruitable;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_recruitable_b_for_test(&mut self, recruitable: bool) {
        self.recruitable_b = recruitable;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_cooldown_for_test(&mut self, cooldown: CdTimer) {
        self.cooldown = cooldown;
    }
}

/// Execute one complete native response transaction after the receiver owner
/// has selected the exact Building or protected-Techno call site.
///
/// gamemd-derived: `TechnoClass__RespondToBaseAttack @ 0x00708080`.
pub(crate) fn respond_to_base_attack(
    world: &mut Simulation,
    rules: &RuleSet,
    victim_id: u64,
    attacker_id: u64,
    frame_effects: FrameEffects<'_>,
) {
    let entities = &world.substrate.entities;
    let interner = &world.interner;
    let Some(victim) = entities.get(victim_id) else {
        return;
    };
    let Some(attacker) = entities.get(attacker_id) else {
        return;
    };
    let Some(victim_object) = interner.object_type(victim.type_ref(), rules) else {
        return;
    };
    let Some(attacker_object) = interner.object_type(attacker.type_ref(), rules) else {
        return;
    };
    let victim_owner = victim.owner();
    let attacker_owner = attacker.owner();
    let budget = live_threat_posed(attacker, Some(attacker_object), entities, rules, interner)
        .wrapping_mul(rules.general.computer_base_defense_response);
    let current_frame = world.session.binary_frame as i32;

    if is_allied_with(
        &world.house_alliances,
        interner.resolve(victim_owner),
        interner.resolve(attacker_owner),
    ) || world
        .houses
        .get(&victim_owner)
        .is_some_and(|house| house.is_human)
        || attacker.lifecycle.in_limbo()
        // `0x00708114`: `[g_GameMode 0x00A8B238] == 0 && this->Is_Armed
        // (vt+0x2AC)` — an armed victim in that game mode defends itself
        // instead of calling for help.
        || (!world.session.game_mode_nonzero && is_armed(victim, victim_object))
        || !matches!(
            attacker.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        )
        || victim_object.insignificant
        || !attacker
            .base_defense_response
            .cooldown
            .expired(current_frame)
    {
        return;
    }

    // Rust's map caches are derived rather than native globals. A live positive
    // scan cannot be exact without both authorities; fail atomically before the
    // native Team suspension point instead of partially mutating the response.
    if budget > 0
        && (world.zone_grid.is_none()
            || world.resolved_terrain.is_none()
            || world.playfield_bounds.is_none()
            || world.playfield_size_height.is_none())
    {
        return;
    }

    world.suspend_teams_for_base_defense(
        victim_owner,
        rules.general.suspend_priority,
        response_delay_frames(rules.general.suspend_delay_minutes),
        rules,
        frame_effects,
    );
    let mut selection = ResponseSelection::new(budget);
    if !selection.can_scan() {
        return;
    }

    // Admission retains native lazy map-query effects on shared Dummy. Entity
    // and mission mutations follow the scan.
    let sim: &Simulation = world;
    let entities = &sim.substrate.entities;
    let (Some(victim), Some(attacker)) = (entities.get(victim_id), entities.get(attacker_id))
    else {
        return;
    };
    let attacker_coord = ground_pose::object_get_coords(attacker, sim.resolved_terrain.as_ref());
    let attacker_coord = [attacker_coord.x, attacker_coord.y, attacker_coord.z];
    let victim_is_self_anchor = victim.archive_target() == Some(TargetKind::Entity(victim_id));
    let candidate_ids = entities.keys_sorted();
    for class in [ResponderClass::Infantry, ResponderClass::Unit] {
        for &candidate_id in &candidate_ids {
            if !selection.can_scan() {
                break;
            }
            let expected_category = match class {
                ResponderClass::Infantry => EntityCategory::Infantry,
                ResponderClass::Unit => EntityCategory::Unit,
            };
            let Some(candidate) = entities.get(candidate_id) else {
                continue;
            };
            if candidate.category() != expected_category {
                continue;
            }
            let Some(candidate_object) = sim.interner.object_type(candidate.type_ref(), rules)
            else {
                continue;
            };
            if !candidate_admitted(sim, rules, candidate, candidate_object, victim, attacker_id) {
                continue;
            }

            let terrain = sim
                .resolved_terrain
                .as_ref()
                .expect("positive scan validated terrain");
            // Original70829C/7082DB (Infantry) and7084F4/708533 (Unit)
            // resolve the victim before the candidate. Source-layer queries
            // use the same Foot/ground owners as movement, never NavCom.
            let Ok(victim_destination) =
                destination_cell(victim, entities, Some(terrain), rules, &sim.interner)
            else {
                continue;
            };
            let Ok(destination) =
                destination_cell(candidate, entities, Some(terrain), rules, &sim.interner)
            else {
                continue;
            };
            let cells = NativeCellQuery::canonical(terrain);
            let current = ground_pose::position_world_coord(&candidate.position);
            let in_tube = candidate.low_bridge_tube_state.is_some();
            // Foot4DDC40 short-circuits before virtual+4C in a Tube.
            let navigation = if in_tube {
                current
            } else {
                match sim.foot_navigation_coordinate(candidate_id) {
                    Ok(coord) => coord,
                    Err(_) => continue,
                }
            };
            let Ok(source_should_be_on_bridge) = ground_pose::navigation_should_be_on_bridge(
                &cells,
                navigation,
                current,
                candidate.on_bridge(),
                in_tube,
            ) else {
                continue;
            };
            let bounds = sim
                .playfield_bounds
                .expect("positive scan validated bounds");
            let size = (
                bounds.base,
                sim.playfield_size_height
                    .expect("positive scan validated Size"),
            );
            if !sim
                .zone_grid
                .as_ref()
                .expect("positive scan validated zone grid")
                .can_reach_native(
                    &cells,
                    ZoneQueryCell::Copied((destination.0 as i16, destination.1 as i16)),
                    ZoneQueryCell::Copied((
                        victim_destination.0 as i16,
                        victim_destination.1 as i16,
                    )),
                    candidate_object.movement_zone,
                    source_should_be_on_bridge,
                    false,
                    false,
                    bounds,
                    size,
                )
                .unwrap_or(false)
            {
                continue;
            }

            let current_coord = ground_pose::object_get_coords(candidate, Some(terrain));
            let raw_score = evaluate_target_threat(ThreatFacts {
                threat_posed: live_threat_posed(
                    candidate,
                    Some(candidate_object),
                    entities,
                    rules,
                    &sim.interner,
                ),
                //4D985F/4D986E read virtual+38C=70EFE0, the retained
                // type Speed+678. The shared reader conversion supplies its
                // integer budget; Foot4DB1A0's live fractions are a different
                // getter and do not enter this score.
                speed_leptons_per_frame: ra2_speed_to_leptons_per_frame(candidate_object.speed),
                current_coord: [current_coord.x, current_coord.y, current_coord.z],
                attacker_coord: Some(attacker_coord),
                primary_range_leptons: primary_range_leptons(
                    candidate,
                    candidate_object,
                    entities,
                    rules,
                    &sim.interner,
                ),
                existing_target: current_target_disposition(
                    candidate,
                    attacker_id,
                    entities,
                    rules,
                    &sim.interner,
                ),
                in_non_base_defense_team: sim
                    .team_script_vm
                    .team_for_member(candidate_id)
                    .is_some_and(|(_, is_base_defense)| !is_base_defense),
                mission_is_harvest: candidate.mission.current().known()
                    == Some(MissionType::Harvest),
            });
            if raw_score == 0 {
                continue;
            }
            // Original7085A4/7085A9 evaluate threat first;7085AD then
            // refuses a Unit for a slave victim. Earlier rejection would
            // lose this candidate's shared-Dummy coordinate/layer/zone writes.
            if class == ResponderClass::Unit && victim.slave.owner().is_some() {
                continue;
            }
            selection.consider(candidate_id, raw_score, class, victim_is_self_anchor);
        }
    }

    selection.dispatch(world, rules, victim_id, attacker_id);
}

#[cfg(test)]
#[path = "base_defense_response_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "base_defense_response_oracle_tests.rs"]
mod oracle_tests;
