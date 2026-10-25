//! Native Warhead Shake fields through the production Bullet detonation path.

use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

fn layered_rules() -> RuleSet {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Warheads]\n0=BOTH\n1=YONLY\n2=OFF\n\
         [BOTH]\nShakeXlo=1\nShakeXhi=6\nShakeYlo=-3\nShakeYhi=5\n\
         [YONLY]\nShakeXlo=0\nShakeXhi=0\nShakeYlo=-2\nShakeYhi=4\n\
         [OFF]\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BOTH]\nShakeXhi=10\n"),
    );
    RuleSet::from_processed_rules(&layers.process().expect("rules pass stack"))
        .expect("projected rules")
}

fn detonation(world: &mut Simulation, warhead: &str, projectile_id: u64) -> ProjectileDetonation {
    let payload = ProjectilePayload::new(0, world.interner.intern(warhead), None, false);
    ProjectileDetonation {
        projectile_id,
        source_id: crate::sim::combat::RAD_NO_ATTACKER,
        target: ProjectileTarget::Cell { rx: 10, ry: 10 },
        impact: ProjectileCoord::new(2688, 2688, 0),
        payload,
        reason: ProjectileDetonationReason::DeathWeapon,
    }
}

#[test]
fn warhead_shake_rules_drive_ordered_conditional_main_rng_detonation_updates() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let stock = retail.rules.warhead("HE").expect("stock HE warhead");
    assert_eq!(
        [
            stock.shake_x_lo,
            stock.shake_x_hi,
            stock.shake_y_lo,
            stock.shake_y_hi,
        ],
        [0; 4]
    );

    let rules = layered_rules();
    let both = rules.warhead("BOTH").expect("registered Warhead");
    assert_eq!(both.shake_x_lo, 1);
    assert_eq!(both.shake_x_hi, 10);
    assert_eq!(both.shake_y_lo, -3);
    assert_eq!(both.shake_y_hi, 5);
    let off = rules.warhead("OFF").expect("registered default Warhead");
    assert_eq!(
        [
            off.shake_x_lo,
            off.shake_x_hi,
            off.shake_y_lo,
            off.shake_y_hi
        ],
        [0; 4]
    );

    let mut world = Simulation::with_seed(0x5A_AA_1E);
    world.intern_rule_type_ids(&rules);
    world.resolve_type_handles(&rules);
    let mut expected_rng = world.main_rng.snapshot_for_test();
    let expected_x = expected_rng.next_range_i32_inclusive(1, 10);
    let expected_y = expected_rng.next_range_i32_inclusive(-3, 5);
    let expected_y_only = expected_rng.next_range_i32_inclusive(-2, 4);
    let expected_state = expected_rng.logical_state();

    let requests = [
        detonation(&mut world, "BOTH", 1),
        detonation(&mut world, "YONLY", 2),
        detonation(&mut world, "OFF", 3),
    ];
    let mut run = super::world_receiver::ReceiverRun::default();
    super::world_receiver::commit_projectiles(&mut world, &mut run, &requests, &rules, None);
    run.finish();

    assert_eq!(
        &world.screen_shake_updates[..],
        &[
            crate::sim::combat::ScreenShakeUpdate {
                x: Some(expected_x),
                y: Some(expected_y),
            },
            crate::sim::combat::ScreenShakeUpdate {
                x: None,
                y: Some(expected_y_only),
            },
        ]
    );
    assert_eq!(world.main_rng.logical_state(), expected_state);
}
