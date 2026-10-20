//! ra2-mono availability scenarios adapted to VERA's authoritative House counts.
//! Source: GameLogicManagerTriggerSuperweaponTests.cs, AuxBuildings_BlockUseBeforeCostAndCharge
//! and BlackWidowStructureBoundSuperweapons_FilterMutualExclusiveUpgradeState.
//! These are synthetic Rust regressions, not Ares/native executable comparisons.
//! Contributed under VERA's GPLv3, from the author's earlier ra2-mono work.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::object_type::ObjectCategory;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;

const OWNER: &str = "Americans";
const OTHER: &str = "Russians";
const BASE: &str = "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
    [BuildingTypes]\n0=GACLAB\n1=GASCEA\n2=GASCPF\n3=GAMERC\n4=NATIVEAUX\n\
    [GACLAB]\nStrength=1000\nSuperWeapon=BlackWidowSpecial\nSuperWeapon2=BlackWidowAlphaSpecial\n\
    [GASCEA]\nStrength=1000\nPower=-100\n[GASCPF]\nStrength=1000\n\
    [GAMERC]\nStrength=1000\n[NATIVEAUX]\nStrength=1000\n\
    [SuperWeaponTypes]\n0=BlackWidowSpecial\n1=BlackWidowAlphaSpecial\n\
    [BlackWidowSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
    SW.AuxBuildings=GASCEA,GASCPF\nSW.NegBuildings=GAMERC\n\
    [BlackWidowAlphaSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
    SW.AuxBuildings=GAMERC\n";
const ART: &str = "[GACLAB]\nFoundation=1x1\n[GASCEA]\nFoundation=1x1\n\
    [GASCPF]\nFoundation=1x1\n[GAMERC]\nFoundation=1x1\n[NATIVEAUX]\nFoundation=1x1\n";
const NORMAL: &str = "BlackWidowSpecial";
const ALPHA: &str = "BlackWidowAlphaSpecial";

fn rules_with(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(BASE);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART)).unwrap()
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    for (index, name) in [OWNER, OTHER].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, index as u8, None, true, 2000, 10));
        construct_house_supers(&mut sim, rules, owner, false);
    }
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        super::cell_receiver_tests::test_terrain_cell(x, y)
    }));
    sim.playfield_bounds = Some(super::cell_receiver_tests::test_playfield_bounds());
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, name: &str, owner: &str, x: u16) -> u64 {
    sim.spawn_object_at_height(name, owner, x, 10, 0, 0, rules)
        .unwrap()
}

fn allowed(sim: &Simulation, rules: &RuleSet, owner: &str, name: &str) -> bool {
    building_requirements_met(
        sim,
        rules,
        sim.interner.get(owner).unwrap(),
        rules.super_weapon(name).unwrap(),
    )
}

fn active(sim: &Simulation, owner: &str, name: &str) -> bool {
    sim.super_weapons[&sim.interner.get(owner).unwrap()][&sim.interner.get(name).unwrap()].is_active
}

fn recheck(sim: &mut Simulation, rules: &RuleSet, owner: &str) {
    let owner = sim.interner.get(owner).unwrap();
    sim.recheck_house_tech_tree(owner, rules);
    // Revocation itself requests a follow-up tail. Settle it so each next
    // lifecycle change must publish its own invalidation, not inherit one.
    sim.recheck_house_tech_tree(owner, rules);
    assert!(!sim.houses.get_mut(&owner).unwrap().take_tech_recheck());
}

fn ready(sim: &mut Simulation, name: &str) -> InternedId {
    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(name).unwrap();
    let instance = sim
        .super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap();
    instance.is_active = true;
    instance.is_ready = true;
    instance.charge_start_tick = sim.session.binary_frame() as i32;
    instance.charge_duration = 1;
    id
}

