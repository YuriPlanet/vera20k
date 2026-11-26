//! V39: rule-driven answers to a visible powered defensive battery.
//!
//! The gun line is a production problem before it is a movement problem.
//! Keep selecting a legal standoff gun (including fragile V3/Prism vehicles),
//! a safely approached aircraft, or genuinely heavy armour until a *bounded*
//! counter group exists. A tank with a legal Attack is not automatically a
//! profitable tank to send through a longer-ranged tower's envelope.
//!
//! Every enemy fact here comes from the current player's visible projection.
//! A lost sighting is reconnaissance intelligence, never an Attack target.

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::{CanBuild, ProductionCategory};

use super::AssistBrain;
use super::build;
use super::orders;
use super::squads::Role;
use super::tracked::TrackedKind;
use super::advance::cell_distance;

const BATTERY_LOOK: usize = 160;
const COUNTER_CAP: usize = 4;
const AIR_RAID_ORDERS: usize = 3;

/// Maximum native weapon range, not `Range - MinimumRange`: the latter
/// underestimates both V3 (18/5) and Grand Cannon (15/3) when deciding who
/// can shoot *first*. MinimumRange is tested independently at fire time.
pub(super) fn max_range_cells(weapon: &crate::rules::weapon_type::WeaponType) -> i32 {
    let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
    weapon.range_leptons.max(0).div_euclid(per_cell)
        + i32::from(weapon.range_leptons.max(0).rem_euclid(per_cell) != 0)
}

pub(super) fn ground_reach(rules: &RuleSet, ty: &ObjectType) -> i32 {
    (0..ty.weapon_record_count()).filter_map(|slot| {
        let weapon = ty.weapon_at(slot).and_then(|name| rules.weapon(name))?;
        (!weapon.never_use && weapon.damage > 0
            && rules.weapon_projectile(weapon).is_some_and(|p| p.ag))
            .then_some(max_range_cells(weapon))
    }).max().unwrap_or(0)
}


/// Some powered defenses (the retail Yuri psychic tower in particular) have
/// no IsBaseDefense flag and deal control instead of direct HP damage. Their
/// weapon range still blocks ordinary short-range advances.
pub(super) fn dangerous_battery(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.category == ObjectCategory::Building
        && (ty.is_base_defense || super::tactics::mind_control_reach(rules, ty).is_some())
}

