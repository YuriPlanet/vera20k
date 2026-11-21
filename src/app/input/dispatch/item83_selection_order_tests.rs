//! Tests for the selection ledger, selection mutations and their voice and
//! action-line policies.

use super::{
    HELD_TYPE_SELECT_VOICE_POLICY, ORDINARY_SELECTION_ACTION_LINE_POLICY,
    ORDINARY_SELECTION_VOICE_POLICY, TYPE_SELECT_TAP_ACTION_LINE_POLICY,
    TYPE_SELECT_TAP_VOICE_POLICY, apply_selection_action_line_policy_at_frame,
    insert_selected_id_by_role, selected_stable_ids_in_order, selection_membership_committed,
    selection_voice_recipients,
};
use crate::app::presentation::target_lines::TargetLineState;
use crate::audio::events::GameSoundEvent;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
use std::collections::VecDeque;

#[test]
fn item83_positive_damage_technos_prepend_and_noncombat_technos_append() {
    let mut order = VecDeque::from([10]);
    insert_selected_id_by_role(&mut order, 20, true);
    insert_selected_id_by_role(&mut order, 30, true);
    insert_selected_id_by_role(&mut order, 40, false);
    assert_eq!(order, [30, 20, 10, 40]);
}

fn mixed_selection_fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ARMED\n1=PLAIN\n[ARMED]\nPrimary=GUN\n[GUN]\nDamage=10\n[PLAIN]\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    for (id, type_name, selected) in [
        (10, "ARMED", true),
        (20, "PLAIN", true),
        (30, "ARMED", true),
        (40, "ARMED", false),
        (50, "PLAIN", true),
    ] {
        let mut entity = GameEntity::test_default(id, type_name, "Americans", 10, 20);
        entity.set_type_ref_for_test(sim.intern(type_name));
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.seldom.selection.set_selected_for_test(selected);
        sim.entities_mut_for_test().insert(entity);
    }
    (sim, rules)
}

#[test]
fn selection_read_keeps_pending_authority_and_native_recovery_order() {
    let (sim, rules) = mixed_selection_fixture();
    let read = |prior: &[u64], pending| {
        selected_stable_ids_in_order(Some(&sim), Some(&rules), prior, pending)
    };
    // Missing armed entries prepend in original scan order; missing plain
    // entries append. Removed IDs disappear, as do committed deselections.
    assert_eq!(read(&[20, 999, 40], false), [30, 10, 20, 50]);
    assert_eq!(read(&[], false), [30, 10, 20, 50]);
    // A complete ledger keeps its existing order regardless of role.
    assert_eq!(read(&[50, 10, 30, 20], false), [50, 10, 30, 20]);
    // Before the command commits, old selected bits cannot revive actors
    // omitted by input, or remove the newly selected actor40.
    assert_eq!(read(&[20, 999, 40], true), [20, 40]);
    assert!(read(&[], true).is_empty());
    assert!(selected_stable_ids_in_order(None, Some(&rules), &[10], false).is_empty());
}

/// The per-draw read trusts the ledger only as a frame's reconciliation
/// left it: an edited ledger, other rules, a later frame or another world
/// end that.
#[test]
fn committed_selection_holds_for_its_ledger_rules_and_frame_only() {
    let (mut sim, rules) = mixed_selection_fixture();
    let mut input = crate::app::input::state::MatchInputState::new(Default::default());
    super::reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules), &[]);
    let ledger = input.selection_order.clone();
    assert_eq!(ledger, [30, 10, 20, 50]);
    let committed = input.selection_committed.as_ref().unwrap();
    assert!(committed.holds(&sim, Some(&rules), &ledger));
    assert!(!committed.holds(&sim, Some(&rules), &ledger[1..]));
    assert!(!committed.holds(&sim, None, &ledger));
    sim.advance_app_frame(
        &[],
        RuleSet::empty_for_test(),
        67,
        crate::sim::world::TickLane::Ordinary,
        &mut crate::sim::world::FrameEffects::empty(),
    )
    .unwrap();
    assert!(!committed.holds(&sim, Some(&rules), &ledger));
    super::selection_navigation::reset_for_world_replacement(&mut input);
    assert!(input.selection_committed.is_none());
}

