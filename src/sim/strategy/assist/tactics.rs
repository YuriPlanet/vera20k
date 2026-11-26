//! Visible threat geometry for expensive front-line decisions. Mind control
//! is not ordinary damage: a captured tank joins the enemy and starts shooting
//! us. Only current sight and live rule-defined warheads enter this analysis.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::{EnemyEntity, Observation};

use super::threats::distance;

/// The longest weapon range which may mind-control its victim, in cells.
/// `None` means the type has no such weapon, not that it is safe to shoot.
/// Non-DPS urgency of a **visible** threat. Suicide explosions, C4, mind
/// control and high-altitude bombers can destroy an economy before their
/// ordinary per-shot score becomes large. Read behaviors from actual rules;
/// do not special-case retail names. Time-limited invulnerability is an
/// instance state and is evaluated separately through Observation.
pub(super) fn emergency_threat_score(rules: &RuleSet, ty: &ObjectType) -> i32 {
    let suicide_weapon = (0..ty.weapon_record_count()).any(|slot| {
        ty.weapon_at(slot).and_then(|name| rules.weapon(name))
            .is_some_and(|weapon| weapon.suicide)
    });
    let bomb = suicide_weapon || (ty.explodes && ty.death_weapon.is_some());
    let air = ty.category == ObjectCategory::Aircraft || ty.balloon_hover || ty.jumpjet;
    let mind = mind_control_reach(rules, ty).is_some();
    if bomb { 1_200 }
    else if ty.c4 { 1_100 }
    else if mind { 1_050 }
    else if air && ty.strength >= 500 { 1_000 }
    else if anti_infantry_standoff(rules,ty) { 900 }
    else if air { 550 }
    else if ty.strength >= 700 { 330 }
    else { 0 }
}

/// Long-range personnel-killers (Sniper/Virus and rule-equivalent mods).
/// Classify the actual weapon by its range and interaction, not its name.
pub(super) fn anti_infantry_standoff(rules:&RuleSet,ty:&ObjectType)->bool {
    if ty.category!=ObjectCategory::Infantry {return false;}
    (0..ty.weapon_record_count()).filter_map(|slot|rules.weapon(ty.weapon_at(slot)?))
        .any(|w|!w.never_use && w.damage>=35
            && super::siege_counter::max_range_cells(w)>=8
            && rules.weapon_projectile(w).is_some_and(|p|p.ag)
            && w.warhead.as_deref().and_then(|name|rules.warhead(name)).is_some_and(|head|{
                let soft=["none","flak","plate"].into_iter().map(|armor|
                    f64::from_bits(head.interaction(crate::rules::armor::armor_index(armor)).multiplier().bits()))
                    .fold(0.0f64,f64::max);
                let steel=f64::from_bits(head.interaction(crate::rules::armor::armor_index("heavy")).multiplier().bits());
                soft*f64::from(w.damage)>=50.0 && steel<=soft*0.25
            }))
}

/// A candidate can close on a personnel specialist when its actual armour
/// makes even the enemy's best ordinary volley a small fraction of its HP.
pub(super) fn personnel_resistant(rules:&RuleSet,enemy:&ObjectType,ours:&ObjectType)->bool {
    if !matches!(ours.category,ObjectCategory::Vehicle|ObjectCategory::Aircraft)
        || ours.strength<=0 || ours.speed<=0 {return false;}
    let air=ours.category==ObjectCategory::Aircraft || ours.balloon_hover || ours.jumpjet;
    let worst=(0..enemy.weapon_record_count()).filter_map(|slot|rules.weapon(enemy.weapon_at(slot)?))
        .filter(|w|!w.never_use && rules.weapon_projectile(w).is_some_and(|p|
            if air {p.aa}else{p.ag}))
        .filter_map(|w|{
            let head=rules.warhead(w.warhead.as_deref()?)?;
            if head.mind_control || head.parasite || head.temporal || head.is_locomotor {
                return Some(f64::from(ours.strength));
            }
            let mult=f64::from_bits(head.interaction(ours.armor_id()).multiplier().bits());
            Some(f64::from(w.damage.max(0))*mult.max(0.0)*f64::from(w.burst.max(1)))
        }).fold(0.0f64,f64::max);
    worst<=f64::from(ours.strength)*0.10
}

pub(super) fn psychic_immune(rules:&RuleSet,obs:&Observation<'_>,actor:u64)->bool {
    obs.entity_type(rules,actor).is_some_and(|ty|
        ty.immune_to_psionics)
}

pub(super) fn actor_control_reach(rules:&RuleSet,obs:&Observation<'_>,actor:u64)->Option<i32> {
    let ty=obs.entity_type(rules,actor)?;
    (0..ty.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,actor,slot))
        .filter(|weapon|!weapon.never_use && weapon.warhead.as_deref()
            .and_then(|name|rules.warhead(name)).is_some_and(|head|head.mind_control))
        .map(super::siege_counter::max_range_cells).max()
}