pub(super) fn battery_reach(rules: &RuleSet, ty: &ObjectType) -> i32 {
    ground_reach(rules, ty)
        .max(super::tactics::mind_control_reach(rules, ty).unwrap_or(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Counter { Standoff, Air, Heavy }

#[derive(Debug, Clone, Copy)]
pub(super) struct Battery {
    pub(super) id: u64,
    pub(super) cell: (u16, u16),
    pub(super) reach: i32,
    pub(super) nearby_guns: usize,
}

fn is_flying(ty: &ObjectType) -> bool {
    ty.category == ObjectCategory::Aircraft || ty.balloon_hover
        || ty.jumpjet || ty.movement_zone == MovementZone::Fly
}

/// The retail V3 rangefinder has `Damage=1`, `Spawner=yes` and the
/// placeholder `Warhead=Special`: damage is done later by the spawned
/// rocket's `V3RocketDamage/V3Warhead`. A direct warhead check on the
/// rangefinder would discard precisely the long-range siege unit we need.
/// Resolve the *actual spawned missile* from the live rules instead of
/// relying on retail object names. Non-missile spawners retain their normal
/// weapon interaction semantics.
pub(super) fn effective_ground_warhead(
    rules: &RuleSet, launcher: &ObjectType,
    weapon: &crate::rules::weapon_type::WeaponType,
    target: &ObjectType,
) -> bool {
    let normal_head = || weapon.warhead.as_deref().and_then(|n| rules.warhead(n));
    let interaction = |head: &crate::rules::warhead_type::WarheadType| {
        !head.mind_control && !head.is_locomotor && !head.ivan_bomb && !head.parasite
            && head.interaction(target.armor_id()).can_fire()
            && f64::from_bits(head.interaction(target.armor_id()).multiplier().bits()) > 0.0
    };
    if weapon.spawner {
        if let Some(family) = launcher.spawns.as_deref().filter(|s| !s.is_empty())
            .and_then(|s| rules.missile_spawn.family_of(s))
        {
            let rocket = rules.missile_spawn.params(family);
            return rocket.damage > 0 && rules.warhead(&rocket.warhead)
                .is_some_and(interaction);
        }
    }
    normal_head().is_some_and(interaction)
}

fn valid_ground_reach(rules: &RuleSet, ty: &ObjectType, target: &ObjectType) -> i32 {
    if ty.gunner { return 0; } // Gunner uses its *current* occupant's weapon, not every slot.
    (0..ty.weapon_record_count()).filter_map(|slot| {
        let gun = ty.weapon_at(slot).and_then(|name| rules.weapon(name))?;
        if gun.never_use || gun.suicide || gun.damage <= 0
            || !rules.weapon_projectile(gun).is_some_and(|shot| shot.ag)
            || !effective_ground_warhead(rules,ty,gun,target)
        { return None; }
        let range = max_range_cells(gun);
        // A launcher with MinimumRange >= Range has no usable ring against
        // this object even though its maximum range looks attractive.
        (range > 0 && gun.minimum_range_leptons < gun.range_leptons)
            .then_some(range)
    }).max().unwrap_or(0)
}

fn viable(rules: &RuleSet, ty: &ObjectType, target: &ObjectType,
    defense_reach: i32, air_ok: bool) -> Option<(Counter, i32)> {
    if ty.naval || ty.speed <= 0 || ty.strength <= 0
        || ty.resource_gatherer || ty.construction_yard || ty.is_base_defense
        || !orders::is_combat_unit(rules, ty)
    { return None; }
    let range = valid_ground_reach(rules, ty, target);
    if range == 0 { return None; }
    if is_flying(ty) {
        // A balloon-hovering VehicleType uses the *vehicle* factory; a
        // Harrier uses an aircraft factory. Both need real anti-ground fire.
        return air_ok.then_some((Counter::Air, range));
    }
    if range > defense_reach + 1 {
        return Some((Counter::Standoff, range));
    }
    // Heavy armour is an explicit fallback, not another cheap IFV. If it
    // cannot trade at range, it must withstand a synchronized short assault.
    (ty.category == ObjectCategory::Vehicle && ty.strength >= 600)
        .then_some((Counter::Heavy, range))
}

/// The high-value tower currently visible in the *same* strategic theatre as
/// the attacking team. A faraway isolated gun does not consume the economy.
/// No perception memory is used to look up invisible entity types or status.
pub(super) fn focus(brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>) -> Option<Battery> {
    let front = brain.push_target.map(|(_, _, cell)| cell);
    let squad = brain.squads.squad(Role::Attack);
    let front_units: Vec<_> = brain.census.units.iter().filter(|unit|
        squad.is_some_and(|s| s.members.contains(&unit.entity_id)))
        .map(|unit| unit.cell).take(96).collect();
    let mut guns: Vec<_> = obs.visible_enemy_entities()
        .filter_map(|enemy| {
            let ty = obs.entity_type(rules, enemy.entity_id)?;
            if !dangerous_battery(rules, ty) || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            { return None; }
            let reach = battery_reach(rules, ty);
            if reach < 7 { return None; }
            let near_front = front.is_some_and(|cell| cell_distance(cell, enemy.cell) <= 22);
            let near_army = front_units.iter().any(|cell|
                cell_distance(*cell, enemy.cell) <= 30);
            let mining_incident=brain.mine_incidents.get(&enemy.entity_id)
                .is_some_and(|(_,end)|obs.tick()<*end);
            (near_front || near_army || mining_incident)
                .then_some((enemy.entity_id, enemy.cell, reach))
        }).take(BATTERY_LOOK).collect();
    if guns.is_empty() { return None; }
    // Prefer the battery covering the current push; then its actual reach.
    guns.sort_unstable_by_key(|(id,cell,reach)| (
        !brain.mine_incidents.get(id).is_some_and(|(_,end)|obs.tick()<*end),
        front.map_or(0, |at| cell_distance(*cell, at)),
        std::cmp::Reverse(*reach), *id));
    let (id, cell, reach) = guns[0];
    let nearby_guns = guns.iter().filter(|(_, at, _)| cell_distance(*at, cell) <= 16).count();
    Some(Battery { id, cell, reach, nearby_guns })
}

/// An enemy AA weapon is relevant only if it has a real AA projectile and a
/// warhead that can affect this *candidate aircraft's* armour. Unpowered
/// defenses are not counted as operational firing positions.
fn anti_air_reach(rules: &RuleSet, gun: &ObjectType, plane: &ObjectType) -> i32 {
    (0..gun.weapon_record_count()).filter_map(|slot| {
        let weapon = gun.weapon_at(slot).and_then(|name| rules.weapon(name))?;
        if weapon.never_use || !rules.weapon_projectile(weapon).is_some_and(|shot| shot.aa)
            || !weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
                .is_some_and(|head| f64::from_bits(
                    head.interaction(plane.armor_id()).multiplier().bits()) > 0.0)
        { return None; }
        Some(max_range_cells(weapon))
    }).max().unwrap_or(0)
}

fn air_corridor_known(obs: &Observation<'_>, at: (u16,u16)) -> bool {
    // Visible tower != proved absence of AA outside the scout's view.
    // Require observations from at least two approaches around the tower.
    let surveyed = [(7,0),(-7,0),(0,7),(0,-7)]
        .into_iter().filter_map(|(dx,dy)| {
            let x = u16::try_from(i32::from(at.0)+dx).ok()?;
            let y = u16::try_from(i32::from(at.1)+dy).ok()?;
            obs.is_cell_in_playfield(x,y).then_some(obs.is_cell_visible(x,y))
        });
    surveyed.filter(|seen| *seen).take(2).count() >= 2
}

/// `from`: actual air unit's position when issuing Attack, or None when
/// evaluating a *future* build. Known guns on the approach must be respected.
fn air_approach_clear(rules: &RuleSet, obs: &Observation<'_>, battery: Battery,
    plane: &ObjectType, from: Option<(u16,u16)>) -> bool {
    if !air_corridor_known(obs, battery.cell) { return false; }
    !obs.visible_enemy_entities().any(|enemy| {
        let Some(ty) = obs.entity_type(rules, enemy.entity_id) else { return false; };
        if ty.is_base_defense && !obs.visible_enemy_building_operational(rules, enemy.entity_id) {
            return false;
        }
        let reach = anti_air_reach(rules, ty, plane);
        reach > 0 && (cell_distance(enemy.cell, battery.cell) <= reach + 2
            || from.is_some_and(|start| orders::path_crosses(
                &[enemy.cell], start, battery.cell, reach + 2)))
    })
}

#[derive(Clone, Copy)]
struct Choice<'r> {
    ty: &'r ObjectType,
    id: InternedId,
    category: ProductionCategory,
    kind: Counter,
    range: i32,
    price: i32,
}

/// Evaluate all *currently native-buildable* aircraft and ground vehicles,
/// before choosing which factory may spend this frame. A Soviet V3 and an
/// Allied Prism can both win a range trade despite only 150 HP; the V33
/// `strength>=300` filter accidentally excluded both from the build list.
fn choice<'r>(brain: &AssistBrain, rules: &'r RuleSet,
    obs: &Observation<'_>, battery: Battery, free:i32) -> Option<Choice<'r>> {
    let tower = obs.entity_type(rules, battery.id)?;
    let mut options = Vec::new();
    for (category, line) in [
        (ObjectCategory::Vehicle, ProductionCategory::Vehicle),
        (ObjectCategory::Aircraft, ProductionCategory::Aircraft),
    ] {
        if brain.census.line(line).busy() {continue;}
        for (ty, id) in build::all_class_types(rules, obs, category) {
            if obs.can_build(rules, ty, true) != CanBuild::Yes || !obs.has_producer(rules, ty)
                || !super::composition::air_stock_allows(rules,obs,ty) { continue; }
            let clear = is_flying(ty) && air_approach_clear(rules,obs,battery,ty,None);
            let Some((kind,range)) = viable(rules,ty,tower,battery.reach,clear) else { continue; };
            // This module should not pretend to have a production facility
            // just because a prerequisite name was satisfied.
            if category == ObjectCategory::Aircraft && !obs.own_entities().any(|entity|
                obs.object_type(rules, entity.type_id).is_some_and(|building|
                    building.factory_type() == Some(
                        crate::rules::object_type::FactoryType::AircraftType))) { continue; }
            let price = obs.cost_of(rules,ty);
            if price<=0 || price>free || obs.in_production(rules,id) {continue;}
            options.push(Choice { ty,id,category:line,kind,range,price });
        }
    }
    // True winning-range weapons outrank hazardous air runs. When no ground
    // standoff is obtainable, *observably* clear airspace beats a frontal
    // armour exchange. Heavy armour is the last, assembled option.
    options.into_iter().max_by_key(|option| {
        let priority = match option.kind { Counter::Standoff => 3,
            Counter::Air => 2, Counter::Heavy => 1 };
        (priority, option.range,
            if option.kind == Counter::Heavy { option.ty.strength } else { 0 },
            std::cmp::Reverse(brain.census.type_count(option.id)),
            std::cmp::Reverse(option.price), std::cmp::Reverse(option.id))
    })
}

fn desired_count(kind: Counter, reach: i32, towers: usize) -> usize {
    let base: usize = match kind { Counter::Standoff => 2,
        Counter::Air => 3, Counter::Heavy => 6 };
    (base + usize::from(reach >= 11) + usize::from(towers >= 3)).min(
        if kind == Counter::Heavy { 8 } else { COUNTER_CAP })
}

/// Count *the whole counter category* instead of only one unit name. Owning
/// two different standoff cannons is as useful as two identical cannons.
fn counter_held(brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, battery: Battery, kind: Counter) -> usize {
    let Some(tower) = obs.entity_type(rules, battery.id) else { return 0; };
    brain.census.units.iter().filter(|unit| unit.traits.combat && !unit.traits.spawned
        && !unit.hand && unit.health_permille > 0 && !unit.traits.harvester
        && !unit.traits.yard_maker)
        .filter(|unit| obs.entity_type(rules,unit.entity_id).is_some_and(|ty| {
            // Stock is not the same as an immediately usable sortie. An
            // aircraft rearming or on a different route still occupies its
            // procurement allocation; readiness is evaluated by air orders.
            let clear = kind == Counter::Air && is_flying(ty);
            viable(rules,ty,tower,battery.reach,clear)
                .is_some_and(|(k,_)| k == kind)
        })).take(12).count()
}

/// A siege battalion cannot buy a Prism/V3/other advanced standoff cannon
/// until its *native* prerequisite structure exists. Without a persistent
/// research request the cheap barracks and factory cycles exhaust the wallet
/// before any laboratory becomes affordable.
pub(super) fn technology_step<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
) -> Option<super::plan::Chosen<'r>> {
    if brain.mobilization.active() || brain.census.fighters < 10 || brain.census.refineries == 0
        || brain.census.harvesters == 0 || brain.director.under_attack_now()
        || brain.director.recovering_income()
        || build::count_in(obs, rules, &brain.census, &rules.build_tech_types) > 0
    { return None; }
    let battery = focus(brain, rules, obs)?;
    let tower = obs.entity_type(rules, battery.id)?;
    let already_ranged = brain.census.units.iter().any(|unit| unit.health_permille > 0 && !unit.traits.spawned
        && obs.entity_type(rules, unit.entity_id)
            .is_some_and(|ty| valid_ground_reach(rules, ty, tower) > battery.reach + 1));
    if already_ranged { return None; }
    let (item, selected) = if let Some(lab) = build::tech_building(rules, obs) {
        (super::plan::Item::Tech, lab)
    } else if build::count_in(obs, rules, &brain.census, &rules.build_radar_types) == 0 {
        (super::plan::Item::Radar,
            build::first_buildable(rules, obs, &rules.build_radar_types)?)
    } else { return None; };
    Some(super::plan::Chosen {
        step: None, item, ty: selected.0, type_id: selected.1,
    })
}