/// The game's own Selects (TechnoClass::Select vt+0x14C from Logic, e.g.
/// a reselected escapee and its planning-node partners) reach the vector
/// in call order through ObjectClass::Select's front/back rule
/// (`0x005F468C`/`0x005F470E`), not in entity-store order. A ledger entry
/// whose byte was cleared and set again left the vector at its Deselect
/// (`0x005F44A0`) and re-enters at its last Select.
#[test]
fn logic_selects_enter_the_ledger_in_call_order() {
    use super::selection_order_for_world;
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules) = mixed_selection_fixture();
    let read = |sim: &Simulation, prior: &[u64], pending, selects: &[u64]| {
        selection_order_for_world(sim, Some(&rules), prior, pending, selects)
    };
    // Store order would give [30, 10, 20, 50].
    assert_eq!(read(&sim, &[20], false, &[50, 30, 10]).0, [10, 30, 20, 50]);
    // The last of repeated Selects places the object; an object whose
    // Select was undone (40) stays out.
    assert_eq!(
        read(&sim, &[20, 50], false, &[10, 30, 40, 10]).0,
        [10, 30, 20, 50]
    );
    assert_eq!(
        read(&sim, &[10, 20, 30, 50], false, &[20]).0,
        [10, 30, 50, 20]
    );
    // A queued input Select keeps its provisional place; a logic Select
    // joins the provisional ledger.
    let owner = sim.intern("Americans");
    sim.queue_command(CommandEnvelope::new(
        owner,
        2,
        Command::Select {
            entity_ids: vec![20],
            additive: false,
        },
    ));
    assert_eq!(read(&sim, &[20], true, &[30, 20]), (vec![30, 20], true));
}

#[test]
fn selection_pending_clears_only_when_committed_membership_matches() {
    let (sim, _) = mixed_selection_fixture();
    assert!(selection_membership_committed(&sim, &[50, 10, 30, 20]));
    assert!(!selection_membership_committed(&sim, &[50, 10, 40, 20]));
    assert!(!selection_membership_committed(&sim, &[50, 10, 20]));
    assert!(!selection_membership_committed(&sim, &[50, 10, 30, 20, 40]));
    assert!(!selection_membership_committed(&sim, &[50, 10, 30, 30]));
}

