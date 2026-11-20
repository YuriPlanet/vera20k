//! Regressions for the Phobos `Units.*` repair overrides: the repair-tick
//! step/percent/cost switch, the `UnitReload=` separate repair loop and its
//! skip-effects latch.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};

fn scene(depot_lines: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n\
         [BuildingTypes]\n0=DEPOT\n1=AIRFIELD\n2=HOSP\n3=SERVICE\n\
         [VehicleTypes]\n0=TANK\n[InfantryTypes]\n0=GI\n\
         [General]\nRepairPercent=15%\nRepairStep=8\nIRepairStep=4\n\
         URepairRate=.016\nIRepairRate=.016\n\
         [DEPOT]\nStrength=1000\nFoundation=1x1\nUnitRepair=yes\n\
         {depot_lines}\
         [AIRFIELD]\nStrength=1000\nFoundation=1x1\nUnitReload=yes\n\
         Units.RepairRate=0.0025\n\
         [HOSP]\nStrength=1000\nFoundation=1x1\nHospital=yes\nUnits.RepairRate=0.004\n\
         [SERVICE]\nStrength=1000\nFoundation=1x1\nUnitRepair=yes\nUnits.RepairRate=0.002\n\
         [TANK]\nStrength=300\nCost=300\nSpeed=6\nArmor=none\n\
         [GI]\nImage=GI\nStrength=100\nCost=100\nSpeed=4\nArmor=none\n"
    ));
    let art = IniFile::from_str(&format!(
        "{}\n[DEPOT]\nFoundation=1x1\n[AIRFIELD]\nFoundation=1x1\n\
         [HOSP]\nFoundation=1x1\n[SERVICE]\nFoundation=1x1\n",
        crate::rules::retail_ini_fixture::GI_ART_EXCERPT,
    ));
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("units repair rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(1000);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object_at_height(kind, "Americans", x, y, 0, 0, rules)
        .expect("spawn")
}

fn set_health(sim: &mut Simulation, id: u64, health: i32) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .health
        .set_for_test(health);
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().health.current()
}

fn credits(sim: &Simulation) -> i32 {
    let owner = sim.interner.get("Americans").unwrap();
    sim.houses[&owner].economy().credits()
}

fn repair_tick(sim: &mut Simulation, rules: &RuleSet, from: u64, to: u64) -> RadioResponse {
    radio::transmit(
        sim,
        from,
        to,
        RadioMessage::RepairTick,
        RadioPayload::default(),
        rules,
        FrameEffects::default(),
    )
}

