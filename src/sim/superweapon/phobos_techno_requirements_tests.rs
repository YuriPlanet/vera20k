//! Phobos `SWTypeExt`'s techno gates: `SW.TechLevel`, `SW.AuxTechnos` and
//! `SW.NegTechnos` (`src/Ext/SWType/SWHelpers.cpp::IsAvailable`). Synthetic
//! Rust regressions over the shared availability owner, not native executable
//! comparisons.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;

const OWNER: &str = "Americans";
const BASE: &str = "[InfantryTypes]\n0=E1\n1=DESO\n[VehicleTypes]\n0=MTNK\n\
[AircraftTypes]\n[BuildingTypes]\n0=GACLAB\n1=NATIVEAUX\n\
[GACLAB]\nStrength=1000\n\
[NATIVEAUX]\nStrength=1000\n\
[E1]\nStrength=100\nSpeed=4\n\
[DESO]\nStrength=100\nSpeed=4\n\
[MTNK]\nStrength=300\nSpeed=6\n\
[SuperWeaponTypes]\n0=GateSpecial\n\
[GateSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n";
const ART: &str = "[GACLAB]\nFoundation=1x1\n[NATIVEAUX]\nFoundation=1x1\n";
const SW: &str = "GateSpecial";

fn rules_with(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(BASE);
    ini.merge(&IniFile::from_str(extra));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(ART))
        .expect("rules with the Phobos gates parse")
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern(OWNER);
    sim.houses
        .register(HouseState::new(owner, 0u8, None, true, 2000, 10));
    construct_house_supers(&mut sim, rules, owner, false);
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        super::cell_receiver_tests::test_terrain_cell(x, y)
    }));
    sim.set_playfield_bounds_for_test(Some(super::cell_receiver_tests::test_playfield_bounds()));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, name: &str, x: u16) {
    sim.spawn_object_at_height(name, OWNER, x, 10, 0, 0, rules)
        .expect("techno spawns");
}

fn allowed(sim: &Simulation, rules: &RuleSet) -> bool {
    super_weapon_available(sim, rules, sim.interner.get(OWNER).unwrap(), SW)
}

fn recheck(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    sim.recheck_house_tech_tree(owner, rules);
    sim.recheck_house_tech_tree(owner, rules);
    assert!(!sim.houses[&owner].tech_recheck_pending());
}

fn active(sim: &Simulation, owner: InternedId) -> bool {
    sim.super_weapons[&owner][&sim.interner.get(SW).unwrap()].is_active
}

/// The three keys land in the projected availability with their native
/// meaning: a minimum TechLevel, any-of `SW.AuxTechnos`, none-of
/// `SW.NegTechnos`, and unknown techno names skipped.
#[cfg(feature = "phobos")]
#[test]
fn phobos_reads_the_three_superweapon_gates() {
    let rules = rules_with(
        "[GateSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
         SW.TechLevel=5\nSW.AuxTechnos=e1,NOSUCHTYPE\nSW.NegTechnos=DESO\n",
    );
    let availability = rules
        .phobos_super_weapon_metadata(SW)
        .expect("the capability projects every registered definition");
    assert!(availability.allows_tech_level(5));
    assert!(
        !availability.allows_tech_level(4),
        "a plain minimum, not a cap"
    );
    assert!(
        availability.allows_technos(|id| id.eq_ignore_ascii_case("E1")),
        "the known token binds case-insensitively"
    );
    assert!(
        !availability.allows_technos(|_| false),
        "an aux list of one known type needs that type"
    );
    assert!(
        !availability.allows_technos(|id| id.eq_ignore_ascii_case("DESO")),
        "a present neg techno refuses"
    );

    let unknown_only = rules_with(
        "[GateSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
                    SW.AuxTechnos=NOSUCHTYPE,NOSUCHOTHER\n",
    );
    let availability = unknown_only
        .phobos_super_weapon_metadata(SW)
        .expect("registered definition");
    assert!(
        availability.allows_technos(|_| false),
        "unknown names are skipped, leaving an empty gate"
    );
}