#[test]
fn item83_voice_policy_emits_every_tap_voice_in_candidate_order_only() {
    let rules = RuleSet::from_ini_with_sounds_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n0=E1\n1=E2\n\n\
             [VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
             [E1]\nStrength=100\nArmor=none\nSpeed=4\nVoiceSelect=VoiceOne\n\n\
             [E2]\nStrength=100\nArmor=none\nSpeed=4\nVoiceSelect=VoiceTwo\n",
        ),
        &["VoiceOne", "VoiceTwo"],
    )
    .expect("item83 voice rules");
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    for (id, type_name) in [(1, "E1"), (2, "E2")] {
        let type_ref = sim.intern(type_name);
        sim.entities_mut_for_test()
            .insert(GameEntity::new_at_frame_zero_for_test(
                id,
                10 + id as u16,
                10,
                0,
                0,
                owner,
                Health::new(100),
                type_ref,
                EntityCategory::Infantry,
                0,
                5,
                false,
            ));
    }
    let candidate_order = [2, 1];
    let mut emitted = |policy| {
        selection_voice_recipients(policy, true, &candidate_order, 1, true)
            .iter()
            .filter_map(|id| {
                crate::app::match_runtime::sound_dispatch::selection_voice_event(
                    &mut sim, &rules, *id, true,
                )
            })
            .map(|event| match event {
                GameSoundEvent::UnitSelected { sound_id, .. } => {
                    sound_id.name_in(&rules).to_string()
                }
                other => panic!("unexpected selection event: {other:?}"),
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        emitted(TYPE_SELECT_TAP_VOICE_POLICY),
        vec!["VoiceTwo".to_string(), "VoiceOne".to_string()],
        "tap voices retain successful candidate/Select call order"
    );
    assert!(
        emitted(HELD_TYPE_SELECT_VOICE_POLICY).is_empty(),
        "held exact-type group-select suppresses the whole batch"
    );
    assert_eq!(
        emitted(ORDINARY_SELECTION_VOICE_POLICY),
        vec!["VoiceTwo".to_string()],
        "ordinary band/click policy retains only the first success"
    );
}

#[test]
fn item83_type_select_tap_preserves_action_line_timer_while_mouse_selection_starts_it() {
    let mut target_lines = TargetLineState::default();

    apply_selection_action_line_policy_at_frame(
        &mut target_lines,
        10,
        TYPE_SELECT_TAP_ACTION_LINE_POLICY,
    );
    assert!(
        !target_lines.is_selected_action_active(10),
        "a short TypeSelect tap leaves a zero timer untouched"
    );

    apply_selection_action_line_policy_at_frame(
        &mut target_lines,
        10,
        ORDINARY_SELECTION_ACTION_LINE_POLICY,
    );
    assert!(
        target_lines.is_selected_action_active(10),
        "ordinary click/bandbox selection still opens the action-line window"
    );

    apply_selection_action_line_policy_at_frame(
        &mut target_lines,
        20,
        TYPE_SELECT_TAP_ACTION_LINE_POLICY,
    );
    assert!(
        !target_lines.is_selected_action_active(35),
        "a later TypeSelect tap preserves rather than restarts the existing timer"
    );
}

#[test]
fn a_planning_select_adds_the_tail_node_members_who_speak_with_it() {
    use super::{SelectionMutation, SelectionVoicePolicy, resolve_selection_mutation};
    use crate::sim::command::Command;
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[VehicleTypes]\n0=TESTUNIT\n[TESTUNIT]\n"),
    )
    .expect("selection fixture rules");
    let mut sim = Simulation::new();
    let local = sim.intern("Local");
    let kind = sim.intern("TESTUNIT");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            local, 0, None, true, 0, 10,
        ));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    for id in [42, 43, 44] {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            1,
            1,
            0,
            0,
            local,
            Health::new(100),
            kind,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
    }
    // Same-frame planned moves to one cell share the tail node.
    for id in [42, 43] {
        let record = sim
            .encode_planning_order_record(
                local,
                &Command::Move {
                    entity_id: id,
                    target_rx: 5,
                    target_ry: 7,
                },
                None,
            )
            .unwrap();
        let command = sim.decode_native_command_record(&record, 0).unwrap();
        assert!(sim.apply_command("Local", &command.payload, Some(&rules)));
    }
    assert_eq!(sim.planning_select_partners(42, true), vec![42, 43]);
    assert!(sim.planning_select_partners(42, false).is_empty());
    let resolve = |current: Vec<u64>, select: Vec<u64>, planning| {
        resolve_selection_mutation(
            &sim,
            Some(&rules),
            current,
            None,
            false,
            planning,
            &SelectionMutation {
                clear: true,
                select,
                ..Default::default()
            },
        )
    };
    let effects = resolve(vec![], vec![42], true);
    assert_eq!(effects.successful_adds, vec![42, 43]);
    assert_eq!(effects.first_select_adds, 2);
    assert_eq!(
        selection_voice_recipients(
            SelectionVoicePolicy::FirstAdded,
            true,
            &effects.successful_adds,
            effects.first_select_adds,
            effects.first_attempt_added,
        ),
        [42, 43]
    );
    // A later band-box pick's expansion runs after the latch clear.
    let effects = resolve(vec![], vec![44, 42], true);
    assert_eq!(effects.successful_adds, vec![44, 42, 43]);
    assert_eq!(effects.first_select_adds, 1);
    assert_eq!(resolve(vec![], vec![42], false).successful_adds, vec![42]);
    // An already-selected pick makes no Select call, so no expansion.
    let effects = resolve_selection_mutation(
        &sim,
        None,
        vec![42],
        None,
        false,
        true,
        &SelectionMutation {
            select: vec![42],
            ..Default::default()
        },
    );
    assert!(effects.successful_adds.is_empty());
}

