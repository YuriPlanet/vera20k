//! Small synthetic scenes through ordered rules, Fire_SW and normal Put.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::superweapon_type::SuperWeaponKind;
#[cfg(feature = "ares")]
use crate::sim::combat::TargetKind;

fn scene() -> (RuleSet, Simulation, InternedId, InternedId) {
    scene_with_late_rules("[IGNORED]\nHunterSeeker.Ignore=?invalid\n")
}

fn scene_with_late_rules(extra: &str) -> (RuleSet, Simulation, InternedId, InternedId) {
    scene_with(extra, "[LAUNCHER]\nFoundation=1x1\n")
}

fn scene_with(extra: &str, art: &str) -> (RuleSet, Simulation, InternedId, InternedId) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(concat!(
        "[Countries]\n0=Americans\n1=Russians\n[Sides]\nAllied=Americans\nSoviet=Russians\n",
        "[Allied]\nHunterSeeker=HUNTER\n[SpecialWeapons]\nHSBuilding=LAUNCHER\n",
        "[VehicleTypes]\n0=HUNTER\n1=FLYER\n2=IGNORED\n3=TARGET\n4=HARV\n",
        "[BuildingTypes]\n0=LAUNCHER\n[LAUNCHER]\nStrength=500\n",
        "[HUNTER]\nStrength=1\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n",
        "Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        "[FLYER]\nStrength=100\nSpeed=5\nSpeedType=Winged\nMovementZone=Fly\n",
        "Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n",
        "[IGNORED]\nStrength=100\nHunterSeeker.Ignore=yes\n",
        "[TARGET]\nStrength=100\n[HARV]\nStrength=100\nHarvester=yes\n",
        "[SuperWeaponTypes]\n0=Seek\n[Seek]\nType=HunterSeeker\nIsPowered=no\n",
        "HunterSeeker.Type=HUNTER\nAction=Custom\nMoney.Amount=-50\n"
    ));
    let mut layers = RulesLayerStack::new(ini);
    // Unknown scalar pointers retain the earlier value; an explicit none clears
    // it, selecting the side fallback. Missing SW buildings use HSBuilding.
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Seek]\nHunterSeeker.Type=<none>\nSW.MaxCount=1\n"),
    );
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(extra));
    let processed = layers
        .process_with_fixed_art(&IniFile::from_str(art))
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Americans").unwrap();
    let sw_type = sim.interner.get("Seek").unwrap();
    let mut instance = SuperWeaponInstance::new(sw_type, owner, 0);
    instance.is_active = true;
    instance.is_ready = true;
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    (rules, sim, owner, sw_type)
}

#[cfg(feature = "ares")]
#[test]
fn hunter_seeker_launch_uses_side_and_global_fallbacks_and_normal_lifecycle() {
    let (rules, mut sim, owner, sw_type) =
        scene_with_late_rules("[Seek]\nSW.InitialReady=yes\nSW.VirtualCharge=yes\n");
    sim.super_weapons
        .instance_mut(&owner, &sw_type)
        .unwrap()
        .deactivate();
    assert!(grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        &rules,
        owner,
        sw_type,
        false,
        false,
    ));
    assert!(sim.super_weapons[&owner][&sw_type].is_ready());
    let sw = rules.super_weapon("Seek").unwrap();
    assert_eq!(sw.kind, SuperWeaponKind::HunterSeeker);
    assert!(sw.action.is_none());
    assert!(sw.hunter_seeker.as_ref().unwrap().unit_type.is_none());
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 100);
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        sw_type,
        (0, 0),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(
        sim.houses[&owner].economy().available_money(),
        100,
        "failed effect does not transact its fee"
    );
    assert!(!sim.super_weapons[&owner][&sw_type].is_ready());
    super::chronosphere_tests::charge_super(&mut sim, owner, "Seek");
    sim.spawn_object("LAUNCHER", "Americans", 8, 8, 0, &rules)
        .unwrap();
    sim.spawn_object("LAUNCHER", "Americans", 20, 20, 0, &rules)
        .unwrap();
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        sw_type,
        (0, 0),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.houses[&owner].economy().available_money(), 50);
    let hunters: Vec<_> = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            sim.interner
                .object_type(e.type_ref(), &rules)
                .is_some_and(|ty| ty.id == "HUNTER")
        })
        .collect();
    assert_eq!(hunters.len(), 1);
    let hunter = hunters[0];
    assert_eq!(hunter.health.current(), 1);
    assert!(!hunter.lifecycle.in_limbo());
    assert!(hunter.position.rx().abs_diff(8) <= 1 && hunter.position.ry().abs_diff(8) <= 1);
    assert_eq!(
        hunter.mission.current(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Attack)
    );
    assert!(
        hunter.attack_target.target().is_none(),
        "Drive Acquire is a no-op"
    );
    assert_eq!(
        hunter.hunter_seeker_origin(),
        Some(crate::sim::hunter_seeker::HunterSeekerOrigin { owner, sw_type })
    );
    let id = hunter.stable_id();
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "hunter-seeker",
        0,
    );
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(
        restored.entities().get(id).unwrap().hunter_seeker_origin(),
        Some(crate::sim::hunter_seeker::HunterSeekerOrigin { owner, sw_type })
    );
}