/// Single owner for counter funding and purchase. A busy line, already held
/// stock, impossible funding horizon or current emergency returns no request.
/// This is a proposal, not an accepted native purchase.
pub(super) fn procurement<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
    free: i32, income: Option<i32>,
) -> Option<(ProductionCategory, &'r ObjectType, InternedId)> {
    if brain.mobilization.active() || brain.census.refineries == 0
        || brain.census.harvesters == 0 || brain.director.under_attack_now()
        || brain.director.recovering_income() || brain.director.recovering_under_fire()
        || brain.campaign.economy_resting() || brain.siege_charge.is_some() {
        return None;
    }
    let battery = focus(brain,rules,obs)?;
    if counter_held(brain,rules,obs,battery,Counter::Standoff)
        >=desired_count(Counter::Standoff,battery.reach,battery.nearby_guns) {
        return None;
    }
    let picked = choice(brain,rules,obs,battery,free)?;
    if picked.kind==Counter::Heavy
        && counter_held(brain,rules,obs,battery,Counter::Air)
            >=desired_count(Counter::Air,battery.reach,battery.nearby_guns) {
        return None;
    }
    if brain.census.line(picked.category).busy()
        || obs.in_production(rules,picked.id)
        || !super::economy::can_save_for(free,picked.price,income)
        || counter_held(brain,rules,obs,battery,picked.kind)
            >= desired_count(picked.kind,battery.reach,battery.nearby_guns) {
        return None;
    }
    Some((picked.category,picked.ty,picked.id))
}