pub(super) fn actor_reach(rules:&RuleSet,obs:&Observation<'_>,actor:u64)->i32 {
    let Some(ty)=obs.entity_type(rules,actor) else{return 0;};
    // Preserve native deployed weapon selection for GIs and artillery.
    if obs.is_deployed(actor) {
        return super::advance::weapon_range_cells(rules,ty,true);
    }
    (0..ty.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,actor,slot))
        .filter(|weapon|!weapon.never_use)
        .map(super::siege_counter::max_range_cells).max().unwrap_or(0)
}

pub(super) fn mind_control_reach(rules: &RuleSet, ty: &ObjectType) -> Option<i32> {
    (0..ty.weapon_record_count()).filter_map(|slot| {
        let weapon = rules.weapon(ty.weapon_at(slot)?)?;
        let head = rules.warhead(weapon.warhead.as_deref()?)?;
        if !head.mind_control || weapon.never_use {
            return None;
        }
        let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
        Some((weapon.range_leptons.max(0) + per_cell - 1) / per_cell)
    }).max()
}

/// A *currently visible* mind-controller whose reach covers a proposed cell.
/// If two cover it, the closer one takes priority; ID is the tie breaker.
pub(super) fn controller_covering(
    rules: &RuleSet, obs: &Observation<'_>, cell: (u16,u16),
) -> Option<(u64, (u16,u16), i32)> {
    obs.visible_enemy_entities().filter_map(|enemy| {
        let reach = actor_control_reach(rules,obs,enemy.entity_id)?;
        // One cell of grace for moving targets and turning the turret.
        (distance(cell, enemy.cell) <= reach + 1)
            .then_some((enemy.entity_id, enemy.cell, reach))
    }).min_by_key(|(id, origin, _)| (distance(*origin, cell), *id))
}

/// Whether a straight proposed march enters a Chebyshev weapon box.
/// Checking only its endpoint missed the common case of an army marching
/// *through* a Yuri's control radius to reach the next base. Clip the line
/// segment to the actual square instead of sampling a handful of tiles.
pub(super) fn path_enters_control_box(
    from: (u16, u16), to: (u16, u16), at: (u16, u16), radius: i32,
) -> bool {
    let mut first = 0.0_f64;
    let mut last = 1.0_f64;
    for (a, b, center) in [
        (from.0, to.0, at.0),
        (from.1, to.1, at.1),
    ] {
        let start = f64::from(a);
        let change = f64::from(b) - start;
        let low = f64::from(center) - f64::from(radius);
        let high = f64::from(center) + f64::from(radius);
        if change == 0.0 {
            if start < low || start > high { return false; }
        } else {
            let near = ((low - start) / change).min((high - start) / change);
            let far = ((low - start) / change).max((high - start) / change);
            first = first.max(near);
            last = last.min(far);
            if first > last { return false; }
        }
    }
    first <= last
}

/// A controller currently visible to this house which would control the
/// proposed route, even if neither end of the march is in its radius.
/// Never peek at controllers inside fog or at future world state.
pub(super) fn controller_covering_path(
    rules: &RuleSet, obs: &Observation<'_>,
    from: (u16, u16), to: (u16, u16),
) -> Option<(u64, (u16, u16), i32)> {
    obs.visible_enemy_entities().filter_map(|enemy| {
        let reach = actor_control_reach(rules,obs,enemy.entity_id)?;
        path_enters_control_box(from, to, enemy.cell, reach + 2)
            .then_some((enemy.entity_id, enemy.cell, reach))
    }).min_by_key(|(id, cell, _)| (distance(from, *cell), *id))
}

/// Walk a short distance away from the controller instead of charging into
/// the same fire again. All candidates use native movement-zone connectivity.
pub(super) fn evade_controller(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    at: (u16,u16), threat: (u16,u16),
) -> Option<(u16,u16)> {
    let zone = obs.entity_type(rules, actor)?.movement_zone;
    let dx = (i32::from(at.0) - i32::from(threat.0)).signum();
    let dy = (i32::from(at.1) - i32::from(threat.1)).signum();
    // The nearest wall may block the direct escape. Try all eight directions
    // at increasing standoff distances; prefer paths moving *away* from the
    // controller over sideways retreats, and reject every destination still
    // covered by any currently visible controller.
    let mut candidates = Vec::new();
    for step in [6, 8, 10, 12, 14] {
        for (sx, sy) in [(-1,-1), (-1,0), (-1,1), (0,-1),
                         (0,1), (1,-1), (1,0), (1,1)] {
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(at.0) + sx * step),
                u16::try_from(i32::from(at.1) + sy * step),
            ) else { continue; };
            let cell = (x, y);
            if distance(cell, threat) <= distance(at, threat)
                || !obs.is_ground_walkable(x, y)
                || !obs.can_reach(zone, at, cell)
                || controller_covering(rules, obs, cell).is_some()
            { continue; }
            let alignment = (sx - dx).abs() + (sy - dy).abs();
            candidates.push((alignment, step, cell));
        }
    }
    candidates.sort_unstable_by_key(|(alignment, step, cell)| (*alignment, *step, *cell));
    candidates.first().map(|(_, _, cell)| *cell)
}

