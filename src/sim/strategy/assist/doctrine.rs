//! V40 combat doctrine: a special-purpose weapon is not a general-purpose army.
//!
//! This is deliberately read from the *current RuleSet*. Country labels and
//! retail type IDs are used only in regression tests; Ares/Phobos/MOD units with
//! the same native weapon semantics receive the same treatment.
//!
//! A high-risk unit has two separate questions: (1) should another one be
//! produced before we have a sustainable line army, and (2) is this particular
//! visible target a worthwhile and safe use of its real weapon? Neither
//! `CanBuild` nor `can_hurt` answers those strategic questions on its own.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::AssistBrain;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Duty {
    Line,
    Scout,
    Parasite,
    BombPlanter,
    Disposable,
    Radiation,
    Marksman,
    TankHunter,
    Controller,
}

fn primary<'r>(rules: &'r RuleSet, ty: &ObjectType)
    -> Option<&'r crate::rules::weapon_type::WeaponType>
{
    ty.weapon_at(0).and_then(|name| rules.weapon(&name))
}

fn multiplier(rules: &RuleSet, weapon: &crate::rules::weapon_type::WeaponType,
    armour: crate::rules::armor::ArmorId) -> f64
{
    weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
        .map_or(0.0, |wh| f64::from_bits(wh.interaction(armour).multiplier().bits()))
}

/// One primary weapon's signature determines whether the unit actually has a
/// narrow target class. A `NeverUse` scanner must not turn a terror drone into
/// a regular rifleman; deploy-only area weapons are checked separately.
pub(super) fn duty(rules: &RuleSet, ty: &ObjectType) -> Duty {
    if ty.category == ObjectCategory::Building { return Duty::Line; }
    if (0..ty.weapon_record_count()).filter_map(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(&name)))
        .filter(|weapon| !weapon.never_use).any(|weapon| weapon.suicide)
    { return Duty::Disposable; }
    if ty.ivan || (0..ty.weapon_record_count()).filter_map(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(&name)))
        .filter(|weapon| !weapon.never_use).any(|weapon|
            weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
                .is_some_and(|wh| wh.ivan_bomb))
    { return Duty::BombPlanter; }
    if (0..ty.weapon_record_count()).filter_map(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(&name)))
        .filter(|weapon| !weapon.never_use).any(|weapon| weapon.area_fire
            && (weapon.rad_level > 0 || weapon.is_rad_eruption
                || weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
                    .is_some_and(|wh| wh.radiation)))
    { return Duty::Radiation; }
    if (0..ty.weapon_record_count()).filter_map(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(&name)))
        .filter(|weapon| !weapon.never_use).any(|weapon| weapon.warhead.as_deref()
            .and_then(|name| rules.warhead(name)).is_some_and(|wh| wh.mind_control))
    { return Duty::Controller; }
    if (0..ty.weapon_record_count()).filter_map(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(&name)))
        .filter(|weapon| !weapon.never_use).any(|weapon| weapon.warhead.as_deref()
            .and_then(|name| rules.warhead(name)).is_some_and(|wh| wh.parasite))
    {
        return if ty.category == ObjectCategory::Vehicle {
            Duty::Parasite
        } else { Duty::Scout };
    }
    if let Some(weapon) = primary(rules,ty) {
        let none = multiplier(rules,weapon,crate::rules::armor::ArmorId::NONE);
        let wood = multiplier(rules,weapon,crate::rules::armor::ArmorId::WOOD);
        let heavy = multiplier(rules,weapon,crate::rules::armor::armor_index("heavy"));
        if none >= 0.8 && wood <= 0.05 && heavy <= 0.1
            && ty.category == ObjectCategory::Infantry
        { return Duty::Marksman; }
        if none <= 0.08 && heavy >= 0.3 && ty.category == ObjectCategory::Vehicle
        { return Duty::TankHunter; }
    }
    Duty::Line
}

/// Combat troops able to cover ordinary infantry/tank duties. Dogs, demolition
/// specialists, suicide attackers and mind controllers are not a substitute
/// for a line of conscripts, initiates and tanks in the opening.
pub(super) fn line_count(brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>) -> usize
{
    brain.census.units.iter().filter(|unit| unit.traits.combat
        && obs.object_type(rules,unit.type_id).is_some_and(|ty|
            matches!(ty.category, ObjectCategory::Infantry | ObjectCategory::Vehicle)
            && !ty.naval && duty(rules,ty) == Duty::Line))
        .count()
}

