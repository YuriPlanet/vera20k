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

const SURVEY_INTERVAL: u64 = 30;
const MAX_SITES: usize = 160;
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

/// A minimal, fog-safe contract for the joint-force commander. `Owned`
/// means our *actual* property list contains the prize; sending Capture
/// never produces this status. Unseen sightings authorize scouting only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CoalitionStatus {
    Unseen,
    Threatened,
    Open,
    Capturing,
    Owned,
    Unavailable,
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

/// Lower-tier neutral assets justify a limited nearby clearance mission.
/// Remote speculative public prizes do not borrow the main fist.
fn security_priority(value:Value,income:bool,distance_sq:i32)->Option<i32> {
    if value>=Value::Factory || (income && distance_sq<=28*28)
        || (value>=Value::Technology && distance_sq<=28*28)
        || distance_sq<=20*20
    {
        Some(value.score()+if income {35} else {0})
    } else {None}
}

pub(super) fn capture_priority(rules:&RuleSet,obs:&Observation<'_>,
    target:&EnemyEntity,from:(u16,u16),short_income:bool)->i32 {
    let Some(ty)=obs.object_type(rules,target.type_id) else {return i32::MIN;};
    let value=value_of(rules,ty).map_or(10,Value::score);
    let public=orders::strategic_public_asset(ty);
    let hostile=obs.is_hostile(target.entity_id);
    let distance=super::advance::cell_distance(from,target.cell);
    let income=ty.produce_cash_amount>0 || ty.resource_destination;
    let recurring=if ty.produce_cash_amount>0 {
        (i64::from(ty.produce_cash_amount)*900/i64::from(ty.produce_cash_delay.max(1)))
            .clamp(0,6000) as i32
    }else{0};
    let foreign_yard=ty.build_const_eligible() && super::build::foreign_yard(rules,obs,ty);
    value*100
        + if hostile && obs.easy_capture_candidate(rules,target.entity_id) {1800}else{0}
        + if foreign_yard {3500}else{0}
        + if public {4500}else{0}
        + if income && short_income {4000}else{0}
        + if recurring>0 {3000+recurring.min(4000)}else{0}
        + if ty.hospital || ty.unit_repair {1500}else{0}
        + if !hostile {1800}else{0}
        - distance.saturating_mul(120)
}

fn prize_ready(contested: bool, restricted: bool, remote: bool, income: bool, guards: usize) -> bool {
    !contested && (!restricted || !remote || income || guards >= 2)
}

