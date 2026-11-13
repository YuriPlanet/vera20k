//! Ares SWTypeExt charge-drain keys, SW.AlwaysGranted and the Battery and
//! Protect types through the production rules reader, Grant, ClickFire,
//! SuperClass::AI and House power owners. Expected boundaries follow the
//! installed Ares bodies cited beside the code; no native execution corpus.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::sim::command::Command;

fn rules(extra: &str) -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [BuildingTypes]\n0=PLANT\n\
         [PLANT]\nStrength=1000\nTurret=no\nSight=0\nPower=-100\nPowered=yes\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nSight=0\n\
         [Warheads]\n0=Payload\n[Payload]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         {extra}"
    ));
    let art = IniFile::from_str("[PLANT]\nFoundation=1x1\n");
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

fn scene(rules: &RuleSet, name: &str) -> (Simulation, InternedId, InternedId) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 29);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.get("Americans").unwrap();
    for house in ["Americans", "Russians"] {
        let house = sim.interner.get(house).unwrap();
        construct_house_supers(&mut sim, rules, house, false);
    }
    let sw = sim.interner.get(name).unwrap();
    (sim, owner, sw)
}

fn grant(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, sw: InternedId) {
    assert!(grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        rules,
        owner,
        sw,
        false,
        false
    ));
}

fn at(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    tick_house_superweapon_instances(sim, rules, owner);
}

fn fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: InternedId,
    cell: (u16, u16),
) -> bool {
    let name = sim.interner.resolve(owner).to_owned();
    sim.apply_command_with_overlays(
        &name,
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    )
}

#[test]
fn battery_drains_by_its_ratio_lends_power_and_stops_when_drain_money_runs_out() {
    let rules = rules(
        "[SuperWeaponTypes]\n0=Cell\n\
         [Cell]\nType=Battery\nRechargeTime=1\nIsPowered=no\nAction=Nuke\n\
         Battery.Power=50\nBattery.KeepOnline=PLANT,UNKNOWN\nBattery.Overpower=PLANT\n\
         SW.ChargeToDrainRatio=0.5\nMoney.DrainAmount=-10\nMoney.DrainDelay=100\n\
         SW.Unstoppable=yes\n",
    );
    let cell = rules.super_weapon("Cell").unwrap();
    assert!(
        cell.use_charge_drain && cell.action.is_none(),
        "1006ECC0 forces both"
    );
    let (mut sim, owner, sw) = scene(&rules, "Cell");
    sim.session.set_binary_frame_for_test(100);
    grant(&mut sim, &rules, owner, sw);
    at(&mut sim, &rules, owner, 1000);
    assert_eq!(sim.super_weapons[&owner][&sw].charge_drain_state(), 1);

    let plant = sim
        .spawn_object("PLANT", "Americans", 8, 8, 0, &rules)
        .unwrap();
    sim.assess_power_for_test(&rules);
    assert!(sim.power_states[&owner].low_power());
    assert!(fire(&mut sim, &rules, owner, sw, (0, 0)));
    let instance = &sim.super_weapons[&owner][&sw];
    // 10035130: (900 - 0) * 0.5 through F2I, not ChargeToDrainRatio=3.0.
    assert_eq!(
        (instance.charge_drain_state(), instance.charge_duration()),
        (2, 450)
    );
    let power = &sim.power_states[&owner];
    assert!(power.has_battery() && power.keeps_online("PLANT") && power.overpowers("PLANT"));
    sim.assess_power_for_test(&rules);
    assert_eq!(
        (
            sim.power_states[&owner].total_output(),
            sim.power_states[&owner].total_drain()
        ),
        (50, 100),
        "508C7F starts the counters from AuxPower"
    );
    assert!(
        sim.building_operational_state(plant, &rules).unwrap(),
        "4555D5: a KeepOnline type needs no power"
    );
    let view = superweapon_views_for_owner(&sim, &rules, &owner)
        .pop()
        .unwrap();
    assert_eq!(view.ares_refusal, Some(AresCameoRefusal::Unstoppable));

    let money = |sim: &Simulation| sim.houses[&owner].economy().available_money();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .set_credits_for_test(15);
    at(&mut sim, &rules, owner, 1049);
    assert_eq!(money(&sim), 15, "449 frames left is no DrainDelay multiple");
    at(&mut sim, &rules, owner, 1050);
    assert_eq!(money(&sim), 5);
    sim.sound_events.clear_for_test();
    at(&mut sim, &rules, owner, 1150);
    assert_eq!(money(&sim), 5, "an unaffordable drain takes nothing");
    let instance = &sim.super_weapons[&owner][&sw];
    assert_eq!(
        (instance.charge_drain_state(), instance.charge_start_tick()),
        (0, 1150),
        "100358A0 ends the drain as its expiry would"
    );
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::AresSuperWeaponDrainRefused { owner: house, sw_type }
            if *house == owner && *sw_type == sw)));
    let power = &sim.power_states[&owner];
    assert!(!power.has_battery() && !power.keeps_online("PLANT"));
    sim.assess_power_for_test(&rules);
    assert_eq!(sim.power_states[&owner].total_output(), 0);
    assert!(!sim.building_operational_state(plant, &rules).unwrap());
}

