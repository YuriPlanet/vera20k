//! House domain claims (RFC 0001 6i) on the retail Battle against one
//! computer house: a `Production` claim stops the native AI building, a
//! human house refuses claims, a stopped module releases its claims, a save
//! keeps them, and a "mod AI" seat starts with every domain claimed.

use vera20k_wasm_ext::{DOMAIN_ALL, DOMAIN_PRODUCTION, Status};

use super::tests::{
    ext_hash, install, mem_i32, prop, saved_and_loaded, scenario_with_computer,
    session_with_computer,
};
use super::{ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::map::entities::EntityCategory;

/// (the computer house's index, the player's house index).
fn houses(scene: &HeadlessScenario) -> (usize, usize) {
    let sim = scene.sim();
    let computer = sim
        .interner
        .get("Computer1")
        .and_then(|id| sim.houses.index_of(id))
        .expect("the battle has a computer house");
    let human = sim
        .houses
        .order()
        .iter()
        .position(|id| sim.houses[id].is_human())
        .expect("the battle has a player");
    (computer, human)
}

/// Live structures house `index` owns.
fn structures(scene: &HeadlessScenario, index: usize) -> usize {
    let sim = scene.sim();
    let owner = sim.houses.order()[index];
    sim.entities()
        .iter_sorted()
        .filter(|(_, e)| {
            e.owner() == owner
                && e.category() == EntityCategory::Structure
                && e.lifecycle.object_alive()
        })
        .count()
}

/// Claims `domains` of `computer` and of `human` in `on_scenario_start`
/// (statuses at 0 and 4, the computer's `ControlDomains` read back at 8);
/// with `stop_after`, its `on_frame` traps on that call.
fn claiming_module(computer: usize, human: usize, domains: i64, stop_after: i32) -> Vec<u8> {
    let (computer, human) = (house_ref(computer), house_ref(human));
    let control = prop("ControlDomains");
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "set" (func $set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (i32.store (i32.const 0) (call $set (i64.const {computer}) (i32.const {control}) (i64.const {domains})))
    (i32.store (i32.const 4) (call $set (i64.const {human}) (i32.const {control}) (i64.const {domains})))
    (drop (call $get (i64.const {computer}) (i32.const {control}) (i32.const 8))))
  (func (export "on_frame")
    (i32.store (i32.const 16) (i32.add (i32.load (i32.const 16)) (i32.const 1)))
    (if (i32.eq (i32.load (i32.const 16)) (i32.const {stop_after})) (then unreachable))))"#
    ))
    .expect("test module parses")
}

/// A module claims the computer's `Production`: while the native AI of an
/// unclaimed run deploys its MCV and builds, the claimed house builds
/// nothing and starts no factory. The player's house refuses the claim.
#[test]
fn a_production_claim_stops_the_computer_building() {
    let Some(mut plain) = scenario_with_computer() else {
        return;
    };
    let mut claimed = scenario_with_computer().expect("second load");
    let (computer, human) = houses(&claimed);
    install(
        &mut claimed,
        ModuleSource::Map,
        &claiming_module(computer, human, DOMAIN_PRODUCTION, -1),
    );
    let start = structures(&plain, computer);
    let mut frames = 0;
    while structures(&plain, computer) < start + 2 && frames < 2400 {
        plain.tick();
        claimed.tick();
        frames += 1;
    }
    assert_eq!(mem_i32(&claimed, 0, 0), Status::Ok as i32);
    assert_eq!(mem_i32(&claimed, 0, 4), Status::Refused as i32);
    assert_eq!(mem_i32(&claimed, 0, 8), DOMAIN_PRODUCTION as i32);
    assert_eq!(
        claimed.sim().ext_modules().claimed_seats(ModuleSource::Map),
        1
    );
    assert!(
        structures(&plain, computer) >= start + 2,
        "the native AI builds within {frames} frames"
    );
    // The MCV's own deploy may stand as a yard; nothing else is built.
    assert!(structures(&claimed, computer) <= start + 1);
    let sim = claimed.sim();
    let owner = sim.houses.order()[computer];
    assert!(
        sim.entities()
            .iter_sorted()
            .filter(|(_, e)| e.owner() == owner)
            .all(|(id, _)| sim.production.factories().building_factory(id).is_none())
    );
}

/// A module that stops releases its claims; a save keeps them.
#[test]
fn a_stopped_module_releases_and_a_save_keeps_its_claims() {
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (computer, human) = houses(&scene);
    let owner = scene.sim().houses.order()[computer];
    install(
        &mut scene,
        ModuleSource::Map,
        &claiming_module(computer, human, DOMAIN_PRODUCTION, 3),
    );
    scene.tick();
    assert!(scene.sim().module_claims(owner, DOMAIN_PRODUCTION));
    let loaded = saved_and_loaded(&scene);
    assert_eq!(
        loaded.sim().ext_modules().claimed_seats(ModuleSource::Map),
        1
    );
    assert!(loaded.sim().module_claims(owner, DOMAIN_PRODUCTION));
    assert_eq!(ext_hash(&loaded), ext_hash(&scene));
    for _ in 0..3 {
        scene.tick();
    }
    assert!(scene.sim().ext_modules().status()[0].1.is_some());
    assert_eq!(
        scene.sim().ext_modules().claimed_seats(ModuleSource::Map),
        0
    );
    assert!(!scene.sim().module_claims(owner, DOMAIN_PRODUCTION));
}

/// A "mod AI" seat's house starts with every domain claimed by the mod
/// module when the mod is a controller; otherwise the native AI keeps it.
#[test]
fn a_mod_ai_seat_starts_claimed_only_under_a_controller() {
    let mut session = session_with_computer();
    session.mod_ai_slots = vec![session.opponents[0].raw_slot_index];
    let seats = session.mod_ai_house_names();
    assert_eq!(seats, ["Computer1"]);
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (computer, _) = houses(&scene);
    let module =
        wat::parse_str(r#"(module (memory (export "memory") 1))"#).expect("test module parses");
    install(&mut scene, ModuleSource::Mod, &module);
    let sim = &mut scene.runtime.simulation;
    sim.seat_mod_ai_houses(&seats);
    assert_eq!(sim.ext_modules().claimed_seats(ModuleSource::Mod), 0);
    sim.ext_modules.controller = true;
    sim.seat_mod_ai_houses(&seats);
    assert_eq!(sim.ext_modules().claimed_seats(ModuleSource::Mod), 1);
    assert_eq!(sim.ext_modules.claims.mask(computer), DOMAIN_ALL);
}
