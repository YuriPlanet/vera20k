//! Phobos `LimboDelivery`/`LimboKill` through the production launch tail
//! (`Ext/SWType/FireSuperWeapon.cpp:86-262`).

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::superweapon::chronosphere_tests::{charge_super, click};
use crate::sim::superweapon::construct_house_supers;

const OWNER: &str = "Americans";
const RIVAL: &str = "Russians";
const ROOT: &str = "RootSpecial";
const KILL: &str = "KillSpecial";
const TARGET: (u16, u16) = (30, 30);

const BASE: &str = "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
[BuildingTypes]\n0=B1\n1=B2\n\
[B1]\nStrength=100\n[B2]\nStrength=100\n\
[SuperWeaponTypes]\n0=RootSpecial\n1=KillSpecial\n\
[RootSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
[KillSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n";

const ART: &str = "[B1]\nFoundation=1x1\n[B2]\nFoundation=1x1\n";

fn rules_with(extra: &str) -> RuleSet {
    let mut ini = IniFile::from_str(BASE);
    ini.merge(&IniFile::from_str(extra));
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
        .expect("rules with the limbo keys parse");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

fn world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let mut owner = None;
    for (name, human) in [(OWNER, true), (RIVAL, false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, human, 0, 10,
            ));
        construct_house_supers(&mut sim, &rules, id, false);
        owner.get_or_insert(id);
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().super_weapons = true;
    (rules, sim, owner.unwrap())
}

fn buildings(sim: &Simulation, owner: InternedId, name: &str) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| entity.owner() == owner)
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == name)
        .map(|entity| entity.stable_id())
        .collect()
}

fn delivered(sim: &Simulation, owner: InternedId) -> Vec<(u64, i32)> {
    sim.houses[&owner].tracking().limbo_delivered().to_vec()
}

fn owned(sim: &Simulation, owner: InternedId, name: &str) -> i32 {
    let type_id = sim.interner.get(name).expect("interned building type");
    sim.houses[&owner]
        .tracking()
        .count_owned_present_and_limboed(EntityCategory::Structure, type_id)
}

/// `ApplyLimboDelivery` creates both listed types with their `LimboID`s and
/// registers them in the house's delivered list and limbo counter.
#[test]
fn delivery_creates_the_listed_buildings_with_their_ids() {
    let rules = rules_with("[RootSpecial]\nLimboDelivery.Types=B1,B2\nLimboDelivery.IDs=5,7\n");
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    let b1 = buildings(&sim, owner, "B1");
    let b2 = buildings(&sim, owner, "B2");
    assert_eq!(b1.len(), 1, "one B1 delivered");
    assert_eq!(b2.len(), 1, "one B2 delivered");
    let list = delivered(&sim, owner);
    assert_eq!(list.len(), 2);
    assert!(list.contains(&(b1[0], 5)), "B1 keeps LimboID 5: {list:?}");
    assert!(list.contains(&(b2[0], 7)), "B2 keeps LimboID 7: {list:?}");
    assert_eq!(
        owned(&sim, owner, "B1"),
        2,
        "the delivered B1 counts as on-map and as limbo-tracked"
    );
}

/// A weighted delivery rolls once per `LimboDelivery.RollChances` entry and
/// takes the id of the picked index.
#[test]
fn weighted_delivery_selects_one_type() {
    let rules = rules_with(
        "[RootSpecial]\nLimboDelivery.Types=B1,B2\nLimboDelivery.IDs=5,7\n\
         LimboDelivery.RandomWeights=0,1\n",
    );
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    assert!(buildings(&sim, owner, "B1").is_empty(), "weight 0 skips B1");
    let b2 = buildings(&sim, owner, "B2");
    assert_eq!(b2.len(), 1);
    assert_eq!(delivered(&sim, owner), [(b2[0], 7)]);
}

