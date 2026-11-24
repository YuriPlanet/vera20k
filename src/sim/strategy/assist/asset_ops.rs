//! V37 strategic asset operations. An abandoned yard is not a disposable
//! engineer target: it is a persistent reconnaissance -> clearance -> capture
//! -> ownership verification -> technology exploitation operation.
//!
//! This director does NOT execute captures itself. `orders::captures` owns
//! native Capture admission, TaskLedger and specialist safety, and `escort`
//! follows a confirmed engineer. The director supplies a stable preference,
//! scouts' revisit priority and a *bounded* forward security muster. A target
//! under shroud is never treated as an entity that may be clicked.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::intern::InternedId;

use super::AssistBrain;
use super::census::Census;
use super::orders;
use super::squads::Role;
use super::squared_distance;
use super::tasks::JobKind;
use super::tracked::{TrackedKind, Tracker};

const SURVEY_INTERVAL: u64 = 75;
const MAX_SITES: usize = 72;
const SIGHTING_TTL: u64 = 5_400;
const CAPTURE_SETTLE: u64 = 90;
const RETRY_BASE: u64 = 360;
const TECH_WINDOW: u64 = 3_600;
const GUARD_RESERVE: usize = 3;
const MUSTER_COOLDOWN: u64 = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Value {
    Public,
    Technology,
    Barracks,
    Factory,
    Yard,
}

impl Value {
    fn score(self) -> i32 {
        match self {
            Self::Public => 25,
            Self::Technology => 45,
            Self::Barracks => 60,
            Self::Factory => 80,
            Self::Yard => 100,
        }
    }
}

fn value_of(rules: &RuleSet, ty: &ObjectType) -> Option<Value> {
    if !ty.capturable || ty.category != ObjectCategory::Building { return None; }
    if ty.build_const_eligible() { Some(Value::Yard) }
    else if ty.weapons_factory { Some(Value::Factory) }
    else if rules.build_barracks_types.iter().any(|id| *id == ty.id) {
        Some(Value::Barracks)
    } else if rules.build_tech_types.iter().any(|id| *id == ty.id) {
        Some(Value::Technology)
    } else if ty.factory_type().is_some() { Some(Value::Factory) }
    else if orders::strategic_public_asset(ty) { Some(Value::Public) }
    else { None }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Recon,       // Last sighting is stale: scouts may revisit, engineers may not click.
    Secure,      // A currently seen threat blocks the specialist's approach.
    Ready,       // Presently visible and may be assigned to an engineer.
    Capturing,   // A Capture was submitted, not proof of acceptance or ownership.
    Cooldown,    // No observed success; bounded retries, never a permanent blacklist.
    Owned,       // An owned entity with the same ID was actually observed.
}

#[derive(Clone, Copy, Debug)]
struct Site {
    cell: (u16, u16),
    type_id: InternedId,
    value: Value,
    hostile: bool,
    last_seen: u64,
    phase: Phase,
    claimant: Option<u64>,
    submitted_at: u64,
    retry_after: u64,
    failures: u8,
}

fn muster_capacity(defenders: usize) -> usize {
    defenders.saturating_sub(GUARD_RESERVE).min(2)
}

fn backoff(failures: u8) -> u64 {
    RETRY_BASE.saturating_mul(u64::from(failures.min(4)).max(1))
}

fn attempt_failed(site: &mut Site, now: u64) {
    site.failures = site.failures.saturating_add(1);
    site.phase = Phase::Cooldown;
    site.claimant = None;
    site.retry_after = now.saturating_add(backoff(site.failures));
}

fn threat_on_approach(rules: &RuleSet, obs: &Observation<'_>,
    target: (u16, u16), origin: (u16, u16),
) -> bool {
    // All danger comes from current legal sight, never hidden world objects.
    obs.visible_enemy_entities().take(256).any(|enemy| {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else { return false };
        if ty.category == ObjectCategory::Building {
            ty.is_base_defense
                && obs.visible_enemy_building_operational(rules, enemy.entity_id)
                && (squared_distance(enemy.cell, target) <= 11 * 11
                    || orders::path_crosses(&[enemy.cell], origin, target, 7))
        } else {
            squared_distance(enemy.cell, target) <= 7 * 7
                || orders::path_crosses(&[enemy.cell], origin, target, 4)
        }
    })
}

