//! Persistent campaign discipline: contain, grow, commit one fist, then refit.
//!
//! The native game continues to own movement and combat. This director only
//! decides when the main Attack squad may be *spent*. Scouts, harvesters,
//! harassers and defenders keep their existing jobs while the fist accumulates.
//! Observations come exclusively from the current player's visible enemies
//! and its own tracked economic/army ledger; absence from fog is not proof an
//! opponent has run out of money or stopped mining.

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;

use super::census::Census;
use super::strategy::Signals;
use super::{squads, squared_distance};

const INTEL_LIFETIME: u64 = 5_400;
const INTEL_SAMPLE_FRAMES: u64 = 90;
const CONTAIN_FRAMES: u64 = 180;
const STOCKPILE_LIMIT: u64 = 3_600;
const COMMIT_FRAMES: u64 = 1_350;
const REFIT_FRAMES: u64 = 600;
const MIN_COMMIT_FIGHTERS: usize = 7;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Contest,
    Contain,
    Stockpile,
    Commit,
    Refit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Route {
    #[default]
    Mass,
    Quality,
}

#[derive(Debug, Clone, Copy)]
struct OpponentSightings {
    last_seen: u64,
    last_miner_seen: Option<u64>,
    last_defense_seen: Option<u64>,
}

/// Pure strategic facts assembled from the player's observation, not the
/// simulation's unrevealed enemy owner, income or production queues.
#[derive(Debug, Clone, Copy, Default)]
struct Picture {
    has_known_opponent: bool,
    economy_works: bool,
    economy_surplus: bool,
    emergency: bool,
    attack_members: usize,
    coherent_members: usize,
    attack_points: i32,
    live_batteries: usize,
    can_standoff: bool,
    advanced_available: bool,
}

fn commitment_size(picture: Picture) -> usize {
    let base = if picture.live_batteries == 0 {
        11
    } else {
        14usize.saturating_add(picture.live_batteries.min(4) * 3)
    };
    let with_artillery = if picture.can_standoff { base.saturating_sub(4) } else { base };
    // A stranded economy must be allowed to contest territory with a smaller
    // fist. A wealthy house is allowed to wait for a real decisive force.
    if !picture.economy_surplus { with_artillery.min(12) } else { with_artillery.min(28) }
}

fn can_commit(picture: Picture, wanted: usize, waited: u64) -> bool {
    if picture.attack_members < MIN_COMMIT_FIGHTERS || picture.coherent_members < MIN_COMMIT_FIGHTERS {
        return false;
    }
    (picture.attack_members >= wanted && picture.coherent_members >= wanted.saturating_sub(2)
        && picture.attack_points >= 2_500)
        || (waited >= STOCKPILE_LIMIT && picture.coherent_members >= MIN_COMMIT_FIGHTERS
            && picture.attack_points >= 2_500)
}

#[derive(Debug, Default)]
pub(super) struct Campaign {
    phase: Phase,
    route: Route,
    since: u64,
    initialized: bool,
    wanted: usize,
    committed_points: i32,
    observed_at: Option<u64>,
    sightings: BTreeMap<InternedId, OpponentSightings>,
}

impl Campaign {
    pub(super) fn phase(&self) -> Phase { self.phase }
    pub(super) fn route(&self) -> Route { self.route }
    pub(super) fn wanted(&self) -> usize { self.wanted }

    /// Only the main battle fist is held. The Harass/Defend/Scout squads are
    /// deliberately unaffected, and a true raid on the base can preempt us.
    pub(super) fn holding_fist(&self) -> bool {
        matches!(self.phase, Phase::Contain | Phase::Stockpile | Phase::Refit)
    }

    pub(super) fn prefer_quality(&self) -> bool {
        matches!(self.phase, Phase::Stockpile | Phase::Commit)
            && self.route == Route::Quality
    }

