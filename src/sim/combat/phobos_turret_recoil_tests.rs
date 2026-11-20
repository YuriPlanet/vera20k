//! Phobos `TechnoClass_FireAt_TurretRecoil` (`0x6FF0DD`,
//! `Ext/Techno/Hooks.Firing.cpp:820-833`): a weapon with
//! `TurretRecoil.Suppress=yes` skips the unit recoil record the vanilla arm
//! would arm, so the fired unit's recoil state stays clear.

use super::world_fire;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::world::{FrameEffects, Simulation};

/// `TRK` (turreted, ART `TurretRecoil=yes`) fires `GUN` at an unarmed `TGT`
/// two cells east. `weapon` adds keys to `GUN`.
fn scene(weapon: &str) -> (Simulation, RuleSet, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=TRK\n1=TGT\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=5\nROF=40\nRange=9.5\nSpeed=100\nProjectile=INVIS\n\
         Warhead=HIT\n{weapon}\
         [TRK]\nStrength=200\nArmor=none\nSpeed=4\nPrimary=GUN\nTurret=yes\n\
         [TGT]\nStrength=200\nArmor=none\nSpeed=4\n\
         [INVIS]\nInviso=yes\nAG=yes\n\
         [HIT]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let art = IniFile::from_str("[TRK]\nTurretRecoil=yes\n");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("TRK", "Americans", 10, 10, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TGT", "Russians", 12, 10, 0, &rules)
        .expect("target");
    // The ART recoil pass is component-tested separately; install the same
    // constructor state here so the fire path is the only subject.
    let mut recoil = crate::rules::recoil::RecoilConfig::default();
    recoil.apply_pass(
        IniFile::from_str(
            "[TRK]
TurretRecoil=yes
",
        )
        .section("TRK")
        .unwrap(),
    );
    sim.substrate
        .entities
        .get_mut(firer)
        .unwrap()
        .initialize_voxel_recoil(recoil);
    (sim, rules, firer, target)
}

fn fire(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64) {
    sim.commit_fire_visit(
        world_fire::FireVisit::Direct {
            id: firer,
            target: TargetKind::Entity(target),
            weapon_index: 0,
        },
        rules,
        None,
        FrameEffects::empty(),
    );
}

#[test]
fn the_suppress_key_keeps_the_unit_recoil_clear() {
    let (mut sim, rules, firer, target) = scene("");
    fire(&mut sim, &rules, firer, target);
    assert!(
        sim.substrate.entities.get(firer).unwrap().voxel_recoil().1,
        "the stock shot arms the recoil"
    );

    let (mut sim, rules, firer, target) = scene("TurretRecoil.Suppress=yes\n");
    let before = sim.projectiles.iter().count();
    fire(&mut sim, &rules, firer, target);
    assert_eq!(
        sim.projectiles.iter().count(),
        before + 1,
        "the suppressed shot still launches"
    );
    assert_eq!(
        sim.substrate.entities.get(firer).unwrap().voxel_recoil(),
        ([0.0; 2], false),
        "the suppressed weapon records no recoil"
    );
}
