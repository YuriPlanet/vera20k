//! `InfantryClass::Movement_Actions`' unreachable-NavCom recovery
//! (`0x00520FAF..0x00521144`) on retail rules, through the production
//! Movement_Actions entry.

use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::FrameEffects;

/// Beyond the Water column, out of an infantryman's zone.
const ACROSS: NavTargetRef = NavTargetRef::Cell { rx: 20, ry: 10 };
/// On his side of it.
const NEAR: NavTargetRef = NavTargetRef::Cell { rx: 12, ry: 10 };

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Plain,
    Moving,
    EnemyPlant,
    Tethered,
    OffPlayfield,
    Entering,
}

/// One Movement_Actions visit of a `type_id` man standing at (10,10) on a
/// retail arena that a Water column at x 16 splits, holding `nav` as his
/// NavCom: whether he keeps it (or drops it to none).
fn visit(type_id: &str, nav: NavTargetRef, case: Case) -> Option<bool> {
    let rules: RuleSet = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flood(&mut sim, &rules, 32, (0..32).map(|y| (16, y)));
    let id = sim
        .spawn_object(type_id, "Americans", 10, 10, 0, &rules)
        .unwrap();
    let partner = sim
        .spawn_object("E1", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let mut nav = nav;
    if case == Case::EnemyPlant {
        let plant = sim
            .spawn_object("GAPOWR", "Russians", 22, 10, 0, &rules)
            .unwrap();
        nav = NavTargetRef::Object { id: plant };
    }
    if case == Case::Moving {
        // A Move on his side of the water gets his Walk locomotor going.
        let order = crate::sim::command::Command::Move {
            entity_id: id,
            target_rx: 4,
            target_ry: 10,
        };
        assert!(sim.apply_command("Americans", &order, Some(&rules)));
        let path = sim.path_grid_snapshot();
        for _ in 0..30 {
            let walking = crate::sim::movement::motion_query::is_moving(
                sim.substrate.entities.get(id).unwrap(),
            ) == Some(true);
            if walking {
                break;
            }
            sim.advance_tick(&[], Some(&rules), path.as_deref(), None, 67);
        }
    }
    let man = sim.substrate.entities.get_mut(id).unwrap();
    assert!(
        man.in_playfield(),
        "{type_id} {case:?}: spawned in the playfield"
    );
    man.navigation.set_nav_com_for_test(Some(nav));
    match case {
        Case::Plain | Case::EnemyPlant => {}
        Case::Moving => assert_eq!(
            crate::sim::movement::motion_query::is_moving(man),
            Some(true),
            "the Move got him walking"
        ),
        Case::Tethered => man.tether.set_for_test(Some(partner)),
        Case::OffPlayfield => man.set_in_playfield(false),
        Case::Entering => man.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Enter),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        }),
    }
    if case != Case::Moving {
        assert_ne!(
            man.mission.effective().known(),
            Some(MissionType::Move),
            "the stopped Move prefix stays out of these rows"
        );
    }
    sim.infantry_movement_actions(id, &rules, None, FrameEffects::default());
    let after = sim.substrate.entities.get(id).unwrap().navigation.nav_com();
    assert!(
        after.is_none() || after == Some(nav),
        "{type_id} {case:?}: NavCom became {after:?}"
    );
    Some(after.is_some())
}

/// A standing rifleman drops a NavCom across the water, a cell or an enemy
/// plant there, and keeps one on his side; a walking man, a tethered man,
/// one outside the playfield, one entering and an `Infiltrate=` spy keep
/// theirs.
#[test]
fn an_infantryman_drops_a_nav_com_he_cannot_reach() {
    let Some(kept) = visit("E1", ACROSS, Case::Plain) else {
        return;
    };
    assert!(!kept, "unreachable cell");
    assert!(
        !visit("E1", ACROSS, Case::EnemyPlant).unwrap(),
        "unreachable plant"
    );
    assert!(visit("E1", NEAR, Case::Plain).unwrap(), "reachable");
    for case in [
        Case::Moving,
        Case::Tethered,
        Case::OffPlayfield,
        Case::Entering,
    ] {
        assert!(visit("E1", ACROSS, case).unwrap(), "{case:?}");
    }
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().unwrap();
    assert!(rules.object("SPY").unwrap().infiltrate);
    assert!(visit("SPY", ACROSS, Case::Plain).unwrap(), "spy");
}