#[test]
fn auxiliary_is_any_of_negative_is_any_veto_and_native_aux_remains_required() {
    let rules = rules_with("[BlackWidowSpecial]\nAuxBuilding=NATIVEAUX\n");
    for auxiliary in ["GASCEA", "GASCPF"] {
        let mut sim = world(&rules);
        assert!(!allowed(&sim, &rules, OWNER, NORMAL));
        spawn(&mut sim, &rules, auxiliary, OTHER, 8);
        assert!(
            !allowed(&sim, &rules, OWNER, NORMAL),
            "other owner's building"
        );
        spawn(&mut sim, &rules, auxiliary, OWNER, 10);
        assert!(
            !allowed(&sim, &rules, OWNER, NORMAL),
            "native AuxBuilding is independent"
        );
        spawn(&mut sim, &rules, "NATIVEAUX", OWNER, 12);
        assert!(allowed(&sim, &rules, OWNER, NORMAL));
        sim.power_states
            .entry(sim.interner.get(OWNER).unwrap())
            .or_default()
            .set_is_low_power_for_test(true);
        assert!(
            allowed(&sim, &rules, OWNER, NORMAL),
            "presence does not require power"
        );
        spawn(&mut sim, &rules, "GAMERC", OWNER, 14);
        assert!(
            !allowed(&sim, &rules, OWNER, NORMAL),
            "negative overrides both prerequisites"
        );
    }
}

#[test]
fn black_widow_upgrade_switches_grants_at_the_existing_dirty_house_tail() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    spawn(&mut sim, &rules, "GACLAB", OWNER, 4);
    recheck(&mut sim, &rules, OWNER);
    assert!(!active(&sim, OWNER, NORMAL));
    assert!(!active(&sim, OWNER, ALPHA));
    spawn(&mut sim, &rules, "GASCEA", OWNER, 8);
    recheck(&mut sim, &rules, OWNER);
    assert!(active(&sim, OWNER, NORMAL));
    assert!(!active(&sim, OWNER, ALPHA));

    let upgrade = spawn(&mut sim, &rules, "GAMERC", OWNER, 12);
    // The stale normal grant cannot be fired even before the dirty tail.
    let id = ready(&mut sim, NORMAL);
    let owner = sim.interner.get(OWNER).unwrap();
    assert!(!sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (15, 15),
        None,
        FrameEffects::default(),
        false
    ));
    sim.sound_events.clear();
    recheck(&mut sim, &rules, OWNER);
    assert!(!active(&sim, OWNER, NORMAL));
    assert!(active(&sim, OWNER, ALPHA));
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::SuperWeaponStatusChanged { owner: changed_owner, sw_type }
            if *changed_owner == owner && *sw_type == id)));
    sim.uninit_with_rules(upgrade, &rules);
    recheck(&mut sim, &rules, OWNER);
    assert!(active(&sim, OWNER, NORMAL));
    assert!(!active(&sim, OWNER, ALPHA));
}

#[test]
fn capture_moves_last_negative_and_grants_between_both_houses() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    for (owner, offset) in [(OWNER, 0), (OTHER, 10)] {
        spawn(&mut sim, &rules, "GACLAB", owner, 4 + offset);
        spawn(&mut sim, &rules, "GASCEA", owner, 6 + offset);
    }
    let first = spawn(&mut sim, &rules, "GAMERC", OWNER, 8);
    let second = spawn(&mut sim, &rules, "GAMERC", OWNER, 10);
    recheck(&mut sim, &rules, OWNER);
    recheck(&mut sim, &rules, OTHER);
    let other = sim.interner.get(OTHER).unwrap();
    sim.change_owner_with_rules(first, other, &rules, None, FrameEffects::default());
    recheck(&mut sim, &rules, OWNER);
    recheck(&mut sim, &rules, OTHER);
    assert!(!active(&sim, OWNER, NORMAL), "one negative still remains");
    assert!(active(&sim, OWNER, ALPHA));
    assert!(!active(&sim, OTHER, NORMAL));
    assert!(active(&sim, OTHER, ALPHA));
    sim.change_owner_with_rules(second, other, &rules, None, FrameEffects::default());
    recheck(&mut sim, &rules, OWNER);
    recheck(&mut sim, &rules, OTHER);
    assert!(active(&sim, OWNER, NORMAL));
    assert!(!active(&sim, OWNER, ALPHA));
    assert!(!active(&sim, OTHER, NORMAL));
    assert!(active(&sim, OTHER, ALPHA));
}

#[test]
fn limbo_buildings_do_not_satisfy_auxiliary_or_block_as_negative() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    spawn(&mut sim, &rules, "GACLAB", OWNER, 4);
    sim.spawn_object_limbo_at_height("GASCEA", OWNER, 8, 10, 0, 0, &rules)
        .unwrap();
    assert!(!allowed(&sim, &rules, OWNER, NORMAL));
    let auxiliary = spawn(&mut sim, &rules, "GASCPF", OWNER, 10);
    sim.spawn_object_limbo_at_height("GAMERC", OWNER, 12, 10, 0, 0, &rules)
        .unwrap();
    assert!(allowed(&sim, &rules, OWNER, NORMAL));
    recheck(&mut sim, &rules, OWNER);
    assert!(active(&sim, OWNER, NORMAL));
    sim.techno_limbo_with_rules(auxiliary, &rules, None);
    assert!(!allowed(&sim, &rules, OWNER, NORMAL));
    recheck(&mut sim, &rules, OWNER);
    assert!(!active(&sim, OWNER, NORMAL));
}

