//! Execute96/109 integration. Native96 controls execute gamemd.exe SHA256
//! 3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600,
//! reusing trigger_action_values::nuke_strike_dispatch_case's ABI transport.
//! Edge/velocity literals below are observed native outputs, not Rust results.
use super::*;
use crate::map::{map_file::MapFile, playfield::PlayfieldBounds, waypoints::Waypoint};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::RAD_NO_ATTACKER;
use crate::sim::projectile::ProjectileTarget;
use crate::sim::superweapon::chronosphere_tests::{place, world_with};
use std::collections::HashMap;

fn rules(inviso: bool) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nIronCurtainInvokeAnim=none\n\
         [VehicleTypes]\n0=Carrier\n[Carrier]\nStrength=500\nArmor=heavy\nPrimary=ChemLauncher\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [ChemLauncher]\nDamage=100\nSpeed=250\nBright=yes\nProjectile=ChemMissile\nWarhead=Gas\n\
         [ChemMissile]\nROT=4\nInviso={inviso}\n\
         [Warheads]\n0=Gas\n[Gas]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nIronCurtainDuration=750\n"
    )))
    .unwrap()
}

fn waypoint(sim: &mut Simulation, cell: (i16, i16)) {
    sim.session.install_waypoints(&HashMap::from([(
        1,
        Waypoint {
            index: 1,
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        },
    )]));
}

fn run(sim: &mut Simulation, rules: &RuleSet, kind: i32) -> bool {
    // The primary numeric field is deliberately not the tail waypoint B.
    let entries = crate::map::actions::parse_actions(&IniFile::from_str(&format!(
        "[Actions]\nCASE=1,{kind},0,999,0,0,0,0,B\n"
    )));
    let action = RuntimeDefinitions::default().resolve_action(
        entries["CASE"].entries()[0].clone(),
        None,
        None,
    );
    let house = sim.interner.intern("IgnoredIncomingHouse");
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(house),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn chemical_strike_uses_original_edge_origins_and_fixed_unowned_launch() {
    // Original Execute96, real586AC0/46B050/trig; Ground and cell lookups
    // are ABI seams. Rust uses an explicitly flat rectangular terrain crop.
    for (cell, origin) in [
        ((16, 20), [1152, 8320, 0]),
        ((20, 16), [8320, 1152, 0]),
        ((16, 48), [3968, 12160, 0]),
        ((48, 16), [12160, 3968, 0]),
        ((48, 48), [16000, 8832, 0]),
        ((32, 32), [11904, 4736, 0]),
        ((17, 31), [6272, 9856, 0]),
    ] {
        let (rules, mut sim, _) = world_with(rules(false), 64, &[]);
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 32,
            off_fc: 2,
            off_100: 2,
            off_104: 28,
            off_108: 28,
        });
        waypoint(&mut sim, cell);
        let before = rng(&sim);
        assert!(run(&mut sim, &rules, 96));
        let bullets: Vec<_> = sim.projectiles.iter().collect();
        assert_eq!(bullets.len(), 1);
        let bullet = bullets[0].1;
        assert_eq!(
            [
                bullet.launch_origin().x,
                bullet.launch_origin().y,
                bullet.launch_origin().z
            ],
            origin
        );
        // Original Fire468AAC..468B57 persisted values, after its ROT tail;
        // the action ABI controls separately establish the pre-Fire vector.
        assert_eq!(
            bullet.velocity().native().map(|v| v.bits()),
            [0x3ca1a5ffffffffff, 0x3feffffffffffffe, 0]
        );
        assert_eq!(bullet.guidance().unwrap().max_speed, 20);
        assert_eq!(bullet.source_id(), RAD_NO_ATTACKER);
        assert_eq!(bullet.firer_house(), None);
        assert_eq!(bullet.payload().installed_weapon(), None);
        assert!(!bullet.payload().bright());
        assert_eq!(bullet.payload().base_damage, 100);
        assert_eq!(
            bullet.target(),
            ProjectileTarget::Cell {
                rx: cell.0 as u16,
                ry: cell.1 as u16
            }
        );
        assert_eq!(rng(&sim), before);
    }
}

