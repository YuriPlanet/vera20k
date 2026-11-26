//! Local security and stalled-front evidence, not another order producer.
//! Only current visible hostiles may become targets; a missing observation
//! is not evidence of a kill. Timers are bounded review permissions, never
//! authorisation to bypass the native attack or safety checks.
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::{EnemyEntity, Observation};
use super::advance::cell_distance;
use super::census::Census;

pub(super) type Cell = (u16,u16);
// Shorter than the 1350-frame campaign commitment; only count
// continuous near-front observation, never the approach march or refit.
const REVIEW_FRAMES: u64 = 1_080;
const RETRY_FRAMES: u64 = 1_200;
const REVIEW_RADIUS: i32 = 12;

/// A combat unit threatening the resource chain or the current formation.
/// Passive enemy miners are economic targets, not an emergency alarm.
pub(super) fn mobile_threat(rules: &RuleSet, obs: &Observation<'_>, enemy: &EnemyEntity) -> bool {
    obs.object_type(rules,enemy.type_id).is_some_and(|ty|
        ty.category != ObjectCategory::Building && !ty.spawned
        && (ty.engineer || ty.agent
            || super::advance::weapon_range_cells(rules,ty,obs.is_deployed(enemy.entity_id)) > 0
            || super::tactics::mind_control_reach(rules,ty).is_some()))
}

pub(super) fn near_income(census: &Census, cell: Cell) -> bool {
    census.refinery_cells.iter().any(|site| cell_distance(*site,cell) <= 12)
        || census.units.iter().any(|u| u.traits.harvester && cell_distance(u.cell,cell) <= 10)
}

fn contact_rank(asset: bool, distance: i32) -> Option<(bool,i32)> {
    // Do not recall a remote fist across the map for each miner alarm.
    (distance <= if asset { 24 } else { 12 }).then_some((!asset,distance))
}

pub(super) fn local_rank(census: &Census, anchor: Cell, enemy: &EnemyEntity) -> Option<(bool,i32,u64)> {
    let asset = near_income(census,enemy.cell)
        || census.yard_cells.iter().any(|site| cell_distance(*site,enemy.cell) <= 14);
    contact_rank(asset,cell_distance(anchor,enemy.cell)).map(|(a,d)| (a,d,enemy.entity_id))
}

#[derive(Debug, Clone, Copy)]
struct Sample { lane: u64, target: u64, cell: Cell, since: u64, best_hp: i32, last: u64 }

#[derive(Debug, Default)]
pub(super) struct FrontReview {
    sample: Option<Sample>,
    deferred: Option<(u64,Cell,u64)>,
}

impl FrontReview {
    /// A meaningful net HP decrease resets the clock. Allow small chip
    /// damage to accumulate rather than refreshing on every 1 HP change.
    fn sample(&mut self, now: u64, lane: u64, target: u64, cell: Cell, hp: i32) -> bool {
        if self.sample.is_none_or(|s| s.target != target || s.lane != lane
            || now.saturating_sub(s.last) > 180) {
            self.sample = Some(Sample {lane,target,cell,since:now,best_hp:hp,last:now});
            return false;
        }
        let Some(s) = &mut self.sample else { return false; };
        s.last = now;
        if hp <= s.best_hp.saturating_sub((s.best_hp/20).clamp(10,50)) {
            s.best_hp = hp;
            s.since = now;
        }
        if now.saturating_sub(s.since) < REVIEW_FRAMES { return false; }
        self.deferred = Some((lane,s.cell,now.saturating_add(RETRY_FRAMES)));
        self.sample = None;
        true
    }

    pub(super) fn pause(&mut self) { self.sample = None; }

    /// A confirmed unproductive repeated assault may redirect the same
    /// target-selection mechanism as a stalled battery. Never delete the
    /// fallback or manufacture a confirmed kill.
    pub(super) fn defer_failed_charge(&mut self,now:u64,lane:u64,cell:Cell) {
        self.sample=None;
        self.deferred=Some((lane,cell,now.saturating_add(RETRY_FRAMES)));
    }

    pub(super) fn observe(&mut self, rules: &RuleSet, obs: &Observation<'_>, lane: u64, target: u64, cell: Cell) {
        if self.sample.is_some_and(|s| s.target != target || s.lane != lane) { self.pause(); }
        if obs.tick() % 90 != 0 { return; }
        if target == 0 || !obs.visible_enemy_entities().any(|e| e.entity_id == target
            && obs.object_type(rules,e.type_id).is_some_and(|ty| ty.category == ObjectCategory::Building)) {
            self.pause();
            return;
        }
        // health() itself is not a fog-safe API. The visible-id check above
        // must precede this read on every sample, including continuation.
        let Some(hp) = obs.health(target) else { self.pause(); return; };
        if self.sample(obs.tick(),lane,target,cell,hp) {
            log::info!("auto play: front_deferred tick={} lane={} target={} cell={:?}",obs.tick(),lane,target,cell);
        }
    }

    pub(super) fn deferred(&self, now: u64) -> Option<(u64,Cell)> {
        self.deferred.filter(|(_,_,until)| now < *until).map(|(id,cell,_)| (id,cell))
    }
}

/// Deprioritise an entire blocked gun line, not just the chosen economy
/// building behind the gun. A caller MUST retain an all-candidate fallback.
pub(super) fn outside_deferred(enemy: &EnemyEntity, deferred: Option<(u64,Cell)>) -> bool {
    deferred.is_none_or(|(id,cell)| enemy.entity_id != id && cell_distance(enemy.cell,cell) > REVIEW_RADIUS)
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    #[test]
    fn nearby_miner_threat_precedes_front_contact_but_not_global_recall() {
        assert!(contact_rank(true,20) < contact_rank(false,3));
        assert_eq!(contact_rank(true,25),None);
        assert_eq!(contact_rank(false,13),None);
    }
    #[test]
    fn review_requires_continuous_observed_no_progress_and_expires() {
        let mut r = FrontReview::default();
        for t in (0..REVIEW_FRAMES).step_by(90) { assert!(!r.sample(t,1,2,(4,4),500)); }
        assert!(r.sample(REVIEW_FRAMES,1,2,(4,4),500));
        assert_eq!(r.deferred(REVIEW_FRAMES+RETRY_FRAMES-1),Some((1,(4,4))));
        assert_eq!(r.deferred(REVIEW_FRAMES+RETRY_FRAMES),None);
        r.pause();
        assert!(!r.sample(4000,1,2,(4,4),500));
        assert!(!r.sample(6000,1,2,(4,4),500)); // observation gap
    }
    #[test]
    fn repeated_failed_charge_uses_bounded_front_deferral() {
        let mut r=FrontReview::default();
        r.defer_failed_charge(100,7,(12,14));
        assert_eq!(r.deferred(100),Some((7,(12,14))));
        assert_eq!(r.deferred(100+RETRY_FRAMES),None);
    }
    #[test]
    fn damage_resets_review_clock() {
        let mut r = FrontReview::default();
        for t in (0..=REVIEW_FRAMES+720).step_by(90) {
            let hp = if t < 900 {500} else {450};
            assert!(!r.sample(t,1,2,(4,4),hp));
        }
        assert_eq!(r.deferred(1800),None);
    }
}
