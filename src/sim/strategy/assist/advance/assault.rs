//! V49: bounded, rule-derived forecast for one *concentrated* armoured wave.
//! This is an AutoPlay heuristic, not a replica of native combat resolution.
//! Enemy inputs are current visible entities only. Unknown effects are a reason
//! to choose standoff or another front, not free damage for our tanks.
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::control::Goal;
use crate::sim::control::observation::{EnemyEntity, Engage, Observation};
use super::super::{orders, siege_counter, tactics, census::{Slot, Unit}};
use super::cell_distance;

pub(super) const MIN_COLUMN: usize = 3;
pub(super) const MAX_COLUMN: usize = 24;
pub(super) const MAX_COHORT_SEARCH: usize = 48;

/// Candidates are already ordered by distance to the visible battery and ID.
/// Prefer the densest actual local group; proximity breaks ties. Never use
/// all-map army size as a substitute for a physically assembled wave.
pub(super) fn cohesive_leader(cells: &[(u16,u16)]) -> Option<(u16,u16)> {
    let pool=&cells[..cells.len().min(MAX_COHORT_SEARCH)];
    pool.iter().copied().enumerate()
        .max_by_key(|(index,cell)|(
            pool.iter().filter(|other|cell_distance(**other,*cell)<=8).count().min(MAX_COLUMN),
            std::cmp::Reverse(*index)))
        .map(|(_,cell)|cell)
}
const MAX_THREATS: usize = 32;

#[derive(Clone, Copy, Debug)]
struct Fire { dps_milli: i64, volley: i64, reach: i32, splash: bool }

fn direct_weapon(rules: &RuleSet, weapon: &WeaponType, target: &ObjectType) -> Option<Fire> {
    if weapon.never_use || weapon.damage <= 0 || weapon.spawner || weapon.suicide
        || weapon.area_fire || weapon.minimum_range_leptons >= weapon.range_leptons
        || !rules.weapon_projectile(weapon).is_some_and(|p| p.ag) { return None; }
    let head = rules.warhead(weapon.warhead.as_deref()?)?;
    if head.mind_control || head.parasite || head.ivan_bomb || head.is_locomotor { return None; }
    let verses = head.interaction(target.armor_id());
    let mult = f64::from_bits(verses.multiplier().bits());
    if !verses.can_fire() || !mult.is_finite() || mult <= 0.0 { return None; }
    let volley = ((f64::from(weapon.damage) * mult).clamp(0.0, 10_000.0) *
        f64::from(weapon.burst.clamp(1, 16))) as i64;
    let dps_milli = volley.saturating_mul(1000) / i64::from(weapon.rof.max(1));
    (dps_milli > 0).then_some(Fire {dps_milli, volley,
        reach: siege_counter::max_range_cells(weapon), splash: head.cell_spread_f64 > 0.5})
}

fn fire(rules: &RuleSet, obs: &Observation<'_>, actor: u64, ty: &ObjectType,
    victim: &ObjectType) -> Option<Fire> {
    if ty.gunner { return direct_weapon(rules,obs.active_gunner_weapon(rules,actor)?,victim); }
    (0..ty.weapon_record_count()).filter_map(|slot| {
        direct_weapon(rules,rules.weapon(ty.weapon_at(slot)?)?,victim)
    }).max_by_key(|f| (f.dps_milli,f.reach))
}

pub(super) fn eligible(rules: &RuleSet, obs: &Observation<'_>, unit: &Unit,
    battery: EnemyEntity) -> bool {
    if !unit.controllable() || unit.in_service() || unit.slot != Slot::Vehicle
        || !unit.traits.combat || unit.traits.yard_maker || unit.traits.harvester
        || unit.traits.irreplaceable || unit.traits.carries || unit.traits.suicide
        || unit.health_permille < 700 || obs.is_deployed(unit.entity_id) { return false; }
    let Some(ty) = obs.entity_type(rules,unit.entity_id) else { return false; };
    let Some(target) = obs.entity_type(rules,battery.entity_id) else { return false; };
    if ty.naval || ty.balloon_hover || ty.jumpjet || ty.speed <= 0 || ty.strength < 300
        || !orders::admits(rules,obs,unit.entity_id,&Goal::Attack {target:battery.entity_id})
        || fire(rules,obs,unit.entity_id,ty,target).is_none() { return false; }
    obs.engage(rules,unit.entity_id,battery.entity_id) == Engage::Ready
        || orders::building_approachable(obs,ty.movement_zone,unit.cell,battery.cell)
}