#[derive(Debug, Default)]
pub(super) struct Director {
    sites: BTreeMap<u64, Site>,
    surveyed_at: Option<u64>,
    foreign_tech_until: u64,
    muster_after: u64,
    /// Guard -> (asset, safe post, expiry). Never drain all home defenders.
    musters: BTreeMap<u64, (u64, (u16,u16), u64)>,
}

impl Director {
    pub(super) fn technology_window(&self, now: u64) -> bool {
        now < self.foreign_tech_until
    }

    /// Never ask production to buy an engineer using an unseen neutral
    /// structure's current existence or ownership. A remembered location
    /// belongs to reconnaissance until it is again visible.
    pub(super) fn visible_opportunities(&self, obs: &Observation<'_>) -> usize {
        let now = obs.tick();
        self.sites.values().filter(|site|
            !site.hostile && site.value >= Value::Technology
                && matches!(site.phase, Phase::Ready | Phase::Secure | Phase::Capturing)
                && now >= site.retry_after
                && obs.is_cell_visible(site.cell.0, site.cell.1)
        ).take(8).count()
    }

    pub(super) fn recon_request(&self, obs: &Observation<'_>) -> Option<(u16, u16)> {
        self.sites.values().filter(|site| site.phase == Phase::Recon
            && obs.tick().saturating_sub(site.last_seen) < SIGHTING_TTL)
            .max_by_key(|site| (site.value.score(), std::cmp::Reverse(site.cell)))
            .map(|site| site.cell)
    }

