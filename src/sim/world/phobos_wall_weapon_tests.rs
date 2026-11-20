//! Phobos `TechnoExt::GetWeaponIndexAgainstWall`
//! (`Ext/Techno/WeaponHelpers.cpp:196-228`): the wall checks at
//! `0x51C1F1`, `0x73F495` and `0x70095A` read
//! `GetWeapon(GetWeaponIndexAgainstWall(..))` instead of the native
//! `GetWeapon(0)`, so with `AllowWeaponSelectAgainstWalls` a secondary
//! wall-breaker answers a wall the current weapon cannot hurt.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::world::entry_test_fixture;

const RULES: &str = "[BuildingTypes]\n1=WALLB\n[WALLB]\nWall=yes\nFoundation=1x1\nStrength=300\n\
     [VehicleTypes]\n0=DUALV\n1=PRIMARYV\n\
     [DUALV]\nSpeedType=Wheel\nStrength=100\nPrimary=WEAKGUN\nSecondary=WALLGUN\nAmmo=0\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [PRIMARYV]\nSpeedType=Wheel\nStrength=100\nPrimary=WALLGUN\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WEAKGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WEAKWH\n\
     [WALLGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WALLWH\n\
     [TESTPROJ]\nAG=yes\n\
     [Warheads]\n2=WEAKWH\n3=WALLWH\n\
     [WEAKWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [WALLWH]\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [O2]\nWall=yes\nLegalTarget=yes\nStrength=300\n";

const DUALV: u64 = 90;
const PRIMARYV: u64 = 91;

fn fixture(
    global: &str,
    dual_extra: &str,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let rules_text = format!("[CombatDamage]\n{global}{RULES}");
    // The type-level key belongs to DUALV's section only.
    let rules_text = rules_text.replace("[DUALV]\n", &format!("[DUALV]\n{dual_extra}"));
    let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules_and_fixed_art(
        &rules_text,
        &IniFile::from_str("[WALLB]\nToOverlay=O2\n[O2]\nDamageLevels=3\n"),
    );
    let american = sim.intern("Americans");
    for (id, name) in [(DUALV, "DUALV"), (PRIMARYV, "PRIMARYV")] {
        let mut mover = GameEntity::test_default(id, name, "Americans", 15, 15);
        mover.set_owner_for_test(american);
        mover.set_type_ref_for_test(sim.intern(name));
        mover.set_category_for_test(EntityCategory::Unit);
        mover.set_in_playfield(true);
        mover.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        sim.substrate.entities.insert(mover);
    }
    (sim, rules, registry)
}

fn map_enemy_wall(sim: &mut Simulation) {
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 15, 2, 0);
    sim.overlay_cell_mut_for_test(16, 15).overlay_id = Some(2);
    let enemy = sim.intern("Russians");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_wall_owner(16, 15, enemy);
}

fn entry(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    id: u64,
) -> Result<u8, String> {
    let cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((16, 15));
    sim.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules)
}

/// Unit `0x73F495`: the secondary answers the wall only under the key.
#[test]
fn the_global_key_lets_the_secondary_answer_a_wall_cell() {
    for (global, expected) in [("", Ok(7)), ("AllowWeaponSelectAgainstWalls=yes\n", Ok(5))] {
        let (mut sim, rules, registry) = fixture(global, "");
        map_enemy_wall(&mut sim);
        assert_eq!(
            entry(&sim, &rules, &registry, DUALV),
            expected,
            "{global:?}"
        );
        // The wall-breaking primary needs no fallback in either row.
        assert_eq!(
            entry(&sim, &rules, &registry, PRIMARYV),
            Ok(5),
            "{global:?}"
        );
    }
}

/// The type's `Nullable` key overrides the global in both directions.
#[test]
fn the_type_key_overrides_the_global() {
    for (global, type_key, expected) in [
        ("", "AllowWeaponSelectAgainstWalls=yes\n", Ok(5)),
        (
            "AllowWeaponSelectAgainstWalls=yes\n",
            "AllowWeaponSelectAgainstWalls=no\n",
            Ok(7),
        ),
        // The secondary does not answer while `NoSecondaryWeaponFallback`
        // holds and no AE gate refuses the primary.
        (
            "AllowWeaponSelectAgainstWalls=yes\n",
            "NoSecondaryWeaponFallback=yes\n",
            Ok(7),
        ),
    ] {
        let (mut sim, rules, registry) = fixture(global, type_key);
        map_enemy_wall(&mut sim);
        assert_eq!(
            entry(&sim, &rules, &registry, DUALV),
            expected,
            "{global:?} {type_key:?}"
        );
    }
}

/// What_Action's wall rung (`0x70095A`) also reads the wall-aware slot: the
/// ATTACK cursor appears against an enemy wall only when some slot can hurt
/// it, and the global key decides for the dual-weapon unit.
#[test]
fn the_attack_rung_reads_the_wall_weapon() {
    for (global, expected) in [("", false), ("AllowWeaponSelectAgainstWalls=yes\n", true)] {
        let (mut sim, rules, registry) = fixture(global, "");
        map_enemy_wall(&mut sim);
        let american = sim.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                american, 0, None, true, 0, 10,
            ));
        assert_eq!(
            sim.cell_attack_rung_admits(DUALV, (16, 15), &rules, Some(&registry)),
            expected,
            "{global:?}: DUALV"
        );
        assert!(
            sim.cell_attack_rung_admits(PRIMARYV, (16, 15), &rules, Some(&registry)),
            "{global:?}: the wall-breaking primary"
        );
    }
}
