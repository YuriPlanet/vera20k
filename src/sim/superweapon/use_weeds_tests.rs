//! Phobos `UseWeeds=` through the production Super AI and sidebar owners.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::util::native_x87::NativeF32Bits;

fn weed_rules(extra: &str, general: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=NAPOWR\n\
         [NAPOWR]\nStrength=1000\nPower=100\nSuperWeapon=WeedSpecial\n\
         [SuperWeaponTypes]\n0=WeedSpecial\n\
         [WeedSpecial]\nType=MultiMissile\nRechargeTime=10\nIsPowered=no\nShowTimer=yes\n\
         UseWeeds=yes\nUseWeeds.StorageTimer=yes\n{extra}\n{general}"
    )))
    .expect("UseWeeds rules")
}

/// A house with one granted `WeedSpecial` and its provider building.
fn scene(rules: &RuleSet) -> (Simulation, InternedId, InternedId) {
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let type_id = sim.interner.intern("WeedSpecial");
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(type_id, SuperWeaponInstance::new(type_id, owner, 0));
    sim.spawn_object("NAPOWR", "Americans", 10, 10, 0, rules)
        .unwrap();
    sim.power_states
        .map_mut_for_test()
        .entry(owner)
        .or_default();
    grant_provided_supers(&mut sim, rules, owner);
    sim.session.set_binary_frame_for_test(100);
    (sim, owner, type_id)
}

fn give(sim: &mut Simulation, owner: InternedId, count: i32, slot: usize) {
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .give_weed_for_test(count, slot, 10_000);
}

fn instance<'a>(
    sim: &'a Simulation,
    owner: InternedId,
    type_id: InternedId,
) -> &'a SuperWeaponInstance {
    &sim.super_weapons[&owner][&type_id]
}