    pub(super) fn observe(&mut self, rules: &RuleSet,
        obs: &Observation<'_>, census: &Census, tracked: &Tracker,
    ) {
        let now = obs.tick();
        if self.surveyed_at.is_some_and(|time| now.saturating_sub(time) < SURVEY_INTERVAL) {
            return;
        }
        self.surveyed_at = Some(now);
        let owned: BTreeSet<u64> = obs.own_entities().map(|entity| entity.entity_id).collect();
        let mut visible = BTreeSet::new();
        // Capture targets are enumerated globally by the native simulation;
        // visibility filtering MUST happen before their details influence us.
        for target in obs.capturable_buildings(rules)
            .filter(|target| obs.is_cell_visible(target.cell.0, target.cell.1))
            .take(MAX_SITES * 2)
        {
            let Some(ty) = obs.object_type(rules, target.type_id) else { continue };
            let Some(value) = value_of(rules, ty) else { continue };
            visible.insert(target.entity_id);
            let hostile = obs.is_hostile(target.entity_id);
            let site = self.sites.entry(target.entity_id).or_insert(Site {
                cell: target.cell, type_id: target.type_id, value, hostile,
                last_seen: now, phase: Phase::Ready, claimant: None,
                submitted_at: 0, retry_after: 0, failures: 0,
            });
            site.cell = target.cell;
            site.type_id = target.type_id;
            site.last_seen = now;
            site.hostile = hostile;
            site.value = value;
            if site.phase == Phase::Recon { site.phase = Phase::Ready; }
        }
        // A loss of visibility is not target destruction, nor permission to
        // query its true hidden status. Confirm ownership only for OUR assets.
        for (id, site) in &mut self.sites {
            if owned.contains(id) {
                if site.phase != Phase::Owned {
                    log::info!("auto play: asset operation confirmed ownership of {} at {:?}", id, site.cell);
                    if site.value == Value::Yard
                        && obs.object_type(rules, site.type_id)
                            .is_some_and(|ty| super::build::foreign_yard(rules, obs, ty))
                    {
                        self.foreign_tech_until = now.saturating_add(TECH_WINDOW);
                    }
                }
                site.phase = Phase::Owned;
                site.claimant = None;
            } else if !visible.contains(id) {
                if obs.is_cell_visible(site.cell.0, site.cell.1) {
                    // The cell is visible and the building is not capturable:
                    // no more orders to this old entity. Remove below.
                    site.phase = Phase::Cooldown;
                    site.retry_after = u64::MAX;
                } else {
                    // Loss of OUR former asset is observable even if its old
                    // cell is now shrouded. Any replacement owner must be
                    // confirmed by future scouting, never inferred here.
                    site.phase = Phase::Recon;
                    site.claimant = None;
                }
            } else if site.phase == Phase::Capturing {
                let active = site.claimant.is_some_and(|actor|
                    tracked.tasks.get(actor).is_some_and(|task|
                        task.kind == JobKind::Engineer && task.target == Some(*id)));
                if !active && now.saturating_sub(site.submitted_at) >= CAPTURE_SETTLE {
                    attempt_failed(site, now);
                }
            } else if site.phase == Phase::Cooldown {
                if now >= site.retry_after { site.phase = Phase::Ready; }
            } else {
                let origin = site.claimant.and_then(|actor|
                    census.units.iter().find(|unit| unit.entity_id == actor)
                        .map(|unit| unit.cell))
                    .or(census.base_cell).unwrap_or(site.cell);
                site.phase = if threat_on_approach(rules, obs, site.cell, origin) {
                    Phase::Secure
                } else { Phase::Ready };
            }
        }
        self.sites.retain(|_,site|
            site.phase == Phase::Owned && now.saturating_sub(site.last_seen) < TECH_WINDOW
            || site.retry_after != u64::MAX
                && now.saturating_sub(site.last_seen) <= SIGHTING_TTL);
        if self.sites.len() > MAX_SITES {
            let mut ranks: Vec<_> = self.sites.iter().map(|(id, site)|
                (site.phase == Phase::Capturing, site.last_seen, *id)).collect();
            ranks.sort_unstable();
            for (_,_,id) in ranks.into_iter().take(self.sites.len() - MAX_SITES) {
                self.sites.remove(&id);
            }
        }
        if now % 600 == 0 && !self.sites.is_empty() {
            let count = |phase| self.sites.values()
                .filter(|site| site.phase == phase).count();
            log::info!(
                "auto play: strategic assets total={} recon={} secure={} ready={} capturing={} cooldown={} owned={} foreign_tech_window={}",
                self.sites.len(), count(Phase::Recon), count(Phase::Secure),
                count(Phase::Ready), count(Phase::Capturing), count(Phase::Cooldown),
                count(Phase::Owned), self.technology_window(now),
            );
        }
    }

    /// Only a currently visible and approach-safe objective outranks the
    /// traditional capture-value heuristic. Existing specialist job ownership
    /// and native CanTakeBuilding checks are done by `orders::captures`.
    pub(super) fn preferred_target(&self, obs: &Observation<'_>,
        engineer: u64, from: (u16, u16), candidates: &[EnemyEntity],
        guards: &[(u16,u16)], guns: &[(u16,u16)],
    ) -> Option<EnemyEntity> {
        candidates.iter().filter(|target| {
            let Some(site) = self.sites.get(&target.entity_id) else { return false };
            site.phase == Phase::Ready
                && obs.is_cell_visible(target.cell.0, target.cell.1)
                && !guards.iter().any(|cell| squared_distance(*cell, target.cell) <= 6*6)
                && !orders::path_crosses(guards, from, target.cell, 3)
                && !orders::path_crosses(guns, from, target.cell, 4)
                && site.claimant.is_none_or(|claim| claim == engineer)
        }).max_by_key(|target| {
            let site = &self.sites[&target.entity_id];
            // Do not sacrifice the first income line chasing a distant
            // abandoned yard when the house cannot yet finance its army.
            (site.value.score() * 100 - i32::from(site.hostile) * 1000
                + (if obs.credits() < 3_500 && site.value == Value::Public {
                    6_500
                } else { 0 })
                - squared_distance(from, target.cell) / 24,
                std::cmp::Reverse(target.entity_id))
        }).copied()
    }