/// `ApplyLimboKill` only walks houses the mask accepts, caps by `Counts` and
/// destroys the selected buildings through the lifecycle.
#[test]
fn kill_removes_matching_ids_for_accepted_houses() {
    let rules = rules_with("[RootSpecial]\nLimboDelivery.Types=B1,B2\nLimboDelivery.IDs=5,7\n");
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    let b1 = buildings(&sim, owner, "B1")[0];
    let b2 = buildings(&sim, owner, "B2")[0];

    // The rival's kill (default Owner) leaves the American deliveries alone.
    let rival = sim.interner.get(RIVAL).unwrap();
    let rules_rival = rules_with(
        "[RootSpecial]\nLimboDelivery.Types=B1,B2\nLimboDelivery.IDs=5,7\n\
         [KillSpecial]\nLimboKill.IDs=5\n",
    );
    charge_super(&mut sim, rival, KILL);
    click(&mut sim, &rules_rival, rival, KILL, TARGET);
    assert!(
        sim.substrate.entities.get(b1).is_some(),
        "another house's kill does not touch the owner's list"
    );

    let rules_owner = rules_with(
        "[RootSpecial]\nLimboDelivery.Types=B1,B2\nLimboDelivery.IDs=5,7\n\
         [KillSpecial]\nLimboKill.IDs=5\n",
    );
    charge_super(&mut sim, owner, KILL);
    click(&mut sim, &rules_owner, owner, KILL, TARGET);
    assert!(
        sim.substrate.entities.get(b1).is_none(),
        "the matching delivery is destroyed"
    );
    assert!(
        sim.substrate.entities.get(b2).is_some(),
        "a different LimboID is untouched"
    );
    assert_eq!(delivered(&sim, owner), [(b2, 7)]);

    // Counts caps the removals for a repeated id.
    let capped = rules_with(
        "[RootSpecial]\nLimboDelivery.Types=B1,B1\nLimboDelivery.IDs=9,9\n\
         [KillSpecial]\nLimboKill.IDs=9\nLimboKill.Counts=1\n",
    );
    let (capped, mut sim, owner) = world(capped);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &capped, owner, ROOT, TARGET);
    assert_eq!(buildings(&sim, owner, "B1").len(), 2);
    charge_super(&mut sim, owner, KILL);
    click(&mut sim, &capped, owner, KILL, TARGET);
    assert_eq!(
        buildings(&sim, owner, "B1").len(),
        1,
        "Counts=1 removes one of the two id-9 deliveries"
    );
}

/// `LimboCreate` (`Ext/SWType/FireSuperWeapon.cpp:87-170`, :131-142) appends
/// a `FactoryPlant=` building to `RestrictedFactoryPlants` instead of
/// House+140 when either type list is non-empty. Phobos writes that append by
/// hand because `LimboCreate` bypasses Unlimbo; VERA delivers through
/// `spawn_object_at_height_with_overlay_context` -> `try_reveal_entity_with_context`
/// -> `append_house_base_building`, so the same `append_membership` gate
/// decides which list receives it.
#[test]
fn delivered_restricted_factory_plant_uses_the_restricted_list() {
    let rules = rules_with(
        "[VehicleTypes]\n0=MTNK\n\
         [MTNK]\nCost=1000\nStrength=100\nSpeed=4\n\
         [B1]\nFactoryPlant=yes\nUnitsCostBonus=0.8\nFactoryPlant.AllowTypes=MTNK\n\
         [RootSpecial]\nLimboDelivery.Types=B1\n",
    );
    let (rules, mut sim, owner) = world(rules);
    charge_super(&mut sim, owner, ROOT);
    click(&mut sim, &rules, owner, ROOT, TARGET);
    let b1 = buildings(&sim, owner, "B1");
    assert_eq!(b1.len(), 1, "one restricted plant delivered");
    let base = sim.houses[&owner].base_projection();
    assert!(base.factory_plants().is_empty());
    assert_eq!(base.restricted_plants(), &[b1[0]][..]);
}