/// Prevent per-second micro from breaking the siege gate by giving a
/// short-ranged line unit a solo Attack on a live tower. Real synchronized
/// heavy charges and emergency defence remain owned by their commanders.
pub(super) fn avoid_solo_tower_attack(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64, target: u64,
) -> bool {
    let Some(tower) = obs.entity_type(rules,target) else { return false; };
    if !dangerous_battery(rules, tower) || !obs.visible_enemy_building_operational(rules,target) {
        return false;
    }
    let Some(actor_ty) = obs.entity_type(rules,actor) else { return false; };
    if is_flying(actor_ty) { return false; }
    let hostile_range = super::tactics::actual_threat_reach(rules,obs,target,actor)
        .max(battery_reach(rules, tower));
    let friendly_range = super::tactics::actual_threat_reach(rules,obs,actor,target);
    hostile_range >= 7 && friendly_range <= hostile_range + 1
}

/// A proposal to move into a safe firing annulus or fire from it. The
/// native mission remains authoritative. Never replace a not-yet-in-range
/// Move with Attack, which would restart the chase through the gunline.
pub(super) fn work_battery(tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
    unit: &super::census::Unit, target: u64, at: (u16,u16), search: bool) -> bool {
    let id = unit.entity_id;
    if super::safety::firing_here(rules,obs,unit,target) {
        if obs.attack_target(id) == Some(target) { return false; }
        return orders::issue_tracked(tracker,trace,out,rules,obs,id,
            TrackedKind::Attack,Some(target),Some(at),Goal::Attack {target});
    }
    if let Some(post) = super::safety::firing_post(rules,obs,unit,target,at,search) {
        if obs.destination(id) == Some(post) && obs.attack_target(id).is_none() { return false; }
        if orders::issue_tracked(tracker,trace,out,rules,obs,id,
            TrackedKind::Avoid,Some(target),Some(post),Goal::Move {cell:post}) {
            log::info!("auto play: siege_post_request tick={} actor={} target={}",obs.tick(),id,target);
            return true;
        }
    } else if obs.attack_target(id) == Some(target) {
        // A prior legal shot may become an unsafe chase after a target moves
        // or the weapon is no longer in range. Stop the old pursuit.
        return orders::issue_tracked(tracker,trace,out,rules,obs,id,
            TrackedKind::Avoid,None,None,Goal::Stop);
    }
    false
}