/// A visible battery actually meeting the current front. Owning or being
/// able to buy a longer-range weapon is not proof that its fire is effective.
pub(super) fn siege_barrier(
    rules: &RuleSet, obs: &Observation<'_>,
    target: (u16,u16), fighters: &[&super::census::Unit],
) -> Option<(crate::sim::control::observation::EnemyEntity,i32)> {
    obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules,enemy.entity_id)?;
        if !super::siege_counter::dangerous_battery(rules,ty)
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            || distance(enemy.cell,target) > 14
            || !fighters.iter().any(|unit| distance(unit.cell,enemy.cell) <= 18
                && (obs.can_hurt(rules,enemy.entity_id,unit.entity_id)
                    || mind_control_reach(rules,ty).is_some())
                && (distance(unit.cell,enemy.cell)<=actual_threat_reach(
                    rules,obs,enemy.entity_id,unit.entity_id).max(mind_control_reach(rules,ty).unwrap_or(0))+1
                    || obs.object_type(rules,unit.type_id).is_some_and(|kind|
                        super::orders::building_approachable(obs,kind.movement_zone,unit.cell,enemy.cell))))
        { return None; }
        let reach = super::siege_counter::battery_reach(rules,ty);
        // Light defenses are handled by normal local superiority and
        // fragility checks; heavy / control guns get a dedicated operation.
        (reach >= 7 || mind_control_reach(rules,ty).is_some())
            .then_some((enemy,reach,ty.strength))
    }).max_by_key(|(enemy,reach,hp)| (*reach,*hp,enemy.entity_id))
        .map(|(enemy,reach,_)| (enemy,reach))
}


/// A valuable, already damaged fighter can be one volley from disappearing
/// before the ordinary 30% health retreat triggers. Keep the retreat *owner*
/// in `orders::managed`: this only answers whether the danger is imminent.
/// Both the attacker and its weapon must be currently visible and rule-legal.
fn early_extraction(health_permille: u16, tier: usize, single_volley_lethal: bool) -> bool {
    let caution_line = if tier >= 2 { 600 } else { 450 };
    health_permille <= caution_line && (single_volley_lethal || health_permille <= 400)
}

pub(super) fn exposed_casualty(
    rules: &RuleSet, obs: &Observation<'_>, unit: &super::census::Unit,
) -> bool {
    use super::census::Slot;
    if !unit.traits.combat || !unit.controllable() || unit.in_service()
        || !matches!(unit.slot, Slot::Vehicle | Slot::Infantry)
        || obs.is_deployed(unit.entity_id) || unit.health_permille > 600
    { return false; }
    let Some(victim) = obs.entity_type(rules, unit.entity_id) else { return false; };
    let health = obs.health(unit.entity_id).unwrap_or(0);
    if health <= 0 { return false; }
    obs.visible_enemy_entities().any(|enemy| {
        if !obs.can_hurt(rules, enemy.entity_id, unit.entity_id) { return false; }
        let Some(attacker) = obs.entity_type(rules, enemy.entity_id) else { return false; };
        let reach = effective_reach_against(rules, obs, enemy.entity_id, unit.entity_id);
        if reach <= 0 || distance(enemy.cell, unit.cell) > reach + 1 { return false; }
        let lethal = super::avoidance::kills_outright(rules, attacker, victim, health);
        // A gun that is firing at somebody else should not empty our entire
        // front just because it happens to cover the area. A lethal volley is
        // the exception: when it changes target there will be no second turn.
        let targeting = obs.attack_target(enemy.entity_id) == Some(unit.entity_id);
        (targeting || lethal) && early_extraction(unit.health_permille, unit.tier, lethal)
    })
}

