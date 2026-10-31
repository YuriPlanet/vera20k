//! Phobos airstrike target eligibility through the production consumers:
//! `WarheadType::AirstrikeTargets`, `TechnoType::AllowAirstrike`, the
//! `What_Weapon_Should_I_Use` arm at `0x6F3477` and the `CanFire` gate.
//! Source: `src/Ext/Techno/Hooks.Airstrike.cpp`,
//! `src/Ext/Techno/Hooks.Firing.cpp`, `src/Ext/WarheadType/Body.cpp:400`.

use super::TargetKind;
use super::combat_weapon::{
    attacker_facts, get_weapon, techno_target_facts, what_weapon_should_i_use,
};
use super::fire_error::FireError;
use super::fire_error_world::FireSubject;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::world::Simulation;

/// A Boris-shaped vehicle: primary `PRIMARY`, secondary `SECONDARY` whose
/// warhead carries the airstrike flag under test.
fn rules_for(airstrike_targets: &str, target_flags: &str, primary_airstrike: bool) -> RuleSet {
    let airstrike = if primary_airstrike {
        "Airstrike=yes\n"
    } else {
        ""
    };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Clear]\nTrack=100%\nBuildable=yes\n\
         [VehicleTypes]\n0=SCORER\n1=TARGET\n\
         [InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [SCORER]\nStrength=300\nSpeed=6\nSight=8\n\
         Primary=PRIMARY\nSecondary=SECONDARY\nCanRetaliate=yes\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [TARGET]\nStrength=300\nSpeed=6\nSight=8\n{target_flags}\n\
         Primary=PRIMARY\nSecondary=SECONDARY\nCanRetaliate=yes\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         [PRIMARY]\nDamage=101\nROF=30\nRange=6\nProjectile=SHOT\nWarhead=SWH\nOmniFire=yes\n\
         [SECONDARY]\nDamage=101\nROF=30\nRange=6\nProjectile=SHOT\nWarhead=AWH\nOmniFire=yes\n\
         [SHOT]\nInviso=yes\nAG=yes\nAA=no\n\
         [SWH]\nCellSpread=0\n{airstrike}\
         [AWH]\nCellSpread=0\nAirstrike=yes\nAirstrikeTargets={airstrike_targets}\n"
    )))
    .expect("airstrike rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let scorer = sim
        .spawn_object("SCORER", "Russians", 10, 10, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Americans", 12, 10, 0, rules)
        .unwrap();
    (sim, scorer, target)
}

fn chosen_weapon(rules: &RuleSet, sim: &Simulation, scorer: u64, target: u64) -> i32 {
    let actor = sim.substrate.entities.get(scorer).unwrap();
    let object = rules.object("SCORER").unwrap();
    let facts = techno_target_facts(
        sim.substrate.entities.get(target).unwrap(),
        rules.object("TARGET").unwrap(),
        sim.resolved_terrain.as_ref(),
        false,
        rules,
        &sim.interner,
    );
    what_weapon_should_i_use(
        rules,
        object,
        &attacker_facts(actor, object),
        Some(&facts),
        |index| {
            get_weapon(
                actor,
                object,
                index,
                &sim.substrate.entities,
                rules,
                &sim.interner,
            )
        },
    )
}

#[test]
fn the_default_target_list_refuses_a_vehicle() {
    // `AirstrikeTargets` unset keeps Phobos' `buildings` default, so the
    // secondary airstrike weapon is not selected against a vehicle.
    let rules = rules_for("", "", false);
    let (sim, scorer, target) = scene(&rules);
    assert_eq!(chosen_weapon(&rules, &sim, scorer, target), 0);
}

#[test]
fn units_in_the_list_select_the_airstrike_weapon() {
    let rules = rules_for("units", "", false);
    let (sim, scorer, target) = scene(&rules);
    assert_eq!(chosen_weapon(&rules, &sim, scorer, target), 1);
}

#[test]
fn allow_airstrike_no_refuses_a_type_the_list_admits() {
    let rules = rules_for("all", "AllowAirstrike=no", false);
    let (sim, scorer, target) = scene(&rules);
    assert_eq!(chosen_weapon(&rules, &sim, scorer, target), 0);
}

#[test]
fn allow_airstrike_reads_can_c4_for_a_building_and_true_otherwise() {
    let rules = rules_for("all", "", false);
    let target = rules.object("TARGET").unwrap();
    // A vehicle: CanC4 defaults false, yet the non-building default is true.
    assert!(!target.can_c4);
    assert!(target.allows_airstrike(false));
    assert!(target.allows_airstrike_weapon(false));
    let denied = rules_for("all", "AllowAirstrike=no", false);
    let target = denied.object("TARGET").unwrap();
    assert!(!target.allows_airstrike(false));
}

#[test]
fn a_primary_airstrike_warhead_refuses_an_ineligible_target() {
    // Phobos Hooks.Firing: the gate runs whatever the weapon slot, so a
    // primary airstrike warhead cannot fire at a kind its list excludes.
    let rules = rules_for("buildings", "", true);
    let (sim, scorer, target) = scene(&rules);
    let weapon_index = 0;
    let error = FireSubject {
        world: &sim,
        rules: &rules,
        overlay_registry: None,
        fog: None,
        firer: sim.substrate.entities.get(scorer).unwrap(),
        obj: rules.object("SCORER").unwrap(),
        target: Some(TargetKind::Entity(target)),
        weapon_index,
    }
    .fire_error(false);
    assert_eq!(error, FireError::Illegal);
}
