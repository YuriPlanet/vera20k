use super::*;
use crate::map::resolved_terrain::{test_clear_cell, test_grid};
use crate::rules::ruleset::CountryIdx;
use crate::sim::house_state::{HouseOutcomeKind, HouseState};
use crate::sim::intern::InternedId;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{MasterFrameTestRung, OutcomeWaitBoundary, TickLane};

struct VoxIdle(std::cell::Cell<bool>);

impl OutcomeWaitBoundary for VoxIdle {
    fn wait_complete(&self, _owner: InternedId, _kind: HouseOutcomeKind) -> bool {
        self.0.get()
    }
}

#[test]
fn admitted_victory_survives_a_later_house_tag_replacing_the_pending_result() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Fight.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    assert_eq!(rules.country_index("Russians"), Some(CountryIdx(8)));
    assert_eq!(rules.country_index("YuriCountry"), Some(CountryIdx(9)));
    assert_eq!(
        crate::rules::ruleset::native_minutes_to_frames(rules.general.savour_delay_minutes),
        90
    );
    let mut sim = Simulation::new();
    sim.install_session_mode(ScenarioGameMode::Skirmish);
    sim.publish_scenario_started();
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    sim.install_resolved_terrain_for_new_map(test_grid(32, 32, test_clear_cell));
    let [player, later_ai] = [
        ("Player", "Russians", true),
        ("LaterAI", "YuriCountry", false),
    ]
    .map(|(name, country, human)| {
        let owner = sim.interner.intern(name);
        let country_id = sim.interner.intern(country);
        sim.register_house(HouseState::new(
            owner,
            rules.country_side_index_raw(country).unwrap(),
            Some(country_id),
            human,
            0,
            10,
        ));
        owner
    });
    sim.session.set_current_house_for_test(Some(player));
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nWIN=Russians,<none>,Win,0,1,1,1,0\n\
         LATE=YuriCountry,<none>,Late loss,0,1,1,1,0\n\
         [Events]\nWIN=1,13,0,0\nLATE=1,15,0,0\n\
         [Actions]\nWIN=1,67,0,0,0,0,0,0,A\nLATE=1,2,0,8,0,0,0,0,A\n\
         [Tags]\nWIN_TAG=0,Win,WIN\nLATE_TAG=0,Late loss,LATE\n",
    );

    // Steam gamemd: LogicClass::PerTickUpdate (0x0055AFB0) announces Win
    // before its House loop (0x0055B68D). HouseClass::Update (0x004F8440)
    // admits it at 0x004F85BE with idle Vox, setting the flag at 0x004F8692.
    // The later House raises BuildingsLost15 (0x004F8F87); Action2
    // (0x006DEA67) -> Flag_To_Lose (0x004FCBD0) changes only PlayerPtr.
    let vox = VoxIdle(std::cell::Cell::new(false));
    let waiting = sim
        .advance_app_frame(
            &[],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            FrameEffects::default().with_outcome_wait(&vox),
        )
        .unwrap();
    assert!(waiting.outcome_waiting);
    assert_eq!(sim.trigger_runtime.action_trace, vec![(67, 0)]);
    assert!(sim.houses[&player].has_won());
    assert!(!sim.houses[&player].has_lost());
    assert!(sim.ready_outcome_for_owner(player).is_none());
    vox.0.set(true);
    let frame = sim
        .resume_outcome_wait(
            &rules,
            None,
            FrameEffects::default().with_outcome_wait(&vox),
        )
        .unwrap();
    assert!(!frame.outcome_waiting);
    assert_eq!(sim.trigger_runtime.action_trace, vec![(67, 0), (2, 8)]);
    assert!(sim.houses[&player].has_lost());
    assert!(!sim.houses[&player].has_won());
    assert_eq!(sim.houses[&player].outcome_timer().remaining(0), 90);
    assert!(!sim.houses[&later_ai].has_lost());
    assert_eq!(
        sim.ready_outcome_for_owner(player)
            .map(|outcome| outcome.kind),
        Some(HouseOutcomeKind::Victory)
    );
    // Main_Tick (0x0055DE4F..0x0055DEC8) skips commit and deferred destruction.
    assert!(!frame.tick.frame_committed);
    assert_eq!((sim.session.binary_frame(), sim.session.tick()), (0, 0));
    let rungs = sim.take_master_frame_test_trace();
    assert!(!rungs.contains(&MasterFrameTestRung::FrameCommit));
    assert!(!rungs.contains(&MasterFrameTestRung::PendingDelete));
    assert!(!sim.pending_delete().is_empty());
    assert_eq!(sim.trigger_runtime.tags.iter().flatten().count(), 2);
    assert!(sim.trigger_runtime.freed_trace.is_empty());

    // A Rust semantic snapshot retains the process request independently of
    // the later House flags/timer. This is not a native menu-save claim.
    let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 0, 0, "outcome", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .ready_outcome_for_owner(player)
            .map(|outcome| outcome.kind),
        Some(HouseOutcomeKind::Victory)
    );
    assert!(restored.houses[&player].has_lost());
    assert_eq!(restored.houses[&player].outcome_timer().remaining(0), 90);
}
