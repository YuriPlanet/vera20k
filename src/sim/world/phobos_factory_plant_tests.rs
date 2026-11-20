//! Phobos `FactoryPlant.AllowTypes` / `.DisallowTypes` / `.MaxCount` through
//! the real House registration, Cost_Of and GetRefund owners.
//!
//! Native: `BuildingClass_Unlimbo_FactoryPlant @ 0x441501`
//! (`Ext/Building/Hooks.cpp:437-454`), `HouseExt::GetRestrictedFactoryPlantMult`
//! (`Ext/House/Body.cpp:538-588`), the cost/refund hooks `0x711F39`/`0x711FDF`
//! (`Ext/TechnoType/Hooks.cpp:97-122`) and `HouseExt::OnDetach`
//! (`Ext/House/Body.cpp:743-772`).

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";
const OTHER_OWNER: &str = "Russians";

fn make_rules(plant_keys: &str) -> RuleSet {
    make_rules_bonuses(plant_keys, "0.8", "0.8")
}

fn make_rules_bonuses(plant_keys: &str, buildings_bonus: &str, defenses_bonus: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n0=PLANT\n1=PLAIN\n2=DEFB\n\
         [E1]\nCost=1000\nStrength=100\n\
         [MTNK]\nCost=1000\nStrength=100\nSpeed=4\n\
         [PLANT]\nFactoryPlant=yes\nCost=100\nStrength=1000\nFoundation=1x1\n\
         InfantryCostBonus=0.8\nUnitsCostBonus=0.8\nAircraftCostBonus=0.8\n\
         BuildingsCostBonus={buildings_bonus}\nDefensesCostBonus={defenses_bonus}\n{plant_keys}\
         [PLAIN]\nCost=100\nStrength=1000\nFoundation=1x1\n\
         [DEFB]\nCost=1000\nStrength=1000\nFoundation=1x1\nBuildCat=Combat\n"
    )))
    .unwrap()
}

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    owner: InternedId,
    other: InternedId,
    plants: Vec<u64>,
}

fn make_scene(plant_keys: &str, count: usize) -> Scene {
    make_scene_with_rules(make_rules(plant_keys), count)
}

fn make_scene_with_rules(rules: RuleSet, count: usize) -> Scene {
    let mut sim = Simulation::with_seed(0x143);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern(OWNER);
    let other = sim.intern(OTHER_OWNER);
    for (id, is_human) in [(owner, true), (other, true)] {
        sim.houses
            .register(HouseState::new(id, 0, None, is_human, 10_000, 10));
    }
    let mut plants = Vec::new();
    for index in 0..count {
        let id = 70 + index as u64;
        let ty = sim.interner.intern("PLANT");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            10 + index as u16 * 3,
            20,
            0,
            0,
            owner,
            Health::new(1000),
            ty,
            EntityCategory::Structure,
            0,
            3,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(false);
        entity.set_in_playfield(true);
        sim.set_next_stable_object_id_for_test(sim.next_stable_object_id().max(id + 1));
        sim.substrate.entities.insert(entity);
        // Unlimbo's registration and House+140/restricted append
        // (`0x4414F7..0x441553`).
        Simulation::register_house_base_building(
            &sim.interner,
            &sim.substrate,
            &mut sim.houses,
            id,
            &rules,
        );
        sim.append_house_base_building_for_test(id);
        plants.push(id);
    }
    Scene {
        sim,
        rules,
        owner,
        other,
        plants,
    }
}

fn priced(scene: &Scene, owner: InternedId, type_name: &str) -> i32 {
    scene
        .sim
        .cost_of(owner, scene.rules.object(type_name).unwrap(), &scene.rules)
}

/// `AllowTypes` (`Ext/House/Body.cpp:548-549`): the listed type receives the
/// plant's 0.8 factor, every other type keeps the undiscounted price.
#[test]
fn allow_types_discounts_only_the_listed_type() {
    let scene = make_scene("FactoryPlant.AllowTypes=MTNK\n", 1);
    assert_eq!(priced(&scene, scene.owner, "MTNK"), 800);
    assert_eq!(priced(&scene, scene.owner, "E1"), 1000);
    // `Parser<TechnoTypeClass*>::Parse` drops an unknown token before
    // `ValueableVector::size()` is asked, so an all-unknown allow list is
    // empty and applies no filter at all.
    let unknown = make_scene("FactoryPlant.AllowTypes=NOTATYPE\n", 1);
    assert_eq!(priced(&unknown, unknown.owner, "E1"), 800);
}

/// `DisallowTypes` (`Ext/House/Body.cpp:551-552`) with no allow list: the
/// denied type keeps its price and the other one is discounted.
#[test]
fn disallow_types_refuses_only_the_listed_type() {
    let scene = make_scene("FactoryPlant.DisallowTypes=MTNK\n", 1);
    assert_eq!(priced(&scene, scene.owner, "MTNK"), 1000);
    assert_eq!(priced(&scene, scene.owner, "E1"), 800);
}

