//! Authored original-engine upgrades, through Building Unlimbo4408DF and
//! House50B1D0/50AF10. No installed stock upgrade producer is assumed.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::Command;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, PlacementEvidence};

const OWNER: &str = "Americans";
const OTHER: &str = "Russians";
const PRIMARY: &str = "UpgradeReveal";
const SECONDARY: &str = "UpgradeReveal2";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n1=Russians\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=HOST\n1=DUAL\n2=SECOND\n3=MISSING\n\
             [HOST]\nStrength=1000\nUpgrades=3\nPowered=yes\nTurret=no\nSight=0\nSuperWeapon=UpgradeReveal\n\
             [DUAL]\nStrength=100\nPowersUpBuilding=HOST\nPowersUpToLevel=3\n\
             SuperWeapon=UpgradeReveal\nSuperWeapon2=UpgradeReveal2\nTurret=no\n\
             [SECOND]\nStrength=100\nPowersUpBuilding=HOST\nSuperWeapon2=UpgradeReveal2\nTurret=no\n\
             [MISSING]\nStrength=100\n\
             [SuperWeaponTypes]\n0=UpgradeReveal\n1=UpgradeReveal2\n\
             [UpgradeReveal]\nType=PsychicReveal\nRechargeTime=0.1\nIsPowered=yes\nAuxBuilding=MISSING\n\
             [UpgradeReveal2]\nType=PsychicReveal\nRechargeTime=0.1\nIsPowered=yes\nAuxBuilding=MISSING\n\
             [CombatDamage]\nPsychicRevealRadius=2\n",
        ),
        &IniFile::from_str(
            "[HOST]\nFoundation=1x1\n[DUAL]\nFoundation=1x1\n\
             [SECOND]\nFoundation=1x1\n[MISSING]\nFoundation=1x1\n",
        ),
    )
    .unwrap()
}

fn scene(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.fog.set_width_for_test(32);
    sim.fog.set_height_for_test(32);
    for name in [OWNER, OTHER] {
        let owner = sim.interner.get(name).unwrap();
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_is_human_for_test(true);
        house.set_player_control_for_test(true);
        construct_house_supers(&mut sim, rules, owner, false);
    }
    sim.session
        .set_current_house_for_test(sim.interner.get(OWNER));
    // Both human Houses keep a real building; no AI recovery or defeat is
    // supplied to make the authored provider history work.
    sim.spawn_object("HOST", OTHER, 27, 27, 0, rules).unwrap();
    sim
}

fn host(sim: &mut Simulation, rules: &RuleSet, x: u16) -> u64 {
    sim.spawn_object("HOST", OWNER, x, 10, 0, rules).unwrap()
}

fn install(sim: &mut Simulation, rules: &RuleSet, host: u64, kind: &str) -> u64 {
    let building = sim.substrate.entities.get(host).unwrap();
    let (x, y) = (building.position.rx(), building.position.ry());
    let temporary = sim
        .construct_object_limbo_at_height(kind, OWNER, x, y, 0, 0, rules, FrameEffects::default())
        .unwrap();
    assert_eq!(
        sim.reveal_constructed_object_at_height(
            temporary,
            x,
            y,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            rules,
            FrameEffects::default(),
        ),
        Some(temporary)
    );
    let consumed = sim.substrate.entities.get(temporary).unwrap();
    assert!(!consumed.is_object_alive() && consumed.lifecycle.in_limbo());
    temporary
}

fn instance<'a>(sim: &'a Simulation, owner: &str, name: &str) -> &'a SuperWeaponInstance {
    &sim.super_weapons[&sim.interner.get(owner).unwrap()][&sim.interner.get(name).unwrap()]
}

fn settle(sim: &mut Simulation, rules: &RuleSet, owner: &str) {
    let owner = sim.interner.get(owner).unwrap();
    sim.recheck_house_tech_tree(owner, rules);
    // A hold/revocation can request a follow-up. Consume it so the next
    // transition must publish its own request rather than inherit this one.
    sim.recheck_house_tech_tree(owner, rules);
    assert!(!sim.houses.get_mut(&owner).unwrap().take_tech_recheck());
}