/// The callback seeds FindNearbyLocation from GetCoords (vt+0x48 at
/// 100721ED), a building's foundation centre, and queues Area Guard for a
/// `Harvester=yes` unit (100722F5..1007230C).
#[cfg(feature = "ares")]
#[test]
fn hunter_seeker_seeds_from_the_foundation_centre_and_guards_with_harvesters() {
    let (rules, mut sim, owner, sw_type) = scene_with(
        "[Seek]\nHunterSeeker.Type=HARV\n",
        "[LAUNCHER]\nFoundation=4x4\n",
    );
    crate::sim::credit_income::add_credits(&mut sim.houses, owner, 100);
    sim.spawn_object("LAUNCHER", "Americans", 8, 8, 0, &rules)
        .unwrap();
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        sw_type,
        (0, 0),
        None,
        FrameEffects::default(),
        false
    ));
    let harvester = sim
        .substrate
        .entities
        .values()
        .find(|e| {
            sim.interner
                .object_type(e.type_ref(), &rules)
                .is_some_and(|ty| ty.id == "HARV")
        })
        .unwrap();
    let (x, y) = (harvester.position.rx(), harvester.position.ry());
    assert!(
        x == 12 || y == 12,
        "first ring around the centre, not the north-west cell: ({x}, {y})"
    );
    assert_eq!(
        harvester.mission.current(),
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::AreaGuard)
    );
}

#[cfg(feature = "ares")]
#[test]
fn hunter_seeker_acquisition_filters_ignored_types_and_does_not_redraw_held_targets() {
    let (rules, mut sim, owner, sw_type) = scene();
    assert!(rules.object("IGNORED").unwrap().hunter_seeker_ignored());
    let (allowed, ..) = scene_with_late_rules("[IGNORED]\nHunterSeeker.Ignore=no\n");
    assert!(!allowed.object("IGNORED").unwrap().hunter_seeker_ignored());
    assert_ne!(
        rules.simulation_config_hash(),
        allowed.simulation_config_hash()
    );
    let hunter = sim
        .spawn_object("FLYER", "Americans", 8, 8, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(hunter)
        .unwrap()
        .set_hunter_seeker_origin(crate::sim::hunter_seeker::HunterSeekerOrigin { owner, sw_type });
    sim.spawn_object("IGNORED", "Russians", 12, 12, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Russians", 16, 16, 0, &rules)
        .unwrap();
    assert!(sim.acquire_hunter_seeker_target(hunter, &rules, None, FrameEffects::default()));
    assert_eq!(
        sim.substrate
            .entities
            .get(hunter)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(target))
    );
    let rng = sim.scenario_rng.logical_state();
    assert!(!sim.acquire_hunter_seeker_target(hunter, &rules, None, FrameEffects::default()));
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

#[cfg(not(feature = "ares"))]
#[test]
fn hunter_seeker_is_not_admitted_without_ares() {
    let (rules, mut sim, owner, sw_type) = scene();
    assert_eq!(
        rules.super_weapon("Seek").unwrap().kind,
        SuperWeaponKind::NoEffect
    );
    assert!(rules.super_weapon("Seek").unwrap().hunter_seeker.is_none());
    // Also refuse a forged extension record before charge, RNG or wallet writes.
    let mut rules = rules;
    rules.super_weapons.get_mut("Seek").unwrap().kind = SuperWeaponKind::HunterSeeker;
    let before = sim.state_hash();
    assert!(!sim.fire_super_weapon(
        &rules,
        owner,
        sw_type,
        (0, 0),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.state_hash(), before);
}