#[test]
fn command_refusal_preserves_charge_state_and_rng_then_same_command_launches() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    let owner = sim.interner.get(OWNER).unwrap();
    let id = ready(&mut sim, NORMAL);
    let command = Command::LaunchSuperWeapon {
        sw_type_id: id,
        target_rx: 15,
        target_ry: 15,
    };
    sim.sound_events.clear();
    let before = sim.state_hash();
    sim.apply_command_with_overlays(OWNER, &command, Some(&rules), None, FrameEffects::default());
    assert_eq!(
        sim.state_hash(),
        before,
        "refusal must not alter charge, money or RNG"
    );
    assert!(
        sim.sound_events.is_empty(),
        "no launch sound or defense alerts"
    );
    spawn(&mut sim, &rules, "GASCEA", OWNER, 8);
    sim.sound_events.clear();
    sim.apply_command_with_overlays(OWNER, &command, Some(&rules), None, FrameEffects::default());
    assert!(!sim.super_weapons[&owner][&id].is_ready);
    assert!(
        sim.sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
    );
}

#[test]
fn unavailable_ai_does_not_enter_target_search_or_draw_rng() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    let owner = sim.interner.get(OWNER).unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(false);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_player_control_for_test(false);
    assert!(!sim.houses[&owner].is_controlled_by_human(sim.session.game_mode_nonzero()));
    ready(&mut sim, NORMAL);
    let before = sim.state_hash();
    ai_fire::AI_FIRE_LOG.set(Some(Vec::new()));
    ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
    let observed = ai_fire::AI_FIRE_LOG.take().unwrap();
    assert_eq!(observed, vec![ai_fire::AiFireEvent::TryFire]);
    assert_eq!(sim.state_hash(), before);
}

#[test]
fn snapshot_retains_counts_and_pending_recheck_without_new_availability_state() {
    let rules = rules_with("");
    let mut sim = world(&rules);
    spawn(&mut sim, &rules, "GACLAB", OWNER, 4);
    spawn(&mut sim, &rules, "GASCEA", OWNER, 8);
    recheck(&mut sim, &rules, OWNER);
    spawn(&mut sim, &rules, "GAMERC", OWNER, 12);
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "building-gates",
        0,
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    // Static map authority is supplied by resources, not this synthetic
    // header-only snapshot. Reattach it before rebuilding derived indexes.
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    // This header-only fixture does not restore a full map's navigation
    // resources. Compare the promised persistent owners, not a full-world
    // hash (which also includes that unrelated navigation history).
    let availability_state = |sim: &Simulation| {
        crate::util::save_codec::serialize(&(&sim.houses, &sim.super_weapons)).unwrap()
    };
    assert_eq!(availability_state(&restored), availability_state(&sim));
    assert!(!allowed(&restored, &rules, OWNER, NORMAL));
    assert!(allowed(&restored, &rules, OWNER, ALPHA));
    recheck(&mut sim, &rules, OWNER);
    recheck(&mut restored, &rules, OWNER);
    assert!(!active(&restored, OWNER, NORMAL));
    assert!(active(&restored, OWNER, ALPHA));
    assert_eq!(availability_state(&restored), availability_state(&sim));
}