/// Shell a live fixed battery *during* Stockpile/Contain, from a real
/// winning-range annulus. Reservists with short-range cannons remain behind
/// the screen; artillery which already reached its safe firing post should
/// not sit there idle waiting for the whole army to be released.
pub(super) fn standoff_strike(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    if brain.director.under_attack_now() { return; }
    let Some(battery) = focus(brain,rules,obs) else { return; };
    let Some(attack) = brain.squads.squad(Role::Attack) else { return; };
    let members = attack.members.clone();
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= 3 { break; }
        if !members.contains(&unit.entity_id) || brain.joint_ops.reserves(unit.entity_id)
            || !brain.census.main_available(unit)
            || unit.health_permille < 500
            || obs.is_deployed(unit.entity_id)
            || orders::ordered_this_frame(out,unit.entity_id)
        { continue; }
        let Some(ty) = obs.entity_type(rules,unit.entity_id) else { continue; };
        if is_flying(ty) || ty.naval || ty.gunner { continue; }
        let Some(tower) = obs.entity_type(rules,battery.id) else { continue; };
        let max = valid_ground_reach(rules,ty,tower);
        if max <= battery.reach + 1 || !obs.can_hurt(rules,unit.entity_id,battery.id) { continue; }
        let preparing=brain.campaign.preparation().is_some_and(|p|
            cell_distance(p.front,battery.cell)<=16);
        if preparing && !super::safety::firing_here(rules,obs,unit,battery.id) {continue;}
        let search = !brain.director.recovering_under_fire() && !brain.mobilization.active() && !brain.campaign.economy_resting()
            && !preparing && obs.tick()%15 == unit.entity_id%15;
        // During recovery preserve safe fire, but do not advance the gun.
        if (brain.director.recovering_under_fire() || brain.mobilization.active() || brain.campaign.economy_resting())
            && !super::safety::firing_here(rules,obs,unit,battery.id) { continue; }
        if work_battery(&mut brain.tracked,brain.trace,rules,obs,out,
            unit,battery.id,battery.cell,search) { issued += 1; }
    }
}