/// `MaxCount` (`Ext/House/Body.cpp:545-546`) counts per plant BuildingType;
/// both plants must be restricted, so the allow list is present. Two 0.8
/// bonuses fold to 0.64 when the cap is the constructor's -1.
#[test]
fn max_count_caps_identical_restricted_plants() {
    let capped = make_scene("FactoryPlant.AllowTypes=MTNK\nFactoryPlant.MaxCount=1\n", 2);
    assert_eq!(priced(&capped, capped.owner, "MTNK"), 800);
    let uncapped = make_scene("FactoryPlant.AllowTypes=MTNK\n", 2);
    assert_eq!(priced(&uncapped, uncapped.owner, "MTNK"), 640);
}

/// The Unlimbo hook's `Skip = 0x441553`: a restricted plant must not join
/// House+140, so the plain product cannot discount a type its allow list
/// excludes.
#[test]
fn a_restricted_plant_never_enters_the_plain_house_product() {
    let scene = make_scene("FactoryPlant.AllowTypes=MTNK\n", 1);
    let base = scene.sim.houses[&scene.owner].base_projection();
    assert!(base.factory_plants().is_empty());
    assert_eq!(base.restricted_plants(), scene.plants.as_slice());
    assert_eq!(priced(&scene, scene.owner, "E1"), 1000);
}

/// `0x711FDF` multiplies the same restricted factor into GetRefund, so a
/// full refund of a discounted type equals its discounted Cost_Of.
#[test]
fn refund_uses_the_same_restricted_factor_as_cost() {
    let scene = make_scene("FactoryPlant.AllowTypes=MTNK\n", 1);
    let object = scene.rules.object("MTNK").unwrap();
    let factors =
        Simulation::house_cost_factors_for(&scene.sim.houses, scene.owner, object).unwrap();
    let refund = crate::sim::production::type_refund(
        &scene.rules,
        object,
        &scene.sim.houses[&scene.owner],
        &factors,
        scene.sim.session().rule_policy(),
        true,
    );
    assert_eq!(refund, priced(&scene, scene.owner, "MTNK"));
    assert_eq!(refund, 800);
}

/// ChangeOwner (`0x448A31`/`0x449149`) moves the plant between the two
/// Houses' restricted vectors; OnDetach (`Ext/House/Body.cpp:765-770`)
/// removes it on death. A FactoryPlant= type without either list stays in
/// House+140 (`0x4414F7..0x4414FF`).
#[test]
fn capture_moves_the_restricted_plant_and_death_removes_it() {
    let mut scene = make_scene("FactoryPlant.AllowTypes=MTNK\n", 1);
    let id = scene.plants[0];
    scene.sim.houses.leave_house_base_lists(id, scene.owner);
    scene
        .sim
        .houses
        .move_house_base_tracking(id, scene.owner, scene.other);
    scene
        .sim
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_owner_for_test(scene.other);
    scene.sim.append_house_base_building_for_test(id);
    assert!(
        scene.sim.houses[&scene.owner]
            .base_projection()
            .restricted_plants()
            .is_empty()
    );
    assert_eq!(
        scene.sim.houses[&scene.other]
            .base_projection()
            .restricted_plants(),
        &[id][..]
    );
    assert_eq!(priced(&scene, scene.other, "MTNK"), 800);

    scene.sim.houses.leave_house_base_lists(id, scene.other);
    assert!(
        scene.sim.houses[&scene.other]
            .base_projection()
            .restricted_plants()
            .is_empty()
    );
    assert_eq!(priced(&scene, scene.other, "MTNK"), 1000);
}

/// `FactoryPlant.Multiplier` (`Ext/House/Body.cpp:585`): the restricted
/// product is folded as `1 - (1 - product) * multiplier` before the cost
/// multiplies. One 0.8 plant with a multiplier of 2 discounts to 0.6.
#[test]
fn factory_plant_multiplier_scales_the_restricted_discount() {
    // `FactoryPlant.Multiplier` is an Ares TypeExt key: only a game whose
    // Rules are read the Ares way holds it.
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    // `GetRestrictedFactoryPlantMult` multiplies by the *priced* type's
    // `FactoryPlant.Multiplier` (`Ext/House/Body.cpp:585`).
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n0=PLANT\n1=PLAIN\n\
         [MTNK]\nCost=1000\nStrength=100\nSpeed=4\nFactoryPlant.Multiplier=2\n\
         [PLANT]\nFactoryPlant=yes\nCost=100\nStrength=1000\nFoundation=1x1\n\
         UnitsCostBonus=0.8\nFactoryPlant.AllowTypes=MTNK\n\
         [PLAIN]\nCost=100\nStrength=1000\nFoundation=1x1\n",
    ))
    .unwrap();
    let scene = make_scene_with_rules(rules, 1);
    assert_eq!(priced(&scene, scene.owner, "MTNK"), 600);
}

/// The priced type's `WhatAmI`/`BuildCat` slot picks the plant bonus
/// (`Ext/House/Body.cpp:565-583`): a `BuildCat=Combat` building reads
/// `DefensesCostBonus`, an ordinary building `BuildingsCostBonus`.
#[test]
fn a_combat_building_reads_the_defenses_bonus_slot() {
    let scene = make_scene_with_rules(
        make_rules_bonuses("FactoryPlant.AllowTypes=PLAIN,DEFB\n", "0.9", "0.5"),
        1,
    );
    // C++ float: 100 * 0.9f truncates to 89.
    assert_eq!(priced(&scene, scene.owner, "PLAIN"), 89);
    assert_eq!(priced(&scene, scene.owner, "DEFB"), 500);
}