#[test]
fn ordered_building_lists_find_existing_types_without_allocation() {
    let art = IniFile::empty();
    let source = |lists: &str| {
        IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=First\n1=SECOND\n2=Blocked\n\
         [SuperWeaponTypes]\n0=Gate\n[Gate]\nType=PsychicReveal\n{lists}\n\
         [First]\nStrength=432\n[SECOND]\nStrength=123\n[Blocked]\nStrength=100\n"
        ))
    };
    let mut layers = RulesLayerStack::new(source(
        "SW.AuxBuildings=first,Missing,SECOND\nSW.NegBuildings=Blocked,Missing",
    ));
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let control = RulesLayerStack::new(source("SW.AuxBuildings=none\nSW.NegBuildings=none"))
        .process_with_fixed_art(&art)
        .unwrap();
    assert_eq!(
        processed.native_type_construction_trace().events(),
        control.native_type_construction_trace().events()
    );
    let inspect = |layers: &RulesLayerStack| {
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let saved = crate::util::save_codec::serialize(&processed).unwrap();
        let restored = crate::util::save_codec::deserialize(&saved).unwrap();
        let rules = RuleSet::from_processed_rules(&restored).unwrap();
        let requirements = rules
            .super_weapon("Gate")
            .unwrap()
            .building_requirements
            .clone()
            .unwrap();
        (rules, requirements)
    };
    let (rules, requirements) = inspect(&layers);
    assert_eq!(requirements.auxiliary, ["First", "SECOND"]);
    assert_eq!(requirements.negative, ["Blocked"]);
    assert!(
        rules
            .object_in_category(ObjectCategory::Building, "Missing")
            .is_none()
    );

    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[Gate]\nSW.AuxBuildings=\nRechargeTime=2"),
    );
    assert_eq!(
        inspect(&layers).1,
        requirements,
        "empty and omitted keys retain"
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BuildingTypes]\n3=Missing\n[Missing]\nStrength=77"),
    );
    assert_eq!(
        inspect(&layers).1,
        requirements,
        "later registration cannot retroactively bind"
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Gate]\nSW.AuxBuildings=missing\nSW.NegBuildings=Unknown"),
    );
    let (_, requirements) = inspect(&layers);
    assert_eq!(requirements.auxiliary, ["Missing"]);
    assert!(
        requirements.negative.is_empty(),
        "unknown-only replaces previous list"
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Gate]\nSW.AuxBuildings=none\nSW.NegBuildings=,,,"),
    );
    let (_, requirements) = inspect(&layers);
    assert!(requirements.auxiliary.is_empty());
    assert!(requirements.negative.is_empty());
    assert!(requirements.allows(|_| false));
}

#[test]
fn absent_tags_preserve_stock_and_resolved_extension_lists_participate_in_hash() {
    let mut rules = rules_with("");
    let original = rules.simulation_config_hash();
    rules
        .super_weapons
        .values_mut()
        .find(|sw| sw.id == NORMAL)
        .unwrap()
        .building_requirements
        .as_mut()
        .unwrap()
        .negative
        .clear();
    assert_ne!(rules.simulation_config_hash(), original);
    let stock = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[SuperWeaponTypes]\n0=Stock\n[Stock]\nType=PsychicReveal\n"),
        &IniFile::empty(),
    )
    .unwrap();
    assert!(
        stock
            .super_weapon("Stock")
            .unwrap()
            .building_requirements
            .is_none()
    );
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[SuperWeaponTypes]\n0=Stock\n[Stock]\nType=PsychicReveal\n",
    ))
    .process_with_fixed_art(&IniFile::empty())
    .unwrap();
    let saved = crate::util::save_codec::serialize(&processed).unwrap();
    let restored = crate::util::save_codec::deserialize(&saved).unwrap();
    let restored_rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert!(
        restored_rules
            .super_weapon("Stock")
            .unwrap()
            .building_requirements
            .is_none()
    );
    assert_eq!(
        restored_rules.simulation_config_hash(),
        stock.simulation_config_hash()
    );
}

#[test]
fn generic_warhead_uses_the_same_gate_before_damage_and_charge() {
    let rules = rules_with(
        "[BlackWidowSpecial]\nType=GenericWarhead\nSW.Warhead=Payload\nSW.Damage=75\n[Payload]\nCellSpread=0.5\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    let mut sim = world(&rules);
    let target = spawn(&mut sim, &rules, "GAMERC", OTHER, 15);
    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(NORMAL).unwrap();
    assert!(trigger_grant_super(&mut sim, &rules, owner, id, true));
    ready(&mut sim, NORMAL);
    sim.sound_events.clear();
    let before = sim.state_hash();
    assert!(!sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (15, 10),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.state_hash(), before);
    assert!(sim.sound_events.is_empty());
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 1000);
    let auxiliary = spawn(&mut sim, &rules, "GASCEA", OWNER, 8);
    sim.sound_events.clear();
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (15, 10),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.entities().get(target).unwrap().health.current(), 925);
    // A new retained ready instance is gated immediately after removal, before
    // the house's grant/revoke tail can hide a missing final-command check.
    ready(&mut sim, NORMAL);
    sim.uninit_with_rules(auxiliary, &rules);
    let before = sim.state_hash();
    assert!(!sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (15, 10),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(sim.state_hash(), before);
}
