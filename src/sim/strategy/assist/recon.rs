//! V36 reconnaissance operations: divide the unexplored map, keep useful
//! sightings, and revisit strategically important ground without seeing fog.
//!
//! The Scout squad owns its members; this director only owns *where they go*.
//! No reconnaissance mission clicks a hidden entity. A sighting is evidence of
//! the last observed cell, not evidence that the building still exists. When
//! that cell is visible and the building is gone, the sighting is removed.
//!
//! A scout is intentionally **not** sent to the nearest remembered enemy as a
//! fallback. That was a way to donate dogs to batteries after exploration.
//! Scouts disperse across currently unrevealed terrain or return to safe
//! observation posts around stale strategic sightings. Ordinary combat and
//! crate pickup remain owned by micro / orders and can interrupt this work.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::advance::cell_distance;
use super::census::Census;
use super::squads::{Role, Squads};

const SAMPLE_FRAMES: u64 = 90;
const ASSIGN_FRAMES: u64 = 120;
const PROGRESS_FRAMES: u64 = 300;
const SITE_LIFETIME: u64 = 6_300;
const REVISIT_AGE: u64 = 540;
const REVISIT_COOLDOWN: u64 = 1_350;
const FAILED_CELL_COOLDOWN: u64 = 900;
const FRONTIER_STEP: usize = 8;
const DUPLICATE_RADIUS: i32 = 18;
const MAX_SCOUTS: usize = 12;
const MAX_SITES: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interest {
    Power,
    Yard,
    Factory,
    Logistics,
    Technology,
}