#[test]
fn always_granted_needs_no_provider_but_keeps_availability_gates() {
    let rules = rules(
        "[SuperWeaponTypes]\n0=Strike\n1=Plain\n\
         [Strike]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.AlwaysGranted=yes\n\
         SW.AllowAI=no\nIsPowered=no\n\
         [Plain]\nType=GenericWarhead\nSW.Warhead=Payload\nIsPowered=no\n",
    );
    let (mut sim, owner, strike) = scene(&rules, "Strike");
    let plain = sim.interner.get("Plain").unwrap();
    let computer = sim.interner.get("Russians").unwrap();
    for house in [owner, computer] {
        grant_provided_supers(&mut sim, &rules, house);
        update_owned_supers(&mut sim, &rules, house);
    }
    assert!(sim.super_weapons[&owner][&strike].is_active());
    assert!(!sim.super_weapons[&owner][&plain].is_active());
    assert!(
        !sim.super_weapons[&computer][&strike].is_active(),
        "IsAvailable's SW.AllowAI still applies"
    );
}

#[test]
fn protect_curtains_its_rectangle_with_its_duration_and_affected_houses() {
    let rules = rules(
        "[SuperWeaponTypes]\n0=Shield\n\
         [Shield]\nType=Protect\nRechargeTime=1\nIsPowered=no\nProtect.Duration=200\n\
         SW.AffectsHouse=owner\nSW.Range=2,2\n",
    );
    let shield = rules.super_weapon("Shield").unwrap();
    assert_eq!(shield.action.as_deref(), Some("IronCurtain"));
    assert_eq!(shield.presentation.height, 5, "Initialize10074B60");
    let (mut sim, owner, sw) = scene(&rules, "Shield");
    let own = sim
        .spawn_object("TANK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let left = sim
        .spawn_object("TANK", "Americans", 9, 10, 0, &rules)
        .unwrap();
    let outside = sim
        .spawn_object("TANK", "Americans", 11, 10, 0, &rules)
        .unwrap();
    let enemy = sim
        .spawn_object("TANK", "Russians", 10, 9, 0, &rules)
        .unwrap();
    grant(&mut sim, &rules, owner, sw);
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .is_ready = true;
    sim.sound_events.clear_for_test();
    assert!(fire(&mut sim, &rules, owner, sw, (10, 10)));
    let curtained = |id: u64| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .invulnerability
            .as_ref()
            .map(|state| state.timer().remaining(sim.session.binary_frame() as i32))
    };
    // A 2x2 rectangle starts at center - 1: cells (9..=10, 9..=10).
    assert_eq!(curtained(own), Some(200));
    assert_eq!(curtained(left), Some(200));
    assert_eq!(curtained(outside), None);
    assert_eq!(curtained(enemy), None, "SW.AffectsHouse=owner");
    // The common Launch10032ED0 tail: SW.CreateRadarEvent's default type 13.
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::RadarEvent { radar }
            if radar.event_type == crate::sim::radar::RadarEventType::ImpactSilent)));
    assert_eq!(sim.super_weapons[&owner][&sw].ares_launch_count, 1);
}