/// The current human house `Local` with units 42, 43 and 44 and a
/// computer house `Enemy` with units 50 and 51, out of Limbo; the
/// `discovered` enemy units have `+0x41B` set.
fn two_house_fixture(discovered: &[u64]) -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TESTUNIT\n[TESTUNIT]\n",
    ))
    .expect("selection fixture rules");
    let mut sim = Simulation::new();
    let local = sim.intern("Local");
    let enemy = sim.intern("Enemy");
    let kind = sim.intern("TESTUNIT");
    for (house, index, human) in [(local, 0, true), (enemy, 1, false)] {
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                house, index, None, human, 0, 10,
            ));
    }
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(local));
    for (id, owner) in [
        (42, local),
        (43, local),
        (44, local),
        (50, enemy),
        (51, enemy),
    ] {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            1,
            1,
            0,
            0,
            owner,
            Health::new(100),
            kind,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity
            .seldom
            .discovery
            .set_discovered_by_current_house_for_test(discovered.contains(&id));
        sim.entities_mut_for_test().insert(entity);
    }
    (sim, rules)
}

/// Put each actor on the player's node at (5, 7) in turn: the player's
/// by a planned move, another house's by a connect (`0x0063AD50`).
fn plan_to_shared_node(sim: &mut Simulation, rules: &RuleSet, actors: &[u64]) {
    use crate::sim::command::Command;
    let local = sim.intern("Local");
    for &actor in actors {
        let record = if sim.house_is_human_player(sim.entities().get(actor).unwrap().owner()) {
            sim.encode_planning_order_record(
                local,
                &Command::Move {
                    entity_id: actor,
                    target_rx: 5,
                    target_ry: 7,
                },
                None,
            )
        } else {
            sim.encode_plan_connect_record(local, actor, (5, 7))
        }
        .unwrap();
        let command = sim.decode_native_command_record(&record, 0).unwrap();
        assert!(sim.apply_command("Local", &command.payload, Some(rules)));
    }
}

/// A connect (`0x0063AD50`) can put another house's object on the
/// player's node. Each partner Select passes TechnoClass::Select's gate
/// (`0x006FBFA3..0x006FBFC3`: discovered `+0x41B` or the human player's)
/// and ObjectClass::Select's Unselect_All against the first selected
/// object (`0x005F45AF..0x005F45F8`), through the simulation's owners.
#[test]
fn a_planning_partner_of_another_house_passes_the_select_gate_and_unselects_all() {
    use super::{SelectionMutation, resolve_selection_mutation};
    for discovered in [true, false] {
        let (mut sim, rules) = two_house_fixture(if discovered { &[50] } else { &[] });
        plan_to_shared_node(&mut sim, &rules, &[42, 50, 43, 51]);
        assert_eq!(sim.planning_select_partners(42, true), [42, 50, 43, 51]);
        let effects = resolve_selection_mutation(
            &sim,
            Some(&rules),
            vec![],
            Some(42),
            false,
            true,
            &SelectionMutation {
                clear: true,
                select: vec![42],
                ..Default::default()
            },
        );
        if discovered {
            // 50 drains 42 (Follow with it); 43 drains 50; 51 is refused.
            assert_eq!(effects.ordered, [43]);
            assert_eq!(effects.successful_adds, [42, 50, 43]);
            assert_eq!(effects.follow_target, None);
        } else {
            assert_eq!(effects.ordered, [42, 43]);
            assert_eq!(effects.successful_adds, [42, 43]);
        }
    }
}