fn stage(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> i32 {
    superweapon_views_for_owner(sim, rules, &owner)
        .into_iter()
        .find(|view| view.display_name == "WeedSpecial")
        .expect("granted WeedSpecial view")
        .display
        .stage
}

/// `SuperClass_AI_UseWeeds @ 0x6CBD2C` and
/// `SuperClass_AnimStage_UseWeeds @ 0x6CBEF4`: below the amount the timer
/// parks at 15 frames (at or above the readiness percentage) or 915, and the
/// cameo stage comes from the weed ratio; reaching the amount drains the store
/// and marks the Super ready, which shows the full stage.
#[test]
fn use_weeds_charges_from_the_house_store_and_parks_the_cameo_below_it() {
    let rules = weed_rules(
        "UseWeeds.Amount=10\nUseWeeds.ReadinessAnimationPercentage=0.9\n",
        "",
    );
    let (mut sim, owner, type_id) = scene(&rules);
    give(&mut sim, owner, 9, 0);
    tick_house_superweapon_instances(&mut sim, &rules, owner, crate::sim::frame_effects::FrameEffects::empty());
    let parked = instance(&sim, owner, type_id);
    assert!(!parked.is_ready);
    assert_eq!(parked.charge_start_tick, 100);
    assert_eq!(parked.charge_duration, 15, "9 of 10 is at the 0.9 edge");
    assert_eq!(stage(&sim, &rules, owner), 48, "int(9 * 54 / 10)");

    let rules_low = weed_rules(
        "UseWeeds.Amount=10\nUseWeeds.ReadinessAnimationPercentage=0.9\n",
        "",
    );
    let (mut sim, owner, type_id) = scene(&rules_low);
    give(&mut sim, owner, 8, 0);
    tick_house_superweapon_instances(&mut sim, &rules_low, owner, crate::sim::frame_effects::FrameEffects::empty());
    let parked = instance(&sim, owner, type_id);
    assert_eq!(parked.charge_duration, 915, "below the readiness edge");
    assert_eq!(stage(&sim, &rules_low, owner), 43, "int(8 * 54 / 10)");

    let rules_ready = weed_rules("UseWeeds.Amount=10\n", "");
    let (mut sim, owner, type_id) = scene(&rules_ready);
    give(&mut sim, owner, 20, 0);
    tick_house_superweapon_instances(&mut sim, &rules_ready, owner, crate::sim::frame_effects::FrameEffects::empty());
    let charged = instance(&sim, owner, type_id);
    assert!(charged.is_ready);
    assert_eq!(charged.charge_duration, 0, "RechargeTimer.Start(0)");
    assert_eq!(
        sim.houses[&owner].economy().owned_weed_total(),
        10,
        "one amount leaves the store"
    );
    assert_eq!(stage(&sim, &rules_ready, owner), 54, "ready shows the end");
    // A ready UseWeeds Super never consumes again.
    tick_house_superweapon_instances(&mut sim, &rules_ready, owner, crate::sim::frame_effects::FrameEffects::empty());
    assert_eq!(sim.houses[&owner].economy().owned_weed_total(), 10);
}

/// The store is compared as a whole but the native `RemoveAmount(amount, 0)`
/// drains slot 0 only, and a scripted charge set is refused for these Supers.
#[test]
fn use_weeds_drains_slot_zero_only_and_refuses_a_scripted_charge() {
    let rules = weed_rules("UseWeeds.Amount=10\n", "");
    let (mut sim, owner, type_id) = scene(&rules);
    give(&mut sim, owner, 3, 0);
    give(&mut sim, owner, 7, 1);
    assert!(
        instance(&sim, owner, type_id).timer_place.is_none(),
        "UseWeeds clears ShowTimer, so no sidebar timer slot is taken"
    );
    let sw = rules.super_weapon("WeedSpecial").unwrap().clone();
    assert!(
        !sim.super_weapons
            .instance_mut(&owner, &type_id)
            .unwrap()
            .set_charge_percentage(100, &sw, 100)
    );
    // SetCharged 0x6CC080 is a different function, reached by a one-time
    // grant: it must keep charging a UseWeeds type.
    let mut one_time = SuperWeaponInstance::new(type_id, owner, 0);
    one_time.is_active = true;
    one_time.set_charged(&sw, 0);
    assert!(one_time.is_ready);
    tick_house_superweapon_instances(&mut sim, &rules, owner, crate::sim::frame_effects::FrameEffects::empty());
    assert!(instance(&sim, owner, type_id).is_ready);
    let amounts = sim.houses[&owner].economy().owned_weed_amounts();
    assert_eq!(amounts[0], NativeF32Bits::POSITIVE_ZERO, "slot 0 drains");
    assert_eq!(
        amounts[1],
        NativeF32Bits::from_bits(7.0_f32.to_bits()),
        "slot 1 keeps what the drain cannot reach"
    );
    assert_eq!(sim.houses[&owner].economy().owned_weed_total(), 7);
}

/// `UseWeeds` plus `UseChargeDrain`: the ready gate stays open for a draining
/// type and the charged branch runs the native +0x7C state machine.
#[test]
fn use_weeds_with_charge_drain_runs_the_drain_state_machine() {
    let rules = weed_rules(
        "UseWeeds.Amount=4
UseChargeDrain=yes
",
        "",
    );
    let (mut sim, owner, type_id) = scene(&rules);
    give(&mut sim, owner, 4, 0);
    tick_house_superweapon_instances(&mut sim, &rules, owner, crate::sim::frame_effects::FrameEffects::empty());
    let charged = instance(&sim, owner, type_id);
    assert_eq!(charged.charge_drain_state, 1, "the drain starts");
    assert!(charged.is_ready);

    // A launched draining Super (+0x7C = 2) that refills the store restarts
    // its recharge timer from the type's RechargeTime.
    {
        let launched = sim.super_weapons.instance_mut(&owner, &type_id).unwrap();
        launched.charge_drain_state = 2;
        launched.is_ready = true;
    }
    give(&mut sim, owner, 4, 0);
    tick_house_superweapon_instances(&mut sim, &rules, owner, crate::sim::frame_effects::FrameEffects::empty());
    let restarted = instance(&sim, owner, type_id);
    assert_eq!(restarted.charge_drain_state, 0, "the drain ends");
    assert_eq!(
        restarted.charge_duration, 9000,
        "RechargeTime=10 minutes restarts the clock"
    );
}

/// The constructor default for `UseWeeds.Amount=` is `[General]
/// WeedCapacity`; an explicit key overrides it.
#[test]
fn use_weeds_amount_defaults_to_weed_capacity() {
    let defaulted = weed_rules("", "[General]\nWeedCapacity=4\n");
    let (mut sim, owner, type_id) = scene(&defaulted);
    give(&mut sim, owner, 3, 0);
    tick_house_superweapon_instances(&mut sim, &defaulted, owner, crate::sim::frame_effects::FrameEffects::empty());
    assert!(!instance(&sim, owner, type_id).is_ready, "3 of 4 is short");

    let (mut sim, owner, type_id) = scene(&defaulted);
    give(&mut sim, owner, 4, 0);
    tick_house_superweapon_instances(&mut sim, &defaulted, owner, crate::sim::frame_effects::FrameEffects::empty());
    assert!(instance(&sim, owner, type_id).is_ready, "4 of 4 charges");

    let explicit = weed_rules("UseWeeds.Amount=10\n", "[General]\nWeedCapacity=4\n");
    let (mut sim, owner, type_id) = scene(&explicit);
    give(&mut sim, owner, 4, 0);
    tick_house_superweapon_instances(&mut sim, &explicit, owner, crate::sim::frame_effects::FrameEffects::empty());
    assert!(
        !instance(&sim, owner, type_id).is_ready,
        "the explicit amount wins over WeedCapacity"
    );
}
