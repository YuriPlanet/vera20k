//! Joined gunner callbacks through commands and retained snapshot state.

use super::{OWNER, entity, temporal_attack_arena};
use crate::sim::command::Command;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::SnapshotProcessState;

/// The normal Unload command clears TarCom before its delayed departure.
/// Continue both the live state and its saved equivalent through that command,
/// real Unlimbo and the departing CLEG's next admitted Temporal shot.
#[test]
fn retail_unload_and_snapshot_resume_preserve_the_gunners_rearm() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid, ifv, passenger, target) = temporal_attack_arena(rules);
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let saved = GameSnapshot::save(&sim, 0, 0, "Temporal gunner before unload", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.retain_process_state_for_load(SnapshotProcessState::Live(&sim));
    // The existing native load owner consumes the saved Scenario bytes, then
    // seeds zero. Compare continuations with that same RNG input; the gunner's
    // retained burst, timer and link must survive independently of this reset.
    sim.scenario_rng = SimRng::new(0);
    assert_eq!(restored.rng_state(), sim.rng_state());
    restored.restore_after_snapshot_load().unwrap();
    restored.resolve_type_handles(rules);
    restored.install_resolved_terrain_for_new_map(terrain);
    assert!(restored.rebuild_dynamic_navigation(rules));
    for id in [ifv, passenger, target] {
        let (before, after) = (entity(&sim, id), entity(&restored, id));
        assert_eq!(before.rearm_timer, after.rearm_timer);
        assert_eq!(before.weapon_burst.index(), after.weapon_burst.index());
        assert_eq!(before.temporal, after.temporal);
    }
    let mut histories = Vec::new();
    for mut sim in [sim, restored] {
        let commanded = sim.session.binary_frame() as i32;
        assert!(sim.apply_command(
            OWNER,
            &Command::UnloadPassengers { transport_id: ifv },
            Some(rules),
        ));
        for _ in 0..180 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            if !entity(&sim, passenger).passenger_role.is_inside_transport() {
                break;
            }
        }
        let gunner = entity(&sim, passenger);
        assert!(
            gunner.passenger_role.is_none(),
            "ordinary unload must finish"
        );
        assert!(!gunner.lifecycle.in_limbo());
        assert!(gunner.temporal.has_link());
        assert!(!entity(&sim, ifv).temporal.has_link());
        assert!(!entity(&sim, target).temporal.is_warped());
        let rearm = gunner.rearm_timer;
        assert!(rearm.start_frame() >= commanded);
        assert!(rearm.remaining(sim.session.binary_frame() as i32) > 0);
        assert!(sim.apply_command(
            OWNER,
            &Command::Attack {
                attacker_id: passenger,
                target_id: target,
            },
            Some(rules),
        ));
        let mut first_shot = None;
        for _ in 0..240 {
            let frame = sim.session.binary_frame() as i32;
            let waiting = entity(&sim, passenger).rearm_timer.remaining(frame);
            sim.fire_events.clear();
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            if let Some(shot) = sim
                .fire_events
                .iter()
                .find(|event| event.attacker_id == passenger)
            {
                assert_eq!(waiting, 0, "the returned gunner cannot fire during rearm");
                assert_eq!(sim.interner.resolve(shot.weapon_id), "NeutronRifle");
                first_shot.get_or_insert(frame);
            }
            if first_shot.is_some()
                && entity(&sim, passenger).temporal.warp_target() == Some(target)
            {
                break;
            }
        }
        assert!(
            first_shot.is_some(),
            "the departing gunner must fire after rearm"
        );
        assert_eq!(entity(&sim, passenger).temporal.warp_target(), Some(target));
        histories.push((
            rearm.start_frame(),
            rearm.duration(),
            first_shot,
            entity(&sim, passenger).rearm_timer,
            sim.scenario_rng.native_state_hex(),
        ));
    }
    assert_eq!(histories[0], histories[1], "saved and live continuations");
}