/// `SW.AuxTechnos` looks at every techno category, so one owned vehicle
/// satisfies a list whose other entry is infantry.
#[cfg(feature = "phobos")]
#[test]
fn phobos_aux_technos_scan_every_category() {
    let rules = rules_with(
        "[GateSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
         SW.AuxTechnos=E1,MTNK\n",
    );
    let mut sim = world(&rules);
    assert!(!allowed(&sim, &rules), "no listed techno is owned");
    spawn(&mut sim, &rules, "MTNK", 12);
    assert!(allowed(&sim, &rules), "the vehicle satisfies the list");
}

/// `SW.NegTechnos` refuses availability as soon as one listed techno exists,
/// and `SW.TechLevel` gates before any techno test.
#[cfg(feature = "phobos")]
#[test]
fn phobos_neg_technos_and_tech_level_gate_availability() {
    let rules = rules_with(
        "[GateSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
         SW.TechLevel=5\nSW.NegTechnos=DESO\n",
    );
    let mut sim = world(&rules);
    assert!(
        allowed(&sim, &rules),
        "level 10 meets the gate and no DESO exists"
    );
    spawn(&mut sim, &rules, "DESO", 14);
    assert!(!allowed(&sim, &rules), "the owned neg techno refuses");

    let owner = sim.interner.get(OWNER).unwrap();
    let mut no_neg_sim = world(&rules);
    no_neg_sim
        .houses
        .get_mut(&owner)
        .unwrap()
        .set_tech_level_for_test(4);
    assert!(
        !allowed(&no_neg_sim, &rules),
        "below SW.TechLevel is refused without any neg techno"
    );
}

#[test]
fn phobos_aux_counts_present_insignificant_techno_but_not_its_limbo() {
    let rules = rules_with(
        "[GateSpecial]\nSW.AuxTechnos=E1\n[E1]\nInsignificant=yes\n[GACLAB]\nSuperWeapon=GateSpecial\n",
    );
    let mut sim = world(&rules);
    let owner = sim.interner.get(OWNER).unwrap();
    spawn(&mut sim, &rules, "GACLAB", 4);
    recheck(&mut sim, &rules, owner);
    assert!(!active(&sim, owner));
    let id = sim
        .spawn_object_at_height("E1", OWNER, 12, 10, 0, 0, &rules)
        .unwrap();
    assert!(allowed(&sim, &rules), "present Insignificant technos count");
    assert!(
        sim.houses[&owner].tech_recheck_pending(),
        "Phobos Foot Init requests the real grant pass"
    );
    recheck(&mut sim, &rules, owner);
    assert!(active(&sim, owner));
    sim.techno_limbo_with_rules(id, &rules, None);
    assert!(
        !allowed(&sim, &rules),
        "Phobos's limbo hook excludes Insignificant"
    );
}