    /// The campaign chooses *whether* to spend the army, not how the tactical
    /// advance should handle artillery, power cuts, or dangerous batteries.
    pub(super) fn observe(&mut self, now: u64, rules: &RuleSet,
        obs: &Observation<'_>, census: &Census, teams: &squads::Squads,
        signals: Signals, strategic_emergency: bool, remembered_target: Option<(u16,u16)>,
    ) -> Option<(Phase, Phase)> {
        // The economy/defence director already samples urgent raids every
        // frame. Do not make a 120-house match walk the visible enemy store,
        // factories and combat arsenal again at 15Hz. A strategic decision
        // can wait 90 frames; an emergency cannot.
        if strategic_emergency || signals.raiders_visible > 0
            || signals.yards == 0 || signals.refineries == 0 || signals.harvesters == 0
        {
            let old = self.phase;
            self.phase = Phase::Contest;
            self.since = now;
            self.initialized = true;
            self.committed_points = 0;
            return (old != Phase::Contest).then_some((old, Phase::Contest));
        }
        if self.observed_at.is_some_and(|at|
            now.saturating_sub(at) < INTEL_SAMPLE_FRAMES)
        { return None; }
        self.observed_at = Some(now);
        {
            for enemy in obs.visible_enemy_entities() {
                let Some(house) = obs.visible_enemy_house(enemy.entity_id) else { continue };
                let Some(ty) = obs.object_type(rules, enemy.type_id) else { continue };
                let entry = self.sightings.entry(house).or_insert(OpponentSightings {
                    last_seen: now, last_miner_seen: None, last_defense_seen: None,
                });
                entry.last_seen = now;
                if ty.harvester || ty.resource_gatherer || ty.enslaves.is_some() {
                    entry.last_miner_seen = Some(now);
                }
                if ty.is_base_defense && obs.visible_enemy_building_operational(rules, enemy.entity_id) {
                    entry.last_defense_seen = Some(now);
                }
            }
            self.sightings.retain(|_, intel| now.saturating_sub(intel.last_seen) < INTEL_LIFETIME);
        }

        let attack = teams.squad(squads::Role::Attack);
        let fighters: Vec<_> = attack.into_iter().flat_map(|team| census.units.iter()
            .filter(move |unit| team.members.contains(&unit.entity_id)
                && unit.traits.combat && unit.controllable()
                && !unit.traits.yard_maker && !unit.traits.harvester)).collect();
        let attack_points = fighters.iter().fold(0i32, |sum, unit| sum.saturating_add(unit.points));
        // One linear pass for the formation; don't compare every fighter with
        // every other fighter on the 120-player maps. If the attack group is
        // dispersed, do not turn an all-map roster count into permission to
        // attack a tower with the leading tank.
        let centre = remembered_target.and_then(|target| fighters.iter()
            .min_by_key(|unit| squared_distance(unit.cell, target))
            .map(|unit| unit.cell)).or_else(|| fighters.first().map(|unit| unit.cell));
        let coherent_members = centre.map_or(0, |centre| fighters.iter().filter(|unit|
            squared_distance(unit.cell, centre) <= 20 * 20).count());

        let mut live_batteries = 0usize;
        let mut can_standoff = false;
        for enemy in obs.visible_enemy_entities() {
            let Some(ty) = obs.object_type(rules, enemy.type_id) else { continue };
            if !ty.is_base_defense
                || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
                || remembered_target.is_some_and(|front|
                    squared_distance(front, enemy.cell) > 28 * 28)
            { continue; }
            let threat_reach = super::advance::weapon_range_cells(rules, ty, false);
            if threat_reach < 7 { continue; }
            live_batteries += 1;
            if !can_standoff && live_batteries <= 8 {
                can_standoff = fighters.iter().take(96).any(|unit|
                    obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                        && super::tactics::effective_reach_against(rules, obs,
                            unit.entity_id, enemy.entity_id) > threat_reach + 1);
            }
        }
        // An advanced line exists only when the current rule set actually
        // permits it. This is a production route, not a list of retail IDs.
        let line_price = super::economy::line_vehicle_floor(rules, obs).unwrap_or(1_000);
        let advanced_available = super::build::class_types(rules, obs, ObjectCategory::Vehicle)
            .any(|(ty, _)| !ty.naval && super::orders::is_combat_unit(rules, ty)
                && obs.cost_of(rules, ty) >= line_price.saturating_mul(3) / 2
                && ty.weapon_record_count() > 0);

        let picture = Picture {
            has_known_opponent: self.sightings.values().any(|intel|
                now.saturating_sub(intel.last_seen) <= 1_800),
            economy_works: signals.yards > 0 && signals.refineries > 0
                && signals.harvesters > 0,
            economy_surplus: signals.harvesters >= 2
                && (signals.income_per_minute.is_some_and(|income| income >= 1_200)
                    || signals.credits >= 7_000),
            emergency: strategic_emergency || signals.raiders_visible > 0,
            attack_members: fighters.len(), coherent_members, attack_points,
            live_batteries, can_standoff, advanced_available,
        };
        self.update(now, picture)
    }