pub(super) fn capture_opportunity(brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, target: &EnemyEntity, visible: &[EnemyEntity]) -> bool {
    let Some(ty) = obs.object_type(rules,target.type_id) else { return false; };
    // Do not interpret absence of a currently seen guard as a safe remote
    // building in fog. Public assets still retain their normal recon path.
    if !obs.is_cell_visible(target.cell.0,target.cell.1) { return false; }
    if !super::site_risk::capture_site_safe(&brain.census,rules,obs,target.entity_id,visible) { return false; }
    if value_of(rules,ty).is_none() { return true; }
    let restricted = obs.is_hostile(target.entity_id) && brain.census.yards > 0
        && !orders::strategic_public_asset(ty)
        && (brain.campaign.economy_resting() || brain.director.recovering_under_fire()
            || brain.director.under_attack_now());
    let remote = brain.census.base_cell.is_none_or(|home| squared_distance(home,target.cell)>16*16)
        && brain.census.refinery_cells.iter().all(|home| squared_distance(*home,target.cell)>16*16);
    let income = ty.produce_cash_amount > 0 || ty.resource_destination;
    let guards = if restricted && remote && !income { brain.census.units.iter().filter(|u|
        brain.census.available(u) && u.health_permille >= 600 && !u.traits.aircraft
            && matches!(u.slot,super::census::Slot::Infantry|super::census::Slot::Vehicle)
            && squared_distance(u.cell,target.cell)<=10*10).take(2).count() } else { 0 };
    // Destination/route danger is evaluated with real weapons. An otherwise
    // clear neutral technology site need not wait for two idle guards.
    prize_ready(false,restricted,remote,income,guards)
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

    pub(super) fn has_muster(&self, actor: u64) -> bool {
        self.musters.contains_key(&actor)
    }

    /// Select only *currently visible* valuable unowned sites that actually
    /// need a security operation. Never interrogate hidden enemy ownership.
    pub(super) fn coalition_target(&self,rules:&RuleSet,obs:&Observation<'_>,
        home:(u16,u16),after:&BTreeMap<u64,u64>,
    )->Option<(u64,(u16,u16))> {
        let now=obs.tick();
        self.sites.iter().filter(|(id,site)|
            site.phase==Phase::Secure
                && (!site.hostile || obs.object_type(rules,site.type_id)
                    .is_some_and(orders::strategic_public_asset))
                && now>=site.retry_after
                && now>=after.get(*id).copied().unwrap_or(0)
                && obs.is_cell_visible(site.cell.0,site.cell.1)
        ).filter_map(|(id,site)|{
            let ty=obs.object_type(rules,site.type_id)?;
            let distance=squared_distance(home,site.cell);
            let priority=security_priority(site.value,ty.produce_cash_amount>0
                || ty.resource_destination,distance)?;
            Some((priority*200-distance/12,std::cmp::Reverse(*id),site.cell))
        }).max_by_key(|(score,id,_)|(*score,*id))
            .map(|(_,id,cell)|(id.0,cell))
    }

    pub(super) fn coalition_status(&self, obs: &Observation<'_>,
        id: u64,
    ) -> CoalitionStatus {
        let Some(site) = self.sites.get(&id) else {
            return CoalitionStatus::Unavailable;
        };
        match site.phase {
            Phase::Owned => CoalitionStatus::Owned,
            Phase::Recon => CoalitionStatus::Unseen,
            Phase::Secure if obs.is_cell_visible(site.cell.0,site.cell.1) =>
                CoalitionStatus::Threatened,
            Phase::Ready if obs.is_cell_visible(site.cell.0,site.cell.1) =>
                CoalitionStatus::Open,
            Phase::Capturing if obs.is_cell_visible(site.cell.0,site.cell.1) =>
                CoalitionStatus::Capturing,
            Phase::Secure | Phase::Ready | Phase::Capturing => CoalitionStatus::Unseen,
            Phase::Cooldown => CoalitionStatus::Unavailable,
        }
    }

    /// Never ask production to buy an engineer using an unseen neutral
    /// structure's current existence or ownership. A remembered location
    /// belongs to reconnaissance until it is again visible.
    /// A live site cooldown or another claimant must not trigger new purchases.
    pub(super) fn purchase_ready(&self,target:u64)->bool {
        self.sites.get(&target).is_none_or(|site|site.phase==Phase::Ready)
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
        if self.surveyed_at.is_none() {
            log::info!("auto play: asset_control_metrics tick={now} schema=1");
        }
        self.surveyed_at = Some(now);
        let owned: BTreeSet<u64> = obs.own_entities().map(|entity| entity.entity_id).collect();
        let threats: Vec<_> = obs.visible_enemy_entities().collect();
        let mut visible = BTreeSet::new();
        // Capture targets are enumerated globally by the native simulation;
        // visibility filtering MUST happen before their details influence us.
        for target in obs.capturable_buildings(rules)
            .filter(|target| obs.is_cell_visible(target.cell.0, target.cell.1))
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
            if site.phase == Phase::Recon {
                site.phase = if now < site.retry_after { Phase::Cooldown } else { Phase::Ready };
            }
        }
        // A loss of visibility is not target destruction, nor permission to
        // query its true hidden status. Confirm ownership only for OUR assets.
        for (id, site) in &mut self.sites {
            if owned.contains(id) {
                if site.phase != Phase::Owned {
                    log::info!("auto play: asset operation confirmed ownership of {} at {:?}", id, site.cell);
                    if let Some(ty)=obs.object_type(rules,site.type_id) {
                        log::info!("auto play: asset_control_confirmed tick={now} target={} type={} from_nonhostile={} income={}",
                            id,ty.id,!site.hostile,ty.produce_cash_amount>0 || ty.resource_destination);
                    }
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
                if now >= site.retry_after {
                    site.phase = if super::site_risk::capture_site_safe(census,rules,obs,*id,&threats) {
                        Phase::Ready
                    } else { Phase::Secure };
                }
            } else {
                // Grade the actual prospective asset. A passive unit or
                // nominal AA weapon is not automatically a lethal approach.
                // The claimant's route is checked again by the order owner.
                site.phase = if !super::site_risk::capture_site_safe(census,rules,obs,*id,&threats) {
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
                (site.phase == Phase::Capturing, site.phase == Phase::Ready,
                    site.value.score(), site.last_seen, *id)).collect();
            ranks.sort_unstable();
            for (_,_,_,_,id) in ranks.into_iter().take(self.sites.len() - MAX_SITES) {
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
    pub(super) fn preferred_target(&self, rules:&RuleSet, obs: &Observation<'_>,
        engineer: u64, from: (u16, u16), candidates: &[EnemyEntity],
        _guards: &[(u16,u16)], _guns: &[(u16,u16)], focus: Option<u64>, tracked:&Tracker,
    ) -> Option<EnemyEntity> {
        let victim=obs.entity_type(rules,engineer)?;
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let home=obs.own_entities().filter(|e|obs.object_type(rules,e.type_id)
            .is_some_and(|ty|ty.resource_destination)).take(2).count();
        candidates.iter().filter(|target|self.permits_capture(engineer,target.entity_id)
            && obs.is_cell_visible(target.cell.0,target.cell.1))
            .max_by_key(|target|{
                let risk=super::site_risk::engineer_route_risk(rules,obs,victim,&visible,from,target.cell);
                let bonus=if focus==Some(target.entity_id)
                    && super::advance::cell_distance(from,target.cell)<=18 {1000}else{0};
                (std::cmp::Reverse(risk),
                    capture_priority(rules,obs,target,from,home<2)+bonus
                        + if obs.engineer_takes_in_one(rules,engineer,target.entity_id) {1800}else{0}
                        + if tracked.capture_purchase_live(obs.tick(),target.entity_id) {3500}else{0},
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

    /// Safety invalidated a specific proposed/native assignment. Release only
    /// its claimant; never turn an unseen site into a current target.
    pub(super) fn aborted(&mut self, now:u64, engineer:u64,target:u64) {
        if let Some(site)=self.sites.get_mut(&target)
            && site.phase==Phase::Capturing && site.claimant==Some(engineer)
        {
            site.phase=Phase::Cooldown;
            site.claimant=None;
            // A safety abort is not a lost engineer. Do not escalate the
            // death/failure backoff just because a tank appeared briefly.
            site.retry_after=now.saturating_add(240);
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
        let home_raid = brain.director.under_attack_now() || brain.campaign.economy_resting()
            || brain.mobilization.active() || brain.director.recovering_under_fire();
        let defenders: BTreeSet<u64> = brain.squads.squad(Role::Defend)
            .map(|squad| squad.members.clone()).unwrap_or_default();
        // Forget a muster once the prize is clear, the guard is gone, or
        // the short operation times out. A detached escort never remains a
        // permanent missing defender in a 120-house game.
        let expired: Vec<_> = brain.asset_ops.musters.iter()
            .filter(|(guard, (target, _, deadline))|
                now >= *deadline || home_raid || !defenders.contains(*guard)
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
        let Some(home)=brain.census.base_cell else {return;};
        let Some((target,cell))=brain.asset_ops.sites.iter()
            .filter(|(_,site)|site.phase==Phase::Secure && !site.hostile
                && now>=site.retry_after && obs.is_cell_visible(site.cell.0,site.cell.1))
            .filter_map(|(id,site)|{
                let ty=obs.object_type(rules,site.type_id)?;
                let distance=squared_distance(home,site.cell);
                let priority=security_priority(site.value,ty.produce_cash_amount>0
                    || ty.resource_destination,distance)?;
                Some((priority*200-distance/12,std::cmp::Reverse(*id),site.cell))
            }).max_by_key(|(score,id,_)|(*score,*id))
            .map(|(_,id,cell)|(id.0,cell)) else {return;};
        let Some(guard) = brain.census.units.iter()
            .filter(|unit| defenders.contains(&unit.entity_id)
                && brain.census.available(unit) && unit.traits.armed
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
        assert!(op.purchase_ready(id));
        op.submitted(20, 8, id);
        assert!(!op.purchase_ready(id));
        assert_eq!(op.sites[&id].phase, Phase::Capturing);
        assert_eq!(op.sites[&id].claimant, Some(8));
        assert!(op.permits_capture(8, id));
        assert!(!op.permits_capture(9, id));
        op.submitted(90, 8, id); // A repeated order cannot reset the watchdog.
        assert_eq!(op.sites[&id].submitted_at, 20);
        op.aborted(100,9,id); // Another engineer cannot release this prize.
        assert!(op.permits_capture(8,id));
        op.aborted(110,8,id);
        assert_eq!(op.sites[&id].failures,0);
        assert_eq!(op.sites[&id].retry_after,350);
        assert!(!op.permits_capture(8,id));
        attempt_failed(op.sites.get_mut(&id).unwrap(), 120);
        assert!(!op.permits_capture(8, id));
        assert!(!op.purchase_ready(id));
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

#[cfg(all(test, tests_sim_p_s))]
mod acquisition_tests {
    use super::*;
    #[test]
    fn nearby_income_and_technology_can_request_limited_clearance() {
        assert!(security_priority(Value::Public,true,28*28).is_some());
        assert!(security_priority(Value::Technology,false,24*24).is_some());
        assert!(security_priority(Value::Public,false,20*20).is_some());
        assert!(security_priority(Value::Public,true,29*29).is_none());
        assert!(security_priority(Value::Public,false,21*21).is_none());
        assert!(security_priority(Value::Factory,false,60*60).is_some());
    }
    #[test]
    fn contested_prize_is_not_made_safe_by_a_large_escort() {
        assert!(!prize_ready(true,false,false,true,10));
        assert!(!prize_ready(false,true,true,false,1));
        assert!(prize_ready(false,true,true,false,2));
        assert!(prize_ready(false,true,true,true,0));
    }
}
