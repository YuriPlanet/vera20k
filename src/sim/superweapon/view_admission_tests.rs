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
        // A never-recognized type retains the native constructor's -1.
        assert_eq!(rules.super_weapon_order[1], "UnknownOnly");
        assert_eq!(
            rules.super_weapon("UnknownOnly").unwrap().kind,
            SuperWeaponKind::NoEffect
        );

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

/// Native ctor/ReadINI6CE5B0/6CEA20 retain Type=-1; Grant6CB560 and
/// ClickFire6CB920 still own charging/consumption around default Launch6CC3AC.
#[test]
fn initial_unknown_type_keeps_provider_trigger_and_saved_click_lifecycle() {
    use crate::map::map_file::MapFile;
    use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
    use crate::sim::command::Command;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::FrameEffects;
    use crate::util::save_codec;

    fn world(rules: &RuleSet) -> Simulation {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 31);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().super_weapons = true;
        sim.session.set_binary_frame_for_test(4100);
        let local = sim.interner.get("Americans").unwrap();
        sim.houses
            .get_mut(&local)
            .unwrap()
            .set_player_control_for_test(true);
        sim.session.set_current_house_for_test(Some(local));
        for name in ["Americans", "Russians"] {
            let owner = sim.interner.get(name).unwrap();
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_country_for_test(Some(owner));
            construct_house_supers(&mut sim, rules, owner, false);
        }
        sim
    }
    fn fire(sim: &mut Simulation, rules: &RuleSet, id: InternedId) {
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &Command::LaunchSuperWeapon {
                sw_type_id: id,
                target_rx: 20,
                target_ry: 20,
            },
            rules,
            None,
            FrameEffects::default()
        ));
    }

    let art = IniFile::from_str("[POWER]\nFoundation=1x1\n[PROVIDER]\nFoundation=1x1\n");
    let processed = RulesLayerStack::new(crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n\
         [BuildingTypes]\n0=POWER\n1=PROVIDER\n\
         [POWER]\nStrength=1000\nPower=100\nTurret=no\nSight=0\n\
         [PROVIDER]\nStrength=1000\nPower=-10\nTurret=no\nSight=0\nSuperWeapon=Blank\n\
         [SuperWeaponTypes]\n0=Blank\n\
         [Blank]\nType=NoSuchKind\nRechargeTime=1\nIsPowered=yes\nShowTimer=yes\nAction=none\nSidebarImage=EMPTYICO\n",
    )).process_with_fixed_art(&art).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        rules
            .super_weapon("Blank")
            .expect("allocated native Type=-1 remains a runtime type")
            .kind
            .native_index(),
        -1
    );
    assert_eq!(rules.super_weapon_index("Blank"), Some(0));

    let mut sim = world(&rules);
    let local = sim.interner.get("Americans").unwrap();
    let enemy = sim.interner.get("Russians").unwrap();
    let id = sim.interner.get("Blank").unwrap();
    for (name, at) in [("Americans", 8), ("Russians", 24)] {
        sim.spawn_object("POWER", name, at, 8, 0, &rules).unwrap();
        sim.spawn_object("PROVIDER", name, at, 12, 0, &rules)
            .unwrap();
    }
    sim.assess_power_for_test(&rules);
    for owner in [local, enemy] {
        assert!(!sim.power_states.get(&owner).unwrap().low_power());
        sim.recheck_house_tech_tree(owner, &rules);
        let instance = &sim.super_weapons[&owner][&id];
        assert!(instance.is_active() && !instance.is_ready() && !instance.is_suspended());
        assert_eq!(
            (instance.charge_start_tick(), instance.charge_duration()),
            (4100, 900)
        );
    }
    assert_eq!(super_timer_views(&sim, &rules).len(), 2);
    let view = superweapon_views_for_owner(&sim, &rules, &local)
        .pop()
        .unwrap();
    assert!(!view.can_fire && view.is_online);
    assert_eq!(
        rules
            .super_weapon(sim.interner.resolve(view.type_id))
            .unwrap()
            .sidebar_image
            .as_deref(),
        Some("EMPTYICO")
    );

    // Same-build rules-resource wire plus the saved dynamic world. No native
    // SAV import or pre-fix rules-hash compatibility is implied by this fixture.
    sim.session.set_binary_frame_for_test(4550);
    let expected = save_codec::serialize(&sim.super_weapons).unwrap();
    let rng = sim.rng_state();
    let rules_hash = rules.simulation_config_hash();
    let resources = save_codec::serialize(&processed).unwrap();
    let retained: ProcessedRulesLayers = save_codec::deserialize(&resources).unwrap();
    let restored_rules = RuleSet::from_processed_rules(&retained).unwrap();
    assert_eq!(restored_rules.simulation_config_hash(), rules_hash);
    assert_eq!(
        restored_rules
            .super_weapon("Blank")
            .unwrap()
            .kind
            .native_index(),
        -1
    );
    let bytes = GameSnapshot::save_validated(&sim, 0, rules_hash, "unassigned super", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    crate::sim::arena_fixture::flat_ground(&mut loaded, &restored_rules);
    loaded.resolve_type_handles(&restored_rules);
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(
        save_codec::serialize(&loaded.super_weapons).unwrap(),
        expected
    );
    let restored_rng = loaded.rng_state();
    assert_eq!(restored_rng.main, rng.main);
    assert_eq!(restored_rng.mapgen, rng.mapgen);
    assert_eq!(
        restored_rng.scenario,
        crate::sim::rng::SimRng::new(0).logical_state()
    );
    assert_eq!(super_timer_views(&loaded, &restored_rules).len(), 2);
    assert_eq!(
        loaded.super_weapons[&local][&id].charge_remaining(4550),
        450
    );
    sim = loaded;
    let rules = restored_rules;
    sim.session.set_binary_frame_for_test(5000);
    for owner in [local, enemy] {
        tick_house_superweapon_instances(&mut sim, &rules, owner);
    }
    assert!(superweapon_views_for_owner(&sim, &rules, &local)[0].can_fire);
    sim.sound_events.clear_for_test();
    let computer = sim.houses.get_mut(&enemy).unwrap();
    computer.set_enemy_house_for_test(Some(local));
    computer.set_preferred_super_weapon_target((20, 20));
    let rng = sim.rng_state();
    ai_fire::AI_FIRE_LOG.set(Some(Vec::new()));
    ai_fire::try_fire(&mut sim, &rules, enemy, None, FrameEffects::default());
    assert_eq!(
        ai_fire::AI_FIRE_LOG.take().unwrap(),
        [ai_fire::AiFireEvent::TryFire]
    );
    assert_eq!(sim.rng_state(), rng);
    assert!(sim.super_weapons[&enemy][&id].is_ready());
    let object_count = sim.substrate.entities.len();
    fire(&mut sim, &rules, id);
    let instance = &sim.super_weapons[&local][&id];
    assert!(instance.is_active() && !instance.is_ready());
    assert_eq!(
        (instance.charge_start_tick(), instance.charge_duration()),
        (5000, 900)
    );
    assert_eq!(instance.ready_tick(), 5000);
    assert_eq!(super_timer_views(&sim, &rules).len(), 2);
    assert!(
        sim.sound_events.is_empty(),
        "default Launch emits no case effect or selection cleanup"
    );
    assert_eq!(sim.substrate.entities.len(), object_count);
    assert_eq!(sim.rng_state(), rng);
    sim.session.set_binary_frame_for_test(5900);
    tick_house_superweapon_instances(&mut sim, &rules, local);
    assert!(sim.super_weapons[&local][&id].is_ready());

    // Original map action33 grants a fresh retained slot once; no provider
    // or direct ready/active override participates in this second history.
    let mut sim = world(&rules);
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Triggers]\nGRANT=Americans,<none>,Grant,0,1,1,1,0\n[Events]\nGRANT=1,8,0,0\n[Actions]\nGRANT=1,33,11,0,0,0,0,0,A\n[Tags]\nGRANTTAG=0,Grant,GRANT\n").unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(&rules);
    sim.advance_triggers(&rules, None, FrameEffects::default());
    let instance = &sim.super_weapons[&local][&id];
    assert!(instance.is_active() && instance.is_ready() && instance.one_time);
    assert_eq!(super_timer_views(&sim, &rules).len(), 1);
    let rng = sim.rng_state();
    sim.sound_events.clear_for_test();
    fire(&mut sim, &rules, id);
    let instance = &sim.super_weapons[&local][&id];
    assert!(!instance.is_active() && !instance.is_ready() && !instance.one_time);
    assert_eq!(
        (instance.charge_start_tick(), instance.charge_duration()),
        (4100, 0)
    );
    assert_eq!(instance.ready_tick(), 4100);
    assert!(super_timer_views(&sim, &rules).is_empty());
    assert!(superweapon_views_for_owner(&sim, &rules, &local).is_empty());
    assert!(sim.sound_events.is_empty());
    assert_eq!(sim.rng_state(), rng);
}