/// Effective range for the weapons that can *actually* engage this target.
/// Checking just a unit's primary weapon loses its secondary AA missile,
/// which is exactly what makes anti-air look harmless to rocket soldiers.
pub(super) fn effective_reach_against(
    rules: &RuleSet, obs: &Observation<'_>, attacker: u64, victim: u64,
) -> i32 {
    use crate::rules::object_type::ObjectCategory;
    let Some(gun) = obs.entity_type(rules, attacker) else { return 0; };
    // An unpowered defense does not have a real weapon envelope; continue
    // treating its *building* as a hostile strategic target, not live fire.
    if gun.category == ObjectCategory::Building
        && !obs.visible_enemy_building_operational(rules, attacker)
    { return 0; }
    let Some(target) = obs.entity_type(rules, victim) else { return 0; };
    let air = target.category == ObjectCategory::Aircraft || target.balloon_hover
        || (target.jumpjet && !obs.is_deployed(victim));
    if gun.gunner {
        let Some(weapon) = obs.active_gunner_weapon(rules, attacker) else { return 0; };
        if weapon.never_use { return 0; }
        let projectile_ok = rules.weapon_projectile(weapon).is_some_and(|p|
            if air { p.aa } else { p.ag });
        let verses_ok = super::siege_counter::effective_ground_warhead(
            rules,gun,weapon,target);
        return if projectile_ok && verses_ok {
            super::siege_counter::max_range_cells(weapon)
        } else { 0 };
    }
    (0..gun.weapon_record_count()).filter_map(|slot| {
        let weapon = obs.observed_actor_weapon(rules,attacker,slot)?;
        if weapon.never_use { return None; }
        let viable = rules.weapon_projectile(weapon).is_some_and(|projectile|
            if air { projectile.aa } else { projectile.ag })
            && super::siege_counter::effective_ground_warhead(
                rules,gun,weapon,target);
        viable.then_some(super::siege_counter::max_range_cells(weapon))
    }).max().unwrap_or(0)
}

/// A ranking estimate, not a damage receiver or a guaranteed kill. It
/// only compares visible targets for ordinary positive-damage fire.
pub(super) fn shots_to_finish(rules:&RuleSet,obs:&Observation<'_>,actor:u64,target:u64)->u32 {
    let Some(ours)=obs.entity_type(rules,actor) else{return u32::MAX;};
    let Some(enemy)=obs.entity_type(rules,target) else{return u32::MAX;};
    let Some(hp)=obs.health(target).filter(|hp|*hp>0) else{return u32::MAX;};
    let air=enemy.category==ObjectCategory::Aircraft || enemy.balloon_hover
        || (enemy.jumpjet && !obs.is_deployed(target));
    let volley=(0..ours.weapon_record_count()).filter_map(|slot|obs.observed_actor_weapon(rules,actor,slot))
        .filter(|w|!w.never_use && w.damage>0 && !w.suicide && !w.spawner && !w.area_fire
            && rules.weapon_projectile(w).is_some_and(|p|if air{p.aa}else{p.ag}))
        .filter_map(|w|{
            let head=rules.warhead(w.warhead.as_deref()?)?;
            if head.mind_control || head.parasite || head.temporal || head.is_locomotor {return None;}
            let mult=f64::from_bits(head.interaction(enemy.armor_id()).multiplier().bits());
            if !mult.is_finite() || mult<=0.0 {return None;}
            Some((f64::from(w.damage)*mult*f64::from(w.burst.max(1))).clamp(0.0,100000.0) as u32)
        }).max().unwrap_or(0);
    if volley==0 {u32::MAX}else{(hp as u32).div_ceil(volley)}
}

pub(super) fn actual_threat_reach(
    rules: &RuleSet, obs: &Observation<'_>, attacker: u64, victim: u64,
) -> i32 {
    effective_reach_against(rules, obs, attacker, victim)
}

/// Best rule-driven sustained damage against a vehicle, using the actual
/// weapon's warhead/armor multiplier and rate of fire. Avoid evaluating only
/// nominal damage: an IFV's small rocket cannot trade evenly with heavy armor.
fn vehicle_exchange_output(rules: &RuleSet, attacker: &ObjectType, defender: &ObjectType) -> i64 {
    (0..attacker.weapon_record_count()).filter_map(|slot| {
        let weapon = rules.weapon(attacker.weapon_at(slot)?)?;
        if weapon.never_use || weapon.damage <= 0 { return None; }
        let head = rules.warhead(weapon.warhead.as_deref()?)?;
        let multiplier = f64::from_bits(
            head.interaction(defender.armor_id()).multiplier().bits());
        if multiplier <= 0.0 { return None; }
        // Bounded output prevents modded weapon values from overflowing a
        // tactical comparison; the engine still resolves real damage itself.
        let burst = i64::from(weapon.burst.max(1));
        let hit = ((f64::from(weapon.damage) * multiplier).clamp(0.0, 100_000.0)) as i64;
        Some(hit.saturating_mul(burst).saturating_mul(90) /
            i64::from(weapon.rof.max(15)))
    }).max().unwrap_or(0)
}

/// An empty/loaded Gunner cannot claim the best DPS of *every* IFVMode.
/// Use the native selected weapon for an existing instance; non-Gunner units
/// retain their target-dependent primary/secondary selection estimate.
fn vehicle_exchange_output_live(
    rules: &RuleSet, obs: &Observation<'_>, id: u64,
    attacker: &ObjectType, defender: &ObjectType,
) -> i64 {
    if !attacker.gunner { return vehicle_exchange_output(rules, attacker, defender); }
    let Some(weapon) = obs.active_gunner_weapon(rules, id) else { return 0; };
    if weapon.never_use || weapon.damage <= 0
        || !rules.weapon_projectile(weapon).is_some_and(|p| p.ag) { return 0; }
    let Some(head) = weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
    else { return 0; };
    let multiplier = f64::from_bits(
        head.interaction(defender.armor_id()).multiplier().bits());
    if multiplier <= 0.0 { return 0; }
    let hit = (f64::from(weapon.damage) * multiplier).clamp(0.0, 100_000.0) as i64;
    hit.saturating_mul(i64::from(weapon.burst.max(1))).saturating_mul(90)
        / i64::from(weapon.rof.max(15))
}