#[test]
fn sonar_pulse_marks_affected_enemies_in_range_and_whole_map_pulses_need_no_target() {
    let rules = rules(
        "[SuperWeaponTypes]\n0=Sonar\n1=Wide\n\
         [Sonar]\nType=SonarPulse\nRechargeTime=1\nIsPowered=no\nSonarPulse.Delay=100\n\
         SW.AffectsTarget=all\nSW.RequiresTarget=land\n\
         [Wide]\nType=SonarPulse\nRechargeTime=1\nIsPowered=no\nSW.Range=-1\n",
    );
    assert_eq!(
        rules.super_weapon("Sonar").unwrap().action.as_deref(),
        Some("Attack")
    );
    assert_eq!(
        rules.super_weapon("Wide").unwrap().action,
        None,
        "10075710: a whole-map pulse has no action"
    );
    let (mut sim, owner, sw) = scene(&rules, "Sonar");
    let near = sim
        .spawn_object("TANK", "Russians", 16, 10, 0, &rules)
        .unwrap();
    let far = sim
        .spawn_object("TANK", "Russians", 21, 10, 0, &rules)
        .unwrap();
    let own = sim
        .spawn_object("TANK", "Americans", 11, 10, 0, &rules)
        .unwrap();
    sim.session.set_binary_frame_for_test(500);
    grant(&mut sim, &rules, owner, sw);
    sim.super_weapons
        .instance_mut(&owner, &sw)
        .unwrap()
        .is_ready = true;
    assert!(fire(&mut sim, &rules, owner, sw, (10, 10)));
    let sonar = |sim: &Simulation, id: u64| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .ares
            .sonar_timer
            .remaining(500)
    };
    assert_eq!(sonar(&sim, near), 100, "radius 10 from GetRange10075680");
    assert_eq!(sonar(&sim, far), 0, "11 cells away");
    assert_eq!(sonar(&sim, own), 0, "Initialize100756D0: enemies only");
    // The default water-only target mask leaves land units alone, even
    // across the whole map.
    let wide = sim.interner.get("Wide").unwrap();
    grant(&mut sim, &rules, owner, wide);
    sim.super_weapons
        .instance_mut(&owner, &wide)
        .unwrap()
        .is_ready = true;
    assert!(fire(&mut sim, &rules, owner, wide, (0, 0)));
    assert_eq!(sonar(&sim, far), 0);
}

/// `SW.TimerVisibility` (10033370) through IsTimerVisible (10032E80, hook
/// 0x6D49D1): the owner sees every timer, another player only those whose
/// mask has the enemies bit, and an observer those with the allies bit.
#[test]
fn ares_timer_visibility_hides_an_owner_only_timer_from_other_players() {
    let rules = rules(
        "[SuperWeaponTypes]\n0=Mine\n1=Open\n2=Team\n\
         [Mine]\nRechargeTime=1\nSW.TimerVisibility=owner\n\
         [Open]\nRechargeTime=1\n\
         [Team]\nRechargeTime=1\nSW.TimerVisibility=team\n",
    );
    let (mut sim, owner, mine) = scene(&rules, "Mine");
    let open = sim.interner.get("Open").unwrap();
    let team = sim.interner.get("Team").unwrap();
    let enemy = sim.interner.get("Russians").unwrap();
    let seen = |sim: &Simulation| {
        [mine, open, team].map(|sw| {
            ares_timer_visible(
                sim,
                &rules,
                SuperTimerView::for_test(owner, sw, 0, false, 0, 0),
            )
        })
    };
    sim.session.set_current_house_for_test(Some(owner));
    assert_eq!(seen(&sim), [true, true, true]);
    sim.session.set_current_house_for_test(Some(enemy));
    assert_eq!(seen(&sim), [false, true, false]);
    sim.session.install_multiplayer_house_identity(
        crate::skirmish_modes::MpModeClass::Battle,
        vec![enemy],
    );
    assert_eq!(seen(&sim), [false, true, true]);
}
