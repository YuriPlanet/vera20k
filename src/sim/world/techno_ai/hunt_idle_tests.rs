//! Hunt's no-target arm (`0x004D54EE..0x004D557C`) through the Foot
//! dispatcher: `IsControlledByHuman(owner)` (`0x0050B730`) or a nonzero
//! `g_GameMode` (`0x00A8B238`) takes the idle-action virtual (`vt+0x478`);
//! only a computer house in a campaign marches on the player instead.

use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

/// On retail rules a rifleman (`E1`) hunting a map with no enemy runs its
/// idle action for every owner in a non-campaign match, computer included,
/// and for a human in a campaign; a campaign computer's does not.
#[test]
fn a_hunter_without_a_target_idles_unless_a_campaign_computer_owns_it() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    assert!(!rules.object("E1").unwrap().stupid_hunt);
    for (owner, game_mode_nonzero, idles) in [
        ("Russians", true, true),
        ("Americans", true, true),
        ("Americans", false, true),
        ("Russians", false, false),
    ] {
        let mut sim = Simulation::with_seed(9);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
            let id = sim.interner.intern(name);
            sim.houses
                .insert(id, HouseState::new(id, side, None, human, 0, 10));
            sim.session.house_order.push(id);
        }
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(game_mode_nonzero),
        );
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let hunter = sim
            .spawn_object_at_height("E1", owner, 10, 10, 0, 0, &rules)
            .unwrap();
        // The idle receiver admits only a standing man (Doing Ready, Guard or
        // Tread); the ordinary Do_Action receiver puts him there.
        assert!(
            sim.infantry_do_action(
                hunter,
                crate::sim::movement::infantry_action::DO_READY,
                false,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
        );
        let now = sim.session.binary_frame;
        let entity = sim.substrate.entities.get_mut(hunter).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Hunt),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: now,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(now),
        });
        let idle_timer = entity.infantry.as_ref().unwrap().idle_action_timer();
        assert!(idle_timer.due(now), "a fresh rifleman is ready to idle");

        crate::sim::world::dispatch_foot_mission(&mut sim, hunter, &rules, ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(hunter).unwrap();
        assert_eq!(entity.attack_target.target(), None, "no enemy to hunt");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Hunt));
        assert_eq!(
            !entity
                .infantry
                .as_ref()
                .unwrap()
                .idle_action_timer()
                .due(now),
            idles,
            "{owner}, game mode nonzero {game_mode_nonzero}: idle action re-arms its timer"
        );
    }
}