/// Recall (`0x007311C0`) clears, then Selects each member through
/// vt+0x14C (`0x00731344`). When the first member's tail node ends on
/// another house's discovered object, that partner is left alone in the
/// selection, and the next member's own Select drains it
/// (`0x005F45AF..0x005F45F8`).
#[test]
fn a_later_member_select_drains_another_houses_planning_partner() {
    use super::{SelectionMutation, resolve_selection_mutation};
    let (mut sim, rules) = two_house_fixture(&[50]);
    plan_to_shared_node(&mut sim, &rules, &[42, 50]);
    assert_eq!(sim.planning_select_partners(42, true), [42, 50]);
    let effects = resolve_selection_mutation(
        &sim,
        Some(&rules),
        vec![],
        None,
        false,
        true,
        &SelectionMutation {
            clear: true,
            select: vec![42, 44],
            ..Default::default()
        },
    );
    assert_eq!(effects.ordered, [44]);
    assert_eq!(effects.successful_adds, [42, 50, 44]);
}

/// The band callback (`0x004AC2B0`) Selects only the player's objects
/// (vt+0x14C at `0x004AC2F1`), and a Shift drag skips the clear. A
/// selected enemy is current-objects `[0]`, so the first Select runs
/// Unselect_All (`0x005F45F8`), whose Deselect also releases Follow.
#[test]
fn undeployable_building_band_commits_and_tracks_bunker_occupancy() {
    use super::resolve_selection_mutation;
    use crate::app::input::entity_pick::compute_box_selection_snapshot;
    use crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch;
    use crate::sim::command::Command;
    use crate::sim::docking::bunker_link::{can_auto_deploy_here, install_bunker_link};

    let Some(retail) = retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str("[GAPILL]\nUndeploysInto=AMCV\nBunker=yes\nNumberImpassableRows=0\n"),
    ) else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.object("GAPILL").unwrap().is_1x1_with_undeploy());
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let pill = sim
        .spawn_object("GAPILL", "Americans", 16, 16, 0, rules)
        .unwrap();
    let tank = sim
        .spawn_object("MTNK", "Americans", 12, 16, 0, rules)
        .unwrap();
    assert!(can_auto_deploy_here(&sim, tank, rules));
    assert_eq!(
        sim.unit_building_click_action(tank, pill, rules),
        Some(crate::sim::world::EntryClickAction::Enter)
    );
    // Encounter order is supplied here; helpers' companion regression
    // obtains it from the production Display/screen-registration owner.
    let band = |sim: &Simulation, prior: &[u64]| {
        compute_box_selection_snapshot(
            sim.entities(),
            &[pill],
            &[pill],
            prior,
            None,
            Some("Americans"),
            -1.0e6,
            -1.0e6,
            1.0e6,
            1.0e6,
            false,
            Some(rules),
            Some(sim.houses()),
            Some(sim.interner()),
        )
        .unwrap()
    };
    let mut prior = Vec::new();
    for (phase, admitted) in [(0, true), (1, false), (2, true)] {
        if phase == 1 {
            // Production install-end seam, after real CAN_LOAD admission;
            // this control does not claim the custom tank's walk to the pad.
            install_bunker_link(&mut sim, pill, tank, rules);
            let building = sim.entities().get(pill).unwrap();
            assert_eq!(building.bunker.occupant(), Some(tank));
            assert_eq!(building.bunker.link().installed_in(), None);
            assert_eq!(
                sim.entities()
                    .get(tank)
                    .unwrap()
                    .bunker
                    .link()
                    .installed_in(),
                Some(pill)
            );
        } else if phase == 2 {
            // Phase 1 installed through the production install-end seam;
            // clear the same two links for the post-release band. This
            // control is about selection, not about the eject chain: the
            // VERA-only opcode 0x31 now queues the Unload mission instead
            // of releasing at command time (#3675).
            {
                let entities = sim.entities_mut_for_test();
                entities
                    .get_mut_for_test(pill)
                    .unwrap()
                    .bunker
                    .set_bunker_occupant_for_test(None);
                entities
                    .get_mut_for_test(tank)
                    .unwrap()
                    .bunker
                    .set_bunker_link_for_test(crate::sim::docking::bunker_link::BunkerLink::None);
            }
            assert_eq!(sim.entities().get(pill).unwrap().bunker.occupant(), None);
            assert_eq!(
                sim.entities()
                    .get(tank)
                    .unwrap()
                    .bunker
                    .link()
                    .installed_in(),
                None
            );
        }
        let mutation = band(&sim, &prior);
        assert_eq!(mutation.select.contains(&pill), admitted, "phase {phase}");
        if phase == 0 {
            let refused =
                resolve_selection_mutation(&sim, Some(rules), vec![], None, true, false, &mutation);
            assert!(
                refused.ordered.is_empty(),
                "pending placement retains final refusal"
            );
        }
        let effects =
            resolve_selection_mutation(&sim, Some(rules), prior, None, false, false, &mutation);
        prior = effects.ordered;
        assert_eq!(prior.contains(&pill), admitted);
        sim.apply_command(
            "Americans",
            &Command::Select {
                entity_ids: prior.clone(),
                additive: false,
            },
            Some(rules),
        );
        assert_eq!(
            sim.entities()
                .get(pill)
                .unwrap()
                .seldom
                .selection
                .is_selected(),
            admitted
        );
    }
}