fn charge(sim: &mut Simulation, rules: &RuleSet) {
    let duration = rules.super_weapon(PRIMARY).unwrap().recharge_time_frames;
    assert!(duration > 0 && duration < 100);
    for _ in 0..=duration {
        sim.advance_tick(&[], Some(rules), None, None, 33);
    }
    assert!(instance(sim, OWNER, PRIMARY).is_ready());
    assert!(instance(sim, OWNER, SECONDARY).is_ready());
}

#[test]
fn admitted_third_slot_upgrade_grants_both_supers_before_the_house_tail_and_can_launch() {
    let rules = rules();
    let mut sim = scene(&rules);
    let host = host(&mut sim, &rules, 10);
    settle(&mut sim, &rules, OWNER);
    assert!(!instance(&sim, OWNER, PRIMARY).is_active());
    install(&mut sim, &rules, host, "DUAL");
    assert_eq!(
        sim.substrate
            .entities
            .get(host)
            .unwrap()
            .building_upgrades()
            .types(),
        &[None, None, sim.interner.get("DUAL")]
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(host)
            .unwrap()
            .building_upgrades()
            .count(),
        3
    );
    // The ordinary installer calls50B1D0 immediately. Its upgrade-pointer
    // scan bypasses AuxBuilding; Building457630/457690 still does not.
    assert!(instance(&sim, OWNER, PRIMARY).is_active());
    assert!(instance(&sim, OWNER, SECONDARY).is_active());
    assert!(
        provided_supers(&sim, &rules, sim.substrate.entities.get(host).unwrap())
            .next()
            .is_none()
    );
    assert!(!aux_building_present(
        &sim,
        &rules,
        sim.interner.get(OWNER).unwrap(),
        rules.super_weapon(PRIMARY).unwrap()
    ));

    charge(&mut sim, &rules);
    let owner = sim.interner.get(OWNER).unwrap();
    let sw_type_id = sim.interner.get(PRIMARY).unwrap();
    assert!(!sim.fog.is_cell_revealed(owner, 20, 20));
    assert!(sim.apply_command_with_overlays(
        OWNER,
        &Command::LaunchSuperWeapon {
            sw_type_id,
            target_rx: 20,
            target_ry: 20
        },
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert!(sim.fog.is_cell_revealed(owner, 20, 20));
    assert!(!instance(&sim, OWNER, PRIMARY).is_ready());
    assert!(instance(&sim, OWNER, SECONDARY).is_ready());
}

#[test]
fn host_online_aggregation_and_secondary_only_sale_keep_native_refresh_timing() {
    let rules = rules();
    let mut sim = scene(&rules);
    let first = host(&mut sim, &rules, 10);
    let second = host(&mut sim, &rules, 14);
    install(&mut sim, &rules, first, "DUAL");
    install(&mut sim, &rules, second, "SECOND");
    settle(&mut sim, &rules, OWNER);
    charge(&mut sim, &rules);

    assert!(sim.go_building_offline(first, &rules, FrameEffects::default()));
    settle(&mut sim, &rules, OWNER);
    assert!(instance(&sim, OWNER, PRIMARY).is_suspended);
    assert!(!instance(&sim, OWNER, SECONDARY).is_suspended);
    assert!(sim.go_building_offline(second, &rules, FrameEffects::default()));
    settle(&mut sim, &rules, OWNER);
    assert!(instance(&sim, OWNER, SECONDARY).is_suspended);
    assert!(instance(&sim, OWNER, SECONDARY).is_active());
    assert!(instance(&sim, OWNER, SECONDARY).is_ready());
    assert!(sim.go_building_online(first, &rules, FrameEffects::default()));
    settle(&mut sim, &rules, OWNER);
    assert!(!instance(&sim, OWNER, PRIMARY).is_suspended);
    assert!(!instance(&sim, OWNER, SECONDARY).is_suspended);

    crate::sim::production::begin_selling(&mut sim, &rules, first, false);
    crate::sim::production::sell_stage_zero(&mut sim, &rules, first, FrameEffects::default());
    assert!(!instance(&sim, OWNER, PRIMARY).is_active());
    assert!(instance(&sim, OWNER, SECONDARY).is_active());
    assert!(instance(&sim, OWNER, SECONDARY).is_suspended);
    settle(&mut sim, &rules, OWNER);

    crate::sim::production::begin_selling(&mut sim, &rules, second, false);
    crate::sim::production::sell_stage_zero(&mut sim, &rules, second, FrameEffects::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(second)
            .unwrap()
            .building_upgrades()
            .types(),
        &[None; 3]
    );
    // PowerDown451690 calls50AF10 only for the removed primary field.
    // SECOND has only SuperWeapon2, so even a clean House tail retains it.
    assert!(instance(&sim, OWNER, SECONDARY).is_active());
    settle(&mut sim, &rules, OWNER);
    assert!(instance(&sim, OWNER, SECONDARY).is_active());
    // This real online transition supplies the later +1FC request. It is
    // deliberately not injected by the test or by upgrade removal.
    assert!(sim.go_building_online(second, &rules, FrameEffects::default()));
    settle(&mut sim, &rules, OWNER);
    assert!(!instance(&sim, OWNER, SECONDARY).is_active());
}

#[test]
fn restored_installed_provider_follows_human_capture_and_first_limbo() {
    let rules = rules();
    let mut sim = scene(&rules);
    let host = host(&mut sim, &rules, 10);
    install(&mut sim, &rules, host, "DUAL");
    settle(&mut sim, &rules, OWNER);
    settle(&mut sim, &rules, OTHER);
    let provider_authority = |sim: &Simulation| {
        crate::util::save_codec::serialize(&(
            &sim.houses,
            &sim.super_weapons,
            sim.substrate
                .entities
                .get(host)
                .unwrap()
                .building_upgrades(),
        ))
        .unwrap()
    };
    let before = provider_authority(&sim);
    let bytes = GameSnapshot::save(
        &sim,
        0,
        rules.simulation_config_hash(),
        "upgrade-provider",
        0,
    );
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    // This component snapshot uses the same supplied static terrain, not a
    // whole resource-loader fixture. Restore shared references before use.
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.clone().unwrap(),
        Default::default(),
        &rules,
    );
    // Compare exact authoritative owners. The full world hash also contains
    // Scenario RNG and navigation history, which have independent load rules.
    assert_eq!(provider_authority(&restored), before);
    assert_eq!(
        restored
            .substrate
            .entities
            .get(host)
            .unwrap()
            .building_upgrades(),
        sim.substrate
            .entities
            .get(host)
            .unwrap()
            .building_upgrades()
    );
    assert!(instance(&restored, OWNER, PRIMARY).is_active());
    assert!(instance(&restored, OWNER, SECONDARY).is_active());
    assert!(!instance(&restored, OTHER, PRIMARY).is_active());

    let other = restored.interner.get(OTHER).unwrap();
    restored.change_owner_with_rules(host, other, &rules, None, FrameEffects::default());
    settle(&mut restored, &rules, OWNER);
    settle(&mut restored, &rules, OTHER);
    assert!(!instance(&restored, OWNER, PRIMARY).is_active());
    assert!(!instance(&restored, OWNER, SECONDARY).is_active());
    assert!(instance(&restored, OTHER, PRIMARY).is_active());
    assert!(instance(&restored, OTHER, SECONDARY).is_active());
    assert_eq!(
        restored
            .substrate
            .entities
            .get(host)
            .unwrap()
            .building_upgrades()
            .types(),
        &[None, None, restored.interner.get("DUAL")]
    );

    restored.uninit_with_rules(host, &rules);
    // Building445D99's first Limbo requests the later House pass. The
    // base-only destructor provider predicate is not broadened for upgrades.
    assert!(instance(&restored, OTHER, PRIMARY).is_active());
    settle(&mut restored, &rules, OTHER);
    assert!(!instance(&restored, OTHER, PRIMARY).is_active());
    assert!(!instance(&restored, OTHER, SECONDARY).is_active());
}