#[test]
fn counted_trigger_dispatches_both_strikes_then_continues() {
    let (rules, mut sim, _) = world_with(rules(false), 64, &[]);
    let victim = place(&mut sim, &rules, "Carrier", (40, 40), 0, false, false);
    waypoint(&mut sim, (40, 40));
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
        [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n\
        [Triggers]\nT=Neutral,<none>,Strikes,0,1,1,1,0\n[Events]\nT=1,13,0,0\n\
        [Actions]\nT=3,96,0,999,0,0,0,0,B,109,0,999,0,0,0,0,B,56,0,0,0,0,0,0,A\n\
        [Tags]\nTAG=0,Strikes,T\n[VariableNames]\n0=Finished,0\n",
    )
    .unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(None);
    sim.session.set_started(true);
    sim.advance_triggers(Some(&rules), None, FrameEffects::default());
    assert!(sim.trigger_local_is_set_for_test(0));
    assert_eq!(sim.projectiles.iter().count(), 1);
    let entity = sim.entities().get(victim).unwrap();
    assert_eq!(
        entity.invulnerability.as_ref().unwrap().timer().duration(),
        750
    );
    assert!(!sim.sound_events.iter().any(|event| matches!(
        event,
        crate::sim::world::SimSoundEvent::SuperWeaponLaunched { .. }
            | crate::sim::world::SimSoundEvent::RadarEvent { .. }
    )));
}

#[test]
fn chemical_strike_uses_existing_fire_impact_and_retirement() {
    // Inviso is an explicit custom input, making this a Rust integration
    // regression through Bullet AI, not a ChemMissile flight comparison.
    let (rules, mut sim, _) = world_with(rules(true), 64, &[]);
    let victim = place(&mut sim, &rules, "Carrier", (40, 40), 0, false, false);
    waypoint(&mut sim, (40, 40));
    let health = sim.entities().get(victim).unwrap().health.current();
    assert!(run(&mut sim, &rules, 96));
    for _ in 0..4 {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    assert!(sim.entities().get(victim).unwrap().health.current() < health);
    assert_eq!(sim.projectiles.iter().count(), 0);
}

#[test]
fn retail_chemical_weapon_requires_real_allocation_before_find() {
    let Some(stock) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(stock.authored_rules.section("ChemLauncher").is_some());
    assert!(
        stock.rules.weapon("ChemLauncher").is_none(),
        "an unreferenced section is not a WeaponType allocation"
    );
    let Some(registered) =
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
            "[VehicleTypes]\n9999=ChemCarrier\n[ChemCarrier]\nPrimary=ChemLauncher\n",
        ))
    else {
        return;
    };
    // First allocated during the map pass: a section seen only in the earlier
    // RULESMD pass cannot retroactively initialize this WeaponType. Native
    // retained processing leaves Damage0 and no projectile/warhead pointers.
    let late_weapon = registered.rules.weapon("ChemLauncher").unwrap();
    assert_eq!(late_weapon.damage, 0);
    assert_eq!(late_weapon.projectile, None);
    assert_eq!(late_weapon.warhead, None);
    let mut map_text =
        String::from("[VehicleTypes]\n9999=ChemCarrier\n[ChemCarrier]\nPrimary=ChemLauncher\n");
    // Copy physical retail records into an explicit custom map layer; all
    // values still pass through the production readers and native pass order.
    for name in ["ChemLauncher", "ChemMissile", "Gas"] {
        map_text.push_str(&format!("[{name}]\n"));
        for (key, value) in stock.authored_rules.section(name).unwrap().raw_entries() {
            map_text.push_str(&format!("{key}={value}\n"));
        }
    }
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str(&map_text),
    )
    .unwrap()
    .rules;
    let weapon = rules.weapon("ChemLauncher").unwrap();
    assert_eq!(weapon.damage, 100);
    assert_eq!(weapon.projectile.as_deref(), Some("ChemMissile"));
    assert_eq!(weapon.warhead.as_deref(), Some("Gas"));
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    waypoint(&mut sim, (40, 40));
    assert!(run(&mut sim, &rules, 96));
    let bullet = sim.projectiles.iter().next().unwrap().1;
    assert_eq!(bullet.payload().base_damage, 100);
    assert_eq!(bullet.payload().installed_weapon(), None);
    assert_eq!(bullet.firer_house(), None);
}

#[test]
fn direct_strikes_reject_missing_safe_prerequisites() {
    let (rules, mut sim, _) = world_with(rules(false), 64, &[]);
    // An unset *valid* waypoint is native (0,0), not a missing input. The
    // absent terrain adapter, unlike an unset slot, is a safe rejection.
    assert!(run(&mut sim, &rules, 109));
    let terrain = sim.resolved_terrain.take();
    for kind in [96, 109] {
        assert!(!run(&mut sim, &rules, kind));
    }
    sim.resolved_terrain = terrain;
    waypoint(&mut sim, (40, 40));
    let missing = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    assert!(!run(&mut sim, &missing, 96));
    assert!(
        run(&mut sim, &missing, 109),
        "no targets is still a successful action"
    );
    assert_eq!(sim.projectiles.iter().count(), 0);
}