/// True only for a *pronounced* losing solo exchange, not an equal fight.
/// An overwhelming allied tank group can still make a good coordinated push.
fn losing_vehicle_trade(our_dps: i64, their_dps: i64, our_hp: i32, their_hp: i32,
    our_reach: i32, their_reach: i32) -> bool {
    if their_dps <= 0 || our_hp <= 0 || their_hp <= 0 { return false; }
    if our_reach >= their_reach + 3 { return false; } // can standoff
    if our_dps <= 0 { return true; }
    // Time to kill them / time for them to kill us >= 1.8.
    i64::from(their_hp).saturating_mul(their_dps).saturating_mul(10)
        >= i64::from(our_hp).saturating_mul(our_dps).saturating_mul(18)
}

pub(super) fn unfavourable_vehicle_duel(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64, enemy: u64,
) -> bool {
    use crate::rules::object_type::ObjectCategory;
    let (Some(ours), Some(theirs)) =
        (obs.entity_type(rules, actor), obs.entity_type(rules, enemy))
    else { return false; };
    if ours.category != ObjectCategory::Vehicle || theirs.category != ObjectCategory::Vehicle
        || !obs.can_hurt(rules, enemy, actor) { return false; }
    let (Some(own_hp), Some(enemy_hp)) = (obs.health(actor), obs.health(enemy))
    else { return false; };
    losing_vehicle_trade(
        vehicle_exchange_output_live(rules, obs, actor, ours, theirs),
        vehicle_exchange_output_live(rules, obs, enemy, theirs, ours), own_hp, enemy_hp,
        effective_reach_against(rules, obs, actor, enemy),
        effective_reach_against(rules, obs, enemy, actor),
    )
}

pub(super) fn armored_support(
    census: &super::census::Census, rules: &RuleSet,
    obs: &Observation<'_>, actor: u64, enemy: u64, at: (u16, u16),
) -> bool {
    census.units.iter().filter(|unit| {
        unit.entity_id != actor && unit.controllable()
            && unit.slot == super::census::Slot::Vehicle && unit.traits.combat
            && !unit.in_service() && distance(unit.cell, at) <= 9
            && obs.can_hurt(rules, unit.entity_id, enemy)
    }).take(2).count() >= 2
}

/// A short-ranged unit must not march through a visible weapon envelope
/// that it cannot answer. This includes AA against jumpjet infantry, snipers
/// against infantry and fixed artillery against short-range tanks. A whole
/// army may have adequate points while one such unit has no useful trade.
/// Combat strength is a group property, not a reason to kite every healthy
/// armored unit from a single weak pillbox. Never apply this to fragile air or
/// infantry, and never override the psychic controller emergency.
pub(super) fn overwhelming_armor_commit(
    armored: bool, health_permille: u16, ours: i32, theirs: i32,
) -> bool {
    armored && health_permille >= 650 && theirs > 0
        && ours >= theirs.saturating_mul(3)
}

/// Heavy fixed artillery can destroy a column before its first shot. Its
/// range mismatch is not repaired by the 3:1 HP exception used for bunkers.
/// No type name: mods with another long-range cannon receive the same safety.
pub(super) fn extreme_battery_mismatch(own_reach: i32, hostile_reach: i32) -> bool {
    hostile_reach >= 11 && own_reach.saturating_add(3) < hostile_reach
}

fn loses_range_trade(own_reach: i32, hostile_reach: i32, can_answer: bool) -> bool {
    hostile_reach > 0 && (!can_answer || own_reach <= hostile_reach + 1)
}

pub(super) fn lethal_approach(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    from: (u16, u16), destination: (u16, u16),
) -> Option<((u16,u16), i32)> {
    use crate::rules::object_type::ObjectCategory;
    let actor_ty = obs.entity_type(rules, actor)?;
    let fragile = matches!(actor_ty.category,
        ObjectCategory::Infantry | ObjectCategory::Aircraft) || actor_ty.jumpjet;

    obs.visible_enemy_entities().filter_map(|enemy| {
        let hostile = obs.entity_type(rules, enemy.entity_id)?;
        // A tank vs tank exchange is decided by micro, not by the siege gate.
        // Only fixed guns, or a long-ranged anti-personnel / anti-air weapon,
        // should stop a normal marching soldier before reaching the line.
        if !hostile.is_base_defense && !fragile { return None; }
        let reach = effective_reach_against(rules, obs, enemy.entity_id, actor);
        if reach <= 0 { return None; }
        if reach < 4 || (!hostile.is_base_defense && reach < 6) { return None; }
        let own_reach = effective_reach_against(rules, obs, actor, enemy.entity_id);
        if !loses_range_trade(own_reach, reach,
            obs.can_hurt(rules, actor, enemy.entity_id)) { return None; }
        let in_lane = distance(enemy.cell, destination) <= reach + 1
            || distance(enemy.cell, from) <= reach + 1
            || super::orders::path_crosses(&[enemy.cell], from, destination, reach);
        in_lane.then_some((enemy.cell, reach))
    }).max_by_key(|(cell, reach)| (*reach, std::cmp::Reverse(distance(from,*cell))))
}