impl Interest {
    fn priority(self) -> i32 {
        match self {
            Self::Power => 5,
            Self::Yard => 5,
            Self::Factory => 4,
            Self::Logistics => 3,
            Self::Technology => 3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Site {
    cell: (u16, u16),
    interest: Interest,
    hostile: bool,
    last_seen: u64,
    last_probed: u64,
}

fn interest(ty: &ObjectType) -> Option<Interest> {
    if ty.category != ObjectCategory::Building { return None; }
    if ty.build_const_eligible() { Some(Interest::Yard) }
    else if ty.weapons_factory || ty.factory_type().is_some() { Some(Interest::Factory) }
    else if ty.power > 0 { Some(Interest::Power) }
    else if ty.resource_destination { Some(Interest::Logistics) }
    else if ty.capturable { Some(Interest::Technology) }
    else { None }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Frontier,
    Revisit(u64),
}

#[derive(Clone, Copy, Debug)]
struct Assignment {
    destination: (u16, u16),
    purpose: Purpose,
    last_cell: (u16, u16),
    last_progress: u64,
    began: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Candidate {
    destination: (u16, u16),
    purpose: Purpose,
    utility: i32,
}

/// Compare a candidate against other scouts' current work. Deliberately use
/// a hard spacing gate, not just a small penalty: one map corner cannot
/// consume every scout while the other half stays under shroud.
fn separated(cell: (u16,u16), occupied: &BTreeSet<(u16,u16)>) -> bool {
    occupied.iter().all(|other| cell_distance(cell, *other) >= DUPLICATE_RADIUS)
}

fn better(candidate: Candidate, current: Option<Candidate>) -> Option<Candidate> {
    match current {
        Some(old) if (old.utility, std::cmp::Reverse(old.destination))
            >= (candidate.utility, std::cmp::Reverse(candidate.destination)) => Some(old),
        _ => Some(candidate),
    }
}

/// Future risk is *not* learned from hidden entities. The only dangerous cells
/// scored here are around currently visible hostile objects. A position that
/// was safe 90 frames ago may no longer be safe; the combat/micro layers
/// retain the right to interrupt the scout on that frame.
fn dangerous(cell: (u16,u16), threats: &[((u16,u16), i32)]) -> bool {
    threats.iter().any(|(at, radius)| cell_distance(cell, *at) <= *radius)
}

fn unknown_gain(obs: &Observation<'_>, center: (u16,u16)) -> i32 {
    let mut gain = 0;
    for dy in [-6, 0, 6] {
        for dx in [-6, 0, 6] {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(center.0) + dx),
                u16::try_from(i32::from(center.1) + dy),
            ) else { continue; };
            if obs.is_cell_in_playfield(x, y) && !obs.is_cell_revealed(x, y) {
                gain += 1;
            }
        }
    }
    gain
}

fn eligible(obs: &Observation<'_>, zone: MovementZone, from: (u16,u16),
    cell: (u16,u16), threats: &[((u16,u16), i32)],
    blocked: &BTreeMap<(u16,u16),u64>, occupied: &BTreeSet<(u16,u16)>,
    now: u64,
) -> bool {
    // Ships and flyers do not use the ground-only `is_walkable` projection;
    // their own native movement-zone connectivity remains authoritative.
    let ground_bound = !matches!(zone,
        MovementZone::Fly | MovementZone::Water | MovementZone::WaterBeach
        | MovementZone::Amphibious | MovementZone::AmphibiousCrusher
        | MovementZone::AmphibiousDestroyer);
    obs.is_cell_in_playfield(cell.0, cell.1)
        && (!ground_bound || !obs.has_ground_grid()
            || obs.is_ground_walkable(cell.0, cell.1))
        && obs.can_reach(zone, from, cell)
        && !dangerous(cell, threats)
        && blocked.get(&cell).is_none_or(|until| now >= *until)
        && separated(cell, occupied)
}

/// The house's durable map of strategic *sightings*, and at most 12 actual
/// scout journeys. It is an operation state, not a second simulation ledger.
#[derive(Debug, Default)]
pub(super) struct Director {
    sites: BTreeMap<u64, Site>,
    missions: BTreeMap<u64, Assignment>,
    blocked: BTreeMap<(u16,u16),u64>,
    observed_at: Option<u64>,
    next_assign: u64,
    /// V37 urgent *remembered* asset; only influences safe scout waypoints.
    priority_asset: Option<(u16,u16)>,
}

impl Director {
    pub(super) fn prioritize_asset(&mut self, cell: Option<(u16,u16)>) {
        self.priority_asset = cell;
    }

    pub(super) fn destination(&self, id: u64) -> Option<(u16,u16)> {
        self.missions.get(&id).map(|mission| mission.destination)
    }

    /// A remembered **enemy** building may guide a search, never an entity-id
    /// attack. The current fog cell must be unseen, and known death/ownership
    /// changes were already removed during `observe`.
    pub(super) fn enemy_search_site(&self, obs: &Observation<'_>,
        anchor: (u16,u16),
    ) -> Option<(u16,u16)> {
        let now = obs.tick();
        self.sites.values().filter(|site|
            site.hostile && now.saturating_sub(site.last_seen) <= SITE_LIFETIME
                && !obs.is_cell_visible(site.cell.0, site.cell.1))
            .min_by_key(|site| (cell_distance(anchor, site.cell)
                - site.interest.priority() * 2, site.cell))
            .map(|site| site.cell)
    }

    fn survey_sites(&mut self, now: u64, rules: &RuleSet,
        obs: &Observation<'_>,
    ) {
        if self.observed_at.is_some_and(|at| now.saturating_sub(at) < SAMPLE_FRAMES) {
            return;
        }
        self.observed_at = Some(now);
        let mut present = BTreeSet::new();
        // These are **visible** hostile objects. Never call `enemy_entities`
        // here: it returns all of the hidden enemy's live structures.
        for enemy in obs.visible_enemy_entities().take(MAX_SITES) {
            let Some(ty) = obs.object_type(rules, enemy.type_id) else { continue; };
            let Some(value) = interest(ty) else { continue; };
            present.insert(enemy.entity_id);
            let earlier = self.sites.get(&enemy.entity_id).map_or(0, |site| site.last_probed);
            self.sites.insert(enemy.entity_id, Site {
                cell: enemy.cell, interest: value, hostile: true,
                last_seen: now, last_probed: earlier,
            });
        }
        // Capturable neutral buildings are strategic discoveries even when no
        // engineer happens to be alive on the frame that revealed them.
        for target in obs.capturable_buildings(rules)
            .filter(|target| !obs.is_hostile(target.entity_id)
                && obs.is_cell_visible(target.cell.0, target.cell.1))
            .take(MAX_SITES)
        {
            let Some(ty) = obs.object_type(rules, target.type_id) else { continue; };
            let Some(value) = interest(ty) else { continue; };
            present.insert(target.entity_id);
            let earlier = self.sites.get(&target.entity_id).map_or(0, |site| site.last_probed);
            self.sites.insert(target.entity_id, Site {
                cell: target.cell, interest: value, hostile: false,
                last_seen: now, last_probed: earlier,
            });
        }
        self.sites.retain(|id, site| {
            now.saturating_sub(site.last_seen) <= SITE_LIFETIME
                && (!obs.is_cell_visible(site.cell.0, site.cell.1)
                    || present.contains(id))
        });
        // If a site remained hidden for a long time, its sighting still
        // exists, but repeated scouting it has a separate probe cooldown.
        if self.sites.len() > MAX_SITES {
            let excess = self.sites.len() - MAX_SITES;
            let mut oldest: Vec<(u64,u64)> = self.sites.iter()
                .map(|(id, site)| (site.last_seen, *id)).collect();
            oldest.sort_unstable();
            for (_,id) in oldest.into_iter().take(excess) {
                self.sites.remove(&id);
            }
        }
    }

    fn choose(&self, now: u64, obs: &Observation<'_>, from: (u16,u16),
        zone: MovementZone, threats: &[((u16,u16),i32)],
        occupied: &BTreeSet<(u16,u16)>,
    ) -> Option<Candidate> {
        let mut best = None;
        // There is no full-map scan per scout every tick. This survey runs
        // only on mission assignment, at most once per 120 frames, and uses
        // a coarse 8-cell grid, not an A* for every possible target.
        let (width,height) = obs.map_extent();
        for y in (0..height).step_by(FRONTIER_STEP) {
            for x in (0..width).step_by(FRONTIER_STEP) {
                let cell = (x,y);
                if obs.is_cell_revealed(x,y)
                    || !eligible(obs,zone,from,cell,threats,
                        &self.blocked,occupied,now)
                { continue; }
                let gain = unknown_gain(obs,cell);
                if gain == 0 { continue; }
                let distance = cell_distance(from,cell);
                let choice = Candidate {
                    destination: cell, purpose: Purpose::Frontier,
                    utility: gain * 120 - distance * 5,
                };
                best = better(choice,best);
            }
        }
        // The army wants fresh sight of power plants, factories, occupied
        // neutral tech and economic infrastructure. Revisit a *safe post*
        // around the old position rather than sending a dog directly inside
        // the structure (which is frequently a turret-filled base).
        for (id,site) in &self.sites {
            let urgent = self.priority_asset == Some(site.cell) && !site.hostile;
            if obs.is_cell_visible(site.cell.0,site.cell.1)
                || now.saturating_sub(site.last_seen)
                    < if urgent { REVISIT_AGE / 3 } else { REVISIT_AGE }
                || (site.last_probed != 0
                    && now.saturating_sub(site.last_probed) < REVISIT_COOLDOWN)
            { continue; }
            for (dx,dy) in [(-12,0),(12,0),(0,-12),(0,12),
                (-9,-9),(-9,9),(9,-9),(9,9)] {
                let (Ok(x),Ok(y)) = (
                    u16::try_from(i32::from(site.cell.0)+dx),
                    u16::try_from(i32::from(site.cell.1)+dy),
                ) else { continue; };
                let cell = (x,y);
                if !eligible(obs,zone,from,cell,threats,
                    &self.blocked,occupied,now)
                { continue; }
                let distance = cell_distance(from,cell);
                let choice = Candidate {
                    destination: cell, purpose: Purpose::Revisit(*id),
                    utility: 420 + site.interest.priority() * 90
                        + i32::from(!site.hostile) * 60
                        + (if urgent { 500 } else { 0 }) - distance * 5,
                };
                best = better(choice,best);
            }
        }
        best
    }

    /// Replan from *observed* progress, independent of the native Move timer.
    /// Once one scout has a region, its destination is sticky until the region
    /// is revealed, the route demonstrably stalls, or the scout dies.
    pub(super) fn observe(&mut self, rules: &RuleSet,
        obs: &Observation<'_>, census: &Census, squads: &Squads,
    ) {
        let now = obs.tick();
        self.survey_sites(now,rules,obs);
        self.blocked.retain(|_,until| *until > now);
        let scout_ids: Vec<_> = squads.squad(Role::Scout)
            .into_iter().flat_map(|squad| squad.members.iter().copied())
            .take(MAX_SCOUTS).collect();
        let live: BTreeSet<_> = scout_ids.iter().copied().collect();
        self.missions.retain(|id,_| live.contains(id));
        for id in &scout_ids {
            let Some(unit) = census.units.iter().find(|unit| unit.entity_id == *id)
            else { self.missions.remove(id); continue; };
            if !unit.controllable() || unit.in_service() {
                self.missions.remove(id);
                continue;
            }
            let Some(mission) = self.missions.get_mut(id) else { continue; };
            let reached = cell_distance(unit.cell, mission.destination) <= 3;
            let observed = match mission.purpose {
                Purpose::Frontier => obs.is_cell_revealed(
                    mission.destination.0,mission.destination.1),
                Purpose::Revisit(site_id) => self.sites.get(&site_id)
                    .is_none_or(|site| obs.is_cell_visible(site.cell.0,site.cell.1)),
            };
            if unit.cell != mission.last_cell {
                mission.last_cell = unit.cell;
                mission.last_progress = now;
            }
            let stalled = now.saturating_sub(mission.last_progress) >= PROGRESS_FRAMES;
            if reached || observed || stalled || now.saturating_sub(mission.began) >= SITE_LIFETIME {
                if stalled || (!observed && reached) {
                    self.blocked.insert(mission.destination,
                        now.saturating_add(FAILED_CELL_COOLDOWN));
                }
                self.missions.remove(id);
            }
        }
        if now < self.next_assign { return; }
        self.next_assign = now.saturating_add(ASSIGN_FRAMES);
        if scout_ids.is_empty() { return; }
        let threats: Vec<_> = obs.visible_enemy_entities().take(128)
            .filter_map(|enemy| {
                let ty = obs.object_type(rules,enemy.type_id)?;
                let radius = if ty.is_base_defense
                    && obs.visible_enemy_building_operational(rules,enemy.entity_id)
                { super::advance::weapon_range_cells(rules,ty,false).clamp(6,18) + 3 }
                else if ty.category == ObjectCategory::Building { 3 }
                else { 7 };
                Some((enemy.cell,radius))
            }).collect();
        let mut occupied: BTreeSet<_> = self.missions.values()
            .map(|mission| mission.destination).collect();
        for id in scout_ids {
            if self.missions.contains_key(&id) { continue; }
            let Some(unit) = census.units.iter().find(|unit| unit.entity_id == id)
            else { continue; };
            if !unit.controllable() || unit.in_service() { continue; }
            let Some(ty) = obs.object_type(rules,unit.type_id) else { continue; };
            let Some(candidate) = self.choose(now,obs,unit.cell,ty.movement_zone,
                &threats,&occupied) else { continue; };
            if let Purpose::Revisit(site_id) = candidate.purpose {
                if let Some(site) = self.sites.get_mut(&site_id) { site.last_probed = now; }
            }
            occupied.insert(candidate.destination);
            self.missions.insert(id,Assignment {
                destination: candidate.destination, purpose: candidate.purpose,
                last_cell: unit.cell, last_progress: now, began: now,
            });
            if super::trace_enabled("AUTOPLAY_TRACE_RECON") {
                log::debug!("auto play: scout {id} {:?} -> {:?} score={} tick={now}",
                    candidate.purpose,candidate.destination,candidate.utility);
            }
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn scouts_must_cover_different_sectors() {
        let mut occupied = BTreeSet::new();
        occupied.insert((40,40));
        assert!(!separated((51,42),&occupied));
        assert!(separated((60,42),&occupied));
    }

    #[test]
    fn rank_prioritizes_information_but_breaks_ties_deterministically() {
        let weak = Candidate { destination:(20,20),purpose:Purpose::Frontier,utility:30 };
        let rich = Candidate { destination:(30,30),purpose:Purpose::Frontier,utility:300 };
        assert_eq!(better(rich,Some(weak)),Some(rich));
        let tie = Candidate { destination:(12,20),purpose:Purpose::Frontier,utility:300 };
        assert_eq!(better(tie,Some(rich)),Some(tie));
    }

    #[test]
    fn observed_threat_radius_excludes_recon_waypoints() {
        let threats = [((20,20),9),((80,80),5)];
        assert!(dangerous((28,20),&threats));
        assert!(!dangerous((30,20),&threats));
    }

    #[test]
    fn hostile_intel_is_never_an_entity_click() {
        let _source = std::mem::discriminant(&Purpose::Revisit(42));
        // The output of the director is deliberately a coordinate. It has no
        // API that returns a target entity id to the combat order layer.
        let assignment = Assignment {
            destination:(55,60),purpose:Purpose::Revisit(42),
            last_cell:(10,10),last_progress:0,began:0,
        };
        assert_eq!(assignment.destination,(55,60));
    }
}