fn allowance(kind: Duty, line: usize) -> usize {
    match kind {
        Duty::Line => usize::MAX,
        Duty::Scout => if line >= 9 { 4 } else { 2 },
        Duty::Parasite => if line < 5 { 0 } else { (1 + line / 14).min(3) },
        Duty::BombPlanter | Duty::Disposable => usize::from(line >= 12),
        Duty::Radiation => if line < 8 { 0 } else if line < 20 { 1 } else { 2 },
        Duty::Marksman => if line < 6 { 0 } else { (1 + line / 14).min(3) },
        Duty::TankHunter => if line < 5 { 0 } else { (1 + line / 10).min(4) },
        Duty::Controller => if line < 6 { 0 } else { (1 + line / 12).min(3) },
    }
}

/// Apply the same ceiling to native recommendations, country signatures and
/// the role-based fallback. Otherwise a bypass in any of those paths may buy
/// twenty Ivans while the rest of the policy thinks it has a balanced army.
pub(super) fn may_produce(brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, candidate: &ObjectType) -> bool
{
    if !super::orders::is_combat_unit(rules,candidate) { return true; }
    let kind = duty(rules,candidate);
    if kind == Duty::Line { return true; }
    let line = line_count(brain,rules,obs);
    let cap = allowance(kind,line);
    if cap == 0 { return false; }
    let held = brain.census.units.iter().filter(|unit| {
        obs.object_type(rules,unit.type_id).is_some_and(|ty|
            ty.category == candidate.category && duty(rules,ty) == kind)
    }).count();
    if held >= cap { return false; }
    if matches!(kind, Duty::Disposable | Duty::BombPlanter) {
        // Never buy an expendable unit speculatively at the start of a match:
        // require a *currently visible*, economically useful and safe target.
        return obs.visible_enemy_entities().any(|enemy| {
            let Some(target) = obs.entity_type(rules,enemy.entity_id) else { return false; };
            valuable(candidate,target) && friends_clear(rules,obs,None,enemy.cell,
                blast_radius(rules,candidate).max(4))
        });
    }
    true
}

fn valuable(attacker: &ObjectType, target: &ObjectType) -> bool {
    matches!(target.category, ObjectCategory::Building | ObjectCategory::Vehicle)
        && target.cost >= attacker.cost.saturating_mul(2).max(500)
}

/// The blast includes other suicide bodies too: an explosion may trigger a
/// chain reaction. Count our structures, tanks and infantry, not merely the
/// non-suicide subset, and do not use hidden enemy positions for this test.
pub(super) fn friends_clear(rules: &RuleSet, obs: &Observation<'_>,
    source: Option<u64>, site: (u16,u16), radius: i32) -> bool
{
    let bound = radius.max(0).min(40);
    !obs.own_entities().any(|friend| {
        friend.health > 0 && Some(friend.entity_id) != source
            && obs.entity_type(rules,friend.entity_id).is_some_and(|ty| !ty.spawned)
            && super::advance::cell_distance(site,friend.cell) <= bound
    })
}

pub(super) fn blast_radius(rules: &RuleSet, ty: &ObjectType) -> i32 {
    let mut radius = 0;
    for slot in 0..ty.weapon_record_count() {
        let Some(weapon) = ty.weapon_at(slot).and_then(|name| rules.weapon(&name))
        else { continue; };
        if !weapon.suicide { continue; }
        let splash = weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
            .map_or(0, |wh| wh.cell_spread_f64.ceil() as i32);
        radius = radius.max(splash);
    }
    if let Some(weapon) = ty.death_weapon.as_deref().and_then(|name| rules.weapon(name)) {
        let splash = weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
            .map_or(0, |wh| wh.cell_spread_f64.ceil() as i32);
        radius = radius.max(splash);
    }
    radius.clamp(1, 32) + 1
}

/// A radiation deploy emits an area hazard from the *unit's* cell, not from
/// the nearest enemy. The effect persists after undeploy, so be conservative
/// about every non-immune friendly infantry/vehicle standing in that area.
pub(super) fn radiation_deploy_radius(rules: &RuleSet, ty: &ObjectType) -> i32 {
    if duty(rules,ty) != Duty::Radiation || !ty.deploy_fire { return 0; }
    (0..ty.weapon_record_count()).filter_map(|slot| {
        let weapon = ty.weapon_at(slot).and_then(|name| rules.weapon(&name))?;
        let hazard = weapon.area_fire && (weapon.rad_level > 0
            || weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
                .is_some_and(|wh| wh.radiation));
        if !hazard { return None; }
        Some(weapon.warhead.as_deref().and_then(|name| rules.warhead(name))
            .map_or(5,|wh| (wh.cell_spread_f64.ceil() as i32).max(5)) + 1)
    }).max().unwrap_or(0)
}

