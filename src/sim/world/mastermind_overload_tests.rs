//! Ordinary stock Mastermind simulation regressions; presentation consumers
//! reuse the single arena fixture from their owning layer.

use crate::sim::arena_fixture::mastermind::{MindScene, spark_ids};
use crate::sim::world::FrameEffects;

#[test]
fn a_due_overload_component_replaces_its_rate_without_erasing_natural_roll() {
    let Some(mut scene) = MindScene::retail() else {
        return;
    };
    scene.capture_four_through_real_attacks();
    scene.first_pulse();
    let before = scene.words();
    assert!(
        before[0] & 0x7fff_ffff != 0,
        "prior angle comes from a real ordinary rocking visit"
    );
    assert_ne!(
        before[2] & 0x7fff_ffff,
        0x3c75_c28f,
        "the real normal updater has already damped the previous pulse's rate"
    );
    let health = scene
        .simulation()
        .entities()
        .get(scene.mind())
        .unwrap()
        .health
        .current();
    let births = spark_ids(&scene);
    // Declared CaptureManager component continuation. Visit every real
    // countdown step monotonically, without Actor/Particle AI or another
    // RockingUpdate. No timer, node, angle, rate, RNG or health is supplied.
    let rules = scene.rules().clone();
    let mind = scene.mind();
    for _ in 0..=rules.mind_control.overload_frames[1] {
        scene
            .simulation_mut()
            .capture_manager_update(mind, &rules, None, FrameEffects::default());
        let current = scene
            .simulation()
            .entities()
            .get(scene.mind())
            .unwrap()
            .health
            .current();
        if current < health {
            assert_eq!(
                health - current,
                scene.rules().mind_control.overload_damage[1]
            );
            assert_eq!(spark_ids(&scene).difference(&births).count(), 5);
            let after = scene.words();
            assert_eq!(after[2] & 0x7fff_ffff, 0x3c75_c28f);
            assert_eq!(
                [after[0], after[1], after[3]],
                [before[0], before[1], before[3]],
                "471C78 assigns one rate, preserving naturally nonzero roll"
            );
            return;
        }
        assert_eq!(scene.words(), before);
        scene.simulation_mut().session.binary_frame += 1;
    }
    panic!("the actual retained countdown never reached its next component pulse");
}
