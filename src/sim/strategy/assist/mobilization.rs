//! Opening procurement and release of the first ground wave. This is a
//! permission shared by the existing plan, wallet and squad commanders, not
//! another task owner. Only live own units and native production rights count.
use crate::rules::armor::armor_index;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::CanBuild;
use super::{build, census::Census, doctrine, plan::Item};

const OPENING_LIMIT: u64 = 9_000;
const SAMPLE_PERIOD: u64 = 45;
const WAVE_VEHICLES: usize = 3;
const WAVE_POINTS: i32 = 650;

/// A conservative direct-fire ground vehicle: no hypothetical IFV passenger,
/// spawned missile, mind control, parasite or self-destruction counted as
/// ordinary anti-armour firepower. Read the native armour multipliers.
pub(super) fn line_quality(rules: &RuleSet, ty: &ObjectType) -> i32 {
    if ty.category != ObjectCategory::Vehicle || ty.naval || ty.balloon_hover
        || ty.jumpjet || ty.movement_zone == MovementZone::Fly || ty.gunner
        || ty.strength < 200 || ty.speed <= 0 || ty.resource_gatherer
        || ty.spawned || !super::orders::is_combat_unit(rules, ty)
        || doctrine::duty(rules, ty) != doctrine::Duty::Line
    { return 0; }
    let Some(gun) = ty.weapon_at(0).and_then(|name| rules.weapon(name)) else { return 0; };
    if gun.never_use || gun.suicide || gun.spawner || gun.area_fire || gun.damage <= 0
        || !rules.weapon_projectile(gun).is_some_and(|p| p.ag)
    { return 0; }
    let Some(wh) = gun.warhead.as_deref().and_then(|name| rules.warhead(name)) else { return 0; };
    if wh.mind_control || wh.parasite || wh.ivan_bomb || wh.is_locomotor { return 0; }
    let mult = ["medium", "heavy"].into_iter().map(|name| {
        f64::from_bits(wh.interaction(armor_index(name)).multiplier().bits())
    }).fold(0.0_f64, f64::max);
    if mult < 0.35 { return 0; }
    let damage = (f64::from(gun.damage) * mult).clamp(0.0, 8_000.0) as i32;
    damage.saturating_mul(gun.burst.max(1)).saturating_mul(90) / gun.rof.max(15)
}

fn choices<'r>(rules: &'r RuleSet, obs: &Observation<'_>)
    -> Vec<(&'r ObjectType, InternedId, i32, i32)>
{
    build::class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| obs.can_build(rules, ty, true) == CanBuild::Yes
            && obs.has_producer(rules, ty))
        .filter_map(|(ty, id)| {
            let quality = line_quality(rules, ty);
            let cost = obs.cost_of(rules, ty);
            (quality > 0 && cost > 0).then_some((ty, id, cost, quality))
        }).collect()
}

fn ready(count: usize, points: i32) -> bool {
    count >= WAVE_VEHICLES && points >= WAVE_POINTS
}

#[derive(Debug, Default)]
pub(super) struct Mobilization {
    active: bool,
    completed: bool,
    last_sample: Option<u64>,
    floor: Option<i32>,
}

impl Mobilization {
    pub(super) fn active(&self) -> bool { self.active }
    pub(super) fn floor(&self) -> Option<i32> { self.floor.filter(|_| self.active) }

