//! Sidebar view fire admission (`SuperWeaponView::can_fire`) against the
//! real grant/charge/hold owners. Native identity: `SuperClass::CanFire @
//! 0x006CC360` refuses a suspended Super (`+0x70`) before its charge
//! (`+0x6F`) is read, and `SelectClass::Action @ 0x006AAD00` reads exactly
//! that at `0x006AAEEA` before arming the sidebar's target mode.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;

/// NAMISL provides NukeSpecial (`IsPowered=yes`): the issue #1313 fixture.
fn nuke_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=NAMISL\n\
         [NAMISL]\nStrength=1000\nSuperWeapon=NukeSpecial\n\
         [SuperWeaponTypes]\n0=NukeSpecial\n\
         [NukeSpecial]\nType=MultiMissile\nRechargeTime=10\nIsPowered=yes\nShowTimer=yes\n",
    ))
    .unwrap()
}

fn nuke_scene() -> (Simulation, RuleSet, InternedId, InternedId) {
    let rules = nuke_rules();
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let type_id = sim.interner.intern("NukeSpecial");
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(type_id, SuperWeaponInstance::new(type_id, owner, 0));
    sim.spawn_object("NAMISL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.power_states
        .map_mut_for_test()
        .entry(owner)
        .or_default();
    (sim, rules, owner, type_id)
}

fn nuke_view(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> SuperWeaponView {
    superweapon_views_for_owner(sim, rules, &owner)
        .into_iter()
        .find(|view| view.display_name == "NukeSpecial")
        .expect("granted NukeSpecial view")
}

#[test]
fn charged_super_held_by_low_power_stays_charged_but_cannot_fire() {
    let (mut sim, rules, owner, type_id) = nuke_scene();

    // House50B1D0 grant while powered starts the recharge.
    grant_provided_supers(&mut sim, &rules, owner);
    let view = nuke_view(&sim, &rules, owner);
    assert!(!view.is_ready, "the grant starts the recharge");
    assert!(view.is_online);
    assert!(!view.can_fire, "a charging Super cannot fire");

    // The House Super AI visit completes the charge (RechargeTime=10 is
    // minutes -> 9000 native frames).
    sim.session.set_binary_frame_for_test(9000);
    tick_house_superweapon_instances(&mut sim, &rules, owner);
    let view = nuke_view(&sim, &rules, owner);
    assert!(view.is_ready);
    assert!(view.is_online);
    assert!(view.can_fire, "charged and online admits firing");

    // The power edge holds the charged Super: the view keeps the charge
    // (and the timer/flash readers), drops online, and drops the fire
    // admission — 0x006CC360 reads the hold before the charge.
    sim.power_states
        .get_mut(&owner)
        .unwrap()
        .set_is_low_power_for_test(true);
    update_owned_supers(&mut sim, &rules, owner);
    assert!(sim.super_weapons[&owner][&type_id].is_suspended);
    let view = nuke_view(&sim, &rules, owner);
    assert!(view.is_ready, "the hold keeps the charge");
    assert!(!view.is_online);
    assert!(
        !view.can_fire,
        "a suspended Super is refused before its charge"
    );

    // Restoring power releases the hold; the admission returns.
    sim.power_states
        .get_mut(&owner)
        .unwrap()
        .set_is_low_power_for_test(false);
    update_owned_supers(&mut sim, &rules, owner);
    assert!(!sim.super_weapons[&owner][&type_id].is_suspended);
    assert!(nuke_view(&sim, &rules, owner).can_fire);
}

mod layered_enum_history {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    use crate::rules::superweapon_type::SuperWeaponKind;
    use crate::sim::command::Command;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::superweapon::{
        construct_house_supers, pre_dependent, superweapon_views_for_owner,
    };
    use crate::sim::world::{FrameEffects, Simulation};

    const OWNER: &str = "Americans";
    const REVEAL: &str = "RetainedReveal";

    fn processed_rules(layers: &RulesLayerStack) -> RuleSet {
        let art = IniFile::from_str("[PROVIDER]\nFoundation=1x1\n");
        let processed = layers.process_with_fixed_art(&art).unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    }

    fn world(rules: &RuleSet) -> Simulation {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 23);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().super_weapons = true;
        sim.fog.set_width_for_test(32);
        sim.fog.set_height_for_test(32);
        for name in [OWNER, "Russians"] {
            let owner = sim.interner.get(name).unwrap();
            let house = sim.houses.get_mut(&owner).unwrap();
            house.set_is_human_for_test(true);
            house.set_player_control_for_test(true);
            construct_house_supers(&mut sim, rules, owner, false);
        }
        sim.session
            .set_current_house_for_test(sim.interner.get(OWNER));
        sim.spawn_object("PROVIDER", OWNER, 10, 10, 0, rules)
            .unwrap();
        sim.spawn_object("PROVIDER", "Russians", 27, 27, 0, rules)
            .unwrap();
        sim
    }

    #[test]
    fn unknown_later_type_keeps_the_provider_charge_and_reveal_command() {
        let mut layers = RulesLayerStack::new(crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=PROVIDER\n\
             [PROVIDER]\nStrength=1000\nTurret=no\nSight=0\nSuperWeapon=RetainedReveal\n\
             [SuperWeaponTypes]\n0=RetainedReveal\n1=UnknownOnly\n\
             [RetainedReveal]\nType=MultiMissile\nRechargeTime=0.1\nIsPowered=no\n\
             [UnknownOnly]\nType=NoSuchKind\n\
             [CombatDamage]\nPsychicRevealRadius=2\n",
        ));
        // ReadINI6CEC2D copies39 characters before trimming: the X is
        // outside the buffer, so this pass accepts a different known kind.
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str(&format!(
                "[RetainedReveal]\nType=pSyChIcReVeAl{}X\n",
                " ".repeat(26)
            )),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[RetainedReveal]\nType=NoSuchKind\n[UnknownOnly]\nType=StillUnknown\n",
            ),
        );
        let rules = processed_rules(&layers);
        assert_eq!(
            rules.super_weapon(REVEAL).unwrap().kind,
            SuperWeaponKind::PsychicReveal
        );
        // Preserve the existing typed-projection boundary for a type that
        // never had a recognized value. This does not model native Type=-1.
        assert_eq!(rules.super_weapon_order[1], "UnknownOnly");
        assert!(rules.super_weapon("UnknownOnly").is_none());

        let mut sim = world(&rules);
        let owner = sim.interner.get(OWNER).unwrap();
        let sw_type_id = sim.interner.get(REVEAL).unwrap();
        // Ordinary Building Unlimbo requests the real House grant suffix.
        sim.recheck_house_tech_tree(owner, &rules);
        assert!(sim.super_weapons[&owner][&sw_type_id].is_active());
        assert!(!sim.super_weapons[&owner][&sw_type_id].is_ready());
        let duration = rules.super_weapon(REVEAL).unwrap().recharge_time_frames;
        assert!(duration > 0 && duration < 100);
        for _ in 0..=duration {
            sim.advance_tick(&[], Some(&rules), None, None, 33);
        }
        assert!(
            superweapon_views_for_owner(&sim, &rules, &owner)
                .iter()
                .any(|view| view.type_id == sw_type_id && view.can_fire)
        );
        assert!(!sim.fog.is_cell_revealed(owner, 20, 20));
        assert!(sim.apply_command_with_overlays(
            OWNER,
            &Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx: 20,
                target_ry: 20,
            },
            &rules,
            None,
            FrameEffects::default(),
        ));
        assert!(sim.fog.is_cell_revealed(owner, 20, 20));
        assert!(!sim.super_weapons[&owner][&sw_type_id].is_ready());
    }

    #[test]
    fn unknown_later_predependent_keeps_the_last_accepted_registry_pair() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [SuperWeaponTypes]\n0=First\n1=Second\n2=Third\n3=Source\n4=Destination\n\
             [First]\nType=PsychicReveal\n[Second]\nType=PsychicReveal\n[Third]\nType=PsychicReveal\n\
             [Source]\nType=ChronoSphere\nPreClick=yes\nRechargeTime=0.1\n\
             [Destination]\nType=ChronoWarp\nPostClick=yes\nPreDependent=MultiMissile\n",
        ));
        // The same40-byte reader at6CEC98 cuts the X, then trims the
        // remaining spaces. This accepted replacement changes0 to3.
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str(&format!(
                "[Destination]\nPreDependent=cHrOnOsPhErE{}X\n",
                " ".repeat(27)
            )),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Destination]\nPreDependent=NoSuchKind\n"),
        );
        let rules = processed_rules(&layers);
        let destination = rules.super_weapon("Destination").unwrap();
        assert_eq!(destination.pre_dependent, 3);
        // Fire_SW4FAE6C indexes the registry by this field; it does not
        // search for a type with a matching dispatch kind. Lookup only,
        // without claiming a complete two-click warp execution.
        let (name, source) = pre_dependent(&rules, destination).unwrap();
        assert_eq!(name, rules.super_weapon_order[3]);
        assert_eq!(name, "Source");
        assert_eq!(source.kind, SuperWeaponKind::ChronoSphere);
        assert!(source.pre_click && destination.post_click);
    }
}
