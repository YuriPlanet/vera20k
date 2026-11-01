//! Player-file NoInit versus mechanical simulation continuation.
use super::*;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};

fn cloak(sim: &Simulation, id: u64) -> CloakRuntime {
    sim.entities().get(id).unwrap().cloak.unwrap()
}

fn transition_fixture(state: i32) -> Option<(Simulation, RuleSet, ResolvedTerrainGrid, u64)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str("[E1]\nCloakable=yes\n"),
    )?
    .rules;
    assert!(rules.object("E1").unwrap().cloakable);
    assert_eq!(rules.object("E1").unwrap().cloaking_speed, 7);
    // Reuse the flat load fixture; only the layered rules come from Hills.
    let (mut sim, terrain) = super::infantry_death_restore_tests::death_load_world(&rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let id = sim
        .spawn_object("E1", "Americans", 1, 1, 0, &rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(id)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap()
        .set_state_for_test(0);
    let registry = OverlayTypeRegistry::empty();
    for _ in 0..32 {
        sim.advance_app_frame(
            &[],
            &rules,
            Some(&registry),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
        if cloak(&sim, id).state() == 1 && cloak(&sim, id).depth() == 2 {
            break;
        }
    }
    assert_eq!((cloak(&sim, id).state(), cloak(&sim, id).depth()), (1, 2));
    if state == 3 {
        // A saved reversing fade: native StartUncloaking7036C0 writes state3
        // and delta-1. Keep the live stage timer rather than inventing a clock.
        let mut value = serde_json::to_value(cloak(&sim, id)).unwrap();
        value["state"] = serde_json::json!(3);
        value["step_delta"] = serde_json::json!(-1);
        sim.entities_mut_for_test().get_mut(id).unwrap().cloak =
            Some(serde_json::from_value(value).unwrap());
    }
    Some((sim, rules, terrain, id))
}

fn prepare(saved: &Simulation, rules: &RuleSet, terrain: &ResolvedTerrainGrid) -> Simulation {
    PreparedLoad::prepare_candidate(
        &snapshot_bytes(saved, rules),
        Some(saved),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(rules),
        Some(terrain),
        Some(&OverlayTypeRegistry::empty()),
    )
    .unwrap()
    .simulation
}

fn frame_cloak_sounds(sim: &mut Simulation, rules: &RuleSet) -> usize {
    let output = sim
        .advance_app_frame(
            &[],
            rules,
            Some(&OverlayTypeRegistry::empty()),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(output.tick.frame_committed);
    output
        .sound_events
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::CloakSound { .. }))
        .count()
}

#[test]
fn player_load_restarts_mid_cloak_without_reentering_cloak() {
    let Some((saved, rules, terrain, id)) = transition_fixture(1) else {
        return;
    };
    let before = cloak(&saved, id);
    let mut internal = GameSnapshot::load(&snapshot_bytes(&saved, &rules))
        .unwrap()
        .sim;
    internal.restore_after_snapshot_load().unwrap();
    assert_eq!(
        cloak(&internal, id),
        before,
        "generic continuation preserves its clock"
    );
    let mut restored = prepare(&saved, &rules, &terrain);
    let loaded = cloak(&restored, id);
    // InfantryLoad521A0C -> TechnoNoInit6F4335..4365, original gamemd.exe.
    assert_eq!(loaded.state(), 1);
    assert_eq!(loaded.depth(), 0);
    assert_eq!(loaded.step_delta(), 1);
    assert_eq!(loaded.step_timer().speed(), 0);
    assert_eq!(loaded.recloak_delay(), before.recloak_delay());
    assert_eq!(
        loaded.step_timer().timer(),
        crate::sim::timer::CdTimer::started(restored.session().binary_frame() as i32, 0)
    );
    assert_eq!(frame_cloak_sounds(&mut restored, &rules), 0);
    assert_eq!(cloak(&restored, id).state(), 1);
    assert_eq!(cloak(&restored, id).depth(), 0);
    assert_eq!(cloak(&restored, id).step_timer().speed(), 1);
    assert_eq!(frame_cloak_sounds(&mut restored, &rules), 0);
    assert_eq!(cloak(&restored, id).depth(), 1);
    for _ in 0..rules.general.cloaking_stages {
        if cloak(&restored, id).is_fully_cloaked() {
            break;
        }
        assert_eq!(frame_cloak_sounds(&mut restored, &rules), 0);
    }
    assert!(
        cloak(&restored, id).is_fully_cloaked(),
        "ordinary completion still runs"
    );
}

#[test]
fn player_load_finishes_mid_uncloak_on_first_update() {
    let Some((saved, rules, terrain, id)) = transition_fixture(3) else {
        return;
    };
    let mut restored = prepare(&saved, &rules, &terrain);
    assert_eq!(cloak(&restored, id).state(), 3);
    assert_eq!(cloak(&restored, id).depth(), 0);
    assert_eq!(cloak(&restored, id).step_delta(), 1);
    let first_frame = restored.session().binary_frame();
    assert_eq!(frame_cloak_sounds(&mut restored, &rules), 0);
    // Native6FB9AC..6FBA07 finishes progress0 and starts the shared rule delay.
    let finished = cloak(&restored, id);
    assert_eq!(finished.state(), 0);
    assert_eq!(finished.depth(), 0);
    assert_eq!(finished.step_timer().speed(), 0);
    assert_eq!(
        finished.recloak_delay(),
        crate::sim::timer::CdTimer::started(first_frame as i32, rules.general.cloak_delay_frames)
    );
    for _ in 0..=rules.general.cloak_delay_frames.max(0) {
        if cloak(&restored, id)
            .recloak_delay()
            .expired(restored.session().binary_frame() as i32)
        {
            break;
        }
        assert_eq!(frame_cloak_sounds(&mut restored, &rules), 0);
        assert_eq!(cloak(&restored, id).state(), 0);
    }
    assert_eq!(frame_cloak_sounds(&mut restored, &rules), 1);
    assert_eq!(cloak(&restored, id).state(), 1);
    assert_eq!(cloak(&restored, id).step_timer().speed(), 7);
}