#[test]
fn a_shift_band_over_own_units_drains_a_selected_enemy() {
    use super::{SelectionMutation, resolve_selection_mutation};
    let (sim, rules) = two_house_fixture(&[50]);
    let order: Vec<u64> = sim
        .entities()
        .values()
        .map(|entity| entity.stable_id())
        .collect();
    let mutation = crate::app::input::entity_pick::compute_box_selection_snapshot(
        sim.entities(),
        &order,
        &order,
        &[50],
        None,
        Some("Local"),
        -1.0e6,
        -1.0e6,
        1.0e6,
        1.0e6,
        true,
        Some(&rules),
        None,
        Some(sim.interner()),
    )
    .expect("the band caught objects");
    assert_eq!(
        mutation,
        SelectionMutation {
            select: vec![42, 43, 44],
            ..Default::default()
        }
    );
    let effects = resolve_selection_mutation(
        &sim,
        Some(&rules),
        vec![50],
        Some(50),
        false,
        false,
        &mutation,
    );
    assert_eq!(effects.ordered, [42, 43, 44]);
    assert_eq!(effects.successful_adds, [42, 43, 44]);
    assert_eq!(effects.follow_target, None);
}

/// A Shift click while the player's units are selected is the toggle
/// arm's Select (`0x004ABDBA`), which clears nothing itself. For an enemy
/// its ObjectClass::Select drains the units (`0x005F45AF..0x005F45F8`);
/// TechnoClass::Select's gate (`0x006FBFA3..0x006FBFC3`) refuses an
/// undiscovered one first, which leaves them selected.
#[test]
fn a_shift_click_on_an_enemy_selects_it_through_select() {
    use super::{SelectionMutation, resolve_selection_mutation};
    let (sim, rules) = two_house_fixture(&[50]);
    for (enemy, ordered) in [(50, vec![50]), (51, vec![42, 43])] {
        let (x, y) =
            crate::render::locomotor_visual::screen_position(sim.entities().get(enemy).unwrap());
        let mutation = crate::app::input::entity_pick::compute_click_selection_snapshot(
            sim.entities(),
            &[enemy],
            &[42, 43],
            None,
            Some("Local"),
            x,
            y,
            true,
            Some(&rules),
            Some(sim.interner()),
        )
        .expect("the click picked the enemy");
        assert_eq!(
            mutation,
            SelectionMutation {
                select: vec![enemy],
                ..Default::default()
            }
        );
        let effects = resolve_selection_mutation(
            &sim,
            Some(&rules),
            vec![42, 43],
            Some(42),
            false,
            false,
            &mutation,
        );
        assert_eq!(effects.ordered, ordered, "enemy {enemy}");
        assert_eq!(effects.follow_target, (enemy == 51).then_some(42));
    }
}