/// Aircraft are not ground tanks: direct fire on the currently visible tower
/// can occur while short-range armour stays staged. The real L2 Attack and
/// flight engine still decide whether a sortie launches and hits.
pub(super) fn air_strike(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    if brain.director.under_attack_now() || brain.director.recovering_under_fire()
        || brain.mobilization.active() || brain.campaign.economy_resting() { return; }
    let Some(battery) = focus(brain,rules,obs) else { return; };
    let Some(squad) = brain.squads.squad(Role::Attack) else { return; };
    let attack_members = squad.members.clone();
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= AIR_RAID_ORDERS { break; }
        if !attack_members.contains(&unit.entity_id)
            || brain.joint_ops.reserves(unit.entity_id)
            || !brain.census.main_available(unit)
            || super::orders::ordered_this_frame(out, unit.entity_id)
            || obs.is_deployed(unit.entity_id)
        { continue; }
        let Some(ty) = obs.entity_type(rules,unit.entity_id) else { continue; };
        if !is_flying(ty) || !air_approach_clear(rules,obs,battery,ty,Some(unit.cell))
            || !obs.can_hurt(rules,unit.entity_id,battery.id)
            || obs.attack_target(unit.entity_id) == Some(battery.id)
        { continue; }
        let goal = Goal::Attack { target: battery.id };
        if !orders::admits(rules,obs,unit.entity_id,&goal) { continue; }
        if orders::issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
            unit.entity_id,TrackedKind::Attack,Some(battery.id),Some(battery.cell),goal)
        { issued += 1; }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn psychic_tower_is_a_real_battery_even_without_is_base_defense() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let controller = rules.object("YAPSYT").expect("psychic tower");
        assert!(!controller.is_base_defense);
        assert!(dangerous_battery(&rules, controller));
        assert!(battery_reach(&rules, controller) >= 7);
        let prism = rules.object("ATESLA").expect("prism tower");
        assert!(dangerous_battery(&rules, prism));
        assert!(battery_reach(&rules, prism) >= 8);
    }

    #[test]
    fn retail_fragile_v3_and_prism_are_standoff_answers() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let tower = rules.object("ATESLA").expect("prism tower");
        let v3 = rules.object("V3").expect("v3 launcher");
        let prism = rules.object("SREF").expect("prism tank");
        let reach = ground_reach(&rules,tower);
        assert_eq!(viable(&rules,v3,tower,reach,false).map(|(k,_)| k),
            Some(Counter::Standoff));
        assert_eq!(viable(&rules,prism,tower,reach,false).map(|(k,_)| k),
            Some(Counter::Standoff));
    }

    #[test]
    fn grand_cannon_demands_true_standoff_not_line_tanks() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let tower = rules.object("GTGCAN").expect("grand cannon");
        let v3 = rules.object("V3").expect("v3 launcher");
        let apoc = rules.object("APOC").expect("apocalypse tank");
        let reach = ground_reach(&rules,tower);
        assert_eq!(viable(&rules,v3,tower,reach,false).map(|(k,_)| k),
            Some(Counter::Standoff));
        assert_eq!(viable(&rules,apoc,tower,reach,false).map(|(k,_)| k),
            Some(Counter::Heavy));
        assert_eq!(reach,15);
        assert_eq!(valid_ground_reach(&rules,v3,tower),18);
        let rangefinder = rules.weapon("V3Launcher").expect("V3 spawner");
        assert!(rangefinder.spawner);
        assert!(effective_ground_warhead(&rules,v3,rangefinder,tower),
            "the 200-damage spawned rocket's warhead, not Special, decides armour");
        assert_eq!(desired_count(Counter::Standoff,reach,1),3);
        assert_eq!(super::super::advance::weapon_reach_cells(
            rules.weapon("V3Launcher").expect("v3 launcher")),18);
        assert_eq!(super::super::advance::weapon_reach_cells(
            rules.weapon("GrandCannonWeapon").expect("grand cannon")),15);
    }

    #[test]
    fn magnetron_can_shake_prism_but_not_outrange_grand_cannon() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let magnetron = rules.object("TELE").expect("magnetron");
        let prism_tower = rules.object("ATESLA").expect("prism tower");
        let grand_cannon = rules.object("GTGCAN").expect("grand cannon");
        assert_eq!(valid_ground_reach(&rules,magnetron,prism_tower),10);
        assert_eq!(viable(&rules,magnetron,prism_tower,
            ground_reach(&rules,prism_tower),false).map(|(k,_)| k),
            Some(Counter::Standoff));
        assert_eq!(viable(&rules,magnetron,grand_cannon,
            ground_reach(&rules,grand_cannon),false),None);
    }

    #[test]
    fn air_answer_requires_observed_airspace_clearance() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let tower = rules.object("ATESLA").expect("prism tower");
        let harrier = rules.object("ORCA").expect("harrier");
        let apoc = rules.object("APOC").expect("apocalypse");
        let reach = ground_reach(&rules,tower);
        assert_eq!(viable(&rules,harrier,tower,reach,false),None);
        assert_eq!(viable(&rules,harrier,tower,reach,true).map(|(k,_)| k),
            Some(Counter::Air));
        assert_eq!(viable(&rules,apoc,tower,reach,false).map(|(k,_)| k),
            Some(Counter::Heavy));
        assert!(anti_air_reach(&rules, rules.object("NASAM").expect("patriot"),harrier)>0);
    }
}
