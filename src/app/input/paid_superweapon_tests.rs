use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::house_state::HouseState;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::world::Simulation;

fn fixture(paid: bool) -> (RuleSet, Simulation, InternedId, InternedId, InternedId) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n[BuildingTypes]\n0=PROVIDER\n\
         [PROVIDER]\nStrength=200\nSuperWeapon=Sphere\n\
         [SuperWeaponTypes]\n0=A\n1=B\n2=C\n3=Sphere\n4=Warp\n\
         [A]\nType=PsychicReveal\n[B]\nType=PsychicReveal\n[C]\nType=PsychicReveal\n\
         [Sphere]\nType=ChronoSphere\nAction=ChronoSphere\nPreClick=yes\nIsPowered=no\nMoney.Amount={}\n\
         [Warp]\nType=ChronoWarp\nPostClick=yes\nPreDependent=ChronoSphere\nIsPowered=no\n",
        if paid { -500 } else { 0 }
    ));
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[PROVIDER]\nFoundation=1x1\n"),
    )
    .unwrap();
    let mut sim = Simulation::with_seed(42);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 1000, 10));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .super_weapons = true;
    sim.spawn_object("PROVIDER", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let source = sim.interner().get("Sphere").unwrap();
    let destination = sim.interner().get("Warp").unwrap();
    for id in [source, destination] {
        let mut instance = SuperWeaponInstance::new(id, owner, 0);
        instance.set_is_active_for_test(id == source);
        instance.set_is_ready_for_test(id == source);
        sim.super_weapons_mut_for_test()
            .entry(owner)
            .or_default()
            .insert(id, instance);
    }
    (rules, sim, owner, source, destination)
}

fn drain(sim: &mut Simulation, rules: &RuleSet) {
    for command in sim.take_due_commands() {
        assert!(sim.apply_command("Americans", &command.payload, Some(rules),));
    }
}

#[cfg(feature = "ares")]
#[test]
fn paid_superweapon_input_cancels_before_source_exec_and_does_not_duplicate_cancel() {
    let (rules, mut sim, owner, source, _) = fixture(true);
    let command = super_weapon_click_command(&sim, &rules, owner, source, (10, 10)).unwrap();
    let Command::BeginPaidSuperWeapon {
        selection: first, ..
    } = command
    else {
        panic!("ticketed source");
    };
    schedule_command_in_sim(&mut sim, "Americans", command);
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, Some("Sphere")).is_none());
    let cancel = paid_super_weapon_cursor_cancellation(&sim, owner, None).unwrap();
    schedule_command_in_sim(&mut sim, "Americans", cancel);
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, None).is_none());
    let next = super_weapon_click_command(&sim, &rules, owner, source, (11, 11)).unwrap();
    let Command::BeginPaidSuperWeapon {
        selection: second, ..
    } = next
    else {
        panic!("ticketed source");
    };
    assert!(second.sequence > first.sequence);
    schedule_command_in_sim(&mut sim, "Americans", next);
    drain(&mut sim, &rules);
    assert_eq!(sim.paid_super_weapon_selection(owner), Some(second));
    assert_eq!(sim.houses()[&owner].economy().credits(), 1000);
}

#[cfg(feature = "ares")]
#[test]
fn paid_superweapon_input_final_and_mode_switch_use_the_saved_ticket() {
    let (rules, mut sim, owner, source, destination) = fixture(true);
    let source_click = super_weapon_click_command(&sim, &rules, owner, source, (10, 10)).unwrap();
    schedule_command_in_sim(&mut sim, "Americans", source_click);
    drain(&mut sim, &rules);
    let ticket = sim.paid_super_weapon_selection(owner).unwrap();
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, Some("Warp")).is_none());
    for changed_mode in [None, Some("Sphere"), Some("OtherPower"), Some("Building")] {
        assert_eq!(
            paid_super_weapon_cursor_cancellation(&sim, owner, changed_mode),
            Some(Command::CancelPaidSuperWeapon { selection: ticket })
        );
    }
    let final_click =
        super_weapon_click_command(&sim, &rules, owner, destination, (15, 15)).unwrap();
    assert!(
        matches!(final_click, Command::CompletePaidSuperWeapon { selection, .. } if selection == ticket)
    );
    schedule_command_in_sim(&mut sim, "Americans", final_click);
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, None).is_none());
    drain(&mut sim, &rules);
    assert_eq!(sim.houses()[&owner].economy().credits(), 500);
    assert!(sim.paid_super_weapon_selection(owner).is_none());
}

#[test]
fn paid_superweapon_input_free_pair_keeps_native_command_shape() {
    let (rules, sim, owner, source, destination) = fixture(false);
    for sw_type_id in [source, destination] {
        assert_eq!(
            super_weapon_click_command(&sim, &rules, owner, sw_type_id, (10, 10)),
            Some(Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx: 10,
                target_ry: 10
            })
        );
    }
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, None).is_none());
}

#[cfg(not(feature = "ares"))]
#[test]
fn disabled_ares_money_keeps_two_stage_clicks_on_stock_commands() {
    let (rules, mut sim, owner, source, destination) = fixture(true);
    let source_click = super_weapon_click_command(&sim, &rules, owner, source, (10, 10)).unwrap();
    assert_eq!(
        source_click,
        Command::LaunchSuperWeapon {
            sw_type_id: source,
            target_rx: 10,
            target_ry: 10,
        }
    );
    schedule_command_in_sim(&mut sim, "Americans", source_click);
    drain(&mut sim, &rules);
    assert!(sim.paid_super_weapon_selection(owner).is_none());
    assert_eq!(sim.houses()[&owner].economy().credits(), 1000);
    assert_eq!(
        super_weapon_click_command(&sim, &rules, owner, destination, (15, 15)),
        Some(Command::LaunchSuperWeapon {
            sw_type_id: destination,
            target_rx: 15,
            target_ry: 15,
        })
    );
    assert!(paid_super_weapon_cursor_cancellation(&sim, owner, None).is_none());
}