    fn update(&mut self, now: u64, p: Picture) -> Option<(Phase, Phase)> {
        self.wanted = commitment_size(p);
        self.route = if p.advanced_available && p.economy_surplus {
            Route::Quality
        } else { Route::Mass };
        let current = self.phase;
        let elapsed = now.saturating_sub(self.since);
        let next = if !p.economy_works || p.emergency || !p.has_known_opponent {
            Phase::Contest
        } else {
            match current {
                Phase::Contest => {
                    if p.attack_members >= MIN_COMMIT_FIGHTERS { Phase::Contain }
                    else { Phase::Contest }
                }
                Phase::Contain => {
                    if elapsed >= CONTAIN_FRAMES { Phase::Stockpile }
                    else { Phase::Contain }
                }
                Phase::Stockpile => {
                    if can_commit(p, self.wanted, elapsed) { Phase::Commit }
                    else { Phase::Stockpile }
                }
                Phase::Commit => {
                    if elapsed >= COMMIT_FRAMES || (self.committed_points >= 2_500
                        && p.attack_points.saturating_mul(2) < self.committed_points)
                    { Phase::Refit } else { Phase::Commit }
                }
                Phase::Refit => {
                    if elapsed >= REFIT_FRAMES { Phase::Stockpile }
                    else { Phase::Refit }
                }
            }
        };
        if !self.initialized || next != current {
            self.initialized = true;
            self.phase = next;
            self.since = now;
            if next == Phase::Commit { self.committed_points = p.attack_points; }
            if next == Phase::Contest { self.committed_points = 0; }
            if next != current { return Some((current,next)); }
        }
        None
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    fn picture() -> Picture {
        Picture { has_known_opponent: true, economy_works: true,
            economy_surplus: true, attack_members: 8, coherent_members: 8,
            attack_points: 4_000, ..Picture::default() }
    }

    #[test]
    fn saving_never_turns_into_an_unbounded_no_attack_deadlock() {
        let mut c = Campaign::default();
        let p = picture();
        c.update(0, p);
        assert_eq!(c.phase(), Phase::Contain);
        c.update(180, p);
        assert_eq!(c.phase(), Phase::Stockpile);
        c.update(180 + STOCKPILE_LIMIT, p);
        assert_eq!(c.phase(), Phase::Commit);
        c.update(180 + STOCKPILE_LIMIT + COMMIT_FRAMES, p);
        assert_eq!(c.phase(), Phase::Refit);
    }

    #[test]
    fn a_big_real_fist_commits_without_the_timeout() {
        let mut c = Campaign::default();
        let mut p = picture();
        p.attack_members = 18;
        p.coherent_members = 17;
        c.update(0, p);
        c.update(200, p);
        assert_eq!(c.phase(), Phase::Stockpile);
        c.update(201, p);
        assert_eq!(c.phase(), Phase::Commit);
        p.attack_points = 1_000;
        c.update(202, p);
        assert_eq!(c.phase(), Phase::Refit);
    }

    #[test]
    fn lost_income_or_a_base_raid_interrupts_a_committed_operation() {
        let mut c = Campaign::default();
        let mut p = picture();
        p.attack_members = 22;
        p.coherent_members = 22;
        c.update(0, p);
        c.update(200, p);
        c.update(201, p);
        assert_eq!(c.phase(), Phase::Commit);
        p.emergency = true;
        c.update(202, p);
        assert_eq!(c.phase(), Phase::Contest);
    }

    #[test]
    fn heavy_batteries_require_a_bigger_army_unless_true_artillery_exists() {
        let mut p = picture();
        p.live_batteries = 2;
        assert!(commitment_size(p) > 14);
        let normal = commitment_size(p);
        p.can_standoff = true;
        assert!(commitment_size(p) < normal);
        p.economy_surplus = false;
        assert_eq!(commitment_size(p), 12);
    }

    #[test]
    fn never_call_fog_silence_proof_of_suppression() {
        let mut c = Campaign::default();
        let mut p = picture();
        p.has_known_opponent = false;
        c.update(0, p);
        assert_eq!(c.phase(), Phase::Contest);
        p.has_known_opponent = true;
        c.update(1, p);
        assert_eq!(c.phase(), Phase::Contain);
    }
}