/// A rough upper allowance for travel before the last tank can fire. Retail
/// Speed= is percentage of a cell/frame (see dodge.rs). Include turn/traffic
/// slack and use one cell less than the rounded-up nominal firing range.
fn approach_frames(distance: i32, range: i32, speed: i32) -> i64 {
    i64::from((distance - (range-1).max(1)).max(0)) * 150 / i64::from(speed.max(1)) + 20
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Forecast {
    pub(super) hp: i64,
    pub(super) target_hp: i32,
    pub(super) frames: u64,
    pub(super) approach: u64,
    pub(super) loss_hp: i64,
    pub(super) loss_credits: i64,
    pub(super) allowed: bool,
}

fn acceptable(n: usize, hp: i64, cost: i64, target_cost: i64, frames: i64, loss: i64) -> bool {
    n >= MIN_COLUMN && hp > 0 && frames <= 540 && loss.saturating_mul(100) <= hp.saturating_mul(45)
        && loss.saturating_mul(cost.max(1)) / hp <= target_cost.max(1)*2 + cost/10
}

/// Only evaluate the exact cohort to which the subsequent order is issued.
/// Damage is a conservative approximation: 65% of friendly nominal fire,
/// full hostile fire + one opening volley, splash/overlap allowance, and
/// no credit for unobserved powers, friendly repairs or future purchases.
pub(super) fn estimate(rules: &RuleSet, obs: &Observation<'_>, battery: EnemyEntity,
    ready: &[&Unit]) -> Option<Forecast> {
    if ready.len() < MIN_COLUMN || ready.len() > MAX_COLUMN
        || !obs.visible_enemy_entities().any(|e| e.entity_id == battery.entity_id)
        || obs.visible_enemy_invulnerable(battery.entity_id) { return None; }
    let target = obs.entity_type(rules,battery.entity_id)?;
    let target_hp = obs.health(battery.entity_id)?.max(1);
    let mut hp = 0i64;
    let mut cost = 0i64;
    let mut dps = 0i64;
    let mut travel = 0i64;
    for &u in ready {
        let ty = obs.entity_type(rules,u.entity_id)?;
        let gun = fire(rules,obs,u.entity_id,ty,target)?;
        hp += i64::from(obs.health(u.entity_id)?);
        cost += i64::from(obs.cost_of(rules,ty).max(1));
        dps += gun.dps_milli;
        let t = if obs.engage(rules,u.entity_id,battery.entity_id) == Engage::Ready {20}
            else {approach_frames(cell_distance(u.cell,battery.cell),gun.reach,ty.speed)};
        travel = travel.max(t);
    }
    if hp <= 0 || dps <= 0 { return None; }
    let effective_dps = (dps*65/100).max(1);
    let frames = travel + (i64::from(target_hp)*1000 + effective_dps-1)/effective_dps + 30;
    if frames > 540 { return None; }
    let mut incoming = 0i64;
    let mut volley = 0i64;
    let mut threats = 0usize;
    for enemy in obs.visible_enemy_entities() {
        let Some(ty) = obs.entity_type(rules,enemy.entity_id) else { continue; };
        let fixed = ty.category == ObjectCategory::Building;
        if fixed && !obs.visible_enemy_building_operational(rules,enemy.entity_id) { continue; }
        // Mobile reinforcements are not assumed to stay still. A distant
        // fixed gun cannot shoot this lane, but a nearby mobile one can.
        let reach = siege_counter::battery_reach(rules,ty);
        if cell_distance(enemy.cell,battery.cell) > (if fixed {reach+20} else {20+reach}) { continue; }
        let mut worst = 0i64;
        let mut opening = 0i64;
        for &u in ready {
            let victim = obs.entity_type(rules,u.entity_id)?;
            if let Some(control) = tactics::mind_control_reach(rules,ty) {
                if !victim.immune_to_psionics && !victim.immune_to_psionic_weapons
                    && tactics::path_enters_control_box(u.cell,battery.cell,enemy.cell,
                        control + if fixed {1} else {7}) { return None; }
            }
            let effective = tactics::effective_reach_against(rules,obs,enemy.entity_id,u.entity_id);
            if effective <= 0 || !tactics::path_enters_control_box(u.cell,battery.cell,
                enemy.cell,effective+if fixed {2} else {7}) { continue; }
            if !obs.can_hurt(rules,enemy.entity_id,u.entity_id) { continue; }
            // If a lethal/contact/spawn effect isn't described by direct DPS,
            // do not grant an optimistic blind assault through that threat.
            let Some(gun) = fire(rules,obs,enemy.entity_id,ty,victim) else { return None; };
            let splash = if gun.splash {150} else {100};
            worst = worst.max(gun.dps_milli*splash/100);
            opening = opening.max(gun.volley*splash/100);
        }
        if worst == 0 { continue; }
        threats += 1;
        if threats > MAX_THREATS { return None; }
        incoming += worst;
        volley += opening;
    }
    // An extra overlap allowance accounts for focus/support beams and
    // imperfect spacing, without assuming every tower fires at every tank.
    let loss = (incoming*frames/1000 + volley) * (if threats > 1 {125} else {100}) /100;
    Some(Forecast {hp,target_hp,frames:frames as u64,approach:travel as u64,
        loss_hp:loss,loss_credits:loss.saturating_mul(cost)/hp,
        allowed:acceptable(ready.len(),hp,cost,i64::from(obs.cost_of(rules,target)),frames,loss)})
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    #[test]
    fn a_viable_line_tank_group_is_not_required_to_be_heavy_armour() {
        // 8 ordinary 300-HP vehicles; 20% forecast HP loss for a useful tower.
        assert!(acceptable(8,2400,5600,1200,180,480));
        assert!(acceptable(3,1200,2400,1200,180,520));
        assert!(!acceptable(3,1200,2400,1200,180,550));
        assert!(!acceptable(2,2400,5600,1200,180,480));
        assert!(!acceptable(8,2400,5600,1200,180,1200));
        assert!(!acceptable(8,2400,5600,1200,541,200));
        // Even a survivable fight may be an unacceptable cash trade.
        assert!(!acceptable(8,2400,10000,100,180,700));
    }
    #[test]
    fn forecast_uses_native_weapon_armour_multiplier_burst_and_rof() {
        let rules=RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=TOWER\n\
             [TANK]\nStrength=300\nSpeed=6\nPrimary=Gun\nArmor=heavy\n\
             [TOWER]\nStrength=600\nFoundation=1x1\nArmor=steel\n\
             [Gun]\nDamage=100\nBurst=2\nROF=50\nRange=5\nWarhead=WH\nProjectile=Bullet\n\
             [Bullet]\nAG=yes\nAA=no\nInviso=yes\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,50%,100%,100%,100%\n"
        )).unwrap();
        let tower=rules.object("TOWER").unwrap();
        let gun=rules.weapon("Gun").unwrap();
        let fire=direct_weapon(&rules,gun,tower).unwrap();
        assert_eq!(fire.volley,100);
        assert_eq!(fire.dps_milli,2000);
    }
    #[test]
    fn a_nearest_straggler_cannot_hide_the_assembled_column() {
        let cells=[(20,20),(30,20),(31,21),(32,19),(33,20)];
        assert_eq!(cohesive_leader(&cells),Some((30,20)));
        assert_eq!(cohesive_leader(&[(1,1),(20,20)]),Some((1,1)));
        assert_eq!(cohesive_leader(&[]),None);
    }
    #[test]
    fn approach_cost_accounts_for_the_range_gap_and_speed() {
        assert!(approach_frames(12,5,6)>approach_frames(8,5,6));
        assert!(approach_frames(12,5,4)>approach_frames(12,5,8));
    }
}