/// `0x6F4D1A`: the depot's `Units.RepairStep=7` and `Units.RepairPercent=50%`
/// set the heal and the cost; the double cost formula truncates.
#[test]
fn units_repair_step_and_percent_override_the_receiver() {
    let (mut sim, rules) = scene(
        "Units.RepairStep=7
Units.RepairPercent=50%
",
    );
    let depot = spawn(&mut sim, &rules, "DEPOT", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", 12, 10);
    set_health(&mut sim, tank, 200);
    assert_eq!(
        repair_tick(&mut sim, &rules, depot, tank),
        RadioResponse::Roger
    );
    assert_eq!(health(&sim, tank), 207, "Units.RepairStep=7 heals");
    assert_eq!(credits(&sim), 997, "trunc(300 / (300/7) * 50%) costs 3");
}

/// An infantryman pays nothing unless `Units.UseRepairCost=yes`; the override
/// step still applies to the heal.
#[test]
fn units_use_repair_cost_defaults_to_free_for_infantry() {
    let (mut sim, rules) = scene(
        "Units.RepairStep=7
Units.RepairPercent=50%
",
    );
    let depot = spawn(&mut sim, &rules, "DEPOT", 10, 10);
    let gi = spawn(&mut sim, &rules, "GI", 12, 10);
    set_health(&mut sim, gi, 50);
    let _ = repair_tick(&mut sim, &rules, depot, gi);
    assert_eq!(health(&sim, gi), 57, "the override step still heals");
    assert_eq!(credits(&sim), 1000, "infantry repair is free by default");
}

/// With `Units.UseRepairCost=yes` the infantryman pays the truncated cost
/// (at least 1) over the passed `IRepairStep`.
#[test]
fn units_use_repair_cost_can_charge_infantry() {
    let (mut sim, rules) = scene(
        "Units.UseRepairCost=yes
",
    );
    let depot = spawn(&mut sim, &rules, "DEPOT", 10, 10);
    let gi = spawn(&mut sim, &rules, "GI", 12, 10);
    set_health(&mut sim, gi, 50);
    let _ = repair_tick(&mut sim, &rules, depot, gi);
    assert_eq!(
        health(&sim, gi),
        54,
        "IRepairStep=4 heals without an override"
    );
    assert_eq!(credits(&sim), 999, "trunc(100 / (100/4) * 15%) = 0 -> 1");
}

/// A `UnitReload=` building with `Units.RepairRate=` owns its repairs: its
/// stock REPAIR_TICK effects are skipped, the separate view applies them.
#[test]
fn a_unit_reload_rate_skips_the_stock_repair_tick() {
    let (mut sim, rules) = scene("");
    let airfield = spawn(&mut sim, &rules, "AIRFIELD", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", 12, 10);
    set_health(&mut sim, tank, 200);
    assert_eq!(
        repair_tick(&mut sim, &rules, airfield, tank),
        RadioResponse::Roger
    );
    assert_eq!(health(&sim, tank), 200, "the stock arm's tick is skipped");
    assert_eq!(credits(&sim), 1000);
    crate::sim::radio::receive::techno_repair_tick_separate(
        &mut sim,
        tank,
        Some(airfield),
        &rules,
        FrameEffects::default(),
    );
    assert_eq!(health(&sim, tank), 208, "RepairStep=8 heals");
    assert_eq!(credits(&sim), 999, "trunc(300 / (300/8) * 15%) = 1");
}

/// The separate loop repairs a linked, grounded, damaged unit on the
/// `int(max(rate * 900, 1))`-frame period.
#[test]
fn the_unit_reload_rate_repairs_linked_units_on_its_period() {
    let (mut sim, rules) = scene("");
    let airfield = spawn(&mut sim, &rules, "AIRFIELD", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", 12, 10);
    set_health(&mut sim, tank, 200);
    sim.substrate
        .entities
        .get_mut(airfield)
        .unwrap()
        .radio_contacts
        .insert(tank);
    // 0.0025 * 900 = 2.25 -> 2 frames.
    sim.session.set_binary_frame_for_test(1);
    crate::sim::docking::airfield_reload::mission_reload(
        &mut sim,
        &rules,
        airfield,
        FrameEffects::default(),
    );
    assert_eq!(health(&sim, tank), 200, "frame 1 is off the period");
    sim.session.set_binary_frame_for_test(2);
    crate::sim::docking::airfield_reload::mission_reload(
        &mut sim,
        &rules,
        airfield,
        FrameEffects::default(),
    );
    assert_eq!(health(&sim, tank), 208, "frame 2 repairs");
}

/// The separate-repair latch answers before the native health refusal: a
/// full-health contact on such a dock still gets ROGER, so the stock arm does
/// not release it.
#[test]
fn a_full_health_contact_answers_roger_on_a_separate_repair_dock() {
    let (mut sim, rules) = scene("");
    let airfield = spawn(&mut sim, &rules, "AIRFIELD", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", 12, 10);
    assert_eq!(
        repair_tick(&mut sim, &rules, airfield, tank),
        RadioResponse::Roger
    );
    assert_eq!(health(&sim, tank), 300);
    assert_eq!(credits(&sim), 1000);
}

/// `Units.RepairRate` on a hospital replaces `[General] IRepairRate=` for the
/// RepairTick threshold (0.004 * 900 against 0.016 * 900).
#[test]
fn units_repair_rate_overrides_the_hospital_period() {
    let (mut sim, rules) = scene("");
    let hosp = spawn(&mut sim, &rules, "HOSP", 10, 10);
    let gi = spawn(&mut sim, &rules, "GI", 12, 10);
    set_health(&mut sim, gi, 50);
    sim.substrate
        .entities
        .get_mut(hosp)
        .unwrap()
        .radio_contacts
        .insert(gi);
    let tick = |sim: &mut Simulation| {
        crate::sim::docking::hospital::mission_repair(
            sim,
            &rules,
            hosp,
            None,
            FrameEffects::default(),
        );
    };
    sim.session.set_binary_frame_for_test(0);
    tick(&mut sim);
    for frame in 1..=3 {
        sim.session.set_binary_frame_for_test(frame);
        tick(&mut sim);
    }
    assert_eq!(health(&sim, gi), 50, "3 < 0.004 * 900 = 3.6");
    sim.session.set_binary_frame_for_test(4);
    tick(&mut sim);
    assert_eq!(health(&sim, gi), 54, "the overridden rate ticks at 4");
}