pub(super) fn radiation_deploy_safe(rules: &RuleSet, obs: &Observation<'_>,
    unit: u64, cell: (u16,u16)) -> bool
{
    let Some(ty) = obs.entity_type(rules,unit) else { return false; };
    let radius = radiation_deploy_radius(rules,ty);
    if radius == 0 { return true; }
    !obs.own_entities().any(|friend| friend.entity_id != unit
        && friend.health > 0
        && obs.entity_type(rules,friend.entity_id).is_some_and(|other|
            matches!(other.category, ObjectCategory::Infantry | ObjectCategory::Vehicle)
                && !other.immune_to_radiation && !other.spawned)
        && super::advance::cell_distance(friend.cell,cell) <= radius)
}

/// Semantic target permission, *after* native can_engage/can_hurt. Low but
/// nonzero Verses is not permission for a 900-credit tank destroyer to chase
/// a 100-credit rifleman or a 600-credit sniper to fire at a concrete wall.
pub(super) fn target_ok(rules: &RuleSet, obs: &Observation<'_>,
    attacker: u64, target: u64) -> bool
{
    let (Some(ty),Some(victim)) = (obs.entity_type(rules,attacker),
        obs.entity_type(rules,target)) else { return false; };
    match duty(rules,ty) {
        Duty::Line | Duty::Controller => true,
        Duty::Scout | Duty::Marksman => victim.category == ObjectCategory::Infantry,
        Duty::Parasite | Duty::TankHunter => victim.category == ObjectCategory::Vehicle,
        Duty::Disposable => valuable(ty,victim) && friends_clear(rules,obs,Some(attacker),
            obs.object_cell_and_type(target).map_or((0,0), |(cell,_)|cell),
            blast_radius(rules,ty)),
        Duty::BombPlanter => valuable(ty,victim) && friends_clear(rules,obs,Some(attacker),
            obs.object_cell_and_type(target).map_or((0,0), |(cell,_)|cell), 4),
        Duty::Radiation => {
            // A RadBeam still creates a hazard near its impact, even when the
            // deploy-only large eruption is not currently active.
            let cell = obs.object_cell_and_type(target).map_or((0,0), |(cell,_)|cell);
            radiation_deploy_safe(rules,obs,attacker,cell)
        }
    }
}

