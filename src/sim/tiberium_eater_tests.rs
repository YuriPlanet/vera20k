//! TiberiumEater through the production AI-tail owner.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::flying_text::BountyText;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::miner::ResourceType;
use crate::sim::tiberium::test_support::{
    bales_at, flat_terrain, overlay_registry, place_tiberium_on_map, tiberium_rules_text,
};
use crate::sim::world::{FrameEffects, Simulation};

fn scene(eater: &str) -> (Simulation, RuleSet, u64, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=EATER\n\
         [EATER]\nStrength=500\nArmor=none\nSpeed=6\n{eater}{}",
        tiberium_rules_text()
    )))
    .expect("rules");
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(flat_terrain(16, 16));
    place_tiberium_on_map(&mut sim, (5, 5), ResourceType::Ore, 6);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let id = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default_of_category(
        id,
        "EATER",
        "Americans",
        5,
        5,
        crate::map::entities::EntityCategory::Unit,
    );
    entity.set_type_ref_for_test(sim.interner.intern("EATER"));
    entity.set_owner_for_test(owner);
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(entity);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules, id, owner)
}

/// `FootExt::UpdateTiberiumEater` (`Ext/Foot/Body.cpp:15-121`): the FLH cell's
/// tiberium is reduced by `AmountPerCell`, credits pay
/// `amount * value * CashMultiplier`, and a label is emitted when `Display`
/// is on.
#[test]
fn an_eater_takes_amount_per_cell_and_pays_cash() {
    let (mut sim, rules, id, owner) =
        scene("TiberiumEater.TransDelay=0\nTiberiumEater.AmountPerCell=2\n");
    let texts = std::cell::RefCell::new(Vec::new());
    let observe = |_: &Simulation, _: &RuleSet, text: BountyText| {
        texts.borrow_mut().push(text);
    };
    sim.phobos_update_tiberium_eater(
        id,
        &rules,
        Some(overlay_registry()),
        FrameEffects::empty().with_bounty_text(&observe),
    );
    assert_eq!(bales_at(&sim, 5, 5), 4, "two bales were taken");
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        50,
        "2 bales x 25 credits"
    );
    let texts = texts.borrow();
    assert_eq!(texts.len(), 1, "one label per eaten cell");
    assert_eq!(texts[0].amount, 50);
    assert_eq!(
        texts[0].display_to,
        Some(crate::rules::phobos_shields::AffectedHouse::ALL)
    );
}

/// `CashMultiplier` scales the payment, and `Display=no` emits no label.
#[test]
fn cash_multiplier_scales_and_display_off_is_silent() {
    let (mut sim, rules, id, owner) = scene(
        "TiberiumEater.TransDelay=0\nTiberiumEater.AmountPerCell=2\n\
         TiberiumEater.CashMultiplier=2.0\nTiberiumEater.Display=no\n",
    );
    let texts = std::cell::RefCell::new(Vec::new());
    let observe = |_: &Simulation, _: &RuleSet, text: BountyText| {
        texts.borrow_mut().push(text);
    };
    sim.phobos_update_tiberium_eater(
        id,
        &rules,
        Some(overlay_registry()),
        FrameEffects::empty().with_bounty_text(&observe),
    );
    assert_eq!(bales_at(&sim, 5, 5), 4);
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        100,
        "2 bales x 25 x 2"
    );
    assert!(texts.borrow().is_empty());
}

/// A type without a positive `TiberiumEater.TransDelay` reads no eater and
/// eats nothing.
#[test]
fn a_negative_trans_delay_disables_the_eater() {
    let (mut sim, rules, id, owner) = scene("TiberiumEater.TransDelay=-1\n");
    sim.phobos_update_tiberium_eater(id, &rules, Some(overlay_registry()), FrameEffects::empty());
    assert_eq!(bales_at(&sim, 5, 5), 6, "nothing was taken");
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        0
    );
}

/// `Cell{i}=` stores `x * LeptonsPerCell` (`TiberiumEaterTypeClass.cpp`).
#[test]
fn cell_offsets_are_stored_in_leptons() {
    let (_sim, rules, _id, _owner) = scene("TiberiumEater.TransDelay=0\nTiberiumEater.Cell0=1,0\n");
    let settings = rules
        .object("EATER")
        .and_then(|object| object.phobos_tiberium_eater.as_ref())
        .expect("eater settings");
    assert_eq!(settings.cells, vec![(256, 0)]);
}

/// A `TransDelay` timer defers the next scan, and a limbo'd object extends
/// the running timer by one frame (`StartTime++`).
#[test]
fn trans_delay_defers_and_limbo_holds_the_timer() {
    let (mut sim, rules, id, owner) =
        scene("TiberiumEater.TransDelay=3\nTiberiumEater.AmountPerCell=2\n");
    let effects = FrameEffects::empty;
    sim.phobos_update_tiberium_eater(id, &rules, Some(overlay_registry()), effects());
    assert_eq!(bales_at(&sim, 5, 5), 4);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .phobos_tiberium_eater_timer()
            .duration(),
        3
    );
    sim.phobos_update_tiberium_eater(id, &rules, Some(overlay_registry()), effects());
    assert_eq!(bales_at(&sim, 5, 5), 4, "the timer defers the second scan");
    {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.lifecycle.set_in_limbo_for_test(true);
    }
    sim.phobos_update_tiberium_eater(id, &rules, Some(overlay_registry()), effects());
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .phobos_tiberium_eater_timer()
            .start_frame(),
        1,
        "limbo extended the running timer by one frame"
    );
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        50
    );
}

/// `AmountPerCell=0` takes the whole cell; the credits use the full amount
/// even when the removal clamps.
#[test]
fn amount_per_cell_zero_takes_the_whole_cell() {
    let (mut sim, rules, id, owner) = scene("TiberiumEater.TransDelay=0\n");
    sim.phobos_update_tiberium_eater(id, &rules, Some(overlay_registry()), FrameEffects::empty());
    assert_eq!(bales_at(&sim, 5, 5), 0, "the whole cell was taken");
    assert_eq!(
        sim.houses.get(&owner).unwrap().economy().available_money(),
        175,
        "7 bales x 25 credits"
    );
}