    pub(super) fn observe(&mut self, rules: &RuleSet, obs: &Observation<'_>, census: &Census) {
        let now = obs.tick();
        if !obs.production_enabled() || census.yards == 0 {
            // A player disabling production or a house with no yard must not
            // be forced to wait for an opening which it cannot execute.
            self.active = false;
            self.floor = None;
            self.last_sample = None;
            return;
        }
        if self.completed { return; }
        if now >= OPENING_LIMIT {
            if self.active { log::info!("auto play: mobilization_exit tick={now} reason=deadline"); }
            self.active = false;
            self.completed = true;
            return;
        }
        if self.last_sample.is_some_and(|at| now.saturating_sub(at) < SAMPLE_PERIOD) { return; }
        self.last_sample = Some(now);
        let (mut count, mut points) = (0_usize, 0_i32);
        for unit in &census.units {
            if census.available(unit) && unit.health_permille >= 500
                && obs.object_type(rules, unit.type_id).is_some_and(|ty| line_quality(rules, ty) > 0)
            {
                count += 1;
                points = points.saturating_add(unit.points.max(0));
            }
        }
        if ready(count, points) {
            self.active = false;
            self.completed = true;
            log::info!("auto play: mobilization_exit tick={now} reason=wave-ready vehicles={count} points={points}");
            return;
        }
        self.floor = choices(rules, obs).into_iter().map(|(_, _, cost, _)| cost).min();
        // No native line yet: prioritise the first factory. A mod with an
        // operational factory but no matching line vehicle should use its
        // ordinary policy, not wait nine thousand frames for an impossible unit.
        let factory = obs.own_entities().any(|entity| {
            !obs.building_under_construction(entity.entity_id)
                && obs.object_type(rules, entity.type_id).is_some_and(|ty|
                    ty.factory_type() == Some(crate::rules::object_type::FactoryType::UnitType)
                    && !ty.naval && !rules.shipyard_types.iter().any(|name| name == &ty.id))
        });
        let was = self.active;
        self.active = !factory || self.floor.is_some();
        if was != self.active || now % 600 < SAMPLE_PERIOD {
            log::info!("auto play: mobilization tick={now} active={} vehicles={count} points={points} floor={:?}", self.active, self.floor);
        }
    }

    /// Core infrastructure remains open; native repeatable reinforcement
    /// providers may be bought early if two real vehicles remain funded.
    /// No country name is a policy input.
    pub(super) fn admits(&self, item: Item, rules: &RuleSet, obs: &Observation<'_>,
        census: &Census, free: i32) -> bool
    {
        if !self.active { return true; }
        if matches!(item, Item::Power | Item::Refinery | Item::Barracks | Item::Weapons | Item::Defense) {
            return true;
        }
        let Some(floor) = self.floor else { return false; };
        let provider = match item {
            Item::Aircraft => build::line_factory(rules, obs, census, crate::sim::production::ProductionCategory::Aircraft),
            Item::Radar => build::first_buildable(rules, obs, &rules.build_radar_types),
            _ => None,
        };
        provider.is_some_and(|(ty, _)| grants_reinforcement(rules, ty)
            && free.saturating_sub(obs.cost_of(rules, ty)) >= floor.saturating_mul(2))
    }
}

fn grants_reinforcement(rules: &RuleSet, ty: &ObjectType) -> bool {
    [ty.super_weapon.as_deref(), ty.super_weapon2.as_deref()].into_iter().flatten()
        .filter_map(|name| rules.super_weapon(name))
        .any(|power| matches!(power.kind, SuperWeaponKind::ParaDrop | SuperWeaponKind::AmerParaDrop))
}

/// Choose an affordable, legal first-wave vehicle. The price and native
/// producer are rechecked on the purchase frame, not taken from a cached ID.
pub(super) fn vehicle<'r>(rules: &'r RuleSet, obs: &Observation<'_>, free: i32)
    -> Option<(&'r ObjectType, InternedId)>
{
    choices(rules, obs).into_iter().filter(|(_, _, cost, _)| *cost <= free)
        .max_by_key(|(ty, id, cost, quality)| (
            i64::from(*quality).saturating_mul(i64::from(ty.strength)).saturating_mul(1_000) / i64::from(*cost),
            std::cmp::Reverse(*cost), std::cmp::Reverse(*id)))
        .map(|(ty, id, _, _)| (ty, id))
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    #[test]
    fn damaged_or_too_few_vehicles_do_not_release_wave() {
        assert!(!ready(2, 1_000));
        assert!(!ready(3, 649));
        assert!(ready(3, 650));
    }
    #[test]
    fn retail_line_is_not_a_dog_drone_ifv_or_miner() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else { return; };
        for id in ["MTNK", "HTNK", "LTNK"] {
            assert!(line_quality(&rules, rules.object(id).unwrap()) > 0, "{id}");
        }
        for id in ["DRON", "FV", "HARV", "CMIN", "SMIN", "DTRUCK", "DOG", "ZEP"] {
            assert_eq!(line_quality(&rules, rules.object(id).unwrap()), 0, "{id}");
        }
    }
    #[test]
    fn repeatable_reinforcement_is_a_rules_property() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else { return; };
        assert!(grants_reinforcement(&rules, rules.object("AMRADR").unwrap()));
        assert!(!grants_reinforcement(&rules, rules.object("GAAIRC").unwrap()));
    }
}
