//! Ordered rules and the production Grant/Launch owners, without retail assets
//! or a second cooldown implementation. Expected boundaries follow Ares Grant
//! 100354E0 and HouseExt launch history100226D0/10022710.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::sim::snapshot::GameSnapshot;

fn scene(flags: &str) -> (RuleSet, Simulation, InternedId, InternedId) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n[SuperWeaponTypes]\n0=Strike\n\
         [Strike]\nType=GenericWarhead\nIsPowered=no\nRechargeTime=1\n\
         SW.Warhead=Payload\n{flags}\n[Warheads]\n0=Payload\n\
         [Payload]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let mut layers = RulesLayerStack::new(ini);
    // Missing initial-ready and invalid virtual-charge retain the earlier pass.
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Strike]\nSW.VirtualCharge=?invalid\n"),
    );
    let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Americans").unwrap();
    construct_house_supers(&mut sim, &rules, owner, false);
    let sw = sim.interner.get("Strike").unwrap();
    (rules, sim, owner, sw)
}

fn grant(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: InternedId,
    frame: u32,
    once: bool,
) {
    sim.session.set_binary_frame_for_test(frame);
    assert!(grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        rules,
        owner,
        sw,
        once,
        false
    ));
}

#[test]
fn ares_initial_ready_is_first_shot_only_and_virtual_charge_survives_restore_and_regrant() {
    let (rules, mut sim, owner, sw) = scene("SW.InitialReady=yes\nSW.VirtualCharge=yes");
    grant(&mut sim, &rules, owner, sw, 100, false);
    assert!(sim.super_weapons[&owner][&sw].is_ready());
    assert_eq!(sim.super_weapons[&owner][&sw].charge_duration(), 0);
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        sw,
        (20, 20),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.super_weapons[&owner][&sw].ares_launch_count, 1);
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    let bytes = GameSnapshot::save(&sim, 0, rules.simulation_config_hash(), "charge", 0);
    sim = GameSnapshot::load(&bytes).unwrap().sim;
    sim.restore_after_snapshot_load().unwrap();
    grant(&mut sim, &rules, owner, sw, 500, false);
    let instance = &sim.super_weapons[&owner][&sw];
    assert_eq!(
        (instance.charge_start_tick(), instance.charge_duration()),
        (100, 900)
    );
    assert!(
        !instance.is_ready(),
        "initial-ready does not recharge a fired power"
    );
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    grant(&mut sim, &rules, owner, sw, 1000, false);
    assert!(
        sim.super_weapons[&owner][&sw].is_ready(),
        "ready at the original deadline"
    );
}

#[test]
fn ares_virtual_charge_keeps_first_recharge_origin_before_any_launch() {
    let (rules, mut sim, owner, sw) = scene("SW.VirtualCharge=yes");
    grant(&mut sim, &rules, owner, sw, 100, false);
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    grant(&mut sim, &rules, owner, sw, 500, false);
    let instance = &sim.super_weapons[&owner][&sw];
    assert_eq!(instance.ares_launch_count, 0);
    assert_eq!(
        (instance.charge_start_tick(), instance.charge_duration()),
        (100, 900)
    );
    assert!(!instance.is_ready());
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    grant(&mut sim, &rules, owner, sw, 1000, false);
    assert!(sim.super_weapons[&owner][&sw].is_ready());
}

#[test]
fn ares_absent_charge_flags_restart_normally_and_keep_one_time_grants_ready() {
    let (rules, mut sim, owner, sw) = scene("");
    grant(&mut sim, &rules, owner, sw, 100, false);
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    grant(&mut sim, &rules, owner, sw, 500, false);
    assert_eq!(sim.super_weapons[&owner][&sw].charge_start_tick(), 500);
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .deactivate();
    grant(&mut sim, &rules, owner, sw, 600, true);
    assert!(sim.super_weapons[&owner][&sw].is_ready());
    assert_eq!(sim.super_weapons[&owner][&sw].charge_duration(), 0);
}