/// Modest score adjustment for ordinary weapons. The primary/secondary
/// warhead-to-armour ratios break ties between legal nearby targets without
/// pretending a special (Ivan/parasite/mind-control) warhead is ordinary DPS.
pub(super) fn preference_penalty(rules: &RuleSet, obs: &Observation<'_>,
    attacker: u64, target: u64) -> u8
{
    let (Some(source), Some(victim)) = (obs.entity_type(rules,attacker),
        obs.entity_type(rules,target)) else { return 0; };
    if duty(rules,source) != Duty::Line { return 0; }
    let efficacy = (0..source.weapon_record_count()).filter_map(|slot| {
        let weapon = source.weapon_at(slot).and_then(|name| rules.weapon(&name))?;
        if weapon.never_use || weapon.damage <= 0 { return None; }
        Some(multiplier(rules,weapon,victim.armor_id()))
    }).fold(0.0_f64,f64::max);
    if efficacy >= 0.7 { 0 }
    else if efficacy >= 0.4 { 1 }
    else if efficacy >= 0.2 { 2 }
    else { 3 }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn stock_factions_get_line_units_before_specialists() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        for name in ["E1", "E2", "INIT", "HTNK", "LTNK"] {
            assert_eq!(duty(&rules,rules.object(name).expect(name)),Duty::Line,"{}",name);
        }
        for (name,kind) in [
            ("IVAN",Duty::BombPlanter), ("TERROR",Duty::Disposable),
            ("DTRUCK",Duty::Disposable), ("DRON",Duty::Parasite),
            ("DESO",Duty::Radiation), ("SNIPE",Duty::Marksman),
            ("TNKD",Duty::TankHunter), ("YURI",Duty::Controller),
        ] {
            assert_eq!(duty(&rules,rules.object(name).expect(name)),kind,"{}",name);
        }
        assert_eq!(allowance(Duty::BombPlanter,0),0);
        assert_eq!(allowance(Duty::Parasite,4),0);
        assert_eq!(allowance(Duty::Radiation,7),0);
        assert_eq!(allowance(Duty::Marksman,6),1);
        assert_eq!(allowance(Duty::Disposable,18),1);
    }

    #[test]
    fn native_bomb_and_radiation_splash_are_not_one_size_fits_all() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        assert_eq!(blast_radius(&rules,rules.object("TERROR").expect("terrorist")),3);
        assert_eq!(blast_radius(&rules,rules.object("DTRUCK").expect("truck")),9);
        assert!(radiation_deploy_radius(&rules,
            rules.object("DESO").expect("desolator")) >= 10);
        assert_eq!(radiation_deploy_radius(&rules,
            rules.object("E1").expect("GI")),0);
    }

    #[test]
    fn retail_specialists_do_not_consider_ineffective_targets_good_trades() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let sniper = rules.object("SNIPE").expect("sniper");
        let tank_hunter = rules.object("TNKD").expect("tank hunter");
        assert_eq!(duty(&rules,sniper),Duty::Marksman);
        assert_eq!(duty(&rules,tank_hunter),Duty::TankHunter);
        assert!(valuable(rules.object("TERROR").expect("terrorist"),
            rules.object("GACNST").expect("yard")));
        assert!(!valuable(rules.object("DTRUCK").expect("truck"),
            rules.object("E1").expect("rifleman")));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod native_order_regression {
    use super::*;
    use crate::rules::unit_role::RoleTable;
    use crate::sim::world::Simulation;
    use crate::sim::control::observation::Observation;
    use super::super::census::Census;

    #[test]
    fn sniper_and_tank_destroyer_must_not_waste_shots_on_wrong_class() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let sniper = sim.spawn_object("SNIPE", "British", 15, 15, 255, &rules).unwrap();
        let hunter = sim.spawn_object("TNKD", "British", 16, 15, 255, &rules).unwrap();
        let rifle = sim.spawn_object("E2", "Russians", 20, 17, 255, &rules).unwrap();
        let tank = sim.spawn_object("HTNK", "Russians", 23, 17, 255, &rules).unwrap();
        let yard = sim.spawn_object("NACNST", "Russians", 27, 17, 255, &rules).unwrap();
        let owner = sim.interner.intern("British");
        let obs = Observation::new(&sim,owner);
        assert!(target_ok(&rules,&obs,sniper,rifle));
        assert!(!target_ok(&rules,&obs,sniper,yard));
        assert!(!target_ok(&rules,&obs,sniper,tank));
        assert!(target_ok(&rules,&obs,hunter,tank));
        assert!(!target_ok(&rules,&obs,hunter,rifle));
        assert!(!target_ok(&rules,&obs,hunter,yard));
    }

    #[test]
    fn explosives_and_radiation_respect_friendly_locations() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let bomber = sim.spawn_object("DTRUCK", "Africans", 10, 12, 255, &rules).unwrap();
        let victim = sim.spawn_object("GACNST", "British", 23, 20, 255, &rules).unwrap();
        let friend = sim.spawn_object("E2", "Africans", 24, 20, 255, &rules).unwrap();
        let owner = sim.interner.intern("Africans");
        let obs = Observation::new(&sim,owner);
        assert!(!target_ok(&rules,&obs,bomber,victim),
            "the bomb truck must not destroy a nearby friendly");
        assert_eq!(obs.entity_type(&rules,friend).unwrap().category,ObjectCategory::Infantry);

        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let deso = sim.spawn_object("DESO", "Arabs", 20, 20, 255, &rules).unwrap();
        sim.spawn_object("E2", "Arabs", 24, 20, 255, &rules).unwrap();
        let owner = sim.interner.intern("Arabs");
        let obs = Observation::new(&sim,owner);
        assert!(!radiation_deploy_safe(&rules,&obs,deso,(20,20)));
    }

    #[test]
    fn unformed_armies_cannot_mass_drones_or_crazy_ivans() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.spawn_object("E2", "Russians", 20, 20, 255, &rules).unwrap();
        let owner = sim.interner.intern("Russians");
        let obs = Observation::new(&sim,owner);
        let mut brain = AssistBrain::new(0x40);
        let mut census = Census::default();
        census.refresh(&RoleTable::build(&rules),&rules,&obs);
        brain.census = census;
        assert!(!may_produce(&brain,&rules,&obs,rules.object("DRON").unwrap()));
        assert!(!may_produce(&brain,&rules,&obs,rules.object("IVAN").unwrap()));
        assert!(may_produce(&brain,&rules,&obs,rules.object("E2").unwrap()));
        assert!(may_produce(&brain,&rules,&obs,rules.object("HTNK").unwrap()));
    }
}