/// A rally post is usable only outside all visible fixed guns and psychic
/// coverage. The decision is shared by artillery staging and reinforcement
/// assembly, so neither can send its men to the danger the other avoids.
pub(super) fn unsafe_post(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64, post: (u16,u16),
) -> bool {
    controller_covering(rules,obs,post).is_some() || obs.visible_enemy_entities().any(|enemy| {
        let Some(ty) = obs.entity_type(rules, enemy.entity_id) else { return false; };
        effective_reach_against(rules, obs, enemy.entity_id, actor) > 0
            && (ty.is_base_defense
                || obs.entity_type(rules, actor).is_some_and(|ours| {
                    ours.jumpjet || matches!(ours.category,
                        crate::rules::object_type::ObjectCategory::Infantry
                        | crate::rules::object_type::ObjectCategory::Aircraft)
                }))
            && distance(enemy.cell,post) <= effective_reach_against(
                rules,obs,enemy.entity_id,actor) + 1
    })
}

/// Find a legal standoff cell on the *friendly* side of a visible gun. This
/// does not choose an attack target: the target/mission belongs to `advance`.
/// Every candidate is checked against all currently visible defensive guns,
/// not just the first one, so a second pillbox cannot make the staging post
/// more dangerous than the road we are leaving.
fn standoff_lane(
    from: (u16,u16), gun: (u16,u16), radius: i32, shift: i32,
) -> Option<(u16,u16)> {
    let mut dx = (i32::from(from.0) - i32::from(gun.0)).signum();
    let dy = (i32::from(from.1) - i32::from(gun.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let lateral = (-dy,dx);
    let (Ok(x),Ok(y)) = (
        u16::try_from(i32::from(gun.0) + dx * radius + lateral.0 * shift),
        u16::try_from(i32::from(gun.1) + dy * radius + lateral.1 * shift),
    ) else { return None; };
    Some((x,y))
}

pub(super) fn siege_post(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    from: (u16,u16), gun: (u16,u16), reach: i32,
) -> Option<(u16,u16)> {
    siege_post_impl(rules,obs,actor,from,from,gun,reach,&[],&[],0,None)
}

/// Main-fist staging uses a common approach direction. If that direction
/// is unsafe or unreachable, the actor can still use its own safe side.
pub(super) fn siege_post_on_lane(
    rules:&RuleSet,obs:&Observation<'_>,actor:u64,
    from:(u16,u16),approach_from:(u16,u16),gun:(u16,u16),reach:i32,
    visible:&[EnemyEntity], reserved:&[(u64,(u16,u16))], gap:i32,
)->Option<(u16,u16)> {
    let guns=super::safety::envelopes(rules,obs,actor,visible,false);
    let zone=obs.entity_type(rules,actor)?.movement_zone;
    let existing=obs.destination(actor).filter(|post|
        obs.attack_target(actor).is_none()
        && distance(*post,gun)>=reach+2 && distance(*post,gun)<=reach+8
        && standoff_lane(approach_from,gun,reach+2,0)
            .is_some_and(|centre|distance(centre,*post)<=8)
        && obs.is_ground_walkable(post.0,post.1)
        && obs.can_reach(zone,from,*post)
        && super::safety::safe_leg(&guns,from,*post,true)
        && !unsafe_post(rules,obs,actor,*post));
    if gap==0 && existing.is_some() {return existing;}
    // Prefer retaining the current legal position to shuffling at a safe
    // post. A lower-ID reservation wins a conflict, so two neighbours do
    // not continually trade destinations. Candidate scoring still avoids
    // all neighbours, not only the higher-priority ones.
    let stable=existing.or_else(|| (gap>0 && distance(from,gun)>=reach+2
        && distance(from,gun)<=reach+8 && !unsafe_post(rules,obs,actor,from)
        && super::safety::safe_leg(&guns,from,from,false)).then_some(from));
    if stable.is_some_and(|cell| crowding(actor,cell,reserved,gap,true)==0) {return stable;}
    siege_post_impl(rules,obs,actor,from,approach_from,gun,reach,&guns,reserved,gap,stable)
        .or_else(|| {
            (approach_from!=from).then(||
                siege_post_impl(rules,obs,actor,from,from,gun,reach,&guns,reserved,gap,stable)).flatten()
        })
}

fn crowding(actor:u64,cell:(u16,u16),reserved:&[(u64,(u16,u16))],gap:i32,priority_only:bool)->i32 {
    reserved.iter().filter(|(id,_)| *id!=actor && (!priority_only || *id<actor))
        .map(|(_,other)|(gap-distance(cell,*other)).max(0)).sum()
}

/// Whole-cell blast radius from the real weapon, not a named unit. A cap
/// avoids turning large modded blast radii into map-wide dispersal orders.
pub(super) fn battery_spacing(rules:&RuleSet,ty:&crate::rules::object_type::ObjectType)->i32 {
    (0..ty.weapon_record_count()).filter_map(|slot|rules.weapon(ty.weapon_at(slot)?))
        .filter(|w|w.damage>0 && !w.never_use && rules.weapon_projectile(w).is_some_and(|p|p.ag))
        .filter_map(|w|rules.warhead(w.warhead.as_deref()?))
        .map(|head|head.cell_spread_f64.floor() as i32)
        .max().filter(|&r|r>0).map_or(0,|r|r.saturating_add(1).clamp(2,4))
}

fn siege_post_impl(
    rules:&RuleSet,obs:&Observation<'_>,actor:u64,
    from:(u16,u16),approach_from:(u16,u16),gun:(u16,u16),reach:i32,
    guns:&[super::safety::Envelope], reserved:&[(u64,(u16,u16))],gap:i32,
    stable:Option<(u16,u16)>,
)->Option<(u16,u16)> {
    let zone = obs.entity_type(rules, actor)?.movement_zone;
    // Give adjacent units distinct lanes; one shared post would turn the
    // staging line into a traffic jam. Actor IDs break ties deterministically.
    let lane = (actor % 5) as i32 - 2;
    // The first safe annulus is Reach+2; Range is an OUTER radius and a
    // Prism Tank (Range=10) can fire on a prism tower (Range=8) here.
    // Starting at +3 made an otherwise winning gun park out of range.
    let mut best=stable.map(|cell|(crowding(actor,cell,reserved,gap,false),cell));
    let shifts=[lane*2,0,-lane*2,-4,4,-6,6];
    for radius in [reach + 2, reach + 3, reach + 5, reach + 7] {
        for &shift in &shifts[..if gap>0 {shifts.len()} else {3}] {
            let Some((x,y)) = standoff_lane(approach_from,gun,radius,shift) else { continue; };
            let post = (x,y);
            let penalty=crowding(actor,post,reserved,gap,false);
            if best.is_some_and(|(old,_)|penalty>=old) {continue;}
            if !obs.is_ground_walkable(x,y) || !obs.can_reach(zone,from,post) {
                continue;
            }
            if super::safety::safe_leg(guns,from,post,true)
                && !unsafe_post(rules,obs,actor,post) {
                if penalty==0 {return Some(post);}
                best=Some((penalty,post));
            }
        }
    }
    best.map(|(_,cell)|cell)
}

/// The army has enough strength in total, but not at the point of contact.
/// This is a case for one *bounded* reinforcement wave, not a reason to raise
/// the all-map attack threshold (which would hold victories at the home base).
fn support_gap(front: i32, rear: i32, enemy: i32, front_count: usize, rear_count: usize) -> bool {
    enemy > 0 && front_count >= 2 && rear_count >= 3
        && front.saturating_mul(1_000) < enemy.saturating_mul(1_200)
        && front.saturating_add(rear).saturating_mul(1_000)
            >= enemy.saturating_mul(1_250)
}

/// How much armed enemy strength is *presently visible* at the front. Historic
/// sightings guide scouting, but they must not create fictitious gun pressure.
fn visible_front_points(
    rules: &RuleSet, obs: &Observation<'_>, at: (u16,u16),
) -> i32 {
    obs.visible_enemy_entities()
        .filter(|enemy| distance(enemy.cell,at) <= 18)
        .filter_map(|enemy| {
            let ty = obs.entity_type(rules,enemy.entity_id)?;
            if ty.is_base_defense
                && !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            { return None; }
            let health = obs.health(enemy.entity_id)?.max(0);
            let fraction = i64::from(health).saturating_mul(1_000)
                / i64::from(ty.strength.max(1));
            let fraction = u16::try_from(fraction.clamp(0,1_000)).unwrap_or(0);
            Some(super::census::fight_points(ty,fraction))
        })
        .fold(0i32, |total, points| total.saturating_add(points))
}

pub(super) fn waiting_for_support(
    rules: &RuleSet, obs: &Observation<'_>, target: (u16,u16),
    fighters: &[&super::census::Unit],
) -> bool {
    let enemy = visible_front_points(rules,obs,target);
    let mut front = 0i32;
    let mut rear = 0i32;
    let mut front_count = 0usize;
    let mut rear_count = 0usize;
    for fighter in fighters {
        if distance(fighter.cell,target) <= 18 {
            front = front.saturating_add(fighter.points);
            front_count += 1;
        } else {
            rear = rear.saturating_add(fighter.points);
            rear_count += 1;
        }
    }
    support_gap(front,rear,enemy,front_count,rear_count)
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn a_light_vehicle_does_not_trade_its_life_for_heavy_armor() {
        assert!(losing_vehicle_trade(12, 50, 200, 400, 5, 5));
        assert!(!losing_vehicle_trade(100, 100, 400, 200, 5, 5));
        assert!(!losing_vehicle_trade(12, 50, 200, 400, 12, 5));
    }

    #[test]
    fn a_weaponless_vehicle_must_not_challenge_an_armed_tank() {
        assert!(losing_vehicle_trade(0, 60, 250, 300, 5, 5));
        assert!(!losing_vehicle_trade(0, 0, 250, 300, 5, 5));
    }

    #[test]
    fn damaged_elites_leave_before_an_imminent_second_volley() {
        assert!(early_extraction(550,2,true));
        assert!(!early_extraction(700,2,true));
        assert!(!early_extraction(550,0,true));
        assert!(early_extraction(390,0,false));
    }

    #[test]
    fn spread_reservations_do_not_count_self_or_force_low_id_to_yield() {
        let posts=[(1,(20,20)),(2,(21,20)),(3,(30,30))];
        assert_eq!(crowding(1,(20,20),&posts,3,true),0);
        assert_eq!(crowding(2,(21,20),&posts,3,true),2);
        assert_eq!(crowding(2,(24,20),&posts,3,false),0);
        assert_eq!(crowding(2,(21,20),&posts,0,false),0);
    }

    #[test]
    fn staging_lanes_spread_tanks_and_keep_edge_cells_valid() {
        let leading = standoff_lane((20,40),(40,40),9,0);
        let adjacent = standoff_lane((20,40),(40,40),9,3);
        assert_eq!(leading,Some((31,40)));
        assert_eq!(adjacent,Some((31,37)));
        assert_eq!(standoff_lane((0,0),(1,0),8,0),None);
    }

    #[test]
    fn reinforcement_gate_requires_an_actual_winnable_wave() {
        assert!(support_gap(200,900,700,2,6));
        assert!(!support_gap(200,900,0,2,6));
        assert!(!support_gap(200,200,700,2,6));
        assert!(!support_gap(900,300,700,5,3));
        assert!(!support_gap(200,900,700,2,1));
    }

    #[test]
    fn retail_psychic_units_are_rule_identified() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        // Modded rules can carry the same trait under another object name.
        if let Some(yuri) = rules.object("YURI") {
            assert!(mind_control_reach(&rules, yuri).is_some());
        }
        if let Some(grizzly) = rules.object("MTNK") {
            assert!(mind_control_reach(&rules, grizzly).is_none());
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v14_counter_tests {
    use super::*;

    #[test]
    fn a_short_range_soldier_cannot_trade_into_a_long_range_gun() {
        assert!(loses_range_trade(5, 12, true));
        assert!(loses_range_trade(12, 5, false));
        assert!(!loses_range_trade(12, 5, true));
        assert!(!loses_range_trade(0, 0, false));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v18_commit_tests {
    use super::*;

    #[test]
    fn only_healthy_overwhelming_armor_may_ignore_a_minor_pillbox() {
        assert!(overwhelming_armor_commit(true, 900, 900, 300));
        assert!(!overwhelming_armor_commit(false, 900, 900, 300));
        assert!(!overwhelming_armor_commit(true, 500, 900, 300));
        assert!(!overwhelming_armor_commit(true, 900, 899, 300));
        assert!(!overwhelming_armor_commit(true, 900, 900, 0));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v26_long_range_battery_tests {
    use super::*;

    #[test]
    fn extreme_batteries_never_use_the_minor_bunker_overrun_exception() {
        assert!(extreme_battery_mismatch(5, 15));
        assert!(extreme_battery_mismatch(7, 12));
        assert!(!extreme_battery_mismatch(9, 11));
        assert!(!extreme_battery_mismatch(5, 7),
            "normal pillboxes still admit the old healthy-armor overrun");
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v23_mind_control_route_tests {
    use super::path_enters_control_box;

    #[test]
    fn march_crosses_controller_even_when_both_endpoints_are_safe() {
        assert!(path_enters_control_box((2, 10), (40, 10), (22, 13), 4));
        assert!(!path_enters_control_box((2, 10), (40, 10), (22, 18), 4));
    }

    #[test]
    fn zero_length_and_diagonal_routes_handle_square_boundaries() {
        assert!(path_enters_control_box((14, 14), (14, 14), (16, 16), 2));
        assert!(!path_enters_control_box((14, 14), (14, 14), (17, 17), 2));
        assert!(path_enters_control_box((0, 0), (20, 20), (12, 12), 1));
        assert!(!path_enters_control_box((0, 0), (20, 20), (12, 18), 1));
    }
}