    /// A cooldown is a target-level obligation, not just a preference.
    /// Without this gate the legacy nearest-building fallback can instantly
    /// re-send a dead engineer's successor during the retry window.
    pub(super) fn permits_capture(&self, engineer: u64, target: u64) -> bool {
        match self.sites.get(&target) {
            None => true, // other capturable assets retain the existing behavior
            Some(site) => match site.phase {
                Phase::Ready => true,
                Phase::Capturing => site.claimant == Some(engineer),
                Phase::Recon | Phase::Secure | Phase::Cooldown | Phase::Owned => false,
            },
        }
    }

    pub(super) fn submitted(&mut self, now: u64, engineer: u64, target: u64) {
        if let Some(site) = self.sites.get_mut(&target) {
            if site.phase != Phase::Capturing || site.claimant != Some(engineer) {
                site.submitted_at = now;
            }
            site.phase = Phase::Capturing;
            site.claimant = Some(engineer);
        }
    }

    /// Stage one *spare* combat guard behind a threatened high-value asset.
    /// This is not permission to send an engineer through enemy guns: the
    /// capture selector continues to refuse while visible threats persist.
    /// The ordinary escort subsystem takes over after a Capture is confirmed.
    pub(super) fn muster(brain: &mut AssistBrain, rules: &RuleSet,
        obs: &Observation<'_>, out: &mut Outbox,
    ) {
        let now = obs.tick();
        if brain.asset_ops.musters.is_empty() && now < brain.asset_ops.muster_after {
            return;
        }
        let home_raid = brain.director.under_attack_now();
        let defenders: BTreeSet<u64> = brain.squads.squad(Role::Defend)
            .map(|squad| squad.members.clone()).unwrap_or_default();
        // Forget a muster once the prize is clear, the guard is gone, or
        // the short operation times out. A detached escort never remains a
        // permanent missing defender in a 120-house game.
        let expired: Vec<_> = brain.asset_ops.musters.iter()
            .filter(|(guard, (target, _, deadline))|
                now >= *deadline || home_raid || !defenders.contains(guard)
                    || brain.asset_ops.sites.get(target)
                        .is_none_or(|site| site.phase != Phase::Secure))
            .map(|(guard, _)| *guard).collect();
        for guard in expired {
            if let Some((_,post,_)) = brain.asset_ops.musters.remove(&guard) {
                if brain.tracked.primary_goal(guard)
                    == Some(Goal::AttackMove { cell: post })
                {
                    brain.tracked.forget_primary(guard);
                    brain.tracked.void(guard, TrackedKind::AttackMove);
                    if home_raid && !out.ordered_this_frame(guard) {
                        let _ = out.want(guard, 0, Goal::Stop);
                    }
                }
            }
        }
        if now < brain.asset_ops.muster_after || home_raid { return; }
        let capacity = muster_capacity(defenders.len());
        if brain.asset_ops.musters.len() >= capacity { return; }
        let Some((target, cell)) = brain.asset_ops.sites.iter()
            .filter(|(_,site)| site.phase == Phase::Secure
                && site.value >= Value::Factory && !site.hostile
                && obs.is_cell_visible(site.cell.0, site.cell.1))
            .max_by_key(|(id,site)| (site.value, std::cmp::Reverse(**id)))
            .map(|(id,site)| (*id,site.cell)) else { return };
        let Some(guard) = brain.census.units.iter()
            .filter(|unit| defenders.contains(&unit.entity_id)
                && unit.controllable() && unit.traits.combat && unit.traits.armed
                && !unit.in_service() && !unit.traits.irreplaceable
                && !unit.traits.aircraft && !obs.is_deployed(unit.entity_id)
                && !out.ordered_this_frame(unit.entity_id)
                && obs.destination(unit.entity_id).is_none()
                && obs.attack_target(unit.entity_id).is_none()
                && !brain.escorts.values().any(|esc| esc.guard == unit.entity_id)
                && !brain.asset_guards.contains_key(&unit.entity_id)
                && !brain.asset_ops.musters.contains_key(&unit.entity_id)
                && matches!(brain.tracked.live_kind(now, unit.entity_id),
                    None | Some(TrackedKind::Guard)))
            .filter(|unit| squared_distance(unit.cell, cell) <= 28*28)
            .min_by_key(|unit| (squared_distance(unit.cell, cell), unit.entity_id)) else { return };
        let Some(ty) = obs.object_type(rules, guard.type_id) else { return };
        let dx = (i32::from(guard.cell.0) - i32::from(cell.0)).signum();
        let dy = (i32::from(guard.cell.1) - i32::from(cell.1)).signum();
        let mut rally = None;
        for distance in [11, 13, 16] {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(cell.0) + dx * distance),
                u16::try_from(i32::from(cell.1) + dy * distance),
            ) else { continue };
            let post = (x,y);
            if obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                && obs.can_reach(ty.movement_zone, guard.cell, post)
                && !super::tactics::unsafe_post(rules, obs, guard.entity_id, post)
            { rally = Some(post); break; }
        }
        let Some(post) = rally else { return };
        if orders::issue_tracked(&mut brain.tracked, brain.trace, out,
            rules, obs, guard.entity_id, TrackedKind::AttackMove,
            None, Some(post), Goal::AttackMove { cell: post })
        {
            brain.asset_ops.muster_after = now.saturating_add(MUSTER_COOLDOWN);
            brain.asset_ops.musters.insert(guard.entity_id, (target, post,
                now.saturating_add(450)));
            log::debug!("auto play: asset {target} requests spare guard {} at {:?}", guard.entity_id, post);
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn strategic_assets_rank_above_another_public_building() {
        assert!(Value::Yard > Value::Factory);
        assert!(Value::Factory > Value::Barracks);
        assert!(Value::Barracks > Value::Technology);
        assert!(Value::Technology > Value::Public);
    }

    #[test]
    fn a_submitted_capture_is_not_a_success_receipt() {
        let mut op = Director::default();
        let id = 11;
        op.sites.insert(id, Site { cell: (9,9), type_id: InternedId::default(),
            value: Value::Yard, hostile: false, last_seen: 10,
            phase: Phase::Ready, claimant: None, submitted_at: 0,
            retry_after: 0, failures: 0 });
        op.submitted(20, 8, id);
        assert_eq!(op.sites[&id].phase, Phase::Capturing);
        assert_eq!(op.sites[&id].claimant, Some(8));
        assert!(op.permits_capture(8, id));
        assert!(!op.permits_capture(9, id));
        op.submitted(90, 8, id); // A repeated order cannot reset the watchdog.
        assert_eq!(op.sites[&id].submitted_at, 20);
        attempt_failed(op.sites.get_mut(&id).unwrap(), 120);
        assert!(!op.permits_capture(8, id));
        assert_eq!(op.sites[&id].phase, Phase::Cooldown);
        assert!(op.sites[&id].retry_after > 120);
    }


    #[test]
    fn retail_rules_identify_real_capturable_technology() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return };
        for (name, expected) in [
            ("GACNST", Value::Yard),
            ("GAWEAP", Value::Factory),
            ("GAPILE", Value::Barracks),
            ("GATECH", Value::Technology),
            ("CAOILD", Value::Public),
        ] {
            let ty = rules.object(name).expect("standard rules object");
            assert_eq!(value_of(&rules, ty), Some(expected), "{name}");
        }
    }

    #[test]
    fn security_muster_keeps_three_defenders_at_home() {
        assert_eq!(muster_capacity(0), 0);
        assert_eq!(muster_capacity(3), 0);
        assert_eq!(muster_capacity(4), 1);
        assert_eq!(muster_capacity(9), 2);
    }

    #[test]
    fn failed_operations_backoff_and_can_retry() {
        assert_eq!(backoff(1), RETRY_BASE);
        assert_eq!(backoff(2), RETRY_BASE * 2);
        assert_eq!(backoff(20), RETRY_BASE * 4);
    }
}