#[test]
fn phobos_aux_excludes_factory_product_until_first_placement() {
    let rules = rules_with("[GateSpecial]\nSW.AuxTechnos=E1\n");
    let mut sim = world(&rules);
    let id = sim
        .construct_object_limbo_at_height(
            "E1",
            OWNER,
            12,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(
        !allowed(&sim, &rules),
        "construction alone is not Phobos presence"
    );
    sim.reveal_constructed_object_at_height(
        id,
        12,
        10,
        0,
        0,
        crate::sim::world::PlacementEvidence::EvaluateMark,
        &rules,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        allowed(&sim, &rules),
        "first placement does not subtract a limbo count"
    );
}

#[test]
fn phobos_limbo_gates_survive_save_transfer_and_uninit() {
    let rules = rules_with("[GateSpecial]\nSW.NegTechnos=E1\n[GACLAB]\nSuperWeapon=GateSpecial\n");
    let mut sim = world(&rules);
    let other = sim.interner.intern("Other");
    sim.houses
        .register(HouseState::new(other, 1u8, None, true, 2000, 10));
    construct_house_supers(&mut sim, &rules, other, false);
    let owner = sim.interner.get(OWNER).unwrap();
    spawn(&mut sim, &rules, "GACLAB", 4);
    sim.spawn_object_at_height("GACLAB", "Other", 20, 10, 0, 0, &rules)
        .unwrap();
    let id = sim
        .spawn_object_at_height("E1", OWNER, 12, 10, 0, 0, &rules)
        .unwrap();
    sim.techno_limbo_with_rules(id, &rules, None);
    sim.techno_limbo_with_rules(id, &rules, None);
    sim.spawn_from_map(&[], Some(&rules));
    assert!(
        !allowed(&sim, &rules),
        "ordinary and repeated limbo retain one neg techno"
    );
    recheck(&mut sim, &rules, owner);
    recheck(&mut sim, &rules, other);
    assert!(!active(&sim, owner));
    assert!(active(&sim, other));
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "phobos-limbo",
        0,
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert!(!allowed(&restored, &rules));
    restored
        .reveal_constructed_object_at_height(
            id,
            12,
            10,
            0,
            0,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    restored.techno_limbo_with_rules(id, &rules, None);
    restored.change_owner_with_rules(id, other, &rules, None, FrameEffects::default());
    assert!(restored.houses[&owner].tech_recheck_pending());
    assert!(restored.houses[&other].tech_recheck_pending());
    recheck(&mut restored, &rules, owner);
    recheck(&mut restored, &rules, other);
    assert!(active(&restored, owner));
    assert!(!active(&restored, other));
    assert!(
        allowed(&restored, &rules),
        "capture removes the old owner's limbo count"
    );
    assert!(!super_weapon_available(&restored, &rules, other, SW));
    restored.uninit_with_rules(id, &rules);
    assert!(
        super_weapon_available(&restored, &rules, other, SW),
        "UnInit removes limbo presence before the pending-delete drain"
    );
}

#[test]
fn phobos_techno_gates_count_installed_upgrade_on_its_host() {
    let rules = rules_with(
        "[BuildingTypes]\n2=UPGRADE\n[GACLAB]\nUpgrades=1\n\
        [UPGRADE]\nStrength=100\nPowersUpBuilding=GACLAB\nPowersUpToLevel=1\n\
        [SuperWeaponTypes]\n1=BlockSpecial\n[BlockSpecial]\nType=PsychicReveal\nSW.NegTechnos=UPGRADE\n\
        [GateSpecial]\nSW.AuxTechnos=UPGRADE\n",
    );
    let mut sim = world(&rules);
    let owner = sim.interner.get(OWNER).unwrap();
    let host = sim
        .spawn_object_at_height("GACLAB", OWNER, 12, 10, 0, 0, &rules)
        .unwrap();
    let temporary = sim
        .construct_object_limbo_at_height(
            "UPGRADE",
            OWNER,
            12,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(!allowed(&sim, &rules), "producing upgrades are excluded");
    assert!(super_weapon_available(&sim, &rules, owner, "BlockSpecial"));
    assert!(sim.try_install_building_upgrade(
        temporary,
        host,
        &rules,
        crate::sim::world::UninitContext::with_rules(&rules)
    ));
    assert!(allowed(&sim, &rules), "the host owns the installed upgrade");
    assert!(!super_weapon_available(&sim, &rules, owner, "BlockSpecial"));
    sim.power_down_building_upgrade(host, &rules);
    assert!(!allowed(&sim, &rules));
    assert!(super_weapon_available(&sim, &rules, owner, "BlockSpecial"));
}

#[test]
fn phobos_command_refuses_unmet_gate_without_consuming_a_retained_charge() {
    let rules = rules_with("[GateSpecial]\nType=ForceShield\nSW.AuxTechnos=E1\n");
    let mut sim = world(&rules);
    let owner = sim.interner.get(OWNER).unwrap();
    let sw = sim.interner.get(SW).unwrap();
    spawn(&mut sim, &rules, "GACLAB", 12);
    let instance = sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw)
        .unwrap();
    instance.is_active = true;
    instance.is_ready = true;
    instance.charge_start_tick = sim.session.binary_frame() as i32;
    instance.charge_duration = 1;
    let before = crate::util::save_codec::serialize(&sim.super_weapons).unwrap();
    assert!(!sim.fire_super_weapon(
        &rules,
        owner,
        sw,
        (12, 10),
        None,
        FrameEffects::default(),
        false
    ));
    assert_eq!(
        crate::util::save_codec::serialize(&sim.super_weapons).unwrap(),
        before
    );
    spawn(&mut sim, &rules, "E1", 16);
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        sw,
        (12, 10),
        None,
        FrameEffects::default(),
        false
    ));
}
